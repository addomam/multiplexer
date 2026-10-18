//! Incremental byte framing, independent of capability matching and UTF-8 decoding.
//! Payloads exclude framing; CSI retains its final byte. Unknown frames stay public.

/// Maximum bytes in one framed payload (not counting introducer/terminator).
pub const MAX_SEQUENCE: usize = 8192;
const MAX_WRAPPER_DEPTH: u8 = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Input(Vec<u8>),
    /// Opaque bracketed-paste bytes retain provenance even if boundary events are dropped.
    PasteData(Vec<u8>),
    /// Three byte/UTF-8 mouse fields, excluding CSI M. Payload is bounded to 12 bytes.
    LegacyMouse(Vec<u8>),
    Csi(Vec<u8>),
    Osc(Vec<u8>),
    Dcs(Vec<u8>),
    Apc(Vec<u8>),
    Escape(Vec<u8>),
    Malformed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StringKind {
    Osc,
    Dcs,
    Apc,
    Other(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    Ss3,
    AltUtf8 {
        remaining: u8,
    },
    Csi {
        intermediate: bool,
    },
    LegacyMouse {
        fields: u8,
        continuation: u8,
        byte_oriented: bool,
    },
    DiscardLegacyMouse {
        fields: u8,
        continuation: u8,
        byte_oriented: bool,
    },
    Paste {
        matched: u8,
        c1: bool,
    },
    String {
        kind: StringKind,
        escape: bool,
    },
    DiscardCsi,
    DiscardEscape,
    DiscardString {
        kind: StringKind,
        escape: bool,
    },
}

// UTF-8 mouse encoding can survive a permanently-set inherited mode. Ambiguous
// high-byte pairs are kept inside the frame, never exposed as consent keys.
fn mouse_field(fields: u8, continuation: u8, byte: u8, byte_oriented: bool) -> (u8, u8) {
    if byte_oriented {
        return (fields + 1, 0);
    }
    let mut fields = fields;
    if continuation > 0 {
        if (0x80..=0xbf).contains(&byte) {
            return (fields + u8::from(continuation == 1), continuation - 1);
        }
        fields += 1;
        if fields == 3 {
            return (fields, 0);
        }
    }
    match byte {
        0xc2..=0xdf => (fields, 1),
        0xe0..=0xef => (fields, 2),
        0xf0..=0xf4 => (fields, 3),
        _ => (fields + 1, 0),
    }
}
// Transport framing is independent of semantic framing. Each decoded layer keeps
// its own partial wrapper while every layer shares one semantic byte stream.
#[derive(Debug, Default)]
struct Transport {
    probe: [u8; 7],
    probe_len: usize,
    payload: Vec<u8>,
    size: usize,
    state: TransportState,
}

#[derive(Clone, Copy, Debug, Default)]
enum TransportState {
    #[default]
    Ground,
    Wrapper {
        escape: bool,
        utf8: u8,
    },
    Discard {
        escape: bool,
        utf8: u8,
    },
}

#[derive(Debug)]
pub struct Parser {
    state: State,
    payload: Vec<u8>,
    utf8_remaining: u8,
    transports: [Transport; MAX_WRAPPER_DEPTH as usize + 1],
    paste_wrapped: bool,
    frame_wrapped: bool,
    // Propagates Ctrl-C provenance back through nested transport frames.
    ctrl_c_opaque: bool,
    // At most three bytes: emit complete UTF-8 scalars to stateless input consumers.
    input_tail: Vec<u8>,
    legacy_bytes: bool,
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser {
    pub fn new() -> Self {
        Self {
            state: State::Ground,
            payload: Vec::new(),
            utf8_remaining: 0,
            transports: std::array::from_fn(|_| Transport::default()),
            paste_wrapped: false,
            frame_wrapped: false,
            ctrl_c_opaque: false,
            input_tail: Vec::new(),
            legacy_bytes: false,
        }
    }

    /// Select normal byte coordinates only after the endpoint establishes that
    /// UTF-8 mouse encoding is inactive. Existing partial reports keep their format.
    pub fn set_legacy_mouse_bytes(&mut self, enabled: bool) {
        self.legacy_bytes = enabled;
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Event> {
        let mut events = Vec::new();
        let mut input = std::mem::take(&mut self.input_tail);
        for &byte in bytes {
            self.transport_byte(0, byte, &mut input, &mut events);
        }
        if self.state == State::Ground && self.utf8_remaining > 0 {
            if let Some(start) = input.iter().rposition(|byte| !(0x80..=0xbf).contains(byte)) {
                self.input_tail = input.split_off(start);
            }
        }
        if matches!(self.state, State::Paste { .. }) {
            flush_paste(&mut input, &mut events);
        } else {
            flush_input(&mut input, &mut events);
        }
        events
    }

    /// Caller supplies the timeout policy; a lone ESC is an ordinary Escape key.
    pub fn expire(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let mut input = std::mem::take(&mut self.input_tail);
        for layer in 0..self.transports.len() {
            if !matches!(self.state, State::Paste { .. }) {
                let transport = &mut self.transports[layer];
                let probe = transport.probe;
                let len = std::mem::take(&mut transport.probe_len);
                for &byte in &probe[..len] {
                    self.byte(byte, layer, &mut input, &mut events);
                }
            }
            let transport = &mut self.transports[layer];
            if let TransportState::Wrapper { escape, utf8, .. } = transport.state {
                transport.payload.clear();
                transport.state = TransportState::Discard { escape, utf8 };
                events.push(Event::Malformed(
                    "partial tmux wrapper expired; quarantining late tail".into(),
                ));
            }
        }
        if self.state == State::Ground && self.utf8_remaining > 0 {
            self.input_tail = input;
        } else if matches!(self.state, State::Paste { .. }) {
            flush_paste(&mut input, &mut events);
        } else {
            flush_input(&mut input, &mut events);
        }
        events.extend(self.expire_semantic());
        events
    }

    fn expire_semantic(&mut self) -> Vec<Event> {
        if let State::LegacyMouse {
            fields,
            continuation,
            byte_oriented,
        } = self.state
        {
            self.reset();
            // A late suffix still belongs to this report, even after repeated timeouts.
            self.state = State::DiscardLegacyMouse {
                fields,
                continuation,
                byte_oriented,
            };
            return vec![Event::Malformed(
                "partial legacy mouse report expired".into(),
            )];
        }
        if matches!(
            self.state,
            State::DiscardLegacyMouse { .. }
                | State::Paste { .. }
                | State::DiscardCsi
                | State::DiscardEscape
                | State::DiscardString { .. }
        ) {
            return Vec::new();
        }
        let discard = match self.state {
            State::String { kind, escape } => Some(State::DiscardString { kind, escape }),
            State::Csi { .. } => Some(State::DiscardCsi),
            State::Escape if !self.payload.is_empty() => Some(State::DiscardEscape),
            State::Ss3 => Some(State::DiscardCsi),
            _ => None,
        };
        if let Some(state) = discard {
            self.payload.clear();
            self.state = state;
            return vec![Event::Malformed(
                "partial sequence expired; quarantining late tail".into(),
            )];
        }
        let event = match self.state {
            State::Ground if !self.input_tail.is_empty() => {
                Some(Event::Malformed("partial UTF-8 input expired".into()))
            }
            State::Ground => None,
            State::Escape if self.payload.is_empty() => Some(Event::Input(vec![27])),
            _ => Some(Event::Malformed("partial sequence expired".into())),
        };
        self.reset();
        self.utf8_remaining = 0;
        self.input_tail.clear();
        event.into_iter().collect()
    }

    fn reset(&mut self) {
        self.state = State::Ground;
        self.payload.clear();
        self.utf8_remaining = 0;
    }

    fn start(&mut self, state: State, input: &mut Vec<u8>, events: &mut Vec<Event>) {
        flush_input(input, events);
        self.payload.clear();
        self.state = state;
        self.utf8_remaining = 0;
    }

    fn transport_byte(
        &mut self,
        layer: usize,
        byte: u8,
        input: &mut Vec<u8>,
        events: &mut Vec<Event>,
    ) {
        // Bare paste has no transport quoting: every byte reaches the semantic
        // delimiter matcher, including wrapper-looking headers and repeated ESC.
        if matches!(self.state, State::Paste { .. }) && !self.paste_wrapped {
            self.byte(byte, layer, input, events);
            return;
        }
        match self.transports[layer].state {
            TransportState::Ground => {
                let transport = &mut self.transports[layer];
                if transport.probe_len == 0 {
                    // UTF-8 continuation bytes and legacy mouse coordinates are
                    // data, even when they resemble a C1 wrapper introducer.
                    if matches!(
                        self.state,
                        State::LegacyMouse { .. } | State::DiscardLegacyMouse { .. }
                    ) || (self.utf8_remaining > 0 && (0x80..=0xbf).contains(&byte))
                        || !matches!(byte, 27 | 0x90)
                    {
                        self.forward_byte(layer, byte, input, events);
                        return;
                    }
                    transport.probe[0] = byte;
                    transport.probe_len = 1;
                    return;
                }
                let header: &[u8] = if transport.probe[0] == 27 {
                    b"\x1bPtmux;"
                } else {
                    b"\x90tmux;"
                };
                if byte == header[transport.probe_len] {
                    transport.probe[transport.probe_len] = byte;
                    transport.probe_len += 1;
                    if transport.probe_len == header.len() {
                        transport.probe_len = 0;
                        transport.payload.extend_from_slice(b"tmux;");
                        transport.size = 5;
                        transport.state = TransportState::Wrapper {
                            escape: false,
                            // A preceding decoded frame may end mid-scalar.
                            // Its continuation remains data in this payload too.
                            utf8: self.utf8_remaining,
                        };
                    }
                } else {
                    let probe = transport.probe;
                    let len = std::mem::take(&mut transport.probe_len);
                    for &held in &probe[..len] {
                        self.forward_byte(layer, held, input, events);
                    }
                    // A mismatching ESC may begin the next transport wrapper.
                    self.transport_byte(layer, byte, input, events);
                }
            }
            TransportState::Discard { escape, utf8 } => {
                if byte == 3
                    && !matches!(
                        self.state,
                        State::Paste { .. }
                            | State::LegacyMouse { .. }
                            | State::DiscardLegacyMouse { .. }
                    )
                {
                    self.transports[layer].state = TransportState::Ground;
                    self.byte(byte, layer, input, events);
                } else {
                    self.transports[layer].state =
                        if (escape && byte == b'\\') || (!escape && byte == 0x9c && utf8 == 0) {
                            TransportState::Ground
                        } else {
                            TransportState::Discard {
                                escape: byte == 27 && !escape,
                                utf8: utf8_after(utf8, byte),
                            }
                        };
                }
            }
            TransportState::Wrapper { escape, utf8 } => {
                if byte == 3 {
                    // Decode the prefix before delivering emergency input: it
                    // may have opened an opaque paste or mouse report.
                    let transport = &mut self.transports[layer];
                    let mut payload = std::mem::take(&mut transport.payload);
                    if escape {
                        payload.push(27);
                    }
                    self.decode_wrapper(layer, &mut payload, input, events);
                    payload.clear();
                    payload.extend_from_slice(b"tmux;");
                    self.transports[layer].payload = payload;
                    self.transports[layer].state = TransportState::Wrapper {
                        escape: false,
                        utf8: 0,
                    };
                    self.transports[layer].size += 1 + usize::from(escape);
                    self.ctrl_c_opaque = matches!(
                        self.state,
                        State::Paste { .. }
                            | State::LegacyMouse { .. }
                            | State::DiscardLegacyMouse { .. }
                    );
                    if self.transports[layer].size > MAX_SEQUENCE {
                        self.transports[layer].payload.clear();
                        self.transports[layer].state = TransportState::Discard {
                            escape: false,
                            utf8: 0,
                        };
                        events.push(Event::Malformed("sequence payload limit exceeded".into()));
                        if !matches!(
                            self.state,
                            State::Paste { .. }
                                | State::LegacyMouse { .. }
                                | State::DiscardLegacyMouse { .. }
                        ) {
                            self.byte(byte, layer, input, events);
                        }
                    } else if layer < usize::from(MAX_WRAPPER_DEPTH) {
                        self.transport_byte(layer + 1, byte, input, events);
                    } else {
                        self.byte(byte, layer, input, events);
                    }
                    if !self.ctrl_c_opaque {
                        self.transports[layer].payload.clear();
                        self.transports[layer].state = TransportState::Discard {
                            escape: false,
                            utf8: 0,
                        };
                    }
                    return;
                }
                if (escape && byte == b'\\') || (!escape && byte == 0x9c && utf8 == 0) {
                    let transport = &mut self.transports[layer];
                    transport.state = TransportState::Ground;
                    let mut payload = std::mem::take(&mut transport.payload);
                    self.decode_wrapper(layer, &mut payload, input, events);
                    payload.clear();
                    self.transports[layer].payload = payload;
                } else {
                    let transport = &mut self.transports[layer];
                    let append_escape = escape;
                    let append_byte = byte != 27 || escape;
                    let count = usize::from(append_escape) + usize::from(append_byte);
                    let next_escape = byte == 27 && !escape;
                    // Nested transport headers do not consume the semantic
                    // scalar left partial by a previous decoded wrapper. The
                    // outer layer must protect its continuation before the
                    // nested layer can be decoded at ST.
                    let nested_header = byte == b';'
                        && (transport.payload.ends_with(b"\x1bPtmux")
                            || transport.payload.ends_with(b"\x90tmux"));
                    let next_utf8 = if nested_header {
                        self.utf8_remaining
                    } else {
                        utf8_after(utf8, byte)
                    };
                    if transport.size + count > MAX_SEQUENCE {
                        transport.payload.clear();
                        transport.state = TransportState::Discard {
                            escape: next_escape,
                            utf8: next_utf8,
                        };
                        events.push(Event::Malformed("sequence payload limit exceeded".into()));
                    } else {
                        transport.size += count;
                        if append_escape {
                            transport.payload.push(27);
                        }
                        if append_byte {
                            transport.payload.push(byte);
                        }
                        transport.state = TransportState::Wrapper {
                            escape: next_escape,
                            utf8: next_utf8,
                        };
                    }
                }
            }
        }
    }

    fn forward_byte(
        &mut self,
        layer: usize,
        byte: u8,
        input: &mut Vec<u8>,
        events: &mut Vec<Event>,
    ) {
        if let Some(next) = (layer + 1..self.transports.len()).find(|&next| {
            self.transports[next].probe_len > 0
                || !matches!(self.transports[next].state, TransportState::Ground)
        }) {
            self.transport_byte(next, byte, input, events);
        } else {
            self.byte(byte, layer, input, events);
        }
    }

    fn decode_wrapper(
        &mut self,
        layer: usize,
        payload: &mut Vec<u8>,
        input: &mut Vec<u8>,
        events: &mut Vec<Event>,
    ) {
        if layer == usize::from(MAX_WRAPPER_DEPTH) {
            events.push(Event::Malformed("tmux nesting limit exceeded".into()));
            return;
        }
        // Validate the complete quoted payload before exposing any of it.
        let mut source = 5;
        while source < payload.len() {
            if payload[source] == 27 {
                if payload.get(source + 1) != Some(&27) {
                    events.push(Event::Dcs(std::mem::take(payload)));
                    return;
                }
                source += 1;
            }
            source += 1;
        }
        let mut source = 5;
        while source < payload.len() {
            let byte = payload[source];
            source += if byte == 27 { 2 } else { 1 };
            self.transport_byte(layer + 1, byte, input, events);
        }
    }

    fn byte(&mut self, byte: u8, layer: usize, input: &mut Vec<u8>, events: &mut Vec<Event>) {
        self.frame_wrapped = if self.state == State::Ground {
            layer > 0
        } else {
            self.frame_wrapped || layer > 0
        };
        if byte == 3 {
            self.ctrl_c_opaque = matches!(
                self.state,
                State::Paste { .. } | State::LegacyMouse { .. } | State::DiscardLegacyMouse { .. }
            );
        }
        if byte == 3
            && self.state != State::Ground
            && !matches!(
                self.state,
                State::Paste { .. } | State::LegacyMouse { .. } | State::DiscardLegacyMouse { .. }
            )
        {
            flush_input(input, events);
            self.reset();
            input.push(byte); // Emergency Ctrl-C remains available through broken replies.
            return;
        }
        if self.state != State::Ground
            && !matches!(
                self.state,
                State::LegacyMouse { .. } | State::DiscardLegacyMouse { .. } | State::Paste { .. }
            )
            && matches!(byte, 0x18 | 0x1a)
        {
            flush_input(input, events);
            events.push(Event::Malformed("sequence cancelled".into()));
            self.reset();
            input.push(byte);
            return;
        }
        if self.state != State::Ground && !matches!(self.state, State::Paste { .. }) {
            flush_input(input, events);
        }
        match self.state {
            State::Ground => {
                // C1 bytes embedded in UTF-8 must not become framing controls.
                if self.utf8_remaining > 0 && (0x80..=0xbf).contains(&byte) {
                    self.utf8_remaining -= 1;
                    input.push(byte);
                    return;
                }
                self.utf8_remaining = match byte {
                    0xc2..=0xdf => 1,
                    0xe0..=0xef => 2,
                    0xf0..=0xf4 => 3,
                    _ => 0,
                };
                match byte {
                    27 => self.start(State::Escape, input, events),
                    0x9b => self.start(
                        State::Csi {
                            intermediate: false,
                        },
                        input,
                        events,
                    ),
                    0x9d => self.start(
                        State::String {
                            kind: StringKind::Osc,
                            escape: false,
                        },
                        input,
                        events,
                    ),
                    0x90 => self.start(
                        State::String {
                            kind: StringKind::Dcs,
                            escape: false,
                        },
                        input,
                        events,
                    ),
                    0x9f => self.start(
                        State::String {
                            kind: StringKind::Apc,
                            escape: false,
                        },
                        input,
                        events,
                    ),
                    0x9e => self.start(
                        State::String {
                            kind: StringKind::Other(b'^'),
                            escape: false,
                        },
                        input,
                        events,
                    ),
                    0x98 => self.start(
                        State::String {
                            kind: StringKind::Other(b'X'),
                            escape: false,
                        },
                        input,
                        events,
                    ),
                    _ => input.push(byte),
                }
            }
            State::Escape => {
                if self.payload.is_empty() {
                    let state = match byte {
                        b'[' => Some(State::Csi {
                            intermediate: false,
                        }),
                        b']' => Some(State::String {
                            kind: StringKind::Osc,
                            escape: false,
                        }),
                        b'P' => Some(State::String {
                            kind: StringKind::Dcs,
                            escape: false,
                        }),
                        b'_' => Some(State::String {
                            kind: StringKind::Apc,
                            escape: false,
                        }),
                        b'^' | b'X' => Some(State::String {
                            kind: StringKind::Other(byte),
                            escape: false,
                        }),
                        b'O' => Some(State::Ss3),
                        _ => None,
                    };
                    if let Some(state) = state {
                        if byte == b'O' {
                            self.payload.push(byte);
                        }
                        self.state = state;
                        return;
                    }
                }
                if byte == 27 {
                    events.push(Event::Input(vec![27]));
                    self.payload.clear();
                } else if (0x20..=0x2f).contains(&byte) {
                    self.push(byte, events);
                } else if self.payload.is_empty() && (0xc2..=0xf4).contains(&byte) {
                    self.payload.push(byte);
                    self.state = State::AltUtf8 {
                        remaining: match byte {
                            0xc2..=0xdf => 1,
                            0xe0..=0xef => 2,
                            _ => 3,
                        },
                    };
                } else if (0x30..=0x7e).contains(&byte) || byte >= 0x80 {
                    if self.push(byte, events) {
                        events.push(Event::Escape(std::mem::take(&mut self.payload)));
                    }
                    self.reset();
                } else {
                    events.push(Event::Malformed("invalid escape byte".into()));
                    self.reset();
                    input.push(byte);
                }
            }
            State::AltUtf8 { remaining } => {
                if (0x80..=0xbf).contains(&byte) {
                    self.payload.push(byte);
                    if remaining == 1 {
                        events.push(Event::Escape(std::mem::take(&mut self.payload)));
                        self.reset();
                    } else {
                        self.state = State::AltUtf8 {
                            remaining: remaining - 1,
                        };
                    }
                } else {
                    events.push(Event::Escape(std::mem::take(&mut self.payload)));
                    self.reset();
                    self.byte(byte, layer, input, events);
                }
            }
            State::Ss3 => {
                if (0x40..=0x7e).contains(&byte) {
                    self.payload.push(byte);
                    events.push(Event::Escape(std::mem::take(&mut self.payload)));
                    self.reset();
                } else {
                    events.push(Event::Malformed("invalid SS3 final".into()));
                    self.payload.clear();
                    self.state = State::DiscardCsi;
                }
            }
            State::Csi { intermediate } => {
                if byte == 27 {
                    events.push(Event::Malformed("CSI interrupted by ESC".into()));
                    self.start(State::Escape, input, events);
                } else if byte < 0x20 || byte == 0x7f {
                    input.push(byte);
                } else if (0x40..=0x7e).contains(&byte) {
                    flush_input(input, events);
                    if byte == b'M' && self.payload.is_empty() {
                        self.state = State::LegacyMouse {
                            fields: 0,
                            continuation: 0,
                            byte_oriented: self.legacy_bytes,
                        };
                    } else if self.push(byte, events) {
                        let paste = self.payload == b"200~";
                        let wrapped = self.frame_wrapped;
                        events.push(Event::Csi(std::mem::take(&mut self.payload)));
                        self.reset();
                        if paste {
                            self.paste_wrapped = wrapped;
                            self.state = State::Paste {
                                matched: 0,
                                c1: false,
                            };
                        }
                    } else {
                        // This byte was already the final; no further discard needed.
                        self.reset();
                    }
                } else if (0x20..=0x2f).contains(&byte) {
                    self.state = State::Csi { intermediate: true };
                    self.push(byte, events);
                } else if (0x30..=0x3f).contains(&byte) && !intermediate {
                    self.push(byte, events);
                } else {
                    flush_input(input, events);
                    events.push(Event::Malformed("invalid CSI byte ordering".into()));
                    self.payload.clear();
                    self.state = State::DiscardCsi;
                }
            }
            State::LegacyMouse {
                fields,
                continuation,
                byte_oriented,
            } => {
                // Coordinates are opaque to all other framing, including ESC/C1.
                self.payload.push(byte);
                let (fields, continuation) = mouse_field(fields, continuation, byte, byte_oriented);
                if fields == 3 {
                    events.push(Event::LegacyMouse(std::mem::take(&mut self.payload)));
                    self.reset();
                } else {
                    self.state = State::LegacyMouse {
                        fields,
                        continuation,
                        byte_oriented,
                    };
                }
            }
            State::DiscardLegacyMouse {
                fields,
                continuation,
                byte_oriented,
            } => {
                let (fields, continuation) = mouse_field(fields, continuation, byte, byte_oriented);
                if fields == 3 {
                    self.reset();
                } else {
                    self.state = State::DiscardLegacyMouse {
                        fields,
                        continuation,
                        byte_oriented,
                    };
                }
            }
            State::Paste { matched, c1 } => {
                let end: &[u8] = if c1 { b"\x9b201~" } else { b"\x1b[201~" };
                let continuation = self.utf8_remaining > 0 && (0x80..=0xbf).contains(&byte);
                self.utf8_remaining = utf8_after(self.utf8_remaining, byte);
                if !continuation && byte == end[usize::from(matched)] {
                    let matched = matched + 1;
                    if usize::from(matched) == end.len() {
                        flush_paste(input, events);
                        events.push(Event::Csi(b"201~".to_vec()));
                        self.reset();
                    } else {
                        self.state = State::Paste { matched, c1 };
                    }
                } else {
                    input.extend_from_slice(&end[..usize::from(matched)]);
                    self.state = if byte == 27 || (byte == 0x9b && !continuation) {
                        State::Paste {
                            matched: 1,
                            c1: byte == 0x9b,
                        }
                    } else {
                        input.push(byte);
                        State::Paste {
                            matched: 0,
                            c1: false,
                        }
                    };
                }
            }
            State::String { kind, escape } => {
                if escape {
                    if byte == b'\\' {
                        self.finish_string(kind, input, events);
                    } else {
                        if !self.push(27, events) {
                            self.byte(byte, layer, input, events);
                            return;
                        }
                        self.state = State::String {
                            kind,
                            escape: false,
                        };
                        self.byte(byte, layer, input, events);
                    }
                } else if byte == 27 {
                    self.state = State::String { kind, escape: true };
                } else if (kind == StringKind::Osc && byte == 7)
                    || (byte == 0x9c && self.utf8_remaining == 0)
                {
                    self.finish_string(kind, input, events);
                } else {
                    if self.utf8_remaining > 0 && (0x80..=0xbf).contains(&byte) {
                        self.utf8_remaining -= 1;
                    } else {
                        self.utf8_remaining = match byte {
                            0xc2..=0xdf => 1,
                            0xe0..=0xef => 2,
                            0xf0..=0xf4 => 3,
                            _ => 0,
                        };
                    }
                    self.push(byte, events);
                }
            }
            State::DiscardEscape => {
                if byte == 27 {
                    self.start(State::Escape, input, events);
                } else if (0x30..=0x7e).contains(&byte) {
                    self.reset();
                }
            }
            State::DiscardCsi => {
                if byte == 27 {
                    self.start(State::Escape, input, events);
                } else if (0x40..=0x7e).contains(&byte) {
                    self.reset();
                }
            }
            State::DiscardString { kind, escape } => {
                if (escape && byte == b'\\')
                    || (kind == StringKind::Osc && byte == 7)
                    || (byte == 0x9c && self.utf8_remaining == 0)
                {
                    self.reset();
                } else {
                    self.state = State::DiscardString {
                        kind,
                        escape: byte == 27,
                    };
                    if self.utf8_remaining > 0 && (0x80..=0xbf).contains(&byte) {
                        self.utf8_remaining -= 1;
                    } else {
                        self.utf8_remaining = match byte {
                            0xc2..=0xdf => 1,
                            0xe0..=0xef => 2,
                            0xf0..=0xf4 => 3,
                            _ => 0,
                        };
                    }
                }
            }
        }
    }

    fn push(&mut self, byte: u8, events: &mut Vec<Event>) -> bool {
        if self.payload.len() == MAX_SEQUENCE {
            events.push(Event::Malformed("sequence payload limit exceeded".into()));
            self.state = match self.state {
                State::String { kind, .. } => State::DiscardString {
                    kind,
                    escape: false,
                },
                State::Escape => State::DiscardEscape,
                _ => State::DiscardCsi,
            };
            self.payload.clear();
            false
        } else {
            self.payload.push(byte);
            true
        }
    }

    fn finish_string(&mut self, kind: StringKind, input: &mut Vec<u8>, events: &mut Vec<Event>) {
        flush_input(input, events);
        let payload = std::mem::take(&mut self.payload);
        self.reset();
        events.push(match kind {
            StringKind::Osc => Event::Osc(payload),
            StringKind::Dcs => Event::Dcs(payload),
            StringKind::Apc => Event::Apc(payload),
            // Preserve unknown privacy/SOS strings as Escape payloads, including introducer.
            StringKind::Other(introducer) => {
                let mut framed = vec![introducer];
                framed.extend(payload);
                Event::Escape(framed)
            }
        });
    }
}

fn utf8_after(remaining: u8, byte: u8) -> u8 {
    if remaining > 0 && (0x80..=0xbf).contains(&byte) {
        remaining - 1
    } else {
        match byte {
            0xc2..=0xdf => 1,
            0xe0..=0xef => 2,
            0xf0..=0xf4 => 3,
            _ => 0,
        }
    }
}

fn flush_input(input: &mut Vec<u8>, events: &mut Vec<Event>) {
    if !input.is_empty() {
        events.push(Event::Input(std::mem::take(input)));
    }
}

fn flush_paste(input: &mut Vec<u8>, events: &mut Vec<Event>) {
    if !input.is_empty() {
        events.push(Event::PasteData(std::mem::take(input)));
    }
}

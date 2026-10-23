//! Evidence-based discovery. Reply support is not a claim of correct rendering.
//! Queries are read-only and never infer support from environment or silence.
//!
//! Wire formats: <https://invisible-island.net/xterm/ctlseqs/ctlseqs.html>,
//! <https://sw.kovidgoyal.net/kitty/keyboard-protocol/> and
//! <https://sw.kovidgoyal.net/kitty/graphics-protocol/>.
//! In particular, XTQMODKEYS is CSI ?4m, not the state-changing CSI >4m.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::protocol::{Event, MAX_SEQUENCE};

pub const QUERY_TIMEOUT_MS: u64 = 1000;
const MAX_EVIDENCE: usize = 8;
const MAX_TEXT_BYTES: usize = 256;
const MODES: [u16; 16] = [
    6, 7, 9, 25, 1000, 1001, 1002, 1003, 1004, 1005, 1006, 1015, 1016, 1049, 2004, 2026,
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Verified,
    Advertised,
    Visual,
    Unsupported,
    #[default]
    Unknown,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capability {
    pub id: String,
    pub name: String,
    pub category: String,
    pub status: Status,
    /// Objective evidence survives a user visual verdict and is exported separately.
    #[serde(default)]
    pub objective_status: Status,
    pub evidence: Vec<String>,
    pub visual: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub identity: String,
    pub columns: u16,
    pub rows: u16,
    pub capabilities: Vec<Capability>,
}

impl Report {
    pub fn new(columns: u16, rows: u16) -> Self {
        let mut capabilities = Vec::new();
        for (category, entries) in CATALOG {
            for (id, name) in *entries {
                capabilities.push(Capability {
                    id: (*id).into(),
                    name: (*name).into(),
                    category: (*category).into(),
                    status: Status::Unknown,
                    objective_status: Status::Unknown,
                    evidence: Vec::new(),
                    visual: None,
                });
            }
        }
        Self {
            schema_version: 1,
            identity: "Unknown terminal (no correlated identity reply)".into(),
            columns,
            rows,
            capabilities,
        }
    }

    pub fn get(&self, id: &str) -> Option<&Capability> {
        self.capabilities
            .iter()
            .find(|capability| capability.id == id)
    }

    pub fn mark_visual(&mut self, id: &str, pass: bool) {
        if let Some(capability) = self
            .capabilities
            .iter_mut()
            .find(|capability| capability.id == id)
        {
            capability.visual = Some(pass);
            append_evidence(
                capability,
                if pass {
                    "User visual verdict: pass (not objective verification)"
                } else {
                    "User visual verdict: fail (not an objective query failure)"
                },
            );
            update_effective_status(capability);
        }
    }

    pub fn note(&mut self, id: &str, status: Status, evidence: &str) {
        if let Some(capability) = self
            .capabilities
            .iter_mut()
            .find(|capability| capability.id == id)
        {
            // Weaker advertisements and unanswered queries cannot erase an objective
            // observation. A failed objective check also survives a visual pass.
            let preserve = matches!(
                (capability.objective_status, status),
                (_, Status::Unknown)
                    | (
                        Status::Verified,
                        Status::Advertised | Status::Visual | Status::Unsupported
                    )
                    | (Status::Failed, _)
            );
            if !preserve {
                capability.objective_status = status;
            }
            append_evidence(capability, evidence);
            update_effective_status(capability);
        }
    }
}

fn update_effective_status(capability: &mut Capability) {
    capability.status = match capability.visual {
        Some(false) => Status::Failed,
        Some(true) if capability.objective_status == Status::Unknown => Status::Visual,
        _ => capability.objective_status,
    };
}

fn append_evidence(capability: &mut Capability, text: &str) {
    let text = sanitize(text);
    if capability.evidence.last() == Some(&text) {
        return;
    }
    if capability.evidence.len() >= MAX_EVIDENCE {
        capability.evidence.remove(0);
    }
    capability.evidence.push(text);
}

/// Never emit terminal controls, newlines, or bidi overrides from an endpoint.
fn sanitize(text: &str) -> String {
    let mut result = String::with_capacity(text.len().min(MAX_TEXT_BYTES));
    for character in text.chars() {
        if character.is_control()
            || matches!(character, '\u{061c}' | '\u{200e}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        {
            let escaped = character.escape_unicode();
            if result.len() + escaped.clone().count() > MAX_TEXT_BYTES {
                break;
            }
            result.extend(escaped);
        } else {
            if result.len() + character.len_utf8() > MAX_TEXT_BYTES {
                break;
            }
            result.push(character);
        }
    }
    result
}

// Stable IDs are report schema, not terminal-brand heuristics.
type CatalogGroup = (&'static str, &'static [(&'static str, &'static str)]);
const CATALOG: &[CatalogGroup] = &[
    (
        "Display and color",
        &[
            ("ansi16", "ANSI 16 colors"),
            ("indexed256", "Indexed 256 colors"),
            ("truecolor", "24-bit RGB"),
            ("default_colors", "Default foreground/background"),
            ("palette", "Palette query"),
            ("attributes", "Text attributes"),
            ("underlines", "Underline styles"),
            ("underline_color", "Underline color"),
            ("sgr_reset", "SGR reset"),
        ],
    ),
    (
        "Typography and Unicode",
        &[
            ("box", "Box drawing"),
            ("blocks", "Block elements"),
            ("braille", "Braille cells"),
            ("powerline", "Powerline glyphs"),
            ("combining", "Combining marks"),
            ("graphemes", "Grapheme clusters"),
            ("emoji", "Emoji shaping"),
            ("variation", "Variation selectors"),
            ("wide", "Wide characters"),
            ("ambiguous", "Ambiguous-width characters"),
        ],
    ),
    (
        "Graphics",
        &[
            ("kitty_graphics", "Kitty graphics"),
            ("sixel", "Sixel graphics"),
            ("iterm_images", "iTerm inline images"),
        ],
    ),
    (
        "Cursor and screen management",
        &[
            ("overwrite", "Overwrite and CR/BS"),
            ("cursor", "Cursor movement"),
            ("cursor_visibility", "Cursor visibility"),
            ("save_restore", "Save/restore cursor"),
            ("cursor_shape", "Cursor shape"),
            ("erase", "Erase operations"),
            ("insert_delete", "Insert/delete cells and lines"),
            ("scroll_region", "Scroll region"),
            ("origin", "Origin mode"),
            ("autowrap", "Autowrap mode"),
            ("reverse_index", "Reverse index"),
            ("alternate", "Alternate screen"),
            ("sync", "Synchronized output"),
            ("resize", "Resize handling"),
        ],
    ),
    (
        "Keyboard",
        &[
            ("keyboard", "Traditional keys"),
            ("kitty_keyboard", "Kitty keyboard"),
            ("modify_other_keys", "modifyOtherKeys"),
            ("focus", "Focus events"),
            ("paste", "Bracketed paste"),
        ],
    ),
    (
        "Mouse",
        &[
            ("mouse", "Mouse reporting"),
            ("mouse_pixel", "Pixel mouse reporting"),
        ],
    ),
    (
        "Terminal integration",
        &[
            ("da1", "Primary device attributes"),
            ("da2", "Secondary device attributes"),
            ("xtgettcap", "Termcap capability query"),
            ("version", "Terminal version query"),
            ("decrqm", "Private mode query"),
            ("hyperlinks", "OSC 8 hyperlinks"),
            ("clipboard", "OSC 52 clipboard"),
            ("title", "Window title"),
            ("notifications", "Desktop notifications"),
            ("cwd", "Working-directory hint"),
        ],
    ),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Query {
    Da1,
    Da2,
    Version,
    Termcap(&'static str),
    Mode(u16),
    Color(u8),
    Palette(u8),
    KittyGraphics,
    KittyKeyboard,
    ModifyOtherKeys,
    CursorStyle,
}

impl Query {
    fn capability(self) -> &'static str {
        match self {
            Self::Da1 => "da1",
            Self::Da2 => "da2",
            Self::Version => "version",
            Self::Termcap(_) => "xtgettcap",
            Self::Mode(_) => "decrqm",
            Self::Color(_) => "default_colors",
            Self::Palette(_) => "palette",
            Self::KittyGraphics => "kitty_graphics",
            Self::KittyKeyboard => "kitty_keyboard",
            Self::ModifyOtherKeys => "modify_other_keys",
            Self::CursorStyle => "cursor_shape",
        }
    }
}

#[derive(Debug)]
struct Pending {
    query: Query,
    deadline: u64,
}

#[derive(Debug, Default)]
pub struct Discovery {
    started: bool,
    pending: Vec<Pending>,
    /// DECRPM values 0..4, keyed only by the queried DEC private mode.
    pub modes: BTreeMap<u16, u8>,
    pub kitty_flags: Option<u32>,
    pub cursor_style: Option<u8>,
    pub modify_other_keys: Option<u8>,
}

impl Discovery {
    pub fn new() -> Self {
        Self::default()
    }

    /// One batch per instance. No enable, reset, clipboard, title or image placement.
    pub fn queries(&mut self, now_ms: u64) -> Vec<u8> {
        if self.started {
            return Vec::new();
        }
        self.started = true;
        let deadline = now_ms.saturating_add(QUERY_TIMEOUT_MS);
        let mut bytes = Vec::new();
        let mut add = |query, sequence: &[u8]| {
            self.pending.push(Pending { query, deadline });
            bytes.extend_from_slice(sequence);
        };
        // Query images only: the endpoint must not store or display this pixel.
        add(
            Query::KittyGraphics,
            b"\x1b_Gi=77331,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\",
        );
        add(Query::KittyKeyboard, b"\x1b[?u");
        add(Query::Da1, b"\x1b[c");
        add(Query::Da2, b"\x1b[>c");
        add(Query::Version, b"\x1b[>0q");
        add(Query::Termcap("Co"), b"\x1bP+q436f\x1b\\");
        add(Query::Termcap("RGB"), b"\x1bP+q524742\x1b\\");
        add(Query::Termcap("TN"), b"\x1bP+q544e\x1b\\");
        add(Query::Color(10), b"\x1b]10;?\x1b\\");
        add(Query::Color(11), b"\x1b]11;?\x1b\\");
        add(Query::Palette(0), b"\x1b]4;0;?\x1b\\");
        add(Query::ModifyOtherKeys, b"\x1b[?4m");
        add(Query::CursorStyle, b"\x1bP$q q\x1b\\");
        for mode in MODES {
            add(Query::Mode(mode), format!("\x1b[?{mode}$p").as_bytes());
        }
        bytes
    }

    /// True means a syntactically valid reply matched a live query exactly.
    /// False leaves the original event available to input/diagnostic consumers.
    pub fn ingest(&mut self, event: &Event, report: &mut Report, now_ms: u64) -> bool {
        self.expire(report, now_ms);
        let Some(response) = Response::parse(event) else {
            return false;
        };
        let matched = self
            .pending
            .iter()
            .filter(|pending| response.matches_query(pending.query))
            .count();
        if matched == 0 || matched != response.query_count() {
            return false;
        }
        self.pending
            .retain(|pending| !response.matches_query(pending.query));
        match response {
            Response::Da1(values) => {
                report.note(
                    "da1",
                    Status::Verified,
                    &format!("Correlated DA1 reply: {values:?}; attributes are advertisements"),
                );
                if values.iter().skip(1).any(|value| *value == 4) {
                    report.note(
                        "sixel",
                        Status::Advertised,
                        "DA1 includes Sixel attribute 4; rendering not tested",
                    );
                }
            }
            Response::Da2(values) => report.note(
                "da2",
                Status::Verified,
                &format!("Correlated DA2 identity fields: {values:?}; not rendering proof"),
            ),
            Response::Version(version) => {
                report.identity = sanitize(&version);
                report.note(
                    "version",
                    Status::Verified,
                    &format!("Correlated XTVERSION: {version}"),
                );
            }
            Response::Termcap(values) => {
                for (name, value) in values {
                    report.note(
                        "xtgettcap",
                        Status::Verified,
                        &format!(
                            "Correlated XTGETTCAP {name}: {}",
                            value.as_deref().unwrap_or("unrecognized")
                        ),
                    );
                    if let Some(value) = value {
                        match name.as_str() {
                            "Co" => {
                                if let Some(colors) = decimal(&value) {
                                    if colors >= 16 {
                                        report.note(
                                            "ansi16",
                                            Status::Advertised,
                                            &format!(
                                                "XTGETTCAP Co={colors}; visual colors unverified"
                                            ),
                                        );
                                    }
                                    if colors >= 256 {
                                        report.note("indexed256", Status::Advertised, &format!("XTGETTCAP Co={colors}; palette rendering unverified"));
                                    }
                                }
                            }
                            "RGB" => report.note(
                                "truecolor",
                                if value == "0" {
                                    Status::Unsupported
                                } else {
                                    Status::Advertised
                                },
                                &format!("XTGETTCAP RGB={value}; gradient rendering unverified"),
                            ),
                            "TN" => {
                                if report.get("version").is_some_and(|capability| {
                                    capability.objective_status != Status::Verified
                                }) {
                                    report.identity = sanitize(&format!("Terminfo hint: {value}"));
                                }
                            }
                            _ => unreachable!("validated termcap name"),
                        }
                    }
                }
            }
            Response::Mode(mode, value) => {
                self.modes.insert(mode, value);
                report.note(
                    "decrqm",
                    Status::Verified,
                    &format!("Correlated DECRPM mode {mode}, state {value}"),
                );
                if let Some(id) = mode_capability(mode) {
                    let supported = self.modes.iter().any(|(reported_mode, state)| {
                        mode_capability(*reported_mode) == Some(id) && *state != 0
                    });
                    let all_negative = MODES
                        .iter()
                        .filter(|mode| mode_capability(**mode) == Some(id))
                        .all(|mode| self.modes.get(mode) == Some(&0));
                    let status = if supported {
                        Status::Advertised
                    } else if all_negative {
                        Status::Unsupported
                    } else {
                        Status::Unknown
                    };
                    report.note(
                        id,
                        status,
                        &format!(
                            "DECRPM ?{mode}={value}; mode recognition, not rendering verification"
                        ),
                    );
                }
            }
            Response::Color(index, value) => report.note(
                "default_colors",
                Status::Advertised,
                &format!("Correlated OSC {index} color {value}; display unverified"),
            ),
            Response::Palette(index, value) => report.note(
                "palette",
                Status::Advertised,
                &format!("Correlated OSC 4 palette {index}: {value}; display unverified"),
            ),
            Response::KittyGraphics(message) => {
                let status = if message == "OK" {
                    Status::Advertised
                } else if message.starts_with("ENOTSUP:") || message == "ENOTSUP" {
                    Status::Unsupported
                } else {
                    Status::Failed
                };
                report.note(
                    "kitty_graphics",
                    status,
                    &format!(
                        "Correlated Kitty query i=77331: {message}; no image rendering tested"
                    ),
                );
            }
            Response::KittyKeyboard(flags) => {
                self.kitty_flags = Some(flags);
                report.note(
                    "kitty_keyboard",
                    Status::Advertised,
                    &format!("Correlated Kitty keyboard flags={flags}; key delivery unverified"),
                );
            }
            Response::ModifyOtherKeys(level) => {
                self.modify_other_keys = Some(level);
                report.note(
                    "modify_other_keys",
                    Status::Advertised,
                    &format!("Correlated modifyOtherKeys level={level}; key delivery unverified"),
                );
            }
            Response::CursorStyle(style) => {
                self.cursor_style = style;
                if let Some(style) = style {
                    report.note(
                        "cursor_shape",
                        Status::Advertised,
                        &format!(
                            "Correlated DECRQSS cursor style={style}; shape rendering unverified"
                        ),
                    );
                } else {
                    report.note(
                        "cursor_shape",
                        Status::Unsupported,
                        "Correlated DECRQSS invalid-request reply for cursor style",
                    );
                }
            }
        }
        true
    }

    pub fn expire(&mut self, report: &mut Report, now_ms: u64) {
        self.pending.retain(|pending| {
            if now_ms < pending.deadline {
                return true;
            }
            report.note(
                pending.query.capability(),
                Status::Unknown,
                &format!(
                    "Query {:?} timed out; absence is not unsupported",
                    pending.query
                ),
            );
            false
        });
    }

    pub fn finished(&self) -> bool {
        self.started && self.pending.is_empty()
    }
}

fn mode_capability(mode: u16) -> Option<&'static str> {
    match mode {
        6 => Some("origin"),
        7 => Some("autowrap"),
        25 => Some("cursor_visibility"),
        1006 => Some("mouse"),
        1004 => Some("focus"),
        1016 => Some("mouse_pixel"),
        1049 => Some("alternate"),
        2004 => Some("paste"),
        2026 => Some("sync"),
        _ => None,
    }
}

#[derive(Debug)]
enum Response {
    Da1(Vec<u32>),
    Da2(Vec<u32>),
    Version(String),
    Termcap(Vec<(String, Option<String>)>),
    Mode(u16, u8),
    Color(u8, String),
    Palette(u8, String),
    KittyGraphics(String),
    KittyKeyboard(u32),
    ModifyOtherKeys(u8),
    CursorStyle(Option<u8>),
}

impl Response {
    fn parse(event: &Event) -> Option<Self> {
        let payload = match event {
            Event::Csi(bytes) | Event::Osc(bytes) | Event::Dcs(bytes) | Event::Apc(bytes) => bytes,
            _ => return None,
        };
        if payload.len() > MAX_SEQUENCE {
            return None;
        }
        let text = std::str::from_utf8(payload).ok()?;
        match event {
            Event::Csi(_) => {
                if let Some(body) = text
                    .strip_prefix('?')
                    .and_then(|text| text.strip_suffix('c'))
                {
                    let values = numbers(body)?;
                    if values.len() > 32 {
                        return None;
                    }
                    return Some(Self::Da1(values));
                }
                if let Some(body) = text
                    .strip_prefix('>')
                    .and_then(|text| text.strip_suffix('c'))
                {
                    let values = numbers(body)?;
                    return (values.len() == 3).then_some(Self::Da2(values));
                }
                if let Some(body) = text
                    .strip_prefix('?')
                    .and_then(|text| text.strip_suffix("$y"))
                {
                    let values = numbers(body)?;
                    if values.len() != 2 || values[1] > 4 {
                        return None;
                    }
                    return Some(Self::Mode(u16::try_from(values[0]).ok()?, values[1] as u8));
                }
                if let Some(body) = text
                    .strip_prefix('?')
                    .and_then(|text| text.strip_suffix('u'))
                {
                    return Some(Self::KittyKeyboard(decimal(body)?));
                }
                if let Some(body) = text
                    .strip_prefix(">4;")
                    .and_then(|text| text.strip_suffix('m'))
                {
                    let value = decimal(body)?;
                    return (value <= 2).then_some(Self::ModifyOtherKeys(value as u8));
                }
                None
            }
            Event::Dcs(_) => {
                if let Some(version) = text.strip_prefix(">|") {
                    return (!version.is_empty()).then(|| Self::Version(version.into()));
                }
                if let Some(style) = text
                    .strip_prefix("1$r")
                    .and_then(|text| text.strip_suffix(" q"))
                {
                    let style = decimal(style)?;
                    return (style <= 6).then_some(Self::CursorStyle(Some(style as u8)));
                }
                if text == "0$r" || text == "0$r q" {
                    return Some(Self::CursorStyle(None));
                }
                let (success, body) = if let Some(body) = text.strip_prefix("1+r") {
                    (true, body)
                } else if let Some(body) = text.strip_prefix("0+r") {
                    (false, body)
                } else {
                    return None;
                };
                let mut values = Vec::new();
                for field in body.split(';') {
                    if values.len() == 3 {
                        return None;
                    }
                    let (name, value) = if success {
                        let (name, value) = field.split_once('=')?;
                        (hex_decode(name)?, Some(hex_decode(value)?))
                    } else {
                        (hex_decode(field)?, None)
                    };
                    if !matches!(name.as_str(), "Co" | "RGB" | "TN")
                        || values.iter().any(|(existing, _)| *existing == name)
                    {
                        return None;
                    }
                    // Color counts are decimal; RGB's boolean/numeric or channel-bit forms
                    // are advertisements. Arbitrary TN identity is sanitized when exported.
                    if name == "Co"
                        && value
                            .as_deref()
                            .is_some_and(|value| decimal(value).is_none())
                    {
                        return None;
                    }
                    if name == "RGB" && value.as_deref().is_some_and(|value| !valid_rgb_cap(value))
                    {
                        return None;
                    }
                    if name == "TN" && value.as_deref() == Some("") {
                        return None;
                    }
                    values.push((name, value));
                }
                Some(Self::Termcap(values))
            }
            Event::Osc(_) => {
                let mut fields = text.split(';');
                let command = decimal(fields.next()?)?;
                if command == 10 || command == 11 {
                    let value = fields.next()?;
                    if fields.next().is_some() || !valid_color(value) {
                        return None;
                    }
                    Some(Self::Color(command as u8, value.into()))
                } else if command == 4 {
                    let index = u8::try_from(decimal(fields.next()?)?).ok()?;
                    let value = fields.next()?;
                    if fields.next().is_some() || !valid_color(value) {
                        return None;
                    }
                    Some(Self::Palette(index, value.into()))
                } else {
                    None
                }
            }
            Event::Apc(_) => {
                let body = text.strip_prefix('G')?;
                let (header, message) = body.split_once(';')?;
                // Our query did not request a placement; reject unrelated response keys.
                let id = decimal(header.strip_prefix("i=")?)?;
                if id != 77331
                    || message.is_empty()
                    || !message.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
                {
                    return None;
                }
                Some(Self::KittyGraphics(message.into()))
            }
            _ => None,
        }
    }

    fn query_count(&self) -> usize {
        match self {
            Self::Termcap(values) => values.len(),
            _ => 1,
        }
    }

    fn matches_query(&self, query: Query) -> bool {
        match (self, query) {
            (Self::Da1(_), Query::Da1)
            | (Self::Da2(_), Query::Da2)
            | (Self::Version(_), Query::Version)
            | (Self::KittyGraphics(_), Query::KittyGraphics)
            | (Self::KittyKeyboard(_), Query::KittyKeyboard)
            | (Self::ModifyOtherKeys(_), Query::ModifyOtherKeys)
            | (Self::CursorStyle(_), Query::CursorStyle) => true,
            (Self::Termcap(values), Query::Termcap(name)) => values
                .iter()
                .any(|(reported_name, _)| reported_name == name),
            (Self::Mode(mode, _), Query::Mode(queried)) => *mode == queried,
            (Self::Color(index, _), Query::Color(queried)) => *index == queried,
            (Self::Palette(index, _), Query::Palette(queried)) => *index == queried,
            _ => false,
        }
    }
}

fn decimal(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn numbers(text: &str) -> Option<Vec<u32>> {
    if text.len() > 384 {
        return None;
    }
    text.split(';').map(decimal).collect()
}

fn hex_decode(text: &str) -> Option<String> {
    if text.len() & 1 != 0 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let bytes: Option<Vec<u8>> = text
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digit = |byte: u8| (byte as char).to_digit(16).map(|value| value as u8);
            Some(digit(pair[0])? * 16 + digit(pair[1])?)
        })
        .collect();
    String::from_utf8(bytes?).ok()
}

fn valid_color(text: &str) -> bool {
    let Some(body) = text.strip_prefix("rgb:") else {
        return false;
    };
    let mut count = 0;
    for component in body.split('/') {
        if component.is_empty()
            || component.len() > 4
            || !component.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return false;
        }
        count += 1;
    }
    count == 3
}

fn valid_rgb_cap(text: &str) -> bool {
    if text.is_empty() || decimal(text).is_some() {
        return true;
    }
    let mut count = 0;
    for component in text.split('/') {
        if !decimal(component).is_some_and(|bits| (1..=32).contains(&bits)) {
            return false;
        }
        count += 1;
    }
    count == 3
}

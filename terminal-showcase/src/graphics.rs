//! Direct, bounded image demonstrations and strict endpoint input decoding.
//!
//! `frame` coordinates are 1-based and dimensions are canvas cells, not pixels.
//! Raster sizes never exceed 64×32. Four parent ticks advance one lifecycle phase:
//! two images → replace → reposition → crop → overwrite text → scroll → delete
//! secondary → delete primary. Scheduling/rate and explicit protocol consent belong
//! to the caller. Protocol acknowledgments do not prove visual rendering.
//!
//! Kitty provides addressable placements/deletion; Sixel and OSC 1337 do not.
//! Their replacement/deletion is a bounded text-erase experiment, not a guarantee
//! that the endpoint frees raster storage. No files, sockets or shared memory are
//! used. Input mouse coordinates retain wire units: SGR 1006 cells and 1016 pixels
//! have identical framing, so the caller must label its enabled reporting mode.
//! Legacy CSI M reports use exactly three offset bytes (cell coordinates); UTF-8
//! mouse encoding 1005 is not decoded and must be disabled by the terminal guard.

use std::fmt::Write as _;

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};

use crate::protocol::Event;

const PRIMARY: u32 = 77332;
const SECONDARY: u32 = 77333;
const PALETTE: [[u8; 3]; 4] = [
    [24, 36, 54],
    [54, 196, 208],
    [242, 122, 106],
    [223, 232, 242],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Kitty,
    Sixel,
    Iterm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Canvas {
    protocol: Protocol,
    row: u16,
    column: u16,
    width: u16,
    height: u16,
}

#[derive(Default)]
pub struct Graphics {
    canvas: Option<Canvas>,
    phase: Option<u8>,
    kitty_images: [bool; 2],
    raster_emitted: bool,
}

impl Graphics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the latest frame/clear emitted raster data, not merely control bytes.
    /// Transmission is not evidence that the endpoint displayed the image.
    pub fn raster_emitted(&self) -> bool {
        self.raster_emitted
    }

    /// Non-storing, non-placing direct RGB query; never use a demo image ID here.
    pub fn query() -> Vec<u8> {
        b"\x1b_Ga=q,t=d,i=77331,f=24,s=1,v=1;AAAA\x1b\\".to_vec()
    }

    /// Emit only changed lifecycle phases/geometry. Caller owns low-rate scheduling.
    pub fn frame(
        &mut self,
        protocol: Protocol,
        tick: u64,
        row: u16,
        column: u16,
        width: u16,
        height: u16,
    ) -> Vec<u8> {
        self.raster_emitted = false;
        if width == 0 || height == 0 {
            return self.clear();
        }
        let row = row.max(1);
        let column = column.max(1);
        let canvas = Canvas {
            protocol,
            row,
            column,
            width: width.min(64).min(u16::MAX - column + 1),
            height: height.min(32).min(u16::MAX - row + 1),
        };
        let phase = ((tick / 4) % 8) as u8;
        let changed_canvas = self.canvas != Some(canvas);
        if !changed_canvas && self.phase == Some(phase) {
            return Vec::new();
        }
        let mut out = String::with_capacity(20_000);
        if changed_canvas {
            let old = self.clear();
            out.push_str(std::str::from_utf8(&old).expect("graphics output is ASCII"));
        }
        out.push_str("\x1b7");
        if protocol != Protocol::Kitty {
            erase_canvas(&mut out, canvas);
        }
        canvas_text(&mut out, canvas, phase);
        let image_row = row.saturating_add(u16::from(canvas.height > 1));
        let image_height = canvas
            .height
            .saturating_sub(if canvas.height > 2 { 2 } else { 1 })
            .clamp(1, 8);
        let image_width = (canvas.width / 2).clamp(1, 32);
        let second_column = column + canvas.width.saturating_sub(image_width);
        let pixel_width = canvas.width.saturating_mul(2).min(64);
        let pixel_height = canvas.height.saturating_mul(4).min(32);
        if protocol == Protocol::Kitty {
            // Entering midway or resizing creates the necessary images before actions.
            for (index, id, col) in [(0, PRIMARY, column), (1, SECONDARY, second_column)] {
                if !self.kitty_images[index]
                    && phase < 6
                    && phase != 0
                    && !(phase == 1 && index == 0)
                {
                    kitty_image(
                        &mut out,
                        id,
                        image_row,
                        col,
                        image_width,
                        image_height,
                        pixel_width,
                        pixel_height,
                        phase + index as u8,
                    );
                    self.raster_emitted = true;
                    self.kitty_images[index] = true;
                }
            }
            match phase {
                0 | 1 => {
                    kitty_image(
                        &mut out,
                        PRIMARY,
                        image_row,
                        column,
                        image_width,
                        image_height,
                        pixel_width,
                        pixel_height,
                        phase,
                    );
                    self.raster_emitted = true;
                    self.kitty_images[0] = true;
                    if phase == 0 {
                        kitty_image(
                            &mut out,
                            SECONDARY,
                            image_row,
                            second_column,
                            image_width,
                            image_height,
                            pixel_width,
                            pixel_height,
                            2,
                        );
                        self.kitty_images[1] = true;
                        self.raster_emitted = true;
                    }
                }
                2 => {
                    cup(
                        &mut out,
                        image_row,
                        column + canvas.width.saturating_sub(image_width) / 2,
                    );
                    let _ = write!(
                        out,
                        "\x1b_Ga=p,i={SECONDARY},p=1,c={image_width},r={image_height},C=1,q=2;\x1b\\"
                    );
                }
                3 => {
                    cup(&mut out, image_row, column);
                    let x = pixel_width / 4;
                    let y = pixel_height / 4;
                    let _ = write!(
                        out,
                        "\x1b_Ga=p,i={PRIMARY},p=1,x={x},y={y},w={},h={},c={},r={},C=1,q=2;\x1b\\",
                        (pixel_width / 2).max(1),
                        (pixel_height / 2).max(1),
                        (image_width / 2).max(1),
                        (image_height / 2).max(1)
                    );
                }
                4 => write_at(
                    &mut out,
                    image_row,
                    column,
                    canvas.width,
                    "text overwrites image cells",
                ),
                5 => scroll_canvas(&mut out, canvas),
                6 | 7 => {
                    let index = usize::from(phase == 6);
                    let id = if index == 0 { PRIMARY } else { SECONDARY };
                    delete_kitty(&mut out, id);
                    self.kitty_images[index] = false;
                }
                _ => unreachable!(),
            }
        } else {
            // These protocols have no image IDs. Redraw bounded inline regions; never
            // claim their erase semantics or storage cleanup have been verified.
            let visible = match phase {
                6 => 1,
                7 => 0,
                _ => 2,
            };
            for index in 0..visible {
                let col = if index == 0 {
                    column
                } else if phase == 2 {
                    column + canvas.width.saturating_sub(image_width) / 2
                } else {
                    second_column
                };
                cup(&mut out, image_row, col);
                let w = if phase == 3 {
                    (pixel_width / 2).max(1)
                } else {
                    pixel_width
                };
                let h = if phase == 3 {
                    (pixel_height / 2).max(1)
                } else {
                    pixel_height
                };
                match protocol {
                    Protocol::Sixel => sixel(&mut out, w, h, phase + index),
                    Protocol::Iterm => {
                        let png = png(w, h, phase + index);
                        let _ = write!(
                            out,
                            "\x1b]1337;File=inline=1;size={};width={image_width};height={image_height};preserveAspectRatio=0:{}\x07",
                            png.len(),
                            STANDARD.encode(png)
                        );
                    }
                    Protocol::Kitty => unreachable!(),
                }
                self.raster_emitted = true;
            }
            if phase == 4 {
                write_at(
                    &mut out,
                    image_row,
                    column,
                    canvas.width,
                    "text overwrite: inspect visually",
                );
            }
            if phase == 5 {
                scroll_canvas(&mut out, canvas);
            }
        }
        out.push_str("\x1b8");
        self.canvas = Some(canvas);
        self.phase = Some(phase);
        out.into_bytes()
    }

    /// Deletes only owned Kitty IDs. Legacy raster cleanup remains visual evidence.
    pub fn clear(&mut self) -> Vec<u8> {
        self.raster_emitted = false;
        let mut out = String::new();
        if self
            .canvas
            .is_some_and(|canvas| canvas.protocol == Protocol::Kitty)
        {
            delete_kitty(&mut out, PRIMARY);
            delete_kitty(&mut out, SECONDARY);
        }
        if let Some(canvas) = self.canvas {
            out.push_str("\x1b7");
            erase_canvas(&mut out, canvas);
            out.push_str("\x1b8");
        }
        self.kitty_images = [false; 2];
        self.canvas = None;
        self.phase = None;
        out.into_bytes()
    }
}

fn cup(out: &mut String, row: u16, column: u16) {
    let _ = write!(out, "\x1b[{row};{column}H");
}

fn write_at(out: &mut String, row: u16, column: u16, width: u16, text: &str) {
    cup(out, row, column);
    let _ = write!(out, "\x1b[{width}X");
    out.push_str(&text[..text.len().min(usize::from(width))]);
}

fn canvas_text(out: &mut String, canvas: Canvas, phase: u8) {
    const LABELS: [&str; 8] = [
        "two procedural images",
        "replace primary image",
        "move secondary placement",
        "crop / clipping",
        "text over image",
        "scroll image with text",
        "delete secondary image",
        "delete primary image",
    ];
    write_at(
        out,
        canvas.row,
        canvas.column,
        canvas.width,
        LABELS[usize::from(phase)],
    );
    if canvas.height > 2 {
        write_at(
            out,
            canvas.row + (canvas.height - 1),
            canvas.column,
            canvas.width,
            "surrounding text: visual verdict only",
        );
    }
}

fn erase_canvas(out: &mut String, canvas: Canvas) {
    for offset in 0..canvas.height {
        cup(out, canvas.row + offset, canvas.column);
        let _ = write!(out, "\x1b[{}X", canvas.width);
    }
}

fn scroll_canvas(out: &mut String, canvas: Canvas) {
    if canvas.height > 2 {
        // The bounded scroll region prevents demo images entering scrollback.
        let _ = write!(
            out,
            "\x1b[{};{}r\x1b[1S\x1b[r",
            canvas.row + 1,
            canvas.row + (canvas.height - 1)
        );
    }
}

fn delete_kitty(out: &mut String, id: u32) {
    let _ = write!(out, "\x1b_Ga=d,d=I,i={id},q=2;\x1b\\");
}

fn color_index(x: u16, y: u16, phase: u8) -> usize {
    usize::from(((x / 8 + y / 4 + u16::from(phase)) % 4) as u8)
}

fn rgb(width: u16, height: u16, phase: u8) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(usize::from(width) * usize::from(height) * 3);
    for y in 0..height {
        for x in 0..width {
            pixels.extend_from_slice(&PALETTE[color_index(x, y, phase)]);
        }
    }
    pixels
}

#[allow(clippy::too_many_arguments)]
fn kitty_image(
    out: &mut String,
    id: u32,
    row: u16,
    column: u16,
    cells_w: u16,
    cells_h: u16,
    width: u16,
    height: u16,
    phase: u8,
) {
    cup(out, row, column);
    let encoded = STANDARD.encode(rgb(width, height, phase));
    let chunks = encoded.as_bytes().chunks(4096);
    let count = chunks.len();
    for (index, chunk) in chunks.enumerate() {
        if index == 0 {
            let _ = write!(
                out,
                "\x1b_Ga=T,t=d,f=24,i={id},p=1,s={width},v={height},c={cells_w},r={cells_h},C=1,q=2,m={};",
                u8::from(index + 1 < count)
            );
        } else {
            let _ = write!(out, "\x1b_Gm={},q=2;", u8::from(index + 1 < count));
        }
        out.push_str(std::str::from_utf8(chunk).expect("base64 is ASCII"));
        out.push_str("\x1b\\");
    }
}

fn sixel(out: &mut String, width: u16, height: u16, phase: u8) {
    let _ = write!(out, "\x1bP0;1;0q\"1;1;{width};{height}");
    for (index, color) in PALETTE.iter().enumerate() {
        let _ = write!(
            out,
            "#{index};2;{};{};{}",
            u16::from(color[0]) * 100 / 255,
            u16::from(color[1]) * 100 / 255,
            u16::from(color[2]) * 100 / 255
        );
    }
    for y in (0..height).step_by(6) {
        for index in 0..4 {
            let _ = write!(out, "#{index}");
            for x in 0..width {
                let mut bits = 0u8;
                for dy in 0..6 {
                    if y + dy < height && color_index(x, y + dy, phase) == index {
                        bits |= 1 << dy;
                    }
                }
                out.push(char::from(63 + bits));
            }
            out.push('$');
        }
        if y + 6 < height {
            out.push('-');
        }
    }
    out.push_str("\x1b\\");
}

// One bounded stored DEFLATE block: no compressor dependency or external I/O.
fn png(width: u16, height: u16, phase: u8) -> Vec<u8> {
    // At most 6176 bytes: one stored DEFLATE block suffices.
    let len = height * (1 + width * 3);
    let mut result = Vec::with_capacity(usize::from(len) + 68);
    result.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut header = [0u8; 13];
    header[..4].copy_from_slice(&u32::from(width).to_be_bytes());
    header[4..8].copy_from_slice(&u32::from(height).to_be_bytes());
    header[8] = 8;
    header[9] = 2;
    png_chunk(&mut result, b"IHDR", &header);
    result.extend_from_slice(&(u32::from(len) + 11).to_be_bytes());
    let start = result.len();
    result.extend_from_slice(b"IDAT");
    result.extend_from_slice(&[0x78, 0x01, 0x01]);
    result.extend_from_slice(&len.to_le_bytes());
    result.extend_from_slice(&(!len).to_le_bytes());
    let (mut a, mut b) = (1u32, 0u32);
    for y in 0..height {
        result.push(0); // PNG filter None.
        b = (b + a) % 65521;
        for x in 0..width {
            for byte in PALETTE[color_index(x, y, phase)] {
                result.push(byte);
                a = (a + u32::from(byte)) % 65521;
                b = (b + a) % 65521;
            }
        }
    }
    result.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let crc = crc32(&result[start..]);
    result.extend_from_slice(&crc.to_be_bytes());
    png_chunk(&mut result, b"IEND", &[]);
    result
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let start = out.len() - data.len() - kind.len();
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

/// Only uppercase explicit actions change endpoint state; no reads or user data.
pub fn integration_action(key: u8) -> Option<Vec<u8>> {
    Some(match key {
        b'C' => b"\x1b]52;c;dGVybWluYWwtc2hvd2Nhc2U=\x07".to_vec(),
        b'N' => b"\x1b]9;terminal-showcase: explicit notification demo\x07".to_vec(),
        b'T' => b"\x1b]2;terminal-showcase: explicit title demo\x07".to_vec(),
        b'W' => b"\x1b]7;file://localhost/tmp\x07".to_vec(),
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyProtocol {
    Traditional,
    Kitty,
    ModifyOtherKeys,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyKind {
    Press,
    Repeat,
    Release,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseKind {
    Press,
    Release,
    Drag,
    Motion,
    Wheel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseProtocol {
    Sgr,
    Legacy,
}

/// Modifiers are un-offset bits: shift=1, alt=2, ctrl=4, super=8,
/// hyper=16, meta=32, caps-lock=64, num-lock=128. Legacy ambiguity remains.
/// Key codes use Unicode/Kitty functional codes, including private-use keys.
/// Mouse button is low two wire bits (0 left, 1 middle, 2 right); for wheel
/// 0/1/2/3 means up/down/left/right. Extended buttons 8–11 use zero-based
/// indices 7–10. Coordinates are 1-based wire units.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum InputEvent {
    Key {
        protocol: KeyProtocol,
        code: u32,
        modifiers: u16,
        kind: KeyKind,
        shifted: Option<u32>,
        base_layout: Option<u32>,
        text: String,
    },
    Text {
        text: String,
        alt: bool,
    },
    Mouse {
        protocol: MouseProtocol,
        button: Option<u8>,
        kind: MouseKind,
        modifiers: u16,
        x: u32,
        y: u32,
    },
    Focus {
        focused: bool,
    },
    Paste {
        start: bool,
    },
}

/// Strict syntax dispatch: replies, malformed sequences and invalid Unicode are
/// not converted into key events. Parser-tagged paste data is never a keyboard event.
pub fn decode_input(event: &Event) -> Option<InputEvent> {
    match event {
        Event::Input(bytes) => traditional(bytes, false),
        Event::Escape(bytes) => {
            // Intermediate escape controls and PM/SOS strings are not Alt keys.
            if bytes
                .first()
                .is_some_and(|byte| (0x20..=0x2f).contains(byte) || matches!(*byte, b'^' | b'X'))
            {
                return None;
            }
            if bytes.len() == 2 && bytes[0] == b'O' {
                return functional(bytes[1])
                    .map(|code| key(KeyProtocol::Traditional, code, 0, KeyKind::Press));
            }
            traditional(bytes, true)
        }
        Event::Csi(bytes) => decode_csi(bytes),
        Event::LegacyMouse(bytes) => legacy_mouse(bytes),
        _ => None,
    }
}

fn key(protocol: KeyProtocol, code: u32, modifiers: u16, kind: KeyKind) -> InputEvent {
    InputEvent::Key {
        protocol,
        code,
        modifiers,
        kind,
        shifted: None,
        base_layout: None,
        text: String::new(),
    }
}

/// Allocation-free legacy ASCII normalization shared with live batched input.
pub fn traditional_byte(byte: u8, alt: bool) -> Option<InputEvent> {
    let (code, control) = match byte {
        0 => (32, true),
        8 | 127 => (127, false),
        9 => (9, false),
        10 | 13 => (13, false),
        27 => (27, false),
        n @ 1..=26 => (u32::from(n) + 96, true),
        n @ 28..=31 => (u32::from(n) + 64, true),
        n @ 32..=126 => (u32::from(n), false),
        _ => return None,
    };
    Some(key(
        KeyProtocol::Traditional,
        code,
        (if alt { 2 } else { 0 }) | (if control { 4 } else { 0 }),
        KeyKind::Press,
    ))
}

fn traditional(bytes: &[u8], alt: bool) -> Option<InputEvent> {
    if bytes.len() == 1 && matches!(bytes[0], 0..=31 | 127) {
        if let Some(input) = traditional_byte(bytes[0], alt) {
            return Some(input);
        }
    }
    let text = std::str::from_utf8(bytes).ok()?;
    if text.is_empty() || text.chars().any(char::is_control) {
        return None;
    }
    Some(InputEvent::Text {
        text: text.into(),
        alt,
    })
}

fn decimal(value: &str) -> Option<u32> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

fn scalar(value: &str) -> Option<u32> {
    let code = decimal(value)?;
    char::from_u32(code)?;
    Some(code)
}

fn modifiers_kind(value: Option<&str>) -> Option<(u16, KeyKind)> {
    let Some(value) = value else {
        return Some((0, KeyKind::Press));
    };
    if value.is_empty() {
        return Some((0, KeyKind::Press));
    }
    let mut fields = value.split(':');
    let modifiers = decimal(fields.next()?)?;
    if !(1..=256).contains(&modifiers) {
        return None;
    }
    let kind = match fields.next() {
        None => KeyKind::Press,
        Some("1") => KeyKind::Press,
        Some("2") => KeyKind::Repeat,
        Some("3") => KeyKind::Release,
        _ => return None,
    };
    if fields.next().is_some() {
        return None;
    }
    Some(((modifiers - 1) as u16, kind))
}

fn decode_csi(bytes: &[u8]) -> Option<InputEvent> {
    let (&final_byte, parameters) = bytes.split_last()?;
    let parameters = std::str::from_utf8(parameters).ok()?;
    if matches!(final_byte, b'M' | b'm') {
        return mouse(parameters, final_byte == b'm');
    }
    if parameters.is_empty() {
        match final_byte {
            b'I' => return Some(InputEvent::Focus { focused: true }),
            b'O' => return Some(InputEvent::Focus { focused: false }),
            b'Z' => return Some(key(KeyProtocol::Traditional, 9, 1, KeyKind::Press)),
            _ => {}
        }
    }
    if final_byte == b'~' {
        match parameters {
            "200" => return Some(InputEvent::Paste { start: true }),
            "201" => return Some(InputEvent::Paste { start: false }),
            _ => {}
        }
    }
    let mut fields = parameters.split(';');
    let first = fields.next()?;
    let second = fields.next();
    let third = fields.next();
    if fields.next().is_some() {
        return None;
    }
    if final_byte == b'u' {
        let mut codes = first.split(':');
        let code = scalar(codes.next()?)?;
        let shifted = match codes.next() {
            None | Some("") => None,
            Some(value) => Some(scalar(value)?),
        };
        let base_layout = match codes.next() {
            None => None,
            Some(value) => Some(scalar(value)?),
        };
        if codes.next().is_some() {
            return None;
        }
        let (modifiers, kind) = modifiers_kind(second)?;
        let mut text = String::new();
        if let Some(value) = third {
            for number in value.split(':') {
                text.push(char::from_u32(scalar(number)?)?);
            }
        }
        if code == 0 && text.is_empty() {
            return None;
        }
        return Some(InputEvent::Key {
            protocol: KeyProtocol::Kitty,
            code,
            modifiers,
            kind,
            shifted,
            base_layout,
            text,
        });
    }
    if final_byte == b'~' && first == "27" {
        let second = second?;
        if !(1..=8).contains(&decimal(second)?) {
            return None;
        }
        let (modifiers, kind) = modifiers_kind(Some(second))?;
        return Some(key(
            KeyProtocol::ModifyOtherKeys,
            scalar(third?)?,
            modifiers,
            kind,
        ));
    }
    if third.is_some() {
        return None;
    }
    let (modifiers, kind) = modifiers_kind(second)?;
    let code = if final_byte == b'~' {
        tilde(decimal(first)?)?
    } else {
        if !parameters.is_empty() && first != "1" {
            return None;
        }
        functional(final_byte)?
    };
    // CSI R is a cursor-position reply as well as a historical F3 encoding;
    // only SS3 R is interpreted as F3 to avoid fabricating a key from a CPR.
    if final_byte == b'R' {
        return None;
    }
    let protocol = if second.is_some_and(|value| value.contains(':')) {
        KeyProtocol::Kitty
    } else {
        KeyProtocol::Traditional
    };
    Some(key(protocol, code, modifiers, kind))
}

fn functional(final_byte: u8) -> Option<u32> {
    Some(match final_byte {
        b'A' => 57352,
        b'B' => 57353,
        b'C' => 57351,
        b'D' => 57350,
        b'H' => 57356,
        b'F' => 57357,
        b'P' => 57364,
        b'Q' => 57365,
        b'R' => 57366,
        b'S' => 57367,
        _ => return None,
    })
}

fn tilde(number: u32) -> Option<u32> {
    Some(match number {
        1 | 7 => 57356,
        4 | 8 => 57357,
        2 => 57348,
        3 => 57349,
        5 => 57354,
        6 => 57355,
        11..=15 => 57364 + number - 11,
        17..=21 => 57369 + number - 17,
        23..=24 => 57374 + number - 23,
        29 => 57363,
        _ => return None,
    })
}

fn mouse(parameters: &str, release: bool) -> Option<InputEvent> {
    let mut fields = parameters.strip_prefix('<')?.split(';');
    let button = decimal(fields.next()?)?;
    let x = decimal(fields.next()?)?;
    let y = decimal(fields.next()?)?;
    if fields.next().is_some() {
        return None;
    }
    mouse_event(MouseProtocol::Sgr, button, x, y, release)
}

fn legacy_mouse(bytes: &[u8]) -> Option<InputEvent> {
    let (button, x, y) = if let &[button, x, y] = bytes {
        (u32::from(button), u32::from(x), u32::from(y))
    } else {
        let mut fields = std::str::from_utf8(bytes).ok()?.chars();
        let values = (
            fields.next()? as u32,
            fields.next()? as u32,
            fields.next()? as u32,
        );
        if fields.next().is_some() {
            return None;
        }
        values
    };
    let (button, x, y) = (
        button.checked_sub(32)?,
        x.checked_sub(32)?,
        y.checked_sub(32)?,
    );
    // Legacy release identifies no button, unlike SGR's explicit release final.
    let release = button & (3 | 32 | 64 | 128) == 3;
    let mut event = mouse_event(MouseProtocol::Legacy, button, x, y, release)?;
    if release {
        if let InputEvent::Mouse { button, .. } = &mut event {
            *button = None;
        }
    }
    Some(event)
}

fn mouse_event(
    protocol: MouseProtocol,
    button: u32,
    x: u32,
    y: u32,
    release: bool,
) -> Option<InputEvent> {
    if x == 0 || y == 0 || button > 255 {
        return None;
    }
    let modifiers = ((button & 4) >> 2) | ((button & 8) >> 2) | ((button & 16) >> 2);
    let motion = button & 32 != 0;
    let wheel = button & 64 != 0;
    let extended = button & 128 != 0;
    let low = (button & 3) as u8;
    if wheel && (motion || release || extended) {
        return None;
    }
    if release && motion {
        return None;
    }
    let kind = if wheel {
        MouseKind::Wheel
    } else if release {
        MouseKind::Release
    } else if motion && low == 3 && !extended {
        MouseKind::Motion
    } else if motion {
        MouseKind::Drag
    } else {
        MouseKind::Press
    };
    if !motion && !release && !wheel && !extended && low == 3 {
        return None;
    }
    Some(InputEvent::Mouse {
        protocol,
        button: if motion && low == 3 && !extended {
            None
        } else {
            Some(low + if extended { 7 } else { 0 })
        },
        kind,
        modifiers: modifiers as u16,
        x,
        y,
    })
}

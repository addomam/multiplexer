//! Pure, bounded terminal frames. Protocol actions belong to the terminal owner;
//! a frame contains only printable graphemes, SGR, and optional OSC 8 links.
use crate::capability::{Report, Status};
use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const RESET: &str = "\x1b[0m";
const LINK_END: &str = "\x1b]8;;\x1b\\";
const BG: &str = "\x1b[48;2;15;23;34m";
const WHITE: &str = "\x1b[38;2;221;230;239m";
const MUTED: &str = "\x1b[38;2;132;151;173m";
const CYAN: &str = "\x1b[38;2;100;221;220m";
const CORAL: &str = "\x1b[38;2;251;146;128m";
const CATEGORIES: [(&str, &str); 7] = [
    ("Display and color", "Display & color"),
    ("Typography and Unicode", "Typography & Unicode"),
    ("Graphics", "Graphics"),
    ("Cursor and screen management", "Cursor & screen"),
    ("Keyboard", "Keyboard"),
    ("Mouse", "Mouse"),
    ("Terminal integration", "Terminal integration"),
];

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Demo {
    Overview,
    Colors,
    Unicode,
    Typography,
    Graphics,
    Control,
    Input,
    Integration,
}

impl Demo {
    pub const ALL: [Self; 8] = [
        Self::Overview,
        Self::Colors,
        Self::Unicode,
        Self::Typography,
        Self::Graphics,
        Self::Control,
        Self::Input,
        Self::Integration,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Colors => "colors",
            Self::Unicode => "unicode",
            Self::Typography => "typography",
            Self::Graphics => "graphics",
            Self::Control => "control",
            Self::Input => "input",
            Self::Integration => "integration",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Colors => "Color laboratory",
            Self::Unicode => "Unicode geometry",
            Self::Typography => "Type specimen",
            Self::Graphics => "Graphics atelier",
            Self::Control => "Screen mechanics",
            Self::Input => "Input inspector",
            Self::Integration => "Terminal integration",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|demo| demo.id() == value)
    }

    pub fn capability_ids(self) -> &'static [&'static str] {
        match self {
            Self::Overview => &[
                "truecolor",
                "box",
                "blocks",
                "combining",
                "graphemes",
                "emoji",
                "wide",
                "attributes",
                "underlines",
                "braille",
            ],
            Self::Colors => &[
                "ansi16",
                "indexed256",
                "truecolor",
                "default_colors",
                "palette",
            ],
            Self::Unicode => &[
                "box",
                "blocks",
                "braille",
                "powerline",
                "combining",
                "graphemes",
                "emoji",
                "variation",
                "wide",
                "ambiguous",
                "overwrite",
            ],
            Self::Typography => &["attributes", "underlines", "underline_color", "sgr_reset"],
            Self::Graphics => &["kitty_graphics", "sixel", "iterm_images", "braille"],
            Self::Control => &[
                "cursor",
                "cursor_visibility",
                "save_restore",
                "cursor_shape",
                "erase",
                "insert_delete",
                "scroll_region",
                "origin",
                "autowrap",
                "reverse_index",
                "alternate",
                "sync",
                "resize",
            ],
            Self::Input => &[
                "keyboard",
                "kitty_keyboard",
                "modify_other_keys",
                "mouse",
                "mouse_pixel",
                "focus",
                "paste",
            ],
            Self::Integration => &[
                "da1",
                "da2",
                "xtgettcap",
                "version",
                "decrqm",
                "hyperlinks",
                "clipboard",
                "title",
                "notifications",
                "cwd",
            ],
        }
    }
}

#[derive(Clone, Debug)]
pub struct View {
    pub demo: Demo,
    pub tick: u64,
    pub paused: bool,
    pub screenshot: bool,
    pub expanded: bool,
    pub diagnostics: bool,
    pub mouse_pixel_requested: bool,
    pub input_log: Vec<String>,
}

impl View {
    pub fn new(demo: Demo, screenshot: bool) -> Self {
        Self {
            demo,
            tick: if screenshot { 42 } else { 0 },
            paused: false,
            screenshot,
            expanded: false,
            diagnostics: false,
            mouse_pixel_requested: false,
            input_log: Vec::new(),
        }
    }
}

// Escape sequences are kept at their visible byte offsets. Segmenting the
// unstyled text first preserves a grapheme even when SGR splits its codepoints.
struct Visible<'a> {
    text: String,
    styles: Vec<(usize, &'a str)>,
}

fn visible(input: &str, collect_styles: bool) -> Visible<'_> {
    let bytes = input.as_bytes();
    let mut text = String::with_capacity(input.len());
    let mut styles = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != 0x1b {
            let ch = input[cursor..].chars().next().expect("UTF-8 boundary");
            if !ch.is_control() {
                text.push(ch);
            }
            cursor += ch.len_utf8();
            continue;
        }
        let start = cursor;
        cursor += 1;
        match bytes.get(cursor).copied() {
            Some(b'[') => {
                cursor += 1;
                while cursor < bytes.len() && !(0x40..=0x7e).contains(&bytes[cursor]) {
                    cursor += 1;
                }
                if cursor < bytes.len() {
                    cursor += 1;
                    if collect_styles
                        && bytes[cursor - 1] == b'm'
                        && bytes[start + 2..cursor - 1]
                            .iter()
                            .all(|byte| byte.is_ascii_digit() || matches!(*byte, b';' | b':'))
                    {
                        styles.push((text.len(), &input[start..cursor]));
                    }
                }
            }
            Some(b']' | b'P' | b'_' | b'^' | b'X') => {
                let osc = bytes[cursor] == b']';
                cursor += 1;
                let payload_start = cursor;
                let mut terminated = false;
                while cursor < bytes.len() {
                    if osc && bytes[cursor] == 7 {
                        cursor += 1;
                        terminated = true;
                        break;
                    }
                    if bytes[cursor] == 0x1b && bytes.get(cursor + 1) == Some(&b'\\') {
                        cursor += 2;
                        terminated = true;
                        break;
                    }
                    cursor += 1;
                }
                if collect_styles && terminated && osc && input[payload_start..].starts_with("8;") {
                    styles.push((text.len(), &input[start..cursor]));
                }
            }
            Some(byte) if (0x20..=0x2f).contains(&byte) => {
                while bytes
                    .get(cursor)
                    .is_some_and(|byte| (0x20..=0x2f).contains(byte))
                {
                    cursor += 1;
                }
                if cursor < bytes.len() {
                    cursor += input[cursor..]
                        .chars()
                        .next()
                        .expect("UTF-8 boundary")
                        .len_utf8();
                }
            }
            Some(_) => {
                cursor += input[cursor..]
                    .chars()
                    .next()
                    .expect("UTF-8 boundary")
                    .len_utf8();
            }
            None => {}
        }
    }
    Visible { text, styles }
}

/// Width of printable, ANSI/OSC-stripped graphemes, not byte or scalar count.
pub fn cell_width(input: &str) -> usize {
    if !input.chars().any(char::is_control) {
        return input.graphemes(true).map(UnicodeWidthStr::width).sum();
    }
    visible(input, false)
        .text
        .graphemes(true)
        .map(UnicodeWidthStr::width)
        .sum()
}

/// Clip at an extended grapheme boundary, never in half of a wide cell. Only
/// SGR and OSC 8 are retained; any retained style/link is explicitly closed.
pub fn clip(input: &str, width: usize) -> String {
    clipped(input, width).0
}

fn clipped(input: &str, width: usize) -> (String, usize) {
    if width == 0 {
        return (String::new(), 0);
    }
    let visible = visible(input, true);
    let mut end = 0;
    let mut cells = 0;
    for (offset, grapheme) in visible.text.grapheme_indices(true) {
        let next = UnicodeWidthStr::width(grapheme);
        if cells + next > width {
            break;
        }
        cells += next;
        end = offset + grapheme.len();
    }
    if end == 0 {
        return (String::new(), 0);
    }
    let mut output = String::with_capacity(input.len().min(end + visible.styles.len() * 16) + 16);
    let mut copied = 0;
    let mut sgr = false;
    let mut link = false;
    for (offset, style) in visible.styles {
        if offset > end || (offset == end && end < visible.text.len()) {
            break;
        }
        output.push_str(&visible.text[copied..offset]);
        output.push_str(style);
        copied = offset;
        if style.starts_with("\x1b[") {
            sgr = true;
        }
        if let Some(payload) = style.strip_prefix("\x1b]8;") {
            if let Some((_, target)) = payload.split_once(';') {
                link = !target
                    .trim_end_matches('\x07')
                    .trim_end_matches("\x1b\\")
                    .is_empty();
            }
        }
    }
    output.push_str(&visible.text[copied..end]);
    if link {
        output.push_str(LINK_END);
    }
    if sgr {
        output.push_str(RESET);
    }
    (output, cells)
}

fn paint(style: &str, text: &str) -> String {
    format!("{BG}{style}{text}{RESET}")
}

fn padded(text: &str, width: usize) -> String {
    let (mut line, cells) = clipped(text, width);
    line.push_str(&" ".repeat(width.saturating_sub(cells)));
    line
}

fn frame_row(text: &str, width: usize) -> String {
    // Background padding is emitted after any specimen's local reset.
    let (line, cells) = clipped(text, width);
    format!(
        "{BG}{WHITE}{line}{BG}{}{RESET}",
        " ".repeat(width.saturating_sub(cells))
    )
}

fn sidebar_width(columns: u16) -> u16 {
    if columns >= 100 { 28 } else { 24 }
}

/// One-based raw canvas shared with the terminal owner. Small screens keep the
/// matrix and textual guide instead of emitting unsafe out-of-bounds controls.
pub fn demo_canvas(report: &Report) -> Option<(u16, u16, u16, u16)> {
    if report.columns < 72 || report.rows < 18 {
        return None;
    }
    let columns = report.columns.min(500);
    let rows = report.rows.min(200);
    let column = sidebar_width(columns) + 3;
    Some((rows - 7, column, columns - column + 1, 4))
}

/// IL/DL and vertical scrolling affect an entire terminal row. Repair only the
/// outside-of-canvas sidebar, without erasing the raw visual result.
pub fn canvas_repair(report: &Report, frame: &[String]) -> Vec<u8> {
    let Some((row, column, _, height)) = demo_canvas(report) else {
        return Vec::new();
    };
    let mut output = Vec::new();
    for index in row..row + height {
        output.extend_from_slice(format!("\x1b[{index};1H").as_bytes());
        if let Some(line) = frame.get(usize::from(index - 1)) {
            output.extend_from_slice(clip(line, usize::from(column - 1)).as_bytes());
        }
    }
    output
}

fn status_style(status: Status) -> (&'static str, &'static str) {
    match status {
        Status::Verified => (CYAN, "●"),
        Status::Advertised => (WHITE, "≈"),
        Status::Visual => (CORAL, "◇"),
        Status::Unsupported => (MUTED, "×"),
        Status::Unknown => (MUTED, "?"),
        Status::Failed => (CORAL, "!"),
    }
}

fn category_status(report: &Report, category: &str) -> String {
    let mut ribbon = String::new();
    for capability in report
        .capabilities
        .iter()
        .filter(|capability| capability.category == category)
    {
        let (style, mark) = status_style(capability.status);
        ribbon.push_str(&paint(style, mark));
    }
    ribbon
}

// A compact category badge never promotes mixed or missing evidence to a
// support claim. Its number is the catalog size, not a verified count.
fn compact_status(report: &Report, category: &str) -> String {
    let mut capabilities = report
        .capabilities
        .iter()
        .filter(|capability| capability.category == category);
    let Some(first) = capabilities.next() else {
        return paint(MUTED, "?(0)");
    };
    let mut status = first.status;
    let mut total = 1;
    for capability in capabilities {
        total += 1;
        status = if status == Status::Failed || capability.status == Status::Failed {
            Status::Failed
        } else if status == capability.status {
            status
        } else {
            Status::Unknown
        };
    }
    let (style, mark) = status_style(status);
    paint(style, &format!("{mark}({total})"))
}

fn short_label(id: &str) -> &str {
    match id {
        "ansi16" => "16",
        "indexed256" => "256",
        "truecolor" => "RGB",
        "default_colors" => "def",
        "palette" => "pal",
        "attributes" => "SGR",
        "underlines" => "UL",
        "underline_color" => "ULc",
        "sgr_reset" => "rst",
        "box" => "box",
        "blocks" => "blk",
        "braille" => "brl",
        "powerline" => "pwl",
        "combining" => "cmb",
        "graphemes" => "grp",
        "emoji" => "emo",
        "variation" => "VS",
        "wide" => "CJK",
        "ambiguous" => "amb",
        "kitty_graphics" => "Kty",
        "sixel" => "six",
        "iterm_images" => "iTm",
        "overwrite" => "owr",
        "cursor" => "CUP",
        "cursor_visibility" => "vis",
        "save_restore" => "sav",
        "cursor_shape" => "cur",
        "erase" => "EL",
        "insert_delete" => "I/D",
        "scroll_region" => "scr",
        "origin" => "org",
        "autowrap" => "wrp",
        "reverse_index" => "RI",
        "alternate" => "alt",
        "sync" => "syn",
        "resize" => "siz",
        "keyboard" => "key",
        "kitty_keyboard" => "Kkb",
        "modify_other_keys" => "MOK",
        "focus" => "foc",
        "paste" => "pst",
        "mouse" => "SGR",
        "mouse_pixel" => "px",
        "da1" => "DA1",
        "da2" => "DA2",
        "xtgettcap" => "XTC",
        "version" => "VER",
        "decrqm" => "RQM",
        "hyperlinks" => "lnk",
        "clipboard" => "clp",
        "title" => "ttl",
        "notifications" => "ntf",
        "cwd" => "cwd",
        _ => id,
    }
}

fn sidebar(report: &Report, view: &View, height: usize) -> Vec<String> {
    let mut rows = Vec::with_capacity(height);
    let detailed = height >= 16;
    for (category, title) in CATEGORIES {
        let selected = view.demo.capability_ids().iter().any(|id| {
            report
                .get(id)
                .is_some_and(|capability| capability.category == category)
        });
        if detailed {
            rows.push(paint(
                if selected { CYAN } else { WHITE },
                &format!("{} {title}", if selected { "▸" } else { "·" }),
            ));
            if height >= 24 {
                let mut line = String::new();
                let mut count = 0;
                for cap in report
                    .capabilities
                    .iter()
                    .filter(|cap| cap.category == category)
                {
                    let (style, mark) = status_style(cap.status);
                    line.push_str(&paint(
                        style,
                        &format!("{mark}{:<3} ", short_label(&cap.id)),
                    ));
                    count += 1;
                    if count == 4 {
                        rows.push(format!("  {line}"));
                        line.clear();
                        count = 0;
                    }
                }
                if !line.is_empty() {
                    rows.push(format!("  {line}"));
                }
            } else {
                rows.push(format!("  {}", category_status(report, category)));
            }
        } else {
            rows.push(format!(
                "{} {}",
                clip(&compact_status(report, category), 1),
                paint(if selected { CYAN } else { WHITE }, title)
            ));
        }
    }
    if rows.len() < height {
        rows.push(String::new());
    }
    if rows.len() < height {
        rows.push(paint(MUTED, "REPLY ≠ RENDERING"));
    }
    if rows.len() < height {
        rows.push(paint(MUTED, "◇ needs a visual verdict"));
    }
    if rows.len() < height {
        rows.push(paint(MUTED, "? means not established"));
    }
    rows.resize(height, String::new());
    rows.truncate(height);
    rows
}

fn legends() -> [String; 2] {
    [
        format!(
            "{} Verified  {} Advertised  {} Visual",
            paint(CYAN, "●"),
            paint(WHITE, "≈"),
            paint(CORAL, "◇")
        ),
        format!(
            "{} Unsupported  {} Unknown  {} Failed",
            paint(MUTED, "×"),
            paint(MUTED, "?"),
            paint(CORAL, "!")
        ),
    ]
}

fn gradient(width: usize, tick: u64, band: usize) -> String {
    let mut line = String::with_capacity(width * 48);
    for x in 0..width {
        let phase = (x * 255 / width.max(1) + (tick % 256) as usize + band * 29) % 256;
        let top = [25 + phase / 3, 70 + phase / 2, 112 + (255 - phase) / 3];
        let bottom = [100 + phase / 2, 220 - phase / 3, 190 - phase / 4];
        line.push_str(&format!(
            "\x1b[48;2;{};{};{}m\x1b[38;2;{};{};{}m▄",
            top[0], top[1], top[2], bottom[0], bottom[1], bottom[2]
        ));
    }
    line.push_str(RESET);
    line
}

fn typography_preview() -> String {
    format!(
        "{}  {}  {}  {}",
        paint("\x1b[1m", "Bold"),
        paint("\x1b[3m", "Italic"),
        paint("\x1b[4:3m", "Curly"),
        paint(CORAL, "Aa 09")
    )
}

fn overview(width: usize, tick: u64, compact: bool) -> Vec<String> {
    if compact {
        return vec![
            gradient(width, tick, 0),
            paint(CYAN, "┌─┐ ▀▄▚ ⠿ é 界 👩‍💻 →"),
            typography_preview(),
        ];
    }
    vec![
        paint(&format!("{CYAN}\x1b[1m"), "A terminal is a canvas."),
        paint(MUTED, "Color. Geometry. Type. A living specimen."),
        String::new(),
        gradient(width, tick, 0),
        gradient(width, tick, 1),
        paint(MUTED, "TRUECOLOR   foreground × background"),
        String::new(),
        paint(CYAN, "┌─────────┬─────────┐  ╭──────────╮"),
        paint(CYAN, "│ é → 界  │ ░▒▓█ ▚  │  │ 👩‍💻  🇫🇷   │"),
        paint(CYAN, "└─────────┴─────────┘  ╰──────────╯"),
        paint(MUTED, "GRAPHEMES   combining · wide · joined"),
        String::new(),
        typography_preview(),
        format!(
            "{}  {}",
            paint("\x1b[9m", "Strikethrough"),
            paint("\x1b[53m", "Overline")
        ),
        paint(MUTED, "TYPOGRAPHY   independently styled runs"),
        String::new(),
        paint(CORAL, "⣀⣠⣤⣴⣶⣿⣷⣶⣦⣤⣀⡀⢀⣀⣤⣴⣶⣦⣤⣀"),
        paint(MUTED, "Evidence lives at left. Rendering is visual."),
    ]
}

fn colors(width: usize, tick: u64) -> Vec<String> {
    let mut rows = vec![paint(CYAN, "ANSI 16 / foreground and background")];
    for bright in [false, true] {
        let mut foreground = String::new();
        let mut background = String::new();
        for index in 0..8 {
            let fg = if bright { 90 + index } else { 30 + index };
            let bg = if bright { 100 + index } else { 40 + index };
            foreground.push_str(&format!(
                "{BG}\x1b[{fg}m {:02} {RESET}",
                index + if bright { 8 } else { 0 }
            ));
            background.push_str(&format!(
                "\x1b[{bg}m\x1b[{}m {:02} {RESET}",
                if index == 0 || index == 4 { 97 } else { 30 },
                index + if bright { 8 } else { 0 }
            ));
        }
        rows.push(foreground);
        rows.push(background);
    }
    rows.push(paint(CYAN, "256 / six RGB cube slices, both planes"));
    let mut base = String::new();
    for index in 0..16 {
        base.push_str(&format!("\x1b[48;5;{index}m\x1b[38;5;{}m▄", 15 - index));
    }
    base.push_str(RESET);
    rows.push(base);
    for red in 0..6 {
        let mut row = String::new();
        for green in 0..6 {
            for blue in 0..6 {
                let index = 16 + red * 36 + green * 6 + blue;
                row.push_str(&format!(
                    "\x1b[48;5;{index}m\x1b[38;5;{}m▄",
                    231 - (index - 16)
                ));
            }
            row.push_str(RESET);
            if green != 5 {
                row.push(' ');
            }
        }
        rows.push(row);
    }
    let mut gray = String::new();
    for index in 232..=255 {
        gray.push_str(&format!("\x1b[48;5;{index}m\x1b[38;5;{}m▄", 487 - index));
    }
    gray.push_str(RESET);
    rows.push(gray);
    rows.push(paint(CYAN, "24-BIT / continuous light, no palette steps"));
    rows.push(gradient(width, tick, 0));
    rows.push(format!("\x1b[39;49mDefault foreground / background{RESET}"));
    rows.push(paint(
        MUTED,
        "Palette replies advertise values, not fidelity.",
    ));
    rows
}

fn geometry() -> Vec<String> {
    let cases = [
        ("Box", "┌─┬─┐╭─╮╞═╡"),
        ("Blocks", "░▒▓█▀▄▌▐▚▞"),
        ("Braille", "⠁⠃⠇⠏⠟⠿⣿"),
        ("Powerline", "\u{e0b0}\u{e0b1}\u{e0b2}\u{e0b3}"),
        ("Combining", "e\u{301} n\u{303} a\u{308}"),
        ("Graphemes", "👩‍💻 👍🏽 👨‍👩‍👧"),
        ("Emoji", "🚀 🌊 🇫🇷"),
        ("Variation", "♥︎ ♥️ ✈︎ ✈️"),
        ("Wide / CJK", "界 日本 語"),
        ("Ambiguous", "· Ω α → ≠"),
    ];
    let mut rows = vec![paint(CYAN, "CELL GRID  │ fixed rails; glyphs stay inside")];
    for (label, sample) in cases {
        let cells = cell_width(sample);
        let clusters = sample.graphemes(true).count();
        rows.push(format!(
            "{} {}{}{} {}",
            paint(MUTED, &format!("{label:<11}")),
            paint(CYAN, "│"),
            padded(sample, 16),
            paint(CYAN, "│"),
            paint(MUTED, &format!("{cells:>2}c {clusters:>2}g"))
        ));
    }
    rows.push(paint(MUTED, "c = expected cells; g = grapheme clusters"));
    rows.push(paint(MUTED, "Font shaping ≠ segmentation ≠ cursor advance"));
    rows.push(paint(
        WHITE,
        "ffi → 3 cells / 3 clusters; ligature is font-only",
    ));
    rows.push(paint(WHITE, "é → 1 cell / 1 cluster / 2 Unicode scalars"));
    rows
}

fn typography(screenshot: bool) -> Vec<String> {
    let sample = |style: &str, label: &str| paint(style, label);
    vec![
        paint(CYAN, "ATTRIBUTES / real SGR specimen"),
        format!(
            "{}  {}  {}",
            sample("\x1b[1m", "Bold 01"),
            sample("\x1b[2m", "Dim 02"),
            sample("\x1b[3m", "Italic 03")
        ),
        format!(
            "{}  {}  {}",
            sample("\x1b[7m", "Reverse 07"),
            sample("\x1b[9m", "Strike 09"),
            sample("\x1b[53m", "Overline 53")
        ),
        if screenshot {
            paint(MUTED, "Blink 05 / 06 · live specimen only")
        } else {
            format!(
                "{}  {}",
                sample("\x1b[5m", "Slow blink 05"),
                sample("\x1b[6m", "Rapid blink 06")
            )
        },
        "Conceal 08: [\x1b[8mhidden\x1b[28m]  ← brackets remain".to_string(),
        String::new(),
        paint(CYAN, "UNDERLINES / style and independent color"),
        format!(
            "{}  {}",
            sample("\x1b[4:1m", "Single ─────"),
            sample("\x1b[4:2m", "Double ═════")
        ),
        format!(
            "{}  {}",
            sample("\x1b[4:3m", "Curly ∿∿∿∿∿"),
            sample("\x1b[4:4m", "Dotted ·····")
        ),
        sample("\x1b[4:5m", "Dashed ┄┄┄┄┄  /  4:5"),
        format!(
            "{}  {}",
            sample("\x1b[4:0m", "Off / 4:0"),
            sample("\x1b[21m", "Legacy double / 21")
        ),
        sample(
            "\x1b[4:3;58:2::251:146:128m",
            "Coral underline / white foreground",
        ),
        sample("\x1b[4;58;5;45m", "Indexed cyan underline / 58;5;45"),
        format!("\x1b[4;58;2;251;146;128mColor\x1b[59m default\x1b[24m no underline{RESET}"),
        String::new(),
        paint(CYAN, "SELECTIVE RESET / each run loses one layer"),
        format!(
            "{BG}{CYAN}\x1b[1;3;4mAll\x1b[22m no bold\x1b[23m no italic\x1b[24m no line{RESET}"
        ),
        format!("{BG}{CORAL}\x1b[1;7;9;53mAll\x1b[27;29;55m plain\x1b[0m full reset"),
        if screenshot {
            paint(MUTED, "Selective blink reset 25 / conceal reset 28")
        } else {
            format!("\x1b[5mBlink\x1b[25m steady  \x1b[8mhidden\x1b[28m visible{RESET}")
        },
        paint(
            MUTED,
            if screenshot {
                "Unsupported SGR may look plain; inspect each run."
            } else {
                "Unsupported SGR can look plain: mark P or F."
            },
        ),
    ]
}

fn braille_art(width: usize, tick: u64) -> Vec<String> {
    let width = width.min(64);
    let height = 8usize;
    let mut pixels = vec![false; width * 2 * height];
    for x in 0..width * 2 {
        let phase = x as f64 * 0.12 + (tick % 360) as f64 * 0.04;
        let y = ((phase.sin() + 1.0) * 3.1) as usize;
        pixels[y.min(height - 1) * width * 2 + x] = true;
        let second = height - 1 - y.min(height - 1);
        pixels[second * width * 2 + x] = true;
    }
    let dots = [[1u32, 8], [2, 16], [4, 32], [64, 128]];
    let mut rows = Vec::with_capacity(2);
    for row in 0..2 {
        let mut line = String::with_capacity(width * 3);
        for column in 0..width {
            let mut bits = 0;
            for (dy, pair) in dots.iter().enumerate() {
                for (dx, bit) in pair.iter().enumerate() {
                    if pixels[(row * 4 + dy) * width * 2 + column * 2 + dx] {
                        bits |= bit;
                    }
                }
            }
            line.push(char::from_u32(0x2800 + bits).expect("braille scalar"));
        }
        rows.push(paint(CYAN, &line));
    }
    rows
}

fn graphics(width: usize, tick: u64, screenshot: bool) -> Vec<String> {
    let mut rows = vec![
        paint(CYAN, "BRAILLE / procedural interference study"),
        String::new(),
    ];
    rows.extend(braille_art(width, tick));
    rows.push(String::new());
    rows.push(paint(WHITE, "⣿⣷⣶⣦⣤⣀  ░▒▓█  ⣀⣤⣦⣶⣷⣿"));
    rows.push(paint(
        MUTED,
        "Text artwork is the always-available fallback.",
    ));
    rows.push(paint(
        CYAN,
        "IMAGE PROTOCOLS / independent endpoint evidence",
    ));
    rows.push(paint(
        WHITE,
        "Kitty    placement · replacement · owned deletion",
    ));
    rows.push(paint(WHITE, "Sixel    indexed pixels · clipping · scroll"));
    rows.push(paint(WHITE, "iTerm2   inline image · resize · placement"));
    if !screenshot {
        rows.push(paint(
            CORAL,
            "G cycles images; unknown protocols require opt-in.",
        ));
    }
    rows.push(paint(
        MUTED,
        "An acknowledgment does not verify visible pixels.",
    ));
    rows
}

fn controls(screenshot: bool) -> Vec<String> {
    let mut rows = vec![
        paint(CYAN, "RAW CANVAS / follow the visible landmarks"),
        paint(WHITE, "CUP/CUF/CUB  + and < follow cursor movement"),
        paint(WHITE, "ICH/DCH      IN replaces two shifted letters"),
        paint(WHITE, "IL/DL        inserted and deleted line labels"),
        paint(WHITE, "Scroll       retained text stays in the region"),
        paint(WHITE, "Reverse index moves text downward at top"),
        paint(WHITE, "EL           XXXXX vanishes; clean remains"),
        paint(WHITE, "DECOM        origin relative lands in region"),
        paint(WHITE, "Cursor shape only shown when prior shape known"),
        paint(
            WHITE,
            if screenshot {
                "CPR suite    save/restore · wrap · cursor replies"
            } else {
                "R suite      save/restore · wrap · cursor replies"
            },
        ),
        paint(WHITE, "Exit/resize  shell restored; viewport bounded"),
    ];
    if !screenshot {
        rows.push(paint(
            CORAL,
            "R cursor suite · P/F visual verdict · E export",
        ));
    }
    rows.push(paint(
        MUTED,
        "Raw controls and objective DSR checks are separate.",
    ));
    rows
}

fn input(view: &View) -> Vec<String> {
    let mut rows = vec![
        paint(CYAN, "LIVE INPUT / decoded events, not support guesses"),
        paint(WHITE, "Keyboard   ← ↑ ↓ →  Shift · Alt · Ctrl"),
        paint(
            WHITE,
            if view.mouse_pixel_requested {
                "Mouse      pixels requested · units need visual inspection"
            } else {
                "Mouse      cells requested · button · modifiers"
            },
        ),
        paint(WHITE, "Focus      gained / lost   Paste  begin / end"),
        paint(MUTED, "Kitty key kind distinguishes press/repeat/release."),
        paint(MUTED, "modifyOtherKeys preserves modified ordinary keys."),
        String::new(),
        paint(CYAN, "EVENT TRANSCRIPT"),
    ];
    if view.screenshot {
        rows.push(paint(
            MUTED,
            "A deterministic specimen; live events are hidden.",
        ));
    } else if view.input_log.is_empty() {
        rows.push(paint(
            MUTED,
            "Waiting for a key, mouse, focus, or paste event…",
        ));
    } else {
        let start = view.input_log.len().saturating_sub(8);
        for (index, event) in view.input_log[start..].iter().enumerate() {
            rows.push(format!(
                "{} {}",
                paint(MUTED, &format!("{:02}", start + index + 1)),
                clip(event, 512)
            ));
        }
    }
    rows.push(String::new());
    rows.push(paint(
        MUTED,
        "Protocol replies never masquerade as input events.",
    ));
    rows
}

fn integration(screenshot: bool) -> Vec<String> {
    let mut rows = vec![
        paint(CYAN, "HYPERLINK / OSC 8 with a real target"),
        format!(
            "{BG}{CYAN}\x1b[4m\x1b]8;;https://terminalguide.namepad.de/\x1b\\Open the terminal protocol field guide{LINK_END}{RESET}"
        ),
        paint(MUTED, "Hover or activate: target must be the guide URL."),
        String::new(),
        paint(CYAN, "INTEGRATION / explicit, consent-bound actions"),
    ];
    if screenshot {
        rows.extend([
            paint(
                WHITE,
                "Clipboard · notification · title · working directory",
            ),
            paint(MUTED, "No side effects are emitted by this presentation."),
        ]);
    } else {
        rows.extend([
            paint(WHITE, "C  clipboard     copy a known short specimen"),
            paint(WHITE, "N  notification  show one terminal notification"),
            paint(WHITE, "T  title         set a temporary specimen title"),
            paint(WHITE, "W  directory     publish this working directory"),
            paint(CORAL, "Nothing above runs until its explicit key action."),
        ]);
    }
    rows.push(String::new());
    rows.push(paint(
        CYAN,
        "IDENTITY / evidence rather than environment lore",
    ));
    rows.push(paint(
        WHITE,
        "DA1 · DA2 · version · XTGETTCAP · mode reports",
    ));
    rows.push(paint(
        MUTED,
        "No reply = unknown. An advertisement is not proof.",
    ));
    rows
}

fn details(report: &Report, view: &View, width: usize) -> Vec<String> {
    let mut rows = vec![paint(
        CYAN,
        "CAPABILITY EVIDENCE / independent visual verdict",
    )];
    for id in view.demo.capability_ids() {
        let Some(capability) = report.get(id) else {
            continue;
        };
        let (style, mark) = status_style(capability.status);
        let visual = match capability.visual {
            Some(true) => "visual PASS",
            Some(false) => "visual FAIL",
            None => "visual unmarked",
        };
        rows.push(format!(
            "{} {} · {visual}",
            paint(style, mark),
            paint(WHITE, &capability.name)
        ));
        if capability.evidence.is_empty() {
            rows.push(paint(MUTED, "  No correlated endpoint evidence."));
        }
        for evidence in capability.evidence.iter().take(2) {
            rows.push(paint(
                MUTED,
                &format!("  {}", clip(evidence, width.saturating_sub(2))),
            ));
        }
    }
    rows
}

fn demo_rows(report: &Report, view: &View, width: usize, compact: bool) -> Vec<String> {
    let tick = if view.screenshot { 42 } else { view.tick };
    if view.expanded && !view.screenshot {
        return details(report, view, width);
    }
    let mut rows = match view.demo {
        Demo::Overview => overview(width, tick, compact),
        Demo::Colors => colors(width, tick),
        Demo::Unicode => geometry(),
        Demo::Typography => typography(view.screenshot),
        Demo::Graphics => graphics(width, tick, view.screenshot),
        Demo::Control => controls(view.screenshot),
        Demo::Input => input(view),
        Demo::Integration => integration(view.screenshot),
    };
    if view.diagnostics && !view.screenshot {
        rows.push(paint(
            CORAL,
            "DIAGNOSTIC / compare cells, not UTF-8 byte length",
        ));
        rows.push(paint(
            MUTED,
            &format!(
                "viewport {}×{} · frame {} · {} capabilities",
                report.columns,
                report.rows,
                tick,
                report.capabilities.len()
            ),
        ));
        rows.push(paint(
            MUTED,
            "Font shaping and presentation need a human verdict.",
        ));
    }
    rows
}

fn page_rows(source: &[String], capacity: usize, view: &View) -> Vec<String> {
    if capacity == 0 {
        return Vec::new();
    }
    if source.len() <= capacity {
        return source.to_vec();
    }
    // Hold the page while paused. The owner freezes tick, so resize cannot
    // introduce an independent clock or nondeterministic animation phase.
    let usable = if capacity > 3 && !view.screenshot {
        capacity - 1
    } else {
        capacity
    };
    let pages = source.len().div_ceil(usable);
    let page = if view.screenshot {
        0
    } else {
        (view.tick / 180) as usize % pages
    };
    let start = page * usable;
    let mut rows = source[start..(start + usable).min(source.len())].to_vec();
    if usable < capacity {
        rows.resize(usable, String::new());
        rows.push(paint(
            MUTED,
            &format!("SPECIMEN {:02}/{pages:02} · [/] pages", page + 1),
        ));
    }
    rows
}

fn footer(view: &View, wide: bool) -> String {
    if view.screenshot {
        return paint(MUTED, "VISUAL REFERENCE · FIXED FRAME 42");
    }
    let action = match view.demo {
        Demo::Graphics => "G images",
        Demo::Control => "R cursor suite",
        Demo::Integration => "C/N/T/W actions",
        _ => {
            if wide {
                "Tab details d diag s shot"
            } else {
                "Tab/d/s"
            }
        }
    };
    if wide {
        paint(
            MUTED,
            &format!("←/→ Space P/F verdict E export [/] page {action} q"),
        )
    } else {
        paint(MUTED, &format!("P/F E [/] {action} q"))
    }
}

/// Render at most 500×200 independently replaceable rows/cells. Physical report
/// geometry remains intact for objective cursor tests and viewport metadata.
/// Wide layouts keep a seven-category sidebar; compact layouts keep a
/// four-row matrix below the visual specimen, never a hidden category tab.
pub fn render(report: &Report, view: &View) -> Vec<String> {
    let width = usize::from(report.columns.min(500));
    let height = usize::from(report.rows.min(200));
    if width == 0 {
        return vec![String::new(); height];
    }
    if height == 0 {
        return Vec::new();
    }
    let wide = report.columns >= 72 && report.rows >= 18;
    let mut rows = Vec::with_capacity(height);
    let heading = if width >= 60 {
        let state = if view.screenshot {
            "REFERENCE"
        } else if view.paused {
            "PAUSED"
        } else {
            "LIVE"
        };
        let right = format!("{state} / {}", view.demo.title());
        let left = paint(&format!("{CYAN}\x1b[1m"), "TERMINAL / SHOWCASE");
        format!(
            "{}{}",
            padded(&left, width.saturating_sub(cell_width(&right) + 1)),
            paint(MUTED, &right)
        )
    } else {
        paint(&format!("{CYAN}\x1b[1m"), "TERMINAL / SHOWCASE")
    };
    rows.push(heading);
    let geometry = format!("{}×{}", report.columns, report.rows);
    let identity = format!(
        "{} {}",
        paint(MUTED, "ENDPOINT"),
        paint(WHITE, &report.identity)
    );
    rows.push(if width >= 40 {
        format!(
            "{} {}",
            padded(&identity, width.saturating_sub(cell_width(&geometry) + 1)),
            paint(MUTED, &geometry)
        )
    } else {
        identity
    });
    if wide {
        rows.push(paint(MUTED, &"─".repeat(width)));
        let body_height = height - 6;
        let side_width = usize::from(sidebar_width(report.columns));
        let content_width = width - side_width - 2;
        let side = sidebar(report, view, body_height);
        let has_canvas = matches!(view.demo, Demo::Graphics | Demo::Control | Demo::Unicode);
        let canvas = if has_canvas {
            demo_canvas(report)
        } else {
            None
        };
        let capacity = canvas.map_or(body_height, |(row, _, _, _)| {
            usize::from(row).saturating_sub(5)
        });
        let main = page_rows(
            &demo_rows(report, view, content_width, false),
            capacity,
            view,
        );
        for (index, sidebar_row) in side.iter().enumerate() {
            let physical_row = index + 4;
            let content = if let Some((row, _, _, canvas_height)) = canvas {
                let begin = usize::from(row);
                if physical_row == begin - 1 {
                    paint(
                        MUTED,
                        match view.demo {
                            Demo::Unicode => "Overwrite: [ X] wide · [Y] combining",
                            Demo::Control => match if view.screenshot {
                                3
                            } else {
                                (view.tick / 8) % 6
                            } {
                                0 => "CUP → CUF → CUB / + then <",
                                1 => "ICH / DCH / two shifted letters replaced",
                                2 => "IL / DL / inserted then deleted line",
                                3 => "Scroll region ↑ / reverse index ↓",
                                4 => "EL / XXXXX erased, clean retained",
                                _ => "DECOM / origin relative inside region",
                            },
                            _ => "IMAGE CANVAS / text artwork stays above",
                        },
                    )
                } else if (begin..begin + usize::from(canvas_height)).contains(&physical_row) {
                    " ".repeat(content_width)
                } else if physical_row == begin + usize::from(canvas_height) {
                    paint(MUTED, "Objective replies ≠ visual correctness")
                } else {
                    main.get(index).cloned().unwrap_or_default()
                }
            } else {
                main.get(index).cloned().unwrap_or_default()
            };
            rows.push(format!(
                "{}  {}",
                padded(sidebar_row, side_width),
                clip(&content, content_width)
            ));
        }
    } else {
        let body_height = height.saturating_sub(9);
        let main = page_rows(&demo_rows(report, view, width, true), body_height, view);
        for index in 0..body_height {
            rows.push(main.get(index).cloned().unwrap_or_default());
        }
        let badges: Vec<String> = CATEGORIES
            .iter()
            .map(|(category, _)| compact_status(report, category))
            .collect();
        rows.extend([
            format!(
                "{} {} · {} {}",
                paint(CYAN, "Color"),
                badges[0],
                paint(CYAN, "Typography"),
                badges[1]
            ),
            format!(
                "{} {} · {} {}",
                paint(CYAN, "Graphics"),
                badges[2],
                paint(CYAN, "Cursor/screen"),
                badges[3]
            ),
            format!(
                "{} {} · {} {}",
                paint(CYAN, "Keyboard"),
                badges[4],
                paint(CYAN, "Mouse"),
                badges[5]
            ),
            format!(
                "{} {} · {}",
                paint(CYAN, "Integration"),
                badges[6],
                paint(MUTED, "reply ≠ rendering")
            ),
        ]);
    }
    rows.extend(legends());
    rows.push(footer(view, wide));
    rows.resize(height, String::new());
    rows.truncate(height);
    rows.into_iter().map(|row| frame_row(&row, width)).collect()
}

/// Absolute CUP + erase-line for changed rows only, including stale old rows.
/// Synchronized output brackets a nonempty update, never an idle frame.
pub fn diff(previous: &[String], next: &[String], sync: bool) -> Vec<u8> {
    let mut output = Vec::new();
    for index in 0..previous.len().max(next.len()) {
        if previous.get(index) == next.get(index) {
            continue;
        }
        if output.is_empty() && sync {
            output.extend_from_slice(b"\x1b[?2026h");
        }
        output.extend_from_slice(format!("\x1b[{};1H\x1b[2K", index + 1).as_bytes());
        if let Some(row) = next.get(index) {
            output.extend_from_slice(row.as_bytes());
        }
    }
    if sync && !output.is_empty() {
        output.extend_from_slice(b"\x1b[?2026l");
    }
    output
}

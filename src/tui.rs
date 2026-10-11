use anyhow::{Context, Result};
use crossterm::{
    cursor,
    event::{
        self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    style::{self, Attribute},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    io::{self, Write},
    path::Path,
    time::{Duration, Instant},
};

mod view;

const HEADER: u16 = 3;
const FRAME: Duration = Duration::from_millis(34);

struct Restore;
impl Restore {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        let guard = Self;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste,
            EnableFocusChange,
            cursor::Hide
        )?;
        Ok(guard)
    }
}
impl Drop for Restore {
    fn drop(&mut self) {
        let _ = execute!(
            io::stdout(),
            style::ResetColor,
            style::SetAttribute(Attribute::Reset),
            cursor::Show,
            DisableMouseCapture,
            DisableBracketedPaste,
            DisableFocusChange,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

fn call(socket: &Path, mut request: Value) -> Result<Value> {
    request["version"] = json!(1);
    crate::client::request(socket, request)
}
fn send_input(socket: &Path, terminal: &str, bytes: &[u8], status: &mut String) -> Result<()> {
    if bytes.len() > crate::runtime::MAX_INPUT {
        *status = "invalid-input: input exceeds 64 KiB".into();
        return Ok(());
    }
    input_result(
        call(
            socket,
            json!({"op":"input","terminal":terminal,"bytes":bytes}),
        ),
        status,
    )
}
fn input_result(result: Result<Value>, status: &mut String) -> Result<()> {
    match result {
        Ok(_) => Ok(()),
        Err(error) => {
            let message = error.to_string();
            match message.split_once(':').map(|(code, _)| code) {
                Some("exited" | "backpressure" | "invalid-input" | "not-found") => {
                    *status = message;
                    Ok(())
                }
                _ => Err(error),
            }
        }
    }
}
fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}
fn mode(screen: &Value, name: &str) -> bool {
    screen["modes"][name].as_bool().unwrap_or(false)
}

/// Encode actual terminal input; the picker is the only command interception layer.
fn key_bytes(key: KeyEvent, application_cursor: bool) -> Vec<u8> {
    let modifiers = key.modifiers;
    let modifier = 1
        + u8::from(modifiers.contains(KeyModifiers::SHIFT))
        + 2 * u8::from(modifiers.contains(KeyModifiers::ALT))
        + 4 * u8::from(modifiers.contains(KeyModifiers::CONTROL));
    let sequence = match key.code {
        KeyCode::Char(c) => {
            let c = if modifiers.contains(KeyModifiers::CONTROL) {
                match c {
                    'a'..='z' => ((c as u8 - b'a') + 1) as char,
                    'A'..='Z' => ((c as u8 - b'A') + 1) as char,
                    '@' | ' ' | '2' => '\0',
                    '[' | '3' => '\x1b',
                    '\\' | '4' => '\x1c',
                    ']' | '5' => '\x1d',
                    '^' | '6' => '\x1e',
                    '_' | '7' | '/' => '\x1f',
                    '?' | '8' => '\x7f',
                    _ => c,
                }
            } else {
                c
            };
            c.to_string()
        }
        KeyCode::Enter => "\r".into(),
        KeyCode::Tab => "\t".into(),
        KeyCode::BackTab => "\x1b[Z".into(),
        KeyCode::Backspace => "\x7f".into(),
        KeyCode::Esc => "\x1b".into(),
        KeyCode::Up
        | KeyCode::Down
        | KeyCode::Right
        | KeyCode::Left
        | KeyCode::Home
        | KeyCode::End => {
            let final_byte = match key.code {
                KeyCode::Up => 'A',
                KeyCode::Down => 'B',
                KeyCode::Right => 'C',
                KeyCode::Left => 'D',
                KeyCode::Home => 'H',
                _ => 'F',
            };
            if modifier > 1 {
                format!("\x1b[1;{modifier}{final_byte}")
            } else if application_cursor {
                format!("\x1bO{final_byte}")
            } else {
                format!("\x1b[{final_byte}")
            }
        }
        KeyCode::Insert | KeyCode::Delete | KeyCode::PageUp | KeyCode::PageDown => {
            let n = match key.code {
                KeyCode::Insert => 2,
                KeyCode::Delete => 3,
                KeyCode::PageUp => 5,
                _ => 6,
            };
            if modifier > 1 {
                format!("\x1b[{n};{modifier}~")
            } else {
                format!("\x1b[{n}~")
            }
        }
        KeyCode::F(n) => {
            if (1..=4).contains(&n) {
                let end = (b'P' + n - 1) as char;
                if modifier > 1 {
                    format!("\x1b[1;{modifier}{end}")
                } else {
                    format!("\x1bO{end}")
                }
            } else {
                let number = match n {
                    5 => 15,
                    6 => 17,
                    7 => 18,
                    8 => 19,
                    9 => 20,
                    10 => 21,
                    11 => 23,
                    12 => 24,
                    13 => 25,
                    14 => 26,
                    15 => 28,
                    16 => 29,
                    17 => 31,
                    18 => 32,
                    19 => 33,
                    20 => 34,
                    _ => return Vec::new(),
                };
                if modifier > 1 {
                    format!("\x1b[{number};{modifier}~")
                } else {
                    format!("\x1b[{number}~")
                }
            }
        }
        _ => return Vec::new(),
    };
    let mut bytes = sequence.into_bytes();
    if modifiers.contains(KeyModifiers::ALT)
        && matches!(
            key.code,
            KeyCode::Char(_) | KeyCode::Enter | KeyCode::Backspace | KeyCode::Tab
        )
    {
        bytes.insert(0, 27);
    }
    bytes
}

fn prefix_key(config: &str) -> KeyEvent {
    let lower = config.to_lowercase();
    let character = lower
        .strip_prefix("ctrl-")
        .or_else(|| lower.strip_prefix("c-"))
        .or_else(|| lower.strip_prefix("control-"))
        .and_then(|s| s.chars().next())
        .unwrap_or('b');
    KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL)
}
fn is_prefix(key: KeyEvent, prefix: KeyEvent) -> bool {
    key.code == prefix.code && key.modifiers == prefix.modifiers
}

fn fuzzy(needle: &str, haystack: &str) -> bool {
    let haystack = haystack.to_lowercase();
    let mut remaining = haystack.chars();
    needle
        .to_lowercase()
        .chars()
        .all(|wanted| remaining.any(|c| c == wanted))
}

#[derive(Clone, Debug)]
struct Row {
    id: String,
    label: String,
    depth: usize,
}
fn tree_rows(snapshot: &Value, query: &str) -> Vec<Row> {
    let terminals = snapshot["terminals"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let projects = snapshot["projects"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    fn visit(
        node: &Value,
        depth: usize,
        terminals: &[Value],
        projects: &[Value],
        query: &str,
        seen: &mut HashSet<String>,
        rows: &mut Vec<Row>,
    ) {
        let id = text(node, "id");
        if !seen.insert(id.to_owned()) {
            return;
        }
        let project = projects
            .iter()
            .find(|p| text(p, "id") == text(node, "project_id"));
        let project_name = project.map(|p| text(p, "name")).unwrap_or("");
        let cwd = node["workspace"]["cwd"].as_str().unwrap_or("");
        let git = &node["workspace"]["git"];
        let branch = git["branch"]
            .as_str()
            .or_else(|| git["detached"].as_str())
            .or_else(|| git["commit"].as_str())
            .unwrap_or("");
        let activity = node["integration"]["agent"]["activity"]
            .as_str()
            .unwrap_or("");
        let divergence = if git["ahead"].is_number() {
            format!(" ↑{} ↓{}", git["ahead"], git["behind"])
        } else {
            String::new()
        };
        let changes = format!(
            " staged:{} unstaged:{} untracked:{}",
            git["staged"]["count"].as_u64().unwrap_or(0),
            git["unstaged"]["count"].as_u64().unwrap_or(0),
            git["untracked"]["count"].as_u64().unwrap_or(0)
        );
        let git_label = if branch.is_empty() {
            String::new()
        } else {
            format!(
                "{branch}{}{}{}",
                if git["isolated"] == true {
                    " [worktree]"
                } else {
                    ""
                },
                divergence,
                changes
            )
        };
        let exit = if node["alive"] == false {
            node["exit_status"]
                .as_u64()
                .map(|code| format!(" · exit {code}"))
                .unwrap_or_else(|| " · exiting".into())
        } else {
            String::new()
        };
        let label = format!(
            "{}  [{}]  {}{}  {}\n{}",
            text(node, "name"),
            project_name,
            activity,
            exit,
            git_label,
            cwd,
        );
        if fuzzy(query, &label) {
            rows.push(Row {
                id: id.into(),
                label,
                depth,
            });
        }
        for child in terminals.iter().filter(|t| text(t, "parent_id") == id) {
            visit(child, depth + 1, terminals, projects, query, seen, rows);
        }
    }
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    for node in terminals.iter().filter(|t| {
        t["parent_id"].is_null()
            || !terminals
                .iter()
                .any(|p| text(p, "id") == text(t, "parent_id"))
    }) {
        visit(node, 0, terminals, projects, query, &mut seen, &mut rows);
    }
    for node in terminals {
        visit(node, 0, terminals, projects, query, &mut seen, &mut rows);
    }
    rows
}

fn clip_label(label: &str, cols: u16) -> String {
    use unicode_width::UnicodeWidthChar;
    let mut width = 0usize;
    label
        .chars()
        .filter(|c| !c.is_control())
        .take_while(|c| {
            width += c.width().unwrap_or(0);
            width <= cols as usize
        })
        .collect()
}
type Selection = ((u16, u16), (u16, u16));
fn selection_contains(selection: Option<Selection>, point: (u16, u16)) -> bool {
    selection.is_some_and(|(a, b)| {
        let a = (a.1, a.0);
        let b = (b.1, b.0);
        let point = (point.1, point.0);
        point >= a.min(b) && point <= a.max(b)
    })
}

fn mouse_bytes(mouse: MouseEvent, screen: &Value) -> Vec<u8> {
    let motion = matches!(mouse.kind, MouseEventKind::Moved);
    let drag = matches!(mouse.kind, MouseEventKind::Drag(_));
    if motion && !mode(screen, "mouse_motion")
        || drag && !(mode(screen, "mouse_drag") || mode(screen, "mouse_motion"))
    {
        return Vec::new();
    }
    let button_code = |button| match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    let mut button = match mouse.kind {
        MouseEventKind::Down(b) | MouseEventKind::Up(b) => button_code(b),
        MouseEventKind::Drag(b) => button_code(b) + 32,
        MouseEventKind::Moved => 35,
        MouseEventKind::ScrollUp => 64,
        MouseEventKind::ScrollDown => 65,
        MouseEventKind::ScrollLeft => 66,
        MouseEventKind::ScrollRight => 67,
    };
    if mouse.modifiers.contains(KeyModifiers::SHIFT) {
        button += 4;
    }
    if mouse.modifiers.contains(KeyModifiers::ALT) {
        button += 8;
    }
    if mouse.modifiers.contains(KeyModifiers::CONTROL) {
        button += 16;
    }
    let x = u32::from(mouse.column) + 1;
    let y = u32::from(mouse.row.saturating_sub(HEADER)) + 1;
    if mode(screen, "mouse_sgr") {
        format!(
            "\x1b[<{button};{x};{y}{}",
            if matches!(mouse.kind, MouseEventKind::Up(_)) {
                'm'
            } else {
                'M'
            }
        )
        .into_bytes()
    } else if mode(screen, "mouse_utf8") && x <= 2015 && y <= 2015 {
        let code = if matches!(mouse.kind, MouseEventKind::Up(_)) {
            35 + (button & 28)
        } else {
            button + 32
        };
        let mut bytes = Vec::with_capacity(9);
        bytes.extend_from_slice(b"\x1b[M");
        let mut buffer = [0; 4];
        for value in [u32::from(code), x + 32, y + 32] {
            bytes.extend_from_slice(
                char::from_u32(value)
                    .expect("bounded UTF8 mouse coordinate")
                    .encode_utf8(&mut buffer)
                    .as_bytes(),
            );
        }
        bytes
    } else if !mode(screen, "mouse_utf8") && x <= 223 && y <= 223 {
        vec![
            27,
            b'[',
            b'M',
            if matches!(mouse.kind, MouseEventKind::Up(_)) {
                35 + (button & 28)
            } else {
                button + 32
            },
            x as u8 + 32,
            y as u8 + 32,
        ]
    } else {
        Vec::new()
    }
}
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in bytes.chunks(3) {
        let n = ((chunk[0] as u32) << 16)
            | ((chunk.get(1).copied().unwrap_or(0) as u32) << 8)
            | chunk.get(2).copied().unwrap_or(0) as u32;
        result.push(TABLE[((n >> 18) & 63) as usize] as char);
        result.push(TABLE[((n >> 12) & 63) as usize] as char);
        result.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        result.push(if chunk.len() > 2 {
            TABLE[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    result
}
fn selected_text(screen: &Value, start: (u16, u16), end: (u16, u16)) -> String {
    let (start, end) = if (start.1, start.0) <= (end.1, end.0) {
        (start, end)
    } else {
        (end, start)
    };
    let mut result = String::new();
    if let Some(lines) = screen["cells"].as_array() {
        for row in start.1..=end.1 {
            if let Some(cells) = lines.get(row as usize).and_then(Value::as_array) {
                let mut left = if row == start.1 { start.0 as usize } else { 0 };
                if left > 0
                    && cells
                        .get(left)
                        .is_some_and(|cell| cell["wide_spacer"].as_bool().unwrap_or(false))
                {
                    left -= 1;
                }
                let right = if row == end.1 {
                    end.0 as usize + 1
                } else {
                    cells.len()
                };
                for cell in cells.iter().take(right).skip(left) {
                    if !cell["wide_spacer"].as_bool().unwrap_or(false) {
                        result.push_str(cell["text"].as_str().unwrap_or(" "));
                    }
                }
                if row < end.1 {
                    result.push('\n');
                }
            }
        }
    }
    result
}

fn window_exited(snapshot: &Value, id: &str) -> bool {
    snapshot["terminals"].as_array().is_some_and(|ts| {
        ts.iter()
            .any(|t| text(t, "id") == id && t["alive"] == false)
    })
}
fn open_window(source: &str, omp: bool) -> Value {
    let mut request = json!({"op":"open","omp":omp});
    if !source.is_empty() {
        request["source"] = json!(source);
    }
    request
}
fn close_window(snapshot: &Value, id: &str) -> Value {
    json!({"op":if window_exited(snapshot,id) {"remove"} else {"close"},"terminal":id})
}
fn apply_window(
    socket: &Path,
    request: Value,
    active: &mut String,
    snapshot: &mut Value,
) -> Result<()> {
    let opens = request["op"] == "open";
    let result = call(socket, request)?;
    *snapshot = call(socket, json!({"op":"snapshot"}))?;
    if opens {
        *active = text(&result, "id").to_owned();
    } else if !snapshot["terminals"]
        .as_array()
        .is_some_and(|ts| ts.iter().any(|t| text(t, "id") == active))
    {
        *active = snapshot["terminals"]
            .as_array()
            .and_then(|ts| {
                ts.iter()
                    .find(|t| t["alive"] == true)
                    .or_else(|| ts.first())
            })
            .map(|t| text(t, "id").to_owned())
            .unwrap_or_default();
    }
    Ok(())
}

pub fn run(socket: &Path, terminal: Option<String>) -> Result<()> {
    let mut snapshot = call(socket, json!({"op":"snapshot"}))?;
    let detach_generation = snapshot["detach_generation"].as_u64().unwrap_or(0);
    let terminals = snapshot["terminals"]
        .as_array()
        .context("snapshot has no terminals")?;
    let mut active = if let Some(selector) = terminal {
        let state = call(socket, json!({"op":"state","terminal":selector}))?;
        state["id"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| {
                terminals
                    .iter()
                    .find(|t| text(t, "id") == selector || text(t, "name") == selector)
                    .map(|t| text(t, "id").to_owned())
            })
            .context("terminal state has no id")?
    } else if let Some(first) = terminals
        .iter()
        .find(|t| t["alive"] == true)
        .or_else(|| terminals.first())
    {
        text(first, "id").to_owned()
    } else {
        text(&call(socket, json!({"op":"open"}))?, "id").to_owned()
    };
    let _restore = Restore::enter()?;
    let mut display =
        ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(io::stdout()))?;
    let mut size = terminal::size()?;
    let mut geometry = view::geometry(ratatui::layout::Rect::new(0, 0, size.0, size.1));
    call(
        socket,
        json!({"op":"resize","terminal":active,"cols":size.0.clamp(2,500),"rows":size.1.saturating_sub(HEADER).clamp(2,300)}),
    )?;
    let mut screen = Value::Null;
    let mut picker = false;
    let mut prefix_pending = false;
    let mut query = String::new();
    let mut selected = 0usize;
    let mut offset = 0usize;
    let mut status = String::new();
    let mut pending_parent: Option<(String, bool)> = None;
    let mut selection = None;
    let mut focused = true;
    let mut snapshot_at = Instant::now();
    let mut dirty = true;
    let mut frame_at = Instant::now() - FRAME;
    loop {
        if frame_at.elapsed() >= FRAME {
            if snapshot_at.elapsed() >= Duration::from_millis(500) {
                snapshot = call(socket, json!({"op":"snapshot"}))?;
                if snapshot["detach_generation"].as_u64().unwrap_or(0) != detach_generation {
                    break;
                }
                snapshot_at = Instant::now();
                if !snapshot["terminals"]
                    .as_array()
                    .is_some_and(|ts| ts.iter().any(|t| text(t, "id") == active))
                {
                    active = snapshot["terminals"]
                        .as_array()
                        .and_then(|ts| ts.first())
                        .map(|t| text(t, "id").to_owned())
                        .unwrap_or_default();
                    picker = true;
                    offset = 0;
                    screen = Value::Null;
                    selection = None;
                }
                dirty = true;
            }
            if !picker {
                let update = call(
                    socket,
                    json!({"op":"screen","terminal":active,"offset":offset,"limit":size.1.saturating_sub(HEADER).max(2),"since":if dirty {Value::Null}else{screen["generation"].clone()}}),
                )?;
                if update["unchanged"] != true {
                    screen = update;
                    dirty = true;
                }
            }
            if dirty {
                let rows = tree_rows(&snapshot, &query);
                selected = selected.min(rows.len().saturating_sub(1));
                display.draw(|frame| {
                    geometry = view::draw(
                        frame, &screen, &snapshot, &active, picker, &rows, selected, &query,
                        &status, offset, focused, selection,
                    );
                })?;
                dirty = false;
            }
            frame_at = Instant::now();
        }
        if !event::poll(FRAME.saturating_sub(frame_at.elapsed()))? {
            continue;
        }
        let event = event::read()?;
        dirty = true;
        let previous_prefix = prefix_pending;
        if !matches!(
            event,
            Event::Key(KeyEvent {
                kind: KeyEventKind::Release,
                ..
            })
        ) {
            prefix_pending = false;
        }
        let prefix = prefix_key(snapshot["config"]["prefix"].as_str().unwrap_or("Ctrl-b"));
        let rows = tree_rows(&snapshot, &query);
        match event {
            Event::Resize(cols, rows) => {
                size = (cols, rows);
                // A picker is an overlay: even foreground resize is deferred until it closes.
                if !picker {
                    call(
                        socket,
                        json!({"op":"resize","terminal":active,"cols":cols.clamp(2,500),"rows":rows.saturating_sub(HEADER).clamp(2,300)}),
                    )?;
                }
            }
            Event::FocusGained => {
                focused = true;
                if mode(&screen, "focus_reporting") {
                    send_input(socket, &active, b"\x1b[I", &mut status)?;
                }
            }
            Event::FocusLost => {
                focused = false;
                if mode(&screen, "focus_reporting") {
                    send_input(socket, &active, b"\x1b[O", &mut status)?;
                }
            }
            Event::Paste(paste) => {
                if picker {
                    query.push_str(&paste.replace(['\n', '\r'], " "));
                    selected = 0;
                } else {
                    offset = 0;
                    if paste.len() > crate::runtime::MAX_INPUT {
                        status = "invalid-input: input exceeds 64 KiB".into();
                        continue;
                    }
                    input_result(
                        call(
                            socket,
                            json!({"op":"input","terminal":active,"text":paste,"paste":true}),
                        ),
                        &mut status,
                    )?;
                }
            }
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if is_prefix(key, prefix) {
                    if picker && previous_prefix && !active.is_empty() {
                        picker = false;
                        call(
                            socket,
                            json!({"op":"resize","terminal":active,"cols":size.0.clamp(2,500),"rows":size.1.saturating_sub(HEADER).clamp(2,300)}),
                        )?;
                        send_input(
                            socket,
                            &active,
                            &key_bytes(prefix, mode(&screen, "application_cursor")),
                            &mut status,
                        )?;
                    } else {
                        picker = true;
                        prefix_pending = true;
                        query.clear();
                        selected = rows.iter().position(|r| r.id == active).unwrap_or(0);
                        pending_parent = None;
                    }
                    continue;
                }
                if picker {
                    let chosen = rows.get(selected).map(|r| r.id.clone());
                    let control = key.modifiers.contains(KeyModifiers::CONTROL);
                    let action = match key.code {
                        KeyCode::Char('q') if control => break,
                        KeyCode::Esc => {
                            picker = active.is_empty();
                            pending_parent = None;
                            None
                        }
                        KeyCode::Up => {
                            selected = selected.saturating_sub(1);
                            None
                        }
                        KeyCode::Down => {
                            selected = (selected + 1).min(rows.len().saturating_sub(1));
                            None
                        }
                        KeyCode::Backspace => {
                            query.pop();
                            selected = 0;
                            None
                        }
                        KeyCode::Enter => {
                            if let Some(id) = chosen {
                                if let Some((child, demote)) = pending_parent.take() {
                                    Some(
                                        json!({"op":if demote {"demote"} else {"reparent"},"terminal":child,"parent":id}),
                                    )
                                } else {
                                    active = id;
                                    screen = Value::Null;
                                    selection = None;
                                    status.clear();
                                    offset = 0;
                                    picker = false;
                                    None
                                }
                            } else {
                                None
                            }
                        }
                        KeyCode::Char('n') if control => {
                            Some(open_window(chosen.as_deref().unwrap_or(&active), false))
                        }
                        KeyCode::Char('o') if control => {
                            Some(open_window(chosen.as_deref().unwrap_or(&active), true))
                        }
                        KeyCode::Char('p') if control => {
                            chosen.map(|id| json!({"op":"promote","terminal":id}))
                        }
                        KeyCode::Char('x') if control => {
                            chosen.map(|id| close_window(&snapshot, &id))
                        }
                        KeyCode::Char('r' | 'd') if control => {
                            if let Some(id) = chosen {
                                pending_parent = Some((id, key.code == KeyCode::Char('d')));
                                status = "Select new parent and press Enter; Esc cancels".into();
                            }
                            None
                        }
                        KeyCode::Char(c)
                            if !control && !key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            query.push(c);
                            selected = 0;
                            None
                        }
                        _ => None,
                    };
                    if let Some(request) = action {
                        let opens = request["op"] == "open";
                        match apply_window(socket, request, &mut active, &mut snapshot) {
                            Ok(()) => {
                                status.clear();
                                offset = 0;
                                screen = Value::Null;
                                selection = None;
                                if opens {
                                    picker = false;
                                }
                                if active.is_empty() {
                                    picker = true;
                                }
                            }
                            Err(error) => status = format!("{error:#}"),
                        }
                    }
                    if !picker {
                        call(
                            socket,
                            json!({"op":"resize","terminal":active,"cols":size.0.clamp(2,500),"rows":size.1.saturating_sub(HEADER).clamp(2,300)}),
                        )?;
                    }
                } else if window_exited(&snapshot, &active) {
                    let request = match key.code {
                        KeyCode::Char('n') | KeyCode::Enter => Some(open_window(&active, false)),
                        KeyCode::Char('x') | KeyCode::Delete => {
                            Some(close_window(&snapshot, &active))
                        }
                        KeyCode::Esc => {
                            picker = true;
                            None
                        }
                        _ => None,
                    };
                    if let Some(request) = request {
                        match apply_window(socket, request, &mut active, &mut snapshot) {
                            Ok(()) => {
                                picker = active.is_empty();
                                screen = Value::Null;
                                offset = 0;
                                selection = None;
                                status.clear();
                                if !picker {
                                    call(
                                        socket,
                                        json!({"op":"resize","terminal":active,"cols":size.0.clamp(2,500),"rows":size.1.saturating_sub(HEADER).clamp(2,300)}),
                                    )?;
                                }
                            }
                            Err(error) => status = format!("{error:#}"),
                        }
                    }
                } else if key.modifiers.contains(KeyModifiers::SHIFT)
                    && matches!(key.code, KeyCode::PageUp | KeyCode::PageDown)
                {
                    let page = size.1.saturating_sub(HEADER) as usize;
                    offset = if key.code == KeyCode::PageUp {
                        (offset + page).min(screen["history_size"].as_u64().unwrap_or(0) as usize)
                    } else {
                        offset.saturating_sub(page)
                    };
                } else {
                    offset = 0;
                    let bytes = key_bytes(key, mode(&screen, "application_cursor"));
                    if !bytes.is_empty() {
                        send_input(socket, &active, &bytes, &mut status)?;
                    }
                }
            }
            Event::Mouse(mouse) => {
                if view::Geometry::hit(geometry.switch, mouse.column, mouse.row)
                    && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                {
                    picker = !picker || active.is_empty();
                    query.clear();
                    pending_parent = None;
                    selected = rows.iter().position(|r| r.id == active).unwrap_or(0);
                    status.clear();
                    if !picker {
                        call(
                            socket,
                            json!({"op":"resize","terminal":active,"cols":size.0.clamp(2,500),"rows":size.1.saturating_sub(HEADER).clamp(2,300)}),
                        )?;
                    }
                } else if picker {
                    if matches!(mouse.kind, MouseEventKind::ScrollUp) {
                        selected = selected.saturating_sub(3);
                    } else if matches!(mouse.kind, MouseEventKind::ScrollDown) {
                        selected = (selected + 3).min(rows.len().saturating_sub(1));
                    } else if view::Geometry::hit(geometry.body, mouse.column, mouse.row)
                        && mouse.row.saturating_sub(geometry.body.y) / view::ROW_HEIGHT
                            < geometry.body.height / view::ROW_HEIGHT
                        && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                    {
                        let start = view::list_start(selected, geometry.body.height);
                        if let Some(row) = rows.get(
                            start
                                + (mouse.row.saturating_sub(geometry.body.y) / view::ROW_HEIGHT)
                                    as usize,
                        ) {
                            if let Some((child, demote)) = pending_parent.take() {
                                match call(
                                    socket,
                                    json!({"op":if demote {"demote"} else {"reparent"},"terminal":child,"parent":row.id}),
                                ) {
                                    Ok(_) => {
                                        snapshot = call(socket, json!({"op":"snapshot"}))?;
                                    }
                                    Err(error) => status = format!("{error:#}"),
                                }
                            } else {
                                active = row.id.clone();
                                picker = false;
                                offset = 0;
                                screen = Value::Null;
                                selection = None;
                                status.clear();
                                call(
                                    socket,
                                    json!({"op":"resize","terminal":active,"cols":size.0.clamp(2,500),"rows":size.1.saturating_sub(HEADER).clamp(2,300)}),
                                )?;
                            }
                        }
                    }
                } else if window_exited(&snapshot, &active) {
                    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                        let request = if view::Geometry::hit(geometry.new, mouse.column, mouse.row)
                        {
                            Some(open_window(&active, false))
                        } else if view::Geometry::hit(geometry.close, mouse.column, mouse.row) {
                            Some(close_window(&snapshot, &active))
                        } else {
                            None
                        };
                        if let Some(request) = request {
                            match apply_window(socket, request, &mut active, &mut snapshot) {
                                Ok(()) => {
                                    picker = active.is_empty();
                                    screen = Value::Null;
                                    offset = 0;
                                    selection = None;
                                    status.clear();
                                    if !picker {
                                        call(
                                            socket,
                                            json!({"op":"resize","terminal":active,"cols":size.0.clamp(2,500),"rows":size.1.saturating_sub(HEADER).clamp(2,300)}),
                                        )?;
                                    }
                                }
                                Err(error) => status = format!("{error:#}"),
                            }
                        }
                    }
                } else if mouse.row >= HEADER {
                    let application_mouse = mode(&screen, "mouse_click")
                        || mode(&screen, "mouse_drag")
                        || mode(&screen, "mouse_motion");
                    if application_mouse
                        && !mouse.modifiers.contains(KeyModifiers::SHIFT)
                        && offset == 0
                    {
                        let bytes = mouse_bytes(mouse, &screen);
                        if !bytes.is_empty() {
                            send_input(socket, &active, &bytes, &mut status)?;
                        }
                    } else {
                        let point = (mouse.column, mouse.row - HEADER);
                        match mouse.kind {
                            MouseEventKind::ScrollUp => {
                                offset = (offset + 3)
                                    .min(screen["history_size"].as_u64().unwrap_or(0) as usize)
                            }
                            MouseEventKind::ScrollDown => offset = offset.saturating_sub(3),
                            MouseEventKind::Down(MouseButton::Left) => {
                                selection = Some((point, point))
                            }
                            MouseEventKind::Drag(MouseButton::Left) => {
                                if let Some((_, end)) = selection.as_mut() {
                                    *end = point;
                                }
                            }
                            MouseEventKind::Up(MouseButton::Left) => {
                                if let Some((start, _)) = selection.take() {
                                    let selected = selected_text(&screen, start, point);
                                    if !selected.is_empty() {
                                        write!(
                                            io::stdout(),
                                            "\x1b]52;c;{}\x07",
                                            base64(selected.as_bytes())
                                        )?;
                                        io::stdout().flush()?;
                                        status = "Selection copied via OSC52 (terminal permission may be required)".into();
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utf8_mouse_reports_non_ascii_and_large_coordinates() {
        let screen = json!({"modes":{"mouse_utf8":true}});
        for column in [95, 223] {
            let event = MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row: 110,
                modifiers: KeyModifiers::NONE,
            };
            let bytes = mouse_bytes(event, &screen);
            let decoded = std::str::from_utf8(&bytes).unwrap();
            assert_eq!(
                decoded.chars().map(u32::from).collect::<Vec<_>>(),
                [27, 91, 77, 32, u32::from(column) + 33, 140]
            );
        }
        let event = MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 100,
            row: 110,
            modifiers: KeyModifiers::CONTROL,
        };
        assert_eq!(
            std::str::from_utf8(&mouse_bytes(event, &screen))
                .unwrap()
                .chars()
                .nth(3),
            Some('3')
        );
    }
    #[test]
    fn native_input_preserves_control_alt_unicode_and_application_cursor() {
        assert_eq!(
            key_bytes(
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                false
            ),
            [3]
        );
        assert_eq!(
            key_bytes(KeyEvent::new(KeyCode::Char('λ'), KeyModifiers::ALT), false),
            "\x1bλ".as_bytes()
        );
        assert_eq!(
            key_bytes(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), true),
            b"\x1bOA"
        );
        assert_eq!(
            key_bytes(
                KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
                false
            ),
            b"\x1b[1;6D"
        );
        assert_eq!(
            key_bytes(KeyEvent::new(KeyCode::F(12), KeyModifiers::ALT), false),
            b"\x1b[24;3~"
        );
    }
    #[test]
    fn picker_preserves_hierarchy_and_matches_workspace_metadata() {
        let snapshot = json!({"projects":[{"id":"p","name":"Compiler"}],"terminals":[
            {"id":"child","name":"worker","parent_id":"root","project_id":"p","workspace":{"cwd":"/src/compiler","git":{"branch":"feature/unicode"}}},
            {"id":"root","name":"host","parent_id":null,"project_id":"p","workspace":{"cwd":"/src"}}
        ]});
        let rows = tree_rows(&snapshot, "");
        assert_eq!(
            rows.iter()
                .map(|r| (r.id.as_str(), r.depth))
                .collect::<Vec<_>>(),
            vec![("root", 0), ("child", 1)]
        );
        assert_eq!(tree_rows(&snapshot, "funi")[0].id, "child");
        assert_eq!(tree_rows(&snapshot, "CMP").len(), 2);
        assert!(tree_rows(&snapshot, "zzzz").is_empty());
    }
    #[test]
    fn malformed_cycle_does_not_hide_terminals_or_loop() {
        let snapshot = json!({"terminals":[{"id":"a","parent_id":"b"},{"id":"b","parent_id":"a"}]});
        assert_eq!(tree_rows(&snapshot, "").len(), 2);
    }
    #[test]
    fn selection_skips_wide_spacers_and_clipboard_encoding_is_padded() {
        let screen = json!({"cells":[[{"text":"界"},{"text":"","wide_spacer":true},{"text":"x"}]]});
        assert_eq!(selected_text(&screen, (0, 0), (2, 0)), "界x");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
    }
    #[test]
    fn mouse_protocol_uses_surface_coordinates_and_sgr_release() {
        let mouse = MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 2,
            row: HEADER + 4,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            mouse_bytes(mouse, &json!({"modes":{"mouse_sgr":true}})),
            b"\x1b[<0;3;5m"
        );
    }
    #[test]
    fn unicode_labels_are_cell_bounded_and_prefix_is_configurable() {
        assert_eq!(clip_label("a界b", 3), "a界");
        assert_eq!(clip_label("a界b", 2), "a");
        assert_eq!(clip_label("bad\u{1b}label", 8), "badlabel");
        assert!(is_prefix(
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
            prefix_key("Ctrl-a")
        ));
        assert!(!is_prefix(
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
            prefix_key("Ctrl-a")
        ));
    }
}

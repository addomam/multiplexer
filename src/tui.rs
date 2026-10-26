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
    cell::RefCell,
    collections::HashSet,
    io::{self, Write},
    path::Path,
    time::{Duration, Instant, SystemTime},
};

mod agent;
mod control;
mod launch;
mod view;
use control::{Input, NativeControl};

const HEADER: u16 = 3;
const FRAME: Duration = Duration::from_millis(8);
const INPUT_FRAME: Duration = Duration::from_millis(1);
const INPUT_BURST: Duration = Duration::from_millis(50);
const EVENTS_BEFORE_FRAME: usize = 32;
const RESUME_GAP: Duration = Duration::from_secs(2);

fn pause_expired(last: SystemTime) -> bool {
    // Instant's Darwin uptime clock excludes system sleep. Wall-clock gaps
    // catch machine suspend too; backward clock changes conservatively fence.
    last.elapsed().map_or(true, |elapsed| elapsed >= RESUME_GAP)
}

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

type Session = RefCell<crate::client::Connection>;

fn call(socket: &Session, mut request: Value) -> Result<Value> {
    request["version"] = json!(1);
    request["persistent"] = json!(true);
    let timeout = request["timeout_ms"].as_u64().unwrap_or(30000).min(300000) + 5000;
    socket
        .borrow_mut()
        .request(&request, Duration::from_millis(timeout.min(3000)))
}

#[derive(serde::Serialize)]
struct ScreenRequest<'a> {
    version: u8,
    persistent: bool,
    op: &'static str,
    terminal: &'a str,
    offset: usize,
    limit: u16,
    since: Option<u64>,
    row_updates: bool,
}

#[derive(Default)]
struct ScreenViewport {
    terminal: String,
    offset: usize,
    limit: u16,
}

fn refresh_screen(
    socket: &Session,
    screen: &mut Value,
    viewport: &mut ScreenViewport,
    surface: &mut view::NativeSurface,
    terminal: &str,
    offset: usize,
    limit: u16,
) -> Result<bool> {
    // UI dirtiness is not native damage. Reuse only the same terminal viewport,
    // regardless of key/focus/status changes; resizing and switching require full rows.
    let since =
        if viewport.terminal == terminal && viewport.offset == offset && viewport.limit == limit {
            screen["generation"].as_u64()
        } else {
            None
        };
    let update = socket.borrow_mut().request(
        &ScreenRequest {
            version: 1,
            persistent: true,
            op: "screen",
            terminal,
            offset,
            limit,
            since,
            row_updates: true,
        },
        Duration::from_secs(3),
    )?;
    if update["unchanged"] == true {
        return Ok(false);
    }
    merge_screen(screen, update, surface)?;
    if viewport.terminal != terminal {
        viewport.terminal.clear();
        viewport.terminal.push_str(terminal);
    }
    viewport.offset = offset;
    viewport.limit = limit;
    Ok(true)
}

fn merge_screen(
    screen: &mut Value,
    mut update: Value,
    surface: &mut view::NativeSurface,
) -> Result<()> {
    if update["delta"] != true {
        surface.update(&update);
        *screen = update;
        return Ok(());
    }
    anyhow::ensure!(
        update["base_generation"].as_u64().is_some()
            && update["base_generation"] == screen["generation"]
            && update["cols"] == screen["cols"]
            && update["rows"] == screen["rows"]
            && update["offset"] == screen["offset"],
        "protocol: screen delta does not match the displayed viewport"
    );
    let indices = update["row_indices"]
        .as_array()
        .context("protocol: screen delta has no row indices")?;
    for field in ["cells", "lines"] {
        let rows = update[field]
            .as_array()
            .context("protocol: screen delta has no rows")?;
        let original = screen[field]
            .as_array()
            .context("protocol: screen delta has no base rows")?;
        anyhow::ensure!(
            rows.len() == indices.len(),
            "protocol: mismatched screen delta rows"
        );
        for index in indices {
            anyhow::ensure!(
                index
                    .as_u64()
                    .is_some_and(|row| row < original.len() as u64),
                "protocol: screen delta row is out of bounds"
            );
        }
    }
    surface.update(&update);
    // Move unchanged rows, rather than cloning every cell on every key.
    let indices = update["row_indices"].take();
    let indices = indices.as_array().expect("validated row indices");
    for field in ["cells", "lines"] {
        let changed = update[field].take();
        let Value::Array(changed) = changed else {
            unreachable!("validated delta rows")
        };
        let mut original = screen[field].take();
        let rows = original.as_array_mut().expect("validated base rows");
        for (index, row) in indices.iter().zip(changed) {
            rows[index.as_u64().expect("validated row index") as usize] = row;
        }
        update[field] = original;
    }
    let metadata = update
        .as_object_mut()
        .context("protocol: invalid screen update")?;
    metadata.remove("delta");
    metadata.remove("base_generation");
    metadata.remove("row_indices");
    *screen = update;
    Ok(())
}
fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}
fn mode(screen: &Value, name: &str) -> bool {
    screen["modes"][name].as_bool().unwrap_or(false)
}

/// Encode native input after classifying wrapper actions and ownership controls.
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
    if let Some(agents) = snapshot["agent_views"].as_array() {
        for agent in agents {
            let project = projects
                .iter()
                .find(|project| project["id"] == agent["project_id"]);
            let label = format!(
                "ACP · {} · {}\n{}",
                text(agent, "name"),
                text(agent, "lifecycle"),
                project
                    .map(|project| text(project, "name"))
                    .unwrap_or("project")
            );
            if fuzzy(query, &label) {
                rows.push(Row {
                    id: text(agent, "id").to_owned(),
                    label,
                    depth: 0,
                });
            }
        }
    }
    rows
}

fn clip_label(label: &str, cols: u16) -> String {
    use unicode_width::UnicodeWidthChar;
    let mut width = 0usize;
    label
        .chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(*c, '\u{061c}' | '\u{200e}' | '\u{200f}'
                    | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
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

fn agent_view<'a>(snapshot: &'a Value, id: &str) -> Option<&'a Value> {
    snapshot["agent_views"]
        .as_array()?
        .iter()
        .find(|view| view["id"] == id)
}

fn agent_target(view: &Value) -> Value {
    json!({"kind":"acp","id":view["id"],"instance_id":view["instance_id"],
        "connection_generation":view["connection_generation"],"session_id":view["session_id"]})
}

fn contains_view(snapshot: &Value, id: &str) -> bool {
    agent_view(snapshot, id).is_some()
        || snapshot["terminals"]
            .as_array()
            .is_some_and(|terminals| terminals.iter().any(|terminal| terminal["id"] == id))
}

fn resize_native(socket: &Session, snapshot: &Value, id: &str, size: (u16, u16)) -> Result<()> {
    if agent_view(snapshot, id).is_none() && !id.is_empty() {
        call(
            socket,
            json!({"op":"resize","terminal":id,"cols":size.0.clamp(2,500),
            "rows":size.1.saturating_sub(HEADER).clamp(2,300)}),
        )?;
    }
    Ok(())
}

fn open_from_view(snapshot: &Value, id: &str, omp: bool) -> Value {
    if let Some(view) = agent_view(snapshot, id) {
        json!({"op":"open","project":view["project_id"],"omp":omp})
    } else {
        open_window(id, omp)
    }
}

fn close_agent(
    authority: &mut NativeControl,
    snapshot: &Value,
    id: &str,
    status: &mut String,
) -> bool {
    let Some(view) = agent_view(snapshot, id) else {
        return false;
    };
    let Some(grant) = authority.acp_grant(id) else {
        *status = "ACP observer: obtain control before explicitly closing this instance".into();
        return true;
    };
    let request = json!({"op":"acp-close","target":agent_target(view),
        "control":grant["control"],"owner_generation":grant["owner_generation"],
        "call_id":uuid::Uuid::new_v4().to_string()});
    let _ = authority.acp_action(request, status);
    true
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
    socket: &Session,
    request: Value,
    active: &mut String,
    snapshot: &mut Value,
    resume: &mut Option<Resume>,
) -> Result<()> {
    let opens = request["op"] == "open";
    let result = call(socket, request)?;
    *snapshot = call(socket, json!({"op":"snapshot"}))?;
    if opens {
        *active = text(&result, "id").to_owned();
    } else if !contains_view(snapshot, active) {
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
    Resume::select(resume, snapshot, active);
    Ok(())
}

#[derive(Clone)]
struct Resume {
    daemon: Value,
    active: String,
    target: Value,
    detach_generation: u64,
}

#[derive(Debug)]
struct AttachmentEnded(&'static str);

impl std::fmt::Display for AttachmentEnded {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}
impl std::error::Error for AttachmentEnded {}

impl Resume {
    fn select(current: &mut Option<Self>, snapshot: &Value, active: &str) {
        if current
            .as_ref()
            .is_none_or(|original| original.active != active)
        {
            *current = Some(Self::capture(snapshot, active));
        }
    }

    fn capture(snapshot: &Value, active: &str) -> Self {
        Self {
            daemon: snapshot["daemon_id"].clone(),
            active: active.to_owned(),
            target: agent_view(snapshot, active)
                .map(agent_target)
                .unwrap_or_else(|| json!({"kind":"pty","id":active})),
            detach_generation: snapshot["detach_generation"].as_u64().unwrap_or(0),
        }
    }

    fn launched(snapshot: &Value, opened: &Value) -> Result<Self> {
        Ok(Self {
            daemon: snapshot["daemon_id"].clone(),
            active: opened["target"]["id"]
                .as_str()
                .context("ACP launch has no exact target")?
                .to_owned(),
            target: opened["target"].clone(),
            detach_generation: snapshot["detach_generation"].as_u64().unwrap_or(0),
        })
    }

    fn validate(&mut self, snapshot: &Value) -> Result<()> {
        if !self.daemon.is_string() || snapshot["daemon_id"] != self.daemon {
            return Err(AttachmentEnded("Ended attachment: daemon replaced or continuity unreported; original work was not relaunched").into());
        }
        if self.target["kind"] == "acp" {
            let Some(view) = agent_view(snapshot, &self.active) else {
                return Err(AttachmentEnded(
                    "Ended attachment: original ACP target is missing; no same-name substitution",
                )
                .into());
            };
            // Only initial session establishment may advance identity. A
            // known session, instance, connection or daemon never follows a
            // replacement, including across recovery.
            if ["id", "instance_id", "connection_generation"]
                .iter()
                .any(|key| self.target[key] != view[key])
                || (!self.target["session_id"].is_null()
                    && self.target["session_id"] != view["session_id"])
            {
                return Err(AttachmentEnded("Ended attachment: exact original ACP instance/session changed; no automatic attachment").into());
            }
            if view["lifecycle"] == "ended" || view["lifecycle"] == "closing" {
                return Err(AttachmentEnded(
                    "Ended: original ACP agent ended; no replacement launched",
                )
                .into());
            }
            if self.target["session_id"].is_null() && !view["session_id"].is_null() {
                self.target["session_id"] = view["session_id"].clone();
            }
        } else if !snapshot["terminals"].as_array().is_some_and(|terminals| {
            terminals
                .iter()
                .any(|terminal| terminal["id"] == self.active && terminal["alive"] == true)
        }) {
            return Err(AttachmentEnded(
                "Ended: original terminal is missing or exited; no replacement opened",
            )
            .into());
        }
        Ok(())
    }
}

/// Start one ACP instance with the real original native controller, then attach.
pub fn start(socket: &Path, launch: Value) -> Result<()> {
    supervise(socket, None, Some(launch))
}

pub fn run(socket: &Path, terminal: Option<String>) -> Result<()> {
    supervise(socket, terminal, None)
}

fn supervise(path: &Path, mut terminal: Option<String>, mut launch: Option<Value>) -> Result<()> {
    let _restore = Restore::enter()?;
    let mut resume = None;
    loop {
        match attached(path, terminal.take(), launch.take(), &mut resume) {
            Ok(()) => return Ok(()),
            Err(error) => {
                let ended = error.downcast_ref::<AttachmentEnded>().is_some();
                if !ended && (!crate::client::is_connection_error(&error) || resume.is_none()) {
                    return Err(error);
                }
                if !recovery_wait(&format!("{error:#}"), !ended)? {
                    return Ok(());
                }
            }
        }
    }
}

fn recovery_wait(reason: &str, retry: bool) -> Result<bool> {
    let mut display =
        ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(io::stdout()))?;
    // This is a new diff buffer over an existing alternate screen. Reset the
    // physical surface too, so stale projection/error text cannot survive it.
    display.clear()?;
    let message = format!(
        "{}\n{}\n\nLocal input, drafts, claim/consent state discarded. Broker work and questions retained; no mutation replay or automatic control acquisition.\nCtrl-Q / Esc quits safely.{}\n\n{}",
        if retry {
            "Disconnected · input fenced"
        } else {
            "Ended · original attachment unavailable"
        },
        if retry {
            "Resynchronizing with the existing daemon socket only"
        } else {
            "Agent/daemon was not relaunched"
        },
        if retry {
            " Reconnect waits for queued input to drain."
        } else {
            ""
        },
        clip_label(reason, 1024)
    );
    display.draw(|frame| {
        frame.render_widget(ratatui::widgets::Clear, frame.area());
        frame.render_widget(
            ratatui::widgets::Paragraph::new(message.as_str())
                .wrap(ratatui::widgets::Wrap { trim: false }),
            frame.area(),
        );
    })?;
    let mut quiet = Instant::now();
    loop {
        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key)
                    if key.kind != KeyEventKind::Release
                        && (key.code == KeyCode::Esc
                            || key.code == KeyCode::Char('q')
                                && key.modifiers.contains(KeyModifiers::CONTROL)) =>
                {
                    return Ok(false);
                }
                _ => {}
            }
            quiet = Instant::now();
        } else if retry && quiet.elapsed() >= Duration::from_millis(250) {
            return Ok(true);
        }
    }
}

fn attached(
    path: &Path,
    terminal: Option<String>,
    launch_request: Option<Value>,
    resume: &mut Option<Resume>,
) -> Result<()> {
    let recovered = resume.is_some();
    let socket = &Session::new(crate::client::Connection::connect(path)?);
    let mut snapshot = call(socket, json!({"op":"snapshot"}))?;
    if let Some(original) = resume.as_mut() {
        original.validate(&snapshot)?;
        if snapshot["detach_generation"].as_u64().unwrap_or(0) != original.detach_generation {
            return Ok(());
        }
    }
    let terminal = resume
        .as_ref()
        .map(|original| original.active.clone())
        .or(terminal);
    let detach_generation = snapshot["detach_generation"].as_u64().unwrap_or(0);
    let terminals = snapshot["terminals"]
        .as_array()
        .context("snapshot has no terminals")?;
    let mut active = if launch_request.is_some() {
        String::new()
    } else if let Some(selector) = terminal {
        if agent_view(&snapshot, &selector).is_some() {
            selector
        } else {
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
        }
    } else if let Some(first) = terminals
        .iter()
        .find(|t| t["alive"] == true)
        .or_else(|| terminals.first())
    {
        text(first, "id").to_owned()
    } else if let Some(first) = snapshot["agent_views"]
        .as_array()
        .and_then(|views| views.first())
    {
        text(first, "id").to_owned()
    } else {
        text(&call(socket, json!({"op":"open"}))?, "id").to_owned()
    };
    *resume = Some(Resume::capture(&snapshot, &active));
    let mut display =
        ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(io::stdout()))?;
    display.clear()?;
    let mut size = terminal::size()?;
    let mut geometry = view::geometry(ratatui::layout::Rect::new(0, 0, size.0, size.1));
    resize_native(socket, &snapshot, &active, size)?;
    // Observe, validate, select and finish fallible display setup before
    // spending a controller registration on an automatic recovery attempt.
    let mut authority = NativeControl::register(
        path,
        &snapshot["daemon_id"],
        socket.borrow().shutdown_handle()?,
    )?;
    if recovered {
        authority.recovered();
    }
    authority.sync_agent_targets(&snapshot);
    let mut screen = Value::Null;
    let mut picker = active.is_empty();
    let mut prefix_pending = false;
    let mut query = String::new();
    let mut selected = 0usize;
    let mut offset = 0usize;
    let mut status = if recovered {
        "Reconnecting · observer snapshot; discarding queued input".to_owned()
    } else {
        String::new()
    };
    let mut launch_dialog: Option<launch::Dialog> = None;
    if let Some(request) = launch_request {
        authority.launch(request, &mut status);
    }
    if !active.is_empty() {
        authority.report_view(&active, None, &mut status);
    }
    let mut pending_parent: Option<(String, bool)> = None;
    let mut selection = None;
    let mut focused = true;
    let mut reported_focus = None;
    let mut snapshot_at = Instant::now();
    let mut dirty = true;
    let mut viewport = ScreenViewport::default();
    let mut surface = view::NativeSurface::default();
    let mut agent = agent::AgentView::new();
    let mut agent_projection = Value::Null;
    let mut agent_error = String::new();
    let mut agent_id = String::new();
    let mut agent_after = 0;
    let mut agent_at = Instant::now() - Duration::from_millis(100);
    let mut responsive_until = Instant::now();
    let mut urgent_frame = false;
    let mut queued_events = 0usize;
    let mut frame_at = Instant::now() - FRAME;
    let mut drain_recovered_input = recovered;
    let mut tick = SystemTime::now();
    let outcome = (|| -> Result<()> {
        loop {
            if pause_expired(tick) {
                return Err(crate::client::connection_lost(
                    "attachment resumed after a pause; old input fenced",
                ));
            }
            tick = SystemTime::now();
            if !socket.borrow().connected() || !authority.connected() {
                return Err(crate::client::connection_lost(
                    "observation or original native connection lost",
                ));
            }
            authority.heartbeat();
            Resume::select(resume, &snapshot, &active);
            authority.sync_agent_targets(&snapshot);
            let authority_changed = authority.poll(&active, &mut status, |action, grant| {
                agent.sync(&agent_projection, grant);
                agent.restore_unadmitted(action);
            });
            dirty |= authority_changed;
            let launched = authority.take_launched();
            if let Some(opened) = launched.as_ref() {
                let continuity = Resume::launched(&snapshot, opened)?;
                active.clone_from(&continuity.active);
                *resume = Some(continuity);
            }
            if !authority.connected() {
                return Err(crate::client::connection_lost(
                    "original native control stream lost",
                ));
            }
            if launched.is_some() {
                snapshot = call(socket, json!({"op":"snapshot"}))?;
                authority.sync_agent_targets(&snapshot);
                resume
                    .as_mut()
                    .expect("acknowledged launch")
                    .validate(&snapshot)?;
                screen = Value::Null;
                selection = None;
                offset = 0;
                picker = false;
                status = "ACP launched on this original native controller".into();
                dirty = true;
            }
            let is_agent = agent_view(&snapshot, &active).is_some();
            if is_agent && agent_id != active {
                agent_id.clone_from(&active);
                agent_after = 0;
                agent_projection = Value::Null;
                agent_error.clear();
                agent_at = Instant::now() - Duration::from_millis(100);
                agent.sync(&Value::Null, None);
            } else if !is_agent && !agent_id.is_empty() {
                agent_id.clear();
                agent_projection = Value::Null;
                agent_after = 0;
                agent_error.clear();
                agent.sync(&Value::Null, None);
            }
            if is_agent && agent_at.elapsed() >= Duration::from_millis(100) {
                if let Some(view) = agent_view(&snapshot, &active) {
                    let target = agent_target(view);
                    if agent_projection["target"] != target {
                        // A real identity reset must fetch its complete projection.
                        // Failed reads retain the previous cache and captured draft.
                        agent_after = 0;
                    }
                    match call(
                        socket,
                        json!({"op":"acp-inspect","target":target,
                    "include_content":true,"after":agent_after,"limit":128}),
                    ) {
                        Ok(projection) => {
                            agent_error.clear();
                            agent_after = projection["content"]["through"]
                                .as_u64()
                                .unwrap_or(agent_after);
                            agent_projection = projection;
                        }
                        Err(error) => {
                            if crate::client::is_connection_error(&error) {
                                return Err(error);
                            }
                            let code = error
                                .downcast_ref::<crate::client::DaemonError>()
                                .map(|error| error.code.as_str());
                            if code == Some("stale-target") {
                                // session/new can complete between a snapshot
                                // and this read. Reconcile the exact attachment
                                // before classifying it as ended; only reads
                                // are refreshed, never a captured mutation.
                                snapshot = call(socket, json!({"op":"snapshot"}))?;
                                resume
                                    .as_mut()
                                    .context("ACP observation has no continuity target")?
                                    .validate(&snapshot)?;
                            } else if matches!(code, Some("target-ended" | "not-found")) {
                                return Err(AttachmentEnded(
                                    "Ended: original ACP target is no longer inspectable",
                                )
                                .into());
                            }
                            let cache_status = if agent_projection.is_null() {
                                "no confirmed projection"
                            } else {
                                "showing last confirmed projection"
                            };
                            agent_error = format!("ACP inspect failed; {cache_status}: {error:#}");
                        }
                    }
                }
                agent.sync(&agent_projection, authority.acp_grant(&active));
                agent_at = Instant::now();
                dirty = true;
            } else if is_agent && authority_changed {
                agent.sync(&agent_projection, authority.acp_grant(&active));
            }
            // Consume already received human events before changing native view
            // facts. The picker never changes the selected-terminal provenance.
            if !active.is_empty()
                && event::input_readiness().context("draining input before native view")?
                    == event::InputReadiness::Empty
            {
                authority.report_view(&active, reported_focus, &mut status);
            }
            let interval = if !picker && Instant::now() < responsive_until {
                INPUT_FRAME
            } else {
                FRAME
            };
            if (urgent_frame || frame_at.elapsed() >= interval)
                && (screen.is_null()
                    || drain_recovered_input
                    || queued_events >= EVENTS_BEFORE_FRAME
                    || !event::poll(Duration::ZERO).context("checking queued client input")?)
            {
                if snapshot_at.elapsed() >= Duration::from_millis(500) {
                    snapshot = call(socket, json!({"op":"snapshot"}))?;
                    if snapshot["detach_generation"].as_u64().unwrap_or(0) != detach_generation {
                        break;
                    }
                    if let Some(original) = resume.as_mut()
                        && original.active == active
                        && original.target["kind"] == "acp"
                    {
                        original.validate(&snapshot)?;
                    }
                    snapshot_at = Instant::now();
                    if !contains_view(&snapshot, &active) {
                        active = snapshot["terminals"]
                            .as_array()
                            .and_then(|ts| ts.first())
                            .map(|t| text(t, "id").to_owned())
                            .unwrap_or_default();
                        Resume::select(resume, &snapshot, &active);
                        picker = true;
                        offset = 0;
                        screen = Value::Null;
                        selection = None;
                    }
                    dirty = true;
                }
                if !picker
                    && !is_agent
                    && refresh_screen(
                        socket,
                        &mut screen,
                        &mut viewport,
                        &mut surface,
                        &active,
                        offset,
                        size.1.saturating_sub(HEADER).max(2),
                    )?
                {
                    dirty = true;
                }
                if dirty {
                    let rows = tree_rows(&snapshot, &query);
                    selected = selected.min(rows.len().saturating_sub(1));
                    let shown_status = if is_agent && !agent_error.is_empty() {
                        &agent_error
                    } else {
                        &status
                    };
                    let mut painted_size = size;
                    display
                        .draw(|frame| {
                            let area = frame.area();
                            painted_size = (area.width, area.height);
                            geometry = view::draw(
                                frame,
                                &screen,
                                &surface,
                                &snapshot,
                                &active,
                                picker,
                                &rows,
                                selected,
                                &query,
                                shown_status,
                                offset,
                                focused,
                                selection,
                            );
                            if is_agent && !picker {
                                agent.draw(frame, geometry.body);
                            }
                            if let Some(dialog) = &launch_dialog {
                                dialog.draw(frame);
                            }
                            authority.draw(
                                frame,
                                geometry,
                                &active,
                                &snapshot,
                                picker,
                                &query,
                                shown_status,
                                offset,
                            );
                        })
                        .context("drawing terminal surface")?;
                    // SIGWINCH can coalesce or be lost; the actual painted
                    // viewport is authoritative before consent becomes ready.
                    if painted_size != size {
                        size = painted_size;
                        authority.panel_event(&Event::Resize(size.0, size.1), &mut status);
                        if !picker {
                            resize_native(socket, &snapshot, &active, size)?;
                        }
                        screen = Value::Null;
                        dirty = true;
                        urgent_frame = true;
                        continue;
                    }
                    dirty = false;
                    if drain_recovered_input {
                        // Registration and the first projection can block.
                        // Until this observer frame is painted, queued events
                        // are recovery input, never deliberate new authority.
                        let mut input_empty = false;
                        for _ in 0..1024 {
                            match event::input_readiness()? {
                                event::InputReadiness::Empty => {
                                    input_empty = true;
                                    break;
                                }
                                event::InputReadiness::Pending => {}
                                event::InputReadiness::Inconclusive => break,
                            }
                            if let Event::Key(key) = event::read()?
                                && key.kind != KeyEventKind::Release
                                && (key.code == KeyCode::Esc
                                    || key.code == KeyCode::Char('q')
                                        && key.modifiers.contains(KeyModifiers::CONTROL))
                            {
                                return Ok(());
                            }
                        }
                        if !input_empty {
                            // A fragment or bounded yield is not fresh-human
                            // provenance. Keep draining without reconnect churn;
                            // normal event reads below remain discard-only.
                            dirty = true;
                            urgent_frame = false;
                            frame_at = Instant::now();
                            queued_events = 0;
                            continue;
                        }
                        // Resize is structural, not replayable application input.
                        // Reconcile the kernel size after the blocking setup,
                        // then paint and drain again before declaring readiness.
                        let current_size = terminal::size()?;
                        if current_size != size {
                            size = current_size;
                            resize_native(socket, &snapshot, &active, size)?;
                            screen = Value::Null;
                            dirty = true;
                            urgent_frame = true;
                            continue;
                        }
                        drain_recovered_input = false;
                        status = "Reconnected · observer; request control deliberately; nothing replayed"
                            .to_owned();
                        dirty = true;
                        urgent_frame = true;
                        continue;
                    }
                    if event::input_readiness().context("draining input before native consent")?
                        == event::InputReadiness::Empty
                    {
                        dirty = authority.after_draw(&mut status);
                    }
                }
                frame_at = Instant::now();
                urgent_frame = false;
                queued_events = 0;
            }
            let wait = if urgent_frame {
                Duration::ZERO
            } else {
                interval.saturating_sub(frame_at.elapsed())
            };
            if !event::poll(wait).context("polling client input")? {
                continue;
            }
            let event = event::read().context("reading client input")?;
            dirty = true;
            urgent_frame = true;
            queued_events += 1;
            responsive_until = Instant::now() + INPUT_BURST;
            if drain_recovered_input {
                // A bounded drain may span frames, never turn old input into
                // ordinary shortcuts while waiting for a conclusive boundary.
                match event {
                    Event::Key(key)
                        if key.kind != KeyEventKind::Release
                            && (key.code == KeyCode::Esc
                                || key.code == KeyCode::Char('q')
                                    && key.modifiers.contains(KeyModifiers::CONTROL)) =>
                    {
                        return Ok(());
                    }
                    Event::Resize(cols, rows) => {
                        size = (cols, rows);
                        resize_native(socket, &snapshot, &active, size)?;
                        screen = Value::Null;
                    }
                    _ => {}
                }
                continue;
            }
            if let Event::Mouse(mouse) = &event
                && authority.mouse_consumed(mouse.kind)
            {
                continue;
            }
            let (consume_warning, reset_warning) =
                authority.warning_event(&active, &event, &mut status);
            if reset_warning {
                prefix_pending = false;
                pending_parent = None;
            }
            if consume_warning {
                picker = active.is_empty();
                query.clear();
                selection = None;
                continue;
            }
            if let Some(dialog) = launch_dialog.as_mut() {
                match &event {
                    Event::Resize(cols, rows) => {
                        size = (*cols, *rows);
                        authority.panel_event(&event, &mut status);
                    }
                    Event::FocusGained => {
                        focused = true;
                        reported_focus = Some(true);
                    }
                    Event::FocusLost => {
                        focused = false;
                        reported_focus = Some(false);
                    }
                    Event::Paste(_)
                    | Event::Key(KeyEvent {
                        kind: KeyEventKind::Press | KeyEventKind::Repeat,
                        ..
                    }) => authority.activity(&active, &mut status),
                    _ => {}
                }
                match dialog.event(&event) {
                    launch::Action::None => {}
                    launch::Action::Cancel => launch_dialog = None,
                    launch::Action::Submit(request) => {
                        launch_dialog = None;
                        authority.launch(request, &mut status);
                    }
                }
                continue;
            }
            if authority.launching() {
                match event {
                    Event::Key(key)
                        if key.kind != KeyEventKind::Release
                            && key.code == KeyCode::Char('q')
                            && key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        break;
                    }
                    Event::Resize(cols, rows) => size = (cols, rows),
                    Event::FocusGained => {
                        focused = true;
                        reported_focus = Some(true);
                    }
                    Event::FocusLost => {
                        focused = false;
                        reported_focus = Some(false);
                    }
                    _ => {}
                }
                continue;
            }
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
            if picker
                && !previous_prefix
                && matches!(&event, Event::Key(key)
            if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('a')
                && key.modifiers.contains(KeyModifiers::CONTROL))
            {
                authority.activity(&active, &mut status);
                let source = rows
                    .get(selected)
                    .map(|row| row.id.as_str())
                    .unwrap_or(&active);
                match call(socket, json!({"op":"acp-profiles"}))
                    .and_then(|catalog| launch::Dialog::new(&snapshot, source, catalog))
                {
                    Ok(dialog) => launch_dialog = Some(dialog),
                    Err(error) => {
                        if crate::client::is_connection_error(&error) {
                            return Err(error);
                        }
                        status = format!("{error:#}");
                    }
                }
                continue;
            }
            if let Event::Key(key) = &event
                && key.kind != KeyEventKind::Release
                && previous_prefix
                && key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('l' | 'f'))
            {
                picker = false;
                prefix_pending = false;
                pending_parent = None;
                authority.open(&active, key.code == KeyCode::Char('f'), &mut status);
                continue;
            }
            if authority.panel_event(&event, &mut status) {
                continue;
            }
            let prefix_event = matches!(&event, Event::Key(key)
            if key.kind != KeyEventKind::Release && is_prefix(*key, prefix));
            if is_agent && !picker && !prefix_event {
                match &event {
                    Event::Resize(cols, rows) => size = (*cols, *rows),
                    Event::FocusGained => {
                        focused = true;
                        reported_focus = Some(true);
                    }
                    Event::FocusLost => {
                        focused = false;
                        reported_focus = Some(false);
                    }
                    Event::Mouse(mouse)
                        if view::Geometry::hit(geometry.switch, mouse.column, mouse.row)
                            && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) =>
                    {
                        authority.activity(&active, &mut status);
                        authority.suppress_mouse(mouse.kind);
                        picker = true;
                        query.clear();
                        pending_parent = None;
                        selected = rows.iter().position(|row| row.id == active).unwrap_or(0);
                    }
                    _ => {
                        if matches!(
                            &event,
                            Event::Paste(_)
                                | Event::Key(KeyEvent {
                                    kind: KeyEventKind::Press | KeyEventKind::Repeat,
                                    ..
                                })
                        ) || matches!(&event, Event::Mouse(mouse) if !matches!(mouse.kind, MouseEventKind::Moved))
                        {
                            authority.activity(&active, &mut status);
                        }
                        if let Some(request) = agent.event(&event)
                            && let Err(rejected) = authority.acp_action(request, &mut status)
                        {
                            agent.sync(&agent_projection, authority.acp_grant(&active));
                            agent.restore_unadmitted(rejected);
                        }
                    }
                }
                // None means consumed/local navigation, never raw PTY fallback.
                continue;
            }
            match event {
                Event::Resize(cols, rows) => {
                    size = (cols, rows);
                    // A picker is an overlay: even foreground resize is deferred until it closes.
                    if !picker {
                        resize_native(socket, &snapshot, &active, size)?;
                    }
                }
                Event::FocusGained => {
                    focused = true;
                    reported_focus = Some(true);
                }
                Event::FocusLost => {
                    focused = false;
                    reported_focus = Some(false);
                }
                Event::Paste(paste) => {
                    if picker {
                        authority.activity(&active, &mut status);
                        query.push_str(&paste.replace(['\n', '\r'], " "));
                        selected = 0;
                    } else {
                        offset = 0;
                        authority.input(&active, Input::Paste(paste), &mut status);
                    }
                }
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    if is_prefix(key, prefix) {
                        if picker && previous_prefix && !active.is_empty() {
                            picker = false;
                            resize_native(socket, &snapshot, &active, size)?;
                            if !is_agent {
                                authority.input(
                                    &active,
                                    Input::Bytes(key_bytes(
                                        prefix,
                                        mode(&screen, "application_cursor"),
                                    )),
                                    &mut status,
                                );
                            }
                        } else {
                            authority.activity(&active, &mut status);
                            picker = true;
                            prefix_pending = true;
                            query.clear();
                            selected = rows.iter().position(|r| r.id == active).unwrap_or(0);
                            pending_parent = None;
                        }
                        continue;
                    }
                    if picker {
                        authority.activity(&active, &mut status);
                        let chosen = rows.get(selected).map(|r| r.id.clone());
                        let control = key.modifiers.contains(KeyModifiers::CONTROL);
                        let empty = active.is_empty()
                            && snapshot["terminals"]
                                .as_array()
                                .is_none_or(|terminals| terminals.is_empty());
                        let action = match key.code {
                            KeyCode::Char('q') if control || empty => break,
                            KeyCode::Esc if empty => break,
                            KeyCode::Enter if empty => Some(open_window("", false)),
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
                                        if agent_view(&snapshot, &id).is_some() {
                                            status = "ACP views are not PTY parents".into();
                                            None
                                        } else {
                                            Some(
                                                json!({"op":if demote {"demote"} else {"reparent"},
                                            "terminal":child,"parent":id}),
                                            )
                                        }
                                    } else {
                                        active = id;
                                        Resume::select(resume, &snapshot, &active);
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
                            KeyCode::Char('n') if control || empty => Some(open_from_view(
                                &snapshot,
                                chosen.as_deref().unwrap_or(&active),
                                false,
                            )),
                            KeyCode::Char('o') if control => Some(open_from_view(
                                &snapshot,
                                chosen.as_deref().unwrap_or(&active),
                                true,
                            )),
                            KeyCode::Char('p') if control => chosen
                                .filter(|id| agent_view(&snapshot, id).is_none())
                                .map(|id| json!({"op":"promote","terminal":id})),
                            KeyCode::Char('x') if control => chosen.and_then(|id| {
                                if close_agent(&mut authority, &snapshot, &id, &mut status) {
                                    None
                                } else {
                                    Some(close_window(&snapshot, &id))
                                }
                            }),
                            KeyCode::Char('r' | 'd') if control => {
                                if let Some(id) = chosen {
                                    if agent_view(&snapshot, &id).is_some() {
                                        status = "ACP views are not PTY parents or children".into();
                                        continue;
                                    }
                                    pending_parent = Some((id, key.code == KeyCode::Char('d')));
                                    status =
                                        "Select new parent and press Enter; Esc cancels".into();
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
                            match apply_window(socket, request, &mut active, &mut snapshot, resume)
                            {
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
                                Err(error) => {
                                    if crate::client::is_connection_error(&error) {
                                        return Err(error);
                                    }
                                    status = format!("{error:#}");
                                }
                            }
                        }
                        if !picker {
                            resize_native(socket, &snapshot, &active, size)?;
                        }
                    } else if window_exited(&snapshot, &active) {
                        let request = match key.code {
                            KeyCode::Char('n') | KeyCode::Enter => {
                                Some(open_window(&active, false))
                            }
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
                            match apply_window(socket, request, &mut active, &mut snapshot, resume)
                            {
                                Ok(()) => {
                                    picker = active.is_empty();
                                    screen = Value::Null;
                                    offset = 0;
                                    selection = None;
                                    status.clear();
                                    if !picker {
                                        resize_native(socket, &snapshot, &active, size)?;
                                    }
                                }
                                Err(error) => {
                                    if crate::client::is_connection_error(&error) {
                                        return Err(error);
                                    }
                                    status = format!("{error:#}");
                                }
                            }
                        }
                    } else if key.modifiers.contains(KeyModifiers::SHIFT)
                        && matches!(key.code, KeyCode::PageUp | KeyCode::PageDown)
                    {
                        authority.activity(&active, &mut status);
                        let page = size.1.saturating_sub(HEADER) as usize;
                        offset = if key.code == KeyCode::PageUp {
                            (offset + page)
                                .min(screen["history_size"].as_u64().unwrap_or(0) as usize)
                        } else {
                            offset.saturating_sub(page)
                        };
                    } else {
                        offset = 0;
                        let bytes = key_bytes(key, mode(&screen, "application_cursor"));
                        if !bytes.is_empty() {
                            authority.input(&active, Input::Bytes(bytes), &mut status);
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    let application_mouse = mode(&screen, "mouse_click")
                        || mode(&screen, "mouse_drag")
                        || mode(&screen, "mouse_motion");
                    if picker || mouse.row < HEADER {
                        authority.suppress_mouse(mouse.kind);
                    }
                    if (picker
                        || mouse.row < HEADER
                        || !application_mouse
                        || mouse.modifiers.contains(KeyModifiers::SHIFT)
                        || selection.is_some()
                        || offset != 0)
                        && !matches!(mouse.kind, MouseEventKind::Moved)
                    {
                        authority.activity(&active, &mut status);
                    }
                    if view::Geometry::hit(geometry.switch, mouse.column, mouse.row)
                        && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                    {
                        if picker && active.is_empty() {
                            break;
                        }
                        picker = !picker || active.is_empty();
                        query.clear();
                        pending_parent = None;
                        selected = rows.iter().position(|r| r.id == active).unwrap_or(0);
                        status.clear();
                        if !picker {
                            resize_native(socket, &snapshot, &active, size)?;
                        }
                    } else if picker {
                        let empty = active.is_empty()
                            && snapshot["terminals"]
                                .as_array()
                                .is_none_or(|terminals| terminals.is_empty());
                        if empty && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                            if view::Geometry::hit(geometry.close, mouse.column, mouse.row) {
                                break;
                            }
                            if view::Geometry::hit(geometry.new, mouse.column, mouse.row) {
                                match apply_window(
                                    socket,
                                    open_window("", false),
                                    &mut active,
                                    &mut snapshot,
                                    resume,
                                ) {
                                    Ok(()) => {
                                        picker = false;
                                        screen = Value::Null;
                                        offset = 0;
                                        selection = None;
                                        status.clear();
                                        resize_native(socket, &snapshot, &active, size)?;
                                    }
                                    Err(error) => {
                                        if crate::client::is_connection_error(&error) {
                                            return Err(error);
                                        }
                                        status = format!("{error:#}");
                                    }
                                }
                            }
                            continue;
                        }
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
                                    if agent_view(&snapshot, &row.id).is_some() {
                                        status = "ACP views are not PTY parents".into();
                                        continue;
                                    }
                                    match call(
                                        socket,
                                        json!({"op":if demote {"demote"} else {"reparent"},"terminal":child,"parent":row.id}),
                                    ) {
                                        Ok(_) => {
                                            snapshot = call(socket, json!({"op":"snapshot"}))?;
                                        }
                                        Err(error) => {
                                            if crate::client::is_connection_error(&error) {
                                                return Err(error);
                                            }
                                            status = format!("{error:#}");
                                        }
                                    }
                                } else {
                                    active = row.id.clone();
                                    Resume::select(resume, &snapshot, &active);
                                    picker = false;
                                    offset = 0;
                                    screen = Value::Null;
                                    selection = None;
                                    status.clear();
                                    resize_native(socket, &snapshot, &active, size)?;
                                }
                            }
                        }
                    } else if window_exited(&snapshot, &active) {
                        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                            let request =
                                if view::Geometry::hit(geometry.new, mouse.column, mouse.row) {
                                    Some(open_window(&active, false))
                                } else if view::Geometry::hit(
                                    geometry.close,
                                    mouse.column,
                                    mouse.row,
                                ) {
                                    Some(close_window(&snapshot, &active))
                                } else {
                                    None
                                };
                            if let Some(request) = request {
                                match apply_window(
                                    socket,
                                    request,
                                    &mut active,
                                    &mut snapshot,
                                    resume,
                                ) {
                                    Ok(()) => {
                                        picker = active.is_empty();
                                        screen = Value::Null;
                                        offset = 0;
                                        selection = None;
                                        status.clear();
                                        if !picker {
                                            resize_native(socket, &snapshot, &active, size)?;
                                        }
                                    }
                                    Err(error) => {
                                        if crate::client::is_connection_error(&error) {
                                            return Err(error);
                                        }
                                        status = format!("{error:#}");
                                    }
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
                            && selection.is_none()
                        {
                            let bytes = mouse_bytes(mouse, &screen);
                            if !bytes.is_empty() {
                                authority.application_mouse(
                                    &active,
                                    mouse.kind,
                                    bytes,
                                    &mut status,
                                );
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
    })();
    // Cancel before any local projection/draft destructors and before joining:
    // the old worker may not drain queued authority after observation loss.
    authority.stop();
    if let Some(opened) = authority.take_launched() {
        *resume = Some(Resume::launched(&snapshot, &opened)?);
        if outcome
            .as_ref()
            .is_err_and(|error| error.downcast_ref::<AttachmentEnded>().is_some())
        {
            // An old view's Ended classification cannot decide the fate of a
            // different acknowledged launch. Validate its exact target fresh.
            return Err(crate::client::connection_lost(
                "acknowledged ACP launch requires fresh continuity validation",
            ));
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projected_native(
        screen: &Value,
        surface: &view::NativeSurface,
        width: u16,
        selection: Option<Selection>,
    ) -> ratatui::buffer::Buffer {
        let mut display = ratatui::Terminal::new(ratatui::backend::TestBackend::new(
            width,
            screen["rows"].as_u64().unwrap() as u16 + HEADER,
        ))
        .unwrap();
        let snapshot = json!({"terminals":[{"id":"native","alive":true}],"projects":[]});
        display
            .draw(|frame| {
                view::draw(
                    frame,
                    screen,
                    surface,
                    &snapshot,
                    "native",
                    false,
                    &[],
                    0,
                    "",
                    "",
                    0,
                    true,
                    selection,
                );
            })
            .unwrap();
        display.backend().buffer().clone()
    }

    #[test]
    fn native_row_updates_preserve_untouched_styles_and_update_cursor_and_modes() {
        let mut screen = json!({
            "cols":2, "rows":2, "offset":0, "generation":10,
            "cells":[[{"text":"界","wide":true},{"text":"","wide_spacer":true}],
                [{"text":"x","fg":[1,2,3],"bold":true},{"text":" "}]],
            "lines":["界","x"],
            "cursor":{"row":1,"col":1,"visible":true},
            "modes":{"application_cursor":false}
        });
        let mut surface = view::NativeSurface::default();
        surface.update(&screen);
        merge_screen(
            &mut screen,
            json!({
                "cols":2, "rows":2, "offset":0, "generation":12,
                "delta":true, "base_generation":10, "row_indices":[0],
                "cells":[[{"text":"é","italic":true},{"text":"!"}]], "lines":["é!"],
                "cursor":{"row":0,"col":1,"visible":false},
                "modes":{"application_cursor":true}
            }),
            &mut surface,
        )
        .unwrap();
        assert_eq!(
            screen["cells"][0],
            json!([{"text":"é","italic":true},{"text":"!"}])
        );
        assert_eq!(
            screen["cells"][1],
            json!([{"text":"x","fg":[1,2,3],"bold":true},{"text":" "}])
        );
        assert_eq!(screen["lines"], json!(["é!", "x"]));
        assert_eq!(screen["cursor"], json!({"row":0,"col":1,"visible":false}));
        assert!(mode(&screen, "application_cursor"));
        assert_eq!(screen["generation"], 12);
        let projected = projected_native(&screen, &surface, 2, None);
        assert_eq!(projected[(0, HEADER)].symbol(), "é");
        assert!(
            projected[(0, HEADER)]
                .modifier
                .contains(ratatui::style::Modifier::ITALIC)
        );
        assert_eq!(
            projected[(0, HEADER + 1)].fg,
            ratatui::style::Color::Rgb(1, 2, 3)
        );
        assert!(
            projected[(0, HEADER + 1)]
                .modifier
                .contains(ratatui::style::Modifier::BOLD)
        );

        // A mode-only update must not erase rows when there is no cell damage.
        merge_screen(
            &mut screen,
            json!({
                "cols":2, "rows":2, "offset":0, "generation":13,
                "delta":true, "base_generation":12, "row_indices":[],
                "cells":[], "lines":[],
                "cursor":{"row":0,"col":1,"visible":true},
                "modes":{"application_cursor":false}
            }),
            &mut surface,
        )
        .unwrap();
        assert_eq!(screen["lines"], json!(["é!", "x"]));
        assert_eq!(screen["cells"][1][0]["fg"], json!([1, 2, 3]));
        assert!(!mode(&screen, "application_cursor"));
        assert_eq!(projected_native(&screen, &surface, 2, None), projected);
        let selected = projected_native(&screen, &surface, 2, Some(((0, 1), (0, 1))));
        assert_eq!(selected[(0, HEADER + 1)].fg, ratatui::style::Color::White);
        assert_eq!(projected_native(&screen, &surface, 2, None), projected);

        // Full frames clear old styling and a narrower client must not wrap a wide cell.
        merge_screen(
            &mut screen,
            json!({
                "cols":3,"rows":1,"offset":0,"generation":14,
                "cells":[[{"text":"a"},{"text":"界","wide":true},{"text":"","wide_spacer":true}]],
                "lines":["a界"],"cursor":{"visible":false}
            }),
            &mut surface,
        )
        .unwrap();
        let clipped = projected_native(&screen, &surface, 2, None);
        assert_eq!(clipped[(0, HEADER)].symbol(), "a");
        assert_eq!(
            clipped[(0, HEADER)].modifier,
            ratatui::style::Modifier::empty()
        );
        assert_eq!(clipped[(1, HEADER)].symbol(), " ");
        merge_screen(
            &mut screen,
            json!({
                "cols":2,"rows":1,"offset":0,"generation":15,
                "cells":[[{"text":"b"},{"text":"c"}]],"lines":["bc"],"cursor":{"visible":false}
            }),
            &mut surface,
        )
        .unwrap();
        let resized = projected_native(&screen, &surface, 2, None);
        assert_eq!(resized[(0, HEADER)].symbol(), "b");
        assert_eq!(resized[(1, HEADER)].symbol(), "c");
        assert_eq!(resized[(0, HEADER)].fg, ratatui::style::Color::Reset);
    }

    #[test]
    fn stale_or_out_of_viewport_updates_cannot_replace_displayed_native_rows() {
        let base = json!({
            "cols":1, "rows":1, "offset":0, "generation":10,
            "cells":[[{"text":"a"}]], "lines":["a"]
        });
        for (generation, row) in [(9, 0), (10, 1)] {
            let mut screen = base.clone();
            let mut surface = view::NativeSurface::default();
            surface.update(&screen);
            let result = merge_screen(
                &mut screen,
                json!({
                    "cols":1, "rows":1, "offset":0, "generation":11,
                    "delta":true, "base_generation":generation, "row_indices":[row],
                    "cells":[[{"text":"b"}]], "lines":["b"]
                }),
                &mut surface,
            );
            assert!(result.is_err());
            assert_eq!(screen, base);
            assert_eq!(
                projected_native(&screen, &surface, 1, None)[(0, HEADER)].symbol(),
                "a"
            );
        }
    }
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

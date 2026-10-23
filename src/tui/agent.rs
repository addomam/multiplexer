//! Human ACP projection/editor. The parent consumes every active-ACP event,
//! including `None`, and dispatches returned envelopes unchanged. This module
//! neither acquires authority nor forwards input to a native terminal.
//!
//! Enter captures a prompt; Alt/Shift-Enter inserts a newline. F8 explicitly
//! chooses the highlighted advertised agent authentication method. Ctrl-C
//! captures the exact foreground prompt ID, never a generic process signal.
//! Up/Down selects auth methods during auth-required, otherwise scrolls history;
//! PageUp/PageDown, Ctrl-Home/End and the wheel are read-only history navigation.
//! Left/Right, Home/End and Backspace/Delete edit the grapheme-aware local draft.
//!
//! First edit captures the complete target tuple and original grant. Changed
//! identity/authority discards the draft; no action is retried or rebound.
//! Only sequenced canonical events enter history. The cache holds at most
//! 2048 events / 4 MiB, with explicit eviction and per-event clipping notices.
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use serde_json::{Value, json};
use std::{borrow::Cow, collections::VecDeque, fmt::Write as _, io, ops::Range};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const PROMPT_BYTES: usize = 64 * 1024;
const HISTORY_BYTES: usize = 4 * 1024 * 1024;
const HISTORY_EVENTS: usize = 2048;
const EVENT_BYTES: usize = 64 * 1024;
const AUTH_METHODS: usize = 64;
const MUTED: Color = Color::Rgb(149, 163, 184);
const ACCENT: Color = Color::Rgb(103, 213, 224);

#[derive(Clone, PartialEq)]
struct Binding {
    target: Value,
    control: Value,
    generation: u64,
}

impl Binding {
    fn action(&self, op: &str) -> Value {
        json!({
            "version": 1,
            "op": op,
            "target": self.target,
            "control": self.control,
            "owner_generation": self.generation,
            "call_id": uuid::Uuid::new_v4().to_string(),
        })
    }
}

struct Entry {
    seq: u64,
    kind: &'static str,
    text: String,
    rows: Vec<Range<usize>>,
}

#[derive(PartialEq)]
struct AuthMethod {
    id: String,
    kind: String,
    label: String,
}

pub(super) struct AgentView {
    target: Option<Value>,
    binding: Option<Binding>,
    projection_seq: u64,
    lifecycle: &'static str,
    agent: String,
    foreground: Option<String>,
    foreground_state: String,
    authentication: Vec<AuthMethod>,
    auth_selected: usize,
    auth_clipped: bool,
    work: Vec<String>,
    resource: String,
    history: VecDeque<Entry>,
    history_bytes: usize,
    history_rows: usize,
    history_width: usize,
    through: u64,
    broker_gap: String,
    local_gap: Option<u64>,
    scroll: usize,
    page_rows: usize,
    draft: String,
    draft_binding: Option<Binding>,
    cursor: usize,
    editor: String,
    editor_cursor: usize,
    editor_rows: Vec<Range<usize>>,
    editor_width: usize,
    notice: &'static str,
}

impl AgentView {
    pub(super) fn new() -> Self {
        Self {
            target: None,
            binding: None,
            projection_seq: 0,
            lifecycle: "starting",
            agent: String::new(),
            foreground: None,
            foreground_state: String::new(),
            authentication: Vec::new(),
            auth_selected: 0,
            auth_clipped: false,
            work: Vec::new(),
            resource: String::new(),
            history: VecDeque::new(),
            history_bytes: 0,
            history_rows: 0,
            history_width: 80,
            through: 0,
            broker_gap: String::new(),
            local_gap: None,
            scroll: 0,
            page_rows: 1,
            draft: String::new(),
            draft_binding: None,
            cursor: 0,
            editor: String::new(),
            editor_cursor: 0,
            editor_rows: wrap_rows("", 80),
            editor_width: 80,
            notice: "Canonical history only; navigation does not acquire control.",
        }
    }

    pub(super) fn sync(&mut self, projection: &Value, grant: Option<&Value>) {
        let target = exact_target(&projection["target"]);
        let binding = target.as_ref().and_then(|target| {
            let grant = grant?;
            let control = &grant["control"];
            Some(Binding {
                target: target.clone(),
                control: json!({
                    "kind": identifier(&control["kind"])?,
                    "ref": identifier(&control["ref"])?,
                }),
                generation: grant["owner_generation"].as_u64()?,
            })
        });
        let changed = self.target != target;
        // Bootstrap events remain canonical when this same connection gains its
        // first session. Drafts still use the full tuple and are discarded below.
        let established = self.target.as_ref().zip(target.as_ref()).is_some_and(|(old, new)| {
            old["session_id"].is_null() && new["session_id"].is_string()
                && old["id"] == new["id"]
                && old["instance_id"] == new["instance_id"]
                && old["connection_generation"] == new["connection_generation"]
        });
        if self.draft_binding.is_some() && self.draft_binding != binding {
            self.clear_draft();
            self.notice = "Draft discarded: exact target/session or original authority changed. Nothing replayed.";
        }
        self.binding = binding;
        if changed {
            if !established {
                self.reset_history();
            }
            self.authentication.clear();
            self.auth_selected = 0;
            self.target = target;
            self.projection_seq = 0;
        }
        if self.target.is_none() {
            self.lifecycle = "unavailable";
            self.agent.clear();
            self.foreground = None;
            self.foreground_state.clear();
            self.authentication.clear();
            self.work.clear();
            self.resource.clear();
            return;
        }
        // A delayed inspection cannot roll canonical lifecycle/content backwards.
        // Authority loss above still invalidates drafts, even on an older page.
        let seq = projection["seq"].as_u64().unwrap_or(0);
        if !changed && seq < self.projection_seq {
            return;
        }
        self.projection_seq = seq;
        self.lifecycle = match projection["lifecycle"].as_str() {
            Some("starting") => "starting",
            Some("auth-required") => "auth-required",
            Some("ready") => "ready",
            Some("failed") => "failed",
            Some("closing") => "closing",
            Some("ended") => "ended",
            _ => "unavailable",
        };
        if self.lifecycle != "ready" && self.draft_binding.is_some() {
            self.clear_draft();
            self.notice = "Draft discarded: session is no longer ready. Nothing submitted.";
        }
        self.foreground = identifier(&projection["foreground"]["prompt_id"]).map(str::to_owned);
        self.foreground_state = display_value(&projection["foreground"]["state"], 128);
        let content = &projection["content"];
        let agent = content.get("agent").unwrap_or(&projection["agent"]);
        self.agent = display_value(agent, 512);
        let auth = content.get("authentication").unwrap_or(&projection["authentication"]);
        self.sync_auth(auth);
        self.work.clear();
        if let Some(work) = projection["work"].as_array() {
            // These are canonical dispositions, not optimistic local receipts.
            for record in work.iter().rev().take(3).rev() {
                let mut summary = String::from("Canonical work");
                // Put actual dispositions before IDs so narrow panes do not
                // hide an unknown/submitted outcome behind long provenance.
                for key in ["state", "delivery", "outcome"] {
                    if let Some(value) = record.get(key) {
                        let _ = write!(summary, " | {key}={}", display_value(value, 128));
                    }
                }
                let _ = write!(summary, " | {}", display_value(record, 512));
                self.work.push(summary);
            }
        }
        self.resource.clear();
        for key in ["resource", "resource_failure", "truncation"] {
            if let Some(value) = projection.get(key).filter(|v| !v.is_null()) {
                let _ = write!(self.resource, "{key}: {} ", display_value(value, 256));
            }
        }
        self.sync_content(content);
    }

    fn sync_auth(&mut self, value: &Value) {
        let methods = value.as_array();
        self.auth_clipped = methods.is_some_and(|methods| methods.len() > AUTH_METHODS);
        let mut next = Vec::new();
        if let Some(methods) = methods {
            for method in methods.iter().take(AUTH_METHODS) {
                let Some(id) = identifier(&method["method_id"]) else { continue };
                let Some(kind) = identifier(&method["kind"]) else { continue };
                next.push(AuthMethod {
                    id: id.to_owned(),
                    kind: kind.to_owned(),
                    label: display_value(&method["name"], 256),
                });
            }
        }
        if next != self.authentication {
            // Never preserve an index that now denotes a different method.
            let selected = self.authentication.get(self.auth_selected);
            self.auth_selected = selected
                .and_then(|old| next.iter().position(|method| method == old))
                .unwrap_or(0);
            self.authentication = next;
        }
    }

    fn sync_content(&mut self, content: &Value) {
        let Some(through) = content["through"].as_u64() else { return };
        if through < self.through {
            return;
        }
        if let Some(gap) = content.get("gap").filter(|v| !v.is_null() && **v != false) {
            self.broker_gap = format!("Canonical gap: {}", display_value(gap, 256));
        }
        if content["after"].as_u64().is_some_and(|after| after > self.through) {
            self.broker_gap = "Projection cursor skipped unseen history; continuity is not claimed.".to_owned();
        }
        let Some(events) = content["events"].as_array() else { return };
        for event in events.iter().take(128) {
            let Some(seq) = event["seq"].as_u64() else { continue };
            if seq <= self.through || seq > through {
                continue;
            }
            let kind = match event["kind"].as_str() {
                Some("user-message") => "user-message",
                Some("agent-message") => "agent-message",
                Some("thought") => "thought",
                Some("tool") => "tool",
                Some("plan") => "plan",
                Some("completion") => "completion",
                Some("error") => "error",
                _ => "unknown-event",
            };
            let text = event_text(event, seq, kind);
            let rows = wrap_rows(&text, self.history_width.max(1));
            let row_count = rows.len();
            self.history_bytes += text.len();
            self.history_rows += row_count;
            if self.scroll > 0 {
                self.scroll = self.scroll.saturating_add(row_count);
            }
            self.history.push_back(Entry { seq, kind, text, rows });
            self.through = seq;
            while self.history.len() > HISTORY_EVENTS || self.history_bytes > HISTORY_BYTES {
                if let Some(old) = self.history.pop_front() {
                    self.history_bytes -= old.text.len();
                    self.history_rows -= old.rows.len();
                    self.local_gap = Some(old.seq);
                }
            }
        }
        if events.len() > 128 {
            self.broker_gap = "Projection page exceeds 128 events; excess content was not cached.".to_owned();
        } else {
            self.through = through;
        }
        self.clamp_scroll();
    }

    /// None always means consumed/local, never permission for native forwarding.
    pub(super) fn event(&mut self, event: &Event) -> Option<Value> {
        match event {
            Event::Paste(text) => {
                self.insert(text);
                None
            }
            Event::Mouse(mouse) => {
                match mouse.kind {
                    MouseEventKind::ScrollUp => self.scroll_up(3),
                    MouseEventKind::ScrollDown => self.scroll_down(3),
                    _ => {}
                }
                None
            }
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                let control = key.modifiers.contains(KeyModifiers::CONTROL);
                let alt = key.modifiers.contains(KeyModifiers::ALT);
                // Key repeats may edit/navigate, but cannot create mutations.
                let deliberate = key.kind == KeyEventKind::Press;
                match key.code {
                    KeyCode::Char('c' | 'C') if key.modifiers == KeyModifiers::CONTROL && deliberate => self.interrupt(),
                    KeyCode::F(8) if key.modifiers.is_empty() && deliberate => self.authenticate(),
                    KeyCode::Enter if key.modifiers == KeyModifiers::ALT || key.modifiers == KeyModifiers::SHIFT => {
                        self.insert("\n");
                        None
                    }
                    KeyCode::Enter if key.modifiers.is_empty() && deliberate => self.submit(),
                    KeyCode::PageUp => { self.scroll_up(self.page_rows); None }
                    KeyCode::PageDown => { self.scroll_down(self.page_rows); None }
                    KeyCode::Home if control => { self.scroll = self.history_rows; self.clamp_scroll(); None }
                    KeyCode::End if control => { self.scroll = 0; None }
                    KeyCode::Up | KeyCode::Down if self.lifecycle == "auth-required" => {
                        if !self.authentication.is_empty() {
                            if key.code == KeyCode::Up {
                                self.auth_selected = self.auth_selected.saturating_sub(1);
                            } else {
                                self.auth_selected = (self.auth_selected + 1).min(self.authentication.len() - 1);
                            }
                        }
                        None
                    }
                    KeyCode::Up => { self.scroll_up(1); None }
                    KeyCode::Down => { self.scroll_down(1); None }
                    KeyCode::Left if !control && !alt => {
                        self.cursor = previous_boundary(&self.draft, self.cursor);
                        self.refresh_editor();
                        None
                    }
                    KeyCode::Right if !control && !alt => {
                        self.cursor = next_boundary(&self.draft, self.cursor);
                        self.refresh_editor();
                        None
                    }
                    KeyCode::Home if !alt => {
                        self.cursor = self.draft[..self.cursor].rfind('\n').map_or(0, |i| i + 1);
                        self.refresh_editor();
                        None
                    }
                    KeyCode::End if !alt => {
                        self.cursor += self.draft[self.cursor..].find('\n').unwrap_or(self.draft.len() - self.cursor);
                        self.refresh_editor();
                        None
                    }
                    KeyCode::Backspace if !control && !alt => {
                        if self.cursor > 0 && self.editable() {
                            let previous = previous_boundary(&self.draft, self.cursor);
                            self.draft.replace_range(previous..self.cursor, "");
                            self.cursor = previous;
                            self.refresh_editor();
                        }
                        None
                    }
                    KeyCode::Delete if !control && !alt => {
                        if self.cursor < self.draft.len() && self.editable() {
                            let next = next_boundary(&self.draft, self.cursor);
                            self.draft.replace_range(self.cursor..next, "");
                            self.refresh_editor();
                        }
                        None
                    }
                    KeyCode::Char(c) if !control && !alt && !c.is_control() => {
                        let mut bytes = [0; 4];
                        self.insert(c.encode_utf8(&mut bytes));
                        None
                    }
                    _ => None,
                }
            }
            // Resize/focus/mouse/escape never imply cancel, deny, or input replay.
            _ => None,
        }
    }

    fn editable(&mut self) -> bool {
        if self.binding.is_none() {
            self.notice = "Observer: editing and actions require current explicit control; navigation is read-only.";
            return false;
        }
        if self.lifecycle != "ready" || self.target.as_ref().is_none_or(|t| t["session_id"].is_null()) {
            self.notice = "Session is not ready for a prompt; no content submitted.";
            return false;
        }
        if self.draft_binding.is_none() {
            // First edit is the only binding point. sync discards it on mismatch.
            self.draft_binding = self.binding.clone();
        }
        self.draft_binding == self.binding
    }

    fn insert(&mut self, text: &str) {
        if text.is_empty() || !self.editable() {
            return;
        }
        // Reject the whole paste rather than silently submit a truncated prompt.
        // CRLF normalization is local editing only, never an Enter action.
        if self.draft.len().saturating_add(text.len()) > PROMPT_BYTES {
            self.notice = "Prompt limit is 64 KiB UTF-8; edit/paste rejected in full.";
            return;
        }
        let normalized = normalize_newlines(text);
        self.draft.insert_str(self.cursor, &normalized);
        self.cursor += normalized.len();
        self.refresh_editor();
        self.notice = "Local draft only; Enter submits deliberately. Paste never submits.";
    }

    fn submit(&mut self) -> Option<Value> {
        if self.draft.is_empty() {
            self.notice = "No prompt draft to submit. Authentication requires explicit F8.";
            return None;
        }
        if self.lifecycle != "ready" || self.draft_binding.is_none()
            || self.draft_binding != self.binding
        {
            self.notice = "Draft has no matching original ready-session authority; nothing submitted.";
            return None;
        }
        if self.foreground.is_some() {
            self.notice = "Foreground prompt is busy; draft retained, not queued or submitted.";
            return None;
        }
        let mut action = self.draft_binding.as_ref()?.action("acp-prompt");
        action["text"] = Value::String(std::mem::take(&mut self.draft));
        self.clear_draft();
        self.notice = "Prompt action captured, not yet admitted. Only canonical history records the outcome.";
        Some(action)
    }

    fn authenticate(&mut self) -> Option<Value> {
        if self.lifecycle != "auth-required" {
            self.notice = "Authentication is not currently required; no action sent.";
            return None;
        }
        let Some(binding) = &self.binding else {
            self.notice = "Observer: authentication requires current explicit control.";
            return None;
        };
        let Some(method) = self.authentication.get(self.auth_selected) else {
            self.notice = "No advertised authentication method is available.";
            return None;
        };
        if method.kind != "agent" {
            self.notice = "Only advertised agent-type authentication is supported; no URL, terminal or credentials opened.";
            return None;
        }
        let mut action = binding.action("acp-authenticate");
        action["method_id"] = Value::String(method.id.clone());
        self.notice = "Explicit authentication choice captured; canonical result is pending.";
        Some(action)
    }

    fn interrupt(&mut self) -> Option<Value> {
        if self.lifecycle != "ready" {
            self.notice = "Session is not ready; no foreground interrupt action sent.";
            return None;
        }
        let Some(binding) = &self.binding else {
            self.notice = "Observer: interrupt requires current explicit control.";
            return None;
        };
        let Some(prompt) = &self.foreground else {
            self.notice = "No exact foreground prompt to interrupt; no action sent.";
            return None;
        };
        let mut action = binding.action("acp-interrupt");
        action["prompt_id"] = Value::String(prompt.clone());
        self.notice = "Exact foreground interrupt requested; not confirmation of cancellation or tool termination.";
        Some(action)
    }

    fn clear_draft(&mut self) {
        self.draft.clear();
        self.draft_binding = None;
        self.cursor = 0;
        self.refresh_editor();
    }

    fn refresh_editor(&mut self) {
        // Deletion/insertion can join graphemes across the edit point. Keep
        // both raw slices and future cursor operations on a complete boundary.
        self.cursor = self.draft.grapheme_indices(true)
            .map(|(i, _)| i).find(|i| *i >= self.cursor).unwrap_or(self.draft.len());
        self.editor.clear();
        push_inert(&mut self.editor, &self.draft[..self.cursor], PROMPT_BYTES * 10, true);
        self.editor_cursor = self.editor.len();
        push_inert(&mut self.editor, &self.draft[self.cursor..], PROMPT_BYTES * 10, true);
        self.layout_editor();
    }

    fn layout_editor(&mut self) {
        self.editor_rows = wrap_rows(&self.editor, self.editor_width.max(1));
        if let Some(last) = self.editor_rows.last()
            && last.start < last.end
            && UnicodeWidthStr::width(&self.editor[last.clone()]) >= self.editor_width.max(1)
        {
            // The insertion point after an exactly-full last row is a new row,
            // not a cursor painted over the last visible character.
            self.editor_rows.push(self.editor.len()..self.editor.len());
        }
    }

    fn reset_history(&mut self) {
        self.history.clear();
        self.history_bytes = 0;
        self.history_rows = 0;
        self.through = 0;
        self.broker_gap.clear();
        self.local_gap = None;
        self.scroll = 0;
    }

    fn scroll_up(&mut self, rows: usize) {
        self.scroll = self.scroll.saturating_add(rows);
        self.clamp_scroll();
    }

    fn scroll_down(&mut self, rows: usize) {
        self.scroll = self.scroll.saturating_sub(rows);
    }

    fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.min(self.history_rows.saturating_sub(self.page_rows));
    }

    pub(super) fn draw(&mut self, frame: &mut Frame<'_>, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let mut remaining = area;
        let owner = if self.binding.is_some() { "owner (input-admission)" } else { "observer (read-only)" };
        let id = self.target.as_ref().map_or(String::new(), |t| display_value(&t["id"], 256));
        let session = self.target.as_ref().map_or(String::new(), |t| display_value(&t["session_id"], 256));
        let connection = self.target.as_ref().and_then(|t| t["connection_generation"].as_u64()).unwrap_or(0);
        let header = format!("ACP | {} | {owner} | {id}\nAgent: {}\nSession: {session} | connection {connection}\nForeground: {} {}",
            self.lifecycle, self.agent,
            self.foreground.as_ref().map_or("none".to_owned(), |id| inert(id, 256)),
            self.foreground_state);
        frame.render_widget(Paragraph::new(header).style(Style::default().fg(ACCENT)), take_top(&mut remaining, 4));
        let gap = format!("Cursor {} | {}{}{}{}",
            self.through,
            if self.scroll == 0 { "following" } else { "scrollback" },
            if self.broker_gap.is_empty() { "" } else { " | " }, self.broker_gap,
            self.local_gap.map_or(String::new(), |seq| format!(" | local history evicted through {seq}")));
        frame.render_widget(Paragraph::new(gap).style(Style::default().fg(Color::Yellow)), take_top(&mut remaining, 1));
        let footer = take_bottom(&mut remaining, 2);
        let editor_width = usize::from(area.width.saturating_sub(2)).max(1);
        if self.editor_width != editor_width {
            self.editor_width = editor_width;
            self.layout_editor();
        }
        let editor_height = (self.editor_rows.len() + 2).clamp(3, 8) as u16;
        let editor = take_bottom(&mut remaining, editor_height);
        let work = take_bottom(&mut remaining, (self.work.len() + usize::from(!self.resource.is_empty())) as u16);
        let mut work_lines: Vec<Line<'_>> = self.work.iter().map(|line| Line::raw(line.as_str())).collect();
        if !self.resource.is_empty() {
            work_lines.push(Line::raw(self.resource.as_str()));
        }
        frame.render_widget(Paragraph::new(work_lines).style(Style::default().fg(MUTED)), work);
        if self.lifecycle == "auth-required" {
            let height = (self.authentication.len().min(4) + 1 + usize::from(self.auth_clipped)) as u16;
            let auth = take_top(&mut remaining, height);
            let start = self.auth_selected.saturating_sub(3);
            let mut lines = vec![Line::raw("Authentication: Up/Down selects; F8 explicitly chooses agent-type only.")];
            for (index, method) in self.authentication.iter().enumerate().skip(start).take(4) {
                let label = format!("{} {} [{}] {}", if index == self.auth_selected { ">" } else { " " },
                    inert(&method.id, 256), inert(&method.kind, 256), method.label);
                let style = if index == self.auth_selected { Style::default().fg(ACCENT).add_modifier(Modifier::BOLD) }
                    else { Style::default().fg(MUTED) };
                lines.push(Line::styled(label, style));
            }
            if self.auth_clipped {
                lines.push(Line::raw("Additional authentication methods omitted by the local display bound."));
            }
            frame.render_widget(Paragraph::new(lines), auth);
        }
        self.draw_history(frame, remaining);
        self.draw_editor(frame, editor);
        frame.render_widget(Paragraph::new(vec![
            Line::raw(self.notice),
            Line::styled("Enter submit | Alt/Shift-Enter newline | F8 auth | Ctrl-C exact interrupt | PgUp/PgDn scroll",
                Style::default().fg(MUTED)),
        ]), footer);
    }

    fn draw_history(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let width = usize::from(area.width).max(1);
        if self.history_width != width {
            self.history_width = width;
            self.history_rows = 0;
            for entry in &mut self.history {
                entry.rows = wrap_rows(&entry.text, width);
                self.history_rows += entry.rows.len();
            }
        }
        self.page_rows = usize::from(area.height).max(1);
        self.clamp_scroll();
        let start = self.history_rows.saturating_sub(self.scroll).saturating_sub(self.page_rows);
        let mut skip = start;
        let mut lines = Vec::with_capacity(self.page_rows);
        for entry in &self.history {
            if skip >= entry.rows.len() {
                skip -= entry.rows.len();
                continue;
            }
            for row in entry.rows.iter().skip(skip) {
                lines.push(Line::from(Span::styled(&entry.text[row.clone()], category_style(entry.kind))));
                if lines.len() == self.page_rows {
                    break;
                }
            }
            skip = 0;
            if lines.len() == self.page_rows {
                break;
            }
        }
        if self.history.is_empty() {
            lines.push(Line::styled("No canonical transcript content in this projection.", Style::default().fg(MUTED)));
        }
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn draw_editor(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::default().borders(Borders::ALL);
        let inner = block.inner(area);
        if inner.width == 0 || inner.height == 0 {
            frame.render_widget(block, area);
            return;
        }
        let cursor_row = self.editor_rows.iter().rposition(|row| row.start <= self.editor_cursor).unwrap_or(0);
        let start = cursor_row.saturating_sub(usize::from(inner.height) - 1);
        let title = if self.binding.is_some() {
            format!(" Local draft {}/65536 B | rows {}..{}/{}; not history ",
                self.draft.len(), start + 1,
                (start + usize::from(inner.height)).min(self.editor_rows.len()), self.editor_rows.len())
        } else {
            " Observer; no prompt authority ".to_owned()
        };
        frame.render_widget(block.title(title), area);
        let lines: Vec<Line<'_>> = self.editor_rows.iter().skip(start).take(usize::from(inner.height))
            .map(|row| Line::raw(&self.editor[row.clone()])).collect();
        frame.render_widget(Paragraph::new(lines), inner);
        if self.binding.is_some() && self.lifecycle == "ready" {
            let row = &self.editor_rows[cursor_row];
            let column = UnicodeWidthStr::width(&self.editor[row.start..self.editor_cursor.min(row.end)]);
            frame.set_cursor_position((
                inner.x + column.min(usize::from(inner.width) - 1) as u16,
                inner.y + (cursor_row - start) as u16,
            ));
        }
    }
}

fn identifier(value: &Value) -> Option<&str> {
    value.as_str().filter(|s| !s.is_empty() && s.len() <= 256)
}

fn exact_target(value: &Value) -> Option<Value> {
    if value["kind"] != "acp" {
        return None;
    }
    let session = value.get("session_id")?;
    if !session.is_null() && identifier(session).is_none() {
        return None;
    }
    Some(json!({
        "kind": "acp",
        "id": identifier(&value["id"])?,
        "instance_id": identifier(&value["instance_id"])?,
        "connection_generation": value["connection_generation"].as_u64()?,
        "session_id": session,
    }))
}

fn previous_boundary(text: &str, at: usize) -> usize {
    text[..at].grapheme_indices(true).next_back().map_or(0, |(i, _)| i)
}

fn next_boundary(text: &str, at: usize) -> usize {
    text[at..].graphemes(true).next().map_or(text.len(), |g| at + g.len())
}

fn normalize_newlines(text: &str) -> Cow<'_, str> {
    if !text.contains('\r') {
        return Cow::Borrowed(text);
    }
    let mut normalized = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            normalized.push('\n');
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
        } else {
            normalized.push(c);
        }
    }
    Cow::Owned(normalized)
}

fn wrap_rows(text: &str, width: usize) -> Vec<Range<usize>> {
    let mut rows = Vec::new();
    let mut start = 0;
    let mut used = 0;
    for (index, grapheme) in text.grapheme_indices(true) {
        if grapheme == "\n" {
            rows.push(start..index);
            start = index + 1;
            used = 0;
            continue;
        }
        let columns = UnicodeWidthStr::width(grapheme);
        if used > 0 && used + columns > width {
            rows.push(start..index);
            start = index;
            used = 0;
        }
        used += columns;
    }
    rows.push(start..text.len());
    rows
}

fn category_style(kind: &str) -> Style {
    Style::default().fg(match kind {
        "user-message" => Color::White,
        "agent-message" => ACCENT,
        "thought" => MUTED,
        "tool" => Color::Yellow,
        "plan" => Color::Blue,
        "completion" => Color::Green,
        "error" => Color::Red,
        _ => Color::Magenta,
    })
}

fn event_text(event: &Value, seq: u64, kind: &str) -> String {
    let mut text = format!("#{seq} {kind}\n");
    let mut complete = true;
    if let Some(body) = event["text"].as_str() {
        complete &= push_inert(&mut text, body, EVENT_BYTES, true);
    }
    if complete && let Some(fields) = event.as_object() {
        for (key, value) in fields {
            if matches!(key.as_str(), "seq" | "kind" | "text") || value.is_null() {
                continue;
            }
            complete = push_inert(&mut text, "\n", EVENT_BYTES, true)
                && push_inert(&mut text, key, EVENT_BYTES, false)
                && push_inert(&mut text, ": ", EVENT_BYTES, true);
            if complete {
                let remaining = EVENT_BYTES.saturating_sub(text.len());
                complete = push_inert(&mut text, &display_value(value, remaining), EVENT_BYTES, true);
            }
            if !complete {
                break;
            }
        }
    }
    if !complete {
        text.push_str("\n[event clipped by local 64 KiB display bound]");
    }
    text
}

// serde's streaming writer bounds intermediate JSON as well as the final
// sanitized display. Arbitrary canonical tool/plan details need no guessed schema.
struct LimitedJson {
    bytes: Vec<u8>,
    limit: usize,
}

impl io::Write for LimitedJson {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let count = bytes.len().min(self.limit.saturating_sub(self.bytes.len()));
        self.bytes.extend_from_slice(&bytes[..count]);
        if count == 0 && !bytes.is_empty() {
            Err(io::Error::other("display bound"))
        } else {
            Ok(count)
        }
    }

    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

fn display_value(value: &Value, limit: usize) -> String {
    if value.is_null() {
        return String::new();
    }
    if let Some(text) = value.as_str() {
        return inert(text, limit);
    }
    let mut writer = LimitedJson { bytes: Vec::new(), limit };
    let clipped = serde_json::to_writer(&mut writer, value).is_err();
    let mut display = inert(&String::from_utf8_lossy(&writer.bytes), limit);
    if clipped {
        display.push_str(" [clipped]");
    }
    display
}

fn inert(text: &str, limit: usize) -> String {
    let mut display = String::new();
    if !push_inert(&mut display, text, limit, false) {
        display.push_str(" [clipped]");
    }
    display
}

fn push_inert(output: &mut String, text: &str, limit: usize, multiline: bool) -> bool {
    for c in text.chars() {
        let escaped = (c.is_control() && !(c == '\n' && multiline) && c != '\t')
            || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{2028}' | '\u{2029}');
        let bytes = if escaped {
            4 + ((32 - (c as u32).leading_zeros()).div_ceil(4).max(1) as usize)
        } else if c == '\t' {
            4
        } else {
            c.len_utf8()
        };
        if output.len().saturating_add(bytes) > limit {
            return false;
        }
        if escaped {
            let _ = write!(output, "\\u{{{:x}}}", c as u32);
        } else if c == '\t' {
            output.push_str("    ");
        } else {
            output.push(c);
        }
    }
    true
}

fn take_top(area: &mut Rect, height: u16) -> Rect {
    let height = height.min(area.height);
    let taken = Rect::new(area.x, area.y, area.width, height);
    area.y = area.y.saturating_add(height);
    area.height -= height;
    taken
}

fn take_bottom(area: &mut Rect, height: u16) -> Rect {
    let height = height.min(area.height);
    area.height -= height;
    Rect::new(area.x, area.y.saturating_add(area.height), area.width, height)
}

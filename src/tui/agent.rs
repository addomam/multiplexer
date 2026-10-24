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
//! F9 cycles interactions/history. Tab/BackTab moves form fields; arrows choose,
//! Space toggles, F4 unsets. F5 validates/reviews, then F5 accepts the unchanged
//! review; F6 declines and F7 cancels. Enter selects an offered permission only.
//! Interaction PgUp/PgDn and wheel scrolling never select or answer anything.
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
const INTERACTIONS: usize = 64;
const FORM_FIELDS: usize = 64;
const OPTIONS: usize = 256;
const INTERACTION_BYTES: usize = 4 * 1024 * 1024;
const INTERACTION_DISPLAY: usize = INTERACTION_BYTES * 10;
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

#[derive(Clone, Copy, PartialEq)]
enum FieldKind {
    Text,
    Number,
    Integer,
    Boolean,
    Enum,
    Multi,
}

struct Field {
    name: String,
    kind: FieldKind,
    value: Option<Value>,
    text: String,
    cursor: usize,
    choice: usize,
}

struct Interaction {
    id: String,
    revision: u64,
    detail: Value,
    fields: Vec<Field>,
    content_bytes: usize,
    field: usize,
    option: usize,
    binding: Option<Binding>,
    captured: Option<String>,
    reviewed: bool,
    status: String,
    supported: bool,
    dirty: bool,
    focus: bool,
    display: String,
    rows: Vec<Range<usize>>,
    width: usize,
    scroll: usize,
    page: usize,
    anchor_row: usize,
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
    interactions: Vec<Interaction>,
    interaction: Option<usize>,
    interaction_gap: &'static str,
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
            interactions: Vec::new(),
            interaction: None,
            interaction_gap: "",
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
        let established = self
            .target
            .as_ref()
            .zip(target.as_ref())
            .is_some_and(|(old, new)| {
                old["session_id"].is_null()
                    && new["session_id"].is_string()
                    && old["id"] == new["id"]
                    && old["instance_id"] == new["instance_id"]
                    && old["connection_generation"] == new["connection_generation"]
            });
        if self.draft_binding.is_some() && self.draft_binding != binding {
            self.clear_draft();
            self.notice = "Draft discarded: exact target/session or original authority changed. Nothing replayed.";
        }
        for interaction in &mut self.interactions {
            if interaction.binding.is_some() && interaction.binding != binding {
                interaction.discard();
                self.notice = "Interaction draft discarded: exact target/session or authority changed; nothing replayed.";
            }
        }
        self.binding = binding;
        if changed {
            if !established {
                self.reset_history();
            }
            self.authentication.clear();
            self.auth_selected = 0;
            self.target = target;
            self.interactions.clear();
            self.interaction = None;
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
        let refresh_interactions = changed || seq != self.projection_seq;
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
        self.agent = if agent.is_null() || agent.as_object().is_some_and(|a| a.is_empty()) {
            "unknown (agent has not supplied an initialized identity)".to_owned()
        } else {
            display_value(agent, 512)
        };
        let auth = content
            .get("authentication")
            .unwrap_or(&projection["authentication"]);
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
        if refresh_interactions {
            self.sync_interactions(content.get("interactions"));
        }
    }

    fn sync_interactions(&mut self, value: Option<&Value>) {
        let Some(details) = value.and_then(Value::as_array) else {
            return;
        };
        self.interaction_gap = "";
        if details.len() > INTERACTIONS {
            self.interactions.clear();
            self.interaction = None;
            self.interaction_gap = "Interaction projection exceeds local bounds; no response controls available.";
            return;
        }
        let selected = self.interaction.and_then(|i| self.interactions.get(i))
            .map(|item| (item.id.clone(), item.revision));
        if self.interactions.iter().any(|item| item.binding.is_some() && !details.iter().any(|detail| {
            detail["interaction_id"].as_str() == Some(item.id.as_str())
                && detail["revision"].as_u64() == Some(item.revision)
        })) {
            self.notice = "Interaction/revision changed; original local response discarded, never rebound or replayed.";
        }
        self.interactions.retain(|item| details.iter().any(|detail| {
            detail["interaction_id"].as_str() == Some(item.id.as_str())
                && detail["revision"].as_u64() == Some(item.revision)
        }));
        // Immutable revision material is measured only on arrival/change, not
        // reserialized on every transcript update or frame.
        let mut sizes = [0usize; INTERACTIONS];
        let mut remaining = INTERACTION_BYTES;
        for (index, detail) in details.iter().enumerate() {
            let cached = self.interactions.iter().find(|item| {
                detail["interaction_id"].as_str() == Some(item.id.as_str())
                    && detail["revision"].as_u64() == Some(item.revision)
                    && ["state", "selected_work_id", "delivery", "application", "selection"]
                        .iter().all(|key| item.detail.get(*key) == detail.get(*key))
            });
            let size = cached.map(|item| item.content_bytes).or_else(|| {
                let mut budget = JsonBudget { remaining };
                serde_json::to_writer(&mut budget, detail).ok()?;
                Some(remaining - budget.remaining)
            });
            let Some(size) = size.filter(|size| *size <= remaining) else {
                self.interactions.clear();
                self.interaction = None;
                self.interaction_gap = "Interaction content exceeds 4 MiB; no response was captured.";
                return;
            };
            sizes[index] = size;
            remaining -= size;
        }
        let mut new_pending = None;
        for (index, detail) in details.iter().enumerate() {
            let Some(id) = identifier(&detail["interaction_id"]) else { continue };
            let Some(revision) = detail["revision"].as_u64() else { continue };
            if let Some(item) = self.interactions.iter_mut().find(|item| {
                item.id == id && item.revision == revision
            }) {
                item.content_bytes = sizes[index];
                // Schemas/context are immutable within one revision. Only
                // changed canonical disposition/selection material is copied.
                for key in ["state", "selected_work_id", "delivery", "application", "selection"] {
                    if item.detail.get(key) != detail.get(key) {
                        if let Some(value) = detail.get(key) {
                            item.detail[key] = value.clone();
                        } else if let Some(object) = item.detail.as_object_mut() {
                            object.remove(key);
                        }
                        item.dirty = true;
                    }
                }
                if !item.pending() && item.binding.is_some() {
                    item.discard();
                    item.status = "Canonical disposition changed; local draft cannot be replayed.".to_owned();
                }
            } else {
                let item = Interaction::new(id, revision, detail, sizes[index]);
                if item.pending() && new_pending.is_none() {
                    new_pending = Some(self.interactions.len());
                }
                self.interactions.push(item);
            }
        }
        self.interaction = selected
            .and_then(|(id, revision)| self.interactions.iter().position(|item| {
                item.id == id && item.revision == revision
            }))
            .or(new_pending);
    }

    fn sync_auth(&mut self, value: &Value) {
        let methods = value.as_array();
        self.auth_clipped = methods.is_some_and(|methods| methods.len() > AUTH_METHODS);
        let mut next = Vec::new();
        if let Some(methods) = methods {
            for method in methods.iter().take(AUTH_METHODS) {
                let Some(id) = identifier(&method["method_id"]) else {
                    continue;
                };
                let Some(kind) = identifier(&method["kind"]) else {
                    continue;
                };
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
        let Some(through) = content["through"].as_u64() else {
            return;
        };
        if through < self.through {
            return;
        }
        if let Some(gap) = content.get("gap").filter(|v| !v.is_null() && **v != false) {
            self.broker_gap = format!("Canonical gap: {}", display_value(gap, 256));
        }
        if content["after"]
            .as_u64()
            .is_some_and(|after| after > self.through)
        {
            self.broker_gap =
                "Projection cursor skipped unseen history; continuity is not claimed.".to_owned();
        }
        let Some(events) = content["events"].as_array() else {
            return;
        };
        for event in events.iter().take(128) {
            let Some(seq) = event["seq"].as_u64() else {
                continue;
            };
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
            self.history.push_back(Entry {
                seq,
                kind,
                text,
                rows,
            });
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
            self.broker_gap =
                "Projection page exceeds 128 events; excess content was not cached.".to_owned();
        } else {
            self.through = through;
        }
        self.clamp_scroll();
    }

    /// None always means consumed/local, never permission for native forwarding.
    pub(super) fn event(&mut self, event: &Event) -> Option<Value> {
        if matches!(event, Event::Key(key) if key.kind == KeyEventKind::Press
            && key.code == KeyCode::F(9) && key.modifiers.is_empty())
        {
            self.interaction = match self.interaction {
                Some(index) if index + 1 < self.interactions.len() => Some(index + 1),
                Some(_) => None,
                None if !self.interactions.is_empty() => Some(0),
                None => None,
            };
            self.notice = "F9 cycles canonical interactions and the prompt/history; navigation never answers.";
            return None;
        }
        // Auth and explicit interrupt remain available even during bootstrap
        // questions. Every other interaction event stays out of the prompt.
        let global = matches!(event, Event::Key(key)
            if key.kind != KeyEventKind::Release
                && ((key.code == KeyCode::F(8) && key.modifiers.is_empty())
                    || (matches!(key.code, KeyCode::Char('c' | 'C'))
                        && key.modifiers == KeyModifiers::CONTROL)));
        if !global && let Some(index) = self.interaction {
            return self.interactions[index].event(event, self.binding.as_ref(), &mut self.notice);
        }
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
                    KeyCode::Char('c' | 'C')
                        if key.modifiers == KeyModifiers::CONTROL && deliberate =>
                    {
                        self.interrupt()
                    }
                    KeyCode::F(8) if key.modifiers.is_empty() && deliberate => self.authenticate(),
                    KeyCode::Enter
                        if key.modifiers == KeyModifiers::ALT
                            || key.modifiers == KeyModifiers::SHIFT =>
                    {
                        self.insert("\n");
                        None
                    }
                    KeyCode::Enter if key.modifiers.is_empty() && deliberate => self.submit(),
                    KeyCode::PageUp => {
                        self.scroll_up(self.page_rows);
                        None
                    }
                    KeyCode::PageDown => {
                        self.scroll_down(self.page_rows);
                        None
                    }
                    KeyCode::Home if control => {
                        self.scroll = self.history_rows;
                        self.clamp_scroll();
                        None
                    }
                    KeyCode::End if control => {
                        self.scroll = 0;
                        None
                    }
                    KeyCode::Up | KeyCode::Down if self.lifecycle == "auth-required" => {
                        if !self.authentication.is_empty() {
                            if key.code == KeyCode::Up {
                                self.auth_selected = self.auth_selected.saturating_sub(1);
                            } else {
                                self.auth_selected =
                                    (self.auth_selected + 1).min(self.authentication.len() - 1);
                            }
                        }
                        None
                    }
                    KeyCode::Up => {
                        self.scroll_up(1);
                        None
                    }
                    KeyCode::Down => {
                        self.scroll_down(1);
                        None
                    }
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
                        self.cursor += self.draft[self.cursor..]
                            .find('\n')
                            .unwrap_or(self.draft.len() - self.cursor);
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
        if self.lifecycle != "ready"
            || self
                .target
                .as_ref()
                .is_none_or(|t| t["session_id"].is_null())
        {
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
        if self.lifecycle != "ready"
            || self.draft_binding.is_none()
            || self.draft_binding != self.binding
        {
            self.notice =
                "Draft has no matching original ready-session authority; nothing submitted.";
            return None;
        }
        if self.foreground.is_some() {
            self.notice = "Foreground prompt is busy; draft retained, not queued or submitted.";
            return None;
        }
        let mut action = self.draft_binding.as_ref()?.action("acp-prompt");
        action["text"] = Value::String(std::mem::take(&mut self.draft));
        self.clear_draft();
        self.notice = "Prompt captured; canonical work records admission and outcome.";
        Some(action)
    }

    /// Only a synchronous local queue rejection may return the owned envelope.
    /// Restore its original binding, never a new grant or an automatic retry.
    pub(super) fn restore_unqueued(&mut self, mut action: Value) {
        self.notice = "Action not queued; no automatic retry.";
        if matches!(action["op"].as_str(), Some("acp-answer" | "acp-permission")) {
            if let Some(interaction) = self.interactions.iter_mut().find(|item| {
                action["interaction_id"].as_str() == Some(item.id.as_str())
                    && action["revision"].as_u64() == Some(item.revision)
            }) {
                interaction.restore_unqueued(&action, self.binding.as_ref());
            }
            return;
        }
        if action["op"] != "acp-prompt" {
            return;
        }
        let Some(binding) = &self.binding else {
            return;
        };
        if action["target"] != binding.target
            || action["control"] != binding.control
            || action["owner_generation"].as_u64() != Some(binding.generation)
        {
            self.notice = "Unqueued draft authority changed; nothing restored or replayed.";
            return;
        }
        let Some(Value::String(text)) = action.as_object_mut().and_then(|a| a.remove("text"))
        else {
            return;
        };
        self.draft = text;
        self.cursor = self.draft.len();
        self.draft_binding = Some(Binding {
            target: action["target"].take(),
            control: action["control"].take(),
            generation: binding.generation,
        });
        self.refresh_editor();
        self.notice = "Prompt not queued; draft retained. Enter submits deliberately.";
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
        self.cursor = self
            .draft
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|i| *i >= self.cursor)
            .unwrap_or(self.draft.len());
        self.editor.clear();
        push_inert(
            &mut self.editor,
            &self.draft[..self.cursor],
            PROMPT_BYTES * 10,
            true,
        );
        self.editor_cursor = self.editor.len();
        push_inert(
            &mut self.editor,
            &self.draft[self.cursor..],
            PROMPT_BYTES * 10,
            true,
        );
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
        self.scroll = self
            .scroll
            .min(self.history_rows.saturating_sub(self.page_rows));
    }

    pub(super) fn draw(&mut self, frame: &mut Frame<'_>, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let mut remaining = area;
        let owner = if self.binding.is_some() {
            "owner (input-admission)"
        } else {
            "observer (read-only)"
        };
        let id = self
            .target
            .as_ref()
            .map_or(String::new(), |t| display_value(&t["id"], 256));
        let session = self
            .target
            .as_ref()
            .map_or(String::new(), |t| display_value(&t["session_id"], 256));
        let connection = self
            .target
            .as_ref()
            .and_then(|t| t["connection_generation"].as_u64())
            .unwrap_or(0);
        let header = format!(
            "ACP | {} | {owner} | {id}\nAgent: {}\nSession: {session} | connection {connection}\nForeground: {} {}",
            self.lifecycle,
            self.agent,
            self.foreground
                .as_ref()
                .map_or("none".to_owned(), |id| inert(id, 256)),
            self.foreground_state
        );
        frame.render_widget(
            Paragraph::new(header).style(Style::default().fg(ACCENT)),
            take_top(&mut remaining, 4),
        );
        let gap = format!(
            "Cursor {} | {}{}{}{}",
            self.through,
            if self.scroll == 0 {
                "following"
            } else {
                "scrollback"
            },
            if self.broker_gap.is_empty() {
                ""
            } else {
                " | "
            },
            self.broker_gap,
            self.local_gap.map_or(String::new(), |seq| format!(
                " | local history evicted through {seq}"
            ))
        );
        frame.render_widget(
            Paragraph::new(gap).style(Style::default().fg(Color::Yellow)),
            take_top(&mut remaining, 1),
        );
        let footer = take_bottom(&mut remaining, 2);
        let editor_width = usize::from(area.width.saturating_sub(2)).max(1);
        if self.editor_width != editor_width {
            self.editor_width = editor_width;
            self.layout_editor();
        }
        let editor_height = (self.editor_rows.len() + 2).clamp(3, 8) as u16;
        let editor = take_bottom(&mut remaining, editor_height);
        let work = take_bottom(
            &mut remaining,
            (self.work.len() + usize::from(!self.resource.is_empty())) as u16,
        );
        let mut work_lines: Vec<Line<'_>> = self
            .work
            .iter()
            .map(|line| Line::raw(line.as_str()))
            .collect();
        if !self.resource.is_empty() {
            work_lines.push(Line::raw(self.resource.as_str()));
        }
        frame.render_widget(
            Paragraph::new(work_lines).style(Style::default().fg(MUTED)),
            work,
        );
        if self.lifecycle == "auth-required" {
            let height =
                (self.authentication.len().min(4) + 1 + usize::from(self.auth_clipped)) as u16;
            let auth = take_top(&mut remaining, height);
            let start = self.auth_selected.saturating_sub(3);
            let mut lines = vec![Line::raw(
                "Authentication: Up/Down selects; F8 explicitly chooses agent-type only.",
            )];
            for (index, method) in self.authentication.iter().enumerate().skip(start).take(4) {
                let label = format!(
                    "{} {} [{}] {}",
                    if index == self.auth_selected {
                        ">"
                    } else {
                        " "
                    },
                    inert(&method.id, 256),
                    inert(&method.kind, 256),
                    method.label
                );
                let style = if index == self.auth_selected {
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(MUTED)
                };
                lines.push(Line::styled(label, style));
            }
            if self.auth_clipped {
                lines.push(Line::raw(
                    "Additional authentication methods omitted by the local display bound.",
                ));
            }
            frame.render_widget(Paragraph::new(lines), auth);
        }
        if let Some(index) = self.interaction {
            self.interactions[index].draw(frame, remaining);
        } else {
            self.draw_history(frame, remaining);
            if !self.interaction_gap.is_empty() {
                frame.render_widget(Paragraph::new(self.interaction_gap), take_top(&mut remaining, 1));
            }
        }
        self.draw_editor(frame, editor);
        frame.render_widget(Paragraph::new(vec![
            Line::raw(self.notice),
            Line::styled("F9 questions/history | Tab fields | F4 unset | F5 review/accept F6 decline F7 cancel | Enter prompt/permission | F8 auth | Ctrl-C interrupt | PgUp/PgDn scroll",
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
        let start = self
            .history_rows
            .saturating_sub(self.scroll)
            .saturating_sub(self.page_rows);
        let mut skip = start;
        let mut lines = Vec::with_capacity(self.page_rows);
        for entry in &self.history {
            if skip >= entry.rows.len() {
                skip -= entry.rows.len();
                continue;
            }
            for row in entry.rows.iter().skip(skip) {
                lines.push(Line::from(Span::styled(
                    &entry.text[row.clone()],
                    category_style(entry.kind),
                )));
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
            lines.push(Line::styled(
                "No canonical transcript content in this projection.",
                Style::default().fg(MUTED),
            ));
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
        let cursor_row = self
            .editor_rows
            .iter()
            .rposition(|row| row.start <= self.editor_cursor)
            .unwrap_or(0);
        let start = cursor_row.saturating_sub(usize::from(inner.height) - 1);
        let title = if self.binding.is_some() {
            format!(
                " Local draft {}/65536 B | rows {}..{}/{}; not history ",
                self.draft.len(),
                start + 1,
                (start + usize::from(inner.height)).min(self.editor_rows.len()),
                self.editor_rows.len()
            )
        } else {
            " Observer; no prompt authority ".to_owned()
        };
        frame.render_widget(block.title(title), area);
        let lines: Vec<Line<'_>> = self
            .editor_rows
            .iter()
            .skip(start)
            .take(usize::from(inner.height))
            .map(|row| Line::raw(&self.editor[row.clone()]))
            .collect();
        frame.render_widget(Paragraph::new(lines), inner);
        if self.interaction.is_none() && self.binding.is_some() && self.lifecycle == "ready" {
            let row = &self.editor_rows[cursor_row];
            let column =
                UnicodeWidthStr::width(&self.editor[row.start..self.editor_cursor.min(row.end)]);
            frame.set_cursor_position((
                inner.x + column.min(usize::from(inner.width) - 1) as u16,
                inner.y + (cursor_row - start) as u16,
            ));
        }
    }
}

impl Interaction {
    fn new(id: &str, revision: u64, detail: &Value, content_bytes: usize) -> Self {
        let mut item = Self {
            id: id.to_owned(),
            revision,
            detail: detail.clone(),
            content_bytes,
            fields: Vec::new(),
            field: 0,
            option: 0,
            binding: None,
            captured: None,
            reviewed: false,
            status: String::new(),
            supported: false,
            dirty: true,
            focus: false,
            display: String::new(),
            rows: Vec::new(),
            width: 0,
            scroll: 0,
            anchor_row: 0,
            page: 1,
        };
        item.reset_fields();
        item
    }

    fn pending(&self) -> bool {
        self.detail["state"] == "pending" && self.detail["selected_work_id"].is_null()
    }

    fn reset_fields(&mut self) {
        self.fields.clear();
        self.field = 0;
        self.option = 0;
        self.supported = match self.detail["kind"].as_str() {
            Some("form") => self.load_fields().is_some(),
            Some("permission") => self.detail["options"].as_array().is_some_and(|options| {
                !options.is_empty() && options.len() <= OPTIONS
                    && options.iter().all(|option| identifier(&option["option_id"]).is_some()
                        && option["name"].is_string() && option["kind"].is_string())
            }),
            _ => false,
        };
        if !self.supported {
            self.fields.clear();
            self.status = "Unsupported schema/options; no inferred control or response is available.".to_owned();
        }
        self.dirty = true;
    }

    // Presentation decoding only. The one backend helper owns answer
    // validation; this rejects unknown controls instead of guessing a type.
    fn load_fields(&mut self) -> Option<()> {
        let schema = &self.detail["schema"];
        if schema["type"] != "object" {
            return None;
        }
        let properties = schema["properties"].as_object()?;
        if properties.len() > FORM_FIELDS {
            return None;
        }
        let mut options = 0;
        let mut draft_budget = JsonBudget { remaining: PROMPT_BYTES };
        for (name, property) in properties {
            let kind = match property["type"].as_str()? {
                "string" if property.get("enum").is_some_and(|v| !v.is_null())
                    || property.get("oneOf").is_some_and(|v| !v.is_null()) => FieldKind::Enum,
                "string" => FieldKind::Text,
                "number" => FieldKind::Number,
                "integer" => FieldKind::Integer,
                "boolean" => FieldKind::Boolean,
                "array" => FieldKind::Multi,
                _ => return None,
            };
            serde_json::to_writer(&mut draft_budget, name).ok()?;
            if let Some(value) = property.get("default").filter(|value| !value.is_null()) {
                serde_json::to_writer(&mut draft_budget, value).ok()?;
            }
            if matches!(kind, FieldKind::Enum | FieldKind::Multi) {
                let choices = field_choices(property, kind)?;
                if !choices.iter().any(|choice| choice_allowed(property, kind, choice)) {
                    return None;
                }
                options += choices.len();
                if options > OPTIONS {
                    return None;
                }
                for choice in choices {
                    choice_value(choice)?;
                }
            }
            let default = property.get("default").filter(|value| !value.is_null());
            let text = match kind {
                FieldKind::Text => default.and_then(Value::as_str).unwrap_or("").to_owned(),
                FieldKind::Number | FieldKind::Integer => default.map(ToString::to_string).unwrap_or_default(),
                _ => String::new(),
            };
            let choice = default.and_then(Value::as_str).and_then(|value| {
                field_choices(property, kind)?.iter().position(|choice| choice_value(choice) == Some(value))
            }).or_else(|| {
                field_choices(property, kind)?.iter().position(|choice| choice_allowed(property, kind, choice))
            }).unwrap_or(0);
            let value = if matches!(kind, FieldKind::Text | FieldKind::Number | FieldKind::Integer) {
                default.map(|_| Value::Null)
            } else {
                default.cloned()
            };
            self.fields.push(Field { name: name.clone(), kind, value, cursor: text.len(), text, choice });
        }
        Some(())
    }

    fn discard(&mut self) {
        self.binding = None;
        self.captured = None;
        self.reviewed = false;
        self.status.clear();
        self.reset_fields();
    }

    fn editable(&mut self, binding: Option<&Binding>, notice: &mut &'static str) -> bool {
        if !self.pending() || !self.supported {
            *notice = "Interaction is not pending/supported; no action captured.";
            return false;
        }
        let Some(binding) = binding else {
            *notice = "Observer: interaction edits/actions require current explicit control.";
            return false;
        };
        if self.captured.is_some() {
            *notice = "Action already captured; inspect canonical disposition. No replay or automatic retry.";
            return false;
        }
        if self.binding.is_none() {
            self.binding = Some(binding.clone());
        }
        if self.binding.as_ref() != Some(binding) {
            *notice = "Original interaction authority no longer matches; no response captured.";
            return false;
        }
        true
    }

    fn changed(&mut self) {
        self.reviewed = false;
        self.status = "Local draft only; F5 validates and reviews before acceptance.".to_owned();
        self.dirty = true;
        self.focus = true;
    }

    fn event(&mut self, event: &Event, binding: Option<&Binding>, notice: &mut &'static str) -> Option<Value> {
        match event {
            Event::Mouse(mouse) => {
                match mouse.kind {
                    MouseEventKind::ScrollUp => self.scroll = self.scroll.saturating_sub(3),
                    MouseEventKind::ScrollDown => self.scroll = self.scroll.saturating_add(3),
                    _ => {}
                }
                return None;
            }
            Event::Paste(text) => {
                self.insert(text, binding, notice);
                return None;
            }
            _ => {}
        }
        let Event::Key(key) = event else { return None };
        if key.kind == KeyEventKind::Release {
            return None;
        }
        let deliberate = key.kind == KeyEventKind::Press;
        let plain = key.modifiers.is_empty();
        match key.code {
            KeyCode::PageUp => { self.scroll = self.scroll.saturating_sub(self.page); return None; }
            KeyCode::PageDown => { self.scroll = self.scroll.saturating_add(self.page); return None; }
            KeyCode::Home if key.modifiers == KeyModifiers::CONTROL => { self.scroll = 0; return None; }
            KeyCode::End if key.modifiers == KeyModifiers::CONTROL => { self.scroll = self.rows.len(); return None; }
            KeyCode::Tab | KeyCode::BackTab
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if !self.fields.is_empty() {
                    let backwards = key.code == KeyCode::BackTab || key.modifiers == KeyModifiers::SHIFT;
                    self.field = if backwards {
                        (self.field + self.fields.len() - 1) % self.fields.len()
                    } else {
                        (self.field + 1) % self.fields.len()
                    };
                    self.dirty = true;
                    self.focus = true;
                }
                return None;
            }
            KeyCode::F(5) if plain && deliberate && self.detail["kind"] == "form" =>
                return self.answer("accept", binding, notice),
            KeyCode::F(6) if plain && deliberate && self.detail["kind"] == "form" =>
                return self.answer("decline", binding, notice),
            KeyCode::F(7) if plain && deliberate && self.detail["kind"] == "form" =>
                return self.answer("cancel", binding, notice),
            KeyCode::Enter if plain && deliberate && self.detail["kind"] == "permission" =>
                return self.permission(binding, notice),
            _ => {}
        }
        if self.detail["kind"] == "permission" {
            if plain && matches!(key.code, KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right)
                && self.editable(binding, notice)
            {
                let count = self.detail["options"].as_array().map_or(0, Vec::len);
                let previous = matches!(key.code, KeyCode::Up | KeyCode::Left);
                self.option = if previous { self.option.saturating_sub(1) } else { (self.option + 1).min(count - 1) };
                self.changed();
                self.status = "Local permission highlight; Enter deliberately selects this exact offered option.".to_owned();
            }
            return None;
        }
        if self.fields.is_empty() {
            return None;
        }
        if let KeyCode::Char(c) = key.code
            && !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            && !c.is_control()
        {
            let kind = self.fields[self.field].kind;
            if c != ' ' || matches!(kind, FieldKind::Text | FieldKind::Number | FieldKind::Integer) {
                let mut bytes = [0; 4];
                self.insert(c.encode_utf8(&mut bytes), binding, notice);
                return None;
            }
        }
        let newline = key.code == KeyCode::Enter
            && (key.modifiers == KeyModifiers::ALT || key.modifiers == KeyModifiers::SHIFT);
        if newline {
            self.insert("\n", binding, notice);
            return None;
        }
        if !plain || !matches!(key.code, KeyCode::F(4) | KeyCode::Char(' ') | KeyCode::Left
            | KeyCode::Right | KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End
            | KeyCode::Backspace | KeyCode::Delete)
        {
            return None;
        }
        if !self.editable(binding, notice) {
            return None;
        }
        let draft_bytes = self.draft_size();
        let field = &mut self.fields[self.field];
        match key.code {
            KeyCode::F(4) if deliberate => {
                field.value = None;
                field.text.clear();
                field.cursor = 0;
            }
            KeyCode::Char(' ') if deliberate && field.kind == FieldKind::Boolean =>
                field.value = Some(Value::Bool(!field.value.as_ref().and_then(Value::as_bool).unwrap_or(false))),
            KeyCode::Char(' ') if deliberate && field.kind == FieldKind::Multi => {
                let property = &self.detail["schema"]["properties"][&field.name];
                let choices = field_choices(property, field.kind)?;
                let choice = choices.get(field.choice)?;
                if !choice_allowed(property, field.kind, choice) {
                    return None;
                }
                let value = choice_value(choice)?;
                let existing = field.value.as_ref().and_then(Value::as_array)
                    .and_then(|values| values.iter().position(|v| v.as_str() == Some(value)));
                if existing.is_none() && !string_fits(value, PROMPT_BYTES.saturating_sub(draft_bytes + 2)) {
                    *notice = "Answer limit is 64 KiB; offered value cannot be added to this draft.";
                    return None;
                }
                let values = field.value.get_or_insert_with(|| Value::Array(Vec::new())).as_array_mut()?;
                if let Some(index) = existing {
                    values.remove(index);
                } else {
                    values.push(Value::String(value.to_owned()));
                }
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down
                if matches!(field.kind, FieldKind::Enum | FieldKind::Multi) =>
            {
                let property = &self.detail["schema"]["properties"][&field.name];
                let choices = field_choices(property, field.kind)?;
                let previous = matches!(key.code, KeyCode::Left | KeyCode::Up);
                let next = if previous {
                    (0..field.choice).rev().find(|index| choice_allowed(property, field.kind, &choices[*index]))
                } else {
                    (field.choice + 1..choices.len()).find(|index| choice_allowed(property, field.kind, &choices[*index]))
                }.unwrap_or(field.choice);
                let value = choice_value(&choices[next])?;
                if field.kind == FieldKind::Enum {
                    let old_bytes = field.value.as_ref().map_or(2, json_size);
                    if !string_fits(value, PROMPT_BYTES.saturating_sub(draft_bytes.saturating_sub(old_bytes))) {
                        *notice = "Answer limit is 64 KiB; offered value cannot be added to this draft.";
                        return None;
                    }
                    field.value = Some(Value::String(value.to_owned()));
                }
                field.choice = next;
            }
            KeyCode::Left => field.cursor = previous_boundary(&field.text, field.cursor),
            KeyCode::Right => field.cursor = next_boundary(&field.text, field.cursor),
            KeyCode::Home => field.cursor = 0,
            KeyCode::End => field.cursor = field.text.len(),
            KeyCode::Backspace if field.cursor > 0 => {
                let previous = previous_boundary(&field.text, field.cursor);
                field.text.replace_range(previous..field.cursor, "");
                field.cursor = previous;
                field.value = Some(Value::Null);
            }
            KeyCode::Delete if field.cursor < field.text.len() => {
                let next = next_boundary(&field.text, field.cursor);
                field.text.replace_range(field.cursor..next, "");
                field.value = Some(Value::Null);
            }
            _ => return None,
        }
        self.changed();
        None
    }

    fn insert(&mut self, text: &str, binding: Option<&Binding>, notice: &mut &'static str) {
        if self.fields.is_empty() || text.is_empty() || !self.editable(binding, notice) {
            return;
        }
        let kind = self.fields[self.field].kind;
        if !matches!(kind, FieldKind::Text | FieldKind::Number | FieldKind::Integer) {
            *notice = "This offered control is not text-editable; paste never chooses an option.";
            return;
        }
        if self.draft_size().saturating_add(string_size(text).saturating_sub(2)) > PROMPT_BYTES {
            *notice = "Answer draft limit is 64 KiB; edit/paste rejected in full.";
            return;
        }
        let field = &mut self.fields[self.field];
        let normalized = normalize_newlines(text);
        field.text.insert_str(field.cursor, &normalized);
        field.cursor += normalized.len();
        field.cursor = field.text.grapheme_indices(true).map(|(i, _)| i)
            .find(|i| *i >= field.cursor).unwrap_or(field.text.len());
        field.value = Some(Value::Null); // Explicitly set, including empty text.
        self.changed();
    }

    fn draft_size(&self) -> usize {
        self.fields.iter().fold(2usize, |bytes, field| {
            bytes.saturating_add(string_size(&field.name) + 2)
                .saturating_add(if matches!(field.kind, FieldKind::Text | FieldKind::Number | FieldKind::Integer) {
                    string_size(&field.text)
                } else {
                    field.value.as_ref().map_or(2, json_size)
                })
        })
    }

    fn values(&self) -> Result<Value, &'static str> {
        let mut values = serde_json::Map::new();
        for field in &self.fields {
            let Some(value) = &field.value else { continue };
            let value = match field.kind {
                FieldKind::Text => Value::String(field.text.clone()),
                FieldKind::Integer => Value::from(field.text.parse::<i64>().map_err(|_| "integer requires an i64 value")?),
                FieldKind::Number => {
                    let number = field.text.parse::<f64>().map_err(|_| "number requires a finite numeric value")?;
                    Value::Number(serde_json::Number::from_f64(number).ok_or("number must be finite")?)
                }
                _ => value.clone(),
            };
            values.insert(field.name.clone(), value);
        }
        let values = Value::Object(values);
        if !json_fits(&values, PROMPT_BYTES) {
            return Err("answer exceeds 64 KiB");
        }
        crate::acp::validate_form_values(&self.detail["schema"], &values)?;
        Ok(values)
    }

    fn answer(&mut self, choice: &str, binding: Option<&Binding>, notice: &mut &'static str) -> Option<Value> {
        if !self.editable(binding, notice) {
            return None;
        }
        let values = if choice == "accept" {
            match self.values() {
                Ok(values) => {
                    if !self.reviewed {
                        self.reviewed = true;
                        self.status = "Supported local checks passed. Review every field; F5 again accepts. Pattern/format are NOT certified.".to_owned();
                        self.dirty = true;
                        self.focus = false;
                        self.scroll = 0;
                        return None;
                    }
                    Some(values)
                }
                Err(error) => {
                    self.reviewed = false;
                    self.status = format!("Validation: {}", inert(error, 256));
                    self.dirty = true;
                    return None;
                }
            }
        } else {
            None
        };
        let mut action = self.binding.as_ref()?.action("acp-answer");
        action["interaction_id"] = Value::String(self.id.clone());
        action["revision"] = Value::from(self.revision);
        action["action"] = Value::String(choice.to_owned());
        if let Some(values) = values {
            action["values"] = values;
        }
        self.capture(&action, notice);
        Some(action)
    }

    fn permission(&mut self, binding: Option<&Binding>, notice: &mut &'static str) -> Option<Value> {
        if self.rows.is_empty() || self.anchor_row < self.scroll || self.anchor_row >= self.scroll + self.page {
            self.focus = true;
            self.dirty = true;
            *notice = "Review the exact highlighted permission option now in view; Enter deliberately chooses it.";
            return None;
        }
        if !self.editable(binding, notice) {
            return None;
        }
        let option = self.detail["options"].as_array()?.get(self.option)?;
        let mut action = self.binding.as_ref()?.action("acp-permission");
        action["interaction_id"] = Value::String(self.id.clone());
        action["revision"] = Value::from(self.revision);
        action["option_id"] = option["option_id"].clone();
        self.capture(&action, notice);
        Some(action)
    }

    fn capture(&mut self, action: &Value, notice: &mut &'static str) {
        self.captured = action["call_id"].as_str().map(str::to_owned);
        self.status = "Local action captured. Only canonical disposition records admission, delivery and application.".to_owned();
        self.dirty = true;
        *notice = "Deliberate interaction action captured; no optimistic history or automatic retry.";
    }

    fn restore_unqueued(&mut self, action: &Value, current: Option<&Binding>) {
        let Some(binding) = &self.binding else { return };
        if self.pending() && self.binding.as_ref() == current
            && action["target"] == binding.target
            && action["control"] == binding.control
            && action["owner_generation"].as_u64() == Some(binding.generation)
            && action["interaction_id"].as_str() == Some(self.id.as_str())
            && action["revision"].as_u64() == Some(self.revision)
            && action["call_id"].as_str() == self.captured.as_deref()
            && self.captured.is_some()
        {
            self.captured = None;
            self.reviewed = false;
            self.status = "Synchronous local queue rejection: original values/highlight retained. Deliberate new action required.".to_owned();
            self.dirty = true;
        }
    }

    fn draw(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::default().borders(Borders::ALL).title(format!(
            " {} | revision {} | F9 next/history ", inert(&self.id, 2048), self.revision
        ));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        self.page = usize::from(inner.height).saturating_sub(2).max(1);
        if self.width != usize::from(inner.width) {
            self.focus = true;
        }
        if self.dirty || self.width != usize::from(inner.width) {
            self.rebuild(usize::from(inner.width));
        }
        self.scroll = self.scroll.min(self.rows.len().saturating_sub(self.page));
        let status = format!("Canonical {} | delivery={} | application={} | rows {}..{}/{}\n{}",
            display_value(&self.detail["state"], 128),
            display_value(&self.detail["delivery"], 128),
            display_value(&self.detail["application"], 128),
            self.scroll + 1, (self.scroll + self.page).min(self.rows.len()), self.rows.len(),
            if self.reviewed { "REVIEW: F5 accepts; F6 declines; F7 cancels" } else { self.status.as_str() });
        let mut body = inner;
        frame.render_widget(Paragraph::new(status).style(Style::default().fg(Color::Yellow)), take_top(&mut body, 2));
        let lines: Vec<Line<'_>> = self.rows.iter().skip(self.scroll).take(usize::from(body.height))
            .map(|row| Line::raw(&self.display[row.clone()])).collect();
        frame.render_widget(Paragraph::new(lines), body);
    }

    fn rebuild(&mut self, width: usize) {
        self.display.clear();
        let mut anchor = 0;
        // Canonical context is complete and scrollable, never terminal commands.
        for key in ["kind", "state", "selected_work_id", "delivery", "application", "scope",
            "message", "tool_call", "selection"]
        {
            if let Some(value) = self.detail.get(key).filter(|value| !value.is_null()) {
                let _ = writeln!(self.display, "{key}: {}", display_value(value, INTERACTION_DISPLAY));
            }
        }
        self.display.push_str("Application is unknown unless independently observed; local capture is not admission.\n");
        if self.detail["kind"] == "form" {
            for key in ["title", "description"] {
                if let Some(value) = self.detail["schema"].get(key) {
                    let _ = writeln!(self.display, "{key}: {}", display_value(value, INTERACTION_DISPLAY));
                }
            }
            self.display.push_str("Tab/Shift-Tab fields; text/numeric editing/paste; arrows offered choices; Space bool/multiselect; F4 unset.\nF5 validate/review, then F5 accept; F6 decline; F7 cancel. Defaults are editable, never automatic responses.\nLocal validation is not full JSON Schema or pattern/format certification; the agent validates independently.\n");
            for (index, field) in self.fields.iter().enumerate() {
                let property = &self.detail["schema"]["properties"][&field.name];
                let active = index == self.field;
                if active { anchor = self.display.len(); }
                let required = self.detail["schema"]["required"].as_array()
                    .is_some_and(|names| names.iter().any(|name| name.as_str() == Some(field.name.as_str())));
                let _ = writeln!(self.display, "{} Field {} [{}]: {}",
                    if active { ">" } else { " " }, inert(&field.name, INTERACTION_DISPLAY),
                    if required { "required" } else { "optional" },
                    if field.value.is_none() { "UNSET (omitted)" } else { "explicit local value" });
                let _ = writeln!(self.display, "Schema/constraints: {}", display_value(property, INTERACTION_DISPLAY));
                if matches!(field.kind, FieldKind::Text | FieldKind::Number | FieldKind::Integer) {
                    self.display.push_str("Draft: ");
                    push_inert(&mut self.display, &field.text[..field.cursor], INTERACTION_DISPLAY, true);
                    if active { anchor = self.display.len(); self.display.push('│'); }
                    push_inert(&mut self.display, &field.text[field.cursor..], INTERACTION_DISPLAY, true);
                    self.display.push('\n');
                } else {
                    let _ = writeln!(self.display, "Draft: {}", field.value.as_ref()
                        .map_or_else(|| "UNSET".to_owned(), |value| display_value(value, INTERACTION_DISPLAY)));
                }
                if let Some(choices) = field_choices(property, field.kind) {
                    for (index, choice) in choices.iter().enumerate() {
                        if !choice_allowed(property, field.kind, choice) {
                            continue;
                        }
                        if active && index == field.choice { anchor = self.display.len(); }
                        let value = choice_value(choice).unwrap_or("");
                        let selected = match &field.value {
                            Some(Value::Array(values)) => values.iter().any(|v| v.as_str() == Some(value)),
                            Some(Value::String(selected)) => selected == value,
                            _ => false,
                        };
                        let _ = writeln!(self.display, " {} [{}] {}",
                            if active && index == field.choice { ">" } else { " " },
                            if selected { "x" } else { " " }, display_value(choice, INTERACTION_DISPLAY));
                    }
                }
            }
            if !self.supported {
                let _ = writeln!(self.display, "Unsupported raw schema: {}", display_value(&self.detail["schema"], INTERACTION_DISPLAY));
            }
        } else if let Some(options) = self.detail["options"].as_array() {
            self.display.push_str("Arrows highlight; Enter deliberately chooses the exact offered ID. Escape/focus/detach never deny or cancel.\n");
            for (index, option) in options.iter().enumerate() {
                if index == self.option { anchor = self.display.len(); }
                let _ = writeln!(self.display, "{} {}", if index == self.option { ">" } else { " " },
                    display_value(option, INTERACTION_DISPLAY));
            }
        }
        if self.display.len() > INTERACTION_DISPLAY {
            let mut end = INTERACTION_DISPLAY;
            while !self.display.is_char_boundary(end) { end -= 1; }
            self.display.truncate(end);
            self.display.push_str("\n[Local interaction display limit reached; no hidden option is selectable.]\n");
            self.supported = false;
        }
        self.rows = wrap_rows(&self.display, width.max(1));
        self.anchor_row = self.rows.iter().rposition(|row| row.start <= anchor).unwrap_or(0);
        if self.focus && (self.anchor_row < self.scroll || self.anchor_row >= self.scroll + self.page) {
            self.scroll = self.anchor_row.saturating_sub(self.page / 2);
        }
        self.width = width;
        self.dirty = false;
        self.focus = false;
    }
}

fn field_choices(property: &Value, kind: FieldKind) -> Option<&[Value]> {
    let property = if kind == FieldKind::Multi {
        &property["items"]
    } else if kind == FieldKind::Enum {
        property
    } else {
        return None;
    };
    if property.get("type").is_some_and(|value| value != "string") {
        return None;
    }
    // Prefer the titled representation; navigation applies the enum intersection.
    property.get(if kind == FieldKind::Multi { "anyOf" } else { "oneOf" })
        .filter(|value| !value.is_null())
        .or_else(|| property.get("enum")).and_then(Value::as_array).map(Vec::as_slice)
}

fn choice_value(choice: &Value) -> Option<&str> {
    choice.as_str().or_else(|| choice["const"].as_str())
}

fn choice_allowed(property: &Value, kind: FieldKind, choice: &Value) -> bool {
    let Some(value) = choice_value(choice) else { return false };
    let schema = if kind == FieldKind::Multi { &property["items"] } else { property };
    schema.get("enum").filter(|v| !v.is_null()).is_none_or(|values| {
        values.as_array().is_some_and(|values| values.iter().any(|entry| entry.as_str() == Some(value)))
    })
}

fn string_fits(value: &str, limit: usize) -> bool {
    serde_json::to_writer(JsonBudget { remaining: limit }, value).is_ok()
}

struct JsonBudget {
    remaining: usize,
}

impl io::Write for JsonBudget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(io::Error::other("JSON byte limit"));
        }
        self.remaining -= bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

fn json_fits(value: &Value, limit: usize) -> bool {
    serde_json::to_writer(JsonBudget { remaining: limit }, value).is_ok()
}

fn json_size(value: &Value) -> usize {
    let mut budget = JsonBudget { remaining: PROMPT_BYTES };
    if serde_json::to_writer(&mut budget, value).is_ok() {
        PROMPT_BYTES - budget.remaining
    } else {
        PROMPT_BYTES + 1
    }
}

fn string_size(value: &str) -> usize {
    let mut budget = JsonBudget { remaining: PROMPT_BYTES };
    if serde_json::to_writer(&mut budget, value).is_ok() {
        PROMPT_BYTES - budget.remaining
    } else {
        PROMPT_BYTES + 1
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
    text[..at]
        .grapheme_indices(true)
        .next_back()
        .map_or(0, |(i, _)| i)
}

fn next_boundary(text: &str, at: usize) -> usize {
    text[at..]
        .graphemes(true)
        .next()
        .map_or(text.len(), |g| at + g.len())
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
                complete = push_inert(
                    &mut text,
                    &display_value(value, remaining),
                    EVENT_BYTES,
                    true,
                );
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

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn display_value(value: &Value, limit: usize) -> String {
    if value.is_null() {
        return String::new();
    }
    if let Some(text) = value.as_str() {
        return inert(text, limit);
    }
    let mut writer = LimitedJson {
        bytes: Vec::new(),
        limit,
    };
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
    Rect::new(
        area.x,
        area.y.saturating_add(area.height),
        area.width,
        height,
    )
}

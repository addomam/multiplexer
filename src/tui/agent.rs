//! Human ACP projection/editor. The parent consumes every active-ACP event,
//! including `None`, and dispatches returned envelopes unchanged. This module
//! neither acquires authority nor forwards input to a native terminal.
//!
//! Conversation and the current question stay above protocol diagnostics.
//! Details/F10 and History are scrollable read-only surfaces; Help/F1 is local.
//! Tab/BackTab and painted mouse targets reach fields and explicit actions.
//! Plain j/k/? navigate only with action focus; they remain literal in inputs.
//! Escape backs out one level without answering, denying or replaying a draft.
//! F8 explicitly chooses an advertised agent authentication method. Ctrl-C
//! captures the exact foreground prompt ID, never a generic process signal.
//! F9 cycles pending/completed interactions and the prompt. F4 unsets, F5
//! validates/reviews then accepts unchanged values, F6 declines only if offered,
//! and F7 cancels only if offered. Paste never submits or chooses an option.
//! Every response/accelerator uses the same painted identity. New mutation
//! choices need a visible, input-drained armed repaint; ordinary editing stays immediate.
//! Parenthesized choices are visible but settling; bracketed choices are armed.
//!
//! First edit captures the complete target tuple and original grant. Changed
//! identity/authority discards the draft; no action is retried or rebound.
//! Only sequenced canonical events enter history. The cache holds at most
//! 2048 events / 4 MiB, with explicit eviction and per-event clipping notices.
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
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
const INTERACTIONS: usize = crate::acp::INTERACTION_RECORDS;
const FORM_FIELDS: usize = 64;
const OPTIONS: usize = 256;
const INTERACTION_BYTES: usize = 4 * 1024 * 1024;
const INTERACTION_DISPLAY: usize = INTERACTION_BYTES * 10;
const MUTED: Color = Color::Rgb(149, 163, 184);
const ACCENT: Color = Color::Rgb(103, 213, 224);

const HELP: &str = "ACP conversation\n\nTab / Shift-Tab moves between inputs and visible actions. Enter activates a focused action. Click a field to edit it; j, k and ? are literal text while editing. Escape leaves an input for actions; Escape again returns to the prompt. Drafts are preserved unless their original authority or target changes.\n\nForms: F5 validates and opens review; F5 again submits the unchanged reviewed values. Review is not full JSON Schema/pattern/format certification. F4 unsets a field. Arrows choose advertised values; Space toggles boolean/multiple choices. Defaults are never submitted automatically. F6 declines only when offered; F7 cancels.\n\nPermissions: arrows highlight exact advertised options; Enter or an option button deliberately selects that exact option. No inferred allow/deny choices. Escape never denies or cancels.\n\nPrompt: Enter or Send submits; Alt/Shift-Enter inserts a newline. Paste only edits. New questions do not take an active prompt draft's focus. F8 chooses the highlighted advertised agent authentication method. Ctrl-C requests interruption of the exact foreground prompt, not process termination.\n\nF9 cycles questions and completed interactions/history. History shows all bounded canonical records; Details/F10 shows identities, settings, protocol diagnostics and complete interaction context. PgUp/PgDn, arrows and wheel scroll read-only views. F1 opens this help; Escape returns one level. No event on this screen falls through to a native PTY. Navigation never acquires control, replays input or acknowledges application delivery.";

#[derive(Clone, PartialEq)]
enum Action {
    Field(usize),
    Permission(usize),
    Review(bool),
    Decline,
    Cancel,
    Prompt,
    Send,
    Authenticate(usize, String),
    Interrupt(String),
    Details,
    History,
    Questions,
    Help,
    Back,
}

impl Action {
    fn needs_input_fence(&self) -> bool {
        matches!(
            self,
            Self::Permission(_)
                | Self::Review(true)
                | Self::Decline
                | Self::Cancel
                | Self::Authenticate(_, _)
                | Self::Interrupt(_)
        )
    }
}

#[derive(Clone, PartialEq)]
struct Painted {
    target: Option<Value>,
    binding: Option<Binding>,
    interaction: Option<(String, u64, u64, bool, Option<String>)>,
    foreground: Option<String>,
    foreground_state: String,
    lifecycle: &'static str,
    authentication: (u64, usize),
    covered: bool,
}

struct Hit {
    rect: Rect,
    action: Action,
    painted: Painted,
    armed: bool,
    fence_eligible: bool,
}

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
    body: Range<usize>,
}

#[derive(PartialEq)]
struct AuthMethod {
    id: String,
    kind: String,
    label: String,
}

impl AuthMethod {
    fn caption(&self) -> String {
        let kind = match self.kind.as_str() {
            "agent" | "client" | "terminal" | "unsupported" => self.kind.as_str(),
            _ => "unsupported kind",
        };
        format!(
            "{} [{kind}]{}",
            self.label,
            if self.kind == "agent" {
                ""
            } else {
                " · unsupported"
            }
        )
    }
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
    paint_revision: u64,
    review_visible: bool,
    review_values: Option<Value>,
    status: String,
    supported: bool,
    dirty: bool,
    focus: bool,
    display: String,
    rows: Vec<Range<usize>>,
    scroll: usize,
    page: usize,
    anchor_row: usize,
    visible_options: Vec<usize>,
    question: String,
    question_rows: Vec<Range<usize>>,
    question_width: usize,
}

pub(super) struct AgentView {
    target: Option<Value>,
    binding: Option<Binding>,
    projection_seq: u64,
    lifecycle: &'static str,
    agent: String,
    configuration: Value,
    settings: String,
    settings_open: bool,
    settings_rows: Vec<Range<usize>>,
    settings_width: usize,
    settings_scroll: usize,
    settings_page: usize,
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
    help: Option<super::view::Help>,
    history_open: bool,
    action_focus: Option<Action>,
    hits: Vec<Hit>,
    drawn: Option<Painted>,
    draw_area: Rect,
    input_ready: bool,
    settled_actions: Vec<Action>,
    paint_complete: bool,
    auth_revision: u64,
    prompt_active: bool,
    diagnostics: String,
    diagnostic_seq: Option<u64>,
    failure: String,
    attention: Option<&'static str>,
}

impl AgentView {
    pub(super) fn new() -> Self {
        Self {
            target: None,
            binding: None,
            projection_seq: 0,
            lifecycle: "starting",
            agent: String::new(),
            configuration: Value::Null,
            settings: String::new(),
            settings_open: false,
            settings_rows: Vec::new(),
            settings_width: 0,
            settings_scroll: 0,
            settings_page: 1,
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
            notice: "Tab / Shift-Tab focus · Enter acts · Esc back · PgUp/PgDn scroll · F1 help",
            help: None,
            history_open: false,
            action_focus: None,
            hits: Vec::new(),
            drawn: None,
            draw_area: Rect::default(),
            input_ready: false,
            settled_actions: Vec::new(),
            paint_complete: false,
            auth_revision: 0,
            prompt_active: false,
            diagnostics: String::new(),
            diagnostic_seq: None,
            failure: String::new(),
            attention: None,
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
        if self.binding != binding {
            self.invalidate_presentation();
            self.diagnostic_seq = None;
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
            self.configuration = Value::Null;
            self.settings.clear();
            if !established {
                self.settings_open = false;
                self.settings_scroll = 0;
                self.history_open = false;
            }
            self.settings_rows.clear();
            self.action_focus = None;
            self.prompt_active = false;
            self.hits.clear();
            self.diagnostic_seq = None;
        }
        if self.target.is_none() {
            self.lifecycle = "unavailable";
            self.failure.clear();
            self.attention = None;
            self.agent.clear();
            self.foreground = None;
            self.foreground_state.clear();
            self.authentication.clear();
            self.work.clear();
            self.resource.clear();
            if self.settings.is_empty() {
                self.settings = "Settings unreported · model: unreported · mode: unreported · effort: unreported\nNo confirmed configuration for this exact target.\nMode metadata is not a permission guarantee.".to_owned();
            }
            return;
        }
        // A delayed inspection cannot roll canonical lifecycle/content backwards.
        // Authority loss above still invalidates drafts, even on an older page.
        let seq = projection["seq"].as_u64().unwrap_or(0);
        if !changed && seq < self.projection_seq {
            return;
        }
        let refresh_interactions = changed || seq != self.projection_seq;
        if self.configuration != projection["configuration"] || self.settings.is_empty() {
            self.configuration = projection["configuration"].clone();
            self.settings_rows.clear();
            self.settings_scroll = 0;
            let configuration = &self.configuration;
            let reported = &configuration["reported"];
            let current = |key: &str| {
                reported[key]["current"]
                    .as_str()
                    .map(|value| inert(value, 256))
                    .unwrap_or_else(|| "unreported".to_owned())
            };
            let requested = |key: &str| {
                configuration["requested"][key]
                    .as_str()
                    .map(|value| inert(value, 256))
                    .unwrap_or_else(|| "agent default".to_owned())
            };
            self.settings = format!(
                "Settings [{}] · model: {} · mode: {} · effort: {}\nRequested · model: {} · mode: {} · effort: {} · options: {}\nMode metadata is not a permission guarantee. Reported options: {}{}",
                configuration["state"].as_str().unwrap_or("unreported"),
                current("model"),
                current("mode"),
                current("effort"),
                requested("model"),
                requested("mode"),
                requested("effort"),
                display_value(&configuration["requested"]["options"], 512),
                display_value(&reported["options"], 512),
                configuration["failure"]
                    .as_str()
                    .map(|failure| format!(" · failure: {} (no fallback)", inert(failure, 256)))
                    .unwrap_or_default(),
            );
        }
        self.projection_seq = seq;
        let lifecycle = match projection["lifecycle"].as_str() {
            Some("starting") => "starting",
            Some("auth-required") => "auth-required",
            Some("ready") => "ready",
            Some("failed") => "failed",
            Some("closing") => "closing",
            Some("ended") => "ended",
            _ => "unavailable",
        };
        if self.lifecycle != lifecycle
            || self.foreground.as_deref() != identifier(&projection["foreground"]["prompt_id"])
        {
            self.invalidate_presentation();
        }
        self.lifecycle = lifecycle;
        if self.lifecycle != "ready" && self.draft_binding.is_some() {
            self.clear_draft();
            self.notice = "Draft discarded: session is no longer ready. Nothing submitted.";
        }
        self.foreground = identifier(&projection["foreground"]["prompt_id"]).map(str::to_owned);
        let foreground_state = display_value(&projection["foreground"]["state"], 128);
        if self.foreground_state != foreground_state {
            self.invalidate_presentation();
        }
        self.foreground_state = foreground_state;
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
        self.failure.clear();
        for key in ["failure", "resource_failure"] {
            if let Some(value) = projection.get(key).filter(|value| !value.is_null()) {
                let _ = write!(self.failure, "{key}: {} ", display_value(value, 512));
            }
        }
        if let Some(value) = self
            .configuration
            .get("failure")
            .filter(|value| !value.is_null())
        {
            let _ = write!(
                self.failure,
                "configuration: {} (no fallback)",
                display_value(value, 512)
            );
        }
        if let Some(value) = projection["resource"]
            .get("failure")
            .filter(|value| !value.is_null())
        {
            let _ = write!(self.failure, "resource: {} ", display_value(value, 512));
        }
        self.attention = match projection["attention"]["state"].as_str() {
            Some("needs-answer") => Some("needs answer"),
            Some("permission-needed") => Some("permission needed"),
            Some("working") => Some("working"),
            Some("cancelling") => Some("cancelling"),
            Some("ready") => Some("ready"),
            Some("finished") => Some("finished"),
            Some("auth-required") => Some("authentication required"),
            Some("starting") => Some("starting"),
            Some("closing") => Some("closing"),
            Some("failed") => Some("failed"),
            _ => None,
        };
        if (self.lifecycle == "failed" || self.attention == Some("failed"))
            && self.failure.is_empty()
        {
            self.failure =
                "Agent failed · inspect the canonical error in History / Details.".to_owned();
        }
        if refresh_interactions {
            self.diagnostic_seq = None;
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
            self.interaction_gap =
                "Interaction projection exceeds local bounds; no response controls available.";
            return;
        }
        let selected = self
            .interaction
            .and_then(|i| self.interactions.get(i))
            .map(|item| (item.id.clone(), item.revision, item.pending()));
        if self.interactions.iter().any(|item| {
            item.binding.is_some()
                && !details.iter().any(|detail| {
                    detail["interaction_id"].as_str() == Some(item.id.as_str())
                        && detail["revision"].as_u64() == Some(item.revision)
                })
        }) {
            self.notice = "Interaction/revision changed; original local response discarded, never rebound or replayed.";
        }
        self.interactions.retain(|item| {
            details.iter().any(|detail| {
                detail["interaction_id"].as_str() == Some(item.id.as_str())
                    && detail["revision"].as_u64() == Some(item.revision)
            })
        });
        // Immutable revision material is measured only on arrival/change, not
        // reserialized on every transcript update or frame.
        let mut sizes = [0usize; INTERACTIONS];
        // Broker content accounting excludes the fixed-size server-generated
        // record metadata. Reserve at most 512 additional bytes per record.
        let mut remaining = INTERACTION_BYTES + INTERACTIONS * 512;
        for (index, detail) in details.iter().enumerate() {
            let cached = self.interactions.iter().find(|item| {
                detail["interaction_id"].as_str() == Some(item.id.as_str())
                    && detail["revision"].as_u64() == Some(item.revision)
                    && [
                        "state",
                        "selected_work_id",
                        "delivery",
                        "application",
                        "selection",
                    ]
                    .iter()
                    .all(|key| item.detail.get(*key) == detail.get(*key))
            });
            let size = cached.map(|item| item.content_bytes).or_else(|| {
                let mut budget = JsonBudget { remaining };
                serde_json::to_writer(&mut budget, detail).ok()?;
                Some(remaining - budget.remaining)
            });
            let Some(size) = size.filter(|size| *size <= remaining) else {
                self.interactions.clear();
                self.interaction = None;
                self.interaction_gap = "Interaction projection exceeds bounded content/metadata capacity; no response was captured.";
                return;
            };
            sizes[index] = size;
            remaining -= size;
        }
        let mut new_pending = None;
        for (index, detail) in details.iter().enumerate() {
            let Some(id) = identifier(&detail["interaction_id"]) else {
                continue;
            };
            let Some(revision) = detail["revision"].as_u64() else {
                continue;
            };
            if let Some(item) = self
                .interactions
                .iter_mut()
                .find(|item| item.id == id && item.revision == revision)
            {
                item.content_bytes = sizes[index];
                // Schemas/context are immutable within one revision. Only
                // changed canonical disposition/selection material is copied.
                for key in [
                    "state",
                    "selected_work_id",
                    "delivery",
                    "application",
                    "selection",
                ] {
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
                    item.status =
                        "Canonical disposition changed; local draft cannot be replayed.".to_owned();
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
            .as_ref()
            .and_then(|(id, revision, was_pending)| {
                self.interactions.iter().position(|item| {
                    item.id == *id
                        && item.revision == *revision
                        && (!*was_pending || item.pending())
                })
            })
            .or(if self.prompt_active {
                None
            } else {
                new_pending
            });
        if selected.as_ref().is_some_and(|(id, revision, _)| {
            self.interaction
                .and_then(|index| self.interactions.get(index))
                .is_none_or(|item| item.id != *id || item.revision != *revision)
        }) {
            self.action_focus = None;
            self.invalidate_presentation();
        }
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
                    label: display_value(&method["method_id"], 2048),
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
            self.auth_revision = self.auth_revision.wrapping_add(1);
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
            let (text, body) = event_text(event, seq, kind);
            let rows = wrap_rows(&text[body.clone()], self.history_width.max(1));
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
                body,
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

    fn painted(&self) -> Painted {
        Painted {
            target: self.target.clone(),
            binding: self.binding.clone(),
            interaction: self
                .interaction
                .and_then(|index| self.interactions.get(index))
                .map(|item| {
                    (
                        item.id.clone(),
                        item.revision,
                        if item.reviewed || item.detail["kind"] == "permission" {
                            item.paint_revision
                        } else {
                            0
                        },
                        item.reviewed,
                        item.captured.clone(),
                    )
                }),
            foreground: self.foreground.clone(),
            foreground_state: self.foreground_state.clone(),
            lifecycle: self.lifecycle,
            authentication: (self.auth_revision, self.auth_selected),
            covered: self.settings_open || self.history_open || self.help.is_some(),
        }
    }

    /// Covered or resized presentations cannot lend their old painted intent
    /// to a later key. Drafts and reviewed values remain local and intact.
    pub(super) fn invalidate_presentation(&mut self) {
        self.hits.clear();
        self.drawn = None;
        self.input_ready = false;
        self.settled_actions.clear();
        self.paint_complete = false;
    }

    /// The parent calls this only after a successful, actually visible draw
    /// with the terminal input queue empty. A subsequent paint exposes armed
    /// mutations; already queued events never become deferred consent.
    pub(super) fn after_draw(&mut self) -> bool {
        if !self.needs_input_fence() {
            return false;
        }
        self.input_ready = true;
        self.settled_actions.clear();
        self.settled_actions.extend(
            self.hits
                .iter()
                .filter(|hit| hit.fence_eligible)
                .map(|hit| hit.action.clone()),
        );
        true
    }

    /// Request another bounded-cadence frame while visible mutation choices
    /// await an input-empty boundary, even when no public input event arrived.
    pub(super) fn needs_input_fence(&self) -> bool {
        if !self.paint_complete
            || self.help.is_some()
            || self.settings_open
            || self.history_open
            || self.drawn.as_ref() != Some(&self.painted())
        {
            return false;
        }
        self.hits
            .iter()
            .any(|hit| hit.fence_eligible && !self.settled_actions.contains(&hit.action))
    }

    fn activate_painted(&mut self, action: Action) -> Option<Value> {
        let Some(hit) = self.hits.iter().find(|hit| hit.action == action) else {
            self.notice = "Action is not painted; nothing submitted. Review the current screen.";
            return None;
        };
        let painted = hit.painted.clone();
        let armed = hit.armed;
        self.activate(action, &painted, armed)
    }

    fn actions(&self) -> Vec<Action> {
        if self.settings_open || self.history_open {
            return vec![Action::Back, Action::Help];
        }
        let mut actions = Vec::new();
        actions.push(Action::Prompt);
        if let Some(item) = self
            .interaction
            .and_then(|index| self.interactions.get(index))
        {
            if item.pending() && item.supported && item.captured.is_none() && self.binding.is_some()
            {
                if item.detail["kind"] == "form" {
                    actions.extend((0..item.fields.len()).map(Action::Field));
                    if item.form_action_offered("accept") {
                        actions.push(Action::Review(item.reviewed));
                    }
                    if item.decline_offered() {
                        actions.push(Action::Decline);
                    }
                    if item.form_action_offered("cancel") {
                        actions.push(Action::Cancel);
                    }
                } else if let Some(options) = item.detail["options"].as_array() {
                    actions.extend((0..options.len()).map(Action::Permission));
                }
            }
        } else if self.lifecycle == "ready" && self.binding.is_some() && self.foreground.is_none() {
            actions.push(Action::Send);
        }
        if self.lifecycle == "auth-required" {
            actions.extend(
                self.authentication
                    .iter()
                    .enumerate()
                    .map(|(index, method)| Action::Authenticate(index, method.id.clone())),
            );
        }
        if let Some(id) = &self.foreground {
            actions.push(Action::Interrupt(id.clone()));
        }
        actions.extend([
            Action::Questions,
            Action::History,
            Action::Details,
            Action::Help,
        ]);
        actions
    }

    fn focus_action(&mut self, action: Action) {
        match action {
            Action::Field(index) => {
                if let Some(item) = self.interaction.and_then(|i| self.interactions.get_mut(i)) {
                    item.field = index;
                    item.focus = true;
                    item.paint_revision = item.paint_revision.wrapping_add(1);
                    item.dirty = true;
                }
                self.action_focus = None;
            }
            Action::Prompt => {
                self.interaction = None;
                self.prompt_active = true;
                self.action_focus = None;
            }
            Action::Permission(index) => {
                if let Some(item) = self.interaction.and_then(|i| self.interactions.get_mut(i)) {
                    if !item.editable(self.binding.as_ref(), &mut self.notice) {
                        return;
                    }
                    item.option = index;
                    item.paint_revision = item.paint_revision.wrapping_add(1);
                    item.focus = true;
                    item.dirty = true;
                }
                self.action_focus = Some(Action::Permission(index));
            }
            Action::Authenticate(index, id) => {
                self.auth_selected = index;
                self.action_focus = Some(Action::Authenticate(index, id));
            }
            _ => self.action_focus = Some(action),
        }
    }

    fn tab(&mut self, backwards: bool) {
        let actions = self.actions();
        let current = self.action_focus.clone().unwrap_or_else(|| {
            self.interaction
                .and_then(|i| self.interactions.get(i))
                .map_or(Action::Prompt, |item| Action::Field(item.field))
        });
        let index = actions.iter().position(|action| *action == current);
        let next = match index {
            Some(index) if backwards => (index + actions.len() - 1) % actions.len(),
            Some(index) => (index + 1) % actions.len(),
            None if backwards => actions.len() - 1,
            None => 0,
        };
        self.focus_action(actions[next].clone());
    }

    fn activate(&mut self, action: Action, painted: &Painted, armed: bool) -> Option<Value> {
        if *painted != self.painted() || !self.actions().contains(&action) {
            self.notice = "Rendered target, revision or action changed; nothing submitted. Review the current screen.";
            return None;
        }
        let context_changed = match &action {
            Action::Permission(index) => self
                .interaction
                .and_then(|i| self.interactions.get(i))
                .is_some_and(|item| item.option != *index),
            Action::Authenticate(index, _) => self.auth_selected != *index,
            _ => false,
        };
        if context_changed
            && UnicodeWidthStr::width(self.action_label(&action).as_str()) + 2
                > usize::from(self.draw_area.width)
        {
            // A contextual control first reveals its exact canonical choice.
            // Mutation needs a fresh visible/drained paint of that selection.
            self.focus_action(action);
            self.invalidate_presentation();
            return None;
        }
        if matches!(
            action,
            Action::Review(_)
                | Action::Decline
                | Action::Cancel
                | Action::Permission(_)
                | Action::Send
                | Action::Authenticate(_, _)
                | Action::Interrupt(_)
        ) && (!armed
            || self.drawn.as_ref() != Some(painted)
            || action.needs_input_fence()
                && (!self.input_ready || !self.settled_actions.contains(&action)))
        {
            self.notice = "Wait for the current visible choices and input to settle; nothing submitted or queued.";
            return None;
        }
        if matches!(
            action,
            Action::Prompt | Action::Details | Action::History | Action::Questions | Action::Back
        ) {
            self.invalidate_presentation();
        }
        match action {
            Action::Field(_) | Action::Prompt => self.focus_action(action),
            Action::Review(reviewed) => {
                let item = self.interactions.get_mut(self.interaction?)?;
                if item.reviewed == reviewed {
                    let result = item.answer("accept", self.binding.as_ref(), &mut self.notice);
                    self.action_focus = item.reviewed.then_some(Action::Review(true));
                    return result;
                }
            }
            Action::Decline | Action::Cancel => {
                let choice = if action == Action::Decline {
                    "decline"
                } else {
                    "cancel"
                };
                return self.interactions.get_mut(self.interaction?)?.answer(
                    choice,
                    self.binding.as_ref(),
                    &mut self.notice,
                );
            }
            Action::Permission(index) => {
                let item = self.interactions.get_mut(self.interaction?)?;
                item.option = index;
                return item.permission(self.binding.as_ref(), &mut self.notice);
            }
            Action::Send => return self.submit(),
            Action::Authenticate(index, id) => {
                if self
                    .authentication
                    .get(index)
                    .is_some_and(|method| method.id == id)
                {
                    self.auth_selected = index;
                    return self.authenticate();
                }
            }
            Action::Interrupt(id) if self.foreground.as_ref() == Some(&id) => {
                return self.interrupt();
            }
            Action::Details => {
                self.settings_open = true;
                self.settings_scroll = 0;
                self.diagnostic_seq = None;
                self.action_focus = None;
            }
            Action::History => {
                self.history_open = true;
                self.action_focus = None;
            }
            Action::Questions => {
                self.settings_open = false;
                self.history_open = false;
                self.prompt_active = false;
                self.interaction = match self.interaction {
                    Some(index) if index + 1 < self.interactions.len() => Some(index + 1),
                    Some(_) => None,
                    None if !self.interactions.is_empty() => Some(0),
                    None => None,
                };
                self.action_focus = None;
            }
            Action::Help => {
                self.invalidate_presentation();
                self.help = Some(self.context_help());
            }
            Action::Back => {
                self.settings_open = false;
                self.history_open = false;
                self.action_focus = None;
            }
            Action::Interrupt(_) => {}
        }
        None
    }

    /// None always means consumed/local, never permission for native forwarding.
    pub(super) fn event(&mut self, event: &Event) -> Option<Value> {
        if let Some(help) = &mut self.help {
            if help.event(event) {
                self.invalidate_presentation();
                self.help = None;
            }
            return None;
        }
        if matches!(event, Event::Resize(_, _)) {
            self.invalidate_presentation();
            return None;
        }
        if let Event::Mouse(mouse) = event
            && mouse.kind == MouseEventKind::Down(MouseButton::Left)
        {
            let point = (mouse.column, mouse.row);
            if let Some(hit) = self.hits.iter().find(|hit| hit.rect.contains(point.into())) {
                let action = hit.action.clone();
                let painted = hit.painted.clone();
                let armed = hit.armed;
                self.action_focus = Some(action.clone());
                return self.activate(action, &painted, armed);
            }
            return None;
        }
        if let Event::Key(key) = event
            && key.kind == KeyEventKind::Press
        {
            if key.code == KeyCode::F(1) && key.modifiers.is_empty() {
                self.invalidate_presentation();
                self.help = Some(self.context_help());
                return None;
            }
            if key.code == KeyCode::F(10) && key.modifiers.is_empty() {
                if self.settings_open {
                    self.invalidate_presentation();
                    self.settings_open = false;
                    self.action_focus = None;
                } else {
                    self.history_open = false;
                    let painted = self.painted();
                    return self.activate(Action::Details, &painted, false);
                }
                return None;
            }
            if key.code == KeyCode::F(9) && key.modifiers.is_empty() {
                self.settings_open = false;
                self.history_open = false;
                let painted = self.painted();
                return self.activate(Action::Questions, &painted, false);
            }
            if matches!(key.code, KeyCode::Tab | KeyCode::BackTab)
                && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT)
            {
                self.tab(key.code == KeyCode::BackTab || key.modifiers == KeyModifiers::SHIFT);
                return None;
            }
            if key.code == KeyCode::Esc && key.modifiers.is_empty() {
                self.invalidate_presentation();
                if self.settings_open || self.history_open {
                    self.settings_open = false;
                    self.history_open = false;
                    self.action_focus = None;
                } else if self.action_focus.is_some() {
                    self.interaction = None;
                    self.action_focus = None;
                    self.prompt_active = true;
                } else {
                    self.action_focus = Some(Action::Details);
                }
                return None;
            }
            if self.action_focus.is_some() && key.modifiers.is_empty() {
                match key.code {
                    KeyCode::Up | KeyCode::Down if !self.settings_open && !self.history_open => {
                        self.tab(key.code == KeyCode::Up);
                        return None;
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        let action = self.action_focus.clone()?;
                        return self.activate_painted(action);
                    }
                    KeyCode::Char('?') => {
                        self.invalidate_presentation();
                        self.help = Some(self.context_help());
                        return None;
                    }
                    KeyCode::Char('j') | KeyCode::Right => {
                        self.tab(false);
                        return None;
                    }
                    KeyCode::Char('k') | KeyCode::Left => {
                        self.tab(true);
                        return None;
                    }
                    KeyCode::Char(_) => return None,
                    _ => {}
                }
            }
        }
        if self.action_focus.is_some() {
            match event {
                Event::Paste(_) => return None,
                Event::Key(key)
                    if !matches!(
                        key.code,
                        KeyCode::F(5..=8) | KeyCode::PageUp | KeyCode::PageDown
                    ) && !(matches!(
                        key.code,
                        KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End
                    ) && (self.settings_open || self.history_open))
                        && !(matches!(key.code, KeyCode::Char('c' | 'C'))
                            && key.modifiers == KeyModifiers::CONTROL) =>
                {
                    return None;
                }
                _ => {}
            }
        }
        if self.history_open {
            match event {
                Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                    KeyCode::PageUp => self.scroll_up(self.page_rows),
                    KeyCode::PageDown => self.scroll_down(self.page_rows),
                    KeyCode::Up => self.scroll_up(1),
                    KeyCode::Down => self.scroll_down(1),
                    KeyCode::Home => {
                        self.scroll = self.history_rows;
                        self.clamp_scroll();
                    }
                    KeyCode::End => self.scroll = 0,
                    _ => {}
                },
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => self.scroll_up(3),
                    MouseEventKind::ScrollDown => self.scroll_down(3),
                    _ => {}
                },
                _ => {}
            }
            return None;
        }
        if self.settings_open {
            match event {
                Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                    KeyCode::Esc if key.modifiers.is_empty() => self.settings_open = false,
                    KeyCode::PageUp => {
                        self.settings_scroll =
                            self.settings_scroll.saturating_sub(self.settings_page);
                    }
                    KeyCode::PageDown => {
                        self.settings_scroll =
                            self.settings_scroll.saturating_add(self.settings_page);
                    }
                    KeyCode::Up => self.settings_scroll = self.settings_scroll.saturating_sub(1),
                    KeyCode::Down => self.settings_scroll = self.settings_scroll.saturating_add(1),
                    KeyCode::Home => self.settings_scroll = 0,
                    KeyCode::End => self.settings_scroll = self.settings_rows.len(),
                    _ => {}
                },
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        self.settings_scroll = self.settings_scroll.saturating_sub(3);
                    }
                    MouseEventKind::ScrollDown => {
                        self.settings_scroll = self.settings_scroll.saturating_add(3);
                    }
                    _ => {}
                },
                _ => {}
            }
            // This read-only surface never edits/replays a draft, authenticates,
            // interrupts, answers an interaction or requests control.
            return None;
        }
        // Compatibility accelerators use exactly the same immutable painted
        // action as mouse/Enter. Never construct intent from newly synced data.
        if let Event::Key(key) = event
            && key.kind == KeyEventKind::Press
        {
            let action = match (key.code, key.modifiers) {
                (KeyCode::F(5), KeyModifiers::NONE) => self.hits.iter().find_map(|hit| {
                    matches!(hit.action, Action::Review(_)).then(|| hit.action.clone())
                }),
                (KeyCode::F(6), KeyModifiers::NONE) => Some(Action::Decline),
                (KeyCode::F(7), KeyModifiers::NONE) => Some(Action::Cancel),
                (KeyCode::F(8), KeyModifiers::NONE) => self.hits.iter().find_map(|hit| {
                    matches!(&hit.action, Action::Authenticate(index, _) if *index == self.auth_selected)
                        .then(|| hit.action.clone())
                }),
                (KeyCode::Char('c' | 'C'), KeyModifiers::CONTROL) => {
                    self.hits.iter().find_map(|hit| {
                        matches!(hit.action, Action::Interrupt(_)).then(|| hit.action.clone())
                    })
                }
                (KeyCode::Enter, KeyModifiers::NONE) => {
                    self.interaction
                        .and_then(|i| self.interactions.get(i))
                        .filter(|item| item.detail["kind"] == "permission")
                        .map(|item| Action::Permission(item.option))
                        .or_else(|| self.interaction.is_none().then_some(Action::Send))
                }
                _ => None,
            };
            if let Some(action) = action {
                return self.activate_painted(action);
            }
            if key.modifiers.is_empty() && matches!(key.code, KeyCode::F(5..=8))
                || key.modifiers == KeyModifiers::CONTROL
                    && matches!(key.code, KeyCode::Char('c' | 'C'))
            {
                return None;
            }
        }
        if let Some(index) = self.interaction {
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
                match key.code {
                    KeyCode::Enter
                        if key.modifiers == KeyModifiers::ALT
                            || key.modifiers == KeyModifiers::SHIFT =>
                    {
                        self.insert("\n");
                        None
                    }
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
        self.prompt_active = true;
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

    /// Proven non-admission only: local queue rejection or definite daemon error.
    /// Restore its original binding, never a new grant or an automatic retry.
    pub(super) fn restore_unadmitted(&mut self, mut action: Value) {
        self.notice = "Action not admitted; deliberate new action required. No automatic retry.";
        if matches!(action["op"].as_str(), Some("acp-answer" | "acp-permission")) {
            if let Some(interaction) = self.interactions.iter_mut().find(|item| {
                action["interaction_id"].as_str() == Some(item.id.as_str())
                    && action["revision"].as_u64() == Some(item.revision)
            }) {
                interaction.restore_unadmitted(&action, self.binding.as_ref());
                self.action_focus = None;
                self.invalidate_presentation();
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
        self.prompt_active = true;
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
        self.invalidate_presentation();
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
        self.invalidate_presentation();
        Some(action)
    }

    fn clear_draft(&mut self) {
        self.draft.clear();
        self.draft_binding = None;
        self.prompt_active = false;
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
        if self.interaction.is_none() && !self.settings_open {
            self.history_open = true;
            self.action_focus = Some(Action::Back);
        }
        self.scroll = self.scroll.saturating_add(rows);
        self.clamp_scroll();
    }

    fn scroll_down(&mut self, rows: usize) {
        if self.interaction.is_none() && !self.settings_open {
            self.history_open = true;
            self.action_focus = Some(Action::Back);
        }
        self.scroll = self.scroll.saturating_sub(rows);
    }

    fn clamp_scroll(&mut self) {
        self.scroll = self
            .scroll
            .min(self.history_rows.saturating_sub(self.page_rows));
    }

    fn action_label(&self, action: &Action) -> String {
        match action {
            Action::Field(index) => format!("Field {}", index + 1),
            Action::Permission(index) => self
                .interaction
                .and_then(|i| self.interactions.get(i))
                .map_or_else(String::new, |item| item.option_label(*index)),
            Action::Review(false) => "Review (F5)".to_owned(),
            Action::Review(true) => "Submit (F5)".to_owned(),
            Action::Decline => "Decline (F6)".to_owned(),
            Action::Cancel => "Cancel (F7)".to_owned(),
            Action::Prompt => "Prompt".to_owned(),
            Action::Send => "Send".to_owned(),
            Action::Authenticate(index, _) => self
                .authentication
                .get(*index)
                .map_or_else(String::new, AuthMethod::caption),
            Action::Interrupt(_) => "Interrupt".to_owned(),
            Action::Details => "Details F10".to_owned(),
            Action::History => "History".to_owned(),
            Action::Questions => "Questions F9".to_owned(),
            Action::Help => "Help F1".to_owned(),
            Action::Back => "Back Esc".to_owned(),
        }
    }

    fn context_help(&self) -> super::view::Help {
        if self.lifecycle == "auth-required"
            && let Some(method) = self.authentication.get(self.auth_selected)
        {
            return super::view::Help::new(format!(
                "Authentication method {} of {}\n{}\n\nOnly advertised agent-type authentication is supported. Client/terminal methods never open a URL, credentials or process. Tab selects a method; F8, Enter or a mouse action authenticates only the exact visible method after input settles. Esc returns without authenticating.\n\n{HELP}",
                self.auth_selected + 1,
                self.authentication.len(),
                method.caption()
            ));
        }
        super::view::Help::new(HELP)
    }

    fn action_rows(&self, actions: &[Action], width: u16) -> usize {
        if width == 0 || actions.is_empty() {
            return 0;
        }
        let mut rows = 1;
        let mut used = 0;
        for action in actions {
            let columns = (UnicodeWidthStr::width(self.action_label(action).as_str()) + 2)
                .min(usize::from(width));
            if used > 0 && used + columns > usize::from(width) {
                rows += 1;
                used = 0;
            }
            used += columns + 1;
        }
        rows
    }

    fn draw_actions(&mut self, frame: &mut Frame<'_>, area: Rect, actions: &[Action]) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let start = if self.action_rows(actions, area.width) <= usize::from(area.height) {
            0
        } else {
            self.action_focus
                .as_ref()
                .and_then(|focus| actions.iter().position(|action| action == focus))
                .unwrap_or_else(|| {
                    self.interaction
                        .and_then(|i| self.interactions.get(i))
                        .and_then(|item| {
                            actions
                                .iter()
                                .position(|action| *action == Action::Permission(item.option))
                        })
                        .or_else(|| {
                            if self.lifecycle != "auth-required" {
                                return None;
                            }
                            let method = self.authentication.get(self.auth_selected)?;
                            actions.iter().position(|action| {
                                matches!(action, Action::Authenticate(index, id)
                                    if *index == self.auth_selected && id == &method.id)
                            })
                        })
                        .unwrap_or(0)
                })
        };
        let mut x = area.x;
        let mut y = area.y;
        for action in actions.iter().skip(start) {
            let label = self.action_label(action);
            let width =
                (UnicodeWidthStr::width(label.as_str()) + 2).min(usize::from(area.width)) as u16;
            if x > area.x && x.saturating_add(width) > area.right() {
                x = area.x;
                y = y.saturating_add(1);
            }
            if y >= area.bottom() {
                break;
            }
            let rect = Rect::new(x, y, width, 1);
            let focused = self.action_focus.as_ref() == Some(action)
                || (self.action_focus.is_none()
                    && match action {
                        Action::Permission(index) => self
                            .interaction
                            .and_then(|i| self.interactions.get(i))
                            .is_some_and(|item| item.option == *index),
                        Action::Authenticate(index, _) => {
                            self.lifecycle == "auth-required" && *index == self.auth_selected
                        }
                        _ => false,
                    });
            let contextual = UnicodeWidthStr::width(label.as_str()) + 2 > usize::from(area.width);
            let selected = self.interaction.and_then(|i| self.interactions.get(i));
            let context_ready = !contextual
                || match action {
                    Action::Permission(index) => selected.is_some_and(|item| item.option == *index),
                    Action::Authenticate(index, _) => self.auth_selected == *index,
                    _ => true,
                };
            let label = match action {
                Action::Permission(index) if contextual => {
                    format!(
                        "{} option {} · context",
                        if context_ready { "Choose" } else { "Review" },
                        index + 1
                    )
                }
                Action::Authenticate(index, _) if contextual => format!(
                    "{} method {} · context",
                    if context_ready { "Choose" } else { "Review" },
                    index + 1
                ),
                _ => label,
            };
            let fence_eligible = action.needs_input_fence()
                && self.paint_complete
                && context_ready
                && UnicodeWidthStr::width(label.as_str())
                    + if *action == Action::Review(true) {
                        0
                    } else {
                        2
                    }
                    <= usize::from(width);
            let armed = self.paint_complete
                && context_ready
                && (!action.needs_input_fence()
                    || fence_eligible && self.input_ready && self.settled_actions.contains(action));
            let label = if *action == Action::Review(true) {
                if armed {
                    label
                } else {
                    "Review settling".to_owned()
                }
            } else if action.needs_input_fence() && context_ready {
                // Keep the exact choice visible before Empty; brackets only
                // appear on the subsequent armed repaint.
                if armed {
                    format!("[{label}]")
                } else {
                    format!("({label})")
                }
            } else {
                label
            };
            let label = fit_label(&label, usize::from(width));
            super::view::button(frame, rect, &label, focused);
            self.hits.push(Hit {
                rect,
                action: action.clone(),
                painted: self.painted(),
                armed,
                fence_eligible,
            });
            if let Action::Permission(index) = action
                && context_ready
                && let Some(item) = self.interaction.and_then(|i| self.interactions.get_mut(i))
            {
                item.visible_options.push(*index);
            }
            x = x.saturating_add(width).saturating_add(1);
        }
    }

    pub(super) fn draw(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let painted = self.painted();
        if self.drawn.as_ref() != Some(&painted) || self.draw_area != area {
            self.input_ready = false;
            self.settled_actions.clear();
        }
        self.drawn = Some(painted);
        self.draw_area = area;
        self.paint_complete = false;
        self.hits.clear();
        for item in &mut self.interactions {
            item.visible_options.clear();
        }
        if area.width == 0 || area.height == 0 {
            self.settled_actions.clear();
            return;
        }
        if self.settings_open {
            self.draw_settings(frame, area);
        } else if self.history_open {
            let mut body = area;
            let buttons = take_bottom(&mut body, 1);
            frame.render_widget(
                Paragraph::new("Canonical history · PgUp/PgDn / wheel scroll")
                    .style(Style::default().fg(ACCENT)),
                take_top(&mut body, 1),
            );
            if !self.broker_gap.is_empty() || self.local_gap.is_some() {
                frame.render_widget(
                    Paragraph::new("History has a gap / eviction · exact notice in Details")
                        .style(Style::default().fg(Color::Yellow)),
                    take_top(&mut body, 1),
                );
            }
            self.draw_history(frame, body);
            self.draw_actions(frame, buttons, &[Action::Back, Action::Help]);
        } else {
            let mut body = area;
            let nav = take_bottom(&mut body, 1);
            let notice = if body.height >= 7 {
                take_bottom(&mut body, 1)
            } else {
                Rect::default()
            };
            let actions = self.actions();
            let primary: Vec<Action> = actions
                .iter()
                .filter(|action| {
                    !matches!(
                        action,
                        Action::Field(_)
                            | Action::Prompt
                            | Action::Questions
                            | Action::History
                            | Action::Details
                            | Action::Help
                    )
                })
                .cloned()
                .collect();
            let controls = if primary.is_empty() {
                Rect::default()
            } else {
                // Primary controls wrap, but cannot displace the entire
                // question/editor. Overflow follows the focused offered action.
                let rows = self
                    .action_rows(&primary, body.width)
                    .min(4)
                    .min(usize::from(body.height.saturating_sub(3)))
                    .max(1) as u16;
                take_bottom(&mut body, rows)
            };
            if body.height >= 5 {
                let pending = self
                    .interactions
                    .iter()
                    .filter(|item| item.pending())
                    .count();
                let state = if self
                    .interactions
                    .iter()
                    .any(|item| item.pending() && item.detail["kind"] == "permission")
                {
                    "permission needed"
                } else if pending > 0 {
                    "needs answer"
                } else if self.lifecycle != "ready" {
                    self.lifecycle
                } else if self.foreground_state.contains("cancelling") {
                    "cancelling"
                } else if self.foreground.is_some() {
                    "working"
                } else {
                    self.attention.unwrap_or_else(|| {
                        self.history
                            .iter()
                            .rev()
                            .find_map(|entry| match entry.kind {
                                "completion" => Some("finished"),
                                "error" => Some("failed"),
                                "user-message" => Some("ready"),
                                _ => None,
                            })
                            .unwrap_or("ready")
                    })
                };
                frame.render_widget(
                    Paragraph::new(format!(
                        "ACP · {state}{}",
                        if self.binding.is_some() {
                            ""
                        } else {
                            " · read-only"
                        }
                    ))
                    .style(Style::default().fg(ACCENT)),
                    take_top(&mut body, 1),
                );
            }
            if !self.failure.is_empty() {
                frame.render_widget(
                    Paragraph::new(self.failure.as_str()).style(Style::default().fg(Color::Red)),
                    take_top(&mut body, 1),
                );
            } else if !self.interaction_gap.is_empty() {
                frame.render_widget(
                    Paragraph::new(self.interaction_gap).style(Style::default().fg(Color::Yellow)),
                    take_top(&mut body, 1),
                );
            }
            if let Some(index) = self.interaction {
                // Keep a conversation preview when there is room; the question
                // and focused control have priority on small terminals.
                if !self.interactions[index].pending() || body.height >= 9 {
                    let history_height = if self.interactions[index].pending() {
                        (body.height / 3).min(6)
                    } else {
                        body.height.saturating_sub(3)
                    };
                    let history = take_top(&mut body, history_height);
                    self.draw_conversation(frame, history);
                }
                let editing =
                    self.action_focus.is_none() && self.binding.is_some() && self.help.is_none();
                if let Some(rect) = self.interactions[index].draw(frame, body, editing) {
                    self.paint_complete = body.width > 0
                        && body.height > 0
                        && (!self.interactions[index].reviewed
                            || self.interactions[index].review_visible);
                    self.hits.push(Hit {
                        rect,
                        action: Action::Field(self.interactions[index].field),
                        painted: self.painted(),
                        armed: false,
                        fence_eligible: false,
                    });
                }
                if self.interactions[index].fields.is_empty() {
                    self.paint_complete = body.width > 0
                        && body.height > 0
                        && (!self.interactions[index].reviewed
                            || self.interactions[index].review_visible);
                }
            } else if self.lifecycle == "auth-required" {
                if let Some(method) = self.authentication.get(self.auth_selected) {
                    let context = Paragraph::new(method.caption()).wrap(Wrap { trim: false });
                    if body.height >= 3 {
                        frame.render_widget(
                            context.block(Block::bordered().title("Authentication method")),
                            body,
                        );
                    } else {
                        frame.render_widget(context, body);
                    }
                    self.paint_complete = body.width > 2 && body.height > 0;
                }
            } else {
                if let Some(item) = self.interactions.iter().find(|item| item.pending()) {
                    let message = item.detail["message"]
                        .as_str()
                        .or_else(|| item.detail["schema"]["title"].as_str())
                        .unwrap_or("Question available");
                    frame.render_widget(
                        Paragraph::new(format!(
                            "Pending question: {} · Questions F9",
                            inert(message, 1024)
                        ))
                        .style(Style::default().fg(Color::Yellow)),
                        take_top(&mut body, 1),
                    );
                }
                let editor_height =
                    ((self.editor_rows.len() + 2).clamp(3, 6) as u16).min(body.height);
                let editor = take_bottom(&mut body, editor_height);
                self.draw_conversation(frame, body);
                self.editor_width = usize::from(editor.width.saturating_sub(2)).max(1);
                self.layout_editor();
                self.draw_editor(frame, editor);
                if editor.width > 0 && editor.height > 0 {
                    self.paint_complete = editor.width > 2 && editor.height > 0;
                    self.hits.push(Hit {
                        rect: editor,
                        action: Action::Prompt,
                        painted: self.painted(),
                        armed: false,
                        fence_eligible: false,
                    });
                }
            }
            if !self.paint_complete {
                self.input_ready = false;
            }
            self.draw_actions(frame, controls, &primary);
            self.draw_actions(
                frame,
                nav,
                &[
                    Action::Prompt,
                    Action::Questions,
                    Action::History,
                    Action::Details,
                    Action::Help,
                ],
            );
            let notice_text = if !self.broker_gap.is_empty() || self.local_gap.is_some() {
                "History has a gap / eviction; Details retains the exact notice."
            } else {
                self.notice
            };
            frame.render_widget(
                Paragraph::new(notice_text).style(Style::default().fg(MUTED)),
                notice,
            );
        }
        // An action that disappeared or became clipped cannot inherit a prior
        // Empty boundary when focus/scroll later exposes it again.
        self.settled_actions.retain(|action| {
            self.hits
                .iter()
                .any(|hit| hit.action == *action && hit.fence_eligible)
        });
        if let Some(help) = &mut self.help {
            self.hits.clear();
            self.input_ready = false;
            self.settled_actions.clear();
            self.paint_complete = false;
            self.drawn = None;
            help.draw(frame, area);
        }
    }

    fn draw_conversation(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let width = usize::from(area.width).max(1);
        let limit = usize::from(area.height);
        if limit == 0 {
            return;
        }
        let mut lines = Vec::with_capacity(limit);
        for entry in self.history.iter().rev().filter(|entry| {
            matches!(
                entry.kind,
                "user-message" | "agent-message" | "completion" | "error"
            )
        }) {
            let text = &entry.text[entry.body.clone()];
            let rows: Cow<'_, [Range<usize>]> = if width == self.history_width {
                Cow::Borrowed(&entry.rows)
            } else {
                Cow::Owned(wrap_rows(text, width))
            };
            for row in rows.iter().rev() {
                lines.push(Line::styled(&text[row.clone()], category_style(entry.kind)));
                if lines.len() == limit {
                    break;
                }
            }
            if lines.len() == limit {
                break;
            }
        }
        lines.reverse();
        if lines.is_empty() {
            lines.push(Line::styled(
                "Conversation · no canonical messages yet",
                Style::default().fg(MUTED),
            ));
        }
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn draw_settings(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let mut body = area;
        let buttons = take_bottom(&mut body, 1);
        let footer = if body.height >= 5 {
            take_bottom(&mut body, 1)
        } else {
            Rect::default()
        };
        frame.render_widget(
            Paragraph::new(
                "ACP settings (read-only) · Details\nPgUp/PgDn, arrows, Home/End, wheel · Esc back",
            )
            .style(Style::default().fg(ACCENT)),
            take_top(&mut body, 2),
        );
        if self.diagnostic_seq != Some(self.projection_seq) {
            self.diagnostics = format!(
                "ACP | {} | {}\nSession: {}\nSelected interaction: {}\nCursor {} | {}\n\n{}\n\nAgent: {}\nExact target: {}\nForeground: {} {}\n{}\n{}\n{}\n",
                self.lifecycle,
                if self.binding.is_some() {
                    "owner (input-admission)"
                } else {
                    "observer (read-only)"
                },
                self.target
                    .as_ref()
                    .and_then(|target| target["session_id"].as_str())
                    .unwrap_or("not established"),
                self.interaction
                    .and_then(|index| self.interactions.get(index))
                    .map_or_else(
                        || "none".to_owned(),
                        |item| format!(
                            "{} | revision {}",
                            display_value(&item.detail["interaction_id"], 256),
                            display_value(&item.detail["revision"], 64)
                        )
                    ),
                self.through,
                if self.scroll == 0 {
                    "following"
                } else {
                    "scrollback"
                },
                self.settings,
                self.agent,
                display_value(self.target.as_ref().unwrap_or(&Value::Null), 4096),
                self.foreground
                    .as_deref()
                    .map_or("none".to_owned(), |id| inert(id, 256)),
                self.foreground_state,
                self.broker_gap,
                self.local_gap.map_or(String::new(), |seq| format!(
                    "Local history evicted through {seq}"
                )),
                self.interaction_gap,
            );
            for line in &self.work {
                let _ = writeln!(self.diagnostics, "{line}");
            }
            let _ = writeln!(self.diagnostics, "{}\n{}", self.resource, self.failure);
            for method in &self.authentication {
                let _ = writeln!(
                    self.diagnostics,
                    "Authentication: {} [{}] {}",
                    inert(&method.id, 256),
                    inert(&method.kind, 256),
                    method.label
                );
            }
            if self.auth_clipped {
                self.diagnostics.push_str(
                    "Additional authentication methods omitted by the local display bound.\n",
                );
            }
            for entry in &self.history {
                let _ = writeln!(self.diagnostics, "\n{}", entry.text);
            }
            // Every canonical distinction remains available, including
            // unsupported schemas, completed callbacks and exact option IDs.
            for item in &self.interactions {
                let _ = writeln!(
                    self.diagnostics,
                    "\nCanonical interaction: {}",
                    display_value(&item.detail, INTERACTION_DISPLAY)
                );
            }
            if let Some(item) = self.interaction.and_then(|i| self.interactions.get_mut(i)) {
                let scroll = item.scroll;
                if item.dirty || item.display.is_empty() {
                    item.rebuild(usize::from(body.width).max(1));
                }
                item.scroll = scroll;
                let _ = write!(
                    self.diagnostics,
                    "\nSelected interaction / local draft:\n{}\n{}\n",
                    item.status, item.display
                );
            }
            self.settings_rows.clear();
            self.diagnostic_seq = Some(self.projection_seq);
        }
        if self.settings_width != body.width as usize || self.settings_rows.is_empty() {
            self.settings_width = body.width as usize;
            self.settings_rows = wrap_rows(&self.diagnostics, self.settings_width.max(1));
        }
        self.settings_page = (body.height as usize).max(1);
        self.settings_scroll = self.settings_scroll.min(
            self.settings_rows
                .len()
                .saturating_sub(body.height as usize),
        );
        for (offset, range) in self
            .settings_rows
            .iter()
            .skip(self.settings_scroll)
            .take(body.height as usize)
            .enumerate()
        {
            frame.buffer_mut().set_stringn(
                body.x,
                body.y + offset as u16,
                &self.diagnostics[range.clone()],
                body.width as usize,
                Style::default().fg(MUTED),
            );
        }
        frame.render_widget(
            Paragraph::new(format!(
                "Details rows {}-{}/{} · Application input discarded",
                self.settings_scroll + 1,
                (self.settings_scroll + body.height as usize).min(self.settings_rows.len()),
                self.settings_rows.len()
            ))
            .style(Style::default().fg(MUTED)),
            footer,
        );
        self.draw_actions(frame, buttons, &[Action::Back, Action::Help]);
    }

    fn draw_history(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let width = usize::from(area.width).max(1);
        if self.history_width != width {
            self.history_width = width;
            self.history_rows = 0;
            for entry in &mut self.history {
                entry.rows = wrap_rows(&entry.text[entry.body.clone()], width);
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
                    &entry.text[entry.body.start + row.start..entry.body.start + row.end],
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
        let block = if area.height >= 3 {
            Block::default().borders(Borders::ALL)
        } else {
            Block::default()
        };
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
        if self.interaction.is_none()
            && self.action_focus.is_none()
            && self.help.is_none()
            && self.binding.is_some()
            && self.lifecycle == "ready"
        {
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
    fn form_action_offered(&self, choice: &str) -> bool {
        self.detail["actions"]
            .as_array()
            .is_none_or(|actions| actions.iter().any(|action| action == choice))
    }

    fn decline_offered(&self) -> bool {
        self.form_action_offered("decline")
    }

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
            paint_revision: 0,
            review_visible: false,
            review_values: None,
            status: String::new(),
            supported: false,
            dirty: true,
            focus: false,
            display: String::new(),
            rows: Vec::new(),
            scroll: 0,
            anchor_row: 0,
            page: 1,
            visible_options: Vec::new(),
            question: String::new(),
            question_rows: Vec::new(),
            question_width: 0,
        };
        item.reset_fields();
        for value in [
            &detail["schema"]["title"],
            &detail["message"],
            &detail["schema"]["description"],
            &detail["tool_call"]["title"],
        ] {
            if let Some(text) = value.as_str() {
                if !item.question.is_empty() {
                    item.question.push('\n');
                }
                if !push_inert(&mut item.question, text, INTERACTION_DISPLAY, true) {
                    item.question.push_str(
                        "\n[Question display clipped; complete canonical context in Details]",
                    );
                    break;
                }
            }
        }
        if item.question.is_empty() {
            item.question = if detail["kind"] == "permission" {
                "Permission request · full tool context in Details".to_owned()
            } else {
                "Question · full canonical context in Details".to_owned()
            };
        }
        item
    }

    fn pending(&self) -> bool {
        self.detail["state"] == "pending" && self.detail["selected_work_id"].is_null()
    }

    fn option_label(&self, index: usize) -> String {
        self.detail["options"]
            .as_array()
            .and_then(|options| options.get(index))
            .map_or_else(String::new, |option| {
                format!(
                    "{} [{}]",
                    inert(option["name"].as_str().unwrap_or(""), 1024),
                    inert(option["kind"].as_str().unwrap_or(""), 256)
                )
            })
    }

    fn reset_fields(&mut self) {
        self.fields.clear();
        self.field = 0;
        self.option = 0;
        self.supported = match self.detail["kind"].as_str() {
            Some("form") => self.load_fields().is_some(),
            Some("permission") => self.detail["options"].as_array().is_some_and(|options| {
                !options.is_empty()
                    && options.len() <= OPTIONS
                    && options.iter().all(|option| {
                        identifier(&option["option_id"]).is_some()
                            && option["name"].is_string()
                            && option["kind"].is_string()
                    })
            }),
            _ => false,
        };
        if !self.supported {
            self.fields.clear();
            self.status =
                "Unsupported schema/options; no inferred control or response is available."
                    .to_owned();
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
        // Declared defaults and names belong to the bounded incoming schema.
        // They may require explicit unset before an answer fits 64 KiB.
        let mut draft_budget = JsonBudget {
            remaining: INTERACTION_BYTES,
        };
        for (name, property) in properties {
            let kind = match property["type"].as_str()? {
                "string"
                    if property.get("enum").is_some_and(|v| !v.is_null())
                        || property.get("oneOf").is_some_and(|v| !v.is_null()) =>
                {
                    FieldKind::Enum
                }
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
                if !choices
                    .iter()
                    .any(|choice| choice_allowed(property, kind, choice))
                {
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
                FieldKind::Number | FieldKind::Integer => {
                    default.map(ToString::to_string).unwrap_or_default()
                }
                _ => String::new(),
            };
            let choice = default
                .and_then(Value::as_str)
                .and_then(|value| {
                    field_choices(property, kind)?
                        .iter()
                        .position(|choice| choice_value(choice) == Some(value))
                })
                .or_else(|| {
                    field_choices(property, kind)?
                        .iter()
                        .position(|choice| choice_allowed(property, kind, choice))
                })
                .unwrap_or(0);
            let value = if matches!(
                kind,
                FieldKind::Text | FieldKind::Number | FieldKind::Integer
            ) {
                default.map(|_| Value::Null)
            } else {
                default.cloned()
            };
            self.fields.push(Field {
                name: name.clone(),
                kind,
                value,
                cursor: text.len(),
                text,
                choice,
            });
        }
        Some(())
    }

    fn discard(&mut self) {
        self.binding = None;
        self.captured = None;
        self.paint_revision = self.paint_revision.wrapping_add(1);
        self.reviewed = false;
        self.review_values = None;
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
        self.paint_revision = self.paint_revision.wrapping_add(1);
        self.reviewed = false;
        self.review_values = None;
        self.status = "Local draft only; F5 validates and reviews before acceptance.".to_owned();
        self.dirty = true;
        self.focus = true;
    }

    fn event(
        &mut self,
        event: &Event,
        binding: Option<&Binding>,
        notice: &mut &'static str,
    ) -> Option<Value> {
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
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(self.page);
                return None;
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(self.page);
                return None;
            }
            KeyCode::Home if key.modifiers == KeyModifiers::CONTROL => {
                self.scroll = 0;
                return None;
            }
            KeyCode::End if key.modifiers == KeyModifiers::CONTROL => {
                self.scroll = self.rows.len();
                return None;
            }
            KeyCode::Tab | KeyCode::BackTab
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if !self.fields.is_empty() {
                    let backwards =
                        key.code == KeyCode::BackTab || key.modifiers == KeyModifiers::SHIFT;
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
            _ => {}
        }
        if self.detail["kind"] == "permission" {
            if plain
                && matches!(
                    key.code,
                    KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right
                )
                && self.editable(binding, notice)
            {
                let count = self.detail["options"].as_array().map_or(0, Vec::len);
                let previous = matches!(key.code, KeyCode::Up | KeyCode::Left);
                self.option = if previous {
                    self.option.saturating_sub(1)
                } else {
                    (self.option + 1).min(count - 1)
                };
                self.changed();
                self.status = "Local permission highlight; Enter deliberately selects this exact offered option.".to_owned();
            }
            return None;
        }
        if self.fields.is_empty() {
            return None;
        }
        if let KeyCode::Char(c) = key.code
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            && !c.is_control()
        {
            let kind = self.fields[self.field].kind;
            if c != ' '
                || matches!(
                    kind,
                    FieldKind::Text | FieldKind::Number | FieldKind::Integer
                )
            {
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
        if !plain
            || !matches!(
                key.code,
                KeyCode::F(4)
                    | KeyCode::Char(' ')
                    | KeyCode::Left
                    | KeyCode::Right
                    | KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::Home
                    | KeyCode::End
                    | KeyCode::Backspace
                    | KeyCode::Delete
            )
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
            KeyCode::Char(' ') if deliberate && field.kind == FieldKind::Boolean => {
                field.value = Some(Value::Bool(
                    !field
                        .value
                        .as_ref()
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                ))
            }
            KeyCode::Char(' ') if deliberate && field.kind == FieldKind::Multi => {
                let property = &self.detail["schema"]["properties"][&field.name];
                let choices = field_choices(property, field.kind)?;
                let choice = choices.get(field.choice)?;
                if !choice_allowed(property, field.kind, choice) {
                    return None;
                }
                let value = choice_value(choice)?;
                let existing = field
                    .value
                    .as_ref()
                    .and_then(Value::as_array)
                    .and_then(|values| values.iter().position(|v| v.as_str() == Some(value)));
                if existing.is_none()
                    && !string_fits(value, PROMPT_BYTES.saturating_sub(draft_bytes + 2))
                {
                    *notice =
                        "Answer limit is 64 KiB; offered value cannot be added to this draft.";
                    return None;
                }
                let values = field
                    .value
                    .get_or_insert_with(|| Value::Array(Vec::new()))
                    .as_array_mut()?;
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
                    (0..field.choice)
                        .rev()
                        .find(|index| choice_allowed(property, field.kind, &choices[*index]))
                } else {
                    (field.choice + 1..choices.len())
                        .find(|index| choice_allowed(property, field.kind, &choices[*index]))
                }
                .unwrap_or(field.choice);
                let value = choice_value(&choices[next])?;
                if field.kind == FieldKind::Enum {
                    let old_bytes = field.value.as_ref().map_or(2, json_size);
                    if !string_fits(
                        value,
                        PROMPT_BYTES.saturating_sub(draft_bytes.saturating_sub(old_bytes)),
                    ) {
                        *notice =
                            "Answer limit is 64 KiB; offered value cannot be added to this draft.";
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
        if !matches!(
            kind,
            FieldKind::Text | FieldKind::Number | FieldKind::Integer
        ) {
            *notice = "This offered control is not text-editable; paste never chooses an option.";
            return;
        }
        let field = &self.fields[self.field];
        let newly_set = if field.value.is_none() {
            string_size(&field.name) + 2 + string_size(&field.text)
        } else {
            0
        };
        if self
            .draft_size()
            .saturating_add(newly_set)
            .saturating_add(string_size(text).saturating_sub(2))
            > PROMPT_BYTES
        {
            *notice = "Answer draft limit is 64 KiB; edit/paste rejected in full.";
            return;
        }
        let field = &mut self.fields[self.field];
        let normalized = normalize_newlines(text);
        field.text.insert_str(field.cursor, &normalized);
        field.cursor += normalized.len();
        field.cursor = field
            .text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|i| *i >= field.cursor)
            .unwrap_or(field.text.len());
        field.value = Some(Value::Null); // Explicitly set, including empty text.
        self.changed();
    }

    fn draft_size(&self) -> usize {
        self.fields
            .iter()
            .filter(|field| field.value.is_some())
            .fold(2usize, |bytes, field| {
                bytes
                    .saturating_add(string_size(&field.name) + 2)
                    .saturating_add(
                        if matches!(
                            field.kind,
                            FieldKind::Text | FieldKind::Number | FieldKind::Integer
                        ) {
                            string_size(&field.text)
                        } else {
                            field.value.as_ref().map_or(2, json_size)
                        },
                    )
            })
    }

    fn values(&self) -> Result<Value, &'static str> {
        let mut values = serde_json::Map::new();
        for field in &self.fields {
            let Some(value) = &field.value else { continue };
            let value = match field.kind {
                FieldKind::Text => Value::String(field.text.clone()),
                FieldKind::Integer => Value::from(
                    field
                        .text
                        .parse::<i64>()
                        .map_err(|_| "integer requires an i64 value")?,
                ),
                FieldKind::Number => {
                    let number = field
                        .text
                        .parse::<f64>()
                        .map_err(|_| "number requires a finite numeric value")?;
                    Value::Number(
                        serde_json::Number::from_f64(number).ok_or("number must be finite")?,
                    )
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

    fn answer(
        &mut self,
        choice: &str,
        binding: Option<&Binding>,
        notice: &mut &'static str,
    ) -> Option<Value> {
        if self.detail["kind"] != "form" || !self.form_action_offered(choice) {
            *notice = "This exact question does not offer that action; nothing submitted.";
            return None;
        }
        if !self.editable(binding, notice) {
            return None;
        }
        let values = if choice == "accept" {
            match self.values() {
                Ok(values) => {
                    if !self.reviewed || self.review_values.as_ref() != Some(&values) {
                        self.reviewed = true;
                        self.paint_revision = self.paint_revision.wrapping_add(1);
                        self.review_values = Some(values);
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
                    self.review_values = None;
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

    fn permission(
        &mut self,
        binding: Option<&Binding>,
        notice: &mut &'static str,
    ) -> Option<Value> {
        if !self.visible_options.contains(&self.option) {
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
        *notice =
            "Deliberate interaction action captured; no optimistic history or automatic retry.";
    }

    fn restore_unadmitted(&mut self, action: &Value, current: Option<&Binding>) {
        let Some(binding) = &self.binding else { return };
        if self.pending()
            && self.binding.as_ref() == current
            && action["target"] == binding.target
            && action["control"] == binding.control
            && action["owner_generation"].as_u64() == Some(binding.generation)
            && action["interaction_id"].as_str() == Some(self.id.as_str())
            && action["revision"].as_u64() == Some(self.revision)
            && action["call_id"].as_str() == self.captured.as_deref()
            && self.captured.is_some()
        {
            self.captured = None;
            self.paint_revision = self.paint_revision.wrapping_add(1);
            self.reviewed = false;
            self.review_values = None;
            self.status = "Action not admitted: original values/highlight retained. Deliberate new action required.".to_owned();
            self.dirty = true;
        }
    }

    fn draw(&mut self, frame: &mut Frame<'_>, area: Rect, editing: bool) -> Option<Rect> {
        self.review_visible = false;
        if area.width == 0 || area.height == 0 {
            return None;
        }
        let mut body = area;
        let field_height = if self.fields.is_empty() || !self.pending() {
            0
        } else if body.height >= 5 {
            3
        } else {
            2.min(body.height.saturating_sub(1))
        };
        let field_area = take_bottom(&mut body, field_height);
        let review_heading = body.height >= 3;
        if review_heading {
            let status = if self.reviewed {
                "REVIEW · Submit accepts unchanged values; Tab reviews fields · no pattern/format certification".to_owned()
            } else if !self.status.is_empty() {
                self.status.clone()
            } else if self.pending() {
                if self.detail["kind"] == "permission" {
                    "Permission · explicitly choose one of the offered options".to_owned()
                } else {
                    "Question · local answer only until Review, then Submit".to_owned()
                }
            } else {
                format!(
                    "{} · {} · delivery {} · application {}",
                    if self.detail["kind"] == "permission" {
                        "Permission"
                    } else {
                        "Question"
                    },
                    display_value(&self.detail["state"], 128),
                    display_value(&self.detail["delivery"], 128),
                    display_value(&self.detail["application"], 128)
                )
            };
            frame.render_widget(
                Paragraph::new(status).style(Style::default().fg(Color::Yellow)),
                take_top(&mut body, 1),
            );
        }
        let width = usize::from(body.width).max(1);
        self.review_visible = self.reviewed && review_heading && body.height > 0;
        if self.detail["kind"] == "permission"
            && UnicodeWidthStr::width(self.option_label(self.option).as_str()) + 2 > width
        {
            // Huge names/kinds retain their complete inert canonical context
            // in the bounded scrollable body, not an arbitrarily tall button.
            self.page = usize::from(body.height).max(1);
            if self.dirty || self.question_width != width || self.rows.is_empty() {
                self.display.clear();
                let _ = write!(
                    self.display,
                    "Option {} / {} · complete offered context\n{}\n\n{}",
                    self.option + 1,
                    self.detail["options"].as_array().map_or(0, Vec::len),
                    display_value(&self.detail["options"][self.option], INTERACTION_DISPLAY),
                    self.question,
                );
                self.rows = wrap_rows(&self.display, width);
                self.scroll = 0;
                self.anchor_row = 0;
                self.dirty = false;
                self.focus = false;
                self.question_width = width;
            }
            self.scroll = self.scroll.min(self.rows.len().saturating_sub(self.page));
            let lines: Vec<Line<'_>> = self
                .rows
                .iter()
                .skip(self.scroll)
                .take(usize::from(body.height))
                .map(|row| Line::raw(&self.display[row.clone()]))
                .collect();
            frame.render_widget(
                Paragraph::new(lines).style(Style::default().fg(ACCENT)),
                body,
            );
            return None;
        }
        if self.question_width != width || self.question_rows.is_empty() {
            self.question_rows = wrap_rows(&self.question, width);
            self.question_width = width;
        }
        self.page = usize::from(body.height).max(1);
        self.scroll = self
            .scroll
            .min(self.question_rows.len().saturating_sub(self.page));
        let lines: Vec<Line<'_>> = self
            .question_rows
            .iter()
            .skip(self.scroll)
            .take(usize::from(body.height))
            .map(|row| Line::raw(&self.question[row.clone()]))
            .collect();
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().fg(ACCENT)),
            body,
        );
        let field = self.fields.get(self.field)?;
        if field_area.width == 0 || field_area.height == 0 {
            return None;
        }
        let property = &self.detail["schema"]["properties"][&field.name];
        let required = self.detail["schema"]["required"]
            .as_array()
            .is_some_and(|names| {
                names
                    .iter()
                    .any(|name| name.as_str() == Some(field.name.as_str()))
            });
        let title = format!(
            " {} · {} {}/{} · {} ",
            inert(property["title"].as_str().unwrap_or(&field.name), 2048),
            if required { "required" } else { "optional" },
            self.field + 1,
            self.fields.len(),
            if field.value.is_none() {
                "UNSET (F4)"
            } else {
                "local value"
            }
        );
        let mut text = String::new();
        let text_field = matches!(
            field.kind,
            FieldKind::Text | FieldKind::Number | FieldKind::Integer
        );
        let cursor = if text_field {
            push_inert(
                &mut text,
                &field.text[..field.cursor],
                INTERACTION_DISPLAY,
                true,
            );
            let cursor = text.len();
            push_inert(
                &mut text,
                &field.text[field.cursor..],
                INTERACTION_DISPLAY,
                true,
            );
            cursor
        } else {
            text = field.value.as_ref().map_or_else(
                || "UNSET".to_owned(),
                |value| display_value(value, INTERACTION_DISPLAY),
            );
            if let Some(choices) = field_choices(property, field.kind)
                && let Some(choice) = choices.get(field.choice)
            {
                let _ = write!(
                    text,
                    " · highlighted: {} · arrows choose / Space toggles",
                    display_value(choice, INTERACTION_DISPLAY)
                );
            } else if field.kind == FieldKind::Boolean {
                text.push_str(" · Space toggles");
            }
            0
        };
        let inner = if field_area.height >= 3 {
            let block = Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(if editing { ACCENT } else { MUTED }));
            let inner = block.inner(field_area);
            frame.render_widget(block, field_area);
            inner
        } else if field_area.height == 1 {
            field_area
        } else {
            let mut inner = field_area;
            frame.render_widget(
                Paragraph::new(title).style(Style::default().fg(MUTED)),
                take_top(&mut inner, 1),
            );
            inner
        };
        self.review_visible &= inner.width > 0 && inner.height > 0;
        let mut rows = wrap_rows(&text, usize::from(inner.width).max(1));
        if let Some(last) = rows.last()
            && last.start < last.end
            && UnicodeWidthStr::width(&text[last.clone()]) >= usize::from(inner.width).max(1)
        {
            rows.push(text.len()..text.len());
        }
        let cursor_row = rows
            .iter()
            .rposition(|row| row.start <= cursor)
            .unwrap_or(0);
        let start = cursor_row.saturating_sub(usize::from(inner.height).saturating_sub(1));
        frame.render_widget(
            Paragraph::new(
                rows.iter()
                    .skip(start)
                    .take(usize::from(inner.height))
                    .map(|row| Line::raw(&text[row.clone()]))
                    .collect::<Vec<_>>(),
            ),
            inner,
        );
        if editing
            && text_field
            && self.pending()
            && self.captured.is_none()
            && inner.width > 0
            && inner.height > 0
        {
            let row = &rows[cursor_row];
            let column = UnicodeWidthStr::width(&text[row.start..cursor.min(row.end)]);
            frame.set_cursor_position((
                inner.x + column.min(usize::from(inner.width) - 1) as u16,
                inner.y + (cursor_row - start) as u16,
            ));
        }
        Some(field_area)
    }

    fn rebuild(&mut self, width: usize) {
        // Details and the focused oversized-option body reuse this buffer,
        // but never mistake one surface's cached rows for the other's.
        self.question_width = 0;
        self.display.clear();
        let mut anchor = 0;
        // Canonical context is complete and scrollable, never terminal commands.
        for key in [
            "kind",
            "state",
            "selected_work_id",
            "delivery",
            "application",
            "scope",
            "message",
            "tool_call",
            "selection",
        ] {
            if let Some(value) = self.detail.get(key).filter(|value| !value.is_null()) {
                let _ = writeln!(
                    self.display,
                    "{key}: {}",
                    display_value(value, INTERACTION_DISPLAY)
                );
            }
        }
        self.display.push_str("Application is unknown unless independently observed; local capture is not admission.\n");
        if self.detail["kind"] == "form" {
            for key in ["title", "description"] {
                if let Some(value) = self.detail["schema"].get(key) {
                    let _ = writeln!(
                        self.display,
                        "{key}: {}",
                        display_value(value, INTERACTION_DISPLAY)
                    );
                }
            }
            self.display.push_str("Tab/Shift-Tab fields; text/numeric editing/paste; arrows offered choices; Space bool/multiselect; F4 unset.\nF5 validate/review, then F5 accept; F7 cancel. Defaults are editable, never automatic responses.\n");
            self.display.push_str(if self.decline_offered() {
                "F6 decline.\n"
            } else {
                "Decline is not offered by this question.\n"
            });
            self.display.push_str("Local validation is not full JSON Schema or pattern/format certification; the agent validates independently.\n");
            for (index, field) in self.fields.iter().enumerate() {
                let property = &self.detail["schema"]["properties"][&field.name];
                let active = index == self.field;
                if active {
                    anchor = self.display.len();
                }
                let required = self.detail["schema"]["required"]
                    .as_array()
                    .is_some_and(|names| {
                        names
                            .iter()
                            .any(|name| name.as_str() == Some(field.name.as_str()))
                    });
                let _ = writeln!(
                    self.display,
                    "{} Field {} [{}]: {}",
                    if active { ">" } else { " " },
                    inert(&field.name, INTERACTION_DISPLAY),
                    if required { "required" } else { "optional" },
                    if field.value.is_none() {
                        "UNSET (omitted)"
                    } else {
                        "explicit local value"
                    }
                );
                let _ = writeln!(
                    self.display,
                    "Schema/constraints: {}",
                    display_value(property, INTERACTION_DISPLAY)
                );
                if matches!(
                    field.kind,
                    FieldKind::Text | FieldKind::Number | FieldKind::Integer
                ) {
                    self.display.push_str("Draft: ");
                    push_inert(
                        &mut self.display,
                        &field.text[..field.cursor],
                        INTERACTION_DISPLAY,
                        true,
                    );
                    if active {
                        anchor = self.display.len();
                        self.display.push('│');
                    }
                    push_inert(
                        &mut self.display,
                        &field.text[field.cursor..],
                        INTERACTION_DISPLAY,
                        true,
                    );
                    self.display.push('\n');
                } else {
                    let _ = writeln!(
                        self.display,
                        "Draft: {}",
                        field.value.as_ref().map_or_else(
                            || "UNSET".to_owned(),
                            |value| display_value(value, INTERACTION_DISPLAY)
                        )
                    );
                }
                if let Some(choices) = field_choices(property, field.kind) {
                    for (index, choice) in choices.iter().enumerate() {
                        if !choice_allowed(property, field.kind, choice) {
                            continue;
                        }
                        if active && index == field.choice {
                            anchor = self.display.len();
                        }
                        let value = choice_value(choice).unwrap_or("");
                        let selected = match &field.value {
                            Some(Value::Array(values)) => {
                                values.iter().any(|v| v.as_str() == Some(value))
                            }
                            Some(Value::String(selected)) => selected == value,
                            _ => false,
                        };
                        let _ = writeln!(
                            self.display,
                            " {} [{}] {}",
                            if active && index == field.choice {
                                ">"
                            } else {
                                " "
                            },
                            if selected { "x" } else { " " },
                            display_value(choice, INTERACTION_DISPLAY)
                        );
                    }
                }
            }
            if !self.supported {
                let _ = writeln!(
                    self.display,
                    "Unsupported raw schema: {}",
                    display_value(&self.detail["schema"], INTERACTION_DISPLAY)
                );
            }
        } else if let Some(options) = self.detail["options"].as_array() {
            self.display.push_str("Arrows highlight; Enter deliberately chooses the exact offered ID. Escape/focus/detach never deny or cancel.\n");
            for (index, option) in options.iter().enumerate() {
                if index == self.option {
                    anchor = self.display.len();
                }
                let _ = writeln!(
                    self.display,
                    "{} {}",
                    if index == self.option { ">" } else { " " },
                    display_value(option, INTERACTION_DISPLAY)
                );
            }
        }
        if self.display.len() > INTERACTION_DISPLAY {
            let mut end = INTERACTION_DISPLAY;
            while !self.display.is_char_boundary(end) {
                end -= 1;
            }
            self.display.truncate(end);
            self.display.push_str(
                "\n[Local interaction display limit reached; no hidden option is selectable.]\n",
            );
            self.supported = false;
        }
        self.rows = wrap_rows(&self.display, width.max(1));
        self.anchor_row = self
            .rows
            .iter()
            .rposition(|row| row.start <= anchor)
            .unwrap_or(0);
        if self.focus
            && (self.anchor_row < self.scroll || self.anchor_row >= self.scroll + self.page)
        {
            self.scroll = self.anchor_row.saturating_sub(self.page / 2);
        }
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
    property
        .get(if kind == FieldKind::Multi {
            "anyOf"
        } else {
            "oneOf"
        })
        .filter(|value| !value.is_null())
        .or_else(|| property.get("enum"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
}

fn choice_value(choice: &Value) -> Option<&str> {
    choice.as_str().or_else(|| choice["const"].as_str())
}

fn choice_allowed(property: &Value, kind: FieldKind, choice: &Value) -> bool {
    let Some(value) = choice_value(choice) else {
        return false;
    };
    let schema = if kind == FieldKind::Multi {
        &property["items"]
    } else {
        property
    };
    schema
        .get("enum")
        .filter(|v| !v.is_null())
        .is_none_or(|values| {
            values
                .as_array()
                .is_some_and(|values| values.iter().any(|entry| entry.as_str() == Some(value)))
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

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn json_fits(value: &Value, limit: usize) -> bool {
    serde_json::to_writer(JsonBudget { remaining: limit }, value).is_ok()
}

fn json_size(value: &Value) -> usize {
    let mut budget = JsonBudget {
        remaining: PROMPT_BYTES,
    };
    if serde_json::to_writer(&mut budget, value).is_ok() {
        PROMPT_BYTES - budget.remaining
    } else {
        PROMPT_BYTES + 1
    }
}

fn string_size(value: &str) -> usize {
    let mut budget = JsonBudget {
        remaining: PROMPT_BYTES,
    };
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
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        // Canonical JSON can retain megabytes in one printable ASCII line.
        // Its byte and cell boundaries coincide; do not run Unicode grapheme
        // segmentation once per byte just to produce viewport row ranges.
        // Other lines retain the complete grapheme/width behavior below.
        if used == 0 && content.bytes().all(|byte| (b' '..=b'~').contains(&byte)) {
            let end = offset + content.len();
            let mut cursor = offset;
            let columns = width.max(1);
            while end - cursor > columns {
                cursor += columns;
                rows.push(start..cursor);
                start = cursor;
            }
            used = end - cursor;
            if content.len() != line.len() {
                rows.push(start..end);
                start = offset + line.len();
                used = 0;
            }
        } else {
            for (index, grapheme) in line.grapheme_indices(true) {
                let index = offset + index;
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
        }
        offset += line.len();
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

fn event_text(event: &Value, seq: u64, kind: &str) -> (String, Range<usize>) {
    let mut text = format!("#{seq} {kind}\n");
    let start = text.len();
    text.push_str(match kind {
        "user-message" => "You: ",
        "agent-message" => "Agent: ",
        "thought" => "Thought: ",
        "completion" => "Finished: ",
        "error" => "Agent error: ",
        "tool" => "Tool update: ",
        "plan" => "Plan: ",
        _ => "Canonical update: ",
    });
    let mut complete = true;
    if let Some(body) = event["text"].as_str() {
        complete &= push_inert(&mut text, body, EVENT_BYTES, true);
    } else {
        let summary = match kind {
            "completion" => display_value(&event["stop_reason"], 256),
            "error" => format!(
                "code {} · full canonical record in Details",
                display_value(&event["code"], 256)
            ),
            _ => "full canonical record in Details".to_owned(),
        };
        complete &= push_inert(&mut text, &summary, EVENT_BYTES, true);
    }
    if !complete {
        text.push_str("\n[message clipped by local 64 KiB display bound]");
    }
    let body = start..text.len();
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
    (text, body)
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

fn fit_label(label: &str, width: usize) -> Cow<'_, str> {
    if UnicodeWidthStr::width(label) <= width {
        return Cow::Borrowed(label);
    }
    let mut text = String::new();
    let mut used = 0;
    for grapheme in label.graphemes(true) {
        let columns = UnicodeWidthStr::width(grapheme);
        if used + columns >= width {
            break;
        }
        text.push_str(grapheme);
        used += columns;
    }
    if width > 0 {
        text.push('…');
    }
    Cow::Owned(text)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, MouseEvent};
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn narrow_text_rows_preserve_exact_boundaries_blanks_and_graphemes() {
        let text = "ABCD\n\nEFG\n👩‍💻e\u{301}\nHI";
        let visible: Vec<_> = wrap_rows(text, 2)
            .into_iter()
            .map(|row| &text[row])
            .collect();
        assert_eq!(visible, ["AB", "CD", "", "EF", "G", "👩‍💻", "e\u{301}", "HI"]);
        let text = "AB\n";
        let visible: Vec<_> = wrap_rows(text, 2)
            .into_iter()
            .map(|row| &text[row])
            .collect();
        assert_eq!(visible, ["AB", ""]);
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn form(revision: u64) -> Value {
        json!({
            "interaction_id":"question-original", "revision":revision,
            "kind":"form", "state":"pending", "selected_work_id":null,
            "delivery":"not-submitted", "application":"unknown",
            "actions":["accept","cancel"], "message":"What should happen next?",
            "schema":{"type":"object","title":"Fixture question",
                "properties":{"answer":{"type":"string"}},"required":["answer"]}
        })
    }

    fn ready() -> AgentView {
        let mut view = AgentView::new();
        let target = json!({"kind":"acp","id":"private-original-target",
            "instance_id":"instance-original","connection_generation":7,"session_id":"session-original"});
        view.target = Some(target.clone());
        view.binding = Some(Binding {
            target,
            control: json!({"kind":"view","ref":"grant-original"}),
            generation: 11,
        });
        view.lifecycle = "ready";
        view
    }

    fn paint_pending(view: &mut AgentView, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                view.draw(frame, area);
            })
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .chunks(usize::from(width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn paint(view: &mut AgentView, width: u16, height: u16) -> String {
        let screen = paint_pending(view, width, height);
        if view.after_draw() {
            paint_pending(view, width, height)
        } else {
            screen
        }
    }

    fn click(view: &mut AgentView, action: Action) -> Option<Value> {
        let rect = view
            .hits
            .iter()
            .find(|hit| hit.action == action)
            .unwrap()
            .rect;
        click_rect(view, rect)
    }

    fn click_rect(view: &mut AgentView, rect: Rect) -> Option<Value> {
        view.event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        }))
    }

    fn reject_queued_mutations_until_empty_repaint(
        view: &mut AgentView,
        action: Action,
        accelerator: &Event,
    ) {
        // Model the parent's actual ordering: sync, repaint with input still
        // pending, then consume keys/mouse. No repaint proves fresh consent.
        for _ in 0..2 {
            paint_pending(view, 80, 11);
            assert!(view.event(accelerator).is_none());
            assert!(click(view, action.clone()).is_none());
            assert!(view.event(&key(KeyCode::Enter)).is_none());
            assert!(view.event(&key(KeyCode::Char(' '))).is_none());
            assert!(view.needs_input_fence());
        }
        assert!(view.after_draw());
        // Empty is not a retroactive upgrade of already-painted disabled hits.
        assert!(view.event(accelerator).is_none());
        assert!(click(view, action).is_none());
        assert!(view.event(&key(KeyCode::Enter)).is_none());
        assert!(view.event(&key(KeyCode::Char(' '))).is_none());
        paint_pending(view, 80, 11);
    }

    #[test]
    fn details_does_not_turn_old_enter_auth_or_interrupt_into_an_application_action() {
        let mut view = ready();
        view.foreground = Some("original-pending-turn".to_owned());
        view.foreground_state = "submitted".to_owned();
        view.event(&key(KeyCode::F(10)));
        paint(&mut view, 80, 11);
        for event in [
            Event::Paste("must stay local".to_owned()),
            key(KeyCode::Enter),
            key(KeyCode::F(8)),
            Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        ] {
            assert!(view.event(&event).is_none());
            assert!(view.settings_open);
        }
        assert!(view.draft.is_empty());
        assert_eq!(view.foreground.as_deref(), Some("original-pending-turn"));
        paint(&mut view, 80, 11);
        click(&mut view, Action::Back);
        assert!(!view.settings_open);
    }

    #[test]
    fn completed_question_returns_to_prompt_without_losing_canonical_history() {
        let mut view = ready();
        view.sync_interactions(Some(&json!([form(1)])));
        assert_eq!(view.interaction, Some(0));
        view.event(&Event::Paste("15".to_owned()));
        paint(&mut view, 80, 11);
        assert!(click(&mut view, Action::Review(false)).is_none());
        paint(&mut view, 80, 11);
        assert!(click(&mut view, Action::Review(true)).is_some());
        let mut completed = form(1);
        completed["state"] = json!("selected");
        completed["selected_work_id"] = json!("answer-work");
        completed["delivery"] = json!("submitted");
        completed["selection"] = json!({"action":"accept","content":{"answer":"15"}});
        view.sync_interactions(Some(&json!([completed])));
        assert!(view.interaction.is_none());
        assert!(view.action_focus.is_none());
        assert_eq!(
            view.interactions[0].detail["selection"]["content"]["answer"],
            "15"
        );
        paint(&mut view, 80, 11);
        click(&mut view, Action::Questions);
        assert_eq!(view.interaction, Some(0));
        assert!(!view.actions().iter().any(|action| matches!(
            action,
            Action::Field(_) | Action::Review(_) | Action::Decline | Action::Cancel
        )));
        paint(&mut view, 80, 11);
        click(&mut view, Action::Prompt);
        assert!(view.interaction.is_none());
        view.event(&Event::Paste("next deliberate prompt".to_owned()));
        paint(&mut view, 80, 11);
        let request = click(&mut view, Action::Send).unwrap();
        assert_eq!(request["text"], "next deliberate prompt");
        assert_eq!(request["target"], view.binding.as_ref().unwrap().target);
    }

    #[test]
    fn literal_field_input_and_painted_two_step_review_keep_original_authority() {
        let mut view = ready();
        view.sync_interactions(Some(&json!([form(1)])));
        for c in ['j', 'k', '?'] {
            assert!(view.event(&key(KeyCode::Char(c))).is_none());
        }
        assert_eq!(view.interactions[0].fields[0].text, "jk?");
        view.event(&key(KeyCode::Tab));
        paint(&mut view, 80, 11);
        assert!(view.event(&key(KeyCode::Enter)).is_none());
        assert!(view.interactions[0].reviewed);
        // A queued second activation cannot reuse the pre-review paint.
        assert!(view.event(&key(KeyCode::Enter)).is_none());
        paint(&mut view, 80, 11);
        let action = view.event(&key(KeyCode::Enter)).unwrap();
        assert_eq!(action["values"], json!({"answer":"jk?"}));
        assert_eq!(action["target"], view.binding.as_ref().unwrap().target);
        assert_eq!(action["control"], view.binding.as_ref().unwrap().control);
        assert_eq!(action["owner_generation"], 11);
        assert_eq!(action["interaction_id"], "question-original");
        assert_eq!(action["revision"], 1);
    }

    #[test]
    fn stale_mouse_rectangle_cannot_review_a_replacement_revision() {
        let mut view = ready();
        view.sync_interactions(Some(&json!([form(1)])));
        paint(&mut view, 80, 11);
        let rect = view
            .hits
            .iter()
            .find(|hit| hit.action == Action::Review(false))
            .unwrap()
            .rect;
        view.sync_interactions(Some(&json!([form(2)])));
        assert!(click_rect(&mut view, rect).is_none());
        assert!(!view.interactions[0].reviewed);
        assert!(view.interactions[0].binding.is_none());
    }

    #[test]
    fn permission_mouse_uses_the_exact_offered_id_and_never_infers_a_deny() {
        let mut view = ready();
        view.sync_interactions(Some(&json!([{
            "interaction_id":"permission-original","revision":4,"kind":"permission",
            "state":"pending","selected_work_id":null,"message":"May this tool proceed?",
            "options":[
                {"option_id":"vendor/first-73","name":"Proceed once","kind":"allow_once"},
                {"option_id":"vendor/not-this-42","name":"Not this time","kind":"reject_once"}
            ]
        }])));
        let screen = paint(&mut view, 80, 11);
        assert!(screen.contains("May this tool proceed?"));
        let action = click(&mut view, Action::Permission(1)).unwrap();
        assert_eq!(action["option_id"], "vendor/not-this-42");
        assert_eq!(action["interaction_id"], "permission-original");
        assert_eq!(action["revision"], 4);
        assert_eq!(action["owner_generation"], 11);
    }

    #[test]
    fn new_question_preserves_an_active_prompt_and_help_backs_out_locally() {
        let mut view = ready();
        view.event(&Event::Paste("jk? draft".to_owned()));
        view.sync_interactions(Some(&json!([form(1)])));
        assert!(view.interaction.is_none());
        let screen = paint(&mut view, 80, 11);
        assert!(screen.contains("Pending question: What should happen next?"));
        assert!(screen.contains("jk? draft"));
        view.event(&key(KeyCode::F(1)));
        assert!(view.help.is_some());
        assert!(
            view.event(&Event::Paste("not replayed".to_owned()))
                .is_none()
        );
        view.event(&key(KeyCode::Esc));
        assert!(view.help.is_none());
        assert_eq!(view.draft, "jk? draft");
        assert!(view.interaction.is_none());
    }

    #[test]
    fn queued_acceptance_is_discarded_across_repaints_until_visible_input_empty_review() {
        for acceptance in [KeyCode::F(5), KeyCode::Enter] {
            let mut view = ready();
            view.sync_interactions(Some(&json!([form(1)])));
            paint(&mut view, 80, 11);
            // First local Review remains natural after ordinary field editing.
            view.event(&Event::Paste("review this exact answer".to_owned()));
            assert!(view.event(&key(KeyCode::F(5))).is_none());
            assert!(view.interactions[0].reviewed);

            // The parent may repaint between queued Press events. Neither
            // repaint proves those keys were entered after review appeared.
            for _ in 0..2 {
                paint_pending(&mut view, 80, 11);
                assert!(view.event(&key(acceptance)).is_none());
                assert!(view.interactions[0].captured.is_none());
                assert!(!view.input_ready);
                assert!(view.needs_input_fence());
            }
            assert!(view.after_draw());
            assert!(!view.needs_input_fence());
            // Empty cannot retroactively arm an already-painted disabled hit.
            assert!(view.event(&key(acceptance)).is_none());
            paint_pending(&mut view, 80, 11);
            let action = view.event(&key(acceptance)).unwrap();
            assert_eq!(
                action["values"],
                json!({"answer":"review this exact answer"})
            );
            assert_eq!(action["interaction_id"], "question-original");
            assert_eq!(action["revision"], 1);
            assert_eq!(action["owner_generation"], 11);
        }
    }

    #[test]
    fn response_shortcuts_never_rebind_to_replacement_interactions_or_grants() {
        for shortcut in [KeyCode::F(5), KeyCode::F(6), KeyCode::F(7)] {
            for replace_grant in [false, true] {
                let mut view = ready();
                let mut original = form(1);
                original["actions"] = json!(["accept", "decline", "cancel"]);
                view.sync_interactions(Some(&json!([original.clone()])));
                view.event(&Event::Paste("old local answer".to_owned()));
                paint(&mut view, 80, 11);
                let mut replacement = original;
                let generation = if replace_grant { 12 } else { 11 };
                if replace_grant {
                    let projection = json!({
                        "target":view.target, "seq":1, "lifecycle":"ready",
                        "configuration":{}, "foreground":null,
                        "content":{"through":0,"events":[],"interactions":[replacement.clone()]}
                    });
                    view.sync(
                        &projection,
                        Some(&json!({
                            "control":{"kind":"view","ref":"replacement-grant"},
                            "owner_generation":generation
                        })),
                    );
                } else {
                    replacement["interaction_id"] = json!("replacement-question");
                    replacement["revision"] = json!(2);
                    view.sync_interactions(Some(&json!([replacement.clone()])));
                }
                assert!(view.event(&key(shortcut)).is_none());
                assert!(!view.interactions[0].reviewed);
                assert!(view.interactions[0].captured.is_none());
                assert!(view.interactions[0].binding.is_none());
                assert!(view.interactions[0].fields[0].text.is_empty());

                // A genuinely fresh advertised shortcut still works; rejection
                // neither queues the old key nor removes the new control.
                if shortcut == KeyCode::F(5) {
                    view.event(&Event::Paste("fresh replacement answer".to_owned()));
                }
                if shortcut == KeyCode::F(5) {
                    paint(&mut view, 80, 11);
                } else {
                    reject_queued_mutations_until_empty_repaint(
                        &mut view,
                        if shortcut == KeyCode::F(6) {
                            Action::Decline
                        } else {
                            Action::Cancel
                        },
                        &key(shortcut),
                    );
                    assert!(view.interactions[0].captured.is_none());
                }
                let action = if shortcut == KeyCode::F(5) {
                    assert!(view.event(&key(shortcut)).is_none());
                    paint(&mut view, 80, 11);
                    view.event(&key(shortcut)).unwrap()
                } else {
                    view.event(&key(shortcut)).unwrap()
                };
                assert_eq!(action["interaction_id"], replacement["interaction_id"]);
                assert_eq!(action["revision"], replacement["revision"]);
                assert_eq!(action["owner_generation"], generation);
                assert_eq!(action["control"], view.binding.as_ref().unwrap().control);
                if shortcut == KeyCode::F(5) {
                    assert_eq!(
                        action["values"],
                        json!({"answer":"fresh replacement answer"})
                    );
                }
            }
        }
    }

    #[test]
    fn interrupt_and_authentication_shortcuts_require_their_original_painted_facts() {
        let mut view = ready();
        view.foreground = Some("foreground-before-replacement".to_owned());
        paint(&mut view, 80, 11);
        let projection = json!({
            "target":view.target, "seq":1, "lifecycle":"ready", "configuration":{},
            "foreground":{"prompt_id":"foreground-after-replacement","state":"submitted"},
            "content":{"through":0,"events":[],"interactions":[]}
        });
        let grant = json!({
            "control":view.binding.as_ref().unwrap().control,
            "owner_generation":11
        });
        view.sync(&projection, Some(&grant));
        let interrupt = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(view.event(&interrupt).is_none());
        reject_queued_mutations_until_empty_repaint(
            &mut view,
            Action::Interrupt("foreground-after-replacement".to_owned()),
            &interrupt,
        );
        let action = view.event(&interrupt).unwrap();
        assert_eq!(action["prompt_id"], "foreground-after-replacement");
        assert_eq!(action["owner_generation"], 11);
        assert!(view.event(&interrupt).is_none());

        let mut view = ready();
        view.lifecycle = "auth-required";
        view.sync_auth(&json!([{"method_id":"old-method","kind":"agent"}]));
        paint(&mut view, 80, 11);
        view.sync_auth(&json!([{"method_id":"new-method","kind":"agent"}]));
        assert!(view.event(&key(KeyCode::F(8))).is_none());
        reject_queued_mutations_until_empty_repaint(
            &mut view,
            Action::Authenticate(0, "new-method".to_owned()),
            &key(KeyCode::F(8)),
        );
        let action = view.event(&key(KeyCode::F(8))).unwrap();
        assert_eq!(action["method_id"], "new-method");
        assert_eq!(action["owner_generation"], 11);
        assert!(view.event(&key(KeyCode::F(8))).is_none());
    }

    #[test]
    fn covered_help_resize_and_external_overlays_preserve_values_but_disarm_review() {
        let mut view = ready();
        view.sync_interactions(Some(&json!([form(1)])));
        view.event(&Event::Paste("retained reviewed draft".to_owned()));
        paint(&mut view, 80, 11);
        view.event(&key(KeyCode::F(5)));
        paint(&mut view, 80, 11);
        assert!(view.input_ready);
        view.event(&key(KeyCode::F(1)));
        paint_pending(&mut view, 80, 11);
        assert!(!view.after_draw());
        assert!(!view.needs_input_fence());
        assert!(view.event(&key(KeyCode::F(5))).is_none());
        view.event(&key(KeyCode::Esc));
        assert!(view.event(&key(KeyCode::F(5))).is_none());
        assert_eq!(
            view.interactions[0].review_values,
            Some(json!({"answer":"retained reviewed draft"}))
        );

        // An incomplete tiny review must not arm, even with an empty queue.
        view.event(&Event::Resize(35, 4));
        paint_pending(&mut view, 35, 4);
        assert!(!view.after_draw());
        assert!(!view.needs_input_fence());
        assert!(view.event(&key(KeyCode::F(5))).is_none());
        view.event(&Event::Resize(80, 11));
        paint(&mut view, 80, 11);
        view.invalidate_presentation();
        assert!(!view.after_draw());
        assert!(!view.needs_input_fence());
        assert!(view.event(&key(KeyCode::Enter)).is_none());
        assert_eq!(
            view.interactions[0].fields[0].text,
            "retained reviewed draft"
        );
        paint(&mut view, 80, 11);
        let action = view.event(&key(KeyCode::F(5))).unwrap();
        assert_eq!(
            action["values"],
            json!({"answer":"retained reviewed draft"})
        );
    }

    #[test]
    fn short_terminal_renders_and_clicks_every_offered_permission_without_clipping_rejection() {
        let names = [
            "Allow this invocation",
            "Remember permission",
            "Reject this invocation",
            "Remember rejection",
        ];
        let kinds = ["allow_once", "allow_always", "reject_once", "reject_always"];
        let options: Vec<Value> = names
            .iter()
            .zip(kinds)
            .enumerate()
            .map(|(index, (name, kind))| {
                json!({
                    "option_id":format!("vendor/exact-{index}-γ"),"name":name,"kind":kind
                })
            })
            .collect();
        for selected in 0..options.len() {
            let mut view = ready();
            view.sync_interactions(Some(&json!([{
                "interaction_id":"short-permission","revision":3,"kind":"permission",
                "state":"pending","selected_work_id":null,"message":"May this tool proceed?",
                "options":options
            }])));
            let screen = paint(&mut view, 80, 14);
            assert!(screen.contains("May this tool proceed?"));
            for name in names {
                assert!(
                    screen.contains(name),
                    "offered choice must remain readable: {name}"
                );
            }
            let action = click(&mut view, Action::Permission(selected)).unwrap();
            assert_eq!(action["option_id"], options[selected]["option_id"]);
            assert_eq!(action["interaction_id"], "short-permission");
            assert_eq!(action["revision"], 3);
            assert_eq!(action["owner_generation"], 11);
        }
    }

    #[test]
    fn bounded_permission_window_follows_focus_and_never_submits_an_unpainted_choice() {
        let options: Vec<Value> = (0..24)
            .map(|index| {
                json!({
                    "option_id":format!("offered/{index}"),
                    "name":format!("Offered choice {index}"),"kind":"reject_once"
                })
            })
            .collect();
        for selected in 0..options.len() {
            let mut view = ready();
            view.sync_interactions(Some(&json!([{
                "interaction_id":"many-permissions","revision":1,"kind":"permission",
                "state":"pending","selected_work_id":null,"message":"Keep the question visible",
                "options":options
            }])));
            paint(&mut view, 35, 14);
            for _ in 0..selected {
                view.event(&key(KeyCode::Down));
            }
            if selected > 0 {
                assert!(view.event(&key(KeyCode::Enter)).is_none());
            }
            let screen = paint(&mut view, 35, 14);
            let item = &view.interactions[0];
            assert!(screen.contains("Keep the question visible"));
            assert!(item.visible_options.len() < options.len());
            assert!(item.visible_options.contains(&selected));
            let action = view.event(&key(KeyCode::Enter)).unwrap();
            assert_eq!(action["option_id"], options[selected]["option_id"]);
        }
    }

    #[test]
    fn oversized_permission_has_full_scrollable_context_and_keeps_its_exact_offered_id() {
        let mut view = ready();
        let name = format!(
            "{} full-context-tail",
            "long offered description ".repeat(20)
        );
        view.sync_interactions(Some(&json!([{
            "interaction_id":"long-permission","revision":1,"kind":"permission",
            "state":"pending","selected_work_id":null,"message":"Review the complete option",
            "options":[{"option_id":"vendor/not-inferred/δ","name":name,"kind":"reject_once"}]
        }])));
        paint(&mut view, 35, 14);
        view.event(&Event::Key(KeyEvent::new(
            KeyCode::End,
            KeyModifiers::CONTROL,
        )));
        let screen = paint(&mut view, 35, 14);
        let unwrapped: String = screen.lines().map(str::trim).collect();
        assert!(unwrapped.contains("full-context-tail"));
        let action = click(&mut view, Action::Permission(0)).unwrap();
        assert_eq!(action["option_id"], "vendor/not-inferred/δ");
        assert_eq!(action["interaction_id"], "long-permission");
    }

    #[test]
    fn ordinary_prompt_paste_enter_keeps_the_visible_original_authority_without_an_edit_barrier() {
        let mut view = ready();
        paint(&mut view, 80, 11);
        view.event(&Event::Paste("ordinary immediate prompt".to_owned()));
        let action = view.event(&key(KeyCode::Enter)).unwrap();
        assert_eq!(action["text"], "ordinary immediate prompt");
        assert_eq!(action["target"], view.binding.as_ref().unwrap().target);
        assert_eq!(action["control"], view.binding.as_ref().unwrap().control);
        assert_eq!(action["owner_generation"], 11);
        assert!(view.draft.is_empty());

        paint(&mut view, 80, 11);
        view.event(&Event::Paste("retained, never queued".to_owned()));
        view.foreground = Some("newly-busy-foreground".to_owned());
        assert!(view.event(&key(KeyCode::Enter)).is_none());
        assert_eq!(view.draft, "retained, never queued");
    }

    #[test]
    fn editing_a_reviewed_field_never_reuses_its_old_submit_intent() {
        let mut view = ready();
        view.sync_interactions(Some(&json!([form(1)])));
        paint(&mut view, 80, 11);
        view.event(&Event::Paste("before".to_owned()));
        view.event(&key(KeyCode::F(5)));
        paint(&mut view, 80, 11);
        click(&mut view, Action::Field(0));
        view.event(&Event::Paste(" after".to_owned()));
        assert!(!view.interactions[0].reviewed);
        assert!(view.event(&key(KeyCode::F(5))).is_none());
        assert!(!view.interactions[0].reviewed);
        assert!(view.interactions[0].captured.is_none());
        paint(&mut view, 80, 11);
        assert!(view.event(&key(KeyCode::F(5))).is_none());
        paint_pending(&mut view, 80, 11);
        assert!(view.event(&key(KeyCode::F(5))).is_none());
        paint(&mut view, 80, 11);
        let action = view.event(&key(KeyCode::F(5))).unwrap();
        assert_eq!(action["values"], json!({"answer":"before after"}));
    }

    #[test]
    fn permission_changes_reject_queued_gestures_across_repaints_until_empty_then_arm_exact_choice()
    {
        for change in ["arrival", "revision", "interaction", "option"] {
            let mut view = ready();
            let mut permission = json!({
                "interaction_id":"permission-original","revision":1,"kind":"permission",
                "state":"pending","selected_work_id":null,"message":"Exact offered choice",
                "options":[
                    {"option_id":"vendor/allow","name":"Allow once","kind":"allow_once"},
                    {"option_id":"vendor/reject","name":"Reject once","kind":"reject_once"}
                ]
            });
            if change != "arrival" {
                view.sync_interactions(Some(&json!([permission.clone()])));
            }
            paint(&mut view, 80, 11);
            match change {
                "revision" => permission["revision"] = json!(2),
                "interaction" => permission["interaction_id"] = json!("permission-replacement"),
                _ => {}
            }
            view.sync_interactions(Some(&json!([permission.clone()])));
            let selected = if change == "option" {
                view.event(&key(KeyCode::Down));
                1
            } else {
                0
            };
            assert!(view.event(&key(KeyCode::Enter)).is_none());
            reject_queued_mutations_until_empty_repaint(
                &mut view,
                Action::Permission(selected),
                &key(KeyCode::Enter),
            );
            assert!(view.interactions[0].captured.is_none());
            // An unchanged armed repaint remains natural, without a new fence.
            paint_pending(&mut view, 80, 11);
            assert!(!view.needs_input_fence());
            let action = view.event(&key(KeyCode::Enter)).unwrap();
            assert_eq!(
                action["option_id"],
                permission["options"][selected]["option_id"]
            );
            assert_eq!(action["interaction_id"], permission["interaction_id"]);
            assert_eq!(action["revision"], permission["revision"]);
            assert_eq!(action["target"], view.binding.as_ref().unwrap().target);
            assert_eq!(action["control"], view.binding.as_ref().unwrap().control);
            assert_eq!(action["owner_generation"], 11);
        }
    }

    #[test]
    fn authentication_selection_and_new_foreground_need_empty_then_armed_repaint() {
        let mut view = ready();
        view.lifecycle = "auth-required";
        view.sync_auth(&json!([
            {"method_id":"first-method","kind":"agent"},
            {"method_id":"second-method","kind":"agent"}
        ]));
        paint(&mut view, 80, 11);
        view.event(&key(KeyCode::Down));
        reject_queued_mutations_until_empty_repaint(
            &mut view,
            Action::Authenticate(1, "second-method".to_owned()),
            &key(KeyCode::F(8)),
        );
        let action = view.event(&key(KeyCode::F(8))).unwrap();
        assert_eq!(action["method_id"], "second-method");
        assert_eq!(action["owner_generation"], 11);

        for previous in [None, Some("submitted")] {
            let mut view = ready();
            if let Some(state) = previous {
                view.foreground = Some("exact-current-foreground".to_owned());
                view.foreground_state = display_value(&json!(state), 128);
            }
            paint(&mut view, 80, 11);
            let grant = json!({
                "control":view.binding.as_ref().unwrap().control,
                "owner_generation":11
            });
            view.sync(
                &json!({
                    "target":view.target,"seq":1,"lifecycle":"ready","configuration":{},
                    "foreground":{"prompt_id":"exact-current-foreground","state":"cancelling"},
                    "content":{"through":0,"events":[],"interactions":[]}
                }),
                Some(&grant),
            );
            let interrupt = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
            reject_queued_mutations_until_empty_repaint(
                &mut view,
                Action::Interrupt("exact-current-foreground".to_owned()),
                &interrupt,
            );
            let action = view.event(&interrupt).unwrap();
            assert_eq!(action["prompt_id"], "exact-current-foreground");
            assert_eq!(action["owner_generation"], 11);
        }
    }

    #[test]
    fn covered_hidden_or_clipped_mutations_need_new_empty_and_armed_repaint() {
        for kind in [
            "permission",
            "authenticate",
            "interrupt",
            "decline",
            "cancel",
        ] {
            for invalidation in ["help", "hidden", "clipped"] {
                let mut view = ready();
                let (action, accelerator, operation) = match kind {
                    "permission" => {
                        view.sync_interactions(Some(&json!([{
                            "interaction_id":"exact-permission","revision":3,"kind":"permission",
                            "state":"pending","selected_work_id":null,"message":"May this proceed?",
                            "options":[
                                {"option_id":"exact/allow","name":"Allow","kind":"allow_once"},
                                {"option_id":"exact/reject","name":"Reject","kind":"reject_once"}
                            ]
                        }])));
                        (Action::Permission(1), key(KeyCode::Enter), "acp-permission")
                    }
                    "authenticate" => {
                        view.lifecycle = "auth-required";
                        view.sync_auth(&json!([
                            {"method_id":"exact-method","kind":"agent"}
                        ]));
                        (
                            Action::Authenticate(0, "exact-method".to_owned()),
                            key(KeyCode::F(8)),
                            "acp-authenticate",
                        )
                    }
                    "interrupt" => {
                        view.foreground = Some("exact-foreground".to_owned());
                        (
                            Action::Interrupt("exact-foreground".to_owned()),
                            Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
                            "acp-interrupt",
                        )
                    }
                    _ => {
                        let mut question = form(2);
                        question["actions"] = json!(["accept", "decline", "cancel"]);
                        view.sync_interactions(Some(&json!([question])));
                        if kind == "decline" {
                            (Action::Decline, key(KeyCode::F(6)), "acp-answer")
                        } else {
                            (Action::Cancel, key(KeyCode::F(7)), "acp-answer")
                        }
                    }
                };
                paint(&mut view, 80, 11);
                paint_pending(&mut view, 80, 11);
                assert!(!view.needs_input_fence());
                match invalidation {
                    "help" => {
                        view.event(&key(KeyCode::F(1)));
                        paint_pending(&mut view, 80, 11);
                        assert!(!view.after_draw());
                        assert!(view.event(&accelerator).is_none());
                        view.event(&key(KeyCode::Esc));
                    }
                    "hidden" => view.invalidate_presentation(),
                    _ => {
                        view.event(&Event::Resize(80, 2));
                        paint_pending(&mut view, 80, 2);
                        assert!(!view.after_draw());
                        assert!(view.event(&accelerator).is_none());
                        view.event(&Event::Resize(80, 11));
                    }
                }
                reject_queued_mutations_until_empty_repaint(
                    &mut view,
                    action.clone(),
                    &accelerator,
                );
                let request = click(&mut view, action).unwrap();
                assert_eq!(request["op"], operation);
                assert_eq!(request["target"], view.binding.as_ref().unwrap().target);
                assert_eq!(request["control"], view.binding.as_ref().unwrap().control);
                assert_eq!(request["owner_generation"], 11);
                match kind {
                    "permission" => assert_eq!(request["option_id"], "exact/reject"),
                    "authenticate" => assert_eq!(request["method_id"], "exact-method"),
                    "interrupt" => assert_eq!(request["prompt_id"], "exact-foreground"),
                    _ => {
                        assert_eq!(request["interaction_id"], "question-original");
                        assert_eq!(request["revision"], 2);
                        assert_eq!(request["action"], kind);
                    }
                }
            }
        }
    }

    #[test]
    fn painted_authentication_mouse_choice_uses_its_exact_method_not_the_old_highlight() {
        let mut view = ready();
        view.lifecycle = "auth-required";
        view.sync_auth(&json!([
            {"method_id":"agent-method/first","kind":"agent"},
            {"method_id":"agent-method/second","kind":"agent"},
            {"method_id":"client-method/external","kind":"unsupported"}
        ]));
        let surface = paint(&mut view, 80, 14);
        for id in [
            "agent-method/first",
            "agent-method/second",
            "client-method/external",
        ] {
            assert!(surface.contains(id));
        }
        let action = click(
            &mut view,
            Action::Authenticate(1, "agent-method/second".to_owned()),
        )
        .unwrap();
        assert_eq!(action["method_id"], "agent-method/second");
        assert_eq!(action["owner_generation"], 11);
    }

    #[test]
    fn oversized_authentication_reveals_exact_context_before_a_fresh_choice() {
        let mut view = ready();
        view.lifecycle = "auth-required";
        let first = format!("first/{}", "a".repeat(160));
        let second = format!("second/{}", "b".repeat(160));
        view.sync_auth(&json!([
            {"method_id":first,"kind":"agent"},
            {"method_id":second,"kind":"agent"}
        ]));
        paint(&mut view, 35, 24);
        assert!(click(&mut view, Action::Authenticate(1, second.clone())).is_none());
        assert_eq!(view.auth_selected, 1);
        assert!(view.event(&key(KeyCode::F(8))).is_none());
        view.event(&key(KeyCode::F(1)));
        let context = paint(&mut view, 80, 40);
        assert!(context.contains("second/"));
        assert!(context.contains(&"b".repeat(40)));
        view.event(&key(KeyCode::Esc));
        paint(&mut view, 35, 24);
        let action = click(&mut view, Action::Authenticate(1, second.clone())).unwrap();
        assert_eq!(action["method_id"], second);
        assert_eq!(action["owner_generation"], 11);
    }

    #[test]
    fn oversized_unselected_permission_first_reveals_context_then_needs_a_new_painted_choice() {
        let mut view = ready();
        let name = "Full offered context ".repeat(20);
        view.sync_interactions(Some(&json!([{
            "interaction_id":"contextual-options","revision":1,"kind":"permission",
            "state":"pending","selected_work_id":null,"message":"Two exact options",
            "options":[
                {"option_id":"original/first","name":name,"kind":"allow_once"},
                {"option_id":"original/second","name":name,"kind":"reject_once"}
            ]
        }])));
        paint(&mut view, 35, 14);
        assert!(click(&mut view, Action::Permission(1)).is_none());
        assert!(view.interactions[0].captured.is_none());
        assert_eq!(view.interactions[0].option, 1);
        assert!(view.event(&key(KeyCode::Enter)).is_none());
        paint(&mut view, 35, 14);
        let action = click(&mut view, Action::Permission(1)).unwrap();
        assert_eq!(action["option_id"], "original/second");
    }
}

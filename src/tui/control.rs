//! Native ownership UI. The original attachment connection lives on one worker:
//! human input, views, consent and keep are FIFO on that connection. Screen and
//! topology reads use the wrapper's separate observation connection.
use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{
        Arc,
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    time::{Duration, Instant},
};
use unicode_width::UnicodeWidthStr;

const RPC_TIMEOUT: Duration = Duration::from_secs(3);
const REFRESH_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Refresh,
    View,
    Claim,
    Force,
    Accept,
    Cancel,
    Yield,
    Release,
    Consent,
    Keep,
    Activity,
    Input,
}

struct Grant {
    terminal: Arc<str>,
    handle: Value,
    generation: u64,
}

pub(super) enum Input {
    Bytes(Vec<u8>),
    Paste(String),
}

enum Work {
    Rpc {
        target: Arc<str>,
        op: Op,
        request: Value,
    },
    Refresh {
        controller: Value,
        targets: Vec<String>,
    },
    Claim {
        target: Arc<str>,
        controller: Value,
    },
    Input {
        grant: Arc<Grant>,
        input: Input,
    },
}

struct Reply {
    target: Arc<str>,
    op: Op,
    result: Result<Value>,
}

#[derive(Clone)]
enum ForcePresentation {
    Hidden,
    Drawn,
    Confirmable,
}

#[derive(Clone)]
enum PanelKind {
    Locked,
    Progress,
    Owner {
        claim: Option<String>,
        observed: Value,
    },
    Cancel {
        claim: Value,
    },
    Force {
        observed: Value,
        owner: String,
        reservation: String,
        presentation: ForcePresentation,
    },
}
struct Panel {
    target: String,
    kind: PanelKind,
}

struct Warning {
    claim: String,
    activity: u64,
    serial: u64,
    drawn: bool,
    pending: bool,
    keeping: bool,
}

pub(super) struct NativeControl {
    controller: Value,
    id: String,
    tx: SyncSender<Work>,
    rx: Receiver<Reply>,
    controls: HashMap<String, Arc<Grant>>,
    ownership: HashMap<String, Value>,
    claims: HashMap<String, Value>,
    uncertain: HashSet<String>,
    attempted: HashSet<String>,
    busy: HashSet<String>,
    offers: HashSet<String>,
    warnings: HashMap<String, Warning>,
    shown_warning: Option<String>,
    panel: Option<Panel>,
    call_offset: usize,
    mouse_tail: HashSet<MouseButton>,
    view: Option<(String, Option<bool>)>,
    serial: u64,
    acknowledged_serial: u64,
    refresh_pending: bool,
    refreshed: Instant,
    connected: bool,
}

impl NativeControl {
    pub(super) fn register(path: &Path) -> Result<Self> {
        let mut connection = crate::client::Connection::connect(path)?;
        let created = rpc(
            &mut connection,
            json!({"op":"controller-create", "role":"attachment", "label":"native TUI"}),
        )?;
        let (tx, work) = mpsc::sync_channel(128);
        let (reply, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("amux-native-control".into())
            .spawn(move || {
                while let Ok(work) = work.recv() {
                    let (target, op, result) = match work {
                        Work::Rpc {
                            target,
                            op,
                            request,
                        } => {
                            let serial = request["view"]["serial"].as_u64();
                            let mut result = rpc(&mut connection, request);
                            if let (Op::View, Some(serial), Ok(value)) = (op, serial, &mut result) {
                                value["native_view_serial"] = json!(serial);
                            }
                            (target, op, result)
                        }
                        Work::Refresh {
                            controller,
                            targets,
                        } => (
                            Arc::from(""),
                            Op::Refresh,
                            refresh(&mut connection, controller, targets),
                        ),
                        Work::Claim { target, controller } => {
                            // This is one deliberate request, not a retry of a
                            // mutation. Polling may still describe a withdrawn
                            // reservation, so observe on the original FIFO
                            // connection immediately before creating the claim.
                            let result = rpc(
                                &mut connection,
                                json!({"op":"state","terminal":target.as_ref()}),
                            )
                            .and_then(|state| {
                                rpc(
                                    &mut connection,
                                    json!({"op":"ownership-claim","controller":controller,
                                        "terminal":target.as_ref(),"observed":state["ownership"]["observed"],
                                        "mode":"cooperative","when":{"kind":"now"}}),
                                )
                            });
                            (target, Op::Claim, result)
                        }
                        Work::Input { grant, input } => {
                            let result = match input {
                                Input::Bytes(bytes) => connection.request(
                                    &ByteInput {
                                        version: 1,
                                        persistent: true,
                                        op: "input",
                                        terminal: &grant.terminal,
                                        control: &grant.handle,
                                        bytes: &bytes,
                                    },
                                    RPC_TIMEOUT,
                                ),
                                Input::Paste(text) => connection.request(
                                    &PasteInput {
                                        version: 1,
                                        persistent: true,
                                        op: "input",
                                        terminal: &grant.terminal,
                                        control: &grant.handle,
                                        text: &text,
                                        paste: true,
                                    },
                                    RPC_TIMEOUT,
                                ),
                            };
                            (grant.terminal.clone(), Op::Input, result)
                        }
                    };
                    // A framing/transport failure makes the stream uncertain. Never
                    // read another reply on it or replay a possibly applied request.
                    let transport_failed = result
                        .as_ref()
                        .err()
                        .is_some_and(|e| e.downcast_ref::<crate::client::DaemonError>().is_none());
                    if reply.send(Reply { target, op, result }).is_err() || transport_failed {
                        break;
                    }
                }
            })?;
        Ok(Self {
            controller: created["controller"].clone(),
            id: created["controller_id"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            tx,
            rx,
            controls: HashMap::new(),
            ownership: HashMap::new(),
            claims: HashMap::new(),
            uncertain: HashSet::new(),
            attempted: HashSet::new(),
            busy: HashSet::new(),
            offers: HashSet::new(),
            warnings: HashMap::new(),
            shown_warning: None,
            panel: None,
            call_offset: 0,
            mouse_tail: HashSet::new(),
            view: None,
            serial: 0,
            acknowledged_serial: 0,
            refresh_pending: false,
            refreshed: Instant::now() - REFRESH_INTERVAL,
            connected: true,
        })
    }

    fn enqueue(&mut self, work: Work, status: &mut String) -> bool {
        if !self.connected {
            *status =
                "Unknown/resynchronizing: native connection lost; detach and attach afresh".into();
            return false;
        }
        match self.tx.try_send(work) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                *status = "backpressure: native input consumed, not queued for replay".into();
                false
            }
            Err(TrySendError::Disconnected(_)) => {
                self.disconnected(status);
                false
            }
        }
    }

    fn disconnected(&mut self, status: &mut String) {
        self.connected = false;
        self.controls.clear();
        self.uncertain.extend(self.ownership.keys().cloned());
        *status =
            "Unknown/resynchronizing: native connection lost; authority is not released".into();
    }

    fn submit(&mut self, terminal: &str, op: Op, request: Value, status: &mut String) -> bool {
        let busy = !matches!(op, Op::View | Op::Consent | Op::Keep | Op::Activity);
        if busy && self.busy.contains(terminal) {
            return false;
        }
        let sent = self.enqueue(
            Work::Rpc {
                target: Arc::from(terminal),
                op,
                request,
            },
            status,
        );
        if sent && busy {
            self.busy.insert(terminal.to_owned());
        }
        sent
    }

    pub(super) fn report_view(&mut self, terminal: &str, focus: Option<bool>, status: &mut String) {
        if self
            .view
            .as_ref()
            .is_some_and(|(old, f)| old == terminal && *f == focus)
        {
            return;
        }
        let serial = self
            .serial
            .checked_add(1)
            .expect("native view serial exhausted");
        let request = json!({"op":"control-handle","handle":self.controller,"method":"view",
            "view":{"serial":serial,"terminal":if terminal.is_empty(){None}else{Some(terminal)},"focused":focus}});
        if self.submit(terminal, Op::View, request, status) {
            self.serial = serial;
            self.view = Some((terminal.to_owned(), focus));
        }
    }

    pub(super) fn poll(&mut self, active: &str, status: &mut String) -> bool {
        let mut changed = false;
        loop {
            match self.rx.try_recv() {
                Ok(reply) => {
                    changed = true;
                    let terminal = reply.target.as_ref();
                    if !matches!(
                        reply.op,
                        Op::Input | Op::View | Op::Consent | Op::Keep | Op::Activity
                    ) {
                        self.busy.remove(terminal);
                    }
                    if reply.op == Op::Refresh {
                        self.refresh_pending = false;
                    }
                    match reply.result {
                        Ok(value) => self.received(terminal, reply.op, value, status),
                        Err(error) => {
                            if let Some(error) = error.downcast_ref::<crate::client::DaemonError>()
                            {
                                *status = format!(
                                    "{}: current facts refreshed; no input or mutation replayed",
                                    safe(&error.code, 48)
                                );
                                if matches!(
                                    error.code.as_str(),
                                    "stale-handle" | "target-ended" | "not-found"
                                ) {
                                    self.controls.remove(terminal);
                                    self.uncertain.insert(terminal.to_owned());
                                }
                                if reply.op == Op::Force {
                                    // A changed owner/reservation is never silently
                                    // substituted into an already-confirmed action.
                                    self.panel = Some(Panel {
                                        target: terminal.to_owned(),
                                        kind: PanelKind::Progress,
                                    });
                                }
                                if reply.op == Op::Consent {
                                    self.warnings.remove(terminal);
                                }
                                self.refreshed = Instant::now() - REFRESH_INTERVAL;
                            } else {
                                self.disconnected(status);
                            }
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if self.connected {
                        self.disconnected(status);
                        changed = true;
                    }
                    break;
                }
            }
        }
        if self.connected && !self.refresh_pending && self.refreshed.elapsed() >= REFRESH_INTERVAL {
            let mut targets = HashSet::from([active.to_owned()]);
            targets.extend(self.controls.keys().cloned());
            targets.extend(self.claims.keys().cloned());
            if let Some(panel) = &self.panel {
                targets.insert(panel.target.clone());
            }
            targets.remove("");
            self.refresh_pending = self.enqueue(
                Work::Refresh {
                    controller: self.controller.clone(),
                    targets: targets.into_iter().collect(),
                },
                status,
            );
            self.refreshed = Instant::now();
        }
        changed
    }

    fn received(&mut self, terminal: &str, op: Op, mut value: Value, status: &mut String) {
        if op == Op::Input {
            return;
        }
        if op == Op::Refresh {
            if let Some(controls) = value["private"]["controls"].as_array() {
                self.controls
                    .retain(|target, _| controls.iter().any(|grant| grant["terminal"] == *target));
                for grant in controls {
                    self.cache_grant(grant);
                }
            }
            if let Some(states) = value["states"].as_array() {
                for state in states {
                    let Some(target) = state["ownership"]["terminal"].as_str() else {
                        continue;
                    };
                    if state["alive"] == false {
                        self.controls.remove(target);
                        self.uncertain.insert(target.to_owned());
                        self.warnings.remove(target);
                        if self
                            .panel
                            .as_ref()
                            .is_some_and(|panel| panel.target == target)
                        {
                            self.panel = None;
                        }
                    } else {
                        self.uncertain.remove(target);
                        self.observe(target, state["ownership"].clone());
                    }
                }
            }
            if let Some(summaries) = value["private"]["claims"].as_array() {
                for summary in summaries {
                    let Some(target) = summary["terminal"].as_str() else {
                        continue;
                    };
                    if let Some(current) = self
                        .claims
                        .get_mut(target)
                        .filter(|claim| claim["claim_id"] == summary["claim_id"])
                    {
                        current["phase"] = summary["phase"].clone();
                    }
                }
            }
            if let Some(claims) = value["claims"].as_array() {
                for claim in claims {
                    if let Some(target) = claim["terminal"].as_str() {
                        self.claims.insert(target.to_owned(), claim.clone());
                    }
                }
            }
            self.sync_warnings();
            self.accept_offers(status);
            // First-view acquisition is allowed only once for a lifetime. A
            // release/cancel/revocation cannot turn polling into a relock policy.
            let selected = self
                .view
                .as_ref()
                .map(|(selected, _)| selected)
                .filter(|selected| {
                    !selected.is_empty() && !self.attempted.contains(selected.as_str())
                })
                .filter(|selected| self.ownership.contains_key(selected.as_str()))
                .cloned();
            if let Some(selected) = selected {
                self.attempted.insert(selected.clone());
                let ownership = &self.ownership[&selected];
                if ownership["owner"].is_null()
                    && ownership["reservation"].is_null()
                    && ownership["unsettled"].as_array().is_some_and(Vec::is_empty)
                {
                    // Initial acquisition compares the unowned snapshot that
                    // made it eligible. Unlike a deliberate request, it must
                    // not adopt a newly arrived owner and ask them to yield.
                    self.submit(
                        &selected,
                        Op::Claim,
                        json!({"op":"ownership-claim","controller":self.controller,
                            "terminal":selected,"observed":ownership["observed"],
                            "mode":"cooperative","when":{"kind":"now"}}),
                        status,
                    );
                }
            }
            return;
        }
        if op == Op::View {
            self.acknowledged_serial = value["native_view_serial"]
                .as_u64()
                .unwrap_or(self.acknowledged_serial);
        }
        if value["ownership"].is_object() {
            self.observe(terminal, value["ownership"].take());
            self.uncertain.remove(terminal);
        }
        match value["kind"].as_str() {
            Some("control") => {
                self.cache_grant(&value);
                self.uncertain.remove(terminal);
                *status = if value["mode"] == "forced" {
                    "Forced control: prior calls may still apply; work continues".into()
                } else {
                    "Controlled here; intercepted input was not replayed".into()
                };
                if self
                    .panel
                    .as_ref()
                    .is_some_and(|panel| panel.target == terminal)
                {
                    self.panel = None;
                }
            }
            Some("claim-state") => {
                self.claims.insert(terminal.to_owned(), value);
            }
            _ => {}
        }
        if matches!(op, Op::Yield | Op::Release | Op::Cancel) {
            self.controls.remove(terminal);
            self.uncertain.insert(terminal.to_owned());
            self.panel = None;
        }
        if op == Op::Keep {
            self.warnings.remove(terminal);
        }
        self.sync_warnings();
        self.accept_offers(status);
        self.refreshed = Instant::now() - REFRESH_INTERVAL;
    }

    fn cache_grant(&mut self, grant: &Value) {
        if let Some(terminal) = grant["terminal"]
            .as_str()
            .filter(|_| grant["control"].is_object())
        {
            if self
                .controls
                .get(terminal)
                .is_some_and(|current| current.handle == grant["control"])
            {
                return;
            }
            self.controls.insert(
                terminal.to_owned(),
                Arc::new(Grant {
                    terminal: Arc::from(terminal),
                    handle: grant["control"].clone(),
                    generation: grant["generation"].as_u64().unwrap_or(0),
                }),
            );
        }
    }

    fn observe(&mut self, terminal: &str, ownership: Value) {
        if self.controls.get(terminal).is_some_and(|grant| {
            ownership["owner"]["controller_id"] != self.id
                || ownership["observed"]["generation"].as_u64() != Some(grant.generation)
        }) {
            self.controls.remove(terminal);
        }
        self.ownership.insert(terminal.to_owned(), ownership);
    }

    fn accept_offers(&mut self, status: &mut String) {
        let ready: Vec<_> = self
            .claims
            .iter()
            .filter_map(|(terminal, claim)| {
                let reference = claim["offer"]["ref"].as_str()?;
                (claim["phase"] == "offered"
                    && !self.offers.contains(reference)
                    && !self.busy.contains(terminal))
                .then(|| {
                    (
                        terminal.clone(),
                        reference.to_owned(),
                        claim["offer"].clone(),
                    )
                })
            })
            .collect();
        for (terminal, reference, offer) in ready {
            if self.submit(
                &terminal,
                Op::Accept,
                json!({"op":"control-handle","handle":offer,"method":"accept"}),
                status,
            ) {
                self.offers.insert(reference);
            }
        }
    }

    fn can_input(&self, terminal: &str) -> bool {
        self.connected && !self.uncertain.contains(terminal) && self.controls.contains_key(terminal)
    }

    pub(super) fn input(&mut self, terminal: &str, input: Input, status: &mut String) {
        let length = match &input {
            Input::Bytes(bytes) => bytes.len(),
            Input::Paste(text) => text.len(),
        };
        if length > crate::runtime::MAX_INPUT {
            *status = "invalid-input: input exceeds 64 KiB".into();
            return;
        }
        if !self.can_input(terminal) {
            self.panel.get_or_insert_with(|| Panel {
                target: terminal.to_owned(),
                kind: PanelKind::Locked,
            });
            return;
        }
        if let Some(grant) = self.controls.get(terminal).cloned() {
            self.enqueue(Work::Input { grant, input }, status);
        }
    }

    pub(super) fn activity(&mut self, terminal: &str, status: &mut String) {
        if !self.can_input(terminal) {
            return;
        }
        let Some(grant) = self.controls.get(terminal) else {
            return;
        };
        self.submit(
            terminal,
            Op::Activity,
            json!({"op":"control-handle","handle":grant.handle,"method":"activity"}),
            status,
        );
    }

    pub(super) fn open(&mut self, target: &str, force: bool, status: &mut String) {
        if target.is_empty() {
            return;
        }
        self.activity(target, status);
        if force {
            self.force_panel(target, status);
            return;
        }
        let kind = if self.can_input(target) {
            PanelKind::Owner {
                claim: None,
                observed: Value::Null,
            }
        } else if self.claims.get(target).is_some_and(pending) || self.busy.contains(target) {
            PanelKind::Progress
        } else {
            self.request(target, status);
            PanelKind::Progress
        };
        self.panel = Some(Panel {
            target: target.to_owned(),
            kind,
        });
        self.call_offset = 0;
    }

    fn request(&mut self, target: &str, status: &mut String) {
        if self.claims.get(target).is_some_and(pending) || self.busy.contains(target) {
            return;
        }
        self.attempted.insert(target.to_owned());
        if self.enqueue(
            Work::Claim {
                target: Arc::from(target),
                controller: self.controller.clone(),
            },
            status,
        ) {
            self.busy.insert(target.to_owned());
        }
    }

    fn force_panel(&mut self, target: &str, status: &mut String) {
        let Some(ownership) = self.ownership.get(target) else {
            *status = "Unknown/resynchronizing: force requires current facts".into();
            return;
        };
        self.call_offset = 0;
        status.clear();
        self.panel = Some(Panel {
            target: target.to_owned(),
            kind: PanelKind::Force {
                observed: ownership["observed"].clone(),
                owner: owner_label(ownership),
                reservation: safe(
                    ownership["reservation"]["claim_id"]
                        .as_str()
                        .unwrap_or("none"),
                    40,
                ),
                presentation: ForcePresentation::Hidden,
            },
        });
    }

    fn invalidate_force_confirmation(&mut self) {
        if let Some(Panel {
            kind: PanelKind::Force { presentation, .. },
            ..
        }) = self.panel.as_mut()
        {
            *presentation = ForcePresentation::Hidden;
        }
    }

    /// A visible ownership panel captures all application gestures. Local picker
    /// and selection remain available after Escape; no event is held for replay.
    pub(super) fn panel_event(&mut self, event: &Event, status: &mut String) -> bool {
        if matches!(event, Event::Resize(..)) {
            self.invalidate_force_confirmation();
            return false;
        }
        let Some(panel) = self.panel.as_ref() else {
            return false;
        };
        let target = panel.target.clone();
        let kind = panel.kind.clone();
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                self.activity(&target, status);
                if key.code == KeyCode::Esc {
                    self.panel = None;
                    return true;
                }
                if matches!(kind, PanelKind::Progress | PanelKind::Force { .. }) {
                    match key.code {
                        KeyCode::Up | KeyCode::PageUp => {
                            self.call_offset = self.call_offset.saturating_sub(1);
                            self.invalidate_force_confirmation();
                            return true;
                        }
                        KeyCode::Down | KeyCode::PageDown => {
                            let last = self
                                .ownership
                                .get(&target)
                                .and_then(|o| o["unsettled"].as_array())
                                .map_or(0, |calls| calls.len().saturating_sub(1));
                            self.call_offset = self.call_offset.saturating_add(1).min(last);
                            self.invalidate_force_confirmation();
                            return true;
                        }
                        _ => {}
                    }
                }
                match kind {
                    PanelKind::Locked if key.code == KeyCode::Enter => {
                        self.request(&target, status);
                        self.panel = Some(Panel {
                            target,
                            kind: PanelKind::Progress,
                        });
                    }
                    PanelKind::Progress => match key.code {
                        KeyCode::Char('f') => self.force_panel(&target, status),
                        KeyCode::Char('c') => {
                            if let Some(claim) =
                                self.claims.get(&target).filter(|claim| pending(claim))
                            {
                                self.panel = Some(Panel {
                                    target,
                                    kind: PanelKind::Cancel {
                                        claim: claim["claim"].clone(),
                                    },
                                });
                            }
                        }
                        _ => {}
                    },
                    PanelKind::Cancel { claim } if key.code == KeyCode::Enter => {
                        self.submit(
                            &target,
                            Op::Cancel,
                            json!({"op":"control-handle","handle":claim,"method":"cancel"}),
                            status,
                        );
                    }
                    PanelKind::Force {
                        observed,
                        presentation,
                        ..
                    } if key.code == KeyCode::Enter => {
                        if self
                            .ownership
                            .get(&target)
                            .is_none_or(|o| o["observed"] != observed)
                        {
                            *status = "stale-observation: owner/reservation changed; open Force again and confirm fresh facts".into();
                            self.panel = Some(Panel {
                                target,
                                kind: PanelKind::Progress,
                            });
                        } else if !matches!(presentation, ForcePresentation::Confirmable) {
                            *status = "Force confirmation blocked: read the full warning; enlarge if needed, then press Enter again".into();
                        } else {
                            self.submit(
                                &target,
                                Op::Force,
                                json!({"op":"ownership-claim","controller":self.controller,
                                "terminal":target,"mode":"force","observed":observed}),
                                status,
                            );
                        }
                    }
                    PanelKind::Owner { claim, observed } => {
                        if key.code == KeyCode::Char('k') {
                            self.panel = None;
                        } else if !self.can_input(&target)
                            || self
                                .ownership
                                .get(&target)
                                .is_none_or(|ownership| ownership["observed"] != observed)
                        {
                            *status = "stale-observation: owner/request changed; review the current panel before acting".into();
                        } else if let Some(grant) = self.controls.get(&target) {
                            if key.code == KeyCode::Char('y') && claim.is_some() {
                                self.submit(&target, Op::Yield, json!({"op":"control-handle","handle":grant.handle,"method":"yield-to","claim_id":claim}), status);
                            } else if key.code == KeyCode::Char('r') && claim.is_none() {
                                self.submit(&target, Op::Release, json!({"op":"control-handle","handle":grant.handle,"method":"release"}), status);
                            }
                        }
                    }
                    _ => {}
                }
                true
            }
            Event::Key(_) | Event::Paste(_) => true,
            Event::Mouse(mouse) => {
                self.remember_gesture(mouse.kind);
                true
            }
            _ => false,
        }
    }

    fn sync_warnings(&mut self) {
        self.warnings.retain(|terminal, warning| {
            self.controls.contains_key(terminal)
                && self
                    .ownership
                    .get(terminal)
                    .is_some_and(|ownership| ownership["reservation"]["claim_id"] == warning.claim)
        });
        for (terminal, ownership) in &self.ownership {
            if !self.can_input(terminal) {
                continue;
            }
            let reservation = &ownership["reservation"];
            let condition = &reservation["conditional"];
            let Some(claim) = reservation["claim_id"].as_str() else {
                continue;
            };
            let ready = condition["ready_for_consent"] == true;
            let recorded = condition["consent"] == "recorded";
            if !ready && !recorded {
                self.warnings.remove(terminal);
                continue;
            }
            let serial = ownership["views"]
                .as_array()
                .and_then(|views| views.iter().find(|view| view["controller_id"] == self.id))
                .and_then(|view| view["serial"].as_u64())
                .unwrap_or(0);
            let activity = ownership["activity_revision"].as_u64().unwrap_or(0);
            let warning = self
                .warnings
                .entry(terminal.clone())
                .or_insert_with(|| Warning {
                    claim: claim.to_owned(),
                    activity,
                    serial,
                    drawn: false,
                    pending: recorded,
                    keeping: false,
                });
            if !warning.keeping
                && (warning.serial != serial || (!warning.pending && warning.activity != activity))
            {
                *warning = Warning {
                    claim: claim.to_owned(),
                    activity,
                    serial,
                    drawn: false,
                    pending: recorded,
                    keeping: false,
                };
            }
        }
    }

    /// Called before routing any human event, including local wrapper actions.
    /// Keys keep and are consumed. Pending-consent paste/mouse/local activity
    /// explicitly queues keep first on the original FIFO connection.
    pub(super) fn warning_event(
        &mut self,
        active: &str,
        event: &Event,
        status: &mut String,
    ) -> (bool, bool) {
        let human = matches!(event, Event::Key(key) if key.kind != KeyEventKind::Release)
            || matches!(event, Event::Paste(_))
            || matches!(event, Event::Mouse(mouse) if mouse.kind != MouseEventKind::Moved);
        if !human {
            return (false, false);
        }
        let keeping = self.warnings.iter().any(|(target, warning)| {
            warning.drawn
                && warning.keeping
                && (target == active || self.shown_warning.as_ref() == Some(target))
        });
        let visible: Vec<_> = self
            .warnings
            .iter()
            .filter(|(terminal, warning)| {
                warning.drawn
                    && !warning.keeping
                    && (terminal.as_str() == active
                        || self.shown_warning.as_deref() == Some(terminal.as_str()))
            })
            .map(|(terminal, warning)| (terminal.clone(), warning.claim.clone()))
            .collect();
        let mut kept = false;
        for (terminal, claim) in visible {
            let Some(grant) = self.controls.get(&terminal) else {
                continue;
            };
            if !self.submit(&terminal, Op::Keep, json!({"op":"control-handle","handle":grant.handle,"method":"keep","claim_id":claim}), status) {
                // Never forward past an unsent keep in the acknowledgement
                // window, including queue backpressure.
                return (true, true);
            }
            if let Some(warning) = self.warnings.get_mut(&terminal) {
                warning.keeping = true;
            }
            kept = true;
        }
        if kept {
            self.panel = None;
        }
        let consume = (kept || keeping) && matches!(event, Event::Key(_));
        (consume, kept || keeping)
    }

    pub(super) fn after_draw(&mut self, status: &mut String) {
        // The caller reaches this only after a successful frame and drained input.
        if let Some(Panel {
            kind: PanelKind::Force { presentation, .. },
            ..
        }) = self.panel.as_mut()
            && matches!(presentation, ForcePresentation::Drawn)
        {
            *presentation = ForcePresentation::Confirmable;
        }
        let ready: Vec<_> = self
            .warnings
            .iter()
            .filter(|(_, warning)| {
                warning.drawn
                    && !warning.pending
                    && !warning.keeping
                    && warning.serial == self.serial
                    && self.acknowledged_serial == self.serial
            })
            .map(|(terminal, warning)| {
                (
                    terminal.clone(),
                    warning.claim.clone(),
                    warning.serial,
                    warning.activity,
                )
            })
            .collect();
        for (terminal, claim, serial, activity) in ready {
            let Some(grant) = self.controls.get(&terminal) else {
                continue;
            };
            if !self.submit(
                &terminal,
                Op::Consent,
                json!({"op":"control-handle","handle":grant.handle,"method":"consent-to",
                "claim_id":claim,"view_serial":serial,"activity_revision":activity}),
                status,
            ) {
                continue;
            }
            if let Some(warning) = self.warnings.get_mut(&terminal) {
                warning.pending = true;
            }
        }
    }

    fn remember_gesture(&mut self, kind: MouseEventKind) {
        match kind {
            MouseEventKind::Down(button) | MouseEventKind::Drag(button) => {
                self.mouse_tail.insert(button);
            }
            MouseEventKind::Up(button) => {
                self.mouse_tail.remove(&button);
            }
            _ => {}
        }
    }

    pub(super) fn suppress_mouse(&mut self, kind: MouseEventKind) {
        self.remember_gesture(kind);
    }

    pub(super) fn mouse_consumed(&mut self, kind: MouseEventKind) -> bool {
        match kind {
            MouseEventKind::Up(button) => self.mouse_tail.remove(&button),
            MouseEventKind::Drag(button) => self.mouse_tail.contains(&button),
            _ => false,
        }
    }

    pub(super) fn application_mouse(
        &mut self,
        terminal: &str,
        kind: MouseEventKind,
        bytes: Vec<u8>,
        status: &mut String,
    ) {
        if !self.can_input(terminal) {
            self.remember_gesture(kind);
        }
        self.input(terminal, Input::Bytes(bytes), status);
    }

    fn label(&self, terminal: &str) -> (String, Color) {
        if !self.connected || self.uncertain.contains(terminal) {
            return ("? Unknown/resynchronizing".into(), Color::Yellow);
        }
        if self.can_input(terminal) {
            return ("● Controlled here".into(), Color::Cyan);
        }
        let Some(ownership) = self.ownership.get(terminal) else {
            return ("? Unknown/resynchronizing".into(), Color::Yellow);
        };
        let waiting =
            self.claims.get(terminal).is_some_and(pending) || self.busy.contains(terminal);
        if !ownership["owner"].is_null() {
            (
                format!(
                    "🔒 {}Locked · {}",
                    if waiting { "Waiting · " } else { "" },
                    owner_label(ownership)
                ),
                Color::Yellow,
            )
        } else if waiting || !ownership["reservation"].is_null() {
            ("◷ Waiting · reserved".into(), Color::Yellow)
        } else {
            ("○ Unowned".into(), Color::Gray)
        }
    }

    /// Reuses exactly the existing header rectangle; native body coordinates,
    /// dimensions, mouse origin and the Switch button are untouched.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw(
        &mut self,
        frame: &mut Frame<'_>,
        geometry: super::view::Geometry,
        active: &str,
        snapshot: &Value,
        picker: bool,
        query: &str,
        status: &str,
        offset: usize,
    ) {
        if active.is_empty() {
            return;
        }
        let (label, mut color) = self.label(active);
        let name = snapshot["terminals"]
            .as_array()
            .and_then(|ts| ts.iter().find(|t| t["id"] == active))
            .and_then(|t| t["name"].as_str())
            .unwrap_or(active);
        let failure = snapshot["diagnostics"].as_array().and_then(|diagnostics| {
            diagnostics
                .iter()
                .rev()
                .find(|d| d["diagnostic"]["ok"] == false || d["message"].is_string())
        });
        let mut hint = if !status.is_empty() {
            safe(status, 180)
        } else if picker {
            "^Q detach · Esc return · Enter open · ^N new · ^O OMP · ^X close · ^L control · ^F force".into()
        } else if let Some(failure) = failure {
            format!(
                "{}: {}",
                safe(failure["kind"].as_str().unwrap_or("hook"), 32),
                safe(
                    failure["diagnostic"]["error"]
                        .as_str()
                        .or_else(|| failure["message"].as_str())
                        .unwrap_or("hook failed; amux snapshot shows output"),
                    140
                )
            )
        } else if offset > 0 {
            "scrollback · type to return to live surface".into()
        } else {
            "Prefix ^L control · Prefix ^F force · Prefix ^Q detach".into()
        };
        if let Some(reservation) = self
            .ownership
            .get(active)
            .map(|o| &o["reservation"])
            .filter(|r| self.can_input(active) && r["purpose"] == "manual")
        {
            hint = format!(
                "Control requested by #{} · Prefix ^L respond (typing keeps working)",
                safe(reservation["requester_id"].as_str().unwrap_or("?"), 40)
            );
        }
        // Draw each newly ready condition at least once before its consent.
        // Prefer the selected PTY once all conditions have been displayed.
        let warning_target = self
            .warnings
            .iter()
            .min_by(|(a, wa), (b, wb)| {
                (wa.drawn, a.as_str() != active, *a).cmp(&(wb.drawn, b.as_str() != active, *b))
            })
            .map(|(target, _)| target.clone());
        self.shown_warning = warning_target.clone();
        if let Some(target) = &warning_target {
            color = Color::Yellow;
            let condition = &self.ownership[target]["reservation"]["conditional"];
            let warning = self.warnings.get_mut(target).expect("warning exists");
            hint = if warning.keeping {
                "Keeping control; awaiting acknowledgement".into()
            } else if condition["when"]["kind"] == "human-idle" {
                if condition["clock"]["kind"] == "warning" {
                    format!(
                        "Keep control: any key · {}ms / {}ms warning · #{} (zero still waits)",
                        condition["clock"]["remaining_ms"],
                        condition["when"]["warning_ms"],
                        safe(target, 24)
                    )
                } else {
                    format!(
                        "Keep control: any key · requested warning {}ms · #{}",
                        condition["when"]["warning_ms"],
                        safe(target, 24)
                    )
                }
            } else {
                format!(
                    "Background consent ({}) · any key keeps #{} · no countdown",
                    condition["when"]["scope"].as_str().unwrap_or("?"),
                    safe(target, 24)
                )
            };
            // A clipped/undrawable warning is not visible consent. Retain a
            // prior real draw across resize, but never acknowledge a first
            // warning whose requested facts could not fit on this surface.
            warning.drawn |= geometry.header.height >= 3
                && hint.width() <= usize::from(geometry.header.width.saturating_sub(2));
        }
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(color))
            .title(super::clip_label(
                &format!(" amux · {} ", if picker { "WINDOWS" } else { &label }),
                geometry.header.width.saturating_sub(2),
            ))
            .title_bottom(super::clip_label(
                &hint,
                geometry.header.width.saturating_sub(2),
            ));
        let inner = block.inner(geometry.header);
        frame.render_widget(Clear, geometry.header);
        frame.render_widget(block, geometry.header);
        let middle = if picker {
            format!(
                " Search {} · {}",
                safe(
                    if query.is_empty() {
                        "name / project / directory / branch"
                    } else {
                        query
                    },
                    48
                ),
                label
            )
        } else {
            format!(" {} · {}", safe(name, 48), label)
        };
        frame.render_widget(
            Paragraph::new(super::clip_label(&middle, inner.width))
                .style(Style::default().fg(color)),
            inner,
        );
        if let Some(mut panel) = self.panel.take() {
            self.draw_panel(frame, geometry.body, &mut panel);
            self.panel = Some(panel);
        }
    }

    fn draw_panel(&self, frame: &mut Frame<'_>, body: Rect, panel: &mut Panel) {
        if let PanelKind::Force { presentation, .. } = &mut panel.kind {
            *presentation = ForcePresentation::Hidden;
        }
        let target = &panel.target;
        let width = body.width.saturating_sub(2).min(86);
        let wrapped_call_rows = 100usize.div_ceil(usize::from(width.saturating_sub(2).max(1)));
        let call_limit = (usize::from(body.height.saturating_sub(12)) / wrapped_call_rows).max(1);
        let mut lines = vec![format!("Terminal #{}", safe(target, 64))];
        let mut owner_observation = None;
        let title = match &panel.kind {
            PanelKind::Locked => {
                lines.extend([
                    "Input consumed; it will never be replayed.".into(),
                    "Enter Request control · Esc Keep viewing".into(),
                ]);
                "Locked terminal"
            }
            PanelKind::Owner { .. } => {
                let ownership = self.ownership.get(target);
                let claim = ownership
                    .map(|ownership| &ownership["reservation"])
                    .filter(|reservation| reservation["purpose"] == "manual")
                    .and_then(|reservation| reservation["claim_id"].as_str());
                owner_observation = ownership.map(|ownership| (claim, &ownership["observed"]));
                if let Some(claim) = claim {
                    lines.push(format!("Ordinary request #{}", safe(claim, 48)));
                    lines.push("y Yield to this request · k Keep working · Esc Return".into());
                } else {
                    lines.push("r Release (grants nobody) · k Keep working · Esc Return".into());
                }
                "Control owned here"
            }
            PanelKind::Progress => {
                lines.push("Waiting for control; work keeps running.".into());
                self.progress_lines(target, &mut lines, call_limit);
                lines.push("Esc Keep waiting · c Cancel takeover · f Force…".into());
                "Control requested"
            }
            PanelKind::Cancel { .. } => {
                lines.extend([
                    "Cancel takeover revokes pre-transfer authority.".into(),
                    "Grants nobody; earlier forwarded calls may still apply.".into(),
                    "Enter Cancel takeover · Esc Keep waiting".into(),
                ]);
                "Cancel takeover?"
            }
            PanelKind::Force {
                owner, reservation, ..
            } => {
                lines.push(format!("Current owner: {owner}"));
                lines.push(format!("Reservation: {reservation}"));
                lines.push("Prior calls may still apply. Turn/process keeps running.".into());
                self.call_lines(target, &mut lines, call_limit);
                lines.push("Enter Force control · Esc Keep viewing".into());
                "Force control?"
            }
        };
        let height = body
            .height
            .saturating_sub(2)
            .min((lines.len() * 2 + 2) as u16);
        if width < 4 || height < 3 {
            return;
        }
        let area = Rect::new(
            body.x + (body.width - width) / 2,
            body.y + (body.height - height) / 2,
            width,
            height,
        );
        let paragraph = Paragraph::new(lines.join("\n")).wrap(Wrap { trim: false });
        // Measure the same wrapping at the interior width, without border rows.
        let force_visible = matches!(panel.kind, PanelKind::Force { .. })
            && paragraph.line_count(width - 2) <= usize::from(height - 2);
        frame.render_widget(Clear, area);
        frame.render_widget(
            paragraph.block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(Style::default().fg(Color::Yellow)),
            ),
            area,
        );
        if force_visible && let PanelKind::Force { presentation, .. } = &mut panel.kind {
            *presentation = ForcePresentation::Drawn;
        }
        if let (
            Some((claim, observed)),
            PanelKind::Owner {
                claim: displayed_claim,
                observed: displayed_observed,
            },
        ) = (owner_observation, &mut panel.kind)
        {
            // Only an actually drawn ordinary response panel may change the
            // displayed intent. Force and Cancel keep their captured targets.
            if displayed_claim.as_deref() != claim {
                *displayed_claim = claim.map(str::to_owned);
            }
            if *displayed_observed != *observed {
                *displayed_observed = observed.clone();
            }
        }
    }

    fn progress_lines(&self, target: &str, lines: &mut Vec<String>, call_limit: usize) {
        if let Some(claim) = self.claims.get(target) {
            lines.push(format!(
                "Request #{} · {}",
                safe(claim["claim_id"].as_str().unwrap_or("?"), 48),
                safe(claim["phase"].as_str().unwrap_or("unknown"), 32)
            ));
            if let Some(blockers) = claim["blockers"].as_array() {
                for blocker in blockers {
                    if blocker["kind"] != "settlement" {
                        lines.push(format!(
                            "Awaiting {} · {}ms",
                            safe(blocker["kind"].as_str().unwrap_or("unknown"), 40),
                            blocker["elapsed_ms"]
                        ));
                    }
                }
            }
        } else if let Some(reservation) = self
            .ownership
            .get(target)
            .map(|o| &o["reservation"])
            .filter(|r| r.is_object())
        {
            lines.push(format!(
                "Contention: reserved by #{}",
                safe(reservation["requester_id"].as_str().unwrap_or("?"), 40)
            ));
            lines.push(format!(
                "Request #{} is not this attachment's claim",
                safe(reservation["claim_id"].as_str().unwrap_or("?"), 48)
            ));
        } else {
            lines.push("Awaiting owner response / current reservation facts".into());
        }
        self.call_lines(target, lines, call_limit);
    }

    fn call_lines(&self, target: &str, lines: &mut Vec<String>, limit: usize) {
        if let Some(calls) = self
            .ownership
            .get(target)
            .and_then(|o| o["unsettled"].as_array())
        {
            let offset = self.call_offset.min(calls.len().saturating_sub(1));
            if calls.len() > limit {
                lines.push(format!(
                    "Unsettled calls {}–{} / {} · ↑/↓ inspect",
                    offset + 1,
                    (offset + limit).min(calls.len()),
                    calls.len()
                ));
            }
            for call in calls.iter().skip(offset).take(limit) {
                lines.push(format!(
                    "#{} · {} · {}ms · {} outcome",
                    safe(call["id"].as_str().unwrap_or("?"), 48),
                    safe(call["kind"].as_str().unwrap_or("unknown"), 32),
                    call["elapsed_ms"],
                    safe(call["outcome"].as_str().unwrap_or("missing"), 16)
                ));
            }
        }
    }
}

fn pending(claim: &Value) -> bool {
    matches!(
        claim["phase"].as_str(),
        Some("waiting-owner" | "waiting-settlement" | "offered" | "watching" | "countdown")
    )
}

fn owner_label(ownership: &Value) -> String {
    let owner = &ownership["owner"];
    if owner.is_null() {
        return "none".into();
    }
    format!(
        "#{} · {}",
        safe(owner["controller_id"].as_str().unwrap_or("?"), 40),
        safe(owner["label"].as_str().unwrap_or("controller"), 32)
    )
}

fn safe(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| {
            !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(max)
        .collect()
}

fn rpc(connection: &mut crate::client::Connection, mut request: Value) -> Result<Value> {
    request["version"] = json!(1);
    request["persistent"] = json!(true);
    connection.request(&request, RPC_TIMEOUT)
}

fn refresh(
    connection: &mut crate::client::Connection,
    controller: Value,
    mut targets: Vec<String>,
) -> Result<Value> {
    let private = rpc(
        connection,
        json!({"op":"control-handle","handle":controller,"method":"read"}),
    )?;
    let mut claims = Vec::new();
    if let Some(summaries) = private["claims"].as_array() {
        for summary in summaries {
            if !pending(summary) {
                continue;
            }
            if let Some(target) = summary["terminal"].as_str() {
                targets.push(target.to_owned());
            }
            let state = rpc(
                connection,
                json!({"op":"control-handle","handle":summary["claim"],"method":"read"}),
            )?;
            claims.push(state);
        }
    }
    if let Some(controls) = private["controls"].as_array() {
        for control in controls {
            if let Some(target) = control["terminal"].as_str() {
                targets.push(target.to_owned());
            }
        }
    }
    targets.sort();
    targets.dedup();
    let mut states = Vec::with_capacity(targets.len());
    for target in targets {
        match rpc(connection, json!({"op":"state","terminal":target})) {
            Ok(state) => states.push(state),
            Err(error)
                if error
                    .downcast_ref::<crate::client::DaemonError>()
                    .is_some_and(|e| {
                        matches!(e.code.as_str(), "not-found" | "target-ended" | "exited")
                    }) =>
            {
                states.push(json!({"alive":false,"ownership":{"terminal":target}}));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(json!({"private":private,"states":states,"claims":claims}))
}

#[derive(serde::Serialize)]
struct ByteInput<'a> {
    version: u8,
    persistent: bool,
    op: &'static str,
    terminal: &'a str,
    control: &'a Value,
    bytes: &'a [u8],
}
#[derive(serde::Serialize)]
struct PasteInput<'a> {
    version: u8,
    persistent: bool,
    op: &'static str,
    terminal: &'a str,
    control: &'a Value,
    text: &'a str,
    paste: bool,
}

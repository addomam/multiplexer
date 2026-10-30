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
    net::Shutdown,
    os::unix::net::UnixStream,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    time::{Duration, Instant, SystemTime},
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
    Acp,
    Launch,
}

struct Grant {
    terminal: Arc<str>,
    handle: Value,
    generation: u64,
    action_authority: Option<Value>,
}

pub(super) enum Input {
    Bytes(Vec<u8>),
    Paste(String),
}

enum Work {
    Stop,
    Rpc {
        target: Arc<str>,
        op: Op,
        request: Value,
    },
    Refresh {
        controller: Value,
        targets: Vec<(String, bool)>,
    },
    Claim {
        target: Arc<str>,
        controller: Value,
        acp: bool,
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
    rejected_action: Option<Value>,
}

#[derive(Clone, Copy)]
enum Presentation {
    Hidden,
    Drawn,
    Confirmable,
    Armed,
}

struct OwnerAction {
    grant: Arc<Grant>,
    observed: Value,
    owner: Value,
    claim: Option<String>,
}

#[derive(Clone)]
enum PanelKind {
    Locked,
    Progress,
    Owner {
        action: Option<Arc<OwnerAction>>,
        presentation: Presentation,
        geometry: Option<(Rect, Rect)>,
    },
    Cancel {
        claim: Value,
    },
    Force {
        observed: Value,
        owner: String,
        reservation: String,
        presentation: Presentation,
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
    agent_targets: HashSet<String>,
    ended_targets: HashSet<String>,
    ownership: HashMap<String, Value>,
    claims: HashMap<String, Value>,
    uncertain: HashSet<String>,
    attempted: HashSet<String>,
    busy: HashSet<String>,
    offers: HashSet<String>,
    warnings: HashMap<String, Warning>,
    shown_warning: Option<String>,
    panel: Option<Panel>,
    panel_buttons: [(Rect, KeyCode); 4],
    panel_button_target: Option<(String, std::mem::Discriminant<PanelKind>)>,
    panel_focus: Option<usize>,
    panel_help: Option<super::view::Help>,
    call_offset: usize,
    mouse_tail: HashSet<MouseButton>,
    view: Option<(String, Option<bool>)>,
    serial: u64,
    acknowledged_serial: u64,
    refresh_pending: bool,
    refreshed: Instant,
    connected: bool,
    stop: Arc<AtomicBool>,
    heartbeat: Arc<parking_lot::Mutex<SystemTime>>,
    stream: UnixStream,
    socket: std::path::PathBuf,
    worker: Option<std::thread::JoinHandle<()>>,
    auto_acquire: bool,
    launched: Option<Value>,
    launch_rejection: Option<String>,
}

impl NativeControl {
    pub(super) fn register(path: &Path, daemon: &Value, observation: UnixStream) -> Result<Self> {
        let mut connection = crate::client::Connection::connect(path)?;
        let observed = rpc(&mut connection, &mut json!({"op":"snapshot"}))?;
        if !daemon.is_string() || observed["daemon_id"] != *daemon {
            return Err(crate::client::connection_lost(
                "daemon changed before native registration",
            ));
        }
        if !connection.connected() || !crate::client::stream_connected(&observation) {
            return Err(crate::client::connection_lost(
                "native registration lost its active attachment",
            ));
        }
        let stream = connection.shutdown_handle()?;
        let created = rpc(
            &mut connection,
            &mut json!({"op":"controller-create", "role":"attachment", "label":"native TUI"}),
        )?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let heartbeat = Arc::new(parking_lot::Mutex::new(SystemTime::now()));
        let worker_heartbeat = heartbeat.clone();
        let (tx, work) = mpsc::sync_channel(128);
        let (reply, rx) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("amux-native-control".into())
            .spawn(move || {
                while let Ok(work) = work.recv() {
                    // A sleeping UI cannot lend stale queued authority to a
                    // worker which happens to resume first.
                    if worker_stop.load(Ordering::Acquire)
                        || super::pause_expired(*worker_heartbeat.lock())
                        || !connection.connected()
                        || !crate::client::stream_connected(&observation)
                    {
                        worker_stop.store(true, Ordering::Release);
                        break;
                    }
                    let mut rejected_action = None;
                    let (target, op, result) = match work {
                        Work::Stop => break,
                        Work::Rpc {
                            target,
                            op,
                            mut request,
                        } => {
                            let serial = request["view"]["serial"].as_u64();
                            let mut result = rpc(&mut connection, &mut request);
                            if op == Op::Acp
                                && matches!(
                                    request["op"].as_str(),
                                    Some("acp-answer" | "acp-permission")
                                )
                                && result.as_ref().err().is_some_and(|error| {
                                    error.downcast_ref::<crate::client::DaemonError>().is_some()
                                })
                            {
                                // Daemon errors precede admission; transport failures
                                // remain ambiguous. Return identity, never answer text.
                                request.as_object_mut().unwrap().remove("values");
                                rejected_action = Some(request);
                            }
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
                        Work::Claim {
                            target,
                            controller,
                            acp,
                        } => {
                            // One deliberate claim, not replayed application input.
                            let state = if acp {
                                rpc(&mut connection, &mut json!({"op":"snapshot"}))
                                    .and_then(|snapshot| agent_state(&snapshot, &target))
                            } else {
                                rpc(
                                    &mut connection,
                                    &mut json!({"op":"state","terminal":target.as_ref()}),
                                )
                            };
                            let result = state.and_then(|state| {
                                // Observation can span a suspend or shutdown.
                                // Recheck immediately before lending authority,
                                // not only when dequeuing this compound work.
                                if worker_stop.load(Ordering::Acquire)
                                    || super::pause_expired(*worker_heartbeat.lock())
                                    || !connection.connected()
                                    || !crate::client::stream_connected(&observation)
                                {
                                    worker_stop.store(true, Ordering::Release);
                                    return Err(crate::client::connection_lost(
                                        "native claim observation outlived its active attachment",
                                    ));
                                }
                                rpc(
                                    &mut connection,
                                    &mut addressed(
                                        json!({"op":"ownership-claim","controller":controller,
                                    "observed":state["ownership"]["observed"],
                                    "mode":"cooperative","when":{"kind":"now"}}),
                                        &target,
                                        acp,
                                    ),
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
                    if reply
                        .send(Reply {
                            target,
                            op,
                            result,
                            rejected_action,
                        })
                        .is_err()
                        || transport_failed
                    {
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
            agent_targets: HashSet::new(),
            ended_targets: HashSet::new(),
            ownership: HashMap::new(),
            claims: HashMap::new(),
            uncertain: HashSet::new(),
            attempted: HashSet::new(),
            busy: HashSet::new(),
            offers: HashSet::new(),
            warnings: HashMap::new(),
            shown_warning: None,
            panel: None,
            panel_buttons: [(Rect::default(), KeyCode::Null); 4],
            panel_button_target: None,
            panel_focus: None,
            panel_help: None,
            call_offset: 0,
            mouse_tail: HashSet::new(),
            view: None,
            serial: 0,
            acknowledged_serial: 0,
            refresh_pending: false,
            refreshed: Instant::now() - REFRESH_INTERVAL,
            connected: true,
            stop,
            heartbeat,
            stream,
            socket: path.to_owned(),
            worker: Some(worker),
            auto_acquire: true,
            launched: None,
            launch_rejection: None,
        })
    }

    pub(super) fn recovered(&mut self) {
        self.auto_acquire = false;
    }

    pub(super) fn connected(&self) -> bool {
        self.connected
            && !self.stop.load(Ordering::Acquire)
            && crate::client::stream_connected(&self.stream)
    }

    pub(super) fn heartbeat(&self) {
        *self.heartbeat.lock() = SystemTime::now();
    }

    pub(super) fn stop(&mut self) {
        self.connected = false;
        self.stop.store(true, Ordering::Release);
        let _ = self.stream.shutdown(Shutdown::Both);
        let _ = self.tx.try_send(Work::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
            // Joining fences all old work. Retain only an already parsed
            // successful launch receipt as identity, never process replies as
            // live authority or restore a draft during teardown.
            for reply in self.rx.try_iter() {
                if reply.op == Op::Launch
                    && let Ok(opened) = reply.result
                {
                    self.launched = Some(opened);
                }
            }
            if !self.auto_acquire
                && let Ok(mut connection) = crate::client::Connection::connect(&self.socket)
            {
                // Existing close guards refuse owners and pending claims.
                // Cleanup never releases authority or cancels a claim; an
                // unavailable daemon leaves the registration retained.
                let _ = rpc(
                    &mut connection,
                    &mut json!({"op":"control-handle","handle":self.controller,"method":"close"}),
                );
            }
        }
        self.controls.clear();
        self.claims.clear();
        self.warnings.clear();
        self.panel = None;
    }

    pub(super) fn launch(&mut self, mut request: Value, status: &mut String) {
        self.launch_rejection = None;
        request["op"] = json!("acp-open");
        request["controller"] = self.controller.clone();
        if self.submit("", Op::Launch, request, status).is_ok() {
            *status = "Launching ACP agent once on this original native connection".into();
        }
    }

    pub(super) fn take_launched(&mut self) -> Option<Value> {
        self.launched.take()
    }

    pub(super) fn take_launch_rejection(&mut self) -> Option<String> {
        self.launch_rejection.take()
    }

    pub(super) fn launching(&self) -> bool {
        self.busy.contains("")
    }

    fn enqueue(&mut self, work: Work, status: &mut String) -> Result<(), Work> {
        if !self.connected() {
            *status =
                "Unknown/resynchronizing: native connection lost; detach and attach afresh".into();
            return Err(work);
        }
        match self.tx.try_send(work) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(work)) => {
                *status = "backpressure: action not queued; no automatic replay".into();
                Err(work)
            }
            Err(TrySendError::Disconnected(work)) => {
                self.disconnected(status);
                Err(work)
            }
        }
    }

    fn disconnected(&mut self, status: &mut String) {
        self.connected = false;
        self.stop.store(true, Ordering::Release);
        let _ = self.stream.shutdown(Shutdown::Both);
        self.claims.clear();
        self.warnings.clear();
        self.panel = None;
        self.controls.clear();
        self.uncertain.extend(self.ownership.keys().cloned());
        *status =
            "Unknown/resynchronizing: native connection lost; authority is not released".into();
    }

    fn submit(
        &mut self,
        terminal: &str,
        op: Op,
        request: Value,
        status: &mut String,
    ) -> Result<(), Value> {
        let busy = !matches!(op, Op::View | Op::Consent | Op::Keep | Op::Activity);
        if busy && self.busy.contains(terminal) {
            if matches!(op, Op::Acp) {
                *status = "busy: pending control RPC; action not queued".into();
            }
            return Err(request);
        }
        match self.enqueue(
            Work::Rpc {
                target: Arc::from(terminal),
                op,
                request,
            },
            status,
        ) {
            Ok(()) => {
                if busy {
                    self.busy.insert(terminal.to_owned());
                }
                Ok(())
            }
            Err(Work::Rpc { request, .. }) => Err(request),
            Err(_) => unreachable!("RPC enqueue returns its original owned work"),
        }
    }

    pub(super) fn sync_agent_targets(&mut self, snapshot: &Value) {
        let views = snapshot["agent_views"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        self.agent_targets
            .retain(|id| views.iter().any(|view| view["id"] == *id));
        for view in views {
            if let Some(id) = view["id"].as_str()
                && !self.agent_targets.contains(id)
            {
                self.agent_targets.insert(id.to_owned());
            }
        }
        self.ended_targets.retain(|id| {
            views.iter().any(|view| view["id"] == *id)
                || snapshot["terminals"]
                    .as_array()
                    .is_some_and(|terminals| terminals.iter().any(|terminal| terminal["id"] == *id))
        });
    }

    pub(super) fn acp_grant(&self, id: &str) -> Option<&Value> {
        self.can_input(id)
            .then(|| self.controls.get(id)?.action_authority.as_ref())
            .flatten()
    }

    pub(super) fn acp_action(&mut self, request: Value, status: &mut String) -> Result<(), Value> {
        let Some(id) = request["target"]["id"].as_str() else {
            *status = "invalid-request: captured ACP target required".into();
            return Err(request);
        };
        let id = id.to_owned();
        // Already captured by the editor; never substitute the current token.
        let result = self.submit(&id, Op::Acp, request, status);
        if result.is_ok() {
            // A new deliberate action supersedes the prior local rejection.
            // Local enqueue still makes no claim of daemon or agent admission.
            status.clear();
        }
        result
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
        let view = addressed(
            json!({"serial":serial,"focused":focus}),
            terminal,
            self.agent_targets.contains(terminal),
        );
        let request =
            json!({"op":"control-handle","handle":self.controller,"method":"view","view":view});
        if self.submit(terminal, Op::View, request, status).is_ok() {
            self.serial = serial;
            self.view = Some((terminal.to_owned(), focus));
        }
    }

    pub(super) fn poll(
        &mut self,
        active: &str,
        status: &mut String,
        mut rejected: impl FnMut(Value, Option<&Value>),
    ) -> bool {
        let controlled_before = self.can_input(active);
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
                                if reply.op == Op::Launch {
                                    self.launch_rejection = Some(error.to_string());
                                }
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
                                if terminal == active
                                    && let Some(action) = reply.rejected_action
                                {
                                    rejected(action, self.acp_grant(terminal));
                                }
                                self.refreshed = Instant::now() - REFRESH_INTERVAL;
                            } else {
                                self.disconnected(status);
                            }
                        }
                    }
                    self.invalidate_changed_owner();
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
        if controlled_before && !self.can_input(active) && !self.ended_targets.contains(active) {
            *status =
                "Control unavailable; input blocked; intercepted input was not replayed".into();
        }
        if self.connected && !self.refresh_pending && self.refreshed.elapsed() >= REFRESH_INTERVAL {
            let mut targets = HashSet::from([active.to_owned()]);
            targets.extend(self.controls.keys().cloned());
            targets.extend(self.claims.keys().cloned());
            if let Some(panel) = &self.panel {
                targets.insert(panel.target.clone());
            }
            targets.remove("");
            self.refresh_pending = self
                .enqueue(
                    Work::Refresh {
                        controller: self.controller.clone(),
                        targets: targets
                            .into_iter()
                            .map(|id| {
                                let acp = self.agent_targets.contains(&id);
                                (id, acp)
                            })
                            .collect(),
                    },
                    status,
                )
                .is_ok();
            self.refreshed = Instant::now();
        }
        changed
    }

    fn received(&mut self, terminal: &str, op: Op, mut value: Value, status: &mut String) {
        if op == Op::Input {
            return;
        }
        if op == Op::Launch {
            if let Some(id) = value["target"]["id"].as_str() {
                self.agent_targets.insert(id.to_owned());
                self.attempted.insert(id.to_owned());
                self.cache_grant(&value);
                self.launched = Some(value);
            }
            return;
        }
        if op == Op::Refresh {
            if let Some(controls) = value["private"]["controls"].as_array() {
                self.controls.retain(|target, _| {
                    controls
                        .iter()
                        .any(|grant| target_id(grant) == Some(target.as_str()))
                });
                for grant in controls {
                    self.cache_grant(grant);
                }
                self.invalidate_changed_owner();
            }
            if let Some(states) = value["states"].as_array() {
                for state in states {
                    let Some(target) = target_id(&state["ownership"]) else {
                        continue;
                    };
                    if state["alive"] == false {
                        self.controls.remove(target);
                        self.uncertain.remove(target);
                        self.ended_targets.insert(target.to_owned());
                        if status
                            == "Control unavailable; input blocked; intercepted input was not replayed"
                        {
                            *status = "Session ended; New starts another session, Close removes its record".into();
                        }
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
                    let Some(target) = target_id(summary) else {
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
                    if let Some(target) = target_id(claim) {
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
                    self.auto_acquire
                        && !selected.is_empty()
                        && !self.attempted.contains(selected.as_str())
                })
                .filter(|selected| self.ownership.contains_key(selected.as_str()))
                .cloned();
            if let Some(selected) = selected {
                self.attempted.insert(selected.clone());
                let ownership = &self.ownership[&selected];
                if ownership["owner"].is_null()
                    && ownership["reservation"].is_null()
                    && (ownership["settlement_boundary"] == "input-admission"
                        || ownership["unsettled"].as_array().is_some_and(Vec::is_empty))
                {
                    // Initial acquisition compares the unowned snapshot that
                    // made it eligible. Unlike a deliberate request, it must
                    // not adopt a newly arrived owner and ask them to yield.
                    let _ = self.submit(&selected,
                    Op::Claim,
                    addressed(json!({"op":"ownership-claim","controller":self.controller,
                        "observed":ownership["observed"],"mode":"cooperative","when":{"kind":"now"}}),
                        &selected, ownership["target"]["kind"] == "acp"),
                    status,);
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
                    String::new()
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
        if let Some(terminal) = target_id(grant).filter(|_| grant["control"].is_object()) {
            if self.controls.get(terminal).is_some_and(|current| {
                current.handle == grant["control"]
                    && current.generation == grant["generation"].as_u64().unwrap_or(0)
                    && current.action_authority.is_some() == (grant["target"]["kind"] == "acp")
            }) {
                return;
            }
            self.controls.insert(
                terminal.to_owned(),
                Arc::new(Grant {
                    terminal: Arc::from(terminal),
                    handle: grant["control"].clone(),
                    generation: grant["generation"].as_u64().unwrap_or(0),
                    action_authority: (grant["target"]["kind"] == "acp").then(|| {
                        json!({
                            "control":grant["control"],"owner_generation":grant["generation"],
                        })
                    }),
                }),
            );
            self.invalidate_changed_owner();
        }
    }

    fn observe(&mut self, terminal: &str, ownership: Value) {
        if self.controls.get(terminal).is_some_and(|grant| {
            ownership["owner"]["controller_id"] != self.id
                || ownership["observed"]["generation"].as_u64() != Some(grant.generation)
        }) {
            self.controls.remove(terminal);
        }
        if self.panel.as_ref().is_some_and(|panel| {
            panel.target == terminal && matches!(panel.kind, PanelKind::Force { .. })
        }) && self.ownership.get(terminal).is_some_and(|previous| {
            previous["observed"] != ownership["observed"]
                || previous["settlement_boundary"] != ownership["settlement_boundary"]
                || previous["unsettled"] != ownership["unsettled"]
                || previous["retained_work"] != ownership["retained_work"]
        }) {
            self.invalidate_panel_confirmation();
        }
        self.ownership.insert(terminal.to_owned(), ownership);
        self.invalidate_changed_owner();
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
            if self
                .submit(
                    &terminal,
                    Op::Accept,
                    json!({"op":"control-handle","handle":offer,"method":"accept"}),
                    status,
                )
                .is_ok()
            {
                self.offers.insert(reference);
            }
        }
    }

    fn can_input(&self, terminal: &str) -> bool {
        self.connected()
            && !self.uncertain.contains(terminal)
            && self.controls.contains_key(terminal)
    }

    pub(super) fn input(&mut self, terminal: &str, input: Input, status: &mut String) {
        if self.agent_targets.contains(terminal) {
            *status = "ACP application input consumed; native forwarding is unavailable".into();
            return;
        }
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
            let _ = self.enqueue(Work::Input { grant, input }, status);
        }
    }

    pub(super) fn activity(&mut self, terminal: &str, status: &mut String) {
        if !self.can_input(terminal) {
            return;
        }
        let Some(grant) = self.controls.get(terminal) else {
            return;
        };
        let _ = self.submit(
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
                action: None,
                presentation: Presentation::Hidden,
                geometry: None,
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
        if self
            .enqueue(
                Work::Claim {
                    target: Arc::from(target),
                    controller: self.controller.clone(),
                    acp: self.agent_targets.contains(target),
                },
                status,
            )
            .is_ok()
        {
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
                presentation: Presentation::Hidden,
            },
        });
    }

    fn invalidate_panel_confirmation(&mut self) {
        if let Some(Panel {
            kind: PanelKind::Force { presentation, .. } | PanelKind::Owner { presentation, .. },
            ..
        }) = self.panel.as_mut()
        {
            *presentation = Presentation::Hidden;
        }
    }

    fn owner_action_matches(&self, target: &str, action: &OwnerAction) -> bool {
        self.can_input(target)
            && action.grant.terminal.as_ref() == target
            && self
                .controls
                .get(target)
                .is_some_and(|grant| Arc::ptr_eq(grant, &action.grant))
            && self.ownership.get(target).is_some_and(|ownership| {
                ownership["observed"] == action.observed
                    && ownership["owner"] == action.owner
                    && ownership["owner"]["controller_id"] == self.id
                    && ownership["observed"]["generation"].as_u64() == Some(action.grant.generation)
                    && manual_claim(ownership) == action.claim.as_deref()
            })
    }

    fn invalidate_changed_owner(&mut self) {
        let changed = self.panel.as_ref().is_some_and(|panel| {
            matches!(&panel.kind, PanelKind::Owner { action: Some(action), .. }
                if !self.owner_action_matches(&panel.target, action))
        });
        if changed {
            self.invalidate_panel_confirmation();
        }
    }

    pub(super) fn panel_visible(&self) -> bool {
        self.panel.is_some()
    }

    pub(super) fn dismiss_panel(&mut self) {
        // Leaving presentation is not cancelling a claim or releasing control.
        self.panel = None;
        self.panel_help = None;
        self.panel_focus = None;
        self.panel_button_target = None;
        self.panel_buttons.fill((Rect::default(), KeyCode::Null));
    }

    /// A visible ownership panel captures all application gestures. Local picker
    /// and selection remain available after Escape; no event is held for replay.
    pub(super) fn panel_event(&mut self, event: &Event, status: &mut String) -> bool {
        if matches!(
            event,
            Event::Resize(..) | Event::FocusGained | Event::FocusLost
        ) {
            self.invalidate_panel_confirmation();
            self.panel_buttons.fill((Rect::default(), KeyCode::Null));
            self.panel_button_target = None;
            return false;
        }
        if let Some(help) = self.panel_help.as_mut() {
            if help.event(event) {
                self.panel_help = None;
                self.invalidate_panel_confirmation();
            }
            return !matches!(
                event,
                Event::Resize(..) | Event::FocusGained | Event::FocusLost
            );
        }
        let Some(panel) = self.panel.as_ref() else {
            return false;
        };
        let target = panel.target.clone();
        let kind = panel.kind.clone();
        if let Event::Mouse(mouse) = event
            && mouse.kind == MouseEventKind::Down(MouseButton::Left)
        {
            self.remember_gesture(mouse.kind);
            if self
                .panel_button_target
                .as_ref()
                .is_some_and(|(id, shown)| id == &target && *shown == std::mem::discriminant(&kind))
                && let Some((_, code)) = self
                    .panel_buttons
                    .iter()
                    .find(|(rect, _)| rect.contains((mouse.column, mouse.row).into()))
            {
                let key =
                    crossterm::event::KeyEvent::new(*code, crossterm::event::KeyModifiers::NONE);
                return self.panel_event(&Event::Key(key), status);
            }
            return true;
        }
        if let Event::Key(key) = event
            && key.kind == KeyEventKind::Press
        {
            if key.code == KeyCode::F(1) {
                self.panel_help = Some(super::view::Help::new(
                    "SESSION CONTROL\n\nTab/Shift-Tab focus visible actions; Enter/Space activates the focused action. Mouse buttons perform the same explicit actions.\n\nRequest control cooperatively; Yield accepts the exact displayed ordinary request. Release grants nobody. Both wait for complete current choices, drained old input and an armed repaint. Changed requests, authority or presentation require fresh review; repeated keys never confirm. Keep/Return remain local and preserve human control.\n\nForce only changes ownership: it never interrupts a turn, answers a question or kills a process. The complete current warning must be painted and old input drained before confirmation. A clipped warning cannot grant control.\n\nCancel takeover invalidates old control, grants nobody and retains outstanding calls; it is not cancellation of a turn.\n\nEscape closes help, then returns to viewing without cancelling a claim. The configured mode key or prefix returns to Menu. Detaching never releases ownership or cancels a claim.\n\nInput intercepted while viewing or confirming is consumed, never replayed.",
                ));
                self.invalidate_panel_confirmation();
                return true;
            }
            if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
                let count = self
                    .panel_buttons
                    .iter()
                    .filter(|(rect, _)| !rect.is_empty())
                    .count();
                if count > 0 {
                    let reverse = key.code == KeyCode::BackTab
                        || key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::SHIFT);
                    self.panel_focus = Some(match self.panel_focus {
                        Some(index) => (index + if reverse { count - 1 } else { 1 }) % count,
                        None if reverse => count - 1,
                        None => 0,
                    });
                }
                return true;
            }
            if matches!(key.code, KeyCode::Enter | KeyCode::Char(' '))
                && let Some(index) = self.panel_focus.take()
                && let Some((_, code)) = self.panel_buttons.get(index)
                && *code != key.code
            {
                let key =
                    crossterm::event::KeyEvent::new(*code, crossterm::event::KeyModifiers::NONE);
                return self.panel_event(&Event::Key(key), status);
            }
        }
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if key.kind != KeyEventKind::Press
                    && matches!(
                        &kind,
                        PanelKind::Owner { .. }
                            | PanelKind::Force { .. }
                            | PanelKind::Cancel { .. }
                    )
                {
                    return true;
                }
                self.activity(&target, status);
                if key.code == KeyCode::Esc {
                    self.panel = None;
                    return true;
                }
                if matches!(kind, PanelKind::Progress | PanelKind::Force { .. }) {
                    match key.code {
                        KeyCode::Up | KeyCode::PageUp => {
                            self.call_offset = self.call_offset.saturating_sub(1);
                            self.invalidate_panel_confirmation();
                            return true;
                        }
                        KeyCode::Down | KeyCode::PageDown => {
                            let last = self
                                .ownership
                                .get(&target)
                                .and_then(|o| {
                                    o[if o["settlement_boundary"] == "input-admission" {
                                        "retained_work"
                                    } else {
                                        "unsettled"
                                    }]
                                    .as_array()
                                })
                                .map_or(0, |calls| calls.len().saturating_sub(1));
                            self.call_offset = self.call_offset.saturating_add(1).min(last);
                            self.invalidate_panel_confirmation();
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
                        let _ = self.submit(
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
                        } else if !matches!(presentation, Presentation::Confirmable) {
                            *status = "Force confirmation blocked: wait for Enter Force control, or enlarge a clipped warning".into();
                        } else {
                            let _ = self.submit(
                                &target,
                                Op::Force,
                                addressed(
                                    json!({"op":"ownership-claim","controller":self.controller,
                            "mode":"force","observed":observed}),
                                    &target,
                                    self.ownership[&target]["target"]["kind"] == "acp",
                                ),
                                status,
                            );
                        }
                    }
                    PanelKind::Owner {
                        action,
                        presentation,
                        ..
                    } => {
                        if key.code == KeyCode::Char('k') {
                            self.dismiss_panel();
                        } else if matches!(key.code, KeyCode::Char('y' | 'r')) {
                            let Some(action) = action else {
                                return true;
                            };
                            if !self.owner_action_matches(&target, &action) {
                                self.invalidate_panel_confirmation();
                                *status = "stale-observation: owner/request changed; review the current panel before acting".into();
                            } else if !matches!(presentation, Presentation::Armed) {
                                *status = "Owner action blocked: wait for the complete current choices and input to settle".into();
                            } else if key.code == KeyCode::Char('y') && action.claim.is_some() {
                                let _ = self.submit(&target, Op::Yield, json!({"op":"control-handle","handle":action.grant.handle,"method":"yield-to","claim_id":action.claim}), status);
                            } else if key.code == KeyCode::Char('r') && action.claim.is_none() {
                                let _ = self.submit(&target, Op::Release, json!({"op":"control-handle","handle":action.grant.handle,"method":"release"}), status);
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
            if self.submit(&terminal, Op::Keep, json!({"op":"control-handle","handle":grant.handle,"method":"keep","claim_id":claim}), status).is_err() {
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

    pub(super) fn after_draw(&mut self, status: &mut String) -> bool {
        // The caller reaches this only after a successful frame and drained input.
        self.invalidate_changed_owner();
        let mut repaint = false;
        if let Some(Panel {
            kind: PanelKind::Force { presentation, .. } | PanelKind::Owner { presentation, .. },
            ..
        }) = self.panel.as_mut()
            && matches!(presentation, Presentation::Drawn)
        {
            *presentation = Presentation::Confirmable;
            repaint = true;
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
            if self
                .submit(
                    &terminal,
                    Op::Consent,
                    json!({"op":"control-handle","handle":grant.handle,"method":"consent-to",
                        "claim_id":claim,"view_serial":serial,"activity_revision":activity}),
                    status,
                )
                .is_err()
            {
                continue;
            }
            if let Some(warning) = self.warnings.get_mut(&terminal) {
                warning.pending = true;
            }
        }
        repaint
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
        if self.ended_targets.contains(terminal) {
            return ("Closed".into(), Color::Gray);
        }
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
    /// dimensions, mouse origin and the Menu button are untouched.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw(
        &mut self,
        frame: &mut Frame<'_>,
        geometry: super::view::Geometry,
        active: &str,
        snapshot: &Value,
        picker: bool,
        query: &str,
        searching: bool,
        status: &str,
        offset: usize,
    ) {
        if active.is_empty() {
            self.invalidate_panel_confirmation();
            self.panel_buttons.fill((Rect::default(), KeyCode::Null));
            self.panel_button_target = None;
            return;
        }
        if super::window_exited(snapshot, active)
            && self.panel.is_none()
            && self.warnings.is_empty()
        {
            self.shown_warning = None;
            return;
        }
        // Local ownership is the default, not an overlay. Keep the ordinary
        // project header unless a control interaction needs attention.
        if self.can_input(active)
            && self.panel.is_none()
            && self.warnings.is_empty()
            && !self
                .ownership
                .get(active)
                .is_some_and(|o| o["reservation"]["purpose"] == "manual")
        {
            self.shown_warning = None;
            return;
        }
        let (label, mut color) = self.label(active);
        let name = snapshot["terminals"]
            .as_array()
            .and_then(|ts| ts.iter().find(|t| t["id"] == active))
            .or_else(|| super::agent_view(snapshot, active))
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
            if geometry.header.width >= 10 {
                "? help"
            } else {
                ""
            }
            .into()
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
            "Open Menu for control actions or help".into()
        };
        if let Some(reservation) = self
            .ownership
            .get(active)
            .map(|o| &o["reservation"])
            .filter(|r| self.can_input(active) && r["purpose"] == "manual")
        {
            hint = format!(
                "Control requested by #{} · Menu: Control to respond (typing keeps working)",
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
        let title = if picker && geometry.header.width < 20 {
            format!("{}MENU", label.split_whitespace().next().unwrap_or("?"))
        } else if picker {
            format!(" MENU · {} ", label.split(" · ").next().unwrap_or(&label))
        } else {
            format!(" amux · {label} ")
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(color))
            .title(super::clip_label(
                &title,
                geometry.header.width.saturating_sub(2),
            ))
            .title_bottom(super::clip_label(
                &hint,
                geometry.header.width.saturating_sub(2),
            ));
        let inner = block.inner(geometry.header);
        frame.render_widget(Clear, geometry.header);
        frame.render_widget(block, geometry.header);
        if picker {
            super::view::menu_search(frame, inner, query, color, searching);
        } else {
            let middle = format!(" {} · {}", safe(name, 48), label);
            frame.render_widget(
                Paragraph::new(super::clip_label(&middle, inner.width))
                    .style(Style::default().fg(color)),
                inner,
            );
        }
        if let Some(mut panel) = self.panel.take() {
            if let Some(help) = self.panel_help.as_mut() {
                if let PanelKind::Force { presentation, .. }
                | PanelKind::Owner { presentation, .. } = &mut panel.kind
                {
                    *presentation = Presentation::Hidden;
                    self.panel_buttons.fill((Rect::default(), KeyCode::Null));
                    self.panel_button_target = None;
                }
                help.draw(frame, geometry.body);
            } else {
                self.draw_panel(frame, geometry.body, &mut panel);
            }
            self.panel = Some(panel);
        }
    }

    fn draw_panel(&mut self, frame: &mut Frame<'_>, body: Rect, panel: &mut Panel) {
        self.panel_buttons.fill((Rect::default(), KeyCode::Null));
        self.panel_button_target = None;
        if let PanelKind::Owner {
            action,
            presentation,
            geometry,
        } = &mut panel.kind
        {
            if action
                .as_ref()
                .is_none_or(|action| !self.owner_action_matches(&panel.target, action))
            {
                // Replace the entire intent only while painting fresh choices.
                // A gesture never adopts a newer claim or borrows a later grant.
                *action = self
                    .ownership
                    .get(&panel.target)
                    .filter(|ownership| {
                        self.can_input(&panel.target)
                            && ownership["owner"]["controller_id"] == self.id
                            && self.controls.get(&panel.target).is_some_and(|grant| {
                                ownership["observed"]["generation"].as_u64()
                                    == Some(grant.generation)
                            })
                    })
                    .and_then(|ownership| {
                        Some(Arc::new(OwnerAction {
                            grant: self.controls.get(&panel.target)?.clone(),
                            observed: ownership["observed"].clone(),
                            owner: ownership["owner"].clone(),
                            claim: manual_claim(ownership).map(str::to_owned),
                        }))
                    });
                *presentation = Presentation::Hidden;
                self.panel_focus = None;
            }
            let painted_geometry = (frame.area(), body);
            if *geometry != Some(painted_geometry) {
                *geometry = Some(painted_geometry);
                *presentation = Presentation::Hidden;
            }
        }
        // Keep a completed presentation across an unchanged repaint, even if
        // fresh input arrives during it. Facts, resize and scrolling invalidate
        // separately; the fit check below still rejects a clipped warning.
        let was_confirmable = matches!(
            panel.kind,
            PanelKind::Force {
                presentation: Presentation::Confirmable,
                ..
            }
        );
        let owner_ready = matches!(
            panel.kind,
            PanelKind::Owner {
                presentation: Presentation::Confirmable | Presentation::Armed,
                ..
            }
        );
        if let PanelKind::Force { presentation, .. } | PanelKind::Owner { presentation, .. } =
            &mut panel.kind
        {
            *presentation = Presentation::Hidden;
        }
        let target = &panel.target;
        let width = body.width.saturating_sub(2).min(86);
        let wrapped_call_rows = 100usize.div_ceil(usize::from(width.saturating_sub(2).max(1)));
        let call_limit = (usize::from(body.height.saturating_sub(12)) / wrapped_call_rows).max(1);
        let mut lines = vec![format!("Terminal #{}", safe(target, 64))];
        let title = match &panel.kind {
            PanelKind::Locked => {
                lines.extend([
                    "Input consumed; it will never be replayed.".into(),
                    "Enter Request control · Esc Keep viewing".into(),
                ]);
                "Locked terminal"
            }
            PanelKind::Owner { action, .. } => {
                if let Some(action) = action {
                    if let Some(claim) = &action.claim {
                        lines.push(format!("Ordinary request #{}", safe(claim, 48)));
                        lines.push(
                            if owner_ready {
                                "y Yield to this request · k Keep working · Esc Return"
                            } else {
                                "Preparing Yield · k Keep working · Esc Return"
                            }
                            .into(),
                        );
                    } else {
                        lines.push(
                            if owner_ready {
                                "r Release (grants nobody) · k Keep working · Esc Return"
                            } else {
                                "Preparing Release (grants nobody) · k Keep working · Esc Return"
                            }
                            .into(),
                        );
                    }
                } else {
                    lines.push("Owner state unavailable · k Keep working · Esc Return".into());
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
                // Never advertise an enabled action before its full paint and
                // input-drain fence. A title arriving is not that receipt.
                lines.push(
                    if was_confirmable {
                        "Enter Force control · Esc Keep viewing"
                    } else {
                        "Preparing force confirmation · Esc Keep viewing"
                    }
                    .into(),
                );
                "Force control?"
            }
        };
        // Reserve the border and a boxed action row as well as warning text.
        let height = body
            .height
            .saturating_sub(2)
            .min((lines.len() * 2 + 5) as u16);
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
        let actions: [(&str, KeyCode); 4] = match &panel.kind {
            PanelKind::Locked => [
                ("Request control", KeyCode::Enter),
                ("Keep viewing", KeyCode::Esc),
                ("Help", KeyCode::F(1)),
                ("", KeyCode::Null),
            ],
            PanelKind::Owner { action, .. } => [
                (
                    if !owner_ready {
                        "Preparing…"
                    } else if action.as_ref().is_some_and(|action| action.claim.is_some()) {
                        "Yield"
                    } else {
                        "Release"
                    },
                    if action.as_ref().is_some_and(|action| action.claim.is_some()) {
                        KeyCode::Char('y')
                    } else {
                        KeyCode::Char('r')
                    },
                ),
                ("Keep working", KeyCode::Char('k')),
                ("Return", KeyCode::Esc),
                ("Help", KeyCode::F(1)),
            ],
            PanelKind::Progress => [
                ("Cancel takeover…", KeyCode::Char('c')),
                ("Force…", KeyCode::Char('f')),
                ("Keep waiting", KeyCode::Esc),
                ("Help", KeyCode::F(1)),
            ],
            PanelKind::Cancel { .. } => [
                ("Cancel takeover", KeyCode::Enter),
                ("Keep waiting", KeyCode::Esc),
                ("Help", KeyCode::F(1)),
                ("", KeyCode::Null),
            ],
            PanelKind::Force { .. } => [
                (
                    if was_confirmable {
                        "Force control"
                    } else {
                        "Preparing…"
                    },
                    KeyCode::Enter,
                ),
                ("Keep viewing", KeyCode::Esc),
                ("Help", KeyCode::F(1)),
                ("", KeyCode::Null),
            ],
        };
        let inner = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );
        let (buttons, content) =
            super::view::action_buttons(inner, actions.map(|(label, _)| label), 1);
        let complete_visible = paragraph.line_count(content.width.max(1))
            <= usize::from(content.height)
            && !buttons[0].is_empty()
            && usize::from(buttons[0].width.saturating_sub(if buttons[0].height >= 3 {
                2
            } else {
                0
            })) >= actions[0].0.width();
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Yellow)),
            area,
        );
        frame.render_widget(paragraph, content);
        self.panel_button_target = Some((target.to_owned(), std::mem::discriminant(&panel.kind)));
        for (index, (rect, (label, code))) in buttons.into_iter().zip(actions).enumerate() {
            self.panel_buttons[index] = (rect, code);
            super::view::button(frame, rect, label, self.panel_focus == Some(index));
        }
        if complete_visible && let PanelKind::Force { presentation, .. } = &mut panel.kind {
            *presentation = if was_confirmable {
                Presentation::Confirmable
            } else {
                Presentation::Drawn
            };
        }
        if complete_visible
            && let PanelKind::Owner {
                action: Some(_),
                presentation,
                ..
            } = &mut panel.kind
        {
            *presentation = if owner_ready {
                Presentation::Armed
            } else {
                Presentation::Drawn
            };
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
            .filter(|ownership| ownership["settlement_boundary"] == "input-admission")
            .and_then(|ownership| ownership["retained_work"].as_array())
        {
            lines.push("ACP input-admission boundary; accepted work may continue.".into());
            let offset = self.call_offset.min(calls.len().saturating_sub(1));
            lines.push(format!(
                "Canonical retained work {} / {} · ↑/↓ inspect",
                offset,
                calls.len()
            ));
            for call in calls.iter().skip(offset).take(limit) {
                let outcome = match call["outcome"]["kind"].as_str() {
                    Some("response") => safe(
                        call["outcome"]["stop_reason"]
                            .as_str()
                            .unwrap_or("response"),
                        24,
                    ),
                    Some("error") => format!("error {}", call["outcome"]["code"]),
                    _ => "unknown".into(),
                };
                lines.push(format!(
                    "#{} · generation {} · {} / {}",
                    safe(call["call_id"].as_str().unwrap_or("?"), 48),
                    call["owner_generation"],
                    safe(call["state"].as_str().unwrap_or("unknown"), 24),
                    outcome
                ));
            }
            return;
        }
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

impl Drop for NativeControl {
    fn drop(&mut self) {
        self.stop();
    }
}

fn target_id(value: &Value) -> Option<&str> {
    if value["target"]["kind"] == "acp" {
        value["target"]["id"].as_str()
    } else {
        value["terminal"].as_str()
    }
}

fn manual_claim(ownership: &Value) -> Option<&str> {
    (ownership["reservation"]["purpose"] == "manual")
        .then(|| ownership["reservation"]["claim_id"].as_str())
        .flatten()
}

fn addressed(mut value: Value, id: &str, acp: bool) -> Value {
    if acp {
        value["target"] = json!({"kind":"acp","id":id});
    } else {
        value["terminal"] = if id.is_empty() {
            Value::Null
        } else {
            json!(id)
        };
    }
    value
}

fn agent_state(snapshot: &Value, id: &str) -> Result<Value> {
    let view = snapshot["agent_views"]
        .as_array()
        .and_then(|views| views.iter().find(|view| view["id"] == id))
        .ok_or_else(|| crate::client::DaemonError {
            code: "target-ended".into(),
            message: "ACP view is unavailable".into(),
        })?;
    Ok(
        json!({"alive":!matches!(view["lifecycle"].as_str(), Some("failed" | "closing" | "ended")),
        "ownership":view["ownership"]}),
    )
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
            !c.is_control()
                && !matches!(*c, '\u{061c}' | '\u{200e}' | '\u{200f}'
                    | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(max)
        .collect()
}

fn rpc(connection: &mut crate::client::Connection, request: &mut Value) -> Result<Value> {
    request["version"] = json!(1);
    request["persistent"] = json!(true);
    connection.request(request, RPC_TIMEOUT)
}

fn refresh(
    connection: &mut crate::client::Connection,
    controller: Value,
    targets: Vec<(String, bool)>,
) -> Result<Value> {
    let private = rpc(
        connection,
        &mut json!({"op":"control-handle","handle":controller,"method":"read"}),
    )?;
    let mut targets: std::collections::BTreeMap<_, _> = targets.into_iter().collect();
    let mut claims = Vec::new();
    if let Some(summaries) = private["claims"].as_array() {
        for summary in summaries {
            if !pending(summary) {
                continue;
            }
            if let Some(target) = target_id(summary) {
                let acp = summary["target"]["kind"] == "acp";
                targets
                    .entry(target.to_owned())
                    .and_modify(|old| *old |= acp)
                    .or_insert(acp);
            }
            let state = rpc(
                connection,
                &mut json!({"op":"control-handle","handle":summary["claim"],"method":"read"}),
            )?;
            claims.push(state);
        }
    }
    if let Some(controls) = private["controls"].as_array() {
        for control in controls {
            if let Some(target) = target_id(control) {
                let acp = control["target"]["kind"] == "acp";
                targets
                    .entry(target.to_owned())
                    .and_modify(|old| *old |= acp)
                    .or_insert(acp);
            }
        }
    }
    let agents = if targets.values().any(|acp| *acp) {
        Some(rpc(connection, &mut json!({"op":"snapshot"}))?)
    } else {
        None
    };
    let mut states = Vec::with_capacity(targets.len());
    for (target, acp) in targets {
        let result = if acp {
            agent_state(
                agents.as_ref().expect("ACP snapshot was requested"),
                &target,
            )
        } else {
            rpc(connection, &mut json!({"op":"state","terminal":target}))
        };
        match result {
            Ok(state) => states.push(state),
            Err(error)
                if error
                    .downcast_ref::<crate::client::DaemonError>()
                    .is_some_and(|e| {
                        matches!(e.code.as_str(), "not-found" | "target-ended" | "exited")
                    }) =>
            {
                states.push(json!({"alive":false,"ownership":addressed(json!({}), &target, acp)}));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;

    struct OwnerHarness {
        native: NativeControl,
        work: Receiver<Work>,
        _peer: UnixStream,
        _replies: mpsc::Sender<Reply>,
        status: String,
    }

    impl OwnerHarness {
        fn new(claim: Option<&str>) -> Self {
            let (tx, work) = mpsc::sync_channel(128);
            let (replies, rx) = mpsc::channel();
            let (stream, peer) = UnixStream::pair().unwrap();
            let mut native = NativeControl {
                controller: json!({"ref":"owner-controller"}),
                id: "owner-controller".into(),
                tx,
                rx,
                controls: HashMap::new(),
                agent_targets: HashSet::from(["owner-target".into()]),
                ended_targets: HashSet::new(),
                ownership: HashMap::new(),
                claims: HashMap::new(),
                uncertain: HashSet::new(),
                attempted: HashSet::new(),
                busy: HashSet::new(),
                offers: HashSet::new(),
                warnings: HashMap::new(),
                shown_warning: None,
                panel: None,
                panel_buttons: [(Rect::default(), KeyCode::Null); 4],
                panel_button_target: None,
                panel_focus: None,
                panel_help: None,
                call_offset: 0,
                mouse_tail: HashSet::new(),
                view: None,
                serial: 0,
                acknowledged_serial: 0,
                refresh_pending: false,
                refreshed: Instant::now(),
                connected: true,
                stop: Arc::new(AtomicBool::new(false)),
                heartbeat: Arc::new(parking_lot::Mutex::new(SystemTime::now())),
                stream,
                socket: std::path::PathBuf::new(),
                worker: None,
                auto_acquire: false,
                launched: None,
                launch_rejection: None,
            };
            native.cache_grant(&Self::grant("original-control", 1));
            native.observe("owner-target", Self::ownership(claim, 1));
            let mut status = String::new();
            native.open("owner-target", false, &mut status);
            Self {
                native,
                work,
                _peer: peer,
                _replies: replies,
                status,
            }
        }

        fn grant(reference: &str, generation: u64) -> Value {
            json!({
                "target":{"kind":"acp","id":"owner-target"},
                "control":{"ref":reference},"generation":generation
            })
        }

        fn ownership(claim: Option<&str>, generation: u64) -> Value {
            json!({
                "target":{"kind":"acp","id":"owner-target"},
                "observed":{"generation":generation,"reservation_id":claim},
                "owner":{"controller_id":"owner-controller","role":"attachment","label":"human"},
                "reservation":claim.map(|claim| json!({
                    "claim_id":claim,"requester_id":"external-controller",
                    "purpose":"manual","phase":"waiting"
                })),
                "settlement_boundary":"input-admission","retained_work":[]
            })
        }

        fn paint(&mut self, width: u16, height: u16) {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    let geometry = super::super::view::geometry(frame.area());
                    self.native.draw(
                        frame,
                        geometry,
                        "owner-target",
                        &json!({"agent_views":[{"id":"owner-target","name":"human session"}]}),
                        false,
                        "",
                        false,
                        &self.status,
                        0,
                    );
                })
                .unwrap();
        }

        fn input_empty(&mut self) {
            self.native.after_draw(&mut self.status);
        }

        fn arm(&mut self) {
            self.paint(110, 28);
            self.input_empty();
            self.paint(110, 28);
        }

        fn key(&mut self, code: KeyCode, kind: KeyEventKind) {
            self.native.panel_event(
                &Event::Key(crossterm::event::KeyEvent::new_with_kind(
                    code,
                    crossterm::event::KeyModifiers::NONE,
                    kind,
                )),
                &mut self.status,
            );
        }

        fn first_button(&mut self) {
            let area = self.native.panel_buttons[0].0;
            if !area.is_empty() {
                self.native.panel_event(
                    &Event::Mouse(crossterm::event::MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Left),
                        column: area.x + area.width / 2,
                        row: area.y + area.height / 2,
                        modifiers: crossterm::event::KeyModifiers::NONE,
                    }),
                    &mut self.status,
                );
            }
        }

        fn mutations(&self) -> Vec<Value> {
            self.work
                .try_iter()
                .filter_map(|work| match work {
                    Work::Rpc {
                        op: Op::Yield | Op::Release,
                        request,
                        ..
                    } => Some(request),
                    _ => None,
                })
                .collect()
        }

        fn assert_blocked(&mut self) {
            for code in [KeyCode::Char('y'), KeyCode::Char('r')] {
                self.key(code, KeyEventKind::Press);
                self.key(code, KeyEventKind::Repeat);
            }
            if self.native.panel_focus.is_none() {
                self.key(KeyCode::Tab, KeyEventKind::Press);
            }
            self.key(KeyCode::Enter, KeyEventKind::Repeat);
            self.key(KeyCode::Char(' '), KeyEventKind::Repeat);
            self.key(KeyCode::Enter, KeyEventKind::Press);
            self.key(KeyCode::Char(' '), KeyEventKind::Press);
            self.first_button();
            assert!(
                self.mutations().is_empty(),
                "unsettled Owner gesture mutated control"
            );
        }
    }

    #[test]
    fn owner_yield_and_release_need_empty_then_an_armed_exact_presentation() {
        for claim in [None, Some("manual-request")] {
            for route in 0..4 {
                let mut harness = OwnerHarness::new(claim);
                harness.assert_blocked();
                harness.paint(110, 28);
                harness.paint(110, 28);
                harness.assert_blocked();
                harness.input_empty();
                harness.assert_blocked();
                harness.paint(110, 28);
                let code = KeyCode::Char(if claim.is_some() { 'y' } else { 'r' });
                if harness.native.panel_focus.is_none() {
                    harness.key(KeyCode::Tab, KeyEventKind::Press);
                }
                harness.key(code, KeyEventKind::Repeat);
                harness.key(KeyCode::Enter, KeyEventKind::Repeat);
                harness.key(KeyCode::Char(' '), KeyEventKind::Repeat);
                assert!(
                    harness.mutations().is_empty(),
                    "Repeat granted Owner consent"
                );
                match route {
                    0 => harness.key(code, KeyEventKind::Press),
                    1 => harness.key(KeyCode::Enter, KeyEventKind::Press),
                    2 => harness.key(KeyCode::Char(' '), KeyEventKind::Press),
                    _ => harness.first_button(),
                }
                let requests = harness.mutations();
                assert_eq!(requests.len(), 1);
                assert_eq!(requests[0]["handle"], json!({"ref":"original-control"}));
                if let Some(claim) = claim {
                    assert_eq!(requests[0]["method"], "yield-to");
                    assert_eq!(requests[0]["claim_id"], claim);
                } else {
                    assert_eq!(requests[0]["method"], "release");
                    assert!(requests[0].get("claim_id").is_none());
                }
            }
        }
    }

    #[test]
    fn owner_claim_arrival_replacement_and_withdrawal_cannot_rebase_consent() {
        for (before, after) in [
            (None, Some("first-request")),
            (Some("first-request"), Some("second-request")),
            (Some("first-request"), None),
        ] {
            let mut harness = OwnerHarness::new(before);
            harness.arm();
            harness
                .native
                .observe("owner-target", OwnerHarness::ownership(after, 1));
            harness.assert_blocked();
            harness.paint(110, 28);
            harness.assert_blocked();
            harness.input_empty();
            harness.assert_blocked();
            harness.paint(110, 28);
            harness.first_button();
            let requests = harness.mutations();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0]["handle"], json!({"ref":"original-control"}));
            if let Some(claim) = after {
                assert_eq!(requests[0]["method"], "yield-to");
                assert_eq!(requests[0]["claim_id"], claim);
            } else {
                assert_eq!(requests[0]["method"], "release");
            }
        }
    }

    #[test]
    fn owner_presentation_is_invalidated_by_cover_geometry_and_transient_authority_changes() {
        for transition in 0..8 {
            let mut harness = OwnerHarness::new(Some("manual-request"));
            harness.arm();
            match transition {
                0 => {
                    harness
                        .native
                        .panel_event(&Event::Resize(110, 28), &mut harness.status);
                }
                1 => harness.paint(109, 28),
                2 => harness.paint(6, 5),
                3 => {
                    harness.key(KeyCode::F(1), KeyEventKind::Press);
                    harness.paint(110, 28);
                    harness.key(KeyCode::Esc, KeyEventKind::Press);
                }
                4 => {
                    harness
                        .native
                        .panel_event(&Event::FocusLost, &mut harness.status);
                }
                5 => {
                    harness.native.received(
                        "",
                        Op::Refresh,
                        json!({"private":{"controls":[]},"states":[]}),
                        &mut harness.status,
                    );
                    harness
                        .native
                        .cache_grant(&OwnerHarness::grant("original-control", 1));
                }
                6 => {
                    harness.native.observe(
                        "owner-target",
                        OwnerHarness::ownership(Some("intervening-request"), 1),
                    );
                    harness.native.observe(
                        "owner-target",
                        OwnerHarness::ownership(Some("manual-request"), 1),
                    );
                }
                _ => {
                    let mut ownership = OwnerHarness::ownership(Some("manual-request"), 1);
                    ownership["owner"]["label"] = json!("changed human owner facts");
                    harness.native.observe("owner-target", ownership);
                }
            }
            harness.assert_blocked();
            harness.paint(110, 28);
            harness.assert_blocked();
            harness.input_empty();
            harness.assert_blocked();
            harness.paint(110, 28);
            harness.key(KeyCode::Char('y'), KeyEventKind::Press);
            let requests = harness.mutations();
            assert_eq!(requests.len(), 1, "transition {transition} did not rearm");
            assert_eq!(requests[0]["claim_id"], "manual-request");
        }
    }

    #[test]
    fn owner_gesture_never_borrows_a_replacement_grant_or_generation() {
        for generation in [1, 2] {
            let mut harness = OwnerHarness::new(Some("manual-request"));
            harness.arm();
            harness
                .native
                .cache_grant(&OwnerHarness::grant("replacement-control", generation));
            harness.native.observe(
                "owner-target",
                OwnerHarness::ownership(Some("manual-request"), generation),
            );
            harness.assert_blocked();
            harness.paint(110, 28);
            harness.assert_blocked();
            harness.input_empty();
            harness.assert_blocked();
            harness.paint(110, 28);
            harness.key(KeyCode::Char('y'), KeyEventKind::Press);
            let requests = harness.mutations();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0]["handle"], json!({"ref":"replacement-control"}));
            assert_eq!(requests[0]["claim_id"], "manual-request");
        }
    }

    #[test]
    fn owner_keep_and_return_remain_local_before_consent_is_armed() {
        for code in [KeyCode::Char('k'), KeyCode::Esc] {
            let mut harness = OwnerHarness::new(Some("manual-request"));
            harness.key(code, KeyEventKind::Press);
            assert!(harness.native.panel.is_none());
            assert!(harness.native.can_input("owner-target"));
            assert_eq!(
                manual_claim(&harness.native.ownership["owner-target"]),
                Some("manual-request")
            );
            assert!(harness.mutations().is_empty());
        }
    }

    #[test]
    fn e06s03_queued_native_launch_is_fenced_by_observation_loss() {
        struct Root(std::path::PathBuf);
        impl Drop for Root {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        fn read(connection: &mut BufReader<UnixStream>) -> Option<Value> {
            let mut line = String::new();
            (connection.read_line(&mut line).unwrap() != 0)
                .then(|| serde_json::from_str(&line).unwrap())
        }
        fn respond(connection: &mut BufReader<UnixStream>, value: Value) {
            serde_json::to_writer(
                connection.get_mut(),
                &json!({"version":1,"ok":true,"result":value}),
            )
            .unwrap();
            connection.get_mut().write_all(b"\n").unwrap();
        }
        let root = Root(std::path::PathBuf::from("/tmp").join(format!(
            "amux-worker-loss-{}",
            uuid::Uuid::new_v4().simple()
        )));
        std::fs::create_dir(&root.0).unwrap();
        std::fs::set_permissions(&root.0, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = root.0.join("control.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (held, observed) = mpsc::channel();
        let (release, waiting) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(RPC_TIMEOUT)).unwrap();
            stream.set_write_timeout(Some(RPC_TIMEOUT)).unwrap();
            let mut connection = BufReader::new(stream);
            assert_eq!(read(&mut connection).unwrap()["op"], "snapshot");
            respond(&mut connection, json!({"daemon_id":"worker-loss-test"}));
            assert_eq!(read(&mut connection).unwrap()["op"], "controller-create");
            respond(
                &mut connection,
                json!({"controller":"private-test-controller","controller_id":"native-test"}),
            );
            assert_eq!(read(&mut connection).unwrap()["method"], "read");
            held.send(()).unwrap();
            waiting.recv_timeout(RPC_TIMEOUT).unwrap();
            respond(&mut connection, json!({"controls":[],"claims":[]}));
            let dispatched = read(&mut connection);
            if dispatched.is_some() {
                respond(&mut connection, json!({}));
            }
            dispatched.is_some_and(|request| request["op"] == "acp-open")
        });
        let (observation, observation_peer) = UnixStream::pair().unwrap();
        let mut native =
            NativeControl::register(&path, &json!("worker-loss-test"), observation).unwrap();
        let mut status = String::new();
        native.poll("", &mut status, |_, _| {});
        observed.recv_timeout(RPC_TIMEOUT).unwrap();
        native.launch(
            json!({"op":"acp-open","project":"selected-project","agent":"selected-profile"}),
            &mut status,
        );
        assert!(
            native.launching(),
            "the deliberate launch must already be queued"
        );
        // No UI poll/teardown runs while the older worker read is held. A
        // fresh heartbeat cannot stand in for the lost observation socket.
        native.heartbeat();
        observation_peer.shutdown(Shutdown::Both).unwrap();
        release.send(()).unwrap();
        let launched_after_loss = server.join().unwrap();
        native.stop();
        assert!(
            !launched_after_loss,
            "queued launch crossed the original observation-loss fence"
        );
    }
}

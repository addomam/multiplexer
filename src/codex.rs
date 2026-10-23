//! Concrete Codex 0.160.0 local app-server integration.
//!
//! One worker owns each WebSocket. State/authority locks never span provider I/O.
//! Provider response IDs and native input settlement are deliberately independent:
//! `serverRequest/resolved` names a request, not the response that won its race.

use crate::control::Authority;
use anyhow::{Context, Result, bail, ensure};
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::io::{ErrorKind, Read};
use std::net::Shutdown;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tungstenite::client::client_with_config;
use tungstenite::protocol::{Message, WebSocket, WebSocketConfig};
use uuid::Uuid;

const FRAME_LIMIT: usize = 4 * 1024 * 1024;
const CALL_LIMIT: usize = 256;
const REQUEST_LIMIT: usize = 1024;
const INSTANCE_LIMIT: usize = 128;
const EVENT_LIMIT: usize = 512;
const RESPONSE_DEADLINE: Duration = Duration::from_secs(180);
const BACKEND_DEADLINE: Duration = Duration::from_secs(30);

type Event = (String, String, Value);
type Socket = WebSocket<UnixStream>;

pub(crate) struct Broker {
    inner: Arc<Inner>,
}

struct Inner {
    runtime: PathBuf,
    instances: Mutex<BTreeMap<String, Arc<Instance>>>,
    stopped: AtomicBool,
}

/// An unbound lease removes only its own instance on every failure path.
/// After bind, the Broker owns the execution, not this stack value.
pub(crate) struct Prepared {
    pub instance_id: String,
    pub command: String,
    pub env: BTreeMap<String, String>,
    broker: Weak<Inner>,
}

struct Instance {
    id: String,
    root: PathBuf,
    state: Mutex<State>,
    sender: mpsc::SyncSender<Job>,
    stop: AtomicBool,
    cleanup: Mutex<()>,
    process: Mutex<Option<OwnedBackend>>,
    connection: Mutex<Option<UnixStream>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

struct OwnedBackend {
    child: Child,
    tree: crate::runtime::ProcessTree,
    cleanup_attempted: bool,
}

impl OwnedBackend {
    fn new(mut child: Child) -> Result<Self> {
        let Some(tree) = crate::runtime::ProcessTree::capture_owned_session(child.id() as i32)
        else {
            terminate(&mut child);
            bail!("unavailable: cannot capture private backend process identity");
        };
        Ok(Self {
            child,
            tree,
            cleanup_attempted: false,
        })
    }

    fn terminate(&mut self) -> bool {
        self.cleanup_attempted = true;
        self.tree.terminate();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let reaped = matches!(self.child.try_wait(), Ok(Some(_)));
            if reaped && self.tree.terminated() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for OwnedBackend {
    fn drop(&mut self) {
        if !self.cleanup_attempted {
            let _ = self.terminate();
        }
    }
}

struct State {
    terminal: Option<String>,
    session: Option<String>,
    provider_session: Option<String>,
    joining_session: Option<String>,
    expected_resume: Option<String>,
    frontend: &'static str,
    backend: &'static str,
    connection: &'static str,
    session_state: &'static str,
    observation: &'static str,
    observation_started: Option<Instant>,
    activity: &'static str,
    turn: Option<String>,
    turn_outcome: Option<String>,
    ended_turns: BTreeMap<String, String>,
    direct_input: bool,
    gap: bool,
    problem: Option<&'static str>,
    pending: BTreeMap<RequestId, Pending>,
    tombstones: BTreeMap<(String, String, String, RequestId), Value>,
    ambiguous_requests: BTreeSet<RequestId>,
    calls: VecDeque<Call>,
    events: VecDeque<Event>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum RequestId {
    Integer(i64),
    String(String),
}

impl RequestId {
    fn parse(value: &Value) -> Result<Self> {
        if let Some(id) = value.as_i64() {
            Ok(Self::Integer(id))
        } else if let Some(id) = value.as_str() {
            Ok(Self::String(id.to_owned()))
        } else {
            bail!("invalid-request: provider request ID must be a string or signed integer")
        }
    }

    fn value(&self) -> Value {
        match self {
            Self::Integer(id) => json!(id),
            Self::String(id) => json!(id),
        }
    }
}

struct Pending {
    session: String,
    turn: String,
    item: String,
    kind: &'static str,
    method: String,
    params: Value,
    state: &'static str,
    call: Option<String>,
}

impl Pending {
    fn public(&self, id: &RequestId) -> Value {
        let mut value = json!({"session_id":self.session,"turn_id":self.turn,
            "item_id":self.item,"request_id":id.value(),"kind":self.kind,
            "provider_method":self.method,"state":self.state,
            "supported":matches!(self.kind,"question"|"command"|"file-change"),
            "details":public_details(self.kind, &self.params)});
        if self.kind == "unsupported" {
            value["unsupported_reason"] =
                json!(if self.method == "item/permissions/requestApproval" {
                    "provider-only-offers-turn-or-session-grants"
                } else {
                    "unrecognized-request-shape"
                });
        }
        if self.kind == "question" {
            value["questions"] = public_questions(&self.params["questions"]);
        }
        value
    }
}

struct Call {
    id: String,
    op: String,
    target: Value,
    turn: Option<String>,
    item: Option<String>,
    request: Option<RequestId>,
    state: &'static str,
    provider_ack: bool,
    reason: Option<&'static str>,
    authority: Arc<Authority>,
    forwarded: Option<Instant>,
}

impl Call {
    fn public(&self) -> Value {
        json!({"call_id":self.id,"op":self.op,"target":self.target,
            "turn_id":self.turn,"item_id":self.item,"request_id":self.request.as_ref().map(RequestId::value),
            "state":self.state,"provider_ack":self.provider_ack,"reason":self.reason})
    }

    fn finish(&mut self, state: &'static str, reason: Option<&'static str>) {
        self.state = state;
        self.reason = reason;
        if self.forwarded.is_some() {
            self.authority.settle(&self.id, state);
        }
    }
}

struct Job {
    call_id: String,
    op: String,
    request: Value,
}

impl Broker {
    /// Lazy: ordinary daemon/shell startup does not require Codex or authentication.
    pub fn new(runtime: &Path) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(Inner {
                runtime: runtime.to_owned(),
                instances: Mutex::new(BTreeMap::new()),
                stopped: AtomicBool::new(false),
            }),
        })
    }

    pub fn prepare(&self, request: &Value) -> Result<Prepared> {
        request_shape(
            request,
            &[
                "version",
                "persistent",
                "op",
                "controller",
                "source",
                "project",
                "name",
                "cwd",
                "model",
                "resume_thread",
            ],
            &["op", "controller", "model"],
        )?;
        ensure!(
            text(request, "op")? == "agent-open",
            "invalid-request: expected agent-open"
        );
        shape(&request["controller"], &["kind", "ref"], &["kind", "ref"])?;
        ensure!(
            text(&request["controller"], "kind")? == "controller",
            "wrong-kind: expected controller"
        );
        text(&request["controller"], "ref")?;
        let model = text(request, "model")?;
        ensure!(
            !model.trim().is_empty() && model.len() <= 256,
            "invalid-request: explicit model must be nonblank and bounded"
        );
        for field in ["source", "project", "name", "cwd", "resume_thread"] {
            if request.get(field).is_some() {
                text(request, field)?;
            }
        }
        let resume = request.get("resume_thread").and_then(Value::as_str);
        if let Some(id) = resume {
            ensure!(
                Uuid::parse_str(id).is_ok(),
                "invalid-request: resume_thread must be a persisted Codex UUID"
            );
        }
        ensure!(
            !self.inner.stopped.load(Ordering::Acquire),
            "target-ended: broker stopped"
        );
        // Reserve capacity before expensive work without retaining the map lock over I/O.
        ensure!(
            self.inner.instances.lock().len() < INSTANCE_LIMIT,
            "settlement-unavailable: instance retention limit reached"
        );
        let id = Uuid::new_v4().to_string();
        let root = self.inner.runtime.join(format!("cx-{}", &id[..12]));
        fs::create_dir_all(&self.inner.runtime)?;
        private_dir(&root)?;
        let result = self.prepare_at(request, &id, &root, model, resume);
        if result.is_err() {
            let _ = fs::remove_dir_all(&root);
        }
        result
    }

    fn prepare_at(
        &self,
        request: &Value,
        id: &str,
        root: &Path,
        model: &str,
        resume: Option<&str>,
    ) -> Result<Prepared> {
        for dir in ["home", "config", "runtime", "codex"] {
            private_dir(&root.join(dir))?;
        }
        let endpoint = root.join("server.sock");
        ensure!(
            endpoint.as_os_str().as_encoded_bytes().len() < 104,
            "unsupported: private Codex socket path exceeds Unix transport limit"
        );
        let env = BTreeMap::from([
            ("HOME".to_owned(), path_text(&root.join("home"))?),
            (
                "XDG_CONFIG_HOME".to_owned(),
                path_text(&root.join("config"))?,
            ),
            (
                "XDG_RUNTIME_DIR".to_owned(),
                path_text(&root.join("runtime"))?,
            ),
            ("CODEX_HOME".to_owned(), path_text(&root.join("codex"))?),
        ]);
        let executable = path_text(&resolve_codex_executable()?)?;
        check_version(Path::new(&executable), &env)?;
        // Only copies credentials at an explicitly requested managed launch, never new().
        // No credential values are command arguments, public facts, events or diagnostics.
        let source_home = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")));
        if let Some(source) = source_home.as_ref() {
            copy_private_file(
                &source.join("auth.json"),
                &root.join("codex/auth.json"),
                false,
            )?;
            if let Some(resume) = resume {
                copy_resume(source, &root.join("codex"), resume)?;
            }
        } else {
            ensure!(
                resume.is_none(),
                "unavailable: persisted resume source is unavailable"
            );
        }
        let address = format!("unix://{}", path_text(&endpoint)?);
        let mut command = Command::new(&executable);
        command
            .args(["app-server", "--listen", &address])
            .envs(&env)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // A distinct session is essential: inherited login-session membership is
        // never an owned-resource identity.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        if let Some(cwd) = request.get("cwd").and_then(Value::as_str) {
            command.current_dir(cwd);
        }
        let child = command
            .spawn()
            .context("unavailable: cannot start private Codex backend")?;
        let backend = OwnedBackend::new(child)?;
        let socket = connect_backend(&endpoint, &backend)?;
        let control_socket = socket.get_ref().try_clone()?;
        let (sender, receiver) = mpsc::sync_channel(32);
        let instance = Arc::new(Instance {
            id: id.to_owned(),
            root: root.to_owned(),
            sender,
            stop: AtomicBool::new(false),
            cleanup: Mutex::new(()),
            process: Mutex::new(Some(backend)),
            connection: Mutex::new(Some(control_socket)),
            worker: Mutex::new(None),
            state: Mutex::new(State {
                terminal: None,
                session: None,
                provider_session: None,
                joining_session: None,
                expected_resume: resume.map(str::to_owned),
                frontend: "starting",
                backend: "running",
                connection: "ready",
                session_state: "discovering",
                observation: "waiting-for-first-turn",
                observation_started: None,
                activity: "unknown",
                turn: None,
                turn_outcome: None,
                ended_turns: BTreeMap::new(),
                direct_input: false,
                gap: false,
                problem: None,
                pending: BTreeMap::new(),
                tombstones: BTreeMap::new(),
                ambiguous_requests: BTreeSet::new(),
                calls: VecDeque::new(),
                events: VecDeque::new(),
            }),
        });
        {
            let mut instances = self.inner.instances.lock();
            if self.inner.stopped.load(Ordering::Acquire) || instances.len() >= INSTANCE_LIMIT {
                drop(instances);
                instance.close("launch-cancelled");
                bail!("target-ended: launch cancelled or instance limit reached");
            }
            instances.insert(id.to_owned(), instance.clone());
        }
        let worker_instance = instance.clone();
        match thread::Builder::new()
            .name(format!("codex-{}", &id[..8]))
            .spawn(move || run(worker_instance, socket, receiver))
        {
            Ok(worker) => *instance.worker.lock() = Some(worker),
            Err(error) => {
                self.inner.instances.lock().remove(id);
                instance.close("worker-start-failed");
                return Err(error.into());
            }
        }
        // Executable + arguments ONLY. Parent applies exec/env within its startup source.
        let mut args = vec![executable];
        if let Some(resume) = resume {
            args.extend(["resume".to_owned(), resume.to_owned()]);
        }
        args.extend([
            "--remote".to_owned(),
            address,
            "--model".to_owned(),
            model.to_owned(),
            "--ask-for-approval".to_owned(),
            "on-request".to_owned(),
            "--sandbox".to_owned(),
            "workspace-write".to_owned(),
            "--no-alt-screen".to_owned(),
        ]);
        if let Some(cwd) = request.get("cwd").and_then(Value::as_str) {
            args.extend(["--cd".to_owned(), cwd.to_owned()]);
        }
        Ok(Prepared {
            instance_id: id.to_owned(),
            command: args
                .iter()
                .map(|arg| shell_quote(arg))
                .collect::<Vec<_>>()
                .join(" "),
            env,
            broker: Arc::downgrade(&self.inner),
        })
    }

    pub fn bind(&self, instance: &str, terminal: &str) -> Result<()> {
        ensure!(
            !terminal.is_empty(),
            "invalid-request: terminal is required"
        );
        let instances = self.inner.instances.lock();
        ensure!(
            !instances
                .values()
                .any(|candidate| candidate.state.lock().terminal.as_deref() == Some(terminal)),
            "invalid-request: terminal already has a managed instance"
        );
        let instance = instances
            .get(instance)
            .context("stale-target: unknown Codex instance")?;
        let mut state = instance.state.lock();
        ensure!(
            state.terminal.is_none(),
            "invalid-request: instance already bound"
        );
        ensure!(
            state.backend == "running" && state.connection == "ready",
            "unavailable: prepared backend already failed"
        );
        ensure!(
            !instance.stop.load(Ordering::Acquire),
            "target-ended: instance stopped"
        );
        state.terminal = Some(terminal.to_owned());
        state.emit(
            "agent-lifecycle",
            json!({"instance_id":instance.id,"frontend":"starting"}),
        );
        Ok(())
    }

    pub fn inspect(&self, request: &Value) -> Result<Value> {
        request_shape(
            request,
            &["version", "persistent", "op", "terminal", "instance_id"],
            &["op", "terminal", "instance_id"],
        )?;
        ensure!(
            text(request, "op")? == "agent-inspect",
            "invalid-request: expected agent-inspect"
        );
        let instance = self.instance(text(request, "instance_id")?)?;
        let state = instance.state.lock();
        ensure!(
            state.terminal.as_deref() == Some(text(request, "terminal")?),
            "stale-target: terminal does not match instance"
        );
        Ok(state.snapshot(&instance.id))
    }

    pub fn mutate(&self, op: &str, request: &Value, authority: Arc<Authority>) -> Result<Value> {
        validate_mutation(op, request)?;
        authority.check()?;
        let instance = self.instance(text(&request["target"], "instance_id")?)?;
        let mut state = instance.state.lock();
        state.validate_target(&instance.id, &request["target"])?;
        state.validate_action(op, request)?;
        while state.calls.len() >= CALL_LIMIT {
            let removable = state
                .calls
                .iter()
                .position(|call| matches!(call.state, "applied" | "rejected"));
            let Some(index) = removable else {
                bail!("settlement-unavailable: unresolved call retention limit reached");
            };
            state.calls.remove(index);
        }
        let call_id = Uuid::new_v4().to_string();
        let call = Call {
            id: call_id.clone(),
            op: op.to_owned(),
            target: request["target"].clone(),
            turn: request
                .get("turn_id")
                .or_else(|| request.get("expected_turn_id"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            item: request
                .get("item_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            request: request
                .get("request_id")
                .map(RequestId::parse)
                .transpose()?,
            state: "queued",
            provider_ack: false,
            reason: None,
            authority,
            forwarded: None,
        };
        let receipt = call.public();
        state.calls.push_back(call);
        if instance
            .sender
            .try_send(Job {
                call_id: call_id.clone(),
                op: op.to_owned(),
                request: request.clone(),
            })
            .is_err()
        {
            state.finish(&call_id, "rejected", Some("dispatch-queue-unavailable"));
            bail!("unavailable: dispatch queue unavailable; request was not forwarded");
        }
        state.emit("agent-call", receipt.clone());
        Ok(receipt)
    }

    pub fn launch_failed(&self, terminal: &str, _message: &str) {
        // Parent may retain its redacted diagnostic; arbitrary shell/provider text
        // never enters this module's public facts.
        for instance in self.bound(terminal) {
            instance.state.lock().frontend = "exited";
            instance.close("native-start-failed");
        }
    }

    pub fn end_terminal(&self, terminal: &str) {
        for instance in self.bound(terminal) {
            instance.state.lock().frontend = "exited";
            instance.close("terminal-ended");
        }
    }

    pub fn shutdown(&self) {
        self.inner.stopped.store(true, Ordering::Release);
        let instances: Vec<_> = self.inner.instances.lock().values().cloned().collect();
        for instance in instances {
            instance.close("broker-stopped");
        }
    }

    pub fn take_events(&self) -> Vec<Event> {
        let instances: Vec<_> = self.inner.instances.lock().values().cloned().collect();
        let mut events = Vec::new();
        for instance in instances {
            for (kind, terminal, mut data) in instance.state.lock().events.drain(..) {
                data["instance_id"] = json!(instance.id);
                events.push((kind, terminal, data));
            }
        }
        events
    }

    fn instance(&self, id: &str) -> Result<Arc<Instance>> {
        self.inner
            .instances
            .lock()
            .get(id)
            .cloned()
            .context("stale-target: unknown Codex instance")
    }

    fn bound(&self, terminal: &str) -> Vec<Arc<Instance>> {
        self.inner
            .instances
            .lock()
            .values()
            .filter(|instance| instance.state.lock().terminal.as_deref() == Some(terminal))
            .cloned()
            .collect()
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl Drop for Prepared {
    fn drop(&mut self) {
        let Some(broker) = self.broker.upgrade() else {
            return;
        };
        let mut instances = broker.instances.lock();
        if instances
            .get(&self.instance_id)
            .is_some_and(|instance| instance.state.lock().terminal.is_none())
        {
            let instance = instances.remove(&self.instance_id);
            drop(instances);
            if let Some(instance) = instance {
                instance.close("unbound-launch-dropped");
            }
        }
    }
}

impl Instance {
    fn close(&self, reason: &'static str) {
        // Stop revokes work; this lock makes concurrent callers wait for the
        // same worker/process/resource teardown attempt to finish.
        let _cleanup = self.cleanup.lock();
        if self.stop.swap(true, Ordering::AcqRel) {
            return;
        }
        {
            let mut state = self.state.lock();
            state.degrade(reason);
        }
        if let Some(socket) = self.connection.lock().take() {
            let _ = socket.shutdown(Shutdown::Both);
        }
        if let Some(worker) = self.worker.lock().take() {
            let _ = worker.join();
        }
        let cleaned = self
            .process
            .lock()
            .take()
            .is_none_or(|mut backend| backend.terminate());
        {
            let mut state = self.state.lock();
            state.backend = if cleaned { "exited" } else { "unknown" };
            if !cleaned {
                state.problem = Some("owned-process-cleanup-incomplete");
            }
            let backend = state.backend;
            state.emit(
                "agent-lifecycle",
                json!({"instance_id":self.id,"backend":backend,"reason":reason}),
            );
        }
        if fs::remove_dir_all(&self.root).is_err() && self.root.exists() {
            let mut state = self.state.lock();
            state.problem = Some("owned-resource-cleanup-incomplete");
            state.emit(
                "agent-lifecycle",
                json!({"reason":"owned-resource-cleanup-incomplete"}),
            );
        }
    }
}

impl State {
    fn emit(&mut self, kind: &str, data: Value) {
        let Some(terminal) = self.terminal.as_ref() else {
            return;
        };
        if self.events.len() >= EVENT_LIMIT {
            self.events.pop_front();
            // Snapshot is authoritative; parent assigns the global event cursor.
            self.events.push_back((
                "agent-resynchronization-required".to_owned(),
                terminal.clone(),
                json!({"reason":"broker-event-gap"}),
            ));
            if self.events.len() >= EVENT_LIMIT {
                self.events.pop_front();
            }
        }
        self.events
            .push_back((kind.to_owned(), terminal.clone(), data));
    }

    fn snapshot(&self, instance: &str) -> Value {
        let target = self.session.as_ref().map(
            |session| json!({"terminal":self.terminal,"instance_id":instance,"session_id":session}),
        );
        let ready = self.connection == "ready"
            && self.session_state == "ready"
            && !self.gap
            && self.frontend == "attached";
        json!({"terminal":self.terminal,"instance_id":instance,"target":target,"provider_session_id":self.provider_session,
            "lifecycle":{"frontend":self.frontend,"backend":self.backend,"connection":self.connection,
                "session":self.session_state,"observation":self.observation,"activity":self.activity,"turn_id":self.turn,"turn_outcome":self.turn_outcome},
            "turn_outcomes":self.ended_turns,
            "capabilities":{"prompt":ready && self.direct_input,"steer":ready && self.direct_input,
                "answer":ready,"permission":ready,"interrupt":ready},
            "pending":self.pending.iter().map(|(id,pending)| {
                let mut value = pending.public(id);
                value["target"] = json!({"terminal":self.terminal,"instance_id":instance,"session_id":pending.session});
                value
            }).collect::<Vec<_>>(),
            "tombstones":self.tombstones.values().collect::<Vec<_>>(),
            "calls":self.calls.iter().map(Call::public).collect::<Vec<_>>(),
            "resynchronization_required":self.gap,"problem":self.problem})
    }

    fn validate_target(&self, instance: &str, target: &Value) -> Result<()> {
        ensure!(
            self.terminal.as_deref() == Some(text(target, "terminal")?)
                && instance == text(target, "instance_id")?,
            "stale-target: wrong terminal or instance"
        );
        ensure!(
            self.session.as_deref() == Some(text(target, "session_id")?),
            "stale-target: session unavailable or replaced"
        );
        ensure!(
            self.frontend == "attached"
                && self.backend == "running"
                && self.connection == "ready"
                && self.session_state == "ready"
                && !self.gap,
            "unavailable: managed Codex is degraded or not discovered"
        );
        Ok(())
    }

    fn validate_action(&self, op: &str, request: &Value) -> Result<()> {
        let session = text(&request["target"], "session_id")?;
        match op {
            "agent-prompt" => {
                ensure!(
                    self.direct_input,
                    "unsupported: thread cannot accept direct input"
                );
                match text(request, "intent")? {
                    "start" => {
                        ensure!(
                            self.turn.is_none()
                                && self.activity == "idle"
                                && self.pending.is_empty(),
                            "turn-conflict: start requires an idle thread without pending input"
                        );
                        ensure!(
                            !self.calls.iter().any(|call| call.op == "agent-prompt"
                                && call.request.is_none()
                                && call.turn.is_none()
                                && matches!(call.state, "queued" | "forwarded" | "unknown")),
                            "turn-conflict: a start is already unresolved"
                        );
                    }
                    "steer" => ensure!(
                        self.turn.as_deref() == Some(text(request, "expected_turn_id")?),
                        "stale-turn: expected turn is not active"
                    ),
                    _ => bail!("invalid-request: prompt intent must be start or steer"),
                }
            }
            "agent-interrupt" => {
                let turn = text(request, "turn_id")?;
                ensure!(
                    self.turn.as_deref() == Some(turn),
                    "stale-turn: exact turn is not active"
                );
                ensure!(
                    !self.calls.iter().any(|call| call.op == op
                        && call.turn.as_deref() == Some(turn)
                        && call.state != "rejected"),
                    "stale-turn: interrupt already submitted for this exact turn"
                );
            }
            "agent-answer" | "agent-permission" => {
                let id = RequestId::parse(&request["request_id"])?;
                let pending = self
                    .pending
                    .get(&id)
                    .context("stale-request: no matching live provider request")?;
                ensure!(
                    pending.session == session
                        && pending.turn == text(request, "turn_id")?
                        && pending.item == text(request, "item_id")?,
                    "stale-request: complete pending identity mismatch"
                );
                ensure!(
                    self.turn.as_deref() == Some(pending.turn.as_str()),
                    "stale-request: request turn is no longer active"
                );
                ensure!(
                    pending.state == "pending" && pending.call.is_none(),
                    "stale-request: response already dispatched or uncertain"
                );
                if op == "agent-answer" {
                    ensure!(
                        pending.kind == "question",
                        "unsupported: request is not a supported question"
                    );
                    validate_answers(&pending.params, &request["answers"])?;
                } else {
                    let decision = text(request, "decision")?;
                    match pending.kind {
                        "command" => {
                            // The native menu can omit decline. The pinned
                            // response protocol still accepts request-only
                            // denial without the interruption implied by cancel.
                            if decision == "allow"
                                && !pending.params["availableDecisions"].is_null()
                            {
                                let decisions = pending.params["availableDecisions"]
                                    .as_array()
                                    .context("unsupported: malformed approval decisions")?;
                                ensure!(
                                    decisions
                                        .iter()
                                        .any(|value| value.as_str() == Some("accept")),
                                    "unsupported: request does not offer this request-only decision"
                                );
                            }
                        }
                        "file-change" => ensure!(
                            decision == "deny" || pending.params["grantRoot"].is_null(),
                            "unsupported: grantRoot has no proved request-only allow semantics"
                        ),
                        _ => bail!(
                            "unsupported: approval has no supported request-only allow/deny shape"
                        ),
                    }
                }
            }
            _ => bail!("invalid-request: unknown agent mutation"),
        }
        Ok(())
    }

    fn encode(&self, op: &str, request: &Value) -> Result<Value> {
        self.validate_action(op, request)?;
        let session = text(&request["target"], "session_id")?;
        match op {
            "agent-prompt" => {
                let input =
                    json!([{ "type":"text", "text":text(request,"text")?, "text_elements":[] }]);
                if request["intent"] == "start" {
                    Ok(json!({"method":"turn/start","params":{"threadId":session,"input":input}}))
                } else {
                    Ok(
                        json!({"method":"turn/steer","params":{"threadId":session,"expectedTurnId":request["expected_turn_id"],"input":input}}),
                    )
                }
            }
            "agent-interrupt" => Ok(
                json!({"method":"turn/interrupt","params":{"threadId":session,"turnId":request["turn_id"]}}),
            ),
            "agent-answer" => {
                Ok(json!({"id":request["request_id"],"result":{"answers":request["answers"]}}))
            }
            "agent-permission" => Ok(
                json!({"id":request["request_id"],"result":{"decision":if request["decision"] == "allow" {"accept"} else {"decline"}}}),
            ),
            _ => bail!("invalid-request: unknown agent mutation"),
        }
    }

    fn finish(&mut self, id: &str, state: &'static str, reason: Option<&'static str>) {
        let Some(call) = self.calls.iter_mut().find(|call| call.id == id) else {
            return;
        };
        call.finish(state, reason);
        let public = call.public();
        self.emit("agent-call", public);
    }

    fn degrade(&mut self, reason: &'static str) {
        self.connection = "lost";
        self.session_state = "unknown";
        self.observation = "unavailable";
        self.observation_started = None;
        self.activity = "unknown";
        self.gap = true;
        self.problem = Some(reason);
        for pending in self.pending.values_mut() {
            pending.state = "unknown";
        }
        for call in &mut self.calls {
            match call.state {
                "queued" => call.finish("rejected", Some("not-dispatched")),
                "forwarded" => call.finish("unknown", Some(reason)),
                _ => {}
            }
        }
        self.emit(
            "agent-lifecycle",
            json!({"connection":"lost","reason":reason,"resynchronization_required":true}),
        );
    }
}

fn validate_mutation(op: &str, request: &Value) -> Result<()> {
    let mut allowed = vec!["version", "persistent", "op", "target", "control"];
    let mut required = vec!["op", "target", "control"];
    match op {
        "agent-prompt" => {
            allowed.extend(["intent", "text", "expected_turn_id"]);
            required.extend(["intent", "text"]);
            let intent = text(request, "intent")?;
            ensure!(
                matches!(intent, "start" | "steer"),
                "invalid-request: explicit start or steer intent required"
            );
            ensure!(
                (intent == "steer") == request.get("expected_turn_id").is_some(),
                "invalid-request: expected_turn_id is required only for steer"
            );
            text(request, "text")?;
            if intent == "steer" {
                text(request, "expected_turn_id")?;
            }
        }
        "agent-answer" | "agent-permission" => {
            allowed.extend(["turn_id", "item_id", "request_id"]);
            required.extend(["turn_id", "item_id", "request_id"]);
            text(request, "turn_id")?;
            text(request, "item_id")?;
            RequestId::parse(&request["request_id"])?;
            if op == "agent-answer" {
                allowed.push("answers");
                required.push("answers");
            } else {
                allowed.push("decision");
                required.push("decision");
                ensure!(
                    matches!(text(request, "decision")?, "allow" | "deny"),
                    "invalid-request: permission decision must be allow or deny"
                );
            }
        }
        "agent-interrupt" => {
            allowed.push("turn_id");
            required.push("turn_id");
            text(request, "turn_id")?;
        }
        _ => bail!("invalid-request: unsupported agent operation"),
    }
    request_shape(request, &allowed, &required)?;
    ensure!(
        text(request, "op")? == op,
        "invalid-request: operation mismatch"
    );
    shape(
        &request["target"],
        &["terminal", "instance_id", "session_id"],
        &["terminal", "instance_id", "session_id"],
    )?;
    for field in ["terminal", "instance_id", "session_id"] {
        text(&request["target"], field)?;
    }
    shape(&request["control"], &["kind", "ref"], &["kind", "ref"])?;
    ensure!(
        text(&request["control"], "kind")? == "control",
        "wrong-kind: expected control"
    );
    text(&request["control"], "ref")?;
    serde_json::to_writer(MessageSize(0), request)
        .context("invalid-request: agent request too large")?;
    Ok(())
}

struct MessageSize(usize);

impl std::io::Write for MessageSize {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > FRAME_LIMIT / 2 - self.0 {
            return Err(std::io::Error::new(
                ErrorKind::InvalidInput,
                "agent request exceeds bound",
            ));
        }
        self.0 += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn validate_answers(params: &Value, answers: &Value) -> Result<()> {
    let questions = params["questions"]
        .as_array()
        .context("unsupported: malformed question collection")?;
    let answers = answers
        .as_object()
        .context("invalid-request: answers must be keyed by question ID")?;
    ensure!(
        !questions.is_empty() && answers.len() == questions.len(),
        "invalid-request: answer every exact question ID"
    );
    let mut ids = BTreeSet::new();
    for question in questions {
        let id = text(question, "id")?;
        ensure!(ids.insert(id), "unsupported: duplicate question IDs");
        let answer = answers
            .get(id)
            .context("invalid-request: missing exact question ID")?;
        shape(answer, &["answers"], &["answers"])?;
        let choices = answer["answers"]
            .as_array()
            .context("invalid-request: answer values must be arrays")?;
        ensure!(
            !choices.is_empty() && choices.len() <= 128,
            "invalid-request: empty or excessive answer selection"
        );
        let options = question
            .get("options")
            .filter(|options| !options.is_null())
            .map(|options| {
                options
                    .as_array()
                    .context("unsupported: malformed question options")
            })
            .transpose()?;
        let other = question["isOther"].as_bool().unwrap_or(false);
        let mut seen = BTreeSet::new();
        for choice in choices {
            let choice = choice
                .as_str()
                .filter(|text| !text.is_empty())
                .context("invalid-request: answer must contain nonempty strings")?;
            ensure!(
                seen.insert(choice),
                "invalid-request: duplicate answer choice"
            );
            if let Some(options) = options {
                ensure!(
                    other
                        || options
                            .iter()
                            .any(|option| option["label"].as_str() == Some(choice)),
                    "invalid-request: answer is not an offered choice"
                );
            }
        }
    }
    Ok(())
}

fn run(instance: Arc<Instance>, mut socket: Socket, receiver: mpsc::Receiver<Job>) {
    let mut discovery = Instant::now() - Duration::from_secs(1);
    let mut discovery_pending: Option<(&'static str, Instant)> = None;
    'worker: while !instance.stop.load(Ordering::Acquire) {
        let exited = {
            let mut process = instance.process.lock();
            if process
                .as_ref()
                .is_some_and(|backend| backend.tree.root_exited())
            {
                process.take()
            } else {
                None
            }
        };
        if let Some(mut backend) = exited {
            let cleaned = backend.terminate();
            if !cleaned {
                *instance.process.lock() = Some(backend);
            }
            let mut state = instance.state.lock();
            state.backend = if cleaned { "exited" } else { "unknown" };
            state.degrade("backend-exited");
            break;
        }
        let discovering = instance.state.lock().session_state == "discovering";
        if discovering
            && discovery_pending.is_none()
            && discovery.elapsed() >= Duration::from_millis(250)
        {
            if send(
                &mut socket,
                &json!({"id":"amux:loaded","method":"thread/loaded/list","params":{"limit":2}}),
            )
            .is_err()
            {
                break;
            }
            discovery = Instant::now();
            discovery_pending = Some(("amux:loaded", Instant::now()));
        }
        let subscribe = {
            let mut state = instance.state.lock();
            if state.session_state == "ready"
                && state.observation != "ready"
                && (state.activity != "idle" || state.expected_resume.is_some())
            {
                let started = *state.observation_started.get_or_insert_with(Instant::now);
                if started.elapsed() > BACKEND_DEADLINE {
                    state.degrade("native-observer-attachment-unavailable");
                    break;
                }
                if discovery_pending.is_none() && discovery.elapsed() >= Duration::from_millis(250)
                {
                    state.observation = "attaching";
                    state.session.clone()
                } else {
                    None
                }
            } else {
                None
            }
        };
        if let Some(session) = subscribe {
            // A real first native/API turn materializes history. Resume only the
            // already native-created identity; this never creates a setup turn.
            if send(&mut socket, &json!({"id":"amux:observe","method":"thread/resume",
                "params":{"threadId":session,"excludeTurns":true,
                    "initialTurnsPage":{"limit":1,"sortDirection":"desc","itemsView":"notLoaded"}}})).is_err()
            {
                break;
            }
            discovery = Instant::now();
            discovery_pending = Some(("amux:observe", Instant::now()));
        }
        if discovery_pending.is_some_and(|(_, started)| started.elapsed() > BACKEND_DEADLINE) {
            instance.state.lock().degrade("discovery-timeout");
            break;
        }
        // Drain a bounded observation batch before queued actions. A continuously
        // streaming model must not starve an explicit steer/interrupt.
        for _ in 0..64 {
            match socket.read() {
                Ok(Message::Text(message)) => match serde_json::from_str::<Value>(&message) {
                    Ok(message) => {
                        if handle_message(&instance, &mut socket, &message, &mut discovery_pending)
                            .is_err()
                        {
                            instance
                                .state
                                .lock()
                                .degrade("provider-protocol-unavailable");
                            break 'worker;
                        }
                    }
                    Err(_) => {
                        instance.state.lock().degrade("invalid-provider-message");
                        break 'worker;
                    }
                },
                Ok(Message::Ping(_)) => {
                    if socket.flush().is_err() {
                        break 'worker;
                    }
                }
                Ok(Message::Pong(_)) => {}
                Ok(Message::Close(_)) => break 'worker,
                Ok(_) => {
                    instance.state.lock().degrade("unsupported-provider-frame");
                    break 'worker;
                }
                Err(tungstenite::Error::Io(error))
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    break;
                }
                Err(_) => break 'worker,
            }
        }
        match receiver.try_recv() {
            Ok(job) => {
                let message = {
                    let mut state = instance.state.lock();
                    // Exclude this reservation from conflict checks while preserving
                    // all competing queued/forwarded starts and interrupts.
                    let Some(index) = state
                        .calls
                        .iter()
                        .position(|call| call.id == job.call_id && call.state == "queued")
                    else {
                        continue;
                    };
                    let mut call = state.calls.remove(index).expect("located call");
                    let result = state
                        .validate_target(&instance.id, &job.request["target"])
                        .and_then(|()| state.encode(&job.op, &job.request))
                        .and_then(|message| {
                            call.authority.forward(&call.id, "agent-mutation")?;
                            Ok(message)
                        });
                    let result = match result {
                        Ok(mut message) => {
                            call.state = "forwarded";
                            call.forwarded = Some(Instant::now());
                            if let Some(id) = call.request.as_ref() {
                                if let Some(pending) = state.pending.get_mut(id) {
                                    pending.state = "forwarded";
                                    pending.call = Some(call.id.clone());
                                }
                            } else {
                                message["id"] = json!(call.id);
                            }
                            Ok(message)
                        }
                        Err(_) => {
                            call.state = "rejected";
                            call.reason = Some("stale-at-dispatch");
                            Err(())
                        }
                    };
                    let public = call.public();
                    state.calls.insert(index, call);
                    state.emit("agent-call", public);
                    result
                };
                if let Ok(message) = message
                    && send(&mut socket, &message).is_err()
                {
                    break;
                }
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => break,
        }
        let mut state = instance.state.lock();
        let expired: Vec<_> = state
            .calls
            .iter()
            .filter(|call| {
                call.state == "forwarded"
                    && call
                        .forwarded
                        .is_some_and(|at| at.elapsed() >= RESPONSE_DEADLINE)
            })
            .map(|call| call.id.clone())
            .collect();
        for id in expired {
            state.finish(&id, "unknown", Some("provider-response-timeout"));
        }
    }
    {
        let mut state = instance.state.lock();
        if state.connection != "lost" {
            state.degrade("provider-connection-lost");
        }
    }
    let _ = socket.get_mut().shutdown(Shutdown::Both);
}

fn handle_message(
    instance: &Instance,
    socket: &mut Socket,
    message: &Value,
    discovery: &mut Option<(&'static str, Instant)>,
) -> Result<()> {
    if message.get("method").is_none() {
        let mut state = instance.state.lock();
        let provider_id = RequestId::parse(&message["id"])?;
        let id = message["id"].as_str().unwrap_or("");
        if id == "amux:observe" {
            ensure!(
                discovery
                    .as_ref()
                    .is_some_and(|(id, _)| *id == "amux:observe"),
                "unsupported: unsolicited observer attachment response"
            );
            *discovery = None;
            if message["error"]["code"] == -32600 {
                // A submitted first turn can precede persisted metadata. This
                // observation-only attachment stays visibly pending and bounded;
                // no mutation is replayed and no error becomes a ready observer.
                return Ok(());
            }
            ensure!(
                message.get("error").is_none(),
                "unsupported: observer attachment rejected"
            );
            ensure!(
                state.connection == "ready" && !state.gap,
                "unavailable: observer attachment completed after invalidation"
            );
            let thread = &message["result"]["thread"];
            ensure!(
                thread["id"].as_str() == state.session.as_deref(),
                "stale-target: observer attachment names another conversation"
            );
            let turns = message["result"]["initialTurnsPage"]["data"]
                .as_array()
                .context("unsupported: observer omitted requested turn page")?;
            for turn in turns {
                let turn_id = text(turn, "id")?;
                match text(turn, "status")? {
                    "completed" | "interrupted" | "failed" => {
                        ensure!(
                            state.ended_turns.len() < REQUEST_LIMIT
                                || state.ended_turns.contains_key(turn_id),
                            "unsupported: observer turn retention limit"
                        );
                        state
                            .ended_turns
                            .insert(turn_id.to_owned(), text(turn, "status")?.to_owned());
                        if state.turn.as_deref() == Some(turn_id) {
                            state.turn = None;
                            state.turn_outcome = Some(text(turn, "status")?.to_owned());
                        }
                    }
                    "inProgress" if state.ended_turns.contains_key(turn_id) => {}
                    "inProgress" => {
                        ensure!(
                            state
                                .turn
                                .as_deref()
                                .is_none_or(|current| current == turn_id),
                            "unsupported: observer snapshot overlaps another turn"
                        );
                        state.turn = Some(turn_id.to_owned());
                        state.turn_outcome = None;
                    }
                    _ => bail!("unsupported: unknown observer turn status"),
                }
            }
            state.observation = "ready";
            state.observation_started = None;
            state.activity = if !state.pending.is_empty() {
                "awaiting-input"
            } else if state.turn.is_some() {
                "running"
            } else {
                "idle"
            };
            let snapshot = state.snapshot(&instance.id);
            state.emit("agent-observer-ready", snapshot);
            return Ok(());
        }
        if id == "amux:loaded" {
            ensure!(
                discovery
                    .as_ref()
                    .is_some_and(|(id, _)| *id == "amux:loaded"),
                "unsupported: duplicate discovery response"
            );
            *discovery = None;
            ensure!(
                message.get("error").is_none(),
                "unsupported: thread discovery rejected"
            );
            let threads = message["result"]["data"]
                .as_array()
                .context("unsupported: invalid loaded threads")?;
            ensure!(
                message["result"]["nextCursor"].is_null() && threads.len() <= 1,
                "unsupported: ambiguous native conversation"
            );
            if let Some(thread) = threads.first() {
                let session = thread
                    .as_str()
                    .context("unsupported: invalid loaded thread ID")?;
                ensure!(
                    state
                        .expected_resume
                        .as_deref()
                        .is_none_or(|expected| expected == session),
                    "stale-target: unexpected resumed thread"
                );
                state.joining_session = Some(session.to_owned());
                drop(state);
                // Metadata binds the native-created live identity without a
                // setup turn. It does not subscribe this connection to requests
                // or turn events; explicit live resume follows the first real
                // turn, with its snapshot and pending-request replay.
                send(
                    socket,
                    &json!({"id":"amux:join","method":"thread/read","params":{"threadId":session,"includeTurns":false}}),
                )?;
                *discovery = Some(("amux:join", Instant::now()));
            }
            return Ok(());
        }
        if id == "amux:join" {
            ensure!(
                discovery.as_ref().is_some_and(|(id, _)| *id == "amux:join"),
                "unsupported: unsolicited join response"
            );
            *discovery = None;
            ensure!(
                message.get("error").is_none(),
                "unsupported: native conversation join rejected"
            );
            let thread = &message["result"]["thread"];
            let session = text(thread, "id")?;
            ensure!(
                state.joining_session.as_deref() == Some(session),
                "stale-target: join response names another conversation"
            );
            ensure!(
                state
                    .pending
                    .values()
                    .all(|pending| pending.session == session),
                "stale-target: pre-discovery request belongs to another conversation"
            );
            ensure!(
                state
                    .expected_resume
                    .as_deref()
                    .is_none_or(|expected| expected == session),
                "stale-target: unexpected resumed thread"
            );
            ensure!(
                state
                    .session
                    .as_deref()
                    .is_none_or(|current| current == session),
                "stale-target: discovered session replaced"
            );
            state.session = Some(session.to_owned());
            state.provider_session = thread["sessionId"].as_str().map(str::to_owned);
            state.joining_session = None;
            state.session_state = "ready";
            state.frontend = "attached";
            state.direct_input = thread["canAcceptDirectInput"].as_bool().unwrap_or(false);
            state.activity = match thread["status"]["type"].as_str() {
                Some("idle") => "idle",
                Some("active") => "running",
                _ => "unknown",
            };
            if !state.pending.is_empty() {
                state.activity = "awaiting-input";
            }
            let snapshot = state.snapshot(&instance.id);
            state.emit("agent-discovered", snapshot);
            return Ok(());
        }
        let Some(index) = state.calls.iter().position(|call| call.id == id) else {
            // A rejection sent on this connection belongs to our response, but
            // reused request IDs make old/new response attribution unavailable.
            if message.get("error").is_some() {
                let matching: Vec<_> = state
                    .calls
                    .iter()
                    .filter(|call| {
                        call.request.as_ref() == Some(&provider_id) && call.forwarded.is_some()
                    })
                    .map(|call| call.id.clone())
                    .collect();
                if matching.len() == 1 && !state.ambiguous_requests.contains(&provider_id) {
                    state.finish(&matching[0], "rejected", Some("provider-response-rejected"));
                }
            }
            return Ok(());
        };
        ensure!(
            state.calls[index].forwarded.is_some(),
            "unsupported: reply for undispatched call"
        );
        if matches!(state.calls[index].state, "applied" | "rejected") {
            return Ok(());
        }
        if message.get("error").is_some() {
            state.finish(id, "rejected", Some("provider-rejected"));
        } else if message.get("result").is_some() {
            let op = state.calls[index].op.as_str();
            if op == "agent-prompt" {
                let expected = state.calls[index].turn.clone();
                let turn = if expected.is_some() {
                    message["result"]["turnId"].as_str()
                } else {
                    message["result"]["turn"]["id"].as_str()
                };
                let Some(turn) = turn else {
                    state.finish(id, "unknown", Some("invalid-provider-reply"));
                    return Ok(());
                };
                if expected.as_deref().is_some_and(|expected| expected != turn) {
                    state.finish(id, "unknown", Some("provider-turn-mismatch"));
                } else {
                    state.calls[index].turn = Some(turn.to_owned());
                    state.calls[index].provider_ack = true;
                    // A late response never replaces a newer turn or resurrects an
                    // already completed turn; turn events own lifecycle transitions.
                    if state.turn.is_none() && !state.ended_turns.contains_key(turn) {
                        state.turn = Some(turn.to_owned());
                        state.turn_outcome = None;
                        state.activity = "running";
                    }
                    state.finish(id, "applied", None);
                }
            } else if op == "agent-interrupt" {
                state.calls[index].provider_ack = true;
                state.finish(id, "applied", None);
            }
        }
        return Ok(());
    }
    let method = text(message, "method")?;
    let params = &message["params"];
    let mut state = instance.state.lock();
    if message.get("id").is_some() {
        let id = RequestId::parse(&message["id"])?;
        let session = text(params, "threadId")?;
        if state
            .session
            .as_deref()
            .or(state.joining_session.as_deref())
            .is_some_and(|expected| expected != session)
        {
            return Ok(());
        }
        let turn = text(params, "turnId")?;
        let item = text(params, "itemId")?;
        let key = (
            session.to_owned(),
            turn.to_owned(),
            item.to_owned(),
            id.clone(),
        );
        if state.tombstones.contains_key(&key) {
            return Ok(());
        }
        if state
            .tombstones
            .keys()
            .any(|(_, _, _, previous)| *previous == id)
        {
            state.ambiguous_requests.insert(id.clone());
        }
        if let Some(existing) = state.pending.get(&id) {
            if existing.session != session
                || existing.turn != turn
                || existing.item != item
                || existing.params != *params
            {
                state.degrade("provider-request-identity-collision");
            }
            return Ok(());
        }
        if state.pending.len() + state.tombstones.len() >= REQUEST_LIMIT {
            state.degrade("request-retention-limit");
            return Ok(());
        }
        let kind = match method {
            "item/tool/requestUserInput" if valid_questions(params) => "question",
            "item/commandExecution/requestApproval" if valid_approval(params, true) => "command",
            "item/fileChange/requestApproval" if valid_approval(params, false) => "file-change",
            // Installed permissions API only grants turn/session scope, not the
            // required exact-request scope. Never broaden an explicit allow.
            "item/permissions/requestApproval" => "unsupported",
            _ => "unsupported",
        };
        let pending = Pending {
            session: session.to_owned(),
            turn: turn.to_owned(),
            item: item.to_owned(),
            kind,
            method: method.to_owned(),
            params: params.clone(),
            state: "pending",
            call: None,
        };
        let public = pending.public(&id);
        state.pending.insert(id, pending);
        if state.connection == "ready" && !state.gap {
            state.observation = "ready";
            state.observation_started = None;
        }
        state.activity = "awaiting-input";
        state.emit("agent-request", public);
        return Ok(());
    }
    let session = params.get("threadId").and_then(Value::as_str);
    if method == "thread/started" {
        let thread = &params["thread"];
        if let Some(current) = state.session.as_deref()
            && thread["id"].as_str() != Some(current)
            && thread["source"].get("subAgent").is_none()
            && thread["parentThreadId"].is_null()
            // Native 0.160.0 generates titles in a separate ephemeral
            // thread. That auxiliary is not a replacement conversation.
            && !(thread["ephemeral"] == true && thread["threadSource"] == "thread_title")
        {
            state.degrade("native-session-replaced");
        }
        return Ok(());
    }
    if session != state.session.as_deref() || session.is_none() {
        return Ok(());
    }
    match method {
        "turn/started" => {
            if state.connection == "ready" && !state.gap {
                state.observation = "ready";
                state.observation_started = None;
            }
            let turn = text(&params["turn"], "id")?;
            if state.ended_turns.contains_key(turn) {
                return Ok(());
            }
            if state.turn.as_deref().is_some_and(|current| current != turn) {
                state.degrade("overlapping-provider-turns");
            } else {
                state.turn = Some(turn.to_owned());
                state.turn_outcome = None;
                state.activity = "running";
                state.emit(
                    "agent-turn",
                    json!({"session_id":session,"turn_id":turn,"state":"running"}),
                );
            }
        }
        "turn/completed" => {
            let turn = text(&params["turn"], "id")?;
            let outcome = text(&params["turn"], "status")?;
            ensure!(
                matches!(outcome, "completed" | "interrupted" | "failed"),
                "unsupported: unknown terminal turn status"
            );
            if state.ended_turns.len() >= REQUEST_LIMIT && !state.ended_turns.contains_key(turn) {
                state.degrade("turn-retention-limit");
                return Ok(());
            }
            state
                .ended_turns
                .insert(turn.to_owned(), outcome.to_owned());
            if state.turn.as_deref() == Some(turn) {
                state.turn = None;
                state.turn_outcome = Some(outcome.to_owned());
                state.activity = if state.pending.is_empty() {
                    "idle"
                } else {
                    "awaiting-input"
                };
            }
            state.emit(
                "agent-turn",
                json!({"session_id":session,"turn_id":turn,"state":"completed","outcome":outcome}),
            );
        }
        "serverRequest/resolved" => {
            let id = RequestId::parse(&params["requestId"])?;
            if state.ambiguous_requests.contains(&id) {
                let call = state.pending.get_mut(&id).and_then(|pending| {
                    pending.state = "unknown";
                    pending.call.clone()
                });
                if let Some(call) = call {
                    state.finish(
                        &call,
                        "unknown",
                        Some("reused-request-resolution-ambiguous"),
                    );
                }
                state.emit(
                    "agent-request-unknown",
                    json!({"request_id":id.value(),"reason":"reused-request-resolution-ambiguous"}),
                );
                return Ok(());
            }
            if let Some(pending) = state.pending.remove(&id) {
                let mut public = pending.public(&id);
                public["state"] = json!("resolved");
                // This event carries no winning responder/answer. A write followed
                // by resolution is NOT sufficient evidence that our answer won.
                if let Some(call) = pending.call.as_deref() {
                    state.finish(call, "unknown", Some("request-resolved-winner-unavailable"));
                }
                state.tombstones.insert(
                    (pending.session, pending.turn, pending.item, id),
                    public.clone(),
                );
                if state.pending.is_empty() {
                    state.activity = if state.turn.is_some() {
                        "running"
                    } else {
                        "idle"
                    };
                }
                state.emit("agent-request-resolved", public);
            }
        }
        "thread/status/changed" => match params["status"]["type"].as_str() {
            Some("systemError" | "notLoaded") => state.degrade("provider-thread-unavailable"),
            Some("active") => {
                state.activity =
                    if params["status"]["activeFlags"]
                        .as_array()
                        .is_some_and(|flags| {
                            flags.iter().any(|flag| {
                                flag == "waitingOnUserInput" || flag == "waitingOnApproval"
                            })
                        })
                    {
                        "awaiting-input"
                    } else {
                        "running"
                    };
            }
            Some("idle") if state.turn.is_none() && state.pending.is_empty() => {
                state.activity = "idle"
            }
            _ => {}
        },
        "thread/closed" | "thread/archived" => state.degrade("native-thread-ended"),
        "error" if params["willRetry"].as_bool() != Some(true) => {
            state.problem = Some("provider-turn-error");
        }
        _ => {}
    }
    Ok(())
}

fn valid_approval(params: &Value, command: bool) -> bool {
    if params["startedAtMs"].as_i64().is_none() {
        return false;
    }
    if command {
        params
            .get("kind")
            .is_none_or(|kind| matches!(kind.as_str(), Some("command" | "writeStdin")))
            && (params["availableDecisions"].is_null() || params["availableDecisions"].is_array())
    } else {
        params["grantRoot"].is_null() || params["grantRoot"].is_string()
    }
}

fn valid_questions(params: &Value) -> bool {
    let Some(questions) = params["questions"].as_array() else {
        return false;
    };
    if questions.is_empty() || questions.len() > 128 || !params["isBlocking"].is_boolean() {
        return false;
    }
    let mut ids = BTreeSet::new();
    questions.iter().all(|question| {
        shape(
            question,
            &["id", "header", "question", "isOther", "isSecret", "options"],
            &["id", "header", "question"],
        )
        .is_ok()
            && text(question, "id").is_ok_and(|id| ids.insert(id))
            && question["header"].is_string()
            && question["question"].is_string()
            && ["isOther", "isSecret"]
                .iter()
                .all(|field| question.get(field).is_none_or(Value::is_boolean))
            && question.get("options").is_none_or(|options| {
                options.is_null()
                    || options.as_array().is_some_and(|options| {
                        options.len() <= 128
                            && options.iter().all(|option| {
                                text(option, "label").is_ok() && option["description"].is_string()
                            })
                    })
            })
    })
}

fn public_questions(questions: &Value) -> Value {
    let Some(questions) = questions.as_array() else {
        return Value::Null;
    };
    json!(
        questions
            .iter()
            .map(
                |question| json!({"id":question["id"],"header":question["header"],
        "question":question["question"],"isOther":question["isOther"].as_bool().unwrap_or(false),
        "isSecret":question["isSecret"].as_bool().unwrap_or(false),"options":question["options"]})
            )
            .collect::<Vec<_>>()
    )
}

fn public_details(kind: &str, params: &Value) -> Value {
    let fields: &[&str] = match kind {
        "question" => &["isBlocking"],
        "command" => &["command", "cwd", "reason", "kind", "availableDecisions"],
        "file-change" => &["reason", "grantRoot"],
        _ => &[],
    };
    let mut result = serde_json::Map::new();
    for field in fields {
        if let Some(value) = params.get(*field) {
            result.insert((*field).to_owned(), value.clone());
        }
    }
    Value::Object(result)
}

fn connect_backend(endpoint: &Path, backend: &OwnedBackend) -> Result<Socket> {
    let deadline = Instant::now() + BACKEND_DEADLINE;
    let stream = loop {
        ensure!(
            !backend.tree.root_exited(),
            "unavailable: private Codex backend exited during startup"
        );
        match UnixStream::connect(endpoint) {
            Ok(stream) => break stream,
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::NotFound | ErrorKind::ConnectionRefused
                ) && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(20))
            }
            Err(_) => bail!("unavailable: private Codex backend socket did not become ready"),
        }
    };
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let config = WebSocketConfig::default()
        .read_buffer_size(16 * 1024)
        .write_buffer_size(0)
        .max_write_buffer_size(FRAME_LIMIT + 1024)
        .max_message_size(Some(FRAME_LIMIT))
        .max_frame_size(Some(FRAME_LIMIT));
    let (mut socket, _) = client_with_config("ws://localhost/", stream, Some(config))
        .map_err(|_| anyhow::anyhow!("unavailable: Codex Unix WebSocket upgrade failed"))?;
    send(
        &mut socket,
        &json!({"id":"amux:init","method":"initialize","params":{
        "clientInfo":{"name":"amux","title":"amux","version":env!("CARGO_PKG_VERSION")},
        "capabilities":{"experimentalApi":true}}}),
    )?;
    loop {
        ensure!(
            Instant::now() < deadline,
            "unavailable: Codex initialization timeout"
        );
        match socket.read() {
            Ok(Message::Text(message)) => {
                let reply: Value = serde_json::from_str(&message)
                    .context("unsupported: invalid initialization response")?;
                ensure!(
                    reply["id"] == "amux:init"
                        && reply.get("result").is_some()
                        && reply.get("error").is_none(),
                    "unsupported: Codex initialization rejected"
                );
                break;
            }
            Ok(Message::Ping(_)) => socket.flush()?,
            Err(tungstenite::Error::Io(error))
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            _ => bail!("unavailable: Codex disconnected during initialization"),
        }
    }
    send(&mut socket, &json!({"method":"initialized","params":{}}))?;
    socket
        .get_mut()
        .set_read_timeout(Some(Duration::from_millis(20)))?;
    Ok(socket)
}

fn send(socket: &mut Socket, message: &Value) -> Result<()> {
    socket
        .send(Message::Text(serde_json::to_string(message)?.into()))
        .map_err(|_| anyhow::anyhow!("unavailable: private provider write failed"))
}

fn resolve_codex_executable() -> Result<PathBuf> {
    let path = std::env::var_os("PATH").context("unavailable: daemon PATH is not set")?;
    let daemon_cwd =
        std::env::current_dir().context("unavailable: daemon launch directory is unavailable")?;
    // Resolve relative and empty PATH entries before applying the source cwd.
    // Canonicalize once so all three launches use the same absolute identity.
    for directory in std::env::split_paths(&path) {
        let candidate = daemon_cwd.join(directory).join("codex");
        let Ok(executable) = fs::canonicalize(candidate) else {
            continue;
        };
        let Ok(metadata) = fs::metadata(&executable) else {
            continue;
        };
        if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
            return Ok(executable);
        }
    }
    bail!("unavailable: executable Codex was not found in daemon PATH")
}

fn check_version(executable: &Path, env: &BTreeMap<String, String>) -> Result<()> {
    let output_path = PathBuf::from(&env["CODEX_HOME"]).join("version");
    let file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&output_path)?;
    let mut child = Command::new(executable)
        .arg("--version")
        .envs(env)
        .stdin(Stdio::null())
        .stdout(file)
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .context("unavailable: Codex 0.160.0 is not installed")?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(_) => {
                terminate(&mut child);
                bail!("unavailable: Codex version process observation failed");
            }
        }
        if Instant::now() >= deadline {
            terminate(&mut child);
            bail!("unavailable: Codex version check timeout");
        }
        thread::sleep(Duration::from_millis(10));
    };
    ensure!(
        fs::metadata(&output_path)?.len() <= 256,
        "unsupported: unexpected Codex version output"
    );
    let version = fs::read_to_string(&output_path)?;
    fs::remove_file(output_path)?;
    ensure!(
        status.success() && version.trim() == "codex-cli 0.160.0",
        "unsupported: managed integration requires Codex CLI 0.160.0"
    );
    Ok(())
}

fn terminate(child: &mut Child) {
    // Version-only subprocess; backend descendants use the existing identity-
    // checked ProcessTree above, never a naked reusable process-group number.
    let _ = child.kill();
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn private_dir(path: &Path) -> Result<()> {
    fs::DirBuilder::new().mode(0o700).create(path)?;
    Ok(())
}

fn copy_private_file(source: &Path, destination: &Path, required: bool) -> Result<()> {
    let metadata = match fs::symlink_metadata(source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound && !required => return Ok(()),
        Err(_) => bail!("unavailable: private Codex input file is unavailable"),
    };
    ensure!(
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.len() <= 64 * 1024 * 1024,
        "unsupported: Codex input must be a bounded regular file"
    );
    let mut input = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(source)?;
    let mut output = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(destination)?;
    let copied = std::io::copy(&mut input.by_ref().take(64 * 1024 * 1024 + 1), &mut output)?;
    ensure!(
        copied <= 64 * 1024 * 1024,
        "unsupported: private Codex input grew beyond copy bound"
    );
    Ok(())
}

fn copy_resume(source: &Path, destination: &Path, resume: &str) -> Result<()> {
    let sessions = source.join("sessions");
    let mut queue = vec![(sessions.clone(), 0usize)];
    let mut found = None;
    let mut inspected = 0usize;
    while let Some((directory, depth)) = queue.pop() {
        ensure!(
            depth <= 5 && inspected <= 100_000,
            "unavailable: persisted resume search bound exceeded"
        );
        for entry in
            fs::read_dir(&directory).context("unavailable: persisted resume history unavailable")?
        {
            let entry = entry?;
            inspected += 1;
            ensure!(
                inspected <= 100_000,
                "unavailable: persisted resume search bound exceeded"
            );
            let kind = entry.file_type()?;
            if kind.is_dir() {
                queue.push((entry.path(), depth + 1));
            } else if kind.is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with(&format!("-{resume}.jsonl"))
            {
                ensure!(
                    found.is_none(),
                    "unsupported: ambiguous persisted resume history"
                );
                found = Some(entry.path());
            }
        }
    }
    let found = found.context("unavailable: selected persisted thread not found")?;
    let relative = found.strip_prefix(&sessions)?;
    let target = destination.join("sessions").join(relative);
    let parent = target
        .parent()
        .context("unavailable: invalid resume destination")?;
    fs::create_dir_all(parent)?;
    let mut path = parent;
    while path != destination {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        path = path.parent().context("unavailable: invalid resume path")?;
    }
    copy_private_file(&found, &target, true)
}

fn request_shape(request: &Value, allowed: &[&str], required: &[&str]) -> Result<()> {
    shape(request, allowed, required)?;
    ensure!(
        request["version"] == 1,
        "unsupported-version: API version 1 required"
    );
    ensure!(
        request.get("persistent").is_none_or(Value::is_boolean),
        "invalid-request: persistent must be a boolean"
    );
    Ok(())
}

fn shape(value: &Value, allowed: &[&str], required: &[&str]) -> Result<()> {
    let object = value
        .as_object()
        .context("invalid-request: expected object")?;
    ensure!(
        object.keys().all(|field| allowed.contains(&field.as_str())),
        "invalid-request: unknown field"
    );
    ensure!(
        required.iter().all(|field| object.contains_key(*field)),
        "invalid-request: missing required field"
    );
    Ok(())
}

fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value[field]
        .as_str()
        .filter(|text| !text.is_empty() && !text.contains('\0'))
        .context("invalid-request: expected nonempty string")
}

fn path_text(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .context("unsupported: Codex path is not UTF-8")
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct PrivateRoot(PathBuf);

    impl Drop for PrivateRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn shutdown_waits_for_concurrent_terminal_cleanup_and_owned_resources() {
        let root = PrivateRoot(
            std::env::temp_dir().join(format!("amux-close-{}", Uuid::new_v4().simple())),
        );
        fs::DirBuilder::new().mode(0o700).create(&root.0).unwrap();
        let marker = root.0.join("descendant.pid");
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "/bin/sleep 60 & printf '%s\\n' \"$!\" > \"$1\"; wait",
                "amux-cleanup-fixture",
            ])
            .arg(&marker)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let backend = OwnedBackend::new(command.spawn().unwrap()).unwrap();
        let backend_pid = backend.child.id() as i32;
        let deadline = Instant::now() + Duration::from_secs(3);
        let descendant_pid = loop {
            if let Ok(pid) = fs::read_to_string(&marker)
                && let Ok(pid) = pid.trim().parse::<i32>()
            {
                break pid;
            }
            assert!(Instant::now() < deadline, "owned descendant did not start");
            thread::sleep(Duration::from_millis(5));
        };
        let (release, paused) = mpsc::channel();
        let worker = thread::spawn(move || {
            let _ = paused.recv();
        });
        let (sender, _receiver) = mpsc::sync_channel(1);
        let instance = Arc::new(Instance {
            id: "cleanup-fixture".to_owned(),
            root: root.0.clone(),
            sender,
            stop: AtomicBool::new(false),
            process: Mutex::new(Some(backend)),
            cleanup: Mutex::new(()),
            connection: Mutex::new(None),
            worker: Mutex::new(Some(worker)),
            state: Mutex::new(State {
                terminal: Some("pty".to_owned()),
                session: None,
                provider_session: None,
                joining_session: None,
                expected_resume: None,
                frontend: "running",
                backend: "running",
                connection: "ready",
                session_state: "discovering",
                observation: "waiting-for-first-turn",
                observation_started: None,
                activity: "unknown",
                turn: None,
                turn_outcome: None,
                ended_turns: BTreeMap::new(),
                direct_input: false,
                gap: false,
                problem: None,
                pending: BTreeMap::new(),
                tombstones: BTreeMap::new(),
                ambiguous_requests: BTreeSet::new(),
                calls: VecDeque::new(),
                events: VecDeque::new(),
            }),
        });
        let broker = Arc::new(Broker::new(&root.0).unwrap());
        broker
            .inner
            .instances
            .lock()
            .insert(instance.id.clone(), instance.clone());
        let ending = {
            let broker = broker.clone();
            thread::spawn(move || broker.end_terminal("pty"))
        };
        let deadline = Instant::now() + Duration::from_secs(3);
        while !instance.stop.load(Ordering::Acquire) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        let cleanup_started = instance.stop.load(Ordering::Acquire);
        let (attempted, attempt) = mpsc::channel();
        let (finished, finish) = mpsc::channel();
        let shutdown = {
            let broker = broker.clone();
            thread::spawn(move || {
                attempted.send(()).unwrap();
                broker.shutdown();
                finished.send(()).unwrap();
            })
        };
        let shutdown_started = attempt.recv_timeout(Duration::from_secs(3)).is_ok();
        let premature = finish.recv_timeout(Duration::from_millis(250)).is_ok();
        release.send(()).unwrap();
        ending.join().unwrap();
        shutdown.join().unwrap();
        let resources_removed = !root.0.exists();
        for pid in [backend_pid, descendant_pid] {
            assert_eq!(
                unsafe { libc::kill(pid, 0) },
                -1,
                "owned PID {pid} survived"
            );
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH)
            );
        }
        assert!(cleanup_started && shutdown_started);
        assert!(resources_removed, "private root survived completed cleanup");
        assert!(
            !premature,
            "shutdown returned while terminal cleanup was paused"
        );
    }
}

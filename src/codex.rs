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
use std::ffi::{CStr, CString};
use std::fs;
use std::io::{BufRead, BufReader, ErrorKind, Read, Seek};
use std::net::Shutdown;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt, symlink};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};
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

/// Transparent diagnostic wrapper: settlement evidence is not a public error code.
struct PrepareFailure {
    error: anyhow::Error,
    settled: bool,
}

impl std::fmt::Debug for PrepareFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.error, formatter)
    }
}

impl std::fmt::Display for PrepareFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.error, formatter)
    }
}

impl std::error::Error for PrepareFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.error.source()
    }
}

fn prepare_failure(error: anyhow::Error, settled: bool) -> anyhow::Error {
    PrepareFailure { error, settled }.into()
}

/// Absence of evidence, including an unrecognized error, is never settlement.
pub(crate) fn prepare_failure_settled(error: &anyhow::Error) -> bool {
    error
        .chain()
        .find_map(|cause| {
            cause
                .downcast_ref::<PrepareFailure>()
                .map(|failure| failure.settled)
        })
        .unwrap_or(false)
}

fn remove_owned_root(root: &Path) -> bool {
    match fs::remove_dir_all(root) {
        Ok(()) => true,
        Err(error) => error.kind() == ErrorKind::NotFound,
    }
}

pub(crate) struct Broker {
    inner: Arc<Inner>,
}

struct Inner {
    runtime: PathBuf,
    recovery_store: Option<PathBuf>,
    instances: Mutex<BTreeMap<String, Arc<Instance>>>,
    preparing: Mutex<()>,
    stopped: AtomicBool,
}

impl Inner {
    fn remove_settled(&self, instance: &Arc<Instance>) {
        if *instance.cleanup.lock() != Some(true) {
            return;
        }
        let mut instances = self.instances.lock();
        if instances
            .get(&instance.id)
            .is_some_and(|retained| Arc::ptr_eq(retained, instance))
        {
            instances.remove(&instance.id);
        }
    }
}

/// An unbound lease closes only its own instance; uncertain cleanup stays charged.
/// After bind, the Broker owns the execution, not this stack value.
pub(crate) struct Prepared {
    pub instance_id: String,
    pub command: String,
    pub env: BTreeMap<String, String>,
    broker: Weak<Inner>,
    instance: Arc<Instance>,
}

struct Instance {
    id: String,
    root: PathBuf,
    model: String,
    recovery_store: Option<PathBuf>,
    checkpoint: Mutex<Option<Checkpoint>>,
    state: Mutex<State>,
    sender: mpsc::SyncSender<Job>,
    stop: AtomicBool,
    cleanup: Mutex<Option<bool>>,
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
            return Err(prepare_failure(
                anyhow::anyhow!("unavailable: cannot capture private backend process identity"),
                false,
            ));
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
    // Replacement is irreversible for this instance, independent of later diagnostics.
    native_session_replaced: bool,
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

/// Only a file observation, never transcript content. Unchanged heartbeats do
/// not reread or copy the native conversation.
struct Checkpoint {
    path: PathBuf,
    len: u64,
    modified: SystemTime,
    device: u64,
    inode: u64,
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
    pub fn new(runtime: &Path, recovery_store: Option<&Path>) -> Result<Self> {
        if let Some(store) = recovery_store {
            ensure!(
                store.is_absolute(),
                "invalid-request: Codex conversation store must be absolute"
            );
        }
        Ok(Self {
            inner: Arc::new(Inner {
                runtime: runtime.to_owned(),
                recovery_store: recovery_store.map(Path::to_owned),
                instances: Mutex::new(BTreeMap::new()),
                preparing: Mutex::new(()),
                stopped: AtomicBool::new(false),
            }),
        })
    }

    pub fn prepare(&self, request: &Value) -> Result<Prepared> {
        self.prepare_unstarted(request).map_err(|error| {
            if error.downcast_ref::<PrepareFailure>().is_some() {
                error
            } else {
                // Every post-spawn failure is classified at its ownership boundary.
                prepare_failure(error, true)
            }
        })
    }

    fn prepare_unstarted(&self, request: &Value) -> Result<Prepared> {
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
        // Keep admission bounded even when a rejected spawn cannot be cleaned
        // conclusively and must retain its broker slot. This is not a map,
        // topology, authority or PTY guard; existing instances remain independent.
        let _preparing = self.inner.preparing.lock();
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
        if result.as_ref().err().is_some_and(|error| {
            error.downcast_ref::<PrepareFailure>().is_none() || prepare_failure_settled(error)
        }) {
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
        // Only the provider's native rollout directory outlives this private
        // backend. Auth, config, caches and transport state remain ephemeral.
        let recovery_store = self
            .inner
            .recovery_store
            .as_deref()
            .map(prepare_recovery_store)
            .transpose()?;
        if let Some(store) = recovery_store.as_ref() {
            let owned = fs::canonicalize(root)?;
            ensure!(
                !store.starts_with(&owned) && !owned.starts_with(store),
                "invalid-request: Codex conversation store overlaps owned backend cleanup"
            );
            symlink(store.join("sessions"), root.join("codex/sessions"))?;
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
        }
        if let Some(resume) = resume {
            let destination = recovery_store
                .as_deref()
                .map_or_else(|| root.join("codex"), Path::to_owned);
            if find_resume(&destination, resume)?.is_none() {
                let source = source_home
                    .as_deref()
                    .context("unavailable: persisted resume source is unavailable")?;
                copy_resume(source, &destination, resume)?;
            }
            // An explicit resume must never fall through to a fresh thread.
            let saved = find_resume(&destination, resume)?
                .context("unavailable: selected persisted thread not found")?;
            checkpoint_rollout(&saved, resume, &destination)?;
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
                libc::umask(0o077);
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        if let Some(cwd) = request.get("cwd").and_then(Value::as_str) {
            command.current_dir(cwd);
        }
        ensure!(
            !self.inner.stopped.load(Ordering::Acquire),
            "target-ended: broker stopped before native backend startup"
        );
        let child = command
            .spawn()
            .context("unavailable: cannot start private Codex backend")?;
        let (backend, ownership_error) = match OwnedBackend::new(child) {
            Ok(backend) => (Some(backend), None),
            Err(error) => (None, Some(error)),
        };
        let (sender, receiver) = mpsc::sync_channel(32);
        let instance = Arc::new(Instance {
            id: id.to_owned(),
            root: root.to_owned(),
            model: model.to_owned(),
            recovery_store,
            checkpoint: Mutex::new(None),
            sender,
            stop: AtomicBool::new(false),
            cleanup: Mutex::new(None),
            process: Mutex::new(backend),
            connection: Mutex::new(None),
            worker: Mutex::new(None),
            state: Mutex::new(State {
                terminal: None,
                session: None,
                provider_session: None,
                joining_session: None,
                expected_resume: resume.map(str::to_owned),
                native_session_replaced: false,
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
        // Retain every spawned backend, including an uncaptured identity's
        // uncertain settlement. Shutdown waits for setup or its cleanup.
        let cleanup = instance.cleanup.lock();
        let cancelled = {
            let mut instances = self.inner.instances.lock();
            let cancelled =
                self.inner.stopped.load(Ordering::Acquire) || instances.len() >= INSTANCE_LIMIT;
            // Even an admission race cannot discard an already spawned backend.
            instances.insert(id.to_owned(), instance.clone());
            cancelled
        };
        if let Some(error) = ownership_error {
            drop(cleanup);
            instance.close("native-start-failed");
            self.inner.remove_settled(&instance);
            return Err(error);
        }
        if cancelled {
            drop(cleanup);
            let settled = instance.close("launch-cancelled");
            self.inner.remove_settled(&instance);
            return Err(prepare_failure(
                anyhow::anyhow!("target-ended: launch cancelled or instance limit reached"),
                settled,
            ));
        }
        let connected = (|| -> Result<_> {
            let process = instance.process.lock();
            let backend = process
                .as_ref()
                .context("unavailable: prepared backend already failed")?;
            let socket = connect_backend(&endpoint, backend, &self.inner.stopped)?;
            let control_socket = socket.get_ref().try_clone()?;
            Ok((socket, control_socket))
        })();
        let (socket, control_socket) = match connected {
            Ok(connected) => connected,
            Err(error) => {
                drop(cleanup);
                let settled = instance.close("native-start-failed");
                self.inner.remove_settled(&instance);
                return Err(prepare_failure(error, settled));
            }
        };
        *instance.connection.lock() = Some(control_socket);
        let worker_instance = instance.clone();
        match thread::Builder::new()
            .name(format!("codex-{}", &id[..8]))
            .spawn(move || run(worker_instance, socket, receiver))
        {
            Ok(worker) => *instance.worker.lock() = Some(worker),
            Err(error) => {
                drop(cleanup);
                let settled = instance.close("worker-start-failed");
                self.inner.remove_settled(&instance);
                return Err(prepare_failure(error.into(), settled));
            }
        }
        drop(cleanup);
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
            instance,
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

    /// Saved provider-native thread identity, not the separate transport session.
    /// The caller must not hold topology/authority locks across this method.
    pub fn recovery(&self, terminal: &str) -> Result<Option<Value>> {
        let Some(instance) = self.bound(terminal).into_iter().next() else {
            return Ok(None);
        };
        let session = {
            let state = instance.state.lock();
            if state.native_session_replaced {
                return Ok(Some(json!({"agent":"codex","recoverable":false,
                    "reason":"native-session-replaced","root_lifetime":instance.id})));
            }
            if instance.recovery_store.is_none() {
                return Ok(None);
            }
            if state.frontend != "attached" || state.session_state != "ready" {
                return Ok(None);
            }
            let Some(session) = state.session.clone() else {
                return Ok(None);
            };
            session
        };
        let peer_pid = instance
            .process
            .lock()
            .as_ref()
            .map(|backend| backend.child.id());
        let checkpoint = if peer_pid.is_some() {
            instance.checkpoint(&session)
        } else {
            Ok(None)
        };
        // Backing-store I/O does not hold State. Do not publish old intent if
        // the native replacement was observed while its checkpoint was running.
        let state = instance.state.lock();
        if state.native_session_replaced {
            return Ok(Some(json!({"agent":"codex","recoverable":false,
                "reason":"native-session-replaced","root_lifetime":instance.id})));
        }
        let Some(peer_pid) = peer_pid else {
            return Ok(None);
        };
        let Some(path) = checkpoint? else {
            return Ok(None);
        };
        Ok(Some(
            json!({"agent":"codex","recoverable":true,"root_lifetime":instance.id,
            "session_id":session,"model":instance.model,"session_file":path,"peer_pid":peer_pid}),
        ))
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
        // A preparing child may not be in the map yet. Join that ownership
        // scope before taking the final cleanup snapshot.
        let _preparing = self.inner.preparing.lock();
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

impl Prepared {
    /// Roll back this exact preparation after native terminal creation fails.
    /// A missing broker/map entry is not evidence; the retained owned instance is.
    pub(crate) fn cleanup_failed_launch(&self) -> bool {
        let settled = self.instance.close("native-start-failed");
        if let Some(broker) = self.broker.upgrade() {
            broker.remove_settled(&self.instance);
        }
        settled
    }
}

impl Drop for Prepared {
    fn drop(&mut self) {
        let broker = self.broker.upgrade();
        // Match bind's map -> State order, then revoke admission before releasing
        // either guard. Cleanup I/O and worker joins never retain the map lock.
        let instances = broker.as_ref().map(|broker| broker.instances.lock());
        let unbound = {
            let state = self.instance.state.lock();
            if state.terminal.is_none() {
                self.instance.stop.store(true, Ordering::Release);
                true
            } else {
                false
            }
        };
        drop(instances);
        if unbound {
            self.instance.close("unbound-launch-dropped");
            if let Some(broker) = broker {
                broker.remove_settled(&self.instance);
            }
        }
    }
}

impl Instance {
    fn checkpoint(&self, session: &str) -> Result<Option<PathBuf>> {
        let Some(store) = self.recovery_store.as_ref() else {
            return Ok(None);
        };
        // This dedicated I/O lock never nests a state/authority guard.
        let mut checkpoint = self.checkpoint.lock();
        let path = match checkpoint.as_ref() {
            Some(saved) => saved.path.clone(),
            None => match find_resume(store, session)? {
                Some(path) => path,
                None => return Ok(None), // Allocated IDs may not yet have a rollout.
            },
        };
        let metadata = fs::symlink_metadata(&path)
            .context("unavailable: saved Codex conversation disappeared")?;
        let modified = metadata.modified()?;
        if checkpoint.as_ref().is_none_or(|saved| {
            saved.len != metadata.len()
                || saved.modified != modified
                || saved.device != metadata.dev()
                || saved.inode != metadata.ino()
        }) {
            checkpoint_rollout(&path, session, store)?;
            *checkpoint = Some(Checkpoint {
                path: path.clone(),
                len: metadata.len(),
                modified,
                device: metadata.dev(),
                inode: metadata.ino(),
            });
        }
        Ok(Some(path))
    }

    fn close(&self, reason: &'static str) -> bool {
        // Stop revokes work; concurrent callers read the same completed teardown
        // result, never infer success from emptied handles or a missing map entry.
        let mut cleanup = self.cleanup.lock();
        if let Some(settled) = *cleanup {
            return settled;
        }
        self.stop.store(true, Ordering::Release);
        {
            let mut state = self.state.lock();
            state.degrade(reason);
        }
        if let Some(socket) = self.connection.lock().take() {
            let _ = socket.shutdown(Shutdown::Both);
        }
        let joined = self
            .worker
            .lock()
            .take()
            .is_none_or(|worker| worker.join().is_ok());
        let backend = self.process.lock().take();
        let cleaned = backend.is_some_and(|mut backend| {
            if backend.terminate() {
                true
            } else {
                // Keep exact owned process identities as well as the budget
                // charge when termination cannot be conclusively established.
                *self.process.lock() = Some(backend);
                false
            }
        });
        let session = {
            let state = self.state.lock();
            if state.native_session_replaced {
                None
            } else {
                state.session.clone()
            }
        };
        let checkpoint_failed = session
            .as_deref()
            .is_some_and(|session| self.checkpoint(session).is_err());
        {
            let mut state = self.state.lock();
            state.backend = if cleaned { "exited" } else { "unknown" };
            if !cleaned {
                state.problem = Some("owned-process-cleanup-incomplete");
            }
            if checkpoint_failed {
                state.problem = Some("native-conversation-checkpoint-failed");
                state.emit(
                    "agent-lifecycle",
                    json!({"reason":"native-conversation-checkpoint-failed"}),
                );
            }
            let backend = state.backend;
            state.emit(
                "agent-lifecycle",
                json!({"instance_id":self.id,"backend":backend,"reason":reason}),
            );
        }
        let resources_removed = cleaned && joined && remove_owned_root(&self.root);
        if !resources_removed {
            let mut state = self.state.lock();
            state.problem = Some("owned-resource-cleanup-incomplete");
            state.emit(
                "agent-lifecycle",
                json!({"reason":"owned-resource-cleanup-incomplete"}),
            );
        }
        let settled = cleaned && joined && resources_removed;
        *cleanup = Some(settled);
        settled
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
            state.native_session_replaced = true;
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

fn connect_backend(
    endpoint: &Path,
    backend: &OwnedBackend,
    stopped: &AtomicBool,
) -> Result<Socket> {
    let deadline = Instant::now() + BACKEND_DEADLINE;
    let stream = loop {
        ensure!(
            !stopped.load(Ordering::Acquire),
            "target-ended: broker stopped during native backend startup"
        );
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
            !stopped.load(Ordering::Acquire),
            "target-ended: broker stopped during native backend initialization"
        );
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

fn ensure_store_directory(path: &Path) -> Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(error).context("unavailable: cannot create Codex conversation store");
        }
    }
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o077 == 0,
        "unavailable: Codex conversation store must be a private owned directory"
    );
    Ok(())
}

fn prepare_recovery_store(store: &Path) -> Result<PathBuf> {
    ensure_store_directory(store)?;
    let store = fs::canonicalize(store)?;
    ensure_store_directory(&store.join("sessions"))?;
    fs::File::open(store.join("sessions"))?.sync_all()?;
    fs::File::open(&store)?.sync_all()?;
    fs::File::open(
        store
            .parent()
            .context("unavailable: Codex conversation store has no parent")?,
    )?
    .sync_all()?;
    Ok(store)
}

/// Validate only the bounded native header; transcript content stays in the
/// provider store and is never returned as journal metadata.
fn rollout_file(path: &Path, session: &str) -> Result<fs::File> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .context("unavailable: saved Codex conversation is unavailable")?;
    validate_rollout(&mut file, session)?;
    Ok(file)
}

fn validate_rollout(file: &mut fs::File, session: &str) -> Result<()> {
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.nlink() == 1
            && metadata.len() <= 64 * 1024 * 1024,
        "unsupported: saved Codex conversation must be a bounded owned regular file"
    );
    let mut header = Vec::new();
    BufReader::new(&mut *file)
        .take(64 * 1024 + 1)
        .read_until(b'\n', &mut header)?;
    ensure!(
        header.len() <= 64 * 1024 && header.last() == Some(&b'\n'),
        "unsupported: saved Codex conversation header is incomplete or oversized"
    );
    let header: Value = serde_json::from_slice(&header)
        .context("unsupported: saved Codex conversation header is invalid")?;
    ensure!(
        header["type"] == "session_meta" && header["payload"]["id"].as_str() == Some(session),
        "stale-target: saved Codex conversation header names another thread"
    );
    file.rewind()?;
    Ok(())
}

fn checkpoint_rollout(path: &Path, session: &str, store: &Path) -> Result<()> {
    ensure!(
        path.starts_with(store.join("sessions")),
        "unsupported: Codex conversation is outside its native store"
    );
    let file = rollout_file(path, session)?;
    file.sync_all()?;
    let mut directory = path
        .parent()
        .context("unavailable: saved Codex conversation has no parent")?;
    loop {
        let metadata = fs::symlink_metadata(directory)?;
        ensure!(
            metadata.is_dir()
                && !metadata.file_type().is_symlink()
                && metadata.uid() == unsafe { libc::geteuid() },
            "unsupported: saved Codex conversation directory is not owned"
        );
        fs::File::open(directory)?.sync_all()?;
        if directory == store {
            break;
        }
        directory = directory
            .parent()
            .context("unsupported: invalid Codex conversation path")?;
    }
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

fn find_resume(source: &Path, resume: &str) -> Result<Option<PathBuf>> {
    ensure!(
        Uuid::parse_str(resume).is_ok(),
        "unsupported: saved Codex thread identity is not a UUID"
    );
    let sessions = source.join("sessions");
    match fs::symlink_metadata(&sessions) {
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).context("unavailable: persisted resume history unavailable");
        }
    }
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
    Ok(found)
}

fn import_directory(parent: &fs::File, name: &CStr, private: bool) -> Result<fs::File> {
    let open = || unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    let mut descriptor = open();
    if descriptor < 0 && std::io::Error::last_os_error().kind() == ErrorKind::NotFound {
        if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0
            && std::io::Error::last_os_error().kind() != ErrorKind::AlreadyExists
        {
            return Err(std::io::Error::last_os_error())
                .context("unavailable: cannot create native import directory");
        }
        descriptor = open();
    }
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error())
            .context("unsupported: native import directory is unavailable or aliased");
    }
    let directory = unsafe { fs::File::from_raw_fd(descriptor) };
    let metadata = directory.metadata()?;
    ensure!(
        metadata.is_dir()
            && metadata.uid() == unsafe { libc::geteuid() }
            && (!private || metadata.mode() & 0o077 == 0),
        "unsupported: native import directory must be owned and its store root private"
    );
    Ok(directory)
}

fn publish_rollout(directory: &fs::File, temporary: &CStr, target: &CStr) -> Result<()> {
    // Exclusive rename publishes one complete inode without a hard-link window:
    // a crash cannot leave a discoverable rollout with a second staging link.
    #[cfg(target_os = "macos")]
    let result = unsafe {
        libc::renameatx_np(
            directory.as_raw_fd(),
            temporary.as_ptr(),
            directory.as_raw_fd(),
            target.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::renameat2(
            directory.as_raw_fd(),
            temporary.as_ptr(),
            directory.as_raw_fd(),
            target.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        if result != 0 {
            return Err(std::io::Error::last_os_error())
                .context("unavailable: cannot exclusively publish native conversation");
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (directory, temporary, target);
        bail!("unsupported: atomic native import publication requires macOS or Linux")
    }
}

fn copy_resume(source: &Path, destination: &Path, resume: &str) -> Result<()> {
    let found =
        find_resume(source, resume)?.context("unavailable: selected persisted thread not found")?;
    let mut input = rollout_file(&found, resume)?;
    let source_len = input.metadata()?.len();
    let sessions = source.join("sessions");
    let relative = found.strip_prefix(&sessions)?;
    let parent = relative
        .parent()
        .context("unavailable: invalid resume destination")?;
    let target = CString::new(
        relative
            .file_name()
            .context("unavailable: invalid native conversation name")?
            .as_encoded_bytes(),
    )?;
    let root = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(destination)?;
    let metadata = root.metadata()?;
    ensure!(
        metadata.is_dir()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o077 == 0,
        "unsupported: native import store must be a private owned directory"
    );
    // Root, sessions, and at most five levels admitted by find_resume.
    let mut directories = Vec::with_capacity(7);
    directories.push(root);
    directories.push(import_directory(&directories[0], c"sessions", true)?);
    for component in parent.components() {
        let Component::Normal(name) = component else {
            bail!("unsupported: invalid native import directory component");
        };
        let name = CString::new(name.as_encoded_bytes())?;
        directories.push(import_directory(directories.last().unwrap(), &name, false)?);
    }
    let directory = directories.last().unwrap();
    // This name cannot match find_resume's exact UUID/.jsonl discovery suffix.
    let temporary = CString::new(format!(".amux-import-{}.tmp", Uuid::new_v4()))?;
    let mut created = false;
    let mut published = false;
    let result = (|| -> Result<()> {
        let descriptor = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                temporary.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if descriptor < 0 {
            return Err(std::io::Error::last_os_error())
                .context("unavailable: cannot create private native import");
        }
        created = true;
        let mut output = unsafe { fs::File::from_raw_fd(descriptor) };
        let copied = std::io::copy(&mut input.by_ref().take(64 * 1024 * 1024 + 1), &mut output)?;
        ensure!(
            copied <= 64 * 1024 * 1024
                && copied == source_len
                && input.metadata()?.len() == source_len,
            "unavailable: native conversation changed or exceeded the import bound"
        );
        // Publish the copied identity, not merely the source's earlier header.
        output.rewind()?;
        validate_rollout(&mut output, resume)?;
        output.sync_all()?;
        publish_rollout(directory, &temporary, &target)?;
        published = true;
        for ancestor in directories.iter().rev() {
            ancestor
                .sync_all()
                .context("unavailable: published native conversation durability is unconfirmed")?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        if created && !published {
            if unsafe { libc::unlinkat(directory.as_raw_fd(), temporary.as_ptr(), 0) } != 0 {
                return Err(error).context(format!(
                    "unavailable: cannot remove owned partial native import: {}",
                    std::io::Error::last_os_error()
                ));
            }
            if let Err(cleanup) = directory.sync_all() {
                return Err(error).context(format!(
                    "unavailable: partial native import cleanup durability is unconfirmed: {cleanup}"
                ));
            }
        }
        return Err(error);
    }
    Ok(())
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
    use std::os::unix::fs::PermissionsExt;

    struct PrivateRoot(PathBuf);

    impl Drop for PrivateRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn rejected_preparation_is_settled_without_starting_a_backend() {
        let root = PrivateRoot(
            std::env::temp_dir().join(format!("amux-prepare-rejected-{}", Uuid::new_v4().simple())),
        );
        let broker = Broker::new(&root.0, None).unwrap();
        let request = json!({"version":1,"op":"agent-open",
            "controller":{"kind":"controller","ref":"fixture"},"model":"fixture",
            "resume_thread":"not-a-persisted-thread"});
        let error = broker.prepare(&request).err().unwrap();
        assert!(prepare_failure_settled(&error));
        assert_eq!(
            error.to_string(),
            "invalid-request: resume_thread must be a persisted Codex UUID"
        );
        assert!(
            !root.0.exists(),
            "validation unexpectedly reached backend setup"
        );
        assert!(prepare_failure_settled(&error.context("consumer boundary")));
        assert!(!prepare_failure_settled(&anyhow::anyhow!(
            "unavailable: unknown launch"
        )));
    }

    #[test]
    fn filesystem_preparation_failure_retains_its_cause_and_is_retryable() {
        let root = PrivateRoot(std::env::temp_dir().join(format!(
            "amux-prepare-filesystem-{}",
            Uuid::new_v4().simple()
        )));
        private_dir(&root.0).unwrap();
        let runtime = root.0.join("runtime");
        fs::write(&runtime, "not a directory").unwrap();
        let broker = Broker::new(&runtime, None).unwrap();
        let request = json!({"version":1,"op":"agent-open",
            "controller":{"kind":"controller","ref":"fixture"},"model":"fixture"});
        let original = fs::create_dir_all(&runtime).unwrap_err();
        let error = broker.prepare(&request).err().unwrap();
        assert!(prepare_failure_settled(&error));
        assert_eq!(format!("{error:#}"), original.to_string());
        assert_eq!(error.root_cause().to_string(), original.to_string());
        assert_eq!(fs::read_to_string(&runtime).unwrap(), "not a directory");
    }

    #[test]
    fn uncaptured_backend_identity_never_supplies_settlement_evidence() {
        // Deliberately not a new session: killing/reaping this child cannot prove
        // that an unobserved backend descendant or endpoint has been settled.
        let child = Command::new("/bin/sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id() as i32;
        let error = OwnedBackend::new(child).err().unwrap();
        assert_eq!(
            error.to_string(),
            "unavailable: cannot capture private backend process identity"
        );
        assert!(!prepare_failure_settled(
            &error.context("consumer boundary")
        ));
        assert_eq!(
            unsafe { libc::kill(pid, 0) },
            -1,
            "direct child was not reaped"
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }

    #[test]
    fn native_store_rejects_aliases_and_ambiguous_exact_threads() {
        let root = PrivateRoot(
            std::env::temp_dir().join(format!("amux-native-alias-{}", Uuid::new_v4().simple())),
        );
        private_dir(&root.0).unwrap();
        let outside = root.0.join("outside");
        private_dir(&outside).unwrap();
        let store = root.0.join("store");
        symlink(&outside, &store).unwrap();
        assert!(prepare_recovery_store(&store).is_err());
        fs::remove_file(&store).unwrap();
        let store = prepare_recovery_store(&store).unwrap();
        let session = Uuid::new_v4().to_string();
        let first = store
            .join("sessions")
            .join(format!("rollout-first-{session}.jsonl"));
        symlink(outside.join("never-created"), &first).unwrap();
        assert!(rollout_file(&first, &session).is_err());
        assert_eq!(find_resume(&store, &session).unwrap(), None);
        fs::remove_file(&first).unwrap();
        let data = format!(
            "{}\n",
            json!({"type":"session_meta","payload":{"id":session}})
        );
        fs::write(&first, &data).unwrap();
        let second = store
            .join("sessions")
            .join(format!("rollout-second-{session}.jsonl"));
        fs::write(&second, data).unwrap();
        assert!(find_resume(&store, &session).is_err());
        assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
    }

    struct NativeImport {
        root: PrivateRoot,
        source: PathBuf,
        store: PathBuf,
        session: String,
        relative: PathBuf,
        history: Vec<u8>,
    }

    impl NativeImport {
        fn new() -> Self {
            let root = PrivateRoot(
                std::env::temp_dir()
                    .join(format!("amux-native-import-{}", Uuid::new_v4().simple())),
            );
            private_dir(&root.0).unwrap();
            let source = root.0.join("source");
            let session = Uuid::new_v4().to_string();
            let relative = PathBuf::from("2026/10/29")
                .join(format!("rollout-2026-10-29T00-00-00-{session}.jsonl"));
            let mut history = format!(
                "{}\n",
                json!({"type":"session_meta","payload":{"id":session,"cwd":"/original"}})
            )
            .into_bytes();
            for turn in 0..512 {
                history.extend_from_slice(
                    format!(
                        "{}\n",
                        json!({"type":"response_item","payload":{"turn":turn,"text":"history".repeat(128)}})
                    )
                    .as_bytes(),
                );
            }
            let source_file = source.join("sessions").join(&relative);
            fs::create_dir_all(source_file.parent().unwrap()).unwrap();
            fs::write(source_file, &history).unwrap();
            let store = prepare_recovery_store(&root.0.join("store")).unwrap();
            Self {
                root,
                source,
                store,
                session,
                relative,
                history,
            }
        }

        fn target(&self) -> PathBuf {
            self.store.join("sessions").join(&self.relative)
        }
    }

    #[test]
    fn native_import_preserves_whole_history_and_provider_directory_modes() {
        let fixture = NativeImport::new();
        let parent = fixture.target().parent().unwrap().to_owned();
        fs::create_dir_all(&parent).unwrap();
        for directory in [parent.clone(), parent.parent().unwrap().to_owned()] {
            fs::set_permissions(directory, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let source = fixture.source.join("sessions").join(&fixture.relative);
        let mut validated = rollout_file(&source, &fixture.session).unwrap();
        let mut whole_history = Vec::new();
        validated.read_to_end(&mut whole_history).unwrap();
        assert_eq!(whole_history, fixture.history);
        copy_resume(&fixture.source, &fixture.store, &fixture.session).unwrap();
        assert_eq!(fs::read(fixture.target()).unwrap(), fixture.history);
        assert_eq!(fs::read(&source).unwrap(), fixture.history);
        for directory in [parent.clone(), parent.parent().unwrap().to_owned()] {
            assert_eq!(fs::metadata(directory).unwrap().mode() & 0o777, 0o755);
        }
        let metadata = fs::metadata(fixture.target()).unwrap();
        assert_eq!(metadata.mode() & 0o777, 0o600);
        assert_eq!(metadata.nlink(), 1);
        assert_eq!(fs::read_dir(parent).unwrap().count(), 1);
    }

    #[test]
    fn native_import_never_clobbers_an_existing_target() {
        let fixture = NativeImport::new();
        let target = fixture.target();
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        let existing = format!(
            "{}\n{}\n",
            json!({"type":"session_meta","payload":{"id":fixture.session}}),
            json!({"type":"response_item","payload":{"text":"existing complete conversation"}})
        );
        fs::write(&target, &existing).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        let original = fs::metadata(&target).unwrap();
        assert!(copy_resume(&fixture.source, &fixture.store, &fixture.session).is_err());
        let retained = fs::metadata(&target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), existing.as_bytes());
        assert_eq!(retained.ino(), original.ino());
        assert_eq!(retained.mode(), original.mode());
        assert_eq!(fs::read_dir(target.parent().unwrap()).unwrap().count(), 1);
        assert_eq!(
            fs::read(fixture.source.join("sessions").join(&fixture.relative)).unwrap(),
            fixture.history
        );
    }

    fn tree_contents(path: &Path) -> Vec<(PathBuf, u32, Vec<u8>)> {
        let mut queue = vec![path.to_owned()];
        let mut contents = Vec::new();
        while let Some(entry) = queue.pop() {
            let metadata = fs::symlink_metadata(&entry).unwrap();
            let bytes = if metadata.is_dir() {
                queue.extend(
                    fs::read_dir(&entry)
                        .unwrap()
                        .map(|child| child.unwrap().path()),
                );
                Vec::new()
            } else {
                fs::read(&entry).unwrap()
            };
            contents.push((
                entry.strip_prefix(path).unwrap().to_owned(),
                metadata.mode(),
                bytes,
            ));
        }
        contents.sort();
        contents
    }

    #[test]
    fn native_import_rejects_nested_aliases_before_outside_tree_mutation() {
        for alias_depth in 0..3 {
            let fixture = NativeImport::new();
            let outside = fixture.root.0.join("outside");
            private_dir(&outside).unwrap();
            fs::set_permissions(&outside, fs::Permissions::from_mode(0o751)).unwrap();
            let dates = ["2026", "10", "29"];
            let mut outside_parent = outside.clone();
            for date in &dates[alias_depth + 1..] {
                outside_parent.push(date);
                fs::create_dir(&outside_parent).unwrap();
                fs::set_permissions(&outside_parent, fs::Permissions::from_mode(0o755)).unwrap();
            }
            fs::write(outside_parent.join("sentinel"), "unrelated outside history").unwrap();
            fs::set_permissions(
                outside_parent.join("sentinel"),
                fs::Permissions::from_mode(0o640),
            )
            .unwrap();
            let before = tree_contents(&outside);
            let mut destination = fixture.store.join("sessions");
            for date in &dates[..alias_depth] {
                destination.push(date);
                fs::create_dir(&destination).unwrap();
                fs::set_permissions(&destination, fs::Permissions::from_mode(0o755)).unwrap();
            }
            let alias = destination.join(dates[alias_depth]);
            symlink(&outside, &alias).unwrap();
            assert!(copy_resume(&fixture.source, &fixture.store, &fixture.session).is_err());
            assert_eq!(tree_contents(&outside), before, "alias depth {alias_depth}");
            assert!(
                fs::symlink_metadata(alias)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert_eq!(find_resume(&fixture.store, &fixture.session).unwrap(), None);
            assert_eq!(
                fs::read(fixture.source.join("sessions").join(&fixture.relative)).unwrap(),
                fixture.history
            );
        }
    }

    #[test]
    fn native_import_rejects_non_directory_destination_without_mutation() {
        let fixture = NativeImport::new();
        let unexpected = fixture.store.join("sessions/2026");
        fs::write(&unexpected, "provider-created non-directory").unwrap();
        fs::set_permissions(&unexpected, fs::Permissions::from_mode(0o640)).unwrap();
        let before = tree_contents(&fixture.store);
        assert!(copy_resume(&fixture.source, &fixture.store, &fixture.session).is_err());
        assert_eq!(tree_contents(&fixture.store), before);
    }

    #[test]
    fn native_import_write_failure_is_retryable_without_partial_history() {
        const CHILD_ROOT: &str = "AMUX_NATIVE_IMPORT_LIMIT_CHILD_ROOT";
        const CHILD_SESSION: &str = "AMUX_NATIVE_IMPORT_LIMIT_CHILD_SESSION";
        if let Some(root) = std::env::var_os(CHILD_ROOT) {
            let root = PathBuf::from(root);
            let session = std::env::var(CHILD_SESSION).unwrap();
            let source = root.join("source");
            let store = root.join("store");
            let original = find_resume(&source, &session).unwrap().unwrap();
            let history = fs::read(&original).unwrap();
            assert!(history.len() > 8192);
            let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
            assert_eq!(
                unsafe { libc::getrlimit(libc::RLIMIT_FSIZE, limit.as_mut_ptr()) },
                0
            );
            let limit = unsafe { limit.assume_init() };
            let restricted = libc::rlimit {
                rlim_cur: 8192,
                rlim_max: limit.rlim_max,
            };
            let signal = unsafe { libc::signal(libc::SIGXFSZ, libc::SIG_IGN) };
            assert_ne!(signal, libc::SIG_ERR);
            assert_eq!(
                unsafe { libc::setrlimit(libc::RLIMIT_FSIZE, &restricted) },
                0
            );
            let result = copy_resume(&source, &store, &session);
            assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_FSIZE, &limit) }, 0);
            assert_ne!(
                unsafe { libc::signal(libc::SIGXFSZ, signal) },
                libc::SIG_ERR
            );
            let error = result.unwrap_err();
            assert_eq!(
                error
                    .chain()
                    .find_map(|cause| cause.downcast_ref::<std::io::Error>())
                    .and_then(std::io::Error::raw_os_error),
                Some(libc::EFBIG),
                "{error:#}"
            );
            assert_eq!(find_resume(&store, &session).unwrap(), None);
            assert_eq!(
                fs::read_dir(store.join("sessions/2026/10/29"))
                    .unwrap()
                    .count(),
                0
            );
            assert_eq!(fs::read(&original).unwrap(), history);
            copy_resume(&source, &store, &session).unwrap();
            let imported = find_resume(&store, &session).unwrap().unwrap();
            assert_eq!(fs::read(&imported).unwrap(), history);
            assert_eq!(fs::metadata(&imported).unwrap().nlink(), 1);
            assert_eq!(fs::read_dir(imported.parent().unwrap()).unwrap().count(), 1);
            return;
        }

        struct ImportChild(Child);
        impl Drop for ImportChild {
            fn drop(&mut self) {
                if self.0.try_wait().is_ok_and(|status| status.is_none()) {
                    let _ = self.0.kill();
                }
                let _ = self.0.wait();
            }
        }

        let fixture = NativeImport::new();
        let mut child = ImportChild(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "codex::tests::native_import_write_failure_is_retryable_without_partial_history",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env_clear()
                .env(CHILD_ROOT, &fixture.root.0)
                .env(CHILD_SESSION, &fixture.session)
                .stdin(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "bounded native import child failed: {status}"
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "bounded native import child timed out"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(fs::read(fixture.target()).unwrap(), fixture.history);
        assert_eq!(
            fs::read(fixture.source.join("sessions").join(&fixture.relative)).unwrap(),
            fixture.history
        );
    }

    fn cleanup_instance(
        root: PathBuf,
        backend: Option<OwnedBackend>,
        worker: Option<JoinHandle<()>>,
        recovery_store: Option<PathBuf>,
    ) -> Arc<Instance> {
        let (sender, _receiver) = mpsc::sync_channel(1);
        Arc::new(Instance {
            id: "cleanup-fixture".to_owned(),
            root,
            model: "fixture-model".to_owned(),
            recovery_store,
            checkpoint: Mutex::new(None),
            sender,
            stop: AtomicBool::new(false),
            process: Mutex::new(backend),
            cleanup: Mutex::new(None),
            connection: Mutex::new(None),
            worker: Mutex::new(worker),
            state: Mutex::new(State {
                terminal: Some("pty".to_owned()),
                session: None,
                provider_session: None,
                joining_session: None,
                expected_resume: None,
                native_session_replaced: false,
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
        })
    }

    fn owned_sleep() -> OwnedBackend {
        let mut command = Command::new("/bin/sleep");
        command
            .arg("60")
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
        OwnedBackend::new(command.spawn().unwrap()).unwrap()
    }

    #[test]
    fn native_replacement_blocks_saved_thread_even_after_later_cleanup() {
        let root = PrivateRoot(
            std::env::temp_dir().join(format!("amux-native-replaced-{}", Uuid::new_v4().simple())),
        );
        private_dir(&root.0).unwrap();
        let owned_root = root.0.join("backend");
        private_dir(&owned_root).unwrap();
        let store = prepare_recovery_store(&root.0.join("store")).unwrap();
        let session = Uuid::new_v4().to_string();
        let path = store
            .join("sessions")
            .join(format!("rollout-saved-{session}.jsonl"));
        let data = format!(
            "{}\n",
            json!({"type":"session_meta","payload":{"id":session}})
        );
        fs::write(&path, &data).unwrap();
        let instance = cleanup_instance(owned_root, Some(owned_sleep()), None, Some(store));
        {
            let mut state = instance.state.lock();
            state.session = Some(session.clone());
            state.frontend = "attached";
            state.session_state = "ready";
            state.activity = "idle";
            state.direct_input = true;
        }
        let broker = Broker::new(&root.0, None).unwrap();
        broker
            .inner
            .instances
            .lock()
            .insert(instance.id.clone(), instance.clone());
        let saved = broker.recovery("pty").unwrap().unwrap();
        assert_eq!(saved["session_id"], session);
        assert_eq!(saved["recoverable"], true);
        assert_eq!(saved["root_lifetime"], instance.id);
        let (stream, _peer) = UnixStream::pair().unwrap();
        let mut socket =
            WebSocket::from_raw_socket(stream, tungstenite::protocol::Role::Client, None);
        let mut discovery = None;
        let mut replacement = json!({"method":"thread/started","params":{"thread":{
            "id":Uuid::new_v4().to_string(),"source":"cli","parentThreadId":null,
            "ephemeral":true,"threadSource":"thread_title"}}});
        handle_message(&instance, &mut socket, &replacement, &mut discovery).unwrap();
        assert_eq!(
            broker.recovery("pty").unwrap().unwrap()["session_id"],
            session
        );
        replacement["params"]["thread"]["ephemeral"] = json!(false);
        handle_message(&instance, &mut socket, &replacement, &mut discovery).unwrap();
        let inspect =
            json!({"version":1,"op":"agent-inspect","terminal":"pty","instance_id":instance.id});
        let degraded = broker.inspect(&inspect).unwrap();
        assert_eq!(degraded["problem"], "native-session-replaced");
        assert_eq!(degraded["capabilities"]["prompt"], false);
        // Losing A's backing-file validity must not turn an explicit native
        // replacement back into a transient observation or a checkpoint of A.
        let superseded = "old backing context is no longer current\n";
        fs::write(&path, superseded).unwrap();
        for after_close in [false, true] {
            if after_close {
                broker.end_terminal("pty");
                assert!(!instance.root.exists());
                assert_ne!(
                    broker.inspect(&inspect).unwrap()["problem"],
                    "native-conversation-checkpoint-failed"
                );
            }
            let unavailable = broker.recovery("pty").unwrap().unwrap();
            assert_eq!(unavailable["recoverable"], false);
            assert_eq!(unavailable["reason"], "native-session-replaced");
            assert_eq!(unavailable["root_lifetime"], instance.id);
            assert!(unavailable.get("session_id").is_none());
            assert!(unavailable.get("session_file").is_none());
            assert!(unavailable.get("peer_pid").is_none());
            assert_eq!(fs::read_to_string(&path).unwrap(), superseded);
        }
    }

    #[test]
    fn failed_preparations_release_capacity_only_after_conclusive_cleanup() {
        for bound in [false, true] {
            for (has_backend, removable) in [(true, false), (true, true), (false, true)] {
                let settled = has_backend && removable;
                let root = PrivateRoot(
                    std::env::temp_dir()
                        .join(format!("amux-retained-cleanup-{}", Uuid::new_v4().simple())),
                );
                private_dir(&root.0).unwrap();
                let runtime = root.0.join("runtime");
                // A free capacity slot stops at this filesystem error, before
                // executable resolution or any external provider could run.
                fs::write(&runtime, "not a directory").unwrap();
                let broker = Broker::new(&runtime, None).unwrap();
                // Existing completed bound instances retain their ordinary
                // broker slots. Only the preparation under test can free one.
                for index in 0..INSTANCE_LIMIT - 1 {
                    let mut retired =
                        cleanup_instance(root.0.join(format!("retired-{index}")), None, None, None);
                    let retired_id = format!("retired-{index}");
                    let instance = Arc::get_mut(&mut retired).unwrap();
                    instance.id = retired_id.clone();
                    instance.stop.store(true, Ordering::Release);
                    *instance.cleanup.lock() = Some(true);
                    {
                        let mut state = instance.state.lock();
                        state.terminal = Some(format!("retired-terminal-{index}"));
                        state.frontend = "exited";
                        state.backend = "exited";
                        state.connection = "lost";
                        state.session_state = "unknown";
                    }
                    broker.inner.instances.lock().insert(retired_id, retired);
                }
                let owned_root = root.0.join("backend");
                if removable {
                    private_dir(&owned_root).unwrap();
                } else {
                    fs::write(&owned_root, "retained resource").unwrap();
                }
                let backend = has_backend.then(owned_sleep);
                let pid = backend.as_ref().map(|backend| backend.child.id() as i32);
                let instance = cleanup_instance(owned_root.clone(), backend, None, None);
                if !bound {
                    instance.state.lock().terminal = None;
                }
                let id = instance.id.clone();
                broker
                    .inner
                    .instances
                    .lock()
                    .insert(id.clone(), instance.clone());
                let prepared = Prepared {
                    instance_id: id.clone(),
                    command: String::new(),
                    env: BTreeMap::new(),
                    broker: Arc::downgrade(&broker.inner),
                    instance,
                };
                if bound {
                    assert_eq!(prepared.cleanup_failed_launch(), settled);
                    assert_eq!(prepared.cleanup_failed_launch(), settled);
                }
                drop(prepared);
                if let Some(pid) = pid {
                    assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "owned backend survived");
                    assert_eq!(
                        std::io::Error::last_os_error().raw_os_error(),
                        Some(libc::ESRCH)
                    );
                }
                let request = json!({"version":1,"op":"agent-open",
                    "controller":{"kind":"controller","ref":"fixture"},"model":"fixture"});
                let error = broker.prepare(&request).err().unwrap();
                if settled {
                    assert!(!owned_root.exists());
                    assert!(prepare_failure_settled(&error));
                    assert_ne!(
                        error.to_string().split(':').next(),
                        Some("settlement-unavailable"),
                        "conclusive cleanup must release capacity before the guarded filesystem failure"
                    );
                    assert_eq!(
                        broker
                            .bind(&id, "diagnostic")
                            .unwrap_err()
                            .to_string()
                            .split(':')
                            .next(),
                        Some("stale-target")
                    );
                } else {
                    assert_eq!(
                        error.to_string().split(':').next(),
                        Some("settlement-unavailable")
                    );
                    if removable {
                        assert!(
                            owned_root.is_dir(),
                            "missing handles were accepted as cleanup"
                        );
                    } else {
                        assert_eq!(
                            fs::read_to_string(&owned_root).unwrap(),
                            "retained resource"
                        );
                    }
                    if bound {
                        let request = json!({"version":1,"op":"agent-inspect","terminal":"pty","instance_id":id});
                        let state = broker.inspect(&request).unwrap();
                        assert_eq!(state["problem"], "owned-resource-cleanup-incomplete");
                        assert_eq!(state["capabilities"]["prompt"], false);
                        assert!(broker.take_events().iter().any(|(kind, terminal, data)| {
                            kind == "agent-lifecycle"
                                && terminal == "pty"
                                && data["reason"] == "owned-resource-cleanup-incomplete"
                        }));
                        broker.end_terminal("pty");
                        broker.shutdown();
                        assert_eq!(
                            broker.inspect(&request).unwrap()["problem"],
                            state["problem"]
                        );
                    } else {
                        assert_eq!(
                            broker
                                .bind(&id, "diagnostic")
                                .unwrap_err()
                                .to_string()
                                .split(':')
                                .next(),
                            Some("unavailable")
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn failed_resource_cleanup_stays_unsettled_after_handles_and_map_are_gone() {
        let root = PrivateRoot(
            std::env::temp_dir().join(format!("amux-cleanup-failed-{}", Uuid::new_v4().simple())),
        );
        private_dir(&root.0).unwrap();
        let owned_root = root.0.join("backend");
        // A replaced root cannot be conclusively removed as the owned directory.
        fs::write(&owned_root, "retained resource").unwrap();
        let mut command = Command::new("/bin/sleep");
        command
            .arg("60")
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
        let pid = backend.child.id() as i32;
        let instance = cleanup_instance(owned_root.clone(), Some(backend), None, None);
        let prepared = Prepared {
            instance_id: instance.id.clone(),
            command: String::new(),
            env: BTreeMap::new(),
            broker: Weak::new(),
            instance,
        };
        assert!(!prepared.cleanup_failed_launch());
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "owned backend survived");
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
        assert_eq!(
            fs::read_to_string(&owned_root).unwrap(),
            "retained resource"
        );
        fs::remove_file(&owned_root).unwrap();
        // A second call must remember the failed teardown, not accept empty handles.
        assert!(!prepared.cleanup_failed_launch());
    }

    #[test]
    fn shutdown_waits_for_concurrent_terminal_cleanup_and_owned_resources() {
        let root = PrivateRoot(
            std::env::temp_dir().join(format!("amux-close-{}", Uuid::new_v4().simple())),
        );
        fs::DirBuilder::new().mode(0o700).create(&root.0).unwrap();
        let store = PrivateRoot(
            std::env::temp_dir().join(format!("amux-native-store-{}", Uuid::new_v4().simple())),
        );
        let durable = prepare_recovery_store(&store.0).unwrap();
        private_dir(&root.0.join("codex")).unwrap();
        symlink(durable.join("sessions"), root.0.join("codex/sessions")).unwrap();
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
        let instance = cleanup_instance(
            root.0.clone(),
            Some(backend),
            Some(worker),
            Some(durable.clone()),
        );
        let broker = Arc::new(Broker::new(&root.0, None).unwrap());
        broker
            .inner
            .instances
            .lock()
            .insert(instance.id.clone(), instance.clone());
        let prepared = Arc::new(Prepared {
            instance_id: instance.id.clone(),
            command: String::new(),
            env: BTreeMap::new(),
            broker: Arc::downgrade(&broker.inner),
            instance: instance.clone(),
        });
        let session = Uuid::new_v4().to_string();
        {
            let mut state = instance.state.lock();
            state.session = Some(session.clone());
            state.provider_session = Some("not-the-resumable-thread".to_owned());
            state.frontend = "attached";
            state.session_state = "ready";
        }
        // Native creation can allocate an identity before its first durable write.
        assert_eq!(broker.recovery("pty").unwrap(), None);
        let filename = format!("rollout-2026-10-29T00-00-00-{session}.jsonl");
        let native_path = root.0.join("codex/sessions").join(&filename);
        fs::write(
            &native_path,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"wrong-thread\"}}\n",
        )
        .unwrap();
        assert!(broker.recovery("pty").is_err());
        let native_data = format!(
            "{}\n{}\n",
            json!({"type":"session_meta","payload":{"id":session,"cwd":"/private/project"}}),
            json!({"type":"response_item","payload":{"fixture":"native transcript only"}}),
        );
        fs::write(&native_path, &native_data).unwrap();
        let saved = broker.recovery("pty").unwrap().unwrap();
        assert_eq!(saved["session_id"], session);
        assert_eq!(saved["peer_pid"], backend_pid);
        let saved_path = PathBuf::from(saved["session_file"].as_str().unwrap());
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
        let (rollback_started, rollback_start) = mpsc::channel();
        let (rolled_back, rollback_result) = mpsc::channel();
        let rollback = {
            let prepared = prepared.clone();
            thread::spawn(move || {
                rollback_started.send(()).unwrap();
                rolled_back.send(prepared.cleanup_failed_launch()).unwrap();
            })
        };
        let rollback_attempted = rollback_start.recv_timeout(Duration::from_secs(3)).is_ok();
        let premature_rollback = rollback_result
            .recv_timeout(Duration::from_millis(250))
            .is_ok();
        release.send(()).unwrap();
        ending.join().unwrap();
        shutdown.join().unwrap();
        rollback.join().unwrap();
        let rollback_settled = rollback_result
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        assert!(rollback_attempted && !premature_rollback && rollback_settled);
        assert!(
            prepared.cleanup_failed_launch(),
            "exact instance lost completed cleanup evidence"
        );
        let resources_removed = !root.0.exists();
        assert_eq!(fs::read_to_string(&saved_path).unwrap(), native_data);
        assert!(!durable.join("auth.json").exists());
        assert!(!durable.join("config.toml").exists());
        let reopened = store.0.join("reopened-private-home");
        private_dir(&reopened).unwrap();
        copy_resume(&durable, &reopened, &session).unwrap();
        assert_eq!(
            fs::read_to_string(reopened.join("sessions").join(filename)).unwrap(),
            native_data
        );
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

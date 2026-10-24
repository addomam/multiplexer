use crate::{
    client,
    integration::Integration,
    projects::{self, GitCache, Store},
    runtime::Terminal,
};
use anyhow::{Context, Result, bail, ensure};
use parking_lot::{Condvar, Mutex};
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

static SIGNAL_STOP: AtomicBool = AtomicBool::new(false);
const DIAGNOSTIC_LIMIT: usize = 20;
const DIAGNOSTIC_TEXT_LIMIT: usize = 16 * 1024;
extern "C" fn signal_stop(_: libc::c_int) {
    SIGNAL_STOP.store(true, Ordering::Relaxed);
}

struct ClientSlot {
    connections: Arc<Mutex<BTreeMap<u64, UnixStream>>>,
    id: u64,
}
impl Drop for ClientSlot {
    fn drop(&mut self) {
        self.connections.lock().remove(&self.id);
    }
}
#[derive(Clone)]
struct Entry {
    terminal: Arc<Terminal>,
    name: String,
    project_id: String,
    parent_id: Option<String>,
    cwd: PathBuf,
    agent_instance: Option<String>,
}
struct State {
    store: Store,
    terminals: BTreeMap<String, Entry>,
    agent_views: BTreeMap<String, Arc<Value>>,
    opening_terminals: usize,
    seq: u64,
    events: VecDeque<Value>,
    event_bytes: usize,
    diagnostics: Vec<Value>,
    observed: BTreeMap<String, Value>,
    opening_projects: std::collections::BTreeSet<String>,
    detach_generation: u64,
}
struct HookJob {
    hooks: Vec<projects::Hook>,
    cwd: PathBuf,
    env: BTreeMap<String, String>,
    kind: &'static str,
}
struct Daemon {
    state: Mutex<State>,
    controls: Mutex<crate::control::Registry>,
    pending_launches: Mutex<BTreeMap<String, LaunchControl>>,
    codex: crate::codex::Broker,
    acp: crate::acp::Broker,
    changed: Condvar,
    integration: Arc<Integration>,
    git: GitCache,
    stopping: AtomicBool,
    hook_jobs: std::sync::mpsc::SyncSender<Option<HookJob>>,
}
struct LaunchControl {
    controller: Value,
    grant: Value,
    authority: Arc<crate::control::Authority>,
    retire: bool,
}
struct Opened {
    terminal: Value,
    control: Option<Value>,
}
struct OpenContext {
    admission: TerminalAdmission,
    project_id: String,
    cwd: PathBuf,
    env: BTreeMap<String, String>,
    freshness: Value,
    command: Option<String>,
}
struct ProjectOpening {
    daemon: Arc<Daemon>,
    id: String,
}
impl Drop for ProjectOpening {
    fn drop(&mut self) {
        self.daemon.state.lock().opening_projects.remove(&self.id);
        self.daemon.changed.notify_all();
    }
}
struct TerminalAdmission {
    daemon: Arc<Daemon>,
    active: bool,
}
impl Drop for TerminalAdmission {
    fn drop(&mut self) {
        if self.active {
            self.daemon.state.lock().opening_terminals -= 1;
            self.daemon.changed.notify_all();
        }
    }
}
struct SocketFile(PathBuf);
impl Drop for SocketFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn diagnostic_text(text: &str) -> &str {
    let mut end = text.len().min(DIAGNOSTIC_TEXT_LIMIT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
fn bound_diagnostic(value: &mut Value) {
    match value {
        Value::String(text) if text.len() > DIAGNOSTIC_TEXT_LIMIT => {
            let end = diagnostic_text(text).len();
            text.truncate(end);
            text.shrink_to_fit();
        }
        Value::Array(values) => values.iter_mut().for_each(bound_diagnostic),
        Value::Object(fields) => fields.values_mut().for_each(bound_diagnostic),
        _ => {}
    }
}
struct ByteCount(usize);
impl Write for ByteCount {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn json_size(value: &Value) -> usize {
    let mut size = ByteCount(0);
    serde_json::to_writer(&mut size, value)
        .expect("serializing JSON Value to an infallible counter");
    size.0
}
impl Daemon {
    fn emit(state: &mut State, kind: &str, terminal: Option<&str>, data: Value) {
        state.seq += 1;
        let event = json!({"seq":state.seq,"kind":kind,"terminal":terminal,"data":data});
        state.event_bytes += json_size(&event);
        state.events.push_back(event);
        while state.events.len() > 2048 || state.event_bytes > 4 * 1024 * 1024 {
            if let Some(event) = state.events.pop_front() {
                state.event_bytes -= json_size(&event);
            }
        }
    }
    fn diagnostic(state: &mut State, kind: &str, terminal: Option<&str>, mut data: Value) {
        bound_diagnostic(&mut data);
        if state.diagnostics.len() == DIAGNOSTIC_LIMIT {
            state.diagnostics.remove(0);
        }
        state.diagnostics.push(data.clone());
        Self::emit(state, kind, terminal, data);
    }
    fn launch_failure(&self, id: &str, command: Option<&str>, error: &anyhow::Error) {
        let message = error.to_string();
        let mut state = self.state.lock();
        Self::diagnostic(
            &mut state,
            "launch-failed",
            Some(id),
            json!({"terminal":id,"message":diagnostic_text(&message),
                "command":command.map(diagnostic_text),
                "command_truncated":command.is_some_and(|source| source.len() > DIAGNOSTIC_TEXT_LIMIT)}),
        );
        self.changed.notify_all();
    }
    fn reserve_terminal(self: &Arc<Self>) -> Result<TerminalAdmission> {
        let mut state = self.state.lock();
        if self.stopping.load(Ordering::Acquire) {
            bail!("shutdown: daemon is stopping");
        }
        let limit = state.store.config.terminal_limit;
        if state.terminals.len() + state.opening_terminals >= limit {
            bail!(
                "terminal-limit: retained terminals and pending opens reached limit {limit}; remove an exited terminal or raise terminal_limit"
            );
        }
        state.opening_terminals += 1;
        Ok(TerminalAdmission {
            daemon: self.clone(),
            active: true,
        })
    }
    fn resolve(state: &State, selector: &str) -> Result<String> {
        if state.terminals.contains_key(selector) {
            return Ok(selector.to_owned());
        }
        let found: Vec<_> = state
            .terminals
            .iter()
            .filter(|(_, entry)| entry.name == selector)
            .map(|(id, _)| id)
            .collect();
        match found.as_slice() {
            [id] => Ok((*id).clone()),
            [] => bail!("not-found: no terminal matches {selector}"),
            _ => bail!("ambiguous: more than one terminal is named {selector}"),
        }
    }
    fn entry(&self, request: &Value) -> Result<Entry> {
        let selector = request["terminal"]
            .as_str()
            .context("invalid-request: terminal selector required")?;
        let state = self.state.lock();
        let id = Self::resolve(&state, selector)?;
        Ok(state.terminals[&id].clone())
    }
    fn terminal_snapshot(&self, entry: &Entry) -> Result<Value> {
        let mut snapshot = entry.terminal.status();
        snapshot["id"] = entry.terminal.id.clone().into();
        snapshot["name"] = entry.name.clone().into();
        snapshot["project_id"] = entry.project_id.clone().into();
        snapshot["parent_id"] = json!(entry.parent_id);
        let cwd = self
            .integration
            .captured_cwd(&entry.terminal.id)
            .unwrap_or_else(|| entry.cwd.clone());
        snapshot["workspace"] = json!({"cwd":cwd,"git":self.git.get(&cwd)});
        snapshot["integration"] = self.integration.state(&entry.terminal.id);
        snapshot["ownership"] = self.controls.lock().snapshot(&entry.terminal.id);
        if let Some(instance) = &entry.agent_instance {
            snapshot["agent"] = self.codex.inspect(&json!({
                "version":1,"op":"agent-inspect",
                "terminal":entry.terminal.id,"instance_id":instance
            }))?;
        }
        Ok(snapshot)
    }
    fn snapshot(&self) -> Result<Value> {
        let (mut snapshot, views) = {
            let state = self.state.lock();
            let terminals: Vec<_> = state
                .terminals
                .values()
                .map(|entry| self.terminal_snapshot(entry))
                .collect::<Result<_>>()?;
            let views: Vec<_> = state.agent_views.values().cloned().collect();
            (
                json!({"seq":state.seq,"detach_generation":state.detach_generation,
                "config":state.store.config,"projects":state.store.projects.values().collect::<Vec<_>>(),
                "terminals":terminals,"diagnostics":state.diagnostics}),
                views,
            )
        };
        // No topology guard survives into an ACP projection or authority read.
        let mut agents = Vec::with_capacity(views.len());
        for view in views {
            let Some(id) = view["id"].as_str() else {
                continue;
            };
            let Ok(mut metadata) = self.acp.metadata(id) else {
                continue;
            };
            let mut ownership = self.controls.lock().snapshot(id);
            ownership["retained_work"] = metadata["work"].take();
            ownership["work_as_of_seq"] = metadata["seq"].clone();
            agents.push(json!({"id":id,"kind":"acp","name":view["name"],
                "project_id":view["project_id"],"instance_id":view["instance_id"],
                "connection_generation":metadata["target"]["connection_generation"],
                "session_id":metadata["target"]["session_id"],"lifecycle":metadata["lifecycle"],
                "ownership":ownership,"seq":metadata["seq"]}));
        }
        snapshot["agent_views"] = agents.into();
        Ok(snapshot)
    }
    fn acp_open(&self, request: &Value) -> Result<Value> {
        validate_mutation_fields(
            request,
            &[
                "version",
                "op",
                "persistent",
                "project",
                "name",
                "cwd",
                "controller",
                "command",
            ],
        )?;
        let project = request["project"]
            .as_str()
            .context("invalid-request: project ID required")?;
        let default_cwd = self
            .state
            .lock()
            .store
            .projects
            .get(project)
            .context("project-not-found: exact project ID required")?
            .context
            .clone();
        let name = match request.get("name") {
            None => "ACP agent",
            Some(Value::String(name))
                if !name.is_empty()
                    && name.len() <= 128
                    && name.chars().all(|c| {
                        !c.is_control()
                            && !matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}'
                                | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
                    }) =>
            {
                name
            }
            _ => bail!("invalid-request: ACP name must be a safe 1..128 byte label"),
        };
        let cwd = match request.get("cwd") {
            None => default_cwd,
            Some(Value::String(cwd)) => PathBuf::from(cwd),
            _ => bail!("invalid-request: ACP cwd must be an absolute path"),
        };
        ensure!(
            cwd.is_absolute(),
            "invalid-request: ACP cwd must be absolute"
        );
        // Filesystem checks are outside topology, authority and broker locks.
        let cwd = cwd
            .canonicalize()
            .context("invalid-request: ACP cwd is unavailable")?;
        ensure!(cwd.is_dir(), "invalid-request: ACP cwd must be a directory");
        let args = request["command"]
            .as_array()
            .context("invalid-request: command argv required")?;
        ensure!(
            !args.is_empty() && args.len() <= 128,
            "invalid-request: command argv exceeds limit"
        );
        let mut bytes = 0usize;
        let command = args
            .iter()
            .map(|arg| {
                let arg = arg
                    .as_str()
                    .context("invalid-request: command arguments must be strings")?;
                bytes += arg.len();
                ensure!(
                    bytes <= 64 * 1024 && !arg.contains('\0'),
                    "invalid-request: command argv exceeds limit"
                );
                Ok(arg.to_owned())
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            Path::new(&command[0]).is_absolute(),
            "invalid-request: adapter executable must be absolute"
        );
        ensure!(
            !self.stopping.load(Ordering::Acquire),
            "daemon-stopping: cannot open ACP view"
        );
        let descriptor = Arc::new(self.acp.reserve(project, name, &cwd, command)?);
        let id = descriptor["id"]
            .as_str()
            .context("acp-internal: missing reserved target")?;
        let admitted = (|| {
            let mut state = self.state.lock();
            ensure!(
                !self.stopping.load(Ordering::Acquire),
                "daemon-stopping: cannot publish ACP view"
            );
            let mut controls = self.controls.lock();
            let lifetime = controls.add_acp_target(id)?;
            let observed = controls.snapshot(id)["observed"].clone();
            let grant = match controls.claim(&json!({
                "version":1,"op":"ownership-claim","controller":request["controller"],
                "target":{"kind":"acp","id":id},"observed":observed,
                "mode":"cooperative","when":{"kind":"now"},
            })) {
                Ok(grant) => grant,
                Err(error) => {
                    controls.end_acp(id, || {})?;
                    controls.remove_target(id);
                    return Err(error);
                }
            };
            // Publishing a reservation and its authority is one topology step.
            state
                .agent_views
                .insert(id.to_owned(), Arc::clone(&descriptor));
            Self::emit(
                &mut state,
                "acp-opened",
                None,
                json!({"target":{"kind":"acp","id":id}}),
            );
            Ok((lifetime, grant))
        })();
        let (lifetime, grant) = match admitted {
            Ok(admitted) => admitted,
            Err(error) => {
                self.acp.end(id);
                self.acp.cleanup(id);
                if self.acp.discard_empty_ended(id) {
                    self.flush_runtime_events();
                }
                return Err(error);
            }
        };
        if let Err(error) = self.acp.start(id, lifetime) {
            self.controls.lock().end_acp(id, || self.acp.end(id))?;
            self.acp.cleanup(id);
            if self.acp.discard_empty_ended(id) {
                self.flush_runtime_events();
            }
            return Err(error);
        }
        self.changed.notify_all();
        Ok(json!({"kind":"acp-opened","target":{
            "kind":"acp","id":id,"instance_id":descriptor["instance_id"],
            "connection_generation":descriptor["connection_generation"],"session_id":null},
            "control":grant["control"],"generation":grant["generation"],
            "lifecycle":"starting","seq":self.state.lock().seq}))
    }

    fn acp_action(&self, op: &str, request: &Value, connection: u64) -> Result<Value> {
        ensure!(
            !self.stopping.load(Ordering::Acquire),
            "daemon-stopping: ACP input is closed"
        );
        ensure!(
            request["target"]["kind"] == "acp",
            "wrong-target-kind: ACP target required"
        );
        let id = request["target"]["id"]
            .as_str()
            .context("invalid-request: ACP target ID required")?;
        let authority = self
            .controls
            .lock()
            .authorize(id, &request["control"], connection)?;
        let prepared = self.acp.prepare(op, request, authority.controller_id())?;
        let result = authority.admit_acp(op == "acp-close", connection, |generation| {
            self.acp.commit(prepared, generation)
        })?;
        if op == "acp-close" {
            self.acp.cleanup(id);
        }
        self.flush_runtime_events();
        Ok(result)
    }

    fn decorate_acp_work(&self, value: &mut Value) {
        if value["target"]["kind"] == "acp"
            && let Some(id) = value["target"]["id"].as_str()
        {
            if let Ok(mut metadata) = self.acp.metadata(id) {
                value["retained_work"] = metadata["work"].take();
                // This is a current canonical projection, not an as-of-grant fence.
                value["work_as_of_seq"] = metadata["seq"].take();
            } else {
                value["retained_work_status"] = json!("unavailable");
            }
        }
        if let Some(ownership) = value.get_mut("ownership") {
            self.decorate_acp_work(ownership);
        }
        if value["kind"] == "controller-state" {
            for field in ["controls", "claims"] {
                if let Some(items) = value[field].as_array_mut() {
                    for item in items {
                        self.decorate_acp_work(item);
                    }
                }
            }
        }
    }

    fn begin_shutdown(&self) {
        self.stopping.store(true, Ordering::Release);
        let views: Vec<_> = self.state.lock().agent_views.values().cloned().collect();
        for view in views {
            if let Some(id) = view["id"].as_str() {
                let _ = self.controls.lock().end_acp(id, || self.acp.end(id));
            }
        }
        self.changed.notify_all();
    }

    fn execute_source(
        &self,
        entry: &Entry,
        source: &str,
        queue: bool,
        authority: Arc<crate::control::Authority>,
    ) -> Result<Value> {
        authority.check()?;
        self.integration.preflight(&entry.terminal.id, queue)?;
        if queue {
            return self.integration.execute_authorized(
                &entry.terminal.id,
                source,
                true,
                authority,
            );
        }
        let _input = entry.terminal.input_gate.lock();
        entry
            .terminal
            .input_confirmed_authorized(Vec::new(), authority.clone())?;
        self.integration
            .execute_authorized(&entry.terminal.id, source, false, authority)
    }
    fn shell_control(&self, request: &Value) -> Result<Value> {
        let entry = self.entry(request)?;
        let event = &request["event"];
        let execution = event["execution_id"]
            .as_str()
            .context("invalid-request: execution ID required")?;
        let authority = self
            .integration
            .execution_authority(&entry.terminal.id, execution)?;
        if authority.check().is_err() {
            return Ok(json!({"revoked":true}));
        }
        let generation = event["prompt_generation"]
            .as_u64()
            .context("invalid-request: prompt generation required")?;
        let queued = self
            .integration
            .pending_queue(&entry.terminal.id, execution)?;
        // Immediate execute already owns this gate until native acknowledgement.
        // Queued work obtains it here, without delaying initial queue acceptance.
        let _input = if queued {
            Some(entry.terminal.input_gate.lock())
        } else {
            None
        };
        if authority.check().is_err() {
            return Ok(json!({"revoked":true}));
        }
        if event["pid"] != entry.terminal.pid
            || !self
                .integration
                .prompt_matches(&entry.terminal.id, entry.terminal.pid, generation)
        {
            return Ok(json!({"deferred":true}));
        }
        let confirmed = |bytes| -> Result<bool> {
            match entry
                .terminal
                .input_confirmed_authorized(bytes, authority.clone())
            {
                Ok(()) => Ok(true),
                Err(_) if authority.check().is_err() => Ok(false),
                Err(error) => Err(error),
            }
        };
        if event["kind"] == "shell-epoch" {
            if queued {
                // All previously accepted bytes must reach the PTY before ZLE
                // repeats its native buffer/typeahead check.
                if !confirmed(Vec::new())? {
                    return Ok(json!({"revoked":true}));
                }
            }
            return Ok(json!({"input_epoch":entry.terminal.input_epoch.load(Ordering::Acquire)}));
        }
        let expected = event["input_epoch"]
            .as_u64()
            .context("invalid-request: input epoch required")?;
        if expected != entry.terminal.input_epoch.load(Ordering::Acquire) {
            return Ok(json!({"deferred":true}));
        }
        if !confirmed(format!("\x1b[amux-{}~", entry.terminal.id).into_bytes())? {
            return Ok(json!({"revoked":true}));
        }
        Ok(json!({"woke":true}))
    }
    fn hooks(
        &self,
        hooks: Vec<projects::Hook>,
        cwd: PathBuf,
        env: BTreeMap<String, String>,
        kind: &'static str,
    ) {
        if hooks.is_empty() {
            return;
        }
        if let Err(error) = self.hook_jobs.try_send(Some(HookJob {
            hooks,
            cwd,
            env,
            kind,
        })) {
            let (message, job) = match error {
                std::sync::mpsc::TrySendError::Full(job) => ("hook queue is full", job),
                std::sync::mpsc::TrySendError::Disconnected(job) => ("hook worker stopped", job),
            };
            let mut state = self.state.lock();
            if let Some(job) = job {
                for (index, hook) in job.hooks.iter().enumerate() {
                    Self::diagnostic(
                        &mut state,
                        "hook",
                        None,
                        json!({"kind":kind,"message":format!("hook-backpressure: {message}"),
                            "index":index,"command":diagnostic_text(&hook.source),
                            "command_truncated":hook.source.len() > DIAGNOSTIC_TEXT_LIMIT}),
                    );
                }
            }
            self.changed.notify_all();
        }
    }
    fn launch_control(&self, terminal: &str, caller: Option<&Value>) -> Result<LaunchControl> {
        let mut controls = self.controls.lock();
        let controller = if let Some(caller) = caller {
            caller.clone()
        } else {
            controls.create(&json!({
                "version":1,"op":"controller-create","role":"external","label":"automatic launch"
            }), 0)?["controller"].clone()
        };
        let observed = controls.snapshot(terminal);
        let grant = controls.claim(&json!({
            "version":1,"op":"ownership-claim","controller":controller,"terminal":terminal,
            "observed":observed["observed"],
            "mode":"cooperative","when":{"kind":"now"}
        }))?;
        let authority = controls.authorize(terminal, &grant["control"], 0)?;
        Ok(LaunchControl {
            controller,
            grant,
            authority,
            retire: caller.is_none(),
        })
    }

    fn retire_settled_launch(&self, terminal: &str) {
        let launch = {
            let mut pending = self.pending_launches.lock();
            if !pending
                .get(terminal)
                .is_some_and(|launch| launch.authority.settled_or_ended())
            {
                return;
            }
            pending.remove(terminal).expect("pending private launch")
        };
        let mut controls = self.controls.lock();
        // Normal capability validation makes a late release harmless after
        // force. Closing this private controller never closes the newer owner.
        let _ = controls.handle(&json!({
            "version":1,"op":"control-handle","handle":launch.grant["control"],"method":"release"
        }), 0);
        let _ = controls.handle(
            &json!({
                "version":1,"op":"control-handle","handle":launch.controller,"method":"close"
            }),
            0,
        );
    }

    fn discard_unpublished(&self, terminal: &str, launch: Option<&LaunchControl>) {
        {
            let mut controls = self.controls.lock();
            controls.remove_target(terminal);
            if let Some(launch) = launch.filter(|launch| launch.retire) {
                let _ = controls.handle(&json!({
                    "version":1,"op":"control-handle","handle":launch.controller,"method":"close"
                }), 0);
            }
        }
        self.codex.end_terminal(terminal);
        self.integration.remove(terminal);
    }

    fn agent_open(self: &Arc<Self>, request: &Value, connection: u64) -> Result<Value> {
        let controller = &request["controller"];
        let recovered = self.controls.lock().handle(
            &json!({
                "version":1,"op":"control-handle","handle":controller,"method":"read"
            }),
            connection,
        )?;
        if recovered["kind"] != "controller-state" {
            bail!("wrong-handle-kind: agent-open requires a controller");
        }
        let context = self.open_context(request)?;
        let mut launch_request = request.clone();
        launch_request["cwd"] = json!(&context.cwd);
        let prepared = self.codex.prepare(&launch_request)?;
        let opened = self.open_inner(request, context, Some((controller, &prepared)))?;
        self.flush_runtime_events();
        let mut grant = opened
            .control
            .context("internal: managed launch control missing")?;
        let seq = self.state.lock().seq;
        grant["seq"] = seq.into();
        let mut state = self.codex.inspect(&json!({
            "version":1,"op":"agent-inspect",
            "terminal":opened.terminal["id"],"instance_id":prepared.instance_id
        }))?;
        state["seq"] = seq.into();
        Ok(
            json!({"terminal":opened.terminal,"instance_id":prepared.instance_id,
            "control":grant,"state":state,"seq":seq}),
        )
    }

    fn open(self: &Arc<Self>, request: &Value) -> Result<Value> {
        let context = self.open_context(request)?;
        self.open_inner(request, context, None)
            .map(|opened| opened.terminal)
    }

    fn open_context(self: &Arc<Self>, request: &Value) -> Result<OpenContext> {
        let admission = self.reserve_terminal()?;
        let (project_id, cwd, env, freshness, command) = {
            let state = self.state.lock();
            let source = if let Some(selector) = request["source"].as_str() {
                Some(Self::resolve(&state, selector)?)
            } else {
                None
            };
            let project_id = if let Some(selector) = request["project"].as_str() {
                let matches: Vec<_> = state
                    .store
                    .projects
                    .values()
                    .filter(|p| p.id == selector || p.name == selector)
                    .collect();
                match matches.as_slice() {
                    [project] => project.id.clone(),
                    [] => bail!("not-found: project {selector}"),
                    _ => bail!("ambiguous: project {selector}"),
                }
            } else if let Some(source) = &source {
                state.terminals[source].project_id.clone()
            } else {
                state.store.unsaved_id()
            };
            let project = state
                .store
                .projects
                .get(&project_id)
                .context("not-found: project unavailable")?;
            if source
                .as_ref()
                .is_some_and(|id| state.terminals[id].project_id != project_id)
            {
                bail!("cross-project: source belongs to a different project");
            }
            let context_source = source.or_else(|| project.main_terminal.clone());
            let capture = context_source
                .as_ref()
                .and_then(|id| self.integration.capture(id));
            let freshness = context_source
                .as_ref()
                .map(|id| self.integration.state(id)["shell"]["freshness"].clone())
                .unwrap_or(Value::Null);
            let cwd = request["cwd"]
                .as_str()
                .map(PathBuf::from)
                .or_else(|| capture.as_ref().map(|context| context.cwd.clone()))
                .unwrap_or_else(|| project.context.clone());
            let env = capture
                .map(|context| context.env)
                .unwrap_or_else(|| std::env::vars().collect());
            let command = if request["omp"] == true {
                Some("command omp -e \"$AMUX_OMP_EXTENSION\"".to_owned())
            } else if let Some(recipe) = request["recipe"].as_str() {
                Some(
                    project
                        .recipes
                        .get(recipe)
                        .with_context(|| format!("not-found: recipe {recipe}"))?
                        .clone(),
                )
            } else {
                request["command"].as_str().map(str::to_owned)
            };
            (project_id, cwd, env, freshness, command)
        };
        if !cwd.is_dir() {
            bail!("invalid-directory: {}", cwd.display());
        }
        Ok(OpenContext {
            admission,
            project_id,
            cwd,
            env,
            freshness,
            command,
        })
    }

    fn open_inner(
        self: &Arc<Self>,
        request: &Value,
        context: OpenContext,
        managed: Option<(&Value, &crate::codex::Prepared)>,
    ) -> Result<Opened> {
        let OpenContext {
            mut admission,
            project_id,
            cwd,
            mut env,
            freshness,
            mut command,
        } = context;
        let agent_instance = managed.map(|(_, prepared)| prepared.instance_id.clone());
        if let Some((_, prepared)) = managed {
            command = Some(managed_frontend_source(prepared));
        }
        let id = Uuid::new_v4().to_string();
        let launch = match self.integration.prepare(&id) {
            Ok(launch) => launch,
            Err(error) => {
                self.integration.remove(&id);
                self.launch_failure(
                    &id,
                    if managed.is_some() {
                        Some("managed Codex frontend")
                    } else {
                        command.as_deref()
                    },
                    &error,
                );
                return Err(error);
            }
        };
        let original = env.get("ZDOTDIR").cloned();
        env.extend(launch);
        if let Some(zdotdir) = original {
            env.insert("AMUX_ORIGINAL_ZDOTDIR".into(), zdotdir);
        } else {
            env.remove("AMUX_ORIGINAL_ZDOTDIR");
        }
        self.controls.lock().add_target(&id);
        let startup_control = if command.is_some() {
            match self.launch_control(&id, managed.map(|(controller, _)| controller)) {
                Ok(control) => Some(control),
                Err(error) => {
                    self.discard_unpublished(&id, None);
                    return Err(error);
                }
            }
        } else {
            None
        };
        let initial_control = startup_control
            .as_ref()
            .filter(|control| !control.retire)
            .map(|control| control.grant.clone());
        if let Some(instance) = &agent_instance
            && let Err(error) = self.codex.bind(instance, &id)
        {
            self.discard_unpublished(&id, startup_control.as_ref());
            return Err(error);
        }
        let weak = Arc::downgrade(self);
        let exit_id = id.clone();
        let terminal = match Terminal::spawn(
            id.clone(),
            &cwd,
            &env,
            Arc::new(move |status| {
                if let Some(daemon) = weak.upgrade() {
                    daemon.controls.lock().end_target(&exit_id);
                    daemon.retire_settled_launch(&exit_id);
                    daemon.codex.end_terminal(&exit_id);
                    daemon.flush_runtime_events();
                    let mut state = daemon.state.lock();
                    Self::emit(
                        &mut state,
                        "terminal-exited",
                        Some(&exit_id),
                        json!({"exit_status":status}),
                    );
                    daemon.changed.notify_all();
                }
            }),
        ) {
            Ok(terminal) => terminal,
            Err(error) => {
                self.discard_unpublished(&id, startup_control.as_ref());
                self.launch_failure(
                    &id,
                    if managed.is_some() {
                        Some("managed Codex frontend")
                    } else {
                        command.as_deref()
                    },
                    &error,
                );
                return Err(error);
            }
        };
        let entry = {
            let mut state = self.state.lock();
            if self.stopping.load(Ordering::Acquire) {
                drop(state);
                terminal.close();
                self.discard_unpublished(&id, startup_control.as_ref());
                bail!("shutdown: daemon is stopping");
            }
            let project = state.store.projects.get_mut(&project_id).unwrap();
            let parent = project.main_terminal.clone();
            let name = request["name"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    if request["omp"] == true {
                        "OMP".into()
                    } else if parent.is_none() {
                        "main".into()
                    } else {
                        format!("shell-{}", &id[..8])
                    }
                });
            let entry = Entry {
                terminal,
                name,
                project_id: project_id.clone(),
                parent_id: parent,
                cwd: cwd.clone(),
                agent_instance,
            };
            if project.main_terminal.is_none() {
                project.main_terminal = Some(id.clone());
            }
            state.terminals.insert(id.clone(), entry.clone());
            state.opening_terminals -= 1;
            admission.active = false;
            Self::emit(
                &mut state,
                "terminal-opened",
                Some(&id),
                json!({"project_id":project_id,"cwd":cwd,"source_freshness":freshness}),
            );
            self.changed.notify_all();
            entry
        };
        self.git.refresh(&cwd);
        if let Some(command) = command {
            let daemon = self.clone();
            let launch_id = id.clone();
            let managed_launch = managed.is_some();
            thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(15);
                while daemon.integration.awaiting_initial_prompt(&launch_id)
                    && startup_control
                        .as_ref()
                        .is_some_and(|launch| launch.authority.check().is_ok())
                    && Instant::now() < deadline
                    && !daemon.stopping.load(Ordering::Relaxed)
                {
                    thread::sleep(Duration::from_millis(25));
                }
                let result = if daemon.stopping.load(Ordering::Acquire) {
                    Err(anyhow::anyhow!("shutdown: daemon is stopping"))
                } else {
                    daemon
                        .entry(&json!({"terminal":launch_id}))
                        .and_then(|entry| {
                            daemon.execute_source(
                                &entry,
                                &command,
                                false,
                                startup_control
                                    .as_ref()
                                    .expect("launch authority")
                                    .authority
                                    .clone(),
                            )
                        })
                };
                if let Some(launch) = startup_control.filter(|launch| launch.retire) {
                    daemon
                        .pending_launches
                        .lock()
                        .insert(launch_id.clone(), launch);
                    // Also covers settlement/end racing ahead of registration.
                    daemon.retire_settled_launch(&launch_id);
                }
                if managed_launch && let Err(error) = &result {
                    daemon.codex.launch_failed(&launch_id, &error.to_string());
                }
                daemon.flush_runtime_events();
                let mut state = daemon.state.lock();
                if let Err(error) = result {
                    let data = json!({"terminal":launch_id,"message":error.to_string(),
                        "command":if managed_launch { "managed Codex frontend" } else { diagnostic_text(&command) },
                        "command_truncated":!managed_launch && command.len() > DIAGNOSTIC_TEXT_LIMIT});
                    Self::diagnostic(&mut state, "launch-failed", Some(&launch_id), data);
                }
                daemon.changed.notify_all();
            });
        }
        let mut result = self.terminal_snapshot(&entry)?;
        result["source_freshness"] = freshness;
        Ok(Opened {
            terminal: result,
            control: initial_control,
        })
    }
    fn reparent(&self, entry: &Entry, parent: Option<&str>) -> Result<Value> {
        let id = &entry.terminal.id;
        let mut state = self.state.lock();
        let parent = parent
            .map(|selector| Self::resolve(&state, selector))
            .transpose()?;
        if let Some(parent_id) = &parent {
            if state.terminals[parent_id].project_id != entry.project_id {
                bail!("cross-project: terminal ancestry must remain within a project");
            }
            let mut cursor = Some(parent_id.clone());
            while let Some(ancestor) = cursor {
                if ancestor == *id {
                    bail!("cycle: terminal cannot become its own descendant");
                }
                cursor = state.terminals[&ancestor].parent_id.clone();
            }
        }
        state
            .terminals
            .get_mut(id)
            .context("not-found: terminal was removed")?
            .parent_id = parent.clone();
        Self::emit(
            &mut state,
            "terminal-reparented",
            Some(id),
            json!({"parent_id":parent}),
        );
        self.changed.notify_all();
        Ok(json!({"id":id,"parent_id":parent}))
    }
    fn events(&self, after: u64, timeout: Duration) -> Result<Value> {
        let mut state = self.state.lock();
        if state.seq <= after && !self.stopping.load(Ordering::Relaxed) {
            self.changed.wait_for(&mut state, timeout);
        }
        let gap = state
            .events
            .front()
            .is_some_and(|first| after.saturating_add(1) < first["seq"].as_u64().unwrap_or(0))
            || (state.events.is_empty() && after < state.seq);
        let events: Vec<_> = state
            .events
            .iter()
            .filter(|event| event["seq"].as_u64().unwrap_or(0) > after)
            .cloned()
            .collect();
        let seq = state.seq;
        drop(state);
        let mut result = json!({"seq":seq,"gap":gap,"events":events});
        if gap {
            result["snapshot"] = self.snapshot()?;
        }
        Ok(result)
    }
    fn wait_state(&self, request: &Value) -> Result<Value> {
        let entry = self.entry(request)?;
        let condition = request["condition"]
            .as_str()
            .context("invalid-request: wait condition required")?;
        if ![
            "shell-ready",
            "agent-settled",
            "agent-working",
            "needs-input",
            "exited",
        ]
        .contains(&condition)
        {
            bail!("invalid-condition: {condition}");
        }
        let timeout = request["timeout_ms"].as_u64().unwrap_or(30000).min(300000);
        let deadline = Instant::now() + Duration::from_millis(timeout);
        let mut after = request["after"].as_u64();
        loop {
            let state = self.state.lock();
            let current_entry = state
                .terminals
                .get(&entry.terminal.id)
                .context("not-found: terminal was removed")?;
            let current = self.terminal_snapshot(current_entry)?;
            if let Some(cursor) = after {
                if state.events.front().is_some_and(|event| {
                    cursor.saturating_add(1) < event["seq"].as_u64().unwrap_or(0)
                }) || (state.events.is_empty() && cursor < state.seq)
                {
                    bail!(
                        "stream-gap: wait cursor fell outside retained events; take a new snapshot"
                    );
                }
                for event in state.events.iter().filter(|event| {
                    event["seq"].as_u64().unwrap_or(0) > cursor
                        && event["terminal"] == entry.terminal.id
                }) {
                    let report = &event["data"]["event"];
                    let integration = &event["data"]["integration"];
                    let root_report = report["kind"] == "agent" && report["agent_kind"] == "main";
                    let matches = match condition {
                        "shell-ready" => {
                            report["kind"] == "shell-ready"
                                && integration["shell"]["ready"] == true
                                && integration["shell"]["partial_input"] != true
                        }
                        "agent-settled" => {
                            root_report
                                && report["lifecycle_event"] == "agent_end"
                                && integration["agent"]["activity"] == "idle"
                                && integration["agent"]["settled"] == true
                        }
                        "agent-working" => {
                            root_report && integration["agent"]["activity"] == "working"
                        }
                        "needs-input" => {
                            root_report && integration["agent"]["activity"] == "needs-input"
                        }
                        "exited" => event["kind"] == "terminal-exited",
                        _ => false,
                    };
                    if matches {
                        return Ok(
                            json!({"seq":event["seq"],"state":current,"observed":integration,"event":event}),
                        );
                    }
                }
                after = Some(cursor.max(state.seq));
            } else {
                let integration = &current["integration"];
                let matches = match condition {
                    "shell-ready" => {
                        entry.terminal.alive()
                            && integration["shell"]["ready"] == true
                            && integration["shell"]["partial_input"] != true
                    }
                    "agent-settled" => {
                        integration["agent"]["activity"] == "idle"
                            && integration["agent"]["settled"] == true
                    }
                    "agent-working" => integration["agent"]["activity"] == "working",
                    "needs-input" => integration["agent"]["activity"] == "needs-input",
                    "exited" => !entry.terminal.alive(),
                    _ => false,
                };
                if matches {
                    return Ok(json!({"seq":state.seq,"state":current}));
                }
            }
            drop(state);
            if self.stopping.load(Ordering::Relaxed) {
                bail!("shutdown: daemon is stopping");
            }
            if Instant::now() >= deadline {
                bail!("timeout: waiting for {condition}");
            }
            let mut guard = self.state.lock();
            self.changed.wait_for(
                &mut guard,
                Duration::from_millis(30).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }
    fn flush_runtime_events(&self) {
        // Never hold registry/PTY/ACP locks while taking topology/event ownership.
        let mut events = self.controls.lock().take_events();
        events.extend(self.codex.take_events());
        for (_, _, data) in &mut events {
            self.decorate_acp_work(data);
        }
        let agent_events = self.acp.drain_events();
        let mut removed = Vec::new();
        if events.is_empty() && agent_events.is_empty() {
            return;
        }
        {
            let mut state = self.state.lock();
            for (kind, terminal, mut data) in events {
                let acp = data["ownership"]["target"]["kind"] == "acp";
                if acp {
                    data["target"] = data["ownership"]["target"].clone();
                }
                Self::emit(
                    &mut state,
                    &kind,
                    if acp { None } else { Some(&terminal) },
                    data,
                );
            }
            for event in agent_events {
                let Some(kind) = event["kind"].as_str() else {
                    continue;
                };
                let Some(id) = event["id"].as_str() else {
                    continue;
                };
                if kind == "acp-removed" {
                    state.agent_views.remove(id);
                    removed.push(id.to_owned());
                }
                Self::emit(
                    &mut state,
                    kind,
                    None,
                    json!({
                        "target":{"kind":"acp","id":id},"metadata":event["data"],
                    }),
                );
            }
        }
        for id in removed {
            self.controls.lock().remove_target(&id);
        }
        self.changed.notify_all();
    }
    fn dispatch(self: &Arc<Self>, request: Value, connection: u64) -> Result<Value> {
        if request["version"] != 1 {
            bail!("unsupported-version: API version 1 required");
        }
        let op = request["op"]
            .as_str()
            .context("invalid-request: op required")?;
        if request["target"]["kind"] == "acp"
            && !matches!(
                op,
                "ownership-claim"
                    | "acp-inspect"
                    | "acp-prompt"
                    | "acp-authenticate"
                    | "acp-answer"
                    | "acp-permission"
                    | "acp-interrupt"
                    | "acp-close"
            )
        {
            bail!("wrong-target-kind: native operation cannot address ACP");
        }
        if let Some(id) = request["terminal"].as_str()
            && self.state.lock().agent_views.contains_key(id)
        {
            bail!("wrong-target-kind: ACP views are not terminals");
        }
        match op {
            "acp-open" => self.acp_open(&request),
            "acp-inspect" => self.acp.inspect(&request),
            "acp-prompt" | "acp-authenticate" | "acp-answer" | "acp-permission"
            | "acp-interrupt" | "acp-close" => self.acp_action(op, &request, connection),
            "controller-create" | "ownership-claim" | "control-handle" => {
                let mut result = {
                    let mut controls = self.controls.lock();
                    // Only a validated original attachment view may produce
                    // terminal focus reports. These are not human activity.
                    let focus = if op == "control-handle" && request["method"] == "view" {
                        request["view"]["terminal"].as_str().and_then(|terminal| {
                            let read = controls
                                .handle(
                                    &json!({
                                        "version":1,"op":"control-handle",
                                        "handle":request["handle"],"method":"read"
                                    }),
                                    connection,
                                )
                                .ok()?;
                            let snapshot = controls.snapshot(terminal);
                            let previous = snapshot["views"]
                                .as_array()?
                                .iter()
                                .find(|view| view["controller_id"] == read["controller_id"]);
                            let focused = request["view"]["focused"].as_bool()?;
                            if previous.is_some_and(|view| view["focused"] == focused) {
                                return None;
                            }
                            let control = read["controls"]
                                .as_array()?
                                .iter()
                                .find(|grant| grant["terminal"] == terminal)?;
                            let authority =
                                controls.authorize(terminal, &control["control"], 0).ok()?;
                            Some((terminal.to_owned(), focused, authority))
                        })
                    } else {
                        None
                    };
                    let result = match op {
                        "controller-create" => controls.create(&request, connection),
                        "ownership-claim" => controls.claim(&request),
                        _ => controls.handle(&request, connection),
                    };
                    drop(controls);
                    if result.is_ok()
                        && let Some((terminal, focused, authority)) = focus
                    {
                        let entry = self.state.lock().terminals.get(&terminal).cloned();
                        if let Some(entry) = entry
                            && entry.terminal.screen(0, Some(0))["modes"]["focus_reporting"] == true
                        {
                            // A concurrent revocation must drop this report too.
                            let _ = entry.terminal.input_authorized(
                                if focused {
                                    b"\x1b[I".to_vec()
                                } else {
                                    b"\x1b[O".to_vec()
                                },
                                authority,
                            );
                        }
                    }
                    result
                }?;
                self.decorate_acp_work(&mut result);
                self.flush_runtime_events();
                result["seq"] = self.state.lock().seq.into();
                if result["kind"] == "controller-state" {
                    let seq = result["seq"].clone();
                    if let Some(controls) = result["controls"].as_array_mut() {
                        for control in controls {
                            control["seq"] = seq.clone();
                        }
                    }
                }
                Ok(result)
            }
            "snapshot" | "list" => self.snapshot(),
            "open" => self.open(&request),
            "agent-open" => self.agent_open(&request, connection),
            "agent-inspect" => {
                let mut result = self.codex.inspect(&request)?;
                result["seq"] = self.state.lock().seq.into();
                Ok(result)
            }
            "agent-prompt" | "agent-answer" | "agent-permission" | "agent-interrupt" => {
                let terminal = request["target"]["terminal"]
                    .as_str()
                    .context("invalid-request: exact agent target terminal required")?;
                let authority = self
                    .controls
                    .lock()
                    .authorize(terminal, &request["control"], 0)?;
                let mut result = self.codex.mutate(op, &request, authority)?;
                self.flush_runtime_events();
                result["seq"] = self.state.lock().seq.into();
                Ok(result)
            }
            "state" => self.terminal_snapshot(&self.entry(&request)?),
            "screen" => {
                let entry = self.entry(&request)?;
                Ok(entry.terminal.screen_update(
                    request["offset"].as_u64().unwrap_or(0) as usize,
                    request["limit"].as_u64().map(|limit| limit as usize),
                    request["since"].as_u64(),
                    request["row_updates"].as_bool().unwrap_or(false),
                ))
            }
            "resize" => {
                let entry = self.entry(&request)?;
                entry.terminal.resize(
                    request["cols"]
                        .as_u64()
                        .context("invalid-size: cols required")? as usize,
                    request["rows"]
                        .as_u64()
                        .context("invalid-size: rows required")? as usize,
                )?;
                Ok(entry.terminal.status())
            }
            "input" => {
                validate_mutation_fields(
                    &request,
                    &[
                        "version",
                        "op",
                        "persistent",
                        "terminal",
                        "control",
                        "text",
                        "paste",
                        "bytes",
                        "key",
                    ],
                )?;
                let terminal = request["terminal"]
                    .as_str()
                    .context("invalid-request: exact terminal lifetime ID required")?;
                let authority =
                    self.controls
                        .lock()
                        .authorize(terminal, &request["control"], connection)?;
                let entry = self.entry(&request)?;
                let _input = entry.terminal.input_gate.lock();
                if let Some(text) = request["text"].as_str() {
                    if request["paste"] == true {
                        entry.terminal.paste_authorized(text, authority.clone())?;
                    } else {
                        entry
                            .terminal
                            .input_authorized(text.as_bytes().to_vec(), authority.clone())?;
                    }
                } else if let Some(bytes) = request["bytes"].as_array() {
                    entry.terminal.input_authorized(
                        bytes
                            .iter()
                            .map(|byte| {
                                byte.as_u64()
                                    .filter(|byte| *byte < 256)
                                    .map(|byte| byte as u8)
                                    .context("invalid-input: bytes must be 0..255")
                            })
                            .collect::<Result<_>>()?,
                        authority.clone(),
                    )?;
                } else if let Some(key) = request["key"].as_str() {
                    let mode = entry.terminal.screen(0, Some(0))["modes"].clone();
                    entry.terminal.input_authorized(
                        key_bytes(key, mode["application_cursor"] == true)?,
                        authority.clone(),
                    )?;
                } else {
                    bail!("invalid-input: text, bytes or key required");
                }
                if request["text"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
                    || request["bytes"]
                        .as_array()
                        .is_some_and(|bytes| !bytes.is_empty())
                    || request["key"].is_string()
                {
                    entry.terminal.input_epoch.fetch_add(1, Ordering::Release);
                    // Rejected/backpressured payloads never count as activity.
                    // A concurrent revocation may win; input is never replayed.
                    let _ = authority.activity(connection);
                }
                Ok(json!({"accepted":true}))
            }
            "execute" => {
                validate_mutation_fields(
                    &request,
                    &[
                        "version",
                        "op",
                        "persistent",
                        "terminal",
                        "control",
                        "source",
                        "queue",
                    ],
                )?;
                let terminal = request["terminal"]
                    .as_str()
                    .context("invalid-request: exact terminal lifetime ID required")?;
                let authority = self
                    .controls
                    .lock()
                    .authorize(terminal, &request["control"], 0)?;
                let entry = self.entry(&request)?;
                self.execute_source(
                    &entry,
                    request["source"]
                        .as_str()
                        .context("invalid-request: source required")?,
                    request["queue"] == true,
                    authority,
                )
            }
            "report" => {
                if matches!(
                    request["event"]["kind"].as_str(),
                    Some("shell-epoch" | "shell-wake")
                ) {
                    return self.shell_control(&request);
                }
                let selector = request["terminal"]
                    .as_str()
                    .context("invalid-request: terminal required")?;
                let state = self.state.lock();
                let id = if state.terminals.contains_key(selector)
                    || Uuid::parse_str(selector).is_ok()
                {
                    selector.to_owned()
                } else {
                    Self::resolve(&state, selector).unwrap_or_else(|_| selector.to_owned())
                };
                drop(state);
                let result = self.integration.report(&id, request["event"].clone());
                if matches!(
                    request["event"]["kind"].as_str(),
                    Some("execution-accepted" | "execution-rejected")
                ) {
                    self.retire_settled_launch(&id);
                }
                let result = result?;
                let mut state = self.state.lock();
                Self::emit(
                    &mut state,
                    "integration",
                    Some(&id),
                    json!({"event":request["event"],"integration":result}),
                );
                self.changed.notify_all();
                Ok(result)
            }
            "name" => {
                let entry = self.entry(&request)?;
                let name = request["name"]
                    .as_str()
                    .context("invalid-name: name required")?;
                if name.is_empty() || name.len() > 256 {
                    bail!("invalid-name: expected 1..256 bytes");
                }
                let mut state = self.state.lock();
                state
                    .terminals
                    .get_mut(&entry.terminal.id)
                    .context("not-found: terminal was removed")?
                    .name = name.into();
                Self::emit(
                    &mut state,
                    "terminal-named",
                    Some(&entry.terminal.id),
                    json!({"name":name}),
                );
                self.changed.notify_all();
                Ok(json!({"id":entry.terminal.id,"name":name}))
            }
            "reparent" | "demote" => {
                let entry = self.entry(&request)?;
                self.reparent(&entry, request["parent"].as_str())
            }
            "promote" => {
                let entry = self.entry(&request)?;
                let parent = entry.parent_id.as_ref().and_then(|parent| {
                    self.state
                        .lock()
                        .terminals
                        .get(parent)
                        .and_then(|parent| parent.parent_id.clone())
                });
                self.reparent(&entry, parent.as_deref())
            }
            "close" => {
                let entry = self.entry(&request)?;
                self.controls.lock().end_target(&entry.terminal.id);
                self.retire_settled_launch(&entry.terminal.id);
                entry.terminal.close();
                Ok(entry.terminal.status())
            }
            "remove" => {
                let entry = self.entry(&request)?;
                if entry.terminal.alive() {
                    bail!("alive: close terminal before removing it");
                }
                self.controls.lock().end_target(&entry.terminal.id);
                entry.terminal.close();
                let mut state = self.state.lock();
                // Cleanup ran without the tree lock; ancestry may have changed meanwhile.
                let entry = state
                    .terminals
                    .remove(&entry.terminal.id)
                    .context("not-found: terminal was already removed")?;
                state.observed.remove(&entry.terminal.id);
                for child in state
                    .terminals
                    .values_mut()
                    .filter(|child| child.parent_id.as_deref() == Some(&entry.terminal.id))
                {
                    child.parent_id = entry.parent_id.clone();
                }
                let next = state
                    .terminals
                    .values()
                    .find(|terminal| terminal.project_id == entry.project_id)
                    .map(|terminal| terminal.terminal.id.clone());
                if let Some(project) = state.store.projects.get_mut(&entry.project_id)
                    && project.main_terminal.as_deref() == Some(&entry.terminal.id)
                {
                    project.main_terminal = next;
                }
                Self::emit(
                    &mut state,
                    "terminal-removed",
                    Some(&entry.terminal.id),
                    Value::Null,
                );
                self.changed.notify_all();
                drop(state);
                self.controls.lock().remove_target(&entry.terminal.id);
                self.retire_settled_launch(&entry.terminal.id);
                self.integration.remove(&entry.terminal.id);
                Ok(json!({"removed":entry.terminal.id}))
            }
            "events" => self.events(
                request["after"].as_u64().unwrap_or(0),
                Duration::from_millis(request["timeout_ms"].as_u64().unwrap_or(30000).min(300000)),
            ),
            "wait" => self.wait_state(&request),
            "project-save" => {
                let (id, cwd) = {
                    let state = self.state.lock();
                    let id = request["project"]
                        .as_str()
                        .map(|selector| state.store.resolve(selector))
                        .transpose()?
                        .unwrap_or_else(|| state.store.unsaved_id());
                    let project = state
                        .store
                        .projects
                        .get(&id)
                        .context("not-found: project unavailable")?;
                    let cwd = project
                        .main_terminal
                        .as_ref()
                        .and_then(|id| self.integration.captured_cwd(id));
                    (id, cwd)
                };
                let context = cwd
                    .as_deref()
                    .map(projects::resolve_workspace_context)
                    .transpose()?;
                let mut state = self.state.lock();
                if let Some(context) = context {
                    let project = state
                        .store
                        .projects
                        .get_mut(&id)
                        .context("not-found: project unavailable")?;
                    project.context = context;
                }
                let name = request["name"]
                    .as_str()
                    .context("invalid-request: name required")?;
                let project = state.store.save(&id, name)?;
                Self::emit(&mut state, "project-saved", None, json!(project));
                self.changed.notify_all();
                Ok(json!(project))
            }
            "project-new" => {
                let project = {
                    let mut state = self.state.lock();
                    let project = state.store.create(
                        request["name"]
                            .as_str()
                            .context("invalid-request: name required")?,
                        request["directory"].as_str().map(PathBuf::from),
                        request.get("config").cloned().unwrap_or_else(|| json!({})),
                    )?;
                    state.opening_projects.insert(project.id.clone());
                    project
                };
                let opening = ProjectOpening {
                    daemon: self.clone(),
                    id: project.id.clone(),
                };
                let terminal = self.open(&json!({"project":project.id}))?;
                self.hooks(
                    project.hooks.clone(),
                    project.context.clone(),
                    std::env::vars().collect(),
                    "project-open",
                );
                drop(opening);
                Ok(json!({"project":project,"terminal":terminal}))
            }
            "project-open" => {
                let (project, already_open) = loop {
                    let mut state = self.state.lock();
                    if self.stopping.load(Ordering::Acquire) {
                        bail!("shutdown: daemon is stopping");
                    }
                    let project = state.store.open(
                        request["project"]
                            .as_str()
                            .context("invalid-request: project selector required")?,
                    )?;
                    if state.opening_projects.contains(&project.id) {
                        self.changed
                            .wait_for(&mut state, Duration::from_millis(100));
                        continue;
                    }
                    let already = state
                        .terminals
                        .values()
                        .any(|entry| entry.project_id == project.id);
                    if !already {
                        state.opening_projects.insert(project.id.clone());
                    }
                    break (project, already);
                };
                if already_open {
                    return Ok(json!({"project":project,"reattached":true}));
                }
                let opening = ProjectOpening {
                    daemon: self.clone(),
                    id: project.id.clone(),
                };
                let terminal = self.open(&json!({"project":project.id}))?;
                self.hooks(
                    project.hooks.clone(),
                    project.context.clone(),
                    std::env::vars().collect(),
                    "project-open",
                );
                drop(opening);
                Ok(json!({"project":project,"terminal":terminal}))
            }
            "project-config" => {
                let mut state = self.state.lock();
                let selector = request["project"]
                    .as_str()
                    .context("invalid-request: project required")?;
                let ids: Vec<_> = state
                    .store
                    .projects
                    .values()
                    .filter(|project| project.id == selector || project.name == selector)
                    .map(|project| project.id.clone())
                    .collect();
                let id = match ids.as_slice() {
                    [id] => id.clone(),
                    [] => bail!("not-found: project {selector}"),
                    _ => bail!("ambiguous: project {selector}"),
                };
                let project = state.store.configure(&id, request["config"].clone())?;
                Self::emit(&mut state, "project-configured", None, json!(project));
                self.changed.notify_all();
                Ok(json!(project))
            }
            "config" => {
                let mut state = self.state.lock();
                let config = state.store.configure_global(request["config"].clone())?;
                Self::emit(&mut state, "config", None, json!(config));
                self.changed.notify_all();
                Ok(json!(config))
            }
            "detach" => {
                let mut state = self.state.lock();
                state.detach_generation += 1;
                let generation = state.detach_generation;
                Self::emit(
                    &mut state,
                    "detach-requested",
                    None,
                    json!({"generation":generation}),
                );
                self.changed.notify_all();
                Ok(json!({"detach_requested":true,"generation":generation}))
            }
            "shutdown" => Ok(json!({"shutting_down":true})),
            _ => bail!("unknown-operation: {op}"),
        }
    }
}
fn managed_frontend_source(prepared: &crate::codex::Prepared) -> String {
    let mut source = String::with_capacity(
        prepared.command.len()
            + 9
            + prepared
                .env
                .iter()
                .map(|(key, value)| key.len() + value.len() + 4)
                .sum::<usize>(),
    );
    source.push_str("exec env");
    for (key, value) in &prepared.env {
        source.push_str(" '");
        for part in [key.as_str(), "=", value.as_str()] {
            for character in part.chars() {
                if character == '\'' {
                    source.push_str("'\\''");
                } else {
                    source.push(character);
                }
            }
        }
        source.push('\'');
    }
    source.push(' ');
    source.push_str(&prepared.command);
    source
}

fn validate_mutation_fields(request: &Value, allowed: &[&str]) -> Result<()> {
    let fields = request
        .as_object()
        .context("invalid-request: object required")?;
    if let Some(field) = fields
        .keys()
        .find(|field| !allowed.contains(&field.as_str()))
    {
        bail!("invalid-request: unknown mutation field {field}");
    }
    for field in ["persistent", "paste", "queue"] {
        if let Some(value) = fields.get(field)
            && !value.is_boolean()
        {
            bail!("invalid-request: {field} must be boolean");
        }
    }
    for field in ["terminal", "text", "key", "source"] {
        if let Some(value) = fields.get(field)
            && !value.is_string()
        {
            bail!("invalid-request: {field} must be a string");
        }
    }
    if fields.get("bytes").is_some_and(|value| !value.is_array()) {
        bail!("invalid-request: bytes must be an array");
    }
    Ok(())
}
fn key_bytes(key: &str, app_cursor: bool) -> Result<Vec<u8>> {
    let prefix = if app_cursor { "\x1bO" } else { "\x1b[" };
    let bytes = match key.to_ascii_lowercase().as_str() {
        "enter" => b"\r".to_vec(),
        "tab" => b"\t".to_vec(),
        "backspace" => vec![127],
        "escape" | "esc" => vec![27],
        "up" => format!("{prefix}A").into_bytes(),
        "down" => format!("{prefix}B").into_bytes(),
        "right" => format!("{prefix}C").into_bytes(),
        "left" => format!("{prefix}D").into_bytes(),
        "home" => b"\x1b[H".to_vec(),
        "end" => b"\x1b[F".to_vec(),
        "delete" => b"\x1b[3~".to_vec(),
        "pageup" => b"\x1b[5~".to_vec(),
        "pagedown" => b"\x1b[6~".to_vec(),
        control if control.starts_with("ctrl-") && control.len() == 6 => {
            vec![control.as_bytes()[5] & 0x1f]
        }
        _ => bail!("invalid-key: unsupported key {key}; use bytes for arbitrary sequences"),
    };
    Ok(bytes)
}
fn serve(daemon: Arc<Daemon>, stream: UnixStream, connection: u64) -> Result<()> {
    struct Departure(Arc<Daemon>, u64);
    impl Drop for Departure {
        fn drop(&mut self) {
            self.0.controls.lock().depart(self.1);
            self.0.flush_runtime_events();
        }
    }
    let _departure = Departure(daemon.clone(), connection);
    // Socket timeouts alone reset on each syscall; bound the whole frame even
    // when a client trickles bytes instead of completing a request.
    struct DeadlineIo {
        stream: UnixStream,
        deadline: Instant,
    }
    impl DeadlineIo {
        fn remaining(&self) -> std::io::Result<Duration> {
            self.deadline
                .checked_duration_since(Instant::now())
                .filter(|remaining| !remaining.is_zero())
                .ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::TimedOut, "client frame deadline")
                })
        }
    }
    impl Read for DeadlineIo {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            self.stream.set_read_timeout(Some(self.remaining()?))?;
            self.stream.read(bytes)
        }
    }
    impl Write for DeadlineIo {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.stream.set_write_timeout(Some(self.remaining()?))?;
            self.stream.write(bytes)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.stream.flush()
        }
    }
    // Darwin accept inherits listener O_NONBLOCK; workers use blocking I/O.
    stream.set_nonblocking(false)?;
    let mut reader = BufReader::new(DeadlineIo {
        stream: stream.try_clone()?,
        deadline: Instant::now() + Duration::from_secs(5),
    });
    let mut writer = BufWriter::with_capacity(
        16384,
        DeadlineIo {
            stream,
            deadline: Instant::now() + Duration::from_secs(2),
        },
    );
    let mut line = String::new();
    loop {
        if daemon.stopping.load(Ordering::Acquire) {
            return Ok(());
        }
        line.clear();
        reader.get_mut().deadline = Instant::now() + Duration::from_secs(5);
        // Borrow the reader so read-ahead survives each independently capped frame.
        if (&mut reader).take(1024 * 1024).read_line(&mut line)? == 0 {
            return Ok(());
        }
        let mut persistent = false;
        let result = if !line.ends_with('\n') {
            Err(anyhow::anyhow!(
                "invalid-frame: incomplete or oversized request"
            ))
        } else {
            serde_json::from_str::<Value>(&line)
                .map_err(anyhow::Error::from)
                .and_then(|request| {
                    persistent = request["persistent"] == true;
                    let result = daemon.dispatch(request, connection);
                    daemon.flush_runtime_events();
                    result
                })
        };
        let response = match result {
            Ok(result) => json!({"version":1,"ok":true,"result":result}),
            Err(error) => {
                let message = error.to_string();
                let code = message
                    .split_once(':')
                    .map(|(code, _)| code)
                    .filter(|code| code.len() < 64 && !code.contains(' '))
                    .unwrap_or("request-failed");
                json!({"version":1,"ok":false,"error":{"code":code,"message":message}})
            }
        };
        let shutdown = response["ok"] == true && response["result"]["shutting_down"] == true;
        writer.get_mut().deadline = Instant::now() + Duration::from_secs(2);
        let written = (|| -> Result<()> {
            serde_json::to_writer(&mut writer, &response)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
            Ok(())
        })();
        // Publish shutdown only after attempting the acknowledgement. The listener
        // can then disconnect slow clients without racing this response.
        if shutdown {
            daemon.begin_shutdown();
        }
        if shutdown || !persistent {
            return written;
        }
        written?;
    }
}

pub fn run(socket: &Path, projects_dir: Option<PathBuf>) -> Result<()> {
    let runtime = socket.parent().context("socket needs parent directory")?;
    client::private_dir(runtime)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(runtime.join("daemon.lock"))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        bail!("already-running: daemon runtime is locked");
    }
    if UnixStream::connect(socket).is_ok() {
        bail!("already-running: socket is active");
    }
    if socket.exists() {
        fs::remove_file(socket)?;
    }
    let listener = UnixListener::bind(socket)?;
    let _socket_file = SocketFile(socket.to_owned());
    fs::set_permissions(socket, fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let integration = Integration::new(runtime, socket, &std::env::current_exe()?)?;
    let store = Store::load(&client::config_path(), projects_dir)?;
    let startup = store.config.daemon_start_hooks.clone();
    let cwd = std::env::current_dir()?;
    let (hook_jobs, hook_receiver) = std::sync::mpsc::sync_channel::<Option<HookJob>>(64);
    let daemon = Arc::new(Daemon {
        state: Mutex::new(State {
            store,
            terminals: BTreeMap::new(),
            agent_views: BTreeMap::new(),
            opening_terminals: 0,
            seq: 0,
            detach_generation: 0,
            events: VecDeque::new(),
            event_bytes: 0,
            diagnostics: Vec::new(),
            observed: BTreeMap::new(),
            opening_projects: std::collections::BTreeSet::new(),
        }),
        changed: Condvar::new(),
        controls: Mutex::new(crate::control::Registry::new()),
        pending_launches: Mutex::new(BTreeMap::new()),
        codex: crate::codex::Broker::new(runtime)?,
        acp: crate::acp::Broker::new(),
        integration,
        git: GitCache::new(),
        stopping: AtomicBool::new(false),
        hook_jobs,
    });
    let weak = Arc::downgrade(&daemon);
    let hook_worker = thread::spawn(move || {
        while let Ok(Some(job)) = hook_receiver.recv() {
            for (index, hook) in job.hooks.iter().enumerate() {
                let Some(daemon) = weak.upgrade() else {
                    return;
                };
                if daemon.stopping.load(Ordering::Acquire) {
                    break;
                }
                let mut diagnostics = projects::run_hooks(
                    std::slice::from_ref(hook),
                    &job.cwd,
                    &job.env,
                    &daemon.stopping,
                );
                let mut state = daemon.state.lock();
                for diagnostic in &mut diagnostics {
                    diagnostic["index"] = index.into();
                    diagnostic["output_truncated"] = (diagnostic["output_truncated"] == true
                        || ["stdout", "stderr"].iter().any(|field| {
                            diagnostic[*field]
                                .as_str()
                                .is_some_and(|text| text.len() > DIAGNOSTIC_TEXT_LIMIT)
                        }))
                    .into();
                    bound_diagnostic(diagnostic);
                    let data = json!({"kind":job.kind,"cwd":job.cwd,"diagnostic":diagnostic});
                    Daemon::diagnostic(&mut state, "hook", None, data);
                }
                daemon.changed.notify_all();
            }
        }
    });
    // First shell exists regardless of hook failures.
    daemon.open(&json!({"cwd":cwd}))?;
    daemon.hooks(startup, cwd, std::env::vars().collect(), "daemon-start");
    unsafe {
        libc::signal(
            libc::SIGTERM,
            signal_stop as *const () as libc::sighandler_t,
        );
        libc::signal(libc::SIGINT, signal_stop as *const () as libc::sighandler_t);
    }
    let connections = Arc::new(Mutex::new(BTreeMap::<u64, UnixStream>::new()));
    let mut next_client = 0;
    let mut refresh = Instant::now();
    while !daemon.stopping.load(Ordering::Acquire) && !SIGNAL_STOP.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if connections.lock().len() >= 64 {
                    let _=stream.write_all(b"{\"version\":1,\"ok\":false,\"error\":{\"code\":\"busy\",\"message\":\"client capacity reached\"}}\n");
                    continue;
                }
                let connection = match stream.try_clone() {
                    Ok(connection) => connection,
                    Err(error) => {
                        eprintln!("amux client registration: {error}");
                        continue;
                    }
                };
                next_client += 1;
                connections.lock().insert(next_client, connection);
                let slot = ClientSlot {
                    connections: connections.clone(),
                    id: next_client,
                };
                let daemon = daemon.clone();
                if let Err(error) = thread::Builder::new().spawn(move || {
                    let _slot = slot;
                    let _ = serve(daemon, stream, next_client);
                }) {
                    eprintln!("amux client worker: {error}");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5))
            }
            Err(error) => {
                eprintln!("amux accept: {error}");
                thread::sleep(Duration::from_millis(10));
            }
        }
        if refresh.elapsed() >= Duration::from_millis(250) {
            refresh = Instant::now();
            daemon.controls.lock().tick(refresh);
            daemon.flush_runtime_events();
            let entries: Vec<_> = daemon.state.lock().terminals.values().cloned().collect();
            let workspaces: Vec<_> = entries
                .iter()
                .map(|entry| {
                    daemon
                        .integration
                        .captured_cwd(&entry.terminal.id)
                        .unwrap_or_else(|| entry.cwd.clone())
                })
                .collect();
            daemon.git.retain_workspaces(&workspaces);
            for entry in entries {
                let cwd = daemon
                    .integration
                    .captured_cwd(&entry.terminal.id)
                    .unwrap_or(entry.cwd);
                daemon.git.refresh(&cwd);
                let git = daemon.git.get(&cwd);
                let context = git["repository_root"]
                    .as_str()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| cwd.clone());
                let mut state = daemon.state.lock();
                let integration = daemon.integration.state(&entry.terminal.id);
                let mut visible =
                    json!({"shell":integration["shell"],"agent":integration["agent"],"git":git});
                if let Some(git) = visible["git"].as_object_mut() {
                    git.remove("age_ms");
                    git.remove("refreshed_at_ms");
                }
                if state.observed.get(&entry.terminal.id) != Some(&visible) {
                    state.observed.insert(entry.terminal.id.clone(), visible);
                    Daemon::emit(
                        &mut state,
                        "state-refreshed",
                        Some(&entry.terminal.id),
                        json!({"integration":integration,"workspace":{"cwd":cwd,"git":git}}),
                    );
                    daemon.changed.notify_all();
                }
                if let Some(current) = state.terminals.get_mut(&entry.terminal.id) {
                    current.cwd = cwd;
                }
                if let Some(project) = state.store.projects.get_mut(&entry.project_id)
                    && !project.saved
                    && project.main_terminal.as_deref() == Some(&entry.terminal.id)
                    && project.context != context
                {
                    project.context = context.clone();
                    Daemon::emit(
                        &mut state,
                        "project-context",
                        None,
                        json!({"project_id":entry.project_id,"context":context}),
                    );
                    daemon.changed.notify_all();
                }
            }
        }
    }
    daemon.begin_shutdown();
    for connection in connections.lock().values() {
        let _ = connection.shutdown(std::net::Shutdown::Both);
    }
    let entries: Vec<_> = daemon.state.lock().terminals.values().cloned().collect();
    for entry in entries {
        daemon.controls.lock().end_target(&entry.terminal.id);
        entry.terminal.close();
        daemon.integration.remove(&entry.terminal.id);
    }
    // Late opens own a request slot until their post-spawn shutdown check and
    // process cleanup finish. Socket shutdown unblocks slow readers/writers.
    while !connections.lock().is_empty() {
        thread::sleep(Duration::from_millis(5));
    }
    daemon.codex.shutdown();
    daemon.acp.shutdown();
    let _ = daemon.hook_jobs.send(None);
    let _ = hook_worker.join();
    drop(listener);
    Ok(())
}

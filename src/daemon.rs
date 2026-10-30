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

mod recovery;

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
    startup_ready: Option<std::sync::mpsc::Receiver<()>>,
}
struct Daemon {
    daemon_id: String,
    state: Mutex<State>,
    controls: Mutex<crate::control::Registry>,
    pending_launches: Mutex<BTreeMap<String, LaunchControl>>,
    codex: crate::codex::Broker,
    acp: crate::acp::Broker,
    changed: Condvar,
    integration: Arc<Integration>,
    git: GitCache,
    stopping: AtomicBool,
    startup_pending: AtomicBool,
    hook_jobs: std::sync::mpsc::SyncSender<Option<HookJob>>,
    recovery: Mutex<recovery::RecoveryState>,
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
struct StartupReady<'a>(&'a Daemon);
impl Drop for StartupReady<'_> {
    fn drop(&mut self) {
        self.0.startup_pending.store(false, Ordering::Release);
        self.0.changed.notify_all();
    }
}
struct SocketFile(PathBuf);
impl Drop for SocketFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn default_view_name(kind: &str, cwd: &Path, discriminator: &str) -> String {
    let leaf = cwd
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("/");
    let mut name = String::new();
    for part in [kind, " · ", leaf] {
        for c in part.chars() {
            let c = if crate::acp::safe_view_character(c) {
                c
            } else {
                '\u{fffd}'
            };
            if name.len() + c.len_utf8() > 220 {
                break;
            }
            name.push(c);
        }
    }
    name.push_str(" · ");
    name.push_str(discriminator);
    name
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
                json!({"daemon_id":self.daemon_id,"seq":state.seq,"detach_generation":state.detach_generation,
                "config":state.store.config,"projects":state.store.projects.values().collect::<Vec<_>>(),
                "terminals":terminals,"diagnostics":state.diagnostics,"project_save_terminal":true,
                "startup_pending":self.startup_pending.load(Ordering::Acquire)}),
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
            agents.push(json!({"id":id,"kind":"acp","name":metadata["name"],
                "project_id":view["project_id"],"instance_id":view["instance_id"],
                "connection_generation":metadata["target"]["connection_generation"],
                "session_id":metadata["target"]["session_id"],"lifecycle":metadata["lifecycle"],
                "configuration":metadata["configuration"].take(),
                "attention":metadata["attention"].take(),
                "ownership":ownership,"seq":metadata["seq"]}));
        }
        snapshot["agent_views"] = agents.into();
        Ok(snapshot)
    }
    /// Capture only in-memory source identity under topology. Both preview and
    /// save normalize this directory outside the lock, then reject source drift.
    fn project_save_source(&self, state: &State, request: &Value) -> Result<(String, PathBuf)> {
        let project = request
            .get("project")
            .map(|value| {
                let selector = value
                    .as_str()
                    .context("invalid-request: project selector must be a string")?;
                state.store.resolve(selector)
            })
            .transpose()?;
        if let Some(terminal) = request.get("terminal") {
            let selector = terminal
                .as_str()
                .context("invalid-request: terminal selector must be a string")?;
            let (id, cwd) = if let Some(view) = state.agent_views.get(selector) {
                (
                    view["project_id"]
                        .as_str()
                        .context("not-found: ACP project unavailable")?
                        .to_owned(),
                    PathBuf::from(
                        view["cwd"]
                            .as_str()
                            .context("not-found: ACP directory unavailable")?,
                    ),
                )
            } else {
                let terminal = Self::resolve(state, selector)?;
                let entry = &state.terminals[&terminal];
                (
                    entry.project_id.clone(),
                    self.integration
                        .captured_cwd(&terminal)
                        .unwrap_or_else(|| entry.cwd.clone()),
                )
            };
            ensure!(
                project.as_ref().is_none_or(|project| project == &id),
                "invalid-request: terminal belongs to a different project"
            );
            ensure!(
                state.store.projects.contains_key(&id),
                "not-found: project unavailable"
            );
            return Ok((id, cwd));
        }
        let id = project.unwrap_or_else(|| state.store.unsaved_id());
        let project = state
            .store
            .projects
            .get(&id)
            .context("not-found: project unavailable")?;
        let cwd = project
            .main_terminal
            .as_ref()
            .and_then(|terminal| state.terminals.get(terminal))
            .map(|entry| {
                self.integration
                    .captured_cwd(&entry.terminal.id)
                    .unwrap_or_else(|| entry.cwd.clone())
            })
            .unwrap_or_else(|| project.context.clone());
        Ok((id, cwd))
    }

    fn acp_open(
        &self,
        request: &Value,
        restored: Option<&crate::recovery::Record>,
    ) -> Result<Value> {
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
                "agent",
                "settings",
                "new_project",
            ],
        )?;
        let new_project = match request.get("new_project") {
            None => false,
            Some(value) => value
                .as_bool()
                .context("invalid-request: new_project must be boolean")?,
        };
        ensure!(
            !(request.get("agent").is_some() && request.get("command").is_some()),
            "invalid-request: agent profile and raw command are mutually exclusive"
        );
        let agent = request
            .get("agent")
            .map(|value| {
                value
                    .as_str()
                    .context("invalid-request: agent profile ID must be a string")
            })
            .transpose()?;
        let requested_project = request["project"]
            .as_str()
            .context("invalid-request: project ID required")?;
        let (project, default_cwd, profile) = {
            let state = self.state.lock();
            let profile = agent
                .map(|id| crate::acp_profiles::select(&state.store.config.acp_profiles, id))
                .transpose()?;
            let chosen = if let Some(project) = state.store.projects.get(requested_project) {
                project
            } else if profile.is_some() {
                let mut matches = state
                    .store
                    .projects
                    .values()
                    .filter(|project| project.name == requested_project);
                let project = matches
                    .next()
                    .context("project-not-found: project ID or name not found")?;
                ensure!(
                    matches.next().is_none(),
                    "project-ambiguous: exact project ID required"
                );
                project
            } else {
                bail!("project-not-found: exact project ID required");
            };
            let project = if new_project {
                Uuid::new_v4().to_string()
            } else {
                chosen.id.clone()
            };
            (project, chosen.context.clone(), profile)
        };
        // Renamed/generated labels already accepted by the broker remain valid
        // on internal reopening; keep the ordinary explicit launch bound.
        let name_limit = if restored.is_some() { 256 } else { 128 };
        let requested_name = match request.get("name") {
            None => None,
            Some(Value::String(name)) if name.len() <= name_limit => {
                crate::acp::validate_view_name(name).with_context(|| {
                    format!("invalid-request: ACP name must be a safe 1..{name_limit} byte label")
                })?;
                Some(name.as_str())
            }
            _ => bail!("invalid-request: ACP name must be a safe 1..{name_limit} byte label"),
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
        let default_name = requested_name.is_none().then(|| {
            let kind = profile
                .as_ref()
                .map_or("ACP", |profile| profile.label.as_str());
            let mut buffer = Uuid::encode_buffer();
            default_view_name(
                kind,
                &cwd,
                &Uuid::new_v4().simple().encode_lower(&mut buffer)[..8],
            )
        });
        let name = requested_name.unwrap_or_else(|| default_name.as_deref().expect("default name"));
        let (command, settings) = if let Some(mut profile) = profile {
            if let Some(record) = restored {
                // Saved acknowledged choices, not launch defaults, govern the
                // exact conversation being reopened. Reuse normal validation.
                profile.defaults = record.settings.clone();
                crate::acp_profiles::prepare(profile, None)?
            } else {
                crate::acp_profiles::prepare(profile, request.get("settings"))?
            }
        } else {
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
            (
                command,
                request
                    .get("settings")
                    .cloned()
                    .unwrap_or_else(|| json!({})),
            )
        };
        let launch_hash = agent.map(|_| recovery::launch_hash(&command));
        if let Some(record) = restored {
            ensure!(
                record.launch_hash == launch_hash,
                "recovery-unavailable: agent profile launch command changed; saved conversation retained"
            );
        }
        ensure!(
            !self.stopping.load(Ordering::Acquire),
            "daemon-stopping: cannot open ACP view"
        );
        let descriptor = Arc::new(self.acp.reserve(
            &project,
            name,
            &cwd,
            command,
            settings,
            restored.map(|record| record.session_id.clone()),
        )?);
        let id = descriptor["id"]
            .as_str()
            .context("acp-internal: missing reserved target")?;
        // Establish durable launch exclusion and the saved-record association
        // before another connection can observe and close this reservation.
        if let Some(record) = restored
            && let Err(error) = self.mark_reopen(record, true)
        {
            self.acp.end(id);
            self.acp.cleanup(id);
            if self.acp.discard_empty_ended(id) {
                self.flush_runtime_events();
            }
            return Err(error);
        }
        self.stage_acp_recovery(id, &project, name, &cwd, agent.zip(launch_hash), restored);
        let admitted = (|| {
            let mut state = self.state.lock();
            ensure!(
                !self.stopping.load(Ordering::Acquire),
                "daemon-stopping: cannot publish ACP view"
            );
            ensure!(
                new_project || state.store.projects.contains_key(&project),
                "context-changed: ACP project was removed before publication"
            );
            let mut controls = self.controls.lock();
            let lifetime = controls.add_acp_target(id)?;
            let observed = controls.snapshot(id)["observed"].clone();
            let grant = if restored.is_some() {
                // Loading saved context is not an input grant. A fresh human
                // attachment must acquire this new lifetime explicitly.
                json!({})
            } else {
                match controls.claim(&json!({
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
                }
            };
            // Publishing a reservation and its authority is one topology step.
            if new_project {
                state.store.projects.insert(
                    project.clone(),
                    projects::Project {
                        id: project.clone(),
                        name: "unsaved".into(),
                        context: cwd.clone(),
                        saved: false,
                        main_terminal: None,
                        hooks: Vec::new(),
                        recipes: Default::default(),
                    },
                );
            }
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
                self.abandon_recovery_target(id);
                if let Some(record) = restored
                    && let Err(rollback) = self.mark_reopen(record, false)
                {
                    self.recovery_diagnostic(Some(&record.id), &rollback);
                }
                if self.acp.discard_empty_ended(id) {
                    self.flush_runtime_events();
                }
                return Err(error);
            }
        };
        let started = self.acp.start(id, lifetime);
        if let Err(error) = started {
            self.controls.lock().end_acp(id, || self.acp.end(id))?;
            self.acp.cleanup(id);
            self.abandon_recovery_target(id);
            if self.acp.discard_empty_ended(id) {
                self.flush_runtime_events();
            }
            if new_project {
                let mut state = self.state.lock();
                let referenced = state
                    .terminals
                    .values()
                    .any(|entry| entry.project_id == project)
                    || state
                        .agent_views
                        .values()
                        .any(|view| view["project_id"] == project);
                if !referenced
                    && state
                        .store
                        .projects
                        .get(&project)
                        .is_some_and(|project| !project.saved)
                {
                    state.store.projects.remove(&project);
                }
            }
            if let Some(record) = restored
                && let Err(rollback) = self.mark_reopen(record, false)
            {
                self.recovery_diagnostic(Some(&record.id), &rollback);
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
            let forgotten = self.retire_recovery_target(id);
            self.acp.cleanup(id);
            forgotten?;
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
        self.checkpoint_recovery_before_shutdown();
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
        self.queue_hooks(HookJob {
            hooks,
            cwd,
            env,
            kind,
            startup_ready: None,
        });
    }
    fn queue_hooks(&self, job: HookJob) {
        if let Err(error) = self.hook_jobs.try_send(Some(job)) {
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
                        json!({"kind":job.kind,"message":format!("hook-backpressure: {message}"),
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
        self.abandon_recovery_target(terminal);
    }

    fn agent_open(
        self: &Arc<Self>,
        request: &Value,
        connection: u64,
        restored: Option<&crate::recovery::Record>,
    ) -> Result<Value> {
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
        if let Some(record) = restored {
            self.mark_reopen(record, true)?;
        }
        let prepared = match self.codex.prepare(&launch_request) {
            Ok(prepared) => prepared,
            Err(error) => {
                if let Some(record) = restored
                    && crate::codex::prepare_failure_settled(&error)
                    && let Err(rollback) = self.mark_reopen(record, false)
                {
                    self.recovery_diagnostic(Some(&record.id), &rollback);
                }
                return Err(error);
            }
        };
        let opened = match self.open_inner(
            request,
            context,
            Some((restored.is_none().then_some(controller), &prepared)),
            restored,
        ) {
            Ok(opened) => opened,
            Err(error) => {
                let settled = prepared.cleanup_failed_launch();
                if let Some(record) = restored
                    && settled
                    && let Err(rollback) = self.mark_reopen(record, false)
                {
                    self.recovery_diagnostic(Some(&record.id), &rollback);
                }
                return Err(error);
            }
        };
        if restored.is_some() {
            return Ok(json!({"terminal":opened.terminal,"instance_id":prepared.instance_id}));
        }
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
        let mut context = self.open_context(request)?;
        if request["new_project"] == true {
            context.project_id = Uuid::new_v4().to_string();
        }
        self.open_inner(request, context, None, None)
            .map(|opened| opened.terminal)
    }

    fn open_context(self: &Arc<Self>, request: &Value) -> Result<OpenContext> {
        if let Some(name) = request.get("name") {
            crate::acp::validate_view_name(
                name.as_str()
                    .context("invalid-name: name must be a string")?,
            )?;
        }
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
        managed: Option<(Option<&Value>, &crate::codex::Prepared)>,
        restored: Option<&crate::recovery::Record>,
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
            match self.launch_control(&id, managed.and_then(|(controller, _)| controller)) {
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
        let name = request["name"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| {
                default_view_name(
                    if managed.is_some() {
                        "Codex"
                    } else if request["omp"] == true {
                        "OMP"
                    } else {
                        "Shell"
                    },
                    &cwd,
                    &id[..8],
                )
            });
        self.stage_native_recovery(
            &id,
            &project_id,
            &name,
            &cwd,
            managed.and_then(|_| request["model"].as_str()),
            restored,
        );
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
                    if !daemon.stopping.load(Ordering::Acquire)
                        && !SIGNAL_STOP.load(Ordering::Relaxed)
                        && let Err(error) = daemon.native_recovery_exited(&exit_id)
                    {
                        daemon.recovery_diagnostic(Some(&exit_id), &error);
                    }
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
        let mut entry = Entry {
            terminal,
            name,
            project_id: project_id.clone(),
            parent_id: None,
            cwd: cwd.clone(),
            agent_instance,
        };
        // No fallible snapshot or preparation remains after publication. A
        // failed open therefore cannot conceal an admitted application launch.
        let mut result = match self.terminal_snapshot(&entry) {
            Ok(result) => result,
            Err(error) => {
                entry.terminal.close();
                self.discard_unpublished(&id, startup_control.as_ref());
                return Err(error);
            }
        };
        {
            let mut state = self.state.lock();
            if self.stopping.load(Ordering::Acquire) {
                drop(state);
                entry.terminal.close();
                self.discard_unpublished(&id, startup_control.as_ref());
                bail!("shutdown: daemon is stopping");
            }
            if request["new_project"] == true {
                state
                    .store
                    .projects
                    .entry(project_id.clone())
                    .or_insert_with(|| projects::Project {
                        id: project_id.clone(),
                        name: "unsaved".into(),
                        context: cwd.clone(),
                        saved: false,
                        main_terminal: None,
                        hooks: Vec::new(),
                        recipes: Default::default(),
                    });
            }
            let Some(project) = state.store.projects.get_mut(&project_id) else {
                drop(state);
                entry.terminal.close();
                self.discard_unpublished(&id, startup_control.as_ref());
                bail!("context-changed: project was removed before native publication");
            };
            entry.parent_id = project.main_terminal.clone();
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
        }
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
        result["parent_id"] = json!(entry.parent_id);
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
        let changed: std::collections::BTreeSet<_> = events
            .iter()
            .filter(|(kind, _, _)| kind.starts_with("agent-"))
            .map(|(_, terminal, _)| terminal.clone())
            .chain(
                agent_events
                    .iter()
                    .filter_map(|event| event["id"].as_str().map(str::to_owned)),
            )
            .collect();
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
            self.abandon_recovery_target(&id);
        }
        for id in changed {
            self.checkpoint_recovery(&id);
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
        if !matches!(op, "name" | "project-save" | "project-save-preview")
            && let Some(id) = request["terminal"].as_str()
            && self.state.lock().agent_views.contains_key(id)
        {
            bail!("wrong-target-kind: ACP views are not terminals");
        }
        match op {
            "acp-profiles" => {
                validate_mutation_fields(&request, &["version", "op", "persistent"])?;
                let profiles = self.state.lock().store.config.acp_profiles.clone();
                Ok(crate::acp_profiles::list(profiles))
            }
            "acp-open" => self.acp_open(&request, None),
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
            "agent-open" => self.agent_open(&request, connection, None),
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
                if request["event"]["kind"] == "agent"
                    && request["event"]["shutdown"] == true
                    && request["event"]["agent_kind"] == "main"
                {
                    if let Some(root) = request["event"]["instance_id"].as_str() {
                        self.forget_root_recovery(&id, root)?;
                    }
                } else {
                    self.checkpoint_recovery(&id);
                }
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
                let selector = request["terminal"]
                    .as_str()
                    .context("invalid-request: exact terminal or ACP view ID required")?;
                let name = request["name"]
                    .as_str()
                    .context("invalid-name: name required")?;
                crate::acp::validate_view_name(name)?;
                let is_acp = self.state.lock().agent_views.contains_key(selector);
                if is_acp {
                    let descriptor = self.acp.rename(selector, name)?;
                    let mut state = self.state.lock();
                    let view = state
                        .agent_views
                        .get_mut(selector)
                        .context("not-found: ACP view was removed")?;
                    *view = Arc::new(descriptor);
                    Self::emit(
                        &mut state,
                        "acp-named",
                        None,
                        json!({"target":{"kind":"acp","id":selector},"name":name}),
                    );
                } else {
                    let mut state = self.state.lock();
                    state
                        .terminals
                        .get_mut(selector)
                        .context("not-found: exact terminal or ACP view ID required")?
                        .name = name.to_owned();
                    Self::emit(
                        &mut state,
                        "terminal-named",
                        Some(selector),
                        json!({"name":name}),
                    );
                }
                self.changed.notify_all();
                self.checkpoint_recovery(selector);
                Ok(json!({"id":selector,"name":name}))
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
                let forgotten = self.retire_recovery_target(&entry.terminal.id);
                entry.terminal.close();
                forgotten?;
                Ok(entry.terminal.status())
            }
            "remove" => {
                let entry = self.entry(&request)?;
                if entry.terminal.alive() {
                    bail!("alive: close terminal before removing it");
                }
                self.controls.lock().end_target(&entry.terminal.id);
                let forgotten = self.retire_recovery_target(&entry.terminal.id);
                entry.terminal.close();
                forgotten?;
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
            "project-list" => {
                let state = self.state.lock();
                Ok(Value::Array(
                    state
                        .store
                        .projects
                        .values()
                        .filter(|project| project.saved)
                        .map(|project| json!(project))
                        .collect(),
                ))
            }
            "project-remove" => {
                let mut state = self.state.lock();
                let selector = request["project"]
                    .as_str()
                    .context("invalid-request: project selector required")?;
                let project = json!(state.store.remove_saved(selector)?);
                Self::emit(&mut state, "project-removed", None, project.clone());
                self.changed.notify_all();
                Ok(project)
            }
            "project-save" | "project-save-preview" => {
                let preview = op == "project-save-preview";
                validate_mutation_fields(
                    &request,
                    if preview {
                        &["version", "op", "persistent", "terminal", "project"]
                    } else {
                        &[
                            "version",
                            "op",
                            "persistent",
                            "terminal",
                            "project",
                            "name",
                            "expected_context",
                        ]
                    },
                )?;
                let expected = request
                    .get("expected_context")
                    .map(|value| {
                        let path = value.as_str().context(
                            "invalid-request: expected_context must be an absolute path string",
                        )?;
                        ensure!(
                            Path::new(path).is_absolute(),
                            "invalid-request: expected_context must be absolute"
                        );
                        Ok::<_, anyhow::Error>(Path::new(path))
                    })
                    .transpose()?;
                let source = self.project_save_source(&self.state.lock(), &request)?;
                let context = projects::resolve_workspace_context(&source.1)?;
                ensure!(
                    expected.is_none_or(|expected| expected == context.as_path()),
                    "context-changed: project directory changed since preview"
                );
                let mut state = self.state.lock();
                ensure!(
                    self.project_save_source(&state, &request)? == source,
                    "context-changed: project source changed during directory resolution"
                );
                if preview {
                    return Ok(json!({"project":source.0,"context":context}));
                }
                let name = request["name"]
                    .as_str()
                    .context("invalid-request: name required")?;
                let project = state.store.save(&source.0, name, Some(context))?;
                Self::emit(&mut state, "project-saved", None, json!(project));
                self.changed.notify_all();
                drop(state);
                if let Err(error) = self.sync_recovery_project(&project.id) {
                    self.recovery_diagnostic(
                        None,
                        &error.context("project saved but recovery metadata was not synchronized"),
                    );
                }
                Ok(json!(project))
            }
            "project-new" => {
                let project = {
                    let mut state = self.state.lock();
                    let project = state.store.create(
                        request["name"]
                            .as_str()
                            .context("invalid-request: name required")?,
                        PathBuf::from(
                            request["directory"]
                                .as_str()
                                .context("invalid-request: existing project directory required")?,
                        ),
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
                let (project, already_open, agent_views) = loop {
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
                    if already {
                        break (project, true, Vec::new());
                    }
                    let agent_views: Vec<_> = state
                        .agent_views
                        .values()
                        .filter(|view| view["project_id"] == project.id)
                        .cloned()
                        .collect();
                    state.opening_projects.insert(project.id.clone());
                    break (project, false, agent_views);
                };
                if already_open {
                    return Ok(json!({"project":project,"reattached":true}));
                }
                let opening = ProjectOpening {
                    daemon: self.clone(),
                    id: project.id.clone(),
                };
                // Keep project-open serialized, but release topology before
                // reading broker lifecycle, as in snapshot(). Descriptors can
                // outlive their agent; only a current live projection is reuse.
                if agent_views.iter().any(|view| {
                    view["id"].as_str().is_some_and(|id| {
                        self.acp.metadata(id).is_ok_and(|metadata| {
                            matches!(
                                metadata["lifecycle"].as_str(),
                                Some("starting" | "auth-required" | "ready")
                            )
                        })
                    })
                }) {
                    return Ok(json!({"project":project,"reattached":true}));
                }
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
                drop(state);
                if let Err(error) = self.sync_recovery_project(&project.id) {
                    self.recovery_diagnostic(
                        None,
                        &error.context(
                            "project configured but recovery metadata was not synchronized",
                        ),
                    );
                }
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
    let (recovery, recovery_error) = recovery::RecoveryState::open(&client::config_path(), socket);
    let native_store = recovery.agent_store();
    let startup = store.config.daemon_start_hooks.clone();
    let cwd = std::env::current_dir()?;
    let (hook_jobs, hook_receiver) = std::sync::mpsc::sync_channel::<Option<HookJob>>(64);
    let daemon = Arc::new(Daemon {
        daemon_id: Uuid::new_v4().simple().to_string(),
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
        codex: crate::codex::Broker::new(runtime, native_store.as_deref())?,
        acp: crate::acp::Broker::new(),
        integration,
        git: GitCache::new(),
        stopping: AtomicBool::new(false),
        startup_pending: AtomicBool::new(true),
        hook_jobs,
        recovery: Mutex::new(recovery),
    });
    if let Some(error) = recovery_error {
        daemon.recovery_diagnostic(None, &error);
    }
    let weak = Arc::downgrade(&daemon);
    let hook_worker = thread::spawn(move || {
        while let Ok(Some(job)) = hook_receiver.recv() {
            // Only deferred native startup reserves a gated FIFO position.
            // Wait without daemon/runtime guards; cancellation drops the sender.
            if let Some(ready) = job.startup_ready
                && ready.recv().is_err()
            {
                continue;
            }
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
    let records = daemon.recovery_records();
    let defer_initial_shell = records.iter().any(|record| {
        matches!(
            &record.agent,
            crate::recovery::Agent::Omp | crate::recovery::Agent::Codex { .. }
        )
    });
    let deferred_startup = if defer_initial_shell {
        // Native contexts already occupied these slots before restart. Do not
        // publish a disposable shell that either steals their quota or would
        // need to be taken away from a newly attached human.
        let ready = if startup.is_empty() {
            None
        } else {
            let (ready, receiver) = std::sync::mpsc::sync_channel(1);
            daemon.queue_hooks(HookJob {
                hooks: startup,
                cwd: cwd.clone(),
                env: std::env::vars().collect(),
                kind: "daemon-start",
                startup_ready: Some(receiver),
            });
            Some(ready)
        };
        Some((ready, cwd))
    } else {
        // Ordinary startup still has a shell before any hook can fail.
        daemon.open(&json!({"cwd":cwd}))?;
        daemon.hooks(startup, cwd, std::env::vars().collect(), "daemon-start");
        None
    };
    let reopen = Arc::clone(&daemon);
    let recovery_worker = thread::Builder::new()
        .name("conversation-reopen".into())
        .spawn(move || {
            let _ready = StartupReady(&reopen);
            reopen.reopen_conversations(records);
            if let Some((ready, cwd)) = deferred_startup {
                if reopen.stopping.load(Ordering::Acquire) || SIGNAL_STOP.load(Ordering::Relaxed) {
                    return;
                }
                let needs_shell = reopen.state.lock().terminals.is_empty();
                if needs_shell && let Err(error) = reopen.open(&json!({"cwd":cwd})) {
                    reopen.recovery_diagnostic(
                        None,
                        &error.context(
                            "initial shell unavailable after native conversation reopening",
                        ),
                    );
                    return;
                }
                if !reopen.stopping.load(Ordering::Acquire)
                    && !SIGNAL_STOP.load(Ordering::Relaxed)
                    && let Some(ready) = ready
                {
                    let _ = ready.send(());
                }
            }
        })?;
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
                let project_changed = if let Some(project) =
                    state.store.projects.get_mut(&entry.project_id)
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
                    true
                } else {
                    false
                };
                drop(state);
                if project_changed
                    && let Err(error) = daemon.sync_recovery_project(&entry.project_id)
                {
                    daemon.recovery_diagnostic(
                        None,
                        &error.context(
                            "project context changed but recovery metadata was not synchronized",
                        ),
                    );
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
    // Native setup owns a request slot too: publish its stop signal and join
    // its owned preparation before waiting for those handlers to depart.
    daemon.codex.shutdown();
    while !connections.lock().is_empty() {
        thread::sleep(Duration::from_millis(5));
    }
    let _ = recovery_worker.join();
    daemon.acp.shutdown();
    let _ = daemon.hook_jobs.send(None);
    let _ = hook_worker.join();
    drop(listener);
    Ok(())
}

//! Owned ACP v1 transport. Authority admission ends at the memory-only commit;
//! neither workers nor ownership transfer reinterpret an admitted action.
use agent_client_protocol_schema::{ProtocolVersion, v1 as acp};
use anyhow::{Result, bail, ensure};
use parking_lot::{Condvar, Mutex};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use uuid::Uuid;

const FRAME: usize = 1024 * 1024;
const QUEUE_BYTES: usize = 4 * 1024 * 1024;
const INGRESS: usize = 64;
const EGRESS: usize = 128;
const RPCS: usize = 64;
const LOCAL_CONTENT: usize = 64 * 1024;
const HISTORY: usize = 2048;
const COMPLETED: usize = 1024;
const CALLS: usize = 4096;
const STDERR: usize = 64 * 1024;

pub(crate) struct Broker {
    inner: Arc<Mutex<Instances>>,
}

struct Instances {
    targets: BTreeMap<String, Arc<Instance>>,
    removed: VecDeque<String>,
    stopped: bool,
}

struct Instance {
    id: String,
    instance_id: String,
    project_id: String,
    name: String,
    cwd: PathBuf,
    command: Vec<String>,
    state: Mutex<State>,
    wake: Condvar,
    worker: Mutex<Option<JoinHandle<()>>>,
    // Serializes joiners, never acquired by admission or a transport worker.
    cleanup: Mutex<()>,
    broker: Weak<Mutex<Instances>>,
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Target {
    kind: String,
    id: String,
    instance_id: String,
    connection_generation: u64,
    session_id: Option<String>,
}

pub(crate) struct PreparedAction {
    target: Target,
    controller: String,
    generation: u64,
    call_id: String,
    action: Action,
    work_id: String,
    prompt_id: String,
}

enum Action {
    Prompt(String),
    Authenticate(String),
    Interrupt(String),
    Close,
}

#[derive(Clone, Copy)]
enum RpcKind {
    Initialize,
    NewSession,
    Authenticate,
    Prompt,
}

struct Pending {
    kind: RpcKind,
    work_id: Option<String>,
}

struct Outgoing {
    bytes: Vec<u8>,
    work_id: Option<String>,
}

struct Work {
    id: String,
    call_id: String,
    controller: String,
    generation: u64,
    operation: &'static str,
    rpc_id: Option<acp::RequestId>,
    prompt_id: Option<String>,
    state: &'static str,
    delivery: &'static str,
    outcome: Option<Value>,
    done: bool,
}

struct State {
    lifecycle: &'static str,
    started: bool,
    joined: bool,
    session: Option<String>,
    seq: u64,
    dirty: bool,
    failure: Option<&'static str>,
    foreground: Option<(String, bool)>,
    calls: BTreeSet<String>,
    work: VecDeque<Work>,
    completed_evicted: u64,
    outstanding: BTreeMap<acp::RequestId, Pending>,
    next_rpc: i64,
    output: VecDeque<Outgoing>,
    output_count: usize,
    output_bytes: usize,
    input: VecDeque<Vec<u8>>,
    input_count: usize,
    input_bytes: usize,
    reader_done: bool,
    transport_error: Option<&'static str>,
    events: VecDeque<(usize, Value)>,
    event_bytes: usize,
    evicted_through: u64,
    auth: Vec<(String, &'static str)>,
    agent: Value,
    agent_content: Value,
    stderr: Vec<u8>,
    stderr_truncated: bool,
}

impl State {
    fn new() -> Self {
        Self {
            lifecycle: "starting",
            started: false,
            joined: false,
            session: None,
            seq: 0,
            dirty: true,
            failure: None,
            foreground: None,
            calls: BTreeSet::new(),
            work: VecDeque::new(),
            completed_evicted: 0,
            outstanding: BTreeMap::new(),
            next_rpc: 0,
            output: VecDeque::new(),
            output_count: 0,
            output_bytes: 0,
            input: VecDeque::new(),
            input_count: 0,
            input_bytes: 0,
            reader_done: false,
            transport_error: None,
            events: VecDeque::new(),
            event_bytes: 0,
            evicted_through: 0,
            auth: Vec::new(),
            agent: Value::Null,
            agent_content: Value::Null,
            stderr: Vec::new(),
            stderr_truncated: false,
        }
    }

    fn live(&self) -> bool {
        !matches!(self.lifecycle, "closing" | "ended" | "failed")
    }

    fn cleanup_complete(&self) -> bool {
        // Joining the owner cannot prove descendants gone when their identity
        // was never captured. Keep that lifetime charged and observable.
        self.joined && !matches!(self.failure, Some("cleanup" | "process-identity"))
    }

    fn changed(&mut self) {
        self.seq += 1;
        self.dirty = true;
    }

    fn event(&mut self, kind: &'static str, mut data: Value) {
        self.changed();
        data["kind"] = json!(kind);
        data["seq"] = json!(self.seq);
        let bytes = serde_json::to_vec(&data)
            .expect("JSON value serializes")
            .len();
        self.event_bytes += bytes;
        self.events.push_back((bytes, data));
        while self.events.len() > HISTORY || self.event_bytes > QUEUE_BYTES {
            if let Some((bytes, event)) = self.events.pop_front() {
                self.event_bytes -= bytes;
                self.evicted_through = event["seq"].as_u64().unwrap_or(self.seq);
            }
        }
    }

    fn trim_work(&mut self) {
        while self.work.iter().filter(|w| w.done).count() > COMPLETED {
            if let Some(index) = self.work.iter().position(|w| w.done) {
                self.work.remove(index);
                self.completed_evicted += 1;
            }
        }
    }

    fn close(&mut self, failure: Option<&'static str>) {
        if !self.live() {
            // Resource identity failure can arrive after an explicit close won.
            // Ending authority does not establish descendant cleanup.
            if matches!(failure, Some("process-identity" | "cleanup")) && self.failure != failure {
                self.failure = failure;
                self.changed();
            }
            return;
        }
        self.lifecycle = if failure.is_some() {
            "failed"
        } else {
            "closing"
        };
        self.failure = failure;
        self.foreground = None;
        for work in &mut self.work {
            if !work.done {
                work.state = "unknown";
                work.done = true;
            }
        }
        self.outstanding.clear();
        for frame in self.input.drain(..) {
            self.input_count -= 1;
            self.input_bytes -= frame.len();
        }
        // In-flight messages remain charged until their owners return.
        for frame in self.output.drain(..) {
            self.output_count -= 1;
            self.output_bytes -= frame.bytes.len();
        }
        self.trim_work();
        self.changed();
    }

    fn capacity(&self, frames: &[Outgoing], rpc: bool) -> Result<()> {
        ensure!(
            self.output_count + frames.len() <= EGRESS
                && self.output_bytes + frames.iter().map(|f| f.bytes.len()).sum::<usize>()
                    <= QUEUE_BYTES,
            "capacity-exceeded: ACP writer budget"
        );
        ensure!(
            !rpc || self.outstanding.len() < RPCS,
            "capacity-exceeded: ACP RPC budget"
        );
        Ok(())
    }

    fn enqueue(&mut self, frame: Outgoing) {
        self.output_count += 1;
        self.output_bytes += frame.bytes.len();
        self.output.push_back(frame);
    }

    fn rpc(
        &mut self,
        request: acp::ClientRequest,
        kind: RpcKind,
        work: Option<String>,
    ) -> Result<acp::RequestId> {
        let id = acp::RequestId::Number(self.next_rpc);
        let frame = request_frame(id.clone(), request, work.clone())?;
        self.capacity(std::slice::from_ref(&frame), true)?;
        self.next_rpc += 1;
        self.outstanding.insert(
            id.clone(),
            Pending {
                kind,
                work_id: work,
            },
        );
        self.enqueue(frame);
        Ok(id)
    }
}

impl Broker {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Instances {
                targets: BTreeMap::new(),
                removed: VecDeque::new(),
                stopped: false,
            })),
        }
    }

    pub(crate) fn reserve(
        &self,
        project_id: &str,
        name: &str,
        cwd: &Path,
        command: Vec<String>,
    ) -> Result<Value> {
        ensure!(
            cwd.is_absolute() && !command.is_empty() && Path::new(&command[0]).is_absolute(),
            "invalid-request: absolute ACP executable and cwd required"
        );
        ensure!(
            command.len() <= 256
                && command.iter().map(String::len).sum::<usize>() <= LOCAL_CONTENT
                && command.iter().all(|s| !s.contains('\0')),
            "invalid-request: ACP command budget"
        );
        ensure!(
            valid_id(project_id) && name.len() <= 256,
            "invalid-request: ACP descriptor budget"
        );
        let mut inner = self.inner.lock();
        ensure!(!inner.stopped, "target-ended: ACP broker stopped");
        // Reserve event space for every concurrently finishing instance's eviction.
        ensure!(
            inner.removed.len() < 48,
            "capacity-exceeded: ACP event drain required"
        );
        let mut ended = Vec::new();
        let mut live = 0;
        for (id, instance) in &inner.targets {
            let state = instance.state.lock();
            if state.cleanup_complete() {
                ended.push((state.seq, id.clone()));
            } else {
                live += 1;
            }
        }
        ensure!(live < 8, "capacity-exceeded: ACP target budget");
        ended.sort();
        while ended.len() > 8 {
            ensure!(
                inner.removed.len() < 64,
                "capacity-exceeded: ACP event drain required"
            );
            let (_, id) = ended.remove(0);
            inner.targets.remove(&id);
            inner.removed.push_back(id);
        }
        let id = fresh("acp");
        let instance = Arc::new(Instance {
            id: id.clone(),
            instance_id: fresh("instance"),
            project_id: project_id.to_owned(),
            name: inert(name),
            cwd: cwd.to_owned(),
            command,
            state: Mutex::new(State::new()),
            wake: Condvar::new(),
            worker: Mutex::new(None),
            cleanup: Mutex::new(()),
            broker: Arc::downgrade(&self.inner),
        });
        let descriptor = instance.descriptor(&instance.state.lock());
        inner.targets.insert(id, instance);
        Ok(descriptor)
    }

    pub(crate) fn start(&self, id: &str, lifetime: crate::control::AcpLifetime) -> Result<()> {
        let instance = self.instance(id)?;
        let mut handle = instance.worker.lock();
        lifetime.with_live(|| {
            let mut state = instance.state.lock();
            ensure!(
                state.live() && !state.started,
                "target-ended: ACP launch unavailable"
            );
            state.started = true;
            Ok(())
        })?;
        let owned = instance.clone();
        let life = lifetime.clone();
        match thread::Builder::new()
            .name("acp-session".into())
            .spawn(move || run(owned, life))
        {
            Ok(worker) => {
                *handle = Some(worker);
                Ok(())
            }
            Err(_) => {
                lifetime.end(|| {
                    instance.state.lock().close(Some("worker-start"));
                });
                instance.state.lock().joined = true;
                bail!("transport-failed: ACP worker start")
            }
        }
    }

    fn instance(&self, id: &str) -> Result<Arc<Instance>> {
        self.inner
            .lock()
            .targets
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("target-ended: ACP target absent"))
    }

    pub(crate) fn prepare(
        &self,
        op: &str,
        request: &Value,
        controller_id: &str,
    ) -> Result<PreparedAction> {
        let extra: &[&str] = match op {
            "acp-prompt" => &["text"],
            "acp-authenticate" => &["method_id"],
            "acp-interrupt" => &["prompt_id"],
            "acp-close" => &[],
            "acp-answer" | "acp-permission" => bail!("unsupported: ACP interactions unavailable"),
            _ => bail!("invalid-request: unknown ACP mutation"),
        };
        closed(
            request,
            &[
                "version",
                "op",
                "persistent",
                "target",
                "control",
                "owner_generation",
                "call_id",
            ],
            extra,
        )?;
        ensure!(
            request["op"] == op,
            "invalid-request: ACP operation mismatch"
        );
        let target = parse_target(request)?;
        ensure!(
            request.get("control").is_some(),
            "invalid-request: ACP control required"
        );
        let generation = request["owner_generation"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("invalid-request: ACP owner generation required"))?;
        let call_id = string(request, "call_id", 128)?.to_owned();
        ensure!(
            !call_id.is_empty(),
            "invalid-request: nonempty ACP call ID required"
        );
        ensure!(
            valid_id(controller_id),
            "invalid-request: ACP controller identity"
        );
        let action = match op {
            "acp-prompt" => Action::Prompt(string(request, "text", LOCAL_CONTENT)?.to_owned()),
            "acp-authenticate" => {
                Action::Authenticate(string(request, "method_id", 256)?.to_owned())
            }
            "acp-interrupt" => Action::Interrupt(string(request, "prompt_id", 256)?.to_owned()),
            _ => Action::Close,
        };
        Ok(PreparedAction {
            target,
            controller: controller_id.to_owned(),
            generation,
            call_id,
            action,
            work_id: fresh("work"),
            prompt_id: fresh("prompt"),
        })
    }

    // Called only under Authority::admit_acp. All fallible reservations precede
    // work/history/call-ID publication; no process, pipe, join or authority lookup.
    pub(crate) fn commit(
        &self,
        prepared: PreparedAction,
        checked_generation: u64,
    ) -> Result<Value> {
        ensure!(
            prepared.generation == checked_generation,
            "stale-control: ACP generation changed"
        );
        let instance = self.instance(&prepared.target.id)?;
        let mut state = instance.state.lock();
        instance.exact(&state, &prepared.target)?;
        ensure!(state.live(), "target-ended: ACP target ended");
        ensure!(
            !state.calls.contains(&prepared.call_id),
            "duplicate-call: inspect retained ACP work"
        );
        let closing = matches!(prepared.action, Action::Close);
        ensure!(
            state.calls.len() < if closing { CALLS } else { CALLS - 1 },
            "capacity-exceeded: ACP lifetime call budget"
        );
        let work_id = prepared.work_id;
        let mut prompt_id = None;
        let (operation, request, notification) = match &prepared.action {
            Action::Prompt(text) => {
                ensure!(
                    state.lifecycle == "ready",
                    "not-ready: ACP session unavailable"
                );
                ensure!(
                    state.foreground.is_none(),
                    "busy: ACP foreground prompt active"
                );
                let session = state
                    .session
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("not-ready: ACP session unavailable"))?;
                prompt_id = Some(prepared.prompt_id.clone());
                (
                    "acp-prompt",
                    Some((
                        acp::ClientRequest::PromptRequest(acp::PromptRequest::new(
                            acp::SessionId::new(session.as_str()),
                            vec![acp::ContentBlock::from(text.clone())],
                        )),
                        RpcKind::Prompt,
                    )),
                    None,
                )
            }
            Action::Authenticate(method) => {
                ensure!(
                    state.lifecycle == "auth-required",
                    "not-ready: ACP authentication not requested"
                );
                ensure!(
                    !state
                        .outstanding
                        .values()
                        .any(|p| matches!(p.kind, RpcKind::Authenticate)),
                    "busy: ACP authentication pending"
                );
                ensure!(
                    state
                        .auth
                        .iter()
                        .any(|(id, kind)| id == method && *kind == "agent"),
                    "unsupported: ACP authentication method unavailable"
                );
                (
                    "acp-authenticate",
                    Some((
                        acp::ClientRequest::AuthenticateRequest(acp::AuthenticateRequest::new(
                            acp::AuthMethodId::new(method.as_str()),
                        )),
                        RpcKind::Authenticate,
                    )),
                    None,
                )
            }
            Action::Interrupt(prompt) => {
                ensure!(
                    state
                        .foreground
                        .as_ref()
                        .is_some_and(|(id, cancelling)| id == prompt && !cancelling),
                    "stale-interaction: ACP prompt not interruptible"
                );
                let session = state
                    .session
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("not-ready: ACP session unavailable"))?;
                prompt_id = Some(prompt.clone());
                let notification = acp::Notification {
                    method: "session/cancel".into(),
                    params: Some(acp::ClientNotification::CancelNotification(
                        acp::CancelNotification::new(acp::SessionId::new(session.as_str())),
                    )),
                };
                (
                    "acp-interrupt",
                    None,
                    Some(encode(notification, Some(work_id.clone()))?),
                )
            }
            Action::Close => ("acp-close", None, None),
        };
        let rpc_id = if let Some((request, kind)) = request {
            Some(state.rpc(request, kind, Some(work_id.clone()))?)
        } else {
            if let Some(frame) = notification {
                state.capacity(std::slice::from_ref(&frame), false)?;
                state.enqueue(frame);
            }
            None
        };
        // No fallible operation below this point.
        state.calls.insert(prepared.call_id.clone());
        state.work.push_back(Work {
            id: work_id.clone(),
            call_id: prepared.call_id.clone(),
            controller: prepared.controller,
            generation: prepared.generation,
            operation,
            rpc_id,
            prompt_id: prompt_id.clone(),
            state: "accepted",
            delivery: "not-submitted",
            outcome: None,
            done: closing,
        });
        match &prepared.action {
            Action::Prompt(text) => {
                state.foreground = prompt_id.map(|id| (id, false));
                state.event(
                    "user-message",
                    json!({"text":inert(text),"work_id":work_id}),
                );
            }
            Action::Interrupt(_) => {
                if let Some((_, cancelling)) = &mut state.foreground {
                    *cancelling = true;
                }
            }
            Action::Close => state.close(None),
            Action::Authenticate(_) => {}
        }
        state.changed();
        let result = json!({"kind":"acp-admitted","call_id":prepared.call_id,"work_id":work_id,
            "state":"accepted","target":instance.target(&state),"owner_generation":checked_generation,"seq":state.seq});
        instance.wake.notify_all();
        Ok(result)
    }

    pub(crate) fn inspect(&self, request: &Value) -> Result<Value> {
        closed(
            request,
            &[
                "version",
                "op",
                "persistent",
                "target",
                "include_content",
                "after",
                "limit",
            ],
            &[],
        )?;
        let target = parse_target(request)?;
        let content = match request.get("include_content") {
            None => false,
            Some(value) => value.as_bool().ok_or_else(|| {
                anyhow::anyhow!("invalid-request: ACP include_content must be boolean")
            })?,
        };
        let after = optional_u64(request, "after", 0)?;
        let limit = optional_u64(request, "limit", 128)?;
        ensure!(
            (1..=128).contains(&limit),
            "invalid-request: ACP inspect limit"
        );
        let instance = self.instance(&target.id)?;
        let state = instance.state.lock();
        instance.exact(&state, &target)?;
        ensure!(after <= state.seq, "invalid-request: ACP cursor ahead");
        let mut result = instance.projection(&state);
        if content {
            let events: Vec<_> = state
                .events
                .iter()
                .filter(|(_, e)| e["seq"].as_u64().unwrap_or(0) > after)
                .take(limit as usize)
                .map(|(_, e)| e)
                .collect();
            let more = state
                .events
                .iter()
                .filter(|(_, e)| e["seq"].as_u64().unwrap_or(0) > after)
                .count()
                > events.len();
            let through = if more {
                events
                    .last()
                    .and_then(|e| e["seq"].as_u64())
                    .unwrap_or(after)
            } else {
                state.seq
            };
            result["content"] = json!({"after":after,"through":through,"gap":after < state.evicted_through,
                "events":events,"interactions":[],"agent":state.agent_content,"authentication":auth_projection(&state)});
        }
        Ok(result)
    }

    pub(crate) fn metadata(&self, id: &str) -> Result<Value> {
        let instance = self.instance(id)?;
        let state = instance.state.lock();
        Ok(instance.projection(&state))
    }

    pub(crate) fn drain_events(&self) -> Vec<Value> {
        let mut inner = self.inner.lock();
        let mut events: Vec<_> = inner
            .removed
            .drain(..)
            .map(|id| json!({"kind":"acp-removed","id":id,"data":{}}))
            .collect();
        for instance in inner.targets.values() {
            let mut state = instance.state.lock();
            if state.dirty {
                events.push(json!({"kind":"acp-updated","id":instance.id,"data":instance.projection(&state)}));
                state.dirty = false;
            }
        }
        events
    }

    pub(crate) fn end(&self, id: &str) {
        if let Ok(instance) = self.instance(id) {
            let mut state = instance.state.lock();
            state.close(None);
            if !state.started {
                state.joined = true;
                state.lifecycle = "ended";
            }
            instance.wake.notify_all();
            drop(state);
            prune_ended(&self.inner);
        }
    }

    pub(crate) fn cleanup(&self, id: &str) {
        if let Ok(instance) = self.instance(id) {
            let _joiner = instance.cleanup.lock();
            let handle = instance.worker.lock().take();
            if let Some(handle) = handle {
                let _ = handle.join();
            }
        }
    }

    /// Roll back an empty failed-open reservation only after outside-lock cleanup.
    /// A concurrent admitted close or uncertain cleanup must remain recoverable.
    pub(crate) fn discard_empty_ended(&self, id: &str) -> bool {
        let mut inner = self.inner.lock();
        let removable = inner.targets.get(id).is_some_and(|instance| {
            let state = instance.state.lock();
            !state.live() && state.cleanup_complete() && state.work.is_empty()
        });
        if removable {
            inner.targets.remove(id);
            inner.removed.push_back(id.to_owned());
        }
        removable
    }

    pub(crate) fn shutdown(&self) {
        let ids = {
            let mut inner = self.inner.lock();
            inner.stopped = true;
            inner.targets.keys().cloned().collect::<Vec<_>>()
        };
        // Parent ends registry lifetimes first; this is also safe for reservations
        // that never reached start. No callback into authority from broker state.
        for id in &ids {
            self.end(id);
        }
        for id in &ids {
            self.cleanup(id);
        }
    }
}

impl Instance {
    fn target(&self, state: &State) -> Target {
        Target {
            kind: "acp".into(),
            id: self.id.clone(),
            instance_id: self.instance_id.clone(),
            connection_generation: 1,
            session_id: state.session.clone(),
        }
    }

    fn exact(&self, state: &State, target: &Target) -> Result<()> {
        ensure!(
            *target == self.target(state),
            "stale-target: ACP tuple changed"
        );
        Ok(())
    }

    fn descriptor(&self, state: &State) -> Value {
        json!({"id":self.id,"kind":"acp","instance_id":self.instance_id,"connection_generation":1,
            "session_id":state.session,"project_id":self.project_id,"name":self.name,"lifecycle":state.lifecycle,"seq":state.seq})
    }

    fn projection(&self, state: &State) -> Value {
        let work: Vec<_> = state.work.iter().map(|w| json!({"work_id":w.id,"call_id":w.call_id,
            "controller_id":w.controller,"owner_generation":w.generation,"op":w.operation,
            "rpc_id":w.rpc_id,"prompt_id":w.prompt_id,"state":w.state,"delivery":w.delivery,"outcome":w.outcome})).collect();
        json!({"target":self.target(state),"lifecycle":state.lifecycle,"seq":state.seq,
            "foreground":state.foreground.as_ref().map(|(id,cancelling)| json!({"prompt_id":id,"state":if *cancelling {"cancelling"} else {"active"}})),
            "work":work,"interactions":[],"agent":state.agent,"authentication":auth_projection(state),
            "cursor":state.seq,"gap":state.evicted_through > 0,"truncation":{"through":state.evicted_through,"completed_work":state.completed_evicted},
            "resource":{"failure":state.failure,"stderr_truncated":state.stderr_truncated,
                "cleanup_complete":state.cleanup_complete()}})
    }

    fn transport_failed(&self, category: &'static str) {
        let mut state = self.state.lock();
        if state.transport_error.is_none() {
            state.transport_error = Some(category);
        }
        self.wake.notify_all();
    }
}

fn auth_projection(state: &State) -> Value {
    json!(
        state
            .auth
            .iter()
            .map(|(id, kind)| json!({"method_id":id,"kind":kind}))
            .collect::<Vec<_>>()
    )
}

fn reap_uncaptured(mut child: std::process::Child) -> std::io::Result<std::process::ExitStatus> {
    // Only for this unreaped direct child after our successful setsid. The
    // pinned leader prevents group-ID reuse; never signal its group after wait.
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
    child.wait()
}

fn run(instance: Arc<Instance>, lifetime: crate::control::AcpLifetime) {
    let mut command = Command::new(&instance.command[0]);
    command
        .args(&instance.command[1..])
        .current_dir(&instance.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Adapter inherits its explicit launch environment, never terminal/caller
    // integration state. No authentication installation or environment projection.
    for (key, _) in std::env::vars_os() {
        let key_text = key.to_string_lossy();
        if key_text.starts_with("AMUX_")
            || key_text.starts_with("OMP_")
            || key_text.starts_with("HERDR_")
            || matches!(
                key_text.as_ref(),
                "ZDOTDIR" | "PROMPT_COMMAND" | "BASH_ENV" | "ENV"
            )
        {
            command.env_remove(key);
        }
    }
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    // Closing before scheduling spawn should not create a child at all.
    if !instance.state.lock().live() {
        finish(&instance);
        return;
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            lifetime.end(|| instance.state.lock().close(Some("spawn")));
            finish(&instance);
            return;
        }
    };
    let Some(mut tree) = crate::runtime::ProcessTree::capture_owned_session(child.id() as i32)
    else {
        // No birth-qualified tree exists. Reap the pinned child/group, but leave
        // descendant cleanup explicitly uncertain and keep its resource charge.
        let _ = reap_uncaptured(child);
        lifetime.end(|| instance.state.lock().close(Some("process-identity")));
        finish(&instance);
        return;
    };
    let mut threads = [None, None, None];
    let publication = lifetime.with_live(|| {
        let mut state = instance.state.lock();
        ensure!(state.live(), "target-ended: ACP launch ended");
        state.rpc(
            acp::ClientRequest::InitializeRequest(acp::InitializeRequest::new(ProtocolVersion::V1)),
            RpcKind::Initialize,
            None,
        )?;
        state.changed();
        Ok(())
    });
    if publication.is_ok() {
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        if let (Some(stdin), Some(stdout), Some(stderr)) = (stdin, stdout, stderr) {
            let jobs = [
                thread::Builder::new().name("acp-writer".into()).spawn({
                    let instance = instance.clone();
                    move || writer(instance, stdin)
                }),
                thread::Builder::new().name("acp-reader".into()).spawn({
                    let instance = instance.clone();
                    move || reader(instance, stdout)
                }),
                thread::Builder::new().name("acp-stderr".into()).spawn({
                    let instance = instance.clone();
                    move || drain_stderr(instance, stderr)
                }),
            ];
            for (slot, job) in threads.iter_mut().zip(jobs) {
                match job {
                    Ok(handle) => *slot = Some(handle),
                    Err(_) => instance.transport_failed("worker-start"),
                }
            }
            consume(&instance, &lifetime);
        } else {
            lifetime.end(|| instance.state.lock().close(Some("stdio")));
        }
    }
    // No process identity is published until capture succeeds. This owner alone
    // terminates descendants, then reaps, then joins pipes; no post-reap killpg.
    tree.terminate();
    let reaped = child.wait().is_ok();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !tree.terminated() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    let terminated = tree.terminated();
    instance.wake.notify_all();
    let mut workers_joined = true;
    for handle in threads.into_iter().flatten() {
        workers_joined &= handle.join().is_ok();
    }
    if !reaped || !terminated || !workers_joined {
        let mut state = instance.state.lock();
        state.failure = Some("cleanup");
        state.lifecycle = "failed";
    }
    finish(&instance);
}

fn finish(instance: &Instance) {
    let mut state = instance.state.lock();
    if state.lifecycle != "failed" {
        state.lifecycle = "ended";
    }
    state.joined = true;
    state.changed();
    instance.wake.notify_all();
    drop(state);
    if let Some(broker) = instance.broker.upgrade() {
        prune_ended(&broker);
    }
}

fn prune_ended(broker: &Mutex<Instances>) {
    let mut inner = broker.lock();
    let mut ended: Vec<_> = inner
        .targets
        .iter()
        .filter_map(|(id, instance)| {
            let state = instance.state.lock();
            if state.cleanup_complete() {
                Some((state.seq, id.clone()))
            } else {
                None
            }
        })
        .collect();
    ended.sort();
    let excess = ended.len().saturating_sub(8);
    for (_, id) in ended.into_iter().take(excess) {
        inner.targets.remove(&id);
        inner.removed.push_back(id);
    }
}

fn writer(instance: Arc<Instance>, mut stdin: impl Write) {
    loop {
        let frame = {
            let mut state = instance.state.lock();
            while state.output.is_empty() && state.live() && state.transport_error.is_none() {
                instance.wake.wait(&mut state);
            }
            if !state.live() || state.transport_error.is_some() {
                return;
            }
            state.output.pop_front().expect("writer queue checked")
        };
        let result = stdin.write_all(&frame.bytes).and_then(|()| stdin.flush());
        let mut state = instance.state.lock();
        state.output_count -= 1;
        state.output_bytes -= frame.bytes.len();
        if let Some(id) = &frame.work_id {
            if let Some(work) = state.work.iter_mut().find(|w| &w.id == id) {
                // Delivery is evidence independent from the canonical response.
                work.delivery = if result.is_ok() {
                    "submitted"
                } else {
                    "write-failed"
                };
                if !work.done && result.is_ok() {
                    work.state = "submitted";
                }
                if work.operation == "acp-interrupt" && result.is_ok() {
                    work.done = true;
                }
            }
            state.trim_work();
            state.changed();
        }
        if result.is_err() && state.transport_error.is_none() {
            state.transport_error = Some("write");
        }
        instance.wake.notify_all();
        if result.is_err() {
            return;
        }
    }
}

// Bounded before extend, including the newline. EOF in the middle of a frame is
// not a successful message. BufReader itself has a fixed 8KiB read buffer.
fn read_frame(reader: &mut impl BufRead) -> std::io::Result<Option<Vec<u8>>> {
    let mut frame = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if frame.is_empty() {
                Ok(None)
            } else {
                Err(std::io::ErrorKind::UnexpectedEof.into())
            };
        }
        let newline = available.iter().position(|&b| b == b'\n');
        let count = newline.map_or(available.len(), |i| i + 1);
        if frame.len() + count > FRAME {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
        frame.extend_from_slice(&available[..count]);
        reader.consume(count);
        if newline.is_some() {
            return Ok(Some(frame));
        }
    }
}

fn reader(instance: Arc<Instance>, stdout: impl Read) {
    let mut reader = BufReader::new(stdout);
    loop {
        // Charge the reader-owned accumulation buffer before reading. Release
        // unused bytes once the exact frame size is known, not after dequeue.
        {
            let mut state = instance.state.lock();
            if !state.live() {
                return;
            }
            if state.input_count == INGRESS || state.input_bytes + FRAME > QUEUE_BYTES {
                state.transport_error = Some("ingress-capacity");
                instance.wake.notify_all();
                return;
            }
            state.input_count += 1;
            state.input_bytes += FRAME;
        }
        let next = read_frame(&mut reader);
        let mut state = instance.state.lock();
        state.input_count -= 1;
        state.input_bytes -= FRAME;
        if !state.live() {
            return;
        }
        match next {
            Ok(Some(frame)) => {
                state.input_count += 1;
                state.input_bytes += frame.len();
                state.input.push_back(frame);
            }
            Ok(None) => state.reader_done = true,
            Err(_) => state.transport_error = Some("frame"),
        }
        instance.wake.notify_all();
        if state.reader_done || state.transport_error.is_some() {
            return;
        }
    }
}

fn drain_stderr(instance: Arc<Instance>, mut stderr: impl Read) {
    let mut buffer = [0; 8192];
    loop {
        match stderr.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(count) => {
                let mut state = instance.state.lock();
                let retain = count.min(STDERR - state.stderr.len());
                state.stderr.extend_from_slice(&buffer[..retain]);
                if retain < count && !state.stderr_truncated {
                    state.stderr_truncated = true;
                    state.changed();
                }
            }
        }
    }
}

fn consume(instance: &Instance, lifetime: &crate::control::AcpLifetime) {
    loop {
        let next = {
            let mut state = instance.state.lock();
            while state.live()
                && state.input.is_empty()
                && !state.reader_done
                && state.transport_error.is_none()
            {
                instance.wake.wait(&mut state);
            }
            if !state.live() {
                return;
            }
            // Reader EOF/errors follow all complete preceding frames. A local
            // write failure must not erase an already-arrived exact RPC reply.
            match state.input.pop_front() {
                Some(frame) => frame,
                None => {
                    let error = state.transport_error.unwrap_or("eof");
                    drop(state);
                    lifetime.end(|| instance.state.lock().close(Some(error)));
                    instance.wake.notify_all();
                    return;
                }
            }
        };
        // Decoding occurs outside the admission lock. Queue charges include this
        // in-flight frame until its ordered state transition has completed.
        let decoded = serde_json::from_slice::<acp::JsonRpcMessage<Value>>(&next);
        let applied = match decoded {
            Ok(message) => lifetime.with_live(|| {
                let mut state = instance.state.lock();
                if !state.live() {
                    return Ok(());
                }
                apply(instance, &mut state, message.into_inner())
            }),
            Err(_) => Err(anyhow::anyhow!("transport-failed: malformed ACP message")),
        };
        {
            let mut state = instance.state.lock();
            state.input_count -= 1;
            state.input_bytes -= next.len();
        }
        if applied.is_err() {
            lifetime.end(|| {
                let mut state = instance.state.lock();
                let category = state.transport_error.unwrap_or("protocol");
                state.close(Some(category));
            });
            instance.wake.notify_all();
            return;
        }
        instance.wake.notify_all();
    }
}

fn apply(instance: &Instance, state: &mut State, mut message: Value) -> Result<()> {
    ensure!(
        message.is_object(),
        "transport-failed: invalid ACP envelope"
    );
    if let Some(method) = message.get("method") {
        let method = method
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("transport-failed: invalid ACP method"))?
            .to_owned();
        ensure!(
            method.len() <= 256
                && !message
                    .as_object()
                    .expect("object checked")
                    .contains_key("result")
                && !message
                    .as_object()
                    .expect("object checked")
                    .contains_key("error"),
            "transport-failed: invalid ACP envelope"
        );
        if message.get("id").is_some() {
            let request: acp::Request<Value> = decode(message)?;
            validate_rpc_id(&request.id)?;
            // No form/terminal/filesystem/URL capability in s01. Every incoming
            // request gets a reserved explicit response, including unknown methods.
            let response = if method == "session/request_permission"
                && state.foreground.as_ref().is_some_and(|(_, c)| *c)
            {
                match decode::<acp::RequestPermissionRequest>(request.params.unwrap_or(Value::Null))
                {
                    Ok(permission)
                        if state.session.as_deref() == Some(permission.session_id.0.as_ref()) =>
                    {
                        let result = acp::RequestPermissionResponse::new(
                            acp::RequestPermissionOutcome::Cancelled,
                        );
                        encode(acp::Response::new(request.id, Ok(result)), None)?
                    }
                    _ => encode(
                        acp::Response::<Value>::new(request.id, Err(acp::Error::invalid_params())),
                        None,
                    )?,
                }
            } else {
                let error = if method == "elicitation/create" {
                    acp::Error::invalid_params()
                } else {
                    acp::Error::method_not_found()
                };
                encode(acp::Response::<Value>::new(request.id, Err(error)), None)?
            };
            if state
                .capacity(std::slice::from_ref(&response), false)
                .is_err()
            {
                state.transport_error = Some("egress-capacity");
                bail!("transport-failed: ACP response capacity");
            }
            state.enqueue(response);
            state.changed();
            return Ok(());
        }
        if method == "session/update" {
            let update: acp::SessionNotification = decode(message["params"].take())?;
            ensure!(
                state.session.as_deref() == Some(update.session_id.0.as_ref()),
                "transport-failed: wrong update session"
            );
            match update.update {
                acp::SessionUpdate::UserMessageChunk(chunk) => {
                    content_event(state, "user-message", chunk)?
                }
                acp::SessionUpdate::AgentMessageChunk(chunk) => {
                    content_event(state, "agent-message", chunk)?
                }
                acp::SessionUpdate::AgentThoughtChunk(chunk) => {
                    content_event(state, "thought", chunk)?
                }
                acp::SessionUpdate::ToolCall(tool) => {
                    ensure!(
                        valid_id(tool.tool_call_id.0.as_ref()),
                        "transport-failed: invalid ACP tool identity"
                    );
                    state.event("tool", json!({"tool_call_id":tool.tool_call_id,"details":inert_value(serde_json::to_value(&tool)?)}));
                }
                acp::SessionUpdate::ToolCallUpdate(tool) => {
                    ensure!(
                        valid_id(tool.tool_call_id.0.as_ref()),
                        "transport-failed: invalid ACP tool identity"
                    );
                    state.event("tool", json!({"tool_call_id":tool.tool_call_id,"details":inert_value(serde_json::to_value(&tool)?)}));
                }
                acp::SessionUpdate::Plan(plan) => state.event(
                    "plan",
                    json!({"details":inert_value(serde_json::to_value(plan)?)}),
                ),
                _ => {} // Negotiated session metadata is not conversation content.
            }
        }
        return Ok(());
    }
    let response: acp::Response<Value> = decode(message)?;
    let (id, result) = match response {
        acp::Response::Result { id, result } => (id, Ok(result)),
        acp::Response::Error { id, error } => (id, Err(error)),
    };
    validate_rpc_id(&id)?;
    let pending = state
        .outstanding
        .remove(&id)
        .ok_or_else(|| anyhow::anyhow!("transport-failed: unretained ACP response"))?;
    let mut follow_up_error = None;
    let outcome = match (pending.kind, result) {
        (RpcKind::Initialize, Ok(value)) => {
            let raw_methods = value["authMethods"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default();
            ensure!(raw_methods.len() <= 64, "transport-failed: ACP auth budget");
            let agent_types: Vec<_> = raw_methods
                .iter()
                .map(|method| method.get("type").is_none_or(|kind| kind == "agent"))
                .collect();
            let initialized: acp::InitializeResponse = decode(value)?;
            ensure!(
                initialized.auth_methods.len() == agent_types.len(),
                "transport-failed: invalid ACP auth method"
            );
            ensure!(
                initialized.protocol_version == ProtocolVersion::V1,
                "transport-failed: incompatible ACP version"
            );
            let mut methods = BTreeSet::new();
            for (method, agent_type) in initialized.auth_methods.into_iter().zip(agent_types) {
                let id = method.id().0.to_string();
                ensure!(
                    valid_id(&id) && methods.insert(id.clone()),
                    "transport-failed: invalid ACP auth identity"
                );
                // The schema's forward-compatible agent fallback discards the
                // unknown type discriminator; preserve it from the bounded wire.
                let kind = match &method {
                    acp::AuthMethod::Agent(_) if agent_type => "agent",
                    acp::AuthMethod::Terminal(_) => "terminal",
                    _ => "unsupported",
                };
                state.auth.push((id, kind));
            }
            state.agent = json!({"protocol_version":1,"text_prompt":true,"forms":false,"terminal":false,"filesystem":false,
                "capabilities":{"load_session":initialized.agent_capabilities.load_session,
                    "prompt":{"image":initialized.agent_capabilities.prompt_capabilities.image,
                        "audio":initialized.agent_capabilities.prompt_capabilities.audio,
                        "embedded_context":initialized.agent_capabilities.prompt_capabilities.embedded_context}}});
            state.agent_content = json!({"identity":initialized.agent_info.map(|info| json!({"name":safe_label(&info.name),"version":safe_label(&info.version)})),
                "capabilities":state.agent});
            state.rpc(
                acp::ClientRequest::NewSessionRequest(acp::NewSessionRequest::new(&instance.cwd)),
                RpcKind::NewSession,
                None,
            )?;
            None
        }
        (RpcKind::NewSession, Ok(value)) => {
            let session: acp::NewSessionResponse = decode(value)?;
            let session_id = session.session_id.0.to_string();
            ensure!(
                valid_id(&session_id),
                "transport-failed: invalid ACP session identity"
            );
            state.session = Some(session_id);
            state.lifecycle = "ready";
            None
        }
        (RpcKind::NewSession, Err(error)) if i32::from(error.code) == -32000 => {
            state.lifecycle = "auth-required";
            None
        }
        (RpcKind::Authenticate, Ok(value)) => {
            let _: acp::AuthenticateResponse = decode(value)?;
            state.lifecycle = "starting";
            follow_up_error = state
                .rpc(
                    acp::ClientRequest::NewSessionRequest(acp::NewSessionRequest::new(
                        &instance.cwd,
                    )),
                    RpcKind::NewSession,
                    None,
                )
                .err();
            Some(json!({"kind":"response"}))
        }
        (RpcKind::Authenticate, Err(error)) => {
            state.lifecycle = "auth-required";
            Some(json!({"kind":"error","code":i32::from(error.code)}))
        }
        (RpcKind::Prompt, Ok(value)) => {
            let response: acp::PromptResponse = decode(value)?;
            let reason = serde_json::to_value(response.stop_reason)?;
            state.foreground = None;
            state.event(
                "completion",
                json!({"work_id":pending.work_id,"rpc_id":id,"stop_reason":reason}),
            );
            Some(json!({"kind":"response","stop_reason":reason}))
        }
        (RpcKind::Prompt, Err(error)) => {
            state.foreground = None;
            let code = i32::from(error.code);
            state.event(
                "error",
                json!({"work_id":pending.work_id,"rpc_id":id,"code":code}),
            );
            Some(json!({"kind":"error","code":code}))
        }
        _ => bail!("transport-failed: ACP bootstrap failed"),
    };
    if let Some(work_id) = pending.work_id
        && let Some(work) = state.work.iter_mut().find(|w| w.id == work_id)
    {
        work.done = true;
        work.state = if outcome.as_ref().is_some_and(|o| o["kind"] == "error") {
            "failed"
        } else {
            "completed"
        };
        work.outcome = outcome;
    }
    state.trim_work();
    state.changed();
    if let Some(error) = follow_up_error {
        return Err(error);
    }
    Ok(())
}

fn content_event(state: &mut State, kind: &'static str, chunk: acp::ContentChunk) -> Result<()> {
    if let Some(id) = &chunk.message_id {
        ensure!(
            valid_id(id.0.as_ref()),
            "transport-failed: invalid ACP message identity"
        );
    }
    let mut data = match chunk.content {
        acp::ContentBlock::Text(text) => json!({"text":inert(&text.text)}),
        other => json!({"content":inert_value(serde_json::to_value(other)?)}),
    };
    if let Some(id) = chunk.message_id {
        data["message_id"] = json!(id);
    }
    state.event(kind, data);
    Ok(())
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value)
        .map_err(|_| anyhow::anyhow!("transport-failed: invalid ACP payload"))
}

fn encode(message: impl Serialize, work_id: Option<String>) -> Result<Outgoing> {
    let mut bytes = serde_json::to_vec(&acp::JsonRpcMessage::wrap(message))
        .map_err(|_| anyhow::anyhow!("invalid-request: ACP encoding failed"))?;
    bytes.push(b'\n');
    ensure!(bytes.len() <= FRAME, "capacity-exceeded: ACP frame budget");
    Ok(Outgoing { bytes, work_id })
}

fn request_frame(
    id: acp::RequestId,
    request: acp::ClientRequest,
    work: Option<String>,
) -> Result<Outgoing> {
    encode(
        acp::Request {
            id,
            method: request.method().into(),
            params: Some(request),
        },
        work,
    )
}

fn validate_rpc_id(id: &acp::RequestId) -> Result<()> {
    ensure!(
        match id {
            acp::RequestId::Str(id) => valid_id(id),
            acp::RequestId::Number(_) | acp::RequestId::Null => true,
        },
        "transport-failed: invalid ACP RPC identity"
    );
    Ok(())
}

fn parse_target(request: &Value) -> Result<Target> {
    let target: Target = serde_json::from_value(request["target"].clone())
        .map_err(|_| anyhow::anyhow!("invalid-request: exact ACP tuple required"))?;
    ensure!(
        target.kind == "acp",
        "wrong-target-kind: ACP target required"
    );
    ensure!(
        valid_id(&target.id)
            && valid_id(&target.instance_id)
            && target.session_id.as_deref().is_none_or(valid_id),
        "invalid-request: ACP identity budget"
    );
    // Option deserialization accepts a missing field; exact tuple requires null.
    ensure!(
        request["target"].get("session_id").is_some(),
        "invalid-request: explicit ACP session required"
    );
    Ok(target)
}

fn closed(request: &Value, base: &[&str], extra: &[&str]) -> Result<()> {
    let object = request
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("invalid-request: ACP object required"))?;
    ensure!(
        object
            .keys()
            .all(|key| base.contains(&key.as_str()) || extra.contains(&key.as_str())),
        "invalid-request: unknown ACP field"
    );
    Ok(())
}

fn string<'a>(request: &'a Value, field: &str, max: usize) -> Result<&'a str> {
    let value = request[field]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("invalid-request: ACP string required"))?;
    ensure!(value.len() <= max, "invalid-request: ACP string budget");
    Ok(value)
}

fn optional_u64(request: &Value, field: &str, default: u64) -> Result<u64> {
    match request.get(field) {
        None => Ok(default),
        Some(value) => value
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("invalid-request: ACP unsigned integer required")),
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control)
}
fn fresh(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4().simple())
}

// Rendering remains data even for escape/control/bidi payloads. Newline/tab are
// intentional text layout; no terminal escape or invisible direction controls.
fn inert(text: &str) -> String {
    text.chars()
        .map(|c| {
            if (c.is_control() && c != '\n' && c != '\t')
                || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}'
                    | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            {
                '\u{fffd}'
            } else {
                c
            }
        })
        .collect()
}

fn safe_label(text: &str) -> String {
    let end = text
        .char_indices()
        .take_while(|(index, c)| index + c.len_utf8() <= 256)
        .last()
        .map_or(0, |(index, c)| index + c.len_utf8());
    inert(&text[..end])
}

fn inert_value(value: Value) -> Value {
    match value {
        Value::String(s) => json!(inert(&s)),
        Value::Array(values) => Value::Array(values.into_iter().map(inert_value).collect()),
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .filter(|(key, _)| key != "_meta")
                .map(|(key, value)| (inert(&key), inert_value(value)))
                .collect(),
        ),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn acp_framing_rejects_partial_oversized_and_preserves_next_frame() {
        let mut stream = Cursor::new(b"{}\n{\"id\":1}\n");
        assert_eq!(read_frame(&mut stream).unwrap().unwrap(), b"{}\n");
        assert_eq!(read_frame(&mut stream).unwrap().unwrap(), b"{\"id\":1}\n");
        assert!(read_frame(&mut stream).unwrap().is_none());
        assert!(read_frame(&mut Cursor::new(b"{}")).is_err());
        assert!(read_frame(&mut Cursor::new(vec![b'x'; FRAME + 1])).is_err());
        let mut exact = vec![b' '; FRAME];
        exact[FRAME - 1] = b'\n';
        assert_eq!(
            read_frame(&mut Cursor::new(exact)).unwrap().unwrap().len(),
            FRAME
        );
    }

    #[test]
    fn acp_inflight_writer_budget_cannot_be_reused_while_frame_is_owned() {
        let mut state = State::new();
        state.enqueue(Outgoing {
            bytes: vec![0; QUEUE_BYTES],
            work_id: None,
        });
        let _in_flight = state.output.pop_front().unwrap();
        assert!(
            state
                .capacity(
                    &[Outgoing {
                        bytes: vec![0],
                        work_id: None
                    }],
                    false
                )
                .is_err()
        );
    }

    fn ready_broker() -> (Broker, Arc<Instance>, Value) {
        let broker = Broker::new();
        let descriptor = broker
            .reserve(
                "project",
                "unit boundary",
                Path::new("/tmp"),
                vec!["/bin/false".into()],
            )
            .unwrap();
        let instance = broker.instance(descriptor["id"].as_str().unwrap()).unwrap();
        let target = {
            let mut state = instance.state.lock();
            state.lifecycle = "ready";
            state.session = Some("session".into());
            json!(instance.target(&state))
        };
        (broker, instance, target)
    }

    #[test]
    fn acp_consumed_startup_response_cannot_publish_after_lifetime_end() {
        let (broker, instance, _) = ready_broker();
        let mut registry = crate::control::Registry::new();
        let lifetime = registry.add_acp_target(&instance.id).unwrap();
        {
            let mut state = instance.state.lock();
            state.lifecycle = "starting";
            state.session = None;
            state.outstanding.insert(
                acp::RequestId::Number(77),
                Pending {
                    kind: RpcKind::NewSession,
                    work_id: None,
                },
            );
            let frame = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":77,
                "result":{"sessionId":"late-must-not-activate"}}))
            .unwrap();
            state.input_bytes = frame.len();
            state.input_count = 1;
            state.input.push_back(frame);
        }
        let mut consumer = None;
        // Hold the real target ordering point while the production consumer
        // dequeues a complete response. End the lifetime before it can apply.
        // This is a deterministic in-flight race, not a reply sent to a reaped peer.
        lifetime.end(|| {
            let instance_for_consumer = Arc::clone(&instance);
            let lifetime_for_consumer = lifetime.clone();
            consumer = Some(thread::spawn(move || {
                consume(&instance_for_consumer, &lifetime_for_consumer);
            }));
            let deadline = Instant::now() + Duration::from_secs(3);
            while !instance.state.lock().input.is_empty() {
                assert!(
                    Instant::now() < deadline,
                    "consumer did not dequeue response"
                );
                thread::yield_now();
            }
            let mut state = instance.state.lock();
            assert_eq!(state.input_count, 1, "in-flight frame remains charged");
            state.close(None);
        });
        consumer.unwrap().join().unwrap();
        // This memory-only race has no process worker to finish closing.
        let closed = broker.metadata(&instance.id).unwrap();
        assert_eq!(closed["lifecycle"], "closing");
        assert!(closed["target"]["session_id"].is_null());
        assert!(lifetime.with_live(|| Ok(())).is_err());
        let state = instance.state.lock();
        assert_eq!(state.input_count, 0);
        assert_eq!(state.input_bytes, 0);
    }

    fn prompt_request(target: &Value, call: &str) -> Value {
        json!({"op":"acp-prompt","target":target,"control":{"kind":"control","ref":"unit"},
            "owner_generation":7,"call_id":call,"text":"add 17 25"})
    }

    #[test]
    fn acp_failed_open_rollback_requires_empty_complete_ended_lifetime() {
        let (broker, _, target) = ready_broker();
        let id = target["id"].as_str().unwrap();
        assert!(!broker.discard_empty_ended(id));
        broker.end(id);
        broker.cleanup(id);
        assert!(broker.discard_empty_ended(id));
        assert!(broker.metadata(id).is_err());
        assert!(
            broker
                .drain_events()
                .iter()
                .any(|event| event["kind"] == "acp-removed" && event["id"] == id)
        );
    }

    #[test]
    fn acp_failed_open_rollback_preserves_admitted_close_and_uncertain_cleanup() {
        let (broker, _, target) = ready_broker();
        let id = target["id"].as_str().unwrap();
        let request = json!({"op":"acp-close","target":target,
            "control":{"kind":"control","ref":"unit"},"owner_generation":7,
            "call_id":"close-won-before-start"});
        let prepared = broker.prepare("acp-close", &request, "controller").unwrap();
        broker.commit(prepared, 7).unwrap();
        broker.end(id);
        broker.cleanup(id);
        assert!(!broker.discard_empty_ended(id));
        assert_eq!(
            broker.metadata(id).unwrap()["work"][0]["call_id"],
            "close-won-before-start"
        );

        let (uncertain, instance, target) = ready_broker();
        let id = target["id"].as_str().unwrap();
        uncertain.end(id);
        instance.state.lock().close(Some("process-identity"));
        uncertain.cleanup(id);
        assert!(!uncertain.discard_empty_ended(id));
        assert_eq!(
            uncertain.metadata(id).unwrap()["resource"]["cleanup_complete"],
            false
        );
    }

    #[test]
    fn acp_auth_response_survives_follow_up_session_capacity_failure() {
        let (broker, instance, _) = ready_broker();
        let target = {
            let mut state = instance.state.lock();
            state.lifecycle = "auth-required";
            state.session = None;
            state.auth.push(("agent-login".into(), "agent"));
            json!(instance.target(&state))
        };
        let request = json!({"op":"acp-authenticate","target":target,
            "control":{"kind":"control","ref":"unit"},"owner_generation":7,
            "call_id":"auth-before-egress-failure","method_id":"agent-login"});
        let prepared = broker
            .prepare("acp-authenticate", &request, "controller")
            .unwrap();
        broker.commit(prepared, 7).unwrap();
        {
            let mut state = instance.state.lock();
            let id = state.work[0].rpc_id.clone().unwrap();
            while state.output_count < EGRESS {
                state.enqueue(Outgoing {
                    bytes: b"{}\n".to_vec(),
                    work_id: None,
                });
            }
            let error = apply(
                &instance,
                &mut state,
                json!({"jsonrpc":"2.0","id":id,"result":{}}),
            )
            .unwrap_err();
            assert!(error.to_string().starts_with("capacity-exceeded:"));
            state.close(Some("protocol"));
        }
        let metadata = broker.metadata(target["id"].as_str().unwrap()).unwrap();
        assert_eq!(metadata["lifecycle"], "failed");
        assert_eq!(metadata["work"][0]["state"], "completed");
        assert_eq!(metadata["work"][0]["outcome"], json!({"kind":"response"}));
    }

    #[test]
    fn acp_generation_and_writer_capacity_rejection_do_not_commit_or_consume_call_id() {
        let (broker, instance, target) = ready_broker();
        let request = prompt_request(&target, "retry-only-after-local-rejection");
        let prepared = broker
            .prepare("acp-prompt", &request, "original-controller")
            .unwrap();
        assert!(
            broker
                .commit(prepared, 8)
                .unwrap_err()
                .to_string()
                .starts_with("stale-control:")
        );
        {
            let mut state = instance.state.lock();
            for _ in 0..EGRESS {
                state.enqueue(Outgoing {
                    bytes: vec![b'\n'],
                    work_id: None,
                });
            }
        }
        let prepared = broker
            .prepare("acp-prompt", &request, "original-controller")
            .unwrap();
        assert!(
            broker
                .commit(prepared, 7)
                .unwrap_err()
                .to_string()
                .starts_with("capacity-exceeded:")
        );
        {
            let mut state = instance.state.lock();
            assert!(state.calls.is_empty());
            assert!(state.work.is_empty());
            assert!(state.foreground.is_none());
            assert!(state.outstanding.is_empty());
            let released = state.output.pop_front().unwrap();
            state.output_count -= 1;
            state.output_bytes -= released.bytes.len();
        }
        let prepared = broker
            .prepare("acp-prompt", &request, "original-controller")
            .unwrap();
        let admitted = broker.commit(prepared, 7).unwrap();
        let projection = broker.metadata(&instance.id).unwrap();
        assert_eq!(projection["work"][0]["work_id"], admitted["work_id"]);
        assert_eq!(
            projection["work"][0]["controller_id"],
            "original-controller"
        );
    }

    #[test]
    fn acp_exact_lifetime_tombstones_survive_history_eviction_and_reserve_final_close() {
        let (broker, instance, target) = ready_broker();
        for n in 0..CALLS - 1 {
            let request = prompt_request(&target, &format!("call-{n}"));
            let prepared = broker
                .prepare("acp-prompt", &request, "original-controller")
                .unwrap();
            broker.commit(prepared, 7).unwrap();
            let mut state = instance.state.lock();
            let sent = state.output.pop_front().unwrap();
            state.output_count -= 1;
            state.output_bytes -= sent.bytes.len();
            let id = state.work.back().unwrap().rpc_id.clone().unwrap();
            apply(
                &instance,
                &mut state,
                json!({"id":id,"result":{"stopReason":"end_turn"}}),
            )
            .unwrap();
        }
        {
            let state = instance.state.lock();
            assert_eq!(state.work.len(), COMPLETED);
            assert_eq!(state.completed_evicted, (CALLS - 1 - COMPLETED) as u64);
            assert!(!state.work.iter().any(|w| w.call_id == "call-0"));
        }
        let old = prompt_request(&target, "call-0");
        let prepared = broker
            .prepare("acp-prompt", &old, "new-controller")
            .unwrap();
        assert!(
            broker
                .commit(prepared, 7)
                .unwrap_err()
                .to_string()
                .starts_with("duplicate-call:")
        );
        let overflow = prompt_request(&target, "one-too-many");
        let prepared = broker
            .prepare("acp-prompt", &overflow, "new-controller")
            .unwrap();
        assert!(
            broker
                .commit(prepared, 7)
                .unwrap_err()
                .to_string()
                .starts_with("capacity-exceeded:")
        );
        let close = json!({"op":"acp-close","target":target,"control":{"kind":"control","ref":"unit"},
            "owner_generation":7,"call_id":"final-explicit-close"});
        let prepared = broker
            .prepare("acp-close", &close, "new-controller")
            .unwrap();
        let admitted = broker.commit(prepared, 7).unwrap();
        let state = instance.state.lock();
        assert_eq!(state.calls.len(), CALLS);
        assert_eq!(state.lifecycle, "closing");
        assert_eq!(
            state.work.back().unwrap().id,
            admitted["work_id"].as_str().unwrap()
        );
    }

    #[test]
    fn acp_uncaptured_owned_group_is_reaped_without_signalling_other_processes() {
        use std::os::unix::fs::PermissionsExt;
        struct Children {
            owned: Option<std::process::Child>,
            sentinel: std::process::Child,
            root: std::path::PathBuf,
        }
        impl Drop for Children {
            fn drop(&mut self) {
                if let Some(child) = self.owned.take() {
                    let _ = reap_uncaptured(child);
                }
                let _ = self.sentinel.kill();
                let _ = self.sentinel.wait();
                let _ = std::fs::remove_dir_all(&self.root);
            }
        }
        let root = std::env::temp_dir().join(fresh("amux-uncaptured"));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut children = Children {
            owned: None,
            sentinel: Command::new("/bin/sleep")
                .arg("120")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
            root,
        };
        let mut command = Command::new("/bin/zsh");
        command
            .args([
                "-fc",
                "/bin/sleep 120 & print -r -- $! > descendant.pid; wait",
            ])
            .current_dir(&children.root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        children.owned = Some(command.spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(5);
        let descendant = loop {
            if let Ok(text) = std::fs::read_to_string(children.root.join("descendant.pid"))
                && let Ok(pid) = text.trim().parse::<i32>()
            {
                break pid;
            }
            assert!(Instant::now() < deadline, "owned descendant did not start");
            thread::sleep(Duration::from_millis(5));
        };
        let child = children.owned.take().unwrap();
        let root_pid = child.id() as i32;
        let status = reap_uncaptured(child).unwrap();
        assert!(!status.success());
        let mut raw_status = 0;
        assert_eq!(
            unsafe { libc::waitpid(root_pid, &mut raw_status, libc::WNOHANG) },
            -1
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if unsafe { libc::kill(descendant, 0) } == -1
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "uncaptured owned descendant survived"
            );
            thread::sleep(Duration::from_millis(5));
        }
        assert!(children.sentinel.try_wait().unwrap().is_none());
    }

    #[test]
    fn acp_capture_failure_after_close_retains_uncertain_cleanup() {
        let (broker, instance, target) = ready_broker();
        instance.state.lock().close(None);
        instance.state.lock().close(Some("process-identity"));
        finish(&instance);
        let resource = broker.metadata(target["id"].as_str().unwrap()).unwrap()["resource"].clone();
        assert_eq!(resource["failure"], "process-identity");
        assert_eq!(resource["cleanup_complete"], false);
    }

    #[test]
    fn acp_uncertain_capture_cleanup_keeps_capacity_and_is_not_complete() {
        let (broker, instance, target) = ready_broker();
        instance.state.lock().close(Some("process-identity"));
        finish(&instance);
        let id = target["id"].as_str().unwrap();
        assert_eq!(
            broker.metadata(id).unwrap()["resource"]["cleanup_complete"],
            false
        );
        // Repeated normal retirement must not evict an uncertain owned lifetime.
        for _ in 0..16 {
            let ended = broker
                .reserve(
                    "project",
                    "retired",
                    Path::new("/tmp"),
                    vec!["/bin/false".into()],
                )
                .unwrap();
            broker.end(ended["id"].as_str().unwrap());
        }
        assert_eq!(
            broker.metadata(id).unwrap()["resource"]["failure"],
            "process-identity"
        );
        for _ in 0..7 {
            broker
                .reserve(
                    "project",
                    "reserved",
                    Path::new("/tmp"),
                    vec!["/bin/false".into()],
                )
                .unwrap();
        }
        let error = broker
            .reserve(
                "project",
                "over capacity",
                Path::new("/tmp"),
                vec!["/bin/false".into()],
            )
            .unwrap_err();
        assert!(error.to_string().starts_with("capacity-exceeded:"));
    }

    #[test]
    fn acp_closed_identity_and_utf8_byte_boundaries_reject_before_admission() {
        let (broker, instance, target) = ready_broker();
        let mut request = prompt_request(&target, &"λ".repeat(64));
        request["text"] = json!(format!("add 17 25{}", " ".repeat(LOCAL_CONTENT - 9)));
        assert!(broker.prepare("acp-prompt", &request, "controller").is_ok());
        request["call_id"] = json!(format!("{}x", "λ".repeat(64)));
        assert!(
            broker
                .prepare("acp-prompt", &request, "controller")
                .is_err()
        );
        request["call_id"] = json!("content-overflow");
        request["text"] = json!("λ".repeat(LOCAL_CONTENT / 2 + 1));
        assert!(
            broker
                .prepare("acp-prompt", &request, "controller")
                .is_err()
        );
        request["text"] = json!("add 17 25");
        request["target"]
            .as_object_mut()
            .unwrap()
            .remove("session_id");
        assert!(
            broker
                .prepare("acp-prompt", &request, "controller")
                .is_err()
        );
        request["target"] = target;
        request["target"]["session_id"] = json!("different-session");
        let prepared = broker
            .prepare("acp-prompt", &request, "controller")
            .unwrap();
        assert!(
            broker
                .commit(prepared, 7)
                .unwrap_err()
                .to_string()
                .starts_with("stale-target:")
        );
        let state = instance.state.lock();
        assert!(state.calls.is_empty() && state.work.is_empty() && state.output.is_empty());
    }
}

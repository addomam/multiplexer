//! Owned ACP v1 transport. Authority admission ends at the memory-only commit;
//! neither workers nor ownership transfer reinterpret an admitted action.
//! Forms and tool permissions retain exact wire responders across authority and
//! parent-RPC completion. Interaction content (including selections) is bounded
//! separately at 4 MiB; only completed content can be evicted. Local form checks
//! cover the restricted flat ACP schema, not pattern/format or full JSON Schema.
use agent_client_protocol_schema::{ProtocolVersion, v1 as acp};
use anyhow::{Result, bail, ensure};
use parking_lot::{Condvar, Mutex};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use uuid::Uuid;

mod grok;

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
const INTERACTIONS: usize = 64;
const FIELDS: usize = 64;
const OPTIONS: usize = 256;
// Content projections include live responders and retained completed history.
pub(crate) const INTERACTION_RECORDS: usize = INTERACTIONS + COMPLETED;

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
    Answer(String, u64, FormAnswer),
    Permission(String, u64, String),
    Close,
}

enum FormAnswer {
    Accept(Value),
    Decline,
    Cancel,
}

struct Interaction {
    id: String,
    wire_id: acp::RequestId,
    kind: &'static str,
    grok_question: bool,
    session_scoped: bool,
    request_scope: Option<acp::RequestId>,
    parent_state: &'static str,
    detail: Value,
    bytes: usize,
    state: &'static str,
    selected_work: Option<String>,
    selection: Option<Value>,
    delivery: &'static str,
    responding: bool,
}

impl Interaction {
    fn metadata(&self) -> Value {
        json!({"interaction_id":self.id,"revision":1,"kind":self.kind,"state":self.state,
            "selected_work_id":self.selected_work,"delivery":self.delivery,"application":"unknown"})
    }

    fn content(&self) -> Value {
        let mut result = self.metadata();
        let object = result.as_object_mut().expect("metadata object");
        for (key, value) in self.detail.as_object().expect("interaction object") {
            object.insert(key.clone(), value.clone());
        }
        if let Some(selection) = &self.selection {
            result["selection"] = selection.clone();
        }
        if self.request_scope.is_some() {
            result["scope_parent_state"] = json!(self.parent_state);
        }
        result
    }

    fn affected_by_interrupt(&self, state: &State) -> bool {
        self.session_scoped
            || self.request_scope.as_ref().is_some_and(|id| {
                state
                    .outstanding
                    .get(id)
                    .is_some_and(|pending| matches!(pending.kind, RpcKind::Prompt))
            })
    }

    fn cancel_frame(&self, work: &str) -> Result<Outgoing> {
        let mut frame = if self.kind == "permission" {
            encode(
                acp::Response::new(
                    self.wire_id.clone(),
                    Ok(acp::RequestPermissionResponse::new(
                        acp::RequestPermissionOutcome::Cancelled,
                    )),
                ),
                Some(work.to_owned()),
            )?
        } else if self.grok_question {
            encode(
                acp::Response::new(self.wire_id.clone(), Ok(json!({"outcome":"cancelled"}))),
                Some(work.to_owned()),
            )?
        } else {
            encode(
                acp::Response::new(
                    self.wire_id.clone(),
                    Ok(acp::CreateElicitationResponse::new(
                        acp::ElicitationAction::Cancel,
                    )),
                ),
                Some(work.to_owned()),
            )?
        };
        frame.response_id = Some(self.wire_id.clone());
        Ok(frame)
    }
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
    response_id: Option<acp::RequestId>,
    response_write: Option<Arc<AtomicU8>>,
}

struct Responder {
    interaction: Option<String>,
    // 0: not finished, 1: written, 2: write failed. Publishing before taking
    // state prevents a fast peer's legal ID reuse racing writer bookkeeping.
    write: Arc<AtomicU8>,
}

struct Work {
    id: String,
    call_id: String,
    controller: String,
    generation: u64,
    operation: &'static str,
    rpc_id: Option<acp::RequestId>,
    prompt_id: Option<String>,
    interaction_id: Option<String>,
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
    // Includes queued/in-flight errors as well as selectable responders. A wire
    // ID cannot be reused until the actual response write has finished.
    responders: BTreeMap<acp::RequestId, Responder>,
    interactions: VecDeque<Interaction>,
    interaction_bytes: usize,
    interactions_evicted: u64,
    cancel_work: Option<String>,
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
            responders: BTreeMap::new(),
            interactions: VecDeque::new(),
            interaction_bytes: 0,
            interactions_evicted: 0,
            cancel_work: None,
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

    fn retain_capacity(&self, extra: usize) -> Result<()> {
        let live_bytes: usize = self
            .interactions
            .iter()
            .filter(|i| i.state == "pending" || i.responding)
            .map(|i| i.bytes)
            .sum();
        ensure!(
            live_bytes + extra <= QUEUE_BYTES,
            "capacity-exceeded: ACP interaction content budget"
        );
        Ok(())
    }

    fn trim_interactions(&mut self) {
        while self.interaction_bytes > QUEUE_BYTES
            || self
                .interactions
                .iter()
                .filter(|i| i.state != "pending" && !i.responding)
                .count()
                > COMPLETED
        {
            let Some(index) = self
                .interactions
                .iter()
                .position(|i| i.state != "pending" && !i.responding)
            else {
                break;
            };
            if let Some(interaction) = self.interactions.remove(index) {
                self.interaction_bytes -= interaction.bytes;
                self.interactions_evicted += 1;
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
        for interaction in &mut self.interactions {
            if interaction.request_scope.is_some() && interaction.parent_state == "outstanding" {
                interaction.parent_state = "unknown";
            }
            if interaction.state == "pending" {
                self.responders.remove(&interaction.wire_id);
                interaction.state = "unknown";
            } else if interaction.responding {
                // An owned in-flight response can still add delivery evidence.
                interaction.state = "unknown";
            }
        }
        for frame in self.input.drain(..) {
            self.input_count -= 1;
            self.input_bytes -= frame.len();
        }
        // In-flight messages remain charged until their owners return.
        for frame in self.output.drain(..) {
            self.output_count -= 1;
            self.output_bytes -= frame.bytes.len();
            if let Some(id) = &frame.response_id
                && let Some(Responder {
                    interaction: Some(interaction_id),
                    ..
                }) = self.responders.remove(id)
                && let Some(interaction) = self
                    .interactions
                    .iter_mut()
                    .find(|i| i.id == interaction_id)
            {
                interaction.responding = false;
            }
        }
        self.trim_work();
        self.trim_interactions();
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

    fn enqueue(&mut self, mut frame: Outgoing) {
        if let Some(id) = &frame.response_id {
            frame.response_write = Some(
                self.responders
                    .get(id)
                    .expect("reserved responder")
                    .write
                    .clone(),
            );
        }
        self.output_count += 1;
        self.output_bytes += frame.bytes.len();
        self.output.push_back(frame);
    }

    fn finish_response(&mut self, id: &acp::RequestId, write: &Arc<AtomicU8>) {
        let result = write.load(Ordering::Acquire);
        if result == 0
            || !self
                .responders
                .get(id)
                .is_some_and(|r| Arc::ptr_eq(&r.write, write))
        {
            return;
        }
        if let Some(Responder {
            interaction: Some(interaction_id),
            ..
        }) = self.responders.remove(id)
            && let Some(interaction) = self
                .interactions
                .iter_mut()
                .find(|i| i.id == interaction_id)
        {
            interaction.responding = false;
            interaction.delivery = if result == 1 {
                "submitted"
            } else {
                "write-failed"
            };
            if result == 2 && interaction.state != "unknown" {
                interaction.state = "failed";
            }
        }
        self.trim_interactions();
        self.changed();
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
            "acp-answer" => &["interaction_id", "revision", "action", "values"],
            "acp-permission" => &["interaction_id", "revision", "option_id"],
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
            "acp-answer" => {
                let id = string(request, "interaction_id", 256)?.to_owned();
                let revision = request["revision"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("invalid-request: ACP revision required"))?;
                let action = match request["action"].as_str() {
                    Some("accept") => {
                        let values =
                            request
                                .get("values")
                                .filter(|v| v.is_object())
                                .ok_or_else(|| {
                                    anyhow::anyhow!("invalid-request: ACP values object required")
                                })?;
                        ensure!(
                            value_bytes(values) <= LOCAL_CONTENT,
                            "invalid-request: ACP answer budget"
                        );
                        FormAnswer::Accept(values.clone())
                    }
                    Some("decline") if request.get("values").is_none() => FormAnswer::Decline,
                    Some("cancel") if request.get("values").is_none() => FormAnswer::Cancel,
                    _ => bail!("invalid-request: ACP answer action or values"),
                };
                Action::Answer(id, revision, action)
            }
            "acp-permission" => Action::Permission(
                string(request, "interaction_id", 256)?.to_owned(),
                request["revision"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("invalid-request: ACP revision required"))?,
                string(request, "option_id", 256)?.to_owned(),
            ),
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
        let mut selections = Vec::new();
        let mut frames = Vec::new();
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
                for (index, interaction) in state.interactions.iter().enumerate() {
                    if interaction.state == "pending" && interaction.affected_by_interrupt(&state) {
                        frames.push(interaction.cancel_frame(&work_id)?);
                        let selection = if interaction.kind == "permission" {
                            json!({"outcome":"cancelled"})
                        } else {
                            json!({"action":"cancel"})
                        };
                        selections.push((index, value_bytes(&selection), selection));
                    }
                }
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
            Action::Answer(id, revision, answer) => {
                let index = pending_interaction(&state, id, *revision, "form")?;
                let interaction = &state.interactions[index];
                let action = match answer {
                    FormAnswer::Accept(values) => {
                        validate_form_values(&interaction.detail["schema"], values)
                            .map_err(|category| anyhow::anyhow!("invalid-request: {category}"))?;
                        let content =
                            BTreeMap::<String, acp::ElicitationContentValue>::deserialize(values)
                                .map_err(|_| {
                                anyhow::anyhow!("invalid-request: ACP typed form values")
                            })?;
                        acp::ElicitationAction::Accept(
                            acp::ElicitationAcceptAction::new().content(content),
                        )
                    }
                    FormAnswer::Decline => acp::ElicitationAction::Decline,
                    FormAnswer::Cancel => acp::ElicitationAction::Cancel,
                };
                let response = acp::CreateElicitationResponse::new(action);
                let selection = serde_json::to_value(&response)?;
                selections.push((index, value_bytes(&selection), selection));
                let mut frame = if interaction.grok_question {
                    encode(
                        acp::Response::new(
                            interaction.wire_id.clone(),
                            Ok(grok::response(&interaction.detail["schema"], answer)?),
                        ),
                        Some(work_id.clone()),
                    )?
                } else {
                    encode(
                        acp::Response::new(interaction.wire_id.clone(), Ok(response)),
                        Some(work_id.clone()),
                    )?
                };
                frame.response_id = Some(interaction.wire_id.clone());
                ("acp-answer", None, Some(frame))
            }
            Action::Permission(id, revision, option) => {
                let index = pending_interaction(&state, id, *revision, "permission")?;
                let interaction = &state.interactions[index];
                ensure!(
                    interaction.detail["options"]
                        .as_array()
                        .is_some_and(|options| options
                            .iter()
                            .any(|offered| offered["option_id"] == option.as_str())),
                    "invalid-request: ACP permission option unavailable"
                );
                let response = acp::RequestPermissionResponse::new(
                    acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                        acp::PermissionOptionId::new(option.as_str()),
                    )),
                );
                let selection = json!({"option_id":option});
                selections.push((index, value_bytes(&selection), selection));
                let mut frame = encode(
                    acp::Response::new(interaction.wire_id.clone(), Ok(response)),
                    Some(work_id.clone()),
                )?;
                frame.response_id = Some(interaction.wire_id.clone());
                ("acp-permission", None, Some(frame))
            }
            Action::Close => ("acp-close", None, None),
        };
        if let Some(frame) = notification {
            // session/cancel precedes its mandatory response frames on the wire.
            frames.insert(0, frame);
        }
        state.capacity(&frames, request.is_some())?;
        let selection_bytes: usize = selections.iter().map(|(_, bytes, _)| bytes).sum();
        state.retain_capacity(selection_bytes)?;
        let interaction_id = selections
            .first()
            .map(|(index, _, _)| state.interactions[*index].id.clone());
        let response_rpc = if matches!(prepared.action, Action::Answer(..) | Action::Permission(..))
        {
            selections
                .first()
                .map(|(index, _, _)| state.interactions[*index].wire_id.clone())
        } else {
            None
        };
        let rpc_id = if let Some((request, kind)) = request {
            Some(state.rpc(request, kind, Some(work_id.clone()))?)
        } else {
            response_rpc
        };
        // No fallible operation below this point.
        for frame in frames {
            state.enqueue(frame);
        }
        for (index, bytes, selection) in selections {
            let interaction = &mut state.interactions[index];
            interaction.state = "selected";
            interaction.selected_work = Some(work_id.clone());
            interaction.selection = Some(selection);
            interaction.responding = true;
            interaction.bytes += bytes;
            state.interaction_bytes += bytes;
        }
        state.trim_interactions();
        state.calls.insert(prepared.call_id.clone());
        state.work.push_back(Work {
            id: work_id.clone(),
            call_id: prepared.call_id.clone(),
            controller: prepared.controller,
            generation: prepared.generation,
            operation,
            rpc_id,
            prompt_id: prompt_id.clone(),
            interaction_id,
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
                state.cancel_work = Some(work_id.clone());
                if let Some((_, cancelling)) = &mut state.foreground {
                    *cancelling = true;
                }
            }
            Action::Close => state.close(None),
            Action::Authenticate(_) | Action::Answer(..) | Action::Permission(..) => {}
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
            let interactions: Vec<_> = state
                .interactions
                .iter()
                .map(Interaction::content)
                .collect();
            result["content"] = json!({"after":after,"through":through,"gap":after < state.evicted_through,
                "events":events,"interactions":interactions,"agent":state.agent_content,"authentication":auth_projection(&state)});
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
        let work: Vec<_> = state
            .work
            .iter()
            .map(|w| {
                json!({"work_id":w.id,"call_id":w.call_id,
            "controller_id":w.controller,"owner_generation":w.generation,"op":w.operation,
            "rpc_id":w.rpc_id,"prompt_id":w.prompt_id,"interaction_id":w.interaction_id,
            "state":w.state,"delivery":w.delivery,"application":"unknown","outcome":w.outcome})
            })
            .collect();
        json!({"target":self.target(state),"lifecycle":state.lifecycle,"seq":state.seq,
            "foreground":state.foreground.as_ref().map(|(id,cancelling)| json!({"prompt_id":id,"state":if *cancelling {"cancelling"} else {"active"}})),
            "work":work,"interactions":state.interactions.iter().map(Interaction::metadata).collect::<Vec<_>>(),
            "agent":state.agent,"authentication":auth_projection(state),
            "cursor":state.seq,"gap":state.evicted_through > 0,
            "truncation":{"through":state.evicted_through,"completed_work":state.completed_evicted,"interactions":state.interactions_evicted},
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

// Keep the original endpoints owned until final cleanup classification, even
// when an idle or failed worker exits before cancellation. Dropping the wake
// sender interrupts every poll independently of peer EOF.
struct PipeCancellation {
    wake: UnixStream,
    stopped: AtomicBool,
    files: [File; 3],
}

#[derive(Clone, Copy)]
enum PipeEnd {
    Stdin,
    Stdout,
    Stderr,
}

impl PipeEnd {
    fn events(self) -> i16 {
        match self {
            Self::Stdin => libc::POLLOUT,
            Self::Stdout | Self::Stderr => libc::POLLIN,
        }
    }
}

impl PipeCancellation {
    fn nonblocking(&self) -> io::Result<()> {
        for file in &self.files {
            let fd = file.as_raw_fd();
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    fn hung_up(&self, end: PipeEnd) -> io::Result<bool> {
        let mut fd = libc::pollfd {
            fd: self.files[end as usize].as_raw_fd(),
            events: end.events(),
            revents: 0,
        };
        loop {
            if unsafe { libc::poll(&mut fd, 1, 0) } >= 0 {
                return Ok(fd.revents & (libc::POLLHUP | libc::POLLERR) != 0);
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
}

struct PipeIo {
    cancellation: Arc<PipeCancellation>,
    end: PipeEnd,
}

impl PipeIo {
    fn transfer(
        &mut self,
        mut operation: impl FnMut(&File) -> io::Result<usize>,
    ) -> io::Result<usize> {
        loop {
            if self.cancellation.stopped.load(Ordering::Acquire)
                && !self.cancellation.hung_up(self.end)?
            {
                // Interrupted would be retried by write_all, defeating cancellation.
                return Err(io::ErrorKind::ConnectionAborted.into());
            }
            // A completed syscall remains completed even if stop races with it.
            // Check cancellation before the next syscall, not after delivery.
            match operation(&self.cancellation.files[self.end as usize]) {
                Ok(count) => return Ok(count),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() != io::ErrorKind::WouldBlock => return Err(error),
                Err(_) => {}
            }
            let mut fds = [
                libc::pollfd {
                    fd: self.cancellation.files[self.end as usize].as_raw_fd(),
                    events: self.end.events(),
                    revents: 0,
                },
                libc::pollfd {
                    fd: self.cancellation.wake.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, -1) } < 0 {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
            }
        }
    }
}

impl Read for PipeIo {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.transfer(|mut file| file.read(buffer))
    }
}

impl Write for PipeIo {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.transfer(|mut file| file.write(buffer))
    }

    fn flush(&mut self) -> io::Result<()> {
        (&self.cancellation.files[self.end as usize]).flush()
    }
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
    let (wake, wake_sender) = match UnixStream::pair() {
        Ok(pair) => pair,
        Err(_) => {
            lifetime.end(|| instance.state.lock().close(Some("stdio")));
            finish(&instance);
            return;
        }
    };
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
    let cancellation = match (child.stdin.take(), child.stdout.take(), child.stderr.take()) {
        (Some(stdin), Some(stdout), Some(stderr)) => Some(Arc::new(PipeCancellation {
            wake,
            stopped: AtomicBool::new(false),
            files: [
                File::from(OwnedFd::from(stdin)),
                File::from(OwnedFd::from(stdout)),
                File::from(OwnedFd::from(stderr)),
            ],
        })),
        _ => None,
    };
    if cancellation
        .as_ref()
        .is_none_or(|pipes| pipes.nonblocking().is_err())
    {
        lifetime.end(|| instance.state.lock().close(Some("stdio")));
    }
    let mut threads = [None, None, None];
    let publication = lifetime.with_live(|| {
        let mut state = instance.state.lock();
        ensure!(state.live(), "target-ended: ACP launch ended");
        state.rpc(
            acp::ClientRequest::InitializeRequest(
                acp::InitializeRequest::new(ProtocolVersion::V1).client_capabilities(
                    acp::ClientCapabilities::new().elicitation(
                        acp::ElicitationCapabilities::new()
                            .form(acp::ElicitationFormCapabilities::new()),
                    ),
                ),
            ),
            RpcKind::Initialize,
            None,
        )?;
        state.changed();
        Ok(())
    });
    if let (Ok(()), Some(cancellation)) = (publication, &cancellation) {
        let stdin = PipeIo {
            cancellation: cancellation.clone(),
            end: PipeEnd::Stdin,
        };
        let stdout = PipeIo {
            cancellation: cancellation.clone(),
            end: PipeEnd::Stdout,
        };
        let stderr = PipeIo {
            cancellation: cancellation.clone(),
            end: PipeEnd::Stderr,
        };
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
    if let Some(cancellation) = &cancellation {
        cancellation.stopped.store(true, Ordering::Release);
    }
    drop(wake_sender);
    instance.wake.notify_all();
    let mut workers_joined = true;
    for handle in threads.into_iter().flatten() {
        workers_joined &= handle.join().is_ok();
    }
    let endpoints_closed = cancellation.as_ref().is_some_and(|pipes| {
        [PipeEnd::Stdin, PipeEnd::Stdout, PipeEnd::Stderr]
            .into_iter()
            .all(|end| pipes.hung_up(end).unwrap_or(false))
    });
    // Release owned descriptors before finish can release the instance charge.
    drop(cancellation);
    if !reaped || !terminated || !workers_joined || !endpoints_closed {
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
        if let Some(write) = &frame.response_write {
            write.store(if result.is_ok() { 1 } else { 2 }, Ordering::Release);
        }
        let mut state = instance.state.lock();
        state.output_count -= 1;
        state.output_bytes -= frame.bytes.len();
        if let (Some(wire_id), Some(write)) = (&frame.response_id, &frame.response_write) {
            state.finish_response(wire_id, write);
        }
        let final_frame = !state
            .output
            .iter()
            .any(|queued| queued.work_id == frame.work_id);
        if let Some(id) = &frame.work_id {
            if let Some(work) = state.work.iter_mut().find(|w| &w.id == id) {
                // Delivery is evidence independent from the canonical response.
                work.delivery = if result.is_ok() {
                    "submitted"
                } else {
                    "write-failed"
                };
                if !work.done && result.is_ok() && final_frame {
                    work.state = "submitted";
                }
                if matches!(
                    work.operation,
                    "acp-interrupt" | "acp-answer" | "acp-permission"
                ) && final_frame
                {
                    work.done = true;
                    if result.is_err() && work.state != "unknown" {
                        work.state = "failed";
                    }
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
        if let Err(error) = applied {
            lifetime.end(|| {
                let mut state = instance.state.lock();
                let category = state.transport_error.unwrap_or(
                    if error.to_string().starts_with("capacity-exceeded:") {
                        "capacity"
                    } else {
                        "protocol"
                    },
                );
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
            incoming_request(state, &method, request)?;
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
    // Completing an outbound RPC does not cancel its inbound responder. Keep
    // the exact scope until a deliberate response or connection closure.
    for interaction in &mut state.interactions {
        if interaction.request_scope.as_ref() == Some(&id) {
            interaction.parent_state = "completed";
        }
    }
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
            state.agent = json!({"protocol_version":1,"text_prompt":true,"forms":true,"terminal":false,"filesystem":false,
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
            state.cancel_work = None;
            state.event(
                "completion",
                json!({"work_id":pending.work_id,"rpc_id":id,"stop_reason":reason}),
            );
            Some(json!({"kind":"response","stop_reason":reason}))
        }
        (RpcKind::Prompt, Err(error)) => {
            state.foreground = None;
            state.cancel_work = None;
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

fn pending_interaction(state: &State, id: &str, revision: u64, kind: &str) -> Result<usize> {
    ensure!(revision == 1, "stale-interaction: ACP revision changed");
    let index = state
        .interactions
        .iter()
        .position(|i| i.id == id)
        .ok_or_else(|| anyhow::anyhow!("stale-interaction: ACP interaction absent"))?;
    let interaction = &state.interactions[index];
    ensure!(
        interaction.kind == kind
            && interaction.state == "pending"
            && state
                .responders
                .get(&interaction.wire_id)
                .and_then(|r| r.interaction.as_deref())
                == Some(id),
        "stale-interaction: ACP interaction not selectable"
    );
    Ok(index)
}

fn incoming_request(state: &mut State, method: &str, request: acp::Request<Value>) -> Result<()> {
    if let Some(responder) = state.responders.get(&request.id) {
        let result = responder.write.load(Ordering::Acquire);
        ensure!(
            result != 0,
            "transport-failed: ACP responder identity reused"
        );
        let write = responder.write.clone();
        state.finish_response(&request.id, &write);
        if result == 2 {
            state.transport_error = Some("write");
            bail!("transport-failed: ACP responder write failed");
        }
    }
    ensure!(
        state.responders.len() < INTERACTIONS,
        "capacity-exceeded: ACP responder budget"
    );
    let parsed = match method {
        "elicitation/create" => parse_form(state, request.params.unwrap_or(Value::Null)),
        "_x.ai/ask_user_question" => grok::form(request.params.unwrap_or(Value::Null))
            .and_then(|params| parse_form(state, params))
            .map(|(kind, session, scope, mut detail)| {
                detail["actions"] = json!(["accept", "cancel"]);
                (kind, session, scope, detail)
            }),
        "session/request_permission" => {
            parse_permission(state, request.params.unwrap_or(Value::Null))
        }
        _ => return error_response(state, request.id, acp::Error::method_not_found()),
    };
    let (kind, session_scoped, request_scope, detail) = match parsed {
        Ok(parsed) => parsed,
        Err(_) => {
            state.event(
                "error",
                json!({"code":-32602,"category":"unsupported-or-invalid-interaction"}),
            );
            return error_response(state, request.id, acp::Error::invalid_params());
        }
    };
    let mut interaction = Interaction {
        id: fresh("interaction"),
        wire_id: request.id.clone(),
        kind,
        grok_question: method == "_x.ai/ask_user_question",
        session_scoped,
        request_scope,
        parent_state: "outstanding",
        bytes: value_bytes(&detail),
        detail,
        state: "pending",
        selected_work: None,
        selection: None,
        delivery: "not-submitted",
        responding: false,
    };
    let cancelling = interaction.affected_by_interrupt(state)
        && state.foreground.as_ref().is_some_and(|(_, c)| *c);
    let response = if cancelling {
        let work = state
            .cancel_work
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("transport-failed: ACP cancel work absent"))?;
        let response = interaction.cancel_frame(work)?;
        state.capacity(std::slice::from_ref(&response), false)?;
        interaction.state = "selected";
        interaction.responding = true;
        interaction.selected_work = Some(work.to_owned());
        let selection = if kind == "permission" {
            json!({"outcome":"cancelled"})
        } else {
            json!({"action":"cancel"})
        };
        interaction.bytes += value_bytes(&selection);
        interaction.selection = Some(selection);
        Some(response)
    } else {
        None
    };
    state.retain_capacity(interaction.bytes)?;
    // Every budget, including mandatory cancellation, precedes publication.
    state.responders.insert(
        request.id,
        Responder {
            interaction: Some(interaction.id.clone()),
            write: Arc::new(AtomicU8::new(0)),
        },
    );
    state.interaction_bytes += interaction.bytes;
    state.interactions.push_back(interaction);
    if let Some(response) = response {
        state.enqueue(response);
    }
    state.trim_interactions();
    state.changed();
    Ok(())
}

fn error_response(state: &mut State, id: acp::RequestId, error: acp::Error) -> Result<()> {
    let mut frame = encode(acp::Response::<Value>::new(id.clone(), Err(error)), None)?;
    frame.response_id = Some(id.clone());
    if state.capacity(std::slice::from_ref(&frame), false).is_err() {
        state.transport_error = Some("egress-capacity");
        bail!("transport-failed: ACP response capacity");
    }
    state.responders.insert(
        id,
        Responder {
            interaction: None,
            write: Arc::new(AtomicU8::new(0)),
        },
    );
    state.enqueue(frame);
    state.changed();
    Ok(())
}

type ParsedInteraction = (&'static str, bool, Option<acp::RequestId>, Value);

fn parse_form(state: &State, mut params: Value) -> Result<ParsedInteraction> {
    ensure!(
        params["mode"] == "form",
        "unsupported: ACP elicitation mode"
    );
    validate_form_schema(&params["requestedSchema"])
        .map_err(|category| anyhow::anyhow!("unsupported: {category}"))?;
    // Make official omitted defaults explicit for both content consumers,
    // without copying the full schema or erasing any declared constraints.
    let schema = params["requestedSchema"]
        .as_object_mut()
        .expect("validated schema object");
    schema.entry("type").or_insert_with(|| json!("object"));
    schema.entry("properties").or_insert_with(|| json!({}));
    // Check before untagged/default-on-error decoding can choose one scope or
    // erase a malformed tool-call identity.
    ensure!(
        params.get("sessionId").is_some() != params.get("requestId").is_some(),
        "invalid-request: ACP elicitation scope"
    );
    if let Some(tool) = params.get("toolCallId").filter(|v| !v.is_null()) {
        ensure!(
            params.get("sessionId").is_some() && tool.as_str().is_some_and(valid_id),
            "invalid-request: ACP elicitation tool identity"
        );
    }
    let typed = acp::CreateElicitationRequest::deserialize(&params)
        .map_err(|_| anyhow::anyhow!("invalid-request: ACP form"))?;
    let (session_scoped, request_scope) = match typed.scope() {
        acp::ElicitationScope::Session(scope) => {
            ensure!(
                valid_id(scope.session_id.0.as_ref())
                    && state.session.as_deref() == Some(scope.session_id.0.as_ref()),
                "invalid-request: ACP form session"
            );
            (true, None)
        }
        acp::ElicitationScope::Request(scope) => {
            validate_rpc_id(&scope.request_id)?;
            ensure!(
                state.outstanding.contains_key(&scope.request_id)
                    && params.get("toolCallId").is_none(),
                "invalid-request: ACP form request scope"
            );
            (false, Some(scope.request_id.clone()))
        }
        _ => bail!("unsupported: ACP elicitation scope"),
    };
    let scope = serde_json::to_value(typed.scope())?;
    Ok((
        "form",
        session_scoped,
        request_scope,
        json!({
            "message":typed.message,"scope":scope,"schema":params["requestedSchema"].take(),
            "validation_limitations":["pattern and format are displayed but not locally validated; agent validation remains authoritative"]
        }),
    ))
}

fn parse_permission(state: &State, params: Value) -> Result<ParsedInteraction> {
    let permission: acp::RequestPermissionRequest = decode(params)?;
    ensure!(
        state.session.as_deref() == Some(permission.session_id.0.as_ref())
            && valid_id(permission.tool_call.tool_call_id.0.as_ref()),
        "invalid-request: ACP permission scope"
    );
    ensure!(
        !permission.options.is_empty() && permission.options.len() <= OPTIONS,
        "invalid-request: ACP permission options"
    );
    let mut ids = BTreeSet::new();
    let mut options = Vec::with_capacity(permission.options.len());
    for option in permission.options {
        ensure!(
            valid_id(option.option_id.0.as_ref()) && ids.insert(option.option_id.0.clone()),
            "invalid-request: ACP permission option identity"
        );
        ensure!(
            matches!(
                option.kind,
                acp::PermissionOptionKind::AllowOnce
                    | acp::PermissionOptionKind::AllowAlways
                    | acp::PermissionOptionKind::RejectOnce
                    | acp::PermissionOptionKind::RejectAlways
            ),
            "unsupported: ACP permission kind"
        );
        options.push(json!({"option_id":option.option_id,"name":option.name,"kind":option.kind}));
    }
    Ok((
        "permission",
        true,
        None,
        json!({
            "scope":{"sessionId":permission.session_id,"toolCallId":permission.tool_call.tool_call_id},
            "tool_call":permission.tool_call,"options":options
        }),
    ))
}

type Validation = std::result::Result<(), &'static str>;

fn schema_keys(value: &Value, allowed: &[&str]) -> Validation {
    let object = value.as_object().ok_or("invalid form schema")?;
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err("unsupported form schema");
    }
    for key in ["title", "description"] {
        if object
            .get(key)
            .is_some_and(|v| !v.is_null() && !v.is_string())
        {
            return Err("invalid form annotation");
        }
    }
    Ok(())
}

fn choices(list: &Value, titled: bool) -> std::result::Result<usize, &'static str> {
    let entries = list.as_array().ok_or("invalid form choices")?;
    if entries.is_empty() || entries.len() > OPTIONS {
        return Err("form option budget");
    }
    let mut seen = [""; OPTIONS];
    for (index, entry) in entries.iter().enumerate() {
        let value = if titled {
            schema_keys(entry, &["const", "title", "description", "_meta"])?;
            if !entry["title"].is_string() {
                return Err("invalid titled choice");
            }
            entry["const"].as_str()
        } else {
            entry.as_str()
        }
        .ok_or("invalid form choice")?;
        seen[index] = value;
    }
    seen[..entries.len()].sort_unstable();
    if seen[..entries.len()]
        .windows(2)
        .any(|pair| pair[0] == pair[1])
    {
        return Err("duplicate form choices");
    }
    Ok(entries.len())
}

fn optional<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|v| !v.is_null())
}

fn count_bounds(field: &Value, min: &str, max: &str, ceiling: u64) -> Validation {
    for key in [min, max] {
        if optional(field, key).is_some_and(|v| v.as_u64().is_none_or(|n| n > ceiling)) {
            return Err("invalid form count bound");
        }
    }
    if let (Some(min), Some(max)) = (optional(field, min), optional(field, max))
        && min.as_u64() > max.as_u64()
    {
        return Err("invalid form count range");
    }
    Ok(())
}

fn validate_form_schema(schema: &Value) -> Validation {
    schema_keys(
        schema,
        &[
            "type",
            "title",
            "description",
            "properties",
            "required",
            "_meta",
        ],
    )?;
    if schema.get("type").is_some_and(|v| v != "object") {
        return Err("invalid form root type");
    }
    let properties = schema
        .get("properties")
        .map(|v| v.as_object().ok_or("invalid form properties"))
        .transpose()?;
    let field_count = properties.map_or(0, serde_json::Map::len);
    if field_count > FIELDS {
        return Err("form field budget");
    }
    if let Some(required) = optional(schema, "required") {
        let required = required.as_array().ok_or("invalid required fields")?;
        if required.len() > field_count {
            return Err("invalid required fields");
        }
        for (index, name) in required.iter().enumerate() {
            if !name
                .as_str()
                .is_some_and(|name| properties.is_some_and(|p| p.contains_key(name)))
                || required[..index].contains(name)
            {
                return Err("invalid required fields");
            }
        }
    }
    let mut option_count = 0;
    for field in properties.into_iter().flat_map(|p| p.values()) {
        let common = ["type", "title", "description", "default", "_meta"];
        let kind = field["type"].as_str().ok_or("invalid form field type")?;
        let extra: &[&str] = match kind {
            "string" => &[
                "minLength",
                "maxLength",
                "pattern",
                "format",
                "enum",
                "oneOf",
            ],
            "number" | "integer" => &["minimum", "maximum"],
            "boolean" => &[],
            "array" => &["minItems", "maxItems", "items"],
            _ => return Err("unsupported form field"),
        };
        let object = field.as_object().ok_or("invalid form field")?;
        if object
            .keys()
            .any(|key| !common.contains(&key.as_str()) && !extra.contains(&key.as_str()))
        {
            return Err("unsupported form constraint");
        }
        for annotation in ["title", "description"] {
            if optional(field, annotation).is_some_and(|v| !v.is_string()) {
                return Err("invalid form annotation");
            }
        }
        match kind {
            "string" => {
                count_bounds(field, "minLength", "maxLength", u32::MAX as u64)?;
                if optional(field, "pattern").is_some_and(|v| !v.is_string()) {
                    return Err("invalid form pattern");
                }
                if optional(field, "format").is_some_and(|v| {
                    !matches!(v.as_str(), Some("email" | "uri" | "date" | "date-time"))
                }) {
                    return Err("unsupported form format");
                }
                for (key, titled) in [("enum", false), ("oneOf", true)] {
                    if let Some(list) = optional(field, key) {
                        option_count += choices(list, titled)?;
                    }
                }
                if let (Some(plain), Some(titled)) =
                    (optional(field, "enum"), optional(field, "oneOf"))
                {
                    let plain = plain.as_array().ok_or("invalid form choices")?;
                    let titled = titled.as_array().ok_or("invalid form choices")?;
                    if !plain
                        .iter()
                        .any(|v| titled.iter().any(|o| o["const"] == *v))
                    {
                        return Err("empty form choice intersection");
                    }
                }
            }
            "integer" | "number" => {
                for key in ["minimum", "maximum"] {
                    if let Some(bound) = optional(field, key) {
                        if kind == "integer" {
                            if bound.as_i64().is_none() {
                                return Err("invalid integer bound");
                            }
                        } else if !bound.as_f64().is_some_and(f64::is_finite) {
                            return Err("invalid number bound");
                        }
                    }
                }
                if let (Some(min), Some(max)) =
                    (optional(field, "minimum"), optional(field, "maximum"))
                {
                    let reversed = if kind == "integer" {
                        min.as_i64() > max.as_i64()
                    } else {
                        min.as_f64() > max.as_f64()
                    };
                    if reversed {
                        return Err("invalid numeric range");
                    }
                }
            }
            "array" => {
                count_bounds(field, "minItems", "maxItems", u64::MAX)?;
                let items = &field["items"];
                schema_keys(items, &["type", "enum", "anyOf", "_meta"])?;
                if items.get("type").is_some_and(|v| v != "string") {
                    return Err("unsupported multiselect items");
                }
                let (list, titled) = match (items.get("enum"), items.get("anyOf")) {
                    (Some(list), None) if items["type"] == "string" => (list, false),
                    (None, Some(list)) => (list, true),
                    _ => return Err("unsupported multiselect items"),
                };
                let count = choices(list, titled)?;
                option_count += count;
                if optional(field, "minItems")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    > count as u64
                {
                    return Err("impossible multiselect minimum");
                }
            }
            _ => {}
        }
        if option_count > OPTIONS {
            return Err("form option budget");
        }
        if let Some(default) = optional(field, "default") {
            validate_field_value(field, default)?;
        }
    }
    Ok(())
}

fn validate_field_value(field: &Value, value: &Value) -> Validation {
    match field["type"].as_str() {
        Some("string") => {
            let text = value.as_str().ok_or("form string required")?;
            let count = text.chars().count() as u64;
            if optional(field, "minLength")
                .and_then(Value::as_u64)
                .is_some_and(|n| count < n)
                || optional(field, "maxLength")
                    .and_then(Value::as_u64)
                    .is_some_and(|n| count > n)
            {
                return Err("form string length");
            }
            if optional(field, "enum")
                .and_then(Value::as_array)
                .is_some_and(|options| !options.iter().any(|o| o == value))
                || optional(field, "oneOf")
                    .and_then(Value::as_array)
                    .is_some_and(|options| !options.iter().any(|o| o["const"] == *value))
            {
                return Err("form choice unavailable");
            }
        }
        Some("integer") => {
            let number = value.as_i64().ok_or("form i64 integer required")?;
            if optional(field, "minimum")
                .and_then(Value::as_i64)
                .is_some_and(|n| number < n)
                || optional(field, "maximum")
                    .and_then(Value::as_i64)
                    .is_some_and(|n| number > n)
            {
                return Err("form integer bound");
            }
        }
        Some("number") => {
            let number = value
                .as_f64()
                .filter(|n| n.is_finite())
                .ok_or("form finite number required")?;
            if optional(field, "minimum")
                .and_then(Value::as_f64)
                .is_some_and(|n| number < n)
                || optional(field, "maximum")
                    .and_then(Value::as_f64)
                    .is_some_and(|n| number > n)
            {
                return Err("form number bound");
            }
        }
        Some("boolean") if value.is_boolean() => {}
        Some("array") => {
            let selected = value.as_array().ok_or("form string array required")?;
            let count = selected.len() as u64;
            if selected.len() > OPTIONS
                || optional(field, "minItems")
                    .and_then(Value::as_u64)
                    .is_some_and(|n| count < n)
                || optional(field, "maxItems")
                    .and_then(Value::as_u64)
                    .is_some_and(|n| count > n)
            {
                return Err("form selection count");
            }
            let items = &field["items"];
            for (index, item) in selected.iter().enumerate() {
                if !item.is_string() || selected[..index].contains(item) {
                    return Err("form distinct string selections required");
                }
                let offered = if let Some(plain) = items["enum"].as_array() {
                    plain.contains(item)
                } else {
                    items["anyOf"]
                        .as_array()
                        .is_some_and(|options| options.iter().any(|o| o["const"] == *item))
                };
                if !offered {
                    return Err("form choice unavailable");
                }
            }
        }
        _ => return Err("form value type"),
    }
    Ok(())
}

/// Shared local validation, not general JSON Schema or pattern/format
/// certification. Omitted optional fields stay unset rather than defaulted.
pub(crate) fn validate_form_values(
    schema: &Value,
    values: &Value,
) -> std::result::Result<(), &'static str> {
    if value_bytes(values) > LOCAL_CONTENT {
        return Err("form answer budget");
    }
    validate_form_schema(schema)?;
    let properties = schema.get("properties").and_then(Value::as_object);
    let values = values.as_object().ok_or("form values object required")?;
    if let Some(required) = optional(schema, "required").and_then(Value::as_array) {
        for name in required {
            if !values.contains_key(name.as_str().ok_or("invalid required fields")?) {
                return Err("required form value missing");
            }
        }
    }
    for (name, value) in values {
        let field = properties
            .and_then(|p| p.get(name))
            .ok_or("unknown form value")?;
        validate_field_value(field, value)?;
        // Reject dependent notes before native capture, not only at wire encoding.
        if let Some(notes_key) = field["_meta"]["grok_notes_key"].as_str()
            && (value == "Other"
                || value
                    .as_array()
                    .is_some_and(|choices| choices.iter().any(|choice| choice == "Other")))
            && values
                .get(notes_key)
                .and_then(Value::as_str)
                .is_none_or(|notes| notes.is_empty())
        {
            return Err("Other requires answer notes");
        }
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

// Count serialized retained content without allocating a second schema/answer
// buffer. The actual wire frame is allocated only by encode.
fn value_bytes(value: &Value) -> usize {
    struct Counter(usize);
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value).expect("JSON value serializes");
    counter.0
}

fn encode(message: impl Serialize, work_id: Option<String>) -> Result<Outgoing> {
    let mut bytes = serde_json::to_vec(&acp::JsonRpcMessage::wrap(message))
        .map_err(|_| anyhow::anyhow!("invalid-request: ACP encoding failed"))?;
    bytes.push(b'\n');
    ensure!(bytes.len() <= FRAME, "capacity-exceeded: ACP frame budget");
    Ok(Outgoing {
        bytes,
        work_id,
        response_id: None,
        response_write: None,
    })
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
            response_id: None,
            response_write: None,
        });
        let _in_flight = state.output.pop_front().unwrap();
        assert!(
            state
                .capacity(
                    &[Outgoing {
                        bytes: vec![0],
                        work_id: None,
                        response_id: None,
                        response_write: None,
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
    fn e04s03_prepared_action_and_revocation_share_the_real_authority_ordering_point() {
        for commit_first in [false, true] {
            let (broker, instance, target) = ready_broker();
            let mut registry = crate::control::Registry::new();
            registry.add_acp_target(&instance.id).unwrap();
            let a = registry
                .create(
                    &json!({"version":1,"op":"controller-create",
                "role":"external","label":"prepared owner"}),
                    1,
                )
                .unwrap();
            let b = registry
                .create(
                    &json!({"version":1,"op":"controller-create",
                "role":"external","label":"successor"}),
                    2,
                )
                .unwrap();
            let grant = registry
                .claim(&json!({"version":1,"op":"ownership-claim",
                "target":{"kind":"acp","id":instance.id},"controller":a["controller"],
                "observed":registry.snapshot(&instance.id)["observed"],"mode":"force"}))
                .unwrap();
            let authority = registry
                .authorize(&instance.id, &grant["control"], 1)
                .unwrap();
            let request = json!({"op":"acp-prompt","target":target,"control":grant["control"],
                "owner_generation":grant["generation"],"call_id":"captured-before-revoke",
                "text":"add 17 25"});
            let prepared = broker
                .prepare("acp-prompt", &request, authority.controller_id())
                .unwrap();
            let mut prepared = Some(prepared);
            let admitted = commit_first.then(|| {
                authority
                    .admit_acp(false, 1, |generation| {
                        broker.commit(prepared.take().unwrap(), generation)
                    })
                    .unwrap()
            });
            let successor = registry
                .claim(&json!({"version":1,"op":"ownership-claim",
                "target":{"kind":"acp","id":instance.id},"controller":b["controller"],
                "observed":registry.snapshot(&instance.id)["observed"],"mode":"force"}))
                .unwrap();
            assert_ne!(successor["generation"], grant["generation"]);
            if let Some(admitted) = admitted {
                // The old immutable request remains ready for transport after
                // revocation. The real-child case covers its eventual effect.
                let projection = broker
                    .inspect(&json!({"target":target,"include_content":true}))
                    .unwrap();
                assert_eq!(projection["work"][0]["work_id"], admitted["work_id"]);
                assert_eq!(projection["work"][0]["controller_id"], a["controller_id"]);
                assert_eq!(
                    projection["work"][0]["owner_generation"],
                    grant["generation"]
                );
                assert_eq!(projection["work"][0]["state"], "accepted");
                let state = instance.state.lock();
                let wire: Value =
                    serde_json::from_slice(&state.output.front().unwrap().bytes).unwrap();
                assert_eq!(wire["id"], projection["work"][0]["rpc_id"]);
            } else {
                let error = authority
                    .admit_acp(false, 1, |generation| {
                        broker.commit(prepared.take().unwrap(), generation)
                    })
                    .unwrap_err();
                assert!(error.to_string().starts_with("stale-handle:"), "{error}");
                let projection = broker
                    .inspect(&json!({"target":target,"include_content":true}))
                    .unwrap();
                assert_eq!(projection["work"], json!([]));
                assert_eq!(projection["content"]["events"], json!([]));
                let state = instance.state.lock();
                assert!(state.output.is_empty());
                assert!(state.outstanding.is_empty());
                assert!(state.calls.is_empty());
            }
        }
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
                    response_id: None,
                    response_write: None,
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
                    response_id: None,
                    response_write: None,
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

    fn form_message(id: Value, session: &str) -> Value {
        json!({"id":id,"method":"elicitation/create","params":{
            "mode":"form","sessionId":session,"message":"Synthetic question",
            "requestedSchema":{"type":"object","properties":{"n":{"type":"integer"}},"required":["n"]}}})
    }

    fn interaction_action(target: &Value, interaction: &Value, call: &str) -> Value {
        json!({"op":"acp-answer","target":target,"control":{"kind":"control","ref":"unit"},
            "owner_generation":7,"call_id":call,"interaction_id":interaction["interaction_id"],
            "revision":interaction["revision"],"action":"accept","values":{"n":9}})
    }

    fn grok_question(id: i64, session: &str, multiple: bool) -> Value {
        json!({"jsonrpc":"2.0","id":id,"method":"_x.ai/ask_user_question","params":{
            "sessionId":session,"toolCallId":"grok-question-tool","mode":"default",
            "questions":[{"question":"Which synthetic choice?","multiSelect":multiple,
                "options":[{"label":"alpha","description":"First","preview":"<safe-preview>"},
                    {"label":"beta","description":"Second"}]}]}})
    }

    #[test]
    fn e04s04_grok_other_notes_and_unsupported_decline_preserve_pending_responder() {
        let (broker, instance, target) = ready_broker();
        let detail = {
            let mut state = instance.state.lock();
            apply(&instance, &mut state, grok_question(76, "session", false)).unwrap();
            state.interactions[0].content()
        };
        let properties = detail["schema"]["properties"].as_object().unwrap();
        let (field, schema) = properties
            .iter()
            .find(|(_, schema)| schema["title"] == "Which synthetic choice?")
            .unwrap();
        let notes = schema["_meta"]["grok_notes_key"].as_str().unwrap();
        let mut request = interaction_action(&target, &detail, "unsupported-grok-decline");
        request["action"] = json!("decline");
        request.as_object_mut().unwrap().remove("values");
        let prepared = broker.prepare("acp-answer", &request, "original").unwrap();
        assert!(
            broker
                .commit(prepared, 7)
                .unwrap_err()
                .to_string()
                .starts_with("unsupported:")
        );
        request["call_id"] = json!("grok-other-without-notes");
        request["action"] = json!("accept");
        request["values"] = json!({field:"Other"});
        let prepared = broker.prepare("acp-answer", &request, "original").unwrap();
        assert!(
            broker
                .commit(prepared, 7)
                .unwrap_err()
                .to_string()
                .starts_with("invalid-request:")
        );
        {
            let state = instance.state.lock();
            assert!(state.output.is_empty() && state.work.is_empty());
            assert_eq!(state.interactions[0].state, "pending");
            assert!(!state.calls.contains("unsupported-grok-decline"));
            assert!(!state.calls.contains("grok-other-without-notes"));
        }
        request["call_id"] = json!("grok-freeform");
        request["values"] = json!({field:"Other",notes:"synthetic free-form answer"});
        let prepared = broker.prepare("acp-answer", &request, "original").unwrap();
        broker.commit(prepared, 7).unwrap();
        let state = instance.state.lock();
        let response: Value = serde_json::from_slice(&state.output[0].bytes).unwrap();
        assert_eq!(response["id"], 76);
        assert_eq!(
            response["result"],
            json!({"outcome":"accepted",
            "answers":{"Which synthetic choice?":["Other"]},
            "annotations":{"Which synthetic choice?":{"notes":"synthetic free-form answer"}}})
        );
    }

    #[test]
    fn e04s04_grok_nullable_multiselect_is_single_choice_not_an_invalid_question() {
        let (_, instance, _) = ready_broker();
        let mut message = grok_question(75, "session", false);
        message["params"]["questions"][0]["multiSelect"] = Value::Null;
        let mut state = instance.state.lock();
        apply(&instance, &mut state, message).unwrap();
        assert_eq!(
            state.interactions.len(),
            1,
            "installed Grok sends null for an omitted optional flag"
        );
        let detail = state.interactions[0].content();
        let fields = detail["schema"]["properties"].as_object().unwrap();
        let field = fields
            .values()
            .find(|value| value["title"] == "Which synthetic choice?")
            .unwrap();
        assert_eq!(field["type"], "string");
        assert_eq!(field["oneOf"][0]["const"], "alpha");
    }

    #[test]
    fn e04s04_grok_genuine_question_preserves_choices_original_response_and_single_selection() {
        for multiple in [false, true] {
            let (broker, instance, target) = ready_broker();
            {
                let mut state = instance.state.lock();
                apply(
                    &instance,
                    &mut state,
                    grok_question(71, "session", multiple),
                )
                .unwrap();
            }
            let detail = {
                let state = instance.state.lock();
                assert_eq!(
                    state.interactions.len(),
                    1,
                    "real Grok question must be canonical"
                );
                state.interactions[0].content()
            };
            assert_eq!(detail["scope"]["sessionId"], "session");
            assert_eq!(detail["scope"]["toolCallId"], "grok-question-tool");
            let fields = detail["schema"]["properties"].as_object().unwrap();
            let field = fields
                .iter()
                .find(|(_, value)| value["title"] == "Which synthetic choice?")
                .expect("original question text is displayed")
                .0;
            let answer = if multiple {
                json!(["alpha", "beta"])
            } else {
                json!("alpha")
            };
            let values = json!({field:answer});
            let mut request = interaction_action(&target, &detail, "grok-answer");
            request["values"] = values;
            let first = broker.prepare("acp-answer", &request, "original").unwrap();
            request["call_id"] = json!("grok-losing-answer");
            let losing = broker.prepare("acp-answer", &request, "original").unwrap();
            broker.commit(first, 7).unwrap();
            assert!(
                broker
                    .commit(losing, 7)
                    .unwrap_err()
                    .to_string()
                    .starts_with("stale-interaction:")
            );
            let state = instance.state.lock();
            assert_eq!(state.output.len(), 1, "one exact response, no new prompt");
            let response: Value = serde_json::from_slice(&state.output[0].bytes).unwrap();
            assert_eq!(response["id"], 71);
            assert_eq!(response["result"]["outcome"], "accepted");
            assert_eq!(
                response["result"]["answers"]["Which synthetic choice?"],
                if multiple {
                    json!(["alpha", "beta"])
                } else {
                    json!(["alpha"])
                }
            );
            if !multiple {
                assert_eq!(
                    response["result"]["annotations"]["Which synthetic choice?"]["preview"],
                    "<safe-preview>"
                );
            }
            assert_eq!(
                state.interactions[0].selection.as_ref().unwrap()["content"][field],
                answer
            );
            assert!(!state.calls.contains("grok-losing-answer"));
        }
    }

    #[test]
    fn e04s04_grok_bad_scope_duplicate_questions_and_choices_never_publish() {
        for malformed in ["scope", "question", "choice", "mode", "count"] {
            let (_, instance, _) = ready_broker();
            let mut message = grok_question(72, "session", false);
            match malformed {
                "scope" => message["params"]["sessionId"] = json!("another-session"),
                "question" => {
                    let duplicate = message["params"]["questions"][0].clone();
                    message["params"]["questions"]
                        .as_array_mut()
                        .unwrap()
                        .push(duplicate);
                }
                "choice" => {
                    let duplicate = message["params"]["questions"][0]["options"][0].clone();
                    message["params"]["questions"][0]["options"]
                        .as_array_mut()
                        .unwrap()
                        .push(duplicate);
                }
                "mode" => message["params"]["mode"] = json!("invented-mode"),
                _ => {
                    message["params"]["questions"] =
                        json!(vec![message["params"]["questions"][0].clone(); 65])
                }
            }
            let mut state = instance.state.lock();
            apply(&instance, &mut state, message).unwrap();
            assert!(state.interactions.is_empty(), "{malformed}");
            let response: Value = serde_json::from_slice(&state.output[0].bytes).unwrap();
            assert_eq!(response["id"], 72);
            assert_eq!(response["error"]["code"], -32602, "{malformed}");
            assert!(state.work.is_empty() && state.foreground.is_none());
        }
    }

    #[test]
    fn e04s04_grok_interrupt_cancels_exact_pending_and_late_question_without_a_choice_window() {
        let (broker, instance, target) = ready_broker();
        let prompt = broker
            .prepare(
                "acp-prompt",
                &prompt_request(&target, "grok-original"),
                "original",
            )
            .unwrap();
        let admitted = broker.commit(prompt, 7).unwrap();
        {
            let mut state = instance.state.lock();
            apply(&instance, &mut state, grok_question(73, "session", false)).unwrap();
        }
        let interrupt = json!({"op":"acp-interrupt","target":target,"control":{"kind":"control","ref":"unit"},
            "owner_generation":7,"call_id":"grok-explicit-interrupt","prompt_id":admitted["prompt_id"]});
        // Prompt identity comes from retained work, never inferred from a notification.
        let prompt_id = broker.metadata(&instance.id).unwrap()["foreground"]["prompt_id"].clone();
        let mut interrupt = interrupt;
        interrupt["prompt_id"] = prompt_id;
        let prepared = broker
            .prepare("acp-interrupt", &interrupt, "original")
            .unwrap();
        broker.commit(prepared, 7).unwrap();
        let mut state = instance.state.lock();
        apply(&instance, &mut state, grok_question(74, "session", false)).unwrap();
        for id in [73, 74] {
            let responses: Vec<Value> = state
                .output
                .iter()
                .filter_map(|frame| serde_json::from_slice::<Value>(&frame.bytes).ok())
                .filter(|frame| frame["id"] == id)
                .collect();
            assert_eq!(responses.len(), 1);
            assert_eq!(responses[0]["result"], json!({"outcome":"cancelled"}));
        }
        assert!(
            state
                .interactions
                .iter()
                .all(|interaction| interaction.state == "selected")
        );
        assert!(state.foreground.as_ref().unwrap().1);
    }

    #[test]
    fn acp_form_values_preserve_unset_false_zero_empty_and_i64_boundaries() {
        let schema = json!({"type":"object","properties":{
            "optional":{"type":"integer","default":99},
            "flag":{"type":"boolean","default":true},
            "zero":{"type":"integer","minimum":0,"maximum":0},
            "empty":{"type":"string","maxLength":0},
            "min":{"type":"integer","minimum":i64::MIN,"maximum":i64::MIN},
            "max":{"type":"integer","minimum":i64::MAX,"maximum":i64::MAX},
            "number":{"type":"number","minimum":-0.5,"maximum":0.5}
        },"required":["flag","zero","empty","min","max","number"]});
        let values =
            json!({"flag":false,"zero":0,"empty":"","min":i64::MIN,"max":i64::MAX,"number":0.5});
        assert_eq!(validate_form_values(&schema, &values), Ok(()));
        for name in ["flag", "zero", "empty", "min", "max", "number"] {
            let mut missing = values.clone();
            missing.as_object_mut().unwrap().remove(name);
            assert_eq!(
                validate_form_values(&schema, &missing),
                Err("required form value missing")
            );
            let mut null = values.clone();
            null[name] = Value::Null;
            assert!(validate_form_values(&schema, &null).is_err());
        }
        let mut invalid = values.clone();
        invalid["min"] = json!(i64::MIN + 1);
        assert_eq!(
            validate_form_values(&schema, &invalid),
            Err("form integer bound")
        );
        invalid["min"] = json!(i64::MIN);
        invalid["max"] = json!(i64::MAX as u64 + 1);
        assert_eq!(
            validate_form_values(&schema, &invalid),
            Err("form i64 integer required")
        );
        invalid["max"] = json!(i64::MAX);
        invalid["zero"] = json!(0.0);
        assert_eq!(
            validate_form_values(&schema, &invalid),
            Err("form i64 integer required")
        );
        invalid["zero"] = json!(0);
        invalid["number"] = json!(0.50001);
        assert_eq!(
            validate_form_values(&schema, &invalid),
            Err("form number bound")
        );
    }

    #[test]
    fn acp_form_enum_intersection_unicode_and_distinct_multiselect_are_enforced() {
        let schema = json!({"type":"object","properties":{
            "text":{"type":"string","minLength":2,"maxLength":2,"pattern":"agent-checks","format":"email"},
            "single":{"type":"string","enum":["a","b"],"oneOf":[{"const":"b","title":"B"},{"const":"c","title":"C"}]},
            "many":{"type":"array","items":{"type":"string","enum":["a","b"]},"minItems":1,"maxItems":2},
            "titled":{"type":"array","items":{"anyOf":[{"const":"x","title":"X"},{"const":"y","title":"Y"}]},"maxItems":1}
        },"required":["text","single","many","titled"]});
        let values = json!({"text":"λ🙂","single":"b","many":["a","b"],"titled":["y"]});
        assert_eq!(validate_form_values(&schema, &values), Ok(()));
        for (field, invalid) in [
            ("text", json!("λ")),
            ("text", json!("λ🙂a")),
            ("single", json!("a")),
            ("single", json!("c")),
            ("many", json!(["a", "a"])),
            ("many", json!([])),
            ("many", json!(["a", null])),
            ("many", json!(["unoffered"])),
            ("titled", json!(["x", "y"])),
            ("titled", json!([1])),
        ] {
            let mut rejected = values.clone();
            rejected[field] = invalid;
            assert!(
                validate_form_values(&schema, &rejected).is_err(),
                "{rejected}"
            );
        }
    }

    #[test]
    fn acp_raw_schema_rejects_coercible_defaults_root_types_and_unknown_constraints() {
        for schema in [
            json!({"type":"array","properties":{}}),
            json!({"type":null,"properties":{}}),
            json!({"type":"object","properties":{"n":{"type":"integer","default":"9"}}}),
            json!({"type":"object","properties":{"b":{"type":"boolean","default":0}}}),
            json!({"type":"object","properties":{"s":{"type":"string","default":false}}}),
            json!({"type":"object","properties":{"a":{"type":"array","items":{"type":"string","enum":["x"]},"default":["x",5]}}}),
            json!({"type":"object","properties":{"n":{"type":"number","minimum":10,"maximum":9}}}),
            json!({"type":"object","properties":{"n":{"type":"integer","minimum":1.5}}}),
            json!({"type":"object","properties":{"s":{"type":"string","minLength":3,"maxLength":2}}}),
            json!({"type":"object","properties":{"s":{"type":"string","enum":["x"],"oneOf":[{"const":"y","title":"Y"}]}}}),
            json!({"type":"object","properties":{"s":{"type":"string","enum":["x","x"]}}}),
            json!({"type":"object","properties":{"s":{"type":"string","enum":[]}}}),
            json!({"type":"object","properties":{"a":{"type":"array","items":{"type":"integer","enum":[1]}}}}),
            json!({"type":"object","properties":{"nested":{"type":"object","properties":{}}}}),
            json!({"type":"object","properties":{},"required":["absent"]}),
            json!({"type":"object","properties":{"n":{"type":"integer","multipleOf":2}}}),
            json!({"type":"object","properties":{"n":{"type":"integer","default":-1,"minimum":0}}}),
        ] {
            assert!(
                validate_form_values(&schema, &json!({})).is_err(),
                "{schema}"
            );
        }
        let mut fields = serde_json::Map::new();
        for index in 0..FIELDS {
            fields.insert(format!("f{index}"), json!({"type":"boolean"}));
        }
        let mut schema = json!({"type":"object","properties":fields});
        assert_eq!(validate_form_values(&schema, &json!({})), Ok(()));
        schema["properties"]["overflow"] = json!({"type":"boolean"});
        assert_eq!(
            validate_form_values(&schema, &json!({})),
            Err("form field budget")
        );
        let offered: Vec<_> = (0..OPTIONS).map(|n| format!("v{n}")).collect();
        let mut schema =
            json!({"type":"object","properties":{"choice":{"type":"string","enum":offered}}});
        assert_eq!(
            validate_form_values(&schema, &json!({"choice":"v255"})),
            Ok(())
        );
        schema["properties"]["another"] = json!({"type":"string","enum":["extra"]});
        assert_eq!(
            validate_form_values(&schema, &json!({})),
            Err("form option budget")
        );
    }

    #[test]
    fn acp_first_valid_prepared_response_wins_without_losing_history_or_duplicate_wire_response() {
        let (broker, instance, target) = ready_broker();
        {
            let mut state = instance.state.lock();
            apply(&instance, &mut state, form_message(json!(77), "session")).unwrap();
        }
        let interaction = broker.metadata(&instance.id).unwrap()["interactions"][0].clone();
        let winner = interaction_action(&target, &interaction, "winner");
        let mut loser = interaction_action(&target, &interaction, "loser");
        loser["values"]["n"] = json!(999);
        let first = broker.prepare("acp-answer", &winner, "original").unwrap();
        let second = broker.prepare("acp-answer", &loser, "original").unwrap();
        let receipt = broker.commit(first, 7).unwrap();
        assert!(
            broker
                .commit(second, 7)
                .unwrap_err()
                .to_string()
                .starts_with("stale-interaction:")
        );
        let state = instance.state.lock();
        assert_eq!(state.work.len(), 1);
        assert_eq!(
            state.interactions[0].selected_work.as_deref(),
            receipt["work_id"].as_str()
        );
        assert_eq!(
            state.interactions[0].selection.as_ref().unwrap()["content"]["n"],
            9
        );
        assert!(!state.calls.contains("loser"));
        let frame: Value = serde_json::from_slice(&state.output[0].bytes).unwrap();
        assert_eq!(frame["id"], 77);
        assert_eq!(frame["result"]["content"]["n"], 9);
        assert_eq!(state.output_count, 1);
    }

    #[test]
    fn acp_interrupt_reserves_every_frame_and_late_capacity_failure_never_publishes_choice() {
        let (broker, instance, target) = ready_broker();
        let request = prompt_request(&target, "prompt");
        broker
            .commit(
                broker
                    .prepare("acp-prompt", &request, "controller")
                    .unwrap(),
                7,
            )
            .unwrap();
        {
            let mut state = instance.state.lock();
            apply(
                &instance,
                &mut state,
                form_message(json!("form"), "session"),
            )
            .unwrap();
            apply(
                &instance,
                &mut state,
                json!({"id":"permission","method":"session/request_permission",
                "params":{"sessionId":"session","toolCall":{"toolCallId":"tool"},
                "options":[{"optionId":"reject","name":"Deny once","kind":"reject_once"}]}}),
            )
            .unwrap();
            while state.output_count < EGRESS - 2 {
                state.enqueue(Outgoing {
                    bytes: b"{}\n".to_vec(),
                    work_id: None,
                    response_id: None,
                    response_write: None,
                });
            }
        }
        let prompt_id = broker.metadata(&instance.id).unwrap()["foreground"]["prompt_id"].clone();
        let action = json!({"op":"acp-interrupt","target":target,"control":{"kind":"control","ref":"unit"},
            "owner_generation":7,"call_id":"cancel","prompt_id":prompt_id});
        assert!(
            broker
                .commit(
                    broker
                        .prepare("acp-interrupt", &action, "controller")
                        .unwrap(),
                    7
                )
                .unwrap_err()
                .to_string()
                .starts_with("capacity-exceeded:")
        );
        {
            let mut state = instance.state.lock();
            assert!(!state.foreground.as_ref().unwrap().1);
            assert!(state.interactions.iter().all(|i| i.state == "pending"));
            assert!(!state.calls.contains("cancel"));
            let frame = state.output.pop_back().unwrap();
            state.output_bytes -= frame.bytes.len();
            state.output_count -= 1;
        }
        broker
            .commit(
                broker
                    .prepare("acp-interrupt", &action, "controller")
                    .unwrap(),
                7,
            )
            .unwrap();
        let mut state = instance.state.lock();
        assert_eq!(state.output_count, EGRESS);
        assert!(state.foreground.as_ref().unwrap().1);
        assert!(state.interactions.iter().all(|i| i.state == "selected"));
        let reserved: Vec<Value> = state
            .output
            .iter()
            .rev()
            .take(3)
            .map(|frame| serde_json::from_slice(&frame.bytes).unwrap())
            .collect();
        assert_eq!(reserved[0]["result"]["outcome"]["outcome"], "cancelled");
        assert_eq!(reserved[1]["result"]["action"], "cancel");
        assert_eq!(reserved[2]["method"], "session/cancel");
        assert!(
            apply(
                &instance,
                &mut state,
                form_message(json!("late"), "session")
            )
            .is_err()
        );
        assert!(
            !state
                .interactions
                .iter()
                .any(|i| i.wire_id == acp::RequestId::Str("late".into()))
        );
        state.close(Some("egress-capacity"));
        assert!(state.interactions.iter().all(|i| i.state == "unknown"));
    }

    #[test]
    fn acp_wire_id_is_reserved_through_inflight_response_and_typed_ids_do_not_alias() {
        let (broker, instance, target) = ready_broker();
        {
            let mut state = instance.state.lock();
            apply(&instance, &mut state, form_message(json!(41), "session")).unwrap();
            apply(&instance, &mut state, form_message(json!("41"), "session")).unwrap();
        }
        let interaction = broker.metadata(&instance.id).unwrap()["interactions"][0].clone();
        let action = interaction_action(&target, &interaction, "selected");
        broker
            .commit(
                broker.prepare("acp-answer", &action, "controller").unwrap(),
                7,
            )
            .unwrap();
        let mut state = instance.state.lock();
        let _inflight = state.output.pop_front().unwrap();
        assert!(apply(&instance, &mut state, form_message(json!(41), "session")).is_err());
        assert_eq!(state.interactions.len(), 2);
        assert_eq!(state.interactions[1].state, "pending");
        assert_eq!(state.responders.len(), 2);
    }

    #[test]
    fn acp_request_scoped_bootstrap_is_not_guessed_into_current_prompt_interrupt() {
        let (broker, instance, target) = ready_broker();
        let prompt = prompt_request(&target, "prompt");
        broker
            .commit(
                broker.prepare("acp-prompt", &prompt, "controller").unwrap(),
                7,
            )
            .unwrap();
        {
            let mut state = instance.state.lock();
            state.outstanding.insert(
                acp::RequestId::Number(90),
                Pending {
                    kind: RpcKind::Authenticate,
                    work_id: None,
                },
            );
            let mut bootstrap = form_message(json!("bootstrap"), "session");
            bootstrap["params"]
                .as_object_mut()
                .unwrap()
                .remove("sessionId");
            bootstrap["params"]["requestId"] = json!(90);
            apply(&instance, &mut state, bootstrap).unwrap();
            let mut prompt_form = form_message(json!("prompt-form"), "session");
            prompt_form["params"]
                .as_object_mut()
                .unwrap()
                .remove("sessionId");
            prompt_form["params"]["requestId"] = json!(0);
            apply(&instance, &mut state, prompt_form).unwrap();
        }
        let prompt_id = broker.metadata(&instance.id).unwrap()["foreground"]["prompt_id"].clone();
        let interrupt = json!({"op":"acp-interrupt","target":target,"control":{"kind":"control","ref":"unit"},
            "owner_generation":7,"call_id":"interrupt","prompt_id":prompt_id});
        broker
            .commit(
                broker
                    .prepare("acp-interrupt", &interrupt, "controller")
                    .unwrap(),
                7,
            )
            .unwrap();
        let state = instance.state.lock();
        assert_eq!(state.interactions[0].state, "pending");
        assert_eq!(state.interactions[1].state, "selected");
        assert_eq!(
            state.interactions[1].selection.as_ref().unwrap()["action"],
            "cancel"
        );
    }

    #[test]
    fn acp_completed_response_stamp_allows_reuse_before_bookkeeping_without_releasing_new_responder()
     {
        let (broker, instance, target) = ready_broker();
        {
            let mut state = instance.state.lock();
            apply(
                &instance,
                &mut state,
                form_message(json!("reused"), "session"),
            )
            .unwrap();
        }
        let old = broker.metadata(&instance.id).unwrap()["interactions"][0].clone();
        let action = interaction_action(&target, &old, "old-answer");
        broker
            .commit(
                broker.prepare("acp-answer", &action, "controller").unwrap(),
                7,
            )
            .unwrap();
        let mut state = instance.state.lock();
        let in_flight = state.output.pop_front().unwrap();
        let written = in_flight.response_write.as_ref().unwrap();
        // The write finished outside state; the FIFO consumer wins the next
        // state lock before the writer can publish its ordinary bookkeeping.
        written.store(1, Ordering::Release);
        apply(
            &instance,
            &mut state,
            form_message(json!("reused"), "session"),
        )
        .unwrap();
        assert_eq!(state.interactions[0].delivery, "submitted");
        assert_eq!(state.interactions[1].state, "pending");
        assert_ne!(state.interactions[0].id, state.interactions[1].id);
        let new_id = state.interactions[1].id.clone();
        state.finish_response(&acp::RequestId::Str("reused".into()), written);
        assert_eq!(
            state.responders[&acp::RequestId::Str("reused".into())]
                .interaction
                .as_deref(),
            Some(new_id.as_str())
        );
        assert_eq!(state.interactions[1].delivery, "not-submitted");
    }

    #[test]
    fn acp_form_answer_exact_byte_limit_and_empty_official_schema_are_supported() {
        assert_eq!(validate_form_values(&json!({}), &json!({})), Ok(()));
        assert_eq!(
            validate_form_values(&json!({"type":"object"}), &json!({})),
            Ok(())
        );
        assert_eq!(
            validate_form_values(&json!({"type":"object","properties":null}), &json!({})),
            Err("invalid form properties")
        );
        let schema = json!({"type":"object","properties":{"text":{"type":"string"}}});
        let mut values = json!({"text":"x".repeat(LOCAL_CONTENT - 11)});
        assert_eq!(validate_form_values(&schema, &values), Ok(()));
        values["text"] = json!("x".repeat(LOCAL_CONTENT - 10));
        assert_eq!(
            validate_form_values(&schema, &values),
            Err("form answer budget")
        );
    }

    #[test]
    fn acp_scoped_rpc_closure_distinguishes_completed_parent_from_unknown_parent() {
        let (broker, instance, target) = ready_broker();
        let prompt = prompt_request(&target, "first");
        broker
            .commit(
                broker.prepare("acp-prompt", &prompt, "controller").unwrap(),
                7,
            )
            .unwrap();
        {
            let mut state = instance.state.lock();
            let mut form = form_message(json!("first-scoped"), "session");
            form["params"].as_object_mut().unwrap().remove("sessionId");
            form["params"]["requestId"] = json!(0);
            apply(&instance, &mut state, form).unwrap();
            apply(
                &instance,
                &mut state,
                json!({"id":0,"result":{"stopReason":"end_turn"}}),
            )
            .unwrap();
            assert_eq!(state.interactions[0].state, "pending");
            assert_eq!(state.interactions[0].parent_state, "completed");
        }
        let prompt = prompt_request(&target, "second");
        broker
            .commit(
                broker.prepare("acp-prompt", &prompt, "controller").unwrap(),
                7,
            )
            .unwrap();
        let mut state = instance.state.lock();
        let mut form = form_message(json!("second-scoped"), "session");
        form["params"].as_object_mut().unwrap().remove("sessionId");
        form["params"]["requestId"] = json!(1);
        apply(&instance, &mut state, form).unwrap();
        state.close(Some("eof"));
        assert_eq!(state.interactions[0].parent_state, "completed");
        assert_eq!(state.interactions[1].parent_state, "unknown");
        assert!(state.interactions.iter().all(|i| i.state == "unknown"));
        assert_eq!(state.work[0].state, "completed");
        assert_eq!(state.work[1].state, "unknown");
    }
}

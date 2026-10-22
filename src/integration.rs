use anyhow::{Context as _, Result, anyhow, bail, ensure};
use parking_lot::{Condvar, Mutex};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::CString,
    fs,
    io::Write,
    os::unix::{
        ffi::OsStrExt,
        fs::{OpenOptionsExt, PermissionsExt},
        io::FromRawFd,
    },
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const AGENT_LEASE_MS: u64 = 12_000;
const ADAPTER: &str = include_str!("../integrations/zsh/adapter.zsh");
const EXTENSION: &str = include_str!("../integrations/omp/amux.ts");
const LIFECYCLE: &str = include_str!("../integrations/omp/lifecycle.ts");

#[derive(Clone, Debug)]
pub struct Context {
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub captured_at_ms: u64,
}
#[derive(Default)]
struct Shell {
    pid: Option<u32>,
    ready: bool,
    partial: bool,
    lost: bool,
    prompt_generation: u64,
    context: Option<Context>,
}
#[derive(Clone)]
struct Agent {
    value: Value,
    last_seen: u64,
}
#[derive(Default)]
struct Terminal {
    shell: Shell,
    root: Option<String>,
    agents: BTreeMap<String, Agent>,
    executions: BTreeMap<String, Value>,
    authorities: BTreeMap<String, Arc<crate::control::Authority>>,
    execution_order: std::collections::VecDeque<String>,
    executions_discarded: u64,
}
pub struct Integration {
    directory: PathBuf,
    socket: PathBuf,
    executable: PathBuf,
    terminals: Mutex<BTreeMap<String, Terminal>>,
    acknowledgement: Condvar,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn alive(pid: u32) -> bool {
    if pid == 0 || pid > libc::pid_t::MAX as u32 {
        return false;
    }
    // kill(0) checks identity without changing the process.
    unsafe {
        libc::kill(pid as libc::pid_t, 0) == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}
fn valid_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "invalid integration ID"
    );
    Ok(())
}
fn check_shell(shell: &Shell, queue: bool) -> Result<()> {
    ensure!(
        !shell.lost && shell.pid.is_some_and(alive),
        "integration-lost: shell adapter unavailable"
    );
    if !queue {
        ensure!(shell.ready, "busy: shell has no active prompt");
        ensure!(
            !shell.partial,
            "partial-input: shell editing buffer is not empty"
        );
    }
    Ok(())
}
fn check_submission(terminal: &Terminal, queue: bool) -> Result<()> {
    check_shell(&terminal.shell, queue)?;
    ensure!(
        queue
            || !terminal
                .executions
                .values()
                .any(|record| record["status"] == "queued"),
        "busy: an earlier shell submission is pending; queueing must be explicit"
    );
    Ok(())
}
fn private_write(path: &Path, content: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(content)?;
    Ok(())
}
impl Integration {
    pub fn new(runtime: &Path, socket: &Path, executable: &Path) -> Result<Arc<Self>> {
        let directory = runtime.join("integration");
        fs::create_dir_all(&directory)?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        private_write(&directory.join("adapter.zsh"), ADAPTER.as_bytes())?;
        private_write(&directory.join("amux.ts"), EXTENSION.as_bytes())?;
        private_write(&directory.join("lifecycle.ts"), LIFECYCLE.as_bytes())?;
        Ok(Arc::new(Self {
            directory,
            socket: socket.to_owned(),
            executable: executable.to_owned(),
            terminals: Mutex::new(BTreeMap::new()),
            acknowledgement: Condvar::new(),
        }))
    }
    pub fn extension_path(&self) -> PathBuf {
        self.directory.join("amux.ts")
    }
    pub fn prepare(&self, id: &str) -> Result<BTreeMap<String, String>> {
        valid_id(id)?;
        let directory = self.directory.join(id);
        fs::create_dir_all(&directory)?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        let fifo = CString::new(directory.join("commands").as_os_str().as_bytes())?;
        if unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // Global startup files remain Zsh's responsibility; these wrappers source each
        // original user file exactly once, with its own ZDOTDIR in effect.
        for name in [".zshenv", ".zprofile", ".zshrc", ".zlogin", ".zlogout"] {
            let setup = if name == ".zshenv" {
                "if [[ -n ${AMUX_ORIGINAL_ZDOTDIR+x} ]]; then\n  export AMUX_USER_ZDOTDIR=$AMUX_ORIGINAL_ZDOTDIR\nelse\n  export AMUX_USER_ZDOTDIR=$HOME\nfi\n"
            } else {
                ""
            };
            let append = if name == ".zshrc" {
                "\nsource \"$AMUX_ADAPTER\"\n"
            } else {
                ""
            };
            let body = format!(
                "{setup}ZDOTDIR=$AMUX_USER_ZDOTDIR\n[[ -r \"$ZDOTDIR/{name}\" ]] && source \"$ZDOTDIR/{name}\"\nexport AMUX_USER_ZDOTDIR=${{ZDOTDIR:-$HOME}}\nexport ZDOTDIR=$AMUX_INTEGRATION\n{append}"
            );
            private_write(&directory.join(name), body.as_bytes())?;
        }
        self.terminals.lock().insert(id.into(), Terminal::default());
        let mut env = BTreeMap::from([
            ("AMUX_TERMINAL".into(), id.into()),
            (
                "AMUX_SOCKET".into(),
                self.socket.to_string_lossy().into_owned(),
            ),
            (
                "AMUX_EXECUTABLE".into(),
                self.executable.to_string_lossy().into_owned(),
            ),
            (
                "AMUX_INTEGRATION".into(),
                directory.to_string_lossy().into_owned(),
            ),
            (
                "AMUX_ADAPTER".into(),
                self.directory
                    .join("adapter.zsh")
                    .to_string_lossy()
                    .into_owned(),
            ),
            (
                "AMUX_OMP_EXTENSION".into(),
                self.extension_path().to_string_lossy().into_owned(),
            ),
            ("ZDOTDIR".into(), directory.to_string_lossy().into_owned()),
        ]);
        if let Some(original) = std::env::var_os("ZDOTDIR") {
            env.insert(
                "AMUX_ORIGINAL_ZDOTDIR".into(),
                original.to_string_lossy().into_owned(),
            );
        }
        Ok(env)
    }
    pub fn report(&self, id: &str, event: Value) -> Result<Value> {
        let mut terminals = self.terminals.lock();
        let terminal = terminals
            .get_mut(id)
            .ok_or_else(|| anyhow!("integration-lost: terminal integration missing"))?;
        match event["kind"].as_str().unwrap_or("") {
            "shell-context" => {
                let cwd = event["cwd"]
                    .as_str()
                    .ok_or_else(|| anyhow!("missing shell cwd"))?;
                let env: BTreeMap<String, String> = serde_json::from_value(event["env"].clone())?;
                terminal.shell.context = Some(Context {
                    cwd: cwd.into(),
                    env,
                    captured_at_ms: now(),
                });
                terminal.shell.pid = event["pid"].as_u64().map(|p| p as u32);
                terminal.shell.ready = false;
            }
            "shell-ready" => {
                terminal.shell.pid = event["pid"].as_u64().map(|p| p as u32);
                terminal.shell.ready = true;
                terminal.shell.partial = event["partial"].as_bool().unwrap_or(true);
                terminal.shell.prompt_generation = event["prompt_generation"].as_u64().unwrap_or(0);
                terminal.shell.lost = false;
            }
            "shell-busy" | "shell-lost" => {
                terminal.shell.ready = false;
                if event["kind"] == "shell-lost" {
                    terminal.shell.lost = true;
                }
            }
            "execution-accepted" | "execution-rejected" | "execution-completed" => {
                let execution = event["execution_id"]
                    .as_str()
                    .ok_or_else(|| anyhow!("missing execution ID"))?;
                if event["kind"] == "execution-accepted"
                    && terminal
                        .executions
                        .get(execution)
                        .is_none_or(|record| record["status"] != "queued")
                {
                    return Ok(snapshot(terminal, now()));
                }
                if event["kind"] == "execution-accepted" {
                    if let Some(authority) = terminal.authorities.get(execution) {
                        if let Err(error) = authority.check() {
                            authority.settle(execution, "rejected");
                            if let Some(current) = terminal.executions.get_mut(execution) {
                                *current = json!({"execution_id":execution,"status":"rejected","reason":"stale-handle"});
                            }
                            self.acknowledgement.notify_all();
                            return Err(error);
                        }
                        // Native ZLE installed this exact source in BUFFER.
                        // Acceptance settles dispatch, not command completion.
                        authority.settle(execution, "applied");
                    }
                } else if event["kind"] == "execution-rejected" {
                    if let Some(authority) = terminal.authorities.get(execution) {
                        authority.settle(execution, "rejected");
                    }
                }
                if let Some(current) = terminal.executions.get_mut(execution) {
                    if event["kind"] == "execution-accepted" {
                        terminal.shell.ready = false;
                        *current = json!({"execution_id":execution,"status":"accepted"});
                    } else if event["kind"] == "execution-rejected" {
                        *current = json!({"execution_id":execution,"status":"rejected","reason":event["reason"]});
                    } else {
                        *current = json!({"execution_id":execution,"status":"completed","exit_status":event["exit_status"]});
                    }
                    self.acknowledgement.notify_all();
                }
            }
            "agent" => {
                let key = event["instance_id"]
                    .as_str()
                    .ok_or_else(|| anyhow!("missing agent instance ID"))?
                    .to_owned();
                ensure!(
                    matches!(
                        event["activity"].as_str(),
                        Some("idle" | "working" | "needs-input" | "unknown")
                    ),
                    "invalid agent activity"
                );
                let timestamp = now();
                let root = if event["agent_kind"] == "main" {
                    Some(key.as_str())
                } else {
                    terminal.root.as_deref()
                };
                terminal.agents.retain(|instance, agent| {
                    Some(instance.as_str()) == root
                        || (agent.value["agent_kind"] == "sub"
                            && agent.value["shutdown"] != true
                            && timestamp.saturating_sub(agent.last_seen) <= AGENT_LEASE_MS
                            && agent.value["pid"]
                                .as_u64()
                                .is_some_and(|pid| alive(pid as u32)))
                });
                ensure!(
                    terminal.agents.len() < 128 || terminal.agents.contains_key(&key),
                    "busy: active agent reporting limit reached"
                );
                if event["agent_kind"] == "main" {
                    terminal.root = Some(key.clone());
                }
                terminal.agents.insert(
                    key,
                    Agent {
                        value: event,
                        last_seen: timestamp,
                    },
                );
            }
            _ => bail!("invalid integration event"),
        }
        Ok(snapshot(terminal, now()))
    }
    pub fn state(&self, id: &str) -> Value {
        self.terminals.lock().get(id).map(|t| snapshot(t, now())).unwrap_or_else(|| json!({"shell":{"ready":false,"freshness":"unknown"},"agent":{"activity":"unknown","settled":false,"reason":"integration-lost"}}))
    }
    pub fn capture(&self, id: &str) -> Option<Context> {
        self.terminals
            .lock()
            .get(id)
            .and_then(|t| t.shell.context.clone())
    }
    pub(crate) fn captured_cwd(&self, id: &str) -> Option<PathBuf> {
        self.terminals.lock().get(id).and_then(|terminal| {
            terminal
                .shell
                .context
                .as_ref()
                .map(|context| context.cwd.clone())
        })
    }
    pub(crate) fn awaiting_initial_prompt(&self, id: &str) -> bool {
        self.terminals
            .lock()
            .get(id)
            .is_some_and(|terminal| !terminal.shell.lost && terminal.shell.prompt_generation == 0)
    }
    pub(crate) fn preflight(&self, id: &str, queue: bool) -> Result<()> {
        let terminals = self.terminals.lock();
        let terminal = terminals
            .get(id)
            .ok_or_else(|| anyhow!("integration-lost: terminal integration missing"))?;
        check_submission(terminal, queue)
    }
    pub(crate) fn pending_queue(&self, id: &str, execution: &str) -> Result<bool> {
        let terminals = self.terminals.lock();
        let record = terminals
            .get(id)
            .and_then(|terminal| terminal.executions.get(execution))
            .context("integration-lost: pending execution missing")?;
        ensure!(
            record["status"] == "queued",
            "busy: execution is no longer pending"
        );
        record["queue"]
            .as_bool()
            .context("invalid-request: execution queue flag missing")
    }
    pub(crate) fn prompt_matches(&self, id: &str, pid: u32, generation: u64) -> bool {
        self.terminals.lock().get(id).is_some_and(|terminal| {
            !terminal.shell.lost
                && terminal.shell.ready
                && !terminal.shell.partial
                && terminal.shell.pid == Some(pid)
                && terminal.shell.prompt_generation == generation
        })
    }
    pub(crate) fn execution_authority(
        &self,
        id: &str,
        execution: &str,
    ) -> Result<Arc<crate::control::Authority>> {
        self.terminals
            .lock()
            .get(id)
            .and_then(|terminal| terminal.authorities.get(execution))
            .cloned()
            .context("stale-handle: execution has no originating authority")
    }
    pub fn execute(&self, id: &str, source: &str, queue: bool) -> Result<Value> {
        self.execute_inner(id, source, queue, None)
    }
    pub(crate) fn execute_authorized(
        &self,
        id: &str,
        source: &str,
        queue: bool,
        authority: Arc<crate::control::Authority>,
    ) -> Result<Value> {
        authority.check()?;
        self.execute_inner(id, source, queue, Some(authority))
    }
    fn execute_inner(
        &self,
        id: &str,
        source: &str,
        queue: bool,
        authority: Option<Arc<crate::control::Authority>>,
    ) -> Result<Value> {
        ensure!(!source.contains('\0'), "shell source contains NUL");
        let execution = uuid::Uuid::new_v4().to_string();
        let directory = self.directory.join(id);
        let path = directory.join(format!("{execution}.zsh"));
        let mut terminals = self.terminals.lock();
        let terminal = terminals
            .get_mut(id)
            .ok_or_else(|| anyhow!("integration-lost: terminal integration missing"))?;
        check_submission(terminal, queue)?;
        if terminal.executions.len() >= 256 {
            let removable = terminal.execution_order.iter().position(|id| {
                matches!(
                    terminal.executions[id]["status"].as_str(),
                    Some("completed" | "rejected")
                )
            });
            let position =
                removable.ok_or_else(|| anyhow!("busy: pending execution limit reached"))?;
            let old = terminal
                .execution_order
                .remove(position)
                .expect("existing execution");
            terminal.executions.remove(&old);
            terminal.authorities.remove(&old);
            terminal.executions_discarded += 1;
        }
        private_write(&path, source.as_bytes())?;
        terminal.executions.insert(
            execution.clone(),
            json!({"execution_id":execution,"status":"queued","queue":queue}),
        );
        if let Some(authority) = authority {
            terminal.authorities.insert(execution.clone(), authority);
        }
        let fifo = CString::new(directory.join("commands").as_os_str().as_bytes())?;
        let descriptor = unsafe {
            libc::open(
                fifo.as_ptr(),
                libc::O_WRONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            terminal.executions.remove(&execution);
            terminal.authorities.remove(&execution);
            let _ = fs::remove_file(&path);
            bail!("integration-lost: shell command channel unavailable");
        }
        let mut fifo = unsafe { fs::File::from_raw_fd(descriptor) };
        let frame = format!(
            "{execution}:{}:{}\n",
            u8::from(queue),
            terminal.shell.prompt_generation
        );
        if let Some(authority) = terminal.authorities.get(&execution) {
            if let Err(error) = authority.forward(&execution, "shell-execute") {
                terminal.executions.remove(&execution);
                terminal.authorities.remove(&execution);
                let _ = fs::remove_file(&path);
                return Err(error);
            }
        }
        // A single sub-PIPE_BUF write prevents timed ZLE reads from consuming a partial request.
        if let Err(error) = fifo.write_all(frame.as_bytes()) {
            if let Some(authority) = terminal.authorities.get(&execution) {
                authority.settle(&execution, "rejected");
            }
            terminal.executions.remove(&execution);
            terminal.authorities.remove(&execution);
            let _ = fs::remove_file(&path);
            return Err(error).context("integration-lost: shell command channel write failed");
        }
        terminal.execution_order.push_back(execution.clone());
        if queue {
            return Ok(json!({"execution_id":execution,"status":"queued"}));
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let outcome = terminals
                .get(id)
                .and_then(|t| t.executions.get(&execution))
                .cloned()
                .ok_or_else(|| anyhow!("integration-lost: terminal removed"))?;
            match outcome["status"].as_str() {
                Some("accepted" | "completed") => return Ok(outcome),
                Some("rejected") => {
                    let _ = fs::remove_file(&path);
                    bail!(
                        "{}: shell adapter refused execution",
                        outcome["reason"].as_str().unwrap_or("busy")
                    );
                }
                _ => {}
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                let _ = fs::remove_file(&path);
                if let Some(t) = terminals.get_mut(id) {
                    // Unknown acknowledgement is not rejection. Preserve the
                    // execution and authority so late exact reports can settle it.
                    if let Some(authority) = t.authorities.get(&execution) {
                        authority.settle(&execution, "unknown");
                    }
                    t.shell.ready = false;
                    t.shell.lost = true;
                }
                bail!("integration-lost: shell acknowledgement timed out");
            }
            self.acknowledgement.wait_for(&mut terminals, remaining);
        }
    }
    pub fn remove(&self, id: &str) {
        if valid_id(id).is_err() {
            return;
        }
        let _ = self.terminals.lock().remove(id);
        // Preparation can fail before publishing its integration record.
        let _ = fs::remove_dir_all(self.directory.join(id));
        self.acknowledgement.notify_all();
    }
}
fn agent_snapshot(agent: &Agent, timestamp: u64) -> Value {
    let mut value = agent.value.clone();
    let live = value["pid"].as_u64().is_some_and(|p| alive(p as u32));
    if !live
        || timestamp.saturating_sub(agent.last_seen) > AGENT_LEASE_MS
        || value["shutdown"] == true
    {
        value["activity"] = json!("unknown");
        value["settled"] = json!(false);
        value["reason"] = json!(if value["shutdown"] == true {
            "shutdown"
        } else {
            "integration-lost"
        });
    }
    value["alive"] = json!(live);
    value["settled"] = json!(
        value["settled"] == true
            && value["activity"] == "idle"
            && value["retrying"] != true
            && value["generation"].is_number()
            && value["generation"] == value["completed_generation"]
    );
    value["last_report_at_ms"] = json!(agent.last_seen);
    value
}
fn snapshot(terminal: &Terminal, timestamp: u64) -> Value {
    let shell_live = !terminal.shell.lost && terminal.shell.pid.is_some_and(alive);
    let ready = shell_live && terminal.shell.ready;
    let context = terminal.shell.context.as_ref();
    let root = terminal
        .root
        .as_ref()
        .and_then(|key| terminal.agents.get(key));
    let mut agent = root
        .map(|a| agent_snapshot(a, timestamp))
        .unwrap_or_else(|| json!({"activity":"unknown","settled":false,"reason":"not-observed"}));
    let children: Vec<_> = terminal
        .agents
        .iter()
        .filter(|(key, a)| Some(*key) != terminal.root.as_ref() && a.value["agent_kind"] == "sub")
        .map(|(_, a)| agent_snapshot(a, timestamp))
        .collect();
    agent["children"] = json!(children);
    json!({"shell":{"ready":ready,"partial_input":terminal.shell.partial,"prompt_generation":terminal.shell.prompt_generation,"pid":terminal.shell.pid,"cwd":context.map(|c| c.cwd.to_string_lossy()),"captured_at_ms":context.map(|c| c.captured_at_ms),"freshness":if context.is_none() || !shell_live {"unknown"} else if ready {"confirmed"} else {"stale"}},"agent":agent,"executions":terminal.execution_order.iter().filter_map(|id|terminal.executions.get(id)).collect::<Vec<_>>(),"executions_discarded":terminal.executions_discarded})
}

/// Small process invoked by shell hooks and native extensions. No PTY bytes enter this path.
pub fn helper(args: &[String]) -> Result<()> {
    let terminal = std::env::var("AMUX_TERMINAL").context("AMUX_TERMINAL missing")?;
    let event = if args.first().is_some_and(|s| s == "shell") {
        let action = args.get(1).map(String::as_str).unwrap_or("");
        let argument = |index: usize| -> Result<&str> {
            args.get(index)
                .map(String::as_str)
                .ok_or_else(|| anyhow!("missing shell report argument"))
        };
        match action {
            "context" => {
                let private = [
                    "AMUX_TERMINAL",
                    "AMUX_SOCKET",
                    "AMUX_EXECUTABLE",
                    "AMUX_INTEGRATION",
                    "AMUX_ADAPTER",
                    "AMUX_OMP_EXTENSION",
                    "AMUX_ORIGINAL_ZDOTDIR",
                    "AMUX_USER_ZDOTDIR",
                ];
                let mut env: BTreeMap<String, String> = std::env::vars()
                    .filter(|(k, _)| !private.contains(&k.as_str()))
                    .collect();
                if env.get("ZDOTDIR") == std::env::var("AMUX_INTEGRATION").ok().as_ref() {
                    if let Ok(original) = std::env::var("AMUX_USER_ZDOTDIR") {
                        env.insert("ZDOTDIR".into(), original);
                    }
                }
                json!({"kind":"shell-context","pid":argument(2)?.parse::<u32>()?,"cwd":argument(3)?,"env":env})
            }
            "ready" => {
                json!({"kind":"shell-ready","pid":argument(2)?.parse::<u32>()?,"partial":argument(3)? != "0","prompt_generation":argument(4)?.parse::<u64>()?})
            }
            "epoch" => {
                json!({"kind":"shell-epoch","pid":argument(2)?.parse::<u32>()?,"prompt_generation":argument(3)?.parse::<u64>()?,"execution_id":argument(4)?})
            }
            "wake" => {
                json!({"kind":"shell-wake","pid":argument(2)?.parse::<u32>()?,"prompt_generation":argument(3)?.parse::<u64>()?,"execution_id":argument(4)?,"input_epoch":argument(5)?.parse::<u64>()?})
            }
            "busy" | "lost" => {
                json!({"kind":format!("shell-{action}"),"pid":argument(2)?.parse::<u32>()?})
            }
            "accepted" | "rejected" | "completed" => {
                json!({"kind":format!("execution-{action}"),"execution_id":argument(2)?,"reason":args.get(3),"exit_status":args.get(3).and_then(|s| s.parse::<i32>().ok())})
            }
            _ => bail!("unknown shell report action"),
        }
    } else {
        let payload = if args.first().is_some_and(|s| s == "report") {
            args.get(1)
        } else {
            args.first()
        };
        serde_json::from_str(payload.ok_or_else(|| anyhow!("missing report JSON"))?)?
    };
    let epoch = event["kind"] == "shell-epoch";
    let wake = event["kind"] == "shell-wake";
    let result = crate::client::request(
        &crate::client::socket_path(),
        json!({"version":1,"op":"report","terminal":terminal,"event":event}),
    )?;
    if (epoch || wake) && result["revoked"] == true {
        println!("revoked");
    } else if epoch {
        if let Some(value) = result["input_epoch"].as_u64() {
            println!("{value}");
        } else {
            println!("deferred");
        }
    } else if wake {
        println!(
            "{}",
            if result["woke"] == true {
                "woke"
            } else {
                "deferred"
            }
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn heartbeat_loss_invalidates_settle_without_waiting_in_real_time() {
        let agent = Agent {
            value: json!({"activity":"idle","settled":true,"pid":std::process::id(),"generation":4,"completed_generation":4,"retrying":false}),
            last_seen: 100,
        };
        assert_eq!(
            agent_snapshot(&agent, 100 + AGENT_LEASE_MS)["settled"],
            true
        );
        let expired = agent_snapshot(&agent, 101 + AGENT_LEASE_MS);
        assert_eq!(expired["activity"], "unknown");
        assert_eq!(expired["settled"], false);
        assert_eq!(expired["reason"], "integration-lost");
        assert_eq!(expired["alive"], true);
        let shutdown = Agent {
            value: json!({"activity":"idle","settled":true,"shutdown":true,"pid":std::process::id(),"generation":4,"completed_generation":4}),
            last_seen: 100,
        };
        let snapshot = agent_snapshot(&shutdown, 100);
        assert_eq!(snapshot["alive"], true);
        assert_eq!(snapshot["activity"], "unknown");
        assert_eq!(snapshot["settled"], false);
        assert_eq!(snapshot["reason"], "shutdown");
    }
    #[test]
    fn mismatched_generation_or_retry_cannot_be_settled() {
        let mut agent = Agent {
            value: json!({"activity":"idle","settled":true,"pid":std::process::id(),"generation":4,"completed_generation":3,"retrying":false}),
            last_seen: 100,
        };
        assert_eq!(agent_snapshot(&agent, 100)["settled"], false);
        agent.value["completed_generation"] = json!(4);
        agent.value["retrying"] = json!(true);
        assert_eq!(agent_snapshot(&agent, 100)["settled"], false);
        agent.value["pid"] = json!(0);
        assert_eq!(agent_snapshot(&agent, 100)["activity"], "unknown");
    }
}

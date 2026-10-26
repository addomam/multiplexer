//! Opt-in genuine OMP ACP acceptance. Parent supplies private auth and a real
//! provider-request budget gate; this driver never manufactures provider proof.
use super::*;
use serde::Deserialize;
use std::os::unix::fs::MetadataExt;

type Check<T> = Result<T, &'static str>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    command: Vec<String>,
    model: String,
    effort: String,
    expected_executable: PathBuf,
    budget_evidence: PathBuf,
}

fn require(value: bool, code: &'static str) -> Check<()> {
    if value { Ok(()) } else { Err(code) }
}
fn private(path: &Path, directory: bool) -> Check<()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "private-input-unavailable")?;
    require(
        !metadata.file_type().is_symlink()
            && metadata.uid() == unsafe { libc::geteuid() }
            && if directory {
                metadata.is_dir() && metadata.mode() & 0o777 == 0o700
            } else {
                metadata.is_file() && metadata.mode() & 0o777 == 0o600
            },
        "private-input-ownership-mode",
    )
}
fn spec() -> Check<Spec> {
    let path = PathBuf::from(
        std::env::var_os("AMUX_E06_OMP_LIVE_SPEC").ok_or("explicit-private-live-spec-required")?,
    );
    require(path.is_absolute(), "absolute-live-spec-required")?;
    private(&path, false)?;
    let bytes = fs::read(path).map_err(|_| "live-spec-read")?;
    require(bytes.len() <= 64 * 1024, "live-spec-budget")?;
    let spec: Spec = serde_json::from_slice(&bytes).map_err(|_| "live-spec-shape")?;
    require(
        !spec.command.is_empty()
            && spec.command.len() <= 256
            && spec.command.iter().map(String::len).sum::<usize>() <= 64 * 1024
            && spec.command.iter().all(|s| !s.contains('\0'))
            && Path::new(&spec.command[0]).is_absolute(),
        "bounded-absolute-launcher-required",
    )?;
    require(
        !spec.model.is_empty()
            && spec.model.len() <= 256
            && !spec.model.chars().any(char::is_control)
            && !spec.effort.is_empty()
            && spec.effort.len() <= 256
            && !spec.effort.chars().any(char::is_control),
        "exact-model-effort-required",
    )?;
    require(
        spec.expected_executable.is_absolute() && spec.expected_executable.is_file(),
        "actual-omp-executable-required",
    )?;
    require(
        spec.budget_evidence.is_absolute(),
        "absolute-budget-evidence-required",
    )?;
    private(
        spec.budget_evidence
            .parent()
            .ok_or("budget-evidence-parent")?,
        true,
    )?;
    require(
        !spec.budget_evidence.exists(),
        "fresh-budget-evidence-required",
    )?;
    Ok(spec)
}

// Never format a response, prompt, tool argument, PTY surface or private path.
fn request(host: &Host, mut value: Value, deadline: Instant) -> Check<Value> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or("live-deadline")?;
    let mut socket = UnixStream::connect(&host.socket).map_err(|_| "daemon-connect")?;
    socket
        .set_read_timeout(Some(remaining.min(DEADLINE)))
        .map_err(|_| "daemon-timeout")?;
    socket
        .set_write_timeout(Some(remaining.min(DEADLINE)))
        .map_err(|_| "daemon-timeout")?;
    value["version"] = json!(1);
    serde_json::to_writer(&mut socket, &value).map_err(|_| "daemon-write")?;
    socket.write_all(b"\n").map_err(|_| "daemon-write")?;
    let mut line = String::new();
    BufReader::new(socket)
        .take(8 * 1024 * 1024)
        .read_line(&mut line)
        .map_err(|_| "daemon-read")?;
    require(line.ends_with('\n'), "daemon-frame")?;
    let response: Value = serde_json::from_str(&line).map_err(|_| "daemon-frame")?;
    require(response["ok"] == true, "daemon-request-rejected")?;
    Ok(response["result"].clone())
}
fn poll<T>(deadline: Instant, mut probe: impl FnMut() -> Check<Option<T>>) -> Check<T> {
    loop {
        require(Instant::now() < deadline, "live-deadline")?;
        if let Some(value) = probe()? {
            return Ok(value);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn surface(native: &Attachment) -> Check<String> {
    let output = native.output.lock();
    require(output.error.is_none(), "pty-drainer-failed")?;
    Ok((0..ROWS)
        .map(|row| {
            (0..COLS)
                .map(|col| output.term.grid()[Point::new(Line(row as i32), Column(col))].c)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n"))
}
fn send(native: &mut Attachment, bytes: &[u8]) -> Check<()> {
    write_pty(&mut native.master, bytes).map_err(|_| "native-input-write")
}
fn rendered(native: &mut Attachment, marker: &str, deadline: Instant) -> Check<()> {
    poll(deadline, || {
        require(
            native
                .child
                .try_wait()
                .map_err(|_| "native-host-status")?
                .is_none(),
            "native-host-ended",
        )?;
        Ok(surface(native)?.contains(marker).then_some(()))
    })
}

#[cfg(target_os = "macos")]
fn process_executable(pid: i32) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    unsafe extern "C" {
        fn proc_pidpath(pid: i32, buffer: *mut libc::c_void, size: u32) -> i32;
    }
    let mut buffer = [0_u8; 4096];
    let size = unsafe { proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if size <= 0 {
        return None;
    }
    let end = buffer
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(size as usize);
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(&buffer[..end])))
}
#[cfg(not(target_os = "macos"))]
fn process_executable(pid: i32) -> Option<PathBuf> {
    fs::read_link(format!("/proc/{pid}/exe")).ok()
}

// Observe the actual pinned executable in the daemon-owned descendant tree,
// not a PID copied from the request or an invented launcher metadata record.
fn omp_pid(host: &Host, executable: &Path) -> Check<i32> {
    let expected = fs::canonicalize(executable).map_err(|_| "omp-executable-canonicalization")?;
    let output = Command::new("/bin/ps")
        .args(["-axo", "pid=,ppid="])
        .output()
        .map_err(|_| "process-observation")?;
    require(output.status.success(), "process-observation")?;
    let text = std::str::from_utf8(&output.stdout).map_err(|_| "process-observation")?;
    let mut records = Vec::new();
    for row in text.lines() {
        let mut fields = row.split_whitespace();
        if let (Some(pid), Some(parent)) = (fields.next(), fields.next())
            && let (Ok(pid), Ok(parent)) = (pid.parse::<i32>(), parent.parse::<i32>())
        {
            records.push((pid, parent));
        }
    }
    let mut descendants = std::collections::BTreeSet::from([host.daemon.id() as i32]);
    loop {
        let before = descendants.len();
        for (pid, parent) in &records {
            if descendants.contains(parent) {
                descendants.insert(*pid);
            }
        }
        if descendants.len() == before {
            break;
        }
    }
    let matching: Vec<_> = records
        .iter()
        .filter(|(pid, _)| {
            descendants.contains(pid)
                && process_executable(*pid)
                    .and_then(|path| fs::canonicalize(path).ok())
                    .is_some_and(|path| path == expected)
        })
        .map(|(pid, _)| *pid)
        .collect();
    require(matching.len() == 1, "exact-owned-omp-process-required")?;
    Ok(matching[0])
}

struct Live {
    host: Host,
    agent: Option<Agent>,
    native: Option<Attachment>,
    sentinel: Sentinel,
    pid: Option<i32>,
    deadline: Instant,
}
impl Live {
    fn new() -> Self {
        Self {
            host: Host::new(),
            agent: None,
            native: None,
            sentinel: Sentinel::new(),
            pid: None,
            deadline: Instant::now() + Duration::from_secs(120),
        }
    }
    fn agent(&self) -> Check<&Agent> {
        self.agent.as_ref().ok_or("live-session-missing")
    }
    fn projection(&self) -> Check<Value> {
        let projection = request(
            &self.host,
            json!({"op":"acp-inspect","target":self.agent()?.target,"include_content":true}),
            self.deadline,
        )?;
        require(
            projection["target"] == self.agent()?.target,
            "original-session-tuple-changed",
        )?;
        require(
            self.pid.is_none_or(|pid| !pid_gone(pid)),
            "original-omp-process-ended",
        )?;
        Ok(projection)
    }
    fn owner(&self, deadline: Instant) -> Check<Value> {
        let snapshot = request(&self.host, json!({"op":"snapshot"}), deadline)?;
        let id = &self.agent()?.target["id"];
        snapshot["agent_views"]
            .as_array()
            .ok_or("snapshot-shape")?
            .iter()
            .find(|view| view["id"] == *id)
            .map(|view| view["ownership"].clone())
            .ok_or("original-view-missing")
    }
    fn force(&mut self, deadline: Instant) -> Check<()> {
        let controller = request(
            &self.host,
            json!({"op":"controller-create","role":"external","label":"Private OMP acceptance"}),
            deadline,
        )?;
        let grant = request(
            &self.host,
            json!({"op":"ownership-claim","target":{"kind":"acp","id":self.agent()?.target["id"]},
            "controller":controller["controller"],"mode":"force","observed":self.owner(deadline)?["observed"]}),
            deadline,
        )?;
        let agent = self.agent.as_mut().ok_or("live-session-missing")?;
        agent.opened["control"] = grant["control"].clone();
        agent.opened["generation"] = grant["generation"].clone();
        agent.controller_id = controller["controller_id"].clone();
        Ok(())
    }
    fn work(&self, id: &Value) -> Check<Value> {
        self.projection()?["work"]
            .as_array()
            .ok_or("work-shape")?
            .iter()
            .find(|work| work["work_id"] == *id)
            .cloned()
            .ok_or("original-work-missing")
    }
    fn same_work(current: &Value, original: &Value) -> Check<()> {
        require(
            [
                "work_id",
                "call_id",
                "controller_id",
                "owner_generation",
                "rpc_id",
                "prompt_id",
            ]
            .iter()
            .all(|key| current[*key] == original[*key]),
            "original-work-changed",
        )
    }
    fn pending(&self, work: &Value, kind: &str) -> Check<Value> {
        poll(self.deadline, || {
            let projection = self.projection()?;
            let original = projection["work"]
                .as_array()
                .ok_or("work-shape")?
                .iter()
                .find(|item| item["work_id"] == work["work_id"])
                .ok_or("original-work-missing")?;
            Self::same_work(original, work)?;
            let interaction = projection["content"]["interactions"]
                .as_array()
                .ok_or("interaction-shape")?
                .iter()
                .find(|item| item["state"] == "pending")
                .cloned();
            if let Some(item) = interaction {
                require(item["kind"] == kind, "unexpected-live-interaction")?;
                return Ok(Some(item));
            }
            require(
                original["state"] != "completed" && original["state"] != "failed",
                "genuine-interaction-not-produced",
            )?;
            Ok(None)
        })
    }
    fn scenario(&mut self, spec: &Spec) -> Check<()> {
        let cwd = self.host.root.join("omp-work");
        fs::create_dir(&cwd).map_err(|_| "private-workspace-create")?;
        fs::set_permissions(&cwd, fs::Permissions::from_mode(0o700))
            .map_err(|_| "private-workspace-mode")?;
        for (name, file, marker) in [
            ("deny-effect", "deny", "amux-denied"),
            ("allow-effect", "allow", "amux-allowed"),
        ] {
            let helper = cwd.join(name);
            fs::write(
                &helper,
                format!("#!/bin/sh\nprintf '%s\\n' '{marker}' > './{file}.txt'\n"),
            )
            .map_err(|_| "private-effect-helper-create")?;
            fs::set_permissions(&helper, fs::Permissions::from_mode(0o700))
                .map_err(|_| "private-effect-helper-mode")?;
        }
        let snapshot = request(&self.host, json!({"op":"snapshot"}), self.deadline)?;
        let controller = request(
            &self.host,
            json!({"op":"controller-create","role":"external","label":"Private OMP acceptance"}),
            self.deadline,
        )?;
        let opened = request(
            &self.host,
            json!({"op":"acp-open","project":snapshot["projects"][0]["id"],
            "controller":controller["controller"],"command":spec.command,"cwd":cwd,"name":"Private OMP acceptance",
            "settings":{"model":spec.model,"effort":spec.effort}}),
            self.deadline,
        )?;
        self.agent = Some(Agent {
            target: opened["target"].clone(),
            opened,
            root: cwd.clone(),
            controller_id: controller["controller_id"].clone(),
        });
        let view = poll(self.deadline, || {
            let snapshot = request(&self.host, json!({"op":"snapshot"}), self.deadline)?;
            let id = &self.agent()?.target["id"];
            let view = snapshot["agent_views"]
                .as_array()
                .ok_or("snapshot-shape")?
                .iter()
                .find(|v| v["id"] == *id)
                .ok_or("original-view-missing")?;
            require(
                view["lifecycle"] != "ended"
                    && view["lifecycle"] != "failed"
                    && view["lifecycle"] != "auth-required",
                "omp-startup-failed",
            )?;
            Ok((view["lifecycle"] == "ready").then(|| view.clone()))
        })?;
        self.agent.as_mut().ok_or("live-session-missing")?.target = json!({"kind":"acp","id":view["id"],
            "instance_id":view["instance_id"],"connection_generation":view["connection_generation"],"session_id":view["session_id"]});
        require(
            view["configuration"]["state"] == "applied"
                && view["configuration"]["reported"]["model"]["current"] == spec.model
                && view["configuration"]["reported"]["effort"]["current"] == spec.effort,
            "real-startup-settings-not-confirmed",
        )?;
        self.pid = Some(omp_pid(&self.host, &spec.expected_executable)?);
        let deny = cwd.join("deny.txt");
        let allow = cwd.join("allow.txt");
        // Single executable tokens avoid depending on a model's incidental shell
        // quoting. The permission still gates the genuine Bash call and effect.
        let denied_command = "./deny-effect";
        let allowed_command = "./allow-effect";
        // One deliberate agent turn, not four prompts masquerading as four model
        // requests. The parent's provider gate counts every actual continuation.
        let prompt = format!(
            "Use your built-in ask tool once to ask one single-choice question with options alpha and beta. Wait for my structured answer. Then invoke built-in bash with command exactly {denied_command} and wait for its permission decision and tool result. Next invoke built-in bash with command exactly {allowed_command} and wait for its separate permission decision and tool result. Do not retry a denied call or substitute another tool. Use the current session working directory. After both tool results finish with a short final reply and no more tools. Do not use write/edit/MCP or access any other path. Keep all replies short."
        );
        let mut action = self.agent()?.action("acp-prompt", "e06-live-original-turn");
        action["text"] = json!(prompt);
        let receipt = request(&self.host, action, self.deadline)?;
        let work = poll(self.deadline, || {
            let work = self.work(&receipt["work_id"])?;
            Ok((!work["rpc_id"].is_null()).then_some(work))
        })?;
        let form = self.pending(&work, "form")?;
        let properties = form["schema"]["properties"]
            .as_object()
            .ok_or("omp-form-shape")?;
        require(
            properties.len() == 2 && properties.contains_key("q0__other"),
            "genuine-omp-single-question-required",
        )?;
        let choices = properties["q0"]["oneOf"]
            .as_array()
            .ok_or("omp-choice-shape")?;
        require(
            choices.len() == 2
                && choices.iter().all(|choice| choice["const"].is_string())
                && choices[0]["const"] != choices[1]["const"],
            "offered-question-choices",
        )?;
        let selected = choices[0]["const"].clone();
        request(
            &self.host,
            json!({"op":"config","config":{"prefix":"Ctrl-a"}}),
            self.deadline,
        )?;
        request(
            &self.host,
            json!({"op":"control-handle","handle":self.agent()?.opened["control"],"method":"release"}),
            self.deadline,
        )?;
        self.native = Some(Attachment::new(&self.host, self.agent()?));
        let native = self.native.as_mut().ok_or("native-missing")?;
        rendered(native, "owner (input-admission)", self.deadline)?;
        rendered(
            native,
            &format!(
                "{} | revision ",
                form["interaction_id"].as_str().ok_or("form-id-shape")?
            ),
            self.deadline,
        )?;
        send(native, b"\x1b[14~\x1b[D")?;
        send(native, ACCEPT_FORM)?;
        rendered(native, "REVIEW: F5 accepts", self.deadline)?;
        send(native, ACCEPT_FORM)?;
        poll(self.deadline, || {
            let projection = self.projection()?;
            let answer = projection["content"]["interactions"]
                .as_array()
                .ok_or("interaction-shape")?
                .iter()
                .find(|item| item["interaction_id"] == form["interaction_id"])
                .ok_or("original-form-missing")?;
            if answer["state"] == "pending" {
                return Ok(None);
            }
            require(
                answer["selection"]["action"] == "accept"
                    && answer["selection"]["content"]["q0"] == selected,
                "native-answer-not-exact",
            )?;
            Ok((answer["delivery"] == "submitted").then_some(()))
        })?;
        let denied_permission = self.pending(&work, "permission")?;
        require(
            !deny.exists() && !allow.exists(),
            "file-effect-before-permission",
        )?;
        require(
            denied_permission["tool_call"]["rawInput"]["command"] == denied_command,
            "unconfined-denied-command",
        )?;
        self.force(self.deadline)?;
        rendered(
            self.native.as_mut().ok_or("native-missing")?,
            "observer (read-only)",
            self.deadline,
        )?;
        let still_pending = self.pending(&work, "permission")?;
        require(
            still_pending["interaction_id"] == denied_permission["interaction_id"]
                && still_pending["revision"] == denied_permission["revision"],
            "force-replaced-original-permission",
        )?;
        for (permission, kind, command) in [
            (denied_permission, "reject_once", denied_command),
            (Value::Null, "allow_once", allowed_command),
        ] {
            let permission = if permission.is_null() {
                self.pending(&work, "permission")?
            } else {
                permission
            };
            require(
                permission["tool_call"]["rawInput"]["command"] == command,
                "unconfined-permission-command",
            )?;
            require(!deny.exists() && !allow.exists(), "unapproved-file-effect")?;
            let options = permission["options"]
                .as_array()
                .ok_or("permission-options-shape")?;
            let option = options
                .iter()
                .find(|option| option["kind"] == kind)
                .ok_or("once-permission-not-offered")?;
            let mut action = self.agent()?.action(
                "acp-permission",
                if kind == "reject_once" {
                    "e06-live-deny"
                } else {
                    "e06-live-allow"
                },
            );
            action["interaction_id"] = permission["interaction_id"].clone();
            action["revision"] = permission["revision"].clone();
            action["option_id"] = option["option_id"].clone();
            request(&self.host, action, self.deadline)?;
            poll(self.deadline, || {
                let projection = self.projection()?;
                let item = projection["content"]["interactions"]
                    .as_array()
                    .ok_or("interaction-shape")?
                    .iter()
                    .find(|item| item["interaction_id"] == permission["interaction_id"])
                    .ok_or("permission-missing")?;
                require(
                    item["state"] == "pending"
                        || item["selection"]["option_id"] == option["option_id"],
                    "permission-selection-changed",
                )?;
                Ok((item["delivery"] == "submitted").then_some(()))
            })?;
        }
        poll(self.deadline, || {
            let current = self.work(&work["work_id"])?;
            Self::same_work(&current, &work)?;
            require(current["state"] != "failed", "live-turn-failed")?;
            if current["state"] == "completed" {
                require(
                    current["outcome"]["stop_reason"] == "end_turn",
                    "implicit-turn-cancellation",
                )?;
            }
            Ok((current["state"] == "completed").then_some(()))
        })?;
        require(
            !deny.exists() && fs::read(&allow).is_ok_and(|bytes| bytes == b"amux-allowed\n"),
            "real-deny-allow-effects",
        )?;
        require(
            omp_pid(&self.host, &spec.expected_executable)?
                == self.pid.ok_or("original-pid-missing")?,
            "original-omp-pid-changed",
        )?;
        let projection = self.projection()?;
        require(
            projection["work"]
                .as_array()
                .ok_or("work-shape")?
                .iter()
                .all(|w| w["op"] != "acp-interrupt"),
            "implicit-interrupt",
        )?;
        require(
            projection["configuration"]["reported"]["model"]["current"] == spec.model
                && projection["configuration"]["reported"]["effort"]["current"] == spec.effort,
            "unapproved-setting-drift",
        )?;
        Ok(())
    }
    fn cleanup(&mut self) -> Check<()> {
        let deadline = Instant::now() + DEADLINE;
        let detached = (|| {
            let Some(native) = self.native.as_mut() else {
                return Ok(());
            };
            send(native, PREFIX)?;
            send(native, &[17])?;
            poll(deadline, || {
                Ok(fs::read_to_string(&native.tui_exit)
                    .ok()
                    .and_then(|text| text.trim().parse::<i32>().ok()))
            })
            .and_then(|code| require(code == 0, "tui-detach-failed"))?;
            let mut restored: libc::termios = unsafe { std::mem::zeroed() };
            require(
                unsafe { libc::tcgetattr(native.slave.as_raw_fd(), &mut restored) } == 0
                    && restored.c_iflag == native.original.c_iflag
                    && restored.c_oflag == native.original.c_oflag
                    && restored.c_cflag == native.original.c_cflag
                    && restored.c_lflag == native.original.c_lflag
                    && restored.c_cc == native.original.c_cc,
                "native-termios-not-restored",
            )?;
            send(native, b"finish\n")?;
            poll(deadline, || {
                match native.child.try_wait().map_err(|_| "native-host-status")? {
                    Some(status) => {
                        require(status.success(), "native-host-exit")?;
                        Ok(Some(()))
                    }
                    None => Ok(None),
                }
            })?;
            native.stop.store(true, Ordering::Release);
            if let Some(reader) = native.reader.as_ref() {
                poll(deadline, || Ok(reader.is_finished().then_some(())))?;
            }
            if let Some(reader) = native.reader.take() {
                reader.join().map_err(|_| "native-drainer-join")?;
            }
            require(
                native.output.lock().error.is_none(),
                "native-drainer-failure",
            )
        })();
        let closed = (|| {
            if self.agent.is_none() {
                return Ok(());
            }
            let projection = request(
                &self.host,
                json!({"op":"acp-inspect","target":self.agent()?.target}),
                deadline,
            )?;
            if matches!(
                projection["lifecycle"].as_str(),
                Some("failed" | "ended" | "closing")
            ) {
                return poll(deadline, || {
                    let projection = request(
                        &self.host,
                        json!({"op":"acp-inspect","target":self.agent()?.target}),
                        deadline,
                    )?;
                    Ok((projection["resource"]["cleanup_complete"] == true
                        && self.pid.is_none_or(pid_gone))
                    .then_some(()))
                });
            }
            self.force(deadline)?;
            request(
                &self.host,
                self.agent()?.action("acp-close", "e06-live-explicit-close"),
                deadline,
            )?;
            poll(deadline, || {
                let projection = request(
                    &self.host,
                    json!({"op":"acp-inspect","target":self.agent()?.target}),
                    deadline,
                )?;
                Ok((projection["resource"]["cleanup_complete"] == true
                    && projection["lifecycle"] == "ended"
                    && self.pid.is_none_or(pid_gone))
                .then_some(()))
            })
        })();
        let shutdown = (|| {
            request(&self.host, json!({"op":"shutdown"}), deadline)?;
            poll(deadline, || {
                match self.host.daemon.try_wait().map_err(|_| "daemon-status")? {
                    Some(status) => {
                        require(status.success(), "daemon-exit")?;
                        Ok(Some(()))
                    }
                    None => Ok(None),
                }
            })?;
            require(!self.host.socket.exists(), "socket-survived")?;
            require(
                self.sentinel
                    .0
                    .try_wait()
                    .map_err(|_| "sentinel-status")?
                    .is_none(),
                "sentinel-ended",
            )?;
            detached?;
            closed?;
            fs::remove_dir_all(&self.host.root).map_err(|_| "private-root-cleanup")?;
            require(!self.host.root.exists(), "private-root-survived")
        })();
        detached.and(closed).and(shutdown)
    }
}

fn budget(spec: &Spec) -> Check<()> {
    private(&spec.budget_evidence, false)?;
    let bytes = fs::read(&spec.budget_evidence).map_err(|_| "budget-evidence-read")?;
    require(bytes.len() <= 64 * 1024, "budget-evidence-budget")?;
    let evidence: Value = serde_json::from_slice(&bytes).map_err(|_| "budget-evidence-shape")?;
    let count = evidence["model_requests"]
        .as_u64()
        .ok_or("real-model-request-count-required")?;
    let maximum = evidence["max_model_requests"]
        .as_u64()
        .ok_or("real-model-request-maximum-required")?;
    require(
        (1..=maximum).contains(&count)
            && maximum <= 4
            && evidence["max_requested_output_tokens"] == 2000
            && evidence["wall_seconds"]
                .as_u64()
                .is_some_and(|seconds| (1..=120).contains(&seconds))
            && evidence["model"] == spec.model
            && evidence["effort"] == spec.effort
            && evidence["subscription_only"] == true
            && evidence["premium_or_api_key_fallback"] == false
            && evidence["deadline_enforced"] == true
            && evidence["deadline_expired"] == false
            // Catalog/update GETs rejected before transport consume no model
            // request. Any denied mutation or generation is not acceptance.
            && evidence["blocked_requests"].as_u64().is_some_and(|count| {
                evidence["blocked_readonly_requests"].as_u64() == Some(count)
            }),
        "real-provider-budget-not-proved",
    )?;
    let requests = evidence["requests"]
        .as_array()
        .ok_or("real-request-records-required")?;
    require(
        requests.len() as u64 == count
            && requests.iter().all(|request| {
                request["model"] == spec.model
                    && request["effort"] == spec.effort
                    && request["requested_output_tokens"]
                        .as_u64()
                        .is_some_and(|n| (1..=2000).contains(&n))
            }),
        "real-provider-request-bound",
    )
}

pub(crate) fn run() {
    let spec = spec().unwrap_or_else(|code| panic!("OMP live input rejected: {code}"));
    let mut live = Live::new();
    let result = live.scenario(&spec).and_then(|()| budget(&spec));
    let cleanup = live.cleanup();
    if let Err(code) = result {
        eprintln!("e06 OMP live scenario FAIL category={code}");
    }
    if let Err(code) = cleanup {
        eprintln!("e06 OMP live cleanup FAIL category={code}");
    }
    assert!(
        result.is_ok() && cleanup.is_ok(),
        "genuine OMP acceptance did not pass"
    );
    eprintln!(
        "e06 OMP live PASS genuine_question=true native_answer=true api_handoff=true deny_effect_absent=true allow_effect_present=true original_tuple_pid_work=true implicit_cancel=false provider_budget=true owned_cleanup=true"
    );
}

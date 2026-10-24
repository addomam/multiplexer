//! Opt-in real-provider acceptance. Only the root ignored test calls `run`.
//! Configuration and wire-derived model evidence are private, parent-owned inputs.
use super::*;
use serde::Deserialize;
use std::cell::Cell;
use std::os::unix::fs::MetadataExt;

thread_local! {
    static LIVE_DEADLINE: Cell<Option<Instant>> = const { Cell::new(None) };
}

type Check<T> = Result<T, &'static str>;
const QUESTION: &str = "Use your built-in structured question/request_user_input tool exactly once to ask me to choose between alpha and beta. Do not ask in plain prose, use MCP, read or write files, run commands, or invent an answer. Wait for my structured answer, then finish this turn with a short acknowledgement and no further tools. If the built-in structured tool is unavailable, say so and finish without substituting another tool.";
const CONTENT: &[u8] = b"amux-live-approved\n";
const ROWS_LIVE: [&str; 4] = [
    "api-question-native-answer",
    "native-question-force-api-answer",
    "permission-allow-effect",
    "permission-deny-no-effect",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Configuration {
    approval_id: String,
    approved: bool,
    max_prompts_per_adapter: u8,
    max_seconds_per_adapter: u64,
    adapters: Vec<Adapter>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Adapter {
    id: String,
    command: Vec<String>,
    cwd: PathBuf,
    expected_model: String,
    model_evidence: PathBuf,
    auth_method_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelEvidence {
    session_id: String,
    model_id: String,
    pid: i32,
    reasoning_effort: Option<String>,
    counts: Option<WireCounts>,
    #[serde(rename = "question_shape")]
    _question_shape: Option<Value>,
    #[serde(rename = "prompt_stop_reasons")]
    _prompt_stop_reasons: Option<Vec<String>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireCounts {
    prompt_requests: u64,
    form_requests: u64,
    permission_requests: u64,
    cancellations: u64,
    local_commands: u64,
}
fn wire_counts(model: &ModelEvidence, prompts: u8) -> Check<()> {
    if let Some(counts) = &model.counts {
        require(
            counts.local_commands <= 2
                && counts.prompt_requests <= u64::from(prompts) + counts.local_commands
                && counts.prompt_requests <= 6,
            "wire-prompt-bound",
        )?;
        require(counts.cancellations == 0, "unexpected-wire-cancellation")?;
        require(
            counts.form_requests <= 128 && counts.permission_requests <= 128,
            "wire-interaction-bound",
        )?;
    }
    Ok(())
}

fn require(condition: bool, reason: &'static str) -> Check<()> {
    if condition { Ok(()) } else { Err(reason) }
}
fn private(path: &Path, directory: bool) -> Check<()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "private-input-unavailable")?;
    require(!metadata.file_type().is_symlink(), "private-input-symlink")?;
    require(
        metadata.uid() == unsafe { libc::geteuid() },
        "private-input-owner",
    )?;
    require(
        if directory {
            metadata.is_dir()
        } else {
            metadata.is_file()
        },
        "private-input-type",
    )?;
    require(
        metadata.permissions().mode() & 0o777 == if directory { 0o700 } else { 0o600 },
        "private-input-mode",
    )
}
fn absent(path: &Path) -> bool {
    fs::symlink_metadata(path).is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
}
fn configuration() -> Check<Configuration> {
    let path = PathBuf::from(
        std::env::var_os("AMUX_ACP_LIVE_CONFIG").ok_or("explicit-live-config-required")?,
    );
    require(path.is_absolute(), "config-must-be-absolute")?;
    private(&path, false)?;
    let bytes = fs::read(&path).map_err(|_| "config-read")?;
    require(bytes.len() <= 64 * 1024, "config-size")?;
    let config: Configuration = serde_json::from_slice(&bytes).map_err(|_| "config-json")?;
    require(
        config.approved && !config.approval_id.trim().is_empty(),
        "explicit-approval-required",
    )?;
    require(
        config.max_prompts_per_adapter == 4,
        "four-prompt-approval-required",
    )?;
    require(
        (20..=240).contains(&config.max_seconds_per_adapter),
        "time-bound-required",
    )?;
    require(config.adapters.len() == 2, "exact-two-adapters-required")?;
    for id in ["codex", "grok"] {
        require(
            config
                .adapters
                .iter()
                .filter(|adapter| adapter.id == id)
                .count()
                == 1,
            "exact-adapter-ids-required",
        )?;
    }
    for adapter in &config.adapters {
        require(
            !adapter.command.is_empty() && adapter.command.len() <= 256,
            "adapter-command-required",
        )?;
        require(
            Path::new(&adapter.command[0]).is_absolute(),
            "absolute-adapter-required",
        )?;
        require(
            adapter.command.iter().all(|arg| !arg.contains('\0')),
            "invalid-command-argument",
        )?;
        let executable =
            fs::metadata(&adapter.command[0]).map_err(|_| "adapter-executable-unavailable")?;
        require(
            executable.is_file() && executable.permissions().mode() & 0o111 != 0,
            "adapter-not-executable",
        )?;
        require(adapter.cwd.is_absolute(), "absolute-cwd-required")?;
        private(&adapter.cwd, true)?;
        require(
            fs::canonicalize(&adapter.cwd).map_err(|_| "cwd-canonicalization")? == adapter.cwd,
            "canonical-cwd-required",
        )?;
        require(
            !adapter.expected_model.trim().is_empty(),
            "explicit-model-required",
        )?;
        require(
            adapter.model_evidence.is_absolute(),
            "absolute-model-evidence-required",
        )?;
        private(
            adapter
                .model_evidence
                .parent()
                .ok_or("evidence-parent-required")?,
            true,
        )?;
        require(
            absent(&adapter.model_evidence),
            "fresh-model-evidence-required",
        )?;
        require(
            adapter
                .auth_method_id
                .as_ref()
                .is_none_or(|id| !id.trim().is_empty()),
            "invalid-auth-choice",
        )?;
        for name in ["amux-live-allow.txt", "amux-live-deny.txt"] {
            require(
                absent(&adapter.cwd.join(name)),
                "fresh-effect-path-required",
            )?;
        }
    }
    require(
        config.adapters[0].cwd != config.adapters[1].cwd,
        "distinct-private-workdirs-required",
    )?;
    require(
        config.adapters[0].model_evidence != config.adapters[1].model_evidence,
        "distinct-model-evidence-required",
    )?;
    Ok(config)
}

// No response/projection/error text is ever formatted into a diagnostic.
fn wire(host: &Host, mut value: Value) -> Check<Value> {
    let remaining = LIVE_DEADLINE
        .with(|bound| bound.get())
        .and_then(|deadline| deadline.checked_duration_since(Instant::now()))
        .filter(|remaining| !remaining.is_zero())
        .ok_or("adapter-time-bound")?;
    let mut stream = UnixStream::connect(&host.socket).map_err(|_| "daemon-connect")?;
    stream
        .set_read_timeout(Some(remaining.min(DEADLINE)))
        .map_err(|_| "daemon-timeout")?;
    stream
        .set_write_timeout(Some(remaining.min(DEADLINE)))
        .map_err(|_| "daemon-timeout")?;
    value["version"] = json!(1);
    serde_json::to_writer(&mut stream, &value).map_err(|_| "daemon-write")?;
    stream.write_all(b"\n").map_err(|_| "daemon-write")?;
    let mut line = String::new();
    BufReader::new(stream)
        .take(8 * 1024 * 1024)
        .read_line(&mut line)
        .map_err(|_| "daemon-read")?;
    require(line.ends_with('\n'), "daemon-response-incomplete")?;
    serde_json::from_str(&line).map_err(|_| "daemon-response-json")
}
fn request(host: &Host, value: Value) -> Check<Value> {
    let response = wire(host, value)?;
    require(response["ok"] == true, "daemon-request-rejected")?;
    Ok(response["result"].clone())
}
fn poll<T>(deadline: Instant, mut probe: impl FnMut() -> Check<Option<T>>) -> Check<T> {
    loop {
        require(Instant::now() < deadline, "bounded-deadline")?;
        if let Some(value) = probe()? {
            return Ok(value);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn surface(native: &Attachment) -> Check<String> {
    let output = native.output.lock();
    require(output.error.is_none(), "pty-drainer-failure")?;
    Ok((0..ROWS)
        .map(|row| {
            (0..COLS)
                .map(|col| output.term.grid()[Point::new(Line(row as i32), Column(col))].c)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n"))
}
fn send(native: &mut Attachment, mut bytes: &[u8]) -> Check<()> {
    while !bytes.is_empty() {
        require(
            LIVE_DEADLINE
                .with(|bound| bound.get())
                .is_some_and(|deadline| Instant::now() < deadline),
            "adapter-time-bound",
        )?;
        match native.master.write(bytes) {
            Ok(0) => return Err("pty-write-zero"),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(2))
            }
            Err(_) => return Err("pty-write"),
        }
    }
    Ok(())
}
fn rendered(native: &mut Attachment, marker: &str, deadline: Instant) -> Check<()> {
    poll(deadline, || {
        require(
            native
                .child
                .try_wait()
                .map_err(|_| "ctty-status")?
                .is_none(),
            "ctty-ended-early",
        )?;
        Ok(surface(native)?.contains(marker).then_some(()))
    })
}

struct Live {
    // Retire the real attachment before the real daemon on any error path.
    native: Option<Attachment>,
    host: Host,
    sentinel: Sentinel,
    agent: Option<Agent>,
    model: Option<ModelEvidence>,
    deadline: Instant,
    prompts: u8,
    local_commands: u8,
}
impl Live {
    fn new(seconds: u64) -> Check<Self> {
        let deadline = Instant::now() + Duration::from_secs(seconds);
        LIVE_DEADLINE.with(|bound| bound.set(Some(deadline)));
        let sentinel = Sentinel(
            Command::new("/bin/sleep")
                .arg("600")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| "sentinel-launch")?,
        );
        Ok(Self {
            native: None,
            host: Host::new(),
            sentinel,
            agent: None,
            model: None,
            deadline,
            prompts: 0,
            local_commands: 0,
        })
    }
    fn agent(&self) -> Check<&Agent> {
        self.agent.as_ref().ok_or("session-not-open")
    }
    fn projection(&self) -> Check<Value> {
        let agent = self.agent()?;
        let projection = request(
            &self.host,
            json!({"op":"acp-inspect","target":agent.target,"include_content":true}),
        )?;
        require(
            projection["target"] == agent.target,
            "exact-session-changed",
        )?;
        if let Some(model) = &self.model {
            require(!pid_gone(model.pid), "adapter-process-ended")?;
        }
        Ok(projection)
    }
    fn ownership(&self) -> Check<Value> {
        let id = &self.agent()?.target["id"];
        let snapshot = request(&self.host, json!({"op":"snapshot"}))?;
        Ok(snapshot["agent_views"]
            .as_array()
            .ok_or("snapshot-shape")?
            .iter()
            .find(|view| view["id"] == *id)
            .ok_or("session-view-missing")?["ownership"]
            .clone())
    }
    fn open(&mut self, adapter: &Adapter, deadline: Instant) -> Check<()> {
        require(
            absent(&adapter.model_evidence),
            "fresh-model-evidence-required",
        )?;
        let snapshot = request(&self.host, json!({"op":"snapshot"}))?;
        let project = snapshot["projects"]
            .as_array()
            .and_then(|p| p.first())
            .ok_or("project-missing")?["id"]
            .clone();
        let controller = request(
            &self.host,
            json!({"op":"controller-create","role":"external","label":"Private live acceptance"}),
        )?;
        let opened = request(
            &self.host,
            json!({"op":"acp-open","project":project,"controller":controller["controller"],
            "command":adapter.command,"cwd":adapter.cwd,"name":"Private live acceptance"}),
        )?;
        self.agent = Some(Agent {
            target: opened["target"].clone(),
            opened,
            root: adapter.cwd.clone(),
            controller_id: controller["controller_id"].clone(),
        });
        let mut authenticated = false;
        let view = poll(deadline, || {
            let id = &self.agent()?.target["id"];
            let snapshot = request(&self.host, json!({"op":"snapshot"}))?;
            let view = snapshot["agent_views"]
                .as_array()
                .ok_or("snapshot-shape")?
                .iter()
                .find(|view| view["id"] == *id)
                .ok_or("session-view-missing")?
                .clone();
            match view["lifecycle"].as_str() {
                Some("ready") => Ok(Some(view)),
                Some("auth-required") => {
                    let agent = self.agent()?;
                    let projection = request(
                        &self.host,
                        json!({"op":"acp-inspect","target":agent.target,"include_content":true}),
                    )?;
                    if authenticated {
                        require(
                            !projection["work"].as_array().is_some_and(|items| {
                                items.iter().any(|work| {
                                    work["op"] == "acp-authenticate"
                                        && (work["state"] == "completed"
                                            || work["state"] == "failed")
                                })
                            }),
                            "explicit-authentication-failed",
                        )?;
                        return Ok(None);
                    }
                    let method = adapter
                        .auth_method_id
                        .as_deref()
                        .ok_or("explicit-auth-choice-required")?;
                    require(
                        projection["authentication"]
                            .as_array()
                            .is_some_and(|methods| {
                                methods.iter().any(|item| {
                                    item["method_id"] == method && item["kind"] == "agent"
                                })
                            }),
                        "chosen-auth-method-not-advertised",
                    )?;
                    let mut action = agent.action("acp-authenticate", "live-explicit-auth");
                    action["method_id"] = json!(method);
                    request(&self.host, action)?;
                    authenticated = true;
                    Ok(None)
                }
                Some("starting") => Ok(None),
                _ => Err("adapter-bootstrap-failed"),
            }
        })?;
        let agent = self.agent.as_mut().ok_or("session-not-open")?;
        agent.target = json!({"kind":"acp","id":agent.target["id"],"instance_id":view["instance_id"],
            "connection_generation":view["connection_generation"],"session_id":view["session_id"]});
        let model = poll(deadline, || {
            if absent(&adapter.model_evidence) {
                return Ok(None);
            }
            private(&adapter.model_evidence, false)?;
            let bytes = fs::read(&adapter.model_evidence).map_err(|_| "model-evidence-read")?;
            require(bytes.len() <= 16 * 1024, "model-evidence-size")?;
            let model: ModelEvidence =
                serde_json::from_slice(&bytes).map_err(|_| "model-evidence-json")?;
            require(
                model.pid > 1 && !pid_gone(model.pid),
                "model-process-not-live",
            )?;
            require(model.model_id == adapter.expected_model, "unapproved-model")?;
            require(
                self.agent()?.target["session_id"] == model.session_id,
                "model-session-mismatch",
            )?;
            wire_counts(&model, self.prompts)?;
            Ok(Some(model))
        })?;
        self.model = Some(model);
        self.projection()?;
        Ok(())
    }
    fn stable_model(&self, adapter: &Adapter) -> Check<()> {
        private(&adapter.model_evidence, false)?;
        let bytes = fs::read(&adapter.model_evidence).map_err(|_| "model-evidence-read")?;
        let now: ModelEvidence =
            serde_json::from_slice(&bytes).map_err(|_| "model-evidence-json")?;
        let original = self.model.as_ref().ok_or("model-unobserved")?;
        require(
            now.session_id == original.session_id
                && now.model_id == original.model_id
                && now.pid == original.pid
                && now.reasoning_effort == original.reasoning_effort,
            "model-or-process-changed",
        )?;
        wire_counts(&now, self.prompts)?;
        require(!pid_gone(now.pid), "adapter-process-ended")
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
    fn idle(&self) -> Check<()> {
        require(
            self.projection()?["foreground"].is_null(),
            "previous-turn-still-pending",
        )
    }
    fn codex_plan_toggle(&mut self, deadline: Instant) -> Check<()> {
        self.idle()?;
        require(self.local_commands < 2, "local-command-bound")?;
        self.local_commands += 1;
        let mut action = self.agent()?.action(
            "acp-prompt",
            &format!("live-local-plan-{}", self.local_commands),
        );
        action["text"] = json!("/plan");
        let receipt = request(&self.host, action)?;
        let work = self.work(&receipt["work_id"])?;
        self.complete(&work, deadline, false)
    }
    fn prompt(&mut self, text: &str, native: bool, deadline: Instant) -> Check<Value> {
        self.idle()?;
        require(self.prompts < 4, "four-prompt-bound")?;
        // Count before any dispatch: even an uncertain write is never retried.
        self.prompts += 1;
        if !native {
            let mut action = self
                .agent()?
                .action("acp-prompt", &format!("live-prompt-{}", self.prompts));
            action["text"] = json!(text);
            let receipt = request(&self.host, action)?;
            return self.work(&receipt["work_id"]);
        }
        let previous = self.projection()?["work"]
            .as_array()
            .ok_or("work-shape")?
            .clone();
        self.history(deadline)?;
        let attachment = self.native.as_mut().ok_or("native-not-attached")?;
        rendered(attachment, "Foreground: none", deadline)?;
        send(attachment, format!("\x1b[200~{text}\x1b[201~").as_bytes())?;
        send(attachment, b"\r")?;
        poll(deadline, || {
            Ok(self.projection()?["work"]
                .as_array()
                .ok_or("work-shape")?
                .iter()
                .find(|work| {
                    work["op"] == "acp-prompt"
                        && !previous.iter().any(|old| old["work_id"] == work["work_id"])
                })
                .cloned())
        })
    }
    fn interaction(&self, work: &Value, kind: &str, deadline: Instant) -> Check<Value> {
        poll(deadline, || {
            let projection = self.projection()?;
            let original = self.work(&work["work_id"])?;
            if let Some(item) = projection["content"]["interactions"]
                .as_array()
                .ok_or("interaction-shape")?
                .iter()
                .find(|item| item["kind"] == kind && item["state"] == "pending")
            {
                require(
                    original["outcome"].is_null()
                        && projection["foreground"]["prompt_id"] == work["prompt_id"],
                    "question-not-original-pending-turn",
                )?;
                require(
                    item["scope"]["sessionId"] == self.agent()?.target["session_id"]
                        || item["scope"]["requestId"] == original["rpc_id"],
                    "interaction-scope-mismatch",
                )?;
                return Ok(Some(item.clone()));
            }
            require(
                original["state"] != "completed" && original["state"] != "failed",
                "genuine-interaction-unavailable",
            )?;
            Ok(None)
        })
    }
    fn complete(&self, work: &Value, deadline: Instant, denied: bool) -> Check<()> {
        poll(deadline, || {
            let current = self.work(&work["work_id"])?;
            same_work(&current, work)?;
            require(current["state"] != "failed", "original-turn-failed")?;
            if current["state"] == "completed" {
                require(
                    current["outcome"]["stop_reason"] == "end_turn"
                        || (denied
                            && matches!(
                                current["outcome"]["stop_reason"].as_str(),
                                Some("refusal" | "cancelled")
                            )),
                    "original-turn-unexpected-stop-reason",
                )?;
                return Ok(Some(()));
            }
            Ok(None)
        })
    }
    fn selected(
        &self,
        interaction: &Value,
        selection: &Value,
        controller: &Value,
        generation: &Value,
        deadline: Instant,
    ) -> Check<()> {
        poll(deadline, || {
            let projection = self.projection()?;
            let item = projection["content"]["interactions"]
                .as_array()
                .ok_or("interaction-shape")?
                .iter()
                .find(|item| item["interaction_id"] == interaction["interaction_id"])
                .ok_or("interaction-lost")?;
            if item["state"] != "selected" {
                return Ok(None);
            }
            require(
                item["revision"] == interaction["revision"]
                    && item["scope"] == interaction["scope"]
                    && item["selection"] == *selection,
                "selection-mismatch",
            )?;
            let response = self.work(&item["selected_work_id"])?;
            require(
                response["interaction_id"] == interaction["interaction_id"]
                    && response["controller_id"] == *controller
                    && response["owner_generation"] == *generation,
                "response-authority-mismatch",
            )?;
            Ok(Some(()))
        })
    }
    fn force_api(&mut self) -> Check<()> {
        let observed = self.ownership()?["observed"].clone();
        let controller = request(
            &self.host,
            json!({"op":"controller-create","role":"external","label":"Private live handoff"}),
        )?;
        let grant = request(
            &self.host,
            json!({"op":"ownership-claim","target":target(self.agent()?),
            "controller":controller["controller"],"mode":"force","observed":observed}),
        )?;
        let agent = self.agent.as_mut().ok_or("session-not-open")?;
        agent.opened["control"] = grant["control"].clone();
        agent.opened["generation"] = grant["generation"].clone();
        agent.controller_id = controller["controller_id"].clone();
        Ok(())
    }
    fn attach(&mut self, deadline: Instant) -> Check<()> {
        request(
            &self.host,
            json!({"op":"config","config":{"prefix":"Ctrl-a"}}),
        )?;
        request(
            &self.host,
            json!({"op":"control-handle","handle":self.agent()?.opened["control"],"method":"release"}),
        )?;
        self.native = Some(Attachment::new(&self.host, self.agent()?));
        rendered(
            self.native.as_mut().ok_or("native-not-attached")?,
            "owner (input-admission)",
            deadline,
        )?;
        require(
            self.ownership()?["owner"]["role"] == "attachment",
            "native-not-owner",
        )
    }
    fn native_owner(&mut self, deadline: Instant) -> Check<()> {
        if self.ownership()?["owner"]["role"] == "attachment" {
            return Ok(());
        }
        self.force_api()?;
        request(
            &self.host,
            json!({"op":"control-handle","handle":self.agent()?.opened["control"],"method":"release"}),
        )?;
        let native = self.native.as_mut().ok_or("native-not-attached")?;
        send(native, PREFIX)?;
        send(native, &[12])?;
        rendered(native, "owner (input-admission)", deadline)?;
        require(
            self.ownership()?["owner"]["role"] == "attachment",
            "native-not-owner",
        )?;
        // Dismiss the actual ownership panel before application keys.
        send(self.native.as_mut().ok_or("native-not-attached")?, b"\x1b")?;
        poll(deadline, || {
            let text = surface(self.native.as_ref().ok_or("native-not-attached")?)?;
            Ok(
                (!text.contains("Control owned here") && !text.contains("Control requested"))
                    .then_some(()),
            )
        })
    }
    fn history(&mut self, deadline: Instant) -> Check<()> {
        self.show(None, deadline)
    }
    fn show(&mut self, wanted: Option<&Value>, deadline: Instant) -> Check<()> {
        let projection = self.projection()?;
        let ids = projection["content"]["interactions"]
            .as_array()
            .ok_or("interaction-shape")?
            .iter()
            .map(|item| {
                item["interaction_id"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or("interaction-id-shape")
            })
            .collect::<Check<Vec<_>>>()?;
        let wanted = wanted.and_then(|item| item["interaction_id"].as_str());
        let visible = |text: &str| {
            text.lines()
                .find(|line| line.contains(" | revision "))
                .and_then(|line| ids.iter().find(|id| line.contains(id.as_str())))
                .cloned()
        };
        for _ in 0..=ids.len() + 1 {
            let current = visible(&surface(
                self.native.as_ref().ok_or("native-not-attached")?,
            )?);
            if current.as_deref() == wanted {
                return Ok(());
            }
            send(
                self.native.as_mut().ok_or("native-not-attached")?,
                b"\x1b[20~",
            )?;
            poll(deadline, || {
                Ok((visible(&surface(
                    self.native.as_ref().ok_or("native-not-attached")?,
                )?) != current)
                    .then_some(()))
            })?;
        }
        Err("native-interaction-not-reachable")
    }
    fn question_native(&mut self, deadline: Instant) -> Check<()> {
        self.force_api()?;
        let work = self.prompt(QUESTION, false, deadline)?;
        let form = self.interaction(&work, "form", deadline)?;
        if self.native.is_none() {
            self.attach(deadline)?;
        } else {
            self.native_owner(deadline)?;
        }
        let owner = self.ownership()?;
        let (values, edits) = form_values(&form)?;
        rendered(
            self.native.as_mut().ok_or("native-not-attached")?,
            form["interaction_id"]
                .as_str()
                .ok_or("interaction-id-shape")?,
            deadline,
        )?;
        self.show(Some(&form), deadline)?;
        let native = self.native.as_mut().ok_or("native-not-attached")?;
        for (index, edit) in edits.iter().enumerate() {
            if index > 0 {
                send(native, b"\t")?;
            }
            send(native, b"\x1b[14~")?;
            send(native, edit)?;
        }
        send(native, ACCEPT_FORM)?;
        rendered(native, "REVIEW: F5 accepts", deadline)?;
        require(
            self.work(&work["work_id"])?["outcome"].is_null(),
            "review-submitted-early",
        )?;
        send(
            self.native.as_mut().ok_or("native-not-attached")?,
            ACCEPT_FORM,
        )?;
        self.selected(
            &form,
            &json!({"action":"accept","content":values}),
            &owner["owner"]["controller_id"],
            &owner["observed"]["generation"],
            deadline,
        )?;
        self.complete(&work, deadline, false)
    }
    fn question_force(&mut self, deadline: Instant) -> Check<()> {
        if self.native.is_none() {
            self.attach(deadline)?;
        }
        self.native_owner(deadline)?;
        let old = self
            .agent()?
            .action("acp-prompt", "live-stale-api-authority");
        let work = self.prompt(QUESTION, true, deadline)?;
        let form = self.interaction(&work, "form", deadline)?;
        let (values, _) = form_values(&form)?;
        let original = self.work(&work["work_id"])?;
        let owner = self.ownership()?;
        require(
            original["controller_id"] == owner["owner"]["controller_id"],
            "native-prompt-provenance",
        )?;
        self.force_api()?;
        rendered(
            self.native.as_mut().ok_or("native-not-attached")?,
            "observer (read-only)",
            deadline,
        )?;
        let current = self.work(&work["work_id"])?;
        same_work(&current, &original)?;
        require(
            current["outcome"].is_null()
                && self.projection()?["foreground"]["prompt_id"] == original["prompt_id"],
            "force-cancelled-original-work",
        )?;
        let projection = self.projection()?;
        require(
            projection["content"]["interactions"]
                .as_array()
                .is_some_and(|items| {
                    items.iter().any(|item| {
                        item["interaction_id"] == form["interaction_id"]
                            && item["state"] == "pending"
                            && item["revision"] == form["revision"]
                    })
                }),
            "force-lost-pending-question",
        )?;
        // A stale API grant must be rejected before an action can reach the wire.
        let mut stale = old;
        stale["op"] = json!("acp-answer");
        stale["interaction_id"] = form["interaction_id"].clone();
        stale["revision"] = form["revision"].clone();
        stale["action"] = json!("accept");
        stale["values"] = values.clone();
        let denied = wire(&self.host, stale)?;
        require(
            denied["ok"] == false
                && matches!(
                    denied["error"]["code"].as_str(),
                    Some("stale-control" | "stale-handle")
                ),
            "stale-authority-not-rejected",
        )?;
        // Stale physical F5 cannot answer from the original attachment either.
        let serial = self.ownership()?["views"]
            .as_array()
            .ok_or("ownership-views-shape")?
            .iter()
            .find(|view| view["controller_id"] == owner["owner"]["controller_id"])
            .and_then(|view| view["serial"].as_u64())
            .ok_or("native-view-receipt-missing")?;
        send(
            self.native.as_mut().ok_or("native-not-attached")?,
            ACCEPT_FORM,
        )?;
        send(
            self.native.as_mut().ok_or("native-not-attached")?,
            b"\x1b[O",
        )?;
        poll(deadline, || {
            Ok(self.ownership()?["views"]
                .as_array()
                .is_some_and(|views| {
                    views.iter().any(|view| {
                        view["controller_id"] == owner["owner"]["controller_id"]
                            && view["focused"] == false
                            && view["serial"]
                                .as_u64()
                                .is_some_and(|current| current > serial)
                    })
                })
                .then_some(()))
        })?;
        require(
            self.work(&work["work_id"])?["outcome"].is_null(),
            "stale-native-completed-work",
        )?;
        let agent = self.agent()?;
        request(
            &self.host,
            form_answer(
                agent,
                &form,
                "live-api-form-answer",
                "accept",
                Some(values.clone()),
            ),
        )?;
        self.selected(
            &form,
            &json!({"action":"accept","content":values}),
            &agent.controller_id,
            &agent.opened["generation"],
            deadline,
        )?;
        self.complete(&work, deadline, false)
    }
    fn permission(&mut self, adapter: &Adapter, allow: bool, deadline: Instant) -> Check<()> {
        self.idle()?;
        self.force_api()?;
        let path = adapter.cwd.join(if allow {
            "amux-live-allow.txt"
        } else {
            "amux-live-deny.txt"
        });
        require(absent(&path), "effect-present-before-permission")?;
        let command = format!(
            "printf 'amux-live-approved\\n' > '{}'",
            path.to_str().ok_or("effect-path-encoding")?
        );
        require(
            !adapter.cwd.to_string_lossy().contains('\''),
            "unsafe-shell-effect-path",
        )?;
        let prompt = format!(
            "Create exactly the file {} with the exact bytes amux-live-approved followed by a newline. Use one built-in file-edit tool, or exactly this shell command: {command} . Request explicit approval for that exact operation and wait for my decision. Do not access other paths, use MCP, perform any other command, retry after denial, or substitute another tool if denied. After the decision finish this turn without further tools.",
            path.display()
        );
        let work = self.prompt(&prompt, false, deadline)?;
        let permission = self.interaction(&work, "permission", deadline)?;
        require(absent(&path), "effect-occurred-before-permission")?;
        confined_permission(&permission, &path, &command)?;
        let kind = if allow { "allow_once" } else { "reject_once" };
        let options = permission["options"]
            .as_array()
            .ok_or("permission-options-shape")?;
        let option = options
            .iter()
            .find(|option| option["kind"] == kind)
            .ok_or("exact-once-option-unavailable")?;
        let option_id = option["option_id"]
            .as_str()
            .ok_or("permission-option-id-shape")?;
        let agent = self.agent()?;
        request(
            &self.host,
            permission_answer(
                agent,
                &permission,
                if allow {
                    "live-allow-once"
                } else {
                    "live-reject-once"
                },
                option_id,
            ),
        )?;
        self.selected(
            &permission,
            &json!({"option_id":option_id}),
            &agent.controller_id,
            &agent.opened["generation"],
            deadline,
        )?;
        self.complete(&work, deadline, !allow)?;
        if allow {
            let metadata = fs::symlink_metadata(&path).map_err(|_| "allowed-file-missing")?;
            require(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "allowed-effect-not-regular-file",
            )?;
            require(
                fs::read(&path).map_err(|_| "allowed-effect-read")? == CONTENT,
                "allowed-effect-content",
            )?;
        } else {
            require(absent(&path), "denied-effect-exists")?;
        }
        Ok(())
    }
    fn detach(&mut self, deadline: Instant) -> Check<()> {
        let Some(native) = self.native.as_mut() else {
            return Ok(());
        };
        send(native, PREFIX)?;
        send(native, &[17])?;
        let code = poll(deadline, || {
            require(
                native
                    .child
                    .try_wait()
                    .map_err(|_| "ctty-status")?
                    .is_none(),
                "ctty-ended-before-termios",
            )?;
            Ok(fs::read_to_string(&native.tui_exit)
                .ok()
                .and_then(|code| code.trim().parse::<i32>().ok()))
        })?;
        require(code == 0, "tui-exit-nonzero")?;
        let mut restored: libc::termios = unsafe { std::mem::zeroed() };
        require(
            unsafe { libc::tcgetattr(native.slave.as_raw_fd(), &mut restored) } == 0,
            "termios-read",
        )?;
        require(
            restored.c_iflag == native.original.c_iflag
                && restored.c_oflag == native.original.c_oflag
                && restored.c_cflag == native.original.c_cflag
                && restored.c_lflag == native.original.c_lflag
                && restored.c_cc == native.original.c_cc
                && restored.c_ispeed == native.original.c_ispeed
                && restored.c_ospeed == native.original.c_ospeed,
            "termios-not-restored",
        )?;
        send(native, b"finish\n")?;
        poll(deadline, || {
            match native.child.try_wait().map_err(|_| "ctty-status")? {
                Some(status) => {
                    require(status.success(), "ctty-exit-nonzero")?;
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
            reader.join().map_err(|_| "pty-drainer-panicked")?;
        }
        require(native.output.lock().error.is_none(), "pty-drainer-failure")?;
        self.native.take();
        Ok(())
    }
    fn cleanup(&mut self) -> Check<()> {
        let deadline = self.deadline;
        // Every cleanup assertion precedes Host/Attachment fallback destructors.
        // A failed TUI oracle must not prevent explicit broker retirement.
        let detached = self.detach(deadline);
        let closed = (|| {
            if self.agent.is_none() {
                return Ok(());
            }
            self.force_api()?;
            let receipt = request(
                &self.host,
                self.agent()?.action("acp-close", "live-explicit-close"),
            )?;
            poll(deadline, || {
                // The model PID is expected to disappear now, so bypass the live-only guard.
                let projection = request(
                    &self.host,
                    json!({"op":"acp-inspect","target":self.agent()?.target,"include_content":false}),
                )?;
                require(
                    projection["target"] == self.agent()?.target,
                    "close-target-changed",
                )?;
                let joined = projection["resource"]["cleanup_complete"] == true;
                let gone = self.model.as_ref().is_none_or(|model| pid_gone(model.pid));
                let admitted = projection["work"].as_array().is_some_and(|work| {
                    work.iter().any(|work| {
                        work["work_id"] == receipt["work_id"]
                            && work["op"] == "acp-close"
                            && work["state"] == "accepted"
                    })
                });
                Ok(
                    (joined && gone && admitted && projection["lifecycle"] == "ended")
                        .then_some(()),
                )
            })
        })();
        let shutdown = (|| {
            request(&self.host, json!({"op":"shutdown"}))?;
            poll(deadline, || {
                match self.host.daemon.try_wait().map_err(|_| "daemon-status")? {
                    Some(status) => {
                        require(status.success(), "daemon-exit-nonzero")?;
                        Ok(Some(()))
                    }
                    None => Ok(None),
                }
            })?;
            require(!self.host.socket.exists(), "daemon-socket-survived")?;
            require(
                self.sentinel
                    .0
                    .try_wait()
                    .map_err(|_| "sentinel-status")?
                    .is_none(),
                "unrelated-sentinel-ended",
            )?;
            // Do not unlink the PTY's private root until its own lifetime is proved.
            detached?;
            closed?;
            fs::remove_dir_all(&self.host.root).map_err(|_| "private-root-removal")?;
            require(!self.host.root.exists(), "private-root-survived")?;
            require(
                self.sentinel
                    .0
                    .try_wait()
                    .map_err(|_| "sentinel-status")?
                    .is_none(),
                "unrelated-sentinel-ended",
            )
        })();
        detached.and(closed).and(shutdown)
    }
}
impl Drop for Live {
    fn drop(&mut self) {
        // The generic helpers already own fallbacks; none can turn an oracle green.
        self.native.take();
    }
}

fn same_work(current: &Value, original: &Value) -> Check<()> {
    for field in [
        "work_id",
        "call_id",
        "controller_id",
        "owner_generation",
        "op",
        "rpc_id",
        "prompt_id",
        "interaction_id",
    ] {
        require(
            current[field] == original[field],
            "original-work-identity-changed",
        )?;
    }
    Ok(())
}

// Answers come from the actual supported flat schema. No invented field names,
// enums, question records, or unsupported-schema coercion enter the broker.
fn form_values(form: &Value) -> Check<(Value, Vec<Vec<u8>>)> {
    let schema = &form["schema"];
    require(schema["type"] == "object", "unsupported-live-form-schema")?;
    let properties = schema["properties"]
        .as_object()
        .ok_or("unsupported-live-form-schema")?;
    require(
        !properties.is_empty() && properties.len() <= 16,
        "unsupported-live-form-field-count",
    )?;
    let mut values = serde_json::Map::new();
    let mut edits = Vec::new();
    for (name, property) in properties {
        match property["type"].as_str() {
            Some("boolean") => {
                values.insert(name.clone(), json!(true));
                edits.push(b" ".to_vec());
            }
            Some("string") => {
                if property.get("enum").is_some_and(|value| !value.is_null())
                    || property.get("oneOf").is_some_and(|value| !value.is_null())
                {
                    let choices = if let Some(options) = property["enum"].as_array() {
                        options
                            .iter()
                            .map(|value| value.as_str().ok_or("unsupported-live-form-choice"))
                            .collect::<Check<Vec<_>>>()?
                    } else {
                        property["oneOf"]
                            .as_array()
                            .ok_or("unsupported-live-form-choice")?
                            .iter()
                            .map(|choice| {
                                choice["const"]
                                    .as_str()
                                    .ok_or("unsupported-live-form-choice")
                            })
                            .collect::<Check<Vec<_>>>()?
                    };
                    let valid = |choice: &&&str| {
                        let length = choice.chars().count() as u64;
                        property["minLength"]
                            .as_u64()
                            .is_none_or(|min| length >= min)
                            && property["maxLength"]
                                .as_u64()
                                .is_none_or(|max| length <= max)
                    };
                    let first = *choices
                        .iter()
                        .find(valid)
                        .ok_or("unsupported-live-form-choice")?;
                    // Defaults affect the UI cursor even after F4. Anchor to the
                    // first allowed offered choice, then deliberately select it.
                    let mut keys = Vec::new();
                    for _ in 0..choices.len() {
                        keys.extend_from_slice(b"\x1b[D");
                    }
                    keys.extend_from_slice(b"\x1b[D");
                    values.insert(name.clone(), json!(first));
                    edits.push(keys);
                } else {
                    require(
                        property.get("pattern").is_none() && property.get("format").is_none(),
                        "unsupported-live-string-constraint",
                    )?;
                    let minimum = property["minLength"].as_u64().unwrap_or(1).max(1);
                    require(
                        minimum <= 64
                            && property["maxLength"]
                                .as_u64()
                                .is_none_or(|max| minimum <= max),
                        "unsupported-live-string-length",
                    )?;
                    let answer = "a".repeat(minimum as usize);
                    values.insert(name.clone(), json!(answer));
                    edits.push(answer.into_bytes());
                }
            }
            _ => return Err("unsupported-live-form-field-type"),
        }
    }
    Ok((Value::Object(values), edits))
}
fn confined_permission(permission: &Value, path: &Path, command: &str) -> Check<()> {
    let tool = &permission["tool_call"];
    let mut matched = false;
    if let Some(locations) = tool["locations"].as_array() {
        for location in locations {
            let offered = location["path"].as_str().ok_or("permission-path-shape")?;
            require(
                Path::new(offered) == path,
                "permission-outside-approved-effect",
            )?;
            matched = true;
        }
    }
    if let Some(input) = tool.get("rawInput").filter(|input| !input.is_null()) {
        if let Some(offered) = input.get("command") {
            require(
                offered.as_str().is_some_and(|offered| {
                    offered == command
                        || offered == command.replace('\'', "'\\''")
                        || offered == command.replace('\'', "'\"'\"'")
                        || offered
                            == format!(
                                "\"{}\"",
                                command
                                    .replace('\\', "\\\\")
                                    .replace('"', "\\\"")
                                    .replace('$', "\\$")
                                    .replace('`', "\\`")
                            )
                }),
                "permission-command-not-exact-safe-effect",
            )?;
            matched = true;
        }
        for key in ["path", "file_path", "filePath"] {
            if let Some(offered) = input.get(key) {
                require(
                    offered
                        .as_str()
                        .is_some_and(|offered| Path::new(offered) == path),
                    "permission-outside-approved-effect",
                )?;
                matched = true;
            }
        }
    }
    require(matched, "permission-exact-effect-unproven")
}

fn outcome(adapter: &str, row: &str, result: Check<()>) -> bool {
    match result {
        Ok(()) => {
            eprintln!("live adapter={adapter} row={row} result=PASS");
            true
        }
        Err(code) => {
            eprintln!("live adapter={adapter} row={row} result=FAIL code={code}");
            false
        }
    }
}
pub(crate) fn run() {
    let config = configuration()
        .unwrap_or_else(|code| panic!("live acceptance configuration rejected: {code}"));
    let mut passed = true;
    for adapter in &config.adapters {
        let mut live = match Live::new(config.max_seconds_per_adapter) {
            Ok(live) => live,
            Err(code) => {
                passed &= outcome(&adapter.id, "bootstrap", Err(code));
                for row in ROWS_LIVE {
                    outcome(&adapter.id, row, Err("bootstrap-unavailable"));
                }
                continue;
            }
        };
        let bootstrap_deadline =
            (Instant::now() + Duration::from_secs(45)).min(live.deadline - Duration::from_secs(15));
        let bootstrap = live.open(adapter, bootstrap_deadline);
        let mut ready = bootstrap.is_ok();
        passed &= outcome(&adapter.id, "bootstrap-model", bootstrap);
        for (index, row) in ROWS_LIVE.iter().enumerate() {
            let deadline = (Instant::now() + Duration::from_secs(45))
                .min(live.deadline - Duration::from_secs(15));
            let result = if ready {
                (|| {
                    require(Instant::now() < deadline, "adapter-time-bound")?;
                    live.stable_model(adapter)?;
                    if adapter.id == "codex" && matches!(index, 0 | 2) {
                        live.codex_plan_toggle(deadline)?;
                    }
                    let result = match index {
                        0 => live.question_native(deadline),
                        1 => live.question_force(deadline),
                        2 => live.permission(adapter, true, deadline),
                        _ => live.permission(adapter, false, deadline),
                    };
                    result?;
                    live.stable_model(adapter)
                })()
            } else {
                Err("bootstrap-unavailable")
            };
            ready &= result.is_ok();
            passed &= outcome(&adapter.id, row, result);
        }
        passed &= outcome(
            &adapter.id,
            "explicit-cleanup-before-fallback",
            live.cleanup(),
        );
        // Live/Host/Attachment/Sentinel destructors are fallback-only after the
        // observable cleanup result, including failed and unsupported rows.
        drop(live);
    }
    assert!(
        passed,
        "real live acceptance failed; see redacted row outcomes"
    );
}

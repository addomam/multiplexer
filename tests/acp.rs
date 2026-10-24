use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(12);

fn exchange(socket: &Path, mut request: Value) -> std::io::Result<Value> {
    request["version"] = json!(1);
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(DEADLINE))?;
    stream.set_write_timeout(Some(DEADLINE))?;
    serde_json::to_writer(&mut stream, &request)?;
    stream.write_all(b"\n")?;
    let mut line = String::new();
    BufReader::new(stream)
        .take(8 * 1024 * 1024)
        .read_line(&mut line)?;
    if !line.ends_with('\n') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "incomplete daemon response",
        ));
    }
    Ok(serde_json::from_str(&line)?)
}

struct Host {
    root: PathBuf,
    socket: PathBuf,
    daemon: Child,
}

impl Host {
    fn new() -> Self {
        let root = fs::canonicalize("/tmp")
            .unwrap()
            .join(format!("amux-acp-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["home", "startup", "config", "runtime", "projects"] {
            fs::create_dir(root.join(name)).unwrap();
            fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::write(root.join("startup/.zshrc"), "PS1='ACP_FIXTURE> '\n").unwrap();
        let socket = root.join("daemon.sock");
        let log = fs::File::create(root.join("daemon.log")).unwrap();
        let daemon = Command::new(env!("CARGO_BIN_EXE_amux"))
            .arg("--socket")
            .arg(&socket)
            .arg("daemon")
            .arg("--projects-dir")
            .arg(root.join("projects"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("HOME", root.join("home"))
            .env("SHELL", "/bin/zsh")
            .env("USER", "amux-fixture")
            .env("TERM", "xterm-256color")
            .env("ZDOTDIR", root.join("startup"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_RUNTIME_DIR", root.join("runtime"))
            .env("AMUX_RUNTIME_DIR", root.join("runtime"))
            .env("AMUX_SOCKET", &socket)
            .current_dir(&root)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();
        let host = Self {
            root,
            socket,
            daemon,
        };
        host.until("private daemon startup", || {
            exchange(&host.socket, json!({"op":"snapshot"}))
                .ok()
                .filter(|response| response["ok"] == true)
        });
        host
    }

    fn until<T>(&self, label: &str, mut probe: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + DEADLINE;
        loop {
            if let Some(value) = probe() {
                return value;
            }
            assert!(
                Instant::now() < deadline,
                "{label}; {}",
                fs::read_to_string(self.root.join("daemon.log")).unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn call(&self, request: Value) -> Value {
        let response = exchange(&self.socket, request).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        response["result"].clone()
    }

    fn view(&self, id: &str) -> Option<Value> {
        self.call(json!({"op":"snapshot"}))["agent_views"]
            .as_array()?
            .iter()
            .find(|view| view["id"] == id)
            .cloned()
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = exchange(&self.socket, json!({"op":"shutdown"}));
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.daemon.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if self.daemon.try_wait().ok().flatten().is_none() {
            let _ = self.daemon.kill();
        }
        let _ = self.daemon.wait();
        // Only after every test assertion and daemon shutdown/reap may fixture
        // watchdogs provide fallback cleanup. Never let them pass the reap oracle.
        let mut peer_roots = vec![self.root.clone()];
        if let Ok(entries) = fs::read_dir(&self.root) {
            peer_roots.extend(
                entries
                    .flatten()
                    .filter(|entry| entry.file_name().to_string_lossy().starts_with("peer-"))
                    .map(|entry| entry.path()),
            );
        }
        for root in &peer_roots {
            let _ = fs::write(root.join("fixture-fallback"), b"cleanup");
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline
            && peer_roots.iter().any(|root| {
                fs::read_to_string(root.join("peer.pid"))
                    .ok()
                    .and_then(|pid| pid.parse().ok())
                    .is_some_and(|pid| !pid_gone(pid))
            })
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn e04s01_persistent_open_preserves_socket_and_session_effect() {
    let host = Host::new();
    let project = host.call(json!({"op":"snapshot"}))["projects"][0]["id"].clone();
    let controller = host.call(json!({"op":"controller-create","role":"external","label":"Persistent ACP"}))["controller"].clone();
    let mut connection = BufReader::new(UnixStream::connect(&host.socket).unwrap());
    connection
        .get_ref()
        .set_read_timeout(Some(DEADLINE))
        .unwrap();
    connection
        .get_ref()
        .set_write_timeout(Some(DEADLINE))
        .unwrap();
    let mut call = |mut request: Value| {
        request["version"] = json!(1);
        request["persistent"] = json!(true);
        serde_json::to_writer(connection.get_mut(), &request).unwrap();
        connection.get_mut().write_all(b"\n").unwrap();
        let mut line = String::new();
        connection.read_line(&mut line).unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        response["result"].clone()
    };
    let peer = Path::new(env!("CARGO_BIN_EXE_amux"))
        .parent()
        .unwrap()
        .join("examples/acp_fixture");
    let opened = call(
        json!({"op":"acp-open","project":project,"controller":controller,
        "command":[peer,host.root],"name":"Persistent arithmetic"}),
    );
    let id = opened["target"]["id"].as_str().unwrap();
    let view = host.until("persistent ACP ready", || {
        host.view(id).filter(|view| view["lifecycle"] == "ready")
    });
    let target = json!({"kind":"acp","id":id,"instance_id":view["instance_id"],
        "connection_generation":view["connection_generation"],"session_id":view["session_id"]});
    call(
        json!({"op":"acp-prompt","target":target,"control":opened["control"],
        "owner_generation":opened["generation"],"call_id":"persistent-effect","text":"add 17 25"}),
    );
    host.until("persistent same-session application effect", || {
        let applied: Value =
            serde_json::from_slice(&fs::read(host.root.join("effect.json")).ok()?).ok()?;
        (applied["value"] == 42
            && applied["commands"] == 1
            && applied["session_id"] == target["session_id"])
            .then_some(())
    });
    let recovered = call(json!({"op":"acp-inspect","target":target}));
    assert_eq!(recovered["target"], target);
    call(
        json!({"op":"acp-close","target":target,"control":opened["control"],
        "owner_generation":opened["generation"],"call_id":"persistent-close"}),
    );
    host.until("persistent peer reaped before fallback", || {
        let pid: i32 = fs::read_to_string(host.root.join("peer.pid"))
            .ok()?
            .parse()
            .ok()?;
        (unsafe { libc::kill(pid, 0) } == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH))
        .then_some(())
    });
}

#[test]
fn e04s01_prompt_effect_and_stream_share_one_live_instance() {
    let host = Host::new();
    let project = host.call(json!({"op":"snapshot"}))["projects"][0]["id"].clone();
    let controller = host.call(json!({"op":"controller-create","role":"external","label":"ACP tracer"}))["controller"].clone();
    let peer = Path::new(env!("CARGO_BIN_EXE_amux"))
        .parent()
        .unwrap()
        .join("examples/acp_fixture");
    let opened = host.call(
        json!({"op":"acp-open","project":project,"controller":controller,
        "command":[peer,host.root],"name":"Local arithmetic peer"}),
    );
    let id = opened["target"]["id"].as_str().unwrap();
    let view = host.until("same ACP session ready", || {
        host.view(id).filter(|view| view["lifecycle"] == "ready")
    });
    let target = json!({"kind":"acp","id":id,"instance_id":view["instance_id"],
        "connection_generation":view["connection_generation"],"session_id":view["session_id"]});
    let snapshot = host.call(json!({"op":"snapshot"}));
    assert!(
        snapshot["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["id"] != id),
        "ACP must not create a fake PTY"
    );
    host.call(
        json!({"op":"acp-prompt","target":target,"control":opened["control"],
        "owner_generation":opened["generation"],"call_id":"arithmetic-1","text":"add 17 25"}),
    );
    host.until("agent applied arithmetic once", || {
        let effect: Value =
            serde_json::from_slice(&fs::read(host.root.join("effect.json")).ok()?).ok()?;
        (effect["value"] == 42
            && effect["commands"] == 1
            && effect["session_id"] == target["session_id"])
            .then_some(())
    });
    host.until("canonical stream from that live instance", || {
        let inspected =
            host.call(json!({"op":"acp-inspect","target":target,"include_content":true}));
        inspected["content"]["events"]
            .as_array()?
            .iter()
            .any(|event| {
                event["kind"] == "agent-message" && event["text"] == "Total=42; commands=1"
            })
            .then_some(())
    });
    host.call(
        json!({"op":"acp-close","target":target,"control":opened["control"],
        "owner_generation":opened["generation"],"call_id":"close-arithmetic"}),
    );
    host.until("owned peer reaped before fixture fallback", || {
        let pid: i32 = fs::read_to_string(host.root.join("peer.pid"))
            .ok()?
            .parse()
            .ok()?;
        (unsafe { libc::kill(pid, 0) } == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH))
        .then_some(())
    });
}

struct Agent {
    opened: Value,
    target: Value,
    root: PathBuf,
    controller_id: Value,
}

impl Agent {
    fn action(&self, op: &str, call_id: &str) -> Value {
        json!({"op":op,"target":self.target,"control":self.opened["control"],
            "owner_generation":self.opened["generation"],"call_id":call_id})
    }

    fn release(&self, barrier: &str) {
        fs::write(self.root.join(format!("{barrier}.release")), b"continue").unwrap();
    }

    fn effect(&self) -> Value {
        serde_json::from_slice(&fs::read(self.root.join("effect.json")).unwrap()).unwrap()
    }
}

impl Host {
    fn open(&self, mode: &str) -> Agent {
        let root = self
            .root
            .join(format!("peer-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&root).unwrap();
        let project = self.call(json!({"op":"snapshot"}))["projects"][0]["id"].clone();
        let controller = self
            .call(json!({"op":"controller-create","role":"external","label":"Local ACP boundary"}));
        let peer = Path::new(env!("CARGO_BIN_EXE_amux"))
            .parent()
            .unwrap()
            .join("examples/acp_fixture");
        let opened = self.call(
            json!({"op":"acp-open","project":project,"controller":controller["controller"],
            "command":[peer,root,mode],"name":"Boundary peer"}),
        );
        assert_eq!(opened["lifecycle"], "starting", "{opened}");
        assert!(opened["target"]["session_id"].is_null(), "{opened}");
        Agent {
            target: opened["target"].clone(),
            opened,
            root,
            controller_id: controller["controller_id"].clone(),
        }
    }

    fn ready(&self, agent: &mut Agent) {
        let id = agent.target["id"].as_str().unwrap();
        let view = self.until("ACP ready", || {
            self.view(id).filter(|view| view["lifecycle"] == "ready")
        });
        agent.target = json!({"kind":"acp","id":id,"instance_id":view["instance_id"],
            "connection_generation":view["connection_generation"],"session_id":view["session_id"]});
    }

    fn inspect(&self, agent: &Agent) -> Value {
        self.call(json!({"op":"acp-inspect","target":agent.target,"include_content":true}))
    }

    fn barrier(&self, agent: &Agent, name: &str) {
        self.until(name, || {
            agent
                .root
                .join(format!("{name}.received"))
                .exists()
                .then_some(())
        });
    }

    fn prompt(&self, agent: &Agent, call_id: &str, text: &str) -> Value {
        let mut action = agent.action("acp-prompt", call_id);
        action["text"] = json!(text);
        self.call(action)
    }

    fn work(&self, agent: &Agent, id: &Value) -> Value {
        self.inspect(agent)["work"]
            .as_array()
            .unwrap()
            .iter()
            .find(|work| work["work_id"] == *id)
            .expect("retained work")
            .clone()
    }

    fn completed(&self, agent: &Agent, id: &Value) -> Value {
        self.until("exact admitted prompt completed", || {
            let work = self.work(agent, id);
            (work["state"] == "completed").then_some(work)
        })
    }

    fn deny(&self, request: Value, codes: &[&str]) {
        let response = exchange(&self.socket, request).unwrap();
        assert_eq!(response["ok"], false, "{response}");
        assert!(
            codes.contains(&response["error"]["code"].as_str().unwrap()),
            "{response}"
        );
    }

    fn reaped(&self, agent: &Agent, file: &str) {
        self.until("owned process absent before fallback", || {
            let pid: i32 = fs::read_to_string(agent.root.join(file))
                .ok()?
                .parse()
                .ok()?;
            pid_gone(pid).then_some(())
        });
    }

    fn close(&self, agent: &Agent) {
        self.call(agent.action("acp-close", "explicit-close"));
        self.reaped(agent, "peer.pid");
        self.until("pipe workers joined before fallback", || {
            (self.inspect(agent)["resource"]["cleanup_complete"] == true).then_some(())
        });
    }
}

fn pid_gone(pid: i32) -> bool {
    (unsafe { libc::kill(pid, 0) }) == -1
        && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

struct Sentinel(Child);

impl Sentinel {
    fn new() -> Self {
        Self(
            Command::new("/bin/sleep")
                .arg("120")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }

    fn alive(&mut self) {
        assert!(
            self.0.try_wait().unwrap().is_none(),
            "unrelated process signalled"
        );
    }
}

impl Drop for Sentinel {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn e04s01_async_bootstrap_close_is_responsive_while_peer_reply_is_held() {
    for mode in ["hold-initialize", "hold-session"] {
        let host = Host::new();
        let mut sentinel = Sentinel::new();
        let agent = host.open(mode);
        let barrier = if mode == "hold-initialize" {
            "initialize"
        } else {
            "session"
        };
        host.barrier(&agent, barrier);
        assert_eq!(host.inspect(&agent)["lifecycle"], "starting");
        let mut action = agent.action("acp-prompt", "premature");
        action["text"] = json!("add 900 1");
        host.deny(action, &["not-ready"]);
        host.close(&agent);
        let ended = host.inspect(&agent);
        assert_eq!(ended["lifecycle"], "ended", "{ended}");
        assert!(ended["target"]["session_id"].is_null());
        assert!(!agent.root.join("effect.json").exists());
        sentinel.alive();
    }
}

#[test]
fn e04s01_duplicate_busy_stale_tuple_and_generation_have_no_second_effect() {
    let host = Host::new();
    let mut agent = host.open("hold-prompt");
    let starting_target = agent.target.clone();
    host.ready(&mut agent);
    let admitted = host.prompt(&agent, "original", "add 17 25");
    host.barrier(&agent, "prompt");
    let mut duplicate = agent.action("acp-prompt", "original");
    duplicate["text"] = json!("add 900 1");
    host.deny(duplicate.clone(), &["duplicate-call"]);
    duplicate["call_id"] = json!("second-foreground");
    host.deny(duplicate.clone(), &["busy"]);
    duplicate["target"] = starting_target;
    host.deny(duplicate.clone(), &["stale-target"]);
    duplicate["target"] = agent.target.clone();
    duplicate["target"]["instance_id"] = json!("wrong-instance");
    host.deny(duplicate.clone(), &["stale-target"]);
    duplicate["target"] = agent.target.clone();
    duplicate["owner_generation"] = json!(agent.opened["generation"].as_u64().unwrap() + 1);
    host.deny(duplicate.clone(), &["stale-control", "stale-handle"]);
    duplicate["owner_generation"] = agent.opened["generation"].clone();
    duplicate["unrecognized"] = json!(true);
    host.deny(duplicate, &["invalid-request"]);
    agent.release("prompt");
    let work = host.completed(&agent, &admitted["work_id"]);
    assert_eq!(work["outcome"]["stop_reason"], "end_turn");
    assert_eq!(work["rpc_id"], agent.effect()["rpc_id"]);
    assert_eq!(agent.effect()["commands"], 1);
    host.close(&agent);
}

#[test]
fn e04s01_force_and_pending_claim_cancel_retain_original_work_and_rpc() {
    let host = Host::new();
    let mut agent = host.open("hold-prompt");
    host.ready(&mut agent);
    let admitted = host.prompt(&agent, "before-transfer", "add 17 25");
    host.barrier(&agent, "prompt");
    let original = host.work(&agent, &admitted["work_id"]);
    let pid = fs::read_to_string(agent.root.join("peer.pid")).unwrap();
    let claimant =
        host.call(json!({"op":"controller-create","role":"external","label":"ACP transferee"}));
    let ownership = host.view(agent.target["id"].as_str().unwrap()).unwrap()["ownership"].clone();
    let forced = host.call(
        json!({"op":"ownership-claim","target":{"kind":"acp","id":agent.target["id"]},
        "controller":claimant["controller"],"mode":"force","observed":ownership["observed"]}),
    );
    let mut stale = agent.action("acp-close", "stale-close");
    host.deny(stale.clone(), &["stale-control", "stale-handle"]);
    stale["op"] = json!("acp-prompt");
    stale["text"] = json!("add 900 1");
    host.deny(stale, &["stale-control", "stale-handle"]);
    let third =
        host.call(json!({"op":"controller-create","role":"external","label":"Cancelled claimant"}));
    let ownership = host.view(agent.target["id"].as_str().unwrap()).unwrap()["ownership"].clone();
    let pending = host.call(json!({"op":"ownership-claim","target":{"kind":"acp","id":agent.target["id"]},
        "controller":third["controller"],"mode":"cooperative","when":{"kind":"now"},"observed":ownership["observed"]}));
    host.call(json!({"op":"control-handle","handle":pending["claim"],"method":"cancel"}));
    agent.release("prompt");
    let retained = host.completed(&agent, &admitted["work_id"]);
    for key in [
        "work_id",
        "call_id",
        "controller_id",
        "owner_generation",
        "rpc_id",
        "prompt_id",
    ] {
        assert_eq!(retained[key], original[key], "{key}");
    }
    assert_eq!(retained["controller_id"], agent.controller_id);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(
        fs::read_to_string(agent.root.join("peer.pid")).unwrap(),
        pid
    );
    assert!(!agent.root.join("unexpected-cancel").exists());
    // Ordinary claim cancellation grants nobody. Recover deliberately by a new
    // force claim, never by rebinding the original action to a current control.
    let ownership = host.view(agent.target["id"].as_str().unwrap()).unwrap()["ownership"].clone();
    assert!(ownership["owner"].is_null(), "{ownership}");
    let recovered = host.call(
        json!({"op":"ownership-claim","target":{"kind":"acp","id":agent.target["id"]},
        "controller":claimant["controller"],"mode":"force","observed":ownership["observed"]}),
    );
    assert_ne!(recovered["generation"], forced["generation"]);
    agent.opened = recovered;
    host.close(&agent);
}

#[test]
fn e04s01_pending_rpc_survives_caller_socket_loss_and_stream_categories_stay_distinct() {
    let host = Host::new();
    let mut agent = host.open("hold-prompt");
    host.ready(&mut agent);
    let mut request = agent.action("acp-prompt", "lost-socket");
    request["version"] = json!(1);
    request["text"] = json!("add 17 25");
    let mut stream = UnixStream::connect(&host.socket).unwrap();
    serde_json::to_writer(&mut stream, &request).unwrap();
    stream.write_all(b"\n").unwrap();
    host.barrier(&agent, "prompt");
    drop(stream);
    let pending = host.until("all distinct stream categories consumed", || {
        let projection = host.inspect(&agent);
        let events = projection["content"]["events"].as_array().unwrap();
        ["agent-message", "thought", "tool", "plan"]
            .iter()
            .all(|category| events.iter().any(|event| event["kind"] == *category))
            .then_some(projection)
    });
    let work = pending["work"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["call_id"] == "lost-socket")
        .unwrap()
        .clone();
    let visible = pending["content"]["events"].as_array().unwrap();
    assert!(visible.iter().any(|event| event["kind"] == "agent-message"
        && event["message_id"]
            == format!(
                "{}-agent_message_chunk",
                agent.target["session_id"].as_str().unwrap()
            )));
    assert!(
        visible
            .iter()
            .any(|event| event["kind"] == "tool" && event["tool_call_id"] == "arithmetic-tool")
    );
    let public = host
        .call(json!({"op":"events","after":0,"timeout_ms":0}))
        .to_string();
    for private in [
        "synthetic-private-thought",
        "synthetic-private-tool",
        "synthetic-private-plan",
    ] {
        assert!(
            !public.contains(private),
            "stream content leaked into public events"
        );
    }
    for event in visible
        .iter()
        .filter(|event| event["kind"] != "user-message")
    {
        assert!(
            event.get("prompt_id").is_none(),
            "updates cannot guess prompt association"
        );
    }
    agent.release("prompt");
    let completed = host.completed(&agent, &work["work_id"]);
    assert_eq!(completed["rpc_id"], agent.effect()["rpc_id"]);
    assert_eq!(completed["owner_generation"], agent.opened["generation"]);
    assert!(!agent.root.join("unexpected-cancel").exists());
    host.close(&agent);
}

#[test]
fn e04s01_authentication_requires_explicit_advertised_agent_method() {
    let host = Host::new();
    let mut agent = host.open("auth");
    host.until("explicit auth-required lifecycle", || {
        (host.inspect(&agent)["lifecycle"] == "auth-required").then_some(())
    });
    assert!(!agent.root.join("authenticate.received").exists());
    for (n, method) in ["client-login", "terminal-login", "unadvertised"]
        .iter()
        .enumerate()
    {
        let mut request = agent.action("acp-authenticate", &format!("unsupported-auth-{n}"));
        request["method_id"] = json!(method);
        host.deny(request, &["unsupported"]);
    }
    assert!(!agent.root.join("authenticate.received").exists());
    let mut action = agent.action("acp-authenticate", "explicit-agent-auth");
    action["method_id"] = json!("agent-login");
    let admitted = host.call(action.clone());
    host.barrier(&agent, "authenticate");
    action["call_id"] = json!("parallel-auth");
    host.deny(action, &["busy"]);
    let pending = host.work(&agent, &admitted["work_id"]);
    assert!(pending["outcome"].is_null());
    agent.release("authenticate");
    host.ready(&mut agent);
    let auth = host.work(&agent, &admitted["work_id"]);
    assert_eq!(auth["outcome"]["kind"], "response");
    let prompt = host.prompt(&agent, "after-auth", "add 17 25");
    host.completed(&agent, &prompt["work_id"]);
    assert_eq!(agent.effect()["value"], 42);
    host.close(&agent);
}

#[test]
fn e04s01_unsupported_requests_never_hang_bootstrap_or_prompt_responders() {
    for mode in ["unsupported-bootstrap", "unsupported-prompt"] {
        let host = Host::new();
        let mut agent = host.open(mode);
        host.ready(&mut agent);
        let prompt = host.prompt(&agent, "after-unsupported", "add 17 25");
        host.completed(&agent, &prompt["work_id"]);
        assert!(agent.root.join("unsupported-complete").exists());
        assert_eq!(agent.effect()["value"], 42);
        host.close(&agent);
    }
}

#[test]
fn e04s01_protocol_error_releases_foreground_and_public_metadata_excludes_content() {
    let host = Host::new();
    let mut agent = host.open("error-once");
    host.ready(&mut agent);
    let failed = host.prompt(&agent, "private-error-call", "synthetic-private-prompt");
    host.until("actual RPC error retained", || {
        let work = host.work(&agent, &failed["work_id"]);
        (work["state"] == "failed" && work["outcome"]["code"] == -32603).then_some(())
    });
    let snapshot = host.call(json!({"op":"snapshot"})).to_string();
    let events = host
        .call(json!({"op":"events","after":0,"timeout_ms":0}))
        .to_string();
    for secret in [
        "synthetic-private-prompt",
        "synthetic-private-provider-error",
        "synthetic-private-error-data",
        "synthetic-private-agent-label",
    ] {
        assert!(
            !snapshot.contains(secret),
            "public metadata leaked {secret}"
        );
        assert!(!events.contains(secret), "public events leaked {secret}");
    }
    let content = host.inspect(&agent);
    assert!(content["foreground"].is_null());
    assert!(
        content["content"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "user-message" && e["text"] == "synthetic-private-prompt")
    );
    assert!(
        !content
            .to_string()
            .contains("synthetic-private-provider-error")
    );
    let good = host.prompt(&agent, "fresh-after-error", "add 17 25");
    host.completed(&agent, &good["work_id"]);
    assert_eq!(agent.effect()["commands"], 1);
    host.close(&agent);
}

#[test]
fn e04s01_hostile_transport_fails_exact_instance_and_reaps_without_blocking_daemon() {
    for mode in ["oversized", "invalid-utf8", "malformed", "partial", "flood"] {
        let host = Host::new();
        let mut sentinel = Sentinel::new();
        let agent = host.open(mode);
        host.barrier(&agent, "hostile");
        agent.release("hostile");
        host.until(mode, || {
            (host.inspect(&agent)["lifecycle"] == "failed").then_some(())
        });
        host.reaped(&agent, "peer.pid");
        assert!(!agent.root.join("effect.json").exists());
        host.call(
            json!({"op":"controller-create","role":"external","label":"Responsive after failure"}),
        );
        sentinel.alive();
    }
}

#[test]
fn e04s01_eof_during_prompt_is_unknown_not_confirmed_completion_or_cancel() {
    let host = Host::new();
    let mut agent = host.open("eof-prompt");
    host.ready(&mut agent);
    let admitted = host.prompt(&agent, "eof-outstanding", "add 17 25");
    let unknown = host.until("EOF makes exact work unknown", || {
        let work = host.work(&agent, &admitted["work_id"]);
        (work["state"] == "unknown").then_some(work)
    });
    assert!(unknown["outcome"].is_null());
    assert!(host.inspect(&agent)["foreground"].is_null());
    host.reaped(&agent, "peer.pid");
}

#[test]
fn e04s01_stderr_cap_continues_draining_and_writer_stall_is_bounded() {
    for mode in ["stderr", "stalled-writer"] {
        let host = Host::new();
        let mut agent = host.open(mode);
        host.ready(&mut agent);
        let admitted = host.prompt(&agent, "bounded-output", "add 17 25");
        if mode == "stderr" {
            host.completed(&agent, &admitted["work_id"]);
            assert!(agent.root.join("stderr-drained").exists());
            assert_eq!(host.inspect(&agent)["resource"]["stderr_truncated"], true);
            host.close(&agent);
        } else {
            host.until("blocked writer fails bounded instance", || {
                (host.inspect(&agent)["lifecycle"] == "failed").then_some(())
            });
            assert_eq!(host.work(&agent, &admitted["work_id"])["state"], "unknown");
            host.reaped(&agent, "peer.pid");
        }
        host.call(json!({"op":"controller-create","role":"external","label":"Responsive after output pressure"}));
    }
}

#[test]
fn e04s01_incremental_history_eviction_reports_gap_without_losing_pending_rpc() {
    let host = Host::new();
    let mut agent = host.open("history");
    host.ready(&mut agent);
    let admitted = host.prompt(&agent, "long-pending-stream", "add 17 25");
    let original = host.work(&agent, &admitted["work_id"]);
    let mut cursor = 0;
    for wave in 0..140 {
        let barrier = format!("wave-{wave}");
        host.barrier(&agent, &barrier);
        host.until("wave consumed before producing more", || {
            let result = host.call(json!({"op":"acp-inspect","target":agent.target,"include_content":true,"after":cursor,"limit":128}));
            cursor = result["content"]["through"].as_u64().unwrap();
            result["content"]["events"].as_array().unwrap().iter()
                .any(|e| e["text"].as_str().is_some_and(|t| t.starts_with(&format!("history-{wave}-15:"))))
                .then_some(())
        });
        agent.release(&barrier);
    }
    let completed = host.completed(&agent, &admitted["work_id"]);
    assert_eq!(completed["rpc_id"], original["rpc_id"]);
    let resync = host.inspect(&agent);
    assert_eq!(resync["content"]["gap"], true);
    assert!(resync["truncation"]["through"].as_u64().unwrap() > 0);
    host.close(&agent);
}

#[test]
fn e04s01_explicit_close_and_daemon_shutdown_reap_owned_descendants_only() {
    for shutdown in [false, true] {
        let host = Host::new();
        let mut sentinel = Sentinel::new();
        let mut agent = host.open("descendants");
        host.ready(&mut agent);
        host.until("owned descendant published", || {
            agent.root.join("descendant.pid").exists().then_some(())
        });
        if shutdown {
            host.call(json!({"op":"shutdown"}));
        } else {
            host.close(&agent);
        }
        host.reaped(&agent, "peer.pid");
        host.reaped(&agent, "descendant.pid");
        sentinel.alive();
    }
}

#[test]
fn e04s01_native_terminal_retirement_does_not_end_sibling_acp_target() {
    let host = Host::new();
    let mut agent = host.open("arithmetic");
    host.ready(&mut agent);
    let terminal = host.call(json!({"op":"snapshot"}))["terminals"][0]["id"].clone();
    assert!(terminal.is_string());
    host.call(json!({"op":"close","terminal":terminal}));
    host.call(json!({"op":"wait","terminal":terminal,"condition":"exited","timeout_ms":10000}));
    host.call(json!({"op":"remove","terminal":terminal}));
    let admitted = host.prompt(&agent, "after-native-retirement", "add 17 25");
    host.completed(&agent, &admitted["work_id"]);
    assert_eq!(agent.effect()["value"], 42);
    host.close(&agent);
}

#[test]
fn e04s01_interrupt_uses_original_prompt_response_not_notification_as_confirmation() {
    let host = Host::new();
    let mut agent = host.open("interrupt");
    host.ready(&mut agent);
    let admitted = host.prompt(&agent, "interruptible-turn", "add 17 25");
    host.barrier(&agent, "prompt");
    let prompt_id = host.work(&agent, &admitted["work_id"])["prompt_id"].clone();
    let mut action = agent.action("acp-interrupt", "explicit-interrupt");
    action["prompt_id"] = json!("stale-prompt");
    host.deny(action.clone(), &["stale-interaction"]);
    assert!(!agent.root.join("cancel.received").exists());
    action["prompt_id"] = prompt_id;
    let cancel = host.call(action.clone());
    assert_eq!(cancel["state"], "accepted");
    host.deny(action, &["duplicate-call"]);
    let original = host.completed(&agent, &admitted["work_id"]);
    assert_eq!(original["outcome"]["stop_reason"], "cancelled");
    assert_eq!(original["rpc_id"], agent.effect()["rpc_id"]);
    assert!(agent.root.join("cancel.received").exists());
    assert!(!agent.root.join("unexpected-cancel").exists());
    host.close(&agent);
}

fn invalid_cancel_permission_preserves_session(mode: &str) {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open(mode);
    host.ready(&mut agent);
    let admitted = host.prompt(&agent, "cancel-with-invalid-permission", "add 17 25");
    host.barrier(&agent, "prompt");
    let mut action = agent.action("acp-interrupt", "explicit-invalid-permission-interrupt");
    action["prompt_id"] = host.work(&agent, &admitted["work_id"])["prompt_id"].clone();
    host.call(action);
    let original = host.until("cancelled prompt has a terminal disposition", || {
        let work = host.work(&agent, &admitted["work_id"]);
        matches!(
            work["state"].as_str(),
            Some("completed" | "unknown" | "failed")
        )
        .then_some(work)
    });
    assert_eq!(original["state"], "completed", "{original}");
    assert_eq!(original["outcome"]["stop_reason"], "cancelled");
    assert!(agent.root.join("invalid-permission-rejected").exists());
    assert_eq!(host.inspect(&agent)["lifecycle"], "ready");
    let followup = host.prompt(&agent, "after-invalid-permission", "add 10 5");
    host.completed(&agent, &followup["work_id"]);
    let effect = agent.effect();
    assert_eq!(effect["session_id"], agent.target["session_id"]);
    assert_eq!(effect["value"], 57);
    assert_eq!(effect["commands"], 2);
    host.close(&agent);
    sentinel.alive();
}

#[test]
fn e04s01_invalid_cancel_permission_params_preserve_session() {
    invalid_cancel_permission_preserves_session("interrupt-malformed");
}

#[test]
fn e04s01_wrong_session_cancel_permission_preserves_session() {
    invalid_cancel_permission_preserves_session("interrupt-wrong-session");
}

#[test]
fn e04s01_failed_auth_requires_new_explicit_choice_and_retains_failed_work() {
    let host = Host::new();
    let mut agent = host.open("auth-fail-once");
    host.until("auth required", || {
        (host.inspect(&agent)["lifecycle"] == "auth-required").then_some(())
    });
    let mut request = agent.action("acp-authenticate", "failed-explicit-auth");
    request["method_id"] = json!("agent-login");
    let failed = host.call(request.clone());
    host.until("auth error retained without ending session setup", || {
        let work = host.work(&agent, &failed["work_id"]);
        (work["state"] == "failed" && host.inspect(&agent)["lifecycle"] == "auth-required")
            .then_some(())
    });
    assert!(!agent.root.join("authenticate.release").exists());
    host.deny(request.clone(), &["duplicate-call"]);
    request["call_id"] = json!("fresh-explicit-auth");
    let retried = host.call(request);
    host.barrier(&agent, "authenticate");
    agent.release("authenticate");
    host.ready(&mut agent);
    assert_eq!(host.work(&agent, &failed["work_id"])["state"], "failed");
    assert_eq!(host.work(&agent, &retried["work_id"])["state"], "completed");
    host.close(&agent);
}

#[test]
fn e04s01_reserved_target_capacity_and_starting_shutdown_remain_explicit() {
    let host = Host::new();
    let mut agents = Vec::new();
    for _ in 0..8 {
        let agent = host.open("hold-initialize");
        host.barrier(&agent, "initialize");
        agents.push(agent);
    }
    let project = host.call(json!({"op":"snapshot"}))["projects"][0]["id"].clone();
    let controller =
        host.call(json!({"op":"controller-create","role":"external","label":"Over capacity"}));
    let peer = Path::new(env!("CARGO_BIN_EXE_amux"))
        .parent()
        .unwrap()
        .join("examples/acp_fixture");
    host.deny(
        json!({"op":"acp-open","project":project,"controller":controller["controller"],
        "command":[peer,host.root,"hold-initialize"]}),
        &["capacity-exceeded"],
    );
    host.close(&agents[0]);
    let mut replacement = host.open("arithmetic");
    host.ready(&mut replacement);
    let admitted = host.prompt(&replacement, "capacity-released", "add 17 25");
    host.completed(&replacement, &admitted["work_id"]);
    assert_eq!(replacement.effect()["value"], 42);
    host.close(&replacement);
    host.call(json!({"op":"shutdown"}));
    for agent in &agents[1..] {
        host.reaped(agent, "peer.pid");
    }
}

#[test]
fn e04s01_spawn_failure_has_no_child_and_never_reuses_target_lifetime() {
    let host = Host::new();
    let snapshot = host.call(json!({"op":"snapshot"}));
    let controller =
        host.call(json!({"op":"controller-create","role":"external","label":"Failed spawn"}));
    let opened = host.call(
        json!({"op":"acp-open","project":snapshot["projects"][0]["id"],
        "controller":controller["controller"],"command":[host.root.join("does-not-exist")]}),
    );
    let failed = host.until("asynchronous launch failure inspectable", || {
        let result = host.call(json!({"op":"acp-inspect","target":opened["target"]}));
        (result["lifecycle"] == "failed" && result["resource"]["cleanup_complete"] == true)
            .then_some(result)
    });
    assert!(failed["target"]["session_id"].is_null());
    assert!(failed["work"].as_array().unwrap().is_empty());
    let mut replacement = host.open("arithmetic");
    host.ready(&mut replacement);
    assert_ne!(replacement.target["id"], opened["target"]["id"]);
    assert_ne!(
        replacement.target["instance_id"],
        opened["target"]["instance_id"]
    );
    host.close(&replacement);
}

#[test]
fn e04s01_close_racing_daemon_shutdown_reaps_once_without_resurrection() {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("descendants");
    host.ready(&mut agent);
    host.until("descendant alive before shutdown race", || {
        agent.root.join("descendant.pid").exists().then_some(())
    });
    let gate = std::sync::Arc::new(std::sync::Barrier::new(3));
    let socket = host.socket.clone();
    let close_request = agent.action("acp-close", "close-racing-shutdown");
    let close_gate = gate.clone();
    let closer = std::thread::spawn(move || {
        close_gate.wait();
        exchange(&socket, close_request)
    });
    let socket = host.socket.clone();
    let shutdown_gate = gate.clone();
    let shutdown = std::thread::spawn(move || {
        shutdown_gate.wait();
        exchange(&socket, json!({"op":"shutdown"}))
    });
    gate.wait();
    let closed = closer.join().unwrap();
    let stopped = shutdown.join().unwrap();
    assert!(
        closed.as_ref().is_ok_and(|r| r["ok"] == true)
            || stopped.as_ref().is_ok_and(|r| r["ok"] == true),
        "neither close nor shutdown admitted"
    );
    host.reaped(&agent, "peer.pid");
    host.reaped(&agent, "descendant.pid");
    sentinel.alive();
}

#[test]
fn e04s01_native_input_execution_screen_and_close_reject_acp_without_effect() {
    let host = Host::new();
    let mut agent = host.open("arithmetic");
    host.ready(&mut agent);
    let id = agent.target["id"].clone();
    for request in [
        json!({"op":"input","terminal":id,"control":agent.opened["control"],"text":"add 900 1\r"}),
        json!({"op":"execute","terminal":id,"control":agent.opened["control"],"source":"add 900 1"}),
        json!({"op":"screen","terminal":id}),
        json!({"op":"close","terminal":id}),
    ] {
        host.deny(request, &["wrong-target-kind"]);
    }
    assert!(!agent.root.join("effect.json").exists());
    let admitted = host.prompt(&agent, "fresh-typed-prompt", "add 17 25");
    host.completed(&agent, &admitted["work_id"]);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(agent.effect()["value"], 42);
    host.close(&agent);
}

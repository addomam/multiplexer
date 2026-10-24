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

#[path = "acp/native.rs"]
mod native;

#[path = "acp/acceptance.rs"]
mod acceptance;

#[test]
#[ignore = "requires explicit private Codex/Grok OAuth, model and usage approval"]
fn e04s04_live_adapters() {
    native::live::run();
}

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

#[test]
fn e04s02_api_form_answer_resumes_exact_pending_prompt() {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("form-prompt");
    host.ready(&mut agent);
    let admitted = host.prompt(&agent, "prompt-awaiting-genuine-form", "add 17 25");
    let form = host.until("genuine pending form on the original session", || {
        assert!(
            !agent.root.join("form-rejected").exists(),
            "pre-S02 broker rejected the genuine form instead of publishing it"
        );
        let projection = host.inspect(&agent);
        assert_eq!(projection["lifecycle"], "ready", "{projection}");
        projection["content"]["interactions"]
            .as_array()?
            .iter()
            .find(|interaction| interaction["kind"] == "form" && interaction["state"] == "pending")
            .cloned()
    });
    assert_eq!(form["scope"]["sessionId"], agent.target["session_id"]);
    assert!(!agent.root.join("effect.json").exists());
    let mut answer = agent.action("acp-answer", "deliberate-form-answer");
    answer["interaction_id"] = form["interaction_id"].clone();
    answer["revision"] = form["revision"].clone();
    answer["action"] = json!("accept");
    answer["values"] = json!({"bonus":15});
    host.call(answer);
    let completed = host.completed(&agent, &admitted["work_id"]);
    assert_eq!(completed["outcome"]["stop_reason"], "end_turn");
    let effect = agent.effect();
    assert_eq!(effect["session_id"], agent.target["session_id"]);
    assert_eq!(effect["value"], 57);
    assert_eq!(effect["commands"], 1);
    host.close(&agent);
    sentinel.alive();
}

fn pending(host: &Host, agent: &Agent, kind: &str) -> Value {
    host.until("exact pending canonical interaction", || {
        let projection = host.inspect(agent);
        assert!(
            !matches!(projection["lifecycle"].as_str(), Some("failed" | "ended")),
            "{projection}"
        );
        projection["content"]["interactions"]
            .as_array()?
            .iter()
            .find(|i| i["kind"] == kind && i["state"] == "pending")
            .cloned()
    })
}

fn form_answer(
    agent: &Agent,
    form: &Value,
    call: &str,
    action: &str,
    values: Option<Value>,
) -> Value {
    let mut request = agent.action("acp-answer", call);
    request["interaction_id"] = form["interaction_id"].clone();
    request["revision"] = form["revision"].clone();
    request["action"] = json!(action);
    if let Some(values) = values {
        request["values"] = values;
    }
    request
}

fn permission_answer(agent: &Agent, permission: &Value, call: &str, option: &str) -> Value {
    let mut request = agent.action("acp-permission", call);
    request["interaction_id"] = permission["interaction_id"].clone();
    request["revision"] = permission["revision"].clone();
    request["option_id"] = json!(option);
    request
}

// Original persistent registration/provenance, not a controlling human PTY.
// Parent's separate CLI/PTY smoke proves the actual human surface.
struct OriginalNative(BufReader<UnixStream>);

impl OriginalNative {
    fn new(host: &Host) -> Self {
        let stream = UnixStream::connect(&host.socket).unwrap();
        stream.set_read_timeout(Some(DEADLINE)).unwrap();
        stream.set_write_timeout(Some(DEADLINE)).unwrap();
        Self(BufReader::new(stream))
    }

    fn exchange(&mut self, mut request: Value) -> Value {
        request["version"] = json!(1);
        request["persistent"] = json!(true);
        serde_json::to_writer(self.0.get_mut(), &request).unwrap();
        self.0.get_mut().write_all(b"\n").unwrap();
        let mut line = String::new();
        self.0
            .by_ref()
            .take(8 * 1024 * 1024)
            .read_line(&mut line)
            .unwrap();
        assert!(line.ends_with('\n'));
        serde_json::from_str(&line).unwrap()
    }

    fn call(&mut self, request: Value) -> Value {
        let response = self.exchange(request);
        assert_eq!(response["ok"], true, "{response}");
        response["result"].clone()
    }

    fn acquire(&mut self, host: &Host, agent: &mut Agent) {
        let registration = self.call(
            json!({"op":"controller-create","role":"attachment","label":"Original S02 connection"}),
        );
        self.call(
            json!({"op":"control-handle","handle":registration["controller"],"method":"view",
            "view":{"serial":1,"target":{"kind":"acp","id":agent.target["id"]},"focused":true}}),
        );
        let ownership =
            host.view(agent.target["id"].as_str().unwrap()).unwrap()["ownership"].clone();
        let grant = self.call(json!({"op":"ownership-claim","target":{"kind":"acp","id":agent.target["id"]},
            "controller":registration["controller"],"mode":"force","observed":ownership["observed"]}));
        agent.opened["control"] = grant["control"].clone();
        agent.opened["generation"] = grant["generation"].clone();
        agent.controller_id = registration["controller_id"].clone();
    }
}

#[test]
fn e04s02_request_scoped_initialize_and_session_forms_resume_original_bootstrap() {
    for mode in ["s02-init", "s02-session"] {
        let host = Host::new();
        let mut sentinel = Sentinel::new();
        let mut agent = host.open(mode);
        let form = pending(&host, &agent, "form");
        assert!(form["scope"].get("sessionId").is_none(), "{form}");
        assert!(form["scope"]["requestId"].is_number(), "{form}");
        let metadata = host.inspect(&agent);
        assert!(metadata["foreground"].is_null());
        let mut forged = form_answer(
            &agent,
            &form,
            "foreign-revision",
            "accept",
            Some(json!({"bonus":99})),
        );
        forged["revision"] = json!(2);
        host.deny(forged, &["stale-interaction"]);
        host.call(form_answer(
            &agent,
            &form,
            "bootstrap-config",
            "accept",
            Some(json!({"bonus":15})),
        ));
        host.ready(&mut agent);
        let prompt = host.prompt(&agent, "after-exact-bootstrap-response", "add 17 25");
        let original = host.completed(&agent, &prompt["work_id"]);
        assert_eq!(original["outcome"]["stop_reason"], "end_turn");
        assert_eq!(agent.effect()["value"], 57);
        assert_eq!(agent.effect()["rpc_id"], original["rpc_id"]);
        assert_eq!(agent.effect()["session_id"], agent.target["session_id"]);
        host.close(&agent);
        sentinel.alive();
    }
}

#[test]
fn e04s02_authentication_form_uses_exact_retained_auth_rpc_without_new_prompt() {
    let host = Host::new();
    let mut agent = host.open("auth-form");
    host.until("explicit auth required", || {
        (host.inspect(&agent)["lifecycle"] == "auth-required").then_some(())
    });
    let mut authenticate = agent.action("acp-authenticate", "explicit-auth");
    authenticate["method_id"] = json!("agent-login");
    let admitted = host.call(authenticate);
    let form = pending(&host, &agent, "form");
    assert_eq!(
        form["scope"]["requestId"],
        host.work(&agent, &admitted["work_id"])["rpc_id"]
    );
    host.call(form_answer(
        &agent,
        &form,
        "auth-config",
        "accept",
        Some(json!({"bonus":15})),
    ));
    host.barrier(&agent, "authenticate");
    agent.release("authenticate");
    // Session establishment can overtake a caller inspecting the auth result.
    // Resync the exact read tuple; never relax stale-target mutation checks.
    host.ready(&mut agent);
    let completed = host.completed(&agent, &admitted["work_id"]);
    assert_eq!(completed["rpc_id"], form["scope"]["requestId"]);
    let prompt = host.prompt(&agent, "after-auth-form", "add 17 25");
    host.completed(&agent, &prompt["work_id"]);
    assert_eq!(agent.effect()["value"], 57);
    assert!(!agent.root.join("unexpected-cancel").exists());
    host.close(&agent);
}

#[test]
fn e04s02_parent_rpc_completion_retains_responder_without_rebinding_scope() {
    let host = Host::new();
    let mut agent = host.open("s02-parent-completed");
    host.ready(&mut agent);
    let form = pending(&host, &agent, "form");
    assert_eq!(form["scope"]["requestId"], 0);
    assert_eq!(form["scope_parent_state"], "completed");
    host.call(form_answer(
        &agent,
        &form,
        "completed-parent-form",
        "accept",
        Some(json!({"bonus":15})),
    ));
    host.until("original completed parent responder consumed", || {
        agent.root.join("scoped-response").exists().then_some(())
    });
    let prompt = host.prompt(&agent, "not-rebound-to-this-prompt", "add 17 25");
    let work = host.completed(&agent, &prompt["work_id"]);
    assert_ne!(work["rpc_id"], form["scope"]["requestId"]);
    assert_eq!(agent.effect()["value"], 57);
    host.close(&agent);
}

#[test]
fn e04s02_all_supported_values_validate_before_actual_arithmetic_and_defaults_never_submit() {
    let host = Host::new();
    let mut agent = host.open("s02-rich");
    host.ready(&mut agent);
    let prompt = host.prompt(&agent, "typed-rich-form", "add 17 25");
    let form = pending(&host, &agent, "form");
    assert!(!agent.root.join("effect.json").exists());
    let values = json!({"bonus":5,"ratio":1.25,"enabled":false,"text":"λa","choice":"subtract",
        "titled":"twenty","flags":["a","b"],"titled_flags":["red"]});
    for (index, (field, invalid)) in [
        ("bonus", json!(101)),
        ("bonus", json!(0.5)),
        ("ratio", json!(2.01)),
        ("enabled", json!(null)),
        ("text", json!("λ")),
        ("text", json!("four")),
        ("choice", json!("unavailable")),
        ("titled", json!("not-offered")),
        ("flags", json!(["a", "a"])),
        ("flags", json!([])),
        ("flags", json!(["a", 3])),
        ("titled_flags", json!(["red", "blue", "red"])),
        ("optional", json!(null)),
    ]
    .into_iter()
    .enumerate()
    {
        let mut invalid_values = values.clone();
        invalid_values[field] = invalid;
        host.deny(
            form_answer(
                &agent,
                &form,
                &format!("invalid-{index}"),
                "accept",
                Some(invalid_values),
            ),
            &["invalid-request"],
        );
    }
    let mut missing = values.clone();
    missing.as_object_mut().unwrap().remove("enabled");
    host.deny(
        form_answer(&agent, &form, "missing-required", "accept", Some(missing)),
        &["invalid-request"],
    );
    assert!(!agent.root.join("effect.json").exists());
    let accepted = host.call(form_answer(
        &agent,
        &form,
        "typed-accept",
        "accept",
        Some(values.clone()),
    ));
    let original = host.completed(&agent, &prompt["work_id"]);
    assert_eq!(agent.effect()["value"], 77);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(agent.effect()["rpc_id"], original["rpc_id"]);
    let projection = host.inspect(&agent);
    let interaction = projection["content"]["interactions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["interaction_id"] == form["interaction_id"])
        .unwrap();
    assert_eq!(interaction["selection"]["content"], values);
    assert_eq!(interaction["selected_work_id"], accepted["work_id"]);
    assert_eq!(interaction["application"], "unknown");
    assert!(
        interaction["validation_limitations"]
            .as_array()
            .is_some_and(|limitations| limitations.iter().any(|v| v
                .as_str()
                .is_some_and(|s| s.contains("pattern") && s.contains("format"))))
    );
    let public = host.call(json!({"op":"snapshot"})).to_string();
    assert!(!public.contains("Synthetic private arithmetic question"));
    assert!(!public.contains("requestedSchema"));
    let metadata = exchange(
        &host.socket,
        json!({"op":"acp-inspect","target":agent.target}),
    )
    .unwrap()["result"]
        .clone();
    assert!(metadata.get("content").is_none());
    for detail in metadata["interactions"].as_array().unwrap() {
        assert!(detail.get("schema").is_none());
        assert!(detail.get("selection").is_none());
        assert!(detail.get("message").is_none());
    }
    assert!(!projection["work"].as_array().unwrap().iter().any(|w| {
        w["call_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("invalid-") || id == "missing-required")
    }));
    host.close(&agent);
}

#[test]
fn e04s02_decline_and_cancel_are_distinct_deliberate_actions() {
    for (mode, action, expected) in [
        ("s02-decline", "decline", 32),
        ("s02-form-cancel", "cancel", 22),
    ] {
        let host = Host::new();
        let mut agent = host.open(mode);
        host.ready(&mut agent);
        let prompt = host.prompt(&agent, "explicit-disposition", "add 17 25");
        let form = pending(&host, &agent, "form");
        host.deny(
            form_answer(
                &agent,
                &form,
                "invalid-disposition-content",
                action,
                Some(json!({})),
            ),
            &["invalid-request"],
        );
        host.call(form_answer(
            &agent,
            &form,
            "reviewed-disposition",
            action,
            None,
        ));
        let original = host.completed(&agent, &prompt["work_id"]);
        assert_eq!(original["outcome"]["stop_reason"], "end_turn");
        assert_eq!(agent.effect()["value"], expected);
        assert_eq!(
            host.inspect(&agent)["content"]["interactions"][0]["selection"]["action"],
            action
        );
        assert!(!agent.root.join("unexpected-cancel").exists());
        host.close(&agent);
    }
}

#[test]
fn e04s02_exact_offered_allow_and_reject_once_and_always_control_real_tool_effect() {
    for (option, allows) in [
        ("opaque-α", true),
        ("opaque-β", true),
        ("opaque-γ", false),
        ("opaque-δ", false),
    ] {
        let host = Host::new();
        let mut sentinel = Sentinel::new();
        let mut agent = host.open("s02-permission");
        host.ready(&mut agent);
        let prompt = host.prompt(&agent, "tool-gated-original-prompt", "add 17 25");
        let permission = pending(&host, &agent, "permission");
        assert_eq!(permission["scope"]["sessionId"], agent.target["session_id"]);
        assert_eq!(
            permission["scope"]["toolCallId"],
            permission["tool_call"]["toolCallId"]
        );
        host.deny(
            permission_answer(&agent, &permission, "fabricated-option", "allow_once"),
            &["invalid-request"],
        );
        assert!(!agent.root.join("tool-effect").exists());
        host.call(permission_answer(
            &agent,
            &permission,
            "reviewed-exact-option",
            option,
        ));
        let work = host.completed(&agent, &prompt["work_id"]);
        assert_eq!(work["outcome"]["stop_reason"], "end_turn");
        assert_eq!(agent.effect()["value"], if allows { 42 } else { 0 });
        assert_eq!(agent.root.join("tool-effect").exists(), allows);
        assert_eq!(agent.effect()["rpc_id"], work["rpc_id"]);
        assert_eq!(
            host.inspect(&agent)["content"]["interactions"][0]["selection"]["option_id"],
            option
        );
        host.close(&agent);
        sentinel.alive();
    }
}

#[test]
fn e04s02_original_native_and_api_first_valid_response_win_in_both_admission_orders() {
    for kind in ["form", "permission"] {
        for native_first in [false, true] {
            let host = Host::new();
            let mut agent = host.open(if kind == "form" {
                "s02-form"
            } else {
                "s02-permission"
            });
            host.ready(&mut agent);
            let mut native = OriginalNative::new(&host);
            native.acquire(&host, &mut agent);
            let prompt = host.prompt(&agent, "same-native-owned-prompt", "add 17 25");
            let interaction = pending(&host, &agent, kind);
            let api = if kind == "form" {
                form_answer(
                    &agent,
                    &interaction,
                    "api-choice",
                    "accept",
                    Some(json!({"bonus":15})),
                )
            } else {
                permission_answer(&agent, &interaction, "api-choice", "opaque-α")
            };
            let original_native = if kind == "form" {
                form_answer(
                    &agent,
                    &interaction,
                    "native-choice",
                    "accept",
                    Some(json!({"bonus":-7})),
                )
            } else {
                permission_answer(&agent, &interaction, "native-choice", "opaque-γ")
            };
            // Both envelopes capture the same pending revision before either
            // admission. The first receipt is the ordering barrier, not sleep.
            let (winner, loser) = if native_first {
                let winner = native.call(original_native);
                let loser = exchange(&host.socket, api).unwrap();
                (winner, loser)
            } else {
                let winner = host.call(api);
                let loser = native.exchange(original_native);
                (winner, loser)
            };
            assert_eq!(loser["ok"], false, "{loser}");
            assert_eq!(loser["error"]["code"], "stale-interaction", "{loser}");
            let original = host.completed(&agent, &prompt["work_id"]);
            assert_eq!(original["outcome"]["stop_reason"], "end_turn");
            let expected = if kind == "form" {
                if native_first { 35 } else { 57 }
            } else if native_first {
                0
            } else {
                42
            };
            assert_eq!(agent.effect()["value"], expected);
            assert_eq!(agent.effect()["commands"], 1);
            let projection = host.inspect(&agent);
            assert_eq!(
                projection["content"]["interactions"][0]["selected_work_id"],
                winner["work_id"]
            );
            let losing_id = if native_first {
                "api-choice"
            } else {
                "native-choice"
            };
            assert!(
                !projection["work"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|w| w["call_id"] == losing_id)
            );
            assert!(!agent.root.join("unexpected-cancel").exists());
            host.close(&agent);
        }
    }
}

#[test]
fn e04s02_reused_wire_id_gets_fresh_interaction_and_old_ui_answer_cannot_rebind() {
    let host = Host::new();
    let mut agent = host.open("s02-form");
    host.ready(&mut agent);
    let first = host.prompt(&agent, "first-turn", "add 17 25");
    let old = pending(&host, &agent, "form");
    host.call(form_answer(
        &agent,
        &old,
        "first-answer",
        "accept",
        Some(json!({"bonus":15})),
    ));
    host.completed(&agent, &first["work_id"]);
    let second = host.prompt(&agent, "second-turn", "add 1 2");
    let current = pending(&host, &agent, "form");
    assert_ne!(current["interaction_id"], old["interaction_id"]);
    host.deny(
        form_answer(
            &agent,
            &old,
            "stale-ui-choice",
            "accept",
            Some(json!({"bonus":97})),
        ),
        &["stale-interaction"],
    );
    host.call(form_answer(
        &agent,
        &current,
        "new-deliberate-choice",
        "accept",
        Some(json!({"bonus":5})),
    ));
    host.completed(&agent, &second["work_id"]);
    assert_eq!(agent.effect()["value"], 65);
    assert_eq!(agent.effect()["commands"], 2);
    let inspect = host.inspect(&agent);
    assert!(
        !inspect["work"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["call_id"] == "stale-ui-choice")
    );
    host.close(&agent);
}

#[test]
fn e04s02_interrupt_pending_selected_and_late_interactions_reserves_exact_obligations() {
    for selected in ["none", "form", "permission"] {
        let host = Host::new();
        let mut agent = host.open(match selected {
            "form" => "s02-selected-cancel",
            "permission" => "s02-selected-permission-cancel",
            _ => "s02-cancel",
        });
        host.ready(&mut agent);
        let prompt = host.prompt(&agent, "cancel-exact-prompt", "add 17 25");
        host.barrier(&agent, "interactions");
        let form = pending(&host, &agent, "form");
        let permission = pending(&host, &agent, "permission");
        let accepted_answer = match selected {
            "form" => Some(host.call(form_answer(
                &agent,
                &form,
                "selected-before-cancel",
                "accept",
                Some(json!({"bonus":15})),
            ))),
            "permission" => Some(host.call(permission_answer(
                &agent,
                &permission,
                "selected-before-cancel",
                "opaque-γ",
            ))),
            _ => None,
        };
        let mut interrupt = agent.action("acp-interrupt", "cancel-with-obligations");
        interrupt["prompt_id"] = host.work(&agent, &prompt["work_id"])["prompt_id"].clone();
        let cancel = host.call(interrupt);
        host.deny(
            permission_answer(
                &agent,
                &permission,
                "losing-permission-after-cancel",
                "opaque-α",
            ),
            &["stale-interaction"],
        );
        host.deny(
            form_answer(&agent, &form, "losing-form-after-cancel", "decline", None),
            &["stale-interaction"],
        );
        agent.release("interactions");
        host.barrier(&agent, "cancel-responses");
        let projection = host.inspect(&agent);
        assert_eq!(host.work(&agent, &prompt["work_id"])["state"], "submitted");
        assert_eq!(projection["foreground"]["state"], "cancelling");
        let interactions = projection["content"]["interactions"].as_array().unwrap();
        assert_eq!(interactions.len(), 4);
        assert!(interactions.iter().all(|i| i["state"] == "selected"));
        for interaction in interactions {
            let original_selected = (selected == "form"
                && interaction["interaction_id"] == form["interaction_id"])
                || (selected == "permission"
                    && interaction["interaction_id"] == permission["interaction_id"]);
            assert_eq!(
                interaction["selected_work_id"],
                if original_selected {
                    accepted_answer.as_ref().unwrap()["work_id"].clone()
                } else {
                    cancel["work_id"].clone()
                }
            );
            assert_eq!(interaction["application"], "unknown");
            host.deny(
                if interaction["kind"] == "form" {
                    form_answer(
                        &agent,
                        interaction,
                        &format!("late-choice-{}", interaction["interaction_id"]),
                        "cancel",
                        None,
                    )
                } else {
                    permission_answer(
                        &agent,
                        interaction,
                        &format!("late-choice-{}", interaction["interaction_id"]),
                        "opaque-α",
                    )
                },
                &["stale-interaction"],
            );
        }
        agent.release("cancel-responses");
        let original = host.completed(&agent, &prompt["work_id"]);
        assert_eq!(original["outcome"]["stop_reason"], "cancelled");
        assert!(!agent.root.join("effect.json").exists());
        host.close(&agent);
    }
}

#[test]
fn e04s02_interaction_content_and_pending_count_caps_fail_without_eviction_or_choice_loss() {
    for (mode, retained) in [("s02-content-capacity", 6), ("s02-count-capacity", 64)] {
        let host = Host::new();
        let mut sentinel = Sentinel::new();
        let mut agent = host.open(mode);
        host.ready(&mut agent);
        let prompt = host.prompt(&agent, "capacity-original-work", "add 17 25");
        for index in 0..retained {
            host.barrier(&agent, &format!("content-{index}"));
            host.until("new live responder retained before next arrival", || {
                let inspect = host.inspect(&agent);
                (inspect["interactions"].as_array()?.len() == index + 1).then_some(())
            });
            agent.release(&format!("content-{index}"));
        }
        host.until("exact instance reports resource failure", || {
            (host.inspect(&agent)["lifecycle"] == "failed").then_some(())
        });
        let projection = host.inspect(&agent);
        assert_eq!(
            projection["interactions"].as_array().unwrap().len(),
            retained
        );
        assert!(
            projection["interactions"]
                .as_array()
                .unwrap()
                .iter()
                .all(|i| i["state"] == "unknown")
        );
        assert_eq!(host.work(&agent, &prompt["work_id"])["state"], "unknown");
        assert!(!agent.root.join("effect.json").exists());
        host.reaped(&agent, "peer.pid");
        host.call(json!({"op":"controller-create","role":"external","label":"responsive after interaction exhaustion"}));
        sentinel.alive();
    }
}

#[test]
fn e04s02_submitted_form_response_followed_by_eof_is_not_agent_application() {
    let host = Host::new();
    let mut agent = host.open("s02-selected-eof");
    host.ready(&mut agent);
    let prompt = host.prompt(&agent, "unconfirmed-original", "add 17 25");
    host.barrier(&agent, "answer-selected");
    let form = pending(&host, &agent, "form");
    let answer = host.call(form_answer(
        &agent,
        &form,
        "unconfirmed-selection",
        "accept",
        Some(json!({"bonus":15})),
    ));
    host.until("actual response write submitted", || {
        (host.work(&agent, &answer["work_id"])["delivery"] == "submitted").then_some(())
    });
    agent.release("answer-selected");
    host.until("EOF unknown on original prompt", || {
        (host.work(&agent, &prompt["work_id"])["state"] == "unknown").then_some(())
    });
    let projection = host.inspect(&agent);
    assert_eq!(
        projection["content"]["interactions"][0]["application"],
        "unknown"
    );
    assert_eq!(
        host.work(&agent, &answer["work_id"])["application"],
        "unknown"
    );
    assert!(!agent.root.join("effect.json").exists());
    host.reaped(&agent, "peer.pid");
}

#[test]
fn e04s02_malformed_and_unsupported_schema_receive_errors_and_original_prompt_still_applies() {
    for schema in [
        json!({"type":"array","properties":{}}),
        json!({"type":"object","properties":{"n":{"type":"integer","default":"coercible"}}}),
        json!({"type":"object","properties":{"a":{"type":"array","items":{"type":"string","enum":["x"]},"default":["x",4]}}}),
        json!({"type":"object","properties":{"nested":{"type":"object","properties":{}}}}),
        json!({"type":"object","properties":{"n":{"type":"_future"}}}),
        json!({"type":"object","properties":{"n":{"type":"number","minimum":4,"maximum":3}}}),
        json!({"type":"object","properties":{"s":{"type":"string","enum":["a"],"oneOf":[{"const":"b","title":"B"}]}}}),
    ] {
        let host = Host::new();
        let mut agent = host.open("s02-invalid-schema");
        host.ready(&mut agent);
        fs::write(
            agent.root.join("schema.json"),
            serde_json::to_vec(&schema).unwrap(),
        )
        .unwrap();
        let prompt = host.prompt(&agent, "after-unsupported-schema", "add 17 25");
        let original = host.completed(&agent, &prompt["work_id"]);
        assert_eq!(original["outcome"]["stop_reason"], "end_turn");
        assert_eq!(agent.effect()["value"], 42);
        assert!(agent.root.join("invalid-schema-rejected").exists());
        assert_eq!(host.inspect(&agent)["interactions"], json!([]));
        host.close(&agent);
    }
}

#[test]
fn e04s02_unadvertised_url_filesystem_terminal_and_unknown_requests_get_exact_errors() {
    for mode in ["unsupported-bootstrap", "unsupported-prompt"] {
        let host = Host::new();
        let mut agent = host.open(mode);
        host.ready(&mut agent);
        let prompt = host.prompt(&agent, "after-unadvertised-requests", "add 17 25");
        host.completed(&agent, &prompt["work_id"]);
        assert_eq!(agent.effect()["value"], 42);
        assert!(agent.root.join("unsupported-complete").exists());
        host.close(&agent);
    }
}

#[test]
fn e04s02_request_scoped_prompt_and_invalid_scopes_preserve_exact_original_work() {
    for mode in ["s02-request-prompt", "s02-invalid-scopes"] {
        let host = Host::new();
        let mut agent = host.open(mode);
        host.ready(&mut agent);
        let prompt = host.prompt(&agent, "exact-scoped-prompt", "add 17 25");
        let form = pending(&host, &agent, "form");
        if mode == "s02-request-prompt" {
            assert_eq!(
                form["scope"]["requestId"],
                host.work(&agent, &prompt["work_id"])["rpc_id"]
            );
            assert!(form["scope"].get("sessionId").is_none());
        } else {
            assert!(agent.root.join("invalid-scopes-rejected").exists());
        }
        host.call(form_answer(
            &agent,
            &form,
            "scoped-prompt-answer",
            "accept",
            Some(json!({"bonus":15})),
        ));
        let original = host.completed(&agent, &prompt["work_id"]);
        assert_eq!(agent.effect()["value"], 57);
        assert_eq!(agent.effect()["rpc_id"], original["rpc_id"]);
        assert_eq!(agent.effect()["session_id"], agent.target["session_id"]);
        host.close(&agent);
    }
}

#[test]
fn e04s02_native_allow_then_api_deny_resume_same_session_without_second_tool_effect() {
    let host = Host::new();
    let mut agent = host.open("s02-permission");
    host.ready(&mut agent);
    let mut native = OriginalNative::new(&host);
    native.acquire(&host, &mut agent);
    let first = host.prompt(&agent, "native-allowed-tool", "add 17 25");
    let permission = pending(&host, &agent, "permission");
    native.call(permission_answer(
        &agent,
        &permission,
        "native-allow-always",
        "opaque-β",
    ));
    host.completed(&agent, &first["work_id"]);
    assert_eq!(agent.effect()["value"], 42);
    let marker = fs::read(agent.root.join("tool-effect")).unwrap();
    let second = host.prompt(&agent, "api-denied-tool", "add 100 200");
    let next = pending(&host, &agent, "permission");
    assert_ne!(next["interaction_id"], permission["interaction_id"]);
    host.call(permission_answer(
        &agent,
        &next,
        "api-reject-always",
        "opaque-δ",
    ));
    let original = host.completed(&agent, &second["work_id"]);
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(agent.effect()["commands"], 2);
    assert_eq!(agent.effect()["rpc_id"], original["rpc_id"]);
    assert_eq!(fs::read(agent.root.join("tool-effect")).unwrap(), marker);
    host.close(&agent);
}

#[test]
fn e04s02_force_and_pending_claim_cancellation_retain_same_unanswered_form() {
    let host = Host::new();
    let mut agent = host.open("s02-form");
    host.ready(&mut agent);
    let prompt = host.prompt(&agent, "form-through-ownership-transitions", "add 17 25");
    let form = pending(&host, &agent, "form");
    let stale_answer = form_answer(
        &agent,
        &form,
        "stale-answer",
        "accept",
        Some(json!({"bonus":97})),
    );
    let successor =
        host.call(json!({"op":"controller-create","role":"external","label":"Form successor"}));
    let ownership = host.view(agent.target["id"].as_str().unwrap()).unwrap()["ownership"].clone();
    let forced = host.call(
        json!({"op":"ownership-claim","target":{"kind":"acp","id":agent.target["id"]},
        "controller":successor["controller"],"mode":"force","observed":ownership["observed"]}),
    );
    host.deny(stale_answer, &["stale-control", "stale-handle"]);
    let requester = host.call(
        json!({"op":"controller-create","role":"external","label":"Withdrawn pending claimant"}),
    );
    let ownership = host.view(agent.target["id"].as_str().unwrap()).unwrap()["ownership"].clone();
    let claim = host.call(json!({"op":"ownership-claim","target":{"kind":"acp","id":agent.target["id"]},
        "controller":requester["controller"],"mode":"cooperative","when":{"kind":"now"},"observed":ownership["observed"]}));
    host.call(json!({"op":"control-handle","handle":claim["claim"],"method":"cancel"}));
    let retained = pending(&host, &agent, "form");
    assert_eq!(retained["interaction_id"], form["interaction_id"]);
    assert_eq!(retained["scope"], form["scope"]);
    assert!(!agent.root.join("effect.json").exists());
    assert!(!agent.root.join("unexpected-cancel").exists());
    let ownership = host.view(agent.target["id"].as_str().unwrap()).unwrap()["ownership"].clone();
    assert!(ownership["owner"].is_null());
    let recovered = host.call(
        json!({"op":"ownership-claim","target":{"kind":"acp","id":agent.target["id"]},
        "controller":successor["controller"],"mode":"force","observed":ownership["observed"]}),
    );
    assert_ne!(recovered["generation"], forced["generation"]);
    agent.opened = recovered;
    host.call(form_answer(
        &agent,
        &retained,
        "fresh-answer-after-recovery",
        "accept",
        Some(json!({"bonus":15})),
    ));
    let original = host.completed(&agent, &prompt["work_id"]);
    assert_eq!(agent.effect()["value"], 57);
    assert_eq!(agent.effect()["rpc_id"], original["rpc_id"]);
    assert_eq!(original["controller_id"], agent.controller_id);
    host.close(&agent);
}

#[test]
fn e04s02_completed_interaction_content_truncates_without_evicting_live_responder() {
    let host = Host::new();
    let mut agent = host.open("s02-history");
    host.ready(&mut agent);
    let prompt = host.prompt(&agent, "bounded-interaction-history", "add 17 25");
    let anchor = host.until("live anchor interaction", || {
        host.inspect(&agent)["content"]["interactions"]
            .as_array()?
            .iter()
            .find(|i| i["message"] == "Live responder never evicted")
            .cloned()
    });
    let mut oldest = None;
    for index in 0..8 {
        let form = host.until("next large history form", || {
            host.inspect(&agent)["content"]["interactions"]
                .as_array()?
                .iter()
                .find(|i| {
                    i["state"] == "pending" && i["interaction_id"] != anchor["interaction_id"]
                })
                .cloned()
        });
        if oldest.is_none() {
            oldest = Some(form.clone());
        }
        host.call(form_answer(
            &agent,
            &form,
            &format!("history-answer-{index}"),
            "accept",
            Some(json!({"bonus":1})),
        ));
        host.barrier(&agent, &format!("history-{index}"));
        host.until("writer releases completed interaction content", || {
            host.inspect(&agent)["content"]["interactions"]
                .as_array()?
                .iter()
                .find(|i| {
                    i["interaction_id"] == form["interaction_id"] && i["delivery"] == "submitted"
                })
                .map(|_| ())
        });
        agent.release(&format!("history-{index}"));
    }
    host.barrier(&agent, "history-answers");
    let inspect = host.inspect(&agent);
    assert!(inspect["truncation"]["interactions"].as_u64().unwrap() > 0);
    let retained = inspect["content"]["interactions"].as_array().unwrap();
    assert!(
        retained
            .iter()
            .any(|i| i["interaction_id"] == anchor["interaction_id"] && i["state"] == "pending")
    );
    let oldest = oldest.unwrap();
    assert!(
        !retained
            .iter()
            .any(|i| i["interaction_id"] == oldest["interaction_id"])
    );
    host.deny(
        form_answer(
            &agent,
            &oldest,
            "evicted-old-ui-choice",
            "accept",
            Some(json!({"bonus":97})),
        ),
        &["stale-interaction"],
    );
    host.call(form_answer(
        &agent,
        &anchor,
        "retained-live-answer",
        "accept",
        Some(json!({"bonus":15})),
    ));
    agent.release("history-answers");
    let original = host.completed(&agent, &prompt["work_id"]);
    assert_eq!(agent.effect()["value"], 65);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(agent.effect()["rpc_id"], original["rpc_id"]);
    host.close(&agent);
}

#[test]
fn e04s02_actual_response_pipe_failure_records_failed_delivery_not_agent_application() {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("s02-write-fail");
    host.ready(&mut agent);
    let prompt = host.prompt(&agent, "pipe-failure-original", "add 17 25");
    host.barrier(&agent, "closed-input");
    let form = pending(&host, &agent, "form");
    let answer = host.call(form_answer(
        &agent,
        &form,
        "response-to-closed-pipe",
        "accept",
        Some(json!({"bonus":15})),
    ));
    host.until("actual responder write failure", || {
        (host.work(&agent, &answer["work_id"])["delivery"] == "write-failed").then_some(())
    });
    let projection = host.inspect(&agent);
    assert_eq!(host.work(&agent, &answer["work_id"])["state"], "failed");
    assert_eq!(projection["content"]["interactions"][0]["state"], "failed");
    assert_eq!(
        projection["content"]["interactions"][0]["delivery"],
        "write-failed"
    );
    assert_eq!(
        projection["content"]["interactions"][0]["application"],
        "unknown"
    );
    host.until(
        "original prompt is unconfirmed after failed response",
        || (host.work(&agent, &prompt["work_id"])["state"] == "unknown").then_some(()),
    );
    assert!(!agent.root.join("effect.json").exists());
    host.reaped(&agent, "peer.pid");
    sentinel.alive();
}

fn s03_controller(host: &Host, label: &str) -> Value {
    host.call(json!({"op":"controller-create","role":"external","label":label}))
}

fn s03_ownership(host: &Host, agent: &Agent) -> Value {
    host.view(agent.target["id"].as_str().unwrap()).unwrap()["ownership"].clone()
}

fn s03_claim(host: &Host, agent: &Agent, controller: &Value, mode: &str, when: Value) -> Value {
    let mut request = json!({"op":"ownership-claim","target":{"kind":"acp","id":agent.target["id"]},
        "controller":controller["controller"],"mode":mode,
        "observed":s03_ownership(host, agent)["observed"]});
    if mode == "cooperative" {
        request["when"] = when;
    }
    host.call(request)
}

fn s03_handle(host: &Host, handle: &Value, method: &str) -> Value {
    host.call(json!({"op":"control-handle","handle":handle,"method":method}))
}

fn s03_same_work(actual: &Value, original: &Value) {
    for key in [
        "work_id",
        "call_id",
        "controller_id",
        "owner_generation",
        "rpc_id",
        "prompt_id",
    ] {
        assert_eq!(actual[key], original[key], "{key}");
    }
}

#[test]
fn e04s03_nonfatal_stalled_writer_keeps_admitted_work_across_force_and_socket_loss() {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("s03-stalled-prompt");
    host.ready(&mut agent);
    host.barrier(&agent, "s03-reads");
    let tuple = agent.target.clone();
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    // Larger than the pipe capacity, still within the public 64KiB text budget.
    let text = format!("add 17 25{}", " ".repeat(65536 - 9));
    let mut request = agent.action("acp-prompt", "stalled-lost-reply");
    request["version"] = json!(1);
    request["text"] = json!(text);
    let mut socket = UnixStream::connect(&host.socket).unwrap();
    socket.set_write_timeout(Some(DEADLINE)).unwrap();
    serde_json::to_writer(&mut socket, &request).unwrap();
    socket.write_all(b"\n").unwrap();
    let original = host.until("canonical admission despite blocked pipe", || {
        host.inspect(&agent)["work"]
            .as_array()?
            .iter()
            .find(|work| work["call_id"] == "stalled-lost-reply")
            .cloned()
    });
    drop(socket); // Admission is observed before discarding the unread reply.
    assert_eq!(original["state"], "accepted");
    assert_eq!(original["delivery"], "not-submitted");
    assert!(!agent.root.join("effect.json").exists());
    let b = s03_controller(&host, "stalled writer successor");
    let grant = s03_claim(&host, &agent, &b, "force", Value::Null);
    let recovered = s03_handle(&host, &b["controller"], "read");
    assert_eq!(recovered["controls"][0]["control"], grant["control"]);
    let retained = recovered["controls"][0]["retained_work"]
        .as_array()
        .unwrap()
        .iter()
        .find(|work| work["work_id"] == original["work_id"])
        .unwrap();
    s03_same_work(retained, &original);
    assert_eq!(retained["state"], "accepted");
    host.deny(
        agent.action("acp-close", "old-close-during-stall"),
        &["stale-control", "stale-handle"],
    );
    agent.opened = grant;
    assert_eq!(host.inspect(&agent)["target"], tuple);
    assert_eq!(host.inspect(&agent)["lifecycle"], "ready");
    agent.release("s03-reads");
    let completed = host.completed(&agent, &original["work_id"]);
    s03_same_work(&completed, &original);
    assert_eq!(completed["outcome"]["stop_reason"], "end_turn");
    assert_eq!(agent.effect()["rpc_id"], original["rpc_id"]);
    assert_eq!(agent.effect()["session_id"], tuple["session_id"]);
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    assert!(!agent.root.join("unexpected-cancel").exists());
    host.close(&agent);
    sentinel.alive();
}

#[test]
fn e04s03_cooperative_offer_reserves_b_against_c_while_genuine_question_is_active() {
    for kind in ["form", "permission"] {
        let host = Host::new();
        let mut agent = host.open(if kind == "form" {
            "s02-form"
        } else {
            "s02-permission"
        });
        host.ready(&mut agent);
        let tuple = agent.target.clone();
        let pid = fs::read(agent.root.join("peer.pid")).unwrap();
        let prompt = host.prompt(&agent, "question-before-cooperative-yield", "add 17 25");
        let interaction = pending(&host, &agent, kind);
        let original = host.work(&agent, &prompt["work_id"]);
        let b = s03_controller(&host, "reserved B");
        let c = s03_controller(&host, "excluded C");
        let claim = s03_claim(&host, &agent, &b, "cooperative", json!({"kind":"now"}));
        assert_eq!(claim["phase"], "waiting-owner");
        host.call(
            json!({"op":"control-handle","handle":agent.opened["control"],
            "method":"yield-to","claim_id":claim["claim_id"]}),
        );
        let offered = s03_handle(&host, &claim["claim"], "read");
        assert_eq!(offered["phase"], "offered", "{offered}");
        let ownership = s03_ownership(&host, &agent);
        assert!(ownership["owner"].is_null());
        assert_eq!(ownership["reservation"]["claim_id"], claim["claim_id"]);
        host.deny(
            json!({"op":"ownership-claim","target":{"kind":"acp","id":agent.target["id"]},
            "controller":c["controller"],"mode":"cooperative","when":{"kind":"now"},
            "observed":ownership["observed"]}),
            &["control-reserved"],
        );
        let stale = if kind == "form" {
            form_answer(
                &agent,
                &interaction,
                "yielded-old-answer",
                "accept",
                Some(json!({"bonus":97})),
            )
        } else {
            permission_answer(&agent, &interaction, "yielded-old-answer", "opaque-δ")
        };
        host.deny(stale, &["stale-control", "stale-handle"]);
        assert_eq!(
            pending(&host, &agent, kind)["interaction_id"],
            interaction["interaction_id"]
        );
        assert_eq!(
            host.inspect(&agent)["foreground"]["prompt_id"],
            original["prompt_id"]
        );
        assert!(!agent.root.join("effect.json").exists());
        agent.opened = s03_handle(&host, &offered["offer"], "accept");
        // Explicit accept happens while the original prompt is still waiting.
        assert!(host.work(&agent, &prompt["work_id"])["outcome"].is_null());
        host.deny(
            json!({"op":"control-handle","handle":offered["offer"],"method":"accept"}),
            &["offer-consumed"],
        );
        let answer = if kind == "form" {
            form_answer(
                &agent,
                &interaction,
                "b-deliberate-answer",
                "accept",
                Some(json!({"bonus":15})),
            )
        } else {
            permission_answer(&agent, &interaction, "b-deliberate-answer", "opaque-α")
        };
        host.call(answer);
        let completed = host.completed(&agent, &prompt["work_id"]);
        s03_same_work(&completed, &original);
        assert_eq!(
            agent.effect()["value"],
            if kind == "form" { 57 } else { 42 }
        );
        assert_eq!(agent.effect()["commands"], 1);
        assert_eq!(agent.effect()["rpc_id"], original["rpc_id"]);
        assert_eq!(host.inspect(&agent)["target"], tuple);
        assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
        assert!(!agent.root.join("unexpected-cancel").exists());
        host.close(&agent);
    }
}

#[test]
fn e04s03_cancelled_offer_cannot_touch_new_reservation_and_lost_accept_recovers_privately() {
    for yield_first in [false, true] {
        let host = Host::new();
        let mut agent = host.open("s02-form");
        host.ready(&mut agent);
        let tuple = agent.target.clone();
        let pid = fs::read(agent.root.join("peer.pid")).unwrap();
        let prompt = host.prompt(&agent, "retained-through-cancel-and-recovery", "add 17 25");
        let form = pending(&host, &agent, "form");
        let original = host.work(&agent, &prompt["work_id"]);
        let b = s03_controller(&host, "cancelled B");
        let c = s03_controller(&host, "new reserved C");
        let old = s03_claim(&host, &agent, &b, "cooperative", json!({"kind":"now"}));
        let old_control = agent.opened["control"].clone();
        let old_offer = yield_first.then(|| {
            host.call(
                json!({"op":"control-handle","handle":old_control,"method":"yield-to",
                "claim_id":old["claim_id"]}),
            );
            let offered = s03_handle(&host, &old["claim"], "read");
            assert_eq!(offered["phase"], "offered");
            offered["offer"].clone()
        });
        s03_handle(&host, &old["claim"], "cancel");
        let cancelled = s03_ownership(&host, &agent);
        assert!(cancelled["owner"].is_null());
        assert!(cancelled["reservation"].is_null());
        s03_same_work(&host.work(&agent, &prompt["work_id"]), &original);
        host.deny(
            json!({"op":"control-handle","handle":old_control,"method":"yield-to",
            "claim_id":old["claim_id"]}),
            &["stale-handle", "stale-control"],
        );
        agent.opened = s03_claim(&host, &agent, &b, "force", Value::Null);
        let next = s03_claim(&host, &agent, &c, "cooperative", json!({"kind":"now"}));
        for method in ["cancel", "force"] {
            let mut request = json!({"op":"control-handle","handle":old["claim"],"method":method});
            if method == "force" {
                request["observed"] = s03_ownership(&host, &agent)["observed"].clone();
            }
            host.deny(request, &["claim-completed"]);
        }
        if let Some(offer) = old_offer {
            host.deny(
                json!({"op":"control-handle","handle":offer,"method":"accept"}),
                &["stale-offer", "claim-completed"],
            );
        }
        assert_eq!(
            s03_ownership(&host, &agent)["reservation"]["claim_id"],
            next["claim_id"]
        );
        host.call(
            json!({"op":"control-handle","handle":agent.opened["control"],
            "method":"yield-to","claim_id":next["claim_id"]}),
        );
        let offer = s03_handle(&host, &next["claim"], "read");
        assert_eq!(offer["phase"], "offered");
        // Observe the accept through controller-private recovery, not a retry.
        let mut connection = UnixStream::connect(&host.socket).unwrap();
        connection.set_write_timeout(Some(DEADLINE)).unwrap();
        serde_json::to_writer(
            &mut connection,
            &json!({"version":1,"op":"control-handle",
            "handle":offer["offer"],"method":"accept"}),
        )
        .unwrap();
        connection.write_all(b"\n").unwrap();
        let private = host.until("lost accept recovered through original C handle", || {
            let state = s03_handle(&host, &c["controller"], "read");
            state["controls"].as_array()?.first().map(|_| state.clone())
        });
        drop(connection);
        agent.opened = private["controls"][0].clone();
        s03_same_work(&host.work(&agent, &prompt["work_id"]), &original);
        assert_eq!(
            pending(&host, &agent, "form")["interaction_id"],
            form["interaction_id"]
        );
        host.deny(
            json!({"op":"control-handle","handle":offer["offer"],"method":"accept"}),
            &["offer-consumed"],
        );
        host.call(form_answer(
            &agent,
            &form,
            "c-fresh-answer",
            "accept",
            Some(json!({"bonus":15})),
        ));
        let completed = host.completed(&agent, &prompt["work_id"]);
        s03_same_work(&completed, &original);
        assert_eq!(agent.effect()["value"], 57);
        assert_eq!(agent.effect()["rpc_id"], original["rpc_id"]);
        assert_eq!(host.inspect(&agent)["target"], tuple);
        assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
        assert!(!agent.root.join("unexpected-cancel").exists());
        host.close(&agent);
    }
}

#[test]
fn e04s03_interrupt_outcomes_wait_for_original_response_and_stale_id_cannot_cancel_new_work() {
    for outcome in ["cancelled", "end-turn", "error", "eof"] {
        let host = Host::new();
        let mut sentinel = Sentinel::new();
        let mut agent = host.open(&format!("s03-interrupt-{outcome}"));
        host.ready(&mut agent);
        let tuple = agent.target.clone();
        let prompt = host.prompt(&agent, "original-interrupted-prompt", "add 17 25");
        host.barrier(&agent, "prompt");
        let original = host.work(&agent, &prompt["work_id"]);
        let mut interrupt = agent.action("acp-interrupt", "deliberate-current-interrupt");
        interrupt["prompt_id"] = original["prompt_id"].clone();
        let b = s03_controller(&host, "interrupt successor");
        let grant = s03_claim(&host, &agent, &b, "force", Value::Null);
        host.deny(interrupt.clone(), &["stale-handle", "stale-control"]);
        agent.opened = grant;
        interrupt["control"] = agent.opened["control"].clone();
        interrupt["owner_generation"] = agent.opened["generation"].clone();
        let cancel = host.call(interrupt);
        host.barrier(&agent, "s03-cancel");
        let waiting = host.inspect(&agent);
        assert_eq!(waiting["target"], tuple);
        assert_eq!(waiting["foreground"]["state"], "cancelling");
        assert_eq!(waiting["foreground"]["prompt_id"], original["prompt_id"]);
        assert!(host.work(&agent, &prompt["work_id"])["outcome"].is_null());
        assert_ne!(
            host.work(&agent, &cancel["work_id"])["outcome"]["stop_reason"],
            "cancelled"
        );
        agent.release("s03-cancel");
        let terminal = host.until("actual original prompt terminal outcome", || {
            let work = host.work(&agent, &prompt["work_id"]);
            matches!(
                work["state"].as_str(),
                Some("completed" | "failed" | "unknown")
            )
            .then_some(work)
        });
        s03_same_work(&terminal, &original);
        match outcome {
            "cancelled" => {
                assert_eq!(terminal["state"], "completed");
                assert_eq!(terminal["outcome"]["stop_reason"], "cancelled");
            }
            "end-turn" => {
                assert_eq!(terminal["state"], "completed");
                assert_eq!(terminal["outcome"]["stop_reason"], "end_turn");
            }
            "error" => {
                assert_eq!(terminal["state"], "failed");
                assert_eq!(terminal["outcome"], json!({"kind":"error","code":-32603}));
            }
            "eof" => {
                assert_eq!(terminal["state"], "unknown");
                assert_ne!(terminal["outcome"]["stop_reason"], "cancelled");
            }
            _ => unreachable!(),
        }
        assert!(
            !host
                .call(json!({"op":"snapshot"}))
                .to_string()
                .contains("s03-private-interrupt-error")
        );
        if outcome == "eof" {
            host.reaped(&agent, "peer.pid");
            host.until("EOF workers joined before fallback", || {
                (host.inspect(&agent)["resource"]["cleanup_complete"] == true).then_some(())
            });
        } else {
            let next = host.prompt(&agent, "new-prompt-after-interrupt-outcome", "add 10 5");
            host.barrier(&agent, "s03-next");
            let mut stale = agent.action("acp-interrupt", "old-prompt-must-not-cancel-next");
            stale["prompt_id"] = original["prompt_id"].clone();
            host.deny(stale, &["stale-interaction"]);
            assert_eq!(
                host.inspect(&agent)["foreground"]["prompt_id"],
                host.work(&agent, &next["work_id"])["prompt_id"]
            );
            agent.release("s03-next");
            let completed = host.completed(&agent, &next["work_id"]);
            assert_ne!(completed["prompt_id"], original["prompt_id"]);
            assert_ne!(completed["rpc_id"], original["rpc_id"]);
            assert_eq!(completed["outcome"]["stop_reason"], "end_turn");
            assert_eq!(agent.effect()["value"], 57);
            assert_eq!(agent.effect()["commands"], 2);
            assert!(!agent.root.join("unexpected-cancel").exists());
            host.close(&agent);
        }
        sentinel.alive();
    }
}

#[test]
fn e04s03_rendering_gap_resync_retains_tuple_pending_rpc_and_content_privacy() {
    let host = Host::new();
    let mut agent = host.open("s03-rendering");
    host.ready(&mut agent);
    let tuple = agent.target.clone();
    let prompt = host.prompt(&agent, "render-gap-original", "add 17 25");
    host.barrier(&agent, "s03-updates");
    let original = host.work(&agent, &prompt["work_id"]);
    let mut cursor = 0;
    let mut kinds = std::collections::BTreeSet::new();
    let mut tail = false;
    let mut hostile = false;
    let mut previous = None;
    loop {
        let page = host.call(json!({"op":"acp-inspect","target":tuple,
            "include_content":true,"after":cursor,"limit":128}));
        assert_eq!(page["target"], tuple);
        if cursor == 0 {
            assert_eq!(page["content"]["gap"], true);
            assert!(page["truncation"]["through"].as_u64().unwrap() > 0);
        }
        for event in page["content"]["events"].as_array().unwrap() {
            let seq = event["seq"].as_u64().unwrap();
            assert!(seq > cursor);
            if let Some(previous) = previous {
                assert!(seq > previous);
            }
            previous = Some(seq);
            kinds.insert(event["kind"].as_str().unwrap().to_owned());
            tail |= event["text"] == "s03-tail-retained-2112";
            hostile |= event["text"]
                .as_str()
                .is_some_and(|text| text.contains("s03-hostile:"));
            if matches!(
                event["kind"].as_str(),
                Some("agent-message" | "thought" | "tool" | "plan")
            ) {
                assert!(
                    event.get("prompt_id").is_none(),
                    "no arrival-guessed prompt association"
                );
            }
        }
        let through = page["content"]["through"].as_u64().unwrap();
        if through == page["cursor"].as_u64().unwrap() {
            break;
        }
        assert!(through > cursor);
        cursor = through;
    }
    assert!(tail && hostile);
    for kind in ["agent-message", "thought", "tool", "plan"] {
        assert!(kinds.contains(kind));
    }
    assert_eq!(
        host.inspect(&agent)["foreground"]["prompt_id"],
        original["prompt_id"]
    );
    assert!(original["outcome"].is_null());
    let metadata = [
        host.call(json!({"op":"snapshot"})),
        host.call(json!({"op":"events","after":0,"timeout_ms":0})),
        host.call(json!({"op":"acp-inspect","target":tuple})),
    ];
    for projection in metadata {
        let text = projection.to_string();
        for private in [
            "s03-seq-",
            "s03-tail-retained",
            "s03-hostile",
            "s03-title-owned",
            "s03-thought-retained",
            "s03-tool-retained",
            "s03-plan-retained",
            "s03-bidi",
        ] {
            assert!(
                !text.contains(private),
                "content marker leaked into metadata: {private}"
            );
        }
    }
    agent.release("s03-updates");
    let completed = host.completed(&agent, &prompt["work_id"]);
    s03_same_work(&completed, &original);
    assert_eq!(completed["outcome"]["stop_reason"], "end_turn");
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(agent.effect()["session_id"], tuple["session_id"]);
    host.close(&agent);
}

fn s03_native_owner(host: &Host, agent: &mut Agent) -> (OriginalNative, Value) {
    let mut native = OriginalNative::new(host);
    let registration = native.call(json!({"op":"controller-create","role":"attachment",
        "label":"S03 original attachment connection"}));
    native.call(
        json!({"op":"control-handle","handle":registration["controller"],"method":"view",
        "view":{"serial":1,"target":{"kind":"acp","id":agent.target["id"]},"focused":true}}),
    );
    let ownership = s03_ownership(host, agent);
    agent.opened = native.call(json!({"op":"ownership-claim",
        "target":{"kind":"acp","id":agent.target["id"]},"controller":registration["controller"],
        "mode":"force","observed":ownership["observed"]}));
    agent.controller_id = registration["controller_id"].clone();
    (native, registration)
}

fn s03_view(
    native: &mut OriginalNative,
    registration: &Value,
    agent: &Agent,
    serial: u64,
    focused: bool,
) {
    native.call(json!({"op":"control-handle","handle":registration["controller"],"method":"view",
        "view":{"serial":serial,"target":{"kind":"acp","id":agent.target["id"]},"focused":focused}}));
}

fn s03_consent(host: &Host, agent: &Agent, claim: &Value, serial: u64) -> Value {
    json!({"op":"control-handle","handle":agent.opened["control"],"method":"consent-to",
        "claim_id":claim["claim_id"],"view_serial":serial,
        "activity_revision":s03_ownership(host, agent)["activity_revision"]})
}

#[test]
fn e04s03_original_attachment_warning_keep_withdraw_preserve_control_without_native_batch() {
    let host = Host::new();
    let mut agent = host.open("s02-form");
    host.ready(&mut agent);
    let (mut native, registration) = s03_native_owner(&host, &mut agent);
    let control = agent.opened["control"].clone();
    let generation = agent.opened["generation"].clone();
    let mut request = agent.action("acp-prompt", "original-attachment-active-question");
    request["text"] = json!("add 17 25");
    let prompt = native.call(request);
    let form = pending(&host, &agent, "form");
    let activity = s03_ownership(&host, &agent)["activity_revision"]
        .as_u64()
        .unwrap();
    let b = s03_controller(&host, "external idle policy");
    host.deny(
        json!({"op":"control-handle","handle":b["controller"],"method":"view",
        "view":{"serial":1,"target":{"kind":"acp","id":agent.target["id"]},"focused":false}}),
        &["not-native-connection"],
    );
    host.deny(
        json!({"op":"control-handle","handle":registration["controller"],"method":"view",
        "view":{"serial":2,"target":{"kind":"acp","id":agent.target["id"]},"focused":false}}),
        &["not-native-connection"],
    );
    for finish in ["keep", "withdraw"] {
        let when = json!({"kind":"human-idle","idle_ms":0,"warning_ms":1700});
        let claim = s03_claim(&host, &agent, &b, "cooperative", when.clone());
        let facts = s03_ownership(&host, &agent)["reservation"]["conditional"].clone();
        assert_eq!(facts["when"], when);
        assert_eq!(facts["ready_for_consent"], true, "{facts}");
        let consent = s03_consent(&host, &agent, &claim, 1);
        host.deny(consent.clone(), &["not-native-connection"]);
        host.deny(
            json!({"op":"control-handle","handle":control,"method":"keep",
            "claim_id":claim["claim_id"]}),
            &["not-native-connection"],
        );
        for (field, invalid, code) in [
            ("view_serial", json!(0), "stale-view"),
            (
                "activity_revision",
                json!(
                    s03_ownership(&host, &agent)["activity_revision"]
                        .as_u64()
                        .unwrap()
                        + 1
                ),
                "stale-observation",
            ),
        ] {
            let mut stale = consent.clone();
            stale[field] = invalid;
            let response = native.exchange(stale);
            assert_eq!(response["ok"], false, "{response}");
            assert_eq!(response["error"]["code"], code);
        }
        native.call(consent);
        let warning = s03_ownership(&host, &agent)["reservation"]["conditional"].clone();
        assert_eq!(warning["consent"], "recorded");
        assert_eq!(warning["clock"]["kind"], "warning");
        assert!(warning["clock"]["remaining_ms"].as_u64().unwrap() <= 1700);
        if finish == "keep" {
            native.call(
                json!({"op":"control-handle","handle":control,"method":"keep",
                "claim_id":claim["claim_id"]}),
            );
        } else {
            s03_handle(&host, &claim["claim"], "withdraw");
        }
        let state = s03_handle(&host, &claim["claim"], "read");
        assert_eq!(
            state["phase"],
            if finish == "keep" {
                "aborted-by-owner"
            } else {
                "withdrawn"
            }
        );
        let current = native.call(json!({"op":"control-handle","handle":control,"method":"read"}));
        assert_eq!(current["control"], control);
        assert_eq!(current["ownership"]["observed"]["generation"], generation);
        let ownership = s03_ownership(&host, &agent);
        assert!(
            ownership["reservation"].is_null(),
            "external policy must explicitly rearm"
        );
        assert_eq!(ownership["settlement_boundary"], "input-admission");
        assert_eq!(
            pending(&host, &agent, "form")["interaction_id"],
            form["interaction_id"]
        );
        assert_eq!(
            host.inspect(&agent)["foreground"]["prompt_id"],
            host.work(&agent, &prompt["work_id"])["prompt_id"]
        );
        assert!(!agent.root.join("effect.json").exists());
    }
    assert!(
        s03_ownership(&host, &agent)["activity_revision"]
            .as_u64()
            .unwrap()
            > activity
    );
    native.call(form_answer(
        &agent,
        &form,
        "original-human-fresh-answer",
        "accept",
        Some(json!({"bonus":15})),
    ));
    host.completed(&agent, &prompt["work_id"]);
    assert_eq!(agent.effect()["value"], 57);
    assert!(!agent.root.join("unexpected-cancel").exists());
    host.close(&agent);
}

#[test]
fn e04s03_acp_background_offer_revalidates_focus_activity_and_exact_original_connection() {
    let host = Host::new();
    let mut agent = host.open("s02-form");
    host.ready(&mut agent);
    let (mut native, registration) = s03_native_owner(&host, &mut agent);
    let before_borrowed = s03_ownership(&host, &agent)["activity_revision"].clone();
    let prompt = host.prompt(&agent, "background-question-still-active", "add 17 25");
    assert_eq!(
        s03_ownership(&host, &agent)["activity_revision"],
        before_borrowed
    );
    let form = pending(&host, &agent, "form");
    let b = s03_controller(&host, "background policy requester");
    let claim = s03_claim(
        &host,
        &agent,
        &b,
        "cooperative",
        json!({"kind":"views-backgrounded","scope":"owner"}),
    );
    assert_eq!(
        s03_ownership(&host, &agent)["reservation"]["conditional"]["eligibility"],
        "ineligible"
    );
    s03_view(&mut native, &registration, &agent, 2, false);
    assert_eq!(
        s03_ownership(&host, &agent)["reservation"]["conditional"]["ready_for_consent"],
        true
    );
    native.call(s03_consent(&host, &agent, &claim, 2));
    let first = s03_handle(&host, &claim["claim"], "read");
    assert_eq!(first["phase"], "offered");
    // An offer is not a grant. A real changed focus invalidates final acceptance.
    assert_eq!(
        native.call(
            json!({"op":"control-handle","handle":agent.opened["control"],
        "method":"read"})
        )["control"],
        agent.opened["control"]
    );
    s03_view(&mut native, &registration, &agent, 3, true);
    host.deny(
        json!({"op":"control-handle","handle":first["offer"],"method":"accept"}),
        &["stale-offer"],
    );
    assert_eq!(
        s03_ownership(&host, &agent)["reservation"]["conditional"]["eligibility"],
        "ineligible"
    );
    let stale_view = native.exchange(json!({"op":"control-handle","handle":registration["controller"],
        "method":"view","view":{"serial":2,"target":{"kind":"acp","id":agent.target["id"]},"focused":false}}));
    assert_eq!(stale_view["ok"], false);
    assert_eq!(stale_view["error"]["code"], "stale-view");
    s03_view(&mut native, &registration, &agent, 4, false);
    native.call(s03_consent(&host, &agent, &claim, 4));
    let second = s03_handle(&host, &claim["claim"], "read");
    assert_eq!(second["phase"], "offered");
    assert_ne!(first["offer"], second["offer"]);
    let revision = s03_ownership(&host, &agent)["activity_revision"]
        .as_u64()
        .unwrap();
    host.deny(
        json!({"op":"control-handle","handle":agent.opened["control"],"method":"activity"}),
        &["not-native-connection"],
    );
    assert_eq!(s03_ownership(&host, &agent)["activity_revision"], revision);
    native
        .call(json!({"op":"control-handle","handle":agent.opened["control"],"method":"activity"}));
    host.deny(
        json!({"op":"control-handle","handle":second["offer"],"method":"accept"}),
        &["stale-offer", "claim-completed"],
    );
    assert_eq!(
        s03_handle(&host, &claim["claim"], "read")["phase"],
        "aborted-by-owner"
    );
    assert!(s03_ownership(&host, &agent)["reservation"].is_null());
    // Activity preserves the human grant; only external policy creates a new
    // claim. Never silently rearm or rebind the aborted claim's consent.
    let rearmed = s03_claim(
        &host,
        &agent,
        &b,
        "cooperative",
        json!({"kind":"views-backgrounded","scope":"owner"}),
    );
    native.call(s03_consent(&host, &agent, &rearmed, 4));
    let fresh = s03_handle(&host, &rearmed["claim"], "read");
    assert_eq!(fresh["phase"], "offered");
    assert_ne!(fresh["offer"], second["offer"]);
    let ownership = s03_ownership(&host, &agent);
    assert_eq!(ownership["settlement_boundary"], "input-admission");
    let original = host.work(&agent, &prompt["work_id"]);
    assert_eq!(
        host.inspect(&agent)["foreground"]["prompt_id"],
        original["prompt_id"]
    );
    agent.opened = s03_handle(&host, &fresh["offer"], "accept");
    host.call(form_answer(
        &agent,
        &form,
        "background-successor-fresh-answer",
        "accept",
        Some(json!({"bonus":15})),
    ));
    s03_same_work(&host.completed(&agent, &prompt["work_id"]), &original);
    assert_eq!(agent.effect()["value"], 57);
    assert!(!agent.root.join("unexpected-cancel").exists());
    host.close(&agent);
}

//! Real daemon/agent processes, durable native-format peer state, no providers.
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(15);

struct Host {
    root: PathBuf,
    socket: PathBuf,
    daemon: Option<Child>,
    projects: [String; 2],
}

impl Host {
    fn new() -> Self {
        Self::with_mode("recovery")
    }

    fn with_mode(second_mode: &str) -> Self {
        let root = fs::canonicalize("/tmp")
            .unwrap()
            .join(format!("amux-reopen-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        for name in [
            "home",
            "config/amux",
            "runtime",
            "startup",
            "projects",
            "first",
            "second",
            "peer",
        ] {
            fs::create_dir_all(root.join(name)).unwrap();
            fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::write(root.join("startup/.zshrc"), "PS1='RECOVERY_FIXTURE> '\n").unwrap();
        let projects = [
            uuid::Uuid::new_v4().to_string(),
            uuid::Uuid::new_v4().to_string(),
        ];
        let peer = Path::new(env!("CARGO_BIN_EXE_amux"))
            .parent()
            .unwrap()
            .join("examples/acp_fixture");
        let config = json!({"version":1,"config":{"projects_dir":root.join("projects"),"prefix":"Ctrl-a",
        "acp_profiles":{
            "math":{"label":"Arithmetic","command":[peer,root.join("peer"),"recovery"]},
            "other":{"label":"Other arithmetic","command":[peer,root.join("peer"),second_mode]}
        }},"projects":[
            {"id":projects[0],"name":"First project","context":root.join("first")},
            {"id":projects[1],"name":"Second project","context":root.join("second")}
        ]});
        fs::write(
            root.join("config/amux/config.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        let mut host = Self {
            socket: root.join("control.sock"),
            root,
            daemon: None,
            projects,
        };
        host.start();
        host
    }

    fn start(&mut self) {
        self.start_with_path("/usr/bin:/bin:/usr/sbin:/sbin");
    }

    fn start_with_path(&mut self, path: &str) {
        let cwd = self.root.clone();
        self.start_with_path_at(path, &cwd);
    }

    fn start_with_path_at(&mut self, path: &str, cwd: &Path) {
        assert!(self.daemon.is_none());
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(self.root.join("daemon.log"))
            .unwrap();
        self.daemon = Some(
            Command::new(env!("CARGO_BIN_EXE_amux"))
                .args(["--socket"])
                .arg(&self.socket)
                .arg("daemon")
                .env_clear()
                .env("PATH", path)
                .env("HOME", self.root.join("home"))
                .env("SHELL", "/bin/zsh")
                .env("TERM", "xterm-256color")
                .env("ZDOTDIR", self.root.join("startup"))
                .env("XDG_CONFIG_HOME", self.root.join("config"))
                .env("XDG_RUNTIME_DIR", self.root.join("runtime"))
                .env("AMUX_RUNTIME_DIR", self.root.join("runtime"))
                .env(
                    "RECOVERY_PRIVATE_CANARY",
                    "environment-value-must-not-persist",
                )
                .current_dir(cwd)
                .stdin(Stdio::null())
                .stdout(log.try_clone().unwrap())
                .stderr(log)
                .spawn()
                .unwrap(),
        );
        self.until("daemon ready", || {
            self.request(json!({"op":"snapshot"}))
                .ok()
                .filter(|value| value["ok"] == true)
        });
    }

    fn request(&self, mut request: Value) -> std::io::Result<Value> {
        request["version"] = json!(1);
        let mut stream = UnixStream::connect(&self.socket)?;
        stream.set_read_timeout(Some(DEADLINE))?;
        stream.set_write_timeout(Some(DEADLINE))?;
        serde_json::to_writer(&mut stream, &request)?;
        stream.write_all(b"\n")?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line)?;
        Ok(serde_json::from_str(&line)?)
    }

    fn call(&self, request: Value) -> Value {
        let response = self.request(request).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        response["result"].clone()
    }

    fn until<T>(&self, what: &str, mut check: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + DEADLINE;
        loop {
            if let Some(value) = check() {
                return value;
            }
            assert!(
                Instant::now() < deadline,
                "{what}; {}",
                fs::read_to_string(self.root.join("daemon.log")).unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    fn snapshot(&self) -> Value {
        self.call(json!({"op":"snapshot"}))
    }

    fn ready(&self, name: &str) -> Value {
        self.until("exact recovered conversation ready", || {
            self.snapshot()["agent_views"]
                .as_array()?
                .iter()
                .find(|view| view["name"] == name && view["lifecycle"] == "ready")
                .cloned()
        })
    }

    fn open(&self, index: usize, name: &str) -> Value {
        let controller = self.call(
            json!({"op":"controller-create","role":"external","label":"recovery acceptance"}),
        );
        self.call(json!({"op":"acp-open","agent":if index == 0 {"math"} else {"other"},
            "project":self.projects[index],"cwd":self.root.join(if index == 0 {"first"} else {"second"}),
            "controller":controller["controller"],"name":name}))
    }

    fn target(view: &Value) -> Value {
        json!({"kind":"acp","id":view["id"],"instance_id":view["instance_id"],
            "connection_generation":view["connection_generation"],"session_id":view["session_id"]})
    }

    fn claim(&self, view: &Value) -> Value {
        let controller = self.call(
            json!({"op":"controller-create","role":"external","label":"deliberate after reopen"}),
        );
        self.call(
            json!({"op":"ownership-claim","controller":controller["controller"],
            "target":{"kind":"acp","id":view["id"]},"observed":view["ownership"]["observed"],
            "mode":"cooperative","when":{"kind":"now"}}),
        )
    }

    fn prompt(&self, view: &Value, grant: &Value, text: &str, expected: i64) {
        let admitted = self.call(json!({"op":"acp-prompt","target":Self::target(view),
            "control":grant["control"],"owner_generation":grant["generation"],
            "call_id":uuid::Uuid::new_v4().to_string(),"text":text}));
        self.until("deliberate arithmetic completed", || {
            let state = self.call(json!({"op":"acp-inspect","target":Self::target(view)}));
            let work = state["work"]
                .as_array()?
                .iter()
                .find(|work| work["work_id"] == admitted["work_id"])?;
            (work["state"] == "completed" && self.saved(view)["value"] == expected).then_some(())
        });
    }

    fn saved_path(&self, view: &Value) -> PathBuf {
        self.root
            .join("peer/recovery.sessions")
            .join(format!("{}.json", view["session_id"].as_str().unwrap()))
    }

    fn saved(&self, view: &Value) -> Value {
        serde_json::from_slice(&fs::read(self.saved_path(view)).unwrap()).unwrap()
    }

    fn journal_path(&self) -> PathBuf {
        self.until("durable conversation journal", || {
            fs::read_dir(self.root.join("config/amux"))
                .ok()?
                .flatten()
                .find(|entry| {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    name.starts_with("agent-sessions-") && name.ends_with(".json")
                })
                .map(|entry| entry.path())
        })
    }

    fn journal(&self) -> Value {
        serde_json::from_slice(&fs::read(self.journal_path()).unwrap()).unwrap()
    }

    fn wait_records(&self, count: usize) {
        self.until("expected durable conversations", || {
            let journal = self.journal();
            (journal["records"].as_array()?.len() == count).then_some(())
        });
    }

    fn requests(&self) -> Vec<Value> {
        fs::read_to_string(self.root.join("peer/recovery.requests.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn seed(&self) -> [(Value, Value); 2] {
        let a = self.open(0, "Alpha");
        let av = self.ready("Alpha");
        self.prompt(&av, &a, "add 17 25", 42);
        let b = self.open(1, "Beta");
        let bv = self.ready("Beta");
        self.prompt(&bv, &b, "add 8 9", 17);
        self.wait_records(2);
        [(av, a), (bv, b)]
    }

    fn stop(&mut self) {
        if self.daemon.is_none() {
            return;
        }
        self.call(json!({"op":"shutdown"}));
        let mut child = self.daemon.take().unwrap();
        let deadline = Instant::now() + DEADLINE;
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "daemon shutdown {status}");
                break;
            }
            assert!(Instant::now() < deadline, "daemon did not stop");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!self.socket.exists(), "shutdown left socket");
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        if let Some(mut daemon) = self.daemon.take() {
            let _ = self.request(json!({"op":"shutdown"}));
            let deadline = Instant::now() + Duration::from_secs(5);
            while daemon.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if daemon.try_wait().ok().flatten().is_none() {
                let _ = daemon.kill();
            }
            let _ = daemon.wait();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn relaunch_reopens_all_exact_conversations_idle_with_fresh_authority_and_no_private_input() {
    let mut host = Host::new();
    let [(old_a, old_grant), (old_b, _)] = host.seed();
    host.call(json!({"op":"name","terminal":old_a["id"],"name":"Alpha renamed"}));
    let a_saved = host.saved(&old_a);
    let b_saved = host.saved(&old_b);
    let journal_text = fs::read_to_string(host.journal_path()).unwrap();
    for private in [
        "environment-value-must-not-persist",
        "add 17 25",
        old_grant["control"]["ref"].as_str().unwrap(),
    ] {
        assert!(
            !journal_text.contains(private),
            "private runtime input leaked into metadata"
        );
    }
    host.stop();
    let request_start = host.requests().len();
    host.start();
    let a = host.ready("Alpha renamed");
    let b = host.ready("Beta");
    for (old, restored) in [(&old_a, &a), (&old_b, &b)] {
        assert_eq!(restored["session_id"], old["session_id"]);
        assert_eq!(restored["project_id"], old["project_id"]);
        assert_ne!(restored["id"], old["id"]);
        assert_ne!(restored["instance_id"], old["instance_id"]);
        assert!(
            restored["ownership"]["owner"].is_null(),
            "reopen resurrected an owner"
        );
    }
    assert_eq!(
        host.saved(&a),
        a_saved,
        "reopen modified first saved conversation"
    );
    assert_eq!(
        host.saved(&b),
        b_saved,
        "reopen modified second saved conversation"
    );
    let requests = host.requests();
    let resumed: Vec<_> = requests[request_start..]
        .iter()
        .filter(|request| request["method"] == "session/load")
        .collect();
    assert_eq!(resumed.len(), 2);
    assert!(requests[request_start..].iter().all(|request| matches!(
        request["method"].as_str(),
        Some("initialize" | "session/load")
    )));
    let stale = host
        .request(json!({"op":"acp-prompt","target":Host::target(&old_a),
        "control":old_grant["control"],"owner_generation":old_grant["generation"],
        "call_id":"old-lifetime-must-not-replay","text":"add 100 100"}))
        .unwrap();
    assert_eq!(stale["ok"], false);
    assert_eq!(host.saved(&a)["value"], 42);
    let fresh = host.claim(&a);
    host.prompt(&a, &fresh, "add 10 5", 57);
    assert_eq!(host.saved(&a)["commands"], 2);
    assert_eq!(host.saved(&b)["value"], 17);
    host.stop();
}

#[test]
fn explicit_session_close_is_forgotten_while_daemon_shutdown_preserves_other_open_session() {
    let mut host = Host::new();
    let [(a, grant), (b, _)] = host.seed();
    host.call(
        json!({"op":"acp-close","target":Host::target(&a),"control":grant["control"],
        "owner_generation":grant["generation"],"call_id":"deliberate-close"}),
    );
    host.wait_records(1);
    host.stop();
    let request_start = host.requests().len();
    host.start();
    let restored = host.ready("Beta");
    assert_eq!(restored["session_id"], b["session_id"]);
    assert!(
        !host.snapshot()["agent_views"]
            .as_array()
            .unwrap()
            .iter()
            .any(|view| view["name"] == "Alpha")
    );
    assert!(
        host.requests()[request_start..]
            .iter()
            .filter(|request| request["method"] == "session/load")
            .all(|request| request["session_id"] == b["session_id"])
    );
    host.stop();
}

#[test]
fn missing_saved_session_is_reported_without_new_conversation_and_other_session_recovers() {
    let mut host = Host::new();
    let [(a, _), (b, _)] = host.seed();
    host.stop();
    let saved = fs::read(host.saved_path(&b)).unwrap();
    fs::remove_file(host.saved_path(&b)).unwrap();
    let request_start = host.requests().len();
    host.start();
    assert_eq!(host.ready("Alpha")["session_id"], a["session_id"]);
    host.until("missing conversation diagnostic", || {
        host.snapshot()["diagnostics"]
            .as_array()?
            .iter()
            .any(|diagnostic| {
                diagnostic["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("session-reopen-failed"))
            })
            .then_some(())
    });
    assert_eq!(host.journal()["records"].as_array().unwrap().len(), 2);
    assert!(
        host.requests()[request_start..]
            .iter()
            .all(|request| request["method"] != "session/new"
                && request["method"] != "session/prompt")
    );
    host.stop();
    fs::write(host.saved_path(&b), saved).unwrap();
    host.start();
    assert_eq!(
        host.ready("Beta")["session_id"],
        b["session_id"],
        "failed intent was discarded"
    );
    assert_eq!(host.saved(&b)["value"], 17);
    host.stop();
}

#[test]
fn unsupported_reopen_capability_never_falls_back_to_new_or_automatic_work() {
    let mut host = Host::with_mode("recovery-unsupported");
    let [(a, _), (b, _)] = host.seed();
    host.stop();
    let request_start = host.requests().len();
    host.start();
    assert_eq!(host.ready("Alpha")["session_id"], a["session_id"]);
    host.until("unsupported reopen diagnostic", || {
        host.snapshot()["diagnostics"]
            .as_array()?
            .iter()
            .any(|diagnostic| {
                diagnostic["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("session-reopen-unsupported"))
            })
            .then_some(())
    });
    assert_eq!(host.saved(&b)["value"], 17);
    assert_eq!(host.journal()["records"].as_array().unwrap().len(), 2);
    assert!(
        host.requests()[request_start..]
            .iter()
            .all(|request| request["method"] != "session/new"
                && request["method"] != "session/prompt")
    );
    host.stop();
}

#[test]
fn corrupt_journal_is_visible_not_overwritten_and_does_not_prevent_a_fresh_shell() {
    let mut host = Host::new();
    host.seed();
    host.stop();
    let path = host.journal_path();
    fs::write(&path, b"not a valid recovery journal").unwrap();
    host.start();
    let snapshot = host.snapshot();
    assert!(snapshot["agent_views"].as_array().unwrap().is_empty());
    assert!(
        snapshot["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|terminal| terminal["alive"] == true)
    );
    assert!(
        snapshot["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| {
                diagnostic["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("conversation recovery disabled"))
            })
    );
    assert_eq!(fs::read(path).unwrap(), b"not a valid recovery journal");
    host.stop();
}

#[test]
fn unverified_previous_peer_is_not_reopened_or_replaced() {
    let mut host = Host::new();
    let [(a, _), (b, _)] = host.seed();
    host.stop();
    let path = host.journal_path();
    let mut journal = host.journal();
    for record in journal["records"].as_array_mut().unwrap() {
        if record["session_id"] == b["session_id"] {
            record["peer"] = Value::Null;
        }
    }
    fs::write(path, serde_json::to_vec(&journal).unwrap()).unwrap();
    let request_start = host.requests().len();
    host.start();
    assert_eq!(host.ready("Alpha")["session_id"], a["session_id"]);
    assert!(
        !host.snapshot()["agent_views"]
            .as_array()
            .unwrap()
            .iter()
            .any(|view| view["name"] == "Beta")
    );
    assert!(
        host.snapshot()["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| {
                diagnostic["message"].as_str().is_some_and(|message| {
                    message.contains("previous agent process identity was not verified")
                })
            })
    );
    assert!(
        host.requests()[request_start..]
            .iter()
            .filter(|request| request["method"] == "session/load")
            .all(|request| request["session_id"] == a["session_id"])
    );
    host.stop();
}

#[test]
fn hard_daemon_crash_reopens_durable_checkpoints_without_restarting_work() {
    let mut host = Host::new();
    let [(a, _), (b, _)] = host.seed();
    let before_a = host.saved(&a);
    let before_b = host.saved(&b);
    let mut owned: Vec<_> = host.journal()["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| record["peer"]["pid"].as_i64().unwrap() as i32)
        .collect();
    owned.extend(
        host.snapshot()["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .map(|terminal| terminal["pid"].as_i64().unwrap() as i32),
    );
    let mut daemon = host.daemon.take().unwrap();
    daemon.kill().unwrap(); // Only this fixture's owned process, not a global daemon.
    assert!(!daemon.wait().unwrap().success());
    host.until(
        "old private agent and PTY processes exited after daemon crash",
        || {
            owned
                .iter()
                .all(|pid| {
                    (unsafe { libc::kill(*pid, 0) }) == -1
                        && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
                })
                .then_some(())
        },
    );
    let request_start = host.requests().len();
    host.start(); // Handles the stale socket and reacquires the released journal lock.
    assert_eq!(host.ready("Alpha")["session_id"], a["session_id"]);
    assert_eq!(host.ready("Beta")["session_id"], b["session_id"]);
    assert_eq!(host.saved(&a), before_a);
    assert_eq!(host.saved(&b), before_b);
    assert!(host.requests()[request_start..].iter().all(|request| {
        matches!(
            request["method"].as_str(),
            Some("initialize" | "session/load")
        )
    }));
    host.stop();
}

#[test]
fn changed_profile_command_cannot_receive_an_old_opaque_conversation_identity() {
    let mut host = Host::new();
    let [(a, _), (b, _)] = host.seed();
    host.stop();
    let config_path = host.root.join("config/amux/config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["config"]["acp_profiles"]["other"]["command"][2] = json!("recovery-resume-only");
    fs::write(config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let request_start = host.requests().len();
    host.start();
    assert_eq!(host.ready("Alpha")["session_id"], a["session_id"]);
    host.until("changed launch namespace is explicit", || {
        host.snapshot()["diagnostics"]
            .as_array()?
            .iter()
            .any(|diagnostic| {
                diagnostic["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("profile launch command changed"))
            })
            .then_some(())
    });
    assert!(host.requests()[request_start..].iter().all(|request| {
        request["session_id"] != b["session_id"] && request["method"] != "session/new"
    }));
    assert_eq!(host.saved(&b)["value"], 17);
    assert_eq!(host.journal()["records"].as_array().unwrap().len(), 2);
    host.stop();
}

#[test]
fn crash_during_reopening_does_not_treat_the_old_gone_peer_as_a_new_launch_grant() {
    let mut host = Host::new();
    let [(a, _), (b, _)] = host.seed();
    host.stop();
    let path = host.journal_path();
    let mut journal = host.journal();
    for record in journal["records"].as_array_mut().unwrap() {
        if record["session_id"] == b["session_id"] {
            record["launch_pending"] = json!(true);
        }
    }
    fs::write(path, serde_json::to_vec(&journal).unwrap()).unwrap();
    let request_start = host.requests().len();
    host.start();
    assert_eq!(host.ready("Alpha")["session_id"], a["session_id"]);
    host.until("uncertain reopening settlement diagnostic", || {
        host.snapshot()["diagnostics"]
            .as_array()?
            .iter()
            .any(|diagnostic| {
                diagnostic["message"].as_str().is_some_and(|message| {
                    message.contains("previous reopening process settlement is unknown")
                })
            })
            .then_some(())
    });
    assert!(host.requests()[request_start..].iter().all(|request| {
        request["session_id"] != b["session_id"] && request["method"] != "session/new"
    }));
    assert_eq!(host.saved(&b)["value"], 17);
    assert!(
        host.journal()["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["session_id"] == b["session_id"] && record["launch_pending"] == true
            })
    );
    host.stop();
}

fn replace_with_native(host: &Host, original: &Value, agent: Value, session: &str, file: &Path) {
    assert!(
        host.daemon.is_none(),
        "fixture rewrites only a stopped private journal"
    );
    let mut journal = host.journal();
    let record = journal["records"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|record| record["session_id"] == original["session_id"])
        .unwrap();
    record["id"] = json!("0-native");
    record["agent"] = agent;
    record["session_id"] = json!(session);
    record["session_file"] = json!(file);
    record["launch_hash"] = Value::Null;
    record["settings"] = json!({});
    record["launch_pending"] = json!(false);
    record["blocked"] = Value::Null;
    fs::write(host.journal_path(), serde_json::to_vec(&journal).unwrap()).unwrap();
}

fn native_header(host: &Host, session: &str, leaf: &str) -> PathBuf {
    let path = host.root.join(leaf);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    writeln!(
        file,
        "{}",
        json!({"type":"session","version":3,"id":session,"cwd":host.root.join("first")})
    )
    .unwrap();
    path
}

fn native_report(
    host: &Host,
    terminal: &str,
    lifetime: &str,
    session: &str,
    file: &Path,
    persisted: bool,
    pid: u32,
) {
    host.call(json!({"op":"report","terminal":terminal,"event":{
        "kind":"agent","instance_id":lifetime,"agent_id":"main","agent_kind":"main","parent_id":null,
        "activity":"idle","settled":true,"pid":pid,"agent_family":"omp",
        "conversation_id":session,"session_file":file,"cwd":host.root.join("first"),
        "session_persisted":persisted,"lifecycle_event":"session_start"
    }}));
}

struct NativePeer(Child);
impl NativePeer {
    fn new() -> Self {
        Self(Command::new("/bin/sleep").arg("600").spawn().unwrap())
    }
    fn stop(&mut self) {
        if self.0.try_wait().unwrap().is_none() {
            self.0.kill().unwrap();
        }
        self.0.wait().unwrap();
    }
}
impl Drop for NativePeer {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn private_omp(host: &Host, marker: &Path) -> String {
    let bin = host.root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let executable = bin.join("omp");
    fs::write(
        &executable,
        format!(
            "#!/bin/sh\nprintf 'attempted\\n' >> '{}'\nexit 77\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(executable, fs::Permissions::from_mode(0o700)).unwrap();
    let path = format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", bin.display());
    fs::write(
        host.root.join("startup/.zshrc"),
        format!("export PATH='{path}'\nPS1='RECOVERY_FIXTURE> '\n"),
    )
    .unwrap();
    path
}

fn native_preparation_gate(host: &Host) -> (String, PathBuf, PathBuf) {
    let bin = host.root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let gate = host.root.join("pause-native-version");
    let entered = host.root.join("native-version-entered");
    fs::write(&gate, b"paused").unwrap();
    let executable = bin.join("codex");
    fs::write(
        &executable,
        format!(
            "#!/bin/sh\nset -eu\n[ \"$1\" = --version ]\nprintf '%s\\n' \"$$\" > '{}'\nprintf 'entered\\n' >> '{}'\nwhile [ -f '{}' ]; do /bin/sleep 0.02; done\nexit 78\n",
            host.root.join("native-version-pid").display(),
            entered.display(),
            gate.display()
        ),
    )
    .unwrap();
    fs::set_permissions(executable, fs::Permissions::from_mode(0o700)).unwrap();
    (
        format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", bin.display()),
        gate,
        entered,
    )
}

#[test]
fn concurrent_project_metadata_update_preserves_unattempted_exact_conversations() {
    let mut host = Host::new();
    let controller = host.call(
        json!({"op":"controller-create","role":"external","label":"shared unsaved startup group"}),
    );
    let grant = host.call(json!({"op":"acp-open","agent":"math","new_project":true,
        "project":host.projects[0],"cwd":host.root.join("first"),"name":"Alpha",
        "controller":controller["controller"]}));
    let native = host.ready("Alpha");
    host.prompt(&native, &grant, "add 17 25", 42);
    let project = native["project_id"].as_str().unwrap().to_owned();
    host.projects = [project.clone(), project];
    let grant = host.open(1, "Beta");
    let old_b = host.ready("Beta");
    host.prompt(&old_b, &grant, "add 8 9", 17);
    let grant = host.open(1, "Gamma");
    let old_c = host.ready("Gamma");
    host.prompt(&old_c, &grant, "add 1 9", 10);
    host.wait_records(3);
    let histories = [host.saved(&old_b), host.saved(&old_c)];
    let effect = fs::read(host.root.join("peer/effect.json")).unwrap();
    host.stop();
    let session = uuid::Uuid::new_v4().to_string();
    let file = native_header(&host, &session, "native-preparation.jsonl");
    replace_with_native(
        &host,
        &native,
        json!({"kind":"codex","model":"fixture-model"}),
        &session,
        &file,
    );
    let (path, gate, entered) = native_preparation_gate(&host);
    let request_start = host.requests().len();
    host.start_with_path(&path);
    host.until("native preparation holds the startup snapshot", || {
        entered.exists().then_some(())
    });
    let waiting = host.snapshot();
    assert_eq!(waiting["startup_pending"], true);
    assert!(waiting["agent_views"].as_array().unwrap().is_empty());
    let changed = host.call(
        json!({"op":"project-config","project":host.projects[0],"config":{
        "name":"Changed during startup","context":host.root.join("second")}}),
    );
    assert_eq!(changed["name"], "Changed during startup");
    fs::remove_file(gate).unwrap();
    let b = host.ready("Beta");
    let c = host.ready("Gamma");
    host.until("startup snapshot is fully settled", || {
        (host.snapshot()["startup_pending"] == false).then_some(())
    });
    for ((old, restored), history) in [(&old_b, &b), (&old_c, &c)].into_iter().zip(histories) {
        assert_eq!(restored["session_id"], old["session_id"]);
        assert_eq!(restored["project_id"], old["project_id"]);
        assert_ne!(restored["id"], old["id"]);
        assert_ne!(restored["instance_id"], old["instance_id"]);
        assert!(restored["ownership"]["owner"].is_null());
        assert_eq!(host.saved(restored), history);
    }
    assert_eq!(
        fs::read(host.root.join("peer/effect.json")).unwrap(),
        effect
    );
    let requests = host.requests();
    assert!(requests[request_start..].iter().all(|request| matches!(
        request["method"].as_str(),
        Some("initialize" | "session/load")
    )));
    for restored in [&b, &c] {
        assert_eq!(
            requests[request_start..]
                .iter()
                .filter(|request| request["method"] == "session/load"
                    && request["session_id"] == restored["session_id"])
                .count(),
            1
        );
    }
    let owner_b = host.claim(&b);
    host.prompt(&b, &owner_b, "add 1 2", 20);
    let owner_c = host.claim(&c);
    host.prompt(&c, &owner_c, "add 2 3", 15);
    host.call(
        json!({"op":"acp-close","target":Host::target(&c),"control":owner_c["control"],
        "owner_generation":owner_c["generation"],"call_id":"close-after-metadata-update"}),
    );
    host.wait_records(2);
    host.stop();
    assert!(
        host.journal()["records"]
            .as_array()
            .unwrap()
            .iter()
            .all(|record| {
                record["project"]["name"] == changed["name"]
                    && record["project"]["context"] == changed["context"]
            })
    );
    let request_start = host.requests().len();
    host.start_with_path(&path);
    let restored = host.ready("Beta");
    host.until("second startup is settled", || {
        (host.snapshot()["startup_pending"] == false).then_some(())
    });
    assert_eq!(restored["session_id"], b["session_id"]);
    assert_eq!(host.saved(&restored)["value"], 20);
    assert_eq!(host.saved(&restored)["commands"], 2);
    assert_eq!(host.saved(&c)["value"], 15);
    assert!(
        host.requests()[request_start..]
            .iter()
            .all(|request| request["session_id"] != c["session_id"]),
        "explicit close must remain excluded after metadata synchronization"
    );
    let snapshot = host.snapshot();
    let project = snapshot["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|project| project["id"] == changed["id"])
        .unwrap();
    assert_eq!(project["name"], changed["name"]);
    assert_eq!(project["context"], changed["context"]);
    assert_eq!(fs::read_to_string(entered).unwrap(), "entered\nentered\n");
    host.stop();
}

#[test]
fn deferred_native_startup_orders_daemon_hooks_before_explicit_project_open_and_new() {
    let mut host = Host::new();
    let [(native, _), (old_b, _)] = host.seed();
    let history = host.saved(&old_b);
    host.stop();
    let session = uuid::Uuid::new_v4().to_string();
    let file = native_header(&host, &session, "hook-ordering.jsonl");
    replace_with_native(
        &host,
        &native,
        json!({"kind":"codex","model":"fixture-model"}),
        &session,
        &file,
    );
    let setup = host.root.join("daemon-setup");
    let effects = host.root.join("hook-effects");
    let project_hook = |operation: &str| {
        json!({
            "source":format!(
                "set -eu; read -r setup < '{}'; printf '%s:{operation}\\n' \"$setup\" >> '{}'",
                setup.display(), effects.display()
            ),
            "timeout_ms":1000
        })
    };
    let config_path = host.root.join("config/amux/config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["config"]["terminal_limit"] = json!(3);
    config["config"]["daemon_start_hooks"] = json!([{
        "source":format!(
            "set -eu; printf 'daemon-setup\\n' > '{}'; printf 'daemon-start\\n' >> '{}'",
            setup.display(), effects.display()
        ),
        "timeout_ms":1000
    }]);
    config["projects"][0]["hooks"] = json!([project_hook("project-open")]);
    fs::write(config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let (path, gate, entered) = native_preparation_gate(&host);
    let request_start = host.requests().len();
    host.start_with_path(&path);
    host.until("real native version probe holds startup", || {
        entered.exists().then_some(())
    });
    let waiting = host.snapshot();
    assert_eq!(waiting["startup_pending"], true);
    assert!(waiting["terminals"].as_array().unwrap().is_empty());
    assert!(waiting["agent_views"].as_array().unwrap().is_empty());
    let opened = host.call(json!({"op":"project-open","project":host.projects[0]}));
    let directory = host.root.join("created-project");
    fs::create_dir(&directory).unwrap();
    let created = host.call(json!({
        "op":"project-new","name":"created-during-native-preparation","directory":directory,
        "config":{"hooks":[project_hook("project-new")]}
    }));
    for terminal in [&opened["terminal"]["id"], &created["terminal"]["id"]] {
        assert!(terminal.is_string());
        host.until(
            "explicit stock shell remains responsive during native preparation",
            || {
                let state = host.call(json!({"op":"state","terminal":terminal}));
                (state["integration"]["shell"]["ready"] == true).then_some(())
            },
        );
    }
    let rejected = host.request(json!({"op":"open"})).unwrap();
    assert_eq!(rejected["ok"], false);
    assert_eq!(rejected["error"]["code"], "terminal-limit");
    assert_eq!(host.snapshot()["startup_pending"], true);
    assert_eq!(host.snapshot()["terminals"].as_array().unwrap().len(), 2);
    assert!(
        !setup.exists(),
        "startup effects must wait for native preparation"
    );
    assert!(
        !effects.exists(),
        "project hooks must wait behind daemon setup"
    );
    fs::remove_file(gate).unwrap();
    let b = host.ready("Beta");
    let settled = host.until("all three FIFO hook jobs complete after startup", || {
        let snapshot = host.snapshot();
        let completed = snapshot["diagnostics"]
            .as_array()?
            .iter()
            .filter(|entry| {
                matches!(
                    entry["kind"].as_str(),
                    Some("daemon-start" | "project-open")
                )
            })
            .count();
        (snapshot["startup_pending"] == false && completed == 3).then_some(snapshot)
    });
    let diagnostics: Vec<_> = settled["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| {
            matches!(
                entry["kind"].as_str(),
                Some("daemon-start" | "project-open")
            )
        })
        .collect();
    assert!(
        diagnostics
            .iter()
            .all(|entry| entry["diagnostic"]["ok"] == true),
        "{diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|entry| entry["kind"] == "daemon-start")
            .count(),
        1
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|entry| entry["kind"] == "project-open")
            .count(),
        2
    );
    assert_eq!(
        fs::read_to_string(&effects).unwrap(),
        "daemon-start\ndaemon-setup:project-open\ndaemon-setup:project-new\n"
    );
    let terminals = settled["terminals"].as_array().unwrap();
    assert_eq!(
        terminals.len(),
        2,
        "startup must not allocate an extra shell"
    );
    for terminal in [&opened["terminal"]["id"], &created["terminal"]["id"]] {
        assert!(terminals.iter().any(|entry| entry["id"] == *terminal));
    }
    assert_eq!(b["session_id"], old_b["session_id"]);
    assert_eq!(host.saved(&b), history);
    assert!(
        host.requests()[request_start..]
            .iter()
            .all(|request| matches!(
                request["method"].as_str(),
                Some("initialize" | "session/load")
            ))
    );
    let owner = host.claim(&b);
    host.prompt(&b, &owner, "add 1 2", 20);
    host.call(json!({"op":"open"}));
    assert_eq!(host.snapshot()["terminals"].as_array().unwrap().len(), 3);
    let rejected = host.request(json!({"op":"open"})).unwrap();
    assert_eq!(rejected["ok"], false);
    assert_eq!(rejected["error"]["code"], "terminal-limit");
    host.stop();
    assert_eq!(
        fs::read_to_string(effects).unwrap(),
        "daemon-start\ndaemon-setup:project-open\ndaemon-setup:project-new\n"
    );
}

#[test]
fn shutdown_cancels_pending_native_startup_and_queued_project_hooks_without_releasing_the_probe() {
    let mut host = Host::new();
    let grant = host.open(0, "Alpha");
    let native = host.ready("Alpha");
    host.prompt(&native, &grant, "add 17 25", 42);
    host.wait_records(1);
    host.stop();
    let session = uuid::Uuid::new_v4().to_string();
    let file = native_header(&host, &session, "hook-shutdown.jsonl");
    replace_with_native(
        &host,
        &native,
        json!({"kind":"codex","model":"fixture-model"}),
        &session,
        &file,
    );
    let startup_marker = host.root.join("daemon-start-ran");
    let project_marker = host.root.join("project-hook-ran");
    let config_path = host.root.join("config/amux/config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["config"]["terminal_limit"] = json!(2);
    config["config"]["daemon_start_hooks"] = json!([{
        "source":format!("touch '{}'", startup_marker.display()),"timeout_ms":1000
    }]);
    config["projects"][0]["hooks"] = json!([{
        "source":format!("touch '{}'", project_marker.display()),"timeout_ms":1000
    }]);
    fs::write(config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let (path, gate, entered) = native_preparation_gate(&host);
    host.start_with_path(&path);
    host.until("native preparation enters its unreleased probe", || {
        entered.exists().then_some(())
    });
    let probe_pid: i32 = fs::read_to_string(host.root.join("native-version-pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let opened = host.call(json!({"op":"project-open","project":host.projects[0]}));
    let shell_pid = opened["terminal"]["pid"].as_i64().unwrap() as i32;
    assert_eq!(host.snapshot()["startup_pending"], true);
    assert!(!startup_marker.exists());
    assert!(!project_marker.exists());
    host.stop();
    assert!(
        gate.exists(),
        "shutdown must cancel preparation, not release the fixture gate"
    );
    assert!(!startup_marker.exists());
    assert!(!project_marker.exists());
    host.until("shutdown reaps the owned probe and explicit shell", || {
        [probe_pid, shell_pid]
            .iter()
            .all(|pid| {
                (unsafe { libc::kill(*pid, 0) }) == -1
                    && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
            })
            .then_some(())
    });
}

#[test]
fn failed_startup_fallback_finishes_readiness_without_running_daemon_start_hooks() {
    let mut host = Host::new();
    let [(native, _), (old_b, _)] = host.seed();
    let history = host.saved(&old_b);
    host.stop();
    let session = uuid::Uuid::new_v4().to_string();
    let file = native_header(&host, &session, "failed-fallback.jsonl");
    replace_with_native(
        &host,
        &native,
        json!({"kind":"codex","model":"fixture-model"}),
        &session,
        &file,
    );
    let startup_marker = host.root.join("daemon-start-ran");
    let fence_marker = host.root.join("project-hook-ran");
    let config_path = host.root.join("config/amux/config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["config"]["daemon_start_hooks"] = json!([{
        "source":format!("touch '{}'", startup_marker.display()),"timeout_ms":1000
    }]);
    // The failed native row is the only member of First project, so this
    // later explicit open cannot merely reattach an already recovered view.
    config["projects"][0]["hooks"] = json!([{
        "source":format!("touch '{}'", fence_marker.display()),"timeout_ms":1000
    }]);
    fs::write(config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let fallback_cwd = host.root.join("removed-startup-cwd");
    fs::create_dir(&fallback_cwd).unwrap();
    let (path, gate, entered) = native_preparation_gate(&host);
    host.start_with_path_at(&path, &fallback_cwd);
    host.until("native probe gates fallback creation", || {
        entered.exists().then_some(())
    });
    assert_eq!(host.snapshot()["startup_pending"], true);
    fs::remove_dir(&fallback_cwd).unwrap();
    fs::remove_file(gate).unwrap();
    let b = host.ready("Beta");
    let failed = host.until("failed fallback settles startup readiness", || {
        let snapshot = host.snapshot();
        (snapshot["startup_pending"] == false
            && snapshot["diagnostics"]
                .as_array()?
                .iter()
                .any(|diagnostic| {
                    diagnostic["kind"] == "recovery" && diagnostic["target"].is_null()
                }))
        .then_some(snapshot)
    });
    assert!(failed["terminals"].as_array().unwrap().is_empty());
    assert_eq!(failed["agent_views"].as_array().unwrap().len(), 1);
    assert_eq!(b["session_id"], old_b["session_id"]);
    assert_eq!(host.saved(&b), history);
    let owner = host.claim(&b);
    host.prompt(&b, &owner, "add 1 2", 20);
    let opened = host.call(json!({"op":"project-open","project":host.projects[0]}));
    assert!(opened["terminal"]["id"].is_string());
    host.until(
        "later project hook fences the shared FIFO hook worker",
        || fence_marker.exists().then_some(()),
    );
    let snapshot = host.snapshot();
    assert!(
        snapshot["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .all(|diagnostic| diagnostic["kind"] != "daemon-start"),
        "failed first-shell creation must not enqueue daemon-start hooks"
    );
    assert!(!startup_marker.exists());
    assert_eq!(snapshot["startup_pending"], false);
    assert_eq!(snapshot["terminals"].as_array().unwrap().len(), 1);
    assert_eq!(host.saved(&b)["value"], 20);
    host.stop();
}

#[test]
fn saving_a_shared_project_keeps_every_conversation_recoverable_with_the_new_group_metadata() {
    let mut host = Host::new();
    host.projects[1] = host.projects[0].clone();
    let [(a, _), (b, _)] = host.seed();
    host.call(json!({"op":"project-save","terminal":a["id"],"name":"Shared saved project"}));
    let grant = host.open(0, "Gamma");
    let c = host.ready("Gamma");
    host.prompt(&c, &grant, "add 1 9", 10);
    host.wait_records(3);
    assert!(
        host.journal()["records"]
            .as_array()
            .unwrap()
            .iter()
            .all(|record| {
                record["project"]["id"] == host.projects[0]
                    && record["project"]["name"] == "Shared saved project"
            })
    );
    host.stop();
    host.start();
    assert_eq!(host.ready("Alpha")["session_id"], a["session_id"]);
    assert_eq!(host.ready("Beta")["session_id"], b["session_id"]);
    let restored = host.ready("Gamma");
    assert_eq!(restored["session_id"], c["session_id"]);
    assert_eq!(host.saved(&a)["value"], 42);
    assert_eq!(host.saved(&b)["value"], 17);
    assert_eq!(host.saved(&c)["value"], 10);
    let owner = host.claim(&restored);
    host.prompt(&restored, &owner, "add 2 3", 15);
    host.stop();
}

#[test]
fn unsaved_acp_only_project_config_changes_survive_relaunch_without_saving_or_another_target() {
    let mut host = Host::new();
    let controller = host
        .call(json!({"op":"controller-create","role":"external","label":"unsaved project config"}));
    let grant = host.call(
        json!({"op":"acp-open","agent":"math","project":host.projects[0],
        "new_project":true,"cwd":host.root.join("first"),"name":"Configured conversation",
        "controller":controller["controller"]}),
    );
    let original = host.ready("Configured conversation");
    let project_id = original["project_id"].clone();
    host.prompt(&original, &grant, "add 17 25", 42);
    host.wait_records(1);
    let before = host.snapshot();
    let project = before["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|project| project["id"] == project_id)
        .unwrap();
    assert_eq!(project["saved"], false);
    assert!(project["main_terminal"].is_null());
    assert!(
        before["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .all(|terminal| terminal["project_id"] != project_id),
        "a native main would overwrite the manually configured unsaved context"
    );
    let saved = host.saved(&original);
    assert_eq!(saved["session_id"], original["session_id"]);
    assert_eq!(saved["cwd"], json!(host.root.join("first")));
    assert_eq!(saved["commands"], 1);
    let effect = fs::read(host.root.join("peer/effect.json")).unwrap();
    let config_path = host.root.join("config/amux/config.json");

    // This mutation is the last project/target operation before shutdown.
    let configured = host.call(json!({"op":"project-config","project":project_id,"config":{
        "name":"Changed unsaved ACP group","context":host.root.join("second")}}));
    assert_eq!(configured["id"], project_id);
    assert_eq!(configured["name"], "Changed unsaved ACP group");
    assert_eq!(configured["context"], json!(host.root.join("second")));
    assert_eq!(configured["saved"], false);
    assert!(configured["main_terminal"].is_null());
    let disk_after: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    assert!(
        disk_after["projects"]
            .as_array()
            .unwrap()
            .iter()
            .all(|project| project["id"] != project_id),
        "the unsaved group must not become a saved project as a recovery workaround"
    );
    host.stop();
    let request_start = host.requests().len();
    host.start();

    let restored = host.ready("Configured conversation");
    assert_eq!(restored["session_id"], original["session_id"]);
    assert_eq!(restored["project_id"], project_id);
    assert_ne!(restored["id"], original["id"]);
    assert_ne!(restored["instance_id"], original["instance_id"]);
    assert!(restored["ownership"]["owner"].is_null());
    let snapshot = host.snapshot();
    let project = snapshot["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|project| project["id"] == project_id)
        .unwrap();
    assert_eq!(project["name"], "Changed unsaved ACP group");
    assert_eq!(project["context"], json!(host.root.join("second")));
    assert_eq!(project["saved"], false);
    assert!(project["main_terminal"].is_null());
    assert!(
        snapshot["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .all(|terminal| terminal["project_id"] != project_id)
    );
    assert_eq!(
        host.saved(&restored),
        saved,
        "reopen changed the original cwd or history"
    );
    assert_eq!(
        fs::read(host.root.join("peer/effect.json")).unwrap(),
        effect
    );
    let requests = host.requests();
    let reopened = &requests[request_start..];
    assert_eq!(
        reopened
            .iter()
            .filter(|request| request["method"] == "session/load"
                && request["session_id"] == original["session_id"])
            .count(),
        1
    );
    assert!(reopened.iter().all(|request| matches!(
        request["method"].as_str(),
        Some("initialize" | "session/load")
    )));

    let owner = host.claim(&restored);
    host.prompt(&restored, &owner, "add 2 3", 47);
    let after = host.saved(&restored);
    assert_eq!(after["session_id"], original["session_id"]);
    assert_eq!(after["cwd"], saved["cwd"]);
    assert_eq!(after["commands"], 2);
    host.stop();
    let root = host.root.clone();
    drop(host);
    assert!(!root.exists(), "private recovery fixture was not removed");
}

#[test]
fn unsaved_main_shell_cd_refresh_survives_relaunch_without_changing_the_acp_conversation_cwd() {
    let mut host = Host::new();
    let initial = host.snapshot();
    let main = initial["terminals"][0]["id"].clone();
    let project_id = initial["terminals"][0]["project_id"].clone();
    let controller = host.call(
        json!({"op":"controller-create","role":"external","label":"automatic project refresh"}),
    );
    let settled = host.until("main shell ready for an explicit structured cd", || {
        let state = host.call(json!({"op":"state","terminal":main}));
        (state["integration"]["shell"]["ready"] == true
            && state["workspace"]["cwd"] == json!(host.root)
            && state["ownership"]["owner"].is_null()
            && state["ownership"]["reservation"].is_null()
            && state["ownership"]["unsettled"]
                .as_array()
                .is_some_and(Vec::is_empty))
        .then_some(state)
    });
    let shell_grant = host.call(
        json!({"op":"ownership-claim","terminal":main,"controller":controller["controller"],
        "observed":settled["ownership"]["observed"],"mode":"cooperative","when":{"kind":"now"}}),
    );
    let grant = host.call(json!({"op":"acp-open","agent":"math","project":project_id,
        "cwd":host.root.join("first"),"name":"Conversation beside main shell",
        "controller":controller["controller"]}));
    let original = host.ready("Conversation beside main shell");
    host.prompt(&original, &grant, "add 8 9", 17);
    host.wait_records(1);
    let saved = host.saved(&original);
    assert_eq!(saved["session_id"], original["session_id"]);
    assert_eq!(saved["cwd"], json!(host.root.join("first")));
    assert_eq!(saved["commands"], 1);
    let effect = fs::read(host.root.join("peer/effect.json")).unwrap();
    let changed = host.root.join("second");

    let execution = host.call(
        json!({"op":"execute","terminal":main,"control":shell_grant["control"],
        "source":format!("cd '{}'",changed.display())}),
    );
    host.until(
        "structured main-shell cd completed in the new directory",
        || {
            let state = host.call(json!({"op":"state","terminal":main}));
            let completed = state["integration"]["executions"]
                .as_array()?
                .iter()
                .find(|item| item["execution_id"] == execution["execution_id"])?;
            (completed["status"] == "completed"
                && completed["exit_status"] == 0
                && state["integration"]["shell"]["ready"] == true
                && state["workspace"]["cwd"] == json!(changed))
            .then_some(())
        },
    );
    // Observe the real periodic refresh, not a synthetic integration report or save.
    let refreshed = host.until("automatic unsaved-project context refresh", || {
        host.snapshot()["projects"]
            .as_array()?
            .iter()
            .find(|project| project["id"] == project_id && project["context"] == json!(changed))
            .cloned()
    });
    assert_eq!(refreshed["saved"], false);
    assert_eq!(refreshed["main_terminal"], main);
    assert_eq!(original["project_id"], project_id);
    host.stop();
    let request_start = host.requests().len();
    host.start();

    let restored = host.ready("Conversation beside main shell");
    assert_eq!(restored["session_id"], original["session_id"]);
    assert_eq!(restored["project_id"], project_id);
    assert_ne!(restored["id"], original["id"]);
    assert_ne!(restored["instance_id"], original["instance_id"]);
    assert!(restored["ownership"]["owner"].is_null());
    let snapshot = host.snapshot();
    let project = snapshot["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|project| project["id"] == project_id)
        .unwrap();
    assert_eq!(project["name"], refreshed["name"]);
    assert_eq!(project["context"], json!(changed));
    assert_eq!(project["saved"], false);
    assert!(
        project["main_terminal"].is_null(),
        "ordinary shells must not be restored"
    );
    assert_eq!(
        host.saved(&restored),
        saved,
        "project refresh changed the exact ACP cwd or history"
    );
    assert_eq!(
        fs::read(host.root.join("peer/effect.json")).unwrap(),
        effect
    );
    let requests = host.requests();
    let reopened = &requests[request_start..];
    assert_eq!(
        reopened
            .iter()
            .filter(|request| request["method"] == "session/load"
                && request["session_id"] == original["session_id"])
            .count(),
        1
    );
    assert!(reopened.iter().all(|request| matches!(
        request["method"].as_str(),
        Some("initialize" | "session/load")
    )));

    let owner = host.claim(&restored);
    host.prompt(&restored, &owner, "add 2 4", 23);
    let after = host.saved(&restored);
    assert_eq!(after["session_id"], original["session_id"]);
    assert_eq!(after["cwd"], saved["cwd"]);
    assert_eq!(after["commands"], 2);
    host.stop();
    let root = host.root.clone();
    drop(host);
    assert!(!root.exists(), "private recovery fixture was not removed");
}

#[test]
fn a_previously_accepted_long_unicode_name_reopens_exactly_without_truncation() {
    let mut host = Host::new();
    let [(a, _), (b, _)] = host.seed();
    let name = "節".repeat(60);
    host.call(json!({"op":"name","terminal":a["id"],"name":name}));
    host.stop();
    host.start();
    let restored = host.ready(&name);
    assert_eq!(restored["name"], name);
    assert_eq!(restored["session_id"], a["session_id"]);
    assert_eq!(host.ready("Beta")["session_id"], b["session_id"]);
    let owner = host.claim(&restored);
    host.prompt(&restored, &owner, "add 7 8", 57);
    host.stop();
}

#[test]
fn closing_a_starting_recovered_target_forgets_its_intent_before_load_can_complete() {
    let mut host = Host::new();
    let [(a, _), (b, _)] = host.seed();
    host.stop();
    let gate = host
        .root
        .join("peer")
        .join(format!("pause-load-{}", b["session_id"].as_str().unwrap()));
    fs::write(&gate, b"pause").unwrap();
    let request_start = host.requests().len();
    host.start();
    host.until("original Beta load is paused", || {
        host.requests()[request_start..]
            .iter()
            .any(|request| {
                request["method"] == "session/load" && request["session_id"] == b["session_id"]
            })
            .then_some(())
    });
    let starting = host.snapshot()["agent_views"]
        .as_array()
        .unwrap()
        .iter()
        .find(|view| view["name"] == "Beta")
        .unwrap()
        .clone();
    assert_ne!(starting["lifecycle"], "ready");
    let grant = host.claim(&starting);
    host.call(
        json!({"op":"acp-close","target":Host::target(&starting),"control":grant["control"],
        "owner_generation":grant["generation"],"call_id":"explicit-starting-close"}),
    );
    fs::remove_file(gate).unwrap();
    assert_eq!(host.ready("Alpha")["session_id"], a["session_id"]);
    assert!(
        host.journal()["records"]
            .as_array()
            .unwrap()
            .iter()
            .all(|record| record["session_id"] != b["session_id"])
    );
    host.stop();
    let request_start = host.requests().len();
    host.start();
    assert_eq!(host.ready("Alpha")["session_id"], a["session_id"]);
    assert!(
        !host.snapshot()["agent_views"]
            .as_array()
            .unwrap()
            .iter()
            .any(|view| view["name"] == "Beta")
    );
    assert!(
        host.requests()[request_start..]
            .iter()
            .all(|request| request["session_id"] != b["session_id"])
    );
    assert_eq!(host.saved(&b)["value"], 17);
    host.stop();
}

#[test]
fn both_ambiguous_category_choices_survive_reopening_and_control_fresh_arithmetic() {
    let mut host = Host::with_mode("recovery-options");
    let controller =
        host.call(json!({"op":"controller-create","role":"external","label":"explicit choices"}));
    let grant = host.call(
        json!({"op":"acp-open","agent":"other","project":host.projects[1],
        "cwd":host.root.join("second"),"name":"Choices","controller":controller["controller"],
        "settings":{"options":{"thinking":"high","other-thinking":"high"}}}),
    );
    let original = host.ready("Choices");
    host.prompt(&original, &grant, "add 17 25", 342);
    host.wait_records(1);
    host.stop();
    let request_start = host.requests().len();
    host.start();
    let restored = host.ready("Choices");
    assert_eq!(restored["session_id"], original["session_id"]);
    assert_eq!(host.saved(&original)["value"], 342);
    assert_eq!(host.saved(&original)["commands"], 1);
    assert!(
        host.requests()[request_start..]
            .iter()
            .all(|request| request["method"] != "session/prompt")
    );
    let owner = host.claim(&restored);
    host.prompt(&restored, &owner, "add 2 3", 647);
    host.stop();
}

#[test]
fn an_unsaved_or_unsupported_native_context_blocks_the_previous_context_without_stale_shutdown_erasure()
 {
    let mut host = Host::new();
    let mut peer = NativePeer::new();
    let pid = peer.0.id();
    let marker = host.root.join("unexpected-native-reopen");
    let path = private_omp(&host, &marker);
    let terminal = host.snapshot()["terminals"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let a = uuid::Uuid::new_v4().to_string();
    let b = uuid::Uuid::new_v4().to_string();
    let a_file = native_header(&host, &a, "native-a.jsonl");
    let b_file = host.root.join("native-b.jsonl");
    native_report(&host, &terminal, "native-one", &a, &a_file, true, pid);
    host.wait_records(1);
    assert_eq!(host.journal()["records"][0]["session_id"], a);
    native_report(&host, &terminal, "native-one", &b, &b_file, false, pid);
    assert!(host.journal()["records"][0]["blocked"].is_string());
    let b_file = native_header(&host, &b, "native-b.jsonl");
    native_report(&host, &terminal, "native-one", &b, &b_file, true, pid);
    assert_eq!(host.journal()["records"][0]["session_id"], b);
    assert!(host.journal()["records"][0]["blocked"].is_null());
    host.call(json!({"op":"report","terminal":terminal,"event":{
        "kind":"agent","instance_id":"unsupported-root","agent_id":"main","agent_kind":"main","parent_id":null,
        "activity":"idle","settled":true,"pid":pid,"agent_family":"other","lifecycle_event":"session_start"
    }}));
    host.call(json!({"op":"report","terminal":terminal,"event":{
        "kind":"agent","instance_id":"native-one","agent_id":"main","agent_kind":"main","parent_id":null,
        "activity":"idle","settled":true,"pid":pid,"shutdown":true,"lifecycle_event":"session_shutdown"
    }}));
    assert!(
        host.journal()["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["session_id"] == b && record["blocked"].is_string())
    );
    let saved_record = host.journal()["records"][0]["id"].clone();
    host.stop();
    assert!(
        peer.0.try_wait().unwrap().is_none(),
        "daemon signaled a reported peer it does not own"
    );
    peer.stop();
    host.start_with_path(&path);
    host.until(
        "superseded context is blocked even after its peer is gone",
        || {
            host.snapshot()["diagnostics"]
                .as_array()?
                .iter()
                .any(|diagnostic| {
                    diagnostic["kind"] == "recovery" && diagnostic["target"] == saved_record
                })
                .then_some(())
        },
    );
    assert!(
        !marker.exists(),
        "the previous context was sent to the native resume executable"
    );
    host.stop();
}

#[test]
fn an_unsafe_native_fifo_does_not_block_other_reopening_or_shutdown() {
    let mut host = Host::new();
    let [(a, _), (b, _)] = host.seed();
    host.stop();
    let fifo = host.root.join("native-session.fifo");
    let name = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let session = uuid::Uuid::new_v4().to_string();
    replace_with_native(&host, &a, json!({"kind":"omp"}), &session, &fifo);
    host.start();
    assert_eq!(host.ready("Beta")["session_id"], b["session_id"]);
    assert_eq!(host.saved(&b)["value"], 17);
    assert!(
        host.snapshot()["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| {
                diagnostic["kind"] == "recovery" && diagnostic["target"] == "0-native"
            })
    );
    host.stop();
}

#[test]
fn native_codex_prebackend_rejections_can_be_retried_after_the_executable_changes() {
    let mut host = Host::new();
    let [(a, _), (b, _)] = host.seed();
    host.stop();
    let session = uuid::Uuid::new_v4().to_string();
    let file = host.root.join("unused-rollout.jsonl");
    replace_with_native(
        &host,
        &a,
        json!({"kind":"codex","model":"offline"}),
        &session,
        &file,
    );
    host.start();
    assert_eq!(host.ready("Beta")["session_id"], b["session_id"]);
    host.until("conclusive missing-executable marker is clear", || {
        host.journal()["records"]
            .as_array()?
            .iter()
            .find(|record| record["session_id"] == session)
            .filter(|record| record["launch_pending"] == false)
            .map(|_| ())
    });
    host.stop();
    let bin = host.root.join("bin");
    fs::create_dir(&bin).unwrap();
    let attempts = host.root.join("codex-attempts");
    let executable = bin.join("codex");
    fs::write(&executable, format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = '--version' ]; then printf 'codex-cli 99.0.0\\n'; exit 0; fi\nexit 92\n", attempts.display())).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let path = format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", bin.display());
    host.start_with_path(&path);
    assert_eq!(host.ready("Beta")["session_id"], b["session_id"]);
    host.until(
        "replacement executable is actually probed, not blocked by old marker",
        || {
            fs::read_to_string(&attempts)
                .ok()
                .filter(|text| text.lines().any(|line| line == "--version"))
        },
    );
    host.until(
        "conclusive unsupported-version marker is also clear",
        || {
            host.journal()["records"]
                .as_array()?
                .iter()
                .find(|record| record["session_id"] == session)
                .filter(|record| record["launch_pending"] == false)
                .map(|_| ())
        },
    );
    assert!(
        fs::read_to_string(attempts)
            .unwrap()
            .lines()
            .all(|line| line == "--version")
    );
    host.stop();
}

#[test]
fn saved_settings_override_launch_defaults_across_both_alias_forms_and_leave_new_launches_unchanged()
 {
    let mut host = Host::with_mode("recovery-single-option");
    host.stop();
    let config_path = host.root.join("config/amux/config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["config"]["acp_profiles"]["other"]["defaults"] = json!({"options":{"thinking":"high"}});
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    host.start();
    let controller =
        host.call(json!({"op":"controller-create","role":"external","label":"saved choice"}));
    let grant = host.call(json!({
        "op":"acp-open","agent":"other","project":host.projects[1],
        "cwd":host.root.join("second"),"name":"Saved choice","controller":controller["controller"],
        "settings":{"options":{"thinking":"low"}}
    }));
    let original = host.ready("Saved choice");
    host.prompt(&original, &grant, "add 17 25", 42);
    host.wait_records(1);
    host.stop();
    let request_start = host.requests().len();
    host.start();
    let restored = host.ready("Saved choice");
    assert_eq!(restored["session_id"], original["session_id"]);
    assert_eq!(host.saved(&original)["value"], 42);
    assert_eq!(host.saved(&original)["commands"], 1);
    assert!(
        host.requests()[request_start..]
            .iter()
            .all(|request| request["method"] != "session/prompt")
    );
    let owner = host.claim(&restored);
    host.prompt(&restored, &owner, "add 2 3", 47);
    host.stop();
    // Defaults may change representation without changing the launch identity.
    // Existing acknowledged choices stay authoritative; genuinely new sessions
    // must still use the newly loaded defaults.
    config["config"]["acp_profiles"]["other"]["defaults"] = json!({"effort":"high"});
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let request_start = host.requests().len();
    host.start();
    let restored = host.ready("Saved choice");
    assert_eq!(restored["session_id"], original["session_id"]);
    assert_eq!(host.saved(&original)["value"], 47);
    assert_eq!(host.saved(&original)["commands"], 2);
    assert!(
        host.requests()[request_start..]
            .iter()
            .all(|request| request["method"] != "session/prompt")
    );
    let owner = host.claim(&restored);
    host.prompt(&restored, &owner, "add 2 3", 52);
    let fresh_grant = host.open(1, "New default");
    let fresh = host.ready("New default");
    host.prompt(&fresh, &fresh_grant, "add 2 3", 105);
    assert_eq!(host.saved(&original)["value"], 52);
    assert_eq!(host.saved(&original)["commands"], 3);
    host.stop();
}

#[test]
fn saved_settings_keep_exact_model_choices_when_legacy_metadata_has_an_ambiguous_modern_category() {
    let mut host = Host::with_mode("recovery-model-options");
    let controller =
        host.call(json!({"op":"controller-create","role":"external","label":"exact models"}));
    let grant = host.call(json!({
        "op":"acp-open","agent":"other","project":host.projects[1],
        "cwd":host.root.join("second"),"name":"Exact models","controller":controller["controller"],
        "settings":{"options":{"primary-model":"high","secondary-model":"high"}}
    }));
    let original = host.ready("Exact models");
    host.prompt(&original, &grant, "add 17 25", 342);
    host.wait_records(1);
    host.stop();
    let request_start = host.requests().len();
    host.start();
    let restored = host.ready("Exact models");
    assert_eq!(restored["session_id"], original["session_id"]);
    assert_eq!(host.saved(&original)["value"], 342);
    assert_eq!(host.saved(&original)["commands"], 1);
    assert!(
        host.requests()[request_start..]
            .iter()
            .all(|request| request["method"] != "session/prompt")
    );
    let owner = host.claim(&restored);
    host.prompt(&restored, &owner, "add 2 3", 647);
    host.stop();
}

#[test]
fn native_omp_quota_one_reopens_the_original_shell_context_without_a_placeholder_slot() {
    let mut host = Host::new();
    host.stop();
    let config_path = host.root.join("config/amux/config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["config"]["terminal_limit"] = json!(1);
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let marker = host.root.join("quota-one-attempts");
    let path = private_omp(&host, &marker);
    host.start_with_path(&path);
    let original = host.snapshot()["terminals"][0].clone();
    let terminal = original["id"].as_str().unwrap();
    host.call(json!({"op":"name","terminal":terminal,"name":"Original native"}));
    let mut peer = NativePeer::new();
    let session = uuid::Uuid::new_v4().to_string();
    let file = native_header(&host, &session, "quota-one.jsonl");
    native_report(
        &host,
        terminal,
        "quota-original",
        &session,
        &file,
        true,
        peer.0.id(),
    );
    host.wait_records(1);
    let record_id = host.journal()["records"][0]["id"].clone();
    assert_eq!(host.snapshot()["terminals"].as_array().unwrap().len(), 1);
    host.stop();
    assert!(peer.0.try_wait().unwrap().is_none());
    peer.stop();
    host.start_with_path(&path);
    host.until(
        "the original native context consumes its original quota slot",
        || {
            fs::read_to_string(&marker)
                .ok()
                .filter(|text| text == "attempted\n")
        },
    );
    let snapshot = host.snapshot();
    let terminals = snapshot["terminals"].as_array().unwrap();
    assert_eq!(terminals.len(), 1);
    assert_eq!(terminals[0]["name"], "Original native");
    assert_ne!(terminals[0]["id"], original["id"]);
    let rejected = host.request(json!({"op":"open"})).unwrap();
    assert_eq!(rejected["ok"], false);
    assert_eq!(rejected["error"]["code"], "terminal-limit");
    host.stop();
    // The fixture executable deliberately exits without reporting a new native
    // identity. Its unknown launch must block, yet leave a fresh fallback shell.
    host.start_with_path(&path);
    host.until(
        "unknown native launch has a bounded diagnostic and fresh fallback",
        || {
            let snapshot = host.snapshot();
            (snapshot["terminals"].as_array()?.len() == 1
                && snapshot["diagnostics"]
                    .as_array()?
                    .iter()
                    .any(|diagnostic| {
                        diagnostic["kind"] == "recovery" && diagnostic["target"] == record_id
                    }))
            .then_some(())
        },
    );
    assert_eq!(fs::read_to_string(&marker).unwrap(), "attempted\n");
    assert!(
        host.snapshot()["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .all(|terminal| terminal["name"] != "Original native")
    );
    host.stop();
}

#[test]
fn native_omp_quota_reduction_retains_retryable_excess_intent_without_relaxing_the_limit() {
    let mut host = Host::new();
    let [(a, _), (b, _)] = host.seed();
    host.stop();
    let first_session = uuid::Uuid::new_v4().to_string();
    let first_file = native_header(&host, &first_session, "quota-first.jsonl");
    replace_with_native(
        &host,
        &a,
        json!({"kind":"omp"}),
        &first_session,
        &first_file,
    );
    let second_session = uuid::Uuid::new_v4().to_string();
    let second_file = native_header(&host, &second_session, "quota-second.jsonl");
    let mut journal = host.journal();
    let mut excess = journal["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["session_id"] == first_session)
        .unwrap()
        .clone();
    excess["id"] = json!("1-native");
    excess["name"] = json!("Quota limited native");
    excess["session_id"] = json!(second_session);
    excess["session_file"] = json!(second_file);
    journal["records"].as_array_mut().unwrap().push(excess);
    fs::write(host.journal_path(), serde_json::to_vec(&journal).unwrap()).unwrap();
    let marker = host.root.join("quota-attempts");
    let path = private_omp(&host, &marker);
    let config_path = host.root.join("config/amux/config.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["config"]["terminal_limit"] = json!(1);
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    host.start_with_path(&path);
    assert_eq!(host.ready("Beta")["session_id"], b["session_id"]);
    host.until(
        "one native context fits and the excess context is diagnosed",
        || {
            let snapshot = host.snapshot();
            (fs::read_to_string(&marker).ok().as_deref() == Some("attempted\n")
                && snapshot["terminals"].as_array()?.len() == 1
                && snapshot["diagnostics"]
                    .as_array()?
                    .iter()
                    .any(|diagnostic| {
                        diagnostic["kind"] == "recovery" && diagnostic["target"] == "1-native"
                    }))
            .then_some(())
        },
    );
    assert_eq!(
        host.journal()["records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|record| record["session_id"] == second_session)
            .unwrap()["launch_pending"],
        false
    );
    host.stop();
    config["config"]["terminal_limit"] = json!(2);
    fs::write(config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    host.start_with_path(&path);
    assert_eq!(host.ready("Beta")["session_id"], b["session_id"]);
    host.until(
        "the formerly quota-limited context gets one later startup attempt",
        || {
            let snapshot = host.snapshot();
            (fs::read_to_string(&marker).ok().as_deref() == Some("attempted\nattempted\n")
                && snapshot["terminals"].as_array()?.len() <= 2
                && snapshot["terminals"]
                    .as_array()?
                    .iter()
                    .any(|terminal| terminal["name"] == "Quota limited native"))
            .then_some(())
        },
    );
    assert_eq!(host.saved(&b)["value"], 17);
    host.stop();
}

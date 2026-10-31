use parking_lot::Mutex;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

const DEADLINE: Duration = Duration::from_secs(12);
const SESSION: &str = "019a0c00-0000-7000-8000-000000000001";
const MODEL: &str = "fixture-model-no-network";
const HELPER_GUARD: &str = "private-amux-codex-wire-fixture";

// This fixture replaces only the external executable. All admission, retained
// accounting, PTY/process ownership and public API framing belong to real amux.
struct Connection(BufReader<UnixStream>);

impl Connection {
    fn connect(socket: &Path) -> io::Result<Self> {
        let stream = UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(DEADLINE))?;
        stream.set_write_timeout(Some(DEADLINE))?;
        Ok(Self(BufReader::new(stream)))
    }

    fn send(&mut self, mut request: Value) -> io::Result<()> {
        request
            .as_object_mut()
            .unwrap()
            .entry("version")
            .or_insert(json!(1));
        serde_json::to_writer(self.0.get_mut(), &request).map_err(io::Error::other)?;
        self.0.get_mut().write_all(b"\n")?;
        self.0.get_mut().flush()
    }

    fn exchange(&mut self, request: Value) -> io::Result<Value> {
        self.send(request)?;
        self.receive()
    }

    fn receive(&mut self) -> io::Result<Value> {
        let mut line = String::new();
        (&mut self.0).take(8 * 1024 * 1024).read_line(&mut line)?;
        if !line.ends_with('\n') {
            return Err(io::Error::other("incomplete public daemon response"));
        }
        let response: Value = serde_json::from_str(&line).map_err(io::Error::other)?;
        assert_eq!(response["version"], 1, "{response}");
        Ok(response)
    }
}

fn success(response: Value) -> Value {
    assert_eq!(response["ok"], true, "{response}");
    response["result"].clone()
}

fn rejected(response: Value) {
    if response["ok"] == true {
        assert_eq!(response["result"]["state"], "rejected", "{response}");
    } else {
        assert!(
            response["error"]["code"]
                .as_str()
                .is_some_and(|code| !code.is_empty()),
            "{response}"
        );
    }
}

fn private_dir(path: &Path) {
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

struct Host {
    root: PathBuf,
    socket: PathBuf,
    daemon: Child,
    terminals: RefCell<Vec<String>>,
    terminal_pids: RefCell<Vec<i32>>,
}

impl Host {
    fn new() -> Self {
        Self::with_source_path_conflict(false)
    }

    fn with_source_path_conflict(conflict: bool) -> Self {
        let root = fs::canonicalize("/tmp")
            .unwrap()
            .join(format!("amux-cdx-{}", uuid::Uuid::new_v4().simple()));
        private_dir(&root);
        for name in [
            "home",
            "startup",
            "config",
            "runtime",
            "projects",
            "codex",
            "workspace",
            "bin",
            "tmp",
        ] {
            private_dir(&root.join(name));
        }
        // Absolute script/helper paths are private; no host codex or authentication
        // can be selected even if the fixture launcher is invoked incorrectly.
        fs::write(root.join("bin/codex"), r#"#!/bin/sh
set -eu
if [ "${1:-}" = --version ]; then
    if [ -f "$AMUX_CODEX_FIXTURE_ROOT/pause-native-version" ]; then
        printf '%s\n' "$$" > "$AMUX_CODEX_FIXTURE_ROOT/native-pending.pid"
        while [ -f "$AMUX_CODEX_FIXTURE_ROOT/pause-native-version" ]; do /bin/sleep 0.02; done
    fi
    printf 'codex-cli 0.160.0\n'
    exit 0
fi
printf '%s\n' "$@" >> "$AMUX_CODEX_FIXTURE_ROOT/launcher.log"
AMUX_CODEX_FIXTURE_MODE=frontend
if [ "${1:-}" = app-server ]; then
    AMUX_CODEX_FIXTURE_MODE=backend
    if [ -f "$AMUX_CODEX_FIXTURE_ROOT/pause-native-backend" ]; then
        printf '%s\n' "$$" > "$AMUX_CODEX_FIXTURE_ROOT/native-pending.pid"
        while [ -f "$AMUX_CODEX_FIXTURE_ROOT/pause-native-backend" ]; do /bin/sleep 0.02; done
        exit 78
    fi
fi
AMUX_CODEX_FIXTURE_ENDPOINT=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --listen|--remote) shift; AMUX_CODEX_FIXTURE_ENDPOINT="$1" ;;
        --listen=*|--remote=*) AMUX_CODEX_FIXTURE_ENDPOINT="${1#*=}" ;;
    esac
    shift
done
[ -n "$AMUX_CODEX_FIXTURE_ENDPOINT" ]
export AMUX_CODEX_FIXTURE_MODE AMUX_CODEX_FIXTURE_ENDPOINT
exec "$AMUX_CODEX_FIXTURE_BINARY" --ignored --exact protocol_fixture_backend --nocapture --test-threads=1
"#).unwrap();
        fs::set_permissions(root.join("bin/codex"), fs::Permissions::from_mode(0o700)).unwrap();
        let path = format!(
            "{}:/usr/bin:/bin:/usr/sbin:/sbin",
            root.join("bin").display()
        );
        let daemon_path = if conflict {
            "bin:/usr/bin:/bin:/usr/sbin:/sbin".to_owned()
        } else {
            path.clone()
        };
        let source_path = if conflict {
            let conflicting_bin = root.join("workspace/bin");
            private_dir(&conflicting_bin);
            let conflicting = conflicting_bin.join("codex");
            fs::write(&conflicting, "#!/bin/sh\nprintf '%s\\n' \"$PWD\" > \"$AMUX_CODEX_FIXTURE_ROOT/conflicting-codex-ran\"\nexit 91\n").unwrap();
            fs::set_permissions(&conflicting, fs::Permissions::from_mode(0o700)).unwrap();
            format!(
                "{}:/usr/bin:/bin:/usr/sbin:/sbin",
                conflicting_bin.display()
            )
        } else {
            path
        };
        fs::write(
            root.join("startup/.zshrc"),
            format!("export PATH='{source_path}'\nPS1='CODEX_FIXTURE> '\n"),
        )
        .unwrap();
        let socket = root.join("daemon.sock");
        let daemon = Self::spawn(&root, &socket, &daemon_path);
        let host = Self {
            root,
            socket,
            daemon,
            terminals: RefCell::new(Vec::new()),
            terminal_pids: RefCell::new(Vec::new()),
        };
        host.until("private daemon startup", || {
            Connection::connect(&host.socket)
                .ok()?
                .exchange(json!({"op":"snapshot"}))
                .ok()
                .filter(|response| response["ok"] == true)
        });
        for terminal in host.call(json!({"op":"snapshot"}))["terminals"]
            .as_array()
            .unwrap()
        {
            let id = terminal["id"].as_str().unwrap();
            let state = host.call(json!({"op":"state","terminal":id}));
            if let Some(pid) = state["pid"].as_i64() {
                host.terminal_pids.borrow_mut().push(pid as i32);
            }
        }
        host
    }

    fn spawn(root: &Path, socket: &Path, path: &str) -> Child {
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join("daemon.log"))
            .unwrap();
        Command::new(env!("CARGO_BIN_EXE_amux"))
            .args([
                "--socket",
                socket.to_str().unwrap(),
                "daemon",
                "--projects-dir",
                root.join("projects").to_str().unwrap(),
            ])
            .env_clear()
            .env("PATH", path)
            .env("HOME", root.join("home"))
            .env("USER", "amux-fixture")
            .env("SHELL", "/bin/zsh")
            .env("TERM", "xterm-256color")
            .env("ZDOTDIR", root.join("startup"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_RUNTIME_DIR", root.join("runtime"))
            .env("AMUX_RUNTIME_DIR", root.join("runtime"))
            .env("AMUX_SOCKET", socket)
            .env("CODEX_HOME", root.join("codex"))
            .env("TMPDIR", root.join("tmp"))
            .env("AMUX_CODEX_FIXTURE_GUARD", HELPER_GUARD)
            .env("AMUX_CODEX_FIXTURE_ROOT", root)
            .env(
                "AMUX_CODEX_FIXTURE_BINARY",
                std::env::current_exe().unwrap(),
            )
            .current_dir(root)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap()
    }

    fn restart(&mut self) {
        self.call(json!({"op":"shutdown"}));
        let deadline = Instant::now() + DEADLINE;
        loop {
            if let Some(status) = self.daemon.try_wait().unwrap() {
                assert!(status.success(), "private daemon shutdown failed");
                break;
            }
            assert!(
                Instant::now() < deadline,
                "private daemon shutdown deadline"
            );
            std::thread::sleep(Duration::from_millis(15));
        }
        let path = format!(
            "{}:/usr/bin:/bin:/usr/sbin:/sbin",
            self.root.join("bin").display()
        );
        self.daemon = Self::spawn(&self.root, &self.socket, &path);
        self.until("restarted private daemon", || {
            Connection::connect(&self.socket)
                .ok()?
                .exchange(json!({"op":"snapshot"}))
                .ok()
                .filter(|reply| reply["ok"] == true)
        });
        for terminal in self.call(json!({"op":"snapshot"}))["terminals"]
            .as_array()
            .unwrap()
        {
            if let Some(pid) = terminal["pid"].as_i64() {
                self.terminal_pids.borrow_mut().push(pid as i32);
            }
        }
    }

    fn until<T>(&self, label: &str, mut probe: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + DEADLINE;
        loop {
            if let Some(value) = probe() {
                return value;
            }
            assert!(
                Instant::now() < deadline,
                "{label}; daemon: {}; fixture: {}",
                self.log("daemon.log"),
                self.log("transcript.jsonl")
            );
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    fn log(&self, file: &str) -> String {
        fs::read_to_string(self.root.join(file)).unwrap_or_default()
    }

    fn response(&self, request: Value) -> Value {
        Connection::connect(&self.socket)
            .unwrap()
            .exchange(request)
            .unwrap()
    }

    fn call(&self, request: Value) -> Value {
        success(self.response(request))
    }

    fn controller(&self, label: &str) -> Value {
        self.call(json!({"op":"controller-create","role":"external","label":label}))["controller"]
            .clone()
    }

    fn open(&self) -> Agent {
        let controller = self.controller("Codex fixture owner");
        let opened = self.call(json!({"op":"agent-open","controller":controller,"model":MODEL,"cwd":self.root.join("workspace")}));
        self.track_open(controller, opened)
    }

    fn track_open(&self, controller: Value, opened: Value) -> Agent {
        let terminal = opened["terminal"]["id"].as_str().unwrap().to_owned();
        self.terminals.borrow_mut().push(terminal.clone());
        let state = self.call(json!({"op":"state","terminal":terminal}));
        if let Some(pid) = state["pid"].as_i64() {
            self.terminal_pids.borrow_mut().push(pid as i32);
        }
        let instance = opened["instance_id"].as_str().unwrap().to_owned();
        let control = opened["control"]["control"].clone();
        assert_eq!(opened["control"]["kind"], "control", "{opened}");
        assert_eq!(control["kind"], "control", "{opened}");
        let agent = Agent {
            terminal,
            instance,
            controller,
            control,
        };
        self.until("backend driver startup", || {
            self.try_driver(json!({"action":"stats"})).ok()
        });
        agent
    }

    fn inspect(&self, agent: &Agent) -> Value {
        self.call(
            json!({"op":"agent-inspect","terminal":agent.terminal,"instance_id":agent.instance}),
        )
    }

    fn ready(&self, agent: &Agent) -> Value {
        self.driver(json!({"action":"release-native"}));
        self.until("authoritative native-created identity", || {
            let state = self.inspect(agent);
            (state["target"]["session_id"] == SESSION && state["lifecycle"]["session"] == "ready")
                .then_some(state)
        })
    }

    fn try_driver(&self, command: Value) -> io::Result<Value> {
        let stream = UnixStream::connect(self.root.join("fixture-control.sock"))?;
        stream.set_read_timeout(Some(DEADLINE))?;
        stream.set_write_timeout(Some(DEADLINE))?;
        let mut stream = BufReader::new(stream);
        serde_json::to_writer(stream.get_mut(), &command).map_err(io::Error::other)?;
        stream.get_mut().write_all(b"\n")?;
        stream.get_mut().flush()?;
        let mut line = String::new();
        stream.read_line(&mut line)?;
        serde_json::from_str(&line).map_err(io::Error::other)
    }

    fn driver(&self, command: Value) -> Value {
        let response = self.try_driver(command).unwrap();
        assert_eq!(response["ok"], true, "fixture driver: {response}");
        if let Some(violations) = response["result"].get("violations") {
            assert_eq!(
                violations,
                &json!([]),
                "fixture protocol violation: {response}"
            );
        }
        response["result"].clone()
    }

    fn transcript(&self) -> Vec<Value> {
        self.driver(json!({"action":"stats"}))["transcript"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn manager_inputs(&self) -> Vec<Value> {
        self.transcript()
            .into_iter()
            .filter(|row| row["peer"] == "manager" && row["direction"] == "received")
            .map(|row| row["message"].clone())
            .collect()
    }

    fn mutations(&self) -> Vec<Value> {
        self.manager_inputs()
            .into_iter()
            .filter(|message| {
                matches!(
                    message["method"].as_str(),
                    Some("turn/start" | "turn/steer" | "turn/interrupt")
                ) || (message.get("method").is_none() && message.get("id").is_some())
            })
            .collect()
    }

    fn wait_wire(&self, predicate: impl Fn(&Value) -> bool) -> Value {
        self.until("provider wire effect", || {
            self.manager_inputs().into_iter().find(&predicate)
        })
    }

    fn prompt(&self, agent: &Agent, intent: &str, text: &str, turn: Option<&str>) -> Value {
        let mut request = agent.request("agent-prompt");
        request["intent"] = json!(intent);
        request["text"] = json!(text);
        if let Some(turn) = turn {
            request["expected_turn_id"] = json!(turn);
        }
        self.call(request)
    }

    fn backpressure(&self, agent: &Agent, turn: &str) -> Value {
        self.driver(json!({"action":"pause-manager-reads"}));
        self.until(
            "provider has stopped consuming the real manager socket",
            || (self.driver(json!({"action":"stats"}))["reads_paused"] == true).then_some(()),
        );
        // Smaller than the public 1MiB frame limit, larger than the fixture's
        // deliberately constrained Unix receive queue. No production test hook.
        let receipt = self.prompt(agent, "steer", &"B".repeat(768 * 1024), Some(turn));
        self.until(
            "first provider write registered before the blocked socket",
            || {
                let call = retained(&self.inspect(agent), &receipt);
                (call["state"] == "forwarded").then_some(())
            },
        );
        receipt
    }

    fn start(&self, agent: &Agent) -> String {
        let receipt = self.prompt(agent, "start", "explicit fixture turn", None);
        assert_receipt(&receipt, "agent-prompt");
        self.until("authoritative active turn", || {
            let state = self.inspect(agent);
            (state["lifecycle"]["activity"] == "running")
                .then(|| state["lifecycle"]["turn_id"].as_str().map(str::to_owned))
                .flatten()
        })
    }

    fn request(&self, agent: &Agent, envelope: Value, expected: Value) -> Value {
        let id = envelope["id"].clone();
        let item = envelope["params"]["itemId"].clone();
        self.driver(json!({"action":"request","message":envelope,"expected":expected}));
        self.until("exact pending request", || {
            self.inspect(agent)["pending"]
                .as_array()?
                .iter()
                .find(|pending| pending["request_id"] == id && pending["item_id"] == item)
                .cloned()
        })
    }

    fn resolve(&self, id: Value) {
        self.driver(json!({"action":"resolve","request_id":id,"session_id":SESSION}));
    }

    fn complete(&self, turn: &str, status: &str) {
        self.driver(json!({"action":"complete","turn_id":turn,"status":status}));
    }

    fn force(&self, agent: &Agent, controller: &Value) -> Value {
        let ownership =
            self.call(json!({"op":"state","terminal":agent.terminal}))["ownership"].clone();
        self.call(json!({"op":"ownership-claim","terminal":agent.terminal,"controller":controller,"mode":"force","observed":ownership["observed"]}))
    }

    fn close(&self, agent: &Agent) {
        self.call(json!({"op":"close","terminal":agent.terminal}));
        self.call(
            json!({"op":"wait","terminal":agent.terminal,"condition":"exited","timeout_ms":10000}),
        );
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        for terminal in self.terminals.borrow().iter() {
            if let Ok(mut connection) = Connection::connect(&self.socket) {
                let _ = connection.exchange(json!({"op":"close","terminal":terminal}));
            }
        }
        if let Ok(mut connection) = Connection::connect(&self.socket) {
            let _ = connection.exchange(json!({"op":"shutdown"}));
        }
        let deadline = Instant::now() + Duration::from_secs(4);
        while self.daemon.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(15));
        }
        // Only groups created by this fixture, never process-name/global kills.
        let mut pids = self.terminal_pids.borrow().clone();
        pids.push(self.daemon.id() as i32);
        for name in ["backend.pid", "frontend.pid"] {
            if let Ok(contents) = fs::read_to_string(self.root.join(name))
                && let Ok(pid) = contents.trim().parse::<i32>()
            {
                pids.push(pid);
            }
        }
        for pid in pids {
            unsafe {
                let group = libc::getpgid(pid);
                if group > 0 && group != libc::getpgrp() {
                    libc::kill(-group, libc::SIGKILL);
                }
            }
        }
        let _ = self.daemon.wait();
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct Agent {
    terminal: String,
    instance: String,
    controller: Value,
    control: Value,
}

impl Agent {
    fn target(&self) -> Value {
        json!({"terminal":self.terminal,"instance_id":self.instance,"session_id":SESSION})
    }

    fn request(&self, op: &str) -> Value {
        json!({"op":op,"target":self.target(),"control":self.control})
    }

    fn pending_request(&self, op: &str, pending: &Value) -> Value {
        let mut request = self.request(op);
        for field in ["turn_id", "item_id", "request_id"] {
            request[field] = pending[field].clone();
        }
        request
    }
}

fn assert_receipt(receipt: &Value, op: &str) {
    assert!(
        receipt["call_id"].as_str().is_some_and(|id| !id.is_empty()),
        "{receipt}"
    );
    assert_eq!(receipt["op"], op, "{receipt}");
    assert!(
        matches!(
            receipt["state"].as_str(),
            Some("queued" | "forwarded" | "applied")
        ),
        "{receipt}"
    );
    assert!(receipt["provider_ack"].is_boolean(), "{receipt}");
}

fn retained(state: &Value, receipt: &Value) -> Value {
    state["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["call_id"] == receipt["call_id"])
        .unwrap_or_else(|| panic!("missing retained receipt {receipt}: {state}"))
        .clone()
}

fn assert_public(state: &Value, agent: &Agent, root: &Path) {
    fn visit(value: &Value) {
        match value {
            Value::Object(map) => {
                assert!(!map.contains_key("ref"), "public capability leak: {value}");
                for child in map.values() {
                    visit(child);
                }
            }
            Value::Array(values) => {
                for child in values {
                    visit(child);
                }
            }
            _ => {}
        }
    }
    visit(state);
    let text = state.to_string();
    for capability in [&agent.control, &agent.controller] {
        let reference = capability["ref"].as_str().unwrap();
        assert!(!text.contains(reference), "private handle leaked: {state}");
    }
    assert!(
        !text.contains(root.to_str().unwrap()),
        "private provider root leaked: {state}"
    );
}

fn question(id: Value, turn: &str, item: &str) -> Value {
    json!({"id":id,"method":"item/tool/requestUserInput","params":{
        "threadId":SESSION,"turnId":turn,"itemId":item,"isBlocking":true,
        "questions":[
            {"id":"colors","header":"Colors","question":"Choose colors","isOther":false,"isSecret":false,"options":[{"label":"Blue","description":"cool"},{"label":"Green","description":"also cool"}]},
            {"id":"note","header":"Note","question":"Add a note","isOther":true,"isSecret":false,"options":null}
        ]
    }})
}

fn answers() -> Value {
    json!({"answers":{"colors":{"answers":["Blue","Green"]},"note":{"answers":["literal free text, not terminal keys"]}}})
}

fn approval(id: Value, turn: &str, item: &str, method: &str) -> Value {
    let mut request = json!({"id":id,"method":method,"params":{"threadId":SESSION,"turnId":turn,"itemId":item,"startedAtMs":1,"reason":"deterministic per-request fixture"}});
    if method == "item/commandExecution/requestApproval" {
        request["params"]["command"] = json!("printf fixture-only");
        request["params"]["cwd"] = json!("/fixture/workspace");
        request["params"]["kind"] = json!("command");
        request["params"]["availableDecisions"] = json!(["accept", "decline"]);
    } else if method == "item/fileChange/requestApproval" {
        request["params"]["grantRoot"] = Value::Null;
    } else if method == "item/permissions/requestApproval" {
        request["params"]["cwd"] = json!("/fixture/workspace");
        request["params"]["permissions"] = json!({"network":{"enabled":true}});
    }
    request
}

#[test]
fn native_first_initializes_before_discovery_without_bootstrap_turns() {
    let host = Host::new();
    let agent = host.open();
    let initial = host.inspect(&agent);
    assert!(initial["target"].is_null(), "{initial}");
    assert_eq!(initial["lifecycle"]["session"], "discovering");
    assert_eq!(initial["capabilities"]["prompt"], false);
    assert_eq!(initial["pending"], json!([]));
    let mut premature = agent.request("agent-prompt");
    premature["intent"] = json!("start");
    premature["text"] = json!("MUST_NOT_BOOTSTRAP");
    rejected(host.response(premature));
    assert!(host.mutations().is_empty());
    let ready = host.ready(&agent);
    assert_eq!(ready["target"], agent.target());
    assert_eq!(ready["lifecycle"]["activity"], "idle");
    assert!(ready["lifecycle"]["turn_id"].is_null());
    assert_eq!(ready["capabilities"]["prompt"], true);
    let messages = host.manager_inputs();
    assert!(!messages.iter().any(|message| message["method"] == "thread/start" || message["method"] == "turn/start"));
    assert_eq!(
        host.transcript()
            .iter()
            .filter(|row| row["peer"] == "native"
                && row["direction"] == "received"
                && row["message"]["method"] == "thread/start")
            .count(),
        1
    );
    assert_eq!(
        host.driver(json!({"action":"stats"}))["violations"],
        json!([])
    );
    assert_public(&ready, &agent, &host.root);
}

#[test]
fn first_real_turn_completion_is_recovered_when_native_root_requires_explicit_subscription() {
    let host = Host::new();
    fs::write(host.root.join("root-requires-resume"), b"").unwrap();
    let agent = host.open();
    host.ready(&agent);
    let receipt = host.prompt(&agent, "start", "first requested meaningful turn", None);
    let completed = host.until(
        "completion missing from initial native-root observer",
        || {
            let state = host.inspect(&agent);
            (state["lifecycle"]["activity"] == "idle"
                && state["turn_outcomes"]
                    .as_object()
                    .is_some_and(|turns| !turns.is_empty()))
            .then_some(state)
        },
    );
    let call = completed["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|call| call["call_id"] == receipt["call_id"])
        .unwrap();
    let turn = call["turn_id"].as_str().unwrap();
    assert_eq!(completed["turn_outcomes"][turn], "completed");
    assert!(completed["lifecycle"]["turn_id"].is_null());
    assert_eq!(completed["target"], agent.target());
    let mut stale = agent.request("agent-prompt");
    stale["intent"] = json!("steer");
    stale["expected_turn_id"] = json!(turn);
    stale["text"] = json!("must not resurrect the completed first turn");
    rejected(host.response(stale));
}

#[test]
fn auxiliary_title_preserves_native_turn_but_unexpected_root_invalidates_target() {
    let mut host = Host::new();
    let agent = host.open();
    host.ready(&agent);
    let directory = host.root.join("config/amux");
    let native_store = fs::read_dir(&directory)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.is_dir()
                && path
                    .extension()
                    .is_some_and(|extension| extension == "agents")
        })
        .expect("private provider-native store");
    let rollout = native_store
        .join("sessions")
        .join(format!("rollout-{SESSION}.jsonl"));
    fs::write(
        &rollout,
        format!(
            "{}\n",
            json!({"type":"session_meta","payload":{"id":SESSION}})
        ),
    )
    .unwrap();
    fs::set_permissions(&rollout, fs::Permissions::from_mode(0o600)).unwrap();
    host.call(json!({"op":"name","terminal":agent.terminal,"name":"Native recovery A"}));
    let journal = host.until("native A has durable reopening intent", || {
        fs::read_dir(&directory)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("agent-sessions-"))
            })
    });
    let records = || -> Value { serde_json::from_slice(&fs::read(&journal).unwrap()).unwrap() };
    let saved = host.until("native A is eligible before replacement", || {
        records()["records"]
            .as_array()?
            .iter()
            .find(|record| record["session_id"] == SESSION && record["blocked"].is_null())
            .cloned()
    });
    let turn = host.start(&agent);
    let mut title = json!({
        "id":"019a0c00-0000-7000-9000-000000000001",
        "sessionId":"019a0c00-0000-7000-9000-000000000001",
        "cliVersion":"0.160.0","createdAt":1,"updatedAt":1,
        "cwd":host.root.join("workspace"),"ephemeral":true,
        "modelProvider":"fixture","model":MODEL,"preview":"","projectId":null,
        "source":"vscode","threadSource":"thread_title","parentThreadId":null,
        "status":{"type":"idle"},"turns":[]
    });
    host.driver(json!({"action":"emit","message":{
        "method":"thread/started","params":{"thread":title}
    }}));
    host.complete(&turn, "completed");
    let completed = host.until(
        "bound turn completes despite native title generation",
        || {
            let state = host.inspect(&agent);
            assert_ne!(state["lifecycle"]["connection"], "lost", "{state}");
            (state["turn_outcomes"][&turn] == "completed").then_some(state)
        },
    );
    assert_eq!(completed["target"], agent.target());
    assert_eq!(completed["lifecycle"]["activity"], "idle");

    // A classification string alone must not conceal a new ordinary root.
    title["id"] = json!("019a0c00-0000-7000-9000-000000000002");
    title["ephemeral"] = json!(false);
    host.driver(json!({"action":"emit","message":{
        "method":"thread/started","params":{"thread":title}
    }}));
    let lost = host.until("unexpected root invalidates the existing target", || {
        let state = host.inspect(&agent);
        (state["lifecycle"]["connection"] == "lost").then_some(state)
    });
    assert_eq!(lost["problem"], "native-session-replaced");
    let mut request = agent.request("agent-prompt");
    request["intent"] = json!("start");
    request["text"] = json!("must not reach an unbound replacement");
    let before = host.mutations();
    rejected(host.response(request));
    assert_eq!(host.mutations(), before);
    let blocked = host.until(
        "replacement blocks the exact saved native conversation",
        || {
            records()["records"]
                .as_array()?
                .iter()
                .find(|record| record["id"] == saved["id"] && record["blocked"].is_string())
                .cloned()
        },
    );
    assert_eq!(
        blocked["session_id"], SESSION,
        "replacement B must not be guessed"
    );
    assert_eq!(blocked["session_file"], saved["session_file"]);
    let launches = host.log("launcher.log");
    let backing = fs::read(&rollout).unwrap();
    host.restart();
    host.until(
        "superseded native A is refused during cold reopening",
        || {
            host.call(json!({"op":"snapshot"}))["diagnostics"]
                .as_array()?
                .iter()
                .any(|diagnostic| {
                    diagnostic["kind"] == "recovery" && diagnostic["target"] == saved["id"]
                })
                .then_some(())
        },
    );
    assert_eq!(
        host.log("launcher.log"),
        launches,
        "cold reopening launched the superseded native conversation"
    );
    assert_eq!(fs::read(&rollout).unwrap(), backing);
    assert_eq!(
        host.call(json!({"op":"snapshot"}))["terminals"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "cold reopening created a managed native target for the blocked context"
    );
}

#[test]
fn managed_codex_uses_verified_executable_despite_relative_daemon_and_conflicting_source_paths() {
    let host = Host::with_source_path_conflict(true);
    let snapshot = host.call(json!({"op":"snapshot"}));
    let source = snapshot["terminals"][0]["id"].as_str().unwrap();
    host.call(json!({"op":"wait","terminal":source,"condition":"shell-ready","timeout_ms":10000}));
    let controller = host.controller("conflicting source PATH owner");
    let observed =
        host.call(json!({"op":"state","terminal":source}))["ownership"]["observed"].clone();
    let source_grant = host.call(json!({"op":"ownership-claim","terminal":source,
        "controller":controller,"mode":"force","observed":observed}));
    let selected = host.root.join("source-selected-codex");
    let probe = format!("command -v codex > '{}'", selected.display());
    host.call(
        json!({"op":"execute","terminal":source,"control":source_grant["control"],
        "source":probe,"queue":false}),
    );
    let conflict = host.root.join("workspace/bin/codex");
    host.until(
        "source shell actually selects its conflicting Codex",
        || {
            fs::read_to_string(&selected)
                .ok()
                .filter(|selected| selected.trim() == conflict.to_str().unwrap())
        },
    );
    host.call(json!({"op":"wait","terminal":source,"condition":"shell-ready","timeout_ms":10000}));

    let opened = host.call(
        json!({"op":"agent-open","source":source,"controller":controller,
        "model":MODEL,"cwd":host.root.join("workspace")}),
    );
    let agent = host.track_open(controller, opened);
    let ready = host.ready(&agent);
    assert_eq!(ready["target"], agent.target());
    assert_eq!(ready["lifecycle"]["backend"], "running");
    assert_eq!(ready["lifecycle"]["frontend"], "attached");
    let turn = host.start(&agent);
    host.request(
        &agent,
        question(json!("path-boundary"), &turn, "same-native-item"),
        answers(),
    );
    host.until(
        "verified native frontend observes the same live backend request",
        || {
            host.transcript().into_iter().find(|row| {
                row["peer"] == "native"
                    && row["direction"] == "sent"
                    && row["message"]["id"] == "path-boundary"
            })
        },
    );
    assert!(
        !host.root.join("conflicting-codex-ran").exists(),
        "backend or frontend executed the conflicting source-PATH binary"
    );
    assert_eq!(
        host.manager_inputs()
            .iter()
            .filter(|message| message["method"] == "turn/start")
            .count(),
        1
    );

    fs::remove_file(&selected).unwrap();
    host.call(
        json!({"op":"execute","terminal":source,"control":source_grant["control"],
        "source":probe,"queue":false}),
    );
    host.until("ordinary source shell PATH remains unchanged", || {
        fs::read_to_string(&selected)
            .ok()
            .filter(|selected| selected.trim() == conflict.to_str().unwrap())
    });
}

#[test]
fn persistent_agent_operations_preserve_external_and_native_original_connection_authority() {
    for role in ["external", "attachment"] {
        let host = Host::new();
        let mut connection = Connection::connect(&host.socket).unwrap();
        let created = success(
            connection
                .exchange(json!({"op":"controller-create",
            "role":role,"label":"persistent agent owner","persistent":true}))
                .unwrap(),
        );
        let controller = created["controller"].clone();
        let opened = success(
            connection
                .exchange(json!({"op":"agent-open",
            "controller":controller,"model":MODEL,"cwd":host.root.join("workspace"),
            "persistent":true}))
                .unwrap(),
        );
        let agent = host.track_open(controller, opened);
        host.ready(&agent);
        let inspect = json!({"op":"agent-inspect","terminal":agent.terminal,
            "instance_id":agent.instance,"persistent":true});
        assert_eq!(
            success(connection.exchange(inspect.clone()).unwrap())["target"],
            agent.target()
        );
        if role == "attachment" {
            let mut view = json!({"op":"control-handle","handle":agent.controller,
                "method":"view","view":{"serial":1,"terminal":agent.terminal,"focused":true},
                "persistent":true});
            success(connection.exchange(view.clone()).unwrap());
            view["view"]["serial"] = json!(2);
            let borrowed = host.response(view.clone());
            assert_eq!(
                borrowed["error"]["code"], "not-native-connection",
                "{borrowed}"
            );
            success(connection.exchange(view).unwrap());
        }

        let mut start = agent.request("agent-prompt");
        start["persistent"] = json!(true);
        start["intent"] = json!("start");
        start["text"] = json!("start only the explicitly requested persistent turn");
        let start_receipt = success(connection.exchange(start).unwrap());
        assert_receipt(&start_receipt, "agent-prompt");
        let active = host.until(
            "persistent start acknowledged with its exact live turn",
            || {
                let state = success(connection.exchange(inspect.clone()).unwrap());
                (state["lifecycle"]["activity"] == "running"
                    && retained(&state, &start_receipt)["state"] == "applied")
                    .then_some(state)
            },
        );
        let turn = active["lifecycle"]["turn_id"].as_str().unwrap();
        let mut steer = agent.request("agent-prompt");
        steer["persistent"] = json!(true);
        steer["intent"] = json!("steer");
        steer["expected_turn_id"] = json!(turn);
        steer["text"] = json!("steer the same persistent turn");
        let steer_receipt = success(connection.exchange(steer).unwrap());
        assert_receipt(&steer_receipt, "agent-prompt");
        let wire = host.wait_wire(|message| message["method"] == "turn/steer");
        assert_eq!(wire["params"]["threadId"], SESSION);
        assert_eq!(wire["params"]["expectedTurnId"], turn);

        let pending = host.request(
            &agent,
            question(json!(-77), turn, "persistent-question"),
            answers(),
        );
        let mut answer = agent.pending_request("agent-answer", &pending);
        answer["persistent"] = json!(true);
        answer["answers"] = answers()["answers"].clone();
        let answered = success(connection.exchange(answer).unwrap());
        assert_receipt(&answered, "agent-answer");
        let wire =
            host.wait_wire(|message| message["id"] == -77 && message.get("result").is_some());
        assert_eq!(wire, json!({"id":-77,"result":answers()}));
        host.resolve(json!(-77));
        host.until(
            "persistent answer remains unknown without attributable settlement",
            || {
                let state = success(connection.exchange(inspect.clone()).unwrap());
                (state["pending"].as_array()?.is_empty()
                    && retained(&state, &answered)["state"] == "unknown")
                    .then_some(state)
            },
        );

        let pending = host.request(
            &agent,
            approval(
                json!("persistent-permission"),
                turn,
                "persistent-command",
                "item/commandExecution/requestApproval",
            ),
            json!({"decision":"decline"}),
        );
        let mut permission = agent.pending_request("agent-permission", &pending);
        permission["persistent"] = json!(true);
        permission["decision"] = json!("deny");
        let denied = success(connection.exchange(permission).unwrap());
        assert_receipt(&denied, "agent-permission");
        let wire = host.wait_wire(|message| {
            message["id"] == "persistent-permission" && message.get("result").is_some()
        });
        assert_eq!(
            wire,
            json!({"id":"persistent-permission","result":{"decision":"decline"}})
        );
        host.resolve(json!("persistent-permission"));
        host.until(
            "persistent permission remains unknown without attributable settlement",
            || {
                let state = success(connection.exchange(inspect.clone()).unwrap());
                (state["pending"].as_array()?.is_empty()
                    && retained(&state, &denied)["state"] == "unknown")
                    .then_some(state)
            },
        );

        let mut interrupt = agent.request("agent-interrupt");
        interrupt["persistent"] = json!(true);
        interrupt["turn_id"] = json!(turn);
        let interrupted = success(connection.exchange(interrupt).unwrap());
        assert_receipt(&interrupted, "agent-interrupt");
        let wire = host.wait_wire(|message| message["method"] == "turn/interrupt");
        assert_eq!(wire["params"], json!({"threadId":SESSION,"turnId":turn}));
        host.driver(json!({"action":"ack","method":"turn/interrupt"}));
        let acked = host.until(
            "persistent interrupt acknowledgement is not completion",
            || {
                let state = success(connection.exchange(inspect.clone()).unwrap());
                (retained(&state, &interrupted)["provider_ack"] == true).then_some(state)
            },
        );
        assert_eq!(acked["lifecycle"]["turn_id"], turn);
        assert!(acked["lifecycle"]["turn_outcome"].is_null());
        host.complete(turn, "interrupted");
        let completed = host.until("persistent inspection retains exact completed turn", || {
            let state = success(connection.exchange(inspect.clone()).unwrap());
            (state["turn_outcomes"][turn] == "interrupted").then_some(state)
        });
        assert!(completed["lifecycle"]["turn_id"].is_null());
        assert_eq!(completed["target"], agent.target());

        let replacement = host.controller("explicit persistent authority replacement");
        host.force(&agent, &replacement);
        let before = host.mutations();
        let mut stale = agent.request("agent-prompt");
        stale["persistent"] = json!(true);
        stale["intent"] = json!("start");
        stale["text"] = json!("MUST_NOT_REUSE_STALE_PERSISTENT_AUTHORITY");
        rejected(connection.exchange(stale).unwrap());
        let after = success(connection.exchange(inspect).unwrap());
        assert_eq!(after["turn_outcomes"][turn], "interrupted");
        assert_eq!(after["target"], agent.target());
        assert_eq!(host.mutations(), before);
        assert_eq!(
            host.driver(json!({"action":"stats"}))["violations"],
            json!([])
        );
    }
}

#[test]
fn agent_framing_rejects_wrong_types_versions_unknown_fields_and_nested_framing() {
    let host = Host::new();
    let agent = host.open();
    host.ready(&agent);
    let turn = host.start(&agent);
    let mut steer = agent.request("agent-prompt");
    steer["intent"] = json!("steer");
    steer["expected_turn_id"] = json!(turn);
    steer["text"] = json!("valid continuation after invalid framing");
    let open = json!({"op":"agent-open","controller":agent.controller,
        "model":MODEL,"cwd":host.root.join("workspace")});
    let inspect =
        json!({"op":"agent-inspect","terminal":agent.terminal,"instance_id":agent.instance});
    let before = host.mutations();
    for request in [&open, &inspect, &steer] {
        for persistent in [
            Value::Null,
            json!(0),
            json!(1),
            json!("true"),
            json!([]),
            json!({}),
        ] {
            let mut invalid = request.clone();
            invalid["persistent"] = persistent;
            rejected(host.response(invalid));
        }
        for version in [
            Value::Null,
            json!(0),
            json!(2),
            json!(1.0),
            json!("1"),
            json!(true),
        ] {
            let mut invalid = request.clone();
            invalid["version"] = version;
            rejected(host.response(invalid));
        }
        let mut invalid = request.clone();
        invalid["persistent"] = json!(true);
        invalid["framing_passthrough"] = json!(true);
        rejected(host.response(invalid));
    }
    let mut unversioned = Connection::connect(&host.socket).unwrap();
    serde_json::to_writer(unversioned.0.get_mut(), &inspect).unwrap();
    unversioned.0.get_mut().write_all(b"\n").unwrap();
    unversioned.0.get_mut().flush().unwrap();
    rejected(unversioned.receive().unwrap());
    for field in ["target", "control"] {
        for framing in ["persistent", "version"] {
            let mut invalid = steer.clone();
            invalid["persistent"] = json!(true);
            invalid[field][framing] = if framing == "persistent" {
                json!(true)
            } else {
                json!(1)
            };
            rejected(host.response(invalid));
        }
    }
    for framing in ["persistent", "version"] {
        let mut invalid = open.clone();
        invalid["controller"][framing] = if framing == "persistent" {
            json!(true)
        } else {
            json!(1)
        };
        rejected(host.response(invalid));
    }
    let pending = host.request(
        &agent,
        question(json!("closed-answer"), &turn, "closed-answer-item"),
        answers(),
    );
    for framing in ["persistent", "version"] {
        let mut invalid = agent.pending_request("agent-answer", &pending);
        invalid["persistent"] = json!(true);
        invalid["answers"] = answers()["answers"].clone();
        invalid["answers"]["colors"][framing] = if framing == "persistent" {
            json!(true)
        } else {
            json!(1)
        };
        rejected(host.response(invalid));
    }
    let pending = host.request(
        &agent,
        approval(
            json!("closed-permission"),
            &turn,
            "closed-permission-item",
            "item/commandExecution/requestApproval",
        ),
        json!({"decision":"decline"}),
    );
    let mut invalid = agent.pending_request("agent-permission", &pending);
    invalid["persistent"] = json!(true);
    invalid["decision"] = json!("deny");
    invalid["scope"] = json!("session");
    rejected(host.response(invalid));
    assert_eq!(host.mutations(), before);

    let mut connection = Connection::connect(&host.socket).unwrap();
    let mut persistent_inspect = inspect.clone();
    persistent_inspect["persistent"] = json!(true);
    assert_eq!(
        success(connection.exchange(persistent_inspect).unwrap())["target"],
        agent.target()
    );
    let mut one_shot = inspect;
    one_shot["persistent"] = json!(false);
    assert_eq!(
        success(connection.exchange(one_shot).unwrap())["target"],
        agent.target()
    );
    let mut eof = String::new();
    assert_eq!(
        connection.0.read_line(&mut eof).unwrap(),
        0,
        "persistent:false did not close the original connection"
    );
    steer["persistent"] = json!(true);
    assert_receipt(&host.call(steer), "agent-prompt");
    host.wait_wire(|message| {
        message["method"] == "turn/steer" && message["params"]["expectedTurnId"] == turn
    });
}

#[test]
fn closed_requests_and_wrong_instance_session_control_never_forward() {
    let host = Host::new();
    let agent = host.open();
    host.ready(&agent);
    let before = host.mutations();
    for field in ["instance_id", "session_id", "terminal"] {
        let mut request = agent.request("agent-prompt");
        request["target"][field] = json!(format!("wrong-{field}"));
        request["intent"] = json!("start");
        request["text"] = json!("INVALID_TARGET_MARKER");
        rejected(host.response(request));
    }
    for extra in ["call_id", "provider_method", "expected_turn_id"] {
        let mut request = agent.request("agent-prompt");
        request["intent"] = json!("start");
        request["text"] = json!("CLOSED_SHAPE_MARKER");
        request[extra] = json!("not-allowed");
        rejected(host.response(request));
    }
    let mut wrong_kind = agent.request("agent-prompt");
    wrong_kind["control"] = agent.controller.clone();
    wrong_kind["intent"] = json!("start");
    wrong_kind["text"] = json!("WRONG_KIND_MARKER");
    rejected(host.response(wrong_kind));
    rejected(host.response(
        json!({"op":"agent-inspect","terminal":agent.terminal,"instance_id":"wrong-instance"}),
    ));
    assert_eq!(host.mutations(), before);
    assert!(host.inspect(&agent)["lifecycle"]["turn_id"].is_null());
}

#[test]
fn start_and_exact_turn_steer_never_queue_or_retarget_stale_intent() {
    let host = Host::new();
    let agent = host.open();
    host.ready(&agent);
    let turn = host.start(&agent);
    let before = host.mutations();
    let mut conflicting = agent.request("agent-prompt");
    conflicting["intent"] = json!("start");
    conflicting["text"] = json!("NO_SILENT_QUEUE");
    rejected(host.response(conflicting));
    let mut stale = agent.request("agent-prompt");
    stale["intent"] = json!("steer");
    stale["text"] = json!("NO_STALE_RETARGET");
    stale["expected_turn_id"] = json!("previous-turn");
    rejected(host.response(stale));
    assert_eq!(host.mutations(), before);
    let receipt = host.prompt(&agent, "steer", "explicit same-turn steering", Some(&turn));
    assert_receipt(&receipt, "agent-prompt");
    let wire = host.wait_wire(|message| message["method"] == "turn/steer");
    assert_eq!(wire["params"]["threadId"], SESSION);
    assert_eq!(wire["params"]["expectedTurnId"], turn);
    assert_eq!(
        wire["params"]["input"][0]["text"],
        "explicit same-turn steering"
    );
    host.complete(&turn, "completed");
    host.until("completed first turn", || {
        (host.inspect(&agent)["lifecycle"]["activity"] == "idle").then_some(())
    });
    let next = host.start(&agent);
    assert_ne!(next, turn);
    let before = host.mutations();
    let mut old = agent.request("agent-prompt");
    old["intent"] = json!("steer");
    old["text"] = json!("DO_NOT_STEER_REPLACEMENT");
    old["expected_turn_id"] = json!(turn);
    rejected(host.response(old));
    host.complete(&turn, "interrupted");
    host.request(
        &agent,
        question(json!("late-turn-barrier"), &next, "late-turn-barrier-item"),
        answers(),
    );
    assert_eq!(host.inspect(&agent)["lifecycle"]["turn_id"], next);
    assert_eq!(host.mutations(), before);
}

#[test]
fn questions_preserve_integer_string_identity_keyed_choices_and_free_text() {
    let host = Host::new();
    let agent = host.open();
    host.ready(&agent);
    let turn = host.start(&agent);
    let numeric = host.request(&agent, question(json!(7), &turn, "numeric-item"), answers());
    let textual = host.request(
        &agent,
        question(json!("7"), &turn, "string-item"),
        answers(),
    );
    assert_eq!(numeric["request_id"], 7);
    assert_eq!(textual["request_id"], "7");
    assert_eq!(numeric["questions"][0]["id"], "colors");
    assert_eq!(textual["questions"][1]["options"], Value::Null);
    let before = host.mutations();
    for bad in [
        json!({"wrong-question":{"answers":["Blue"]},"note":{"answers":["note"]}}),
        json!({"colors":{"answers":["Red"]},"note":{"answers":["note"]}}),
        json!({"colors":{"answers":[]},"note":{"answers":["note"]}}),
        json!({"colors":{"answers":["Blue","Blue"]},"note":{"answers":["note"]}}),
        json!({"colors":{"answers":["Blue"]}}),
    ] {
        let mut request = agent.pending_request("agent-answer", &numeric);
        request["answers"] = bad;
        rejected(host.response(request));
    }
    let mut wrong_type = agent.pending_request("agent-answer", &numeric);
    wrong_type["request_id"] = json!("7");
    wrong_type["answers"] = answers()["answers"].clone();
    rejected(host.response(wrong_type));
    for invalid_id in [json!(1.5), json!(u64::MAX), Value::Null, json!(true)] {
        let mut invalid = agent.pending_request("agent-answer", &numeric);
        invalid["request_id"] = invalid_id;
        invalid["answers"] = answers()["answers"].clone();
        rejected(host.response(invalid));
    }
    assert_eq!(host.mutations(), before);
    for pending in [&numeric, &textual] {
        let mut request = agent.pending_request("agent-answer", pending);
        request["answers"] = answers()["answers"].clone();
        let receipt = host.call(request);
        assert_receipt(&receipt, "agent-answer");
        let id = pending["request_id"].clone();
        let wire = host.wait_wire(|message| message["id"] == id && message.get("result").is_some());
        assert_eq!(wire, json!({"id":id,"result":answers()}));
        host.resolve(id.clone());
        host.until("resolved request removed independently", || {
            let state = host.inspect(&agent);
            (!state["pending"]
                .as_array()?
                .iter()
                .any(|pending| pending["request_id"] == id))
            .then_some(())
        });
        let recovered = host.until("ambiguous response not invented as winner", || {
            let call = retained(&host.inspect(&agent), &receipt);
            (call["state"] == "unknown").then_some(call)
        });
        assert_ne!(recovered["state"], "applied");
    }
    assert_eq!(
        host.driver(json!({"action":"stats"}))["violations"],
        json!([])
    );
}

#[test]
fn resolved_tombstones_do_not_reopen_on_duplicates_or_reused_request_ids() {
    let host = Host::new();
    let agent = host.open();
    host.ready(&agent);
    let turn = host.start(&agent);
    let envelope = question(json!(-9), &turn, "old-item");
    let pending = host.request(&agent, envelope.clone(), answers());
    host.resolve(json!(-9));
    host.until("native resolution tombstone", || {
        host.inspect(&agent)["pending"]
            .as_array()?
            .is_empty()
            .then_some(())
    });
    host.driver(json!({"action":"emit","message":envelope}));
    let mut stale = agent.pending_request("agent-answer", &pending);
    stale["answers"] = answers()["answers"].clone();
    rejected(host.response(stale.clone()));
    let before = host.mutations();
    let mut old_session = question(json!(-9), &turn, "old-session-item");
    old_session["params"]["threadId"] = json!("previous-session");
    host.driver(json!({"action":"emit","message":old_session}));
    host.complete(&turn, "completed");
    host.until("old turn ends", || {
        (host.inspect(&agent)["lifecycle"]["activity"] == "idle").then_some(())
    });
    let next = host.start(&agent);
    let later = host.request(&agent, question(json!(-9), &next, "new-item"), answers());
    rejected(host.response(stale));
    assert!(
        host.inspect(&agent)["pending"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["item_id"] != "old-item" && entry["session_id"] == SESSION)
    );
    let mut answer = agent.pending_request("agent-answer", &later);
    answer["answers"] = answers()["answers"].clone();
    let receipt = host.call(answer);
    assert_receipt(&receipt, "agent-answer");
    host.wait_wire(|message| message["id"] == -9 && message.get("result").is_some());
    assert_eq!(
        host.mutations().len(),
        before.len() + 2,
        "one new turn and one exact new answer only"
    );
    host.resolve(json!(-9));
    host.until(
        "reused request ID resolution cannot be attributed to a new request",
        || {
            let state = host.inspect(&agent);
            (state["pending"]
                .as_array()?
                .iter()
                .any(|entry| entry["item_id"] == "new-item" && entry["state"] == "unknown")
                && retained(&state, &receipt)["state"] == "unknown")
                .then_some(())
        },
    );
}

#[test]
fn command_and_file_permissions_encode_only_explicit_request_allow_or_deny() {
    let host = Host::new();
    let agent = host.open();
    host.ready(&agent);
    let turn = host.start(&agent);
    for (index, method, decision, encoded) in [
        (
            1,
            "item/commandExecution/requestApproval",
            "allow",
            "accept",
        ),
        (
            2,
            "item/commandExecution/requestApproval",
            "deny",
            "decline",
        ),
        (3, "item/fileChange/requestApproval", "allow", "accept"),
        (4, "item/fileChange/requestApproval", "deny", "decline"),
    ] {
        let id = if index == 1 {
            json!(i64::MAX)
        } else {
            json!(format!("permission-{index}"))
        };
        let mut approval = approval(id.clone(), &turn, &format!("item-{index}"), method);
        if index == 2 {
            // The real native menu omits decline, but the pinned response
            // protocol still supports denying this request without interruption.
            approval["params"]["availableDecisions"] = json!(["accept",
                {"acceptWithExecpolicyAmendment":{"execpolicy_amendment":["printf"]}},
                "cancel"]);
        }
        let pending = host.request(&agent, approval, json!({"decision":encoded}));
        assert_eq!(
            pending["kind"],
            if index < 3 { "command" } else { "file-change" }
        );
        let mut request = agent.pending_request("agent-permission", &pending);
        request["decision"] = json!(decision);
        let receipt = host.call(request);
        assert_receipt(&receipt, "agent-permission");
        let wire = host.wait_wire(|message| message["id"] == id && message.get("result").is_some());
        assert_eq!(wire, json!({"id":id,"result":{"decision":encoded}}));
        assert!(!wire.to_string().contains("Session"));
        host.resolve(id);
    }
    assert_eq!(
        host.driver(json!({"action":"stats"}))["violations"],
        json!([])
    );
    assert_eq!(host.inspect(&agent)["lifecycle"]["turn_id"], turn);
}

#[test]
fn unsupported_approval_scope_and_unavailable_decisions_never_auto_approve() {
    let host = Host::new();
    let agent = host.open();
    host.ready(&agent);
    let turn = host.start(&agent);
    let permissions = host.request(
        &agent,
        approval(
            json!(31),
            &turn,
            "turn-scoped-permissions",
            "item/permissions/requestApproval",
        ),
        Value::Null,
    );
    assert_eq!(permissions["kind"], "unsupported");
    let mut grant_root = approval(
        json!(32),
        &turn,
        "session-root",
        "item/fileChange/requestApproval",
    );
    grant_root["params"]["grantRoot"] = json!("/fixture/session-wide-root");
    let root = host.request(&agent, grant_root, json!({"decision":"decline"}));
    let mut limited = approval(
        json!(33),
        &turn,
        "deny-only",
        "item/commandExecution/requestApproval",
    );
    limited["params"]["availableDecisions"] = json!(["decline"]);
    let limited = host.request(&agent, limited, json!({"decision":"decline"}));
    let mut unsupported = question(json!(34), &turn, "unrecognized-question");
    unsupported["params"]["questions"][0]["multipleSelection"] = json!(true);
    let unsupported = host.request(&agent, unsupported, Value::Null);
    assert_eq!(unsupported["kind"], "unsupported");
    let before = host.mutations();
    for pending in [&permissions, &root, &limited] {
        let mut request = agent.pending_request("agent-permission", pending);
        request["decision"] = json!("allow");
        rejected(host.response(request));
    }
    for invalid in ["acceptForSession", "accept", "cancel"] {
        let mut request = agent.pending_request("agent-permission", &limited);
        request["decision"] = json!(invalid);
        rejected(host.response(request));
    }
    let mut answer = agent.pending_request("agent-answer", &unsupported);
    answer["answers"] = answers()["answers"].clone();
    rejected(host.response(answer));
    assert_eq!(host.mutations(), before);
    assert_eq!(host.inspect(&agent)["pending"].as_array().unwrap().len(), 4);
    let mut deny = agent.pending_request("agent-permission", &limited);
    deny["decision"] = json!("deny");
    assert_receipt(&host.call(deny), "agent-permission");
    host.wait_wire(|message| message["id"] == 33 && message["result"]["decision"] == "decline");
    let mut deny_root = agent.pending_request("agent-permission", &root);
    deny_root["decision"] = json!("deny");
    assert_receipt(&host.call(deny_root), "agent-permission");
    host.wait_wire(|message| message["id"] == 32 && message["result"]["decision"] == "decline");
}

#[test]
fn native_api_response_race_is_unknown_not_an_invented_api_winner() {
    let host = Host::new();
    let agent = host.open();
    host.ready(&agent);
    let turn = host.start(&agent);
    let pending = host.request(
        &agent,
        question(json!("race"), &turn, "raced-item"),
        answers(),
    );
    let mut answer = agent.pending_request("agent-answer", &pending);
    answer["answers"] = answers()["answers"].clone();
    let receipt = host.call(answer.clone());
    host.wait_wire(|message| message["id"] == "race" && message.get("result").is_some());
    // The provider has received the API contender but has not committed it.
    // A separate, initialized native WebSocket now supplies a different answer.
    let native =
        json!({"answers":{"colors":{"answers":["Green"]},"note":{"answers":["native contender"]}}});
    host.driver(json!({"action":"native-answer","request_id":"race","result":native}));
    host.until("native contender sent over its own WebSocket", || {
        host.transcript().into_iter().find(|row| {
            row["peer"] == "native"
                && row["direction"] == "received"
                && row["message"]["id"] == "race"
                && row["message"].get("result").is_some()
        })
    });
    let state = host.until("conservative ambiguous native/API resolution", || {
        let state = host.inspect(&agent);
        (state["pending"].as_array()?.is_empty()
            && retained(&state, &receipt)["state"] == "unknown")
            .then_some(state)
    });
    assert!(retained(&state, &receipt)["provider_ack"] != true);
    rejected(host.response(answer));
    let claimant = host.controller("safe transfer blocked by unresolved contender");
    let ownership = host.call(json!({"op":"state","terminal":agent.terminal}))["ownership"].clone();
    let claim = host.call(json!({"op":"ownership-claim","terminal":agent.terminal,"controller":claimant,"observed":ownership["observed"],"mode":"cooperative","when":{"kind":"now"}}));
    host.call(json!({"op":"control-handle","handle":agent.control,"method":"yield-to","claim_id":claim["claim_id"]}));
    let read = host.call(json!({"op":"control-handle","handle":claim["claim"],"method":"read"}));
    assert_ne!(
        read["phase"], "offered",
        "unknown contender must not become safe grant: {read}"
    );
    assert_ne!(read["phase"], "granted", "{read}");
    assert_public(&state, &agent, &host.root);
}

#[test]
fn interrupt_acknowledgement_completion_and_pending_cleanup_are_distinct() {
    let host = Host::new();
    let agent = host.open();
    host.ready(&agent);
    let turn = host.start(&agent);
    let pending = host.request(
        &agent,
        question(json!("interrupt-question"), &turn, "still-pending"),
        answers(),
    );
    host.driver(json!({"action":"hold","method":"turn/interrupt"}));
    let mut request = agent.request("agent-interrupt");
    request["turn_id"] = json!(turn);
    let receipt = host.call(request.clone());
    assert_receipt(&receipt, "agent-interrupt");
    let wire = host.wait_wire(|message| message["method"] == "turn/interrupt");
    assert_eq!(wire["params"], json!({"threadId":SESSION,"turnId":turn}));
    let before_ack = host.inspect(&agent);
    assert_eq!(retained(&before_ack, &receipt)["provider_ack"], false);
    assert_eq!(before_ack["lifecycle"]["turn_id"], turn);
    host.driver(json!({"action":"ack","method":"turn/interrupt"}));
    let acked = host.until("empty interrupt acknowledgement", || {
        let state = host.inspect(&agent);
        (retained(&state, &receipt)["provider_ack"] == true).then_some(state)
    });
    assert_eq!(acked["lifecycle"]["turn_id"], turn);
    assert!(acked["lifecycle"]["turn_outcome"].is_null());
    assert_eq!(acked["pending"][0]["item_id"], pending["item_id"]);
    host.complete(&turn, "interrupted");
    let completed = host.until("authoritative interrupted completion", || {
        let state = host.inspect(&agent);
        (state["lifecycle"]["turn_outcome"] == "interrupted").then_some(state)
    });
    assert_eq!(
        completed["pending"].as_array().unwrap().len(),
        1,
        "completion must not invent request cleanup"
    );
    assert_eq!(completed["lifecycle"]["backend"], "running");
    assert_ne!(completed["lifecycle"]["frontend"], "exited");
    let before = host.mutations();
    rejected(host.response(request));
    assert_eq!(host.mutations(), before);
    host.resolve(json!("interrupt-question"));
    host.until("later independent request cleanup", || {
        host.inspect(&agent)["pending"]
            .as_array()?
            .is_empty()
            .then_some(())
    });
    let next = host.start(&agent);
    let before = host.mutations();
    let mut stale = agent.request("agent-interrupt");
    stale["turn_id"] = json!(turn);
    rejected(host.response(stale));
    host.complete(&turn, "completed");
    host.request(
        &agent,
        question(
            json!("late-interrupt-barrier"),
            &next,
            "late-interrupt-barrier-item",
        ),
        answers(),
    );
    assert_eq!(host.inspect(&agent)["lifecycle"]["turn_id"], next);
    assert_eq!(host.mutations(), before);
}

#[test]
fn queued_revocation_rechecks_authority_after_real_socket_backpressure() {
    let host = Host::new();
    let agent = host.open();
    host.ready(&agent);
    let turn = host.start(&agent);
    let first = host.backpressure(&agent, &turn);
    let queued = host.prompt(
        &agent,
        "steer",
        "QUEUED_REVOKED_MUST_NOT_FORWARD",
        Some(&turn),
    );
    assert_eq!(
        retained(&host.inspect(&agent), &queued)["state"],
        "queued",
        "fixture must establish the actual queued, not already-forwarded, boundary"
    );
    let replacement = host.controller("force while provider writer blocked");
    let forced = host.force(&agent, &replacement);
    assert_eq!(forced["mode"], "forced");
    host.driver(json!({"action":"resume-manager-reads"}));
    host.wait_wire(|message| {
        message["method"] == "turn/steer"
            && message["params"]["input"][0]["text"]
                .as_str()
                .is_some_and(|text| text.len() == 768 * 1024)
    });
    let state = host.until("revoked queued authority rejected at dispatch", || {
        let state = host.inspect(&agent);
        (retained(&state, &queued)["state"] == "rejected"
            && retained(&state, &first)["provider_ack"] == true)
            .then_some(state)
    });
    assert!(
        !host
            .mutations()
            .iter()
            .any(|message| message["params"]["input"][0]["text"]
                == "QUEUED_REVOKED_MUST_NOT_FORWARD")
    );
    assert_eq!(state["lifecycle"]["turn_id"], turn);
    assert!(state["lifecycle"]["turn_outcome"].is_null());
    assert!(
        !host
            .mutations()
            .iter()
            .any(|message| message["method"] == "turn/interrupt")
    );
}

#[test]
fn native_resolution_before_queue_consumption_rejects_the_api_contender() {
    let host = Host::new();
    let agent = host.open();
    host.ready(&agent);
    let turn = host.start(&agent);
    let pending = host.request(
        &agent,
        question(json!("queued-native-race"), &turn, "queued-native-item"),
        answers(),
    );
    host.backpressure(&agent, &turn);
    let mut answer = agent.pending_request("agent-answer", &pending);
    answer["answers"] = answers()["answers"].clone();
    let queued = host.call(answer);
    assert_eq!(retained(&host.inspect(&agent), &queued)["state"], "queued");
    host.driver(json!({"action":"native-answer","request_id":"queued-native-race",
        "result":{"answers":{"colors":{"answers":["Green"]},"note":{"answers":["native won before dispatch"]}}}}));
    host.until(
        "native resolves over its independent provider connection",
        || {
            host.transcript().into_iter().find(|row| {
                row["peer"] == "native"
                    && row["direction"] == "received"
                    && row["message"]["id"] == "queued-native-race"
                    && row["message"].get("result").is_some()
            })
        },
    );
    host.driver(json!({"action":"resume-manager-reads"}));
    let state = host.until(
        "native resolution invalidates queued exact identity",
        || {
            let state = host.inspect(&agent);
            (retained(&state, &queued)["state"] == "rejected"
                && state["pending"].as_array()?.is_empty())
            .then_some(state)
        },
    );
    assert_eq!(retained(&state, &queued)["provider_ack"], false);
    assert!(!host.manager_inputs().iter().any(|message|
        message["id"] == "queued-native-race" && message.get("result").is_some()));
    assert_eq!(state["lifecycle"]["turn_id"], turn);
}

#[test]
fn dropped_public_reply_recovers_one_retained_call_without_replay() {
    let host = Host::new();
    let agent = host.open();
    host.ready(&agent);
    let mut connection = Connection::connect(&host.socket).unwrap();
    let mut request = agent.request("agent-prompt");
    request["intent"] = json!("start");
    request["text"] = json!("ONE_SHOT_LOST_REPLY");
    connection.send(request).unwrap();
    host.wait_wire(|message| {
        message["method"] == "turn/start"
            && message["params"]["input"][0]["text"] == "ONE_SHOT_LOST_REPLY"
    });
    drop(connection);
    let state = host.until("recover lost public receipt by inspection", || {
        let state = host.inspect(&agent);
        state["calls"]
            .as_array()?
            .iter()
            .any(|call| call["op"] == "agent-prompt" && call["turn_id"].is_string())
            .then_some(state)
    });
    let calls: Vec<_> = state["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|call| call["op"] == "agent-prompt")
        .collect();
    assert_eq!(calls.len(), 1, "{state}");
    assert_eq!(
        host.mutations()
            .iter()
            .filter(|message| message["method"] == "turn/start")
            .count(),
        1
    );
    assert_public(&state, &agent, &host.root);
}

#[test]
fn backend_loss_retains_unknown_calls_and_pending_without_restart_or_completion() {
    let host = Host::new();
    let agent = host.open();
    host.ready(&agent);
    let turn = host.start(&agent);
    host.request(
        &agent,
        question(json!(i64::MIN), &turn, "lost-backend-request"),
        answers(),
    );
    host.driver(json!({"action":"hold","method":"turn/interrupt"}));
    let mut request = agent.request("agent-interrupt");
    request["turn_id"] = json!(turn);
    let receipt = host.call(request);
    host.wait_wire(|message| message["method"] == "turn/interrupt");
    let backend_pid = host.log("backend.pid");
    host.driver(json!({"action":"backend-exit"}));
    let lost = host.until("backend loss is retained unknown", || {
        let state = host.inspect(&agent);
        (state["lifecycle"]["connection"] == "lost"
            && retained(&state, &receipt)["state"] == "unknown")
            .then_some(state)
    });
    assert_eq!(lost["resynchronization_required"], true);
    assert_eq!(lost["lifecycle"]["activity"], "unknown");
    assert!(lost["lifecycle"]["turn_outcome"].is_null());
    assert_eq!(lost["pending"].as_array().unwrap().len(), 1);
    assert_eq!(lost["pending"][0]["request_id"], i64::MIN);
    assert_eq!(lost["pending"][0]["state"], "unknown");
    for capability in ["prompt", "steer", "answer", "permission", "interrupt"] {
        assert_eq!(lost["capabilities"][capability], false);
    }
    let mut unsafe_request = agent.request("agent-prompt");
    unsafe_request["intent"] = json!("start");
    unsafe_request["text"] = json!("NO_RECONNECT_OR_REPLAY");
    rejected(host.response(unsafe_request));
    assert_eq!(host.log("backend.pid"), backend_pid);
    assert_eq!(
        host.log("launcher.log")
            .lines()
            .filter(|line| *line == "app-server")
            .count(),
        1
    );
}

#[test]
fn force_and_cancel_revoke_authority_without_interrupting_or_erasing_old_calls() {
    let host = Host::new();
    let agent = host.open();
    host.ready(&agent);
    let turn = host.start(&agent);
    host.driver(json!({"action":"hold","method":"turn/steer"}));
    let old = host.prompt(&agent, "steer", "forwarded before force", Some(&turn));
    host.wait_wire(|message| message["method"] == "turn/steer");
    let other = host.controller("replacement owner");
    let forced = host.force(&agent, &other);
    assert_eq!(forced["mode"], "forced");
    let before = host.mutations();
    let mut stale = agent.request("agent-interrupt");
    stale["turn_id"] = json!(turn);
    rejected(host.response(stale));
    let mut stale_prompt = agent.request("agent-prompt");
    stale_prompt["intent"] = json!("steer");
    stale_prompt["expected_turn_id"] = json!(turn);
    stale_prompt["text"] = json!("REVOKED_NO_FORWARD");
    rejected(host.response(stale_prompt));
    assert_eq!(host.mutations(), before);
    host.driver(json!({"action":"ack","method":"turn/steer"}));
    host.until("late old call retained after force", || {
        let call = retained(&host.inspect(&agent), &old);
        (call["provider_ack"] == true).then_some(call)
    });
    let state = host.call(json!({"op":"state","terminal":agent.terminal}));
    let claimant = host.controller("cancelled takeover");
    let claim = host.call(json!({"op":"ownership-claim","terminal":agent.terminal,"controller":claimant,"observed":state["ownership"]["observed"],"mode":"cooperative","when":{"kind":"now"}}));
    host.call(json!({"op":"control-handle","handle":claim["claim"],"method":"cancel"}));
    assert_eq!(host.mutations(), before);
    let live = host.inspect(&agent);
    assert_eq!(live["lifecycle"]["turn_id"], turn);
    assert_eq!(live["lifecycle"]["backend"], "running");
    assert!(live["lifecycle"]["turn_outcome"].is_null());
    assert!(
        !host
            .mutations()
            .iter()
            .any(|message| message["method"] == "turn/interrupt")
    );
}

#[test]
fn observer_attachments_detach_without_replacing_or_stopping_the_execution() {
    let host = Host::new();
    let agent = host.open();
    host.ready(&agent);
    let turn = host.start(&agent);
    let backend_pid = host.driver(json!({"action":"stats"}))["pid"].clone();
    let before = host.mutations();
    let mut viewers = Vec::new();
    let mut ids = Vec::new();
    for label in ["first observer", "second observer"] {
        let mut viewer = Connection::connect(&host.socket).unwrap();
        let created = success(
            viewer
                .exchange(json!({"op":"controller-create","role":"attachment",
            "label":label,"persistent":true}))
                .unwrap(),
        );
        ids.push(created["controller_id"].clone());
        success(viewer.exchange(json!({"op":"control-handle","handle":created["controller"],
            "method":"view","view":{"serial":1,"terminal":agent.terminal,"focused":true},"persistent":true})).unwrap());
        viewers.push(viewer);
    }
    drop(viewers);
    host.until("both original attachment connections are detached", || {
        let state = host.call(json!({"op":"state","terminal":agent.terminal}));
        let views = state["ownership"]["views"].as_array()?;
        ids.iter()
            .all(|id| {
                views
                    .iter()
                    .any(|view| view["controller_id"] == *id && view["connected"] == false)
            })
            .then_some(())
    });
    let state = host.inspect(&agent);
    assert_eq!(state["target"], agent.target());
    assert_eq!(state["lifecycle"]["turn_id"], turn);
    assert_eq!(state["lifecycle"]["backend"], "running");
    assert_eq!(state["lifecycle"]["frontend"], "attached");
    assert!(state["lifecycle"]["turn_outcome"].is_null());
    assert_eq!(host.driver(json!({"action":"stats"}))["pid"], backend_pid);
    assert_eq!(host.mutations(), before);
}

#[test]
fn public_event_gap_recovers_snapshot_without_replaying_agent_actions() {
    let host = Host::new();
    let agent = host.open();
    host.ready(&agent);
    let turn = host.start(&agent);
    let pending = host.request(
        &agent,
        question(json!("gap-question"), &turn, "gap-item"),
        answers(),
    );
    let cursor = host.call(json!({"op":"snapshot"}))["seq"].as_u64().unwrap();
    let before = host.mutations();
    let mut producer = Connection::connect(&host.socket).unwrap();
    for index in 0..2100 {
        success(
            producer
                .exchange(json!({"op":"name","terminal":agent.terminal,
            "name":format!("agent-gap-{index}"),"persistent":true}))
                .unwrap(),
        );
    }
    let events = host.call(json!({"op":"events","after":cursor,"timeout_ms":0}));
    assert_eq!(events["gap"], true, "{events}");
    assert!(
        events["snapshot"]["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|terminal| terminal["id"] == agent.terminal)
    );
    let recovered = host.inspect(&agent);
    assert_eq!(recovered["target"], agent.target());
    assert_eq!(recovered["lifecycle"]["turn_id"], turn);
    assert_eq!(recovered["pending"][0]["request_id"], pending["request_id"]);
    assert_eq!(recovered["pending"][0]["state"], "pending");
    assert_eq!(host.mutations(), before);
    assert_public(&recovered, &agent, &host.root);
    let public = events.to_string();
    for capability in [&agent.controller, &agent.control] {
        assert!(
            !public.contains(capability["ref"].as_str().unwrap()),
            "event-gap capability leak"
        );
    }
}

#[test]
fn frontend_exit_is_not_completion_and_explicit_close_removes_owned_backend() {
    let host = Host::new();
    let agent = host.open();
    host.ready(&agent);
    let turn = host.start(&agent);
    let stats = host.driver(json!({"action":"stats"}));
    let backend_socket = PathBuf::from(stats["listen"].as_str().unwrap());
    let backend_pid = stats["pid"].as_u64().unwrap() as i32;
    struct Reap(Child);
    impl Drop for Reap {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut unrelated = Reap(
        Command::new("/bin/sleep")
            .arg("120")
            .process_group(0)
            .spawn()
            .unwrap(),
    );
    let unrelated_pid = unrelated.0.id();
    assert!(unrelated.0.try_wait().unwrap().is_none());
    host.driver(json!({"action":"frontend-exit"}));
    let exited = host.until(
        "frontend exit truthfully distinct from turn completion",
        || {
            let state = host.inspect(&agent);
            (state["lifecycle"]["frontend"] == "exited").then_some(state)
        },
    );
    assert!(exited["lifecycle"]["turn_outcome"].is_null());
    assert_ne!(exited["lifecycle"]["activity"], "idle");
    assert_eq!(exited["lifecycle"]["turn_id"], turn);
    host.close(&agent);
    host.until("only owned backend process and socket cleaned", || {
        let dead = unsafe { libc::kill(backend_pid, 0) } == -1
            && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        (dead && !backend_socket.exists() && !backend_socket.parent().unwrap().exists())
            .then_some(())
    });
    assert_eq!(
        unsafe { libc::kill(unrelated_pid as i32, 0) },
        0,
        "cleanup killed unrelated process"
    );
    drop(unrelated);
}

#[test]
fn daemon_shutdown_reaps_fixture_children_and_removes_private_roots() {
    let mut host = Host::new();
    let agent = host.open();
    host.ready(&agent);
    host.start(&agent);
    let root = host.root.clone();
    let backend_socket = PathBuf::from(
        host.driver(json!({"action":"stats"}))["listen"]
            .as_str()
            .unwrap(),
    );
    let socket = host.socket.clone();
    let daemon_pid = host.daemon.id() as i32;
    let backend_pid: i32 = host.log("backend.pid").trim().parse().unwrap();
    let frontend_pid: i32 = host.log("frontend.pid").trim().parse().unwrap();
    host.call(json!({"op":"shutdown"}));
    let shutdown_deadline = Instant::now() + Duration::from_secs(3);
    let status = loop {
        if let Some(status) = host.daemon.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < shutdown_deadline,
            "daemon required fixture fallback cleanup"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success());
    let deadline = Instant::now() + DEADLINE;
    for pid in [daemon_pid, backend_pid, frontend_pid] {
        loop {
            if unsafe { libc::kill(pid, 0) } == -1
                && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "owned process {pid} survived cleanup"
            );
            std::thread::sleep(Duration::from_millis(15));
        }
    }
    assert!(!socket.exists(), "owned daemon socket survived shutdown");
    assert!(
        !backend_socket.parent().unwrap().exists(),
        "managed private root survived daemon exit"
    );
    drop(host);
    assert!(!root.exists(), "private fixture root survived cleanup");
}

// Installed codex-cli 0.160.0 shapes are deliberately concrete, not a generic RPC
// echo server. The fixture maintains native-created thread/turn/request state,
// checks semantic responses, and lets the test release protocol facts separately.
struct Provider {
    root: PathBuf,
    listen: PathBuf,
    transcript: Vec<Value>,
    violations: Vec<String>,
    subscribers: Vec<(bool, mpsc::Sender<Value>)>,
    loaded: bool,
    requires_resume: bool,
    manager_subscribed: bool,
    last_ended: Option<Value>,
    turn: Option<String>,
    turns: u64,
    requests: Vec<ProviderRequest>,
    held_methods: Vec<String>,
    deferred: VecDeque<(String, Value, mpsc::Sender<Value>)>,
    pause_manager_reads: bool,
    manager_reads_paused: bool,
}

struct ProviderRequest {
    envelope: Value,
    expected: Value,
    resolved: bool,
}

impl Provider {
    fn record(&mut self, native: bool, direction: &str, message: &Value) {
        let row = json!({"peer":if native {"native"} else {"manager"},"direction":direction,"message":message});
        let mut log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join("transcript.jsonl"))
            .unwrap();
        writeln!(log, "{row}").unwrap();
        self.transcript.push(row);
    }

    fn violation(&mut self, message: String) {
        self.violations.push(message);
    }

    fn publish(&self, message: Value) {
        for (native, tx) in &self.subscribers {
            if *native
                || self.manager_subscribed
                || matches!(
                    message["method"].as_str(),
                    Some("thread/started" | "thread/status/changed" | "thread/name/updated")
                )
            {
                let _ = tx.send(message.clone());
            }
        }
    }

    fn thread(&self) -> Value {
        json!({"id":SESSION,"sessionId":"native-session-tree","cliVersion":"0.160.0","createdAt":1,"updatedAt":1,
            "cwd":self.root.join("workspace"),"ephemeral":!self.requires_resume,"modelProvider":"fixture","model":MODEL,
            "preview":"","projectId":null,"source":"cli","canAcceptDirectInput":true,
            "status":if self.turn.is_some() { json!({"type":"active","activeFlags":[]}) } else { json!({"type":"idle"}) },
            "turns":self.last_ended.iter().cloned().chain(self.turn.iter().map(|id| turn_value(id, "inProgress"))).collect::<Vec<_>>()})
    }

    fn thread_response(&self) -> Value {
        json!({"thread":self.thread(),"model":MODEL,"modelProvider":"fixture","cwd":self.root.join("workspace"),
            "approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":"workspaceWrite","writableRoots":[self.root.join("workspace")],"networkAccess":false,"excludeTmpdirEnvVar":true,"excludeSlashTmp":true}})
    }

    fn response(&mut self, native: bool, message: &Value, tx: &mpsc::Sender<Value>) {
        let id = &message["id"];
        let method = message["method"].as_str().unwrap();
        let params = &message["params"];
        if matches!(method, "turn/start" | "turn/steer") && !valid_provider_input(&params["input"])
        {
            self.violation(format!("invalid installed text-input shape: {message}"));
            return;
        }
        let result = match method {
            "thread/loaded/list" => {
                json!({"data":if self.loaded {vec![SESSION]} else {Vec::<&str>::new()},"nextCursor":null})
            }
            "thread/start" if native && !self.loaded => {
                self.loaded = true;
                let result = self.thread_response();
                self.publish(json!({"method":"thread/started","params":{"thread":self.thread()}}));
                result
            }
            "thread/read" if self.loaded && params["threadId"] == SESSION => {
                let mut thread = self.thread();
                thread["turns"] = json!([]);
                json!({"thread":thread})
            }
            "thread/resume" if self.loaded && params["threadId"] == SESSION && self.turns > 0 => {
                self.manager_subscribed = true;
                let mut response = self.thread_response();
                let turns = response["thread"]["turns"].take();
                response["thread"]["turns"] = json!([]);
                response["initialTurnsPage"] =
                    json!({"data":turns,"nextCursor":null,"backwardsCursor":null});
                response
            }
            "turn/start"
                if !native
                    && self.loaded
                    && self.turn.is_none()
                    && params["threadId"] == SESSION =>
            {
                self.turns += 1;
                let turn = format!("019a0c00-0000-7000-8001-{:012}", self.turns);
                self.turn = Some(turn.clone());
                let result = json!({"turn":turn_value(&turn, "inProgress")});
                self.publish(json!({"method":"turn/started","params":{"threadId":SESSION,"turn":turn_value(&turn,"inProgress")}}));
                if self.requires_resume && !self.manager_subscribed {
                    let completed = turn_value(&turn, "completed");
                    self.turn = None;
                    self.last_ended = Some(completed.clone());
                    self.publish(json!({"method":"turn/completed","params":{"threadId":SESSION,"turn":completed}}));
                }
                result
            }
            "turn/steer"
                if !native
                    && self.turn.is_some()
                    && params["threadId"] == SESSION
                    && self.turn.as_deref() == params["expectedTurnId"].as_str() =>
            {
                json!({"turnId":self.turn})
            }
            "turn/interrupt"
                if !native
                    && self.turn.is_some()
                    && params["threadId"] == SESSION
                    && self.turn.as_deref() == params["turnId"].as_str() =>
            {
                json!({})
            }
            _ => {
                self.violation(format!("unexpected or stale provider request: {message}"));
                let _ = tx.send(json!({"id":id,"error":{"code":-32602,"message":"fixture rejected unsupported/stale request"}}));
                return;
            }
        };
        let response = json!({"id":id,"result":result});
        if self.held_methods.iter().any(|held| held == method) {
            self.deferred
                .push_back((method.to_owned(), response, tx.clone()));
        } else {
            let _ = tx.send(response);
        }
    }

    fn receive_response(&mut self, native: bool, message: &Value) {
        let Some(index) = self
            .requests
            .iter()
            .rposition(|pending| pending.envelope["id"] == message["id"] && !pending.resolved)
        else {
            self.violation(format!("response to absent/resolved request: {message}"));
            return;
        };
        if native {
            self.requests[index].resolved = true;
            self.publish(json!({"method":"serverRequest/resolved","params":{"threadId":SESSION,"requestId":message["id"]}}));
        } else if self.requests[index].expected != message["result"] {
            self.violation(format!(
                "incorrect concrete response, expected {}, got {message}",
                self.requests[index].expected
            ));
        }
        // Responses have no JSON-RPC acknowledgement. Merely receiving the API
        // contender does not certify which native/API response the provider used.
    }
}

fn valid_provider_input(input: &Value) -> bool {
    input.as_array().is_some_and(|items| {
        !items.is_empty()
            && items.iter().all(|item| {
                item["type"] == "text"
                    && item["text"].as_str().is_some_and(|text| !text.is_empty())
                    && item["text_elements"].is_array()
            })
    })
}

fn turn_value(id: &str, status: &str) -> Value {
    json!({"id":id,"items":[],"status":status,"error":null,"startedAt":1,"completedAt":if status == "inProgress" {Value::Null} else {json!(2)}})
}

fn endpoint() -> PathBuf {
    let endpoint = std::env::var("AMUX_CODEX_FIXTURE_ENDPOINT").unwrap();
    PathBuf::from(
        endpoint
            .strip_prefix("unix://")
            .expect("fixture requires real Unix WebSocket transport"),
    )
}

fn fixture_root() -> PathBuf {
    assert_eq!(
        std::env::var("AMUX_CODEX_FIXTURE_GUARD").as_deref(),
        Ok(HELPER_GUARD),
        "helper is not a standalone/live smoke test"
    );
    let root = PathBuf::from(std::env::var("AMUX_CODEX_FIXTURE_ROOT").unwrap());
    assert!(root.is_absolute());
    assert_eq!(
        fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert!(root.join("bin/codex").is_file());
    root
}

fn ws_send(socket: &mut WebSocket<UnixStream>, message: &Value) -> Result<(), tungstenite::Error> {
    socket.send(Message::Text(message.to_string().into()))
}

fn temporary_ws_error(error: &tungstenite::Error) -> bool {
    matches!(error, tungstenite::Error::Io(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut))
}

fn provider_connection(stream: UnixStream, state: Arc<Mutex<Provider>>) {
    stream.set_read_timeout(Some(DEADLINE)).unwrap();
    stream.set_write_timeout(Some(DEADLINE)).unwrap();
    let receive_buffer: libc::c_int = 4096;
    assert_eq!(
        unsafe {
            libc::setsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_RCVBUF,
                (&receive_buffer as *const libc::c_int).cast(),
                std::mem::size_of_val(&receive_buffer) as libc::socklen_t,
            )
        },
        0,
        "private fixture receive-buffer limit"
    );
    let mut socket = tungstenite::accept(stream).expect("real WebSocket Upgrade");
    let first = socket.read().expect("initialize must be first");
    let first: Value = serde_json::from_str(first.to_text().unwrap()).unwrap();
    let native = first["params"]["clientInfo"]["name"] == "amux-fixture-native";
    let (tx, rx) = mpsc::channel();
    {
        let mut state = state.lock();
        state.record(native, "received", &first);
        if first["method"] != "initialize"
            || first.get("id").is_none()
            || first["params"]["capabilities"]["experimentalApi"] != true
        {
            state.violation(format!("invalid initialize/order: {first}"));
            return;
        }
        let result = json!({"id":first["id"],"result":{"userAgent":"codex-cli/0.160.0 fixture","codexHome":state.root.join("codex"),"platformFamily":"unix","platformOs":std::env::consts::OS}});
        ws_send(&mut socket, &result).unwrap();
        state.record(native, "sent", &result);
    }
    let initialized = socket
        .read()
        .expect("initialized must follow initialize response");
    let initialized: Value = serde_json::from_str(initialized.to_text().unwrap()).unwrap();
    {
        let mut state = state.lock();
        state.record(native, "received", &initialized);
        if initialized["method"] != "initialized" || initialized.get("id").is_some() {
            state.violation(format!("missing initialized before request: {initialized}"));
            return;
        }
        state.subscribers.push((native, tx.clone()));
    }
    socket
        .get_mut()
        .set_read_timeout(Some(Duration::from_millis(15)))
        .unwrap();
    loop {
        while let Ok(message) = rx.try_recv() {
            if ws_send(&mut socket, &message).is_err() {
                return;
            }
            state.lock().record(native, "sent", &message);
        }
        let paused = {
            let mut state = state.lock();
            let paused = !native && state.pause_manager_reads;
            if !native {
                state.manager_reads_paused = paused;
            }
            paused
        };
        if paused {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                let message: Value = serde_json::from_str(text.as_str()).unwrap();
                let mut state = state.lock();
                state.record(native, "received", &message);
                if message.get("method").is_some() && message.get("id").is_some() {
                    state.response(native, &message, &tx);
                } else if message.get("id").is_some() && message.get("result").is_some() {
                    state.receive_response(native, &message);
                } else {
                    state.violation(format!("unexpected provider input: {message}"));
                }
            }
            Ok(Message::Close(_)) => return,
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {
                let _ = socket.flush();
            }
            Ok(other) => {
                state
                    .lock()
                    .violation(format!("unexpected frame: {other:?}"));
            }
            Err(error) if temporary_ws_error(&error) => {}
            Err(_) => return,
        }
    }
}

fn driver_connection(stream: UnixStream, state: &Arc<Mutex<Provider>>) {
    stream.set_read_timeout(Some(DEADLINE)).unwrap();
    stream.set_write_timeout(Some(DEADLINE)).unwrap();
    let mut stream = BufReader::new(stream);
    let mut line = String::new();
    stream.read_line(&mut line).unwrap();
    let command: Value = serde_json::from_str(&line).unwrap();
    let mut state = state.lock();
    let action = command["action"].as_str().unwrap();
    let result = match action {
        "stats" => {
            json!({"transcript":state.transcript,"violations":state.violations,"listen":state.listen,"pid":std::process::id(),"reads_paused":state.manager_reads_paused})
        }
        "pause-manager-reads" => {
            state.pause_manager_reads = true;
            json!({})
        }
        "resume-manager-reads" => {
            state.pause_manager_reads = false;
            json!({})
        }
        "release-native" => {
            fs::write(state.root.join("release-native"), b"release").unwrap();
            json!({})
        }
        "frontend-exit" => {
            fs::write(state.root.join("exit-frontend"), b"exit").unwrap();
            json!({})
        }
        "request" => {
            let envelope = command["message"].clone();
            state.requests.push(ProviderRequest {
                envelope: envelope.clone(),
                expected: command["expected"].clone(),
                resolved: false,
            });
            state.publish(envelope);
            json!({})
        }
        "emit" => {
            state.publish(command["message"].clone());
            json!({})
        }
        "resolve" => {
            for pending in &mut state.requests {
                if pending.envelope["id"] == command["request_id"] {
                    pending.resolved = true;
                }
            }
            state.publish(json!({"method":"serverRequest/resolved","params":{"threadId":command["session_id"],"requestId":command["request_id"]}}));
            json!({})
        }
        "native-answer" => {
            let temporary = state.root.join("native-answer.tmp");
            fs::write(
                &temporary,
                json!({"id":command["request_id"],"result":command["result"]}).to_string(),
            )
            .unwrap();
            fs::rename(temporary, state.root.join("native-answer.json")).unwrap();
            json!({})
        }
        "hold" => {
            state
                .held_methods
                .push(command["method"].as_str().unwrap().to_owned());
            json!({})
        }
        "ack" => {
            let method = command["method"].as_str().unwrap();
            state.held_methods.retain(|held| held != method);
            let mut retained = VecDeque::new();
            while let Some((held, message, tx)) = state.deferred.pop_front() {
                if held == method {
                    let _ = tx.send(message);
                } else {
                    retained.push_back((held, message, tx));
                }
            }
            state.deferred = retained;
            json!({})
        }
        "complete" => {
            let turn = command["turn_id"].as_str().unwrap();
            if state.turn.as_deref() == Some(turn) {
                state.turn = None;
            }
            state.publish(json!({"method":"turn/completed","params":{"threadId":SESSION,"turn":turn_value(turn,command["status"].as_str().unwrap())}}));
            json!({})
        }
        "backend-exit" => json!({}),
        _ => panic!("unknown fixture driver action: {command}"),
    };
    writeln!(stream.get_mut(), "{}", json!({"ok":true,"result":result})).unwrap();
    stream.get_mut().flush().unwrap();
    if action == "backend-exit" {
        std::process::exit(0);
    }
}

fn run_provider(root: PathBuf) {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(90));
        std::process::exit(124);
    });
    fs::write(root.join("backend.pid"), std::process::id().to_string()).unwrap();
    let listen = endpoint();
    assert!(
        listen.starts_with(&root),
        "provider socket escaped private fixture root"
    );
    let listener = UnixListener::bind(&listen).unwrap();
    let control = UnixListener::bind(root.join("fixture-control.sock")).unwrap();
    let requires_resume = root.join("root-requires-resume").exists();
    let state = Arc::new(Mutex::new(Provider {
        root,
        listen,
        transcript: Vec::new(),
        violations: Vec::new(),
        subscribers: Vec::new(),
        loaded: false,
        requires_resume,
        manager_subscribed: !requires_resume,
        last_ended: None,
        turn: None,
        turns: 0,
        requests: Vec::new(),
        held_methods: Vec::new(),
        deferred: VecDeque::new(),
        pause_manager_reads: false,
        manager_reads_paused: false,
    }));
    let driver_state = state.clone();
    std::thread::spawn(move || {
        for stream in control.incoming() {
            driver_connection(stream.unwrap(), &driver_state);
        }
    });
    for stream in listener.incoming() {
        let state = state.clone();
        std::thread::spawn(move || provider_connection(stream.unwrap(), state));
    }
}

fn run_native(root: PathBuf) {
    fs::write(root.join("frontend.pid"), std::process::id().to_string()).unwrap();
    let stream = UnixStream::connect(endpoint()).unwrap();
    stream.set_read_timeout(Some(DEADLINE)).unwrap();
    stream.set_write_timeout(Some(DEADLINE)).unwrap();
    let (mut socket, _) = tungstenite::client("ws://localhost/", stream).unwrap();
    ws_send(&mut socket, &json!({"id":"native-init","method":"initialize","params":{"clientInfo":{"name":"amux-fixture-native","version":"0.160.0"},"capabilities":{"experimentalApi":true}}})).unwrap();
    let response = socket.read().unwrap();
    let response: Value = serde_json::from_str(response.to_text().unwrap()).unwrap();
    assert_eq!(response["id"], "native-init");
    assert!(response.get("result").is_some());
    ws_send(&mut socket, &json!({"method":"initialized"})).unwrap();
    socket
        .get_mut()
        .set_read_timeout(Some(Duration::from_millis(15)))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut created = false;
    loop {
        assert!(Instant::now() < deadline, "owned native fixture watchdog");
        if root.join("exit-frontend").exists() {
            let _ = socket.close(None);
            return;
        }
        if !created && root.join("release-native").exists() {
            ws_send(&mut socket, &json!({"id":"native-thread","method":"thread/start","params":{"model":MODEL,"cwd":root.join("workspace"),"approvalPolicy":"on-request","sandbox":"workspace-write","ephemeral":true}})).unwrap();
            created = true;
        }
        if let Ok(answer) = fs::read_to_string(root.join("native-answer.json")) {
            let answer: Value = serde_json::from_str(&answer).unwrap();
            fs::remove_file(root.join("native-answer.json")).unwrap();
            ws_send(&mut socket, &answer).unwrap();
        }
        match socket.read() {
            Ok(Message::Close(_)) => return,
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {
                let _ = socket.flush();
            }
            Ok(_) => {}
            Err(error) if temporary_ws_error(&error) => {}
            Err(_) => return,
        }
    }
}

#[test]
#[ignore = "private guarded child process for deterministic Unix WebSocket fixtures, never a live Codex run"]
fn protocol_fixture_backend() {
    let root = fixture_root();
    match std::env::var("AMUX_CODEX_FIXTURE_MODE").as_deref() {
        Ok("backend") => run_provider(root),
        Ok("frontend") => run_native(root),
        other => panic!("invalid private fixture role: {other:?}"),
    }
}

#[test]
fn daemon_shutdown_joins_native_preparation_before_and_after_backend_spawn() {
    for phase in ["version", "backend"] {
        let mut host = Host::new();
        let gate = host.root.join(format!("pause-native-{phase}"));
        fs::write(&gate, b"paused").unwrap();
        let controller = host.controller("pending native preparation");
        let socket = host.socket.clone();
        let cwd = host.root.join("workspace");
        let (finished, response) = mpsc::sync_channel(1);
        let opening = std::thread::spawn(move || {
            let result = Connection::connect(&socket).and_then(|mut connection| {
                connection.exchange(json!({"op":"agent-open","controller":controller,
                    "model":MODEL,"cwd":cwd}))
            });
            let _ = finished.send(result);
        });
        let pid = host.until("owned native preparation reached its gated phase", || {
            fs::read_to_string(host.root.join("native-pending.pid"))
                .ok()?
                .trim()
                .parse::<i32>()
                .ok()
        });
        assert_eq!(
            unsafe { libc::kill(pid, 0) },
            0,
            "gated preparation already exited"
        );
        host.call(json!({"op":"shutdown"}));
        // The listener closes authority connections before joining preparation.
        // Keep the version probe gated until that real shutdown edge is observed.
        let reply = response
            .recv_timeout(DEADLINE)
            .expect("native caller did not disconnect during shutdown");
        assert!(
            !reply.as_ref().is_ok_and(|reply| reply["ok"] == true),
            "a gated native creation returned a new live target during shutdown"
        );
        if phase == "version" {
            fs::remove_file(&gate).unwrap();
        }
        opening.join().unwrap();
        let deadline = Instant::now() + DEADLINE;
        loop {
            if let Some(status) = host.daemon.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "native preparation made normal shutdown fail"
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "native startup ignored shutdown cancellation"
            );
            std::thread::sleep(Duration::from_millis(15));
        }
        assert_eq!(
            unsafe { libc::kill(pid, 0) },
            -1,
            "owned startup process survived daemon exit"
        );
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
        assert!(
            fs::read_dir(&host.root)
                .unwrap()
                .filter_map(Result::ok)
                .all(|entry| { !entry.file_name().to_string_lossy().starts_with("cx-") }),
            "unpublished native preparation left its private resource root behind"
        );
        if phase == "backend" {
            assert!(
                gate.exists(),
                "cleanup depended on unblocking the external executable"
            );
        }
    }
}

use serde_json::{Value, json};
use std::cell::RefCell;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(12);

#[path = "pty_control/native.rs"]
mod native;

// Deliberately use the public wire, not an ownership-aware client helper. A
// failed operation must never be hidden by automatic registration or reacquire.
struct Connection {
    reader: BufReader<UnixStream>,
}

impl Connection {
    fn connect(socket: &Path, timeout: Duration) -> io::Result<Self> {
        let stream = UnixStream::connect(socket)?;
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        Ok(Self {
            reader: BufReader::new(stream),
        })
    }

    fn send(&mut self, mut request: Value) -> io::Result<()> {
        request
            .as_object_mut()
            .unwrap()
            .entry("version")
            .or_insert(json!(1));
        serde_json::to_writer(self.reader.get_mut(), &request).map_err(io::Error::other)?;
        self.reader.get_mut().write_all(b"\n")?;
        self.reader.get_mut().flush()
    }

    fn exchange(&mut self, request: Value) -> io::Result<Value> {
        self.send(request)?;
        let mut response = String::new();
        (&mut self.reader)
            .take(8 * 1024 * 1024)
            .read_line(&mut response)?;
        if !response.ends_with('\n') {
            return Err(io::Error::other("incomplete API response"));
        }
        let response: Value = serde_json::from_str(&response).map_err(io::Error::other)?;
        assert_eq!(response["version"], 1, "{response}");
        Ok(response)
    }

    fn persistent(&mut self, mut request: Value) -> Value {
        request["persistent"] = json!(true);
        success(self.exchange(request).unwrap())
    }
}

fn wire(socket: &Path, request: Value, timeout: Duration) -> io::Result<Value> {
    Connection::connect(socket, timeout)?.exchange(request)
}

fn success(response: Value) -> Value {
    assert_eq!(response["ok"], true, "{response}");
    response["result"].clone()
}

fn denied(response: Value, codes: &[&str]) {
    assert_eq!(response["ok"], false, "unexpected grant: {response}");
    let code = response["error"]["code"].as_str().unwrap();
    assert!(codes.contains(&code), "expected {codes:?}, got {response}");
}

struct Host {
    root: PathBuf,
    socket: PathBuf,
    daemon: Child,
    terminal_pids: RefCell<Vec<i32>>,
}

impl Host {
    fn spawn(root: &Path, socket: &Path) -> Child {
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join("daemon.log"))
            .unwrap();
        Command::new(env!("CARGO_BIN_EXE_amux"))
            .arg("--socket")
            .arg(socket)
            .arg("daemon")
            .arg("--projects-dir")
            .arg(root.join("projects"))
            .env("HOME", root.join("home"))
            .env("ZDOTDIR", root.join("startup"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_RUNTIME_DIR", root.join("runtime"))
            .env("AMUX_RUNTIME_DIR", root.join("runtime"))
            .env("AMUX_SOCKET", socket)
            .env_remove("AMUX_TERMINAL")
            .env_remove("AMUX_INTEGRATION")
            .env_remove("AMUX_ORIGINAL_ZDOTDIR")
            .current_dir(root)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap()
    }

    fn new() -> Self {
        let root = fs::canonicalize("/tmp")
            .unwrap()
            .join(format!("amux-ownership-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["home", "startup", "config", "runtime", "projects"] {
            fs::create_dir(root.join(name)).unwrap();
            fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::write(root.join("startup/.zshrc"), "PS1='OWNERSHIP_NATIVE> '\n").unwrap();
        let socket = root.join("control.sock");
        let daemon = Self::spawn(&root, &socket);
        let host = Self {
            root,
            socket,
            daemon,
            terminal_pids: RefCell::new(Vec::new()),
        };
        host.until("private daemon startup", || {
            wire(
                &host.socket,
                json!({"op":"snapshot"}),
                Duration::from_secs(1),
            )
            .ok()
            .filter(|reply| reply["ok"] == true)
        });
        host.ready(&host.main());
        host
    }

    fn response(&self, request: Value) -> Value {
        wire(&self.socket, request, DEADLINE).unwrap()
    }

    fn call(&self, request: Value) -> Value {
        success(self.response(request))
    }

    fn until<T>(&self, label: &str, mut probe: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + DEADLINE;
        loop {
            if let Some(value) = probe() {
                return value;
            }
            assert!(
                Instant::now() < deadline,
                "{label}; daemon log: {}",
                fs::read_to_string(self.root.join("daemon.log")).unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn main(&self) -> String {
        self.until("initial terminal", || {
            self.call(json!({"op":"snapshot"}))["terminals"]
                .as_array()?
                .first()?["id"]
                .as_str()
                .map(str::to_owned)
        })
    }

    fn state(&self, terminal: &str) -> Value {
        let state = self.call(json!({"op":"state","terminal":terminal}));
        if let Some(pid) = state["pid"].as_i64() {
            let mut pids = self.terminal_pids.borrow_mut();
            if !pids.contains(&(pid as i32)) {
                pids.push(pid as i32);
            }
        }
        state
    }

    fn ownership(&self, terminal: &str) -> Value {
        self.state(terminal)["ownership"].clone()
    }

    fn ready(&self, terminal: &str) {
        self.call(json!({"op":"wait","terminal":terminal,
            "condition":"shell-ready","timeout_ms":10000}));
        self.state(terminal);
    }

    fn controller(&self, label: &str) -> Value {
        let created = self.call(json!({"op":"controller-create","role":"external","label":label}));
        assert_eq!(created["kind"], "controller-created");
        assert_eq!(created["controller"]["kind"], "controller");
        created
    }

    fn claim_request(&self, terminal: &str, controller: &Value, mode: &str) -> Value {
        let mut request = json!({"op":"ownership-claim","terminal":terminal,
            "controller":controller,"observed":self.ownership(terminal)["observed"],"mode":mode});
        if mode == "cooperative" {
            request["when"] = json!({"kind":"now"});
        }
        request
    }

    fn claim(&self, terminal: &str, controller: &Value) -> Value {
        self.call(self.claim_request(terminal, controller, "cooperative"))
    }

    fn acquire(&self, terminal: &str, controller: &Value) -> Value {
        let grant = self.claim(terminal, controller);
        assert_eq!(grant["kind"], "control", "{grant}");
        grant
    }

    fn handle(&self, handle: &Value, method: &str) -> Value {
        self.call(json!({"op":"control-handle","handle":handle,"method":method}))
    }

    fn yield_to(&self, control: &Value, claim: &Value) -> Value {
        self.call(
            json!({"op":"control-handle","handle":control,"method":"yield-to",
            "claim_id":claim["claim_id"]}),
        )
    }

    fn input(&self, terminal: &str, control: &Value, text: &str) -> Value {
        self.call(json!({"op":"input","terminal":terminal,"control":control,"text":text}))
    }

    fn execute(&self, terminal: &str, control: &Value, source: &str) -> Value {
        self.call(json!({"op":"execute","terminal":terminal,"control":control,"source":source}))
    }

    fn marker_source(&self, marker: &str) -> String {
        format!(
            "print -r -- {marker} > '{}'",
            self.root.join(marker).display()
        )
    }

    fn marker(&self, marker: &str) {
        self.until(marker, || {
            fs::read_to_string(self.root.join(marker))
                .ok()
                .filter(|text| text == &format!("{marker}\n"))
        });
    }

    fn events(&self, after: u64) -> Value {
        self.call(json!({"op":"events","after":after,"timeout_ms":0}))
    }

    fn seq(&self) -> u64 {
        self.call(json!({"op":"snapshot"}))["seq"].as_u64().unwrap()
    }

    fn native_calls(&self, terminal: &str) -> Value {
        self.until("retained actual native forwarding", || {
            let calls = self.ownership(terminal)["unsettled"].clone();
            calls
                .as_array()?
                .iter()
                .any(|call| call["kind"] == "native-input")
                .then_some(calls)
        })
    }

    fn assert_denied_marker(
        &self,
        terminal: &str,
        handle: Option<&Value>,
        marker: &str,
        codes: &[&str],
    ) {
        let mut request = json!({"op":"input","terminal":terminal,
            "text":format!("{}\r", self.marker_source(marker))});
        if let Some(handle) = handle {
            request["control"] = handle.clone();
        }
        denied(self.response(request), codes);
    }

    // Each invocation is a separate actual amux process; credentials are carried
    // explicitly in its JSON argument, never cached by this fixture.
    fn cli(&self, request: Value) -> Value {
        let tag = uuid::Uuid::new_v4().to_string();
        let output = self.root.join(format!("cli-{tag}.out"));
        let error = self.root.join(format!("cli-{tag}.err"));
        let mut child = Command::new(env!("CARGO_BIN_EXE_amux"))
            .arg("--socket")
            .arg(&self.socket)
            .arg("api")
            .arg(request.to_string())
            .env("HOME", self.root.join("home"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("AMUX_RUNTIME_DIR", self.root.join("runtime"))
            .current_dir(&self.root)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(fs::File::create(&output).unwrap())
            .stderr(fs::File::create(&error).unwrap())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + DEADLINE;
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.kill();
                let cleanup_deadline = Instant::now() + Duration::from_secs(3);
                while child.try_wait().ok().flatten().is_none() && Instant::now() < cleanup_deadline
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                panic!(
                    "raw CLI deadline: {}",
                    fs::read_to_string(error).unwrap_or_default()
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let stdout = fs::read_to_string(output).unwrap();
        let response: Value = serde_json::from_str(&stdout).unwrap_or_else(|decode_error| {
            panic!(
                "raw CLI JSON: {decode_error}; output {stdout}; stderr {}",
                fs::read_to_string(&error).unwrap_or_default()
            )
        });
        assert_eq!(response["version"], 1);
        assert_eq!(status.success(), response["ok"] == true, "{response}");
        response
    }

    fn stop(&mut self) {
        let _ = wire(
            &self.socket,
            json!({"op":"shutdown"}),
            Duration::from_secs(1),
        );
        let deadline = Instant::now() + Duration::from_secs(6);
        while self.daemon.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        if self.daemon.try_wait().ok().flatten().is_none() {
            for pid in self.terminal_pids.borrow().iter() {
                unsafe {
                    libc::kill(-*pid, libc::SIGKILL);
                }
            }
            unsafe {
                libc::kill(-(self.daemon.id() as i32), libc::SIGKILL);
            }
            let _ = self.daemon.kill();
        }
        let cleanup_deadline = Instant::now() + Duration::from_secs(3);
        while self.daemon.try_wait().ok().flatten().is_none() && Instant::now() < cleanup_deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if self.daemon.try_wait().ok().flatten().is_none() {
            if std::thread::panicking() {
                eprintln!("private daemon reap deadline");
            } else {
                panic!("private daemon reap deadline");
            }
        }
    }

    fn restart(&mut self) {
        self.stop();
        assert!(
            !self.socket.exists(),
            "owned daemon did not remove its socket"
        );
        self.terminal_pids.borrow_mut().clear();
        self.daemon = Self::spawn(&self.root, &self.socket);
        self.until("restarted private daemon", || {
            wire(
                &self.socket,
                json!({"op":"snapshot"}),
                Duration::from_secs(1),
            )
            .ok()
            .filter(|response| response["ok"] == true)
        });
        self.ready(&self.main());
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop();
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn stable_calls(calls: &Value) -> Vec<Value> {
    calls
        .as_array()
        .unwrap()
        .iter()
        .map(|call| {
            let mut call = call.clone();
            assert!(call["elapsed_ms"].is_u64(), "{call}");
            assert!(
                matches!(call["outcome"].as_str(), Some("pending" | "unknown")),
                "{call}"
            );
            call.as_object_mut().unwrap().remove("elapsed_ms");
            call
        })
        .collect()
}

fn no_private_refs(public: &Value, handles: &[&Value]) {
    fn visit(value: &Value) {
        match value {
            Value::Object(object) => {
                assert!(
                    !object.contains_key("ref"),
                    "public capability leak: {value}"
                );
                for child in object.values() {
                    visit(child);
                }
            }
            Value::Array(array) => {
                for child in array {
                    visit(child);
                }
            }
            _ => {}
        }
    }
    visit(public);
    let encoded = public.to_string();
    for handle in handles {
        let reference = handle["ref"].as_str().unwrap();
        assert!(
            !encoded.contains(reference),
            "public private-reference leak"
        );
    }
}

fn execution(state: &Value, id: &Value) -> Value {
    state["integration"]["executions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|execution| execution["execution_id"] == *id)
        .unwrap()
        .clone()
}

#[test]
fn empty_forwarded_ledger_requires_yield_reserved_offer_and_explicit_accept() {
    let h = Host::new();
    let terminal = h.main();
    let a = h.controller("A");
    let b = h.controller("B");
    let c = h.controller("C");
    let granted = h.acquire(&terminal, &a["controller"]);
    assert_eq!(granted["mode"], "safe");
    assert_eq!(granted["unsettled"], json!([]));
    let old = &granted["control"];
    let claim = h.claim(&terminal, &b["controller"]);
    assert_eq!(claim["phase"], "waiting-owner");
    assert_eq!(
        h.ownership(&terminal)["owner"]["controller_id"],
        a["controller_id"]
    );
    let receipt = h.ownership(&terminal)["observed"].clone();
    let read = h.handle(&claim["claim"], "read");
    assert_eq!(read["phase"], "waiting-owner");
    assert_eq!(h.ownership(&terminal)["observed"], receipt);
    denied(
        h.response(h.claim_request(&terminal, &c["controller"], "cooperative")),
        &["control-reserved"],
    );
    denied(
        h.response(json!({"op":"control-handle","handle":a["controller"],"method":"close"})),
        &["controller-busy"],
    );
    h.assert_denied_marker(&terminal, None, "MISSING_AUTH", &["invalid-request"]);
    h.assert_denied_marker(
        &terminal,
        Some(&b["controller"]),
        "CONTROLLER_NOT_CONTROL",
        &["wrong-handle-kind"],
    );
    h.assert_denied_marker(
        &terminal,
        Some(&claim["claim"]),
        "CLAIM_NOT_CONTROL",
        &["wrong-handle-kind"],
    );
    let forged = json!({"kind":"control","ref":claim["claim"]["ref"]});
    h.assert_denied_marker(
        &terminal,
        Some(&forged),
        "FORGED_KIND",
        &["wrong-handle-kind"],
    );
    let random = json!({"kind":"control","ref":uuid::Uuid::new_v4().to_string()});
    h.assert_denied_marker(&terminal, Some(&random), "UNMINTED_REF", &["stale-handle"]);
    denied(
        h.response(
            json!({"op":"execute","terminal":terminal,"control":claim["claim"],
        "source":h.marker_source("CLAIM_EXECUTE_DENIED")}),
        ),
        &["wrong-handle-kind"],
    );
    denied(h.response(json!({"op":"control-handle","handle":old,"method":"yield-to","claim_id":"not-this-claim"})),
        &["stale-observation", "claim-completed"]);
    assert_eq!(h.handle(old, "read")["control"], *old);
    assert_eq!(h.ownership(&terminal)["unsettled"], json!([]));
    assert_eq!(h.yield_to(old, &claim)["outcome"], "yielded");
    let offered = h.handle(&claim["claim"], "read");
    assert_eq!(offered["phase"], "offered", "{offered}");
    assert_eq!(h.handle(&claim["claim"], "read")["offer"], offered["offer"]);
    assert!(h.ownership(&terminal)["owner"].is_null());
    assert_eq!(
        h.ownership(&terminal)["reservation"]["claim_id"],
        claim["claim_id"]
    );
    h.assert_denied_marker(&terminal, Some(old), "YIELDED_INPUT", &["stale-handle"]);
    h.assert_denied_marker(
        &terminal,
        Some(&offered["offer"]),
        "OFFER_NOT_CONTROL",
        &["wrong-handle-kind"],
    );
    denied(
        h.response(h.claim_request(&terminal, &c["controller"], "cooperative")),
        &["control-reserved"],
    );
    let handles = [
        &a["controller"],
        &b["controller"],
        &c["controller"],
        old,
        &claim["claim"],
        &offered["offer"],
    ];
    no_private_refs(&h.call(json!({"op":"snapshot"})), &handles);
    no_private_refs(&h.events(0), &handles);
    let accepted = h.handle(&offered["offer"], "accept");
    assert_eq!(accepted["mode"], "safe");
    assert_eq!(accepted["unsettled"], json!([]));
    assert!(accepted["generation"].as_u64().unwrap() > granted["generation"].as_u64().unwrap());
    assert_ne!(accepted["control"], *old);
    // This is the first actual native input in this test. Denied markers would
    // have run before/with it if any earlier check accidentally forwarded bytes.
    h.input(
        &terminal,
        &accepted["control"],
        &format!("{}\r", h.marker_source("ACCEPTED_INPUT")),
    );
    h.marker("ACCEPTED_INPUT");
    h.ready(&terminal);
    for marker in [
        "MISSING_AUTH",
        "CONTROLLER_NOT_CONTROL",
        "CLAIM_NOT_CONTROL",
        "FORGED_KIND",
        "UNMINTED_REF",
        "CLAIM_EXECUTE_DENIED",
        "YIELDED_INPUT",
        "OFFER_NOT_CONTROL",
    ] {
        assert!(
            !h.root.join(marker).exists(),
            "denied input took effect: {marker}"
        );
    }
    denied(
        h.response(json!({"op":"control-handle","handle":offered["offer"],"method":"accept"})),
        &["offer-consumed"],
    );
    denied(
        h.response(json!({"op":"control-handle","handle":claim["claim"],"method":"cancel"})),
        &["claim-completed"],
    );
    assert_eq!(
        h.ownership(&terminal)["owner"]["controller_id"],
        b["controller_id"]
    );
    no_private_refs(&h.events(0), &[&accepted["control"]]);
}

#[test]
fn request_preserves_owner_input_but_native_delivery_never_becomes_safe_settlement() {
    let h = Host::new();
    let terminal = h.main();
    let a = h.controller("native-batch-owner");
    let b = h.controller("requester");
    let grant = h.acquire(&terminal, &a["controller"]);
    let claim = h.claim(&terminal, &b["controller"]);
    h.input(
        &terminal,
        &grant["control"],
        &format!("{}\r", h.marker_source("OWNER_AFTER_REQUEST")),
    );
    h.marker("OWNER_AFTER_REQUEST");
    h.ready(&terminal);
    let calls = h.native_calls(&terminal);
    assert_eq!(calls.as_array().unwrap().len(), 1);
    assert_eq!(calls[0]["controller_id"], a["controller_id"]);
    assert_eq!(calls[0]["generation"], grant["generation"]);
    h.input(
        &terminal,
        &grant["control"],
        &format!("{}\r", h.marker_source("SAME_NATIVE_BATCH")),
    );
    h.marker("SAME_NATIVE_BATCH");
    h.ready(&terminal);
    assert_eq!(
        stable_calls(&h.ownership(&terminal)["unsettled"]),
        stable_calls(&calls),
        "multiple native writes must retain the one real unknown batch, not invent settlement"
    );
    // A genuine separately accepted/completed shell source is not a native fence.
    h.execute(
        &terminal,
        &grant["control"],
        &h.marker_source("SEPARATE_SOURCE"),
    );
    h.marker("SEPARATE_SOURCE");
    h.ready(&terminal);
    assert_eq!(
        stable_calls(&h.ownership(&terminal)["unsettled"]),
        stable_calls(&calls)
    );
    h.yield_to(&grant["control"], &claim);
    let pending = h.handle(&claim["claim"], "read");
    assert_eq!(pending["phase"], "waiting-settlement", "{pending}");
    assert!(pending.get("offer").is_none());
    assert!(
        pending["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker["kind"] == "settlement"
                && blocker["call"]["id"] == calls[0]["id"])
    );
    assert!(h.ownership(&terminal)["owner"].is_null());
    assert_eq!(
        stable_calls(&pending["ownership"]["unsettled"]),
        stable_calls(&calls)
    );
    h.assert_denied_marker(
        &terminal,
        Some(&grant["control"]),
        "AFTER_YIELD_DENIED",
        &["stale-handle"],
    );
    let forced = h.call(
        json!({"op":"control-handle","handle":claim["claim"],"method":"force",
        "observed":h.ownership(&terminal)["observed"]}),
    );
    assert_eq!(forced["mode"], "forced");
    assert_eq!(forced["warning"]["code"], "prior-calls-may-apply");
    assert_eq!(forced["warning"]["calls"], forced["unsettled"]);
    assert_eq!(stable_calls(&forced["unsettled"]), stable_calls(&calls));
    h.execute(
        &terminal,
        &forced["control"],
        &h.marker_source("AFTER_FORCE_VALID"),
    );
    h.marker("AFTER_FORCE_VALID");
    assert!(!h.root.join("AFTER_YIELD_DENIED").exists());
}

#[test]
fn cancel_revokes_before_yield_grants_nobody_and_prior_owner_freshly_resumes_unknown_work() {
    let h = Host::new();
    let terminal = h.main();
    let a = h.controller("prior");
    let b = h.controller("cancelled-requester");
    let c = h.controller("unrelated");
    let old = h.acquire(&terminal, &a["controller"]);
    h.input(
        &terminal,
        &old["control"],
        &format!("{}\r", h.marker_source("BEFORE_CANCEL")),
    );
    h.marker("BEFORE_CANCEL");
    h.ready(&terminal);
    let calls = h.native_calls(&terminal);
    let claim = h.claim(&terminal, &b["controller"]);
    let cancelled = h.handle(&claim["claim"], "cancel");
    assert_eq!(cancelled["outcome"], "cancelled");
    let public = h.ownership(&terminal);
    assert!(public["owner"].is_null());
    assert!(public["reservation"].is_null());
    assert_eq!(stable_calls(&public["unsettled"]), stable_calls(&calls));
    assert_eq!(h.handle(&claim["claim"], "read")["phase"], "cancelled");
    assert_eq!(h.handle(&a["controller"], "read")["controls"], json!([]));
    assert_eq!(h.handle(&b["controller"], "read")["controls"], json!([]));
    h.assert_denied_marker(
        &terminal,
        Some(&old["control"]),
        "CANCELLED_OLD_TOKEN",
        &["stale-handle"],
    );
    h.assert_denied_marker(
        &terminal,
        Some(&claim["claim"]),
        "CANCELLED_REQUESTER",
        &["wrong-handle-kind"],
    );
    let unrelated = h.claim(&terminal, &c["controller"]);
    assert_eq!(unrelated["phase"], "waiting-settlement");
    assert!(unrelated["ownership"]["owner"].is_null());
    assert_eq!(
        stable_calls(&unrelated["ownership"]["unsettled"]),
        stable_calls(&calls)
    );
    h.assert_denied_marker(
        &terminal,
        Some(&unrelated["claim"]),
        "UNSETTLED_REQUESTER",
        &["wrong-handle-kind"],
    );
    h.handle(&unrelated["claim"], "cancel");
    let resumed = h.acquire(&terminal, &a["controller"]);
    assert_eq!(resumed["mode"], "resumed");
    assert_ne!(resumed["control"], old["control"]);
    assert!(resumed["generation"].as_u64().unwrap() > old["generation"].as_u64().unwrap());
    assert_eq!(stable_calls(&resumed["unsettled"]), stable_calls(&calls));
    h.execute(
        &terminal,
        &resumed["control"],
        &h.marker_source("FRESH_RESUMED_INPUT"),
    );
    h.marker("FRESH_RESUMED_INPUT");
    h.ready(&terminal);
    assert_eq!(
        stable_calls(&h.ownership(&terminal)["unsettled"]),
        stable_calls(&calls)
    );
    assert!(!h.root.join("CANCELLED_OLD_TOKEN").exists());
    assert!(!h.root.join("CANCELLED_REQUESTER").exists());
    denied(
        h.response(
            json!({"op":"control-handle","handle":claim["claim"],"method":"force",
        "observed":h.ownership(&terminal)["observed"]}),
        ),
        &["claim-completed"],
    );
    assert_eq!(
        h.handle(&resumed["control"], "read")["control"],
        resumed["control"]
    );
}

#[test]
fn cancel_first_invalidates_offer_and_accept_first_cannot_be_cancelled_or_replayed() {
    let h = Host::new();
    let terminal = h.main();
    let a = h.controller("first");
    let b = h.controller("cancel-first");
    let c = h.controller("accept-first");
    let first = h.acquire(&terminal, &a["controller"]);
    let cancelled = h.claim(&terminal, &b["controller"]);
    h.yield_to(&first["control"], &cancelled);
    let old_offer = h.handle(&cancelled["claim"], "read")["offer"].clone();
    h.handle(&cancelled["claim"], "cancel");
    assert!(h.ownership(&terminal)["owner"].is_null());
    assert!(h.ownership(&terminal)["reservation"].is_null());
    let next = h.acquire(&terminal, &a["controller"]);
    let claim = h.claim(&terminal, &c["controller"]);
    h.yield_to(&next["control"], &claim);
    let offer = h.handle(&claim["claim"], "read")["offer"].clone();
    let accepted = h.handle(&offer, "accept");
    denied(
        h.response(json!({"op":"control-handle","handle":old_offer,"method":"accept"})),
        &["stale-offer", "claim-completed"],
    );
    denied(
        h.response(json!({"op":"control-handle","handle":claim["claim"],"method":"cancel"})),
        &["claim-completed"],
    );
    denied(
        h.response(json!({"op":"control-handle","handle":offer,"method":"accept"})),
        &["offer-consumed"],
    );
    assert_eq!(
        h.ownership(&terminal)["owner"]["controller_id"],
        c["controller_id"]
    );
    h.execute(
        &terminal,
        &accepted["control"],
        &h.marker_source("ACCEPT_FIRST_STILL_OWNS"),
    );
    h.marker("ACCEPT_FIRST_STILL_OWNS");
    h.ready(&terminal);
    h.handle(&accepted["control"], "release");
    let history = h.handle(&claim["claim"], "read");
    assert_eq!(history["phase"], "granted");
    assert_eq!(history["current"], false);
    assert!(history.get("control").is_none());
    assert_eq!(history["grant_generation"], accepted["generation"]);
    assert_eq!(h.handle(&c["controller"], "read")["controls"], json!([]));
}

#[test]
fn force_supersedes_stranded_offer_using_only_public_receipt_and_warns_even_when_empty() {
    let h = Host::new();
    let terminal = h.main();
    let a = h.controller("owner");
    let b = h.controller("stranded");
    let c = h.controller("rescuer");
    let grant = h.acquire(&terminal, &a["controller"]);
    let claim = h.claim(&terminal, &b["controller"]);
    let before_yield = h.ownership(&terminal)["observed"].clone();
    h.yield_to(&grant["control"], &claim);
    let offer = h.handle(&claim["claim"], "read")["offer"].clone();
    let receipt = h.ownership(&terminal)["observed"].clone();
    let forced = h.call(json!({"op":"ownership-claim","controller":c["controller"],
        "terminal":terminal,"mode":"force","observed":receipt}));
    assert_eq!(forced["mode"], "forced");
    assert_eq!(
        forced["warning"],
        json!({"code":"prior-calls-may-apply","calls":[]})
    );
    assert_eq!(forced["unsettled"], json!([]));
    assert_eq!(h.handle(&claim["claim"], "read")["phase"], "superseded");
    denied(
        h.response(json!({"op":"control-handle","handle":offer,"method":"accept"})),
        &["stale-offer", "claim-completed"],
    );
    denied(
        h.response(json!({"op":"ownership-claim","controller":b["controller"],
        "terminal":terminal,"mode":"force","observed":before_yield})),
        &["stale-observation"],
    );
    denied(
        h.response(json!({"op":"ownership-claim","controller":a["controller"],
        "terminal":terminal,"mode":"force","observed":receipt})),
        &["stale-observation"],
    );
    let recovery = h.handle(&c["controller"], "read");
    assert_eq!(recovery["controls"][0]["control"], forced["control"]);
    assert_eq!(recovery["controls"][0]["mode"], "forced");
    h.execute(
        &terminal,
        &forced["control"],
        &h.marker_source("FORCE_WINNER"),
    );
    h.marker("FORCE_WINNER");
}

#[test]
fn force_retains_exact_unknown_calls_while_same_execution_keeps_running_and_finishes() {
    let h = Host::new();
    let terminal = h.main();
    let a = h.controller("running-owner");
    let b = h.controller("force-requester");
    let grant = h.acquire(&terminal, &a["controller"]);
    let gate = h.root.join("finish-running-source");
    let source = format!(
        "{}; while [[ ! -e '{}' ]]; do print -r -- RUNNING_OUTPUT; sleep 0.03; done; {}",
        h.marker_source("RUNNING_STARTED"),
        gate.display(),
        h.marker_source("RUNNING_COMPLETED")
    );
    let running = h.execute(&terminal, &grant["control"], &source);
    h.marker("RUNNING_STARTED");
    let pid = h.state(&terminal)["pid"].clone();
    // Ctrl-L is actual native input to the live PTY, not an invented settlement
    // report. Its application cannot be certified just because a write returned.
    h.input(&terminal, &grant["control"], "\u{c}");
    let calls = h.native_calls(&terminal);
    let claim = h.claim(&terminal, &b["controller"]);
    let receipt = h.ownership(&terminal)["observed"].clone();
    let screen_before = h.call(json!({"op":"screen","terminal":terminal}))["generation"].clone();
    let started = Instant::now();
    let forced = h.call(
        json!({"op":"control-handle","handle":claim["claim"],"method":"force","observed":receipt}),
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "force waited for running work"
    );
    assert_eq!(forced["mode"], "forced");
    assert_eq!(forced["warning"]["code"], "prior-calls-may-apply");
    assert_eq!(forced["warning"]["calls"], forced["unsettled"]);
    assert_eq!(stable_calls(&forced["unsettled"]), stable_calls(&calls));
    let state = h.state(&terminal);
    assert_eq!(state["pid"], pid);
    assert_eq!(state["alive"], true);
    assert_eq!(
        execution(&state, &running["execution_id"])["status"],
        "accepted"
    );
    assert!(!h.root.join("RUNNING_COMPLETED").exists());
    h.assert_denied_marker(
        &terminal,
        Some(&grant["control"]),
        "FORCED_OLD_INPUT",
        &["stale-handle"],
    );
    let generation = forced["generation"].clone();
    h.until("PTY output after force", || {
        let screen = h.call(json!({"op":"screen","terminal":terminal}));
        (screen["generation"] != screen_before
            && screen["lines"].to_string().contains("RUNNING_OUTPUT"))
        .then_some(())
    });
    fs::write(gate, "finish real work\n").unwrap();
    h.marker("RUNNING_COMPLETED");
    h.ready(&terminal);
    let state = h.state(&terminal);
    assert_eq!(state["pid"], pid);
    assert_eq!(
        execution(&state, &running["execution_id"])["status"],
        "completed"
    );
    assert_eq!(
        execution(&state, &running["execution_id"])["exit_status"],
        0
    );
    assert_eq!(
        state["ownership"]["owner"]["controller_id"],
        b["controller_id"]
    );
    assert_eq!(state["ownership"]["observed"]["generation"], generation);
    assert_eq!(
        stable_calls(&state["ownership"]["unsettled"]),
        stable_calls(&calls)
    );
    h.execute(
        &terminal,
        &forced["control"],
        &h.marker_source("FORCED_FRESH_SOURCE"),
    );
    h.marker("FORCED_FRESH_SOURCE");
    assert!(!h.root.join("FORCED_OLD_INPUT").exists());
    let events = h.events(0);
    let public_grant = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|event| event["kind"] == "control-granted" && event["terminal"] == terminal)
        .unwrap();
    assert_eq!(public_grant["data"]["mode"], "forced");
    assert_eq!(
        public_grant["data"]["ownership"]["owner"]["controller_id"],
        b["controller_id"]
    );
}

#[test]
fn independent_raw_cli_recovers_a_committed_accept_without_reading_or_replaying_its_reply() {
    let h = Host::new();
    let terminal = h.main();
    let a = success(h.cli(json!({"op":"controller-create","role":"external","label":"cli-owner"})));
    let b =
        success(h.cli(json!({"op":"controller-create","role":"external","label":"cli-requester"})));
    let grant = success(h.cli(h.claim_request(&terminal, &a["controller"], "cooperative")));
    let claim = success(h.cli(h.claim_request(&terminal, &b["controller"], "cooperative")));
    assert_eq!(claim["kind"], "claim-state");
    assert_eq!(claim["phase"], "waiting-owner");
    assert!(
        claim.get("control").is_none(),
        "exit zero must not mean granted"
    );
    let read =
        success(h.cli(json!({"op":"control-handle","handle":claim["claim"],"method":"read"})));
    assert_eq!(read["claim"], claim["claim"]);
    assert_eq!(read["phase"], "waiting-owner");
    let owner_private =
        success(h.cli(json!({"op":"control-handle","handle":a["controller"],"method":"read"})));
    let requester_private =
        success(h.cli(json!({"op":"control-handle","handle":b["controller"],"method":"read"})));
    assert!(
        !owner_private
            .to_string()
            .contains(claim["claim"]["ref"].as_str().unwrap())
    );
    assert!(
        !requester_private
            .to_string()
            .contains(grant["control"]["ref"].as_str().unwrap())
    );
    assert_eq!(requester_private["controls"], json!([]));
    assert_eq!(requester_private["claims"][0]["claim"], claim["claim"]);
    // A waiter disappearing has no ownership side effect. It never sends cancel.
    let mut waiter = Connection::connect(&h.socket, DEADLINE).unwrap();
    waiter
        .send(json!({"op":"events","after":h.seq(),"timeout_ms":1000}))
        .unwrap();
    drop(waiter);
    assert_eq!(h.handle(&claim["claim"], "read")["phase"], "waiting-owner");
    success(h.cli(
        json!({"op":"control-handle","handle":grant["control"],"method":"yield-to",
        "claim_id":claim["claim_id"]}),
    ));
    let offered =
        success(h.cli(json!({"op":"control-handle","handle":claim["claim"],"method":"read"})));
    assert_eq!(offered["phase"], "offered");
    assert_eq!(
        success(h.cli(json!({"op":"control-handle","handle":claim["claim"],"method":"read"})))["offer"],
        offered["offer"]
    );
    let mut lost_reply = Connection::connect(&h.socket, DEADLINE).unwrap();
    lost_reply
        .send(json!({"op":"control-handle","handle":offered["offer"],"method":"accept"}))
        .unwrap();
    // Observe commit using private recovery before dropping the unread response:
    // neither a sleep nor a second accept decides whether the first committed.
    let recovered = h.until("lost accept reply committed grant", || {
        let state =
            success(h.cli(json!({"op":"control-handle","handle":b["controller"],"method":"read"})));
        state["controls"].as_array()?.first().cloned()
    });
    drop(lost_reply);
    let current =
        success(h.cli(json!({"op":"control-handle","handle":claim["claim"],"method":"read"})));
    assert_eq!(current["phase"], "granted");
    assert_eq!(current["current"], true);
    assert_eq!(current["control"], recovered["control"]);
    assert_eq!(current["grant_generation"], recovered["generation"]);
    denied(
        h.cli(json!({"op":"control-handle","handle":offered["offer"],"method":"accept"})),
        &["offer-consumed"],
    );
    h.assert_denied_marker(
        &terminal,
        Some(&grant["control"]),
        "CLI_OLD_CONTROL",
        &["stale-handle"],
    );
    success(h.cli(
        json!({"op":"input","terminal":terminal,"control":recovered["control"],
        "text":format!("{}\r",h.marker_source("CLI_RECOVERED_INPUT"))}),
    ));
    h.marker("CLI_RECOVERED_INPUT");
    h.ready(&terminal);
    assert!(!h.root.join("CLI_OLD_CONTROL").exists());
    success(h.cli(json!({"op":"control-handle","handle":recovered["control"],"method":"release"})));
    let history =
        success(h.cli(json!({"op":"control-handle","handle":claim["claim"],"method":"read"})));
    assert_eq!(history["current"], false);
    assert!(history.get("control").is_none());
    assert_eq!(
        success(h.cli(json!({"op":"control-handle","handle":b["controller"],"method":"read"})))["controls"],
        json!([])
    );
}

#[test]
fn attachments_keep_original_connection_identity_view_serials_and_departure_without_release() {
    let h = Host::new();
    let terminal = h.main();
    denied(
        h.response(json!({"op":"controller-create","role":"attachment","label":"one-shot"})),
        &["invalid-request", "not-native-connection"],
    );
    let mut native = Connection::connect(&h.socket, DEADLINE).unwrap();
    let mut observer = Connection::connect(&h.socket, DEADLINE).unwrap();
    let a = native
        .persistent(json!({"op":"controller-create","role":"attachment","label":"same-label"}));
    let b = observer
        .persistent(json!({"op":"controller-create","role":"attachment","label":"same-label"}));
    assert_ne!(a["controller_id"], b["controller_id"]);
    let public = h.ownership(&terminal);
    let initial = public["views"]
        .as_array()
        .unwrap()
        .iter()
        .find(|view| view["controller_id"] == a["controller_id"])
        .unwrap();
    assert_eq!(initial["connected"], true);
    assert_eq!(initial["reported"], false);
    assert!(initial["focused"].is_null());
    native.persistent(
        json!({"op":"control-handle","handle":a["controller"],"method":"view",
        "view":{"serial":1,"terminal":null,"focused":null}}),
    );
    native.persistent(
        json!({"op":"control-handle","handle":a["controller"],"method":"view",
        "view":{"serial":2,"terminal":terminal,"focused":true}}),
    );
    observer.persistent(
        json!({"op":"control-handle","handle":b["controller"],"method":"view",
        "view":{"serial":1,"terminal":terminal,"focused":true}}),
    );
    let grant = native.persistent(h.claim_request(&terminal, &a["controller"], "cooperative"));
    let mut wrong_connection = json!({"op":"control-handle","handle":a["controller"],"method":"view",
        "view":{"serial":3,"terminal":null,"focused":false},"persistent":true});
    denied(
        observer.exchange(wrong_connection.clone()).unwrap(),
        &["not-native-connection"],
    );
    wrong_connection["view"]["serial"] = json!(1);
    denied(native.exchange(wrong_connection).unwrap(), &["stale-view"]);
    let before_borrow = h.ownership(&terminal)["activity_revision"].clone();
    success(h.cli(
        json!({"op":"input","terminal":terminal,"control":grant["control"],
        "text":format!("{}\r", h.marker_source("BORROWED_NATIVE_AUTHORITY"))}),
    ));
    h.marker("BORROWED_NATIVE_AUTHORITY");
    h.ready(&terminal);
    assert_eq!(
        h.ownership(&terminal)["activity_revision"],
        before_borrow,
        "a one-shot borrower must not claim original-connection human activity"
    );
    native.persistent(
        json!({"op":"input","terminal":terminal,"control":grant["control"],
        "text":h.marker_source("ORIGINAL_NATIVE_PASTE"),"paste":true}),
    );
    native.persistent(
        json!({"op":"input","terminal":terminal,"control":grant["control"],"key":"enter"}),
    );
    h.marker("ORIGINAL_NATIVE_PASTE");
    h.ready(&terminal);
    assert!(
        h.ownership(&terminal)["activity_revision"]
            .as_u64()
            .unwrap()
            > before_borrow.as_u64().unwrap()
    );
    assert_eq!(
        h.native_calls(&terminal).as_array().unwrap().len(),
        1,
        "borrowed input, native paste and Enter share one retained generation batch"
    );
    denied(
        h.response(json!({"op":"control-handle","handle":grant["control"],"method":"activity"})),
        &["not-native-connection"],
    );
    let public = h.ownership(&terminal);
    let view = public["views"]
        .as_array()
        .unwrap()
        .iter()
        .find(|view| view["controller_id"] == a["controller_id"])
        .unwrap();
    assert_eq!(view["serial"], 2);
    assert_eq!(view["terminal"], terminal);
    assert_eq!(view["focused"], true);
    drop(native);
    h.until("original attachment departure", || {
        let public = h.ownership(&terminal);
        public["views"]
            .as_array()?
            .iter()
            .any(|view| view["controller_id"] == a["controller_id"] && view["connected"] == false)
            .then_some(())
    });
    assert_eq!(
        h.ownership(&terminal)["owner"]["controller_id"],
        a["controller_id"]
    );
    let recovered =
        success(h.cli(json!({"op":"control-handle","handle":a["controller"],"method":"read"})));
    assert_eq!(recovered["controls"][0]["control"], grant["control"]);
    let forced = observer.persistent(h.claim_request(&terminal, &b["controller"], "force"));
    assert_eq!(forced["mode"], "forced");
    assert_eq!(forced["warning"]["calls"], forced["unsettled"]);
    assert_eq!(
        h.ownership(&terminal)["owner"]["controller_id"],
        b["controller_id"]
    );
    h.assert_denied_marker(
        &terminal,
        Some(&grant["control"]),
        "DEPARTED_STALE_AUTHORITY",
        &["stale-handle"],
    );
    observer.persistent(
        json!({"op":"execute","terminal":terminal,"control":forced["control"],
        "source":h.marker_source("OBSERVER_EXPLICITLY_FORCED")}),
    );
    h.marker("OBSERVER_EXPLICITLY_FORCED");
    assert!(!h.root.join("DEPARTED_STALE_AUTHORITY").exists());
    no_private_refs(
        &h.events(0),
        &[
            &a["controller"],
            &b["controller"],
            &grant["control"],
            &forced["control"],
        ],
    );
}

#[test]
fn three_second_progress_identifies_owner_and_native_settlement_without_timer_grant() {
    let h = Host::new();
    let terminal = h.main();
    let a = h.controller("blocking-owner");
    let b = h.controller("waiting-requester");
    let grant = h.acquire(&terminal, &a["controller"]);
    h.input(
        &terminal,
        &grant["control"],
        &format!("{}\r", h.marker_source("PROGRESS_NATIVE_BATCH")),
    );
    h.marker("PROGRESS_NATIVE_BATCH");
    h.ready(&terminal);
    let calls = h.native_calls(&terminal);
    let cursor = h.seq();
    let started = Instant::now();
    let claim = h.claim(&terminal, &b["controller"]);
    std::thread::sleep(Duration::from_millis(2400));
    let early = h.events(cursor);
    if started.elapsed() < Duration::from_secs(3) {
        assert!(
            !early["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| event["kind"] == "control-progress"
                    && event["data"]["claim_id"] == claim["claim_id"])
        );
    }
    let event = h.until(
        "three-second blocker progress without requester polling",
        || {
            h.events(cursor)["events"]
                .as_array()?
                .iter()
                .find(|event| {
                    event["kind"] == "control-progress"
                        && event["data"]["claim_id"] == claim["claim_id"]
                })
                .cloned()
        },
    );
    assert!(started.elapsed() >= Duration::from_secs(3));
    assert_eq!(event["terminal"], terminal);
    assert!(event["seq"].as_u64().unwrap() > cursor);
    let blockers = event["data"]["blockers"].as_array().unwrap();
    assert!(
        blockers
            .iter()
            .any(|blocker| blocker["kind"] == "owner"
                && blocker["elapsed_ms"].as_u64().unwrap() >= 3000),
        "{event}"
    );
    assert!(
        blockers
            .iter()
            .any(|blocker| blocker["kind"] == "settlement"
                && blocker["call"]["id"] == calls[0]["id"]),
        "{event}"
    );
    assert_eq!(
        event["data"]["ownership"]["owner"]["controller_id"],
        a["controller_id"]
    );
    assert_eq!(
        h.ownership(&terminal)["owner"]["controller_id"],
        a["controller_id"]
    );
    assert_eq!(h.handle(&claim["claim"], "read")["phase"], "waiting-owner");
    h.yield_to(&grant["control"], &claim);
    assert_eq!(
        h.handle(&claim["claim"], "read")["phase"],
        "waiting-settlement"
    );
    h.handle(&claim["claim"], "cancel");
    assert!(h.ownership(&terminal)["owner"].is_null());
    assert_eq!(
        stable_calls(&h.ownership(&terminal)["unsettled"]),
        stable_calls(&calls)
    );
}

#[test]
fn unread_event_observer_cannot_delay_cancel_force_or_live_pty_output() {
    let h = Host::new();
    let terminal = h.main();
    let a = h.controller("output-owner");
    let b = h.controller("cancel-requester");
    let c = h.controller("force-requester");
    let grant = h.acquire(&terminal, &a["controller"]);
    let gate = h.root.join("observer-output-finish");
    let source = format!(
        "{}; while [[ ! -e '{}' ]]; do print -r -- OBSERVER_OUTPUT; sleep 0.02; done; {}",
        h.marker_source("OBSERVER_OUTPUT_STARTED"),
        gate.display(),
        h.marker_source("OBSERVER_OUTPUT_FINISHED")
    );
    let running = h.execute(&terminal, &grant["control"], &source);
    h.marker("OBSERVER_OUTPUT_STARTED");
    let pid = h.state(&terminal)["pid"].clone();
    // Build real public event history and then leave several large replies
    // unread on a small receive buffer. No hook or event acknowledgement occurs.
    let mut producer = Connection::connect(&h.socket, DEADLINE).unwrap();
    for index in 0..384 {
        producer.persistent(json!({"op":"name","terminal":terminal,
            "name":format!("{index}-{}", "x".repeat(220))}));
    }
    let mut slow = Connection::connect(&h.socket, DEADLINE).unwrap();
    let receive_buffer: libc::c_int = 1024;
    assert_eq!(
        unsafe {
            libc::setsockopt(
                slow.reader.get_ref().as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_RCVBUF,
                (&receive_buffer as *const libc::c_int).cast(),
                std::mem::size_of_val(&receive_buffer) as libc::socklen_t,
            )
        },
        0
    );
    for _ in 0..16 {
        slow.send(json!({"op":"events","after":0,"timeout_ms":0,"persistent":true}))
            .unwrap();
    }
    h.until("observer has unread actual event bytes", || {
        let mut bytes: libc::c_int = 0;
        assert_eq!(
            unsafe {
                libc::ioctl(
                    slow.reader.get_ref().as_raw_fd(),
                    libc::FIONREAD,
                    &mut bytes,
                )
            },
            0
        );
        (bytes > 0).then_some(())
    });
    let claim = h.claim(&terminal, &b["controller"]);
    let started = Instant::now();
    h.handle(&claim["claim"], "cancel");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "cancel waited for observer"
    );
    assert!(h.ownership(&terminal)["owner"].is_null());
    let started = Instant::now();
    let forced = h.call(h.claim_request(&terminal, &c["controller"], "force"));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "force waited for observer"
    );
    assert_eq!(forced["mode"], "forced");
    assert_eq!(h.state(&terminal)["pid"], pid);
    assert_eq!(
        execution(&h.state(&terminal), &running["execution_id"])["status"],
        "accepted"
    );
    let before = h.call(json!({"op":"screen","terminal":terminal}))["generation"].clone();
    h.until("output independent of unread observer", || {
        let screen = h.call(json!({"op":"screen","terminal":terminal}));
        (screen["generation"] != before && screen["lines"].to_string().contains("OBSERVER_OUTPUT"))
            .then_some(())
    });
    fs::write(gate, "finish\n").unwrap();
    h.marker("OBSERVER_OUTPUT_FINISHED");
    h.ready(&terminal);
    assert_eq!(
        execution(&h.state(&terminal), &running["execution_id"])["exit_status"],
        0
    );
    assert_eq!(
        h.ownership(&terminal)["owner"]["controller_id"],
        c["controller_id"]
    );
    drop(slow);
}

#[test]
fn delayed_original_shell_wake_cannot_dispatch_after_cancel_and_fresh_owner() {
    let h = Host::new();
    let terminal = h.main();
    let a = h.controller("queued-owner");
    let b = h.controller("cancelling-requester");
    let c = h.controller("fresh-owner");
    let grant = h.acquire(&terminal, &a["controller"]);
    let wrapper = h.root.join("blocked-shell-helper");
    let wake_started = h.root.join("wake-started");
    let resume = h.root.join("resume-wake");
    let finished = h.root.join("wake-finished");
    fs::write(&wrapper, format!(
        "#!/bin/sh\nif [ \"$1\" = internal ] && [ \"$2\" = shell ] && [ \"$3\" = wake ]; then\n  : > '{}'\n  n=0\n  while [ ! -f '{}' ]; do\n    n=$((n + 1)); [ \"$n\" -lt 1000 ] || exit 124\n    sleep 0.01\n  done\n  '{}' \"$@\"\n  result=$?\n  : > '{}'\n  exit \"$result\"\nfi\nexec '{}' \"$@\"\n",
        wake_started.display(), resume.display(), env!("CARGO_BIN_EXE_amux"), finished.display(), env!("CARGO_BIN_EXE_amux"))).unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).unwrap();
    h.execute(
        &terminal,
        &grant["control"],
        &format!("export AMUX_EXECUTABLE='{}'", wrapper.display()),
    );
    h.ready(&terminal);
    let queued = h.call(
        json!({"op":"execute","terminal":terminal,"control":grant["control"],
        "queue":true,"source":h.marker_source("REVOKED_QUEUED_SOURCE")}),
    );
    assert_eq!(queued["status"], "queued");
    h.until("real shell wake held before daemon dispatch", || {
        wake_started.exists().then_some(())
    });
    assert!(!h.root.join("REVOKED_QUEUED_SOURCE").exists());
    let retained = h.ownership(&terminal)["unsettled"].clone();
    assert_eq!(retained.as_array().unwrap().len(), 1);
    assert_eq!(retained[0]["id"], queued["execution_id"]);
    assert_eq!(retained[0]["kind"], "shell-execute");
    assert_eq!(retained[0]["controller_id"], a["controller_id"]);
    assert_eq!(retained[0]["generation"], grant["generation"]);
    let claim = h.claim(&terminal, &b["controller"]);
    h.handle(&claim["claim"], "cancel");
    assert_eq!(
        stable_calls(&h.ownership(&terminal)["unsettled"]),
        stable_calls(&retained)
    );
    let forced = h.call(h.claim_request(&terminal, &c["controller"], "force"));
    assert_eq!(forced["warning"]["code"], "prior-calls-may-apply");
    assert_eq!(forced["warning"]["calls"], forced["unsettled"]);
    assert_eq!(stable_calls(&forced["unsettled"]), stable_calls(&retained));
    let winner_generation = forced["generation"].clone();
    let cursor = h.seq();
    fs::write(resume, "resume actual shell helper\n").unwrap();
    h.until("old helper returned its actual revoked outcome", || {
        finished.exists().then_some(())
    });
    h.until("real adapter rejected old-generation queued source", || {
        let state = h.state(&terminal);
        let old = execution(&state, &queued["execution_id"]);
        (old["status"] == "rejected" && old["reason"] == "stale-handle").then_some(())
    });
    assert_eq!(h.ownership(&terminal)["unsettled"], json!([]));
    assert_eq!(
        h.ownership(&terminal)["observed"]["generation"],
        winner_generation
    );
    let settlements = h.events(cursor);
    let rejected = settlements["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| {
            event["kind"] == "control-call-settled"
                && event["data"]["call_id"] == queued["execution_id"]
        })
        .unwrap();
    assert_eq!(rejected["data"]["outcome"], "rejected");
    assert_eq!(
        rejected["data"]["ownership"]["owner"]["controller_id"],
        c["controller_id"]
    );
    h.ready(&terminal);
    // A fresh accepted source proves the native adapter really consumed the
    // revoked continuation and stayed usable, rather than merely hiding bytes.
    h.execute(
        &terminal,
        &forced["control"],
        &h.marker_source("FRESH_AFTER_REVOKED_WAKE"),
    );
    h.marker("FRESH_AFTER_REVOKED_WAKE");
    h.ready(&terminal);
    assert!(!h.root.join("REVOKED_QUEUED_SOURCE").exists());
    assert_eq!(h.state(&terminal)["integration"]["shell"]["ready"], true);
    assert_eq!(
        h.ownership(&terminal)["owner"]["controller_id"],
        c["controller_id"]
    );
}

#[test]
fn capabilities_do_not_cross_terminal_lifetimes_or_daemon_restart() {
    let mut h = Host::new();
    let terminal = h.main();
    let owner = h.controller("lifetime-owner");
    let grant = h.acquire(&terminal, &owner["controller"]);
    let other = h.call(json!({"op":"open","source":terminal,"name":"other-lifetime"}))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    h.ready(&other);
    h.assert_denied_marker(
        &other,
        Some(&grant["control"]),
        "WRONG_TARGET_LIFETIME",
        &["controller-mismatch", "stale-handle"],
    );
    h.call(json!({"op":"close","terminal":terminal}));
    h.call(json!({"op":"wait","terminal":terminal,"condition":"exited","timeout_ms":10000}));
    h.assert_denied_marker(
        &terminal,
        Some(&grant["control"]),
        "ENDED_TARGET_INPUT",
        &["target-ended", "stale-handle"],
    );
    let replacement = h.call(json!({"op":"open","source":other,"name":"replacement"}))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    h.ready(&replacement);
    assert_ne!(replacement, terminal);
    h.assert_denied_marker(
        &replacement,
        Some(&grant["control"]),
        "REPLACEMENT_OLD_INPUT",
        &["controller-mismatch", "target-ended", "stale-handle"],
    );
    h.restart();
    let restarted = h.main();
    assert_ne!(restarted, terminal);
    denied(
        h.response(json!({"op":"control-handle","handle":owner["controller"],"method":"read"})),
        &["stale-handle"],
    );
    h.assert_denied_marker(
        &restarted,
        Some(&grant["control"]),
        "RESTART_OLD_INPUT",
        &["stale-handle"],
    );
    let fresh = h.controller("restart-owner");
    let fresh_grant = h.acquire(&restarted, &fresh["controller"]);
    h.input(
        &restarted,
        &fresh_grant["control"],
        &format!("{}\r", h.marker_source("RESTART_FRESH_INPUT")),
    );
    h.marker("RESTART_FRESH_INPUT");
    h.ready(&restarted);
    for marker in [
        "WRONG_TARGET_LIFETIME",
        "ENDED_TARGET_INPUT",
        "REPLACEMENT_OLD_INPUT",
        "RESTART_OLD_INPUT",
    ] {
        assert!(
            !h.root.join(marker).exists(),
            "old capability reached target: {marker}"
        );
    }
}

#[test]
fn event_gap_snapshot_recovers_public_reservation_but_only_private_read_recovers_offer() {
    let h = Host::new();
    let terminal = h.main();
    let a = h.controller("gap-owner");
    let b = h.controller("gap-requester");
    let granted = h.acquire(&terminal, &a["controller"]);
    let cursor = h.seq();
    let claim = h.claim(&terminal, &b["controller"]);
    h.yield_to(&granted["control"], &claim);
    let offered = h.handle(&claim["claim"], "read");
    assert_eq!(offered["phase"], "offered");
    let receipt = h.ownership(&terminal)["observed"].clone();
    // Exceed the real bounded event history with ordinary topology observations,
    // never fabricated source/application outcomes.
    let mut producer = Connection::connect(&h.socket, DEADLINE).unwrap();
    for index in 0..2100 {
        producer.persistent(json!({"op":"name","terminal":terminal,"name":format!("gap-{index}")}));
    }
    let events = h.events(cursor);
    assert_eq!(events["gap"], true);
    let snapshot = &events["snapshot"];
    let public = snapshot["terminals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == terminal)
        .unwrap()["ownership"]
        .clone();
    assert!(public["owner"].is_null());
    assert_eq!(
        public["observed"], receipt,
        "unrelated output/topology event sequence cannot change a compare receipt"
    );
    assert_eq!(public["reservation"]["claim_id"], claim["claim_id"]);
    assert_eq!(public["reservation"]["phase"], "offered");
    no_private_refs(
        &events,
        &[
            &a["controller"],
            &b["controller"],
            &granted["control"],
            &claim["claim"],
            &offered["offer"],
        ],
    );
    let recovered = h.handle(&claim["claim"], "read");
    assert_eq!(recovered["offer"], offered["offer"]);
    assert!(
        recovered.get("control").is_none(),
        "snapshot/read must not accept"
    );
    let accepted = h.handle(&recovered["offer"], "accept");
    assert_eq!(accepted["mode"], "safe");
    h.execute(
        &terminal,
        &accepted["control"],
        &h.marker_source("GAP_RECOVERED_ACCEPT"),
    );
    h.marker("GAP_RECOVERED_ACCEPT");
}

#[test]
fn removed_terminals_do_not_receive_later_native_view_events() {
    let h = Host::new();
    let terminal = h.main();
    let mut native = Connection::connect(&h.socket, DEADLINE).unwrap();
    let registration = native
        .persistent(json!({"op":"controller-create","role":"attachment","label":"churn observer"}));
    let mut removed = Vec::new();
    for _ in 0..8 {
        let id = h.call(json!({"op":"open","source":terminal}))["id"]
            .as_str()
            .unwrap()
            .to_owned();
        h.ready(&id);
        h.call(json!({"op":"close","terminal":id}));
        h.call(json!({"op":"wait","terminal":id,"condition":"exited","timeout_ms":10000}));
        h.call(json!({"op":"remove","terminal":id}));
        removed.push(id);
    }
    let cursor = h.seq();
    native.persistent(
        json!({"op":"control-handle","handle":registration["controller"],
        "method":"view","view":{"serial":1,"terminal":terminal,"focused":true}}),
    );
    let events = h.events(cursor);
    let views: Vec<_> = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "control-view-changed")
        .map(|event| event["terminal"].as_str().unwrap())
        .collect();
    assert!(
        views.iter().all(|id| !removed.iter().any(|old| old == id)),
        "removed terminals still consume view-event history: {views:?}"
    );
    assert_eq!(views, [terminal.as_str()]);
}

[Showing lines 1705-1743 of 1744. Use :1744 to continue]

// Delay a genuine native permission callback at the transport boundary. The
// original request survives termination of its sender; nothing is resubmitted.
struct ShellReportGate {
    captured: std::sync::mpsc::Receiver<Value>,
    release: std::sync::mpsc::Sender<()>,
    result: std::sync::mpsc::Receiver<Value>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl ShellReportGate {
    fn new(h: &Host) -> Self {
        use std::sync::{Arc, atomic::AtomicBool, mpsc};
        let path = h.root.join("shell-report.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let (capture_tx, captured) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let (result_tx, result) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let real = h.socket.clone();
        let worker = std::thread::spawn(move || {
            use std::sync::atomic::Ordering;
            let mut held = false;
            while !stopped.load(Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(_) => break,
                };
                stream.set_read_timeout(Some(DEADLINE)).unwrap();
                stream.set_write_timeout(Some(DEADLINE)).unwrap();
                let mut text = String::new();
                let read = BufReader::new(&stream)
                    .take(8 * 1024 * 1024)
                    .read_line(&mut text);
                if !matches!(read, Ok(n) if n > 0) {
                    continue;
                }
                let request: Value = serde_json::from_str(&text).unwrap();
                let hold = !held && request["event"]["kind"] == "execution-accepted";
                if hold {
                    held = true;
                    let _ = capture_tx.send(request.clone());
                    let _ = release_rx.recv_timeout(DEADLINE);
                }
                let response = wire(&real, request, DEADLINE).unwrap();
                if hold {
                    let _ = result_tx.send(response.clone());
                }
                // The native helper may already have been reaped after removal.
                if serde_json::to_writer(&mut stream, &response).is_ok() {
                    let _ = stream.write_all(b"\n");
                }
            }
        });
        fs::write(
            h.root.join("startup/.zshrc"),
            format!("PS1='OWNERSHIP_NATIVE> '\nexport AMUX_SOCKET='{}'\n", path.display()),
        ).unwrap();
        Self { captured, release, result, stop, worker: Some(worker) }
    }

    fn captured(&self, terminal: &str) -> Value {
        let request = self.captured.recv_timeout(DEADLINE).expect("genuine native acceptance");
        assert_eq!(request["terminal"], terminal);
        request
    }

    fn forward(&self) -> Value {
        self.release.send(()).unwrap();
        self.result.recv_timeout(DEADLINE).expect("original report response")
    }
}

impl Drop for ShellReportGate {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        let _ = self.release.send(());
        // Gate cancellation plus socket deadlines bound the worker on failure.
        if let Some(worker) = self.worker.take() {
            let result = worker.join();
            if !std::thread::panicking() {
                result.unwrap();
            }
        }
    }
}

fn unknown_startup(h: &Host, terminal: &str, execution: &Value) {
    h.until("actual startup acknowledgement deadline", || {
        h.ownership(terminal)["unsettled"].as_array()?.iter()
            .any(|call| call["id"] == *execution && call["outcome"] == "unknown")
            .then_some(())
    });
}

#[test]
fn late_automatic_startup_acceptance_retires_control_without_replay() {
    let h = Host::new();
    let gate = ShellReportGate::new(&h);
    let marker = h.root.join("launch-once");
    let terminal = h.call(json!({"op":"open","source":h.main(),
        "command":format!("print -r -- launch >> '{}'", marker.display())}))["id"]
        .as_str().unwrap().to_owned();
    let report = gate.captured(&terminal);
    let execution_id = &report["event"]["execution_id"];
    unknown_startup(&h, &terminal, execution_id);
    assert!(!h.ownership(&terminal)["owner"].is_null());
    success(gate.forward());
    let completed = h.until("real delayed command completion", || {
        let result = execution(&h.state(&terminal), execution_id);
        (result["status"] == "completed").then_some(result)
    });
    let ownership = h.ownership(&terminal);
    assert!(ownership["owner"].is_null(), "inaccessible launch owner: {ownership}; completion: {completed}");
    assert_eq!(completed["exit_status"], 0, "{completed}");
    assert_eq!(fs::read_to_string(marker).unwrap(), "launch\n");
    let next = h.controller("after late startup");
    assert_eq!(h.acquire(&terminal, &next["controller"])["mode"], "safe");
}

#[test]
fn removed_terminal_consumes_original_shell_settlement_without_retargeting() {
    let h = Host::new();
    let gate = ShellReportGate::new(&h);
    let terminal = h.call(json!({"op":"open","source":h.main(),
        "command":h.marker_source("REMOVED_SOURCE_MUST_NOT_RUN")}))["id"]
        .as_str().unwrap().to_owned();
    let report = gate.captured(&terminal);
    unknown_startup(&h, &terminal, &report["event"]["execution_id"]);
    let observer = h.controller("retained ended claim");
    let claim = h.claim(&terminal, &observer["controller"]);
    h.call(json!({"op":"close","terminal":terminal}));
    h.call(json!({"op":"wait","terminal":terminal,"condition":"exited","timeout_ms":10000}));
    h.call(json!({"op":"remove","terminal":terminal}));
    assert!(!h.root.join("runtime/integration").join(&terminal).exists());
    let before = h.handle(&claim["claim"], "read");
    assert_eq!(before["ownership"]["unsettled"][0]["id"], report["event"]["execution_id"], "{before}");
    // Reuse only the display name, never the old stable target identity.
    fs::write(h.root.join("startup/.zshrc"), "PS1='OWNERSHIP_NATIVE> '\n").unwrap();
    let replacement = h.call(json!({"op":"open","source":h.main(),"name":terminal}))["id"]
        .as_str().unwrap().to_owned();
    h.ready(&replacement);
    let replacement_before = h.state(&replacement);
    denied(gate.forward(), &["target-ended", "stale-handle"]);
    let after = h.handle(&claim["claim"], "read");
    assert_eq!(after["ownership"]["unsettled"], json!([]), "{after}");
    assert_ne!(after["phase"], "offered");
    assert!(after["offer"].is_null());
    assert_eq!(h.state(&replacement)["ownership"], replacement_before["ownership"]);
    assert!(!h.root.join("REMOVED_SOURCE_MUST_NOT_RUN").exists());
    // The settlement-only lookup is consumed, not retained as live integration.
    denied(h.response(report), &["integration-lost"]);
    h.handle(&claim["claim"], "close");
    h.handle(&observer["controller"], "close");
}

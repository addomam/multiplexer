//! No amux Rust imports. The daemon is controlled by its public CLI; expectations
//! come from the showcase's independent wire exercises and exported JSON, never amux's screen model.
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct Daemon {
    binary: PathBuf,
    socket: PathBuf,
    directory: PathBuf,
    child: Child,
    controller: Value,
    controls: RefCell<HashMap<String, Value>>,
}
impl Daemon {
    fn call(&self, mut value: Value) -> Option<Value> {
        if matches!(value["op"].as_str(), Some("input" | "execute")) {
            let target = value["terminal"].as_str().expect("mutation terminal");
            let control = self
                .controls
                .borrow()
                .get(target)
                .unwrap_or_else(|| panic!("showcase fixture must acquire control for {target}"))
                .clone();
            value["control"] = control;
        }
        let output = Command::new(&self.binary)
            .args(["--socket"])
            .arg(&self.socket)
            .arg("api")
            .arg(value.to_string())
            .output()
            .ok()?;
        let reply: Value = serde_json::from_slice(&output.stdout).ok()?;
        (output.status.success() && reply["ok"] == true).then(|| reply["result"].clone())
    }
    fn wait(&self, id: &str) {
        assert!(
            self.call(
                json!({"op":"wait","terminal":id,"condition":"shell-ready","timeout_ms":10000})
            )
            .is_some()
        );
    }
    fn acquire(&self, id: &str) {
        let deadline = Instant::now() + Duration::from_secs(15);
        let state = loop {
            let state = self.call(json!({"op":"state","terminal":id})).unwrap();
            if state["integration"]["shell"]["ready"] == true
                && state["ownership"]["owner"].is_null()
                && state["ownership"]["reservation"].is_null()
                && state["ownership"]["unsettled"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
            {
                break state;
            }
            assert!(
                Instant::now() < deadline,
                "shell readiness/automatic launch retirement did not complete: {state}"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        let grant = self
            .call(json!({"op":"ownership-claim","controller":self.controller,
            "terminal":id,"observed":state["ownership"]["observed"],
            "mode":"cooperative","when":{"kind":"now"}}))
            .unwrap();
        assert_eq!(
            grant["kind"], "control",
            "showcase requires unowned settled target: {grant}"
        );
        self.controls
            .borrow_mut()
            .insert(id.to_owned(), grant["control"].clone());
    }
}
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.call(json!({"op":"shutdown"}));
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.directory);
    }
}
fn quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

#[test]
#[ignore = "requires built amux binary: cargo build --workspace && cargo test -p terminal-showcase --test multiplexer -- --ignored"]
fn independent_showcase_runs_inside_amux_and_checks_real_cursor_replies() {
    let binary = std::env::var_os("AMUX_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_BIN_EXE_terminal-showcase")).with_file_name("amux")
        });
    assert!(binary.is_file(), "build amux first or set AMUX_BINARY");
    let directory = std::env::temp_dir().join(format!("showcase-amux-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let startup = directory.join("startup");
    fs::create_dir_all(&startup).unwrap();
    fs::write(startup.join(".zshrc"), "PS1='SHOWCASE_PROMPT> '\n").unwrap();
    let socket = directory.join("control.sock");
    let log = fs::File::create(directory.join("daemon.log")).unwrap();
    let child = Command::new(&binary)
        .arg("--socket")
        .arg(&socket)
        .args(["daemon", "--projects-dir"])
        .arg(directory.join("projects"))
        .env("XDG_CONFIG_HOME", directory.join("config"))
        .env("ZDOTDIR", startup)
        .current_dir(&directory)
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let mut daemon = Daemon {
        binary,
        socket,
        directory,
        child,
        controller: Value::Null,
        controls: RefCell::new(HashMap::new()),
    };
    let deadline = Instant::now() + Duration::from_secs(15);
    let snapshot = loop {
        if let Some(value) = daemon.call(json!({"op":"snapshot"})) {
            break value;
        }
        assert!(
            Instant::now() < deadline,
            "{}",
            fs::read_to_string(daemon.directory.join("daemon.log")).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let id = snapshot["terminals"][0]["id"].as_str().unwrap();
    daemon.controller = daemon
        .call(json!({"op":"controller-create","role":"external","label":"independent showcase"}))
        .unwrap()["controller"]
        .clone();
    daemon.wait(id);
    daemon.acquire(id);
    daemon
        .call(json!({"op":"resize","terminal":id,"cols":100,"rows":30}))
        .unwrap();
    let report = daemon.directory.join("probe.json");
    let exit_file = daemon.directory.join("probe.exit");
    let source = format!(
        "{} --probe --json --check --report {} > /dev/tty; print $? > {}",
        quote(Path::new(env!("CARGO_BIN_EXE_terminal-showcase"))),
        quote(&report),
        quote(&exit_file)
    );
    daemon
        .call(json!({"op":"execute","terminal":id,"source":source}))
        .unwrap();
    daemon.wait(id);
    let value: Value =
        serde_json::from_slice(&fs::read(&report).expect("showcase did not export probe report"))
            .unwrap();
    assert_eq!(value["report"]["columns"], 100);
    assert_eq!(value["report"]["rows"], 30);
    let exercises = value["exercises"].as_array().unwrap();
    for test in exercises {
        assert_eq!(
            test["outcome"], "passed",
            "independent endpoint regression: {test}"
        );
    }
    let origin = exercises
        .iter()
        .find(|v| v["capability"] == "origin")
        .unwrap();
    assert_eq!(
        origin["expected"],
        json!([2, 4]),
        "CPR coordinates must remain origin-relative; do not adopt amux's expectations"
    );
    assert_eq!(origin["actual"], json!([2, 4]));
    let graphemes = exercises
        .iter()
        .find(|v| v["capability"] == "graphemes")
        .unwrap();
    assert_eq!(graphemes["expected"], json!([7, 4]));
    assert_eq!(graphemes["actual"], json!([7, 4]));
    for capability in ["origin", "graphemes"] {
        let test = exercises
            .iter()
            .find(|v| v["capability"] == capability)
            .unwrap();
        let cap = value["report"]["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["id"] == capability)
            .unwrap();
        assert_eq!(test["outcome"], "passed");
        assert_eq!(cap["objective_status"], "verified");
    }
    assert_eq!(
        fs::read_to_string(exit_file).unwrap().trim(),
        "0",
        "strict probe must complete without objective endpoint failures"
    );
    let screenshot = daemon.directory.join("screen.json");
    let source = format!(
        "{} --screenshot --demo unicode --duration 0.4 --report {}",
        quote(Path::new(env!("CARGO_BIN_EXE_terminal-showcase"))),
        quote(&screenshot)
    );
    daemon
        .call(json!({"op":"execute","terminal":id,"source":source}))
        .unwrap();
    daemon.wait(id);
    let output: Value = serde_json::from_slice(&fs::read(&screenshot).unwrap()).unwrap();
    assert_eq!(output["report"]["columns"], 100);
    daemon.call(json!({"op":"execute","terminal":id,"source":"printf 'post-showcase ordinary shell works\\n'"})).unwrap();
    daemon.wait(id);
}

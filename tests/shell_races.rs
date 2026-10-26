use amux::client;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::HashMap;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Host {
    dir: PathBuf,
    socket: PathBuf,
    daemon: Child,
    controller: Value,
    controls: RefCell<HashMap<String, Value>>,
}
impl Host {
    fn new() -> Self {
        let dir = fs::canonicalize("/tmp")
            .unwrap()
            .join(format!("amux-shell-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        fs::create_dir(dir.join("startup")).unwrap();
        fs::create_dir(dir.join("home")).unwrap();
        fs::write(
            dir.join("startup/.zshrc"),
            "PS1='NATIVE_RACE> '\nexport STARTUP_ID=startup\n",
        )
        .unwrap();
        fs::write(
            dir.join("home/.zshrc"),
            "PS1='NATIVE_HOME> '\nexport STARTUP_ID=home\n",
        )
        .unwrap();
        let socket = dir.join("control.sock");
        let log = fs::File::create(dir.join("daemon.log")).unwrap();
        let daemon = Command::new(env!("CARGO_BIN_EXE_amux"))
            .args(["--socket"])
            .arg(&socket)
            .arg("daemon")
            .env("HOME", dir.join("home"))
            .env("ZDOTDIR", dir.join("startup"))
            .env("XDG_CONFIG_HOME", dir.join("config"))
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();
        let mut host = Self {
            dir,
            socket,
            daemon,
            controller: Value::Null,
            controls: RefCell::new(HashMap::new()),
        };
        host.until(|| host.call(json!({"op":"snapshot"})).is_ok());
        host.controller = host
            .call(json!({"op":"controller-create","role":"external","label":"shell race fixture"}))
            .unwrap()["controller"]
            .clone();
        host.ready(&host.main());
        host
    }
    fn call(&self, mut request: Value) -> anyhow::Result<Value> {
        if matches!(request["op"].as_str(), Some("input" | "execute")) {
            let terminal = request["terminal"].as_str().expect("mutation terminal");
            let control = self
                .controls
                .borrow()
                .get(terminal)
                .unwrap_or_else(|| {
                    panic!("fixture must acquire control before mutating {terminal}")
                })
                .clone();
            request["control"] = control;
        }
        client::request(&self.socket, request)
    }
    fn acquire(&self, id: &str) {
        if self.controls.borrow().contains_key(id) {
            return;
        }
        // Bootstrap readiness alone does not imply automatic-launch retirement.
        let mut observed = None;
        self.until(|| {
            let state = self.call(json!({"op":"state","terminal":id})).unwrap();
            let available = state["integration"]["shell"]["ready"] == true
                && state["ownership"]["owner"].is_null()
                && state["ownership"]["reservation"].is_null()
                && state["ownership"]["unsettled"]
                    .as_array()
                    .is_some_and(Vec::is_empty);
            if available {
                observed = Some(state);
            }
            available
        });
        let state = observed.unwrap();
        let grant = self
            .call(json!({"op":"ownership-claim","controller":self.controller,
            "terminal":id,"observed":state["ownership"]["observed"],
            "mode":"cooperative","when":{"kind":"now"}}))
            .unwrap();
        assert_eq!(
            grant["kind"], "control",
            "fixture requires an unowned settled target: {grant}"
        );
        self.controls
            .borrow_mut()
            .insert(id.to_owned(), grant["control"].clone());
    }
    fn main(&self) -> String {
        self.call(json!({"op":"snapshot"})).unwrap()["terminals"][0]["id"]
            .as_str()
            .unwrap()
            .into()
    }
    fn ready(&self, id: &str) {
        self.call(json!({"op":"wait","terminal":id,"condition":"shell-ready","timeout_ms":10000}))
            .unwrap();
        self.acquire(id);
    }
    fn execute(&self, id: &str, source: String) {
        self.call(json!({"op":"execute","terminal":id,"source":source}))
            .unwrap();
        self.ready(id);
    }
    fn until(&self, mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(12);
        while !condition() {
            assert!(
                Instant::now() < deadline,
                "{}",
                fs::read_to_string(self.dir.join("daemon.log")).unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.call(json!({"op":"shutdown"}));
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.daemon.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn queued_wake_cannot_follow_typeahead_into_foreground_application() {
    let h = Host::new();
    let main = h.main();
    let helper = h.dir.join("slow-helper");
    let wake = h.dir.join("wake-started");
    fs::write(&helper,format!("#!/bin/sh\nif [ \"$1\" = internal ] && [ \"$2\" = shell ] && [ \"$3\" = wake ]; then : > '{}'; sleep 0.25; fi\nexec '{}' \"$@\"\n",wake.display(),env!("CARGO_BIN_EXE_amux"))).unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();
    let application = h.dir.join("foreground.py");
    let captured = h.dir.join("foreground-input");
    let done = h.dir.join("queued-done");
    fs::write(&application,"import os,sys,time,select,termios,tty\nfd=0; old=termios.tcgetattr(fd); tty.setraw(fd,termios.TCSANOW)\ntry:\n with open(sys.argv[1],'wb') as f:\n  end=time.monotonic()+1\n  while time.monotonic()<end:\n   if select.select([fd],[],[],max(0,end-time.monotonic()))[0]:\n    f.write(os.read(fd,4096)); f.flush()\nfinally: termios.tcsetattr(fd,termios.TCSANOW,old)\n").unwrap();
    h.execute(
        &main,
        format!("export AMUX_EXECUTABLE='{}'", helper.display()),
    );
    h.call(json!({"op":"execute","terminal":main,"queue":true,"source":format!("print QUEUED_DONE > '{}'",done.display())})).unwrap();
    h.until(|| wake.exists());
    h.call(json!({"op":"input","terminal":main,"text":format!("python3 '{}' '{}'\r",application.display(),captured.display())})).unwrap();
    h.until(|| captured.exists());
    h.ready(&main);
    let bytes = fs::read(captured).unwrap();
    assert!(
        !bytes
            .windows(b"\x1b[amux-".len())
            .any(|window| window == b"\x1b[amux-"),
        "private wake leaked into foreground input: {bytes:?}"
    );
    // Redirection creates the file before the queued builtin writes its effect.
    // Existence alone cannot fence the subsequent content assertion.
    h.until(|| fs::read_to_string(&done).is_ok_and(|text| text == "QUEUED_DONE\n"));
}

#[test]
fn captured_zdotdir_changes_and_removals_select_actual_child_startup() {
    let h = Host::new();
    let main = h.main();
    let alternate = h.dir.join("alternate");
    fs::create_dir(&alternate).unwrap();
    fs::write(
        alternate.join(".zshrc"),
        "PS1='ALTERNATE> '\nexport STARTUP_ID=alternate\n",
    )
    .unwrap();
    h.execute(&main, format!("export ZDOTDIR='{}'", alternate.display()));
    let child = h
        .call(json!({"op":"open","source":main,"name":"alternate-child"}))
        .unwrap();
    let id = child["id"].as_str().unwrap();
    h.ready(id);
    let result = h.dir.join("alternate-result");
    h.execute(
        id,
        format!("print -r -- $STARTUP_ID > '{}'", result.display()),
    );
    assert_eq!(fs::read_to_string(result).unwrap(), "alternate\n");
    h.execute(&main, "unset ZDOTDIR".into());
    let child = h
        .call(json!({"op":"open","source":main,"name":"home-child"}))
        .unwrap();
    let id = child["id"].as_str().unwrap();
    h.ready(id);
    let result = h.dir.join("home-result");
    h.execute(
        id,
        format!("print -r -- $STARTUP_ID > '{}'", result.display()),
    );
    assert_eq!(fs::read_to_string(result).unwrap(), "home\n");
}

#[test]
fn concurrent_first_opens_publish_one_main_and_valid_default_parents() {
    let h = Host::new();
    let main = h.main();
    let project = h
        .call(json!({"op":"project-save","name":"race-project"}))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    h.call(json!({"op":"close","terminal":main})).unwrap();
    h.call(json!({"op":"wait","terminal":main,"condition":"exited","timeout_ms":10000}))
        .unwrap();
    h.call(json!({"op":"remove","terminal":main})).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(17));
    let opens: Vec<_> = (0..16)
        .map(|i| {
            let socket = h.socket.clone();
            let project = project.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                client::request(
                    &socket,
                    json!({"op":"open","project":project,"name":format!("parallel-{i}")}),
                )
                .unwrap()
            })
        })
        .collect();
    barrier.wait();
    for open in opens {
        open.join().unwrap();
    }
    let snapshot = h.call(json!({"op":"snapshot"})).unwrap();
    let terminals = snapshot["terminals"].as_array().unwrap();
    let main = snapshot["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == project)
        .unwrap()["main_terminal"]
        .as_str()
        .unwrap();
    for terminal in terminals {
        if terminal["id"] == main {
            assert!(terminal["parent_id"].is_null());
        } else {
            assert_eq!(terminal["parent_id"], main);
        }
    }
    let child = terminals.iter().find(|t| t["id"] != main).unwrap()["id"]
        .as_str()
        .unwrap();
    assert!(
        h.call(json!({"op":"reparent","terminal":child,"parent":main}))
            .is_ok()
    );
}

#[test]
fn immediate_submission_cannot_deadlock_a_pending_queued_wake() {
    let h = Host::new();
    let main = h.main();
    let helper = h.dir.join("slow-epoch-helper");
    let epoch_started = h.dir.join("epoch-started");
    let done = h.dir.join("pending-finished");
    fs::write(&helper,format!("#!/bin/sh\nif [ \"$1\" = internal ] && [ \"$2\" = shell ] && [ \"$3\" = epoch ]; then : > '{}'; sleep 1; fi\nexec '{}' \"$@\"\n",epoch_started.display(),env!("CARGO_BIN_EXE_amux"))).unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();
    h.execute(
        &main,
        format!("export AMUX_EXECUTABLE='{}'", helper.display()),
    );
    h.call(json!({"op":"execute","terminal":main,"queue":true,"source":format!("print FINISHED > '{}'",done.display())})).unwrap();
    h.until(|| epoch_started.exists());
    let started = Instant::now();
    let error = h
        .call(json!({"op":"execute","terminal":main,"source":"print MUST_NOT_RUN"}))
        .unwrap_err()
        .to_string();
    assert!(error.contains("busy"), "{error}");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "immediate request held the input gate while queued wake needed it"
    );
    h.until(|| done.exists());
    assert_eq!(fs::read_to_string(done).unwrap(), "FINISHED\n");
    h.ready(&main);
}

#[test]
fn concurrent_ancestor_removal_keeps_every_surviving_parent_valid() {
    use std::sync::{Arc, Barrier};
    let h = Host::new();
    let main = h.main();
    for _ in 0..24 {
        let a = h.call(json!({"op":"open","source":main})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let b = h.call(json!({"op":"open","source":main})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let c = h.call(json!({"op":"open","source":main})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        h.call(json!({"op":"reparent","terminal":b,"parent":a}))
            .unwrap();
        h.call(json!({"op":"reparent","terminal":c,"parent":b}))
            .unwrap();
        for id in [&a, &b] {
            h.call(json!({"op":"close","terminal":id})).unwrap();
            h.until(|| h.call(json!({"op":"state","terminal":id})).unwrap()["alive"] == false);
        }
        let barrier = Arc::new(Barrier::new(3));
        let removals = [&a, &b].map(|id| {
            let socket = h.socket.clone();
            let id = id.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                client::request(&socket, json!({"op":"remove","terminal":id})).unwrap();
            })
        });
        barrier.wait();
        for removal in removals {
            removal.join().unwrap();
        }
        let snapshot = h.call(json!({"op":"snapshot"})).unwrap();
        let terminals = snapshot["terminals"].as_array().unwrap();
        for terminal in terminals {
            let mut ancestor = terminal["parent_id"].as_str();
            let mut seen = std::collections::BTreeSet::from([terminal["id"].as_str().unwrap()]);
            while let Some(id) = ancestor {
                assert!(seen.insert(id), "cyclic tree: {snapshot}");
                let parent = terminals
                    .iter()
                    .find(|candidate| candidate["id"] == id)
                    .unwrap_or_else(|| panic!("dangling parent: {snapshot}"));
                ancestor = parent["parent_id"].as_str();
            }
        }
        h.call(json!({"op":"reparent","terminal":c,"parent":main}))
            .unwrap();
        h.call(json!({"op":"close","terminal":c})).unwrap();
        h.until(|| h.call(json!({"op":"state","terminal":c})).unwrap()["alive"] == false);
        h.call(json!({"op":"remove","terminal":c})).unwrap();
    }
}

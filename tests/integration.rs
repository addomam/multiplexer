use amux::integration::Integration;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixListener,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

struct Fixture {
    directory: PathBuf,
    integration: Arc<Integration>,
}
impl Fixture {
    fn new() -> Self {
        let directory =
            PathBuf::from("/tmp").join(format!("amux-integration-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let integration = Integration::new(
            &directory,
            &directory.join("api.sock"),
            &PathBuf::from(env!("CARGO_BIN_EXE_amux")),
        )
        .unwrap();
        integration.prepare("terminal").unwrap();
        Self {
            directory,
            integration,
        }
    }
    fn ready(&self, partial: bool) {
        self.integration.report("terminal", json!({"kind":"shell-context","cwd":"/tmp","env":{"EXPORTED":"value"},"pid":std::process::id()})).unwrap();
        self.integration
            .report(
                "terminal",
                json!({"kind":"shell-ready","partial":partial,"pid":std::process::id()}),
            )
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
struct ServerGuard {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Drop for ServerGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
struct ChildGuard {
    child: Box<dyn portable_pty::Child + Send + Sync>,
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn eventually(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "condition did not become true before deadline"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
fn agent(instance: &str, kind: &str, activity: &str, settled: bool) -> Value {
    json!({"kind":"agent","instance_id":instance,"agent_kind":kind,"agent_id":instance,"activity":activity,"pid":std::process::id(),"conversation_id":"session-one","generation":1,"completed_generation":if settled {Some(1)} else {None},"retrying":false,"settled":settled})
}

#[test]
fn failed_preparation_removes_partial_resources_without_traversing_ids() {
    let fixture = Fixture::new();
    let partial = fixture.directory.join("integration").join("failed");
    fs::create_dir_all(partial.join(".zshrc")).unwrap();
    assert!(fixture.integration.prepare("failed").is_err());
    fixture.integration.remove("failed");
    assert!(!partial.exists());

    let retained = fixture.directory.join("retained");
    fs::create_dir(&retained).unwrap();
    fs::write(retained.join("owned"), "keep").unwrap();
    fixture.integration.remove("../retained");
    assert_eq!(fs::read_to_string(retained.join("owned")).unwrap(), "keep");
}
#[test]
fn busy_partial_and_lost_are_distinct_and_capture_remains_timestamped() {
    let fixture = Fixture::new();
    fixture.ready(true);
    assert!(
        fixture
            .integration
            .execute("terminal", "echo unsafe", false)
            .unwrap_err()
            .to_string()
            .contains("partial-input")
    );
    let context = fixture.integration.capture("terminal").unwrap();
    assert_eq!(context.env["EXPORTED"], "value");
    assert!(context.captured_at_ms > 0);
    fixture
        .integration
        .report("terminal", json!({"kind":"shell-busy"}))
        .unwrap();
    assert!(
        fixture
            .integration
            .execute("terminal", "echo unsafe", false)
            .unwrap_err()
            .to_string()
            .contains("busy")
    );
    assert_eq!(
        fixture.integration.state("terminal")["shell"]["freshness"],
        "stale"
    );
    fixture
        .integration
        .report("terminal", json!({"kind":"shell-lost"}))
        .unwrap();
    assert!(
        fixture
            .integration
            .execute("terminal", "echo unsafe", true)
            .unwrap_err()
            .to_string()
            .contains("integration-lost")
    );
    assert_eq!(
        fixture.integration.state("terminal")["shell"]["freshness"],
        "unknown"
    );
}
#[test]
fn nested_agents_never_overwrite_root_activity_and_shutdown_is_not_settled() {
    let fixture = Fixture::new();
    fixture
        .integration
        .report("terminal", agent("root", "main", "working", false))
        .unwrap();
    fixture
        .integration
        .report("terminal", agent("child", "sub", "idle", true))
        .unwrap();
    let state = fixture.integration.state("terminal");
    assert_eq!(state["agent"]["activity"], "working");
    assert_eq!(state["agent"]["settled"], false);
    assert_eq!(state["agent"]["children"][0]["activity"], "idle");
    fixture
        .integration
        .report("terminal", agent("root", "main", "idle", true))
        .unwrap();
    assert_eq!(
        fixture.integration.state("terminal")["agent"]["settled"],
        true
    );
    let mut shutdown = agent("root", "main", "idle", true);
    shutdown["shutdown"] = json!(true);
    fixture.integration.report("terminal", shutdown).unwrap();
    assert_eq!(
        fixture.integration.state("terminal")["agent"]["activity"],
        "unknown"
    );
    assert_eq!(
        fixture.integration.state("terminal")["agent"]["settled"],
        false
    );
}
#[test]
fn agent_lifetimes_are_bounded_without_losing_root_or_active_children() {
    let fixture = Fixture::new();
    for i in 0..300 {
        fixture
            .integration
            .report(
                "terminal",
                agent(&format!("root-{i}"), "main", "working", false),
            )
            .unwrap();
    }
    for i in 0..127 {
        fixture
            .integration
            .report(
                "terminal",
                agent(&format!("child-{i}"), "sub", "working", false),
            )
            .unwrap();
    }
    assert!(
        fixture
            .integration
            .report("terminal", agent("overflow", "sub", "working", false))
            .unwrap_err()
            .to_string()
            .contains("active agent reporting limit")
    );
    let state = fixture.integration.state("terminal");
    assert_eq!(state["agent"]["agent_id"], "root-299");
    assert_eq!(state["agent"]["activity"], "working");
    let mut ended = agent("child-0", "sub", "unknown", false);
    ended["shutdown"] = true.into();
    fixture.integration.report("terminal", ended).unwrap();
    let state = fixture
        .integration
        .report("terminal", agent("replacement", "sub", "working", false))
        .unwrap();
    assert!(
        state["agent"]["children"]
            .as_array()
            .unwrap()
            .iter()
            .any(|child| child["agent_id"] == "replacement" && child["activity"] == "working")
    );
    assert!(
        !state["agent"]["children"]
            .as_array()
            .unwrap()
            .iter()
            .any(|child| child["agent_id"] == "child-0")
    );
    assert_eq!(state["agent"]["settled"], false);
}

#[test]
fn real_zsh_preserves_startup_exports_input_foreground_and_explicit_queue() {
    let fixture = Fixture::new();
    let user = fixture.directory.join("user");
    fs::create_dir_all(&user).unwrap();
    fs::write(
        user.join(".zshenv"),
        "export STARTUP_ENV=yes\nprint env >> $HOME/startup-count\n",
    )
    .unwrap();
    fs::write(
        user.join(".zprofile"),
        "print profile >> $HOME/startup-count\n",
    )
    .unwrap();
    fs::write(
        user.join(".zshrc"),
        "PROMPT='fixture> '\nexport STARTUP_RC=yes\nprint rc >> $HOME/startup-count\n",
    )
    .unwrap();
    fs::write(user.join(".zlogin"), "print login >> $HOME/startup-count\n").unwrap();
    let listener = UnixListener::bind(fixture.directory.join("api.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_server = stop.clone();
    let integration = fixture.integration.clone();
    let wake_input = Arc::new(parking_lot::Mutex::new(None::<Box<dyn Write + Send>>));
    let server_input = wake_input.clone();
    let server = thread::spawn(move || {
        while !stop_server.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut line = String::new();
                    if BufReader::new(&stream).read_line(&mut line).is_err() {
                        continue;
                    }
                    let request: Value = serde_json::from_str(&line).unwrap();
                    assert_eq!(request["op"], "report");
                    let result = if request["event"]["kind"] == "shell-epoch" {
                        json!({"input_epoch":0})
                    } else if request["event"]["kind"] == "shell-wake" {
                        json!({"woke":true})
                    } else {
                        integration
                            .report(
                                request["terminal"].as_str().unwrap(),
                                request["event"].clone(),
                            )
                            .unwrap()
                    };
                    if request["event"]["kind"] == "shell-wake" {
                        server_input
                            .lock()
                            .as_mut()
                            .unwrap()
                            .write_all(b"\x1b[amux-terminal~")
                            .unwrap();
                    }
                    let _ = writeln!(stream, "{}", json!({"version":1,"ok":true,"result":result}));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(2))
                }
                Err(error) => panic!("report listener failed: {error}"),
            }
        }
    });
    let mut server_guard = ServerGuard {
        stop: stop.clone(),
        thread: Some(server),
    };
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new("zsh");
    command.args(["-il"]);
    command.env("HOME", &user);
    command.env("AMUX_ORIGINAL_ZDOTDIR", &user);
    command.env("AMUX_TERMINAL", "terminal");
    command.env("AMUX_SOCKET", fixture.directory.join("api.sock"));
    command.env("AMUX_EXECUTABLE", env!("CARGO_BIN_EXE_amux"));
    command.env(
        "AMUX_INTEGRATION",
        fixture.directory.join("integration/terminal"),
    );
    command.env("ZDOTDIR", fixture.directory.join("integration/terminal"));
    command.env(
        "AMUX_ADAPTER",
        fixture.directory.join("integration/adapter.zsh"),
    );
    command.env("AMUX_OMP_EXTENSION", fixture.integration.extension_path());
    command.env("TERM", "xterm-256color");
    command.cwd(&user);
    let mut child = ChildGuard {
        child: pair.slave.spawn_command(command).unwrap(),
    };
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let drain = thread::spawn(move || {
        let mut buffer = [0; 8192];
        while reader.read(&mut buffer).is_ok_and(|n| n > 0) {}
    });
    *wake_input.lock() = Some(pair.master.take_writer().unwrap());
    eventually(|| fixture.integration.state("terminal")["shell"]["ready"] == true);
    assert_eq!(
        fs::read_to_string(user.join("startup-count")).unwrap(),
        "env\nprofile\nrc\nlogin\n"
    );
    assert_eq!(
        fixture.integration.capture("terminal").unwrap().env["STARTUP_RC"],
        "yes"
    );
    wake_input
        .lock()
        .as_mut()
        .unwrap()
        .write_all(b"printf partial")
        .unwrap();
    eventually(|| fixture.integration.state("terminal")["shell"]["partial_input"] == true);
    assert!(
        fixture
            .integration
            .execute("terminal", "export REJECTED=yes", false)
            .unwrap_err()
            .to_string()
            .contains("partial-input")
    );
    // Deliberately stale daemon readiness cannot bypass the real editing buffer.
    let shell = fixture.integration.state("terminal")["shell"].clone();
    fixture.integration.report("terminal", json!({"kind":"shell-ready","pid":shell["pid"],"partial":false,"prompt_generation":shell["prompt_generation"]})).unwrap();
    assert!(
        fixture
            .integration
            .execute("terminal", "export REJECTED=yes", false)
            .unwrap_err()
            .to_string()
            .contains("partial-input")
    );
    fixture.integration.report("terminal", json!({"kind":"shell-ready","pid":shell["pid"],"partial":true,"prompt_generation":shell["prompt_generation"]})).unwrap();
    assert!(
        !fixture
            .integration
            .capture("terminal")
            .unwrap()
            .env
            .contains_key("REJECTED")
    );
    wake_input
        .lock()
        .as_mut()
        .unwrap()
        .write_all(b"\x15")
        .unwrap();
    eventually(|| fixture.integration.state("terminal")["shell"]["partial_input"] == false);
    fixture.integration.execute("terminal", "typeset TOP_LEVEL_VALUE=visible; export NEW_EXPORT='space and newline\nvalue'; cd /tmp", false).unwrap();
    eventually(|| {
        fixture.integration.capture("terminal").is_some_and(|c| {
            c.env
                .get("NEW_EXPORT")
                .is_some_and(|v| v == "space and newline\nvalue")
                && c.cwd.as_path() == std::path::Path::new("/tmp")
        })
    });
    eventually(|| fixture.integration.state("terminal")["shell"]["ready"] == true);
    fixture
        .integration
        .execute("terminal", "export FROM_TOP_LEVEL=$TOP_LEVEL_VALUE", false)
        .unwrap();
    eventually(|| {
        fixture
            .integration
            .capture("terminal")
            .is_some_and(|c| c.env.get("FROM_TOP_LEVEL").is_some_and(|v| v == "visible"))
    });
    eventually(|| fixture.integration.state("terminal")["shell"]["ready"] == true);
    wake_input
        .lock()
        .as_mut()
        .unwrap()
        .write_all(b"sleep 1\n")
        .unwrap();
    eventually(|| fixture.integration.state("terminal")["shell"]["ready"] == false);
    assert!(
        fixture
            .integration
            .execute("terminal", "export NOT_ALLOWED=yes", false)
            .unwrap_err()
            .to_string()
            .contains("busy")
    );
    let queued = fixture
        .integration
        .execute("terminal", "export QUEUED=yes", true)
        .unwrap();
    assert_eq!(queued["status"], "queued");
    eventually(|| {
        fixture
            .integration
            .capture("terminal")
            .is_some_and(|c| c.env.get("QUEUED").is_some_and(|v| v == "yes"))
    });
    eventually(|| fixture.integration.state("terminal")["shell"]["ready"] == true);
    fixture
        .integration
        .execute(
            "terminal",
            "zle -F \"$_amux_fd\"; exec {_amux_fd}<&-; _amux_report lost \"$$\"",
            false,
        )
        .unwrap();
    eventually(|| fixture.integration.state("terminal")["shell"]["freshness"] == "unknown");
    wake_input
        .lock()
        .as_mut()
        .unwrap()
        .write_all(
            format!(
                "print STILL_NATIVE > '{}'\n",
                user.join("still-native").display()
            )
            .as_bytes(),
        )
        .unwrap();
    eventually(|| {
        fs::read_to_string(user.join("still-native")).is_ok_and(|s| s == "STILL_NATIVE\n")
    });
    assert_eq!(
        fixture.integration.state("terminal")["shell"]["ready"],
        false
    );
    assert!(
        fixture
            .integration
            .execute("terminal", "print MUST_NOT_RUN", false)
            .unwrap_err()
            .to_string()
            .contains("integration-lost")
    );
    wake_input
        .lock()
        .as_mut()
        .unwrap()
        .write_all(b"exit\n")
        .unwrap();
    child.child.wait().unwrap();
    wake_input.lock().take();
    drop(pair.master);
    drain.join().unwrap();
    stop.store(true, Ordering::Relaxed);
    server_guard.thread.take().unwrap().join().unwrap();
}

use amux::client;
use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

struct Sandbox {
    directory: PathBuf,
    socket: PathBuf,
}
impl Sandbox {
    fn new() -> Self {
        let directory = fs::canonicalize("/tmp")
            .unwrap()
            .join(format!("amux-ready-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        fs::create_dir(directory.join("startup")).unwrap();
        Self {
            socket: directory.join("control.sock"),
            directory,
        }
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_amux"));
        command
            .arg("--socket")
            .arg(&self.socket)
            .env("XDG_CONFIG_HOME", self.directory.join("config"))
            .env("ZDOTDIR", self.directory.join("startup"))
            .current_dir(&self.directory)
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        command
    }
    fn wait(child: &mut Child, timeout: Duration) -> std::process::ExitStatus {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                return status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("private startup process did not finish within {timeout:?}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        if self.socket.exists() {
            let _ = client::request(&self.socket, json!({"op":"shutdown","timeout_ms":0}));
        }
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn existing_compatible_v1_snapshot_is_the_only_readiness_request() {
    let sandbox = Sandbox::new();
    let listener = UnixListener::bind(&sandbox.socket).unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut line = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut line)
            .unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request, json!({"version":1,"op":"snapshot"}));
        // A compatible daemon may legitimately have no retained terminals.
        stream.write_all(b"{\"version\":1,\"ok\":true,\"result\":{\"seq\":7,\"config\":{},\"projects\":[],\"terminals\":[]}}\n").unwrap();
    });
    client::ensure_daemon(&sandbox.socket).unwrap();
    server.join().unwrap();
    assert!(!sandbox.directory.join("daemon.log").exists());
    fs::remove_file(&sandbox.socket).unwrap();
}

#[test]
fn existing_malformed_or_uninitialized_listener_is_not_replaced() {
    for response in [
        "not json\n",
        "{\"version\":2,\"ok\":true,\"result\":{\"seq\":1,\"config\":{},\"projects\":[],\"terminals\":[]}}\n",
        "{\"version\":1,\"ok\":true,\"result\":{}}\n",
        "{\"version\":1,\"ok\":false,\"error\":{\"code\":\"busy\"}}\n",
        "{\"version\":1,\"ok\":true}",
    ] {
        let sandbox = Sandbox::new();
        let listener = UnixListener::bind(&sandbox.socket).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&line).unwrap()["op"],
                "snapshot"
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        let started = Instant::now();
        let error = client::ensure_daemon(&sandbox.socket).unwrap_err();
        assert!(
            format!("{error:#}").starts_with("daemon-readiness:"),
            "{error:#}"
        );
        assert!(started.elapsed() < Duration::from_secs(3));
        server.join().unwrap();
        assert!(!sandbox.directory.join("daemon.log").exists());
        assert!(sandbox.socket.exists());
        fs::remove_file(&sandbox.socket).unwrap();
    }
}

#[test]
fn inert_and_trickling_listeners_have_an_absolute_readiness_deadline() {
    for trickle in [false, true] {
        let sandbox = Sandbox::new();
        let listener = UnixListener::bind(&sandbox.socket).unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_write_timeout(Some(Duration::from_millis(100)))
                .unwrap();
            while !stop.load(Ordering::Acquire) {
                if trickle && stream.write_all(b" ").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        let started = Instant::now();
        let error = client::ensure_daemon(&sandbox.socket).unwrap_err();
        let elapsed = started.elapsed();
        stopped.store(true, Ordering::Release);
        server.join().unwrap();
        assert!(
            format!("{error:#}").contains("readiness deadline"),
            "{error:#}"
        );
        assert!(
            elapsed < Duration::from_secs(3),
            "readiness took {elapsed:?}"
        );
        assert!(!sandbox.directory.join("daemon.log").exists());
        fs::remove_file(&sandbox.socket).unwrap();
    }
}

#[test]
fn concurrent_real_autostarts_create_one_initialized_daemon_and_shell() {
    let sandbox = Sandbox::new();
    let mut frontends: Vec<_> = (0..6)
        .map(|index| {
            let stderr =
                fs::File::create(sandbox.directory.join(format!("frontend-{index}.log"))).unwrap();
            sandbox
                .command()
                .arg("attach")
                .stderr(stderr)
                .spawn()
                .unwrap()
        })
        .collect();
    let deadline = Instant::now() + Duration::from_secs(12);
    let snapshot = loop {
        if let Ok(snapshot) =
            client::request(&sandbox.socket, json!({"op":"snapshot","timeout_ms":0}))
        {
            break snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "autostart never produced a valid snapshot"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(snapshot["terminals"].as_array().unwrap().len(), 1);
    for (index, frontend) in frontends.iter_mut().enumerate() {
        Sandbox::wait(frontend, Duration::from_secs(5));
        let stderr =
            fs::read_to_string(sandbox.directory.join(format!("frontend-{index}.log"))).unwrap();
        assert!(!stderr.contains("daemon-start:"), "{stderr}");
        assert!(!stderr.contains("daemon-readiness:"), "{stderr}");
    }
    let log = fs::read_to_string(sandbox.directory.join("daemon.log")).unwrap();
    assert!(
        !log.contains("already-running:"),
        "duplicate autostart: {log}"
    );
    assert_eq!(
        client::request(&sandbox.socket, json!({"op":"snapshot"})).unwrap()["terminals"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    client::request(&sandbox.socket, json!({"op":"shutdown"})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while sandbox.socket.exists() {
        assert!(
            Instant::now() < deadline,
            "autostarted daemon did not clean up its socket"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn failed_real_autostart_reports_invalid_configuration_and_removes_socket() {
    let cases: [(&[u8], &str); 3] = [
        (b"not valid JSON", "invalid project configuration"),
        (
            br#"{"version":1,"config":{"projects_dir":"/unused","prefix":"C-a","unknown_setting":true},"projects":[]}"#,
            "unknown field",
        ),
        (
            br#"{"version":1,"config":{"projects_dir":"/unused","prefix":"C-a","terminal_limit":0},"projects":[]}"#,
            "terminal_limit must be in 1..256",
        ),
    ];
    for (bytes, cause) in cases {
        let sandbox = Sandbox::new();
        let config = sandbox.directory.join("config/amux");
        fs::create_dir_all(&config).unwrap();
        let config_path = config.join("config.json");
        fs::write(&config_path, bytes).unwrap();
        let error_path = sandbox.directory.join("frontend.log");
        let mut frontend = sandbox
            .command()
            .arg("attach")
            .stderr(fs::File::create(&error_path).unwrap())
            .spawn()
            .unwrap();
        assert!(!Sandbox::wait(&mut frontend, Duration::from_secs(5)).success());
        let error = fs::read_to_string(error_path).unwrap();
        assert!(error.contains("daemon-start:"), "{error}");
        assert!(error.contains(cause), "{error}");
        assert!(error.contains(config_path.to_str().unwrap()), "{error}");
        let daemon_log = fs::read_to_string(sandbox.directory.join("daemon.log")).unwrap();
        assert!(daemon_log.contains(cause), "{daemon_log}");
        assert!(
            daemon_log.contains(config_path.to_str().unwrap()),
            "{daemon_log}"
        );
        assert!(
            !sandbox.socket.exists(),
            "failed startup left a connectable-looking socket"
        );
    }
}

#[test]
fn failed_first_pty_reports_error_and_removes_startup_socket() {
    let sandbox = Sandbox::new();
    let error_path = sandbox.directory.join("failed-pty.log");
    let mut command = sandbox.command();
    command
        .arg("daemon")
        .stderr(fs::File::create(&error_path).unwrap());
    unsafe {
        command.pre_exec(|| {
            let limit = libc::rlimit {
                rlim_cur: 6,
                rlim_max: 6,
            };
            if libc::setrlimit(libc::RLIMIT_NOFILE, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut daemon = command.spawn().unwrap();
    assert!(!Sandbox::wait(&mut daemon, Duration::from_secs(5)).success());
    let error = fs::read_to_string(error_path).unwrap();
    assert!(
        error.to_ascii_lowercase().contains("too many open files"),
        "{error}"
    );
    assert!(
        !sandbox.socket.exists(),
        "first-shell failure left a stale socket"
    );
}

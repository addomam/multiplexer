use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::{
    fs,
    io::{Read, Write},
    sync::mpsc,
    time::{Duration, Instant},
};

struct ChildGuard(Box<dyn portable_pty::Child + Send + Sync>);
impl std::ops::Deref for ChildGuard {
    type Target = dyn portable_pty::Child + Send + Sync;
    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}
impl std::ops::DerefMut for ChildGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut()
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}
#[test]
fn real_application_restores_termios_on_signal_and_exports_resized_visual_verdict() {
    let pair = native_pty_system().openpty(size(100, 30)).unwrap();
    let fd = pair.master.as_raw_fd().unwrap();
    let mut before = unsafe { std::mem::zeroed::<libc::termios>() };
    assert_eq!(unsafe { libc::tcgetattr(fd, &mut before) }, 0);
    let directory = std::env::temp_dir().join(format!("showcase-live-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let report = directory.join("report.json");
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_terminal-showcase"));
    command.args(["--demo", "unicode", "--report"]);
    command.arg(&report);
    command.env("TERM", "xterm-256color");
    let mut child = ChildGuard(pair.slave.spawn_command(command).unwrap());
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (send, recv) = mpsc::channel();
    let drain = std::thread::spawn(move || {
        let mut buf = [0; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if send.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut output = Vec::new();
    loop {
        assert!(Instant::now() < deadline, "dashboard startup timeout");
        if let Ok(bytes) = recv.recv_timeout(Duration::from_millis(50)) {
            output.extend(bytes);
        }
        if output.windows(8).any(|b| b == b"\x1b[?2004h") {
            break;
        }
    }
    pair.master.resize(size(90, 26)).unwrap();
    let mut paste = b"\x1b[200~".to_vec();
    paste.extend(std::iter::repeat_n(b'q', 4096));
    paste.extend_from_slice(b"\x1b[201~");
    writer.write_all(&paste).unwrap();
    std::thread::sleep(Duration::from_millis(150));
    assert!(
        child.try_wait().unwrap().is_none(),
        "pasted q must not exit the application"
    );
    writer.write_all(b"\x1b[113;1:3u\x1bOC").unwrap();
    let mut specimen_output = Vec::new();
    loop {
        if let Ok(bytes) = recv.recv_timeout(Duration::from_millis(50)) {
            specimen_output.extend(bytes);
        }
        if specimen_output.windows(4).any(|b| b == b"\x1b[4:") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "typography specimen never emitted"
        );
    }
    writer.write_all(b"\x1b[27;2;80~\x1bODPE").unwrap();
    loop {
        let snapshot = fs::read(&report)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
        if snapshot.is_some_and(|value| {
            value["report"]["capabilities"]
                .as_array()
                .is_some_and(|caps| {
                    caps.iter()
                        .any(|c| c["id"] == "wide" && c["visual"] == true)
                })
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "input navigation / visual export never acknowledged"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    unsafe {
        libc::kill(child.process_id().unwrap() as i32, libc::SIGTERM);
    }
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "signal cleanup timeout");
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut after = unsafe { std::mem::zeroed::<libc::termios>() };
    assert_eq!(unsafe { libc::tcgetattr(fd, &mut after) }, 0);
    assert_eq!(
        (
            before.c_iflag,
            before.c_oflag,
            before.c_cflag,
            before.c_lflag,
            before.c_cc
        ),
        (
            after.c_iflag,
            after.c_oflag,
            after.c_cflag,
            after.c_lflag,
            after.c_cc
        ),
        "local input/echo modes leaked"
    );
    let json: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(json["report"]["columns"], 90);
    assert_eq!(json["report"]["rows"], 26);
    let caps = json["report"]["capabilities"].as_array().unwrap();
    assert_eq!(
        caps.iter().find(|c| c["id"] == "resize").unwrap()["status"],
        "verified"
    );
    assert_eq!(
        caps.iter().find(|c| c["id"] == "wide").unwrap()["visual"],
        true
    );
    assert_eq!(
        caps.iter().find(|c| c["id"] == "underlines").unwrap()["visual"],
        true,
        "SS3 arrows and modifyOtherKeys P must select/mark the typography demo"
    );
    for id in ["kitty_keyboard", "modify_other_keys"] {
        assert_eq!(
            caps.iter().find(|c| c["id"] == id).unwrap()["status"],
            "verified"
        );
    }
    assert_ne!(
        caps.iter().find(|c| c["id"] == "kitty_graphics").unwrap()["status"],
        "unsupported",
        "silence is not proof of non-support"
    );
    drop(writer);
    drop(pair.master);
    drain.join().unwrap();
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn headless_cli_fixtures_reject_invalid_cursor_reports_and_emit_valid_json() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_terminal-showcase"))
        .args(["--headless", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["mode"], "headless");
    assert!(
        json["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["passed"] == true)
    );
    let bad = std::process::Command::new(env!("CARGO_BIN_EXE_terminal-showcase"))
        .args(["--duration", "NaN"])
        .output()
        .unwrap();
    assert!(!bad.status.success());
}

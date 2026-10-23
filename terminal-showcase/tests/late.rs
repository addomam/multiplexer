use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::{
    fs,
    io::{Read, Write},
    sync::mpsc,
    time::{Duration, Instant},
};

struct ChildGuard(Box<dyn portable_pty::Child + Send + Sync>);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn timed_out_cursor_reply_cannot_verify_a_new_manual_suite() {
    let pair = native_pty_system()
        .openpty(PtySize {
            cols: 100,
            rows: 30,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let directory = std::env::temp_dir().join(format!("showcase-late-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let report = directory.join("report.json");
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_terminal-showcase"));
    command.args(["--demo", "control", "--screenshot", "--report"]);
    command.arg(&report);
    let mut child = ChildGuard(pair.slave.spawn_command(command).unwrap());
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (send, recv) = mpsc::channel();
    let drain = std::thread::spawn(move || {
        let mut bytes = [0; 8192];
        while let Ok(count) = reader.read(&mut bytes) {
            if count == 0 || send.send(bytes[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut output = Vec::new();
    loop {
        if let Ok(bytes) = recv.recv_timeout(Duration::from_millis(20)) {
            output.extend(bytes);
        }
        if output.windows(8).any(|b| b == b"\x1b[?2004h") {
            break;
        }
        assert!(Instant::now() < deadline, "initial timeout never completed");
    }
    // The endpoint deliberately withheld the first startup CPR. A user asks to
    // rerun, then that old (perfectly valid) response arrives 100ms later.
    writer.write_all(b"R").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    writer.write_all(b"\x1b[3;7R").unwrap();
    std::thread::sleep(Duration::from_millis(400));
    writer.write_all(b"Eq").unwrap();
    loop {
        if child.0.try_wait().unwrap().is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "exit blocked by late reply");
        std::thread::sleep(Duration::from_millis(20));
    }
    let json: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    let cursor = json["report"]["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|cap| cap["id"] == "cursor")
        .unwrap();
    assert_eq!(
        cursor["objective_status"], "unknown",
        "a timed-out response is not proof of a new transaction"
    );
    assert_eq!(json["exercises"][0]["outcome"], "timeout");
    drop(writer);
    drop(pair.master);
    drain.join().unwrap();
    fs::remove_dir_all(directory).unwrap();
}

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::{
    fs,
    io::{Read, Write},
    sync::mpsc,
    time::{Duration, Instant},
};

struct Cleanup {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    directory: std::path::PathBuf,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn emitted_default_colors_accept_visual_failure_without_rewriting_query_evidence() {
    let pair = native_pty_system()
        .openpty(PtySize {
            cols: 120,
            rows: 36,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let directory = std::env::temp_dir().join(format!("showcase-colors-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let report = directory.join("report.json");
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_terminal-showcase"));
    command.args(["--demo", "colors", "--screenshot", "--report"]);
    command.arg(&report);
    let mut guard = Cleanup {
        child: pair.slave.spawn_command(command).unwrap(),
        directory,
    };
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (send, recv) = mpsc::channel();
    let drain = std::thread::spawn(move || {
        let mut bytes = [0; 8192];
        while let Ok(n) = reader.read(&mut bytes) {
            if n == 0 || send.send(bytes[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut output = Vec::new();
    let mut replied = false;
    loop {
        if let Ok(bytes) = recv.recv_timeout(Duration::from_millis(20)) {
            output.extend(bytes);
        }
        if !replied && output.windows(7).any(|b| b == b"\x1b]10;?\x1b") {
            writer
                .write_all(b"\x1b]10;rgb:1111/2222/3333\x1b\\\x1b]4;0;rgb:aaaa/bbbb/cccc\x1b\\")
                .unwrap();
            replied = true;
        }
        if output.windows(8).any(|b| b == b"\x1b[39;49m") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "default-color visual run was not emitted"
        );
    }
    writer.write_all(b"FE").unwrap();
    let json: serde_json::Value = loop {
        if let Some(json) = fs::read(&report)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        {
            break json;
        }
        assert!(
            Instant::now() < deadline,
            "visual verdict export did not complete"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let caps = json["report"]["capabilities"].as_array().unwrap();
    let defaults = caps.iter().find(|c| c["id"] == "default_colors").unwrap();
    assert_eq!(defaults["objective_status"], "advertised");
    assert_eq!(defaults["status"], "failed");
    assert_eq!(defaults["visual"], false);
    let palette = caps.iter().find(|c| c["id"] == "palette").unwrap();
    assert_eq!(palette["objective_status"], "advertised");
    assert!(
        palette["visual"].is_null(),
        "a query is not a palette-rendering verdict"
    );
    writer.write_all(b"q").unwrap();
    loop {
        if let Some(status) = guard.child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "normal exit blocked");
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(writer);
    drop(pair.master);
    drain.join().unwrap();
}

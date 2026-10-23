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
fn overflowing_each_deferred_phase_cannot_turn_paste_into_consent_or_exit() {
    for (phase, trigger, limit) in [
        ("probe", b"\x1b_G".as_slice(), 128),
        ("margins", b"\x1bP$qr\x1b\\".as_slice(), 512),
        ("exercise", b"\x1b[6n".as_slice(), 512),
    ] {
        let pair = native_pty_system()
            .openpty(PtySize {
                cols: 100,
                rows: 30,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let directory =
            std::env::temp_dir().join(format!("showcase-overflow-{}-{phase}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let report = directory.join("report.json");
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_terminal-showcase"));
        command.args(["--demo", "integration", "--screenshot", "--report"]);
        command.arg(&report);
        let mut guard = Cleanup {
            child: pair.slave.spawn_command(command).unwrap(),
            directory,
        };
        drop(pair.slave);
        let mut writer = pair.master.take_writer().unwrap();
        let mut reader = pair.master.try_clone_reader().unwrap();
        let (send, recv) = mpsc::channel();
        let drain = std::thread::spawn(move || {
            let mut buf = [0; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || send.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut output = Vec::new();
        let mut sent = false;
        let mut payload_sent = false;
        loop {
            if let Ok(bytes) = recv.recv_timeout(Duration::from_millis(20)) {
                output.extend(bytes);
            }
            if !sent && output.windows(trigger.len()).any(|b| b == trigger) {
                let mut flood = b"\x1b[I".repeat(limit + 1);
                flood.extend_from_slice(b"\x1b[200~");
                writer.write_all(&flood).unwrap();
                sent = true;
            }
            if phase == "probe"
                && sent
                && !payload_sent
                && output.windows(7).any(|b| b == b"\x1bP$qr\x1b\\")
            {
                writer.write_all(b"CNTW").unwrap();
                payload_sent = true;
                // Deliberately deliver payload in the next producer's larger queue.
            }
            if output.windows(19).any(|b| b == b"TERMINAL / SHOWCASE") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "{phase}: dashboard did not start"
            );
        }
        writer.write_all(b"CNTWQ\x1b[201~E").unwrap();
        let json: serde_json::Value = loop {
            if let Some(json) = fs::read(&report)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            {
                break json;
            }
            if let Ok(bytes) = recv.recv_timeout(Duration::from_millis(20)) {
                output.extend(bytes);
            }
            assert!(
                Instant::now() < deadline,
                "{phase}: export/exit controls lost"
            );
        };
        for id in ["clipboard", "notifications", "title", "cwd"] {
            let cap = json["report"]["capabilities"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["id"] == id)
                .unwrap();
            assert_eq!(
                cap["status"], "unknown",
                "{phase}: pasted action {id} escaped its frame"
            );
        }
        assert!(
            guard.child.try_wait().unwrap().is_none(),
            "{phase}: pasted Q exited"
        );
        assert!(
            !output.windows(5).any(|b| b == b"\x1b]52;"),
            "{phase}: clipboard write without a consent key"
        );
        writer.write_all(b"q").unwrap();
        loop {
            if let Some(status) = guard.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "{phase}: normal q did not exit after paste"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(writer);
        drop(pair.master);
        drain.join().unwrap();
    }
}

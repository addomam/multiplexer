use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::{
    fs,
    io::{Read, Write},
    sync::mpsc,
    time::{Duration, Instant},
};

struct Endpoint {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    receiver: mpsc::Receiver<Vec<u8>>,
    drain: Option<std::thread::JoinHandle<()>>,
    directory: std::path::PathBuf,
    output: Vec<u8>,
}

impl Endpoint {
    fn new(name: &str) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                cols: 100,
                rows: 30,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let directory = std::env::temp_dir().join(format!(
            "showcase-paste-boundary-{}-{name}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_terminal-showcase"));
        command.args(["--demo", "integration", "--screenshot", "--report"]);
        command.arg(directory.join("report.json"));
        let child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let writer = pair.master.take_writer().unwrap();
        let mut reader = pair.master.try_clone_reader().unwrap();
        let (send, receiver) = mpsc::channel();
        let drain = std::thread::spawn(move || {
            let mut bytes = [0; 8192];
            while let Ok(n) = reader.read(&mut bytes) {
                if n == 0 || send.send(bytes[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            master: Some(pair.master),
            writer: Some(writer),
            receiver,
            drain: Some(drain),
            directory,
            output: Vec::new(),
        }
    }

    fn read_for(&mut self, duration: Duration) {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            if let Ok(bytes) = self.receiver.recv_timeout(Duration::from_millis(20)) {
                self.output.extend(bytes);
            }
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.writer.as_mut().unwrap().write_all(bytes).unwrap();
        self.writer.as_mut().unwrap().flush().unwrap();
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.writer.take();
        self.master.take();
        if let Some(drain) = self.drain.take() {
            let _ = drain.join();
        }
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn real_integration_demo_keeps_paste_opaque_and_resumes_export_and_quit() {
    for (name, payload) in [
        ("unicode", "Û201~CNTWQЛ201~🛛201~".as_bytes()),
        ("literal-header", b"\x1bPtmux;literalCNTWQ".as_slice()),
        ("incomplete-header", b"\x1bPtmCNTWQ".as_slice()),
        ("trailing-escape", b"CNTWQ\x1b".as_slice()),
        ("trailing-escapes", b"CNTWQ\x1b\x1b\x1b".as_slice()),
    ] {
        let mut endpoint = Endpoint::new(name);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !endpoint
            .output
            .windows(19)
            .any(|bytes| bytes == b"TERMINAL / SHOWCASE")
        {
            endpoint.read_for(Duration::from_millis(20));
            assert!(Instant::now() < deadline, "{name}: dashboard did not start");
        }
        endpoint.send(b"\x1b[200~");
        // Scalar bytes and wrapper-looking text cross real application reads.
        for &byte in payload {
            endpoint.send(&[byte]);
            endpoint.read_for(Duration::from_millis(20));
        }
        endpoint.read_for(Duration::from_millis(200));
        assert!(
            endpoint.child.try_wait().unwrap().is_none(),
            "{name}: pasted Q exited"
        );
        assert!(
            !endpoint.directory.join("report.json").exists(),
            "{name}: paste exported a report"
        );
        endpoint.send(b"\x1b[201~E");
        let json: serde_json::Value = loop {
            if let Some(json) = fs::read(endpoint.directory.join("report.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            {
                break json;
            }
            endpoint.read_for(Duration::from_millis(20));
            assert!(
                Instant::now() < deadline,
                "{name}: E did not resume after genuine end"
            );
        };
        for id in ["clipboard", "notifications", "title", "cwd"] {
            let capability = json["report"]["capabilities"]
                .as_array()
                .unwrap()
                .iter()
                .find(|capability| capability["id"] == id)
                .unwrap();
            assert_eq!(
                capability["status"], "unknown",
                "{name}: pasted {id} action escaped"
            );
        }
        for forbidden in [
            b"\x1b]52;".as_slice(),
            b"\x1b]9;".as_slice(),
            b"\x1b]0;terminal-showcase".as_slice(),
            b"\x1b]7;file://localhost/tmp".as_slice(),
        ] {
            assert!(
                !endpoint
                    .output
                    .windows(forbidden.len())
                    .any(|bytes| bytes == forbidden),
                "{name}: integration action emitted without consent"
            );
        }
        assert!(
            endpoint.child.try_wait().unwrap().is_none(),
            "{name}: pasted Q exited after end"
        );
        endpoint.send(b"q");
        loop {
            if let Some(status) = endpoint.child.try_wait().unwrap() {
                assert!(status.success(), "{name}: application failed");
                break;
            }
            endpoint.read_for(Duration::from_millis(20));
            assert!(
                Instant::now() < deadline,
                "{name}: q did not resume after genuine end"
            );
        }
    }
}

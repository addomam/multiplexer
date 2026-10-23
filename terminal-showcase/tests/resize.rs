use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};
use terminal_showcase::{
    capability::Report,
    exercises,
    protocol::{Event, Parser},
    terminal::Terminal,
};

struct Endpoint {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Option<Box<dyn Write + Send>>,
    receiver: mpsc::Receiver<Vec<u8>>,
    drain: Option<std::thread::JoinHandle<()>>,
    directory: PathBuf,
    output: Vec<u8>,
}
impl Endpoint {
    fn new(name: &str, dimensions: (u16, u16), helper: bool) -> Self {
        let pair = native_pty_system().openpty(size(dimensions)).unwrap();
        let directory =
            std::env::temp_dir().join(format!("showcase-resize-{name}-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let mut command = if helper {
            let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
            command.args(["--exact", "exercise_subprocess", "--ignored", "--nocapture"]);
            command.env("SHOWCASE_RESIZE_HELPER", name);
            command
        } else {
            let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_terminal-showcase"));
            command.args(["--probe", "--check", "--report"]);
            command.arg(directory.join("report.json"));
            command
        };
        command.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = pair.master.take_writer().unwrap();
        let (send, receiver) = mpsc::channel();
        let drain = std::thread::spawn(move || {
            let mut bytes = [0; 8192];
            while let Ok(count) = reader.read(&mut bytes) {
                if count == 0 || send.send(bytes[..count].to_vec()).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            master: pair.master,
            writer: Some(writer),
            receiver,
            drain: Some(drain),
            directory,
            output: Vec::new(),
        }
    }
    fn read(&mut self) {
        if let Ok(bytes) = self.receiver.recv_timeout(Duration::from_millis(10)) {
            self.output.extend(bytes);
        }
    }
    fn send(&mut self, bytes: &[u8]) {
        self.writer.as_mut().unwrap().write_all(bytes).unwrap();
    }
    fn finish(&mut self, deadline: Instant) {
        loop {
            self.read();
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "child failed: {:?}",
                    String::from_utf8_lossy(&self.output)
                );
                while let Ok(bytes) = self.receiver.recv_timeout(Duration::from_millis(20)) {
                    self.output.extend(bytes);
                }
                return;
            }
            assert!(Instant::now() < deadline, "child did not finish");
        }
    }
    fn helper_report(&self) -> Value {
        let marker = b"SHOWCASE_RESIZE_RESULT=";
        let start = self
            .output
            .windows(marker.len())
            .position(|b| b == marker)
            .unwrap()
            + marker.len();
        let end = self.output[start..]
            .iter()
            .position(|b| *b == b'\n')
            .unwrap()
            + start;
        serde_json::from_slice(&self.output[start..end]).unwrap()
    }
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.writer.take();
        if let Some(drain) = self.drain.take() {
            let _ = drain.join();
        }
        let _ = fs::remove_dir_all(&self.directory);
    }
}
fn size((cols, rows): (u16, u16)) -> PtySize {
    PtySize {
        cols,
        rows,
        pixel_width: 0,
        pixel_height: 0,
    }
}
fn capability<'a>(json: &'a Value, id: &str) -> &'a Value {
    json["report"]["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == id)
        .unwrap()
}

// The first eight positions are endpoint replies for the fixed cursor-control
// specimens, not values read from the application's report or implementation.
const FIXED_POSITIONS: [(u16, u16); 8] = [
    (3, 7),
    (4, 8),
    (4, 9),
    (5, 4),
    (6, 3),
    (7, 4),
    (8, 3),
    (2, 4),
];

#[test]
fn probe_check_widening_during_autowrap_cpr_never_fails_stale_geometry() {
    let mut endpoint = Endpoint::new("widen", (100, 30), false);
    let deadline = Instant::now() + Duration::from_secs(6);
    let mut handled = 0;
    let mut margins = false;
    loop {
        endpoint.read();
        if !margins && endpoint.output.windows(7).any(|b| b == b"\x1bP$qr\x1b\\") {
            endpoint.send(b"\x1bP1$r1;30r\x1b\\");
            margins = true;
        }
        let queries = endpoint
            .output
            .windows(4)
            .filter(|b| *b == b"\x1b[6n")
            .count();
        while handled < queries {
            if handled < FIXED_POSITIONS.len() {
                let (row, column) = FIXED_POSITIONS[handled];
                endpoint.send(format!("\x1b[{row};{column}R").as_bytes());
            } else {
                assert_eq!(handled, 8, "a resized suite must stop before a new CPR");
                endpoint.master.resize(size((120, 30))).unwrap();
                // Independently count the actual ASCII cells sent after CUP.
                // A 120-column endpoint leaves 100 As and B on row 9, column 102;
                // the old 100-column expected wrap is no longer evidence of failure.
                let start = endpoint
                    .output
                    .windows(6)
                    .rposition(|b| b == b"\x1b[9;1H")
                    .unwrap()
                    + 6;
                let count = endpoint.output[start..]
                    .iter()
                    .take_while(|b| **b == b'A')
                    .count();
                assert_eq!(count, 100);
                assert_eq!(endpoint.output[start + count], b'B');
                let column = count + 2;
                assert!(column <= 120);
                endpoint.send(format!("\x1b[9;{column}R").as_bytes());
            }
            handled += 1;
        }
        if endpoint.child.try_wait().unwrap().is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "probe never finished");
    }
    endpoint.finish(deadline);
    let json: Value =
        serde_json::from_slice(&fs::read(endpoint.directory.join("report.json")).unwrap()).unwrap();
    assert_eq!(handled, 9);
    assert_eq!(json["report"]["columns"], 120);
    assert_eq!(capability(&json, "resize")["objective_status"], "verified");
    assert_eq!(capability(&json, "autowrap")["objective_status"], "unknown");
    assert_eq!(json["exercises"][8]["outcome"], "invalidated");
}

#[test]
fn shrinking_with_an_outstanding_cpr_preserves_pending_and_deferred_input() {
    let mut endpoint = Endpoint::new("shrink", (100, 30), true);
    let deadline = Instant::now() + Duration::from_secs(4);
    while !endpoint.output.windows(4).any(|b| b == b"\x1b[6n") {
        endpoint.read();
        assert!(Instant::now() < deadline, "CPR was not issued");
    }
    endpoint.master.resize(size((10, 8))).unwrap();
    endpoint.finish(deadline);
    let json = endpoint.helper_report();
    assert_eq!(json["report"]["columns"], 10);
    assert_eq!(json["report"]["rows"], 8);
    assert_eq!(capability(&json, "cursor")["objective_status"], "unknown");
    assert_eq!(capability(&json, "resize")["objective_status"], "verified");
    assert_eq!(json["exercises"].as_array().unwrap().len(), 1);
    assert_eq!(json["exercises"][0]["outcome"], "invalidated");
    assert_eq!(json["pending"], serde_json::json!([true]));
    assert!(
        json["exercises"][0].get("pending_reply").is_none(),
        "pending metadata is not report schema"
    );
    assert_eq!(
        endpoint
            .output
            .windows(4)
            .filter(|b| *b == b"\x1b[6n")
            .count(),
        1
    );
    assert_eq!(json["keys"], serde_json::json!([113]));
    assert_eq!(json["paste"], serde_json::json!([67, 81]));
    assert_eq!(
        json["deferred"],
        serde_json::json!([
            "Osc([55, 55, 55, 59, 114, 101, 116, 97, 105, 110, 101, 100])",
            "Csi([50, 48, 48, 126])",
            "Csi([50, 48, 49, 126])",
            "Csi([51, 59, 55, 82])",
        ])
    );
}

#[test]
fn suite_uses_physical_size_and_distinguishes_consumed_from_timed_out_cpr() {
    let mut endpoint = Endpoint::new("snapshot", (120, 30), true);
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut answered = false;
    loop {
        endpoint.read();
        if !answered && endpoint.output.windows(4).any(|b| b == b"\x1b[6n") {
            endpoint.send(b"\x1b[3;7R");
            answered = true;
        }
        if endpoint.child.try_wait().unwrap().is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "suite did not time out");
    }
    endpoint.finish(deadline);
    let json = endpoint.helper_report();
    assert_eq!(json["report"]["columns"], 120);
    assert_eq!(json["exercises"][0]["outcome"], "passed");
    assert_eq!(json["exercises"][1]["outcome"], "timeout");
    assert_eq!(json["pending"], serde_json::json!([false, true]));
}

#[test]
fn physically_small_viewport_emits_no_request_despite_stale_report_dimensions() {
    let mut endpoint = Endpoint::new("small", (10, 8), true);
    endpoint.finish(Instant::now() + Duration::from_secs(4));
    let json = endpoint.helper_report();
    assert_eq!(json["report"]["columns"], 10);
    assert_eq!(json["report"]["rows"], 8);
    assert_eq!(json["exercises"], serde_json::json!([]));
    assert_eq!(json["pending"], serde_json::json!([]));
    assert!(!endpoint.output.windows(4).any(|b| b == b"\x1b[6n"));
}

#[test]
fn a_resize_round_trip_invalidates_coordinates_even_when_dimensions_match_again() {
    let mut endpoint = Endpoint::new("roundtrip", (100, 30), true);
    let deadline = Instant::now() + Duration::from_secs(4);
    while !endpoint.output.windows(4).any(|bytes| bytes == b"\x1b[6n") {
        endpoint.read();
        assert!(Instant::now() < deadline, "cursor request not emitted");
    }
    endpoint.master.resize(size((120, 30))).unwrap();
    endpoint.master.resize(size((100, 30))).unwrap();
    endpoint.send(b"\x1b[3;99R");
    endpoint.finish(deadline);
    let result = endpoint.helper_report();
    assert_eq!(result["exercises"][0]["outcome"], "invalidated");
    assert_eq!(result["report"]["columns"], 100);
    assert_eq!(result["winch_restored"], true);
    assert_eq!(
        endpoint
            .output
            .windows(4)
            .filter(|bytes| *bytes == b"\x1b[6n")
            .count(),
        1
    );
}

#[test]
#[ignore = "isolated controlling-PTY fixture"]
fn exercise_subprocess() {
    let Ok(scenario) = std::env::var("SHOWCASE_RESIZE_HELPER") else {
        return;
    };
    let old_winch = unsafe { libc::signal(libc::SIGWINCH, libc::SIG_IGN) };
    let terminal = Terminal::new().unwrap();
    let mut report = Report::new(100, 30);
    let mut parser = Parser::new();
    let mut deferred = std::collections::VecDeque::new();
    if scenario == "shrink" {
        // The interactive parser may already own unrelated events and a partial
        // paste terminator when the cursor suite starts its first transaction.
        deferred.extend(parser.feed(b"q\x1b]777;retained\x07\x1b[200~CQ\x1b[201"));
    }
    let results = exercises::run(
        &mut report,
        Duration::from_millis(250),
        &mut parser,
        &mut deferred,
    )
    .unwrap();
    if scenario == "shrink" {
        // The incomplete paste close and the late, untagged CPR still belong to
        // the shared session parser, never to a newly manufactured transaction.
        deferred.extend(parser.feed(b"~\x1b[3;7R"));
    }
    let pending: Vec<_> = results
        .iter()
        .map(|exercise| exercise.pending_reply)
        .collect();
    let mut keys = Vec::new();
    let mut paste = Vec::new();
    let mut retained = Vec::new();
    for event in deferred {
        match event {
            Event::Input(bytes) => keys.extend(bytes),
            Event::PasteData(bytes) => paste.extend(bytes),
            other => retained.push(format!("{other:?}")),
        }
    }
    drop(terminal);
    let winch_restored = unsafe { libc::signal(libc::SIGWINCH, old_winch) } == libc::SIG_IGN;
    println!(
        "SHOWCASE_RESIZE_RESULT={}",
        serde_json::json!({
            "report": report, "exercises": results, "pending": pending,
            "keys": keys, "paste": paste, "deferred": retained,
            "winch_restored": winch_restored,
        })
    );
}

use crate::capability::{Discovery, Report, Status};
use crate::exercises::cursor_position;
use crate::presentation::{self, Demo, View};
use crate::protocol::{Event, Parser};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Check {
    pub name: &'static str,
    pub passed: bool,
}
#[derive(Serialize)]
pub struct Headless {
    pub mode: &'static str,
    pub report: Report,
    pub checks: Vec<Check>,
    pub events: Vec<String>,
    pub queries_hex: String,
}
/// Runs production components against independent, fixed wire expectations.
/// Synthetic endpoint evidence is explicitly labelled and never describes a real terminal.
pub fn run(columns: u16, rows: u16, responses: Option<&[u8]>) -> Headless {
    let mut report = Report::new(columns, rows);
    report.identity = "synthetic endpoint (not live detection)".into();
    let mut discovery = Discovery::new();
    let queries = discovery.queries(0);
    let mut parser = Parser::new();
    let mut events = Vec::new();
    for event in parser.feed(responses.unwrap_or(b"")) {
        discovery.ingest(&event, &mut report, 1);
        events.push(format!("{event:?}"));
    }
    discovery.expire(&mut report, 60_000);
    let mut checks = Vec::new();
    let mut p = Parser::new();
    let first = p.feed(b"\x1b]10;rgb:ffff/");
    let second = p.feed(b"0000/ffff\x1b\\x\x1b[3;7R");
    checks.push(Check {
        name: "fragmented OSC and interleaved CPR/input",
        passed: first.is_empty()
            && second
                .iter()
                .any(|e| matches!(e,Event::Osc(v) if v==b"10;rgb:ffff/0000/ffff"))
            && second
                .iter()
                .any(|e| matches!(e,Event::Csi(v) if v==b"3;7R"))
            && second
                .iter()
                .any(|e| matches!(e,Event::Input(v) if v==b"x")),
    });
    checks.push(Check {
        name: "cursor coordinate validation",
        passed: cursor_position(b"3;7R") == Some((3, 7))
            && cursor_position(b"0;7R").is_none()
            && cursor_position(b"3;7;9R").is_none(),
    });
    checks.push(Check {
        name: "wide/grapheme cell arithmetic",
        passed: presentation::cell_width("界X") == 3
            && presentation::cell_width("e\u{301}\u{302}X") == 2
            && presentation::cell_width("👩\u{200d}💻X") == 3,
    });
    let mut empty = Report::new(columns, rows);
    let mut d = Discovery::new();
    d.queries(0);
    d.expire(&mut empty, 60_000);
    checks.push(Check {
        name: "silence never establishes non-support",
        passed: empty
            .capabilities
            .iter()
            .all(|c| c.status != Status::Unsupported && c.status != Status::Verified),
    });
    let view = View::new(Demo::Overview, true);
    let frame = presentation::render(&report, &view);
    checks.push(Check {
        name: "deterministic screenshot frame",
        passed: frame == presentation::render(&report, &view),
    });
    checks.push(Check {
        name: "unchanged frame emits no terminal bytes",
        passed: presentation::diff(&frame, &frame, false).is_empty(),
    });
    let mut queries_hex = String::with_capacity(queries.len() * 2);
    use std::fmt::Write;
    for byte in queries {
        let _ = write!(queries_hex, "{byte:02x}");
    }
    Headless {
        mode: "headless",
        report,
        checks,
        events,
        queries_hex,
    }
}

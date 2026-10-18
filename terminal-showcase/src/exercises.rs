use crate::capability::{Report, Status};
use crate::protocol::{Event, Parser};
use crate::terminal::Terminal;
use anyhow::Result;
use serde::Serialize;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Serialize)]
pub struct Exercise {
    pub name: String,
    pub capability: String,
    pub expected: (u16, u16),
    pub actual: Option<(u16, u16)>,
    pub outcome: String,
    /// Only an issued, still-unclaimed untagged CPR prevents a new transaction.
    #[serde(skip)]
    pub pending_reply: bool,
}
struct Case {
    name: &'static str,
    capability: &'static str,
    bytes: Vec<u8>,
    expected: (u16, u16),
}
fn cases(columns: u16) -> Vec<Case> {
    let mut cases = vec![
        Case {
            name: "absolute positioning",
            capability: "cursor",
            bytes: b"\x1b[3;7H".to_vec(),
            expected: (3, 7),
        },
        Case {
            name: "relative movement",
            capability: "cursor",
            bytes: b"\x1b[3;7H\x1b[2C\x1b[1D\x1b[2B\x1b[1A".to_vec(),
            expected: (4, 8),
        },
        Case {
            name: "cursor save and restore",
            capability: "save_restore",
            bytes: b"\x1b[4;9H\x1b7\x1b[8;12H\x1b8".to_vec(),
            expected: (4, 9),
        },
        Case {
            name: "double-width cursor arithmetic",
            capability: "wide",
            bytes: "\x1b[5;1H界X".as_bytes().to_vec(),
            expected: (5, 4),
        },
        Case {
            name: "multiple combining marks",
            capability: "combining",
            bytes: "\x1b[6;1He\u{301}\u{302}X".as_bytes().to_vec(),
            expected: (6, 3),
        },
        Case {
            name: "ZWJ grapheme cursor arithmetic",
            capability: "graphemes",
            bytes: "\x1b[7;1H👩\u{200d}💻X".as_bytes().to_vec(),
            expected: (7, 4),
        },
        Case {
            name: "wide-cell overwrite cursor arithmetic (not glyph verification)",
            capability: "overwrite",
            bytes: "\x1b[8;1H界\x1b[8;2HX".as_bytes().to_vec(),
            expected: (8, 3),
        },
        Case {
            name: "origin-mode cursor coordinates",
            capability: "origin",
            bytes: b"\x1b[3;10r\x1b[?6h\x1b[2;4H".to_vec(),
            expected: (2, 4),
        },
    ];
    let mut wrap = b"\x1b[?6l\x1b[r\x1b[?7h\x1b[9;1H".to_vec();
    wrap.extend(std::iter::repeat_n(b'A', usize::from(columns)));
    wrap.push(b'B');
    cases.push(Case {
        name: "autowrap at viewport edge",
        capability: "autowrap",
        bytes: wrap,
        expected: (10, 2),
    });
    cases
}
/// Separate, sequential CPR transactions: no query ids exist in CPR, so never overlap them.
/// Rendering/glyph correctness is deliberately not inferred from cursor replies.
/// The caller owns one parser for the entire stdin session, including query timeouts.
pub fn run(
    report: &mut Report,
    timeout: Duration,
    parser: &mut Parser,
    deferred: &mut VecDeque<Event>,
) -> Result<Vec<Exercise>> {
    let resize_epoch = Terminal::resize_epoch();
    let viewport = observe_viewport(report);
    if viewport.0 < 20 || viewport.1 < 12 {
        return Ok(Vec::new());
    }
    let mut results = Vec::new();
    let mut buffer = [0u8; 4096];
    for case in cases(viewport.0) {
        if Terminal::stopped() {
            break;
        }
        let mut actual = None;
        let mut pending_reply = false;
        let mut invalidated = geometry_changed(report, viewport, resize_epoch);
        if !invalidated {
            Terminal::write(b"\x1b[?6l\x1b[r\x1b[0m")?;
            Terminal::write(&case.bytes)?;
            Terminal::write(b"\x1b[6n")?;
            pending_reply = true;
            let deadline = Instant::now() + timeout;
            while Instant::now() < deadline && !Terminal::stopped() {
                if geometry_changed(report, viewport, resize_epoch) {
                    invalidated = true;
                    break;
                }
                let n = Terminal::read(&mut buffer, 20)?;
                actual = receive_cursor_response(parser, &buffer[..n], deferred);
                pending_reply = actual.is_none();
                // A reply read from the same transaction still cannot prove an
                // expected coordinate calculated for a different physical size.
                if geometry_changed(report, viewport, resize_epoch) {
                    invalidated = true;
                    break;
                }
                if actual.is_some() {
                    break;
                }
            }
        }
        let (status, outcome) = if invalidated {
            (Status::Unknown, "invalidated")
        } else {
            match actual {
                Some(position) if position == case.expected => (Status::Verified, "passed"),
                Some(_) => (Status::Failed, "failed"),
                None => (Status::Unknown, "timeout"),
            }
        };
        let evidence = if invalidated {
            format!(
                "{}: viewport/resize notification changed from {:?}/{} to {:?}/{}; cursor transaction invalidated, received {:?}; no compatibility verdict",
                case.name,
                viewport,
                resize_epoch,
                (report.columns, report.rows),
                Terminal::resize_epoch(),
                actual
            )
        } else {
            format!(
                "{}: expected {:?}, received {:?}; verifies cursor coordinates only",
                case.name, case.expected, actual
            )
        };
        report.note(case.capability, status, &evidence);
        results.push(Exercise {
            name: case.name.into(),
            capability: case.capability.into(),
            expected: case.expected,
            actual,
            outcome: outcome.into(),
            pending_reply,
        });
        if invalidated || pending_reply {
            // An untagged late CPR cannot safely be assigned to the next query.
            // A resize also invalidates this suite's shared geometry snapshot.
            // The caller uses pending_reply, not the outcome, to quarantine replies.
            break;
        }
    }
    Terminal::write(b"\x1b[?6l\x1b[r\x1b[0m\x1b[H")?;
    Ok(results)
}

fn geometry_changed(report: &mut Report, viewport: (u16, u16), resize_epoch: usize) -> bool {
    observe_viewport(report) != viewport || Terminal::resize_epoch() != resize_epoch
}

fn observe_viewport(report: &mut Report) -> (u16, u16) {
    let viewport = Terminal::size();
    if viewport != (report.columns, report.rows) {
        report.columns = viewport.0;
        report.rows = viewport.1;
        report.note(
            "resize",
            Status::Verified,
            "Observed local TTY viewport change while checking cursor exercises; physical geometry updated. Inspect rendering separately.",
        );
    }
    viewport
}

/// Claim the first CPR from one read while retaining other events and incomplete
/// framing for the interactive consumer or the next read of the same transaction.
pub fn receive_cursor_response(
    parser: &mut Parser,
    bytes: &[u8],
    deferred: &mut VecDeque<Event>,
) -> Option<(u16, u16)> {
    let mut actual = None;
    for event in parser.feed(bytes) {
        if actual.is_none() {
            if let Event::Csi(payload) = &event {
                if let Some(position) = cursor_position(payload) {
                    actual = Some(position);
                    continue;
                }
            }
        }
        if deferred.len() < 512 {
            deferred.push_back(event);
        }
    }
    actual
}
pub fn cursor_position(payload: &[u8]) -> Option<(u16, u16)> {
    let body = payload.strip_suffix(b"R")?;
    let text = std::str::from_utf8(body).ok()?;
    let (row, column) = text.split_once(';')?;
    let row = row.parse::<u16>().ok()?;
    let column = column.parse::<u16>().ok()?;
    (row > 0 && column > 0).then_some((row, column))
}
/// Animated raw controls are constrained to the presentation's blank canvas.
/// Operations affect the canvas only; margins/origin are reset before returning.
pub fn control_frame(
    row: u16,
    column: u16,
    width: u16,
    height: u16,
    tick: u64,
    screenshot: bool,
) -> Vec<u8> {
    use std::fmt::Write;
    if width < 20 || height < 4 {
        return Vec::new();
    }
    let bottom = row + height - 1;
    let step = if screenshot { 3 } else { (tick / 8) % 6 };
    let mut out = String::with_capacity(1024);
    let _ = write!(
        out,
        "\x1b[?6l\x1b[{row};{bottom}r\x1b[{row};{column}H\x1b7\x1b[38;2;74;210;204m"
    );
    match step {
        0 => {
            let _ = write!(out, "CUP → CUF → CUB\x1b[4C+\x1b[2D<");
        }
        1 => {
            let _ = write!(out, "abcdefghij\x1b[5D\x1b[2@IN\x1b[2P");
        }
        2 => {
            let _ = write!(
                out,
                "line insertion\x1b[{r};{column}H\x1b[Linserted\x1b[Mdeleted",
                r = row + 1
            );
        }
        3 => {
            let _ = write!(
                out,
                "\x1b[{bottom};{column}Hscroll region ↑\n\x1b[{column}Gretained text\x1b[{row};{column}H\x1bMreverse index ↓"
            );
        }
        4 => {
            let _ = write!(
                out,
                "erase tail XXXXX\x1b[5D\x1b[0Kclean\x1b[{r};{column}H\x1b[2KEL isolated line",
                r = row + 1
            );
        }
        _ => {
            let _ = write!(out, "\x1b[?6h\x1b[1;{column}Horigin relative\x1b[?6l");
        }
    }
    let _ = write!(out, "\x1b8\x1b[r\x1b[?6l\x1b[0m");
    out.into_bytes()
}

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    sync::mpsc,
    time::{Duration, Instant},
};
use terminal_showcase::{
    capability::{Discovery, Report, Status},
    protocol::{Event, Parser},
    terminal::Terminal,
};

const TRACKING: [u16; 5] = [9, 1001, 1000, 1002, 1003];
const ENCODING: [u16; 4] = [1005, 1006, 1015, 1016];
const READY: &str = "SHOWCASE_INPUT_READY";
const RESTORED: &str = "SHOWCASE_RESTORED";

struct ChildGuard(Box<dyn portable_pty::Child + Send + Sync>);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

// Terminal installs process-wide signals and owns fd 0/1. Every guard runs in an
// isolated test process attached to its own PTY, never in the parallel test runner.
#[test]
#[ignore = "isolated PTY fixture, invoked by the parent regressions"]
fn terminal_mode_fixture() {
    let saved = std::env::var("SHOWCASE_MODE_FIXTURE").expect("fixture requires parent PTY");
    let modes = saved
        .split(',')
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let (mode, state) = entry.split_once('=').unwrap();
            (mode.parse().unwrap(), state.parse().unwrap())
        })
        .collect();
    let mut terminal = Terminal::new().unwrap();
    terminal.preserve(&modes, None, None);
    terminal.enable_input(false).unwrap();
    Terminal::write(READY.as_bytes()).unwrap();
    for action in std::env::var("SHOWCASE_PIXEL_ACTIONS").unwrap().bytes() {
        let selected = terminal.pixel_mouse(action == b'1').unwrap();
        Terminal::write(format!("SHOWCASE_PIXEL={}", u8::from(selected)).as_bytes()).unwrap();
    }
    let (columns, rows) = Terminal::size();
    Terminal::write(format!("SHOWCASE_SIZE={columns}x{rows}").as_bytes()).unwrap();
    drop(terminal);
    Terminal::write(RESTORED.as_bytes()).unwrap();
    // Keep the slave alive until the consuming endpoint has captured cleanup.
    // A short-lived child can exit before a macOS PTY reader drains its output.
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut acknowledgment = [0; 1];
    while Terminal::read(&mut acknowledgment, 20).unwrap() == 0 {
        assert!(
            Instant::now() < deadline,
            "cleanup acknowledgment timed out"
        );
    }
}

fn capture(modes: &BTreeMap<u16, u8>, pixels: &str, columns: u16, rows: u16) -> String {
    let pair = native_pty_system()
        .openpty(PtySize {
            cols: columns,
            rows,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        "terminal_mode_fixture",
        "--nocapture",
        "--ignored",
    ]);
    command.env(
        "SHOWCASE_MODE_FIXTURE",
        modes
            .iter()
            .map(|(mode, state)| format!("{mode}={state}"))
            .collect::<Vec<_>>()
            .join(","),
    );
    command.env("SHOWCASE_PIXEL_ACTIONS", pixels);
    let mut child = ChildGuard(pair.slave.spawn_command(command).unwrap());
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (send, recv) = mpsc::channel();
    let drain = std::thread::spawn(move || {
        let mut bytes = [0; 4096];
        loop {
            match reader.read(&mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if send.send(bytes[..count].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut output = Vec::new();
    let mut acknowledged = false;
    loop {
        assert!(
            Instant::now() < deadline,
            "isolated terminal fixture timed out"
        );
        match recv.recv_timeout(Duration::from_millis(20)) {
            Ok(bytes) => {
                output.extend(bytes);
                if !acknowledged
                    && output
                        .windows(RESTORED.len())
                        .any(|bytes| bytes == RESTORED.as_bytes())
                {
                    writer.write_all(b"\n").unwrap();
                    acknowledged = true;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "fixture did not exit");
        std::thread::sleep(Duration::from_millis(5));
    };
    drain.join().unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(status.success(), "fixture failed: {output}");
    assert!(
        output.contains(RESTORED),
        "cleanup was not completed: {output}"
    );
    output
}

fn saved(tracking: u16, encoding: u16) -> BTreeMap<u16, u8> {
    TRACKING
        .into_iter()
        .chain(ENCODING)
        .map(|mode| {
            (
                mode,
                if mode == tracking || mode == encoding {
                    1
                } else {
                    2
                },
            )
        })
        .collect()
}

fn mode_commands(output: &str) -> Vec<(u16, bool)> {
    Parser::new()
        .feed(output.as_bytes())
        .into_iter()
        .filter_map(|event| {
            let Event::Csi(bytes) = event else {
                return None;
            };
            let text = std::str::from_utf8(&bytes).ok()?;
            let enabled = match text.as_bytes().last()? {
                b'h' => true,
                b'l' => false,
                _ => return None,
            };
            let mode = text
                .strip_prefix('?')?
                .get(..text.len() - 2)?
                .parse()
                .ok()?;
            Some((mode, enabled))
        })
        .collect()
}

// Models the consuming terminal's shared protocol selectors: resetting *any*
// member clears the entire selection (as in xterm.js), not independent bits.
fn apply(commands: &[(u16, bool)], tracking: &mut Option<u16>, encoding: &mut Option<u16>) {
    for &(mode, enabled) in commands {
        let selection = if TRACKING.contains(&mode) {
            &mut *tracking
        } else if ENCODING.contains(&mode) {
            &mut *encoding
        } else {
            continue;
        };
        *selection = enabled.then_some(mode);
    }
}

#[test]
fn displaced_mouse_groups_restore_the_saved_selection_last() {
    for (inherited_tracking, inherited_encoding) in [
        (1002, 1005),
        (1003, 1015),
        (1002, 1016),
        (1003, 1016),
        (9, 1005),
        (1001, 1015),
    ] {
        let output = capture(&saved(inherited_tracking, inherited_encoding), "", 80, 24);
        let (running, cleanup) = output.split_once(READY).unwrap();
        let commands = mode_commands(running);
        let enables: Vec<_> = commands
            .iter()
            .filter(|(mode, enabled)| {
                *enabled && (TRACKING.contains(mode) || ENCODING.contains(mode))
            })
            .copied()
            .collect();
        assert_eq!(
            enables,
            [(1006, true), (1003, true)],
            "format precedes one tracking selection"
        );
        let mut tracking = Some(inherited_tracking);
        let mut encoding = Some(inherited_encoding);
        apply(&commands, &mut tracking, &mut encoding);
        assert_eq!((tracking, encoding), (Some(1003), Some(1006)));
        let cleanup = mode_commands(cleanup);
        for group in [&TRACKING[..], &ENCODING[..]] {
            for mode in group {
                assert!(
                    cleanup.contains(&(*mode, false)),
                    "all displaced group members must be cleared"
                );
            }
        }
        apply(&cleanup, &mut tracking, &mut encoding);
        assert_eq!(
            (tracking, encoding),
            (Some(inherited_tracking), Some(inherited_encoding))
        );
    }
}

#[test]
fn unknown_unrecognized_and_permanently_reset_sgr_never_enable_tracking() {
    for state in [None, Some(0), Some(4)] {
        let mut modes = saved(1002, 1005);
        modes.remove(&1006);
        if let Some(state) = state {
            modes.insert(1006, state);
        }
        let output = capture(&modes, "10", 80, 24);
        let (running, cleanup) = output.split_once("SHOWCASE_SIZE=").unwrap();
        let commands = mode_commands(running);
        assert!(!commands.iter().any(
            |(mode, enabled)| *enabled && (TRACKING.contains(mode) || ENCODING.contains(mode))
        ));
        for mode in TRACKING {
            assert!(
                commands.contains(&(mode, false)),
                "inherited tracking must be disabled"
            );
        }
        assert_eq!(running.matches("SHOWCASE_PIXEL=0").count(), 2);
        let mut tracking = Some(1002);
        let mut encoding = Some(1005);
        apply(&commands, &mut tracking, &mut encoding);
        assert_eq!(
            (tracking, encoding),
            (None, None),
            "legacy payload bytes must not reach hotkeys"
        );
        apply(&mode_commands(cleanup), &mut tracking, &mut encoding);
        assert_eq!((tracking, encoding), (Some(1002), Some(1005)));
    }
}

#[test]
fn tracking_selects_only_one_recognized_settable_protocol() {
    for (any, drag, expected) in [(0, 2, Some(1002)), (4, 0, Some(1000)), (0, 4, None)] {
        let mut modes = saved(9, 1005);
        modes.insert(1003, any);
        modes.insert(1002, drag);
        if expected.is_none() {
            modes.insert(1000, 0);
        }
        let output = capture(&modes, "", 80, 24);
        let commands = mode_commands(output.split_once(READY).unwrap().0);
        let enables: Vec<_> = commands
            .into_iter()
            .filter(|(mode, enabled)| *enabled && TRACKING.contains(mode))
            .map(|(mode, _)| mode)
            .collect();
        assert_eq!(enables, expected.into_iter().collect::<Vec<_>>());
    }
}

#[test]
fn pixel_toggle_returns_actual_selection_and_reselects_sgr_after_reset() {
    let output = capture(&saved(1002, 1015), "10", 80, 24);
    let (_, after_input) = output.split_once(READY).unwrap();
    let (pixel, after_pixel) = after_input.split_once("SHOWCASE_PIXEL=1").unwrap();
    let (cell, _) = after_pixel.split_once("SHOWCASE_PIXEL=0").unwrap();
    let mut tracking = Some(1003);
    let mut encoding = Some(1006);
    apply(&mode_commands(pixel), &mut tracking, &mut encoding);
    assert_eq!((tracking, encoding), (Some(1003), Some(1016)));
    apply(&mode_commands(cell), &mut tracking, &mut encoding);
    assert_eq!((tracking, encoding), (Some(1003), Some(1006)));
    for state in [None, Some(0), Some(4)] {
        let mut modes = saved(1002, 1005);
        modes.remove(&1016);
        if let Some(state) = state {
            modes.insert(1016, state);
        }
        let output = capture(&modes, "1", 80, 24);
        let (running, _) = output.split_once("SHOWCASE_SIZE=").unwrap();
        assert!(running.contains("SHOWCASE_PIXEL=0"));
        assert!(!mode_commands(running).contains(&(1016, true)));
        let mut tracking = Some(1002);
        let mut encoding = Some(1005);
        apply(&mode_commands(running), &mut tracking, &mut encoding);
        assert_eq!((tracking, encoding), (Some(1003), Some(1006)));
    }
}

#[test]
fn size_reports_physical_pty_dimensions_beyond_presentation_bounds() {
    let output = capture(&BTreeMap::new(), "", 700, 260);
    assert!(
        output.contains("SHOWCASE_SIZE=700x260"),
        "physical winsize must not be capped: {output}"
    );
}

#[test]
fn discovery_captures_displaced_groups_without_claiming_legacy_mouse_or_cursor_support() {
    let mut discovery = Discovery::new();
    let mut report = Report::new(80, 24);
    let queries = String::from_utf8(discovery.queries(0)).unwrap();
    for mode in TRACKING.into_iter().chain(ENCODING) {
        assert!(queries.contains(&format!("\x1b[?{mode}$p")));
        if mode != 1006 && mode != 1016 {
            assert!(discovery.ingest(
                &Event::Csi(format!("?{mode};1$y").into_bytes()),
                &mut report,
                1
            ));
        }
    }
    assert_eq!(report.get("mouse").unwrap().status, Status::Unknown);
    assert_eq!(report.get("decrqm").unwrap().status, Status::Verified);
    assert!(discovery.ingest(&Event::Csi(b"?1006;0$y".to_vec()), &mut report, 1));
    assert_eq!(report.get("mouse").unwrap().status, Status::Unsupported);
    assert!(discovery.ingest(&Event::Csi(b"?25;0$y".to_vec()), &mut report, 1));
    assert_eq!(
        report.get("cursor_visibility").unwrap().status,
        Status::Unsupported
    );
    assert_eq!(report.get("cursor").unwrap().status, Status::Unknown);
    for mode in [9, 1001, 1005, 1015] {
        assert_eq!(discovery.modes.get(&mode), Some(&1));
    }
}

#[test]
fn correlated_sgr_recognition_advertises_only_the_reporting_format() {
    let mut discovery = Discovery::new();
    let mut report = Report::new(80, 24);
    discovery.queries(0);
    assert!(discovery.ingest(&Event::Csi(b"?1006;2$y".to_vec()), &mut report, 1));
    assert_eq!(report.get("mouse").unwrap().status, Status::Advertised);
    assert_eq!(report.get("mouse_pixel").unwrap().status, Status::Unknown);
    assert_eq!(report.get("cursor").unwrap().status, Status::Unknown);
}

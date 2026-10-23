use crate::Args;
use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeSet, VecDeque};
use std::time::{Duration, Instant};
use terminal_showcase::{
    capability::{Discovery, Report, Status},
    exercises::{self, Exercise},
    graphics::{self, Graphics, InputEvent, KeyKind, KeyProtocol, Protocol},
    presentation::{self, Demo, View},
    protocol::{Event, Parser},
    terminal::Terminal,
};

#[derive(Serialize)]
struct LiveReport<'a> {
    mode: &'static str,
    report: &'a Report,
    exercises: &'a [Exercise],
}
fn export(path: &std::path::Path, report: &Report, exercises: &[Exercise]) -> Result<()> {
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&LiveReport {
            mode: "live",
            report,
            exercises,
        })?,
    )?;
    Ok(())
}
fn elapsed(start: Instant) -> u64 {
    start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}
fn enqueue(queue: &mut VecDeque<Event>, events: Vec<Event>) {
    for event in events {
        if queue.len() == 512 {
            queue.pop_front();
        }
        queue.push_back(event);
    }
}
fn probe(report: &mut Report, parser: &mut Parser) -> Result<(Discovery, VecDeque<Event>)> {
    let mut discovery = Discovery::new();
    let start = Instant::now();
    Terminal::write(&discovery.queries(0))?;
    let mut deferred = VecDeque::new();
    let mut buffer = [0; 8192];
    while start.elapsed() < Duration::from_millis(1400)
        && !discovery.finished()
        && !Terminal::stopped()
    {
        let n = Terminal::read(&mut buffer, 20)?;
        for event in parser.feed(&buffer[..n]) {
            if !discovery.ingest(&event, report, elapsed(start)) && deferred.len() < 128 {
                deferred.push_back(event);
            }
        }
        discovery.expire(report, elapsed(start));
    }
    discovery.expire(report, 60_000);
    Ok((discovery, deferred))
}
fn specimen(report: &mut Report, seen: &mut BTreeSet<&'static str>, id: &'static str) -> bool {
    if !seen.insert(id) {
        return false;
    }
    if report.get(id).is_some_and(|c| c.status == Status::Unknown) {
        report.note(
            id,
            Status::Visual,
            "Specimen emitted; inspect visually and mark P/F. Not protocol verification.",
        );
        return true;
    }
    false
}
/// Recognize top-level SGR parameters, not operands of extended colors.
/// Colon subparameters stay inside one field; semicolon colors consume their
/// mode and palette/RGB operands before the next attribute is considered.
fn has_underline_color(row: &str) -> bool {
    let bytes = row.as_bytes();
    let mut offset = 0;
    while offset + 1 < bytes.len() {
        if bytes[offset..offset + 2] != *b"\x1b[" {
            offset += 1;
            continue;
        }
        let start = offset + 2;
        offset = start;
        while offset < bytes.len() && (0x30..=0x3f).contains(&bytes[offset]) {
            offset += 1;
        }
        if bytes.get(offset) != Some(&b'm') {
            continue;
        }
        let mut fields = row[start..offset].split(';');
        while let Some(field) = fields.next() {
            let parameter = field.split(':').next().unwrap_or("").parse::<u16>();
            if parameter == Ok(58) {
                return true;
            }
            if matches!(parameter, Ok(38 | 48)) && !field.contains(':') {
                let operands = match fields.next() {
                    Some("5") => 1,
                    Some("2") => 3,
                    _ => 0,
                };
                for _ in 0..operands {
                    fields.next();
                }
            }
        }
        offset += 1;
    }
    false
}

fn show_frame(report: &mut Report, seen: &mut BTreeSet<&'static str>, frame: &[String]) -> bool {
    let mut changed = false;
    // Inspect our final, clipped frame, not demo membership: hidden pages and
    // query-only capabilities must not acquire invented visual evidence.
    for (id, needles) in [
        ("ansi16", &["\x1b[30m", "\x1b[90m"][..]),
        ("indexed256", &["\x1b[38;5;", "\x1b[48;5;"]),
        ("truecolor", &["\x1b[38;2;", "\x1b[48;2;"]),
        ("default_colors", &["\x1b[39;49m"]),
        ("attributes", &["\x1b[3m", "\x1b[2m", "\x1b[7m"]),
        ("underlines", &["\x1b[4:", "\x1b[4m"]),
        ("sgr_reset", &["\x1b[22m", "\x1b[27;29;55m"]),
        ("box", &["┼", "┌"]),
        ("blocks", &["▄", "▖"]),
        ("powerline", &["\u{e0b0}", "\u{e0b2}"]),
        ("combining", &["\u{301}"]),
        ("graphemes", &["👩‍💻", "🇫🇷"]),
        ("emoji", &["👩", "🚀"]),
        ("variation", &["♥️", "✈️"]),
        ("wide", &["界", "語"]),
        ("ambiguous", &["Ω"]),
        ("hyperlinks", &["\x1b]8;;https://"]),
    ] {
        if seen.contains(id) {
            continue;
        }
        if frame
            .iter()
            .any(|row| needles.iter().any(|needle| row.contains(needle)))
        {
            changed |= specimen(report, seen, id);
        }
    }
    if !seen.contains("underline_color") && frame.iter().any(|row| has_underline_color(row)) {
        changed |= specimen(report, seen, "underline_color");
    }
    if !seen.contains("braille")
        && frame
            .iter()
            .any(|row| row.chars().any(|c| ('\u{2800}'..='\u{28ff}').contains(&c)))
    {
        changed |= specimen(report, seen, "braille");
    }
    changed
}
/// Plain stdin batching is not a keyboard semantic. Decode scalars without
/// allocating a Vec/event for every ASCII key, and discard only invalid bytes.
struct Inputs<'a> {
    plain: &'a [u8],
    protocol: Option<InputEvent>,
}
impl<'a> Inputs<'a> {
    fn new(event: &'a Event) -> Self {
        match event {
            Event::Input(bytes) => Self {
                plain: bytes,
                protocol: None,
            },
            _ => Self {
                plain: &[],
                protocol: graphics::decode_input(event),
            },
        }
    }
}
impl Iterator for Inputs<'_> {
    type Item = InputEvent;
    fn next(&mut self) -> Option<Self::Item> {
        while !self.plain.is_empty() {
            let count = match self.plain[0] {
                0..=127 => 1,
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf4 => 4,
                _ => {
                    self.plain = &self.plain[1..];
                    continue;
                }
            };
            let Some(bytes) = self.plain.get(..count) else {
                self.plain = &[];
                break;
            };
            let Ok(valid) = std::str::from_utf8(bytes) else {
                self.plain = &self.plain[1..];
                continue;
            };
            let ch = valid.chars().next().expect("nonempty prefix");
            self.plain = &self.plain[ch.len_utf8()..];
            return Some(if ch.is_ascii() {
                graphics::traditional_byte(ch as u8, false).expect("ASCII key")
            } else {
                InputEvent::Text {
                    alt: false,
                    text: ch.to_string(),
                }
            });
        }
        self.protocol.take()
    }
}
fn log(view: &mut View, text: String) {
    if view.input_log.len() == 8 {
        view.input_log.remove(0);
    }
    view.input_log.push(
        text.chars()
            .take(180)
            .map(|c| if c.is_control() { '·' } else { c })
            .collect(),
    );
}
fn command(input: &InputEvent) -> Option<u8> {
    match input {
        InputEvent::Text { text, alt: false } if text.len() == 1 => Some(text.as_bytes()[0]),
        InputEvent::Key {
            code,
            modifiers,
            kind: KeyKind::Press,
            ..
        } => {
            let modifiers = *modifiers & !(64 | 128);
            if modifiers == 4 && *code == 99 {
                return Some(3);
            }
            if modifiers > 1 {
                return None;
            }
            match code {
                57350 => Some(b'<'),
                57351 => Some(b'>'),
                0..=127 => Some(*code as u8),
                _ => None,
            }
        }
        _ => None,
    }
}
fn observe(
    report: &mut Report,
    seen: &mut BTreeSet<&'static str>,
    input: &InputEvent,
    view: &mut View,
    pixel: bool,
) {
    if matches!(
        input,
        InputEvent::Mouse {
            protocol: graphics::MouseProtocol::Legacy,
            ..
        }
    ) {
        if !view.screenshot {
            log(view, format!("{input:?}"));
        }
        return; // Legacy input receipt cannot verify the SGR reporting format.
    }
    // This validates receipt/decoding, not every possible key or pointer behavior.
    let id = match input {
        InputEvent::Focus { .. } => "focus",
        InputEvent::Paste { .. } => "paste",
        InputEvent::Mouse { .. } => "mouse",
        InputEvent::Key {
            protocol: KeyProtocol::Kitty,
            ..
        } => "kitty_keyboard",
        InputEvent::Key {
            protocol: KeyProtocol::ModifyOtherKeys,
            ..
        } => "modify_other_keys",
        _ => "keyboard",
    };
    seen.insert(id);
    report.note(id,Status::Verified,"Received and decoded an actual input event; rendering and other event combinations are unverified.");
    if pixel && matches!(input, InputEvent::Mouse { .. }) {
        seen.insert("mouse_pixel");
        report.note("mouse_pixel", Status::Advertised,
            "Received SGR mouse data while pixel reporting was requested; coordinate units require visual confirmation.");
    }
    if !view.screenshot {
        log(view, format!("{input:?}"));
    }
}
fn select_graphics(report: &Report) -> Option<Protocol> {
    if report
        .get("kitty_graphics")
        .is_some_and(|c| matches!(c.status, Status::Advertised | Status::Verified))
    {
        Some(Protocol::Kitty)
    } else {
        None
    }
}
fn capture_margins(
    report: &mut Report,
    parser: &mut Parser,
    deferred: &mut VecDeque<Event>,
) -> Result<Option<(u16, u16)>> {
    Terminal::write(b"\x1bP$qr\x1b\\")?;
    let deadline = Instant::now() + Duration::from_millis(250);
    let mut buffer = [0; 4096];
    while Instant::now() < deadline && !Terminal::stopped() {
        let count = Terminal::read(&mut buffer, 20)?;
        let mut answer = None;
        for event in parser.feed(&buffer[..count]) {
            if let Event::Dcs(bytes) = &event {
                if let Some(body) = bytes
                    .strip_prefix(b"1$r")
                    .and_then(|b| b.strip_suffix(b"r"))
                {
                    if let Some((top, bottom)) = std::str::from_utf8(body)
                        .ok()
                        .and_then(|s| s.split_once(';'))
                        .and_then(|(a, b)| Some((a.parse::<u16>().ok()?, b.parse::<u16>().ok()?)))
                    {
                        if top > 0 && top < bottom && bottom <= report.rows {
                            report.note("scroll_region",Status::Advertised,"DECRQSS returned current scroll margins; scrolling correctness remains visual.");
                            answer = Some((top, bottom));
                            continue;
                        }
                    }
                }
            }
            if deferred.len() < 512 {
                deferred.push_back(event);
            }
        }
        if answer.is_some() {
            return Ok(answer);
        }
    }
    Ok(None)
}
pub fn run(args: Args, demo: Demo) -> Result<()> {
    let mut terminal = Terminal::new()?;
    let (columns, rows) = Terminal::size();
    let mut report = Report::new(columns, rows);
    let mut parser = Parser::new();
    let (mut discovery, mut deferred) = probe(&mut report, &mut parser)?;
    parser.set_legacy_mouse_bytes(matches!(discovery.modes.get(&1005), Some(0 | 2 | 4)));
    terminal.preserve(
        &discovery.modes,
        discovery.cursor_style,
        discovery.modify_other_keys,
    );
    terminal.preserve_margins(capture_margins(&mut report, &mut parser, &mut deferred)?);
    terminal.enter_screen()?;
    let mut objective = exercises::run(
        &mut report,
        Duration::from_millis(250),
        &mut parser,
        &mut deferred,
    )?;
    if args.probe {
        drop(terminal);
        if let Some(path) = &args.report {
            export(path, &report, &objective)?;
        }
        if args.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&LiveReport {
                    mode: "live",
                    report: &report,
                    exercises: &objective
                })?
            );
        } else {
            println!(
                "Endpoint: {} / {}×{}",
                report.identity, report.columns, report.rows
            );
            for c in &report.capabilities {
                println!("{:<24} {:?}", c.name, c.status);
            }
        }
        if args.check
            && report
                .capabilities
                .iter()
                .any(|cap| cap.objective_status == Status::Failed)
        {
            anyhow::bail!(
                "endpoint failed objective compatibility checks; full evidence remains in the report"
            );
        }
        return Ok(());
    }
    terminal.enable_input(discovery.kitty_flags.is_some())?;
    let mut view = View::new(demo, args.screenshot);
    let mut previous = Vec::new();
    let mut images = Graphics::new();
    let mut graphic = select_graphics(&report);
    let mut graphic_choice = 0;
    // Legacy protocols have no addressable reclamation. One explicit opt-in
    // permits a finite animation, never unbounded automatic raster storage.
    let mut legacy_remaining = 0u8;
    let mut seen = BTreeSet::new();
    specimen(&mut report, &mut seen, "alternate");
    specimen(&mut report, &mut seen, "cursor_visibility");
    let mut pixel = false;
    let mut cpr_pending = objective.last().is_some_and(|case| case.pending_reply);
    let start = Instant::now();
    let mut last_frame = Instant::now();
    let mut last_cycle = Instant::now();
    let mut last_input = Instant::now();
    let mut tick = 42u64;
    let mut cursor_shown = false;
    let mut last_canvas = None;
    let period = Duration::from_secs_f64(1.0 / f64::from(args.fps));
    let mut dirty = true;
    let mut buffer = [0; 8192];
    let loop_result = (|| -> Result<()> {
        loop {
            if Terminal::stopped()
                || args
                    .duration
                    .is_some_and(|seconds| start.elapsed().as_secs_f64() >= seconds)
            {
                break;
            }
            let size = Terminal::size();
            if size != (report.columns, report.rows) {
                Terminal::write(&images.clear())?;
                report.columns = size.0;
                report.rows = size.1;
                report.note("resize",Status::Verified,"Observed local TTY viewport change; layout recomputed. Inspect rendering separately.");
                previous.clear();
                dirty = true;
            }
            let count = Terminal::read(&mut buffer, period.as_millis().clamp(1, 30) as u16)?;
            if count > 0 {
                enqueue(&mut deferred, parser.feed(&buffer[..count]));
                last_input = Instant::now();
            } else if last_input.elapsed() > Duration::from_millis(100) {
                enqueue(&mut deferred, parser.expire());
            }
            while let Some(event) = deferred.pop_front() {
                if discovery.ingest(&event, &mut report, 60_000 + elapsed(start)) {
                    continue;
                }
                if cpr_pending
                    && matches!(&event, Event::Csi(bytes) if exercises::cursor_position(bytes).is_some())
                {
                    cpr_pending = false;
                    log(
                        &mut view,
                        "Discarded a late untagged CPR; R may now start a new suite.".into(),
                    );
                    dirty = true;
                    continue;
                }
                if matches!(&event, Event::PasteData(_)) {
                    seen.insert("paste");
                    report.note("paste", Status::Verified, "Received framed paste data; contents are opaque and never dispatch commands.");
                    if !view.screenshot {
                        log(&mut view, "Paste data received (contents hidden)".into());
                        dirty = true;
                    }
                    continue;
                }
                let inputs = Inputs::new(&event);
                if inputs.plain.is_empty() && inputs.protocol.is_none() {
                    if view.diagnostics && !view.screenshot {
                        log(&mut view, format!("Unmatched protocol/input: {event:?}"));
                        dirty = true;
                    }
                    continue;
                }
                for input in inputs {
                    observe(&mut report, &mut seen, &input, &mut view, pixel);
                    let Some(key) = command(&input) else {
                        if !view.screenshot {
                            dirty = true;
                        }
                        continue;
                    };
                    match key {
                        b'q' | b'Q' | 27 | 3 => return Ok(()),
                        b' ' => {
                            view.paused = !view.paused;
                            last_cycle = Instant::now();
                        }
                        b'<' | b'>' => {
                            let index = Demo::ALL.iter().position(|d| *d == view.demo).unwrap_or(0);
                            let next = if key == b'>' {
                                (index + 1) % Demo::ALL.len()
                            } else {
                                (index + Demo::ALL.len() - 1) % Demo::ALL.len()
                            };
                            Terminal::write(&images.clear())?;
                            view.demo = Demo::ALL[next];
                            last_cycle = Instant::now();
                            previous.clear();
                        }
                        9 => view.expanded = !view.expanded,
                        b'd' | b'D' => view.diagnostics = !view.diagnostics,
                        b'[' | b']' if !view.screenshot => {
                            tick = if key == b']' {
                                tick.saturating_add(180)
                            } else {
                                tick.saturating_sub(180)
                            };
                            view.tick = tick;
                        }
                        b's' | b'S' => {
                            view.screenshot = !view.screenshot;
                            view.tick = 42;
                            last_cycle = Instant::now();
                            Terminal::write(&images.clear())?;
                            previous.clear();
                        }
                        b'p' | b'P' | b'f' | b'F' => {
                            for id in view
                                .demo
                                .capability_ids()
                                .iter()
                                .filter(|id| seen.contains(**id))
                            {
                                report.mark_visual(id, matches!(key, b'p' | b'P'));
                            }
                            let message = format!(
                                "{} visual verdict: {}",
                                view.demo.id(),
                                if matches!(key, b'p' | b'P') {
                                    "PASS"
                                } else {
                                    "FAIL"
                                }
                            );
                            log(&mut view, message);
                        }
                        b'e' | b'E' => {
                            let path = args
                                .report
                                .clone()
                                .unwrap_or_else(|| "terminal-showcase-report.json".into());
                            match export(&path, &report, &objective) {
                                Ok(()) => log(&mut view, format!("Exported {}", path.display())),
                                Err(error) => log(&mut view, format!("Export failed: {error}")),
                            }
                        }
                        b'g' | b'G' if view.demo == Demo::Graphics => {
                            Terminal::write(&images.clear())?;
                            last_canvas = None;
                            graphic = Some(
                                [Protocol::Kitty, Protocol::Sixel, Protocol::Iterm]
                                    [graphic_choice % 3],
                            );
                            graphic_choice += 1;
                            legacy_remaining = if graphic == Some(Protocol::Kitty) {
                                0
                            } else {
                                16
                            };
                            log(&mut view, format!("Explicit visual opt-in: {graphic:?}"));
                        }
                        b'x' | b'X' if view.demo == Demo::Input => {
                            pixel = terminal.pixel_mouse(!pixel)?;
                            view.mouse_pixel_requested = pixel;
                            log(
                                &mut view,
                                format!(
                                    "Mouse coordinates: {}",
                                    if pixel { "pixels" } else { "cells" }
                                ),
                            );
                        }
                        b'r' | b'R' if view.demo == Demo::Control => {
                            if cpr_pending {
                                report.note("cursor", Status::Unknown,
                                    "Previous untagged CPR remains outstanding; no new query was issued. Await its late reply or restart.");
                                log(
                                    &mut view,
                                    "R deferred: an earlier untagged CPR is still outstanding."
                                        .into(),
                                );
                                dirty = true;
                                continue;
                            }
                            Terminal::write(&images.clear())?;
                            objective = exercises::run(
                                &mut report,
                                Duration::from_millis(250),
                                &mut parser,
                                &mut deferred,
                            )?;
                            cpr_pending = objective.last().is_some_and(|case| case.pending_reply);
                            previous.clear();
                        }
                        key if view.demo == Demo::Integration => {
                            if let Some(bytes) =
                                graphics::integration_action(key.to_ascii_uppercase())
                            {
                                if key.eq_ignore_ascii_case(&b't') {
                                    terminal.consent_title()?;
                                }
                                Terminal::write(&bytes)?;
                                let id = match key.to_ascii_uppercase() {
                                    b'C' => "clipboard",
                                    b'N' => "notifications",
                                    b'T' => "title",
                                    _ => "cwd",
                                };
                                specimen(&mut report, &mut seen, id);
                                log(&mut view,"Explicit integration action sent; inspect effect (not verified by emission).".into());
                            }
                        }
                        _ => {}
                    }
                    dirty = true;
                }
            }
            if !view.paused && !view.screenshot && last_cycle.elapsed() >= Duration::from_secs(12) {
                Terminal::write(&images.clear())?;
                let index = Demo::ALL.iter().position(|d| *d == view.demo).unwrap_or(0);
                view.demo = Demo::ALL[(index + 1) % Demo::ALL.len()];
                last_cycle = Instant::now();
                previous.clear();
                dirty = true;
            }
            if !view.paused && !view.screenshot && last_frame.elapsed() >= period {
                tick += 1;
                view.tick = tick;
                dirty = true;
            }
            if !dirty {
                continue;
            }
            let mut frame = presentation::render(&report, &view);
            if frame != previous && show_frame(&mut report, &mut seen, &frame) {
                frame = presentation::render(&report, &view);
            }
            let canvas = presentation::demo_canvas(&report);
            let changed = canvas.is_some_and(|(row, _, _, height)| {
                (usize::from(row - 1)..usize::from(row - 1 + height))
                    .any(|index| previous.get(index) != frame.get(index))
            });
            let stamp = canvas.map(|(row, col, width, height)| {
                (
                    view.demo,
                    if view.screenshot {
                        42
                    } else if view.demo == Demo::Graphics {
                        view.tick / u64::from(args.fps)
                    } else {
                        view.tick / 8
                    },
                    row,
                    col,
                    width,
                    height,
                )
            });
            let update = last_canvas != stamp || changed;
            let raw_update = update
                && canvas.is_some()
                && match view.demo {
                    Demo::Control | Demo::Unicode => true,
                    Demo::Graphics => {
                        graphic.is_some_and(|p| p == Protocol::Kitty || legacy_remaining > 0)
                    }
                    _ => false,
                };
            let mut evidence_changed = false;
            let sync = report
                .get("sync")
                .is_some_and(|c| matches!(c.status, Status::Advertised | Status::Verified))
                && (frame != previous || raw_update);
            if sync && frame != previous {
                specimen(&mut report, &mut seen, "sync");
            }
            if sync {
                Terminal::write(b"\x1b[?2026h")?;
            }
            if cursor_shown && (view.demo != Demo::Control || view.screenshot) {
                Terminal::write(b"\x1b[?25l")?;
                cursor_shown = false;
            }
            Terminal::write(&presentation::diff(&previous, &frame, false))?;
            if let Some((row, col, width, height)) = canvas {
                match view.demo {
                    Demo::Graphics if update => {
                        if let Some(protocol) = graphic
                            .filter(|protocol| *protocol == Protocol::Kitty || legacy_remaining > 0)
                        {
                            if protocol == Protocol::Kitty {
                                terminal.graphics_used();
                            }
                            let id = match protocol {
                                Protocol::Kitty => "kitty_graphics",
                                Protocol::Sixel => "sixel",
                                Protocol::Iterm => "iterm_images",
                            };
                            if changed {
                                Terminal::write(&images.clear())?;
                            }
                            let bytes = images.frame(
                                protocol,
                                if view.screenshot {
                                    42
                                } else {
                                    view.tick * 4 / u64::from(args.fps)
                                },
                                row,
                                col,
                                width,
                                height,
                            );
                            if !bytes.is_empty() {
                                Terminal::write(&bytes)?;
                                if protocol != Protocol::Kitty {
                                    legacy_remaining -= 1;
                                }
                                if images.raster_emitted() {
                                    evidence_changed |= specimen(&mut report, &mut seen, id);
                                }
                                Terminal::write(&presentation::canvas_repair(&report, &frame))?;
                            }
                        }
                    }
                    Demo::Control if update => {
                        Terminal::write(&exercises::control_frame(
                            row,
                            col,
                            width,
                            height,
                            view.tick,
                            view.screenshot,
                        ))?;
                        Terminal::write(&presentation::canvas_repair(&report, &frame))?;
                        let phase = if view.screenshot {
                            3
                        } else {
                            (view.tick / 8) % 6
                        };
                        let ids: &[&str] = match phase {
                            0 => &["cursor"],
                            1 | 2 => &["insert_delete"],
                            3 => &["scroll_region", "reverse_index"],
                            4 => &["erase"],
                            _ => &["origin", "scroll_region"],
                        };
                        for id in ids {
                            evidence_changed |= specimen(&mut report, &mut seen, id);
                        }
                        if discovery.cursor_style.is_some() && !view.screenshot {
                            let style = [2, 4, 6][((view.tick / 24) % 3) as usize];
                            Terminal::write(
                                format!("\x1b[{};{col}H\x1b[{style} q\x1b[?25h", row + height - 1)
                                    .as_bytes(),
                            )?;
                            cursor_shown = true;
                            evidence_changed |= specimen(&mut report, &mut seen, "cursor_shape");
                        }
                    }
                    Demo::Unicode if update => {
                        let bytes = format!(
                            "\x1b[{row};{col}H\x1b[48;2;42;68;88m界\x1b[{row};{c}HX\x1b[{r};{col}He\u{301}\u{302}\x1b[{r};{col}HY\x1b[0m",
                            c = col + 1,
                            r = row + 1
                        );
                        Terminal::write(bytes.as_bytes())?;
                        evidence_changed |= specimen(&mut report, &mut seen, "overwrite");
                    }
                    _ => {}
                }
                last_canvas = stamp;
            }
            if sync {
                Terminal::write(b"\x1b[?2026l")?;
            }
            previous = frame;
            dirty = evidence_changed;
            last_frame = Instant::now();
        }
        Ok(())
    })();
    let cleanup = Terminal::write(&images.clear());
    drop(terminal);
    loop_result?;
    cleanup?;
    if let Some(path) = &args.report {
        export(path, &report, &objective)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_typography_colors_accept_and_export_independent_visual_verdicts() {
        for sgr in [
            "\x1b[4:3;58:2::251:146:128m",
            "\x1b[4;58;5;45m",
            "\x1b[4;58;2;251;146;128m",
        ] {
            let mut report = Report::new(120, 36);
            report.note("da1", Status::Verified, "Independent query reply.");
            let query = serde_json::to_value(report.get("da1").unwrap()).unwrap();
            let rendered = presentation::render(&report, &View::new(Demo::Typography, true));
            let row = rendered.iter().find(|row| row.contains(sgr)).unwrap();
            // Each real rendered specimen independently grants eligibility.
            let mut seen = BTreeSet::new();
            assert!(show_frame(
                &mut report,
                &mut seen,
                std::slice::from_ref(row)
            ));
            assert!(seen.contains("underline_color"), "{sgr:?}");
            assert_eq!(
                report.get("underline_color").unwrap().status,
                Status::Visual
            );
            report.mark_visual("underline_color", true);
            assert_eq!(report.get("underline_color").unwrap().visual, Some(true));
            assert_eq!(
                report.get("underline_color").unwrap().status,
                Status::Visual
            );
            report.mark_visual("underline_color", false);
            let exported = serde_json::to_value(LiveReport {
                mode: "live",
                report: &report,
                exercises: &[],
            })
            .unwrap();
            let color = exported["report"]["capabilities"]
                .as_array()
                .unwrap()
                .iter()
                .find(|capability| capability["id"] == "underline_color")
                .unwrap();
            assert_eq!(color["visual"], false);
            assert_eq!(
                report.get("underline_color").unwrap().status,
                Status::Failed
            );
            assert_eq!(
                serde_json::to_value(report.get("da1").unwrap()).unwrap(),
                query
            );
        }
    }

    #[test]
    fn color_operands_do_not_invent_underline_color_eligibility() {
        for sgr in [
            "\x1b[38;5;58m",
            "\x1b[48;5;58m",
            "\x1b[38;2;58;58;58m",
            "\x1b[48;2;1;58;3m",
            "\x1b[38:2::58:58:58m",
            "\x1b[48:5:58m",
            "\x1b[38;2;1;2;58;4m",
        ] {
            let mut report = Report::new(120, 36);
            let mut seen = BTreeSet::new();
            show_frame(&mut report, &mut seen, &[format!("{sgr}foreground\x1b[0m")]);
            assert!(!seen.contains("underline_color"), "{sgr:?}");
            assert_eq!(
                report.get("underline_color").unwrap().status,
                Status::Unknown
            );
        }
    }

    #[test]
    fn only_retained_typography_pages_and_cells_grant_color_eligibility() {
        for (columns, rows) in [(120, 18), (60, 18), (1, 18), (0, 36), (120, 0)] {
            let mut saw_color_page = false;
            let mut saw_omitted_page = false;
            for page in 0..24 {
                let mut report = Report::new(columns, rows);
                let mut view = View::new(Demo::Typography, false);
                view.tick = page * 180;
                let frame = presentation::render(&report, &view);
                let retained = frame
                    .iter()
                    .any(|row| row.contains("\x1b[4:3;58:") || row.contains("\x1b[4;58;"));
                let mut seen = BTreeSet::new();
                show_frame(&mut report, &mut seen, &frame);
                assert_eq!(
                    seen.contains("underline_color"),
                    retained,
                    "{columns}×{rows} page {page}"
                );
                assert_eq!(
                    report.get("underline_color").unwrap().status,
                    if retained {
                        Status::Visual
                    } else {
                        Status::Unknown
                    }
                );
                saw_color_page |= retained;
                saw_omitted_page |= !retained;
            }
            assert!(saw_omitted_page, "{columns}×{rows}");
            assert_eq!(saw_color_page, columns > 0 && rows > 0, "{columns}×{rows}");
        }
    }

    #[test]
    fn clipping_before_the_color_run_does_not_grant_eligibility() {
        let report = Report::new(120, 36);
        let frame = presentation::render(&report, &View::new(Demo::Typography, true));
        let row = frame
            .iter()
            .find(|row| row.contains("\x1b[4:3;58:"))
            .unwrap();
        let offset = row.find("\x1b[4:3;58:").unwrap();
        let before = presentation::cell_width(&row[..offset]);
        for (width, expected) in [(0, false), (before, false), (before + 1, true)] {
            let mut report = Report::new(120, 36);
            let mut seen = BTreeSet::new();
            show_frame(&mut report, &mut seen, &[presentation::clip(row, width)]);
            assert_eq!(seen.contains("underline_color"), expected, "width {width}");
        }
    }

    fn commands(bytes: &[u8]) -> Vec<u8> {
        Parser::new()
            .feed(bytes)
            .iter()
            .flat_map(Inputs::new)
            .filter_map(|input| command(&input))
            .collect()
    }

    #[test]
    fn keyboard_batching_lock_bits_and_bad_utf8_do_not_hide_exit_or_consent_keys() {
        assert_eq!(commands("界q".as_bytes()), [b'q']);
        assert_eq!(commands("界\u{3}".as_bytes()), [3]);
        assert_eq!(commands(b"\xffCq"), [b'C', b'q']);
        assert_eq!(
            commands(b"\x1b[113;65u\x1b[32;129u\x1b[9;193u\x1b[99;197u"),
            [b'q', b' ', 9, 3]
        );
        assert!(commands(b"\x1b[113;5u\x1b[113;1:2u\x1b[113;1:3u").is_empty());
    }

    #[test]
    fn mouse_coordinates_cannot_invoke_clipboard_pause_or_exit_actions() {
        assert!(commands(b"\x1b[M Cq\x1b[M Pq").is_empty());
        assert_eq!(commands(b"\x1b[M Cqq"), [b'q']);
    }

    #[test]
    fn malformed_or_expired_reply_tails_cannot_be_consent_keys() {
        assert_eq!(commands(b"\x1b[?25$5Cq"), [b'q']);
        for prefix in [b"\x1b]partial".as_slice(), b"\x1bPpartial", b"\x1b_partial"] {
            let mut parser = Parser::new();
            assert!(parser.feed(prefix).is_empty());
            assert!(matches!(parser.expire().as_slice(), [Event::Malformed(_)]));
            let actions: Vec<_> = parser
                .feed(b"C\x1b\\q")
                .iter()
                .flat_map(Inputs::new)
                .filter_map(|input| command(&input))
                .collect();
            assert_eq!(actions, [b'q']);
            let mut interrupted = Parser::new();
            interrupted.feed(prefix);
            interrupted.expire();
            assert_eq!(interrupted.feed(&[3]), [Event::Input(vec![3])]);
        }
    }

    #[test]
    fn sgr_mouse_receipt_does_not_prove_requested_pixel_units() {
        let mut report = Report::new(120, 36);
        report.note(
            "mouse_pixel",
            Status::Advertised,
            "DECRQM claims pixel mode.",
        );
        let mut seen = BTreeSet::new();
        let mut view = View::new(Demo::Input, false);
        let event = Parser::new().feed(b"\x1b[<0;280;140M").remove(0);
        let input = graphics::decode_input(&event).unwrap();
        observe(&mut report, &mut seen, &input, &mut view, true);
        assert_eq!(
            report.get("mouse").unwrap().objective_status,
            Status::Verified
        );
        assert_eq!(
            report.get("mouse_pixel").unwrap().objective_status,
            Status::Advertised
        );
        assert!(seen.contains("mouse_pixel"));
    }

    #[test]
    fn unanswered_queries_and_unexercised_input_resize_stay_unknown() {
        let mut report = Report::new(120, 36);
        let mut seen = BTreeSet::new();
        for demo in Demo::ALL {
            let frame = presentation::render(&report, &View::new(demo, true));
            show_frame(&mut report, &mut seen, &frame);
        }
        for id in [
            "da1",
            "da2",
            "version",
            "xtgettcap",
            "decrqm",
            "palette",
            "resize",
            "mouse_pixel",
            "mouse",
            "focus",
            "paste",
            "kitty_keyboard",
            "modify_other_keys",
            "clipboard",
            "title",
            "notifications",
            "cwd",
        ] {
            assert_eq!(report.get(id).unwrap().status, Status::Unknown, "{id}");
            assert!(!seen.contains(id));
        }
        assert_eq!(report.get("truecolor").unwrap().status, Status::Visual);
        assert_eq!(report.get("hyperlinks").unwrap().status, Status::Visual);
    }

    #[test]
    fn sixel_advertisement_never_grants_legacy_image_consent() {
        let mut report = Report::new(120, 36);
        report.note("sixel", Status::Advertised, "DA1 claims Sixel.");
        assert_eq!(select_graphics(&report), None);
        report.note(
            "kitty_graphics",
            Status::Advertised,
            "Matched nonplacing query.",
        );
        assert_eq!(select_graphics(&report), Some(Protocol::Kitty));
    }
}

#[test]
fn traditional_control_keys_have_normalized_live_inspector_fields() {
    let event = Event::Input(vec![0, 1, 8, 10, 16, 28]);
    let fields: Vec<_> = Inputs::new(&event)
        .map(|input| match input {
            InputEvent::Key {
                code, modifiers, ..
            } => (code, modifiers),
            other => panic!("expected traditional key, got {other:?}"),
        })
        .collect();
    assert_eq!(
        fields,
        [(32, 4), (97, 4), (127, 0), (13, 0), (112, 4), (92, 4)]
    );
}

#[test]
fn reply_wrappers_preserve_paste_and_partial_response_provenance() {
    fn wrap(bytes: &[u8]) -> Vec<u8> {
        let mut out = b"\x1bPtmux;".to_vec();
        for &byte in bytes {
            if byte == 27 {
                out.push(27);
            }
            out.push(byte);
        }
        out.extend_from_slice(b"\x1b\\");
        out
    }
    for wrapped_payload in [false, true] {
        let mut parser = Parser::new();
        let mut events = parser.feed(&wrap(b"\x1b[200~"));
        events.extend(parser.feed(&if wrapped_payload {
            wrap(b"CQ")
        } else {
            b"CQ".to_vec()
        }));
        events.extend(parser.feed(&wrap(b"\x1b[201~")));
        events.extend(parser.feed(b"q"));
        let actions: Vec<_> = events
            .iter()
            .flat_map(Inputs::new)
            .filter_map(|input| command(&input))
            .collect();
        assert_eq!(actions, [b'q'], "wrapped payload={wrapped_payload}");
    }
    let mut parser = Parser::new();
    let mut events = parser.feed(&wrap(b"\x1b]unknown;"));
    events.extend(parser.feed(&wrap(b"C\x1b\\")));
    events.extend(parser.feed(b"q"));
    let actions: Vec<_> = events
        .iter()
        .flat_map(Inputs::new)
        .filter_map(|input| command(&input))
        .collect();
    assert_eq!(actions, [b'q']);
}

#[test]
fn reply_malformed_ss3_payloads_cannot_be_consent_keys() {
    for bytes in [b"\x1bO1;2Cq".as_slice(), b"\x1bO Qq"] {
        let events = Parser::new().feed(bytes);
        let actions: Vec<_> = events
            .iter()
            .flat_map(Inputs::new)
            .filter_map(|input| command(&input))
            .collect();
        assert_eq!(actions, [b'q']);
    }
}

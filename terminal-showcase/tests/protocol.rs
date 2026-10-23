use terminal_showcase::capability::{Discovery, QUERY_TIMEOUT_MS, Report, Status};
use terminal_showcase::protocol::{Event, MAX_SEQUENCE, Parser};

fn responses(discovery: &mut Discovery, report: &mut Report, bytes: &[u8], now: u64) -> Vec<Event> {
    Parser::new()
        .feed(bytes)
        .into_iter()
        .filter(|event| !discovery.ingest(event, report, now))
        .collect()
}

#[test]
fn fragmented_replies_preserve_interleaved_utf8_and_keys() {
    let mut parser = Parser::new();
    assert_eq!(parser.feed(b"a\x1b[?1;"), vec![Event::Input(b"a".to_vec())]);
    assert_eq!(
        parser.feed(b"2c\x1bOA"),
        vec![Event::Csi(b"?1;2c".to_vec()), Event::Escape(b"OA".to_vec())]
    );
    let text = "🙂界".as_bytes();
    let mut recovered = Vec::new();
    for byte in text {
        for event in parser.feed(&[*byte]) {
            match event {
                Event::Input(bytes) => recovered.extend(bytes),
                other => panic!("unexpected {other:?}"),
            }
        }
    }
    assert_eq!(recovered, text);
}

#[test]
fn bel_only_terminates_osc_and_st_terminates_other_strings() {
    let mut parser = Parser::new();
    assert_eq!(
        parser.feed(b"\x1b]10;rgb:ffff/0000/0000\x07"),
        vec![Event::Osc(b"10;rgb:ffff/0000/0000".to_vec())]
    );
    assert!(parser.feed(b"\x1bP>|name\x07").is_empty());
    assert_eq!(
        parser.feed(b"\x1b\\"),
        vec![Event::Dcs(b">|name\x07".to_vec())]
    );
    assert!(parser.feed(b"\x1b_Gi=77331;OK\x1b").is_empty());
    assert_eq!(
        parser.feed(b"\\"),
        vec![Event::Apc(b"Gi=77331;OK".to_vec())]
    );
}

#[test]
fn timeout_cancel_and_malformed_recover_without_inventing_replies() {
    let mut parser = Parser::new();
    assert!(parser.feed(b"\x1b").is_empty());
    assert_eq!(parser.expire(), vec![Event::Input(vec![27])]);
    assert!(parser.feed(b"\x1b[12;").is_empty());
    assert!(matches!(parser.expire().as_slice(), [Event::Malformed(_)]));
    assert!(
        matches!(parser.feed(b"\x1b[12\x18x").as_slice(), [Event::Malformed(_), Event::Input(bytes)] if bytes == b"\x18x")
    );
    assert!(matches!(
        parser.feed(b"\x1b[1$2y").first(),
        Some(Event::Malformed(_))
    ));
    assert_eq!(parser.feed(b"q"), vec![Event::Input(vec![b'q'])]);
}

#[test]
fn exact_limit_and_overflow_discard_until_terminator() {
    let mut parser = Parser::new();
    let mut exact = b"\x1b]".to_vec();
    exact.extend(vec![b'x'; MAX_SEQUENCE]);
    exact.push(7);
    assert!(
        matches!(parser.feed(&exact).as_slice(), [Event::Osc(bytes)] if bytes.len() == MAX_SEQUENCE)
    );
    let mut overflow = b"\x1b_".to_vec();
    overflow.extend(vec![b'x'; MAX_SEQUENCE + 100]);
    overflow.extend_from_slice(b"\x07junk\x1b\\z");
    assert!(
        matches!(parser.feed(&overflow).as_slice(), [Event::Malformed(_), Event::Input(bytes)] if bytes == b"z")
    );
    assert_eq!(parser.expire(), vec![]);
}

fn wrap(bytes: &[u8]) -> Vec<u8> {
    let mut wrapped = b"\x1bPtmux;".to_vec();
    for byte in bytes {
        wrapped.push(*byte);
        if *byte == 27 {
            wrapped.push(27);
        }
    }
    wrapped.extend_from_slice(b"\x1b\\");
    wrapped
}

#[test]
fn nested_tmux_wrappers_are_unquoted_but_unknown_strings_survive() {
    let sequence = wrap(&wrap(b"\x1b[?1;4c\x1b_Gi=77331;OK\x1b\\"));
    let mut parser = Parser::new();
    let mut events = Vec::new();
    for chunk in sequence.chunks(3) {
        events.extend(parser.feed(chunk));
    }
    assert_eq!(
        events,
        vec![
            Event::Csi(b"?1;4c".to_vec()),
            Event::Apc(b"Gi=77331;OK".to_vec())
        ]
    );
    assert_eq!(
        parser.feed(b"\x1bPunknown\x1b\\"),
        vec![Event::Dcs(b"unknown".to_vec())]
    );
    let mut too_deep = b"\x1b[c".to_vec();
    for _ in 0..8 {
        too_deep = wrap(&too_deep);
    }
    assert!(
        parser
            .feed(&too_deep)
            .iter()
            .any(|event| matches!(event, Event::Malformed(_)))
    );
}

#[test]
fn discovery_is_read_only_and_correlation_leaves_unknown_input_available() {
    let mut discovery = Discovery::new();
    let mut report = Report::new(80, 24);
    assert!(!discovery.ingest(&Event::Csi(b"?1;4c".to_vec()), &mut report, 0));
    let queries = discovery.queries(100);
    assert!(queries.windows(4).any(|bytes| bytes == b"\x1b[?u"));
    assert!(queries.windows(5).any(|bytes| bytes == b"\x1b[?4m"));
    assert!(queries.windows(4).all(|bytes| bytes != b"\x1b[>4"));
    assert!(queries.windows(6).any(|bytes| bytes == b"\x1bP$q q"));
    assert!(discovery.queries(101).is_empty());
    let leftovers = responses(
        &mut discovery,
        &mut report,
        b"x\x1b[?999;1$y\x1b_Gi=42;OK\x1b\\\x1b[?1;4c",
        101,
    );
    assert_eq!(leftovers.len(), 3);
    assert_eq!(report.get("da1").unwrap().status, Status::Verified);
    assert_eq!(report.get("sixel").unwrap().status, Status::Advertised);
    assert!(!discovery.ingest(&Event::Csi(b"?1;4c".to_vec()), &mut report, 102));
    assert_eq!(
        report.get("kitty_graphics").unwrap().status,
        Status::Unknown
    );
}

#[test]
fn malformed_semantics_do_not_consume_pending_requests() {
    let mut discovery = Discovery::new();
    let mut report = Report::new(80, 24);
    discovery.queries(0);
    for event in [
        Event::Csi(b"?7;9$y".to_vec()),
        Event::Csi(b">1;2c".to_vec()),
        Event::Csi(b"?nopeu".to_vec()),
        Event::Osc(b"10;rgb:zz/00/00".to_vec()),
        Event::Dcs(b"1+r436f=3".to_vec()),
        Event::Apc(b"Gi=77331;OK\x07".to_vec()),
    ] {
        assert!(!discovery.ingest(&event, &mut report, 1), "{event:?}");
    }
    assert!(responses(&mut discovery, &mut report, b"\x1b[?7;2$y\x1b[>1;2;0c\x1b[?3u\x1b]10;rgb:ff/00/00\x07\x1bP1+r436f=323536\x1b\\\x1b_Gi=77331;OK\x1b\\", 2).is_empty());
    assert_eq!(discovery.modes.get(&7), Some(&2));
    assert_eq!(discovery.kitty_flags, Some(3));
    assert_eq!(report.get("indexed256").unwrap().status, Status::Advertised);
    assert_eq!(
        report.get("kitty_graphics").unwrap().status,
        Status::Advertised
    );
}

#[test]
fn explicit_negative_responses_are_not_timeout_negatives() {
    let mut discovery = Discovery::new();
    let mut report = Report::new(80, 24);
    discovery.queries(0);
    assert!(
        responses(
            &mut discovery,
            &mut report,
            b"\x1b[?2026;0$y\x1b_Gi=77331;ENOTSUP:unsupported\x1b\\",
            1
        )
        .is_empty()
    );
    assert_eq!(report.get("sync").unwrap().status, Status::Unsupported);
    assert_eq!(
        report.get("kitty_graphics").unwrap().status,
        Status::Unsupported
    );
    discovery.expire(&mut report, QUERY_TIMEOUT_MS);
    assert!(discovery.finished());
    assert_eq!(
        report.get("kitty_keyboard").unwrap().status,
        Status::Unknown
    );
    assert!(!report.get("kitty_keyboard").unwrap().evidence.is_empty());
    assert!(!discovery.ingest(
        &Event::Csi(b"?0u".to_vec()),
        &mut report,
        QUERY_TIMEOUT_MS + 1
    ));
}

#[test]
fn mode_and_cursor_reports_preserve_states_not_rendering_claims() {
    let mut discovery = Discovery::new();
    let mut report = Report::new(80, 24);
    discovery.queries(0);
    for mode in [
        6, 7, 25, 1000, 1002, 1003, 1004, 1006, 1016, 1049, 2004, 2026,
    ] {
        assert!(discovery.ingest(
            &Event::Csi(format!("?{mode};2$y").into_bytes()),
            &mut report,
            1
        ));
    }
    assert!(
        responses(
            &mut discovery,
            &mut report,
            b"\x1bP1$r5 q\x1b\\\x1b[>4;2m",
            1
        )
        .is_empty()
    );
    assert_eq!(discovery.cursor_style, Some(5));
    assert_eq!(discovery.modify_other_keys, Some(2));
    for id in [
        "autowrap",
        "origin",
        "cursor_visibility",
        "alternate",
        "mouse",
        "mouse_pixel",
        "paste",
        "focus",
        "sync",
        "cursor_shape",
        "modify_other_keys",
    ] {
        assert_eq!(report.get(id).unwrap().status, Status::Advertised, "{id}");
    }
}

#[test]
fn visual_failure_has_precedence_without_erasing_objective_evidence() {
    let mut report = Report::new(80, 24);
    report.mark_visual("kitty_graphics", false);
    report.note("kitty_graphics", Status::Advertised, "correlated query OK");
    assert_eq!(report.get("kitty_graphics").unwrap().status, Status::Failed);
    assert_eq!(report.get("kitty_graphics").unwrap().visual, Some(false));
    report.mark_visual("kitty_graphics", true);
    assert_eq!(
        report.get("kitty_graphics").unwrap().status,
        Status::Advertised
    );
    assert_eq!(report.get("kitty_graphics").unwrap().visual, Some(true));
    report.mark_visual("braille", true);
    assert_eq!(report.get("braille").unwrap().status, Status::Visual);
    report.note("braille", Status::Unknown, "no objective query");
    assert_eq!(report.get("braille").unwrap().status, Status::Visual);
}

#[test]
fn reports_have_bounded_sanitized_evidence() {
    let mut report = Report::new(120, 36);
    for _ in 0..100 {
        report.note(
            "version",
            Status::Verified,
            &format!("{}\x1b]52;attack\x07\n\u{202e}", "界".repeat(200)),
        );
    }
    let evidence = &report.get("version").unwrap().evidence;
    assert!(evidence.len() <= 8);
    assert!(evidence.iter().all(|line| line.len() <= 256
        && !line.chars().any(char::is_control)
        && !line.contains('\u{202e}')));
    let mut discovery = Discovery::new();
    discovery.queries(0);
    assert!(discovery.ingest(
        &Event::Dcs(b">|name\x07\n\x1b]52;bad".to_vec()),
        &mut report,
        1
    ));
    assert!(!report.identity.chars().any(char::is_control));
    assert!(report.identity.len() <= 256);
}

#[test]
fn every_fragment_boundary_has_identical_framing() {
    let bytes = b"\x1b[?7;2$y\x1b]4;0;rgb:aaaa/bbbb/cccc\x1b\\\x1bP1+r524742=38\x1b\\\x1b_Gi=77331;OK\x1b\\";
    let expected = Parser::new().feed(bytes);
    for boundary in 0..=bytes.len() {
        let mut parser = Parser::new();
        let mut actual = parser.feed(&bytes[..boundary]);
        actual.extend(parser.feed(&bytes[boundary..]));
        actual.extend(parser.expire());
        assert_eq!(actual, expected, "split at {boundary}");
    }
}

#[test]
fn c1_frames_do_not_steal_utf8_continuations_in_text_or_strings() {
    let mut parser = Parser::new();
    assert_eq!(
        parser.feed(b"\x9b?7;2$y\x9d10;rgb:1/2/3\x9c"),
        vec![
            Event::Csi(b"?7;2$y".to_vec()),
            Event::Osc(b"10;rgb:1/2/3".to_vec())
        ]
    );
    let text = "ЛĜü".as_bytes();
    assert_eq!(parser.feed(text), vec![Event::Input(text.to_vec())]);
    let version = ">|ЛĜü";
    let mut bytes = b"\x1bP".to_vec();
    bytes.extend_from_slice(version.as_bytes());
    bytes.extend_from_slice(b"\x1b\\");
    let mut events = Vec::new();
    for byte in bytes {
        events.extend(parser.feed(&[byte]));
    }
    assert_eq!(events, vec![Event::Dcs(version.as_bytes().to_vec())]);
    assert_eq!(
        parser.feed(b"\x1b^private\x1b\\\x1bXsos\x1b\\"),
        vec![
            Event::Escape(b"^private".to_vec()),
            Event::Escape(b"Xsos".to_vec())
        ]
    );
}

#[test]
fn complete_query_batch_can_finish_without_waiting_for_deadline() {
    let mut discovery = Discovery::new();
    let mut report = Report::new(80, 24);
    discovery.queries(0);
    let bytes = b"\x1b[?1;2c\x1b[>0;400;0c\x1bP>|fixture 1.0\x1b\\\x1bP1+r436f=323536;524742=38;544e=66697874757265\x1b\\\x1b]10;rgb:ffff/ffff/ffff\x07\x1b]11;rgb:0000/0000/0000\x1b\\\x1b]4;0;rgb:0000/0000/0000\x07\x1b_Gi=77331;OK\x1b\\\x1b[?0u\x1b[>4;0m\x1bP1$r2 q\x1b\\";
    assert!(responses(&mut discovery, &mut report, bytes, 1).is_empty());
    assert_eq!(report.identity, "fixture 1.0");
    assert_eq!(report.get("truecolor").unwrap().status, Status::Advertised);
    for mode in [
        6, 7, 9, 25, 1000, 1001, 1002, 1003, 1004, 1005, 1006, 1015, 1016, 1049, 2004, 2026,
    ] {
        assert!(discovery.ingest(
            &Event::Csi(format!("?{mode};1$y").into_bytes()),
            &mut report,
            1
        ));
    }
    assert!(discovery.finished());
    discovery.expire(&mut report, QUERY_TIMEOUT_MS);
    assert!(
        report
            .get("version")
            .unwrap()
            .evidence
            .iter()
            .all(|text| !text.contains("timed out"))
    );
}

#[test]
fn compound_termcap_replies_are_validated_and_correlated_atomically() {
    let mut discovery = Discovery::new();
    let mut report = Report::new(80, 24);
    discovery.queries(0);
    // Unknown names, odd hex, duplicates and unattributable bare failures stay public.
    for bytes in [
        b"1+r436f=323536;5a5a=31".as_slice(),
        b"1+r436f=323536;524742=3",
        b"1+r436f=323536;436f=323536",
        b"0+r",
    ] {
        assert!(!discovery.ingest(&Event::Dcs(bytes.to_vec()), &mut report, 1));
    }
    assert!(discovery.ingest(
        &Event::Dcs(b"1+r436f=323536;524742=".to_vec()),
        &mut report,
        2
    ));
    assert_eq!(report.get("truecolor").unwrap().status, Status::Advertised);
    assert_eq!(report.get("indexed256").unwrap().status, Status::Advertised);
    assert!(discovery.ingest(&Event::Dcs(b"0+r544e".to_vec()), &mut report, 2));
    assert!(!discovery.ingest(&Event::Dcs(b"1+r544e=666f6f".to_vec()), &mut report, 3));
}

#[test]
fn deadline_boundary_rejects_late_replies_and_unknown_parameters() {
    let mut discovery = Discovery::new();
    let mut report = Report::new(80, 24);
    discovery.queries(100);
    assert!(!discovery.ingest(&Event::Osc(b"4;1;rgb:0/0/0".to_vec()), &mut report, 101));
    assert!(!discovery.ingest(&Event::Csi(b"99;99R".to_vec()), &mut report, 101)); // parent owns CPR
    assert!(discovery.ingest(
        &Event::Csi(b"?7;4$y".to_vec()),
        &mut report,
        100 + QUERY_TIMEOUT_MS - 1
    ));
    assert!(!discovery.ingest(
        &Event::Csi(b"?6;1$y".to_vec()),
        &mut report,
        100 + QUERY_TIMEOUT_MS
    ));
    assert_eq!(discovery.modes.get(&7), Some(&4));
    assert_eq!(discovery.modes.get(&6), None);
    assert!(discovery.finished());
}

#[test]
fn related_mode_queries_cannot_erase_an_existing_advertisement() {
    let mut discovery = Discovery::new();
    let mut report = Report::new(80, 24);
    discovery.queries(0);
    assert!(discovery.ingest(&Event::Csi(b"?1006;2$y".to_vec()), &mut report, 1));
    assert!(discovery.ingest(&Event::Csi(b"?1003;0$y".to_vec()), &mut report, 1));
    assert_eq!(report.get("mouse").unwrap().status, Status::Advertised);
    assert!(discovery.ingest(&Event::Dcs(b"0$r".to_vec()), &mut report, 1));
    assert_eq!(discovery.cursor_style, None);
    assert_eq!(
        report.get("cursor_shape").unwrap().status,
        Status::Unsupported
    );
}

#[test]
fn evidence_verdicts_and_schema_survive_export_and_import() {
    let mut report = Report::new(80, 24);
    report.note("kitty_graphics", Status::Advertised, "valid query reply");
    report.mark_visual("kitty_graphics", false);
    for index in 0..100 {
        report.note(
            "version",
            Status::Verified,
            &format!("reply {index}: \x1b]52;unsafe\x07\u{202e}"),
        );
    }
    assert_eq!(report.get("version").unwrap().evidence.len(), 8);
    assert!(
        report
            .get("version")
            .unwrap()
            .evidence
            .iter()
            .all(|line| !line.chars().any(char::is_control) && !line.contains('\u{202e}'))
    );
    let exported = serde_json::to_string(&report).unwrap();
    let mut imported: Report = serde_json::from_str(&exported).unwrap();
    assert_eq!(
        imported.get("kitty_graphics").unwrap().objective_status,
        Status::Advertised
    );
    assert_eq!(
        imported.get("kitty_graphics").unwrap().status,
        Status::Failed
    );
    imported.mark_visual("kitty_graphics", true);
    assert_eq!(
        imported.get("kitty_graphics").unwrap().status,
        Status::Advertised
    );
}

#[test]
fn fragmented_alt_utf8_is_one_escape_event_not_a_c1_reply() {
    let mut parser = Parser::new();
    assert!(parser.feed(b"\x1b\xd0").is_empty());
    assert_eq!(
        parser.feed(b"\x9b"),
        vec![Event::Escape("Л".as_bytes().to_vec())]
    );
    assert_eq!(parser.feed(b"x"), vec![Event::Input(b"x".to_vec())]);
}

#[test]
fn weaker_evidence_and_visual_pass_do_not_erase_objective_observations() {
    let mut report = Report::new(80, 24);
    report.note(
        "cursor",
        Status::Verified,
        "independent expected CPR positions matched",
    );
    report.note(
        "cursor",
        Status::Advertised,
        "DECRPM cursor visibility recognized",
    );
    report.note("cursor", Status::Unknown, "auxiliary query timed out");
    assert_eq!(report.get("cursor").unwrap().status, Status::Verified);
    report.note(
        "cursor",
        Status::Failed,
        "independent expected position mismatched",
    );
    report.mark_visual("cursor", true);
    report.note("cursor", Status::Advertised, "mode still recognized");
    assert_eq!(report.get("cursor").unwrap().status, Status::Failed);
    assert_eq!(report.get("cursor").unwrap().visual, Some(true));
}

#[test]
fn graphics_errors_are_printable_protocol_messages_not_assumed_brand_support() {
    let mut discovery = Discovery::new();
    let mut report = Report::new(80, 24);
    discovery.queries(0);
    assert!(discovery.ingest(
        &Event::Apc(b"Gi=77331;image decoding failed".to_vec()),
        &mut report,
        1
    ));
    assert_eq!(report.get("kitty_graphics").unwrap().status, Status::Failed);
    assert_eq!(report.get("kitty_graphics").unwrap().visual, None);
}

#[test]
fn csi_overflow_discard_does_not_turn_tail_into_keystrokes() {
    let mut parser = Parser::new();
    let mut bytes = b"\x1b[".to_vec();
    bytes.extend(vec![b'1'; MAX_SEQUENCE + 1]);
    bytes.extend_from_slice(b";4cnext");
    assert!(
        matches!(parser.feed(&bytes).as_slice(), [Event::Malformed(_), Event::Input(input)] if input == b"next")
    );
    assert!(parser.expire().is_empty());
}

#[test]
fn escape_overflow_uses_escape_final_range_and_resumes_input() {
    for padding in [MAX_SEQUENCE, MAX_SEQUENCE + 1] {
        let mut parser = Parser::new();
        let mut bytes = vec![27];
        bytes.extend(vec![b' '; padding]);
        bytes.extend_from_slice(b"0next");
        assert!(
            matches!(parser.feed(&bytes).as_slice(), [Event::Malformed(_), Event::Input(input)] if input == b"next")
        );
        assert!(parser.expire().is_empty());
    }
}

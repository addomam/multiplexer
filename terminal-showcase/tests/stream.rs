use std::collections::VecDeque;
use terminal_showcase::exercises::receive_cursor_response;
use terminal_showcase::graphics::{InputEvent, MouseKind, MouseProtocol, decode_input};
use terminal_showcase::protocol::{Event, Parser};

#[test]
fn fragmented_legacy_mouse_coordinates_never_become_consent_or_exit_keys() {
    for introducer in [b"\x1b[M".as_slice(), b"\x9bM".as_slice()] {
        for (payload, x, y) in [(b" CP", 35, 48), (b" QC", 49, 35)] {
            let mut bytes = introducer.to_vec();
            bytes.extend_from_slice(payload);
            for boundary in 0..=bytes.len() {
                let mut parser = Parser::new();
                let mut events = parser.feed(&bytes[..boundary]);
                events.extend(parser.feed(&bytes[boundary..]));
                assert_eq!(
                    events,
                    vec![Event::LegacyMouse(payload.to_vec())],
                    "introducer {introducer:?}, split {boundary}"
                );
                assert_eq!(
                    decode_input(&events[0]),
                    Some(InputEvent::Mouse {
                        protocol: MouseProtocol::Legacy,
                        button: Some(0),
                        kind: MouseKind::Press,
                        modifiers: 0,
                        x,
                        y,
                    })
                );
                assert!(parser.expire().is_empty());
            }
        }
    }
}

#[test]
fn expired_legacy_mouse_quarantines_the_late_suffix_across_repeated_timeouts() {
    for received in 0..3 {
        let payload = b" CQ";
        let mut parser = Parser::new();
        assert!(parser.feed(b"\x1b[M").is_empty());
        assert!(parser.feed(&payload[..received]).is_empty());
        assert!(matches!(parser.expire().as_slice(), [Event::Malformed(_)]));
        for byte in &payload[received..] {
            assert!(parser.expire().is_empty());
            assert!(parser.feed(&[*byte]).is_empty());
        }
        assert_eq!(parser.feed(b"q"), vec![Event::Input(b"q".to_vec())]);
        assert!(parser.expire().is_empty());
    }
}

#[test]
fn malformed_legacy_payload_controls_are_never_replayed_as_keys_or_framing() {
    for payload in [
        [0x18, b'C', b'Q'],
        [0x1a, b'P', b'Q'],
        [b' ', 3, b'Q'],
        [b' ', 27, b'C'],
        [b' ', b'C', b' '],
    ] {
        let mut parser = Parser::new();
        let mut events = parser.feed(b"\x1b[M");
        for byte in payload {
            events.extend(parser.feed(&[byte]));
        }
        assert_eq!(events, vec![Event::LegacyMouse(payload.to_vec())]);
        assert_eq!(decode_input(&events[0]), None);
        assert_eq!(parser.feed(b"q"), vec![Event::Input(b"q".to_vec())]);
    }
    for payload in [vec![], vec![b' '], b" CQq".to_vec()] {
        assert_eq!(decode_input(&Event::LegacyMouse(payload)), None);
    }
}

#[test]
fn legacy_mouse_decoding_preserves_release_motion_wheel_modifiers_and_byte_coordinates() {
    for (button, kind, reported_button, modifiers) in [
        (0u8, MouseKind::Press, Some(0), 0),
        (28, MouseKind::Press, Some(0), 7),
        (3, MouseKind::Release, None, 0),
        (31, MouseKind::Release, None, 7),
        (32, MouseKind::Drag, Some(0), 0),
        (35, MouseKind::Motion, None, 0),
        (64, MouseKind::Wheel, Some(0), 0),
        (65, MouseKind::Wheel, Some(1), 0),
        (128, MouseKind::Press, Some(7), 0),
        (163, MouseKind::Drag, Some(10), 0),
    ] {
        let payload = [button + 32, 0x9b, 0xff];
        let mut parser = Parser::new();
        parser.set_legacy_mouse_bytes(true);
        let events = parser.feed(&[0x9b, b'M', payload[0], payload[1], payload[2]]);
        assert_eq!(events, vec![Event::LegacyMouse(payload.to_vec())]);
        assert_eq!(
            decode_input(&events[0]),
            Some(InputEvent::Mouse {
                protocol: MouseProtocol::Legacy,
                button: reported_button,
                kind,
                modifiers,
                x: 123,
                y: 223,
            })
        );
    }
}

#[test]
fn query_timeout_preserves_partial_paste_terminator_without_expiring_the_session_parser() {
    let mut parser = Parser::new();
    let mut deferred = VecDeque::new();
    assert_eq!(
        receive_cursor_response(&mut parser, b"\x1b[200~CQ\x1b[201", &mut deferred),
        None
    );
    // No bytes arrived before the objective query timed out. Its deadline does not
    // own the input framing deadline or the prefix held by the session parser.
    assert_eq!(
        receive_cursor_response(&mut parser, b"", &mut deferred),
        None
    );
    deferred.extend(parser.feed(b"~q\x03"));
    assert_eq!(
        deferred.into_iter().collect::<Vec<_>>(),
        vec![
            Event::Csi(b"200~".to_vec()),
            Event::PasteData(b"CQ".to_vec()),
            Event::Csi(b"201~".to_vec()),
            Event::Input(b"q\x03".to_vec()),
        ]
    );
    assert!(parser.expire().is_empty());
}

#[test]
fn decoded_mouse_protocol_distinguishes_legacy_input_from_sgr_evidence() {
    for (event, protocol) in [
        (Event::LegacyMouse(b" CQ".to_vec()), MouseProtocol::Legacy),
        (Event::Csi(b"<0;35;49M".to_vec()), MouseProtocol::Sgr),
    ] {
        assert_eq!(
            decode_input(&event),
            Some(InputEvent::Mouse {
                protocol,
                button: Some(0),
                kind: MouseKind::Press,
                modifiers: 0,
                x: 35,
                y: 49,
            })
        );
    }
}

#[test]
fn query_response_claims_one_cpr_and_preserves_mouse_keys_unknown_replies_and_partial_csi() {
    let mut parser = Parser::new();
    let mut deferred = VecDeque::new();
    assert_eq!(
        receive_cursor_response(
            &mut parser,
            b"\x1b[M CQ\x1b[?777;0$yq\x1b[3;7R\x1b[4;8R\x1b[99;",
            &mut deferred,
        ),
        Some((3, 7))
    );
    deferred.extend(parser.feed(b"100R"));
    assert_eq!(
        deferred.into_iter().collect::<Vec<_>>(),
        vec![
            Event::LegacyMouse(b" CQ".to_vec()),
            Event::Csi(b"?777;0$y".to_vec()),
            Event::Input(b"q".to_vec()),
            Event::Csi(b"4;8R".to_vec()),
            Event::Csi(b"99;100R".to_vec()),
        ]
    );
    assert!(parser.expire().is_empty());
}

#[test]
fn utf8_scalars_survive_both_directions_of_query_parser_ownership() {
    let before = "界".as_bytes();
    let after = "🙂".as_bytes();
    for entry_boundary in 1..before.len() {
        for return_boundary in 1..after.len() {
            let mut parser = Parser::new();
            let mut deferred = VecDeque::new();
            assert!(parser.feed(&before[..entry_boundary]).is_empty());
            let mut query_read = before[entry_boundary..].to_vec();
            query_read.extend_from_slice(b"q\x1b[3;7R");
            query_read.extend_from_slice(&after[..return_boundary]);
            assert_eq!(
                receive_cursor_response(&mut parser, &query_read, &mut deferred),
                Some((3, 7))
            );
            let mut interactive_read = after[return_boundary..].to_vec();
            interactive_read.push(b'q');
            deferred.extend(parser.feed(&interactive_read));
            assert_eq!(
                deferred.into_iter().collect::<Vec<_>>(),
                vec![
                    Event::Input("界q".as_bytes().to_vec()),
                    Event::Input("🙂q".as_bytes().to_vec()),
                ],
                "entry split {entry_boundary}, return split {return_boundary}"
            );
            assert!(parser.expire().is_empty());
        }
    }
}

#[test]
fn utf8_legacy_mouse_coordinates_remain_opaque_to_hotkeys_and_survive_timeout() {
    let payload = " \u{1234}C".as_bytes();
    let mut bytes = b"\x1b[M".to_vec();
    bytes.extend_from_slice(payload);
    bytes.push(b'q');
    for split in 0..=bytes.len() {
        let mut parser = Parser::new();
        let mut events = parser.feed(&bytes[..split]);
        events.extend(parser.feed(&bytes[split..]));
        assert_eq!(
            events,
            [
                Event::LegacyMouse(payload.to_vec()),
                Event::Input(b"q".to_vec())
            ]
        );
        assert!(matches!(
            decode_input(&events[0]),
            Some(InputEvent::Mouse {
                protocol: MouseProtocol::Legacy,
                x: 4628,
                y: 35,
                ..
            })
        ));
    }
    let mut parser = Parser::new();
    parser.feed(b"\x1b[M \xc3");
    assert!(matches!(parser.expire().as_slice(), [Event::Malformed(_)]));
    assert_eq!(parser.feed(b"\xa7Cq"), [Event::Input(b"q".to_vec())]);
}

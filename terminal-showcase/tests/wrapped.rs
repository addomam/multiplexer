use terminal_showcase::protocol::{Event, MAX_SEQUENCE, Parser};

fn wrap(bytes: &[u8]) -> Vec<u8> {
    let mut wrapped = b"\x1bPtmux;".to_vec();
    for &byte in bytes {
        wrapped.push(byte);
        if byte == 27 {
            wrapped.push(byte);
        }
    }
    wrapped.extend_from_slice(b"\x1b\\");
    wrapped
}

fn fragmented(bytes: &[u8], split: usize) -> Vec<Event> {
    let mut parser = Parser::new();
    let mut events = parser.feed(&bytes[..split]);
    events.extend(parser.feed(&bytes[split..]));
    events
}

#[test]
fn wrappers_preserve_semantic_frames_across_wrappers_and_ordinary_bytes() {
    for (prefix, suffix, expected) in [
        (
            b"\x1b[12;".as_slice(),
            b"34R".as_slice(),
            Event::Csi(b"12;34R".to_vec()),
        ),
        (
            b"\x1b]52;c;".as_slice(),
            b"C\x1b\\".as_slice(),
            Event::Osc(b"52;c;C".to_vec()),
        ),
        (
            b"\x1bPunknown;".as_slice(),
            b"C\x1b\\".as_slice(),
            Event::Dcs(b"unknown;C".to_vec()),
        ),
        (
            b"\x1b_Gi=7;".as_slice(),
            b"C\x1b\\".as_slice(),
            Event::Apc(b"Gi=7;C".to_vec()),
        ),
        (
            b"\x1b^privacy;".as_slice(),
            b"C\x1b\\".as_slice(),
            Event::Escape(b"^privacy;C".to_vec()),
        ),
        (
            b"\x1bXprivate;".as_slice(),
            b"Q\x1b\\".as_slice(),
            Event::Escape(b"Xprivate;Q".to_vec()),
        ),
    ] {
        for wrapped_suffix in [false, true] {
            let mut bytes = wrap(prefix);
            bytes.extend(if wrapped_suffix {
                wrap(suffix)
            } else {
                suffix.to_vec()
            });
            bytes.push(b'q');
            for split in 0..=bytes.len() {
                assert_eq!(
                    fragmented(&bytes, split),
                    vec![expected.clone(), Event::Input(b"q".to_vec())],
                    "{prefix:?}, wrapped suffix {wrapped_suffix}, split {split}"
                );
            }
        }
    }
}

#[test]
fn wrapped_paste_is_opaque_until_wrapped_or_ordinary_closing_marker() {
    for wrapped_payload in [false, true] {
        for wrapped_end in [false, true] {
            let mut bytes = wrap(b"\x1b[200~");
            bytes.extend(if wrapped_payload {
                wrap(b"CQ\x03")
            } else {
                b"CQ\x03".to_vec()
            });
            bytes.extend(if wrapped_end {
                wrap(b"\x1b[201~")
            } else {
                b"\x1b[201~".to_vec()
            });
            bytes.push(b'q');
            for split in 0..=bytes.len() {
                let events = fragmented(&bytes, split);
                assert_eq!(events.first(), Some(&Event::Csi(b"200~".to_vec())));
                assert_eq!(
                    events[events.len() - 2..],
                    [Event::Csi(b"201~".to_vec()), Event::Input(b"q".to_vec())]
                );
                let mut pasted = Vec::new();
                for event in &events[1..events.len() - 2] {
                    let Event::PasteData(data) = event else {
                        panic!("paste provenance lost: {events:?}")
                    };
                    pasted.extend_from_slice(data);
                }
                assert_eq!(pasted, b"CQ\x03");
            }
        }
    }
}

#[test]
fn semantic_utf8_and_nested_transport_prefixes_survive_adjacent_wrappers() {
    let scalar = "🙂".as_bytes();
    for cut in 1..scalar.len() {
        for wrapped_suffix in [false, true] {
            let mut parser = Parser::new();
            assert!(parser.feed(&wrap(&scalar[..cut])).is_empty());
            let suffix = if wrapped_suffix {
                wrap(&scalar[cut..])
            } else {
                scalar[cut..].to_vec()
            };
            assert_eq!(parser.feed(&suffix), vec![Event::Input(scalar.to_vec())]);
        }
    }
    let inner = wrap(b"\x1b]52;c;C\x1b\\");
    for cut in 1..inner.len() {
        let mut parser = Parser::new();
        assert!(parser.feed(&wrap(&inner[..cut])).is_empty());
        assert_eq!(
            parser.feed(&wrap(&inner[cut..])),
            vec![Event::Osc(b"52;c;C".to_vec())]
        );
    }
}

#[test]
fn wrapped_timeout_and_overflow_quarantine_survive_later_wrappers() {
    let mut parser = Parser::new();
    assert!(parser.feed(&wrap(b"\x1b]52;c;")).is_empty());
    assert!(matches!(parser.expire().as_slice(), [Event::Malformed(_)]));
    assert!(parser.feed(&wrap(b"CQ")).is_empty());
    assert!(parser.expire().is_empty());
    assert_eq!(
        parser.feed(&wrap(b"\x1b\\q")),
        vec![Event::Input(b"q".to_vec())]
    );

    let mut parser = Parser::new();
    assert!(parser.feed(&wrap(b"\x1b]52;c;")).is_empty());
    let mut events = Vec::new();
    for _ in 0..3 {
        events.extend(parser.feed(&wrap(&[b'C'; MAX_SEQUENCE / 2])));
    }
    assert!(matches!(events.as_slice(), [Event::Malformed(_)]));
    assert!(parser.feed(b"Q").is_empty());
    assert_eq!(
        parser.feed(&wrap(b"\x1b\\q")),
        vec![Event::Input(b"q".to_vec())]
    );
}

#[test]
fn transport_limits_and_timeout_do_not_leak_quarantined_payload_as_keys() {
    let mut oversized = wrap(&[b'C'; MAX_SEQUENCE + 1]);
    oversized.extend_from_slice(b"q");
    for split in [0, 7, MAX_SEQUENCE, oversized.len()] {
        let events = fragmented(&oversized, split);
        assert!(
            matches!(events.as_slice(), [Event::Malformed(_), Event::Input(key)] if key == b"q")
        );
    }
    let mut parser = Parser::new();
    assert!(parser.feed(b"\x1bPtmux;\x1b\x1b]52;c;").is_empty());
    assert!(matches!(parser.expire().as_slice(), [Event::Malformed(_)]));
    assert!(parser.feed(b"CQ\x1b\x1b\\").is_empty());
    assert!(parser.expire().is_empty());
    assert_eq!(parser.feed(b"\x1b\\q"), vec![Event::Input(b"q".to_vec())]);

    for depth in 1..=5 {
        let mut bytes = b"\x1b[12;34R".to_vec();
        for _ in 0..depth {
            bytes = wrap(&bytes);
        }
        bytes.push(b'q');
        let events = fragmented(&bytes, bytes.len() / 2);
        if depth <= 4 {
            assert_eq!(
                events,
                vec![Event::Csi(b"12;34R".to_vec()), Event::Input(b"q".to_vec())]
            );
        } else {
            assert!(
                matches!(events.as_slice(), [Event::Malformed(_), Event::Input(key)] if key == b"q")
            );
        }
    }
}

#[test]
fn malformed_ss3_quarantines_parameters_and_intermediates_through_final() {
    for bad in [b"\x1bO1;2C".as_slice(), b"\x1bO Q".as_slice()] {
        let mut bytes = bad.to_vec();
        bytes.push(b'q');
        for split in 0..=bytes.len() {
            assert!(
                matches!(fragmented(&bytes, split).as_slice(), [Event::Malformed(_), Event::Input(key)] if key == b"q")
            );
        }
        let mut parser = Parser::new();
        assert!(parser.feed(&bad[..2]).is_empty());
        assert!(matches!(parser.expire().as_slice(), [Event::Malformed(_)]));
        assert!(parser.feed(&bad[2..]).is_empty());
        assert_eq!(parser.feed(b"q"), vec![Event::Input(b"q".to_vec())]);
    }
    let mut parser = Parser::new();
    assert_eq!(
        parser.feed(b"\x1bOA\x1bOB\x1bOC\x1bOD"),
        vec![
            Event::Escape(b"OA".to_vec()),
            Event::Escape(b"OB".to_vec()),
            Event::Escape(b"OC".to_vec()),
            Event::Escape(b"OD".to_vec())
        ]
    );
    assert!(
        matches!(parser.feed(b"\x1bO1;\x03").as_slice(), [Event::Malformed(_), Event::Input(key)] if key == b"\x03")
    );
}

#[test]
fn partial_wrapped_terminators_and_nested_headers_accept_ordinary_continuations() {
    let mut parser = Parser::new();
    assert!(parser.feed(&wrap(b"\x1b]52;c;C\x1b")).is_empty());
    assert_eq!(
        parser.feed(b"\\q"),
        vec![Event::Osc(b"52;c;C".to_vec()), Event::Input(b"q".to_vec())]
    );
    let nested = wrap(b"\x1b[12;34R");
    for cut in 1..nested.len() {
        let mut parser = Parser::new();
        assert!(parser.feed(&wrap(&nested[..cut])).is_empty());
        let mut suffix = nested[cut..].to_vec();
        suffix.push(b'q');
        assert_eq!(
            parser.feed(&suffix),
            vec![Event::Csi(b"12;34R".to_vec()), Event::Input(b"q".to_vec())],
            "nested header cut {cut}"
        );
    }
    let mut parser = Parser::new();
    assert!(parser.feed(&wrap(b"\x1b[20")).is_empty());
    assert_eq!(parser.feed(b"0~"), vec![Event::Csi(b"200~".to_vec())]);
    assert_eq!(
        parser.feed(&wrap(b"CQ")),
        vec![Event::PasteData(b"CQ".to_vec())]
    );
    assert_eq!(
        parser.feed(&wrap(b"\x1b[201~q")),
        vec![Event::Csi(b"201~".to_vec()), Event::Input(b"q".to_vec())]
    );
}

#[test]
fn literal_wrapper_headers_survive_idle_paste_and_large_literal_payloads() {
    let mut literal = wrap(&[b'C'; MAX_SEQUENCE + 1]);
    literal.extend_from_slice(b"\x1bPtmux;literal");
    let mut parser = Parser::new();
    assert_eq!(
        parser.feed(b"\x1b[200~"),
        vec![Event::Csi(b"200~".to_vec())]
    );
    let mut events = parser.feed(&literal[..4]);
    events.extend(parser.expire());
    events.extend(parser.feed(&literal[4..]));
    events.extend(parser.feed(b"\x1b[201~q"));
    let mut content = Vec::new();
    for event in &events[..events.len() - 2] {
        let Event::PasteData(bytes) = event else {
            panic!("literal data escaped paste: {events:?}")
        };
        content.extend_from_slice(bytes);
    }
    assert_eq!(content, literal);
    assert_eq!(
        events[events.len() - 2..],
        [Event::Csi(b"201~".to_vec()), Event::Input(b"q".to_vec())]
    );
}

#[test]
fn c1_wrapper_framing_and_exact_transport_limit_remain_valid() {
    let mut bytes = b"\x90tmux;".to_vec();
    bytes.extend_from_slice(b"\x1b\x1b[12;34R");
    bytes.push(0x9c);
    for split in 0..=bytes.len() {
        assert_eq!(
            fragmented(&bytes, split),
            vec![Event::Csi(b"12;34R".to_vec())]
        );
    }
    let exact = wrap(&[b'C'; MAX_SEQUENCE - 5]);
    assert_eq!(
        Parser::new().feed(&exact),
        vec![Event::Input(vec![b'C'; MAX_SEQUENCE - 5])]
    );
    let mut invalid = b"\x1bPtmux;valid\x1b]C\x1b\\".to_vec();
    invalid.push(b'q');
    assert_eq!(
        Parser::new().feed(&invalid),
        vec![
            Event::Dcs(b"tmux;valid\x1b]C".to_vec()),
            Event::Input(b"q".to_vec())
        ]
    );
}

#[test]
fn emergency_ctrl_c_is_available_in_partial_transport_but_not_wrapped_paste_or_mouse() {
    let mut parser = Parser::new();
    assert_eq!(
        parser.feed(b"\x1bPtmux;\x1b\x1b]broken\x03"),
        vec![Event::Input(vec![3])]
    );
    assert!(parser.feed(b"CQ\x1b\\").is_empty());
    assert_eq!(parser.feed(b"q"), vec![Event::Input(b"q".to_vec())]);
    let mut parser = Parser::new();
    assert_eq!(
        parser.feed(&wrap(b"\x1b[200~CQ\x03\x1b[201~q")),
        vec![
            Event::Csi(b"200~".to_vec()),
            Event::PasteData(b"CQ\x03".to_vec()),
            Event::Csi(b"201~".to_vec()),
            Event::Input(b"q".to_vec())
        ]
    );
    let mut parser = Parser::new();
    assert_eq!(
        parser.feed(&wrap(b"\x1b[M \x03Qq")),
        vec![
            Event::LegacyMouse(b" \x03Q".to_vec()),
            Event::Input(b"q".to_vec())
        ]
    );
}

#[test]
fn bytewise_nested_paste_and_malformed_ss3_transitions_keep_keyboard_provenance() {
    let mut bytes = wrap(&wrap(b"\x1b[200~"));
    bytes.extend_from_slice(&wrap(b"CQ"));
    bytes.extend_from_slice(b"\x03");
    bytes.extend_from_slice(&wrap(&wrap(b"\x1b[201~")));
    bytes.extend_from_slice(&wrap(b"\x1bO1;"));
    bytes.extend_from_slice(&wrap(b"2C"));
    bytes.extend_from_slice(b"q");
    let mut parser = Parser::new();
    let mut events = Vec::new();
    for &byte in &bytes {
        events.extend(parser.feed(&[byte]));
    }
    assert_eq!(events.first(), Some(&Event::Csi(b"200~".to_vec())));
    assert_eq!(events.last(), Some(&Event::Input(b"q".to_vec())));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::Malformed(_)))
            .count(),
        1
    );
    let pasted: Vec<u8> = events
        .iter()
        .filter_map(|event| {
            if let Event::PasteData(bytes) = event {
                Some(bytes.as_slice())
            } else {
                None
            }
        })
        .flatten()
        .copied()
        .collect();
    assert_eq!(pasted, b"CQ\x03");
    assert!(
        events
            .iter()
            .filter(|event| matches!(event, Event::Input(_)))
            .count()
            == 1
    );
}

#[test]
fn empty_privacy_and_sos_strings_retain_control_frame_identity() {
    let bytes = wrap(b"\x1b^\x1b\\\x1bX\x1b\\");
    for split in 0..=bytes.len() {
        assert_eq!(
            fragmented(&bytes, split),
            vec![Event::Escape(vec![b'^']), Event::Escape(vec![b'X'])]
        );
    }
}

use terminal_showcase::protocol::{Event, Parser};

#[test]
fn escape_like_paste_contents_do_not_swallow_the_terminator_or_become_replies() {
    let payload = b"qC\x1b]52;c;unterminated\x1bPunknown\x1b[4;8R\x1b[M Cq";
    let mut bytes = b"\x1b[200~".to_vec();
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(b"\x1b[201~q");
    for split in 0..=bytes.len() {
        let mut parser = Parser::new();
        let mut events = parser.feed(&bytes[..split]);
        events.extend(parser.feed(&bytes[split..]));
        assert_eq!(events.first(), Some(&Event::Csi(b"200~".to_vec())));
        let ending = events
            .iter()
            .position(|e| *e == Event::Csi(b"201~".to_vec()))
            .expect("paste terminator must survive escape-looking content");
        let mut content = Vec::new();
        for event in &events[1..ending] {
            let Event::PasteData(bytes) = event else {
                panic!("paste invented protocol reply: {event:?}")
            };
            content.extend_from_slice(bytes);
        }
        assert_eq!(content, payload);
        assert_eq!(events.last(), Some(&Event::Input(b"q".to_vec())));
    }
}

#[test]
fn idle_paste_and_split_terminators_preserve_the_input_boundary() {
    for (start, end) in [
        (b"\x1b[200~".as_slice(), b"\x1b[201~".as_slice()),
        (b"\x9b200~".as_slice(), b"\x9b201~".as_slice()),
    ] {
        for split in 0..end.len() {
            let mut parser = Parser::new();
            assert_eq!(parser.feed(start), [Event::Csi(b"200~".to_vec())]);
            parser.feed(b"q\x1b]unterminated");
            parser.feed(&end[..split]);
            assert!(
                parser.expire().is_empty(),
                "idle paste is not a timed-out response"
            );
            let mut suffix = end[split..].to_vec();
            suffix.push(b'q');
            assert_eq!(
                parser.feed(&suffix),
                [Event::Csi(b"201~".to_vec()), Event::Input(b"q".to_vec())]
            );
        }
    }
}

fn wrap(bytes: &[u8]) -> Vec<u8> {
    let mut result = b"\x1bPtmux;".to_vec();
    for &byte in bytes {
        result.push(byte);
        if byte == 27 {
            result.push(byte);
        }
    }
    result.extend_from_slice(b"\x1b\\");
    result
}

fn assert_paste(events: &[Event], payload: &[u8]) {
    assert_eq!(events.first(), Some(&Event::Csi(b"200~".to_vec())));
    let end = events
        .iter()
        .position(|e| *e == Event::Csi(b"201~".to_vec()))
        .unwrap();
    let mut data = Vec::new();
    for event in &events[1..end] {
        let Event::PasteData(bytes) = event else {
            panic!("paste became a command or response: {events:?}");
        };
        data.extend_from_slice(bytes);
    }
    assert_eq!(data, payload);
    let mut controls = Vec::new();
    for event in &events[end + 1..] {
        let Event::Input(bytes) = event else {
            panic!("controls did not resume: {events:?}");
        };
        controls.extend_from_slice(bytes);
    }
    assert_eq!(controls, b"Eq");
}

#[test]
fn genuine_paste_boundaries_survive_unicode_literal_headers_and_escape_overlap() {
    for payload in [
        "Û201~CNTWQЛ201~🛛201~".as_bytes(),
        "Û".as_bytes(),
        "Л".as_bytes(),
        "🛛".as_bytes(),
        b"\x1bPtmux;literalCNTWQ",
        b"\x1bPtm",
        b"C\x1b",
        b"C\x1b\x1b",
        b"C\x1b\x1b\x1b",
        b"\xc3\x9b201~C\x9b20X\x1b[20X",
    ] {
        for end in [b"\x1b[201~".as_slice(), b"\x9b201~".as_slice()] {
            let mut wire = b"\x1b[200~".to_vec();
            wire.extend_from_slice(payload);
            wire.extend_from_slice(end);
            wire.extend_from_slice(b"Eq");
            for split in 0..=wire.len() {
                let mut parser = Parser::new();
                let mut events = parser.feed(&wire[..split]);
                events.extend(parser.feed(&wire[split..]));
                assert_paste(&events, payload);
            }
            let mut parser = Parser::new();
            let mut events = parser.feed(b"\x1b[200~");
            for &byte in &wire[6..wire.len() - 2] {
                events.extend(parser.feed(&[byte]));
                events.extend(parser.expire());
            }
            events.extend(parser.feed(b"Eq"));
            assert_paste(&events, payload);
        }
    }
}

#[test]
fn unicode_and_closing_markers_survive_decoded_transport_fragment_boundaries() {
    let payload = "Û201~CNTWQЛ201~🛛201~".as_bytes();
    for depth in 1..=3 {
        for cut in 0..=payload.len() {
            for ordinary_suffix in [false, true] {
                let encode = |bytes: &[u8]| {
                    let mut bytes = bytes.to_vec();
                    for _ in 0..depth {
                        bytes = wrap(&bytes);
                    }
                    bytes
                };
                let mut wire = encode(b"\x1b[200~");
                wire.extend(encode(&payload[..cut]));
                wire.extend(if ordinary_suffix {
                    payload[cut..].to_vec()
                } else {
                    encode(&payload[cut..])
                });
                wire.extend(encode(b"\x9b201~"));
                wire.extend_from_slice(b"Eq");
                for split in 0..=wire.len() {
                    let mut parser = Parser::new();
                    let mut events = parser.feed(&wire[..split]);
                    events.extend(parser.feed(&wire[split..]));
                    assert_paste(&events, payload);
                }
                let mut parser = Parser::new();
                let mut events = Vec::new();
                for &byte in &wire {
                    events.extend(parser.feed(&[byte]));
                }
                assert_paste(&events, payload);
            }
        }
    }
}

#[test]
fn transport_paste_keeps_escape_overlap_and_partial_end_across_idle() {
    for depth in 1..=3 {
        let encode = |bytes: &[u8]| {
            let mut wire = bytes.to_vec();
            for _ in 0..depth {
                wire = wrap(&wire);
            }
            wire
        };
        for escapes in 1..=3 {
            let mut payload = b"CNTWQ".to_vec();
            payload.extend(std::iter::repeat_n(27, escapes));
            for end in [b"\x1b[201~".as_slice(), b"\x9b201~".as_slice()] {
                for cut in 0..=end.len() {
                    for ordinary_tail in [false, true] {
                        let mut parser = Parser::new();
                        let mut events = parser.feed(&encode(b"\x1b[200~"));
                        events.extend(parser.feed(&encode(&payload)));
                        events.extend(parser.feed(&encode(&end[..cut])));
                        events.extend(parser.expire());
                        events.extend(parser.expire());
                        events.extend(parser.feed(&if ordinary_tail {
                            end[cut..].to_vec()
                        } else {
                            encode(&end[cut..])
                        }));
                        events.extend(parser.feed(b"Eq"));
                        assert_paste(&events, &payload);
                    }
                }
            }
        }
    }
}

#[test]
fn c1_continuations_remain_data_across_successive_decoded_wrappers() {
    // Exhaust all two-byte scalars and bounded three-/four-byte scalar windows,
    // selecting every scalar containing a transport/paste-looking continuation.
    for scalar in (0x80..0x800).chain(0x800..0x1000).chain(0x10000..0x10800) {
        let character = char::from_u32(scalar).unwrap();
        let mut storage = [0; 4];
        let bytes = character.encode_utf8(&mut storage).as_bytes();
        if !bytes.iter().any(|byte| matches!(byte, 0x90 | 0x9b | 0x9c)) {
            continue;
        }
        for depth in 1..=3 {
            let encode = |bytes: &[u8]| {
                let mut wire = bytes.to_vec();
                for _ in 0..depth {
                    wire = wrap(&wire);
                }
                wire
            };
            for cut in 1..bytes.len() {
                let mut parser = Parser::new();
                let mut events = parser.feed(&encode(b"\x1b[200~"));
                events.extend(parser.feed(&encode(&bytes[..cut])));
                events.extend(parser.expire());
                events.extend(parser.feed(&encode(&bytes[cut..])));
                events.extend(parser.feed(&encode(b"201~CNTWQ")));
                events.extend(parser.feed(&encode(b"\x1b[201~")));
                events.extend(parser.feed(b"Eq"));
                let mut payload = bytes.to_vec();
                payload.extend_from_slice(b"201~CNTWQ");
                assert_paste(&events, &payload);
            }
        }
    }
}

#[test]
fn bounded_literal_header_and_escape_corpus_has_only_genuine_paste_boundaries() {
    for header in [
        b"".as_slice(),
        b"\x1bPtm",
        b"\x1bPtmux;",
        b"\x1bPtmux;literal",
        b"\x90tmux;literal",
        b"\x1bPtmux;\x1bPtmux;literal",
    ] {
        for scalar in ["Û", "Ü", "\u{90}", "Л", "🛛"] {
            for escapes in 0..=4 {
                let mut payload = header.to_vec();
                payload.extend_from_slice(scalar.as_bytes());
                payload.extend_from_slice(b"201~CNTWQ\x1b[20X\x9b20X");
                payload.extend(std::iter::repeat_n(27, escapes));
                let mut wire = b"\x1b[200~".to_vec();
                wire.extend_from_slice(&payload);
                wire.extend_from_slice(b"\x1b[201~Eq");
                for split in 0..=wire.len() {
                    let mut parser = Parser::new();
                    let mut events = parser.feed(&wire[..split]);
                    events.extend(parser.feed(&wire[split..]));
                    assert_paste(&events, &payload);
                }
            }
        }
    }
}

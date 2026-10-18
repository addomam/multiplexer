use base64::{Engine, engine::general_purpose::STANDARD};
use terminal_showcase::graphics::{
    Graphics, InputEvent, KeyKind, KeyProtocol, MouseKind, Protocol, decode_input,
    integration_action,
};
use terminal_showcase::protocol::Event;

fn csi(value: &str) -> Option<InputEvent> {
    decode_input(&Event::Csi(value.as_bytes().to_vec()))
}

#[test]
fn kitty_probe_is_direct_non_storing_and_non_placing() {
    assert_eq!(
        Graphics::query(),
        b"\x1b_Ga=q,t=d,i=77331,f=24,s=1,v=1;AAAA\x1b\\"
    );
}

fn contains_raster(bytes: &[u8], protocol: Protocol) -> bool {
    let text = std::str::from_utf8(bytes).unwrap();
    match protocol {
        Protocol::Kitty => text.split("\x1b_G").skip(1).any(|apc| {
            let command = apc.split("\x1b\\").next().unwrap();
            let (header, payload) = command.split_once(';').unwrap();
            header.contains("a=T") && !STANDARD.decode(payload).unwrap().is_empty()
        }),
        Protocol::Sixel => text.split("\x1bP0;1;0q").skip(1).any(|dcs| {
            let payload = dcs.split("\x1b\\").next().unwrap();
            // Raster data uses '?' through '~'; metadata is numeric punctuation.
            payload.bytes().any(|byte| (b'?'..=b'~').contains(&byte))
        }),
        Protocol::Iterm => text.split("\x1b]1337;File=").skip(1).any(|osc| {
            let image = osc.split('\x07').next().unwrap();
            let (_, payload) = image.split_once(':').unwrap();
            STANDARD
                .decode(payload)
                .unwrap()
                .starts_with(b"\x89PNG\r\n\x1a\n")
        }),
    }
}

#[test]
fn fresh_lifecycle_entry_reports_only_actual_raster_transmission() {
    for protocol in [Protocol::Kitty, Protocol::Sixel, Protocol::Iterm] {
        for phase in 0..8 {
            let mut graphics = Graphics::new();
            assert!(!graphics.raster_emitted());
            let bytes = graphics.frame(protocol, phase * 4, 5, 2, 40, 12);
            let expected = match protocol {
                Protocol::Kitty => phase < 6,
                Protocol::Sixel | Protocol::Iterm => phase < 7,
            };
            assert!(!bytes.is_empty(), "{protocol:?} phase {phase}");
            assert_eq!(
                contains_raster(&bytes, protocol),
                expected,
                "{protocol:?} phase {phase}: wire payload"
            );
            assert_eq!(
                graphics.raster_emitted(),
                expected,
                "{protocol:?} phase {phase}: metadata"
            );
        }
    }
}

#[test]
fn populated_lifecycle_does_not_claim_placement_text_scroll_or_delete_as_raster() {
    for protocol in [Protocol::Kitty, Protocol::Sixel, Protocol::Iterm] {
        let mut graphics = Graphics::new();
        for phase in 0..8 {
            let bytes = graphics.frame(protocol, phase * 4, 5, 2, 40, 12);
            let expected = match protocol {
                Protocol::Kitty => phase < 2,
                Protocol::Sixel | Protocol::Iterm => phase < 7,
            };
            assert_eq!(
                contains_raster(&bytes, protocol),
                expected,
                "{protocol:?} phase {phase}: wire payload"
            );
            assert_eq!(
                graphics.raster_emitted(),
                expected,
                "{protocol:?} phase {phase}: metadata"
            );
        }
    }
}

#[test]
fn raster_evidence_resets_on_noop_clear_and_zero_canvas() {
    for protocol in [Protocol::Kitty, Protocol::Sixel, Protocol::Iterm] {
        let mut graphics = Graphics::new();
        graphics.frame(protocol, 0, 5, 2, 40, 12);
        assert!(graphics.raster_emitted());
        assert!(graphics.frame(protocol, 1, 5, 2, 40, 12).is_empty());
        assert!(!graphics.raster_emitted());

        let resized = graphics.frame(protocol, 0, 5, 2, 20, 6);
        assert!(contains_raster(&resized, protocol));
        assert!(graphics.raster_emitted());
        let cleared = graphics.clear();
        assert!(!contains_raster(&cleared, protocol));
        assert!(!graphics.raster_emitted());
        assert!(graphics.clear().is_empty());
        assert!(!graphics.raster_emitted());

        graphics.frame(protocol, 0, 5, 2, 40, 12);
        assert!(graphics.raster_emitted());
        let zero_width = graphics.frame(protocol, 0, 5, 2, 0, 12);
        assert!(!contains_raster(&zero_width, protocol));
        assert!(!graphics.raster_emitted());
        graphics.frame(protocol, 0, 5, 2, 40, 12);
        assert!(graphics.raster_emitted());
        let zero_height = graphics.frame(protocol, 0, 5, 2, 40, 0);
        assert!(!contains_raster(&zero_height, protocol));
        assert!(!graphics.raster_emitted());
    }
}

#[test]
fn kitty_lifecycle_is_bounded_chunked_and_only_deletes_owned_images() {
    let mut graphics = Graphics::new();
    let first = graphics.frame(Protocol::Kitty, 0, 5, 2, 80, 20);
    let text = String::from_utf8(first).unwrap();
    assert!(text.contains("i=77332"));
    assert!(text.contains("i=77333"));
    assert!(text.contains("t=d"));
    assert!(!text.contains("t=f") && !text.contains("t=s"));
    assert!(text.len() < 24_000);
    for apc in text.split("\x1b_G").skip(1) {
        let command = apc.split("\x1b\\").next().unwrap();
        if let Some((_, payload)) = command.split_once(';') {
            assert!(payload.len() <= 4096);
        }
    }
    assert!(graphics.frame(Protocol::Kitty, 1, 5, 2, 80, 20).is_empty());
    let replacement = String::from_utf8(graphics.frame(Protocol::Kitty, 4, 5, 2, 80, 20)).unwrap();
    assert!(replacement.contains("a=T") && replacement.contains("i=77332"));
    let placement = String::from_utf8(graphics.frame(Protocol::Kitty, 8, 5, 2, 80, 20)).unwrap();
    assert!(placement.contains("a=p,i=77333,p=1"));
    let clipping = String::from_utf8(graphics.frame(Protocol::Kitty, 12, 5, 2, 80, 20)).unwrap();
    assert!(clipping.contains("x=") && clipping.contains("w="));
    let scroll = String::from_utf8(graphics.frame(Protocol::Kitty, 20, 5, 2, 80, 20)).unwrap();
    assert!(scroll.contains("S") && scroll.contains("\x1b[r"));
    let delete = String::from_utf8(graphics.frame(Protocol::Kitty, 24, 5, 2, 80, 20)).unwrap();
    assert!(delete.contains("a=d,d=I,i=77333"));
    let clear = String::from_utf8(graphics.clear()).unwrap();
    assert!(clear.contains("a=d,d=I,i=77332") && clear.contains("a=d,d=I,i=77333"));
    assert!(!clear.contains("d=A") && !clear.contains("77331"));
    assert!(graphics.clear().is_empty());
}

#[test]
fn zero_canvas_suppresses_images_and_resize_rebuilds_owned_placements() {
    let mut graphics = Graphics::new();
    assert!(graphics.frame(Protocol::Kitty, 0, 1, 1, 0, 0).is_empty());
    graphics.frame(Protocol::Kitty, 0, 1, 1, 40, 12);
    let resized = String::from_utf8(graphics.frame(Protocol::Kitty, 0, 1, 1, 2, 2)).unwrap();
    assert!(resized.contains("a=d,d=I,i=77332"));
    assert!(resized.contains("a=T"));
}

#[test]
fn sixel_has_a_bounded_declared_raster_and_palette() {
    let mut graphics = Graphics::new();
    let bytes = graphics.frame(Protocol::Sixel, 0, 2, 2, 1000, 1000);
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("\x1bP0;1;0q\"1;1;64;32"));
    assert!(text.contains("#0;2;"));
    assert!(text.len() < 24_000);
    assert!(!text.contains("1337") && !text.contains("\x1b_G"));
}

// Decode the PNG independently: check chunks, CRCs, stored DEFLATE and Adler-32.
#[test]
fn iterm_sends_actual_bounded_png_inline_not_a_file_transfer() {
    let text =
        String::from_utf8(Graphics::new().frame(Protocol::Iterm, 0, 2, 2, 1000, 1000)).unwrap();
    let image = text
        .split("\x1b]1337;File=")
        .nth(1)
        .unwrap()
        .split('\x07')
        .next()
        .unwrap();
    let (header, payload) = image.split_once(':').unwrap();
    assert!(header.contains("inline=1"));
    let png = STANDARD.decode(payload).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let mut offset = 8;
    let mut compressed = Vec::new();
    let mut dimensions = (0, 0);
    while offset < png.len() {
        let len = u32::from_be_bytes(png[offset..offset + 4].try_into().unwrap()) as usize;
        let kind = &png[offset + 4..offset + 8];
        let data = &png[offset + 8..offset + 8 + len];
        let expected =
            u32::from_be_bytes(png[offset + 8 + len..offset + 12 + len].try_into().unwrap());
        let mut crc = u32::MAX;
        for &b in &png[offset + 4..offset + 8 + len] {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
            }
        }
        assert_eq!(!crc, expected);
        match kind {
            b"IHDR" => {
                dimensions = (
                    u32::from_be_bytes(data[..4].try_into().unwrap()),
                    u32::from_be_bytes(data[4..8].try_into().unwrap()),
                );
                assert_eq!(&data[8..], &[8, 2, 0, 0, 0]);
            }
            b"IDAT" => compressed.extend_from_slice(data),
            b"IEND" => assert!(data.is_empty()),
            _ => panic!("unexpected PNG chunk"),
        }
        offset += 12 + len;
    }
    assert_eq!(dimensions, (64, 32));
    assert_eq!(&compressed[..3], &[0x78, 0x01, 0x01]);
    let len = u16::from_le_bytes(compressed[3..5].try_into().unwrap());
    let inverted = u16::from_le_bytes(compressed[5..7].try_into().unwrap());
    assert_eq!(len, !inverted);
    let raster = &compressed[7..7 + usize::from(len)];
    assert_eq!(raster.len(), 32 * (1 + 64 * 3));
    assert!(raster.chunks_exact(193).all(|row| row[0] == 0));
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in raster {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    assert_eq!(
        u32::from_be_bytes(compressed[7 + usize::from(len)..].try_into().unwrap()),
        (b << 16) | a
    );
}

#[test]
fn integration_requires_exact_explicit_actions_and_uses_static_safe_values() {
    for key in [b'C', b'N', b'T', b'W'] {
        assert!(integration_action(key).is_some());
    }
    for key in [b'c', b'n', b't', b'w', b'G', b'q', 0, 27] {
        assert!(integration_action(key).is_none());
    }
    assert_eq!(
        integration_action(b'C').unwrap(),
        b"\x1b]52;c;dGVybWluYWwtc2hvd2Nhc2U=\x07"
    );
    assert_eq!(
        integration_action(b'W').unwrap(),
        b"\x1b]7;file://localhost/tmp\x07"
    );
}

#[test]
fn kitty_keys_preserve_alternates_text_modifiers_and_event_kind() {
    assert_eq!(
        csi("97:65:113;198:2;65:769u"),
        Some(InputEvent::Key {
            protocol: KeyProtocol::Kitty,
            code: 97,
            modifiers: 197,
            kind: KeyKind::Repeat,
            shifted: Some(65),
            base_layout: Some(113),
            text: "A\u{301}".into(),
        })
    );
    assert!(matches!(
        csi("57358;1:3u"),
        Some(InputEvent::Key {
            kind: KeyKind::Release,
            ..
        })
    ));
    assert!(matches!(
        csi("97u"),
        Some(InputEvent::Key {
            modifiers: 0,
            kind: KeyKind::Press,
            ..
        })
    ));
    assert!(matches!(csi("0;;229u"), Some(InputEvent::Key { text, .. }) if text == "å"));
    assert!(matches!(
        csi("1;5:3A"),
        Some(InputEvent::Key {
            code: 57352,
            modifiers: 4,
            kind: KeyKind::Release,
            ..
        })
    ));
}

#[test]
fn traditional_and_modify_other_keys_are_distinguished() {
    assert!(matches!(
        csi("27;6;65~"),
        Some(InputEvent::Key {
            protocol: KeyProtocol::ModifyOtherKeys,
            code: 65,
            modifiers: 5,
            ..
        })
    ));
    assert!(matches!(
        csi("A"),
        Some(InputEvent::Key {
            protocol: KeyProtocol::Traditional,
            code: 57352,
            ..
        })
    ));
    assert!(matches!(
        decode_input(&Event::Escape(b"OD".to_vec())),
        Some(InputEvent::Key { code: 57350, .. })
    ));
    assert_eq!(
        decode_input(&Event::Escape(b"x".to_vec())),
        Some(InputEvent::Text {
            text: "x".into(),
            alt: true
        })
    );
    assert!(matches!(
        decode_input(&Event::Input(vec![3])),
        Some(InputEvent::Key {
            code: 99,
            modifiers: 4,
            ..
        })
    ));
    assert!(matches!(
        csi("Z"),
        Some(InputEvent::Key {
            code: 9,
            modifiers: 1,
            ..
        })
    ));
}

#[test]
fn sgr_mouse_focus_and_paste_preserve_observed_fields() {
    assert!(matches!(
        csi("<0;12;7M"),
        Some(InputEvent::Mouse {
            button: Some(0),
            kind: MouseKind::Press,
            x: 12,
            y: 7,
            ..
        })
    ));
    assert!(matches!(
        csi("<20;12;7m"),
        Some(InputEvent::Mouse {
            kind: MouseKind::Release,
            modifiers: 5,
            ..
        })
    ));
    assert!(matches!(
        csi("<32;1000;700M"),
        Some(InputEvent::Mouse {
            button: Some(0),
            kind: MouseKind::Drag,
            x: 1000,
            y: 700,
            ..
        })
    ));
    assert!(matches!(
        csi("<35;2;3M"),
        Some(InputEvent::Mouse {
            button: None,
            kind: MouseKind::Motion,
            ..
        })
    ));
    assert!(matches!(
        csi("<65;2;3M"),
        Some(InputEvent::Mouse {
            button: Some(1),
            kind: MouseKind::Wheel,
            ..
        })
    ));
    assert!(matches!(
        csi("<128;2;3M"),
        Some(InputEvent::Mouse {
            button: Some(7),
            kind: MouseKind::Press,
            ..
        })
    ));
    assert!(matches!(
        csi("<163;2;3M"),
        Some(InputEvent::Mouse {
            button: Some(10),
            kind: MouseKind::Drag,
            ..
        })
    ));
    assert_eq!(csi("I"), Some(InputEvent::Focus { focused: true }));
    assert_eq!(csi("O"), Some(InputEvent::Focus { focused: false }));
    assert_eq!(csi("200~"), Some(InputEvent::Paste { start: true }));
    assert_eq!(csi("201~"), Some(InputEvent::Paste { start: false }));
}

#[test]
fn malformed_input_and_terminal_replies_never_become_user_events() {
    for bytes in [
        "",
        "0u",
        "97;0u",
        "97;257u",
        "97;1:4u",
        "97:65:66:67u",
        "97;1;55296u",
        "1114112u",
        "97;1;65;66u",
        "97;x:1u",
        "?1u",
        ">1u",
        "1;2R",
        "?1;2c",
        "4;600;800t",
        "1;2$y",
        "<0;0;7M",
        "<0;7;0M",
        "<0;7;8;9M",
        "<64;7;8m",
        "<99;7;8M",
        "<0;7;8X",
        "27;0;65~",
        "27;2;65;7~",
        "200;1~",
        "1;2I",
        "2A",
        "1;1:0A",
    ] {
        assert_eq!(csi(bytes), None, "decoded malformed/reply {bytes:?}");
    }
    for bytes in ["27;;65~", "27;9;65~", "27;2:2;65~", "97::;1u"] {
        assert_eq!(csi(bytes), None, "decoded malformed {bytes:?}");
    }
    for event in [
        Event::Osc(b"52;c;AAAA".to_vec()),
        Event::Dcs(b"1+r544e=78".to_vec()),
        Event::Apc(b"Gi=77331;OK".to_vec()),
        Event::Malformed("timeout".into()),
        Event::Input(vec![0xff]),
    ] {
        assert_eq!(decode_input(&event), None);
    }
}

#[test]
fn every_protocol_and_phase_remains_bounded_at_canvas_and_coordinate_extremes() {
    for protocol in [Protocol::Kitty, Protocol::Sixel, Protocol::Iterm] {
        let mut graphics = Graphics::new();
        for (row, column, width, height) in [
            (1, 1, 1, 1),
            (65535, 65535, 65535, 65535),
            (65533, 65533, 65535, 65535),
            (1, 1, 65535, 65535),
        ] {
            for phase in 0..8 {
                let frame = graphics.frame(protocol, phase * 4, row, column, width, height);
                assert!(frame.len() < 24_000, "unbounded {protocol:?} phase {phase}");
                assert!(
                    graphics
                        .frame(protocol, phase * 4 + 1, row, column, width, height)
                        .is_empty()
                );
            }
        }
        assert!(!graphics.clear().is_empty());
        assert!(graphics.clear().is_empty());
    }
}

#[test]
fn input_exports_explicit_typed_events_and_complete_unicode_text() {
    let input = decode_input(&Event::Input("λ🙂".as_bytes().to_vec())).unwrap();
    assert_eq!(
        input,
        InputEvent::Text {
            text: "λ🙂".into(),
            alt: false
        }
    );
    let value = serde_json::to_value(csi("97;6:3u").unwrap()).unwrap();
    assert_eq!(value["event"], "key");
    assert_eq!(value["protocol"], "kitty");
    assert_eq!(value["kind"], "release");
    assert_eq!(value["modifiers"], 5);
    assert_eq!(value["code"], 97);
    let decoded: InputEvent = serde_json::from_value(value).unwrap();
    assert_eq!(Some(decoded), csi("97;6:3u"));
}

#[test]
fn control_frames_do_not_claim_keyboard_receipt() {
    for bytes in [
        b"\x1b^\x1b\\q".as_slice(),
        b"\x1bXC\x1b\\q",
        b"\x1b(Bq",
        b"\x1b#8q",
    ] {
        let events = terminal_showcase::protocol::Parser::new().feed(bytes);
        let inputs: Vec<_> = events.iter().filter_map(decode_input).collect();
        assert_eq!(
            inputs,
            [InputEvent::Text {
                text: "q".into(),
                alt: false
            }]
        );
    }
}

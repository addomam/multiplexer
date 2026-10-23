use terminal_showcase::capability::Report;
use terminal_showcase::presentation::{Demo, View, cell_width, clip, demo_canvas, diff, render};

#[test]
fn clipping_preserves_graphemes_cells_and_closes_terminal_styles() {
    assert_eq!(cell_width("e\u{301}界👩‍💻🇫🇷"), 7);
    assert_eq!(clip("e\u{301}界x", 2), "e\u{301}");
    assert_eq!(clip("界x", 1), "");
    assert_eq!(clip("👩‍💻x", 2), "👩‍💻");
    assert_eq!(clip("🇫🇷x", 2), "🇫🇷");
    let colored = clip("\x1b[31me\x1b[1m\u{301}界z", 1);
    assert!(colored.contains("e\x1b[1m\u{301}"));
    assert!(colored.ends_with("\x1b[0m"));
    assert_eq!(cell_width(&colored), 1);
    let linked = clip("\x1b]8;;https://example.com\x1b\\界abc", 3);
    assert!(linked.contains("界a"));
    assert!(linked.ends_with("\x1b]8;;\x1b\\"));
    assert_eq!(cell_width(&linked), 3);
}

#[test]
fn clipping_drops_non_printing_side_effects_and_handles_unterminated_sequences() {
    assert_eq!(clip("a\x1b[2Jb\x1b]52;c;YWJj\x07c", 20), "abc");
    assert_eq!(clip("ok\x1b[38;2;", 20), "ok");
    assert_eq!(clip("a\n\r\tb", 20), "ab");
    assert_eq!(clip("a\x1b界b", 20), "ab");
    assert_eq!(clip("\x1b[31mx", 0), "");
    assert_eq!(cell_width("\x1b[38;2;1;2;3m♥️\x1b[0m"), 2);
}

#[test]
fn every_demo_remains_bounded_across_resize_boundaries() {
    for (columns, rows) in [
        (0, 0),
        (1, 1),
        (20, 5),
        (39, 11),
        (40, 12),
        (71, 18),
        (72, 18),
        (80, 24),
        (120, 36),
        (600, 300),
        (u16::MAX, u16::MAX),
    ] {
        let report = Report::new(columns, rows);
        for demo in Demo::ALL {
            for expanded in [false, true] {
                let mut view = View::new(demo, false);
                view.expanded = expanded;
                view.diagnostics = true;
                view.input_log = vec!["界👩‍💻 event".repeat(100)];
                let frame = render(&report, &view);
                assert_eq!(
                    frame.len(),
                    usize::from(rows.min(200)),
                    "{demo:?} at {columns}x{rows}"
                );
                assert!(
                    frame
                        .iter()
                        .all(|row| cell_width(row) <= usize::from(columns.min(500)))
                );
                assert!(frame.iter().all(|row| !row.contains("\x1b[2J")));
            }
        }
    }
}

#[test]
fn compact_overview_keeps_all_categories_and_simultaneous_visuals() {
    let report = Report::new(40, 12);
    let frame = render(&report, &View::new(Demo::Overview, true));
    let text = frame.join("\n");
    for category in [
        "Color",
        "Typography",
        "Graphics",
        "Cursor",
        "Keyboard",
        "Mouse",
        "Integration",
    ] {
        assert!(text.contains(category), "missing category {category}");
    }
    assert!(text.contains("▄"), "truecolor gradient remains visible");
    assert!(text.contains("界"), "Unicode remains visible");
    assert!(text.contains("Italic"), "typography remains visible");
}

#[test]
fn screenshot_is_a_fixed_reproducible_frame_without_live_controls() {
    let mut report = Report::new(80, 24);
    report.identity = "fixture endpoint".into();
    let mut view = View::new(Demo::Overview, true);
    let fixture = render(&report, &view);
    view.tick = u64::MAX;
    view.paused = true;
    view.expanded = true;
    view.diagnostics = true;
    view.input_log = vec!["SECRET_DEBUG_EVENT".into()];
    assert_eq!(fixture, render(&report, &view));
    let text = fixture.join("\n");
    assert!(text.contains("fixture endpoint"));
    assert!(text.contains("TERMINAL / SHOWCASE"));
    // Inline golden fixture: first gradient tile is independent of live time
    // and of the number of remaining tiles in the resized viewport.
    assert!(fixture[6].contains("\x1b[48;2;39;91;183m\x1b[38;2;121;206;180m▄"));
    assert!(!text.contains("SECRET_DEBUG_EVENT"));
    assert!(!text.contains("P/F"));
    assert!(!text.contains("R cursor"));
}

#[test]
fn row_diff_only_writes_changed_rows_and_erases_removed_rows() {
    assert!(diff(&["same".into()], &["same".into()], true).is_empty());
    let bytes = diff(
        &["same".into(), "old long row".into(), "stale".into()],
        &["same".into(), "new".into()],
        false,
    );
    let output = String::from_utf8(bytes).unwrap();
    assert!(!output.contains("\x1b[1;1H"));
    assert!(output.contains("\x1b[2;1H\x1b[2K"));
    assert!(output.contains("new"));
    assert!(output.contains("\x1b[3;1H\x1b[2K"));
    assert!(!output.contains("old long row"));
    assert!(!output.contains("\x1b[2J"));
    let synchronized = diff(&[], &["x".into()], true);
    assert!(synchronized.starts_with(b"\x1b[?2026h"));
    assert!(synchronized.ends_with(b"\x1b[?2026l"));
}

#[test]
fn raw_demo_canvas_is_blank_bounded_and_separate_from_matrix() {
    assert!(demo_canvas(&Report::new(40, 12)).is_none());
    for (columns, rows) in [(72, 18), (80, 24), (120, 36)] {
        let report = Report::new(columns, rows);
        let (row, column, width, height) = demo_canvas(&report).unwrap();
        assert!(width >= 20 && height >= 4);
        assert!(column + width - 1 <= columns);
        assert!(row + height - 1 <= rows);
        for demo in [Demo::Control, Demo::Graphics, Demo::Unicode] {
            let frame = render(&report, &View::new(demo, false));
            for canvas_row in row..row + height {
                let line = &frame[usize::from(canvas_row - 1)];
                // The sidebar is populated; the canvas itself is only background spaces.
                assert!(line.contains(&" ".repeat(usize::from(width))));
            }
            let all = frame.join("\n");
            assert!(all.contains("Graphics"));
            assert!(all.contains("Keyboard"));
            assert!(all.contains("Mouse"));
        }
    }
}

#[test]
fn color_laboratory_really_emits_all_256_foreground_and_background_entries() {
    let report = Report::new(80, 24);
    let text = render(&report, &View::new(Demo::Colors, false)).join("\n");
    for index in 0..256 {
        assert!(
            text.contains(&format!("38;5;{index}m")),
            "foreground {index}"
        );
        assert!(
            text.contains(&format!("48;5;{index}m")),
            "background {index}"
        );
    }
    for index in 30..38 {
        assert!(text.contains(&format!("\x1b[{index}m")));
        assert!(text.contains(&format!("\x1b[{}m", index + 10)));
        assert!(text.contains(&format!("\x1b[{}m", index + 60)));
        assert!(text.contains(&format!("\x1b[{}m", index + 70)));
    }
    assert!(text.contains("\x1b[39;49m"));
    assert!(text.contains("\x1b[38;2;"));
    assert!(text.contains("\x1b[48;2;"));
}

#[test]
fn typography_pages_emit_every_underline_style_and_selective_resets() {
    let report = Report::new(80, 24);
    let mut view = View::new(Demo::Typography, false);
    let mut text = render(&report, &view).join("\n");
    view.tick = 180;
    text.push_str(&render(&report, &view).join("\n"));
    for style in 0..=5 {
        assert!(text.contains(&format!("\x1b[4:{style}m")));
    }
    for reset in [22, 23, 24, 25, 28, 59] {
        assert!(text.contains(&format!("\x1b[{reset}m")));
    }
    assert!(text.contains("\x1b[27;29;55m"));
    assert!(text.contains("58:2::251:146:128"));
    assert!(text.contains("58;5;45"));
    assert!(text.contains("\x1b[5m"));
    assert!(text.contains("\x1b[6m"));
    let screenshot = render(&Report::new(120, 36), &View::new(Demo::Typography, true)).join("\n");
    assert!(!screenshot.contains("\x1b[5m"));
    assert!(!screenshot.contains("\x1b[6m"));
    assert!(!screenshot.contains("P/F"));
}

#[test]
fn paused_details_pages_remain_navigable_and_keep_all_categories() {
    let report = Report::new(80, 24);
    let mut view = View::new(Demo::Integration, false);
    view.expanded = true;
    view.paused = true;
    let first = render(&report, &view);
    view.tick = 180;
    let second = render(&report, &view);
    assert_ne!(first, second);
    for frame in [first, second] {
        let text = frame.join("\n");
        for category in [
            "Display & color",
            "Typography & Unicode",
            "Graphics",
            "Cursor & screen",
            "Keyboard",
            "Mouse",
            "Terminal integration",
        ] {
            assert!(
                text.contains(category),
                "{category} disappears on evidence page"
            );
        }
    }
}

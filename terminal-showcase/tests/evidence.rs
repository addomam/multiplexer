use terminal_showcase::{
    capability::{Discovery, Report, Status},
    protocol::Event,
};
#[test]
fn unrecognized_mouse_mode_does_not_prove_unanswered_mouse_modes_unsupported() {
    let mut discovery = Discovery::new();
    let mut report = Report::new(80, 24);
    discovery.queries(0);
    assert!(discovery.ingest(&Event::Csi(b"?1000;0$y".to_vec()), &mut report, 1));
    discovery.expire(&mut report, 2000);
    assert_eq!(
        report.get("mouse").unwrap().status,
        Status::Unknown,
        "1006/1002/1003 never replied; one negative must not classify the whole family"
    );
}
#[test]
fn all_explicit_mouse_mode_negatives_can_establish_non_support() {
    let mut discovery = Discovery::new();
    let mut report = Report::new(80, 24);
    discovery.queries(0);
    for mode in [1000, 1002, 1003, 1006] {
        assert!(discovery.ingest(
            &Event::Csi(format!("?{mode};0$y").into_bytes()),
            &mut report,
            1
        ));
    }
    assert_eq!(report.get("mouse").unwrap().status, Status::Unsupported);
}

#[test]
fn fragmented_unicode_input_reaches_inspector_as_complete_scalars() {
    use terminal_showcase::{
        graphics::{InputEvent, decode_input},
        protocol::Parser,
    };
    let mut parser = Parser::new();
    let mut observed = String::new();
    for byte in "Л界🚀".as_bytes() {
        for event in parser.feed(&[*byte]) {
            if let Some(InputEvent::Text { text, .. }) = decode_input(&event) {
                observed.push_str(&text);
            }
        }
    }
    assert_eq!(
        observed, "Л界🚀",
        "partial UTF-8 reads must not be dropped by the live inspector"
    );
}

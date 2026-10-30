//! Final integrated acceptance, never an isolated fencing probe. The selected
//! real test fails rather than treating an unknown application boundary as safe.
use super::*;
use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Config;
use alacritty_terminal::vte::ansi::Processor;
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

const ROWS: usize = 45;
const COLS: usize = 160;
#[derive(Clone)]
struct Listener;
impl EventListener for Listener {
    fn send_event(&self, _: Event) {}
}
struct Geometry;
impl Dimensions for Geometry {
    fn total_lines(&self) -> usize {
        ROWS
    }
    fn screen_lines(&self) -> usize {
        ROWS
    }
    fn columns(&self) -> usize {
        COLS
    }
}

pub(super) struct NativeScreen {
    term: Term<Listener>,
    parser: Processor,
    line: String,
}

impl NativeScreen {
    pub fn new() -> Self {
        Self {
            term: Term::new(Config::default(), &Geometry, Listener),
            parser: Processor::new(),
            line: String::new(),
        }
    }

    pub fn advance(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    pub fn contains(&mut self, text: &str) -> bool {
        self.line.clear();
        self.line.reserve(COLS * 4);
        for row in 0..ROWS {
            self.line.clear();
            for col in 0..COLS {
                self.line
                    .push(self.term.grid()[Point::new(Line(row as i32), Column(col))].c);
            }
            if self.line.contains(text) {
                return true;
            }
        }
        false
    }
}

fn render(bytes: &[u8]) -> (Vec<String>, Value) {
    let mut screen = NativeScreen::new();
    screen.advance(bytes);
    let term = &screen.term;
    let mut lines = Vec::with_capacity(ROWS);
    let mut colors = Vec::with_capacity(ROWS);
    for row in 0..ROWS {
        let mut line = String::with_capacity(COLS);
        let mut row_colors = Vec::with_capacity(COLS);
        for col in 0..COLS {
            let cell = &term.grid()[Point::new(Line(row as i32), Column(col))];
            line.push(cell.c);
            row_colors.push(json!({"fg":format!("{:?}",cell.fg),"bg":format!("{:?}",cell.bg),"flags":format!("{:?}",cell.flags)}));
        }
        lines.push(line);
        colors.push(row_colors);
    }
    (lines, json!(colors))
}

fn credential_strings(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::String(text) if !text.is_empty() => output.push(text.clone()),
        Value::Array(values) => values
            .iter()
            .for_each(|value| credential_strings(value, output)),
        Value::Object(values) => values
            .values()
            .for_each(|value| credential_strings(value, output)),
        _ => {}
    }
}
fn handle_refs(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::Object(values) => {
            if let Some(reference) = values.get("ref").and_then(Value::as_str) {
                output.push(reference.to_owned());
            }
            values.values().for_each(|value| handle_refs(value, output));
        }
        Value::Array(values) => values.iter().for_each(|value| handle_refs(value, output)),
        _ => {}
    }
}

// Match across terminal wrapping and box borders, but preserve cell positions.
// Only rendered, redacted cells are retained; raw streams/auth/rollouts stay in
// the owned temporary root and are removed by Guard on every exit path.
fn redact(lines: Vec<String>, secrets: &[String]) -> Vec<String> {
    let mut cells: Vec<char> = lines.join("\n").chars().collect();
    let compact: Vec<(usize, char)> = cells
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, c)| !c.is_whitespace() && !('\u{2500}'..='\u{257f}').contains(c))
        .collect();
    let text: Vec<char> = compact.iter().map(|(_, c)| *c).collect();
    for secret in secrets {
        let secret: Vec<char> = secret.chars().filter(|c| !c.is_whitespace()).collect();
        if secret.is_empty() || secret.len() > text.len() {
            continue;
        }
        for (start, window) in text.windows(secret.len()).enumerate() {
            if window == secret {
                for &(index, _) in &compact[start..start + secret.len()] {
                    cells[index] = '•';
                }
            }
        }
    }
    cells
        .into_iter()
        .collect::<String>()
        .split('\n')
        .map(str::to_owned)
        .collect()
}

#[test]
fn rendered_evidence_redacts_wrapped_credentials_and_private_refs() {
    let mut secrets = Vec::new();
    credential_strings(
        &json!({"tokens":{"access":"synthetic-auth-123","refresh":"synthetic-refresh-456"}}),
        &mut secrets,
    );
    handle_refs(
        &json!({"control":{"ref":"private-handle-789"}}),
        &mut secrets,
    );
    secrets.push("/private/owned-root".to_owned());
    let (lines, colors) = render(
        b"\x1b[2J\x1b[H\x1b[32mPUBLIC OWNER\x1b[0m\r\nsynthetic-auth-\r\n123 synthetic-refresh-456\r\nprivate-handle-789\r\n/private/owned-\r\nroot",
    );
    let redacted = redact(lines, &secrets);
    assert_eq!(redacted.len(), ROWS);
    assert!(redacted.iter().all(|line| line.chars().count() == COLS));
    let compact: String = redacted
        .join("")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    for secret in secrets {
        assert!(
            !compact.contains(&secret),
            "private evidence escaped redaction"
        );
    }
    assert!(redacted[0].starts_with("PUBLIC OWNER"));
    assert!(redacted[1].starts_with("•••••••••••••••"));
    assert_ne!(colors[0][0]["fg"], colors[1][0]["fg"]);
}

pub(super) fn retain_native(live: &mut Live, label: &str) {
    let auth: Value = serde_json::from_slice(&fs::read(live.root.join("codex/auth.json")).unwrap())
        .expect("approved auth must be valid JSON before retaining redacted evidence");
    let mut secrets = live.private_refs.clone();
    credential_strings(&auth, &mut secrets);
    handle_refs(&live.controller, &mut secrets);
    handle_refs(&live.control, &mut secrets);
    secrets.push(live.root.to_str().unwrap().to_owned());
    live.captures += 1;
    for (index, viewer) in live.viewers.iter().enumerate() {
        let (lines, colors, raw_bytes_observed) = {
            let output = viewer.capture.lock();
            assert!(
                !output.raw.is_empty(),
                "actual native capture is unavailable"
            );
            let (lines, colors) = render(&output.raw);
            (lines, colors, output.raw.len())
        };
        let lines = redact(lines, &secrets);
        let stem = format!("{:04}-attachment-{index}", live.captures);
        private_file(&live.evidence.join(format!("{stem}.txt")))
            .write_all(lines.join("\n").as_bytes())
            .unwrap();
        private_file(&live.evidence.join(format!("{stem}.json")))
            .write_all(
                serde_json::to_string_pretty(&json!({
                    "phase":label,"rows":ROWS,"columns":COLS,"lines":lines,"colors":colors,
                    "raw_bytes_observed":raw_bytes_observed,"redacted":true
                }))
                .unwrap()
                .as_bytes(),
            )
            .unwrap();
    }
    eprintln!(
        "redacted rendered native captures phase={label:?} directory={} sequence={}",
        live.evidence.display(),
        live.captures
    );
}

type Matrix = BTreeMap<u8, Vec<&'static str>>;
fn same_outstanding(left: &Value, right: &Value) -> bool {
    let (Some(left), Some(right)) = (left.as_array(), right.as_array()) else {
        return false;
    };
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            let (Some(left), Some(right)) = (left.as_object(), right.as_object()) else {
                return false;
            };
            left.iter()
                .all(|(key, value)| key == "elapsed_ms" || right.get(key) == Some(value))
                && right
                    .iter()
                    .all(|(key, value)| key == "elapsed_ms" || left.get(key) == Some(value))
        })
}
fn observed(matrix: &mut Matrix, ids: &[u8], fact: &'static str) {
    for id in ids {
        matrix.entry(*id).or_default().push(fact);
    }
}
fn remember(live: &mut Live, value: &Value) {
    handle_refs(value, &mut live.private_refs);
}
fn state(live: &Live) -> Value {
    call(&live.socket, json!({"op":"state","terminal":live.terminal}))
}
fn ownership(live: &Live) -> Value {
    state(live)["ownership"].clone()
}
fn handle(live: &Live, capability: &Value, method: &str) -> Value {
    call(
        &live.socket,
        json!({"op":"control-handle","handle":capability,"method":method}),
    )
}
fn controller(live: &mut Live, label: &str) -> Value {
    let result = call(
        &live.socket,
        json!({"op":"controller-create","role":"external","label":label}),
    );
    remember(live, &result);
    result
}
fn claim(live: &mut Live, controller: &Value, when: Value, force: bool) -> Value {
    let mut request = json!({"op":"ownership-claim","controller":controller,
        "terminal":live.terminal,"observed":ownership(live)["observed"],
        "mode":if force {"force"} else {"cooperative"}});
    if !force {
        request["when"] = when;
    }
    let result = call(&live.socket, request);
    remember(live, &result);
    result
}
fn yield_to(live: &Live, control: &Value, claim_id: &Value) {
    call(
        &live.socket,
        json!({"op":"control-handle","handle":control,
        "method":"yield-to","claim_id":claim_id}),
    );
}
fn deny(live: &Live, request: Value) {
    let response = exchange(&live.socket, request, DAEMON).unwrap();
    assert!(
        response["ok"] == false,
        "unauthorized/stale action was accepted"
    );
}
fn offer(live: &mut Live, request: &Value) -> Value {
    let value = until(
        "complete-broker safe offer requires authoritative application settlement; unknown native/provider settlement blocks acceptance",
        DAEMON,
        || {
            live.drain();
            let value = handle(live, &request["claim"], "read");
            (value["phase"] == "offered").then_some(value)
        },
    );
    remember(live, &value);
    value
}
fn accept(live: &mut Live, offered: &Value) {
    let grant = handle(live, &offered["offer"], "accept");
    remember(live, &grant);
    assert!(
        grant["mode"] == "safe" && grant["unsettled"] == json!([]),
        "grant was not genuinely safe"
    );
    live.control = grant["control"].clone();
}
fn native(live: &mut Live, viewer: usize, bytes: &[u8]) {
    live.viewers[viewer].master.write_all(bytes).unwrap();
    live.drain();
}
fn drawn(live: &mut Live, viewer: usize, text: &str) {
    until("actual native ownership/request surface", DAEMON, || {
        live.drain();
        let (lines, _) = render(&live.viewers[viewer].capture.lock().raw);
        lines.join("\n").contains(text).then_some(())
    });
}
fn drawn_owner(live: &mut Live, viewer: usize) {
    let snapshot = call(&live.socket, json!({"op":"snapshot"}));
    let terminal = snapshot["terminals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == live.terminal)
        .unwrap();
    let project = snapshot["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == terminal["project_id"])
        .unwrap()["name"]
        .as_str()
        .unwrap();
    until(
        "native ownership and ordinary project header",
        DAEMON,
        || {
            live.drain();
            let (lines, _) = render(&live.viewers[viewer].capture.lock().raw);
            (ownership(live)["owner"]["role"] == "attachment" && lines[0].contains(project))
                .then_some(())
        },
    );
}
fn wait_owner(live: &mut Live, controller_id: &Value) {
    until("committed native ownership", DAEMON, || {
        live.drain();
        (ownership(live)["owner"]["controller_id"] == *controller_id).then_some(())
    });
}
fn same_human(live: &Live, before: &Value) {
    let now = ownership(live);
    assert!(
        now["owner"]["controller_id"] == before["owner"]["controller_id"],
        "human owner changed"
    );
    assert!(
        now["observed"]["generation"] == before["observed"]["generation"],
        "conditional keep/withdraw rotated authority"
    );
}
fn no_private_public(live: &Live) {
    let values = [
        call(&live.socket, json!({"op":"snapshot"})),
        call(
            &live.socket,
            json!({"op":"events","after":0,"timeout_ms":0}),
        ),
        live.inspect(),
    ];
    let mut references = live.private_refs.clone();
    handle_refs(&live.controller, &mut references);
    handle_refs(&live.control, &mut references);
    for value in values {
        let serialized = value.to_string();
        assert!(
            references
                .iter()
                .all(|reference| !serialized.contains(reference)),
            "private capability leaked into public inspection"
        );
    }
}

fn ownership_and_native(live: &mut Live, matrix: &mut Matrix) {
    let requester = live.controller.clone();
    let target = live.target.clone();
    let pid = state(live)["pid"].clone();
    live.attach();
    assert!(live.inspect()["target"] == target && live.viewers.len() == 2);
    live.capture("two native wrappers observe the same managed frontend");
    observed(
        matrix,
        &[1, 24],
        "Two actual controlling-PTY attachments observe one unchanged live target and PID.",
    );
    for capability in [
        live.controller.clone(),
        json!({"kind":"claim","ref":"invalid"}),
    ] {
        deny(
            live,
            json!({"op":"input","terminal":live.terminal,"control":capability,"text":"MUST_NOT_BE_TYPED"}),
        );
    }
    observed(
        matrix,
        &[7, 29],
        "Controller/claim references do not authorize application input.",
    );
    let b = controller(live, "complete broker recovery");
    let old = live.control.clone();
    let pending = claim(live, &b["controller"], json!({"kind":"now"}), false);
    assert!(pending["kind"] == "claim-state");
    assert!(handle(live, &old, "read")["control"] == old);
    let read = handle(live, &pending["claim"], "read");
    assert!(read.get("control").is_none());
    observed(
        matrix,
        &[25, 30, 32],
        "Separate raw claim/read calls preserve old authority and do not grant control.",
    );
    yield_to(live, &old, &pending["claim_id"]);
    let ready = offer(live, &pending);
    assert!(ownership(live)["owner"].is_null());
    let reserved = ownership(live)["reservation"]["claim_id"].clone();
    let opportunist = controller(live, "cannot steal reserved offer");
    deny(
        live,
        json!({"op":"ownership-claim","controller":opportunist["controller"],
        "terminal":live.terminal,"observed":ownership(live)["observed"],"mode":"cooperative","when":{"kind":"now"}}),
    );
    assert!(ownership(live)["reservation"]["claim_id"] == reserved);
    deny(
        live,
        json!({"op":"input","terminal":live.terminal,"control":ready["offer"],"text":"OFFER_IS_NOT_CONTROL"}),
    );
    observed(
        matrix,
        &[8, 28, 29, 30],
        "Yield leaves a reserved, unowned safe-offer gap; neither offer nor opportunist authorizes input.",
    );
    // Intentionally leave the acceptance reply unread. Recover commit through the
    // controller before closing the socket; do not replay the acquisition.
    let mut lost = UnixStream::connect(&live.socket).unwrap();
    lost.set_write_timeout(Some(DAEMON)).unwrap();
    let request = json!({"version":1,"persistent":true,"op":"control-handle","handle":ready["offer"],"method":"accept"});
    serde_json::to_writer(&mut lost, &request).unwrap();
    lost.write_all(b"\n").unwrap();
    let recovered = until(
        "recover committed acceptance without reading/replaying its reply",
        DAEMON,
        || {
            live.drain();
            let value = handle(live, &b["controller"], "read");
            value["controls"]
                .as_array()?
                .iter()
                .find(|row| row["terminal"] == live.terminal)
                .cloned()
        },
    );
    remember(live, &recovered);
    live.control = recovered["control"].clone();
    drop(lost);
    assert!(handle(live, &live.control, "read")["control"] == live.control);
    let generation = ownership(live)["observed"].clone();
    for (capability, method) in [
        (&ready["offer"], "accept"),
        (&pending["claim"], "cancel"),
        (&old, "release"),
    ] {
        deny(
            live,
            json!({"op":"control-handle","handle":capability,"method":method}),
        );
        assert!(ownership(live)["observed"] == generation);
    }
    observed(
        matrix,
        &[27, 31, 32],
        "Unread persistent acceptance survives socket closure, recovers privately, and stale messages cannot rotate the newer grant.",
    );
    // Actual physical observer routes, including a separate paste event, never
    // enter the agent prompt or create an application dispatch record.
    let before = ownership(live)["unsettled"].clone();
    for bytes in [
        b"OBSERVER_NEVER_TYPED".as_slice(),
        b"\x1b[120;1:2u",
        b"\x01\x01",
        b"\x1b[200~OBSERVER_PASTE_NEVER_TYPED\x1b[201~",
        b"\x1b[<0;12;8M",
    ] {
        native(live, 0, bytes);
        drawn(live, 0, "Locked");
        native(live, 0, b"\x1b");
    }
    assert!(same_outstanding(&ownership(live)["unsettled"], &before));
    let screen = call(
        &live.socket,
        json!({"op":"screen","terminal":live.terminal,"offset":0,"limit":45}),
    )
    .to_string();
    assert!(
        !screen.contains("OBSERVER_NEVER_TYPED") && !screen.contains("OBSERVER_PASTE_NEVER_TYPED")
    );
    live.capture("intercepted physical key repeat literal-prefix paste and mouse");
    observed(
        matrix,
        &[7, 16, 24],
        "Actual observer gestures are consumed without native dispatch or replayed prompt text.",
    );
    // The native client explicitly submits and accepts its own claim on its
    // original connection. A request alone still leaves external control valid.
    native(live, 0, b"\x01\x0c");
    let reservation = until("actual native claim", DAEMON, || {
        live.drain();
        ownership(live)["reservation"]
            .as_object()
            .map(|_| ownership(live)["reservation"].clone())
    });
    assert!(handle(live, &live.control, "read")["control"] == live.control);
    drawn(live, 0, "Waiting");
    let progress = Instant::now();
    while progress.elapsed() < Duration::from_millis(3100) {
        live.drain();
        thread::sleep(POLL);
    }
    assert!(ownership(live)["owner"]["controller_id"] == b["controller_id"]);
    live.capture("pending beyond three seconds without automatic transfer");
    observed(
        matrix,
        &[17, 25],
        "Pending progress remains non-granting beyond three seconds; the old control is still valid.",
    );
    yield_to(live, &live.control, &reservation["claim_id"]);
    wait_owner(live, &reservation["requester_id"]);
    drawn_owner(live, 0);
    let human = ownership(live);
    assert!(state(live)["pid"] == pid && live.inspect()["target"] == target);
    live.capture("safe API to native transfer with consumed opening gestures");
    observed(
        matrix,
        &[8, 9, 16, 24, 30],
        "Actual native requester accepts after explicit external yield without replacing the process or agent.",
    );
    // Idle policy is external. The requested warning is drawn before native
    // consent; a consumed keep gesture leaves this exact ownership generation.
    native(live, 0, b"\x1b[I");
    let idle = claim(
        live,
        &requester,
        json!({"kind":"human-idle","idle_ms":0,"warning_ms":1700}),
        false,
    );
    drawn(live, 0, "1700");
    live.capture("requested idle warning rendered before keep");
    native(live, 0, b"\x01");
    until(
        "native keep aborts only the explicit idle attempt",
        DAEMON,
        || {
            live.drain();
            (handle(live, &idle["claim"], "read")["phase"] == "aborted-by-owner").then_some(())
        },
    );
    same_human(live, &human);
    let end = Instant::now() + Duration::from_millis(1800);
    while Instant::now() < end {
        live.drain();
        same_human(live, &human);
        assert!(ownership(live)["reservation"].is_null());
        thread::sleep(POLL);
    }
    observed(
        matrix,
        &[11, 19, 20],
        "Configured idle warning and original-connection keep retain human authority without policy rearming.",
    );
    // Both known views must be backgrounded for an all-scope request. A newer
    // focus fact invalidates eligibility before acceptance; withdrawal keeps it.
    native(live, 0, b"\x1b[O");
    native(live, 1, b"\x1b[I");
    let background = claim(
        live,
        &requester,
        json!({"kind":"views-backgrounded","scope":"all"}),
        false,
    );
    same_human(live, &human);
    assert!(handle(live, &background["claim"], "read")["phase"] != "accepted");
    live.capture("all-view background request blocked by foreground observer");
    native(live, 1, b"\x1b[O");
    let background_offer = offer(live, &background);
    native(live, 1, b"\x1b[I");
    until(
        "new foreground view invalidates background offer",
        DAEMON,
        || {
            live.drain();
            let value = handle(live, &background["claim"], "read");
            (value["phase"] != "offered").then_some(())
        },
    );
    deny(
        live,
        json!({"op":"control-handle","handle":background_offer["offer"],"method":"accept"}),
    );
    handle(live, &background["claim"], "withdraw");
    same_human(live, &human);
    observed(
        matrix,
        &[21, 22],
        "Scoped original native views gate background transfer; stale acceptance and withdrawal preserve the human generation.",
    );
    // Explicit release is immediate hand-back, not an idle policy. No bytes have
    // been sent by the human on this settled path.
    native(live, 0, b"\x01\x0c");
    drawn(live, 0, "Release");
    native(live, 0, b"r");
    until("explicit native hand-back grants nobody", DAEMON, || {
        live.drain();
        ownership(live)["owner"].is_null().then_some(())
    });
    let grant = claim(live, &requester, json!({"kind":"now"}), false);
    assert!(grant["kind"] == "control" && grant["mode"] == "safe");
    live.control = grant["control"].clone();
    observed(
        matrix,
        &[11, 23],
        "Human release needs no idle delay and external reacquisition is an explicit fresh safe request.",
    );
    no_private_public(live);
    observed(
        matrix,
        &[29],
        "Public snapshots/events/agent inspection contain no retained private capability references.",
    );
}

fn model_operations(live: &mut Live, matrix: &mut Matrix) -> (String, Value) {
    let target = live.target.clone();
    let token = format!("COMPLETE_START_{}", uuid::Uuid::new_v4().simple());
    let steered = format!("COMPLETE_STEER_{}", uuid::Uuid::new_v4().simple());
    let (turn, _) = live.start(&format!("Explicitly approved integration check: run sleep 12 without escalation or background jobs, then {}. Follow additional same-turn steering; change no files.", token_instruction(&token)));
    live.wait_state("same-instance active steer target", RESPONSE, |state| {
        state["lifecycle"]["turn_id"] == turn && state["capabilities"]["steer"] == true
    });
    let receipt = live.mutation("agent-prompt", json!({"intent":"steer","expected_turn_id":turn,"text":format!("Also {} in this same turn.", token_instruction(&steered))}));
    live.wait_state("exact steer acknowledgement", RESPONSE, |state| {
        state["calls"].as_array().is_some_and(|rows| {
            rows.iter().any(|row| {
                row["call_id"] == receipt["call_id"]
                    && row["provider_ack"] == true
                    && row["turn_id"] == turn
            })
        })
    });
    live.complete(&turn);
    live.assistant_contains(&turn, &[&token, &steered]);
    live.native_contains("real same-instance prompt and steer continuation", &steered);
    deny(
        live,
        json!({"op":"agent-prompt","target":live.target,"control":live.control,"intent":"steer","expected_turn_id":turn,"text":"must not silently start a new turn"}),
    );
    assert!(live.inspect()["target"] == target);
    observed(
        matrix,
        &[1, 2, 6],
        "Explicit start and exact-turn steer produce authoritative and native continuation; stale steer does not start another turn.",
    );
    live.native_mode("Plan mode");
    let chosen = format!("COMPLETE_BLUE_{}", uuid::Uuid::new_v4().simple());
    let other = format!("COMPLETE_RED_{}", uuid::Uuid::new_v4().simple());
    let done = format!("COMPLETE_ANSWER_{}", uuid::Uuid::new_v4().simple());
    let turn = live.native_start(&format!("Use genuine request_user_input to ask exactly one question id complete_choice with options {chosen} and {other}. Do not simulate or answer it yourself. After the response, {} and the selected label; then finish without commands or implementation.", token_instruction(&done)));
    let question = live.pending(&turn, "question");
    let questions = question["questions"]
        .as_array()
        .expect("genuine question schema");
    assert!(
        questions.len() == 1
            && questions[0]["options"]
                .as_array()
                .is_some_and(|options| options.iter().any(|option| option["label"] == chosen))
    );
    let mut answers = serde_json::Map::new();
    answers.insert(
        questions[0]["id"].as_str().unwrap().to_owned(),
        json!({"answers":[chosen]}),
    );
    let receipt = live.respond("agent-answer", &question, json!({"answers":answers}));
    live.resolved(&question, &receipt);
    live.complete(&turn);
    live.assistant_contains(&turn, &[&done, &chosen]);
    live.native_contains("genuine structured question continuation", &done);
    observed(
        matrix,
        &[3, 6],
        "A genuine exact pending choice is answered and independently consumed; duplicate resolved responses are rejected.",
    );
    live.native_mode("Default");
    for decision in ["allow", "deny"] {
        let marker = live
            .root
            .join("outside")
            .join(format!("complete-{decision}-{}", uuid::Uuid::new_v4()));
        let contents = format!("PERMISSION_{}", uuid::Uuid::new_v4().simple());
        let done = format!("DECISION_{}", uuid::Uuid::new_v4().simple());
        let (turn, _) = live.start(&format!("This explicitly approved test concerns only the owned file below. Request genuine shell approval using sandbox_permissions=require_escalated for exactly printf '%s' '{contents}' > '{}'. Do not add a prefix_rule or use another tool, retry, background job, or denial workaround. After the approval result, {} and finish. Emit a real command approval, not a prose question.",marker.display(),token_instruction(&done)));
        let pending = live.pending(&turn, "command");
        assert!(
            pending["details"]
                .to_string()
                .contains(marker.to_str().unwrap())
                && !marker.exists()
        );
        let receipt = live.respond("agent-permission", &pending, json!({"decision":decision}));
        live.resolved(&pending, &receipt);
        live.complete(&turn);
        live.assistant_contains(&turn, &[&done]);
        live.native_contains("real permission continuation", &done);
        if decision == "allow" {
            until(
                "approved command actually writes only its owned marker",
                RESPONSE,
                || {
                    fs::read_to_string(&marker)
                        .ok()
                        .filter(|text| text == &contents)
                },
            );
        } else {
            let end = Instant::now() + Duration::from_secs(2);
            while Instant::now() < end {
                assert!(!marker.exists(), "denied command executed");
                live.drain();
                thread::sleep(POLL);
            }
        }
    }
    observed(
        matrix,
        &[4],
        "Both genuine request-only permission decisions have the expected post-completion filesystem effects and native continuation.",
    );
    let marker = live
        .root
        .join("outside/complete-interrupt-must-not-execute");
    let (turn,_)=live.start(&format!("Request genuine command approval with sandbox_permissions=require_escalated for exactly printf unexpected > '{}'. No prefix_rule, alternative tool, retry, background job or bypass. Wait for approval; the user will explicitly interrupt this turn before deciding.",marker.display()));
    let pending = live.pending(&turn, "command");
    live.detach();
    assert!(
        live.inspect()["target"] == target
            && live.inspect()["lifecycle"]["turn_id"] == turn
            && live.inspect()["lifecycle"]["activity"] == "awaiting-input"
    );
    live.attach();
    live.attach();
    observed(
        matrix,
        &[9, 13],
        "Actual wrapper detach/reattach leaves the exact active pending turn and frontend/backend alive.",
    );
    let receipt = live.mutation("agent-interrupt", json!({"turn_id":turn}));
    live.wait_state(
        "interrupt acknowledgement separate from completion",
        RESPONSE,
        |state| {
            state["calls"].as_array().is_some_and(|rows| {
                rows.iter()
                    .any(|row| row["call_id"] == receipt["call_id"] && row["provider_ack"] == true)
            })
        },
    );
    live.wait_state("authoritative interrupted outcome", RESPONSE, |state| {
        state["turn_outcomes"][turn.as_str()] == "interrupted"
            && state["lifecycle"]["backend"] == "running"
    });
    live.wait_state(
        "independent exact-turn request cleanup",
        RESPONSE,
        |state| {
            state["pending"]
                .as_array()
                .is_some_and(|rows| !rows.iter().any(|row| same_pending(row, &pending)))
        },
    );
    assert!(!marker.exists());
    live.capture("explicit interrupt outcome without process stop");
    observed(
        matrix,
        &[14],
        "Exact interrupt acknowledgement, interrupted completion, pending cleanup and process survival are independently observed.",
    );
    (turn, pending)
}

fn buffered_native(live: &mut Live, matrix: &mut Matrix, ended_turn: &str) {
    live.native_mode("Plan mode");
    let losing = format!("BUFFERED_RED_{}", uuid::Uuid::new_v4().simple());
    let winning = format!("AUTHORITATIVE_BLUE_{}", uuid::Uuid::new_v4().simple());
    let done = format!("RACE_CONTINUATION_{}", uuid::Uuid::new_v4().simple());
    let turn = live.native_start(&format!("Use genuine request_user_input now: exactly one question id buffered_choice, options in this exact order {losing} then {winning}. Do not simulate or answer it yourself. After the real response, {} and the selected label, then finish without tools or implementation.",token_instruction(&done)));
    let pending = live.pending(&turn, "question");
    deny(
        live,
        json!({"op":"agent-interrupt","target":live.target,"control":live.control,"turn_id":ended_turn}),
    );
    assert!(live.inspect()["lifecycle"]["turn_id"] == turn);
    observed(
        matrix,
        &[6, 14],
        "A stale interrupt cannot target the deliberately started replacement turn.",
    );
    live.native_contains("real native question before application pause", &losing);
    // Deliberate force is exercised as its own risky operation, not a safe-path
    // substitute. It changes authority, never the waiting turn or process.
    native(live, 0, b"\x01\x06");
    drawn(live, 0, "Force");
    native(live, 0, b"\r");
    drawn_owner(live, 0);
    let human = ownership(live);
    assert!(
        human["owner"]["role"] == "attachment" && live.inspect()["lifecycle"]["turn_id"] == turn
    );
    let pid = state(live)["pid"].as_i64().expect("live frontend pid") as i32;
    {
        let mut owned = live.guard.owned.lock();
        owned.discover().unwrap();
        assert!(
            owned.known.contains_key(&pid),
            "application hold must target the owned frontend only"
        );
        assert_eq!(unsafe { libc::kill(pid, libc::SIGSTOP) }, 0);
    }
    native(live, 0, b"\r");
    let retained = until(
        "actual native bytes forwarded while the frontend is stopped",
        DAEMON,
        || {
            live.drain();
            let calls = ownership(live)["unsettled"].clone();
            calls
                .as_array()?
                .iter()
                .any(|row| row["kind"] == "native-input")
                .then_some(calls)
        },
    );
    let release = live.root.join("release-slow-hook");
    let started = live.root.join("slow-hook-started");
    call(
        &live.socket,
        json!({"op":"project-new","name":"complete-slow-hook","directory":live.root,"config":{"hooks":[{"source":format!("print started > '{}'; while [[ ! -e '{}' ]]; do sleep 0.1; done; exit 7",started.display(),release.display()),"timeout_ms":30000}]}}),
    );
    until("owned slow hook is actually running", DAEMON, || {
        started.exists().then_some(())
    });
    let requester = live.controller.clone();
    let queued = claim(live, &requester, json!({"kind":"now"}), false);
    assert!(ownership(live)["owner"]["controller_id"] == human["owner"]["controller_id"]);
    let began = Instant::now();
    handle(live, &queued["claim"], "cancel");
    assert!(
        began.elapsed() < Duration::from_secs(3),
        "slow hook delayed cancellation"
    );
    assert!(
        ownership(live)["owner"].is_null()
            && same_outstanding(&ownership(live)["unsettled"], &retained)
    );
    observed(
        matrix,
        &[25, 26],
        "Ordinary pending cancel revokes the pretransfer native authority, grants nobody and preserves actual outstanding native work.",
    );
    let fresh = claim(live, &requester, json!({"kind":"now"}), false);
    assert!(
        fresh["kind"] == "claim-state"
            && fresh["phase"] == "waiting-settlement"
            && ownership(live)["owner"].is_null(),
        "unsettled buffered native input received a false safe grant"
    );
    assert!(handle(live, &fresh["claim"], "read")["offer"].is_null());
    handle(live, &fresh["claim"], "cancel");
    let began = Instant::now();
    let forced = claim(live, &requester, Value::Null, true);
    assert!(
        began.elapsed() < Duration::from_secs(3)
            && forced["mode"] == "forced"
            && !forced["unsettled"].as_array().unwrap().is_empty(),
        "force must be immediate, explicit and retain risk"
    );
    live.control = forced["control"].clone();
    assert!(
        same_outstanding(&ownership(live)["unsettled"], &retained)
            && live.inspect()["lifecycle"]["turn_id"] == turn
            && state(live)["pid"] == pid
    );
    fs::write(&release, b"release").unwrap();
    observed(
        matrix,
        &[9, 10, 12, 18, 26],
        "Stopped-frontend input stays outstanding through cancel/fresh request/explicit force, while a real slow hook cannot delay control and the same turn/process survives.",
    );
    let questions = pending["questions"]
        .as_array()
        .expect("real buffered question");
    assert!(
        questions.len() == 1
            && questions[0]["options"][0]["label"] == losing
            && questions[0]["options"][1]["label"] == winning
    );
    let mut answers = serde_json::Map::new();
    answers.insert(
        questions[0]["id"].as_str().unwrap().to_owned(),
        json!({"answers":[winning]}),
    );
    let receipt = live.respond("agent-answer", &pending, json!({"answers":answers}));
    live.resolved(&pending, &receipt);
    live.complete(&turn);
    live.assistant_contains(&turn, &[&done, &winning]);
    {
        let mut owned = live.guard.owned.lock();
        owned.discover().unwrap();
        assert!(owned.known.contains_key(&pid));
        assert_eq!(unsafe { libc::kill(pid, libc::SIGCONT) }, 0);
    }
    live.native_contains(
        "winning continuation after releasing buffered native Enter",
        &done,
    );
    live.capture("buffered-native losing-answer presentation after authoritative continuation");
    // The installed resolved notification does not name the winning answer.
    // Do not promote an ambiguous API response to applied from model prose.
    let inspected = live.inspect();
    let response = inspected["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["call_id"] == receipt["call_id"])
        .unwrap();
    let authoritative_winner = response["state"] == "applied";
    if !authoritative_winner {
        eprintln!(
            "complete-broker blocker: supported response-winner attribution is unavailable; retained native captures must not be advertised as authoritative accepted-answer proof"
        );
    }
    // Final application-fence validation is here, after complete integration and
    // genuine interactions. Neither turn completion, the above question, bytes
    // written to the PTY nor a delay clears the native batch.
    let requester = controller(live, "final reserved safe owner")["controller"].clone();
    let pending = claim(live, &requester, json!({"kind":"now"}), false);
    yield_to(live, &live.control, &pending["claim_id"]);
    let offered = offer(live, &pending);
    assert!(
        authoritative_winner,
        "complete-broker acceptance blocked: response-winner attribution remains unknown"
    );
    observed(
        matrix,
        &[5],
        "The exact response winner is authoritative and its actual native presentation is retained separately.",
    );
    accept(live, &offered);
    assert!(
        ownership(live)["unsettled"] == json!([]),
        "required native application settlement is unknown"
    );
    live.check_turn_budget();
    observed(
        matrix,
        &[8, 10, 15],
        "Authoritative native application settlement permits a genuine safe grant after the buffered-input case; force was not used as a substitute.",
    );
}

fn event_recovery(live: &mut Live, matrix: &mut Matrix) {
    let cursor = call(&live.socket, json!({"op":"snapshot"}))["seq"]
        .as_u64()
        .unwrap();
    for index in 0..2050 {
        call(
            &live.socket,
            json!({"op":"name","terminal":live.terminal,"name":format!("complete-gap-{index}")}),
        );
        if index % 16 == 0 {
            live.drain();
        }
    }
    let gap = call(
        &live.socket,
        json!({"op":"events","after":cursor,"timeout_ms":0}),
    );
    assert!(
        gap["gap"] == true && gap["snapshot"].is_object(),
        "lost event history was advertised as lossless"
    );
    assert!(live.inspect()["target"] == live.target);
    observed(
        matrix,
        &[13],
        "A real event-retention overrun explicitly supplies gap plus snapshot recovery without replaying a mutation.",
    );
}

pub(super) fn run() {
    let mut live = Live::open("real_complete_broker_handoff");
    let mut matrix = Matrix::new();
    let evidence = live.evidence.clone();
    let root = live.root.clone();
    let result = catch_unwind(AssertUnwindSafe(|| {
        ownership_and_native(&mut live, &mut matrix);
        event_recovery(&mut live, &mut matrix);
        let (ended, _) = model_operations(&mut live, &mut matrix);
        buffered_native(&mut live, &mut matrix, &ended);
        no_private_public(&live);
        assert!(
            (1..=32).all(|id| matrix.contains_key(&id)),
            "one or more mandatory scenarios lack actual assertions"
        );
    }));
    if result.is_err() {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            live.capture("failing complete-broker checkpoint; not acceptance")
        }));
    }
    let starts = live.starts;
    let cleanup = catch_unwind(AssertUnwindSafe(|| live.finish()));
    let removed = !root.exists();
    let report = json!({
        "test":"real_complete_broker_handoff","acceptance":if result.is_ok()&&cleanup.is_ok()&&removed {"PASS"}else{"BLOCKED_OR_FAILED"},
        "meaningful_turns_submitted":starts,"turn_ceiling":6,"owned_cleanup_verified":cleanup.is_ok()&&removed,
        "native_captures":"separate redacted rendered text and cell colors in this directory",
        "scenarios":(1u8..=32).map(|id|json!({"id":format!("interaction-{id:02}"),
            "observations":matrix.get(&id),"status":if matrix.contains_key(&id){"observed-assertions-see-overall-verdict"}else{"not-observed"}})).collect::<Vec<_>>()
    });
    private_file(&evidence.join("acceptance.json"))
        .write_all(serde_json::to_string_pretty(&report).unwrap().as_bytes())
        .unwrap();
    eprintln!("complete-broker redacted evidence: {}", evidence.display());
    if let Err(error) = result {
        resume_unwind(error);
    }
    if let Err(error) = cleanup {
        resume_unwind(error);
    }
    assert!(
        removed,
        "owned private root survived complete-broker teardown"
    );
}

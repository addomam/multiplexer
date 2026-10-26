//! Deterministic external ACP peer: real stdio and stateful effects, no model/network.
use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

struct Descendant(Child);

impl Drop for Descendant {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn emit(message: Value) -> std::io::Result<()> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &message)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

fn barrier(root: &Path, name: &str) -> std::io::Result<()> {
    fs::write(root.join(format!("{name}.received")), b"ready")?;
    while !root.join(format!("{name}.release")).exists() {
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}

fn stream(session: &str, category: &str, text: &str) -> std::io::Result<()> {
    emit(json!({"jsonrpc":"2.0","method":"session/update","params":{
        "sessionId":session,"update":{"sessionUpdate":category,"messageId":format!("{session}-{category}"),
        "content":{"type":"text","text":text}}}}))
}

fn receive(reader: &mut impl BufRead) -> Result<Option<Value>> {
    let mut line = String::new();
    if (&mut *reader).take(1024 * 1024 + 1).read_line(&mut line)? == 0 {
        return Ok(None);
    }
    if line.len() > 1024 * 1024 || !line.ends_with('\n') {
        return Err("invalid client frame".into());
    }
    Ok(Some(serde_json::from_str(&line)?))
}

fn unsupported(reader: &mut impl BufRead, root: &Path, session: Option<&str>) -> Result<()> {
    let requests = [
        ("unknown", "fixture/unimplemented", json!({}), -32601),
        (
            "url",
            "elicitation/create",
            json!({"mode":"url","message":"not a credential form","elicitationId":"unsupported-url","url":"https://invalid.example/never-open"}),
            -32602,
        ),
        (
            "form",
            "elicitation/create",
            json!({"mode":"form","message":"synthetic unsupported nested form","requestedSchema":{"type":"object","properties":{"nested":{"type":"object","properties":{}}}}}),
            -32602,
        ),
        (
            "filesystem",
            "fs/read_text_file",
            json!({"sessionId":session,"path":"/never-read"}),
            -32601,
        ),
        (
            "terminal",
            "terminal/create",
            json!({"sessionId":session,"command":"never-run"}),
            -32601,
        ),
    ];
    for (id, method, mut params, expected) in requests {
        if method == "elicitation/create" {
            if let Some(session) = session {
                params["sessionId"] = json!(session);
            } else {
                params["requestId"] = json!(0);
            }
        }
        emit(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
        let response = receive(reader)?.ok_or("missing unsupported response")?;
        if response["id"] != id || response["error"]["code"] != expected {
            return Err("unsupported request did not receive its exact protocol error".into());
        }
    }
    // JSON-RPC permits null IDs, though discouraged. Preserve their type and
    // answer explicitly rather than abandoning a valid unsupported responder.
    emit(json!({"jsonrpc":"2.0","id":null,"method":"fixture/null-id","params":{}}))?;
    let response = receive(reader)?.ok_or("null-ID responder missing")?;
    if response.get("id") != Some(&Value::Null) || response["error"]["code"] != -32601 {
        return Err("null-ID request did not receive its exact error".into());
    }
    fs::write(root.join("unsupported-complete"), b"all explicit errors")?;
    Ok(())
}

fn form_schema() -> Value {
    json!({"type":"object","properties":{
        "bonus":{"type":"integer","minimum":-100,"maximum":100,"default":0}
    },"required":["bonus"]})
}

fn rich_schema() -> Value {
    json!({"type":"object","properties":{
        "bonus":{"type":"integer","minimum":-100,"maximum":100,"default":0},
        "ratio":{"type":"number","minimum":0,"maximum":2,"default":1.0},
        "enabled":{"type":"boolean","default":true},
        "text":{"type":"string","minLength":2,"maxLength":3,"pattern":"^.*$","format":"email","default":"ab"},
        "choice":{"type":"string","enum":["add","subtract","unavailable"],"oneOf":[
            {"const":"add","title":"Add"},{"const":"subtract","title":"Subtract"}],"default":"add"},
        "titled":{"type":"string","oneOf":[{"const":"twenty","title":"Twenty"},{"const":"zero","title":"Zero"}]},
        "flags":{"type":"array","items":{"type":"string","enum":["a","b","c"]},"minItems":1,"maxItems":2,"default":["a"]},
        "titled_flags":{"type":"array","items":{"anyOf":[{"const":"red","title":"Red"},{"const":"blue","title":"Blue"}]},"maxItems":2},
        "optional":{"type":"integer","default":99}
    },"required":["bonus","ratio","enabled","text","choice","titled","flags","titled_flags"]})
}

fn send_form(id: Value, scope: Value, schema: Value, message: &str) -> Result<()> {
    let mut params = scope;
    params["mode"] = json!("form");
    params["message"] = json!(message);
    params["requestedSchema"] = schema;
    emit(json!({"jsonrpc":"2.0","id":id,"method":"elicitation/create","params":params}))?;
    Ok(())
}

fn form_bonus(response: &Value, id: &Value, rich: bool) -> Result<i64> {
    if response.get("id") != Some(id) || response.get("error").is_some() {
        return Err("form lost its exact original responder".into());
    }
    match response["result"]["action"].as_str() {
        Some("decline") => Ok(-10),
        Some("cancel") => Ok(-20),
        Some("accept") => {
            let values = &response["result"]["content"];
            let bonus = values["bonus"].as_i64().ok_or("integer answer required")?;
            if !rich {
                return Ok(bonus);
            }
            let ratio = values["ratio"].as_f64().ok_or("number answer required")?;
            let enabled = values["enabled"]
                .as_bool()
                .ok_or("boolean answer required")?;
            let text = values["text"].as_str().ok_or("string answer required")?;
            let choice = match values["choice"].as_str() {
                Some("add") => 10,
                Some("subtract") => -10,
                _ => return Err("unoffered enum".into()),
            };
            let titled = match values["titled"].as_str() {
                Some("twenty") => 20,
                Some("zero") => 0,
                _ => return Err("unoffered titled enum".into()),
            };
            let flags = values["flags"].as_array().ok_or("array answer required")?;
            let titled_flags = values["titled_flags"]
                .as_array()
                .ok_or("titled array answer required")?;
            Ok(bonus
                + (ratio * 4.0) as i64
                + if enabled { 100 } else { 0 }
                + text.chars().count() as i64
                + choice
                + titled
                + flags.len() as i64 * 3
                + titled_flags.len() as i64 * 7
                + values.get("optional").and_then(Value::as_i64).unwrap_or(0))
        }
        _ => Err("explicit form disposition required".into()),
    }
}

fn send_permission(id: Value, session: &str) -> Result<()> {
    emit(
        json!({"jsonrpc":"2.0","id":id,"method":"session/request_permission","params":{
        "sessionId":session,"toolCall":{"toolCallId":"arithmetic-gate","title":"Synthetic private tool",
            "kind":"execute","status":"pending","rawInput":{"operation":"add"}},
        "options":[
            {"optionId":"opaque-α","name":"Allow this invocation","kind":"allow_once"},
            {"optionId":"opaque-β","name":"Remember allow","kind":"allow_always"},
            {"optionId":"opaque-γ","name":"Reject this invocation","kind":"reject_once"},
            {"optionId":"opaque-δ","name":"Remember rejection","kind":"reject_always"}
        ]}}),
    )?;
    Ok(())
}

fn permission_allows(response: &Value, id: &Value) -> Result<bool> {
    if response.get("id") != Some(id) || response["result"]["outcome"]["outcome"] != "selected" {
        return Err("permission lost its exact selected responder".into());
    }
    match response["result"]["outcome"]["optionId"].as_str() {
        Some("opaque-α" | "opaque-β") => Ok(true),
        Some("opaque-γ" | "opaque-δ") => Ok(false),
        _ => Err("fabricated permission reached tool".into()),
    }
}

fn cancel_interactions(
    reader: &mut impl BufRead,
    root: &Path,
    session: &str,
    selected: &str,
) -> Result<()> {
    send_form(
        json!("cancel-form"),
        json!({"sessionId":session}),
        form_schema(),
        "Pending cancellation form",
    )?;
    send_permission(json!(73), session)?;
    barrier(root, "interactions")?;
    let mut notification = 0;
    let mut form = 0;
    let mut permission = 0;
    for _ in 0..3 {
        let response = receive(reader)?.ok_or("cancellation frame missing")?;
        if response["method"] == "session/cancel" && response["params"]["sessionId"] == session {
            notification += 1;
        } else if response["id"] == "cancel-form" {
            if selected == "form" {
                let _ = form_bonus(&response, &json!("cancel-form"), false)?;
            } else if response["result"]["action"] != "cancel" {
                return Err("form not cancelled".into());
            }
            form += 1;
        } else if response["id"] == 73 {
            if selected == "permission" {
                let _ = permission_allows(&response, &json!(73))?;
            } else if response["result"]["outcome"]["outcome"] != "cancelled" {
                return Err("permission not cancelled".into());
            }
            permission += 1;
        } else {
            return Err("wrong or duplicate cancellation response".into());
        }
    }
    if (notification, form, permission) != (1, 1, 1) {
        return Err("cancellation obligations not exactly once".into());
    }
    send_form(
        json!("late-form"),
        json!({"sessionId":session}),
        form_schema(),
        "Late session form",
    )?;
    send_permission(json!("late-permission"), session)?;
    for id in ["late-form", "late-permission"] {
        let response = receive(reader)?.ok_or("late cancellation missing")?;
        if response["id"] != id
            || (id == "late-form" && response["result"]["action"] != "cancel")
            || (id == "late-permission" && response["result"]["outcome"]["outcome"] != "cancelled")
        {
            return Err("late interaction became a choice or lost cancellation".into());
        }
    }
    barrier(root, "cancel-responses")?;
    Ok(())
}

fn s03_rendering(reader: &mut impl BufRead, root: &Path, session: &str) -> Result<()> {
    // Exact protocol round trips pace each 16-event wave below ingress capacity.
    // This tests transcript eviction, not accidental transport overflow.
    for wave in 0..132 {
        for index in 0..16 {
            stream(
                session,
                "agent_message_chunk",
                &format!("s03-seq-{:04}", wave * 16 + index),
            )?;
        }
        let id = format!("s03-drain-{wave}");
        emit(json!({"jsonrpc":"2.0","id":id,"method":"fixture/render-drain","params":{}}))?;
        let response = receive(reader)?.ok_or("render pacing response missing")?;
        if response["id"] != id || response["error"]["code"] != -32601 {
            return Err("render pacing lost exact responder".into());
        }
    }
    stream(session, "agent_thought_chunk", "s03-thought-retained")?;
    emit(
        json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":session,
        "update":{"sessionUpdate":"tool_call","toolCallId":"s03-tool",
        "title":"s03-tool-retained","kind":"other","status":"completed"}}}),
    )?;
    emit(
        json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":session,
        "update":{"sessionUpdate":"plan","entries":[{"content":"s03-plan-retained",
        "priority":"medium","status":"completed"}]}}}),
    )?;
    stream(
        session,
        "agent_message_chunk",
        "s03-hostile:\x1b]0;s03-title-owned\x07\x1b]52;c;czAzLWNsaXBib2FyZC1vd25lZA==\x07\x1b[2J\x1b[H\x1b[31ms03-csi-marker\x1b[0m\r\x08\x00\x7f\u{202e}s03-bidi\u{202c}",
    )?;
    stream(session, "agent_message_chunk", "s03-tail-retained-2112")?;
    emit(
        json!({"jsonrpc":"2.0","id":"s03-drain-tail","method":"fixture/render-drain","params":{}}),
    )?;
    let response = receive(reader)?.ok_or("retained tail pacing response missing")?;
    if response["id"] != "s03-drain-tail" || response["error"]["code"] != -32601 {
        return Err("retained tail not consumed in FIFO order".into());
    }
    barrier(root, "s03-updates")?;
    Ok(())
}

// Called in a fresh, single-threaded executable, before any watchdog exists.
// The intermediary is waited by the peer; only the reparented child retains I/O.
fn retain_orphan_pipes(root: &Path, keep: Option<i32>) -> Result<()> {
    let intermediary = unsafe { libc::getpid() };
    let child = unsafe { libc::fork() };
    if child < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    if child > 0 {
        return Ok(());
    }
    while unsafe { libc::getppid() } == intermediary {
        std::thread::sleep(Duration::from_millis(1));
    }
    if let Some(keep) = keep {
        for fd in [libc::STDIN_FILENO, libc::STDOUT_FILENO, libc::STDERR_FILENO] {
            if fd != keep {
                // Only this freshly forked fixture's own inherited stdio.
                unsafe { libc::close(fd) };
            }
        }
    }
    fs::write(root.join("retainer.pid"), std::process::id().to_string())?;
    fs::write(
        root.join("retainer.ready"),
        serde_json::to_vec(&json!({
            "parent":unsafe { libc::getppid() },
            "session":unsafe { libc::getsid(0) },
            "intermediary":intermediary
        }))?,
    )?;
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while root.exists()
        && !root.join("retainer.release").exists()
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

// Mirrors OMP18.12.6's advertised categories and string-valued setters. This
// fixture is protocol evidence only; it never pretends to be a real OMP run.
fn e06_options(mode: &str) -> Value {
    let mut options = json!([
        {"id":"mode","name":"Mode","category":"mode","type":"select","currentValue":"default",
            "options":[{"value":"default","name":"Default"},{"value":"plan","name":"Plan"}]},
        {"id":"model","name":"Model","category":"model","type":"select","currentValue":"fixture/other",
            "options":[{"value":"fixture/cheap","name":"Cheap"},{"value":"fixture/other","name":"Other"}]},
        {"id":"thinking","name":"Thinking","category":"thought_level","type":"select","currentValue":"off",
            "options":[{"value":"off","name":"Off"},{"value":"low","name":"Low"},{"value":"high","name":"High"}]},
        {"id":"fixture-detail","name":"Detail","type":"select","currentValue":"full",
            "options":[{"value":"brief","name":"Brief"},{"value":"full","name":"Full"}]}
    ]);
    if mode == "e06-settings-no-effort" {
        options.as_array_mut().unwrap().remove(2);
    } else if mode == "e06-settings-ambiguous" {
        let mut duplicate = options[2].clone();
        duplicate["id"] = json!("other-thinking");
        options.as_array_mut().unwrap().push(duplicate);
    } else if mode == "e06-settings-categories" {
        options[1]["id"] = json!("engine");
        options[2]["id"] = json!("depth");
    } else if mode == "e06-settings-model-dependent" {
        options[2]["options"] =
            json!([{"value":"off","name":"Off"},{"value":"high","name":"High"}]);
    } else if mode == "e06-settings-delimited" {
        options[3]["id"] = json!("fixture,detail=level");
        options[3]["options"] = json!([
            {"value":"brief,exact=value","name":"Delimited choice"},
            {"value":"full","name":"Full"}
        ]);
    } else if mode == "e06-settings-long-model" {
        options[1]["options"][0]["value"] = json!(format!("fixture/{}", "m".repeat(196)));
    } else if mode == "e06-settings-boundary-model" {
        options[1]["options"].as_array_mut().unwrap().push(json!({
            "value":format!("fixture/{}", "b".repeat(248)),"name":"Boundary model"
        }));
    }
    options
}

fn e06_update(session: &str, update: Value) -> Result<()> {
    emit(
        json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":session,"update":update}}),
    )?;
    Ok(())
}

fn main() -> Result<()> {
    let root = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("private fixture root required")?,
    );
    let mode = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "arithmetic".into());
    if mode == "descendant" {
        fs::write(root.join("descendant.pid"), std::process::id().to_string())?;
        loop {
            std::thread::park();
        }
    }
    if matches!(
        mode.as_str(),
        "s04-pipe-retainer" | "s04-pipe-retainer-stdin" | "s04-pipe-retainer-stdout"
    ) {
        let keep = match mode.as_str() {
            "s04-pipe-retainer-stdin" => Some(libc::STDIN_FILENO),
            "s04-pipe-retainer-stdout" => Some(libc::STDOUT_FILENO),
            _ => None,
        };
        return retain_orphan_pipes(&root, keep);
    }
    let retainer_mode = match mode.as_str() {
        "s04-retained-pipes" => Some("s04-pipe-retainer"),
        "s04-idle-stdin-retainer" => Some("s04-pipe-retainer-stdin"),
        "s04-stdout-retainer" => Some("s04-pipe-retainer-stdout"),
        _ => None,
    };
    if let Some(retainer_mode) = retainer_mode {
        let mut intermediary = Command::new(std::env::current_exe()?);
        intermediary
            .arg(&root)
            .arg(retainer_mode)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        unsafe {
            intermediary.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        if !intermediary.status()?.success() {
            return Err("retained-pipe intermediary failed".into());
        }
    }
    // Failure-only fixture fallback: if the daemon itself disappears, this
    // still-live session leader pins its own group. No passing cleanup assertion
    // relies on this; Host explicitly enables fallback only after all assertions.
    let parent = unsafe { libc::getppid() };
    let fallback_root = root.clone();
    let _watchdog = std::thread::spawn(move || {
        loop {
            if unsafe { libc::getppid() } != parent
                && fallback_root.join("fixture-fallback").exists()
            {
                let pid = unsafe { libc::getpid() };
                if unsafe { libc::getsid(0) } == pid {
                    unsafe {
                        libc::kill(-pid, libc::SIGKILL);
                    }
                }
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    });
    fs::write(root.join("peer.pid"), std::process::id().to_string())?;
    let _descendant = if matches!(
        mode.as_str(),
        "descendants" | "s04-complete" | "s04-stalled-descendants"
    ) {
        // The broker owns this process tree; Drop only covers fixture-local EOF.
        Some(Descendant(
            Command::new(std::env::current_exe()?)
                .arg(&root)
                .arg("descendant")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?,
        ))
    } else {
        None
    };
    let session = format!("arithmetic-{}", std::process::id());
    let mut total = 0_i64;
    let mut commands = 0_u64;
    let mut authenticated = false;
    let mut failed_once = false;
    let mut startup_bonus = 0_i64;
    let mut parent_completed_form = false;
    let mut configuration = e06_options(&mode);
    let mut settings_sequence = Vec::new();
    let mut reader = BufReader::new(std::io::stdin().lock());
    while let Some(request) = receive(&mut reader)? {
        if request["method"] == "session/cancel" {
            fs::write(root.join("unexpected-cancel"), b"cancel")?;
            continue;
        }
        let Some(id) = request.get("id") else {
            continue;
        };
        if parent_completed_form
            && request["id"] == "bootstrap-form"
            && request.get("method").is_none()
        {
            startup_bonus += form_bonus(&request, &json!("bootstrap-form"), false)?;
            parent_completed_form = false;
            fs::write(
                root.join("scoped-response"),
                b"exact completed parent responder",
            )?;
            continue;
        }
        let result = match request["method"].as_str() {
            Some("initialize") => {
                fs::write(
                    root.join("initialize.json"),
                    serde_json::to_vec(&request["params"])?,
                )?;
                if mode == "hold-initialize" {
                    barrier(&root, "initialize")?;
                }
                if mode == "unsupported-bootstrap" {
                    unsupported(&mut reader, &root, None)?;
                }
                if mode == "s02-init" || mode == "s02-parent-completed" {
                    send_form(
                        json!("bootstrap-form"),
                        json!({"requestId":id}),
                        form_schema(),
                        "Initialize configuration",
                    )?;
                    if mode == "s02-parent-completed" {
                        parent_completed_form = true;
                    } else {
                        let response =
                            receive(&mut reader)?.ok_or("initialize form response missing")?;
                        startup_bonus += form_bonus(&response, &json!("bootstrap-form"), false)?;
                    }
                }
                if [
                    "oversized",
                    "s04-stdout-retainer",
                    "invalid-utf8",
                    "malformed",
                    "partial",
                    "flood",
                ]
                .contains(&mode.as_str())
                {
                    barrier(&root, "hostile")?;
                    let mut stdout = std::io::stdout().lock();
                    match mode.as_str() {
                        "oversized" | "s04-stdout-retainer" => {
                            stdout.write_all(&vec![b'x'; 1024 * 1024 + 1])?
                        }
                        "invalid-utf8" => {
                            stdout.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"\xff\"}\n")?
                        }
                        "malformed" => stdout.write_all(b"not-json\n")?,
                        "partial" => stdout.write_all(b"{\"jsonrpc\":\"2.0\"")?,
                        _ => {
                            // Exceed the reply pipe plus bounded writer queue even
                            // when the consumer keeps up with the ingress burst.
                            for n in 0..4096 {
                                serde_json::to_writer(
                                    &mut stdout,
                                    &json!({"jsonrpc":"2.0","id":n,"method":"fixture/unsupported","params":{"payload":"x".repeat(32768)}}),
                                )?;
                                stdout.write_all(b"\n")?;
                            }
                        }
                    }
                    stdout.flush()?;
                    if mode == "partial" {
                        return Ok(());
                    }
                    drop(stdout);
                    barrier(&root, "hostile-finished")?;
                }
                if mode == "stderr" {
                    std::io::stderr().write_all(&vec![b's'; 256 * 1024])?;
                    fs::write(root.join("stderr-drained"), b"continued")?;
                }
                let methods = if mode.starts_with("auth") {
                    json!([
                        {"id":"agent-login","name":"Agent login","type":"agent"},
                        {"id":"client-login","name":"Never a client login","type":"client"},
                        {"id":"terminal-login","name":"Never a terminal login","type":"terminal"}
                    ])
                } else {
                    json!([])
                };
                json!({"protocolVersion":1,"agentCapabilities":{},"authMethods":methods,
                    "agentInfo":{"name":"synthetic-private-agent-label","version":"1"}})
            }
            Some("authenticate") => {
                if request["params"]["methodId"] != "agent-login" {
                    return Err("non-agent auth reached peer".into());
                }
                fs::write(root.join("authenticate.received"), b"agent only")?;
                if mode == "auth-fail-once" && !failed_once {
                    failed_once = true;
                    emit(
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":"synthetic-private-login-failure"}}),
                    )?;
                    continue;
                }
                if mode == "auth-form" {
                    send_form(
                        json!(41),
                        json!({"requestId":id}),
                        form_schema(),
                        "Non-secret authentication configuration",
                    )?;
                    let response = receive(&mut reader)?.ok_or("auth form response missing")?;
                    startup_bonus += form_bonus(&response, &json!(41), false)?;
                }
                barrier(&root, "authenticate")?;
                authenticated = true;
                json!({})
            }
            Some("session/new") => {
                if mode == "hold-session" {
                    barrier(&root, "session")?;
                }
                if mode.starts_with("auth") && !authenticated {
                    emit(
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":"synthetic-private-auth-error"}}),
                    )?;
                    continue;
                }
                if mode == "s02-session" {
                    send_form(
                        json!("41"),
                        json!({"requestId":id}),
                        form_schema(),
                        "Session configuration",
                    )?;
                    let response = receive(&mut reader)?.ok_or("session form response missing")?;
                    startup_bonus += form_bonus(&response, &json!("41"), false)?;
                }
                if mode.starts_with("e06-") {
                    json!({"sessionId":session,"configOptions":configuration,"modes":{
                        "currentModeId":"default","availableModes":[{"id":"default","name":"Default"},{"id":"plan","name":"Plan"}]}})
                } else {
                    json!({"sessionId":session})
                }
            }
            Some("session/set_config_option" | "session/set_mode") if mode.starts_with("e06-") => {
                if request["params"]["sessionId"] != session {
                    return Err("setting lost original session".into());
                }
                let is_mode = request["method"] == "session/set_mode";
                let (config_id, value) = if is_mode {
                    ("mode", &request["params"]["modeId"])
                } else {
                    (
                        request["params"]["configId"]
                            .as_str()
                            .ok_or("config id required")?,
                        &request["params"]["value"],
                    )
                };
                let entry = if is_mode {
                    json!({"method":"session/set_mode","modeId":value})
                } else {
                    json!({"method":"session/set_config_option","configId":config_id,"value":value})
                };
                settings_sequence.push(entry);
                fs::write(
                    root.join("settings-sequence.json"),
                    serde_json::to_vec(&settings_sequence)?,
                )?;
                if mode == "e06-settings-hold" && settings_sequence.len() == 1 {
                    barrier(&root, "settings")?;
                }
                if mode == "e06-settings-reject" {
                    emit(
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":"fixture rejected setting"}}),
                    )?;
                    continue;
                }
                if mode == "e06-settings-omit" {
                    json!({})
                } else {
                    let option = configuration
                        .as_array_mut()
                        .unwrap()
                        .iter_mut()
                        .find(|option| option["id"] == config_id)
                        .ok_or("unoffered setting reached peer")?;
                    if !option["options"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|choice| choice["value"] == *value)
                    {
                        return Err("unoffered choice reached peer".into());
                    }
                    option["currentValue"] = if mode == "e06-settings-drift" {
                        json!("fixture/other")
                    } else {
                        value.clone()
                    };
                    if mode == "e06-settings-model-dependent" && config_id == "model" {
                        configuration[2]["options"] =
                            json!([{"value":"off","name":"Off"},{"value":"low","name":"Low"}]);
                    }
                    if is_mode {
                        e06_update(
                            &session,
                            json!({"sessionUpdate":"current_mode_update","currentModeId":value}),
                        )?;
                    }
                    e06_update(
                        &session,
                        json!({"sessionUpdate":"config_option_update","configOptions":configuration}),
                    )?;
                    if mode == "e06-settings-null-ack" {
                        Value::Null
                    } else if is_mode {
                        json!({})
                    } else {
                        json!({"configOptions":configuration})
                    }
                }
            }
            Some("session/prompt") if request["params"]["sessionId"] == session => {
                if mode == "error-once" && !failed_once {
                    failed_once = true;
                    emit(
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32603,"message":"synthetic-private-provider-error","data":{"secret":"synthetic-private-error-data"}}}),
                    )?;
                    continue;
                }
                if mode == "eof-prompt" {
                    return Ok(());
                }
                let text = request["params"]["prompt"][0]["text"]
                    .as_str()
                    .ok_or("text prompt required")?;
                let parts: Vec<_> = text.split_whitespace().collect();
                if parts.len() != 3 || parts[0] != "add" {
                    return Err("expected arithmetic command".into());
                }
                if mode == "s02-write-fail" {
                    send_form(
                        json!("write-failed-form"),
                        json!({"sessionId":session}),
                        form_schema(),
                        "Response pipe failure",
                    )?;
                    if unsafe { libc::close(libc::STDIN_FILENO) } != 0 {
                        return Err(std::io::Error::last_os_error().into());
                    }
                    barrier(&root, "closed-input")?;
                    return Ok(());
                }
                if mode == "s02-selected-eof" {
                    send_form(
                        json!("unconfirmed-form"),
                        json!({"sessionId":session}),
                        form_schema(),
                        "Unconfirmed response",
                    )?;
                    barrier(&root, "answer-selected")?;
                    return Ok(());
                }
                if matches!(
                    mode.as_str(),
                    "s02-cancel" | "s02-selected-cancel" | "s02-selected-permission-cancel"
                ) {
                    let selected = match mode.as_str() {
                        "s02-selected-cancel" => "form",
                        "s02-selected-permission-cancel" => "permission",
                        _ => "none",
                    };
                    cancel_interactions(&mut reader, &root, &session, selected)?;
                    emit(json!({"jsonrpc":"2.0","id":id,"result":{"stopReason":"cancelled"}}))?;
                    continue;
                }
                if mode == "s02-content-capacity" || mode == "s02-count-capacity" {
                    let count = if mode == "s02-count-capacity" { 65 } else { 8 };
                    let message = if mode == "s02-count-capacity" {
                        "pending".into()
                    } else {
                        "x".repeat(600 * 1024)
                    };
                    for index in 0..count {
                        send_form(
                            json!(index),
                            json!({"sessionId":session}),
                            form_schema(),
                            &message,
                        )?;
                        barrier(&root, &format!("content-{index}"))?;
                    }
                    return Err("interaction capacity was not enforced".into());
                }
                let mut permitted = true;
                if mode == "s02-invalid-scopes" {
                    for (index, scope) in [
                        json!({"sessionId":format!("{session}-foreign")}),
                        json!({"requestId":99999}),
                        json!({"sessionId":session,"requestId":id}),
                        json!({"sessionId":session,"toolCallId":42}),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        send_form(
                            json!(format!("invalid-scope-{index}")),
                            scope,
                            form_schema(),
                            "Invalid scope",
                        )?;
                        let response =
                            receive(&mut reader)?.ok_or("invalid scope response missing")?;
                        if response["id"] != format!("invalid-scope-{index}")
                            || response["error"]["code"] != -32602
                        {
                            return Err(
                                "invalid scope was guessed into a session or request".into()
                            );
                        }
                    }
                    fs::write(
                        root.join("invalid-scopes-rejected"),
                        b"exact invalid params",
                    )?;
                }
                if mode == "s02-history" {
                    send_form(
                        json!("live-anchor"),
                        json!({"sessionId":session}),
                        form_schema(),
                        "Live responder never evicted",
                    )?;
                    for index in 0..8 {
                        let form_id = json!(format!("completed-content-{index}"));
                        send_form(
                            form_id.clone(),
                            json!({"sessionId":session}),
                            form_schema(),
                            &"h".repeat(600 * 1024),
                        )?;
                        let response = receive(&mut reader)?.ok_or("history response missing")?;
                        startup_bonus += form_bonus(&response, &form_id, false)?;
                        barrier(&root, &format!("history-{index}"))?;
                    }
                    barrier(&root, "history-answers")?;
                    let response = receive(&mut reader)?.ok_or("live anchor response missing")?;
                    startup_bonus += form_bonus(&response, &json!("live-anchor"), false)?;
                }
                if mode == "s04-grok-capacity" && commands == 0 {
                    for index in 0..5 {
                        send_form(
                            json!(format!("capacity-{index}")),
                            json!({"sessionId":session}),
                            json!({"type":"object","description":"x".repeat(832_000),
                                "properties":{"value":{"type":"string"}},"required":["value"]}),
                            "Retained capacity reservation",
                        )?;
                        barrier(&root, &format!("capacity-{index}"))?;
                    }
                }
                let bonus = if matches!(mode.as_str(), "e06-omp" | "e06-omp-cancel") {
                    let tool_id = "opaque".repeat(80);
                    e06_update(
                        &session,
                        json!({"sessionUpdate":"tool_call","toolCallId":tool_id,
                        "title":"Long opaque OMP tool","kind":"other","status":"in_progress"}),
                    )?;
                    e06_update(
                        &session,
                        json!({"sessionUpdate":"tool_call_update","toolCallId":tool_id,
                        "status":"in_progress"}),
                    )?;
                    send_form(
                        json!("omp-question"),
                        json!({"sessionId":session,"toolCallId":tool_id}),
                        json!({"type":"object","properties":{
                            "q0":{"type":"string","title":"Continue?","oneOf":[
                                {"const":"Proceed","title":"Proceed"},{"const":"Stop","title":"Stop"}],"default":"Proceed"},
                            "q0__other":{"type":"string","title":"Other"}}}),
                        "Continue?",
                    )?;
                    let response = receive(&mut reader)?.ok_or("OMP-shaped answer missing")?;
                    if response["id"] != "omp-question"
                        || response["result"]["action"] != "accept"
                        || response["result"]["content"]["q0"] != "Proceed"
                    {
                        return Err("OMP-shaped form lost exact answer".into());
                    }
                    emit(
                        json!({"jsonrpc":"2.0","id":91,"method":"session/request_permission","params":{
                        "sessionId":session,"toolCall":{"toolCallId":tool_id,"title":"Confined effect","kind":"execute","status":"pending"},
                        "options":[{"optionId":"allow_once","name":"Allow once","kind":"allow_once"},
                            {"optionId":"reject_once","name":"Reject","kind":"reject_once"}]}}),
                    )?;
                    if mode == "e06-omp-cancel" {
                        barrier(&root, "omp-permission")?;
                        emit(
                            json!({"jsonrpc":"2.0","method":"$/cancel_request","params":{"requestId":91}}),
                        )?;
                    }
                    let response = receive(&mut reader)?.ok_or("OMP-shaped permission missing")?;
                    if mode == "e06-omp-cancel" {
                        if response["id"] != 91 || response["error"]["code"] != -32800 {
                            return Err("OMP cancellation lost exact responder".into());
                        }
                        permitted = false;
                    } else {
                        if response["id"] != 91
                            || response["result"]["outcome"]["outcome"] != "selected"
                        {
                            return Err("OMP-shaped permission lost exact responder".into());
                        }
                        permitted = match response["result"]["outcome"]["optionId"].as_str() {
                            Some("allow_once") => true,
                            Some("reject_once") => false,
                            _ => return Err("fabricated OMP option".into()),
                        };
                    }
                    fs::write(
                        root.join("omp-permission.json"),
                        serde_json::to_vec(&json!({"allowed":permitted}))?,
                    )?;
                    configuration[0]["currentValue"] = json!("plan");
                    configuration[2]["currentValue"] = json!("high");
                    e06_update(
                        &session,
                        json!({"sessionUpdate":"current_mode_update","currentModeId":"plan"}),
                    )?;
                    e06_update(
                        &session,
                        json!({"sessionUpdate":"config_option_update","configOptions":configuration}),
                    )?;
                    0
                } else if mode == "s04-complete" {
                    send_form(
                        json!("reusable-form"),
                        json!({"sessionId":session}),
                        form_schema(),
                        "Synthetic private arithmetic question",
                    )?;
                    let response = receive(&mut reader)?.ok_or("complete form response missing")?;
                    let bonus = form_bonus(&response, &json!("reusable-form"), false)?;
                    send_permission(json!(73), &session)?;
                    let response =
                        receive(&mut reader)?.ok_or("complete permission response missing")?;
                    permitted = permission_allows(&response, &json!(73))?;
                    bonus
                } else if mode == "s04-grok-large" && commands == 0 {
                    emit(
                        json!({"jsonrpc":"2.0","id":"grok-large","method":"_x.ai/ask_user_question","params":{
                        "sessionId":session,"toolCallId":"grok-large-tool","mode":"default",
                        "questions":[{"question":"Q".repeat(600_000),"multiSelect":false,
                            "options":[{"label":"Zero","description":"No adjustment"}]}]}}),
                    )?;
                    let response = receive(&mut reader)?.ok_or("large Grok response missing")?;
                    if response["id"] != "grok-large" || response["error"]["code"] != -32602 {
                        return Err(
                            "unrepresentable Grok question was not rejected explicitly".into()
                        );
                    }
                    fs::write(
                        root.join("grok-large-rejected.json"),
                        serde_json::to_vec(&response)?,
                    )?;
                    0
                } else if matches!(
                    mode.as_str(),
                    "s04-grok-other" | "s04-grok-large" | "s04-grok-capacity"
                ) {
                    emit(
                        json!({"jsonrpc":"2.0","id":"grok-other","method":"_x.ai/ask_user_question","params":{
                        "sessionId":session,"toolCallId":"grok-other-tool","mode":"default",
                        "questions":[{"question":"Arithmetic adjustment","multiSelect":null,
                            "options":[{"label":"Zero","description":"No adjustment"}]}]}}),
                    )?;
                    let response = receive(&mut reader)?.ok_or("Grok original response missing")?;
                    if response["id"] != "grok-other" || response.get("error").is_some() {
                        return Err("Grok question lost original responder".into());
                    }
                    fs::write(
                        root.join(format!("grok-response-{}.json", commands + 1)),
                        serde_json::to_vec(&response)?,
                    )?;
                    match response["result"]["outcome"].as_str() {
                        Some("accepted") => {
                            if response["result"]["answers"]["Arithmetic adjustment"]
                                != json!(["Other"])
                            {
                                return Err("Grok Other answer was not preserved".into());
                            }
                            response["result"]["annotations"]["Arithmetic adjustment"]["notes"]
                                .as_str()
                                .ok_or("Grok Other notes missing")?
                                .parse::<i64>()?
                        }
                        Some("cancelled") => -20,
                        _ => return Err("Grok question outcome invalid".into()),
                    }
                } else if mode == "form-prompt" {
                    emit(
                        json!({"jsonrpc":"2.0","id":"bonus-form","method":"elicitation/create",
                        "params":{"mode":"form","sessionId":session,"message":"Choose arithmetic adjustment",
                        "requestedSchema":{"type":"object","properties":{
                            "bonus":{"type":"integer","title":"Adjustment","minimum":-100,"maximum":100,"default":0}
                        },"required":["bonus"]}}}),
                    )?;
                    let answer = receive(&mut reader)?.ok_or("form answer missing")?;
                    if !answer["error"].is_null() {
                        fs::write(root.join("form-rejected"), b"explicit protocol error")?;
                        return Err("form was rejected instead of answered".into());
                    }
                    if answer["id"] != "bonus-form" || answer["result"]["action"] != "accept" {
                        return Err("wrong original form response".into());
                    }
                    answer["result"]["content"]["bonus"]
                        .as_i64()
                        .ok_or("typed integer adjustment required")?
                } else if matches!(
                    mode.as_str(),
                    "s02-form"
                        | "s02-request-prompt"
                        | "s02-invalid-scopes"
                        | "s02-rich"
                        | "s02-decline"
                        | "s02-form-cancel"
                        | "s02-invalid-schema"
                ) {
                    let schema = if root.join("schema.json").exists() {
                        serde_json::from_slice(&fs::read(root.join("schema.json"))?)?
                    } else if mode == "s02-rich" {
                        rich_schema()
                    } else {
                        form_schema()
                    };
                    let scope = if mode == "s02-request-prompt" {
                        json!({"requestId":id})
                    } else {
                        json!({"sessionId":session})
                    };
                    send_form(
                        json!("reusable-form"),
                        scope,
                        schema,
                        "Synthetic private arithmetic question",
                    )?;
                    let response = receive(&mut reader)?.ok_or("form response missing")?;
                    if mode == "s02-invalid-schema" {
                        if response["id"] != "reusable-form" || response["error"]["code"] != -32602
                        {
                            return Err("unsupported schema not explicitly rejected".into());
                        }
                        fs::write(root.join("invalid-schema-rejected"), b"invalid_params")?;
                        0
                    } else {
                        form_bonus(&response, &json!("reusable-form"), mode == "s02-rich")?
                    }
                } else if mode == "s02-permission" {
                    send_permission(json!(73), &session)?;
                    let response = receive(&mut reader)?.ok_or("permission response missing")?;
                    permitted = permission_allows(&response, &json!(73))?;
                    if permitted {
                        fs::write(root.join("tool-effect"), b"authorized tool ran")?;
                    }
                    0
                } else {
                    std::mem::take(&mut startup_bonus)
                };
                if permitted {
                    total += parts[1].parse::<i64>()? + parts[2].parse::<i64>()? + bonus;
                    if mode == "s04-complete" {
                        fs::write(
                            root.join(format!("tool-effect-{}", commands + 1)),
                            b"authorized tool ran",
                        )?;
                    }
                }
                commands += 1;
                fs::write(
                    root.join("effect.tmp"),
                    serde_json::to_vec(&json!({
                        "session_id":session,"value":total,"commands":commands,"rpc_id":id
                    }))?,
                )?;
                fs::rename(root.join("effect.tmp"), root.join("effect.json"))?;
                stream(
                    &session,
                    "agent_message_chunk",
                    &format!("Total={total}; commands={commands}"),
                )?;
                if mode == "s03-rendering" {
                    s03_rendering(&mut reader, &root, &session)?;
                }
                if mode.starts_with("s03-interrupt-") && commands == 1 {
                    fs::write(root.join("prompt.received"), b"pending")?;
                    let cancel = receive(&mut reader)?.ok_or("cancel notification missing")?;
                    if cancel.get("id").is_some()
                        || cancel["method"] != "session/cancel"
                        || cancel["params"]["sessionId"] != session
                    {
                        return Err("interrupt lost exact session notification".into());
                    }
                    barrier(&root, "s03-cancel")?;
                    match mode.as_str() {
                        "s03-interrupt-eof" => return Ok(()),
                        "s03-interrupt-error" => {
                            emit(json!({"jsonrpc":"2.0","id":id,"error":{
                                "code":-32603,"message":"s03-private-interrupt-error"}}))?;
                        }
                        "s03-interrupt-end-turn" => {
                            emit(
                                json!({"jsonrpc":"2.0","id":id,"result":{"stopReason":"end_turn"}}),
                            )?;
                        }
                        "s03-interrupt-cancelled" => {
                            emit(
                                json!({"jsonrpc":"2.0","id":id,"result":{"stopReason":"cancelled"}}),
                            )?;
                        }
                        _ => return Err("unknown interrupt outcome mode".into()),
                    }
                    continue;
                }
                if mode.starts_with("s03-interrupt-") && commands > 1 {
                    barrier(&root, "s03-next")?;
                }
                if matches!(
                    mode.as_str(),
                    "interrupt" | "interrupt-malformed" | "interrupt-wrong-session"
                ) && commands == 1
                {
                    fs::write(root.join("prompt.received"), b"pending")?;
                    let cancel = receive(&mut reader)?.ok_or("cancel notification missing")?;
                    if cancel.get("id").is_some()
                        || cancel["method"] != "session/cancel"
                        || cancel["params"]["sessionId"] != session
                    {
                        return Err(
                            "interrupt must be exact-session notification without acknowledgement"
                                .into(),
                        );
                    }
                    fs::write(root.join("cancel.received"), b"explicit interrupt")?;
                    let invalid = match mode.as_str() {
                        "interrupt-malformed" => Some(json!({
                            "sessionId":session,"toolCall":null,"options":[]
                        })),
                        "interrupt-wrong-session" => Some(json!({
                            "sessionId":format!("{session}-stale"),
                            "toolCall":{"toolCallId":"foreign-tool"},
                            "options":[{"optionId":"allow","name":"Allow once","kind":"allow_once"}]
                        })),
                        _ => None,
                    };
                    if let Some(params) = invalid {
                        emit(json!({"jsonrpc":"2.0","id":"invalid-permission",
                            "method":"session/request_permission","params":params}))?;
                        let response =
                            receive(&mut reader)?.ok_or("invalid permission response missing")?;
                        if response["id"] != "invalid-permission"
                            || response["error"]["code"] != -32602
                        {
                            return Err("invalid permission did not receive invalid_params".into());
                        }
                        fs::write(root.join("invalid-permission-rejected"), b"invalid_params")?;
                    }
                    emit(
                        json!({"jsonrpc":"2.0","id":"late-permission","method":"session/request_permission",
                        "params":{"sessionId":session,"toolCall":{"toolCallId":"late-tool"},
                        "options":[{"optionId":"allow","name":"Allow once","kind":"allow_once"}]}}),
                    )?;
                    let response =
                        receive(&mut reader)?.ok_or("late permission cancellation missing")?;
                    if response["id"] != "late-permission"
                        || response["result"]["outcome"]["outcome"] != "cancelled"
                    {
                        return Err("late permission not explicitly cancelled".into());
                    }
                    emit(json!({"jsonrpc":"2.0","id":id,"result":{"stopReason":"cancelled"}}))?;
                    continue;
                }
                if mode == "hold-prompt" {
                    stream(&session, "agent_thought_chunk", "synthetic-private-thought")?;
                    emit(
                        json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":session,
                        "update":{"sessionUpdate":"tool_call","toolCallId":"arithmetic-tool","title":"synthetic-private-tool","kind":"other","status":"completed"}}}),
                    )?;
                    emit(
                        json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":session,
                        "update":{"sessionUpdate":"plan","entries":[{"content":"synthetic-private-plan","priority":"medium","status":"completed"}]}}}),
                    )?;
                    barrier(&root, "prompt")?;
                }
                if mode == "unsupported-prompt" {
                    unsupported(&mut reader, &root, Some(&session))?;
                }
                if mode == "history" {
                    for wave in 0..140 {
                        for n in 0..16 {
                            stream(
                                &session,
                                "agent_message_chunk",
                                &format!("history-{wave}-{n}:{}", "x".repeat(2048)),
                            )?;
                        }
                        barrier(&root, &format!("wave-{wave}"))?;
                    }
                }
                if mode == "stalled-writer" {
                    // Stop reading stdin while generating enough responder work
                    // to block the dedicated writer and exhaust a bounded queue.
                    for n in 0..4096 {
                        emit(
                            json!({"jsonrpc":"2.0","id":format!("unread-{n}"),"method":"fixture/unread","params":{}}),
                        )?;
                    }
                    barrier(&root, "stdin")?;
                }
                json!({"stopReason":"end_turn"})
            }
            _ => {
                emit(
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Unsupported fixture method"}}),
                )?;
                continue;
            }
        };
        emit(json!({"jsonrpc":"2.0","id":id,"result":result}))?;
        if mode == "s03-stalled-prompt" && request["method"] == "session/new" {
            // Publish the real session, then stop before any prompt bytes are read.
            barrier(&root, "s03-reads")?;
        } else if matches!(
            mode.as_str(),
            "s04-stalled-descendants" | "s04-retained-pipes"
        ) && request["method"] == "session/new"
        {
            barrier(&root, "s04-reads")?;
        }
    }
    Ok(())
}

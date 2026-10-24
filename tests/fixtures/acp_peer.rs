//! Deterministic external ACP peer: real stdio and stateful effects, no model/network.
use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
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
            json!({"mode":"form","message":"synthetic form","requestedSchema":{"type":"object","properties":{}}}),
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
    let _descendant = if mode == "descendants" {
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
    let mut reader = BufReader::new(std::io::stdin().lock());
    while let Some(request) = receive(&mut reader)? {
        if request["method"] == "session/cancel" {
            fs::write(root.join("unexpected-cancel"), b"cancel")?;
            continue;
        }
        let Some(id) = request.get("id") else {
            continue;
        };
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
                if ["oversized", "invalid-utf8", "malformed", "partial", "flood"]
                    .contains(&mode.as_str())
                {
                    barrier(&root, "hostile")?;
                    let mut stdout = std::io::stdout().lock();
                    match mode.as_str() {
                        "oversized" => stdout.write_all(&vec![b'x'; 1024 * 1024 + 1])?,
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
                json!({"sessionId":session})
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
                total += parts[1].parse::<i64>()? + parts[2].parse::<i64>()?;
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
    }
    Ok(())
}

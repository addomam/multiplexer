//! Deterministic external ACP peer: real stdio and stateful effects, no model/network.
use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

fn emit(message: Value) -> std::io::Result<()> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &message)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args_os().nth(1).ok_or("private fixture root required")?);
    fs::write(root.join("peer.pid"), std::process::id().to_string())?;
    let session = format!("arithmetic-{}", std::process::id());
    let mut total = 0_i64;
    let mut commands = 0_u64;
    for line in BufReader::new(std::io::stdin().lock()).lines() {
        let request: Value = serde_json::from_str(&line?)?;
        let Some(id) = request.get("id") else { continue };
        let result = match request["method"].as_str() {
            Some("initialize") => json!({"protocolVersion":1,"agentCapabilities":{},"authMethods":[],
                "agentInfo":{"name":"deterministic-arithmetic-peer","version":"1"}}),
            Some("session/new") => json!({"sessionId":session}),
            Some("session/prompt") if request["params"]["sessionId"] == session => {
                let text = request["params"]["prompt"][0]["text"].as_str().ok_or("text prompt required")?;
                let parts: Vec<_> = text.split_whitespace().collect();
                if parts.len() != 3 || parts[0] != "add" {
                    return Err("expected arithmetic command".into());
                }
                total += parts[1].parse::<i64>()? + parts[2].parse::<i64>()?;
                commands += 1;
                fs::write(root.join("effect.tmp"), serde_json::to_vec(&json!({
                    "session_id":session,"value":total,"commands":commands
                }))?)?;
                fs::rename(root.join("effect.tmp"), root.join("effect.json"))?;
                emit(json!({"jsonrpc":"2.0","method":"session/update","params":{
                    "sessionId":session,"update":{"sessionUpdate":"agent_message_chunk",
                    "content":{"type":"text","text":format!("Total={total}; commands={commands}")}}
                }}))?;
                json!({"stopReason":"end_turn"})
            }
            _ => {
                emit(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Unsupported fixture method"}}))?;
                continue;
            }
        };
        emit(json!({"jsonrpc":"2.0","id":id,"result":result}))?;
    }
    Ok(())
}

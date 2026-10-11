use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "amux",
    version,
    about = "Project-oriented native terminal environment"
)]
struct Args {
    #[arg(long, global = true)]
    socket: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Attach the native terminal surface.
    Attach {
        terminal: Option<String>,
    },
    /// Run the local daemon in the foreground.
    Daemon {
        #[arg(long)]
        projects_dir: Option<PathBuf>,
    },
    /// Submit any version-1 API request as JSON.
    Api {
        request: String,
    },
    Snapshot,
    Open {
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(long)]
        command: Option<String>,
        #[arg(long)]
        omp: bool,
        #[arg(long)]
        recipe: Option<String>,
    },
    Name {
        terminal: String,
        name: String,
    },
    Close {
        terminal: String,
    },
    Remove {
        terminal: String,
    },
    Reparent {
        terminal: String,
        parent: Option<String>,
    },
    Promote {
        terminal: String,
    },
    Demote {
        terminal: String,
        parent: String,
    },
    Input {
        terminal: String,
        text: Option<String>,
        #[arg(long)]
        key: Option<String>,
        #[arg(long)]
        paste: bool,
        #[arg(long)]
        bytes: Option<String>,
    },
    Execute {
        terminal: String,
        source: String,
        #[arg(long)]
        queue: bool,
    },
    Screen {
        terminal: String,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long)]
        limit: Option<usize>,
    },
    Resize {
        terminal: String,
        cols: u16,
        rows: u16,
    },
    State {
        terminal: String,
    },
    Events {
        #[arg(long, default_value_t = 0)]
        after: u64,
        #[arg(long, default_value_t = 30000)]
        timeout_ms: u64,
    },
    Wait {
        terminal: String,
        #[arg(value_parser = ["shell-ready", "agent-settled", "agent-working", "needs-input", "exited"])]
        condition: String,
        #[arg(long)]
        after: Option<u64>,
        #[arg(long, default_value_t = 30000)]
        timeout_ms: u64,
    },
    ProjectSave {
        name: String,
    },
    ProjectNew {
        name: String,
        #[arg(long)]
        directory: Option<String>,
        /// Initial hooks and recipes as JSON, installed before first open.
        #[arg(long)]
        config: Option<String>,
    },
    ProjectOpen {
        project: String,
    },
    /// Patch project hooks, recipes, and configuration with JSON.
    ProjectConfig {
        project: String,
        config: String,
    },
    /// Patch global configuration (prefix, projects directory, hooks) with JSON.
    Config {
        config: String,
    },
    Report {
        terminal: String,
        event: String,
    },
    Shutdown,
    Detach,
    #[command(hide = true)]
    Internal {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(hide = true)]
    InternalReport {
        event: String,
    },
}

fn request_for(command: Command) -> Result<Value> {
    let mut value = match command {
        Command::Api { request } => {
            serde_json::from_str::<Value>(&request).context("invalid API JSON")?
        }
        Command::Snapshot => json!({"op":"snapshot"}),
        Command::Open {
            source,
            project,
            name,
            cwd,
            command,
            omp,
            recipe,
        } => {
            json!({"op":"open","source":source,"project":project,"name":name,"cwd":cwd,"command":command,"omp":omp,"recipe":recipe})
        }
        Command::Name { terminal, name } => json!({"op":"name","terminal":terminal,"name":name}),
        Command::Close { terminal } => json!({"op":"close","terminal":terminal}),
        Command::Remove { terminal } => json!({"op":"remove","terminal":terminal}),
        Command::Reparent { terminal, parent } => {
            json!({"op":"reparent","terminal":terminal,"parent":parent})
        }
        Command::Promote { terminal } => json!({"op":"promote","terminal":terminal}),
        Command::Demote { terminal, parent } => {
            json!({"op":"demote","terminal":terminal,"parent":parent})
        }
        Command::Input {
            terminal,
            text,
            key,
            paste,
            bytes,
        } => {
            let bytes = bytes
                .map(|s| {
                    s.split(',')
                        .map(|n| n.trim().parse::<u8>())
                        .collect::<std::result::Result<Vec<_>, _>>()
                })
                .transpose()
                .context("bytes must be comma-separated values in 0..255")?;
            json!({"op":"input","terminal":terminal,"text":text,"key":key,"paste":paste,"bytes":bytes})
        }
        Command::Execute {
            terminal,
            source,
            queue,
        } => json!({"op":"execute","terminal":terminal,"source":source,"queue":queue}),
        Command::Screen {
            terminal,
            offset,
            limit,
        } => json!({"op":"screen","terminal":terminal,"offset":offset,"limit":limit}),
        Command::Resize {
            terminal,
            cols,
            rows,
        } => json!({"op":"resize","terminal":terminal,"cols":cols,"rows":rows}),
        Command::State { terminal } => json!({"op":"state","terminal":terminal}),
        Command::Events { after, timeout_ms } => {
            json!({"op":"events","after":after,"timeout_ms":timeout_ms})
        }
        Command::Wait {
            terminal,
            condition,
            after,
            timeout_ms,
        } => {
            json!({"op":"wait","terminal":terminal,"condition":condition,"after":after,"timeout_ms":timeout_ms})
        }
        Command::ProjectSave { name } => json!({"op":"project-save","name":name}),
        Command::ProjectNew {
            name,
            directory,
            config,
        } => {
            json!({"op":"project-new","name":name,"directory":directory,"config":config.map(|config|serde_json::from_str::<Value>(&config)).transpose().context("invalid initial project configuration JSON")?.unwrap_or_else(||json!({}))})
        }
        Command::ProjectOpen { project } => json!({"op":"project-open","project":project}),
        Command::ProjectConfig { project, config } => {
            json!({"op":"project-config","project":project,"config":serde_json::from_str::<Value>(&config).context("invalid project configuration JSON")?})
        }
        Command::Config { config } => {
            json!({"op":"config","config":serde_json::from_str::<Value>(&config).context("invalid configuration JSON")?})
        }
        Command::Report { terminal, event } => {
            json!({"op":"report","terminal":terminal,"event":serde_json::from_str::<Value>(&event).context("invalid event JSON")?})
        }
        Command::Shutdown => json!({"op":"shutdown"}),
        Command::Detach => json!({"op":"detach"}),
        _ => anyhow::bail!("command is not an API operation"),
    };
    let object = value
        .as_object_mut()
        .context("API request must be a JSON object")?;
    object.entry("version").or_insert(json!(1));
    // Absent optional flags are omitted, while reparent's null explicitly means root.
    object.retain(|key, value| !value.is_null() || key == "parent");
    Ok(value)
}

pub fn run() -> Result<()> {
    let args = Args::parse();
    let socket = args.socket.unwrap_or_else(crate::client::socket_path);
    match args.command {
        None => {
            crate::client::ensure_daemon(&socket)?;
            crate::tui::run(&socket, None)
        }
        Some(Command::Attach { terminal }) => {
            crate::client::ensure_daemon(&socket)?;
            crate::tui::run(&socket, terminal)
        }
        Some(Command::Daemon { projects_dir }) => crate::daemon::run(&socket, projects_dir),
        Some(Command::Internal { args }) => crate::integration::helper(&args),
        Some(Command::InternalReport { event }) => {
            crate::integration::helper(&["report".into(), event])
        }
        Some(command) => {
            let result =
                request_for(command).and_then(|request| crate::client::request(&socket, request));
            match result {
                Ok(result) => {
                    println!("{}", json!({"version":1,"ok":true,"result":result}));
                    Ok(())
                }
                Err(error) => {
                    let message = format!("{error:#}");
                    let (code, detail) = message
                        .split_once(": ")
                        .filter(|(code, _)| {
                            !code.is_empty()
                                && code.bytes().all(|c| c.is_ascii_lowercase() || c == b'-')
                        })
                        .unwrap_or(("request-failed", message.as_str()));
                    println!(
                        "{}",
                        json!({"version":1,"ok":false,"error":{"code":code,"message":detail}})
                    );
                    std::process::exit(1)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reparent_preserves_explicit_root_and_open_omits_missing_fields() {
        assert_eq!(
            request_for(Command::Reparent {
                terminal: "t1".into(),
                parent: None
            })
            .unwrap()["parent"],
            Value::Null
        );
        let open = request_for(Command::Open {
            source: None,
            project: None,
            name: None,
            cwd: None,
            command: None,
            omp: false,
            recipe: None,
        })
        .unwrap();
        assert!(open.get("source").is_none());
        assert_eq!(open["version"], 1);
    }
    #[test]
    fn bytes_are_validated_before_sending() {
        let command = Command::Input {
            terminal: "t1".into(),
            text: None,
            key: None,
            paste: false,
            bytes: Some("27,91,65".into()),
        };
        assert_eq!(request_for(command).unwrap()["bytes"], json!([27, 91, 65]));
        assert!(
            request_for(Command::Input {
                terminal: "t1".into(),
                text: None,
                key: None,
                paste: false,
                bytes: Some("256".into())
            })
            .is_err()
        );
    }
}

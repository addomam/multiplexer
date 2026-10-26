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
    /// Open, inspect or attach an amux-owned ACP conversation.
    Acp {
        #[command(subcommand)]
        command: AcpCommand,
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
        /// Serialized current ControlHandle JSON; never a controller credential.
        #[arg(long)]
        control: String,
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
        /// Serialized current ControlHandle JSON; never implicitly acquired.
        #[arg(long)]
        control: String,
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

#[derive(clap::Args)]
struct AcpStart {
    /// ACP profile: omp, codex, grok, or a configured profile.
    profile: String,
    /// Exact project ID or unambiguous project name.
    #[arg(long)]
    project: String,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    cwd: Option<String>,
    #[arg(long)]
    model: Option<String>,
    #[arg(long)]
    mode: Option<String>,
    #[arg(long)]
    effort: Option<String>,
    /// Advertised config option ID=choice ID; repeat for distinct options.
    #[arg(long = "option", value_name = "ID=VALUE")]
    options: Vec<String>,
    /// JSON object of exact option ID:choice ID pairs, including delimiter-containing IDs.
    #[arg(long, value_name = "JSON")]
    options_json: Option<String>,
    /// Explicit external controller JSON; returns JSON instead of attaching.
    #[arg(long)]
    controller: Option<String>,
}

impl AcpStart {
    fn request(self) -> Result<Value> {
        let mut settings = serde_json::Map::new();
        for (key, value) in [
            ("model", self.model),
            ("mode", self.mode),
            ("effort", self.effort),
        ] {
            if let Some(value) = value {
                settings.insert(key.into(), Value::String(value));
            }
        }
        let mut options = self
            .options_json
            .as_deref()
            .map(crate::acp_profiles::parse_options_json)
            .transpose()?
            .unwrap_or_default();
        for option in self.options {
            let (id, value) = option
                .split_once('=')
                .context("--option requires ID=VALUE")?;
            anyhow::ensure!(
                !id.is_empty() && !value.is_empty(),
                "--option requires nonempty ID=VALUE"
            );
            anyhow::ensure!(!options.contains_key(id), "duplicate --option ID");
            options.insert(id.into(), Value::String(value.into()));
        }
        if !options.is_empty() {
            settings.insert("options".into(), Value::Object(options));
        }
        let controller = self
            .controller
            .map(|text| serde_json::from_str::<Value>(&text))
            .transpose()
            .context("invalid controller JSON")?;
        Ok(
            json!({"op":"acp-open","agent":self.profile,"project":self.project,
            "name":self.name,"cwd":self.cwd,"settings":settings,"controller":controller}),
        )
    }
}

#[derive(Subcommand)]
enum AcpCommand {
    /// Launch a profile and attach a human controller to that same instance.
    Start(AcpStart),
    /// List configured and built-in profiles without exposing adapter argv.
    Profiles,
    /// Start one owned stdio adapter; command argv follows --.
    Open {
        #[arg(long)]
        project: String,
        #[arg(long)]
        controller: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(last = true, required = true, num_args = 1.., allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Inspect an exact target tuple; content is explicit opt-in.
    Inspect {
        target: String,
        #[arg(long)]
        content: bool,
        #[arg(long)]
        after: Option<u64>,
    },
    /// Attach a human view to the same live ACP instance.
    Attach { view: String },
}

fn request_for(command: Command) -> Result<Value> {
    let mut value = match command {
        Command::Api { request } => {
            serde_json::from_str::<Value>(&request).context("invalid API JSON")?
        }
        Command::Acp {
            command: AcpCommand::Start(start),
        } => start.request()?,
        Command::Acp {
            command: AcpCommand::Profiles,
        } => json!({"op":"acp-profiles"}),
        Command::Acp {
            command:
                AcpCommand::Open {
                    project,
                    controller,
                    name,
                    cwd,
                    command,
                },
        } => {
            let controller = serde_json::from_str::<Value>(&controller)
                .context("invalid controller handle JSON")?;
            json!({"op":"acp-open","project":project,"controller":controller,
                "name":name,"cwd":cwd,"command":command})
        }
        Command::Acp {
            command:
                AcpCommand::Inspect {
                    target,
                    content,
                    after,
                },
        } => {
            let target =
                serde_json::from_str::<Value>(&target).context("invalid ACP target JSON")?;
            json!({"op":"acp-inspect","target":target,"include_content":content,"after":after})
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
            control,
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
            let control: Value = serde_json::from_str(&control).context("invalid control JSON")?;
            json!({"op":"input","terminal":terminal,"control":control,"text":text,"key":key,"paste":paste,"bytes":bytes})
        }
        Command::Execute {
            terminal,
            control,
            source,
            queue,
        } => {
            let control: Value = serde_json::from_str(&control).context("invalid control JSON")?;
            json!({"op":"execute","terminal":terminal,"control":control,"source":source,"queue":queue})
        }
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
        Some(Command::Acp {
            command: AcpCommand::Start(start),
        }) if start.controller.is_none() => {
            crate::client::ensure_daemon(&socket)?;
            let launch = request_for(Command::Acp {
                command: AcpCommand::Start(start),
            })?;
            crate::tui::start(&socket, launch)
        }
        Some(Command::Acp {
            command: AcpCommand::Attach { view },
        }) => {
            crate::client::ensure_daemon(&socket)?;
            let snapshot = crate::client::request(&socket, json!({"version":1,"op":"snapshot"}))?;
            anyhow::ensure!(
                snapshot["agent_views"]
                    .as_array()
                    .is_some_and(|views| views.iter().any(|agent| agent["id"] == view)),
                "wrong-target-kind: exact ACP view ID required"
            );
            crate::tui::run(&socket, Some(view))
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
                    let (code, detail) = error
                        .downcast_ref::<crate::client::DaemonError>()
                        .map(|error| (error.code.as_str(), error.message.as_str()))
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
            control: r#"{"kind":"control","ref":"fixture"}"#.into(),
            text: None,
            key: None,
            paste: false,
            bytes: Some("27,91,65".into()),
        };
        assert_eq!(request_for(command).unwrap()["bytes"], json!([27, 91, 65]));
        assert!(
            request_for(Command::Input {
                terminal: "t1".into(),
                control: r#"{"kind":"control","ref":"fixture"}"#.into(),
                text: None,
                key: None,
                paste: false,
                bytes: Some("256".into())
            })
            .is_err()
        );
    }

    #[test]
    fn mutations_require_serialized_control_and_preserve_it() {
        for args in [
            vec!["amux", "input", "t1", "hello"],
            vec!["amux", "execute", "t1", "print hello"],
        ] {
            assert!(Args::try_parse_from(args).is_err());
        }
        let control = r#"{"kind":"control","ref":"private-fixture"}"#;
        for args in [
            vec!["amux", "input", "t1", "--control", control, "hello"],
            vec!["amux", "execute", "t1", "--control", control, "print hello"],
        ] {
            let parsed = Args::try_parse_from(args).unwrap();
            let request = request_for(parsed.command.unwrap()).unwrap();
            assert_eq!(
                request["control"],
                json!({"kind":"control","ref":"private-fixture"})
            );
        }
        let view = request_for(Command::Api {
            request: r#"{"op":"control-handle","handle":{"kind":"controller","ref":"view"},"method":"view","view":{"serial":1,"terminal":null,"focused":null}}"#.into(),
        }).unwrap();
        assert!(view["view"]["terminal"].is_null());
        assert!(view["view"]["focused"].is_null());
        assert!(view["view"].get("terminal").is_some());
        assert!(view["view"].get("focused").is_some());
    }
}

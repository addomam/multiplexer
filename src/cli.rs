use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "amux",
    version,
    about = "Project-oriented native terminals and same-instance ACP agents",
    after_help = "Run without a command to start or attach the native UI. Most other commands\nrequire a running daemon and print version-1 JSON; UI attachments and the foreground\ndaemon are exceptions. Installing a new binary does not upgrade a running daemon.\n\nExamples:\n  amux\n  amux projects --help\n  amux acp --help\n  amux snapshot"
)]
struct Args {
    /// Private daemon socket path (otherwise AMUX_SOCKET or the default runtime path).
    #[arg(long, global = true)]
    socket: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Attach the native UI without restarting existing managed work.
    #[command(
        after_help = "Examples:\n  amux attach\n  amux attach TERMINAL_ID\n\nAn attachment may start a daemon if none exists, but never restores old processes.\nReattachment does not implicitly take control from another owner."
    )]
    Attach {
        /// Terminal ID or unambiguous name; omit to select an existing surface.
        terminal: Option<String>,
    },
    /// Launch, inspect or attach a same-instance ACP conversation.
    #[command(
        after_help = "Examples:\n  amux acp profiles\n  amux acp start omp --project PROJECT_ID\n  amux acp attach VIEW_ID\n\nProfiles launch installed adapters, not terminal-screen automation. Adapter availability\ndoes not establish authentication or model access; no premium fallback is implied."
    )]
    Acp {
        #[command(subcommand)]
        command: AcpCommand,
    },
    /// Manage saved project configuration without restoring interpreter state.
    #[command(
        after_help = "Examples:\n  amux projects list\n  amux projects add scratch\n  amux projects save compiler\n  amux projects open scratch\n  amux projects remove scratch\n\nRemoval forgets saved configuration, not directories or running sessions. Runtime\nprojects remain addressable by ID; reused names can be ambiguous. List/remove require\nan updated daemon. Finish valuable work before deliberately upgrading a running daemon."
    )]
    Projects {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    /// Run the local daemon in the foreground for supervision.
    #[command(
        after_help = "Examples:\n  amux daemon\n  amux --socket /private/path/amux.sock daemon --projects-dir /path/to/projects\n\nThe socket directory must be private and owned by you. A second daemon must use\nits own runtime directory. Stopping a daemon ends its managed work."
    )]
    Daemon {
        /// Override the directory used when creating projects.
        #[arg(long)]
        projects_dir: Option<PathBuf>,
    },
    /// Submit a version-1 JSON API request and print its JSON response.
    #[command(
        after_help = "Example:\n  amux api '{\"op\":\"snapshot\"}'\n\nRequest success is not an ownership grant or completed shell/agent work. Inspect the\nreturned kind and current state. Never replay a mutation after an uncertain reply."
    )]
    Api {
        /// JSON request object; version defaults to 1 when omitted.
        request: String,
    },
    /// Read daemon, project, terminal and agent metadata without transcript content.
    #[command(after_help = "Example:\n  amux snapshot")]
    Snapshot,
    /// Open a native terminal from captured context, a project or a recipe.
    #[command(
        after_help = "Examples:\n  amux open --source main --name build\n  amux open --project scratch --recipe check\n  amux open --source main --omp --name agent\n\nThis is a native terminal, not ACP. Captured exported environment/cwd do not clone\naliases, functions or interpreter state; a busy source can report a stale capture."
    )]
    Open {
        /// Source terminal ID or unambiguous name for captured shell context.
        #[arg(long)]
        source: Option<String>,
        /// Project ID or unambiguous name for the new terminal.
        #[arg(long)]
        project: Option<String>,
        /// Display name for the new terminal.
        #[arg(long)]
        name: Option<String>,
        /// Working directory override.
        #[arg(long)]
        cwd: Option<String>,
        /// Shell command to run in the new terminal.
        #[arg(long)]
        command: Option<String>,
        /// Launch stock OMP in the native terminal, not through ACP.
        #[arg(long)]
        omp: bool,
        /// Named project recipe to run.
        #[arg(long)]
        recipe: Option<String>,
    },
    /// Rename a native terminal or ACP view without changing its runtime identity.
    #[command(
        after_help = "Examples:\n  amux name TERMINAL_ID build\n  amux name ACP_VIEW_ID research\n\nACP rename requires the exact view ID and preserves the live peer, session and pending work."
    )]
    Name {
        /// Native terminal selector or exact ACP view ID.
        terminal: String,
        /// New display name.
        name: String,
    },
    /// Close a native terminal and terminate its managed processes.
    #[command(
        after_help = "Example:\n  amux close TERMINAL_ID\n\nThis ends work in the terminal. To detach without stopping work, use Ctrl-Q in\nthe picker. Use remove after the terminal exits to forget its retained record."
    )]
    Close {
        /// Native terminal ID or unambiguous name.
        terminal: String,
    },
    /// Remove an exited native terminal record, not a saved project.
    #[command(
        after_help = "Example:\n  amux remove TERMINAL_ID\n\nLive terminals must be closed first. This frees a retained terminal slot;\nuse projects remove to forget saved project configuration instead."
    )]
    Remove {
        /// Exited native terminal ID or unambiguous name.
        terminal: String,
    },
    /// Move a native terminal in its project's window hierarchy.
    #[command(
        after_help = "Examples:\n  amux reparent TERMINAL_ID PARENT_ID\n  amux reparent TERMINAL_ID\n\nOmit PARENT to make the terminal a root. This does not restart processes."
    )]
    Reparent {
        /// Terminal to move.
        terminal: String,
        /// New parent terminal; omit to move to the project root.
        parent: Option<String>,
    },
    /// Move a native terminal up one level in its project's hierarchy.
    #[command(
        after_help = "Example:\n  amux promote TERMINAL_ID\n\nA child moves to its grandparent; a root stays a root. To move directly to a\nproject root, use reparent TERMINAL_ID without a parent."
    )]
    Promote {
        /// Terminal to promote.
        terminal: String,
    },
    /// Make a native terminal a child of another terminal in the same project.
    #[command(after_help = "Example:\n  amux demote TERMINAL_ID PARENT_ID")]
    Demote {
        /// Terminal to move.
        terminal: String,
        /// New parent terminal.
        parent: String,
    },
    /// Send native application input using an explicitly acquired current control.
    #[command(
        after_help = "Examples (CONTROL_JSON is your current private ControlHandle):\n  amux input TERMINAL_ID --control \"$CONTROL_JSON\" hello\n  amux input TERMINAL_ID --control \"$CONTROL_JSON\" --key enter\n  amux input TERMINAL_ID --control \"$CONTROL_JSON\" --paste 'pasted text'\n\nNever substitute a controller credential, claim, offer or public ownership receipt.\nInput acknowledgement does not prove application settlement."
    )]
    Input {
        /// Exact native terminal ID bound to the current control.
        terminal: String,
        /// Serialized current ControlHandle JSON; never a controller credential.
        #[arg(long)]
        control: String,
        /// Text to type, or paste when --paste is set.
        text: Option<String>,
        /// Named key to send, such as enter.
        #[arg(long)]
        key: Option<String>,
        /// Treat TEXT as pasted input rather than ordinary typing.
        #[arg(long)]
        paste: bool,
        /// Raw bytes as comma-separated integers in 0..255.
        #[arg(long)]
        bytes: Option<String>,
    },
    /// Submit shell source through the managed Zsh adapter using current control.
    #[command(
        after_help = "Examples (CONTROL_JSON is your current private ControlHandle):\n  amux execute TERMINAL_ID --control \"$CONTROL_JSON\" 'printf hello'\n  amux execute TERMINAL_ID --control \"$CONTROL_JSON\" --queue 'cargo check'\n\nStructured execution is distinct from typing into a foreground program. Inspect\nthe returned execution/state for completion; control is never implicitly acquired."
    )]
    Execute {
        /// Exact native terminal ID bound to the current control.
        terminal: String,
        /// Serialized current ControlHandle JSON; never implicitly acquired.
        #[arg(long)]
        control: String,
        /// Shell source to execute, quoted as one argument.
        source: String,
        /// Queue behind an existing structured shell execution.
        #[arg(long)]
        queue: bool,
    },
    /// Read native screen cells, text, cursor and terminal modes.
    #[command(
        after_help = "Examples:\n  amux screen TERMINAL_ID\n  amux screen TERMINAL_ID --offset 100 --limit 40"
    )]
    Screen {
        /// Native terminal ID or unambiguous name.
        terminal: String,
        /// Scrollback offset in rows; 0 reads the live surface.
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Maximum number of rows to return.
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Resize a native PTY; a visible attachment can later resize it again.
    #[command(after_help = "Example:\n  amux resize TERMINAL_ID 120 40")]
    Resize {
        /// Native terminal ID or unambiguous name.
        terminal: String,
        /// Requested number of terminal columns.
        cols: u16,
        /// Requested number of terminal rows.
        rows: u16,
    },
    /// Inspect native process, shell, execution and ownership state.
    #[command(after_help = "Example:\n  amux state TERMINAL_ID")]
    State {
        /// Native terminal ID or unambiguous name.
        terminal: String,
    },
    /// Read retained daemon events after a cursor, waiting when necessary.
    #[command(
        after_help = "Example:\n  amux events --after 0 --timeout-ms 1000\n\nUse returned event cursors for incremental reads; retained history can have gaps."
    )]
    Events {
        /// Last observed event sequence; return newer events.
        #[arg(long, default_value_t = 0)]
        after: u64,
        /// Maximum wait in milliseconds.
        #[arg(long, default_value_t = 30000)]
        timeout_ms: u64,
    },
    /// Wait for an observed native shell, agent or exit condition.
    #[command(
        after_help = "Examples:\n  amux wait TERMINAL_ID shell-ready\n  amux wait TERMINAL_ID exited --timeout-ms 1000\n\nObserved shell/agent state is not an application-input settlement fence."
    )]
    Wait {
        /// Native terminal ID or unambiguous name.
        terminal: String,
        /// State condition to observe; waiting does not acquire control.
        #[arg(value_parser = ["shell-ready", "agent-settled", "agent-working", "needs-input", "exited"])]
        condition: String,
        /// Event sequence cursor; require a matching event newer than this observation.
        #[arg(long)]
        after: Option<u64>,
        /// Maximum wait in milliseconds.
        #[arg(long, default_value_t = 30000)]
        timeout_ms: u64,
    },
    /// Patch daemon-global configuration with a JSON object.
    #[command(
        after_help = "Example:\n  amux config '{\"prefix\":\"ctrl-b\",\"terminal_limit\":64}'\n\nSupplied hooks/profile maps replace their configured values; this is a deliberate\npersistent configuration change, not an agent authentication or model operation."
    )]
    Config {
        /// JSON configuration patch, such as prefix, terminal_limit or acp_profiles.
        config: String,
    },
    /// Submit a structured shell or native-agent integration event.
    #[command(
        after_help = "Example (EVENT_JSON is a valid managed-integration event):\n  amux report TERMINAL_ID \"$EVENT_JSON\"\n\nNormally called by the managed integration. Reported state is not an ownership\ngrant or proof that application input has settled."
    )]
    Report {
        /// Native terminal ID receiving the report.
        terminal: String,
        /// Integration event as a JSON object.
        event: String,
    },
    /// Stop the daemon and terminate all managed work and unsaved runtime state.
    #[command(
        after_help = "Example:\n  amux shutdown\n\nFinish valuable work first. This is not detach: shells and ACP processes end,\nand conversations, screen history and interpreter state cannot be restored."
    )]
    Shutdown,
    /// Request all attached TUIs to detach while managed work keeps running.
    #[command(
        after_help = "Example:\n  amux detach\n\nThis is asynchronous and applies to all current attachments. To detach only your\ncurrent TUI, use Ctrl-Q in its picker. Detaching does not release ownership."
    )]
    Detach,
    /// Run a private operation used by the managed shell integration.
    #[command(
        hide = true,
        after_help = "Internal shell integration only; not a stable user-facing command."
    )]
    Internal {
        /// Integration operation and its arguments, supplied by managed shell hooks.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Report a private managed-shell integration event.
    #[command(
        hide = true,
        after_help = "Internal shell integration only; normally invoked by managed shell hooks."
    )]
    InternalReport {
        /// JSON integration event supplied by the managed shell.
        event: String,
    },
}

#[derive(Subcommand)]
enum ProjectCommand {
    /// List saved projects; unsaved runtime projects remain visible in snapshot.
    #[command(
        after_help = "Example:\n  amux projects list\n\nReturns saved project metadata as a JSON array; does not open shells or run hooks."
    )]
    List,
    /// Add saved project configuration and open its first native shell.
    #[command(
        after_help = "Examples:\n  amux projects add scratch\n  amux projects add compiler --directory /path/to/compiler\n  amux projects add scratch --config '{\"recipes\":{\"check\":\"cargo check\"}}'\n\nRegisters the caller's current directory, or an explicit existing directory.\nNo project directories are created. In the UI, use Menu -> Save project after cd."
    )]
    Add {
        /// Unique saved name containing letters, digits, '-' or '_'.
        name: String,
        /// Existing project directory; defaults to this command's current directory.
        #[arg(long)]
        directory: Option<String>,
        /// Initial hooks and recipes as JSON, installed before first open.
        #[arg(long)]
        config: Option<String>,
    },
    /// Save the default project's confirmed context under a persistent name.
    #[command(
        after_help = "Example:\n  amux projects save compiler\n\nSaves configuration and the confirmed main-shell cwd/repository root, not running\nprocesses, interpreter state or conversations. Re-saving retains the project ID."
    )]
    Save {
        /// Persistent project name.
        name: String,
    },
    /// Open a project, reusing its represented runtime or starting a new shell.
    #[command(
        after_help = "Example:\n  amux projects open compiler\n\nAn already represented project is not restarted and its hooks are not rerun.\nAfter daemon restart, opening starts a new shell, not restored interpreter state."
    )]
    Open {
        /// Exact project ID or unambiguous project name.
        project: String,
    },
    /// Patch a project's configuration, hooks or recipes with JSON.
    #[command(
        after_help = "Example:\n  amux projects config compiler '{\"recipes\":{\"check\":\"cargo check\"}}'\n\nSupplied hooks arrays and recipe maps replace their previous values; individual\nentries are not merged. Saved project IDs and running processes are retained."
    )]
    Config {
        /// Exact project ID or unambiguous project name.
        project: String,
        /// JSON project configuration patch.
        config: String,
    },
    /// Forget saved project configuration without deleting files or stopping work.
    #[command(
        after_help = "Example:\n  amux projects remove compiler\n\nOnly the saved entry is removed. Directories, files and running native/ACP sessions\nare retained; the runtime project becomes unsaved and remains addressable by ID.\nAn unsaved entry cannot be removed again. A reused name may require an exact ID."
    )]
    Remove {
        /// Saved project ID or unambiguous project name to forget.
        project: String,
    },
}

#[derive(clap::Args)]
#[command(
    after_help = "Examples:\n  amux acp start omp --project PROJECT_ID\n  amux acp start codex --project PROJECT_ID --model MODEL_ID --effort low\n  amux acp start grok --project PROJECT_ID --controller \"$CONTROLLER_JSON\"\n\nWithout --controller, attach the human UI on the original connection. With it,\nreturn JSON for explicit API control. Settings must be supported and confirmed;\nagent defaults can remain unreported. Provider/model usage is not free by implication."
)]
struct AcpStart {
    /// ACP profile: omp, codex, grok, or a configured profile.
    profile: String,
    /// Exact project ID or unambiguous project name.
    #[arg(long)]
    project: String,
    /// Display name for this ACP conversation.
    #[arg(long)]
    name: Option<String>,
    /// Working directory override for the owned adapter.
    #[arg(long)]
    cwd: Option<String>,
    /// Exact advertised model ID; omit to inherit profile/agent defaults.
    #[arg(long)]
    model: Option<String>,
    /// Exact advertised mode ID; not a permission-policy guarantee.
    #[arg(long)]
    mode: Option<String>,
    /// Exact advertised effort choice; omit to inherit profile/agent defaults.
    #[arg(long)]
    effort: Option<String>,
    /// Advertised config option ID=choice ID; repeat for distinct options.
    #[arg(long = "option", value_name = "ID=VALUE")]
    options: Vec<String>,
    /// JSON object of exact option ID:choice ID pairs, including delimiter-containing IDs.
    #[arg(long, value_name = "JSON")]
    options_json: Option<String>,
    /// Explicit external controller handle JSON; return JSON instead of attaching.
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
    #[command(
        after_help = "Example:\n  amux acp profiles\n\nReads profile availability without launching an adapter or contacting a provider."
    )]
    Profiles,
    /// Start one owned stdio adapter with explicit external control.
    #[command(
        after_help = "Example (CONTROLLER_JSON is your registered external controller handle):\n  amux acp open --project PROJECT_ID --controller \"$CONTROLLER_JSON\" -- /absolute/path/to/acp-adapter\n\nAdapter argv follows -- and is executed without a shell. The JSON receipt identifies\nthe exact owned instance and control; it does not prove initialization has completed."
    )]
    Open {
        /// Exact project ID or unambiguous project name.
        #[arg(long)]
        project: String,
        /// Serialized registered external controller handle JSON.
        #[arg(long)]
        controller: String,
        /// Display name for the new ACP conversation.
        #[arg(long)]
        name: Option<String>,
        /// Working directory override for the owned adapter.
        #[arg(long)]
        cwd: Option<String>,
        /// Adapter executable and arguments after --; no shell expansion.
        #[arg(last = true, required = true, num_args = 1.., allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Inspect an exact ACP target; conversation content is explicit opt-in.
    #[command(
        after_help = "Examples (TARGET_JSON is the exact captured ACP target tuple):\n  amux acp inspect \"$TARGET_JSON\"\n  amux acp inspect \"$TARGET_JSON\" --content --after 0\n\nTarget identity includes instance, connection and session. Default inspection is\nmetadata-only; content is bounded and retained history can report a gap."
    )]
    Inspect {
        /// Serialized exact ACP target JSON, not a terminal name or control handle.
        target: String,
        /// Include canonical conversation content in the response.
        #[arg(long)]
        content: bool,
        /// Canonical update cursor for an incremental content read.
        #[arg(long)]
        after: Option<u64>,
    },
    /// Attach the human UI to an existing live ACP view.
    #[command(
        after_help = "Example:\n  amux acp attach VIEW_ID\n\nUse the exact view ID, not a native terminal name. This never relaunches an agent\nor restores a lost session, and does not implicitly take another owner's control."
    )]
    Attach {
        /// Exact ACP view ID returned when the conversation was opened.
        view: String,
    },
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
        Command::Projects { command } => match command {
            ProjectCommand::List => json!({"op":"project-list"}),
            ProjectCommand::Add {
                name,
                directory,
                config,
            } => {
                let directory = std::fs::canonicalize(directory.unwrap_or_else(|| ".".into()))
                    .context("project directory must already exist")?;
                anyhow::ensure!(directory.is_dir(), "project context is not a directory");
                json!({"op":"project-new","name":name,"directory":directory,"config":config.map(|config|serde_json::from_str::<Value>(&config)).transpose().context("invalid initial project configuration JSON")?.unwrap_or_else(||json!({}))})
            }
            ProjectCommand::Save { name } => json!({"op":"project-save","name":name}),
            ProjectCommand::Open { project } => json!({"op":"project-open","project":project}),
            ProjectCommand::Config { project, config } => {
                json!({"op":"project-config","project":project,"config":serde_json::from_str::<Value>(&config).context("invalid project configuration JSON")?})
            }
            ProjectCommand::Remove { project } => json!({"op":"project-remove","project":project}),
        },
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

use anyhow::{Context, Result, bail, ensure};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

fn default_hook_timeout() -> u64 {
    30_000
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hook {
    #[serde(alias = "command")]
    pub source: String,
    #[serde(default = "default_hook_timeout")]
    pub timeout_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub projects_dir: PathBuf,
    pub prefix: String,
    #[serde(default)]
    pub daemon_start_hooks: Vec<Hook>,
}

impl Default for Config {
    fn default() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        Self {
            projects_dir: home.join("projects"),
            prefix: "ctrl-b".into(),
            daemon_start_hooks: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub context: PathBuf,
    pub saved: bool,
    pub main_terminal: Option<String>,
    pub hooks: Vec<Hook>,
    pub recipes: BTreeMap<String, String>,
}

// Deliberately separate disk schema: even future runtime fields on Project cannot leak.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedProject {
    id: String,
    name: String,
    context: PathBuf,
    #[serde(default)]
    hooks: Vec<Hook>,
    #[serde(default)]
    recipes: BTreeMap<String, String>,
}

impl From<&Project> for SavedProject {
    fn from(project: &Project) -> Self {
        Self {
            id: project.id.clone(),
            name: project.name.clone(),
            context: project.context.clone(),
            hooks: project.hooks.clone(),
            recipes: project.recipes.clone(),
        }
    }
}

impl From<SavedProject> for Project {
    fn from(project: SavedProject) -> Self {
        Self {
            id: project.id,
            name: project.name,
            context: project.context,
            saved: true,
            main_terminal: None,
            hooks: project.hooks,
            recipes: project.recipes,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiskConfig {
    version: u32,
    config: Config,
    projects: Vec<SavedProject>,
}

pub struct Store {
    pub config: Config,
    pub projects: BTreeMap<String, Project>,
    path: PathBuf,
    unsaved: String,
}

fn absolute(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn validate_hooks(hooks: &[Hook]) -> Result<()> {
    for hook in hooks {
        ensure!(
            !hook.source.trim().is_empty(),
            "hook source must not be empty"
        );
        ensure!(
            (1..=300_000).contains(&hook.timeout_ms),
            "hook timeout_ms must be in 1..300000"
        );
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<()> {
    ensure!(
        !name.trim().is_empty() && name.len() <= 256 && !name.contains('\0'),
        "project name must be nonempty and at most 256 bytes"
    );
    Ok(())
}

fn validate_config(config: &Config) -> Result<()> {
    ensure!(
        config.projects_dir.is_absolute(),
        "projects_dir must be absolute"
    );
    let prefix = config.prefix.to_ascii_lowercase();
    let letter = prefix
        .strip_prefix("ctrl-")
        .or_else(|| prefix.strip_prefix("control-"))
        .or_else(|| prefix.strip_prefix("c-"));
    ensure!(
        letter.is_some_and(|key| key.len() == 1 && key.as_bytes()[0].is_ascii_alphabetic()),
        "prefix must be a Ctrl-letter chord (Ctrl-x, Control-x or C-x)"
    );
    validate_hooks(&config.daemon_start_hooks)
}

fn read_disk(path: &Path) -> Result<Option<DiskConfig>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("reading project configuration"),
    };
    let disk: DiskConfig =
        serde_json::from_slice(&bytes).context("invalid project configuration")?;
    ensure!(
        disk.version == 1,
        "unsupported project configuration version {}",
        disk.version
    );
    validate_config(&disk.config)?;
    let mut ids = std::collections::HashSet::new();
    for project in &disk.projects {
        ensure!(
            !project.id.is_empty() && ids.insert(&project.id),
            "duplicate or empty project ID"
        );
        validate_name(&project.name)?;
        ensure!(
            project.context.is_absolute(),
            "project context must be absolute"
        );
        validate_hooks(&project.hooks)?;
    }
    Ok(Some(disk))
}

impl Store {
    pub fn load(path: &Path, projects_override: Option<PathBuf>) -> Result<Self> {
        let path = absolute(path)?;
        let mut config = Config::default();
        let mut projects = BTreeMap::new();
        if let Some(disk) = read_disk(&path)? {
            config = disk.config;
            for saved in disk.projects {
                let project = Project::from(saved);
                projects.insert(project.id.clone(), project);
            }
        }
        if let Some(directory) = projects_override {
            config.projects_dir = absolute(&directory)?;
        }
        validate_config(&config)?;
        let unsaved = Uuid::new_v4().to_string();
        projects.insert(
            unsaved.clone(),
            Project {
                id: unsaved.clone(),
                name: "unsaved".into(),
                context: std::env::current_dir()?,
                saved: false,
                main_terminal: None,
                hooks: Vec::new(),
                recipes: BTreeMap::new(),
            },
        );
        Ok(Self {
            config,
            projects,
            path,
            unsaved,
        })
    }

    pub fn unsaved_id(&self) -> String {
        self.unsaved.clone()
    }

    pub(crate) fn resolve(&self, selector: &str) -> Result<String> {
        if self.projects.contains_key(selector) {
            return Ok(selector.to_owned());
        }
        let mut matches = self
            .projects
            .values()
            .filter(|project| project.name == selector);
        let first = matches.next().context("project not found")?;
        ensure!(
            matches.next().is_none(),
            "ambiguous project name: {selector}"
        );
        Ok(first.id.clone())
    }

    fn check_name(&self, id: &str, name: &str) -> Result<()> {
        validate_name(name)?;
        ensure!(
            !self
                .projects
                .values()
                .any(|p| p.id != id && p.saved && p.name == name),
            "saved project name already exists: {name}"
        );
        Ok(())
    }

    pub fn save(&mut self, id: &str, name: &str) -> Result<Project> {
        let id = self.resolve(id)?;
        self.check_name(&id, name)?;
        let old = self.projects[&id].clone();
        let project = self.projects.get_mut(&id).expect("resolved project");
        project.name = name.to_owned();
        project.saved = true;
        let result = project.clone();
        if let Err(error) = self.persist() {
            self.projects.insert(id, old);
            return Err(error);
        }
        Ok(result)
    }

    /// Initial configuration is limited to hooks and recipes, validated before directory creation.
    pub fn create(
        &mut self,
        name: &str,
        directory: Option<PathBuf>,
        patch: Value,
    ) -> Result<Project> {
        self.check_name("", name)?;
        ensure!(
            name.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "new project names must contain only letters, digits, '-' or '_'"
        );
        let fields = patch
            .as_object()
            .context("project config must be an object")?;
        let mut hooks = Vec::new();
        let mut recipes = BTreeMap::new();
        for (field, value) in fields {
            match field.as_str() {
                "hooks" => {
                    hooks = serde_json::from_value(value.clone())?;
                    validate_hooks(&hooks)?;
                }
                "recipes" => recipes = serde_json::from_value(value.clone())?,
                _ => bail!("unknown or runtime-only initial project setting: {field}"),
            }
        }
        let context = absolute(&directory.unwrap_or_else(|| self.config.projects_dir.join(name)))?;
        match fs::metadata(&context) {
            Ok(metadata) => ensure!(metadata.is_dir(), "project context is not a directory"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir_all(&context).context("creating project directory")?;
            }
            Err(error) => return Err(error).context("checking project directory"),
        }
        let project = Project {
            id: Uuid::new_v4().to_string(),
            name: name.to_owned(),
            context,
            saved: true,
            main_terminal: None,
            hooks,
            recipes,
        };
        self.projects.insert(project.id.clone(), project.clone());
        if let Err(error) = self.persist() {
            self.projects.remove(&project.id);
            return Err(error);
        }
        Ok(project)
    }

    pub fn open(&mut self, selector: &str) -> Result<Project> {
        // Reload configuration, not a process or organizational tree.
        if let Some(disk) = read_disk(&self.path)? {
            for saved in disk.projects {
                let mut project = Project::from(saved);
                if let Some(active) = self.projects.get(&project.id) {
                    project.main_terminal = active.main_terminal.clone();
                }
                self.projects.insert(project.id.clone(), project);
            }
        }
        let id = self.resolve(selector)?;
        let project = self.projects[&id].clone();
        ensure!(
            project.context.is_dir(),
            "project directory is unavailable: {}",
            project.context.display()
        );
        Ok(project)
    }

    pub fn configure(&mut self, id: &str, patch: Value) -> Result<Project> {
        let id = self.resolve(id)?;
        let fields = patch
            .as_object()
            .context("project config must be an object")?;
        let mut updated = self.projects[&id].clone();
        for (field, value) in fields {
            match field.as_str() {
                "name" => {
                    updated.name = value.as_str().context("name must be a string")?.to_owned();
                    self.check_name(&id, &updated.name)?;
                }
                "context" if !updated.saved => {
                    updated.context = absolute(Path::new(
                        value.as_str().context("context must be a string")?,
                    ))?;
                    ensure!(
                        updated.context.is_dir(),
                        "context must be an existing directory"
                    );
                }
                "context" => bail!("saved project context cannot be changed"),
                "hooks" => {
                    updated.hooks = serde_json::from_value(value.clone())?;
                    validate_hooks(&updated.hooks)?;
                }
                "recipes" => {
                    updated.recipes = serde_json::from_value(value.clone())?;
                }
                _ => bail!("unknown or runtime-only project setting: {field}"),
            }
        }
        let old = self
            .projects
            .insert(id.clone(), updated.clone())
            .expect("resolved project");
        if let Err(error) = self.persist() {
            self.projects.insert(id, old);
            return Err(error);
        }
        Ok(updated)
    }

    pub fn configure_global(&mut self, patch: Value) -> Result<Config> {
        let fields = patch
            .as_object()
            .context("global config must be an object")?;
        let mut updated = self.config.clone();
        for (field, value) in fields {
            match field.as_str() {
                "projects_dir" => {
                    updated.projects_dir = absolute(Path::new(
                        value.as_str().context("projects_dir must be a string")?,
                    ))?
                }
                "prefix" => {
                    updated.prefix = value
                        .as_str()
                        .context("prefix must be a string")?
                        .to_owned()
                }
                "daemon_start_hooks" => {
                    updated.daemon_start_hooks = serde_json::from_value(value.clone())?
                }
                _ => bail!("unknown global setting: {field}"),
            }
        }
        validate_config(&updated)?;
        let old = std::mem::replace(&mut self.config, updated);
        if let Err(error) = self.persist() {
            self.config = old;
            return Err(error);
        }
        Ok(self.config.clone())
    }

    pub fn persist(&self) -> Result<()> {
        let disk = DiskConfig {
            version: 1,
            config: self.config.clone(),
            projects: self
                .projects
                .values()
                .filter(|p| p.saved)
                .map(SavedProject::from)
                .collect(),
        };
        let bytes = serde_json::to_vec_pretty(&disk)?;
        let parent = self
            .path
            .parent()
            .context("configuration has no parent directory")?;
        fs::create_dir_all(parent)?;
        let temporary = parent.join(format!(".amux-config-{}.tmp", Uuid::new_v4()));
        let result = (|| -> Result<()> {
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, &self.path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result.context("persisting project configuration")
    }
}

const OUTPUT_LIMIT: usize = 1024 * 1024;
const GIT_OUTPUT_LIMIT: usize = 8 * 1024 * 1024;

struct ProcessOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    timed_out: bool,
    truncated: bool,
}

fn nonblocking(reader: &impl AsRawFd) -> Result<()> {
    let fd = reader.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    ensure!(
        flags >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } >= 0,
        "cannot configure process output: {}",
        std::io::Error::last_os_error()
    );
    Ok(())
}

fn drain(reader: &mut impl Read, target: &mut Vec<u8>, limit: usize) -> Result<bool> {
    let mut truncated = false;
    let mut buffer = [0_u8; 8192];
    // Bound each pass even when the producer never stops writing.
    for _ in 0..32 {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                let keep = count.min(limit.saturating_sub(target.len()));
                target.extend_from_slice(&buffer[..keep]);
                truncated |= keep < count;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(truncated)
}

fn bounded_process(mut command: Command, timeout: Duration, limit: usize) -> Result<ProcessOutput> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = command.spawn().context("spawning worker process")?;
    let group = child.id() as i32;
    let outcome = (|| -> Result<ProcessOutput> {
        let mut stdout = child.stdout.take().context("missing process stdout")?;
        let mut stderr = child.stderr.take().context("missing process stderr")?;
        nonblocking(&stdout)?;
        nonblocking(&stderr)?;
        let started = Instant::now();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut truncated = false;
        let mut timed_out = false;
        let status = loop {
            truncated |= drain(&mut stdout, &mut out, limit)?;
            truncated |= drain(&mut stderr, &mut err, limit)?;
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if started.elapsed() >= timeout {
                timed_out = true;
                unsafe {
                    libc::kill(-group, libc::SIGKILL);
                }
                break child.wait()?;
            }
            thread::sleep(Duration::from_millis(5));
        };
        // A shell can exit with descendants still holding its output descriptors.
        // Kill its private group before final nonblocking reads; never wait for EOF.
        unsafe {
            libc::kill(-group, libc::SIGKILL);
        }
        truncated |= drain(&mut stdout, &mut out, limit)?;
        truncated |= drain(&mut stderr, &mut err, limit)?;
        Ok(ProcessOutput {
            status,
            stdout: out,
            stderr: err,
            timed_out,
            truncated,
        })
    })();
    if outcome.is_err() {
        unsafe {
            libc::kill(-group, libc::SIGKILL);
        }
        let _ = child.wait();
    }
    outcome
}

pub fn run_hooks(hooks: &[Hook], cwd: &Path, env: &BTreeMap<String, String>) -> Vec<Value> {
    hooks.iter().enumerate().map(|(index, hook)| {
        let started = Instant::now();
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg(&hook.source).current_dir(cwd).envs(env);
        match bounded_process(command, Duration::from_millis(hook.timeout_ms), OUTPUT_LIMIT) {
            Ok(output) => json!({
                "kind":"hook", "index":index, "source":hook.source,
                "ok":output.status.success() && !output.timed_out,
                "exit_status":output.status.code(), "timed_out":output.timed_out,
                "stdout":String::from_utf8_lossy(&output.stdout),
                "stderr":String::from_utf8_lossy(&output.stderr),
                "output_truncated":output.truncated,
                "duration_ms":started.elapsed().as_millis() as u64,
                "error":if output.timed_out { Some("hook timed out".to_owned()) }
                    else if !output.status.success() { Some(format!("hook exited with {}", output.status)) }
                    else { None },
            }),
            Err(error) => json!({"kind":"hook","index":index,"source":hook.source,
                "ok":false,"exit_status":null,"timed_out":false,"stdout":"","stderr":"",
                "output_truncated":false,"duration_ms":started.elapsed().as_millis() as u64,
                "error":format!("{error:#}")}),
        }
    }).collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

const GIT_CACHE_LIMIT: usize = 256;

#[derive(Default)]
struct CacheState {
    entries: HashMap<PathBuf, CacheEntry>,
    workspaces: HashSet<PathBuf>,
}

struct CacheEntry {
    value: Value,
    pending: bool,
    requested: Instant,
}

/// At most 256 metadata entries, including missing-repository and error results.
/// Refresh evicts the oldest nonpending, unpinned location. If none is eligible,
/// new locations are not admitted; `get` reports unavailable capacity until space
/// is available, and a subsequent `refresh` must retry admission.
pub struct GitCache {
    state: Arc<Mutex<CacheState>>,
    sender: Option<mpsc::SyncSender<PathBuf>>,
    stopping: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Default for GitCache {
    fn default() -> Self {
        Self::new()
    }
}

impl GitCache {
    pub fn new() -> Self {
        let state = Arc::new(Mutex::new(CacheState::default()));
        let stopping = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::sync_channel::<PathBuf>(64);
        let shared = Arc::clone(&state);
        let stop = Arc::clone(&stopping);
        let worker = thread::spawn(move || {
            while let Ok(cwd) = receiver.recv() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let result = git_metadata(&cwd, &stop);
                let mut value = result.unwrap_or_else(|error| {
                    let mut empty = empty_git();
                    empty["error"] = json!(format!("{error:#}"));
                    empty
                });
                value["refreshed_at_ms"] = json!(now_ms());
                if value["workspace"].is_null() {
                    value["workspace"] = json!(cwd);
                }
                let mut state = shared.lock();
                if let Some(entry) = state.entries.get_mut(&cwd) {
                    entry.value = value;
                    entry.pending = false;
                }
            }
        });
        Self {
            state,
            sender: Some(sender),
            stopping,
            worker: Some(worker),
        }
    }

    /// Replace the live workspace pins without doing filesystem or Git work.
    /// Pins track only the currently supplied workspaces, not traversal history.
    pub fn retain_workspaces(&self, workspaces: &[PathBuf]) {
        let workspaces = workspaces
            .iter()
            .filter_map(|path| absolute(path).ok())
            .collect();
        self.state.lock().workspaces = workspaces;
    }

    pub fn refresh(&self, cwd: &Path) {
        // Do not canonicalize or run Git here: this can be called on PTY paths.
        let Ok(key) = absolute(cwd) else {
            return;
        };
        let mut state = self.state.lock();
        if !state.entries.contains_key(&key) && state.entries.len() >= GIT_CACHE_LIMIT {
            let victim = state
                .entries
                .iter()
                .filter(|(path, entry)| !entry.pending && !state.workspaces.contains(*path))
                .min_by_key(|(_, entry)| entry.requested)
                .map(|(path, _)| path.clone());
            let Some(victim) = victim else {
                // Hard cap: if every entry is live or pending, defer new locations.
                // Their get() reports unavailable while admission is exhausted.
                return;
            };
            state.entries.remove(&victim);
        }
        let entry = state
            .entries
            .entry(key.clone())
            .or_insert_with(|| CacheEntry {
                value: empty_git(),
                pending: false,
                requested: Instant::now()
                    .checked_sub(Duration::from_secs(2))
                    .unwrap_or_else(Instant::now),
            });
        if entry.pending || entry.requested.elapsed() < Duration::from_secs(1) {
            return;
        }
        entry.pending = true;
        if self
            .sender
            .as_ref()
            .is_some_and(|sender| sender.try_send(key).is_ok())
        {
            entry.requested = Instant::now();
        } else {
            entry.pending = false;
        }
    }

    pub fn get(&self, cwd: &Path) -> Value {
        let key = absolute(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        let state = self.state.lock();
        let (mut value, pending) = state
            .entries
            .get(&key)
            .map(|entry| (entry.value.clone(), entry.pending))
            .unwrap_or_else(|| (empty_git(), false));
        if !state.entries.contains_key(&key)
            && state.entries.len() >= GIT_CACHE_LIMIT
            && state
                .entries
                .iter()
                .all(|(path, entry)| entry.pending || state.workspaces.contains(path))
        {
            value["error"] = json!("Git metadata unavailable: cache capacity is pinned or pending");
        }
        let captured = value["refreshed_at_ms"].as_u64();
        value["refreshing"] = json!(pending);
        value["freshness"] = json!(match captured {
            None if !value["error"].is_null() => "unavailable",
            None => "unknown",
            Some(_) if !value["error"].is_null() => "error",
            Some(time) if now_ms().saturating_sub(time) > 5_000 => "stale",
            Some(_) => "fresh",
        });
        value["age_ms"] = captured
            .map(|time| json!(now_ms().saturating_sub(time)))
            .unwrap_or(Value::Null);
        value
    }
}

impl Drop for GitCache {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Relaxed);
        drop(self.sender.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn empty_git() -> Value {
    json!({"repository_root":null,"git_dir":null,"common_dir":null,"worktree_id":null,
        "branch":null,"detached":null,"commit":null,"isolated":null,"upstream":null,
        "ahead":null,"behind":null,"staged":null,"unstaged":null,"untracked":null,
        "refreshed_at_ms":null,"error":null})
}

fn workspace_git_command(cwd: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .env_remove("GIT_DIR")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    command
}

/// Resolve the current workspace without consulting asynchronously cached metadata.
/// Callers must perform this bounded filesystem/Git work outside metadata locks.
pub(crate) fn resolve_workspace_context(cwd: &Path) -> Result<PathBuf> {
    let cwd = fs::canonicalize(cwd).context("resolving workspace directory")?;
    let output = bounded_process(
        workspace_git_command(&cwd, &["rev-parse", "--show-toplevel"]),
        Duration::from_secs(3),
        GIT_OUTPUT_LIMIT,
    )?;
    ensure!(!output.timed_out, "Git workspace resolution timed out");
    ensure!(
        !output.truncated,
        "Git workspace resolution exceeds output limit"
    );
    if output.status.success() {
        let root = String::from_utf8(output.stdout).context("Git repository path is not UTF-8")?;
        return fs::canonicalize(root.trim_end_matches('\n'))
            .context("resolving Git workspace root");
    }
    let error = String::from_utf8_lossy(&output.stderr);
    if error.contains("not a git repository") {
        return Ok(cwd);
    }
    bail!("Git workspace resolution failed: {}", error.trim())
}

fn git_output(cwd: &Path, args: &[&str], stopping: &AtomicBool) -> Result<Vec<u8>> {
    ensure!(
        !stopping.load(Ordering::Relaxed),
        "Git cache is shutting down"
    );
    let command = workspace_git_command(cwd, args);
    let output = bounded_process(command, Duration::from_secs(3), GIT_OUTPUT_LIMIT)?;
    ensure!(!output.timed_out, "Git refresh timed out");
    ensure!(
        output.status.success(),
        "Git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    ensure!(!output.truncated, "Git metadata exceeds output limit");
    Ok(output.stdout)
}

#[derive(Default)]
struct DiffStats {
    insertions: u64,
    deletions: u64,
    binary: usize,
}

fn diff_stats(bytes: &[u8]) -> Result<DiffStats> {
    let mut stats = DiffStats::default();
    let mut records = bytes.split(|b| *b == 0);
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        let mut fields = record.splitn(3, |b| *b == b'\t');
        let added = fields.next().context("invalid Git numstat")?;
        let removed = fields.next().context("invalid Git numstat")?;
        let path = fields.next().context("invalid Git numstat")?;
        if added == b"-" || removed == b"-" {
            stats.binary += 1;
        } else {
            stats.insertions += std::str::from_utf8(added)?.parse::<u64>()?;
            stats.deletions += std::str::from_utf8(removed)?.parse::<u64>()?;
        }
        if path.is_empty() {
            records.next().context("missing rename source")?;
            records.next().context("missing rename target")?;
        }
    }
    Ok(stats)
}

fn git_metadata(cwd: &Path, stopping: &AtomicBool) -> Result<Value> {
    let roots = git_output(
        cwd,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--absolute-git-dir",
            "--git-common-dir",
        ],
        stopping,
    )?;
    let roots = String::from_utf8(roots).context("Git repository path is not UTF-8")?;
    let mut roots = roots.lines();
    let root = roots.next().context("missing Git repository root")?;
    let git_dir = roots.next().context("missing Git directory")?;
    let common = roots.next().context("missing Git common directory")?;
    let git_dir = fs::canonicalize(git_dir)?;
    let common = fs::canonicalize(common)?;
    let mut value = empty_git();
    value["workspace"] = json!(fs::canonicalize(cwd)?);
    value["repository_root"] = json!(root);
    value["git_dir"] = json!(git_dir);
    value["common_dir"] = json!(common);
    value["worktree_id"] = json!(git_dir);
    value["isolated"] = json!(git_dir != common);
    let status = git_output(
        cwd,
        &[
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=all",
        ],
        stopping,
    )?;
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut untracked = Vec::new();
    let mut records = status.split(|b| *b == 0);
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(record);
        if let Some(oid) = text.strip_prefix("# branch.oid ") {
            if oid != "(initial)" {
                value["commit"] = json!(oid);
            }
        } else if let Some(branch) = text.strip_prefix("# branch.head ") {
            value["detached"] = json!(branch == "(detached)");
            if branch != "(detached)" {
                value["branch"] = json!(branch);
            }
        } else if let Some(upstream) = text.strip_prefix("# branch.upstream ") {
            value["upstream"] = json!(upstream);
        } else if let Some(divergence) = text.strip_prefix("# branch.ab ") {
            let mut counts = divergence.split_whitespace();
            value["ahead"] = json!(
                counts
                    .next()
                    .context("missing ahead count")?
                    .trim_start_matches('+')
                    .parse::<u64>()?
            );
            value["behind"] = json!(
                counts
                    .next()
                    .context("missing behind count")?
                    .trim_start_matches('-')
                    .parse::<u64>()?
            );
        } else if let Some(path) = text.strip_prefix("? ") {
            untracked.push(path.to_owned());
        } else if record[0] == b'1' || record[0] == b'2' || record[0] == b'u' {
            let field_count = match record[0] {
                b'1' => 9,
                b'2' => 10,
                _ => 11,
            };
            let fields: Vec<&str> = text.splitn(field_count, ' ').collect();
            ensure!(
                fields.len() == field_count && fields[1].len() == 2,
                "invalid Git status record"
            );
            let xy = fields[1].as_bytes();
            let path = fields[field_count - 1];
            let original = if record[0] == b'2' {
                Some(
                    String::from_utf8_lossy(records.next().context("missing rename source")?)
                        .into_owned(),
                )
            } else {
                None
            };
            if xy[0] != b'.' {
                staged.push(json!({"path":path,"status":(xy[0] as char).to_string(),"original_path":original}));
            }
            if xy[1] != b'.' {
                unstaged.push(json!({"path":path,"status":(xy[1] as char).to_string(),"original_path":original}));
            }
        }
    }
    let index = diff_stats(&git_output(
        cwd,
        &[
            "diff",
            "--cached",
            "--numstat",
            "-z",
            "--no-ext-diff",
            "--no-textconv",
        ],
        stopping,
    )?)?;
    let working = diff_stats(&git_output(
        cwd,
        &["diff", "--numstat", "-z", "--no-ext-diff", "--no-textconv"],
        stopping,
    )?)?;
    let staged_count = staged.len();
    let unstaged_count = unstaged.len();
    let untracked_count = untracked.len();
    staged.truncate(100);
    unstaged.truncate(100);
    untracked.truncate(100);
    value["staged"] = json!({"count":staged_count,"files":staged,"files_truncated":staged_count>100,
        "insertions":index.insertions,"deletions":index.deletions,"binary":index.binary});
    value["unstaged"] = json!({"count":unstaged_count,"files":unstaged,"files_truncated":unstaged_count>100,
        "insertions":working.insertions,"deletions":working.deletions,"binary":working.binary});
    value["untracked"] =
        json!({"count":untracked_count,"files":untracked,"files_truncated":untracked_count>100});
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("amux-projects-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn git(cwd: &Path, args: &[&str]) {
        let result = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "amux test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "amux test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    fn repository(path: &Path) {
        fs::create_dir_all(path).unwrap();
        git(path, &["init", "-b", "main"]);
        fs::write(path.join("tracked"), "initial\n").unwrap();
        git(path, &["add", "."]);
        git(path, &["commit", "-m", "initial"]);
    }

    fn metadata(cache: &GitCache, cwd: &Path) -> Value {
        cache.refresh(cwd);
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let value = cache.get(cwd);
            if !value["refreshed_at_ms"].is_null() && value["refreshing"] == false {
                return value;
            }
            assert!(
                Instant::now() < deadline,
                "Git refresh did not finish: {value}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn saved_identity_context_hooks_and_recipes_survive_without_runtime() {
        let temp = TempDir::new();
        let file = temp.0.join("config.json");
        let mut store = Store::load(&file, Some(temp.0.join("projects"))).unwrap();
        let id = store.unsaved_id();
        let context = temp.0.join("context");
        fs::create_dir(&context).unwrap();
        store.projects.get_mut(&id).unwrap().context = context.clone();
        store.projects.get_mut(&id).unwrap().main_terminal = Some("runtime-only".into());
        let saved = store.save(&id, "durable").unwrap();
        store
            .configure(
                &id,
                json!({"hooks":[{"source":"printf hello","timeout_ms":500}],
            "recipes":{"agent":"omp"}}),
            )
            .unwrap();
        let bytes = fs::read_to_string(&file).unwrap();
        assert!(!bytes.contains("main_terminal"));
        assert!(!bytes.contains("runtime-only"));
        let mut reopened = Store::load(&file, None).unwrap();
        assert_ne!(reopened.unsaved_id(), id);
        let project = reopened.open("durable").unwrap();
        assert_eq!(project.id, saved.id);
        assert_eq!(project.context, context);
        assert_eq!(project.hooks[0].source, "printf hello");
        assert_eq!(project.recipes["agent"], "omp");
        assert_eq!(project.main_terminal, None);
    }

    #[test]
    fn project_creation_is_safe_and_uses_configured_directory() {
        let temp = TempDir::new();
        let base = temp.0.join("projects");
        let mut store = Store::load(&temp.0.join("config.json"), Some(base.clone())).unwrap();
        assert!(store.create("../escape", None, json!({})).is_err());
        assert!(store.create("a/b", None, json!({})).is_err());
        assert!(store.create("", None, json!({})).is_err());
        let project = store.create("safe-project", None, json!({})).unwrap();
        assert_eq!(project.context, base.join("safe-project"));
        assert!(project.context.is_dir());
        assert!(store.create("safe-project", None, json!({})).is_err());
        assert!(
            store
                .configure(&project.id, json!({"context":"/tmp/changed"}))
                .is_err()
        );
        assert_eq!(store.open(&project.id).unwrap().context, project.context);
    }

    #[test]
    fn initial_project_hooks_and_recipes_are_ready_on_first_open() {
        let temp = TempDir::new();
        let file = temp.0.join("config.json");
        let mut store = Store::load(&file, Some(temp.0.join("projects"))).unwrap();
        let created = store
            .create(
                "ready",
                None,
                json!({
                    "hooks": [{"source": "printf installed > setup-marker", "timeout_ms": 500}],
                    "recipes": {"check": "cat setup-marker"}
                }),
            )
            .unwrap();
        // The first open reloads the saved configuration, as the daemon does.
        let mut reopened = Store::load(&file, None).unwrap();
        let opened = reopened.open(&created.id).unwrap();
        assert_eq!(opened.hooks.len(), 1);
        assert_eq!(opened.recipes["check"], "cat setup-marker");
        let results = run_hooks(&opened.hooks, &opened.context, &BTreeMap::new());
        assert_eq!(results[0]["ok"], true);
        assert_eq!(
            fs::read_to_string(opened.context.join("setup-marker")).unwrap(),
            "installed"
        );
        let recipe = Command::new("/bin/sh")
            .args(["-c", &opened.recipes["check"]])
            .current_dir(&opened.context)
            .output()
            .unwrap();
        assert!(recipe.status.success());
        assert_eq!(recipe.stdout, b"installed");
    }

    #[test]
    fn invalid_initial_config_changes_neither_disk_store_nor_directory() {
        let temp = TempDir::new();
        let file = temp.0.join("config.json");
        let base = temp.0.join("projects");
        let mut store = Store::load(&file, Some(base.clone())).unwrap();
        store.create("existing", None, json!({})).unwrap();
        let original = fs::read(&file).unwrap();
        let original_count = store.projects.len();
        let invalid = [
            json!(null),
            json!([]),
            json!({"hooks": [{"source": "   "}]}),
            json!({"hooks": [{"source": "printf invalid", "timeout_ms": 0}]}),
            json!({"hooks": [{"source": "printf invalid", "extra": true}]}),
            json!({"hooks": "not an array"}),
            json!({"recipes": {"check": 42}}),
            json!({"name": "renamed"}),
            json!({"context": "/tmp"}),
            json!({"main_terminal": "runtime"}),
            json!({"unknown": true}),
        ];
        for (index, patch) in invalid.into_iter().enumerate() {
            let name = format!("invalid-{index}");
            assert!(store.create(&name, None, patch).is_err());
            assert_eq!(store.projects.len(), original_count);
            assert!(!store.projects.values().any(|project| project.name == name));
            assert_eq!(fs::read(&file).unwrap(), original);
            assert!(!base.join(name).exists());
        }
        assert_eq!(
            Store::load(&file, None)
                .unwrap()
                .projects
                .values()
                .filter(|p| p.saved)
                .count(),
            1
        );
    }

    #[test]
    fn daemon_and_project_hooks_are_independent_and_ordered() {
        let temp = TempDir::new();
        let mut store = Store::load(&temp.0.join("config.json"), None).unwrap();
        store
            .configure_global(
                json!({"daemon_start_hooks":[{"source":"printf daemon","timeout_ms":500}]}),
            )
            .unwrap();
        let id = store.unsaved_id();
        store
            .configure(
                &id,
                json!({"hooks":[
                    {"source":"printf first > order; printf \"$EXPORTED\"","timeout_ms":500},
                    {"source":"cat order; printf second >> order; exit 7","timeout_ms":500},
                    {"source":"cat order","timeout_ms":500}
                ]}),
            )
            .unwrap();
        let env = BTreeMap::from([("EXPORTED".into(), "inherited".into())]);
        let daemon = run_hooks(&store.config.daemon_start_hooks, &temp.0, &env);
        assert_eq!(daemon[0]["stdout"], "daemon");
        let results = run_hooks(&store.projects[&id].hooks, &temp.0, &env);
        assert_eq!(results[0]["stdout"], "inherited");
        assert_eq!(results[1]["stdout"], "first");
        assert_eq!(results[1]["exit_status"], 7);
        assert_eq!(results[2]["stdout"], "firstsecond");
        assert_eq!(results[2]["ok"], true);
    }

    #[test]
    fn timed_out_hook_kills_descendants_and_continues() {
        let temp = TempDir::new();
        let hooks = vec![
            Hook {
                source: "sleep 30 & child=$!; printf '%s' \"$child\" > descendant; wait".into(),
                timeout_ms: 150,
            },
            Hook {
                source: "printf usable".into(),
                timeout_ms: 500,
            },
        ];
        let started = Instant::now();
        let results = run_hooks(&hooks, &temp.0, &BTreeMap::new());
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(results[0]["timed_out"], true);
        assert_eq!(results[1]["stdout"], "usable");
        let pid: i32 = fs::read_to_string(temp.0.join("descendant"))
            .unwrap()
            .parse()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let alive = unsafe { libc::kill(pid, 0) } == 0;
            if !alive {
                break;
            }
            // Linux may retain an already killed orphan as a zombie until init reaps it.
            let zombie = fs::read_to_string(format!("/proc/{pid}/stat"))
                .ok()
                .and_then(|stat| {
                    stat.rsplit_once(") ")
                        .map(|(_, tail)| tail.starts_with('Z'))
                })
                .unwrap_or(false);
            if zombie {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "descendant {pid} survived hook timeout"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn inherited_pipe_from_background_process_cannot_hang_hook() {
        let temp = TempDir::new();
        let hooks = vec![Hook {
            source: "sleep 30 & printf finished".into(),
            timeout_ms: 500,
        }];
        let started = Instant::now();
        let results = run_hooks(&hooks, &temp.0, &BTreeMap::new());
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(results[0]["stdout"], "finished");
        assert_eq!(results[0]["ok"], true);
    }

    #[test]
    fn workspace_traversal_bounds_missing_repo_history_and_retains_live_metadata() {
        let temp = TempDir::new();
        let live = temp.0.join("live");
        repository(&live);
        let cache = GitCache::new();
        cache.retain_workspaces(std::slice::from_ref(&live));
        let live_value = metadata(&cache, &live);
        let mut visited = Vec::new();
        for index in 0..(GIT_CACHE_LIMIT + 32) {
            let directory = temp.0.join(format!("location-{index}"));
            fs::create_dir(&directory).unwrap();
            let value = metadata(&cache, &directory);
            assert!(!value["error"].is_null());
            visited.push(directory);
        }
        let retained = visited
            .iter()
            .filter(|path| !cache.get(path)["refreshed_at_ms"].is_null())
            .count();
        assert_eq!(retained, GIT_CACHE_LIMIT - 1);
        assert!(cache.get(&visited[0])["refreshed_at_ms"].is_null());
        assert_eq!(
            cache.get(&live)["refreshed_at_ms"],
            live_value["refreshed_at_ms"]
        );
        assert_eq!(
            cache.get(&live)["repository_root"],
            json!(fs::canonicalize(&live).unwrap())
        );

        // Pin every resident location: overflow is unknown, not eviction or growth.
        let resident: Vec<_> = visited
            .iter()
            .filter(|path| !cache.get(path)["refreshed_at_ms"].is_null())
            .cloned()
            .chain(std::iter::once(live.clone()))
            .collect();
        cache.retain_workspaces(&resident);
        cache.refresh(&visited[0]);
        assert!(cache.get(&visited[0])["refreshed_at_ms"].is_null());
        assert_eq!(cache.get(&visited[0])["refreshing"], false);
        assert_eq!(cache.get(&visited[0])["freshness"], "unavailable");
        assert!(
            cache.get(&visited[0])["error"]
                .as_str()
                .unwrap()
                .contains("cache capacity")
        );
        assert!(
            resident
                .iter()
                .all(|path| !cache.get(path)["refreshed_at_ms"].is_null())
        );

        // Replacing pins releases inactive locations for the next admission.
        cache.retain_workspaces(std::slice::from_ref(&live));
        assert!(!metadata(&cache, &visited[0])["refreshed_at_ms"].is_null());
        assert_eq!(
            cache.get(&live)["refreshed_at_ms"],
            live_value["refreshed_at_ms"]
        );
        assert_eq!(
            visited
                .iter()
                .filter(|path| !cache.get(path)["refreshed_at_ms"].is_null())
                .count(),
            GIT_CACHE_LIMIT - 1
        );
    }

    #[test]
    fn equal_branches_do_not_alias_distinct_worktrees() {
        let temp = TempDir::new();
        let root = temp.0.join("main");
        repository(&root);
        let worktree = temp.0.join("linked");
        git(
            &root,
            &[
                "worktree",
                "add",
                "--force",
                worktree.to_str().unwrap(),
                "main",
            ],
        );
        fs::write(worktree.join("only-linked"), "untracked\n").unwrap();
        let cache = GitCache::new();
        let main = metadata(&cache, &root);
        let linked = metadata(&cache, &worktree);
        assert_eq!(main["branch"], linked["branch"]);
        assert_ne!(main["git_dir"], linked["git_dir"]);
        assert_ne!(main["repository_root"], linked["repository_root"]);
        assert_eq!(main["common_dir"], linked["common_dir"]);
        assert_eq!(main["isolated"], false);
        assert_eq!(linked["isolated"], true);
        assert_eq!(main["untracked"]["count"], 0);
        assert_eq!(linked["untracked"]["count"], 1);
    }

    #[test]
    fn git_summaries_separate_index_worktree_and_untracked() {
        let temp = TempDir::new();
        repository(&temp.0);
        fs::write(temp.0.join("tracked"), "initial\nstaged\n").unwrap();
        git(&temp.0, &["add", "tracked"]);
        fs::write(temp.0.join("tracked"), "initial\nstaged\nunstaged\n").unwrap();
        fs::write(temp.0.join("new file"), "new\n").unwrap();
        let value = metadata(&GitCache::new(), &temp.0);
        assert_eq!(value["error"], Value::Null);
        assert_eq!(value["staged"]["count"], 1);
        assert_eq!(value["staged"]["insertions"], 1);
        assert_eq!(value["unstaged"]["count"], 1);
        assert_eq!(value["unstaged"]["insertions"], 1);
        assert_eq!(value["untracked"]["files"][0], "new file");
        assert_eq!(value["ahead"], Value::Null);
        assert_eq!(value["behind"], Value::Null);
        assert_eq!(value["detached"], false);
        assert!(value["refreshed_at_ms"].as_u64().unwrap() > 0);
    }

    #[test]
    fn git_reports_detached_commit_and_upstream_divergence() {
        let temp = TempDir::new();
        repository(&temp.0);
        git(&temp.0, &["branch", "upstream"]);
        git(&temp.0, &["branch", "--set-upstream-to=upstream", "main"]);
        fs::write(temp.0.join("tracked"), "main change\n").unwrap();
        git(&temp.0, &["commit", "-am", "main advance"]);
        git(&temp.0, &["checkout", "upstream"]);
        fs::write(temp.0.join("upstream-only"), "upstream change\n").unwrap();
        git(&temp.0, &["add", "upstream-only"]);
        git(&temp.0, &["commit", "-m", "upstream advance"]);
        git(&temp.0, &["checkout", "main"]);
        let value = metadata(&GitCache::new(), &temp.0);
        assert_eq!(value["upstream"], "upstream");
        assert_eq!(value["ahead"], 1);
        assert_eq!(value["behind"], 1);
        git(&temp.0, &["checkout", "--detach", "HEAD"]);
        let detached = metadata(&GitCache::new(), &temp.0);
        assert_eq!(detached["branch"], Value::Null);
        assert_eq!(detached["detached"], true);
        assert!(detached["commit"].as_str().unwrap().len() >= 40);
    }

    #[test]
    fn large_worktrees_keep_complete_counts_and_bounded_file_details() {
        let temp = TempDir::new();
        git(&temp.0, &["init", "-b", "main"]);
        for i in 0..150 {
            fs::write(temp.0.join(format!("untracked-{i}")), "data").unwrap();
        }
        let value = metadata(&GitCache::new(), &temp.0);
        assert_eq!(value["untracked"]["count"], 150);
        assert_eq!(value["untracked"]["files_truncated"], true);
        assert_eq!(value["untracked"]["files"].as_array().unwrap().len(), 100);
        git(&temp.0, &["add", "."]);
        let value = metadata(&GitCache::new(), &temp.0);
        assert_eq!(value["staged"]["count"], 150);
        assert_eq!(value["staged"]["files_truncated"], true);
        assert_eq!(value["untracked"]["count"], 0);
    }

    #[test]
    fn missing_repository_and_new_repository_are_explicit() {
        let temp = TempDir::new();
        let missing = metadata(&GitCache::new(), &temp.0);
        assert!(!missing["error"].is_null());
        assert_eq!(missing["repository_root"], Value::Null);
        git(&temp.0, &["init", "-b", "main"]);
        let value = metadata(&GitCache::new(), &temp.0);
        assert_eq!(value["error"], Value::Null);
        assert_eq!(value["branch"], "main");
        assert_eq!(value["commit"], Value::Null);
        assert_eq!(value["detached"], false);
    }

    #[test]
    fn invalid_configuration_cannot_replace_working_configuration() {
        let temp = TempDir::new();
        let file = temp.0.join("config.json");
        let mut store = Store::load(&file, None).unwrap();
        store.configure_global(json!({"prefix":"C-a"})).unwrap();
        assert!(
            store
                .configure_global(json!({"prefix":"unsupported"}))
                .is_err()
        );
        assert_eq!(store.config.prefix, "C-a");
        let id = store.unsaved_id();
        assert!(
            store
                .configure(
                    &id,
                    json!({"hooks":[{"source":"printf bad","timeout_ms":0}]})
                )
                .is_err()
        );
        assert!(
            store
                .configure(
                    &id,
                    json!({"hooks":[{"source":"printf overflow","timeout_ms":u64::MAX}]})
                )
                .is_err()
        );
        assert!(store.projects[&id].hooks.is_empty());
        assert!(
            store
                .configure(&id, json!({"main_terminal":"persist-me"}))
                .is_err()
        );
        assert_eq!(Store::load(&file, None).unwrap().config.prefix, "C-a");
    }

    #[test]
    fn hooks_capture_stderr_bound_output_and_report_spawn_failures() {
        let temp = TempDir::new();
        let hook = Hook {
            source: "printf diagnostic >&2; dd if=/dev/zero bs=1200000 count=1 2>/dev/null".into(),
            timeout_ms: 2_000,
        };
        let result = run_hooks(&[hook], &temp.0, &BTreeMap::new());
        assert_eq!(result[0]["stderr"], "diagnostic");
        assert_eq!(result[0]["output_truncated"], true);
        assert!(result[0]["stdout"].as_str().unwrap().len() <= OUTPUT_LIMIT);
        let failed = run_hooks(
            &[Hook {
                source: "printf unavailable".into(),
                timeout_ms: 500,
            }],
            &temp.0.join("missing"),
            &BTreeMap::new(),
        );
        assert_eq!(failed[0]["ok"], false);
        assert!(!failed[0]["error"].is_null());
    }

    #[test]
    fn git_refresh_is_async_and_kills_a_hung_git_hook() {
        use std::os::unix::fs::PermissionsExt;
        let temp = TempDir::new();
        repository(&temp.0);
        let script = temp.0.join("fsmonitor-hook");
        fs::write(
            &script,
            "#!/bin/sh\nsleep 30 & child=$!\nprintf '%s' \"$child\" > git-descendant\nwait\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        git(
            &temp.0,
            &["config", "core.fsmonitor", script.to_str().unwrap()],
        );
        let locations: Vec<_> = (0..(GIT_CACHE_LIMIT + 32))
            .map(|index| {
                let path = temp.0.join(format!("pending-location-{index}"));
                fs::create_dir(&path).unwrap();
                path
            })
            .collect();
        let cache = GitCache::new();
        let started = Instant::now();
        cache.refresh(&temp.0);
        let initial = cache.get(&temp.0);
        assert!(started.elapsed() < Duration::from_millis(250));
        assert!(initial["refreshed_at_ms"].is_null());
        for location in &locations {
            cache.refresh(location);
        }
        assert_eq!(cache.get(&temp.0)["refreshing"], true);
        let result = metadata(&cache, &temp.0);
        assert!(result["error"].as_str().unwrap().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(8));
        let pid: i32 = fs::read_to_string(temp.0.join("git-descendant"))
            .unwrap()
            .parse()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if unsafe { libc::kill(pid, 0) } != 0 {
                break;
            }
            let zombie = fs::read_to_string(format!("/proc/{pid}/stat"))
                .ok()
                .and_then(|stat| {
                    stat.rsplit_once(") ")
                        .map(|(_, tail)| tail.starts_with('Z'))
                })
                .unwrap_or(false);
            if zombie {
                break;
            }
            assert!(Instant::now() < deadline, "Git descendant survived timeout");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn git_preserves_renames_binary_files_and_unusual_untracked_paths() {
        let temp = TempDir::new();
        repository(&temp.0);
        git(&temp.0, &["mv", "tracked", "renamed file"]);
        fs::write(temp.0.join("binary"), [0_u8, 1, 2, 3]).unwrap();
        git(&temp.0, &["add", "binary"]);
        fs::write(temp.0.join("line\nbreak"), "untracked\n").unwrap();
        let result = metadata(&GitCache::new(), &temp.0);
        assert_eq!(result["error"], Value::Null);
        assert_eq!(result["staged"]["count"], 2);
        assert_eq!(result["staged"]["binary"], 1);
        let files = result["staged"]["files"].as_array().unwrap();
        let renamed = files
            .iter()
            .find(|file| file["path"] == "renamed file")
            .unwrap();
        assert_eq!(renamed["original_path"], "tracked");
        assert_eq!(renamed["status"], "R");
        assert_eq!(result["untracked"]["files"][0], "line\nbreak");
    }
}

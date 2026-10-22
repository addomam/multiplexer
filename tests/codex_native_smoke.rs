//! Opt-in, billable, real-provider checks. Never run these with a wildcard filter.
//! Each selected test deliberately fails when a required provider/native seam is
//! unavailable. A provider receipt is neither task acceptance nor proof of a winner.
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

#[path = "codex_native_smoke/capture.rs"]
mod capture;
#[path = "codex_native_smoke/complete.rs"]
mod complete;

const DAEMON: Duration = Duration::from_secs(15);
const BACKEND: Duration = Duration::from_secs(30);
const RESPONSE: Duration = Duration::from_secs(180);
// Leave time for bounded owned-tree teardown within the 1800-second ceiling.
const WATCHDOG: Duration = Duration::from_secs(1650);
const POLL: Duration = Duration::from_millis(40);
static INTERRUPTED: AtomicBool = AtomicBool::new(false);
thread_local! {
    static PHASE_DEADLINE: Cell<Option<Instant>> = const { Cell::new(None) };
}

extern "C" fn interrupt_handler(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

fn selected(name: &str) -> (String, PathBuf) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut filters = 0;
    let mut ignored = false;
    let mut exact = false;
    let mut serial = false;
    let mut at = 0;
    while at < args.len() {
        match args[at].as_str() {
            value if value == name => filters += 1,
            "--ignored" if !ignored => ignored = true,
            "--exact" if !exact => exact = true,
            "--nocapture" => {}
            "--test-threads=1" if !serial => serial = true,
            "--test-threads" if !serial => {
                at += 1;
                assert_eq!(
                    args.get(at).map(String::as_str),
                    Some("1"),
                    "live checks must be serial"
                );
                serial = true;
            }
            _ => panic!(
                "live check requires only its own exact filter, --ignored --exact --nocapture --test-threads=1"
            ),
        }
        at += 1;
    }
    assert!(
        filters == 1 && ignored && exact && serial,
        "select exactly {name} with --ignored --exact --nocapture --test-threads=1"
    );
    assert_eq!(
        std::env::var("AMUX_RUN_REAL_CODEX").as_deref(),
        Ok("1"),
        "missing explicit real-provider opt-in"
    );
    assert_eq!(
        std::env::var("AMUX_CODEX_LIVE_APPROVED").as_deref(),
        Ok("1"),
        "separate live-run approval is required"
    );
    let model = std::env::var("AMUX_CODEX_MODEL").expect("AMUX_CODEX_MODEL is required");
    assert!(
        !model.trim().is_empty(),
        "AMUX_CODEX_MODEL must be nonempty"
    );
    let source = PathBuf::from(
        std::env::var_os("AMUX_CODEX_AUTH_SOURCE")
            .expect("AMUX_CODEX_AUTH_SOURCE must explicitly approve a source auth.json"),
    );
    let metadata =
        fs::symlink_metadata(&source).expect("approved authentication source is missing");
    assert!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "auth source must be a regular, non-symlink file"
    );
    assert!(
        metadata.len() > 0 && metadata.len() <= 1024 * 1024,
        "invalid authentication source size"
    );
    (model, source)
}

fn private_dir(path: &Path) {
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn private_file(path: &Path) -> File {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap()
}

fn exchange(socket: &Path, mut request: Value, timeout: Duration) -> io::Result<Value> {
    let remaining = PHASE_DEADLINE.with(|phase| {
        phase
            .get()
            .map(|end| end.saturating_duration_since(Instant::now()))
    });
    let timeout = remaining.map_or(timeout, |remaining| timeout.min(remaining));
    if timeout.is_zero() {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "phase deadline elapsed",
        ));
    }
    let deadline = Instant::now() + timeout;
    request["version"] = json!(1);
    let mut socket = UnixStream::connect(socket)?;
    socket.set_read_timeout(Some(timeout))?;
    socket.set_write_timeout(Some(timeout.min(Duration::from_secs(2))))?;
    serde_json::to_writer(&mut socket, &request).map_err(io::Error::other)?;
    socket.write_all(b"\n")?;
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "phase deadline elapsed while writing",
        ));
    }
    socket.set_read_timeout(Some(remaining))?;
    let mut line = String::new();
    BufReader::new(socket)
        .take(8 * 1024 * 1024)
        .read_line(&mut line)?;
    if !line.ends_with('\n') {
        return Err(io::Error::other("incomplete daemon reply"));
    }
    let response: Value = serde_json::from_str(&line).map_err(io::Error::other)?;
    if response["version"] != 1 {
        return Err(io::Error::other("invalid daemon reply version"));
    }
    Ok(response)
}

struct SavedTerminal {
    master: File,
    original: libc::termios,
}

struct Owned {
    root: PathBuf,
    socket: PathBuf,
    children: Vec<Child>,
    known: HashMap<i32, String>,
    groups: HashSet<i32>,
    readers: Vec<capture::Drainer>,
    terminals: Vec<SavedTerminal>,
    cleaned: bool,
}

impl Owned {
    fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = Command::new(program);
        command.env_clear();
        for key in [
            "PATH",
            "LANG",
            "LC_ALL",
            "SSL_CERT_FILE",
            "SSL_CERT_DIR",
            "HTTPS_PROXY",
            "HTTP_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            // Explicit provider override for an owned local endpoint or an
            // authorized compatible provider; never copy global config.toml.
            "OPENAI_BASE_URL",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command
            .env("HOME", self.root.join("home"))
            .env("ZDOTDIR", self.root.join("startup"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("XDG_RUNTIME_DIR", self.root.join("runtime"))
            .env("AMUX_RUNTIME_DIR", self.root.join("runtime"))
            .env("AMUX_SOCKET", &self.socket)
            .env("CODEX_HOME", self.root.join("codex"))
            .env("TMPDIR", self.root.join("tmp"))
            .env("TERM", "xterm-256color")
            .current_dir(self.root.join("workspace"));
        command
    }

    fn register(&mut self, child: Child) -> u32 {
        let pid = child.id();
        self.known.insert(pid as i32, String::new());
        self.children.push(child);
        pid
    }

    // ps is used only in the explicitly approved runtime, never at test discovery.
    // Capture ancestry, not command lines, so evidence cannot leak auth/arguments.
    fn discover(&mut self) -> io::Result<()> {
        let output_path = self.root.join("process-table");
        let output = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&output_path)?;
        let mut child = Command::new("/bin/ps")
            .args(["-axo", "pid=,ppid=,pgid=,lstart="])
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(output)
            .stderr(Stdio::null())
            .spawn()?;
        let deadline = Instant::now() + Duration::from_secs(1);
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                if !bounded_reap(&mut child, Duration::from_secs(1))? {
                    self.register(child);
                }
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "owned ancestry collection deadline",
                ));
            }
            thread::sleep(Duration::from_millis(5));
        };
        if !status.success() {
            return Err(io::Error::other("cannot inspect owned process ancestry"));
        }
        let rows: Vec<(i32, i32, i32, String)> = fs::read_to_string(output_path)?
            .lines()
            .filter_map(|line| {
                let mut values = line.split_whitespace();
                let pid = values.next()?.parse().ok()?;
                let parent = values.next()?.parse().ok()?;
                let group = values.next()?.parse().ok()?;
                let birth = values.collect::<Vec<_>>().join(" ");
                (!birth.is_empty()).then_some((pid, parent, group, birth))
            })
            .collect();
        // Retain identity as well as PID; an exited child can never authorize
        // signalling an unrelated process which later reuses its number.
        for child in &mut self.children {
            if child.try_wait()?.is_some()
                && self
                    .known
                    .get(&(child.id() as i32))
                    .is_some_and(String::is_empty)
            {
                self.known.remove(&(child.id() as i32));
            }
        }
        self.known.retain(|pid, birth| {
            rows.iter()
                .any(|row| row.0 == *pid && (birth.is_empty() || *birth == row.3))
        });
        for (pid, _, _, birth) in &rows {
            if let Some(known_birth) = self.known.get_mut(pid) {
                if known_birth.is_empty() {
                    known_birth.clone_from(birth);
                }
            }
        }
        loop {
            let before = self.known.len();
            for (pid, parent, _, birth) in &rows {
                if self.known.contains_key(parent) {
                    self.known.entry(*pid).or_insert_with(|| birth.clone());
                }
            }
            if before == self.known.len() {
                break;
            }
        }
        self.groups.clear();
        for (pid, _, group, _) in &rows {
            // Every current member must belong to our recorded ancestry before
            // a process-group signal is allowed, including leaderless groups.
            if self.known.contains_key(pid)
                && *group > 1
                && *group != unsafe { libc::getpgrp() }
                && rows
                    .iter()
                    .all(|row| row.2 != *group || self.known.contains_key(&row.0))
            {
                self.groups.insert(*group);
            }
        }
        Ok(())
    }

    fn cleanup(&mut self) -> Vec<&'static str> {
        if self.cleaned {
            return Vec::new();
        }
        self.cleaned = true;
        let mut errors = Vec::new();
        if self.discover().is_err() {
            errors.push("final owned-tree enumeration failed");
        }
        let _ = exchange(
            &self.socket,
            json!({"op":"shutdown"}),
            Duration::from_millis(500),
        );
        let graceful = Instant::now() + Duration::from_secs(3);
        while Instant::now() < graceful {
            for child in &mut self.children {
                let _ = child.try_wait();
            }
            if self.groups.iter().all(|group| !group_alive(*group)) {
                break;
            }
            thread::sleep(POLL);
        }
        if self.discover().is_err() {
            errors.push("owned-tree refresh before termination failed");
        }
        for signal in [libc::SIGTERM, libc::SIGKILL] {
            if self.discover().is_err() {
                errors.push("owned identity refresh before signal failed");
            }
            for &group in &self.groups {
                if group_alive(group) {
                    unsafe {
                        libc::kill(-group, signal);
                    }
                }
            }
            // Descendants which did not create a private group still belong to
            // this tree. Never signal the test runner's/global process group.
            for &pid in self.known.keys() {
                let group = unsafe { libc::getpgid(pid) };
                if group > 0 && !self.groups.contains(&group) {
                    unsafe {
                        libc::kill(pid, signal);
                    }
                }
            }
            let end = Instant::now() + Duration::from_secs(2);
            while Instant::now() < end {
                for child in &mut self.children {
                    let _ = child.try_wait();
                }
                if self.groups.iter().all(|group| !group_alive(*group)) {
                    break;
                }
                thread::sleep(POLL);
            }
        }
        for child in &mut self.children {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            if !bounded_reap(child, Duration::from_secs(2)).unwrap_or(false) {
                errors.push("owned child reap deadline");
            }
        }
        if self.groups.iter().any(|group| group_alive(*group)) {
            errors.push("owned process group survived teardown");
        }
        if self
            .known
            .keys()
            .any(|pid| unsafe { libc::kill(*pid, 0) == 0 })
        {
            errors.push("owned descendant survived teardown");
        }
        if !errors.is_empty() {
            // Identity-only diagnostics permit safe survivor cleanup without
            // retaining command lines, credentials, or the private auth root.
            eprintln!(
                "owned cleanup identities: root={} known={:?} groups={:?}",
                self.root.display(),
                self.known,
                self.groups
            );
        }
        for reader in &mut self.readers {
            if !reader.finish() {
                errors.push("owned outer-PTY reader join deadline");
            }
        }
        self.readers.clear();
        for terminal in &self.terminals {
            if unsafe {
                libc::tcsetattr(
                    terminal.master.as_raw_fd(),
                    libc::TCSANOW,
                    &terminal.original,
                )
            } != 0
            {
                errors.push("private controlling terminal restoration failed");
            }
            let mut restored: libc::termios = unsafe { std::mem::zeroed() };
            if unsafe { libc::tcgetattr(terminal.master.as_raw_fd(), &mut restored) } != 0
                || restored.c_iflag != terminal.original.c_iflag
                || restored.c_oflag != terminal.original.c_oflag
                || restored.c_cflag != terminal.original.c_cflag
                || restored.c_lflag != terminal.original.c_lflag
                || restored.c_cc != terminal.original.c_cc
                || unsafe { libc::cfgetispeed(&restored) }
                    != unsafe { libc::cfgetispeed(&terminal.original) }
                || unsafe { libc::cfgetospeed(&restored) }
                    != unsafe { libc::cfgetospeed(&terminal.original) }
            {
                errors.push("private terminal modes were not restored");
            }
        }
        self.terminals.clear();
        if self.socket.exists() && fs::remove_file(&self.socket).is_err() {
            errors.push("owned daemon socket removal failed");
        }
        if fs::remove_dir_all(&self.root).is_err() {
            errors.push("private auth/schema/rollout/root removal failed");
        }
        if self.root.exists() || self.socket.exists() {
            errors.push("owned filesystem resources survived teardown");
        }
        errors
    }
}

fn group_alive(group: i32) -> bool {
    unsafe {
        libc::kill(-group, 0) == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}
fn bounded_reap(child: &mut Child, budget: Duration) -> io::Result<bool> {
    let deadline = Instant::now() + budget;
    loop {
        if child.try_wait()?.is_some() {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        thread::sleep(POLL);
    }
}

struct Guard {
    owned: Arc<Mutex<Owned>>,
    cancel: Option<mpsc::Sender<()>>,
    watchdog: Option<thread::JoinHandle<()>>,
    previous_signal: libc::sigaction,
}

impl Guard {
    fn new() -> Self {
        // Keep nested owned Unix socket paths below Darwin's sockaddr_un limit.
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        let root = fs::canonicalize("/tmp")
            .unwrap()
            .join(format!("acx-{}", &nonce[..16]));
        private_dir(&root);
        let owned = Arc::new(Mutex::new(Owned {
            socket: root.join("api.sock"),
            root,
            children: Vec::new(),
            known: HashMap::new(),
            groups: HashSet::new(),
            readers: Vec::new(),
            terminals: Vec::new(),
            cleaned: false,
        }));
        INTERRUPTED.store(false, Ordering::SeqCst);
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        let mut previous_signal: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = interrupt_handler as *const () as usize;
        unsafe {
            libc::sigemptyset(&mut action.sa_mask);
        }
        assert_eq!(
            unsafe { libc::sigaction(libc::SIGINT, &action, &mut previous_signal) },
            0
        );
        let (sender, receiver) = mpsc::channel();
        let resources = owned.clone();
        let watchdog = thread::spawn(move || {
            let end = Instant::now() + WATCHDOG;
            loop {
                match receiver.recv_timeout(Duration::from_millis(200)) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                if INTERRUPTED.load(Ordering::SeqCst) || Instant::now() >= end {
                    let errors = resources.lock().cleanup();
                    eprintln!(
                        "real Codex check aborted by SIGINT/watchdog; owned cleanup errors: {errors:?}"
                    );
                    std::process::exit(1);
                }
                let discovery = resources.lock().discover();
                if let Err(error) = discovery {
                    eprintln!("owned process monitoring failed: {}", error.kind());
                    let errors = resources.lock().cleanup();
                    eprintln!("owned cleanup errors: {errors:?}");
                    std::process::exit(1);
                }
            }
        });
        Self {
            owned,
            cancel: Some(sender),
            watchdog: Some(watchdog),
            previous_signal,
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
        if let Some(watchdog) = self.watchdog.take() {
            let _ = watchdog.join();
        }
        let mut errors = self.owned.lock().cleanup();
        if INTERRUPTED.load(Ordering::SeqCst) {
            errors.push("live run interrupted during cleanup");
        }
        unsafe {
            libc::sigaction(libc::SIGINT, &self.previous_signal, std::ptr::null_mut());
        }
        if !errors.is_empty() {
            if thread::panicking() {
                eprintln!("owned cleanup failed: {errors:?}");
            } else {
                panic!("owned cleanup failed: {errors:?}");
            }
        }
    }
}

struct Attachment {
    master: File,
    pid: u32,
    capture: Arc<Mutex<capture::Output>>,
}

impl Attachment {
    fn drain(&mut self) {
        let output = self.capture.lock();
        assert!(output.error.is_none(), "{:?}", output.error);
    }
}

struct Live {
    // Close the read-only evidence connection before Guard tears down the backend.
    history_socket: Option<tungstenite::WebSocket<UnixStream>>,
    guard: Guard,
    root: PathBuf,
    socket: PathBuf,
    terminal: String,
    instance: String,
    target: Value,
    controller: Value,
    control: Value,
    starts: usize,
    viewers: Vec<Attachment>,
    evidence: PathBuf,
    captures: usize,
    private_refs: Vec<String>,
}

impl Live {
    fn open(name: &str) -> Self {
        let (model, auth_source) = selected(name);
        let guard = Guard::new();
        let (root, socket) = {
            let resources = guard.owned.lock();
            (resources.root.clone(), resources.socket.clone())
        };
        for directory in [
            "home",
            "startup",
            "config",
            "cache",
            "runtime",
            "codex",
            "workspace",
            "outside",
            "tmp",
            "projects",
        ] {
            private_dir(&root.join(directory));
        }
        fs::write(root.join("startup/.zshrc"), "PS1='AMUX_PRIVATE_LIVE> '\n").unwrap();
        let workspace = root.join("workspace");
        let auth = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(auth_source)
            .expect("cannot open explicitly approved auth source");
        assert!(
            auth.metadata().unwrap().is_file(),
            "auth source changed type"
        );
        let mut destination = private_file(&root.join("codex/auth.json"));
        io::copy(&mut auth.take(1024 * 1024 + 1), &mut destination).unwrap();
        assert!(
            destination.metadata().unwrap().len() <= 1024 * 1024,
            "auth source changed size"
        );
        drop(destination);
        let version_path = root.join("codex-version");
        let version_pid = {
            let mut resources = guard.owned.lock();
            let child = resources
                .command("codex")
                .arg("--version")
                .process_group(0)
                .stdin(Stdio::null())
                .stdout(private_file(&version_path))
                .stderr(Stdio::null())
                .spawn()
                .expect("real codex executable is required on PATH");
            resources.register(child)
        };
        wait_child(&guard, version_pid, BACKEND, true);
        assert_eq!(
            fs::read_to_string(version_path).unwrap().trim(),
            "codex-cli 0.160.0",
            "installed Codex must match the bound schema"
        );
        {
            let mut resources = guard.owned.lock();
            let log = private_file(&root.join("daemon.log"));
            let child = resources
                .command(env!("CARGO_BIN_EXE_amux"))
                .arg("--socket")
                .arg(&socket)
                .arg("daemon")
                .arg("--projects-dir")
                .arg(root.join("projects"))
                .process_group(0)
                .stdin(Stdio::null())
                .stdout(log.try_clone().unwrap())
                .stderr(log)
                .spawn()
                .unwrap();
            resources.register(child);
        }
        until("private real daemon startup", DAEMON, || {
            exchange(
                &socket,
                json!({"op":"snapshot"}),
                Duration::from_millis(500),
            )
            .ok()
            .filter(|response| response["ok"] == true)
        });
        call(&socket, json!({"op":"config","config":{"prefix":"Ctrl-a"}}));
        let controller = call(
            &socket,
            json!({"op":"controller-create","role":"external","label":name}),
        );
        let opened = call(
            &socket,
            json!({"op":"agent-open","controller":controller["controller"],
            "name":name,"cwd":workspace,"model":model}),
        );
        let terminal = opened["terminal"]["id"]
            .as_str()
            .expect("agent-open must return ordinary terminal object")
            .to_owned();
        let instance = opened["instance_id"].as_str().unwrap().to_owned();
        assert_eq!(opened["control"]["kind"], "control");
        let control = opened["control"]["control"].clone();
        assert_eq!(control["kind"], "control");
        let evidence_parent =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/e03-codex-native");
        fs::create_dir_all(&evidence_parent).unwrap();
        let evidence = evidence_parent.join(format!("{name}-{}", uuid::Uuid::new_v4()));
        private_dir(&evidence);
        let mut live = Self {
            history_socket: None,
            guard,
            root,
            socket,
            terminal,
            instance,
            target: Value::Null,
            controller: controller["controller"].clone(),
            control,
            starts: 0,
            viewers: Vec::new(),
            evidence,
            captures: 0,
            private_refs: Vec::new(),
        };
        live.attach();
        let mut trusted = false;
        let state = until("native-first authoritative discovery", BACKEND, || {
            live.drain();
            let state = live.inspect();
            if state["lifecycle"]["connection"] == "lost" {
                live.capture("native-first discovery unavailable");
                panic!(
                    "native-first discovery lost its supported provider seam: {}",
                    state["problem"]
                );
            }
            let screen = call(
                &live.socket,
                json!({"op":"screen","terminal":live.terminal,"offset":0,"limit":45}),
            );
            let rendered = screen["lines"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n");
            if !trusted
                && rendered.contains("Trust this folder?")
                && rendered.contains("1. Trust and continue")
            {
                assert!(
                    rendered.contains(workspace.to_str().unwrap()),
                    "refusing trust for any directory except the owned workspace"
                );
                live.capture("actual owned-workspace trust dialog");
                call(
                    &live.socket,
                    json!({"op":"input","terminal":live.terminal,"control":live.control,"text":"1\r"}),
                );
                trusted = true;
            }
            if trusted
                && !rendered.contains("Trust this folder?")
                && rendered.contains("/model")
                && rendered.contains('›')
                && state["target"]["session_id"].is_string()
                && state["lifecycle"]["session"] == "ready"
                && state["lifecycle"]["connection"] == "ready"
                && state["lifecycle"]["frontend"] == "attached"
            {
                return Some(state);
            }
            None
        });
        live.target = state["target"].clone();
        assert_eq!(live.target["terminal"], live.terminal);
        assert_eq!(live.target["instance_id"], live.instance);
        assert_eq!(
            state["lifecycle"]["activity"], "idle",
            "fresh frontend must not run a hidden setup turn"
        );
        assert!(
            state["lifecycle"]["turn_id"].is_null(),
            "hidden setup turn detected"
        );
        assert_eq!(state["calls"], json!([]), "fresh discovery must not prompt");
        live.capture("native trust cleared and actual composer ready");
        live
    }

    fn inspect(&self) -> Value {
        call(
            &self.socket,
            json!({"op":"agent-inspect","terminal":self.terminal,"instance_id":self.instance}),
        )
    }

    fn wait_state(
        &mut self,
        label: &str,
        budget: Duration,
        mut predicate: impl FnMut(&Value) -> bool,
    ) -> Value {
        let mut last_state = Value::Null;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            until(label, budget, || {
                self.drain();
                last_state = self.inspect();
                let state = &last_state;
                assert_ne!(
                    state["lifecycle"]["backend"], "exited",
                    "real backend exited during {label}"
                );
                assert_ne!(
                    state["lifecycle"]["frontend"], "exited",
                    "real frontend exited during {label}"
                );
                assert_ne!(
                    state["lifecycle"]["connection"], "lost",
                    "real protocol connection lost during {label}; reason={}",
                    state["problem"]
                );
                predicate(state).then(|| last_state.take())
            })
        }));
        match result {
            Ok(state) => state,
            Err(failure) => {
                self.capture("failed live state wait");
                eprintln!(
                    "failed live state phase={label:?} lifecycle={} problem={}",
                    last_state["lifecycle"], last_state["problem"]
                );
                for pending in last_state["pending"].as_array().into_iter().flatten() {
                    eprintln!(
                        "pending live request kind={} state={}",
                        pending["kind"], pending["state"]
                    );
                }
                for call in last_state["calls"].as_array().into_iter().flatten() {
                    eprintln!(
                        "retained live call op={} state={} reason={}",
                        call["op"], call["state"], call["reason"]
                    );
                }
                std::panic::resume_unwind(failure)
            }
        }
    }

    fn attach(&mut self) {
        let mut master = -1;
        let mut slave = -1;
        let mut size = libc::winsize {
            ws_row: 45,
            ws_col: 160,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::addr_of_mut!(size),
                )
            },
            0
        );
        let master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        let mut original: libc::termios = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe { libc::tcgetattr(slave.as_raw_fd(), &mut original) },
            0
        );
        for descriptor in [master.as_raw_fd(), slave.as_raw_fd()] {
            assert_eq!(
                unsafe { libc::fcntl(descriptor, libc::F_SETFD, libc::FD_CLOEXEC) },
                0
            );
        }
        let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
        assert!(flags >= 0);
        assert_eq!(
            unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
            0
        );
        let pid = {
            let mut resources = self.guard.owned.lock();
            let mut command = resources.command(env!("CARGO_BIN_EXE_amux"));
            command
                .arg("--socket")
                .arg(&self.socket)
                .arg("attach")
                .arg(&self.terminal)
                .stdin(slave.try_clone().unwrap())
                .stdout(slave.try_clone().unwrap())
                .stderr(slave.try_clone().unwrap());
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            // Darwin revokes the controlling slave when its session exits.
            // The retained master still supports mode restoration and queries.
            resources.terminals.push(SavedTerminal {
                master: master.try_clone().unwrap(),
                original,
            });
            resources.register(
                command
                    .spawn()
                    .expect("real controlling-PTY amux attachment failed"),
            )
        };
        let (capture, reader) = capture::Drainer::start(master.try_clone().unwrap());
        self.guard.owned.lock().readers.push(reader);
        self.viewers.push(Attachment {
            master,
            pid,
            capture,
        });
        until("actual native attachment output", DAEMON, || {
            self.drain();
            self.viewers
                .last()
                .filter(|viewer| !viewer.capture.lock().raw.is_empty())
                .map(|_| ())
        });
    }

    fn drain(&mut self) {
        for viewer in &mut self.viewers {
            viewer.drain();
        }
    }

    fn native_contains(&mut self, label: &str, text: &str) {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            until(label, RESPONSE, || {
                self.drain();
                self.viewers
                    .iter()
                    .all(|viewer| viewer.capture.lock().screen.contains(text))
                    .then_some(())
            });
        }));
        self.capture(label);
        if let Err(failure) = result {
            std::panic::resume_unwind(failure);
        }
    }
    fn native_mode(&mut self, expected: &str) {
        let plan = match expected {
            "Plan mode" => true,
            "Default" => false,
            _ => panic!("unsupported fixture mode"),
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if plan {
                call(
                    &self.socket,
                    json!({"op":"input","terminal":self.terminal,
                    "control":self.control,"text":"/plan"}),
                );
                until(
                    "native slash command rendered before Enter",
                    BACKEND,
                    || {
                        self.drain();
                        self.viewers
                            .iter()
                            .all(|viewer| viewer.capture.lock().screen.contains("/plan"))
                            .then_some(())
                    },
                );
                call(
                    &self.socket,
                    json!({"op":"input","terminal":self.terminal,
                    "control":self.control,"key":"enter"}),
                );
            } else {
                // Codex's enhanced keyboard protocol represents Shift-Tab as
                // CSI 9;2u. Legacy CSI Z does not cycle its current mode.
                call(
                    &self.socket,
                    json!({"op":"input","terminal":self.terminal,
                    "control":self.control,"bytes":[27,91,57,59,50,117]}),
                );
            }
            until(
                "actual native mode transition without a setup turn",
                BACKEND,
                || {
                    self.drain();
                    self.viewers
                        .iter()
                        .all(|viewer| {
                            let mut output = viewer.capture.lock();
                            let plan_badge = output.screen.contains("Plan mode (");
                            if plan {
                                plan_badge
                            } else {
                                !plan_badge && output.screen.contains("for Default mode.")
                            }
                        })
                        .then_some(())
                },
            );
        }));
        self.capture("actual native mode transition");
        if let Err(failure) = result {
            std::panic::resume_unwind(failure);
        }
    }

    fn capture(&mut self, label: &str) {
        self.drain();
        complete::retain_native(self, label);
    }

    fn mutation(&self, op: &str, fields: Value) -> Value {
        let mut request = fields;
        request["op"] = json!(op);
        request["target"] = self.target.clone();
        request["control"] = self.control.clone();
        let receipt = call(&self.socket, request);
        assert!(
            receipt["call_id"].is_string(),
            "mutation must return retained call receipt"
        );
        assert_eq!(receipt["op"], op);
        assert!(
            matches!(
                receipt["state"].as_str(),
                Some("queued" | "forwarded" | "applied" | "unknown")
            ),
            "mutation rejected"
        );
        receipt
    }

    fn start(&mut self, prompt: &str) -> (String, Value) {
        self.check_turn_budget();
        assert!(self.starts < 6, "six meaningful-turn budget exhausted");
        assert_eq!(
            self.inspect()["lifecycle"]["activity"],
            "idle",
            "new turn must be deliberate and nonconflicting"
        );
        self.starts += 1;
        let receipt = self.mutation("agent-prompt", json!({"intent":"start","text":prompt}));
        let id = receipt["call_id"].as_str().unwrap().to_owned();
        let state = self.wait_state("real turn identity", RESPONSE, |state| {
            state["calls"].as_array().is_some_and(|calls| {
                calls
                    .iter()
                    .any(|call| call["call_id"] == id && call["turn_id"].is_string())
            })
        });
        let row = state["calls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|call| call["call_id"] == id)
            .unwrap();
        (row["turn_id"].as_str().unwrap().to_owned(), receipt)
    }

    fn native_start(&mut self, prompt: &str) -> String {
        self.check_turn_budget();
        assert!(self.starts < 6, "six meaningful-turn budget exhausted");
        assert_eq!(self.inspect()["lifecycle"]["activity"], "idle");
        call(
            &self.socket,
            json!({"op":"input","terminal":self.terminal,
            "control":self.control,"text":prompt,"paste":true}),
        );
        let prefix = prompt
            .char_indices()
            .nth(32)
            .map_or(prompt, |(end, _)| &prompt[..end]);
        self.native_contains("actual native composer before prompt submission", prefix);
        self.starts += 1;
        call(
            &self.socket,
            json!({"op":"input","terminal":self.terminal,
            "control":self.control,"key":"enter"}),
        );
        let state = self.wait_state("real native-submitted turn identity", RESPONSE, |state| {
            state["lifecycle"]["turn_id"].is_string()
        });
        state["lifecycle"]["turn_id"].as_str().unwrap().to_owned()
    }

    fn complete(&mut self, turn: &str) -> Value {
        self.wait_state("authoritative same-turn completion", RESPONSE, |state| {
            state["turn_outcomes"][turn] == "completed" && state["lifecycle"]["activity"] == "idle"
        })
    }

    fn pending(&mut self, turn: &str, kind: &str) -> Value {
        let state = self.wait_state(
            "genuine provider request (no synthetic fallback)",
            RESPONSE,
            |state| {
                state["pending"].as_array().is_some_and(|pending| {
                    pending.iter().any(|request| {
                        request["turn_id"] == turn
                            && request["kind"] == kind
                            && request["state"] == "pending"
                    })
                })
            },
        );
        let pending = state["pending"]
            .as_array()
            .unwrap()
            .iter()
            .find(|request| {
                request["turn_id"] == turn
                    && request["kind"] == kind
                    && request["state"] == "pending"
            })
            .unwrap()
            .clone();
        assert_eq!(pending["session_id"], self.target["session_id"]);
        assert!(pending["item_id"].is_string());
        assert!(
            pending["request_id"].is_string() || pending["request_id"].is_i64(),
            "provider request ID type must be preserved"
        );
        self.check_turn_budget();
        if kind == "command" {
            for decision in pending["details"]["availableDecisions"]
                .as_array()
                .into_iter()
                .flatten()
            {
                let name = match decision.as_str() {
                    Some("accept") => "accept",
                    Some("acceptForSession") => "acceptForSession",
                    Some("decline") => "decline",
                    Some("cancel") => "cancel",
                    _ => "other",
                };
                eprintln!("genuine command approval offers={name}");
            }
        }
        self.capture("genuine pending request");
        pending
    }

    fn respond(&self, op: &str, pending: &Value, mut fields: Value) -> Value {
        for key in ["turn_id", "item_id", "request_id"] {
            fields[key] = pending[key].clone();
        }
        self.mutation(op, fields)
    }

    fn resolved(&mut self, pending: &Value, receipt: &Value) {
        let state = self.wait_state("exact pending identity resolution", RESPONSE, |state| {
            state["pending"].as_array().is_some_and(|requests| {
                !requests
                    .iter()
                    .any(|request| same_pending(request, pending))
            })
        });
        let row = state["calls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|call| call["call_id"] == receipt["call_id"])
            .unwrap();
        assert!(
            matches!(
                row["state"].as_str(),
                Some("forwarded" | "unknown" | "applied")
            ),
            "response was not retained truthfully"
        );
        // serverRequest/resolved alone cannot identify a winning contender.
        // Unknown is expected unless separate authoritative evidence exists.
        if row["state"] == "unknown" {
            eprintln!("request resolved; response winner remains conservatively unknown");
        }
        let mut stale = json!({"op":receipt["op"],"target":self.target,"control":self.control,
            "turn_id":pending["turn_id"],"item_id":pending["item_id"],"request_id":pending["request_id"]});
        if receipt["op"] == "agent-answer" {
            stale["answers"] = json!({});
        } else {
            stale["decision"] = json!("allow");
        }
        let response = exchange(&self.socket, stale, Duration::from_secs(2)).unwrap();
        assert_eq!(
            response["ok"], false,
            "a resolved request must never be answered again"
        );
    }

    fn detach(&mut self) {
        call(&self.socket, json!({"op":"detach"}));
        for viewer in &mut self.viewers {
            viewer.drain();
            wait_child(&self.guard, viewer.pid, DAEMON, true);
        }
        let state = self.inspect();
        assert_eq!(
            state["target"], self.target,
            "detach changed same-instance identity"
        );
        assert_eq!(state["lifecycle"]["backend"], "running");
        assert_eq!(
            state["lifecycle"]["frontend"], "attached",
            "viewer detach must not stop frontend"
        );
        self.viewers.clear();
    }

    fn assistant_contains(&mut self, turn: &str, expected: &[&str]) {
        let mut observed = (0, 0, 0);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            until(
                "authoritative assistant continuation, not echoed user prompt",
                RESPONSE,
                || {
                    let turns = self.history();
                    let current = turns.iter().find(|row| row["id"] == turn)?;
                    let items = current["items"]
                        .as_array()
                        .expect("full turn items omitted");
                    let messages = items
                        .iter()
                        .filter(|item| item["type"] == "agentMessage")
                        .count();
                    let plans = items.iter().filter(|item| item["type"] == "plan").count();
                    // The installed protocol defines completed PlanThreadItem.text
                    // as authoritative model output, distinct from user messages.
                    let matched = expected
                        .iter()
                        .filter(|text| {
                            items.iter().any(|item| {
                                matches!(item["type"].as_str(), Some("agentMessage" | "plan"))
                                    && item["text"]
                                        .as_str()
                                        .is_some_and(|message| message.contains(*text))
                            })
                        })
                        .count();
                    observed = (messages, plans, matched);
                    (matched == expected.len()).then_some(())
                },
            );
        }));
        self.capture("authoritative assistant continuation evidence");
        if let Err(failure) = result {
            eprintln!(
                "authoritative output evidence: messages={} plans={} matched_markers={}/{}",
                observed.0,
                observed.1,
                observed.2,
                expected.len()
            );
            std::panic::resume_unwind(failure);
        }
    }

    fn history(&mut self) -> Vec<Value> {
        assert!(
            self.starts > 0,
            "fresh paginated history is not yet materialized"
        );
        if self.history_socket.is_none() {
            let stream = UnixStream::connect(
                self.root
                    .join(format!("cx-{}/server.sock", &self.instance[..12])),
            )
            .unwrap();
            stream.set_read_timeout(Some(BACKEND)).unwrap();
            stream.set_write_timeout(Some(BACKEND)).unwrap();
            let config = tungstenite::protocol::WebSocketConfig::default()
                .read_buffer_size(16 * 1024)
                .write_buffer_size(0)
                .max_write_buffer_size(4 * 1024 * 1024 + 1024)
                .max_message_size(Some(4 * 1024 * 1024))
                .max_frame_size(Some(4 * 1024 * 1024));
            let (mut socket, _) =
                tungstenite::client::client_with_config("ws://localhost/", stream, Some(config))
                    .unwrap();
            history_request(
                &mut socket,
                "initialize",
                json!({
                    "clientInfo":{"name":"amux_evidence","version":"0"},
                    "capabilities":{"experimentalApi":true}
                }),
            );
            socket
                .send(tungstenite::Message::Text(
                    json!({"method":"initialized","params":{}})
                        .to_string()
                        .into(),
                ))
                .unwrap();
            self.history_socket = Some(socket);
        }
        let mut result = history_request(
            self.history_socket.as_mut().unwrap(),
            "thread/turns/list",
            json!({"threadId":self.target["session_id"],
                "limit":7,"sortDirection":"asc","itemsView":"full"}),
        );
        assert!(
            result["nextCursor"].is_null(),
            "turn accounting was truncated"
        );
        let Value::Array(turns) = result["data"].take() else {
            panic!("authoritative turn page omitted");
        };
        assert!(
            turns.len() <= self.starts && turns.len() <= 6,
            "unrequested or over-budget model turn"
        );
        turns
    }

    fn check_turn_budget(&mut self) {
        if self.starts == 0 {
            // A fresh paginated root rejects history reads until its first real
            // turn persists. Do not turn that error into a fabricated zero.
            // The first materialized page must later account for every turn,
            // including any unrequested setup turn.
            let state = self.inspect();
            assert!(state["lifecycle"]["turn_id"].is_null());
            assert!(state["turn_outcomes"].as_object().unwrap().is_empty());
            assert_eq!(state["calls"], json!([]));
            return;
        }
        let turns = until("authoritative paginated turn accounting", BACKEND, || {
            let turns = self.history();
            (turns.len() == self.starts).then_some(turns)
        });
        eprintln!(
            "real provider turns={} (ceiling 6); no task-acceptance inference",
            turns.len()
        );
    }

    fn finish(mut self) {
        self.check_turn_budget();
        self.capture("before explicit execution stop");
        drop(self.history_socket.take());
        self.detach();
        call(&self.socket, json!({"op":"close","terminal":self.terminal}));
        until("explicit execution stop", DAEMON, || {
            let state = call(&self.socket, json!({"op":"state","terminal":self.terminal}));
            state["exit_status"].is_number().then_some(())
        });
        // Guard then verifies owned groups, restores private terminal modes and
        // removes every private auth/schema/rollout/capture/socket/root.
    }
}

// Read-only evidence transport, never an alternate application mutation route.
fn history_request(
    socket: &mut tungstenite::WebSocket<UnixStream>,
    method: &str,
    params: Value,
) -> Value {
    let deadline = PHASE_DEADLINE
        .with(|phase| phase.get())
        .unwrap_or_else(|| Instant::now() + BACKEND)
        .min(Instant::now() + BACKEND);
    let remaining = deadline.saturating_duration_since(Instant::now());
    assert!(!remaining.is_zero(), "provider evidence deadline expired");
    socket.get_mut().set_read_timeout(Some(remaining)).unwrap();
    socket.get_mut().set_write_timeout(Some(remaining)).unwrap();
    socket
        .send(tungstenite::Message::Text(
            json!({"id":"evidence:read","method":method,"params":params})
                .to_string()
                .into(),
        ))
        .unwrap();
    loop {
        assert!(
            Instant::now() < deadline,
            "provider evidence deadline expired"
        );
        let remaining = deadline.saturating_duration_since(Instant::now());
        socket.get_mut().set_read_timeout(Some(remaining)).unwrap();
        let tungstenite::Message::Text(text) = socket.read().unwrap() else {
            continue;
        };
        let mut response: Value = serde_json::from_str(&text).unwrap();
        if response["id"] != "evidence:read" {
            continue;
        }
        assert!(
            response.get("error").is_none(),
            "provider evidence {method} rejected: code={}",
            response["error"]["code"]
        );
        return response["result"].take();
    }
}

fn call(socket: &Path, request: Value) -> Value {
    let response = exchange(socket, request, DAEMON).expect("private daemon API transport failed");
    assert_eq!(
        response["ok"], true,
        "closed agent API rejected operation: code={}",
        response["error"]["code"]
    );
    response["result"].clone()
}

fn until<T>(label: &str, budget: Duration, mut probe: impl FnMut() -> Option<T>) -> T {
    struct RestoreDeadline(Option<Instant>);
    impl Drop for RestoreDeadline {
        fn drop(&mut self) {
            PHASE_DEADLINE.with(|phase| phase.set(self.0));
        }
    }
    let requested = Instant::now() + budget;
    let previous = PHASE_DEADLINE.with(|phase| {
        phase.replace(Some(
            phase.get().map_or(requested, |outer| outer.min(requested)),
        ))
    });
    let _restore = RestoreDeadline(previous);
    let deadline = PHASE_DEADLINE.with(|phase| phase.get().unwrap());
    loop {
        assert!(
            Instant::now() < deadline,
            "{label}: required genuine behavior did not occur within {} seconds; no fallback/skip",
            budget.as_secs()
        );
        if let Some(value) = probe() {
            return value;
        }
        thread::sleep(POLL.min(deadline.saturating_duration_since(Instant::now())));
    }
}

fn wait_child(guard: &Guard, pid: u32, budget: Duration, successful: bool) {
    until("owned child exit/reap", budget, || {
        let mut resources = guard.owned.lock();
        let child = resources
            .children
            .iter_mut()
            .find(|child| child.id() == pid)
            .unwrap();
        child.try_wait().unwrap().map(|status| {
            if successful {
                assert!(status.success(), "owned process exited unsuccessfully");
            }
        })
    });
}

fn same_pending(a: &Value, b: &Value) -> bool {
    ["session_id", "turn_id", "item_id", "request_id"]
        .iter()
        .all(|key| a[*key] == b[*key])
}

// Keep expected output absent from the user's echoed prompt. Seeing the whole
// marker in real attachment bytes must require actual provider continuation.
fn token_instruction(token: &str) -> String {
    // Keep the nonce intact: splitting repeated hex digits across fragments
    // tests the cheap model's string arithmetic, not broker continuation.
    let split = token.rfind('_').expect("marker requires a prefix") + 1;
    let (prefix, nonce) = token.split_at(split);
    format!(
        "output the literal prefix `{prefix}` immediately followed by the intact nonce `{nonce}` as one token, preserving every character with no separator or formatting"
    )
}

#[test]
#[ignore = "billable real Codex; exact selection, explicit model/auth and separate live approval required"]
fn real_native_first_start() {
    let mut live = Live::open("real_native_first_start");
    let fresh = live.target.clone();
    live.attach();
    assert_eq!(
        live.inspect()["target"],
        fresh,
        "second real attachment created another agent"
    );
    assert_eq!(live.inspect()["lifecycle"]["activity"], "idle");
    let token = format!("NATIVE_START_{}", uuid::Uuid::new_v4().simple());
    let steer_token = format!("EXACT_STEER_{}", uuid::Uuid::new_v4().simple());
    let (turn, _) = live.start(&format!(
        "This is an explicitly approved terminal integration check. First run `sleep 12` in the workspace with no escalation. Then {}. Do not modify files or start background jobs. Follow any additional user steering received while sleeping.", token_instruction(&token)
    ));
    live.wait_state("active steerable exact turn", RESPONSE, |state| {
        state["lifecycle"]["turn_id"] == turn
            && state["lifecycle"]["activity"] == "running"
            && state["capabilities"]["steer"] == true
    });
    let receipt = live.mutation("agent-prompt", json!({"intent":"steer","expected_turn_id":turn,
        "text":format!("Additional user instruction for this same turn: also {}. Do not start another turn.", token_instruction(&steer_token))}));
    let id = receipt["call_id"].clone();
    live.wait_state(
        "authoritative same-turn steer acknowledgement",
        RESPONSE,
        |state| {
            state["calls"].as_array().is_some_and(|calls| {
                calls.iter().any(|call| {
                    call["call_id"] == id && call["provider_ack"] == true && call["turn_id"] == turn
                })
            })
        },
    );
    live.complete(&turn);
    live.assistant_contains(&turn, &[&token, &steer_token]);
    live.native_contains(
        "API start visible in each genuine native attachment",
        &token,
    );
    live.native_contains(
        "same-turn steer continuation visible natively",
        &steer_token,
    );
    assert_eq!(live.inspect()["target"], fresh);
    let stale = exchange(&live.socket, json!({"op":"agent-prompt","target":live.target,"control":live.control,
        "intent":"steer","expected_turn_id":turn,"text":"This ended turn must not become a new turn."}), DAEMON).unwrap();
    assert_eq!(
        stale["ok"], false,
        "ended steer silently started another turn"
    );
    live.finish();
}

#[test]
#[ignore = "billable genuine questions/allow/deny; exact selection and separate live approval required"]
fn real_questions_and_permissions() {
    let mut live = Live::open("real_questions_and_permissions");
    // Use the actual ordinary native command, not a provider override or hidden
    // setup model prompt. A missing native Plan mode is a visible prerequisite failure.
    live.native_mode("Plan mode");
    live.check_turn_budget();
    let chosen = format!("QUESTION_CHOICE_{}", uuid::Uuid::new_v4().simple());
    let other = format!("QUESTION_OTHER_{}", uuid::Uuid::new_v4().simple());
    let question_done = format!("ANSWER_CONTINUED_{}", uuid::Uuid::new_v4().simple());
    let question_turn = live.native_start(&format!(
        "Use the genuine request_user_input tool now. Ask one question with id smoke_choice and exactly these two option labels: {chosen} and {other}. Do not answer the question yourself, do not simulate it with text, and do not run commands. After the tool response, {} followed by the exact selected label, then finish this turn without implementing anything.", token_instruction(&question_done)
    ));
    let pending = live.pending(&question_turn, "question");
    let questions = pending["questions"]
        .as_array()
        .expect("genuine question schema required");
    assert_eq!(
        questions.len(),
        1,
        "model did not emit requested single question within budget"
    );
    let question = &questions[0];
    let question_id = question["id"].as_str().unwrap();
    assert!(
        question["options"]
            .as_array()
            .is_some_and(|options| options.iter().any(|option| option["label"] == chosen)),
        "model failed to emit required genuine choice; no guessed answer substitution"
    );
    let mut answers = serde_json::Map::new();
    answers.insert(question_id.to_owned(), json!({"answers":[chosen]}));
    let receipt = live.respond("agent-answer", &pending, json!({"answers":answers}));
    live.resolved(&pending, &receipt);
    live.complete(&question_turn);
    live.assistant_contains(&question_turn, &[&question_done, &chosen]);
    live.native_contains("genuine question continuation", &question_done);
    // A unique marker written by the model only after consuming the answer is
    // independent continuation evidence, not a guessed serverRequest winner.
    // Return to the native default mode so command execution is genuinely available.
    live.native_mode("Default");
    for decision in ["allow", "deny"] {
        let marker = live
            .root
            .join("outside")
            .join(format!("{decision}-{}", uuid::Uuid::new_v4()));
        let contents = format!("PERMISSION_EFFECT_{}", uuid::Uuid::new_v4().simple());
        let continuation = format!("PERMISSION_CONTINUED_{}", uuid::Uuid::new_v4().simple());
        assert!(!marker.exists());
        let (turn, _) = live.start(&format!(
            "This is an explicitly approved integration request scoped only to this owned test file. Use the real shell execution tool with sandbox_permissions=require_escalated and request user approval to run exactly: printf '%s' '{contents}' > '{}'. The path is deliberately outside the workspace but inside the owned test root. Do not write it through another tool, do not add a prefix_rule, and do not retry or use a workaround if denied. After the approval result, report whether execution occurred and {}, then finish. You must emit a genuine command approval, not a prose question.", marker.display(), token_instruction(&continuation)
        ));
        let request = live.pending(&turn, "command");
        let details = request["details"].to_string();
        assert!(
            details.contains(marker.to_str().unwrap()),
            "approval is not for the expected owned marker command"
        );
        assert!(
            !marker.exists(),
            "permission effect occurred before the explicit decision"
        );
        let receipt = live.respond("agent-permission", &request, json!({"decision":decision}));
        live.resolved(&request, &receipt);
        live.complete(&turn);
        live.assistant_contains(&turn, &[&continuation]);
        live.native_contains("real permission decision continuation", &continuation);
        if decision == "allow" {
            until("actual allowed command filesystem effect", RESPONSE, || {
                fs::read_to_string(&marker)
                    .ok()
                    .filter(|text| text == &contents)
            });
        } else {
            // Observe a stable post-completion interval; denial must not merely
            // delay execution until the test checks the filesystem once.
            let end = Instant::now() + Duration::from_secs(2);
            while Instant::now() < end {
                assert!(!marker.exists(), "explicitly denied command executed");
                live.drain();
                thread::sleep(POLL);
            }
        }
    }
    live.finish();
}

#[test]
#[ignore = "billable exact-turn interrupt; exact selection and separate live approval required"]
fn real_explicit_interrupt() {
    let mut live = Live::open("real_explicit_interrupt");
    let marker = live.root.join("outside/should-never-complete");
    let (turn, _) = live.start(&format!(
        "Request genuine user approval with sandbox_permissions=require_escalated to run exactly `printf unexpected > '{}'`. This is inside an owned test root but outside the workspace. Do not add a prefix_rule, do not use a background job, do not retry or bypass approval. Wait for the approval response. This real active turn will be explicitly interrupted by its user before any approval is given.", marker.display()
    ));
    let interrupted_request = live.pending(&turn, "command");
    assert!(
        !marker.exists(),
        "command ran before permission or interruption"
    );
    live.wait_state("real interruptible active request", RESPONSE, |state| {
        state["lifecycle"]["turn_id"] == turn
            && state["lifecycle"]["activity"] == "awaiting-input"
            && state["capabilities"]["interrupt"] == true
    });
    let original_target = live.target.clone();
    live.capture("active turn before viewer detach");
    live.detach();
    assert_eq!(live.inspect()["lifecycle"]["turn_id"], turn);
    assert_eq!(
        live.inspect()["lifecycle"]["activity"],
        "awaiting-input",
        "viewer detach interrupted real work"
    );
    live.attach();
    // A force takeover changes input authority, never the turn or processes.
    let controller = call(
        &live.socket,
        json!({"op":"controller-create","role":"external","label":"explicit interrupt owner"}),
    );
    let ownership =
        call(&live.socket, json!({"op":"state","terminal":live.terminal}))["ownership"].clone();
    let grant = call(
        &live.socket,
        json!({"op":"ownership-claim","terminal":live.terminal,
        "controller":controller["controller"],"observed":ownership["observed"],"mode":"force"}),
    );
    live.control = grant["control"].clone();
    assert_eq!(live.control["kind"], "control");
    let state = live.inspect();
    assert_eq!(state["target"], original_target);
    assert_eq!(state["lifecycle"]["turn_id"], turn);
    assert_eq!(
        state["lifecycle"]["activity"], "awaiting-input",
        "force takeover issued an implicit interrupt"
    );
    let before_pending = state["pending"].clone();
    assert!(
        before_pending
            .as_array()
            .unwrap()
            .iter()
            .any(|request| same_pending(request, &interrupted_request))
    );
    let receipt = live.mutation("agent-interrupt", json!({"turn_id":turn}));
    let id = receipt["call_id"].clone();
    let acknowledged = live.wait_state(
        "interrupt provider acknowledgement (not completion)",
        RESPONSE,
        |state| {
            state["calls"].as_array().is_some_and(|calls| {
                calls
                    .iter()
                    .any(|call| call["call_id"] == id && call["provider_ack"] == true)
            })
        },
    );
    assert_eq!(acknowledged["target"], original_target);
    eprintln!(
        "exact-turn interrupt acknowledged; completion and pending cleanup checked independently"
    );
    let completed = live.wait_state(
        "authoritative exact-turn interrupted completion",
        RESPONSE,
        |state| {
            state["turn_outcomes"][turn.as_str()] == "interrupted"
                && state["lifecycle"]["backend"] == "running"
        },
    );
    let remaining = completed["pending"].as_array().unwrap().len();
    eprintln!(
        "interrupted completion observed; pending at completion={remaining}; pending before interrupt={}",
        before_pending.as_array().unwrap().len()
    );
    live.wait_state(
        "independent potentially delayed exact-turn pending cleanup",
        RESPONSE,
        |state| {
            state["pending"]
                .as_array()
                .is_some_and(|requests| !requests.iter().any(|request| request["turn_id"] == turn))
        },
    );
    assert!(
        !marker.exists(),
        "interrupted command reached its forbidden completion effect"
    );
    live.capture("authoritative interrupted native surface");
    let continuation = format!("AFTER_INTERRUPT_{}", uuid::Uuid::new_v4().simple());
    let (next_turn, _) = live.start(&format!(
        "Please {}. Do not use tools.",
        token_instruction(&continuation)
    ));
    assert_ne!(next_turn, turn);
    let stale = exchange(
        &live.socket,
        json!({"op":"agent-interrupt","target":live.target,
        "control":live.control,"turn_id":turn}),
        DAEMON,
    )
    .unwrap();
    assert_eq!(
        stale["ok"], false,
        "stale interrupt selected replacement turn"
    );
    live.complete(&next_turn);
    live.assistant_contains(&next_turn, &[&continuation]);
    live.native_contains(
        "same frontend/backend survived interrupt and accepted deliberate next turn",
        &continuation,
    );
    assert!(!marker.exists());
    live.finish();
}

#[test]
#[ignore = "billable complete-broker all32 acceptance; exact selection, explicit model/auth and separate live approval required"]
fn real_complete_broker_handoff() {
    complete::run();
}

//! Real native attachment boundaries. Parent registers this child module.
use super::*;

#[path = "native/acceptance.rs"]
mod acceptance;

#[path = "native/live.rs"]
pub(super) mod live;

#[path = "native/daily_use.rs"]
mod daily_use;

#[path = "native/recovery_edges.rs"]
mod recovery_edges;

#[path = "native/omp_live.rs"]
pub(super) mod omp_live;
use alacritty_terminal::Term;
use alacritty_terminal::event::{Event as TerminalEvent, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Config;
use alacritty_terminal::vte::ansi::Processor;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

const COLS: usize = 120;
const ROWS: usize = 40;
const OUTPUT_LIMIT: usize = 16 * 1024 * 1024;
const PREFIX: &[u8] = b"\x01";
const ACCEPT_FORM: &[u8] = b"\x1b[15~";

struct Geometry {
    columns: usize,
    lines: usize,
}
impl Dimensions for Geometry {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.columns
    }
}

#[derive(Clone)]
struct Listener {
    replies: mpsc::Sender<String>,
    effects: Arc<parking_lot::Mutex<Vec<String>>>,
}
impl EventListener for Listener {
    fn send_event(&self, event: TerminalEvent) {
        match event {
            TerminalEvent::PtyWrite(reply) => {
                let _ = self.replies.send(reply);
            }
            TerminalEvent::Title(title) => self.effects.lock().push(title),
            TerminalEvent::ClipboardStore(_, text) => self.effects.lock().push(text),
            _ => {}
        }
    }
}

struct Output {
    term: Term<Listener>,
    raw: Vec<u8>,
    error: Option<String>,
}

fn write_pty(master: &mut fs::File, mut bytes: &[u8]) -> io::Result<()> {
    let deadline = Instant::now() + DEADLINE;
    while !bytes.is_empty() {
        match master.write(bytes) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "PTY write")),
            Ok(n) => bytes = &bytes[n..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

// Zsh remains the actual session leader until termios is observed and explicitly
// acknowledged. Darwin revokes the slave when that leader exits, even if open.
struct Attachment {
    child: Child,
    master: fs::File,
    slave: fs::File,
    original: libc::termios,
    tui_exit: PathBuf,
    output: Arc<parking_lot::Mutex<Output>>,
    effects: Arc<parking_lot::Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    reader: Option<std::thread::JoinHandle<()>>,
}
impl Attachment {
    fn new(host: &Host, agent: &Agent) -> Self {
        let mut master = -1;
        let mut slave = -1;
        let mut size = libc::winsize {
            ws_row: ROWS as u16,
            ws_col: COLS as u16,
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
                    &mut size,
                )
            },
            0,
            "openpty: {}",
            io::Error::last_os_error()
        );
        let master = unsafe { fs::File::from_raw_fd(master) };
        let slave = unsafe { fs::File::from_raw_fd(slave) };
        for fd in [master.as_raw_fd(), slave.as_raw_fd()] {
            assert_eq!(
                unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) },
                0
            );
        }
        let mut original = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe { libc::tcgetattr(slave.as_raw_fd(), &mut original) },
            0
        );
        let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
        assert!(flags >= 0);
        assert_eq!(
            unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
            0
        );
        let tui_exit = host
            .root
            .join(format!("native-{}.exit", uuid::Uuid::new_v4().simple()));
        let mut command = Command::new("/bin/zsh");
        command
            .args(["-f", "-c"])
            .arg(concat!(
                "\"$1\" --socket \"$2\" acp attach \"$3\"; code=$?; ",
                "printf '%s\\n' \"$code\" > \"$4\"; ",
                "while IFS= read -r ack; do ",
                "[[ \"$ack\" == finish ]] && exit \"$code\"; done; exit 125"
            ))
            .arg("native-ctty-host")
            .arg(env!("CARGO_BIN_EXE_amux"))
            .arg(&host.socket)
            .arg(agent.target["id"].as_str().unwrap())
            .arg(&tui_exit)
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("HOME", host.root.join("home"))
            .env("XDG_CONFIG_HOME", host.root.join("config"))
            .env("XDG_RUNTIME_DIR", host.root.join("runtime"))
            .env("AMUX_RUNTIME_DIR", host.root.join("runtime"))
            .env("AMUX_SOCKET", &host.socket)
            .env("TERM", "xterm-256color")
            .current_dir(&host.root)
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave.try_clone().unwrap()));
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let (replies, rx) = mpsc::channel();
        let effects = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let output = Arc::new(parking_lot::Mutex::new(Output {
            term: Term::new(
                Config::default(),
                &Geometry {
                    columns: COLS,
                    lines: ROWS,
                },
                Listener {
                    replies,
                    effects: effects.clone(),
                },
            ),
            raw: Vec::new(),
            error: None,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let mut attachment = Self {
            child: command.spawn().unwrap(),
            master,
            slave,
            original,
            tui_exit,
            output,
            effects,
            stop,
            reader: None,
        };
        let mut master = attachment.master.try_clone().unwrap();
        let output = attachment.output.clone();
        let stop = attachment.stop.clone();
        attachment.reader = Some(std::thread::spawn(move || {
            let mut parser: Processor = Processor::new();
            let mut bytes = [0; 32768];
            while !stop.load(Ordering::Acquire) {
                match master.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(n) => {
                        let mut output = output.lock();
                        if output.raw.len().saturating_add(n) <= OUTPUT_LIMIT {
                            output.raw.extend_from_slice(&bytes[..n]);
                        } else {
                            output.error = Some("PTY evidence limit; draining continues".into());
                        }
                        parser.advance(&mut output.term, &bytes[..n]);
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
                    Err(error) => {
                        output.lock().error = Some(error.to_string());
                        break;
                    }
                }
                // Genuine terminal-generated cursor replies, not injected UI events.
                for reply in rx.try_iter() {
                    if let Err(error) = write_pty(&mut master, reply.as_bytes()) {
                        output.lock().error = Some(format!("cursor reply: {error}"));
                        return;
                    }
                }
            }
        }));
        attachment
    }

    fn text(&self) -> String {
        let output = self.output.lock();
        assert!(output.error.is_none(), "{:?}", output.error);
        (0..output.term.screen_lines())
            .map(|row| {
                (0..output.term.columns())
                    .map(|col| output.term.grid()[Point::new(Line(row as i32), Column(col))].c)
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn resize(&mut self, columns: usize, lines: usize) {
        let mut output = self.output.lock();
        output.term.resize(Geometry { columns, lines });
        let size = libc::winsize {
            ws_row: lines.try_into().unwrap(),
            ws_col: columns.try_into().unwrap(),
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        assert_eq!(
            unsafe { libc::ioctl(self.slave.as_raw_fd(), libc::TIOCSWINSZ, &size) },
            0,
            "resize owned native PTY: {}",
            io::Error::last_os_error()
        );
    }

    fn wait(&mut self, marker: &str) {
        let deadline = Instant::now() + DEADLINE;
        loop {
            let text = self.text();
            if text.contains(marker) {
                return;
            }
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "attachment exited: {text}"
            );
            if Instant::now() >= deadline {
                let mut queued = 0i32;
                let available =
                    unsafe { libc::ioctl(self.slave.as_raw_fd(), libc::FIONREAD, &mut queued) }
                        == 0;
                let recovery = {
                    let output = self.output.lock();
                    let raw = String::from_utf8_lossy(&output.raw);
                    raw.rfind("Disconnected").map(|offset| {
                        raw[offset..]
                            .chars()
                            .take(4096)
                            .flat_map(char::escape_debug)
                            .collect::<String>()
                    })
                };
                panic!(
                    "missing {marker:?}; pending kernel input bytes: {:?}; last recovery screen: {recovery:?}\n{text}",
                    available.then_some(queued)
                );
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn wait_header(&mut self, marker: &str) {
        let deadline = Instant::now() + DEADLINE;
        loop {
            let text = self.text();
            if text.lines().take(3).any(|line| line.contains(marker)) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "selected header {marker:?} missing:\n{text}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn details(&mut self) {
        if !self.text().contains("ACP settings (read-only)") {
            self.send(b"\x1b[21~");
        }
        self.wait("ACP settings (read-only)");
        self.send(b"\x1b[H");
    }

    fn conversation(&mut self) {
        self.send(b"\x1b[21~");
        let deadline = Instant::now() + DEADLINE;
        while self.text().contains("ACP settings (read-only)") {
            assert!(
                Instant::now() < deadline,
                "Details did not close:\n{}",
                self.text()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn ready_action(&mut self, label: &str) {
        // This read-only transition follows preceding PTY edits in input order.
        // Restore the conversation and observe its actionable paint before a
        // fresh activation; never retry or replay an activation rejected early.
        self.details();
        self.conversation();
        self.wait(label);
    }

    fn projected(&mut self, host: &Host, sequence: u64) {
        // The diagnostic cursor moved out of the conversation. Observe the same
        // real rendered projection through Details, then restore literal input.
        self.details();
        host.until("canonical projection reaches the native renderer", || {
            self.text()
                .lines()
                .filter_map(|line| {
                    line.trim_start()
                        .strip_prefix("Cursor ")?
                        .split_whitespace()
                        .next()?
                        .parse::<u64>()
                        .ok()
                })
                .any(|through| through >= sequence)
                .then_some(())
        });
        self.conversation();
    }

    fn selected_interaction(&mut self, ids: &[String]) -> Option<String> {
        self.details();
        self.wait("Selected interaction:");
        let text = self.text();
        let line = text
            .lines()
            .find(|line| line.contains("Selected interaction:"))
            .unwrap();
        let selected = ids.iter().find(|id| line.contains(id.as_str())).cloned();
        self.conversation();
        selected
    }

    fn send(&mut self, bytes: &[u8]) {
        write_pty(&mut self.master, bytes).unwrap();
    }
    fn paste(&mut self, text: &str) {
        self.send(format!("\x1b[200~{text}\x1b[201~").as_bytes());
    }
    fn click_input(&mut self, label: &str) -> Vec<u8> {
        self.wait(label);
        let text = self.text();
        let (row, line) = text
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains(label))
            .last()
            .unwrap_or_else(|| panic!("missing action {label:?}:\n{text}"));
        let offset = line.rfind(label).unwrap();
        let column = unicode_width::UnicodeWidthStr::width(&line[..offset]);
        format!(
            "\x1b[<0;{};{}M\x1b[<0;{};{}m",
            column + 1,
            row + 1,
            column + 1,
            row + 1
        )
        .into_bytes()
    }
    fn click(&mut self, label: &str) {
        let input = self.click_input(label);
        self.send(&input);
    }
    fn action(&mut self, key: u8) {
        self.send(PREFIX);
        self.send(&[key]);
    }
    fn owner(&mut self) {
        if self.text().contains("ACP settings (read-only)") {
            self.conversation();
        }
        let deadline = Instant::now() + DEADLINE;
        loop {
            let text = self.text();
            if text.lines().any(|line| {
                line.starts_with("ACP · ")
                    && !line.contains("read-only")
                    && !line.contains("starting")
                    && !line.contains("unavailable")
            }) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "native control not ready:\n{text}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn observer(&mut self) {
        if self.text().contains("ACP settings (read-only)") {
            self.conversation();
        }
        self.wait(" · read-only");
    }
    fn idle(&mut self) {
        self.details();
        self.wait("Foreground: none");
        self.conversation();
    }

    fn detach(&mut self) {
        self.action(17); // configured prefix + Ctrl-q, outside application input
        let deadline = Instant::now() + DEADLINE;
        let code = loop {
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "controlling host exited before termios observation: {}",
                self.text()
            );
            if let Ok(code) = fs::read_to_string(&self.tui_exit)
                && let Ok(code) = code.trim().parse::<i32>()
            {
                break code;
            }
            assert!(
                Instant::now() < deadline,
                "TUI detach deadline: {}",
                self.text()
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(code, 0, "TUI detach failed: {}", self.text());
        let mut restored: libc::termios = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe { libc::tcgetattr(self.slave.as_raw_fd(), &mut restored) },
            0
        );
        assert_eq!(restored.c_iflag, self.original.c_iflag, "input termios");
        assert_eq!(restored.c_oflag, self.original.c_oflag, "output termios");
        assert_eq!(restored.c_cflag, self.original.c_cflag, "control termios");
        assert_eq!(restored.c_lflag, self.original.c_lflag, "local termios");
        assert_eq!(
            restored.c_cc, self.original.c_cc,
            "terminal control characters"
        );
        assert!(
            self.child.try_wait().unwrap().is_none(),
            "host must pin the controlling PTY"
        );
        self.send(b"finish\n");
        let deadline = Instant::now() + DEADLINE;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "controlling host exit: {status}: {}",
                    self.text()
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "controlling host did not acknowledge finish"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        self.join_reader();
    }

    fn join_reader(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(reader) = self.reader.as_ref() {
            let deadline = Instant::now() + DEADLINE;
            while !reader.is_finished() {
                assert!(Instant::now() < deadline, "PTY drainer did not stop");
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        if let Some(reader) = self.reader.take() {
            reader.join().unwrap();
        }
    }
}
impl Drop for Attachment {
    fn drop(&mut self) {
        let mut cleanup_failed = false;
        unsafe {
            libc::tcsetattr(self.slave.as_raw_fd(), libc::TCSANOW, &self.original);
        }
        // Signal only a known-running original leader; an observation error is not a pin.
        if self.child.try_wait().is_ok_and(|status| status.is_none()) {
            unsafe {
                libc::kill(-(self.child.id() as i32), libc::SIGKILL);
            }
            let _ = self.child.kill();
            let reaped = reap_until(&mut self.child, Duration::from_secs(5));
            if !reaped {
                eprintln!("native PTY host {} was not reaped", self.child.id());
                cleanup_failed = true;
            }
        }
        self.stop.store(true, Ordering::Release);
        // Reads are nonblocking; cursor-reply writes have a deadline. Join even
        // during unwinding, rather than detaching an owned worker on timeout.
        if let Some(reader) = self.reader.take()
            && reader.join().is_err()
        {
            eprintln!("native PTY drainer panicked during cleanup");
        }
        if cleanup_failed {
            assert!(std::thread::panicking(), "native PTY host cleanup failed");
        }
    }
}

fn ownership(host: &Host, agent: &Agent) -> Value {
    host.view(agent.target["id"].as_str().unwrap()).unwrap()["ownership"].clone()
}
fn handle(host: &Host, capability: &Value, method: &str) -> Value {
    host.call(json!({"op":"control-handle","handle":capability,"method":method}))
}
fn target(agent: &Agent) -> Value {
    json!({"kind":"acp","id":agent.target["id"]})
}
fn external(host: &Host) -> Value {
    host.call(json!({"op":"controller-create","role":"external","label":"Native boundary peer"}))
}
fn force(host: &Host, agent: &mut Agent) {
    let controller = external(host);
    let grant = host.call(json!({"op":"ownership-claim","target":target(agent),
        "controller":controller["controller"],"mode":"force",
        "observed":ownership(host, agent)["observed"]}));
    agent.opened["control"] = grant["control"].clone();
    agent.opened["generation"] = grant["generation"].clone();
    agent.controller_id = controller["controller_id"].clone();
}
fn native_attachment(host: &Host, agent: &Agent) -> Attachment {
    host.call(json!({"op":"config","config":{"prefix":"Ctrl-a"}}));
    handle(host, &agent.opened["control"], "release");
    let mut attachment = Attachment::new(host, agent);
    attachment.owner();
    assert_eq!(ownership(host, agent)["owner"]["role"], "attachment");
    attachment
}
fn native_prompt(host: &Host, agent: &Agent, native: &mut Attachment, text: &str) -> Value {
    let previous = host.inspect(agent)["work"]
        .as_array()
        .unwrap()
        .iter()
        .map(|work| work["work_id"].clone())
        .collect::<Vec<_>>();
    native.click("Prompt");
    native.paste(text);
    native.ready_action("Send");
    native.send(b"\r");
    let deadline = Instant::now() + DEADLINE;
    host.until(
        &format!("deliberate native prompt {text:?} admitted"),
        || {
            let projection = host.inspect(agent);
            let admitted = projection["work"]
                .as_array()?
                .iter()
                .find(|work| work["op"] == "acp-prompt" && !previous.contains(&work["work_id"]))
                .cloned();
            assert!(
                admitted.is_some() || Instant::now() < deadline,
                "native prompt {text:?} was not admitted; work: {}; ownership: {}; surface:\n{}",
                projection["work"],
                ownership(host, agent),
                native.text()
            );
            admitted
        },
    )
}
fn finish(mut host: Host, agent: &mut Agent, native: &mut Attachment, sentinel: &mut Sentinel) {
    native.detach();
    force(&host, agent);
    host.close(agent); // observes peer absence and cleanup_complete before fallback
    if agent.root.join("descendant.pid").exists() {
        host.reaped(agent, "descendant.pid");
    }
    assert!(!agent.root.join("fixture-fallback").exists());
    host.call(json!({"op":"shutdown"}));
    let deadline = Instant::now() + DEADLINE;
    loop {
        if let Some(status) = host.daemon.try_wait().unwrap() {
            assert!(status.success(), "daemon shutdown: {status}");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "owned daemon not reaped before fallback"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!host.socket.exists(), "private socket survived shutdown");
    sentinel.alive();
    let root = host.root.clone();
    fs::remove_dir_all(&root).unwrap();
    assert!(
        !root.exists(),
        "private root survived owned cleanup before fallback"
    );
    drop(host);
    assert!(!root.exists(), "private root survived fixture retirement");
    sentinel.alive();
}

#[test]
fn e04s03_native_force_fences_prompt_draft_and_physical_routes_without_replay() {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("arithmetic");
    host.ready(&mut agent);
    let exact = agent.target.clone();
    let mut native = native_attachment(&host, &agent);
    let original_owner = ownership(&host, &agent)["owner"]["controller_id"].clone();
    native.paste("add 900 1");
    native.wait("add 900 1"); // first edit demonstrably reached the native draft
    force(&host, &mut agent);
    native.observer();
    // Physical key/repeat, bracketed paste, SGR mouse/gesture tails, focus and
    // doubled prefix pass through actual crossterm/top-level routing.
    native.send(b"add 800 2\r\x1b[120;1:2u");
    native.paste("add 700 3\r\n");
    native.send(b"\x1b[<0;16;14M\x1b[<32;17;14M\x1b[<0;17;14m\x1b[O\x1b[I");
    native.send(b"\x01\x01");
    native.send(b"\r");
    // A successful fresh ownership gesture is also the processing barrier for
    // all preceding input; do not use a sleep as proof of non-delivery.
    native.action(6);
    native.wait("Enter Force control");
    native.send(b"\r");
    native.owner();
    native.send(b"\r"); // old draft must not be rebound to the fresh grant
    let prompt = native_prompt(&host, &agent, &mut native, "add 17 25");
    let completed = host.completed(&agent, &prompt["work_id"]);
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(agent.effect()["rpc_id"], completed["rpc_id"]);
    assert_eq!(agent.effect()["session_id"], exact["session_id"]);
    assert_eq!(host.inspect(&agent)["target"], exact);
    assert_eq!(prompt["controller_id"], original_owner);
    let projection = host.inspect(&agent);
    assert_eq!(
        projection["work"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|work| work["op"] == "acp-prompt")
            .count(),
        1
    );
    let canonical = serde_json::to_string(&projection["content"]).unwrap();
    for rejected in ["add 900 1", "add 800 2", "add 700 3"] {
        assert!(
            !canonical.contains(rejected),
            "stale draft in canonical history"
        );
    }
    assert!(!agent.root.join("unexpected-cancel").exists());
    finish(host, &mut agent, &mut native, &mut sentinel);
}

#[test]
fn e07s02_native_queued_f5_presses_cannot_submit_before_reviewed_input_drain() {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("s02-form");
    host.ready(&mut agent);
    let exact = agent.target.clone();
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let mut native = native_attachment(&host, &agent);
    let admitted = native_prompt(&host, &agent, &mut native, "add 17 25");
    let original = host.work(&agent, &admitted["work_id"]);
    let form = pending(&host, &agent, "form");
    acceptance::show_interaction(&host, &agent, &mut native, Some(&form));
    native.send(b"\x1b[14~"); // explicitly unset before a fresh typed answer
    native.wait("UNSET (F4)");
    native.paste("15");
    native.wait("local value");
    native.ready_action("Review (F5)");

    // Stop the real attachment, not the daemon/agent, so both physical Press
    // sequences are queued before it can review or drain either input event.
    let group = -(native.child.id() as i32);
    assert_eq!(unsafe { libc::kill(group, libc::SIGSTOP) }, 0);
    let mut status = 0;
    assert_eq!(
        unsafe { libc::waitpid(native.child.id() as i32, &mut status, libc::WUNTRACED) },
        native.child.id() as i32
    );
    assert!(libc::WIFSTOPPED(status));
    native.send(b"\x1b[15~\x1b[15~\x1b[21~");
    let mut queued = 0i32;
    assert_eq!(
        unsafe { libc::ioctl(native.slave.as_raw_fd(), libc::FIONREAD, &mut queued) },
        0
    );
    assert!(queued >= 2 * ACCEPT_FORM.len() as i32);
    assert_eq!(unsafe { libc::kill(group, libc::SIGCONT) }, 0);

    // F10 follows both queued presses: observing Details proves they have been
    // consumed even by the defective implementation, whose transient Submit
    // paint alone would not prove that the second F5 was already processed.
    native.wait("ACP settings (read-only)");
    let projection = host.inspect(&agent);
    let still_pending = projection["content"]["interactions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["interaction_id"] == form["interaction_id"])
        .unwrap();
    assert_eq!(still_pending["state"], "pending");
    assert_eq!(still_pending["revision"], form["revision"]);
    assert_eq!(still_pending["scope"], form["scope"]);
    s03_same_work(&host.work(&agent, &admitted["work_id"]), &original);
    assert!(host.work(&agent, &admitted["work_id"])["outcome"].is_null());
    assert!(!agent.root.join("effect.json").exists());
    assert!(
        !projection["work"]
            .as_array()
            .unwrap()
            .iter()
            .any(|work| { work["op"] == "acp-answer" || work["op"] == "acp-interrupt" })
    );
    assert_eq!(projection["target"], exact);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    native.conversation();
    native.wait("Submit (F5)"); // fully painted review after queued input drains

    native.send(ACCEPT_FORM); // fresh explicit consent after observing the review
    let completed = host.completed(&agent, &admitted["work_id"]);
    s03_same_work(&completed, &original);
    assert_eq!(agent.effect()["value"], 57);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(agent.effect()["rpc_id"], original["rpc_id"]);
    assert_eq!(agent.effect()["session_id"], exact["session_id"]);
    let projection = host.inspect(&agent);
    let selected = projection["content"]["interactions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["interaction_id"] == form["interaction_id"])
        .unwrap();
    assert_eq!(
        selected["selection"],
        json!({"action":"accept","content":{"bonus":15}})
    );
    let answer = host.work(&agent, &selected["selected_work_id"]);
    assert_eq!(answer["interaction_id"], form["interaction_id"]);
    assert_eq!(answer["controller_id"], original["controller_id"]);
    assert_eq!(answer["owner_generation"], original["owner_generation"]);
    assert_eq!(
        projection["work"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|work| work["op"] == "acp-answer")
            .count(),
        1
    );
    assert_eq!(projection["target"], exact);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    assert!(!agent.root.join("unexpected-cancel").exists());
    finish(host, &mut agent, &mut native, &mut sentinel);
}

#[test]
fn e07s02_native_queued_permission_enter_cannot_allow_before_input_drain() {
    native_queued_permission("Allow this invocation", "opaque-α", 42);
}

#[test]
fn e07s02_native_queued_permission_enter_cannot_replace_a_fresh_reject() {
    native_queued_permission("Reject this invocation", "opaque-γ", 0);
}

fn native_queued_permission(label: &str, option: &str, expected: i64) {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("e07-queued-permission");
    host.ready(&mut agent);
    let exact = agent.target.clone();
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let mut native = native_attachment(&host, &agent);
    let admitted = native_prompt(&host, &agent, &mut native, "add 17 25");
    let original = host.work(&agent, &admitted["work_id"]);
    host.barrier(&agent, "permission-gate");
    let before = host.inspect(&agent);
    assert!(
        before["content"]["interactions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    native.projected(&host, before["seq"].as_u64().unwrap());
    let grant = ownership(&host, &agent);
    let controller = grant["owner"]["controller_id"].clone();
    let generation = grant["observed"]["generation"].clone();
    native.send(b"\x1b[I");
    let reported = host.until(
        "original connection reports focused ACP before queuing input",
        || {
            ownership(&host, &agent)["views"]
                .as_array()?
                .iter()
                .find(|view| {
                    view["controller_id"] == controller
                        && view["reported"] == true
                        && view["focused"] == true
                        && view["target"] == target(&agent)
                })
                .cloned()
        },
    );

    // No permission has existed on either canonical or painted surfaces. Pin
    // the genuine controlling-PTY group while physical input is queued and the
    // original peer publishes a callback for its still-outstanding prompt.
    let group = -(native.child.id() as i32);
    assert_eq!(unsafe { libc::kill(group, libc::SIGSTOP) }, 0);
    let mut status = 0;
    assert_eq!(
        unsafe { libc::waitpid(native.child.id() as i32, &mut status, libc::WUNTRACED) },
        native.child.id() as i32
    );
    assert!(libc::WIFSTOPPED(status));
    // FocusIn wakes any pre-stop read. The following loop refreshes canonical
    // facts before Enter/F8. F10 proves that both queued gestures were consumed;
    // the subsequent focus-report receipt fences their asynchronous dispatch.
    let queued_input = b"\x1b[I\r\x1b[19~\x1b[21~";
    native.send(queued_input);
    let mut queued = 0i32;
    assert_eq!(
        unsafe { libc::ioctl(native.slave.as_raw_fd(), libc::FIONREAD, &mut queued) },
        0
    );
    assert!(queued >= queued_input.len() as i32);
    fs::write(
        agent.root.join("permission-gate.release"),
        b"publish callback",
    )
    .unwrap();
    let permission = pending(&host, &agent, "permission");
    // Expire the parent's periodic inspect interval while it cannot consume
    // input. This schedules refresh, not evidence that the input was consumed.
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(unsafe { libc::kill(group, libc::SIGCONT) }, 0);
    native.wait("ACP settings (read-only)");
    native.send(b"\x1b[O");
    host.until(
        "post-Details original-connection receipt follows queued mutations",
        || {
            ownership(&host, &agent)["views"]
                .as_array()?
                .iter()
                .find(|view| {
                    view["controller_id"] == controller
                        && view["connected"] == true
                        && view["reported"] == true
                        && view["focused"] == false
                        && view["target"] == target(&agent)
                        && view["serial"].as_u64().unwrap() > reported["serial"].as_u64().unwrap()
                })
                .cloned()
        },
    );
    let projection = host.inspect(&agent);
    let retained = projection["content"]["interactions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["interaction_id"] == permission["interaction_id"])
        .unwrap();
    assert_eq!(
        retained["state"], "pending",
        "queued Enter answered a new callback"
    );
    assert_eq!(retained["revision"], permission["revision"]);
    assert_eq!(retained["scope"], permission["scope"]);
    s03_same_work(&host.work(&agent, &admitted["work_id"]), &original);
    assert!(host.work(&agent, &admitted["work_id"])["outcome"].is_null());
    assert!(
        projection["work"].as_array().unwrap().iter().all(|work| {
            !matches!(
                work["op"].as_str(),
                Some("acp-permission" | "acp-authenticate" | "acp-interrupt")
            )
        }),
        "old physical gestures must not admit replacement mutations: {}",
        projection["work"]
    );
    for path in ["permission-response.json", "effect.json", "tool-effect"] {
        assert!(
            !agent.root.join(path).exists(),
            "queued input reached {path}"
        );
    }
    assert_eq!(projection["target"], exact);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    let current = ownership(&host, &agent);
    assert_eq!(current["owner"]["controller_id"], controller);
    assert_eq!(current["observed"]["generation"], generation);

    native.conversation();
    acceptance::show_interaction(&host, &agent, &mut native, Some(&permission));
    let offered = permission["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|offered| offered["option_id"] == option)
        .unwrap();
    let armed_label = format!("[{label} [{}]]", offered["kind"].as_str().unwrap());
    native.ready_action(&armed_label);
    native.click(&armed_label); // One fresh deliberate armed option, never a retry.
    let completed = host.completed(&agent, &admitted["work_id"]);
    s03_same_work(&completed, &original);
    let effect = agent.effect();
    assert_eq!(effect["value"], expected);
    assert_eq!(effect["commands"], 1);
    assert_eq!(effect["rpc_id"], original["rpc_id"]);
    assert_eq!(effect["session_id"], exact["session_id"]);
    assert_eq!(agent.root.join("tool-effect").exists(), expected == 42);
    let receipt: Value =
        serde_json::from_slice(&fs::read(agent.root.join("permission-response.json")).unwrap())
            .unwrap();
    assert_eq!(receipt["prompt_rpc_id"], original["rpc_id"]);
    assert_eq!(receipt["response"]["id"], 73);
    assert_eq!(
        receipt["response"]["result"]["outcome"],
        json!({"outcome":"selected","optionId":option})
    );
    let projection = host.inspect(&agent);
    let selected = projection["content"]["interactions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["interaction_id"] == permission["interaction_id"])
        .unwrap();
    assert_eq!(selected["revision"], permission["revision"]);
    assert_eq!(selected["scope"], permission["scope"]);
    assert_eq!(selected["selection"], json!({"option_id":option}));
    let answer = host.work(&agent, &selected["selected_work_id"]);
    assert_eq!(answer["op"], "acp-permission");
    assert_eq!(answer["interaction_id"], permission["interaction_id"]);
    assert_eq!(answer["controller_id"], controller);
    assert_eq!(answer["owner_generation"], generation);
    assert_eq!(answer["rpc_id"], 73);
    assert_eq!(
        projection["work"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|work| work["op"] == "acp-permission")
            .count(),
        1
    );
    assert_eq!(projection["target"], exact);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    assert!(!agent.root.join("unexpected-cancel").exists());
    finish(host, &mut agent, &mut native, &mut sentinel);
}

fn native_focus_receipt(
    host: &Host,
    agent: &Agent,
    native: &mut Attachment,
    controller: &Value,
    focused: bool,
    after: u64,
) -> Value {
    native.send(if focused { b"\x1b[I" } else { b"\x1b[O" });
    host.until("ordered original-connection focus report", || {
        ownership(host, agent)["views"]
            .as_array()?
            .iter()
            .find(|view| {
                view["controller_id"] == *controller
                    && view["connected"] == true
                    && view["reported"] == true
                    && view["focused"] == focused
                    && view["target"] == target(agent)
                    && view["serial"].as_u64().unwrap() > after
            })
            .cloned()
    })
}

#[test]
fn e07s02_native_queued_owner_y_cannot_yield_to_a_later_manual_request() {
    native_queued_owner("y");
}

#[test]
fn e07s02_native_queued_owner_repeat_y_cannot_yield_to_a_later_manual_request() {
    native_queued_owner("repeat-y");
}

#[test]
fn e07s02_native_queued_owner_focused_enter_cannot_yield_to_a_later_manual_request() {
    native_queued_owner("focused-enter");
}

#[test]
fn e07s02_native_queued_owner_focused_repeat_enter_cannot_yield_to_a_later_manual_request() {
    native_queued_owner("focused-repeat-enter");
}

#[test]
fn e07s02_native_queued_owner_focused_repeat_space_cannot_yield_to_a_later_manual_request() {
    native_queued_owner("focused-repeat-space");
}

#[test]
fn e07s02_native_queued_owner_mouse_cannot_yield_to_a_later_manual_request() {
    native_queued_owner("mouse");
}

fn native_queued_owner(route: &str) {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("s02-form");
    host.ready(&mut agent);
    let exact = agent.target.clone();
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let mut native = native_attachment(&host, &agent);
    let admitted = native_prompt(&host, &agent, &mut native, "add 17 25");
    let original = host.work(&agent, &admitted["work_id"]);
    let form = pending(&host, &agent, "form");
    acceptance::show_interaction(&host, &agent, &mut native, Some(&form));
    let foreground = host.inspect(&agent)["foreground"].clone();
    let grant = ownership(&host, &agent);
    let controller = grant["owner"]["controller_id"].clone();
    let generation = grant["observed"]["generation"].clone();
    let requester = external(&host);
    native.action(12);
    native.wait("Control owned here");
    native.wait("r Release");

    // This physical Release click predates the manual request. Repaint may
    // change the first mutation to Yield; the old gesture must not adopt it.
    let input = if route == "mouse" {
        native.click_input("Release")
    } else {
        if route.starts_with("focused-") {
            native.send(b"\t"); // first action is Release, not future Yield
        }
        match route {
            "y" => b"y".to_vec(),
            "repeat-y" => b"\x1b[121;1:2u".to_vec(),
            "focused-enter" => b"\r".to_vec(),
            "focused-repeat-enter" => b"\x1b[13;1:2u".to_vec(),
            "focused-repeat-space" => b"\x1b[32;1:2u".to_vec(),
            _ => unreachable!(),
        }
    };
    let reported = native_focus_receipt(&host, &agent, &mut native, &controller, true, 0);
    assert!(ownership(&host, &agent)["reservation"].is_null());

    // Stop only this original controlling-PTY group. Physical input predates
    // the reservation; FocusIn wakes a read already in progress so the next
    // parent iteration polls ownership before consuming the gesture.
    let group = -(native.child.id() as i32);
    assert_eq!(unsafe { libc::kill(group, libc::SIGSTOP) }, 0);
    let mut status = 0;
    assert_eq!(
        unsafe { libc::waitpid(native.child.id() as i32, &mut status, libc::WUNTRACED) },
        native.child.id() as i32
    );
    assert!(libc::WIFSTOPPED(status));
    let mut queued_input = b"\x1b[I".to_vec();
    queued_input.extend_from_slice(&input);
    queued_input.extend_from_slice(b"k\x1b[21~"); // local Keep working, then Details
    native.send(&queued_input);
    let mut queued = 0i32;
    assert_eq!(
        unsafe { libc::ioctl(native.slave.as_raw_fd(), libc::FIONREAD, &mut queued) },
        0
    );
    assert!(queued >= queued_input.len() as i32);
    let claim = host.call(json!({"op":"ownership-claim","target":target(&agent),
        "controller":requester["controller"],"mode":"cooperative",
        "when":{"kind":"now"},"observed":ownership(&host, &agent)["observed"]}));
    assert_eq!(claim["phase"], "waiting-owner");
    // Ownership's 100ms refresh is due before SIGCONT. This delay schedules
    // refresh; the ordered original-stream receipt below proves input drain.
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(unsafe { libc::kill(group, libc::SIGCONT) }, 0);
    native.wait("ACP settings (read-only)");
    native_focus_receipt(
        &host,
        &agent,
        &mut native,
        &controller,
        false,
        reported["serial"].as_u64().unwrap(),
    );

    let current = ownership(&host, &agent);
    assert_eq!(current["owner"]["controller_id"], controller, "{route}");
    assert_eq!(current["owner"]["role"], "attachment");
    assert_eq!(current["observed"]["generation"], generation);
    assert_eq!(current["reservation"]["claim_id"], claim["claim_id"]);
    assert_eq!(current["reservation"]["purpose"], "manual");
    assert_eq!(
        handle(&host, &claim["claim"], "read")["phase"],
        "waiting-owner"
    );
    let projection = host.inspect(&agent);
    assert_eq!(projection["target"], exact);
    assert_eq!(projection["foreground"], foreground);
    let retained = projection["content"]["interactions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["interaction_id"] == form["interaction_id"])
        .unwrap();
    assert_eq!(retained["state"], "pending");
    assert_eq!(retained["revision"], form["revision"]);
    assert_eq!(retained["scope"], form["scope"]);
    s03_same_work(&host.work(&agent, &admitted["work_id"]), &original);
    assert!(host.work(&agent, &admitted["work_id"])["outcome"].is_null());
    assert!(projection["work"].as_array().unwrap().iter().all(|work| {
        !matches!(
            work["op"].as_str(),
            Some("acp-answer" | "acp-permission" | "acp-authenticate" | "acp-interrupt")
        )
    }));
    assert!(!agent.root.join("effect.json").exists());
    assert!(!agent.root.join("unexpected-cancel").exists());
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);

    // Only a fresh gesture after this exact claim's armed repaint may yield.
    native.conversation();
    native.action(12);
    native.wait(claim["claim_id"].as_str().unwrap());
    native.wait("y Yield to this request");
    native.send(b"y");
    let offered = host.until("fresh exact request is offered, not auto-accepted", || {
        let state = handle(&host, &claim["claim"], "read");
        (state["phase"] == "offered").then_some(state)
    });
    assert!(ownership(&host, &agent)["owner"].is_null());
    assert_eq!(host.inspect(&agent)["foreground"], foreground);
    assert_eq!(
        pending(&host, &agent, "form")["interaction_id"],
        form["interaction_id"]
    );
    s03_same_work(&host.work(&agent, &admitted["work_id"]), &original);
    assert!(host.work(&agent, &admitted["work_id"])["outcome"].is_null());
    assert!(!agent.root.join("effect.json").exists());
    let accepted = handle(&host, &offered["offer"], "accept");
    agent.opened["control"] = accepted["control"].clone();
    agent.opened["generation"] = accepted["generation"].clone();
    agent.controller_id = requester["controller_id"].clone();
    native.observer();
    let answer = host.call(form_answer(
        &agent,
        &form,
        "fresh-requester-exact-form",
        "accept",
        Some(json!({"bonus":15})),
    ));
    let completed = host.completed(&agent, &admitted["work_id"]);
    s03_same_work(&completed, &original);
    let selected = host.work(&agent, &answer["work_id"]);
    assert_eq!(selected["controller_id"], requester["controller_id"]);
    assert_eq!(selected["owner_generation"], accepted["generation"]);
    assert_eq!(selected["interaction_id"], form["interaction_id"]);
    assert_eq!(agent.effect()["value"], 57);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(agent.effect()["rpc_id"], original["rpc_id"]);
    assert_eq!(agent.effect()["session_id"], exact["session_id"]);
    assert_eq!(host.inspect(&agent)["target"], exact);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    assert!(!agent.root.join("unexpected-cancel").exists());
    finish(host, &mut agent, &mut native, &mut sentinel);
}

fn native_auth_ready(native: &mut Attachment, selected: &str, other: &str) {
    // Observe which action actually has focus, not merely two different styles:
    // that inequality alone also accepts the inverse (the old action still focused).
    // Details is the inactive tab in this conversation, so it supplies the real
    // unselected style without pinning the driver's readiness check to theme colors.
    let deadline = Instant::now() + DEADLINE;
    loop {
        let text = native.text();
        let position = |caption: &str| {
            text.lines()
                .enumerate()
                .filter_map(|(row, line)| {
                    let offset = line.rfind(caption)?;
                    Some((row, line, offset))
                })
                .last()
        };
        if let (
            Some((row, line, offset)),
            Some((other_row, other_line, other_offset)),
            Some((inactive_row, inactive_line, inactive_offset)),
        ) = (position(selected), position(other), position("Details"))
        {
            let armed = line[..offset]
                .chars()
                .rev()
                .find(|c| matches!(*c, '[' | '('))
                == Some('[');
            let column = unicode_width::UnicodeWidthStr::width(&line[..offset]);
            let other_column = unicode_width::UnicodeWidthStr::width(&other_line[..other_offset]);
            let inactive_column =
                unicode_width::UnicodeWidthStr::width(&inactive_line[..inactive_offset]);
            let output = native.output.lock();
            let selected_cell = &output.term.grid()[Point::new(Line(row as i32), Column(column))];
            let other_cell =
                &output.term.grid()[Point::new(Line(other_row as i32), Column(other_column))];
            let inactive_cell =
                &output.term.grid()[Point::new(Line(inactive_row as i32), Column(inactive_column))];
            if armed && selected_cell.bg != inactive_cell.bg && other_cell.bg == inactive_cell.bg {
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "auth action {selected:?} not armed/focused:\n{text}"
        );
        assert!(
            native.child.try_wait().unwrap().is_none(),
            "native auth attachment exited"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn e07s02_native_broker_auth_choices_f8_selects_exact_alternate_method() {
    native_broker_auth_choices("f8", "agent-login-alt", 49);
}

#[test]
fn e07s02_native_broker_auth_choices_tab_enter_selects_exact_primary_method() {
    native_broker_auth_choices("tab-enter", "agent-login", 45);
}

#[test]
fn e07s02_native_broker_auth_choices_mouse_selects_exact_not_highlighted_method() {
    native_broker_auth_choices("mouse", "agent-login-alt", 49);
}

fn native_broker_auth_choices(route: &str, method: &str, expected: i64) {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("auth-multiple");
    let initial = host.until("real broker requires an explicit auth choice", || {
        let projection = host.inspect(&agent);
        (projection["lifecycle"] == "auth-required").then_some(projection)
    });
    let exact = initial["target"].clone();
    assert!(exact["session_id"].is_null());
    agent.target = exact.clone();
    let methods = json!([
        {"method_id":"agent-login","kind":"agent"},
        {"method_id":"agent-login-alt","kind":"agent"},
        {"method_id":"client-login","kind":"unsupported"},
        {"method_id":"terminal-login","kind":"terminal"}
    ]);
    // No fabricated names or locally upgraded legacy projection: these are the
    // broker's actual retained public and content representations.
    assert_eq!(initial["authentication"], methods);
    assert_eq!(initial["content"]["authentication"], methods);
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let mut native = native_attachment(&host, &agent);
    native.projected(&host, initial["seq"].as_u64().unwrap());
    for caption in [
        "agent-login [agent]",
        "agent-login-alt [agent]",
        "client-login [unsupported]",
        "terminal-login [terminal]",
    ] {
        native.wait(caption);
    }
    let owner = ownership(&host, &agent);
    let controller = owner["owner"]["controller_id"].clone();
    let generation = owner["observed"]["generation"].clone();
    let reported = native_focus_receipt(&host, &agent, &mut native, &controller, true, 0);
    assert!(!agent.root.join("authenticate.received").exists());
    native.click("Prompt");
    match route {
        "f8" => {
            native.send(b"\x1b[B\x1b[B"); // unsupported client method, index2
            native_auth_ready(
                &mut native,
                "client-login [unsupported]",
                "agent-login [agent]",
            );
            native.send(b"\x1b[19~");
        }
        "tab-enter" => {
            native.send(b"\t\t\t"); // Prompt -> primary -> alternate -> client
            native_auth_ready(
                &mut native,
                "client-login [unsupported]",
                "agent-login [agent]",
            );
            native.send(b"\r");
        }
        "mouse" => {
            // Prompt may already show this selection in an old armed frame.
            // Fence that click before the fresh mouse gesture without resetting
            // keyboard focus in the Tab/Enter path.
            native.details();
            native.conversation();
            native_auth_ready(
                &mut native,
                "agent-login [agent]",
                "agent-login-alt [agent]",
            );
            native.click("client-login [unsupported]");
            native_auth_ready(
                &mut native,
                "client-login [unsupported]",
                "agent-login [agent]",
            );
        }
        _ => unreachable!(),
    }
    // Read-only Details and the original stream's following report fence
    // dispatch of the unsupported gesture, not just a transient paint.
    native.details();
    native_focus_receipt(
        &host,
        &agent,
        &mut native,
        &controller,
        false,
        reported["serial"].as_u64().unwrap(),
    );
    let rejected = host.inspect(&agent);
    assert_eq!(rejected["target"], exact);
    assert_eq!(rejected["lifecycle"], "auth-required");
    assert_eq!(rejected["authentication"], methods);
    assert!(
        rejected["work"]
            .as_array()
            .unwrap()
            .iter()
            .all(|work| { work["op"] != "acp-authenticate" && work["op"] != "acp-prompt" })
    );
    for file in [
        "authenticate.received",
        "authenticate-request.json",
        "effect.json",
    ] {
        assert!(
            !agent.root.join(file).exists(),
            "unsupported {route} reached {file}"
        );
    }
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    native.conversation();
    native.click("Prompt");
    match route {
        "f8" => {
            native.send(b"\x1b[A"); // client -> alternate, not default primary
            native_auth_ready(
                &mut native,
                "agent-login-alt [agent]",
                "agent-login [agent]",
            );
            native.send(b"\x1b[19~");
        }
        "tab-enter" => {
            native.send(b"\t"); // Prompt -> primary, regardless of old client choice
            native_auth_ready(
                &mut native,
                "agent-login [agent]",
                "client-login [unsupported]",
            );
            native.send(b"\r");
        }
        "mouse" => {
            native.send(b"\x1b[A\x1b[A"); // visibly highlight primary first
            native_auth_ready(
                &mut native,
                "agent-login [agent]",
                "client-login [unsupported]",
            );
            native.click("agent-login-alt [agent]");
        }
        _ => unreachable!(),
    }
    host.barrier(&agent, "authenticate");
    let request: Value =
        serde_json::from_slice(&fs::read(agent.root.join("authenticate-request.json")).unwrap())
            .unwrap();
    assert_eq!(request["method"], "authenticate");
    assert_eq!(request["params"]["methodId"], method);
    let auth = host.until(
        "exact original native authentication work is retained",
        || {
            host.inspect(&agent)["work"]
                .as_array()?
                .iter()
                .find(|work| work["op"] == "acp-authenticate")
                .cloned()
        },
    );
    assert_eq!(auth["controller_id"], controller);
    assert_eq!(auth["owner_generation"], generation);
    assert_eq!(auth["rpc_id"], request["id"]);
    assert!(auth["outcome"].is_null());
    assert_eq!(host.inspect(&agent)["target"], exact);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    agent.release("authenticate");
    host.ready(&mut agent);
    for key in ["kind", "id", "instance_id", "connection_generation"] {
        assert_eq!(
            agent.target[key], exact[key],
            "authentication replaced {key}"
        );
    }
    assert!(agent.target["session_id"].is_string());
    let completed_auth = host.completed(&agent, &auth["work_id"]);
    s03_same_work(&completed_auth, &auth);
    assert_eq!(completed_auth["outcome"]["kind"], "response");
    let ready_projection = host.inspect(&agent);
    native.projected(&host, ready_projection["seq"].as_u64().unwrap());
    let prompt = native_prompt(&host, &agent, &mut native, "add 17 25");
    let completed = host.completed(&agent, &prompt["work_id"]);
    assert_eq!(completed["controller_id"], controller);
    assert_eq!(completed["owner_generation"], generation);
    assert_eq!(agent.effect()["value"], expected); // method-specific real effect
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(agent.effect()["rpc_id"], completed["rpc_id"]);
    assert_eq!(agent.effect()["session_id"], agent.target["session_id"]);
    let projection = host.inspect(&agent);
    assert_eq!(projection["target"], agent.target);
    assert_eq!(
        projection["work"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|work| work["op"] == "acp-authenticate")
            .count(),
        1
    );
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    assert!(!agent.root.join("unexpected-cancel").exists());
    finish(host, &mut agent, &mut native, &mut sentinel);
}

#[test]
fn e07s03_native_last_window_removal_keeps_original_acp_question_selectable() {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let snapshot = host.call(json!({"op":"snapshot"}));
    let terminals = snapshot["terminals"].as_array().unwrap();
    assert_eq!(
        terminals.len(),
        1,
        "fixture must begin with exactly one native"
    );
    let terminal = terminals[0]["id"].clone();
    let shell_pid = terminals[0]["pid"].as_i64().unwrap() as i32;
    host.call(json!({"op":"name","terminal":terminal,"name":"Last native"}));
    let mut agent = host.open("s02-form");
    host.ready(&mut agent);
    let exact = agent.target.clone();
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let mut native = native_attachment(&host, &agent);
    let admitted = native_prompt(&host, &agent, &mut native, "add 17 25");
    let original = host.work(&agent, &admitted["work_id"]);
    let form = pending(&host, &agent, "form");
    acceptance::show_interaction(&host, &agent, &mut native, Some(&form));
    let grant = ownership(&host, &agent);
    let controller = grant["owner"]["controller_id"].clone();
    let generation = grant["observed"]["generation"].clone();

    native.send(b"\x1b[24~"); // Menu: select/open the one existing native shell.
    native.wait("Last native");
    native.send(b"\x1b[H\r");
    native.wait("ACP_FIXTURE>");
    host.until("same attachment owns the selected native shell", || {
        host.call(json!({"op":"snapshot"}))["terminals"]
            .as_array()?
            .iter()
            .find(|item| item["id"] == terminal)
            .filter(|item| item["ownership"]["owner"]["controller_id"] == controller)
            .cloned()
    });
    native.action(12);
    native.wait("Control owned here");
    let native_activity =
        host.call(json!({"op":"snapshot"}))["terminals"][0]["ownership"]["activity_revision"]
            .as_u64()
            .unwrap();
    native.send(b"\x1b");
    host.until(
        "original native owner parses Escape and dismisses its panel",
        || {
            let snapshot = host.call(json!({"op":"snapshot"}));
            let activity = snapshot["terminals"][0]["ownership"]["activity_revision"].as_u64()?;
            (activity > native_activity && !native.text().contains("Control owned here"))
                .then_some(())
        },
    );
    native.wait("ACP_FIXTURE>");
    native.send(b"exit\r"); // Normal stock-shell exit, not a close/kill substitute.
    host.until("original native shell has normally exited", || {
        host.call(json!({"op":"snapshot"}))["terminals"]
            .as_array()?
            .iter()
            .find(|item| item["id"] == terminal)
            .filter(|item| item["alive"] == false && item["exit_status"] == 0)
            .cloned()
    });
    host.until("original shell PID is absent before record removal", || {
        pid_gone(shell_pid).then_some(())
    });
    native.send(b"\x1b[24~");
    native.wait("exit 0");
    native.send(b"\x1b[Hx"); // Menu Close removes only the observed exited record.
    host.until(
        "Menu removed the last native and retained the ACP view",
        || {
            let snapshot = host.call(json!({"op":"snapshot"}));
            (snapshot["terminals"].as_array()?.is_empty()
                && snapshot["agent_views"].as_array()?.len() == 1
                && snapshot["agent_views"][0]["id"] == exact["id"])
                .then_some(())
        },
    );
    assert_eq!(pending(&host, &agent, "form"), form);
    s03_same_work(&host.work(&agent, &admitted["work_id"]), &original);
    assert!(host.work(&agent, &admitted["work_id"])["outcome"].is_null());
    assert!(!agent.root.join("effect.json").exists());
    native.wait("Boundary peer"); // Surviving real row, not an empty-window card.
    native.send(b"\x1b[H\r"); // Must open that ACP, never create a new native shell.
    native.owner();
    acceptance::show_interaction(&host, &agent, &mut native, Some(&form));
    let current = ownership(&host, &agent);
    assert_eq!(current["owner"]["controller_id"], controller);
    assert_eq!(current["observed"]["generation"], generation);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    assert_eq!(host.inspect(&agent)["target"], exact);
    assert!(
        host.call(json!({"op":"snapshot"}))["terminals"]
            .as_array()
            .unwrap()
            .is_empty(),
        "opening the surviving ACP must not launch a replacement shell"
    );

    native.send(b"\x1b[14~");
    native.wait("UNSET (F4)");
    native.paste("15");
    native.wait("local value");
    native.ready_action("Review (F5)");
    native.send(ACCEPT_FORM);
    native.wait("Submit (F5)");
    native.send(ACCEPT_FORM);
    let completed = host.completed(&agent, &admitted["work_id"]);
    s03_same_work(&completed, &original);
    assert_eq!(agent.effect()["value"], 57);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(agent.effect()["rpc_id"], original["rpc_id"]);
    assert_eq!(agent.effect()["session_id"], exact["session_id"]);
    let projection = host.inspect(&agent);
    let selected = projection["content"]["interactions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["interaction_id"] == form["interaction_id"])
        .unwrap();
    assert_eq!(selected["revision"], form["revision"]);
    assert_eq!(selected["scope"], form["scope"]);
    assert_eq!(
        selected["selection"],
        json!({"action":"accept","content":{"bonus":15}})
    );
    let answer = host.work(&agent, &selected["selected_work_id"]);
    assert_eq!(answer["controller_id"], controller);
    assert_eq!(answer["owner_generation"], generation);
    assert_eq!(answer["interaction_id"], form["interaction_id"]);
    assert_eq!(projection["target"], exact);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    assert!(
        host.call(json!({"op":"snapshot"}))["terminals"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(!agent.root.join("unexpected-cancel").exists());
    finish(host, &mut agent, &mut native, &mut sentinel);
}

#[test]
fn e07s04_native_definite_launch_rejection_retains_draft_until_fresh_launch() {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut anchor = host.open("arithmetic");
    host.ready(&mut anchor);
    let anchor_target = anchor.target.clone();
    let anchor_pid = fs::read(anchor.root.join("peer.pid")).unwrap();
    let anchor_project =
        host.view(anchor.target["id"].as_str().unwrap()).unwrap()["project_id"].clone();
    let initial = host.call(json!({"op":"snapshot"}));
    let native_ids = |snapshot: &Value| {
        snapshot["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .map(|terminal| terminal["id"].clone())
            .collect::<Vec<_>>()
    };
    let root = host.root.join("rejected-launch-peer");
    fs::create_dir(&root).unwrap();
    let wrapper = host.root.join("owned-acp-wrapper");
    let wrapper_source = "#!/bin/zsh\nprintf invoked > \"$2/wrapper-invoked\"\nexec \"$@\"\n";
    fs::write(&wrapper, wrapper_source).unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).unwrap();
    let peer = Path::new(env!("CARGO_BIN_EXE_amux"))
        .parent()
        .unwrap()
        .join("examples/acp_fixture");
    host.call(
        json!({"op":"config","config":{"acp_profiles":{"rejection-fixture":{
        "label":"Owned rejection fixture","command":[wrapper,peer,root,"e06-settings"],
        "defaults":{"model":"fixture/cheap","mode":"plan","effort":"low"}}}}}),
    );
    let mut native = native_attachment(&host, &anchor);
    native.send(PREFIX);
    native.send(b"\x1b[B");
    native.send(b"\x01"); // Existing explicit New ACP action, no replacement shell.
    native.wait("New ACP session");
    native.send(b"\t");
    for _ in 0..20 {
        if native.text().contains("Owned rejection fixture") {
            break;
        }
        let before = native
            .text()
            .lines()
            .find(|line| line.contains("Profile:"))
            .map(str::to_owned);
        native.send(b"\x1b[C");
        host.until(
            "explicit profile selection reaches a new rendered choice",
            || {
                let text = native.text();
                (text
                    .lines()
                    .find(|line| line.contains("Profile:"))
                    .map(str::to_owned)
                    != before)
                    .then_some(())
            },
        );
    }
    native.wait("Owned rejection fixture");
    native.send(b"\t");
    native.paste("Retained rejected launch");
    native.click("Advanced");
    for (field, value) in [
        ("Model:", "fixture/cheap"),
        ("Mode:", "plan"),
        ("Effort:", "high"),
        ("Options JSON:", r#"{"fixture-detail":"brief"}"#),
    ] {
        native.click(field);
        native.send(b"\x15");
        native.paste(value);
        native.wait(value);
    }
    // The catalog and unchanged form observed a real available executable.
    // Remove only this test's wrapper: local validation still succeeds, while
    // the daemon definitively refuses admission before any peer reservation.
    fs::remove_file(&wrapper).unwrap();
    native.click("Launch");
    native.wait("ACP adapter executable is unavailable");
    native.wait("Retained rejected launch");
    native.wait("Owned rejection fixture");
    for field in [
        "Model: fixture/cheap",
        "Mode: plan",
        "Effort: high",
        r#"{"fixture-detail":"brief"}"#,
    ] {
        native.wait(field);
    }
    let rejected = host.call(json!({"op":"snapshot"}));
    assert_eq!(native_ids(&rejected), native_ids(&initial));
    assert_eq!(rejected["agent_views"].as_array().unwrap().len(), 1);
    assert!(!root.join("peer.pid").exists());
    assert!(!root.join("wrapper-invoked").exists());
    assert_eq!(host.inspect(&anchor)["target"], anchor_target);
    assert_eq!(fs::read(anchor.root.join("peer.pid")).unwrap(), anchor_pid);

    // Restoring availability is not consent to retry. A fresh read-only Help
    // transition exposes the full retained server cause while the wrapper is
    // available again, without another launch or any lost field/Advanced edit.
    fs::write(&wrapper, wrapper_source).unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).unwrap();
    native.send(b"\x1bOP");
    native.wait("Shortcuts");
    native.send(b"\x1b[F");
    native.wait("LAST ACTION / ERROR");
    native.wait("ACP adapter executable is");
    native.wait("unavailable"); // complete cause spans soft-wrapped help rows
    assert!(!root.join("wrapper-invoked").exists());
    assert_eq!(
        host.call(json!({"op":"snapshot"}))["agent_views"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    native.send(b"\x1b");
    native.wait("New ACP session");
    native.wait("Retained rejected launch");
    native.wait("Effort: high");
    native.click("Launch"); // Exactly one deliberate fresh corrected attempt.
    let view = host.until(
        "fresh launch admits the unchanged retained settings",
        || {
            host.call(json!({"op":"snapshot"}))["agent_views"]
                .as_array()?
                .iter()
                .find(|view| {
                    view["name"] == "Retained rejected launch" && view["lifecycle"] == "ready"
                })
                .cloned()
        },
    );
    let mut agent = Agent {
        opened: Value::Null,
        target: json!({"kind":"acp","id":view["id"],"instance_id":view["instance_id"],
            "connection_generation":view["connection_generation"],"session_id":view["session_id"]}),
        root,
        controller_id:
            host.view(view["id"].as_str().unwrap()).unwrap()["ownership"]["owner"]["controller_id"]
                .clone(),
    };
    native.wait_header("Retained rejected launch");
    native.owner();
    let configuration = host.inspect(&agent)["configuration"].clone();
    assert_eq!(configuration["requested"]["model"], "fixture/cheap");
    assert_eq!(configuration["requested"]["mode"], "plan");
    assert_eq!(configuration["requested"]["effort"], "high");
    assert_eq!(
        configuration["requested"]["options"]["fixture-detail"],
        "brief"
    );
    assert_eq!(
        configuration["reported"]["model"]["current"],
        "fixture/cheap"
    );
    assert_eq!(configuration["reported"]["mode"]["current"], "plan");
    assert_eq!(configuration["reported"]["effort"]["current"], "high");
    assert_eq!(view["project_id"], anchor_project);
    let snapshot = host.call(json!({"op":"snapshot"}));
    assert_eq!(snapshot["agent_views"].as_array().unwrap().len(), 2);
    assert_eq!(native_ids(&snapshot), native_ids(&initial));
    let admitted = native_prompt(&host, &agent, &mut native, "add 17 25");
    let completed = host.completed(&agent, &admitted["work_id"]);
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(agent.effect()["rpc_id"], completed["rpc_id"]);
    assert_eq!(agent.effect()["session_id"], agent.target["session_id"]);
    force(&host, &mut anchor);
    host.close(&anchor);
    finish(host, &mut agent, &mut native, &mut sentinel);
}

#[test]
fn e04s03_native_cooperative_question_handoff_discards_old_form_draft_on_reacquisition() {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("s02-form");
    host.ready(&mut agent);
    let exact = agent.target.clone();
    let mut native = native_attachment(&host, &agent);
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let first = native_prompt(&host, &agent, &mut native, "add 17 25");
    let form = pending(&host, &agent, "form");
    let rpc = host.work(&agent, &first["work_id"])["rpc_id"].clone();
    let sequence = host.inspect(&agent)["seq"].as_u64().unwrap();
    native.projected(&host, sequence);
    native.send(b"\x1b[14~"); // F4: explicit unset, not the default 0
    native.paste("99");
    native.wait("99");
    let b = external(&host);
    let claim = host.call(json!({"op":"ownership-claim","target":target(&agent),
        "controller":b["controller"],"mode":"cooperative","when":{"kind":"now"},
        "observed":ownership(&host, &agent)["observed"]}));
    native.action(12);
    // "Preparing Yield" is visible before old input is drained and the action
    // is armed. Send this deliberate choice only after the actionable repaint.
    native.wait("y Yield to this request");
    native.send(b"y");
    let offered = host.until("offer during original pending question", || {
        let state = handle(&host, &claim["claim"], "read");
        (state["phase"] == "offered").then_some(state)
    });
    assert!(ownership(&host, &agent)["owner"].is_null());
    assert_eq!(
        pending(&host, &agent, "form")["interaction_id"],
        form["interaction_id"]
    );
    assert_eq!(host.work(&agent, &first["work_id"])["rpc_id"], rpc);
    assert_ne!(host.work(&agent, &first["work_id"])["state"], "completed");
    let grant = handle(&host, &offered["offer"], "accept");
    agent.opened["control"] = grant["control"].clone();
    agent.opened["generation"] = grant["generation"].clone();
    agent.controller_id = b["controller_id"].clone();
    native.observer();
    native.send(ACCEPT_FORM); // intercepted observer choice must never be replayed
    host.call(form_answer(
        &agent,
        &form,
        "new-owner-reviewed-answer",
        "accept",
        Some(json!({"bonus":15})),
    ));
    let completed = host.completed(&agent, &first["work_id"]);
    assert_eq!(completed["rpc_id"], rpc);
    assert_eq!(agent.effect()["value"], 57);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    assert_eq!(host.inspect(&agent)["target"], exact);
    handle(&host, &grant["control"], "release");
    native.action(12); // fresh deliberate cooperative request, not auto-reacquire
    native.owner();
    // A deliberate claim may leave its ownership panel open. Dismiss it before
    // application input; the original-connection activity receipt fences Escape
    // parsing before returning deliberately to the prompt.
    let activity = ownership(&host, &agent)["activity_revision"]
        .as_u64()
        .unwrap();
    native.send(b"\x1b");
    host.until("fresh owner dismisses ownership panel", || {
        let text = native.text();
        (ownership(&host, &agent)["activity_revision"]
            .as_u64()
            .unwrap()
            > activity
            && !text.contains("Control owned here")
            && !text.contains("Control requested"))
        .then_some(())
    });
    native.click("Prompt");
    // Authority can arrive before the editor's next canonical refresh. A fresh
    // human prompt waits for rendered readiness; busy input is never replayed.
    native.idle();
    let second = native_prompt(&host, &agent, &mut native, "add 1 2");
    let next = pending(&host, &agent, "form");
    assert_ne!(next["interaction_id"], form["interaction_id"]);
    let sequence = host.inspect(&agent)["seq"].as_u64().unwrap();
    native.projected(&host, sequence);
    native.send(b"\x1b[14~"); // F4 explicitly clears the fresh field before editing
    native.wait("UNSET (F4)");
    native.paste("7");
    native.wait("local value");
    native.ready_action("Review (F5)");
    native.send(ACCEPT_FORM); // validate/review the new interaction
    native.wait("Submit (F5)"); // observe the fresh, input-drained reviewed action
    assert_eq!(
        pending(&host, &agent, "form")["interaction_id"],
        next["interaction_id"]
    );
    native.send(ACCEPT_FORM); // deliberate fresh answer, never the stale 99
    host.completed(&agent, &second["work_id"]);
    assert_eq!(agent.effect()["value"], 67);
    assert_eq!(agent.effect()["commands"], 2);
    assert_eq!(host.inspect(&agent)["target"], exact);
    let projection = host.inspect(&agent);
    let selected = projection["content"]["interactions"].as_array().unwrap();
    assert_eq!(
        selected
            .iter()
            .find(|item| item["interaction_id"] == form["interaction_id"])
            .unwrap()["selection"]["content"]["bonus"],
        15
    );
    assert_eq!(
        selected
            .iter()
            .find(|item| item["interaction_id"] == next["interaction_id"])
            .unwrap()["selection"]["content"]["bonus"],
        7
    );
    assert!(!agent.root.join("unexpected-cancel").exists());
    finish(host, &mut agent, &mut native, &mut sentinel);
}

#[test]
fn e04s03_native_original_connection_activity_is_not_borrowed_control_provenance() {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("arithmetic");
    host.ready(&mut agent);
    host.call(json!({"op":"config","config":{"prefix":"Ctrl-a"}}));
    let mut original = OriginalNative::new(&host);
    original.acquire(&host, &mut agent);
    let mut native = Attachment::new(&host, &agent);
    native.observer();
    // Control is intentionally borrowable, original-attachment activity is not.
    let before = ownership(&host, &agent)["activity_revision"]
        .as_u64()
        .unwrap();
    let borrowed = host.prompt(&agent, "borrowed-native-control", "add 17 25");
    host.completed(&agent, &borrowed["work_id"]);
    assert_eq!(ownership(&host, &agent)["activity_revision"], before);
    host.deny(
        json!({"op":"control-handle","handle":agent.opened["control"],
        "method":"activity"}),
        &["not-native-connection"],
    );
    let mut action = agent.action("acp-prompt", "original-connection-prompt");
    action["text"] = json!("add 1 2");
    let admitted = original.call(action);
    host.completed(&agent, &admitted["work_id"]);
    assert_eq!(
        ownership(&host, &agent)["activity_revision"].as_u64(),
        Some(before + 1)
    );
    assert_eq!(agent.effect()["value"], 45);
    assert_eq!(agent.effect()["commands"], 2);
    assert_eq!(
        host.work(&agent, &admitted["work_id"])["controller_id"],
        agent.controller_id
    );
    let snapshot = host.call(json!({"op":"snapshot"}));
    assert!(
        snapshot["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .all(|terminal| {
                terminal["ownership"]["unsettled"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|call| call["controller_id"] != agent.controller_id)
            }),
        "ACP action fabricated a native-input batch"
    );
    drop(original);
    finish(host, &mut agent, &mut native, &mut sentinel);
}

#[test]
fn e04s03_native_gap_resync_renders_hostile_retained_updates_inertly() {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("s03-rendering");
    host.ready(&mut agent);
    let exact = agent.target.clone();
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let prompt = host.prompt(&agent, "mixed-gap-updates", "add 17 25");
    host.barrier(&agent, "s03-updates");
    let initial = host.call(json!({"op":"acp-inspect","target":exact,
        "include_content":true,"after":0,"limit":128}));
    assert_eq!(
        initial["content"]["gap"], true,
        "flood must create canonical gap"
    );
    let mut after = 0;
    let mut events = Vec::new();
    host.until("retained tail canonically consumed after gap", || {
        let projection = host.call(json!({"op":"acp-inspect","target":exact,
            "include_content":true,"after":after,"limit":128}));
        assert_eq!(projection["target"], exact);
        after = projection["content"]["through"].as_u64().unwrap();
        events.extend(
            projection["content"]["events"]
                .as_array()
                .unwrap()
                .iter()
                .cloned(),
        );
        events
            .iter()
            .any(|event| event["text"] == "s03-tail-retained-2112")
            .then_some(())
    });
    for kind in ["agent-message", "thought", "tool", "plan"] {
        assert!(
            events.iter().any(|event| event["kind"] == kind),
            "lost category {kind}"
        );
    }
    assert!(
        events.iter().all(|event| event["prompt_id"].is_null()),
        "unscoped agent updates were guessed into a foreground prompt"
    );
    // Inspect exposes canonical inert content, not the peer's raw transport.
    let hostile = concat!(
        "s03-hostile:\u{fffd}]0;s03-title-owned\u{fffd}",
        "\u{fffd}]52;c;czAzLWNsaXBib2FyZC1vd25lZA==\u{fffd}",
        "\u{fffd}[2J\u{fffd}[H\u{fffd}[31ms03-csi-marker\u{fffd}[0m",
        "\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}s03-bidi\u{fffd}"
    );
    assert!(events.iter().any(|event| event["text"] == hostile));
    let metadata = host.call(json!({"op":"acp-inspect","target":exact}));
    for public in [metadata, host.call(json!({"op":"snapshot"}))] {
        let public = serde_json::to_string(&public).unwrap();
        for marker in [
            "s03-hostile",
            "s03-seq-",
            "s03-tail-retained",
            "s03-thought-retained",
            "s03-tool-retained",
            "s03-plan-retained",
            "s03-title-owned",
            "s03-clipboard-owned",
        ] {
            assert!(
                !public.contains(marker),
                "raw content in public metadata: {marker}"
            );
        }
    }
    host.call(json!({"op":"config","config":{"prefix":"Ctrl-a"}}));
    // New actual attachment starts from cursor zero after canonical eviction.
    let mut native = Attachment::new(&host, &agent);
    native.observer();
    native.wait("gap");
    native.click("History");
    native.wait("s03-tail-retained-2112");
    native.wait("s03-csi-marker");
    native.wait("s03-bidi");
    let output = native.output.lock();
    for forbidden in [
        b"\x1b]0;s03-title-owned".as_slice(),
        b"\x1b]52;c;czAzLWNsaXBib2FyZC1vd25lZA==",
        b"\x1b[31ms03-csi-marker",
        "\u{202e}s03-bidi".as_bytes(),
    ] {
        assert!(
            !output
                .raw
                .windows(forbidden.len())
                .any(|window| window == forbidden),
            "agent-specified terminal instruction escaped inert rendering"
        );
    }
    drop(output);
    assert!(
        !native.effects.lock().iter().any(|effect| {
            effect.contains("s03-title-owned") || effect.contains("s03-clipboard-owned")
        }),
        "host title/clipboard marker effect"
    );
    assert_eq!(host.inspect(&agent)["target"], exact);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    assert_ne!(host.work(&agent, &prompt["work_id"])["state"], "completed");
    agent.release("s03-updates");
    let completed = host.completed(&agent, &prompt["work_id"]);
    assert_eq!(completed["outcome"]["stop_reason"], "end_turn");
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(agent.effect()["rpc_id"], completed["rpc_id"]);
    finish(host, &mut agent, &mut native, &mut sentinel);
}

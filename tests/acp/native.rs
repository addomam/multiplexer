//! Real native attachment boundaries. Parent registers this child module.
use super::*;
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
                &Geometry,
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
        (0..ROWS)
            .map(|row| {
                (0..COLS)
                    .map(|col| output.term.grid()[Point::new(Line(row as i32), Column(col))].c)
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
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
            assert!(Instant::now() < deadline, "missing {marker:?}:\n{text}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        write_pty(&mut self.master, bytes).unwrap();
    }
    fn paste(&mut self, text: &str) {
        self.send(format!("\x1b[200~{text}\x1b[201~").as_bytes());
    }
    fn action(&mut self, key: u8) {
        self.send(PREFIX);
        self.send(&[key]);
    }
    fn owner(&mut self) {
        self.wait("owner (input-admission)");
    }
    fn observer(&mut self) {
        self.wait("observer (read-only)");
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
        self.stop.store(true, Ordering::Release);
        unsafe {
            libc::tcsetattr(self.slave.as_raw_fd(), libc::TCSANOW, &self.original);
        }
        // Signal only a known-running original leader; an observation error is not a pin.
        if self.child.try_wait().is_ok_and(|status| status.is_none()) {
            unsafe {
                libc::kill(-(self.child.id() as i32), libc::SIGKILL);
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        // Reads are nonblocking; cursor-reply writes have a deadline. Join even
        // during unwinding, rather than detaching an owned worker on timeout.
        if let Some(reader) = self.reader.take()
            && reader.join().is_err()
        {
            eprintln!("native PTY drainer panicked during cleanup");
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
    native.paste(text);
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
    native.wait("Force control?");
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
    native.wait(form["interaction_id"].as_str().unwrap());
    native.send(b"\x1b[14~"); // F4: explicit unset, not the default 0
    native.paste("99");
    native.wait("99");
    let b = external(&host);
    let claim = host.call(json!({"op":"ownership-claim","target":target(&agent),
        "controller":b["controller"],"mode":"cooperative","when":{"kind":"now"},
        "observed":ownership(&host, &agent)["observed"]}));
    native.action(12);
    native.wait("Yield");
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
    // parsing so the following F9 cannot be interpreted as Alt-F9.
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
    native.send(b"\x1b[20~"); // F9 leaves the selected old interaction for the prompt editor
    // Authority can arrive before the editor's next canonical refresh. A fresh
    // human prompt waits for rendered readiness; busy input is never replayed.
    native.wait("Foreground: none");
    let second = native_prompt(&host, &agent, &mut native, "add 1 2");
    let next = pending(&host, &agent, "form");
    assert_ne!(next["interaction_id"], form["interaction_id"]);
    native.wait(next["interaction_id"].as_str().unwrap());
    native.send(b"\x1b[14~"); // F4 explicitly clears the fresh field before editing
    native.paste("7");
    native.send(ACCEPT_FORM); // validate/review the new interaction
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

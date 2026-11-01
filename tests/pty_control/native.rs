use super::*;
use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Config;
use alacritty_terminal::vte::ansi::Processor;
use std::os::fd::{FromRawFd, OwnedFd};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

const OUTER_OUTPUT_LIMIT: usize = 16 * 1024 * 1024;
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(3);

struct OuterOutput {
    term: Term<Listener>,
    raw: Vec<u8>,
    error: Option<String>,
}

fn visible_lines(term: &Term<Listener>) -> Vec<String> {
    (0..term.screen_lines())
        .map(|row| {
            (0..term.columns())
                .map(|col| term.grid()[Point::new(Line(row as i32), Column(col))].c)
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

fn drain_outer(
    mut master: fs::File,
    output: Arc<parking_lot::Mutex<OuterOutput>>,
    stop: Arc<AtomicBool>,
) {
    let mut parser: Processor = Processor::new();
    let mut bytes = [0; 32768];
    while !stop.load(Ordering::Acquire) {
        match master.read(&mut bytes) {
            Ok(0) => break,
            Ok(n) => {
                let mut output = output.lock();
                if output.raw.len().saturating_add(n) <= OUTER_OUTPUT_LIMIT {
                    output.raw.extend_from_slice(&bytes[..n]);
                } else if output.error.is_none() {
                    output.error =
                        Some("outer PTY evidence exceeded 16 MiB; draining continues".into());
                }
                parser.advance(&mut output.term, &bytes[..n]);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
            Err(error) => {
                output.lock().error = Some(format!("outer PTY read: {error}"));
                break;
            }
        }
    }
}

// A failed fixture must not hang the test process in wait/join. All workers
// receive cancellation first; join only after observing actual termination.
fn join_until(worker: std::thread::JoinHandle<()>, deadline: Instant) -> bool {
    while !worker.is_finished() {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    worker.join().is_ok()
}

const COLS: usize = 110;
const ROWS: usize = 28;
const PREFIX: &[u8] = b"\x01";

#[derive(Clone)]
struct Listener;
impl EventListener for Listener {
    fn send_event(&self, _: Event) {}
}
struct Geometry {
    cols: usize,
    rows: usize,
}
impl Dimensions for Geometry {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

// This is a real session leader with a controlling terminal, not piped stdin or
// an injected crossterm event. The parent drives physical terminal encodings.
struct Attachment {
    child: Child,
    master: fs::File,
    output: Arc<parking_lot::Mutex<OuterOutput>>,
    stop_reader: Arc<AtomicBool>,
    reader: Option<std::thread::JoinHandle<()>>,
    captures: PathBuf,
}
impl Attachment {
    fn new(h: &Host, terminal: &str, name: &str) -> Self {
        Self::on_socket(h, terminal, name, &h.socket)
    }
    fn on_socket(h: &Host, terminal: &str, name: &str, socket: &Path) -> Self {
        Self::on_socket_with_resize_signal(h, terminal, name, socket, false)
    }
    fn on_socket_with_resize_signal(
        h: &Host,
        terminal: &str,
        name: &str,
        socket: &Path,
        suppress_resize_signal: bool,
    ) -> Self {
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
                    std::ptr::null_mut::<libc::termios>(),
                    &mut size,
                )
            },
            0,
            "openpty: {}",
            io::Error::last_os_error()
        );
        let master = unsafe { fs::File::from_raw_fd(master) };
        let slave = unsafe { OwnedFd::from_raw_fd(slave) };
        for fd in [master.as_raw_fd(), slave.as_raw_fd()] {
            assert_eq!(
                unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) },
                0
            );
        }
        let input = fs::File::from(slave);
        let output = input.try_clone().unwrap();
        let error = input.try_clone().unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_amux"));
        command
            .arg("--socket")
            .arg(socket)
            .arg("attach")
            .arg(terminal)
            .env("HOME", h.root.join("home"))
            .env("XDG_CONFIG_HOME", h.root.join("config"))
            .env("XDG_RUNTIME_DIR", h.root.join("runtime"))
            .env("AMUX_RUNTIME_DIR", h.root.join("runtime"))
            .env("AMUX_SOCKET", socket)
            .env("TERM", "xterm-256color")
            .env_remove("NO_COLOR")
            .env_remove("AMUX_TERMINAL")
            .env_remove("AMUX_INTEGRATION")
            .current_dir(&h.root)
            .stdin(Stdio::from(input))
            .stdout(Stdio::from(output))
            .stderr(Stdio::from(error));
        unsafe {
            command.pre_exec(move || {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                if libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                if suppress_resize_signal {
                    // Model a lost/coalesced notification without changing the
                    // real kernel window size or injecting application events.
                    let mut mask: libc::sigset_t = std::mem::zeroed();
                    libc::sigemptyset(&mut mask);
                    libc::sigaddset(&mut mask, libc::SIGWINCH);
                    if libc::sigprocmask(libc::SIG_BLOCK, &mask, std::ptr::null_mut()) == -1 {
                        return Err(io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
        assert!(flags >= 0);
        assert_eq!(
            unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
            0
        );
        let captures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/e03-native-captures")
            .join(h.root.file_name().unwrap())
            .join(name);
        fs::create_dir_all(&captures).unwrap();
        fs::set_permissions(&captures, fs::Permissions::from_mode(0o700)).unwrap();
        let output = Arc::new(parking_lot::Mutex::new(OuterOutput {
            term: Term::new(
                Config::default(),
                &Geometry {
                    cols: COLS,
                    rows: ROWS,
                },
                Listener,
            ),
            raw: Vec::new(),
            error: None,
        }));
        let stop_reader = Arc::new(AtomicBool::new(false));
        let mut attachment = Self {
            child: command.spawn().unwrap(),
            master,
            output,
            stop_reader,
            reader: None,
            captures,
        };
        let master = attachment.master.try_clone().unwrap();
        let output = attachment.output.clone();
        let stop = attachment.stop_reader.clone();
        attachment.reader = Some(
            std::thread::Builder::new()
                .name("native-outer-pty".into())
                .spawn(move || drain_outer(master, output, stop))
                .unwrap(),
        );
        attachment
    }
    fn drain(&mut self) {
        // The independent reader drains even while this test waits on another
        // attachment, daemon state, consent barriers or elapsed warning time.
        let output = self.output.lock();
        assert!(
            output.error.is_none(),
            "{}",
            output.error.as_deref().unwrap_or("")
        );
    }
    fn lines(&self) -> Vec<String> {
        visible_lines(&self.output.lock().term)
    }
    fn capture_native_body(&mut self, h: &Host, terminal: &str, name: &str) -> Vec<String> {
        let screen = h.call(json!({
            "op":"screen","terminal":terminal,"offset":0,"limit":ROWS - 3
        }));
        assert_eq!(screen["cols"], COLS);
        assert_eq!(screen["rows"], ROWS - 3);
        let expected: Vec<_> = screen["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| line.as_str().unwrap().trim_end().to_owned())
            .collect();
        assert_eq!(expected.len(), ROWS - 3, "full native viewport");
        h.until(
            "native viewport begins immediately below three header rows",
            || {
                self.drain();
                (self.lines()[3..] == expected).then_some(())
            },
        );
        let captured = self.capture(name);
        assert_eq!(
            &captured[3..],
            expected.as_slice(),
            "native viewport origin changed"
        );
        captured
    }
    fn text(&mut self) -> String {
        self.drain();
        self.lines().join("\n")
    }
    fn wait(&mut self, expected: &str) -> String {
        let deadline = Instant::now() + DEADLINE;
        loop {
            let text = self.text();
            if text.contains(expected) {
                return text;
            }
            if Instant::now() >= deadline {
                self.capture("failure");
                panic!(
                    "missing {expected:?}; attach status {:?}; screen:\n{text}",
                    self.child.try_wait().unwrap()
                );
            }
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "attachment exited:\n{text}"
            );
            std::thread::sleep(Duration::from_millis(15));
        }
    }
    fn project_header(&mut self, project: &str) -> bool {
        self.drain();
        self.lines()[0].contains(project)
    }
    fn wait_controlled(&mut self, h: &Host, terminal: &str) {
        let project = project_name(h, terminal);
        h.until("native ownership and ordinary project header", || {
            (h.ownership(terminal)["owner"]["role"] == "attachment"
                && self.project_header(&project))
            .then_some(())
        });
    }
    fn send(&mut self, bytes: &[u8]) {
        // Writes are deliberately small and bounded, even with a nonblocking fd.
        let deadline = Instant::now() + DEADLINE;
        let mut rest = bytes;
        while !rest.is_empty() {
            match self.master.write(rest) {
                Ok(0) => panic!("zero attachment write"),
                Ok(n) => rest = &rest[n..],
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "attachment write deadline");
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("attachment write: {error}"),
            }
        }
    }
    fn send_queued(&mut self, bytes: &[u8]) {
        self.with_stopped(|native| native.send(bytes));
    }
    fn with_stopped(&mut self, drive: impl FnOnce(&mut Self)) {
        // Keep this owned, unreaped child stopped until every encoded key is queued.
        struct Resume(libc::pid_t);
        impl Drop for Resume {
            fn drop(&mut self) {
                unsafe {
                    libc::kill(self.0, libc::SIGCONT);
                }
            }
        }
        let pid = self.child.id() as libc::pid_t;
        assert_eq!(unsafe { libc::kill(pid, libc::SIGSTOP) }, 0);
        let resume = Resume(pid);
        let deadline = Instant::now() + DEADLINE;
        loop {
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            assert_eq!(
                unsafe {
                    libc::waitid(
                        libc::P_PID,
                        pid as libc::id_t,
                        &mut info,
                        libc::WSTOPPED | libc::WNOHANG | libc::WNOWAIT,
                    )
                },
                0
            );
            if info.si_code == libc::CLD_STOPPED {
                break;
            }
            assert!(Instant::now() < deadline, "attachment did not stop");
            std::thread::sleep(Duration::from_millis(5));
        }
        drive(self);
        drop(resume);
    }
    fn resize(&mut self, h: &Host, terminal: &str, cols: usize, rows: usize) {
        self.output.lock().term.resize(Geometry { cols, rows });
        let size = libc::winsize {
            ws_row: rows as u16,
            ws_col: cols as u16,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        assert_eq!(
            unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &size) },
            0
        );
        let deadline = Instant::now() + DEADLINE;
        loop {
            let screen =
                h.call(json!({"op":"screen","terminal":terminal,"offset":0,"limit":rows - 3}));
            if screen["cols"] == cols && screen["rows"] == rows - 3 {
                break;
            }
            let exit = self.child.try_wait().unwrap();
            if exit.is_some() || Instant::now() >= deadline {
                self.capture("resize-failure");
                panic!(
                    "actual attachment resize {cols}x{rows} did not reach native PTY: {}x{}, attachment {exit:?}",
                    screen["cols"], screen["rows"]
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn action(&mut self, suffix: u8) {
        self.send(PREFIX);
        self.send(&[suffix]);
    }
    fn hide_panel(&mut self) {
        self.send(b"\x1b");
        let deadline = Instant::now() + DEADLINE;
        loop {
            let text = self.text();
            if ![
                " MENU ",
                "Locked terminal",
                "Control requested",
                "Cancel takeover?",
                "Force control?",
                "Control owned here",
            ]
            .iter()
            .any(|title| text.contains(title))
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "native panel did not hide:\n{text}"
            );
            std::thread::sleep(Duration::from_millis(15));
        }
    }
    fn capture(&mut self, name: &str) -> Vec<String> {
        self.drain();
        let (lines, chrome, raw, rows, cols) = {
            let output = self.output.lock();
            let lines = visible_lines(&output.term);
            let chrome: Vec<_> = (0..3).map(|row| (0..output.term.columns()).map(|col| {
                let cell = &output.term.grid()[Point::new(Line(row), Column(col))];
                json!({"text":cell.c.to_string(), "fg":format!("{:?}",cell.fg), "bg":format!("{:?}",cell.bg)})
            }).collect::<Vec<_>>()).collect();
            (
                lines,
                chrome,
                output.raw.clone(),
                output.term.screen_lines(),
                output.term.columns(),
            )
        };
        // Never hold the reader's render lock across capture-file I/O.
        fs::write(self.captures.join(format!("{name}.txt")), lines.join("\n")).unwrap();
        fs::write(self.captures.join(format!("{name}.ansi")), raw).unwrap();
        fs::write(self.captures.join(format!("{name}.json")), serde_json::to_vec_pretty(&json!({
            "rows":rows,"cols":cols,"header_rows":3,"attachment_pid":self.child.id(),"chrome":chrome
        })).unwrap()).unwrap();
        eprintln!(
            "native surface capture: {}",
            self.captures.join(name).display()
        );
        lines
    }
    fn chrome_colors(&mut self) -> Vec<String> {
        self.drain();
        let output = self.output.lock();
        (0..3)
            .flat_map(|row| (0..COLS).map(move |col| (row, col)))
            .map(|(row, col)| {
                let cell = &output.term.grid()[Point::new(Line(row), Column(col))];
                format!("{:?}/{:?}", cell.fg, cell.bg)
            })
            .collect()
    }
    fn detach(&mut self) {
        self.action(17);
        let deadline = Instant::now() + DEADLINE;
        loop {
            self.drain();
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "detach failed: {}",
                    self.lines().join("\n")
                );
                return;
            }
            assert!(Instant::now() < deadline, "native detach deadline");
            std::thread::sleep(Duration::from_millis(15));
        }
    }
}
impl Drop for Attachment {
    fn drop(&mut self) {
        let deadline = Instant::now() + CLEANUP_TIMEOUT;
        if self.child.try_wait().ok().flatten().is_none() {
            unsafe {
                libc::kill(-(self.child.id() as i32), libc::SIGKILL);
            }
            let _ = self.child.kill();
        }
        let reaped = loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break true,
                Err(_) => break false,
                Ok(None) if Instant::now() >= deadline => break false,
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            }
        };
        self.stop_reader.store(true, Ordering::Release);
        let drained = self
            .reader
            .take()
            .is_none_or(|reader| join_until(reader, Instant::now() + CLEANUP_TIMEOUT));
        if !reaped || !drained {
            if std::thread::panicking() {
                eprintln!(
                    "native fixture cleanup deadline: reaped={reaped}, drainer_joined={drained}"
                );
            } else {
                panic!(
                    "native fixture cleanup deadline: reaped={reaped}, drainer_joined={drained}"
                );
            }
        }
    }
}

// This writer owns only a clone of the already-nonblocking outer PTY master.
// Cancellation, a finite deadline and a byte ceiling bound every failure path;
// poll is only a cancellable readiness wait, never an input-timing oracle.
struct IncompleteInput {
    stop: Arc<AtomicBool>,
    written: Arc<AtomicUsize>,
    backpressured: Arc<AtomicBool>,
    error: Arc<parking_lot::Mutex<Option<String>>>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl IncompleteInput {
    fn new(native: &Attachment) -> Self {
        let mut master = native.master.try_clone().unwrap();
        let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
        assert!(flags >= 0 && flags & libc::O_NONBLOCK != 0);
        let stop = Arc::new(AtomicBool::new(false));
        let written = Arc::new(AtomicUsize::new(0));
        let backpressured = Arc::new(AtomicBool::new(false));
        let error = Arc::new(parking_lot::Mutex::new(None));
        let (stopping, progress, blocked, failure) = (
            stop.clone(),
            written.clone(),
            backpressured.clone(),
            error.clone(),
        );
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + DEADLINE.saturating_mul(3);
            let payload = [b'x'; 4096];
            let mut pending: &[u8] = b"\x1b[200~";
            while !stopping.load(Ordering::Acquire) {
                if Instant::now() >= deadline {
                    *failure.lock() = Some("incomplete-input producer deadline".into());
                    break;
                }
                match master.write(pending) {
                    Ok(0) => {
                        *failure.lock() = Some("zero incomplete-input write".into());
                        break;
                    }
                    Ok(n) => {
                        let total = progress.fetch_add(n, Ordering::AcqRel) + n;
                        if total >= 16 * 1024 * 1024 {
                            *failure.lock() =
                                Some("incomplete-input producer exceeded 16 MiB".into());
                            break;
                        }
                        pending = &pending[n..];
                        if pending.is_empty() {
                            pending = &payload;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        blocked.store(true, Ordering::Release);
                        let mut ready = libc::pollfd {
                            fd: master.as_raw_fd(),
                            events: libc::POLLOUT,
                            revents: 0,
                        };
                        if unsafe { libc::poll(&mut ready, 1, 10) } < 0 {
                            let error = io::Error::last_os_error();
                            if error.kind() != io::ErrorKind::Interrupted {
                                *failure.lock() = Some(format!("incomplete-input poll: {error}"));
                                break;
                            }
                        } else if ready.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL)
                            != 0
                        {
                            *failure.lock() = Some("incomplete-input PTY closed".into());
                            break;
                        }
                    }
                    Err(error) => {
                        *failure.lock() = Some(format!("incomplete-input write: {error}"));
                        break;
                    }
                }
            }
        });
        Self {
            stop,
            written,
            backpressured,
            error,
            worker: Some(worker),
        }
    }
    fn active_bytes(&self) -> usize {
        {
            let error = self.error.lock();
            assert!(error.is_none(), "{error:?}");
        }
        assert!(
            !self.worker.as_ref().unwrap().is_finished(),
            "incomplete-input producer stopped before the consumer fence"
        );
        self.written.load(Ordering::Acquire)
    }
    fn finish(mut self) -> usize {
        let written = self.active_bytes();
        self.join();
        let error = self.error.lock();
        assert!(error.is_none(), "{error:?}");
        written
    }
    fn join(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take()
            && !join_until(worker, Instant::now() + CLEANUP_TIMEOUT)
        {
            if std::thread::panicking() {
                eprintln!("incomplete-input producer cleanup deadline");
            } else {
                panic!("incomplete-input producer cleanup deadline");
            }
        }
    }
}
impl Drop for IncompleteInput {
    fn drop(&mut self) {
        self.join();
    }
}

fn configured_host() -> Host {
    let h = Host::new();
    h.call(json!({"op":"config","config":{"prefix":"Ctrl-a"}}));
    h
}
fn project_name(h: &Host, terminal: &str) -> String {
    let snapshot = h.call(json!({"op":"snapshot"}));
    let terminal = snapshot["terminals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == terminal)
        .unwrap();
    snapshot["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == terminal["project_id"])
        .unwrap()["name"]
        .as_str()
        .unwrap()
        .to_owned()
}
fn native_owner(h: &Host, terminal: &str) -> Value {
    h.until("native owner registration", || {
        let owner = h.ownership(terminal)["owner"].clone();
        (owner["role"] == "attachment").then_some(owner)
    })
}
fn assert_owner(h: &Host, terminal: &str, owner: &Value) {
    assert_eq!(
        h.ownership(terminal)["owner"]["controller_id"],
        owner["controller_id"]
    );
}
fn reservation(h: &Host, terminal: &str) -> Value {
    h.until("native reservation", || {
        let value = h.ownership(terminal)["reservation"].clone();
        value.is_object().then_some(value)
    })
}
fn wait_unowned(h: &Host, terminal: &str) {
    h.until("unowned after cancellation", || {
        let state = h.ownership(terminal);
        (state["owner"].is_null() && state["reservation"].is_null()).then_some(())
    });
}
fn bytes(h: &Host) -> Vec<u8> {
    fs::read(h.root.join("native-bytes")).unwrap_or_default()
}
fn wait_bytes(h: &Host, expected: &[u8]) {
    h.until("actual native application bytes", || {
        (bytes(h) == expected).then_some(())
    });
}

// The daemon's existing shell launches a plain byte-observable local target;
// there is no model, Codex frontend, second conversation or synthetic UI.
fn recording_target(h: &Host) -> String {
    let terminal = h.main();
    let setup = h.controller("fixture setup");
    let grant = h.acquire(&terminal, &setup["controller"]);
    let source = format!(
        "stty raw -echo; printf '\\033[2J\\033[H'; printf 'NATIVE_SURFACE pid=%s' $$; printf '\\033[?1003h\\033[?1006h\\033[?2004h'; : > '{}'; /bin/cat > '{}'",
        h.root.join("target-ready").display(),
        h.root.join("native-bytes").display()
    );
    h.execute(&terminal, &grant["control"], &source);
    h.until("byte recorder ready", || {
        h.root.join("target-ready").exists().then_some(())
    });
    h.handle(&grant["control"], "release");
    terminal
}

#[test]
fn two_controlling_ptys_share_process_three_header_rows_and_nonmodal_owner_notice() {
    let h = configured_host();
    let terminal = recording_target(&h);
    let pid = h.state(&terminal)["pid"].clone();
    let mut owner = Attachment::new(&h, &terminal, "owner");
    owner.wait_controlled(&h, &terminal);
    let identity = native_owner(&h, &terminal);
    let owner_colors = owner.chrome_colors();
    owner.capture_native_body(&h, &terminal, "owned");
    let mut observer = Attachment::new(&h, &terminal, "observer");
    observer.wait("Locked");
    let locked = observer.capture_native_body(&h, &terminal, "locked");
    assert!(
        locked[..3].join(" ").contains('🔒'),
        "lock icon absent: {locked:?}"
    );
    assert_ne!(
        owner_colors,
        observer.chrome_colors(),
        "ownership must color existing chrome"
    );
    assert_eq!(h.state(&terminal)["pid"], pid);
    assert_eq!(
        h.call(json!({"op":"snapshot"}))["terminals"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    observer.action(12);
    observer.wait("Waiting");
    let claim = reservation(&h, &terminal);
    owner.send(b"y\r");
    wait_bytes(&h, b"y\r");
    assert_owner(&h, &terminal, &identity);
    owner.capture("nonmodal-request-notice");
    observer.hide_panel();
    observer.wait("Locked");
    observer.action(12);
    observer.wait("Waiting");
    assert_eq!(reservation(&h, &terminal)["claim_id"], claim["claim_id"]);
    std::thread::sleep(Duration::from_millis(3150));
    observer.wait("native-input");
    observer.capture("three-second-blockers");
    assert_owner(&h, &terminal, &identity);
    owner.action(12);
    owner.wait("y Yield to this request");
    owner.send(b"y");
    h.until("observer renders departure of the yielded owner", || {
        let text = observer.text();
        (!text
            .lines()
            .take(3)
            .any(|line| line.contains(identity["controller_id"].as_str().unwrap())))
        .then_some(())
    });
    assert!(
        h.ownership(&terminal)["owner"].is_null(),
        "yield must revoke, not fabricate settlement"
    );
    observer.action(6);
    observer.wait("Enter Force control");
    observer.capture("force-risk");
    observer.send(b"\r");
    observer.wait_controlled(&h, &terminal);
    observer.send(b"FRESH");
    wait_bytes(&h, b"y\rFRESH");
    let after = h.ownership(&terminal)["owner"].clone();
    observer.detach();
    assert_owner(&h, &terminal, &after);
    assert_eq!(h.state(&terminal)["pid"], pid);
    assert_eq!(bytes(&h), b"y\rFRESH");
}

#[test]
fn observer_physical_keys_repeat_literal_prefix_paste_mouse_never_replay_after_force() {
    let h = configured_host();
    let terminal = recording_target(&h);
    let mut owner = Attachment::new(&h, &terminal, "input-owner");
    owner.wait_controlled(&h, &terminal);
    let mut observer = Attachment::new(&h, &terminal, "input-observer");
    observer.wait("Locked");
    // Ordinary press, kitty repeat, doubled configured prefix, bracketed paste
    // and SGR application mouse all traverse the actual crossterm input path.
    for sequence in [
        b"BLOCKED".as_slice(),
        b"\x1b[120;1:2u",
        b"\x01\x01",
        b"\x1b[200~PASTE_BLOCKED\x1b[201~",
    ] {
        observer.send(sequence);
        observer.wait("Locked terminal");
        observer.hide_panel();
        assert!(h.ownership(&terminal)["reservation"].is_null());
        assert!(
            bytes(&h).is_empty(),
            "observing event reached native target"
        );
    }
    observer.send(b"\x1b[<0;12;8M");
    observer.wait("Locked terminal");
    observer.send(b"\r"); // Confirmation is consumed, never application Enter.
    observer.wait("Waiting");
    observer.capture("input-confirmation-consumed");
    observer.action(6);
    observer.wait("Enter Force control");
    observer.send(b"\r");
    observer.wait_controlled(&h, &terminal);
    observer.send(b"\x1b[<32;13;8M\x1b[<0;13;8m");
    observer.send(b"AFTER_GRANT");
    wait_bytes(&h, b"AFTER_GRANT");
    observer.capture("gesture-tail-consumed");
    // A fresh deliberate gesture must still work after the swallowed old tail.
    observer.send(b"\x1b[<0;14;9M\x1b[<0;14;9m");
    wait_bytes(&h, b"AFTER_GRANT\x1b[<0;14;6M\x1b[<0;14;6m");
}

#[test]
fn native_cancel_grants_nobody_and_changed_force_facts_require_new_confirmation() {
    let h = configured_host();
    let terminal = recording_target(&h);
    let external = h.controller("external-owner");
    let initial = h.acquire(&terminal, &external["controller"]);
    let mut observer = Attachment::new(&h, &terminal, "cancel-force-observer");
    observer.wait("Locked");
    observer.action(12);
    observer.wait("waiting-owner");
    observer.send(b"c");
    observer.wait("Cancel takeover?");
    observer.capture("cancel-grants-nobody");
    observer.send(b"\r");
    wait_unowned(&h, &terminal);
    observer.wait("Unowned");
    denied(
        h.response(json!({"op":"control-handle","handle":initial["control"],"method":"read"})),
        &["stale-handle"],
    );
    let replacement = h.controller("replacement-owner");
    h.acquire(&terminal, &replacement["controller"]);
    observer.wait("Locked");
    let pid = h.state(&terminal)["pid"].clone();
    observer.send_queued(&[PREFIX[0], 6, b'\r']);
    h.until(
        "queued confirmation rejected before visible warning",
        || {
            (observer.text().contains("Force confirmation blocked")
                || h.ownership(&terminal)["owner"]["controller_id"] != replacement["controller_id"])
                .then_some(())
        },
    );
    assert_owner(&h, &terminal, &replacement);
    let queued = observer.capture("force-queued-before-draw");
    assert!(queued.join("\n").contains("Prior calls may still apply"));
    for rows in [7, 10] {
        observer.hide_panel();
        observer.action(6);
        observer.wait("Force control?");
        observer.resize(&h, &terminal, 80, rows);
        observer.send_queued(b"\r");
        h.until(
            "confirmation rejected after warning becomes invisible",
            || {
                (observer.text().contains("Force confirmation blocked")
                    || h.ownership(&terminal)["owner"]["controller_id"]
                        != replacement["controller_id"])
                    .then_some(())
            },
        );
        assert_owner(&h, &terminal, &replacement);
        let clipped = observer.capture(&format!("force-not-visible-80x{rows}"));
        assert_eq!(clipped.len(), rows);
        assert_eq!(h.state(&terminal)["pid"], pid);
        assert!(
            bytes(&h).is_empty(),
            "intercepted confirmation reached the application"
        );
        observer.resize(&h, &terminal, COLS, ROWS);
    }
    observer.hide_panel();
    observer.action(6);
    observer.wait("Enter Force control");
    let winner = h.controller("newest-owner");
    h.call(h.claim_request(&terminal, &winner["controller"], "force"));
    observer.send(b"\r");
    h.until("stale force response rendered", || {
        let text = observer.text();
        (text.to_lowercase().contains("stale") || text.to_lowercase().contains("changed"))
            .then_some(text)
    });
    observer.capture("stale-force-needs-reconfirmation");
    assert_eq!(
        h.ownership(&terminal)["owner"]["controller_id"],
        winner["controller_id"]
    );
    observer.send(b"\r");
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        h.ownership(&terminal)["owner"]["controller_id"],
        winner["controller_id"],
        "stale confirmation was reused"
    );
    observer.action(6);
    observer.wait("Enter Force control");
    observer.send(b"\r");
    observer.wait_controlled(&h, &terminal);
    assert_ne!(
        h.ownership(&terminal)["owner"]["controller_id"],
        winner["controller_id"]
    );
    assert!(bytes(&h).is_empty());
}

#[test]
#[ignore = "macOS CI timed out waiting to be unowned after cancellation; revisit after rehabilitation"]
fn e06s03_native_filtered_terminal_replies_cannot_confirm_queued_force_enter() {
    let h = configured_host();
    let terminal = recording_target(&h);
    let external = h.controller("filtered-reply-owner");
    h.acquire(&terminal, &external["controller"]);
    let generation = h.ownership(&terminal)["observed"]["generation"].clone();
    let pid = h.state(&terminal)["pid"].clone();
    let gate = ConsentGate::new(&h);
    let mut observer = Attachment::on_socket(&h, &terminal, "filtered-force-enter", &gate.socket);
    observer.wait("Locked");
    observer.send(b"\x1b[I");
    let observer_id = h.until("observer reports focused before queued Force input", || {
        h.ownership(&terminal)["views"]
            .as_array()?
            .iter()
            .find(|view| {
                view["connected"] == true && view["reported"] == true && view["focused"] == true
            })
            .map(|view| view["controller_id"].clone())
    });
    // The stopped, unreaped real client cannot draw between these encodings.
    // Three genuine internal replies precede an Enter that predates the first
    // Force warning; filtering them must not promote that warning to ready.
    observer.send_queued(b"\x01\x06\x1b[20;10R\x1b[?1;2c\x1b[?0u\r\x1b[O");
    h.until(
        "focus report causally fences the queued old Force Enter",
        || {
            h.ownership(&terminal)["views"]
                .as_array()?
                .iter()
                .any(|view| {
                    view["controller_id"] == observer_id
                        && view["connected"] == true
                        && view["reported"] == true
                        && view["focused"] == false
                })
                .then_some(())
        },
    );
    assert_owner(&h, &terminal, &external);
    assert_eq!(h.ownership(&terminal)["observed"]["generation"], generation);
    assert!(h.ownership(&terminal)["reservation"].is_null());
    assert_eq!(h.state(&terminal)["pid"], pid);
    assert!(
        bytes(&h).is_empty(),
        "queued confirmation reached the target"
    );
    {
        let methods = gate.methods.lock();
        assert!(!methods.iter().any(|method| method == "ownership-claim"));
        assert!(!methods.iter().any(|method| method == "consent-to"));
    }
    observer.wait("Force confirmation blocked");
    observer.capture("filtered-replies-old-enter-rejected");
    observer.hide_panel();
    observer.action(6);
    observer.wait("Enter Force control · Esc Keep viewing");
    observer.send(b"\r"); // A separately entered, actually presented confirmation.
    observer.wait_controlled(&h, &terminal);
    assert_eq!(
        h.ownership(&terminal)["owner"]["controller_id"],
        observer_id
    );
    assert_eq!(
        gate.methods
            .lock()
            .iter()
            .filter(|method| method.as_str() == "ownership-claim")
            .count(),
        1
    );
    observer.send(b"AFTER_FRESH_FORCE");
    wait_bytes(&h, b"AFTER_FRESH_FORCE");
    assert_eq!(h.state(&terminal)["pid"], pid);
    observer.action(12);
    observer.wait("Control owned here");
    observer.send(b"r");
    wait_unowned(&h, &terminal);
    observer.detach();
}

#[test]
fn e06s03_native_resize_without_notification_reconciles_before_force_consent() {
    let h = configured_host();
    let terminal = recording_target(&h);
    let external = h.controller("resize-owner");
    h.acquire(&terminal, &external["controller"]);
    let pid = h.state(&terminal)["pid"].clone();
    let mut observer = Attachment::on_socket_with_resize_signal(
        &h,
        &terminal,
        "coalesced-resize",
        &h.socket,
        true,
    );
    observer.wait("Locked");
    observer.action(6);
    observer.wait("Enter Force control");
    observer.resize(&h, &terminal, 80, 7);
    observer.send_queued(b"\r");
    observer.wait("Force confirmation blocked");
    assert_owner(&h, &terminal, &external);
    observer.resize(&h, &terminal, COLS, ROWS);
    observer.wait("Enter Force control");
    assert_owner(&h, &terminal, &external);
    assert_eq!(h.state(&terminal)["pid"], pid);
    assert!(bytes(&h).is_empty(), "resize replayed intercepted input");
    observer.capture("reconciled-without-resize-notification");
    observer.hide_panel();
    observer.detach();
}

#[test]
fn native_lock_label_is_sanitized_clipped_identified_and_contains_no_private_handles() {
    let h = configured_host();
    let terminal = h.main();
    let label = format!("RED\x1b[2J\nOWNER-{}-UNCLIPPED_TAIL", "界".repeat(180));
    let external = h.controller(&label);
    let grant = h.acquire(&terminal, &external["controller"]);
    let mut observer = Attachment::new(&h, &terminal, "sanitized-label");
    observer.wait("Locked");
    let lines = observer.capture("sanitized-clipped-lock");
    let header = lines[..3].join("\n");
    assert!(header.contains("RED"), "owner label absent: {header}");
    assert!(!header.contains("UNCLIPPED_TAIL"));
    assert!(!header.contains('\x1b'));
    let id = external["controller_id"].as_str().unwrap();
    assert!(
        header.contains(id) || header.contains(&id[..id.len().min(8)]),
        "disambiguating ID absent: {header}"
    );
    for handle in [&external["controller"], &grant["control"]] {
        assert!(!lines.join("\n").contains(handle["ref"].as_str().unwrap()));
    }
    assert_eq!(lines.len(), ROWS);
    assert!(
        lines[3..]
            .iter()
            .any(|line| line.contains("OWNERSHIP_NATIVE"))
    );
}

struct NativeWire {
    connection: Connection,
    controller: Value,
    control: Value,
    serial: u64,
}
impl NativeWire {
    fn new(h: &Host, terminal: &str) -> Self {
        let mut connection = Connection::connect(&h.socket, DEADLINE).unwrap();
        let created = connection.persistent(json!({
            "op":"controller-create","role":"attachment","label":"native conditional fixture"
        }));
        let grant =
            connection.persistent(h.claim_request(terminal, &created["controller"], "cooperative"));
        assert_eq!(grant["kind"], "control");
        Self {
            connection,
            controller: created["controller"].clone(),
            control: grant["control"].clone(),
            serial: 0,
        }
    }
    fn view(&mut self, terminal: Option<&str>, focused: Option<bool>) {
        self.serial += 1;
        self.connection
            .persistent(json!({"op":"control-handle","handle":self.controller,
            "method":"view","view":{"serial":self.serial,"terminal":terminal,"focused":focused}}));
    }
    fn consent_request(&self, h: &Host, terminal: &str, claim: &Value) -> Value {
        json!({"op":"control-handle","handle":self.control,"method":"consent-to",
            "claim_id":claim["claim_id"],"view_serial":self.serial,
            "activity_revision":h.ownership(terminal)["activity_revision"]})
    }
    fn consent(&mut self, h: &Host, terminal: &str, claim: &Value) {
        let request = self.consent_request(h, terminal, claim);
        self.connection.persistent(request);
    }
    fn keep(&mut self, claim: &Value) {
        self.connection
            .persistent(json!({"op":"control-handle","handle":self.control,
            "method":"keep","claim_id":claim["claim_id"]}));
    }
    fn assert_same_control(&mut self, h: &Host) {
        let read = self.connection.persistent(json!({"op":"control-handle",
            "handle":self.control,"method":"read"}));
        assert_eq!(read["control"], self.control);
        assert_eq!(
            h.handle(&self.controller, "read")["controls"][0]["control"],
            self.control
        );
    }
}
fn conditional_claim(h: &Host, terminal: &str, requester: &Value, when: Value) -> Value {
    let mut request = h.claim_request(terminal, &requester["controller"], "cooperative");
    request["when"] = when;
    let claim = h.call(request);
    assert_eq!(claim["kind"], "claim-state");
    assert_eq!(claim["purpose"], "conditional");
    claim
}
fn condition(h: &Host, terminal: &str) -> Value {
    h.ownership(terminal)["reservation"]["conditional"].clone()
}
fn conditional_ready(h: &Host, terminal: &str) -> Value {
    h.until("public conditional readiness", || {
        let value = condition(h, terminal);
        (value["ready_for_consent"] == true).then_some(value)
    })
}
fn offered(h: &Host, claim: &Value) -> Value {
    h.until("conditional offer", || {
        let value = h.handle(&claim["claim"], "read");
        (value["phase"] == "offered").then_some(value)
    })
}

#[test]
fn conditional_idle_consent_uses_original_connection_exact_revisions_requested_clock_and_same_token_keep()
 {
    let h = Host::new();
    let terminal = h.main();
    let mut native = NativeWire::new(&h, &terminal);
    native.view(Some(&terminal), Some(true));
    let requester = h.controller("explicit idle policy");
    let when = json!({"kind":"human-idle","idle_ms":0,"warning_ms":1700});
    let claim = conditional_claim(&h, &terminal, &requester, when.clone());
    let ready = conditional_ready(&h, &terminal);
    assert_eq!(ready["when"], when);
    assert_eq!(ready["clock"], json!({"kind":"idle","remaining_ms":0}));
    assert_eq!(ready["consent"], "awaiting");
    let consent = native.consent_request(&h, &terminal, &claim);
    denied(h.response(consent.clone()), &["not-native-connection"]);
    let mut other = Connection::connect(&h.socket, DEADLINE).unwrap();
    denied(
        other
            .exchange(json!({"persistent":true,
        "op":"control-handle","handle":native.control,"method":"consent-to",
        "claim_id":claim["claim_id"],"view_serial":native.serial,
        "activity_revision":h.ownership(&terminal)["activity_revision"]}))
            .unwrap(),
        &["not-native-connection"],
    );
    let mut stale_view = consent.clone();
    stale_view["view_serial"] = json!(native.serial - 1);
    stale_view["persistent"] = json!(true);
    denied(
        native.connection.exchange(stale_view).unwrap(),
        &["stale-view"],
    );
    let mut stale_activity = consent.clone();
    stale_activity["activity_revision"] = json!(
        h.ownership(&terminal)["activity_revision"]
            .as_u64()
            .unwrap()
            + 1
    );
    stale_activity["persistent"] = json!(true);
    denied(
        native.connection.exchange(stale_activity).unwrap(),
        &["stale-observation"],
    );
    let cursor = h.seq();
    let started = Instant::now();
    native.consent(&h, &terminal, &claim);
    let warning = condition(&h, &terminal);
    assert_eq!(warning["consent"], "recorded");
    assert_eq!(warning["clock"]["kind"], "warning");
    let remaining = warning["clock"]["remaining_ms"].as_u64().unwrap();
    assert!(
        remaining <= 1700
            && remaining > 1700u64.saturating_sub(started.elapsed().as_millis() as u64 + 250),
        "warning must use requested daemon duration: {warning}"
    );
    native.assert_same_control(&h);
    let private = h.handle(&claim["claim"], "read");
    assert_eq!(
        private["ownership"]["reservation"]["conditional"]["clock"]["kind"],
        "warning"
    );
    let public = h.events(cursor);
    no_private_refs(
        &public,
        &[
            &native.controller,
            &native.control,
            &requester["controller"],
            &claim["claim"],
        ],
    );
    assert!(public.to_string().contains("conditional"));
    assert!(public.to_string().contains("warning"));
    native.keep(&claim);
    assert_eq!(
        h.handle(&claim["claim"], "read")["phase"],
        "aborted-by-owner"
    );
    native.assert_same_control(&h);
    std::thread::sleep(Duration::from_millis(1750));
    assert!(
        h.ownership(&terminal)["reservation"].is_null(),
        "amux must not rearm external policy"
    );
    native.assert_same_control(&h);
}

#[test]
fn conditional_zero_warning_remains_blocked_by_real_native_unknown_settlement_and_withdraw_keeps_token()
 {
    let h = Host::new();
    let terminal = h.main();
    let mut native = NativeWire::new(&h, &terminal);
    native.view(Some(&terminal), Some(true));
    native
        .connection
        .persistent(json!({"op":"input","terminal":terminal,
        "control":native.control,"text":"\u{c}"}));
    let calls = h.native_calls(&terminal);
    let requester = h.controller("zero clock requester");
    let claim = conditional_claim(
        &h,
        &terminal,
        &requester,
        json!({"kind":"human-idle","idle_ms":0,"warning_ms":65}),
    );
    conditional_ready(&h, &terminal);
    native.consent(&h, &terminal, &claim);
    h.until("warning expired without authority transfer", || {
        let value = condition(&h, &terminal);
        (value["clock"] == json!({"kind":"warning","remaining_ms":0})).then_some(())
    });
    let blocked = h.handle(&claim["claim"], "read");
    assert!(
        blocked.get("offer").is_none(),
        "unknown native settlement fabricated an offer: {blocked}"
    );
    assert!(
        blocked["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker["kind"] == "settlement"
                && blocker["call"]["kind"] == "native-input"
                && blocker["call"]["outcome"] == "unknown"),
        "{blocked}"
    );
    assert_eq!(
        stable_calls(&h.ownership(&terminal)["unsettled"]),
        stable_calls(&calls)
    );
    native.assert_same_control(&h);
    h.handle(&claim["claim"], "withdraw");
    assert_eq!(h.handle(&claim["claim"], "read")["phase"], "withdrawn");
    native.assert_same_control(&h);
    assert!(h.ownership(&terminal)["reservation"].is_null());
}

#[test]
fn background_owner_scope_rejects_unknown_and_stale_views_withdraws_consent_without_revoking_human()
{
    let h = Host::new();
    let terminal = h.main();
    let mut native = NativeWire::new(&h, &terminal);
    let requester = h.controller("background owner scope");
    let claim = conditional_claim(
        &h,
        &terminal,
        &requester,
        json!({"kind":"views-backgrounded","scope":"owner"}),
    );
    let unknown = condition(&h, &terminal);
    assert_eq!(unknown["eligibility"], "unknown");
    assert_eq!(unknown["ready_for_consent"], false);
    assert_eq!(unknown["clock"], json!({"kind":"none","remaining_ms":null}));
    let mut premature = native.consent_request(&h, &terminal, &claim);
    premature["persistent"] = json!(true);
    denied(
        native.connection.exchange(premature).unwrap(),
        &["not-ready", "condition-unavailable"],
    );
    native.view(Some(&terminal), None);
    assert_eq!(condition(&h, &terminal)["eligibility"], "unknown");
    native.view(Some(&terminal), Some(true));
    assert_eq!(condition(&h, &terminal)["eligibility"], "ineligible");
    native.view(Some(&terminal), Some(false));
    conditional_ready(&h, &terminal);
    native.consent(&h, &terminal, &claim);
    let old = offered(&h, &claim);
    native.assert_same_control(&h);
    native.view(Some(&terminal), Some(true));
    assert_eq!(h.handle(&claim["claim"], "read")["phase"], "watching");
    assert_eq!(condition(&h, &terminal)["consent"], "awaiting");
    denied(
        h.response(json!({"op":"control-handle","handle":old["offer"],"method":"accept"})),
        &["stale-offer"],
    );
    let serial = native.serial;
    denied(
        native
            .connection
            .exchange(json!({"persistent":true,"op":"control-handle",
        "handle":native.controller,"method":"view",
        "view":{"serial":serial - 1,"terminal":terminal,"focused":false}}))
            .unwrap(),
        &["stale-view"],
    );
    assert_eq!(condition(&h, &terminal)["eligibility"], "ineligible");
    native.assert_same_control(&h);
    native.view(Some(&terminal), Some(false));
    conditional_ready(&h, &terminal);
    native.consent(&h, &terminal, &claim);
    let fresh = offered(&h, &claim);
    assert_ne!(fresh["offer"], old["offer"]);
    native.keep(&claim);
    denied(
        h.response(json!({"op":"control-handle","handle":fresh["offer"],"method":"accept"})),
        &["stale-offer", "claim-completed"],
    );
    native.assert_same_control(&h);
    assert_eq!(
        h.handle(&claim["claim"], "read")["phase"],
        "aborted-by-owner"
    );
}

#[test]
fn background_all_scope_includes_new_unknown_foreground_and_departed_views_at_acceptance() {
    let h = Host::new();
    let terminal = h.main();
    let mut native = NativeWire::new(&h, &terminal);
    native.view(Some(&terminal), Some(false));
    let mut observer = Connection::connect(&h.socket, DEADLINE).unwrap();
    let view = observer
        .persistent(json!({"op":"controller-create","role":"attachment","label":"second view"}));
    observer.persistent(
        json!({"op":"control-handle","handle":view["controller"],"method":"view",
        "view":{"serial":1,"terminal":terminal,"focused":true}}),
    );
    let requester = h.controller("all views policy");
    // Owner-only ignores another attachment's foreground, all-scope does not.
    let owner_claim = conditional_claim(
        &h,
        &terminal,
        &requester,
        json!({"kind":"views-backgrounded","scope":"owner"}),
    );
    conditional_ready(&h, &terminal);
    h.handle(&owner_claim["claim"], "withdraw");
    native.assert_same_control(&h);
    let claim = conditional_claim(
        &h,
        &terminal,
        &requester,
        json!({"kind":"views-backgrounded","scope":"all"}),
    );
    assert_eq!(condition(&h, &terminal)["eligibility"], "ineligible");
    observer.persistent(
        json!({"op":"control-handle","handle":view["controller"],"method":"view",
        "view":{"serial":2,"terminal":terminal,"focused":false}}),
    );
    conditional_ready(&h, &terminal);
    native.consent(&h, &terminal, &claim);
    let old = offered(&h, &claim);
    let mut joined = Connection::connect(&h.socket, DEADLINE).unwrap();
    let joined_view = joined.persistent(
        json!({"op":"controller-create","role":"attachment","label":"new unknown view"}),
    );
    assert_eq!(condition(&h, &terminal)["eligibility"], "unknown");
    assert_eq!(condition(&h, &terminal)["consent"], "awaiting");
    denied(
        h.response(json!({"op":"control-handle","handle":old["offer"],"method":"accept"})),
        &["stale-offer"],
    );
    native.assert_same_control(&h);
    drop(joined);
    h.until("new view departed and eligibility recovered", || {
        let public = h.ownership(&terminal);
        let departed = public["views"]
            .as_array()?
            .iter()
            .any(|v| v["controller_id"] == joined_view["controller_id"] && v["connected"] == false);
        (departed && public["reservation"]["conditional"]["ready_for_consent"] == true)
            .then_some(())
    });
    assert_eq!(
        condition(&h, &terminal)["consent"],
        "awaiting",
        "departure is never owner consent"
    );
    native.consent(&h, &terminal, &claim);
    let current = offered(&h, &claim);
    assert_ne!(current["offer"], old["offer"]);
    let accepted = h.handle(&current["offer"], "accept");
    assert_eq!(accepted["mode"], "safe");
    assert_ne!(accepted["control"], native.control);
    assert_eq!(
        h.ownership(&terminal)["owner"]["controller_id"],
        requester["controller_id"]
    );
    denied(
        h.response(json!({"op":"control-handle","handle":native.control,"method":"read"})),
        &["stale-handle"],
    );
}

#[test]
fn explicit_release_supersedes_conditional_request_without_policy_and_fresh_now_hands_back_immediately()
 {
    let h = Host::new();
    let terminal = h.main();
    let mut native = NativeWire::new(&h, &terminal);
    native.view(Some(&terminal), Some(false));
    let requester = h.controller("explicit handback recipient");
    // No external request means output, background facts and elapsed time do
    // not schedule anything. Output is real accepted shell work, not input.
    let revision = h.ownership(&terminal)["activity_revision"].clone();
    native.connection.persistent(
        json!({"op":"execute","terminal":terminal,"control":native.control,
        "source":"print -r -- NO_POLICY_OUTPUT"}),
    );
    h.ready(&terminal);
    assert_eq!(h.ownership(&terminal)["activity_revision"], revision);
    assert!(h.ownership(&terminal)["reservation"].is_null());
    let claim = conditional_claim(
        &h,
        &terminal,
        &requester,
        json!({"kind":"human-idle","idle_ms":60000,"warning_ms":2300}),
    );
    h.handle(&native.control, "release");
    assert_eq!(h.handle(&claim["claim"], "read")["phase"], "superseded");
    assert!(h.ownership(&terminal)["reservation"].is_null());
    assert!(h.ownership(&terminal)["owner"].is_null());
    let fresh = h.acquire(&terminal, &requester["controller"]);
    assert_eq!(fresh["mode"], "safe");
    assert_ne!(fresh["control"], native.control);
    assert_eq!(
        h.ownership(&terminal)["owner"]["controller_id"],
        requester["controller_id"]
    );
}

#[test]
fn actual_native_idle_warning_draws_requested_duration_consumes_keep_and_never_rearms() {
    let h = configured_host();
    let terminal = recording_target(&h);
    let mut native = Attachment::new(&h, &terminal, "conditional-warning");
    native.wait_controlled(&h, &terminal);
    native.send(b"\x1b[I"); // Real outer-terminal FocusGained.
    let owner = native_owner(&h, &terminal);
    let generation = h.ownership(&terminal)["observed"]["generation"].clone();
    let requester = h.controller("visible idle warning");
    let claim = conditional_claim(
        &h,
        &terminal,
        &requester,
        json!({"kind":"human-idle","idle_ms":0,"warning_ms":1700}),
    );
    h.until("native warning drawn before consent", || {
        let text = native.text();
        (text.to_lowercase().contains("keep") && text.contains("1700")).then_some(text)
    });
    native.capture("requested-warning-duration");
    h.until("native conditional consent recorded", || {
        (condition(&h, &terminal)["consent"] == "recorded").then_some(())
    });
    native.send(PREFIX); // Warning key is consumed, not a partial future prefix.
    h.until("visible keep abort", || {
        (h.handle(&claim["claim"], "read")["phase"] == "aborted-by-owner").then_some(())
    });
    native.wait_controlled(&h, &terminal);
    native.send(b"AFTER_KEEP");
    wait_bytes(&h, b"AFTER_KEEP");
    assert_owner(&h, &terminal, &owner);
    assert_eq!(h.ownership(&terminal)["observed"]["generation"], generation);
    std::thread::sleep(Duration::from_millis(1800));
    assert!(h.ownership(&terminal)["reservation"].is_null());
    native.capture("same-control-no-rearm");
}

#[test]
fn competing_actual_attachments_default_prefix_and_picker_do_not_steal_or_background_owner() {
    let h = Host::new();
    let terminal = h.main();
    let pid = h.state(&terminal)["pid"].clone();
    // Do not wait for either grant before launching the other real attachment.
    let mut first = Attachment::new(&h, &terminal, "default-first");
    let mut second = Attachment::new(&h, &terminal, "default-second");
    let project = project_name(&h, &terminal);
    let (owner, observer) = h.until("competing native first acquisition", || {
        let a = first.text();
        let b = second.text();
        if first.project_header(&project) && b.contains("Locked") {
            Some((true, false))
        } else if second.project_header(&project) && a.contains("Locked") {
            Some((false, true))
        } else {
            None
        }
    });
    assert_ne!(owner, observer);
    let (owner, observer) = if owner {
        (&mut first, &mut second)
    } else {
        (&mut second, &mut first)
    };
    let identity = native_owner(&h, &terminal);
    owner.send(b"\x1b[I");
    h.until("outer native focus report", || {
        h.ownership(&terminal)["views"]
            .as_array()?
            .iter()
            .any(|view| {
                view["controller_id"] == identity["controller_id"] && view["focused"] == true
            })
            .then_some(())
    });
    let requester = h.controller("picker cannot background policy");
    let background = conditional_claim(
        &h,
        &terminal,
        &requester,
        json!({"kind":"views-backgrounded","scope":"owner"}),
    );
    owner.send(b"\x02");
    owner.wait(" MENU ");
    owner.capture("picker-still-foreground");
    assert_eq!(condition(&h, &terminal)["eligibility"], "ineligible");
    assert_eq!(condition(&h, &terminal)["ready_for_consent"], false);
    assert_owner(&h, &terminal, &identity);
    owner.hide_panel();
    owner.wait_controlled(&h, &terminal);
    owner.send(b"\x1b[O");
    owner.wait("Background consent (owner)");
    owner.capture("owner-background-consent-no-countdown");
    assert_eq!(
        condition(&h, &terminal)["clock"],
        json!({"kind":"none","remaining_ms":null})
    );
    offered(&h, &background);
    assert_owner(&h, &terminal, &identity);
    h.handle(&background["claim"], "withdraw");
    owner.send(b"\x1b[I");
    observer.send(b"\x1b[I");
    h.until("both original native views foreground", || {
        h.ownership(&terminal)["views"]
            .as_array()?
            .iter()
            .filter(|view| view["connected"] == true && view["focused"] == true)
            .count()
            .eq(&2)
            .then_some(())
    });
    let all_background = conditional_claim(
        &h,
        &terminal,
        &requester,
        json!({"kind":"views-backgrounded","scope":"all"}),
    );
    owner.send(b"\x1b[O");
    h.until(
        "all scope still blocked by other foreground attachment",
        || (condition(&h, &terminal)["eligibility"] == "ineligible").then_some(()),
    );
    observer.send(b"\x1b[O");
    owner.wait("Background consent (all)");
    owner.capture("all-background-consent-no-countdown");
    offered(&h, &all_background);
    assert_owner(&h, &terminal, &identity);
    h.handle(&all_background["claim"], "withdraw");
    // Default-prefix Ctrl-L goes directly to the one cooperative request.
    observer.send(b"\x02\x0c");
    observer.wait("Waiting");
    let claim = reservation(&h, &terminal);
    let observer_id = h.ownership(&terminal)["views"]
        .as_array()
        .unwrap()
        .iter()
        .find(|view| {
            view["connected"] == true && view["controller_id"] != identity["controller_id"]
        })
        .unwrap()["controller_id"]
        .clone();
    assert_eq!(claim["purpose"], "manual");
    assert_eq!(claim["requester_id"], observer_id);
    observer.send(b"\x1b");
    observer.send(b"\x02\x0c");
    observer.wait("Waiting");
    assert_eq!(reservation(&h, &terminal)["claim_id"], claim["claim_id"]);
    assert_owner(&h, &terminal, &identity);
    assert_eq!(h.state(&terminal)["pid"], pid);
    first.capture("first-final");
    second.capture("second-final");
}

#[test]
fn actual_native_warning_paste_mouse_and_local_selection_keep_the_same_authority() {
    let h = configured_host();
    let terminal = recording_target(&h);
    let mut native = Attachment::new(&h, &terminal, "warning-input-routes");
    native.wait_controlled(&h, &terminal);
    native.send(b"\x1b[I");
    let identity = native_owner(&h, &terminal);
    let generation = h.ownership(&terminal)["observed"]["generation"].clone();
    let requester = h.controller("explicit per-route idle policy");
    let mut expected = Vec::new();
    for (name, sequence, application) in [
        ("paste", b"\x1b[200~WARNING_PASTE\x1b[201~".as_slice(), true),
        (
            "application-mouse",
            b"\x1b[<0;14;9M\x1b[<0;14;9m".as_slice(),
            true,
        ),
        // Shift forces wrapper-local selection even when native mouse mode is on.
        (
            "local-selection",
            b"\x1b[<4;5;5M\x1b[<36;12;5M\x1b[<4;12;5m".as_slice(),
            false,
        ),
    ] {
        let claim = conditional_claim(
            &h,
            &terminal,
            &requester,
            json!({"kind":"human-idle","idle_ms":0,"warning_ms":1700}),
        );
        h.until("drawn per-route warning", || {
            native
                .text()
                .contains("Keep control: any key")
                .then_some(())
        });
        h.until("recorded per-route consent", || {
            (condition(&h, &terminal)["consent"] == "recorded").then_some(())
        });
        native.capture(&format!("{name}-warning"));
        let before = bytes(&h);
        native.send(sequence);
        h.until("human route aborts this attempt", || {
            (h.handle(&claim["claim"], "read")["phase"] == "aborted-by-owner").then_some(())
        });
        if application {
            h.until("post-consent application admission", || {
                (bytes(&h).len() > before.len()).then_some(())
            });
            if name == "paste" {
                h.until("complete bracketed native paste", || {
                    bytes(&h).ends_with(b"\x1b[201~").then_some(())
                });
                assert_eq!(
                    &bytes(&h)[before.len()..],
                    b"\x1b[200~WARNING_PASTE\x1b[201~"
                );
            } else {
                h.until("complete native mouse release", || {
                    bytes(&h).ends_with(b"m").then_some(())
                });
                let received = bytes(&h);
                let translated = &received[before.len()..];
                assert_eq!(
                    translated, b"\x1b[<0;14;6M\x1b[<0;14;6m",
                    "mouse body coordinates changed"
                );
            }
            expected = bytes(&h);
        } else {
            native.wait_controlled(&h, &terminal);
            assert_eq!(bytes(&h), expected, "local selection became native input");
        }
        assert_owner(&h, &terminal, &identity);
        assert_eq!(h.ownership(&terminal)["observed"]["generation"], generation);
        assert!(h.ownership(&terminal)["reservation"].is_null());
        native.capture(&format!("{name}-kept"));
    }
}

// A transparent framing proxy delays one real daemon response, never fabricates
// ownership, outcomes, capabilities or terminal output. All attachment activity
// still uses the same original persistent daemon connection behind the proxy.
struct ConsentGate {
    socket: PathBuf,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    seen: std::sync::Arc<std::sync::atomic::AtomicBool>,
    release: std::sync::Arc<(parking_lot::Mutex<bool>, parking_lot::Condvar)>,
    hold_method: Arc<parking_lot::Mutex<Option<&'static str>>>,
    held_reply: Arc<parking_lot::Mutex<Value>>,
    methods: std::sync::Arc<parking_lot::Mutex<Vec<String>>>,
    streams: std::sync::Arc<parking_lot::Mutex<Vec<UnixStream>>>,
    native_streams: std::sync::Arc<parking_lot::Mutex<Vec<UnixStream>>>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl ConsentGate {
    fn new(h: &Host) -> Self {
        use parking_lot::{Condvar, Mutex};
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let socket = h.root.join("delayed-consent.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let seen = Arc::new(AtomicBool::new(false));
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let hold_method = Arc::new(Mutex::new(Some("consent-to")));
        let held_reply = Arc::new(Mutex::new(Value::Null));
        let (holding, held) = (hold_method.clone(), held_reply.clone());
        let methods = Arc::new(Mutex::new(Vec::new()));
        let streams = Arc::new(Mutex::new(Vec::new()));
        let native_streams = Arc::new(Mutex::new(Vec::new()));
        let native_sockets = native_streams.clone();
        let daemon_socket = h.socket.clone();
        let (stopping, observed, barrier, calls, sockets) = (
            stop.clone(),
            seen.clone(),
            release.clone(),
            methods.clone(),
            streams.clone(),
        );
        let worker = std::thread::spawn(move || {
            let mut clients = Vec::new();
            while !stopping.load(Ordering::SeqCst) {
                let (client, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(_) => break,
                };
                // Darwin accept inherits the listener's O_NONBLOCK. A framing
                // reader must wait for later/fragmented requests, not abandon
                // the handshake on WouldBlock while retained clones stay open.
                client.set_nonblocking(false).unwrap();
                let Ok(server) = UnixStream::connect(&daemon_socket) else {
                    continue;
                };
                client.set_read_timeout(Some(DEADLINE)).unwrap();
                server.set_read_timeout(Some(DEADLINE)).unwrap();
                client.set_write_timeout(Some(DEADLINE)).unwrap();
                server.set_write_timeout(Some(DEADLINE)).unwrap();
                sockets
                    .lock()
                    .extend([client.try_clone().unwrap(), server.try_clone().unwrap()]);
                let (observed, barrier, calls, native_sockets, holding, held) = (
                    observed.clone(),
                    barrier.clone(),
                    calls.clone(),
                    native_sockets.clone(),
                    holding.clone(),
                    held.clone(),
                );
                clients.push(std::thread::spawn(move || {
                    let mut client = BufReader::new(client);
                    let mut server = BufReader::new(server);
                    let mut native = false;
                    loop {
                        let mut request = String::new();
                        if !matches!(client.read_line(&mut request), Ok(n) if n > 0) {
                            break;
                        }
                        let Ok(value) = serde_json::from_str::<Value>(&request) else {
                            break;
                        };
                        let method = value["method"]
                            .as_str()
                            .or_else(|| value["op"].as_str())
                            .unwrap_or("")
                            .to_owned();
                        if value["op"] == "controller-create" && value["role"] == "attachment" {
                            native = true;
                            native_sockets.lock().extend([
                                client.get_ref().try_clone().unwrap(),
                                server.get_ref().try_clone().unwrap(),
                            ]);
                        }
                        if native {
                            calls.lock().push(method.clone());
                        }
                        if server.get_mut().write_all(request.as_bytes()).is_err() {
                            break;
                        }
                        let mut response = String::new();
                        if !matches!(server.read_line(&mut response), Ok(n) if n > 0) {
                            break;
                        }
                        let delay = native && {
                            let mut holding = holding.lock();
                            if holding.as_deref() == Some(method.as_str()) {
                                holding.take();
                                true
                            } else {
                                false
                            }
                        };
                        if delay {
                            *held.lock() = serde_json::from_str(&response).unwrap();
                            observed.store(true, Ordering::SeqCst);
                            let (lock, wake) = &*barrier;
                            let mut released = lock.lock();
                            let deadline = Instant::now() + DEADLINE;
                            while !*released {
                                let remaining = deadline.saturating_duration_since(Instant::now());
                                if remaining.is_zero()
                                    || wake.wait_for(&mut released, remaining).timed_out()
                                {
                                    break;
                                }
                            }
                        }
                        if client.get_mut().write_all(response.as_bytes()).is_err() {
                            break;
                        }
                    }
                    let _ = client.get_ref().shutdown(std::net::Shutdown::Both);
                    let _ = server.get_ref().shutdown(std::net::Shutdown::Both);
                }));
            }
            for socket in sockets.lock().iter() {
                let _ = socket.shutdown(std::net::Shutdown::Both);
            }
            let deadline = Instant::now() + CLEANUP_TIMEOUT;
            for client in clients {
                assert!(
                    join_until(client, deadline),
                    "native consent proxy client cleanup deadline"
                );
            }
        });
        Self {
            socket,
            stop,
            seen,
            release,
            hold_method,
            held_reply,
            methods,
            streams,
            native_streams,
            worker: Some(worker),
        }
    }
    fn pause_next(&self, method: &'static str) {
        *self.release.0.lock() = false;
        self.seen.store(false, Ordering::SeqCst);
        *self.held_reply.lock() = Value::Null;
        *self.hold_method.lock() = Some(method);
    }
    fn release(&self) {
        let (lock, wake) = &*self.release;
        *lock.lock() = true;
        wake.notify_all();
    }
    fn disconnect_native(&self) {
        for socket in self.native_streams.lock().iter() {
            let _ = socket.shutdown(std::net::Shutdown::Both);
        }
    }
}
impl Drop for ConsentGate {
    fn drop(&mut self) {
        self.release();
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        for socket in self.streams.lock().iter() {
            let _ = socket.shutdown(std::net::Shutdown::Both);
        }
        if let Some(worker) = self.worker.take() {
            let joined = join_until(worker, Instant::now() + CLEANUP_TIMEOUT);
            if !joined {
                if std::thread::panicking() {
                    eprintln!("native consent proxy cleanup deadline");
                } else {
                    panic!("native consent proxy cleanup deadline");
                }
            }
        }
        let _ = fs::remove_file(&self.socket);
    }
}

#[test]
fn actual_native_pending_consent_ack_keeps_before_paste_without_rotating_or_replaying_prefix() {
    let h = configured_host();
    let terminal = recording_target(&h);
    let gate = ConsentGate::new(&h);
    let mut native = Attachment::on_socket(&h, &terminal, "pending-consent-ack", &gate.socket);
    native.wait_controlled(&h, &terminal);
    native.send(b"\x1b[I");
    let identity = native_owner(&h, &terminal);
    let generation = h.ownership(&terminal)["observed"]["generation"].clone();
    let requester = h.controller("pending acknowledgement policy");
    let claim = conditional_claim(
        &h,
        &terminal,
        &requester,
        json!({"kind":"human-idle","idle_ms":0,"warning_ms":2300}),
    );
    h.until("real consent reply withheld", || {
        native.drain();
        gate.seen
            .load(std::sync::atomic::Ordering::SeqCst)
            .then_some(())
    });
    native.wait("Keep control: any key");
    native.capture("warning-before-consent-ack");
    // Queue one warning key plus a paste while its acknowledgement cannot be
    // received. The prefix is consumed; paste follows an explicit FIFO keep.
    native.send(b"\x01\x1b[200~ACK_WINDOW_PASTE\x1b[201~");
    h.until("responsive native pending-ack keep presentation", || {
        native
            .text()
            .to_lowercase()
            .contains("keeping")
            .then_some(())
    });
    native.capture("keep-queued-before-ack");
    gate.release();
    h.until("pending-ack keep aborts original attempt", || {
        (h.handle(&claim["claim"], "read")["phase"] == "aborted-by-owner").then_some(())
    });
    wait_bytes(&h, b"\x1b[200~ACK_WINDOW_PASTE\x1b[201~");
    assert_owner(&h, &terminal, &identity);
    assert_eq!(h.ownership(&terminal)["observed"]["generation"], generation);
    let methods = gate.methods.lock();
    let consent = methods
        .iter()
        .position(|method| method == "consent-to")
        .unwrap();
    let keep = methods
        .iter()
        .enumerate()
        .skip(consent + 1)
        .find(|(_, method)| method.as_str() == "keep")
        .map(|(index, _)| index)
        .unwrap();
    assert!(keep > consent);
    let input = methods
        .iter()
        .enumerate()
        .skip(consent + 1)
        .find(|(_, method)| method.as_str() == "input")
        .map(|(index, _)| index)
        .unwrap();
    assert!(
        keep < input,
        "pending-ack native paste preceded explicit keep"
    );
    drop(methods);
    assert!(!bytes(&h).contains(&1), "consumed warning prefix replayed");
    native.capture("pending-ack-same-authority");
    let pid = h.state(&terminal)["pid"].clone();
    gate.disconnect_native();
    h.until("fresh observer connection after transport loss", || {
        native.drain();
        h.ownership(&terminal)["views"]
            .as_array()?
            .iter()
            .any(|view| {
                view["connected"] == true && view["controller_id"] != identity["controller_id"]
            })
            .then_some(())
    });
    native.wait("Locked");
    native.capture("disconnected-fresh-observer-gated");
    native.send(b"NEVER_REPLAY_UNKNOWN");
    native.wait("Locked terminal");
    assert_eq!(bytes(&h), b"\x1b[200~ACK_WINDOW_PASTE\x1b[201~");
    assert_owner(&h, &terminal, &identity);
    assert_eq!(h.state(&terminal)["pid"], pid);
}

#[test]
fn actual_native_safe_offer_accepts_only_after_deliberate_yield_and_never_replays_prompt_input() {
    let h = configured_host();
    let terminal = h.main();
    let pid = h.state(&terminal)["pid"].clone();
    let mut owner = Attachment::new(&h, &terminal, "safe-owner");
    owner.wait_controlled(&h, &terminal);
    let identity = native_owner(&h, &terminal);
    // Open before any request exists. The same visible owner response panel
    // must follow a later reservation instead of retaining its Release action.
    owner.action(12);
    owner.wait("Control owned here");
    owner.capture("owner-response-before-request");
    let mut observer = Attachment::new(&h, &terminal, "safe-requester");
    observer.wait("Locked");
    let intercepted = format!(
        "print -r -- INTERCEPTED > '{}'\r",
        h.root.join("intercepted").display()
    );
    observer.send(format!("\x1b[200~{intercepted}\x1b[201~").as_bytes());
    observer.wait("Locked terminal");
    observer.capture("safe-prompt-before-request");
    assert!(h.ownership(&terminal)["reservation"].is_null());
    observer.send(b"\r");
    observer.wait("Waiting");
    let claim = reservation(&h, &terminal);
    assert_owner(&h, &terminal, &identity);
    assert!(
        h.ownership(&terminal)["unsettled"]
            .as_array()
            .unwrap()
            .is_empty(),
        "an intercepted paste/confirmation must not create native dispatch"
    );
    h.until(
        "open owner panel displays this exact ordinary request",
        || {
            owner
                .text()
                .contains(claim["claim_id"].as_str().unwrap())
                .then_some(())
        },
    );
    owner.wait("y Yield to this request");
    owner.capture("owner-response-current-request");
    owner.send(b"y");
    observer.wait_controlled(&h, &terminal);
    let public = h.ownership(&terminal);
    assert_eq!(public["owner"]["controller_id"], claim["requester_id"]);
    assert!(public["reservation"].is_null());
    assert!(!h.root.join("intercepted").exists());
    let fresh = format!(
        "print -r -- SAFE_FRESH > '{}'\r",
        h.root.join("safe-fresh").display()
    );
    observer.send(fresh.as_bytes());
    h.until("fresh input after committed safe grant", || {
        fs::read_to_string(h.root.join("safe-fresh"))
            .ok()
            .filter(|value| value == "SAFE_FRESH\n")
    });
    assert!(
        !h.root.join("intercepted").exists(),
        "intercepted paste replayed after safe grant"
    );
    assert_eq!(h.state(&terminal)["pid"], pid);
    observer.capture("safe-grant-fresh-input-only");
}

#[test]
fn actual_native_outer_output_drains_while_test_waits_only_on_daemon() {
    let h = configured_host();
    let terminal = h.main();
    let controller = h.controller("continuous-output fixture");
    let grant = h.acquire(&terminal, &controller["controller"]);
    let mut native = Attachment::new(&h, &terminal, "continuous-outer-output");
    native.wait("Locked");
    let source = format!(
        "for n in {{1..300}}; do printf 'OUTER_%04d %080d\\n' $n 0; sleep 0.002; done; print -r -- OUTER_DRAIN_DONE; {}",
        h.marker_source("outer-output-finished")
    );
    h.execute(&terminal, &grant["control"], &source);
    // No text(), wait(), capture(), read() or explicit draining while the
    // application produces more output than the outer PTY can buffer.
    h.marker("outer-output-finished");
    h.until(
        "outer PTY reaches the final frame without foreground reads",
        || {
            native
                .lines()
                .iter()
                .any(|line| line.trim_end() == "OUTER_DRAIN_DONE")
                .then_some(())
        },
    );
    assert_eq!(
        h.ownership(&terminal)["owner"]["controller_id"],
        controller["controller_id"]
    );
    native.capture("continuous-drain-final-frame");
    let native_id = h.ownership(&terminal)["views"]
        .as_array()
        .unwrap()
        .iter()
        .find(|view| view["connected"] == true)
        .unwrap()["controller_id"]
        .clone();
    native.action(12);
    let claim = reservation(&h, &terminal);
    assert_eq!(claim["requester_id"], native_id);
    assert_eq!(claim["phase"], "waiting-owner");
    assert_eq!(
        h.ownership(&terminal)["owner"]["controller_id"],
        controller["controller_id"]
    );
    native.capture("continuous-drain-responsive-request");
}

#[test]
fn actual_native_deliberate_request_observes_withdrawal_before_one_claim() {
    let h = configured_host();
    let terminal = h.main();
    let mut human = NativeWire::new(&h, &terminal);
    human.view(Some(&terminal), Some(false));
    let owner = h.ownership(&terminal)["owner"].clone();
    let gate = ConsentGate::new(&h);
    let mut observer =
        Attachment::on_socket(&h, &terminal, "fresh-deliberate-request", &gate.socket);
    observer.wait("Locked");
    let observer_id = h.ownership(&terminal)["views"]
        .as_array()
        .unwrap()
        .iter()
        .find(|view| view["connected"] == true && view["controller_id"] != owner["controller_id"])
        .unwrap()["controller_id"]
        .clone();
    let requester = h.controller("withdrawal observation fixture");
    let conditional = conditional_claim(
        &h,
        &terminal,
        &requester,
        json!({"kind":"views-backgrounded","scope":"owner"}),
    );
    conditional_ready(&h, &terminal);
    human.consent(&h, &terminal, &conditional);
    offered(&h, &conditional);
    // Let one complete refresh deliver the conditional observation first.
    // Its next controller read can only be queued after the TUI consumes that
    // refresh, so the later manual request cannot accidentally use an older,
    // already-unreserved receipt that happens to match the withdrawn state.
    gate.pause_next("state");
    h.until("conditional observation held", || {
        gate.seen.load(Ordering::SeqCst).then_some(())
    });
    let after_state = gate.methods.lock().len();
    gate.release();
    h.until("TUI consumes the conditional observation", || {
        gate.methods
            .lock()
            .iter()
            .skip(after_state)
            .any(|method| method == "read")
            .then_some(())
    });
    // Hold a subsequent real response across withdrawal. The Force panel is
    // only a way to observe its exact captured receipt; never confirm Force.
    gate.pause_next("state");
    h.until("original native state response held", || {
        gate.seen.load(Ordering::SeqCst).then_some(())
    });
    observer.action(6);
    h.until("observer displays the conditional reservation", || {
        observer
            .text()
            .contains(conditional["claim_id"].as_str().unwrap())
            .then_some(())
    });
    observer.capture("conditional-observation-before-withdrawal");
    let displayed = gate.held_reply.lock()["result"]["ownership"].clone();
    assert_eq!(
        displayed["reservation"]["claim_id"],
        conditional["claim_id"]
    );
    h.handle(&conditional["claim"], "withdraw");
    let withdrawn = h.ownership(&terminal);
    assert!(withdrawn["reservation"].is_null());
    observer.hide_panel(); // Leave the unconfirmed Force modal before a new prefix gesture.
    observer.action(12);
    observer.wait("Waiting");
    gate.release();
    let claim = reservation(&h, &terminal);
    assert_eq!(claim["purpose"], "manual");
    assert_eq!(claim["phase"], "waiting-owner");
    assert_eq!(claim["requester_id"], observer_id);
    human.assert_same_control(&h);
    let methods = gate.methods.lock().clone();
    assert_eq!(
        methods
            .iter()
            .filter(|method| method.as_str() == "ownership-claim")
            .count(),
        1,
        "a deliberate request must not retry its mutation"
    );
    let claim_index = methods
        .iter()
        .position(|method| method == "ownership-claim")
        .unwrap();
    assert_eq!(
        methods.get(claim_index.wrapping_sub(1)).map(String::as_str),
        Some("state"),
        "the one claim must compare a fresh original-connection observation"
    );
    fs::write(
        observer.captures.join("fresh-request-protocol.json"),
        serde_json::to_vec_pretty(&json!({
            "held_ownership":displayed,
            "withdrawn_ownership":withdrawn,
            "manual_reservation":claim,
            "original_connection_methods":methods
        }))
        .unwrap(),
    )
    .unwrap();
    observer.capture("fresh-request-after-withdrawal");
}

#[test]
fn e06s03_native_recovery_registration_resize_reconciles_surviving_geometry() {
    let h = configured_host();
    let terminal = recording_target(&h);
    let gate = ConsentGate::new(&h);
    let mut native = Attachment::on_socket(&h, &terminal, "recovery-resize", &gate.socket);
    native.wait_controlled(&h, &terminal);
    let owner = native_owner(&h, &terminal);
    let generation = h.ownership(&terminal)["observed"]["generation"].clone();
    let pid = h.state(&terminal)["pid"].clone();
    let recorded = bytes(&h);
    gate.pause_next("controller-create");
    gate.disconnect_native();
    h.until("fresh recovery registration response withheld", || {
        gate.seen.load(Ordering::SeqCst).then_some(())
    });
    // Real outer window change, while setup still waits for its registration.
    let (cols, rows) = (134, 36);
    native.output.lock().term.resize(Geometry { cols, rows });
    let size = libc::winsize {
        ws_row: rows as u16,
        ws_col: cols as u16,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    assert_eq!(
        unsafe { libc::ioctl(native.master.as_raw_fd(), libc::TIOCSWINSZ, &size) },
        0
    );
    gate.release();
    native.wait("Reconnected");
    let screen = h.call(json!({"op":"screen","terminal":terminal,"offset":0,"limit":rows - 3}));
    assert_eq!(
        screen["cols"], cols,
        "ready recovery retained stale PTY width"
    );
    assert_eq!(
        screen["rows"],
        rows - 3,
        "ready recovery retained stale PTY height"
    );
    assert_owner(&h, &terminal, &owner);
    assert_eq!(h.ownership(&terminal)["observed"]["generation"], generation);
    assert!(h.ownership(&terminal)["reservation"].is_null());
    assert_eq!(h.state(&terminal)["pid"], pid);
    assert_eq!(
        bytes(&h),
        recorded,
        "structural resize became application input"
    );
    native.capture("recovered-live-geometry");
    native.detach();
}

#[test]
fn e06s03_native_sustained_incomplete_paste_yields_to_resize_and_disconnect() {
    let h = configured_host();
    let terminal = recording_target(&h);
    let external = h.controller("incomplete-input-owner");
    h.acquire(&terminal, &external["controller"]);
    let generation = h.ownership(&terminal)["observed"]["generation"].clone();
    let pid = h.state(&terminal)["pid"].clone();
    let gate = ConsentGate::new(&h);
    let mut native =
        Attachment::on_socket(&h, &terminal, "sustained-incomplete-input", &gate.socket);
    native.wait("Locked");
    let original = h.until("original observer has a connected view", || {
        h.ownership(&terminal)["views"]
            .as_array()?
            .iter()
            .find(|view| view["connected"] == true && view["reported"] == true)
            .map(|view| view["controller_id"].clone())
    });
    let mut producer = None;
    let mut queued_while_stopped = 0;
    native.with_stopped(|native| {
        let writer = IncompleteInput::new(native);
        h.until(
            "unterminated paste fills the stopped client's real PTY",
            || {
                writer.active_bytes();
                writer.backpressured.load(Ordering::Acquire).then_some(())
            },
        );
        queued_while_stopped = writer.active_bytes();
        producer = Some(writer);
    });
    let producer = producer.unwrap();
    h.until(
        "consumer reads incomplete input while producer stays active",
        || (producer.active_bytes() > queued_while_stopped + 4096).then_some(()),
    );
    // The writer never sends a paste terminator. Its continued progress beyond
    // the stopped-client queue proves that the real reader entered the stream
    // before SIGWINCH, rather than handling resize before seeing any input.
    let before_resize = producer.active_bytes();
    let (cols, rows) = (132, 35);
    native.resize(&h, &terminal, cols, rows);
    h.until(
        "producer remains active across actual resize observation",
        || (producer.active_bytes() > before_resize).then_some(()),
    );
    assert_owner(&h, &terminal, &external);
    assert_eq!(h.ownership(&terminal)["observed"]["generation"], generation);
    assert_eq!(h.state(&terminal)["pid"], pid);
    assert!(
        bytes(&h).is_empty(),
        "unterminated paste reached the application"
    );

    // A held, real replacement-registration reply is a causal witness that the
    // UI observed disconnection while the same unterminated producer was live.
    gate.pause_next("controller-create");
    gate.disconnect_native();
    h.until(
        "client reaches recovery registration during sustained input",
        || {
            producer.active_bytes();
            gate.seen.load(Ordering::SeqCst).then_some(())
        },
    );
    let fresh = gate.held_reply.lock()["result"]["controller_id"].clone();
    assert!(fresh.is_string());
    assert_ne!(fresh, original);
    assert_owner(&h, &terminal, &external);
    assert!(h.ownership(&terminal)["reservation"].is_null());
    assert!(bytes(&h).is_empty());
    let written = producer.finish(); // Cancels and joins before any later input.
    assert!(written > queued_while_stopped + 4096);
    gate.release();
    // Complete the old physical paste only after stopping its sole writer.
    // Recovery must consume it, not release it to this or a later owner.
    native.send(b"\x1b[201~");
    native.wait("Reconnected");
    native.wait("Locked");
    native.send(b"\x1b[I");
    h.until(
        "fresh post-discard focus proves the consumer is responsive",
        || {
            h.ownership(&terminal)["views"]
                .as_array()?
                .iter()
                .any(|view| {
                    view["controller_id"] == fresh
                        && view["connected"] == true
                        && view["reported"] == true
                        && view["focused"] == true
                })
                .then_some(())
        },
    );
    assert_owner(&h, &terminal, &external);
    assert_eq!(h.ownership(&terminal)["observed"]["generation"], generation);
    assert!(h.ownership(&terminal)["reservation"].is_null());
    assert_eq!(h.state(&terminal)["pid"], pid);
    assert!(
        bytes(&h).is_empty(),
        "old paste was replayed after recovery"
    );
    assert!(
        !gate
            .methods
            .lock()
            .iter()
            .any(|method| method == "ownership-claim" || method == "consent-to")
    );
    let screen = h.call(json!({"op":"screen","terminal":terminal,"offset":0,"limit":rows - 3}));
    assert_eq!(screen["cols"], cols);
    assert_eq!(screen["rows"], rows - 3);
    native.capture("sustained-input-resize-disconnect-no-replay");
    native.detach();
}

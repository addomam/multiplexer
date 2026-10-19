use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::{Config, TermDamage, TermMode, cell::Flags};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Processor, Rgb};
use anyhow::{Result, bail};
use parking_lot::Mutex;
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde_json::{Value, json};

mod compatibility;
use compatibility::{Compatibility, TerminalHandler};

const INPUT_QUEUE: usize = 128;
pub(crate) const MAX_INPUT: usize = 65536;

#[derive(Clone, Copy)]
struct Size {
    cols: usize,
    rows: usize,
}
impl Dimensions for Size {
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

struct Input {
    bytes: Vec<u8>,
    ack: Option<mpsc::Sender<std::io::Result<()>>>,
}

#[derive(Clone)]
struct Listener {
    input: mpsc::SyncSender<Input>,
    size: Arc<Mutex<Size>>,
    color_requests: mpsc::SyncSender<Event>,
}
impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let response = match event {
            Event::PtyWrite(text) => Some(text),
            request @ Event::ColorRequest(..) => {
                // Processing already holds the emulator lock. Resolve against the
                // updated palette after this batch, without blocking PTY output.
                let _ = self.color_requests.try_send(request);
                None
            }
            Event::TextAreaSizeRequest(format) => {
                let size = *self.size.lock();
                Some(format(WindowSize {
                    num_lines: size.rows as u16,
                    num_cols: size.cols as u16,
                    cell_width: 0,
                    cell_height: 0,
                }))
            }
            // Clipboard reads are deliberately denied; a managed process must not read the host clipboard.
            Event::ClipboardLoad(_, format) => Some(format("")),
            _ => None,
        };
        if let Some(response) = response {
            let _ = self.input.try_send(Input {
                bytes: response.into_bytes(),
                ack: None,
            });
        }
    }
}

struct Emulator {
    term: Term<Listener>,
    processor: Processor,
    compatibility: Compatibility,
    color_requests: mpsc::Receiver<Event>,
    input: mpsc::SyncSender<Input>,
    generation: u64,
    full_damage_generation: u64,
    row_generations: [u64; 300],
}
impl Emulator {
    fn new(
        mut term: Term<Listener>,
        color_requests: mpsc::Receiver<Event>,
        input: mpsc::SyncSender<Input>,
        history_limit: usize,
    ) -> Self {
        term.reset_damage();
        Self {
            term,
            processor: Processor::new(),
            compatibility: Compatibility::new(history_limit),
            color_requests,
            input,
            generation: 1,
            full_damage_generation: 1,
            row_generations: [1; 300],
        }
    }
    fn advance(&mut self, bytes: &[u8]) {
        self.processor.advance(
            &mut TerminalHandler::new(&mut self.term, &mut self.compatibility, &self.input),
            bytes,
        );
        self.reply_colors();
        self.publish_damage();
    }
    fn flush_sync(&mut self) {
        if self
            .processor
            .sync_timeout()
            .sync_timeout()
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.processor.stop_sync(&mut TerminalHandler::new(
                &mut self.term,
                &mut self.compatibility,
                &self.input,
            ));
            self.reply_colors();
            self.publish_damage();
        }
    }
    fn resize(&mut self, size: Size) {
        self.compatibility.resize(&self.term, size);
        self.term.resize(size);
        self.publish_damage();
    }
    fn publish_damage(&mut self) {
        // Consume damage on mutation, not on requests, so clients can skip
        // generations without losing rows observed by another client.
        self.generation += 1;
        match self.term.damage() {
            TermDamage::Full => self.full_damage_generation = self.generation,
            TermDamage::Partial(_) if self.compatibility.full_damage => {
                self.full_damage_generation = self.generation;
            }
            TermDamage::Partial(lines) => {
                for line in lines {
                    self.row_generations[line.line] = self.generation;
                }
            }
        }
        for (row, dirty) in self.compatibility.dirty_rows.iter_mut().enumerate() {
            if std::mem::take(dirty) {
                self.row_generations[row] = self.generation;
            }
        }
        self.compatibility.full_damage = false;
        self.term.reset_damage();
    }
    fn screen_update(
        &self,
        offset: usize,
        limit: Option<usize>,
        since: Option<u64>,
        row_updates: bool,
    ) -> Value {
        let generation = self.generation;
        if !row_updates {
            return if since == Some(generation) {
                json!({"unchanged":true,"generation":generation})
            } else {
                screen(&self.term, offset, limit, generation)
            };
        }
        let Some(since) = since.filter(|since| {
            offset == 0 && *since >= self.full_damage_generation && *since <= generation
        }) else {
            return screen(&self.term, offset, limit, generation);
        };
        if since == generation {
            return json!({"unchanged":true,"generation":generation});
        }
        let rows = limit
            .unwrap_or(self.term.screen_lines())
            .min(300)
            .min(self.term.screen_lines());
        let indices: Vec<usize> = self
            .row_generations
            .iter()
            .take(rows)
            .enumerate()
            .filter_map(|(row, changed)| (*changed > since).then_some(row))
            .collect();
        let mut update = screen_rows(&self.term, 0, rows, generation, indices.iter().copied());
        update["delta"] = json!(true);
        update["base_generation"] = json!(since);
        update["row_indices"] = json!(indices);
        update
    }
    fn reply_colors(&self) {
        while let Ok(Event::ColorRequest(index, format)) = self.color_requests.try_recv() {
            let [r, g, b] = indexed_color(&self.term, index).unwrap_or([229, 229, 229]);
            let _ = self.input.try_send(Input {
                bytes: format(Rgb { r, g, b }).into_bytes(),
                ack: None,
            });
        }
    }
}

pub struct Terminal {
    pub id: String,
    pub pid: u32,
    processes: Mutex<ProcessTree>,
    emulator: Mutex<Emulator>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    input: mpsc::SyncSender<Input>,
    pub input_gate: Mutex<()>,
    pub(crate) input_epoch: AtomicU64,
    size: Arc<Mutex<Size>>,
    alive: AtomicBool,
    closing: AtomicBool,
    exit_status: Mutex<Option<u32>>,
}

// A PTY child is not owned by Terminal's normal cleanup until setup completes.
// In particular, descriptor duplication and process-identity capture can fail.
struct UninstalledChild(Option<Box<dyn portable_pty::Child + Send + Sync>>);
impl Drop for UninstalledChild {
    fn drop(&mut self) {
        let Some(mut child) = self.0.take() else {
            return;
        };
        if let Some(pid) = child.process_id() {
            // The owned child has not been reaped, so its PID cannot be reused.
            // Kill its PTY process group as well as the root before reaping.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
                libc::kill(pid as i32, libc::SIGKILL);
            }
        } else {
            let _ = child.kill();
        }
        let _ = child.wait();
    }
}

impl Terminal {
    pub fn spawn(
        id: String,
        cwd: &Path,
        env: &BTreeMap<String, String>,
        on_exit: Arc<dyn Fn(u32) + Send + Sync>,
    ) -> Result<Arc<Self>> {
        let pair = native_pty_system().openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        // Resolve the supported stock shell without adding a missing captured PATH.
        let mut command = CommandBuilder::new("/bin/zsh");
        command.env_clear();
        command.arg("-il");
        command.cwd(cwd);
        for (key, value) in env {
            command.env(key, value);
        }
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        let mut pending_child = UninstalledChild(Some(pair.slave.spawn_command(command)?));
        let pid = pending_child
            .0
            .as_ref()
            .unwrap()
            .process_id()
            .ok_or_else(|| anyhow::anyhow!("PTY child has no process id"))?;
        let root = process_info(pid as i32)
            .ok_or_else(|| anyhow::anyhow!("cannot capture PTY child process identity"))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let mut writer = pair.master.take_writer()?;
        let (input, pending) = mpsc::sync_channel::<Input>(INPUT_QUEUE);
        let (color_requests, pending_colors) = mpsc::sync_channel(INPUT_QUEUE);
        let size = Arc::new(Mutex::new(Size { cols: 80, rows: 24 }));
        let listener = Listener {
            input: input.clone(),
            size: size.clone(),
            color_requests,
        };
        let config = Config {
            scrolling_history: 50000,
            ..Config::default()
        };
        let history_limit = config.scrolling_history;
        let term = Term::new(config, &*size.lock(), listener);
        let terminal = Arc::new(Self {
            id,
            pid,
            processes: Mutex::new(ProcessTree::new(root)),
            emulator: Mutex::new(Emulator::new(
                term,
                pending_colors,
                input.clone(),
                history_limit,
            )),
            master: Mutex::new(pair.master),
            input,
            input_gate: Mutex::new(()),
            input_epoch: AtomicU64::new(0),
            size,
            alive: AtomicBool::new(true),
            closing: AtomicBool::new(false),
            exit_status: Mutex::new(None),
        });
        let weak = Arc::downgrade(&terminal);
        thread::spawn(move || {
            while let Ok(input) = pending.recv() {
                let result = writer.write_all(&input.bytes);
                let failed = result.is_err();
                if let Some(ack) = input.ack {
                    let _ = ack.send(result);
                }
                if failed || weak.upgrade().is_none() {
                    break;
                }
            }
        });
        let weak = Arc::downgrade(&terminal);
        thread::spawn(move || {
            let mut bytes = [0u8; 16384];
            loop {
                let n = match reader.read(&mut bytes) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                let Some(terminal) = weak.upgrade() else {
                    break;
                };
                terminal.emulator.lock().advance(&bytes[..n]);
            }
        });
        let weak = Arc::downgrade(&terminal);
        let mut child = pending_child.0.take().unwrap();
        thread::spawn(move || {
            // Keep the original shell's zombie identity until its descendants have
            // been recorded. Retained natural exits must not rediscover a reused PID.
            let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::uninit();
            let exit_observed = loop {
                let result = unsafe {
                    libc::waitid(
                        libc::P_PID,
                        pid,
                        info.as_mut_ptr(),
                        libc::WEXITED | libc::WNOWAIT,
                    )
                };
                if result == 0 {
                    break true;
                }
                if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                    break false;
                }
            };
            let terminal = weak.upgrade();
            let status = match terminal.as_ref().filter(|_| exit_observed) {
                Some(terminal) => {
                    let mut processes = terminal.processes.lock();
                    processes.discover(&process_snapshot());
                    // Reaping shares the cleanup lock: the owned shell cannot be
                    // reaped and reused between root validation and its signal.
                    child.wait().map(|status| status.exit_code()).unwrap_or(255)
                }
                None => child.wait().map(|status| status.exit_code()).unwrap_or(255),
            };
            if let Some(terminal) = terminal {
                let mut emulator = terminal.emulator.lock();
                *terminal.exit_status.lock() = Some(status);
                terminal.alive.store(false, Ordering::Release);
                emulator.publish_damage();
            }
            on_exit(status);
        });
        let weak = Arc::downgrade(&terminal);
        thread::spawn(move || {
            while let Some(terminal) = weak.upgrade() {
                terminal.emulator.lock().flush_sync();
                let alive = terminal.alive();
                drop(terminal);
                if !alive {
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
        });
        Ok(terminal)
    }
    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }
    pub fn status(&self) -> Value {
        let size = *self.size.lock();
        json!({"pid":self.pid,"alive":self.alive(),"exit_status":*self.exit_status.lock(),"size":{"cols":size.cols,"rows":size.rows}})
    }
    pub fn input(&self, bytes: Vec<u8>) -> Result<()> {
        if !self.alive() || self.closing.load(Ordering::Acquire) {
            bail!("exited: terminal no longer accepts input");
        }
        if bytes.len() > MAX_INPUT {
            bail!("invalid-input: input exceeds 64 KiB");
        }
        self.input
            .try_send(Input { bytes, ack: None })
            .map_err(|error| anyhow::anyhow!("backpressure: input queue unavailable: {error}"))
    }
    pub fn input_confirmed(&self, bytes: Vec<u8>) -> Result<()> {
        if !self.alive() || self.closing.load(Ordering::Acquire) {
            bail!("exited: terminal no longer accepts input");
        }
        let (ack, received) = mpsc::channel();
        self.input
            .try_send(Input {
                bytes,
                ack: Some(ack),
            })
            .map_err(|error| anyhow::anyhow!("backpressure: {error}"))?;
        received
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| anyhow::anyhow!("input-timeout: {error}"))??;
        Ok(())
    }
    pub fn paste(&self, text: &str) -> Result<()> {
        let bracketed = self
            .emulator
            .lock()
            .term
            .mode()
            .contains(TermMode::BRACKETED_PASTE);
        let length = if bracketed {
            text.bytes().filter(|byte| *byte != 0x1b).count() + 12
        } else {
            text.len()
        };
        if length > MAX_INPUT {
            bail!("invalid-input: input exceeds 64 KiB");
        }
        let mut bytes = Vec::with_capacity(length);
        if bracketed {
            bytes.extend_from_slice(b"\x1b[200~");
            bytes.extend(text.bytes().filter(|byte| *byte != 0x1b));
            bytes.extend_from_slice(b"\x1b[201~");
        } else {
            bytes.extend_from_slice(text.as_bytes());
        }
        self.input(bytes)
    }
    pub fn resize(&self, cols: usize, rows: usize) -> Result<()> {
        if !(2..=500).contains(&cols) || !(2..=300).contains(&rows) {
            bail!("invalid-size: expected columns 2..500 and rows 2..300");
        }
        let mut emulator = self.emulator.lock();
        self.master.lock().resize(PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let size = Size { cols, rows };
        *self.size.lock() = size;
        emulator.resize(size);
        Ok(())
    }
    pub fn screen(&self, offset: usize, limit: Option<usize>) -> Value {
        let emulator = self.emulator.lock();
        screen(&emulator.term, offset, limit, emulator.generation)
    }
    pub(crate) fn screen_update(
        &self,
        offset: usize,
        limit: Option<usize>,
        since: Option<u64>,
        row_updates: bool,
    ) -> Value {
        if since.is_none() {
            return self.screen(offset, limit);
        }
        self.emulator
            .lock()
            .screen_update(offset, limit, since, row_updates)
    }
    pub fn close(&self) {
        if self.closing.swap(true, Ordering::AcqRel) {
            return;
        }
        self.processes.lock().terminate();
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        self.close();
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ProcessIdentity {
    pid: i32,
    // Linux: kernel start ticks. Darwin: proc_pidinfo's seconds and microseconds.
    start: (u64, u64),
}

impl ProcessIdentity {
    fn matches_current(self) -> bool {
        process_info(self.pid).is_some_and(|info| info.identity == self)
    }

    fn signal(self, signal: i32) {
        if self.matches_current() {
            unsafe {
                libc::kill(self.pid, signal);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct ProcessInfo {
    identity: ProcessIdentity,
    parent: i32,
    session: i32,
    stopped: bool,
    exited: bool,
}

#[cfg(target_os = "linux")]
fn process_info(pid: i32) -> Option<ProcessInfo> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // comm may itself contain spaces and ')'.
    let (_, fields) = stat.rsplit_once(") ")?;
    let mut fields = fields.split_whitespace();
    let state = fields.next()?;
    let parent = fields.next()?.parse().ok()?;
    fields.next()?; // process group
    let session = fields.next()?.parse().ok()?;
    let start = fields.nth(15)?.parse().ok()?; // /proc stat field 22
    Some(ProcessInfo {
        identity: ProcessIdentity {
            pid,
            start: (start, 0),
        },
        parent,
        session,
        stopped: matches!(state, "T" | "t"),
        exited: state == "Z",
    })
}

#[cfg(target_os = "macos")]
fn process_info(pid: i32) -> Option<ProcessInfo> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    // A nonzero argument includes unreaped zombies for this libproc flavor.
    let result = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            1,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if result != size {
        return None;
    }
    let info = unsafe { info.assume_init() };
    Some(ProcessInfo {
        identity: ProcessIdentity {
            pid,
            start: (info.pbi_start_tvsec, info.pbi_start_tvusec),
        },
        parent: info.pbi_ppid as i32,
        session: unsafe { libc::getsid(pid) },
        stopped: info.pbi_status == libc::SSTOP,
        exited: info.pbi_status == libc::SZOMB,
    })
}

fn process_snapshot() -> BTreeMap<i32, ProcessInfo> {
    #[cfg(target_os = "linux")]
    let pids = std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse::<i32>().ok());
    #[cfg(target_os = "macos")]
    let pids = {
        let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
        let mut pids = vec![0i32; count.max(0) as usize + 32];
        loop {
            let count = unsafe {
                libc::proc_listallpids(
                    pids.as_mut_ptr().cast(),
                    (pids.len() * std::mem::size_of::<i32>()) as i32,
                )
            };
            if count < pids.len() as i32 {
                pids.truncate(count.max(0) as usize);
                break;
            }
            pids.resize(pids.len() * 2, 0);
        }
        pids.into_iter().filter(|&pid| pid > 0)
    };
    pids.filter_map(|pid| process_info(pid).map(|info| (pid, info)))
        .collect()
}

struct ProcessTree {
    root: ProcessIdentity,
    session: i32,
    members: BTreeSet<ProcessIdentity>,
}

impl ProcessTree {
    fn new(root: ProcessInfo) -> Self {
        Self {
            root: root.identity,
            session: root.session,
            members: BTreeSet::from([root.identity]),
        }
    }

    fn discover(&mut self, processes: &BTreeMap<i32, ProcessInfo>) {
        let mut parents: BTreeMap<i32, ProcessIdentity> = self
            .members
            .iter()
            .filter(|identity| {
                processes
                    .get(&identity.pid)
                    .is_some_and(|info| info.identity == **identity)
                    && identity.matches_current()
            })
            .map(|&identity| (identity.pid, identity))
            .collect();
        // A session number alone is not an identity. Trust it only while the
        // original session leader is still present, including its unreaped zombie.
        let original_session =
            parents.contains_key(&self.root.pid) && self.root.matches_current() && self.session > 0;
        loop {
            let before = parents.len();
            for info in processes.values() {
                let identity = info.identity;
                if identity.pid == std::process::id() as i32 || parents.contains_key(&identity.pid)
                {
                    continue;
                }
                if (parents
                    .get(&info.parent)
                    .is_some_and(|parent| parent.matches_current())
                    || (original_session
                        && info.session == self.session
                        && self.root.matches_current()))
                    && identity.matches_current()
                {
                    parents.insert(identity.pid, identity);
                    self.members.insert(identity);
                }
            }
            if parents.len() == before {
                break;
            }
        }
    }

    fn freeze(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let before = self.members.len();
            let mut stopped = true;
            // Freeze the original root before taking the first process snapshot,
            // and each newly discovered producer before scanning its children.
            for &identity in &self.members {
                if let Some(info) =
                    process_info(identity.pid).filter(|info| info.identity == identity)
                {
                    if !info.stopped && !info.exited {
                        identity.signal(libc::SIGSTOP);
                        stopped = false;
                    }
                }
            }
            self.discover(&process_snapshot());
            // STOP is asynchronous: all known producers must be confirmed stopped
            // before a rescan with no new identities establishes stability.
            if stopped && self.members.len() == before {
                return;
            }
            if Instant::now() >= deadline {
                eprintln!(
                    "amux cleanup: a process could not be stopped; continuing best-effort termination"
                );
                return;
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn terminate(&mut self) {
        self.freeze();
        for &identity in &self.members {
            identity.signal(libc::SIGTERM);
        }
        for &identity in &self.members {
            identity.signal(libc::SIGCONT);
        }
        thread::sleep(Duration::from_millis(200));
        // A TERM-ignoring producer can fork again after CONT. Freeze and rescan
        // that grace-period population too before the final, identity-checked kill.
        self.freeze();
        for &identity in &self.members {
            identity.signal(libc::SIGKILL);
        }
    }
}

fn palette(index: usize) -> Option<[u8; 3]> {
    const ANSI: [[u8; 3]; 16] = [
        [0, 0, 0],
        [205, 49, 49],
        [13, 188, 121],
        [229, 229, 16],
        [36, 114, 200],
        [188, 63, 188],
        [17, 168, 205],
        [229, 229, 229],
        [102, 102, 102],
        [241, 76, 76],
        [35, 209, 139],
        [245, 245, 67],
        [59, 142, 234],
        [214, 112, 214],
        [41, 184, 219],
        [255, 255, 255],
    ];
    match index {
        0..=15 => Some(ANSI[index]),
        16..=231 => {
            let i = index - 16;
            let value = |n| if n == 0 { 0 } else { 55 + n as u8 * 40 };
            Some([value(i / 36), value((i / 6) % 6), value(i % 6)])
        }
        232..=255 => Some([8 + (index - 232) as u8 * 10; 3]),
        256 => Some([229, 229, 229]),
        257 => Some([0, 0, 0]),
        _ => None,
    }
}
fn indexed_color(term: &Term<Listener>, index: usize) -> Option<[u8; 3]> {
    term.colors()[index]
        .map(|rgb| [rgb.r, rgb.g, rgb.b])
        .or_else(|| palette(index))
}
fn color(term: &Term<Listener>, value: Color) -> Option<[u8; 3]> {
    let index = match value {
        Color::Spec(Rgb { r, g, b }) => return Some([r, g, b]),
        Color::Indexed(index) => index as usize,
        Color::Named(NamedColor::Foreground) | Color::Named(NamedColor::Background) => {
            let Color::Named(named) = value else {
                unreachable!()
            };
            return term.colors()[named].map(|rgb| [rgb.r, rgb.g, rgb.b]);
        }
        Color::Named(named) => named as usize,
    };
    indexed_color(term, index)
}
fn screen(term: &Term<Listener>, offset: usize, limit: Option<usize>, generation: u64) -> Value {
    let grid = term.grid();
    let offset = offset.min(grid.history_size());
    let rows = limit
        .unwrap_or(grid.screen_lines())
        .min(300)
        .min(grid.screen_lines() + offset);
    screen_rows(term, offset, rows, generation, 0..rows)
}

fn screen_rows(
    term: &Term<Listener>,
    offset: usize,
    rows: usize,
    generation: u64,
    row_indices: impl ExactSizeIterator<Item = usize>,
) -> Value {
    let grid = term.grid();
    let cols = grid.columns();
    let mut lines = Vec::with_capacity(row_indices.len());
    let mut cells = Vec::with_capacity(row_indices.len());
    for row in row_indices {
        let mut line = String::with_capacity(cols);
        let mut row_cells = Vec::with_capacity(cols);
        for col in 0..cols {
            let cell = &grid[Point::new(Line(row as i32 - offset as i32), Column(col))];
            let flags = cell.flags;
            let spacer =
                flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
            let mut text = if flags.contains(Flags::HIDDEN) {
                " ".to_owned()
            } else {
                cell.c.to_string()
            };
            if let Some(zero) = cell.zerowidth() {
                text.extend(zero);
            }
            if !spacer {
                line.push_str(&text);
            }
            row_cells.push(json!({"text":text,"fg":color(term,cell.fg),"bg":color(term,cell.bg),"bold":flags.contains(Flags::BOLD),"italic":flags.contains(Flags::ITALIC),"underline":flags.intersects(Flags::ALL_UNDERLINES),"inverse":flags.contains(Flags::INVERSE),"dim":flags.contains(Flags::DIM),"strikeout":flags.contains(Flags::STRIKEOUT),"wide":flags.contains(Flags::WIDE_CHAR),"wide_spacer":spacer}));
        }
        line.truncate(line.trim_end().len());
        lines.push(line);
        cells.push(row_cells);
    }
    let mode = term.mode();
    let cursor = grid.cursor.point;
    json!({"cols":cols,"rows":rows,"cursor":{"row":cursor.line.0+offset as i32,"col":cursor.column.0,"visible":offset==0 && mode.contains(TermMode::SHOW_CURSOR)},"modes":{"application_cursor":mode.contains(TermMode::APP_CURSOR),"application_keypad":mode.contains(TermMode::APP_KEYPAD),"bracketed_paste":mode.contains(TermMode::BRACKETED_PASTE),"mouse_sgr":mode.contains(TermMode::SGR_MOUSE),"mouse_click":mode.contains(TermMode::MOUSE_REPORT_CLICK),"mouse_drag":mode.contains(TermMode::MOUSE_DRAG),"mouse_motion":mode.contains(TermMode::MOUSE_MOTION),"mouse_utf8":mode.contains(TermMode::UTF8_MOUSE),"focus_reporting":mode.contains(TermMode::FOCUS_IN_OUT),"alt_screen":mode.contains(TermMode::ALT_SCREEN)},"lines":lines,"cells":cells,"history_size":grid.history_size(),"offset":offset,"generation":generation})
}

#[cfg(test)]
mod screen_update_tests {
    use super::*;

    fn emulator(cols: usize, rows: usize) -> Emulator {
        emulator_with_replies(cols, rows).0
    }

    fn emulator_with_replies(cols: usize, rows: usize) -> (Emulator, mpsc::Receiver<Input>) {
        emulator_with_history(cols, rows, Config::default().scrolling_history)
    }

    fn emulator_with_history(
        cols: usize,
        rows: usize,
        history_limit: usize,
    ) -> (Emulator, mpsc::Receiver<Input>) {
        let (input, replies) = mpsc::sync_channel(INPUT_QUEUE);
        let (color_requests, pending_colors) = mpsc::sync_channel(INPUT_QUEUE);
        let size = Arc::new(Mutex::new(Size { cols, rows }));
        let listener = Listener {
            input: input.clone(),
            size: size.clone(),
            color_requests,
        };
        let config = Config {
            scrolling_history: history_limit,
            ..Config::default()
        };
        let term = Term::new(config, &*size.lock(), listener);
        (
            Emulator::new(term, pending_colors, input, history_limit),
            replies,
        )
    }

    #[test]
    fn cpr_tracks_origin_margins_modes_reset_resize_and_alternate_screen() {
        let (mut emulator, replies) = emulator_with_replies(12, 24);
        for (bytes, expected) in [
            ("\x1b[4;4H\x1b[6n", "\x1b[4;4R"),
            ("\x1b[3;20r\x1b[?6h\x1b[2;4H\x1b[6n", "\x1b[2;4R"),
            ("\x1b[5;18r\x1b[2;4H\x1b[6n", "\x1b[2;4R"),
            ("\x1b[18;5r\x1b[6n", "\x1b[2;4R"),
            ("\x1b[?1049h\x1b[2;4H\x1b[6n", "\x1b[2;4R"),
            ("\x1b[?1049l\x1b[6n", "\x1b[2;4R"),
            ("\x1b[?6l\x1b[4;4H\x1b[6n", "\x1b[4;4R"),
            ("\x1b[?6h\x1b[2;4H\x1b[5n", "\x1b[0n"),
        ] {
            emulator.advance(bytes.as_bytes());
            assert_eq!(replies.try_recv().unwrap().bytes, expected.as_bytes());
        }
        // A no-op resize must not discard margins; a real resize does.
        emulator.resize(Size { cols: 12, rows: 24 });
        emulator.advance(b"\x1b[6n");
        assert_eq!(replies.try_recv().unwrap().bytes, b"\x1b[2;4R");
        emulator.resize(Size { cols: 13, rows: 25 });
        emulator.advance(b"\x1b[2;4H\x1b[6n");
        assert_eq!(replies.try_recv().unwrap().bytes, b"\x1b[2;4R");
        emulator.advance(b"\x1b[3;20r\x1b[?3h\x1b[2;4H\x1b[6n");
        assert_eq!(replies.try_recv().unwrap().bytes, b"\x1b[2;4R");
        emulator.advance(b"\x1bc\x1b[6n");
        assert_eq!(replies.try_recv().unwrap().bytes, b"\x1b[1;1R");
    }

    #[test]
    fn grapheme_cells_and_cursor_survive_every_utf8_chunk_boundary() {
        for (text, width) in [
            ("👩‍💻", 2),
            ("👨‍👩‍👧‍👦", 2),
            ("👍🏽", 2),
            ("🇺🇳", 2),
            ("1️⃣", 2),
            ("❤️", 2),
            ("界", 2),
            ("é", 1),
            ("⌚︎", 1),
        ] {
            for split in 0..=text.len() {
                let mut emulator = emulator(12, 3);
                emulator.advance(&text.as_bytes()[..split]);
                emulator.advance(&text.as_bytes()[split..]);
                emulator.advance(b"X");
                let frame = emulator.screen_update(0, None, None, true);
                assert_eq!(frame["cells"][0][0]["text"], text, "{text}, split {split}");
                assert_eq!(frame["cells"][0][0]["wide"], width == 2);
                if width == 2 {
                    assert_eq!(frame["cells"][0][1]["wide_spacer"], true);
                }
                assert_eq!(frame["cells"][0][width]["text"], "X");
                assert_eq!(frame["cursor"]["col"], width + 1);
            }
        }
    }

    #[test]
    fn streamed_clusters_wrap_overwrite_and_invalidate_on_controls() {
        let mut emulator = emulator(5, 4);
        for byte in "abc👩‍💻X".as_bytes() {
            emulator.advance(std::slice::from_ref(byte));
        }
        let frame = emulator.screen_update(0, None, None, true);
        assert_eq!(frame["lines"][0], "abc👩‍💻");
        assert_eq!(frame["cells"][0][3]["text"], "👩‍💻");
        assert_eq!(frame["cells"][0][4]["wide_spacer"], true);
        assert_eq!(frame["cells"][1][0]["text"], "X");
        emulator.advance(b"\x1b[1;5HB");
        let overwritten = emulator.screen_update(0, None, None, true);
        assert_eq!(overwritten["cells"][0][3]["text"], " ");
        assert_eq!(overwritten["cells"][0][3]["wide"], false);
        assert_eq!(overwritten["cells"][0][4]["text"], "B");
        assert_eq!(overwritten["cells"][0][4]["wide_spacer"], false);
        emulator.advance("\x1bc👩\x1b[2;1H\u{200d}💻X".as_bytes());
        let moved = emulator.screen_update(0, None, None, true);
        assert_eq!(moved["cells"][0][0]["text"], "👩");
        assert_eq!(moved["cells"][1][0]["text"], "💻");
        assert_eq!(moved["cells"][1][2]["text"], "X");
        emulator.advance("\x1b[1;1H👩‍💻\r\x1b[2K💻".as_bytes());
        assert_eq!(
            emulator.screen_update(0, None, None, true)["cells"][0][0]["text"],
            "💻"
        );
    }

    #[test]
    fn variation_widening_at_margin_preserves_sgr_and_row_deltas() {
        let mut emulator = emulator(5, 4);
        emulator.advance("abcd❤".as_bytes());
        let mut frame = emulator.screen_update(0, None, None, true);
        emulator.advance("\x1b[32m\u{fe0f}X".as_bytes());
        let update = emulator.screen_update(0, None, Some(generation(&frame)), true);
        assert_eq!(update["delta"], true);
        assert_reconstructs(&emulator, &mut frame, &update, 0, None);
        assert_eq!(frame["lines"][0], "abcd");
        assert_eq!(frame["cells"][0][4]["wide_spacer"], true);
        assert_eq!(frame["cells"][1][0]["text"], "❤️");
        assert_eq!(frame["cells"][1][1]["wide_spacer"], true);
        assert_eq!(frame["cells"][1][2]["text"], "X");
        assert_ne!(frame["cells"][1][0]["fg"], frame["cells"][1][2]["fg"]);
        assert_eq!(frame["cursor"]["row"], 1);
        assert_eq!(frame["cursor"]["col"], 3);
    }

    #[test]
    fn width_changes_restore_inserted_cells_and_provisional_scroll() {
        let mut inserted = emulator(10, 3);
        inserted.advance("abcdef\r\x1b[4h❤".as_bytes());
        inserted.advance("\u{fe0f}X".as_bytes());
        assert_eq!(
            inserted.screen_update(0, None, None, true)["lines"][0],
            "❤️Xabcdef"
        );

        for margin in ["", "\x1b[2;3r"] {
            let mut shrinking = emulator(5, 3);
            shrinking.advance("AAAAA\r\nBBBBB\r\nCCCC".as_bytes());
            shrinking.advance(margin.as_bytes());
            shrinking.advance("\x1b[3;5H⌚".as_bytes());
            shrinking.advance("\u{fe0e}".as_bytes());
            let frame = shrinking.screen_update(0, None, None, true);
            assert_eq!(frame["lines"], json!(["AAAAA", "BBBBB", "CCCC⌚︎"]));
            assert_eq!(frame["history_size"], 0);
            assert_eq!(frame["cursor"]["row"], 2);
            assert_eq!(frame["cursor"]["col"], 4);
            assert_eq!(frame["cells"][2][4]["wide"], false);
        }
    }

    #[test]
    fn variation_shrink_preserves_capped_history_and_alternate_screen() {
        for (history_limit, alternate) in [(1, false), (2, false), (2, true)] {
            let (mut emulator, _) = emulator_with_history(5, 3, history_limit);
            if alternate {
                emulator.advance(b"\x1b[?1049h");
            }
            emulator.advance(b"00000\r\n11111\r\n22222\r\n33333\r\n4444");
            let before = screen(&emulator.term, 2, Some(2), emulator.generation);
            let history = emulator.term.grid().history_size();
            emulator.advance("⌚".as_bytes());
            emulator.advance("\u{fe0e}".as_bytes());
            let after = screen(&emulator.term, 2, Some(2), emulator.generation);
            assert_eq!(after["lines"], before["lines"]);
            assert_eq!(after["cells"], before["cells"]);
            assert_eq!(emulator.term.grid().history_size(), history);
            let visible = emulator.screen_update(0, None, None, true);
            assert_eq!(visible["lines"], json!(["22222", "33333", "4444⌚︎"]));
        }
    }

    #[test]
    fn no_wrap_refuses_widened_cluster_without_stray_cells() {
        let mut emulator = emulator(5, 3);
        emulator.advance("\x1b[?7labcd❤".as_bytes());
        emulator.advance("\u{fe0f}X".as_bytes());
        let frame = emulator.screen_update(0, None, None, true);
        assert_eq!(frame["lines"][0], "abcdX");
        for cell in frame["cells"][0].as_array().unwrap() {
            assert_eq!(cell["wide"], false);
            assert_eq!(cell["wide_spacer"], false);
        }
    }

    #[test]
    fn oversized_graphemes_keep_text_and_start_a_new_scalar_segment() {
        let mut joined = emulator(8, 3);
        let within_limit = format!("👩{}\u{fe0f}\u{200d}💻", "\u{301}".repeat(2041));
        assert_eq!(within_limit.len(), 4096);
        joined.advance(within_limit.as_bytes());
        joined.advance(b"X");
        let frame = joined.screen_update(0, None, None, true);
        assert_eq!(frame["cells"][0][0]["text"], within_limit);
        assert_eq!(frame["cells"][0][1]["wide_spacer"], true);
        assert_eq!(frame["cells"][0][2]["text"], "X");
        assert_eq!(frame["cursor"]["col"], 3);

        let mut overflow = emulator(8, 3);
        let prefix = format!("👩{}\u{fe0f}\u{200d}", "\u{301}".repeat(2043));
        assert_eq!(prefix.len(), 4096);
        overflow.advance(prefix.as_bytes());
        overflow.advance("💻X".as_bytes());
        let frame = overflow.screen_update(0, None, None, true);
        assert_eq!(frame["cells"][0][0]["text"], prefix);
        assert_eq!(frame["cells"][0][1]["wide_spacer"], true);
        assert_eq!(frame["cells"][0][2]["text"], "💻");
        assert_eq!(frame["cells"][0][3]["wide_spacer"], true);
        assert_eq!(frame["cells"][0][4]["text"], "X");
        assert_eq!(frame["cursor"]["col"], 5);
        assert_eq!(frame["lines"][0], format!("{prefix}💻X"));

        // Combining overflow uses the native zero-width path rather than
        // dropping marks or retaining an ever-growing composition buffer.
        let mut combining = emulator(8, 3);
        let text = format!("e{}", "\u{301}".repeat(4096));
        combining.advance(text.as_bytes());
        combining.advance(b"X");
        let frame = combining.screen_update(0, None, None, true);
        assert_eq!(frame["cells"][0][0]["text"], text);
        assert_eq!(frame["cells"][0][1]["text"], "X");
        assert_eq!(frame["cursor"]["col"], 2);
    }

    #[test]
    fn incomplete_terminal_setup_kills_and_reaps_its_child() {
        let child = std::process::Command::new("/bin/sleep")
            .arg("120")
            .spawn()
            .unwrap();
        let pid = child.id() as i32;
        let pending = UninstalledChild(Some(Box::new(child)));
        drop(pending);
        let mut status = 0;
        assert_eq!(
            unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) },
            -1
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }

    fn generation(frame: &Value) -> u64 {
        frame["generation"].as_u64().unwrap()
    }

    fn apply_update(frame: &mut Value, update: &Value) {
        if update["unchanged"] == true {
            assert_eq!(frame["generation"], update["generation"]);
            return;
        }
        if update["delta"] != true {
            assert!(update.get("row_indices").is_none());
            assert!(update.get("base_generation").is_none());
            *frame = update.clone();
            return;
        }
        assert_eq!(update["base_generation"], frame["generation"]);
        let indices = update["row_indices"].as_array().unwrap();
        assert_eq!(indices.len(), update["cells"].as_array().unwrap().len());
        assert_eq!(indices.len(), update["lines"].as_array().unwrap().len());
        for (index, row) in indices.iter().enumerate() {
            let row = row.as_u64().unwrap() as usize;
            frame["cells"][row] = update["cells"][index].clone();
            frame["lines"][row] = update["lines"][index].clone();
        }
        for (key, value) in update.as_object().unwrap() {
            if !matches!(
                key.as_str(),
                "delta" | "base_generation" | "row_indices" | "cells" | "lines"
            ) {
                frame[key] = value.clone();
            }
        }
    }

    fn assert_reconstructs(
        emulator: &Emulator,
        frame: &mut Value,
        update: &Value,
        offset: usize,
        limit: Option<usize>,
    ) {
        apply_update(frame, update);
        assert_eq!(
            *frame,
            screen(&emulator.term, offset, limit, emulator.generation)
        );
    }

    #[test]
    fn skipped_generations_and_independent_clients_reconstruct_native_cells() {
        let mut emulator = emulator(10, 4);
        let mut slow = emulator.screen_update(0, None, None, true);
        let mut fast = slow.clone();
        let slow_generation = generation(&slow);

        emulator.advance("\x1b[1;1Hα\x1b[3;1H界\u{301}".as_bytes());
        let first = emulator.screen_update(0, None, Some(generation(&fast)), true);
        assert_reconstructs(&emulator, &mut fast, &first, 0, None);

        emulator.advance(b"\x1b[2;2H\x1b[31;4mB\x1b[0m");
        let second = emulator.screen_update(0, None, Some(generation(&fast)), true);
        assert_reconstructs(&emulator, &mut fast, &second, 0, None);

        let skipped = emulator.screen_update(0, None, Some(slow_generation), true);
        assert_reconstructs(&emulator, &mut slow, &skipped, 0, None);
        assert_eq!(slow, fast);
        let unchanged = emulator.screen_update(0, None, Some(generation(&fast)), true);
        assert_eq!(
            unchanged,
            json!({"unchanged":true,"generation":generation(&fast)})
        );
    }

    #[test]
    fn metadata_only_delta_preserves_modes_cursor_and_limited_view() {
        let mut emulator = emulator(8, 4);
        emulator.advance(b"\x1b[3;2H");
        let mut frame = emulator.screen_update(0, Some(1), None, true);
        emulator.advance(b"\x1b[?1h\x1b[?2004h\x1b[?1004h\x1b[?25l");
        let update = emulator.screen_update(0, Some(1), Some(generation(&frame)), true);
        assert_eq!(update["cursor"], json!({"row":2,"col":1,"visible":false}));
        assert_eq!(update["modes"]["application_cursor"], true);
        assert_eq!(update["modes"]["bracketed_paste"], true);
        assert_eq!(update["modes"]["focus_reporting"], true);
        assert_reconstructs(&emulator, &mut frame, &update, 0, Some(1));

        let current = generation(&frame);
        assert_eq!(
            emulator.screen_update(0, Some(1), Some(current), false),
            json!({"unchanged":true,"generation":current})
        );
        for since in [None, Some(0), Some(current + 1)] {
            let full = emulator.screen_update(0, Some(1), since, true);
            assert!(full.get("delta").is_none());
            assert_reconstructs(&emulator, &mut frame, &full, 0, Some(1));
        }
        let legacy = emulator.screen_update(0, Some(1), Some(current - 1), false);
        assert!(legacy.get("delta").is_none());
        assert_reconstructs(&emulator, &mut frame, &legacy, 0, Some(1));
        let history = emulator.screen_update(1, Some(1), Some(current), true);
        assert!(history.get("delta").is_none());
        assert_reconstructs(&emulator, &mut frame, &history, 1, Some(1));
    }

    #[test]
    fn palette_alternate_screen_and_resize_force_full_reconstruction() {
        let mut emulator = emulator(8, 4);
        emulator.advance(b"\x1b[31mR\x1b[0m");
        let mut frame = emulator.screen_update(0, None, None, true);
        let original_color = frame["cells"][0][0]["fg"].clone();

        emulator.advance(b"\x1b]4;1;rgb:00/ff/00\x07");
        let palette = emulator.screen_update(0, None, Some(generation(&frame)), true);
        assert!(palette.get("delta").is_none());
        assert_ne!(palette["cells"][0][0]["fg"], original_color);
        assert_reconstructs(&emulator, &mut frame, &palette, 0, None);

        emulator.advance(b"\x1b[?1049hALT");
        let alternate = emulator.screen_update(0, None, Some(generation(&frame)), true);
        assert!(alternate.get("delta").is_none());
        assert_eq!(alternate["modes"]["alt_screen"], true);
        assert_reconstructs(&emulator, &mut frame, &alternate, 0, None);

        emulator.advance(b"\x1b[?1049l");
        let normal = emulator.screen_update(0, None, Some(generation(&frame)), true);
        assert!(normal.get("delta").is_none());
        assert_eq!(normal["modes"]["alt_screen"], false);
        assert_reconstructs(&emulator, &mut frame, &normal, 0, None);

        emulator.resize(Size { cols: 9, rows: 6 });
        let resized = emulator.screen_update(0, None, Some(generation(&frame)), true);
        assert!(resized.get("delta").is_none());
        assert_eq!(resized["cols"], 9);
        assert_eq!(resized["rows"], 6);
        assert_reconstructs(&emulator, &mut frame, &resized, 0, None);

        emulator.advance(b"\x1b[6;1Hz");
        let after_resize = emulator.screen_update(0, None, Some(generation(&frame)), true);
        assert_eq!(after_resize["delta"], true);
        assert_reconstructs(&emulator, &mut frame, &after_resize, 0, None);
    }

    #[test]
    fn synchronized_timeout_flush_publishes_previously_buffered_rows() {
        let mut emulator = emulator(8, 4);
        let mut frame = emulator.screen_update(0, None, None, true);
        emulator.advance(b"\x1b[?2026h\x1b[3;1HSYNC");
        let buffered = emulator.screen_update(0, None, Some(generation(&frame)), true);
        assert_reconstructs(&emulator, &mut frame, &buffered, 0, None);
        assert_eq!(frame["lines"][2], "");
        let before_flush = generation(&frame);

        // Exercise the processor's real timeout instead of ending the update
        // through another PTY batch, which would hide missing flush damage.
        let deadline = emulator.processor.sync_timeout().sync_timeout().unwrap();
        thread::sleep(deadline.saturating_duration_since(Instant::now()));
        emulator.flush_sync();
        let flushed = emulator.screen_update(0, None, Some(before_flush), true);
        assert_eq!(flushed["generation"], before_flush + 1);
        assert_eq!(flushed["delta"], true);
        assert!(
            flushed["row_indices"]
                .as_array()
                .unwrap()
                .contains(&json!(2))
        );
        assert_reconstructs(&emulator, &mut frame, &flushed, 0, None);
        assert_eq!(frame["lines"][2], "SYNC");
    }

    #[test]
    fn scrolling_invalidates_old_bases_and_history_never_returns_delta() {
        let mut emulator = emulator(8, 3);
        let mut frame = emulator.screen_update(0, None, None, true);
        emulator.advance(b"a\r\nb\r\nc\r\nd\r\ne");
        let scrolled = emulator.screen_update(0, None, Some(generation(&frame)), true);
        assert!(scrolled.get("delta").is_none());
        assert!(scrolled["history_size"].as_u64().unwrap() > 0);
        assert_reconstructs(&emulator, &mut frame, &scrolled, 0, None);

        let current = generation(&frame);
        let mut history = emulator.screen_update(1, Some(4), Some(current), true);
        assert!(history.get("delta").is_none());
        assert_eq!(history["offset"], 1);
        assert_eq!(history["rows"], 4);
        assert_reconstructs(
            &emulator,
            &mut history,
            &emulator.screen_update(1, Some(4), Some(current), true),
            1,
            Some(4),
        );

        emulator.advance(b"!");
        let delta = emulator.screen_update(0, None, Some(current), true);
        assert_eq!(delta["delta"], true);
        assert_reconstructs(&emulator, &mut frame, &delta, 0, None);
        let historical = emulator.screen_update(1, Some(4), Some(current), true);
        assert!(historical.get("delta").is_none());
        assert_reconstructs(&emulator, &mut history, &historical, 1, Some(4));
    }
}

#[cfg(test)]
mod process_cleanup_tests {
    use super::{ProcessTree, process_info};
    use std::io::BufRead;
    use std::process::{Child, Command, Stdio};

    struct Helper {
        child: Child,
        descendant: super::ProcessIdentity,
    }

    impl Drop for Helper {
        fn drop(&mut self) {
            self.descendant.signal(libc::SIGKILL);
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    #[test]
    fn stale_root_identity_never_signals_or_expands_reused_pid() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "sleep 120 & echo $!; wait"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let descendant = process_info(line.trim().parse().unwrap()).unwrap().identity;
        let mut unrelated = Helper { child, descendant };
        let original = process_info(unrelated.child.id() as i32).unwrap();
        let mut stale = original;
        stale.identity.start.1 = stale.identity.start.1.wrapping_add(1);
        let mut tree = ProcessTree::new(stale);
        let mut survivor = Command::new("/bin/sleep").arg("120").spawn().unwrap();
        // Previously recorded original descendants must still be cleaned even
        // when a different process now occupies the shell PID.
        tree.members
            .insert(process_info(survivor.id() as i32).unwrap().identity);
        tree.terminate();
        assert!(
            !survivor.wait().unwrap().success(),
            "known original descendant survived"
        );
        assert!(
            unrelated.child.try_wait().unwrap().is_none(),
            "reused root PID was signaled"
        );
        for identity in [original.identity, descendant] {
            let info = process_info(identity.pid).expect("unrelated process was killed");
            assert_eq!(info.identity, identity);
            assert!(
                !info.stopped && !info.exited,
                "stale PID was used as a cleanup anchor"
            );
        }
    }
}

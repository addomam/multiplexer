use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const MOUSE_TRACKING: [u16; 5] = [9, 1001, 1000, 1002, 1003];
const MOUSE_ENCODING: [u16; 4] = [1005, 1006, 1015, 1016];

static STOP: AtomicBool = AtomicBool::new(false);
static RESIZE_EPOCH: AtomicUsize = AtomicUsize::new(0);
extern "C" fn interrupted(signal: libc::c_int) {
    if signal == libc::SIGWINCH {
        RESIZE_EPOCH.fetch_add(1, Ordering::Relaxed);
    } else {
        STOP.store(true, Ordering::Relaxed);
    }
}

/// Owns local input state and only the endpoint modes that this application changes.
/// A single instance must be used per process (signals and stdin are process resources).
pub struct Terminal {
    original: libc::termios,
    signals: Vec<(libc::c_int, libc::sigaction)>,
    modes: BTreeMap<u16, u8>,
    changed: Vec<u16>,
    kitty_pushed: bool,
    cursor_style: Option<u8>,
    modify_other_keys: Option<u8>,
    mok_changed: bool,
    title_pushed: bool,
    screen: bool,
    graphics_used: bool,
    margins: Option<(u16, u16)>,
}
impl Terminal {
    pub fn new() -> Result<Self> {
        // All endpoint bytes remain ordinary stdin/stdout; ioctl affects the local TTY only.
        if unsafe { libc::isatty(0) != 1 || libc::isatty(1) != 1 } {
            bail!(
                "interactive/probe mode requires terminal stdin and stdout; use --headless for pipes"
            );
        }
        let mut original = unsafe { std::mem::zeroed::<libc::termios>() };
        if unsafe { libc::tcgetattr(0, &mut original) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
        let mut guard = Self {
            original,
            signals: Vec::new(),
            modes: BTreeMap::new(),
            changed: Vec::new(),
            kitty_pushed: false,
            cursor_style: None,
            modify_other_keys: None,
            mok_changed: false,
            title_pushed: false,
            screen: false,
            graphics_used: false,
            margins: None,
        };
        STOP.store(false, Ordering::Relaxed);
        for signal in [
            libc::SIGINT,
            libc::SIGTERM,
            libc::SIGHUP,
            libc::SIGQUIT,
            libc::SIGPIPE,
            libc::SIGTSTP,
            libc::SIGWINCH,
        ] {
            let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
            action.sa_sigaction = interrupted as *const () as usize;
            unsafe {
                libc::sigemptyset(&mut action.sa_mask);
            }
            let mut old = unsafe { std::mem::zeroed::<libc::sigaction>() };
            if unsafe { libc::sigaction(signal, &action, &mut old) } != 0 {
                return Err(io::Error::last_os_error().into());
            }
            guard.signals.push((signal, old));
        }
        let mut raw = original;
        unsafe {
            libc::cfmakeraw(&mut raw);
        }
        raw.c_cc[libc::VMIN] = 0;
        raw.c_cc[libc::VTIME] = 0;
        if unsafe { libc::tcsetattr(0, libc::TCSANOW, &raw) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(guard)
    }
    pub fn stopped() -> bool {
        STOP.load(Ordering::Relaxed)
    }
    /// Retains resize notifications even when the viewport returns to its old size.
    pub fn resize_epoch() -> usize {
        RESIZE_EPOCH.load(Ordering::Relaxed)
    }
    pub fn size() -> (u16, u16) {
        let mut size = unsafe { std::mem::zeroed::<libc::winsize>() };
        if unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut size) } == 0
            && size.ws_col > 0
            && size.ws_row > 0
        {
            (size.ws_col, size.ws_row)
        } else {
            (80, 24)
        }
    }
    pub fn preserve(
        &mut self,
        modes: &BTreeMap<u16, u8>,
        cursor_style: Option<u8>,
        mok: Option<u8>,
    ) {
        self.modes.clone_from(modes);
        self.cursor_style = cursor_style;
        self.modify_other_keys = mok;
    }
    pub fn preserve_margins(&mut self, margins: Option<(u16, u16)>) {
        self.margins = margins;
    }
    fn mode(&mut self, mode: u16, enabled: bool, output: &mut Vec<u8>) {
        let group = if MOUSE_TRACKING.contains(&mode) {
            &MOUSE_TRACKING[..]
        } else if MOUSE_ENCODING.contains(&mode) {
            &MOUSE_ENCODING[..]
        } else {
            std::slice::from_ref(&mode)
        };
        // Selecting a protocol can implicitly displace every other member.
        for member in group {
            if !self.changed.contains(member) {
                self.changed.push(*member);
            }
        }
        output.extend_from_slice(
            format!("\x1b[?{mode}{}", if enabled { 'h' } else { 'l' }).as_bytes(),
        );
    }
    fn mouse_modes(&mut self, pixels: bool, output: &mut Vec<u8>) -> bool {
        // Clear inherited legacy tracking even when discovery found no safe format.
        for mode in MOUSE_TRACKING {
            self.mode(mode, false, output);
        }
        for mode in MOUSE_ENCODING {
            self.mode(mode, false, output);
        }
        let sgr = matches!(self.modes.get(&1006), Some(1..=3));
        let pixels = pixels && sgr && matches!(self.modes.get(&1016), Some(1..=3));
        if sgr {
            // Request tracking only in the SGR formats; legacy framing is defensive.
            self.mode(if pixels { 1016 } else { 1006 }, true, output);
            if let Some(mode) = [1003, 1002, 1000]
                .into_iter()
                .find(|mode| matches!(self.modes.get(mode), Some(1 | 2)))
            {
                self.mode(mode, true, output);
            }
        }
        pixels
    }
    pub fn enter_screen(&mut self) -> Result<()> {
        let mut bytes = Vec::with_capacity(256);
        if self.modes.get(&1049) != Some(&1) {
            self.mode(1049, true, &mut bytes);
        }
        self.mode(6, false, &mut bytes);
        self.mode(7, true, &mut bytes);
        self.mode(25, false, &mut bytes);
        bytes.extend_from_slice(b"\x1b[?2026l");
        bytes.extend_from_slice(b"\x1b[0m\x1b[2J\x1b[H");
        self.screen = true;
        Self::write(&bytes)
    }
    pub fn enable_input(&mut self, kitty: bool) -> Result<()> {
        let mut bytes = Vec::with_capacity(256);
        self.mouse_modes(false, &mut bytes);
        for mode in [1004, 2004] {
            self.mode(mode, true, &mut bytes);
        }
        if kitty {
            // Push/pop avoids overwriting the surrounding application's keyboard flags.
            bytes.extend_from_slice(b"\x1b[>31u");
            self.kitty_pushed = true;
        } else if self.modify_other_keys.is_some() {
            bytes.extend_from_slice(b"\x1b[>4;2m");
            self.mok_changed = true;
        }
        Self::write(&bytes)
    }
    /// Select only a discovered, decodable format and report the resulting selection.
    pub fn pixel_mouse(&mut self, enabled: bool) -> Result<bool> {
        let mut bytes = Vec::with_capacity(256);
        let pixels = self.mouse_modes(enabled, &mut bytes);
        Self::write(&bytes)?;
        Ok(pixels)
    }
    pub fn consent_title(&mut self) -> Result<()> {
        if !self.title_pushed {
            Self::write(b"\x1b[22;0t")?;
            self.title_pushed = true;
        }
        Ok(())
    }
    pub fn graphics_used(&mut self) {
        self.graphics_used = true;
    }
    pub fn read(buffer: &mut [u8], timeout_ms: u16) -> Result<usize> {
        let mut pollfd = libc::pollfd {
            fd: 0,
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut pollfd, 1, i32::from(timeout_ms)) };
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                return Ok(0);
            }
            return Err(error.into());
        }
        if ready == 0 {
            return Ok(0);
        }
        if pollfd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
            STOP.store(true, Ordering::Relaxed);
            return Ok(0);
        }
        let count = unsafe { libc::read(0, buffer.as_mut_ptr().cast(), buffer.len()) };
        if count < 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(count as usize)
    }
    pub fn write(bytes: &[u8]) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let mut stdout = io::stdout().lock();
        stdout.write_all(bytes).context("writing terminal output")?;
        stdout.flush().context("flushing terminal output")
    }
    pub fn restoration(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(256);
        if self.screen {
            bytes.extend_from_slice(b"\x1b[?2026l\x1b[0m\x1b[r\x1b]8;;\x1b\\");
        }
        if self.kitty_pushed {
            bytes.extend_from_slice(b"\x1b[<u");
        }
        if self.graphics_used {
            bytes.extend_from_slice(
                b"\x1b_Ga=d,d=I,i=77332,q=2;\x1b\\\x1b_Ga=d,d=I,i=77333,q=2;\x1b\\",
            );
        }
        if self.mok_changed {
            bytes.extend_from_slice(
                format!("\x1b[>4;{}m", self.modify_other_keys.unwrap_or(0)).as_bytes(),
            );
        }
        if self.screen {
            if let Some(style) = self.cursor_style {
                bytes.extend_from_slice(format!("\x1b[{style} q").as_bytes());
            }
        }
        if self.title_pushed {
            bytes.extend_from_slice(b"\x1b[23;0t");
        }
        for mode in self.changed.iter().rev() {
            if MOUSE_TRACKING.contains(mode) || MOUSE_ENCODING.contains(mode) {
                continue;
            }
            let enabled = match self.modes.get(mode) {
                Some(1 | 3) => true,
                Some(2 | 4) => false,
                _ => matches!(mode, 7 | 25),
            };
            bytes.extend_from_slice(
                format!("\x1b[?{mode}{}", if enabled { 'h' } else { 'l' }).as_bytes(),
            );
        }
        // Shared-protocol endpoints reset the entire group on any member's reset.
        // Restore encoding before tracking, clearing all displaced selections first.
        for group in [&MOUSE_ENCODING[..], &MOUSE_TRACKING[..]] {
            if group.iter().any(|mode| self.changed.contains(mode)) {
                for mode in group {
                    bytes.extend_from_slice(format!("\x1b[?{mode}l").as_bytes());
                }
                for mode in group {
                    if matches!(self.modes.get(mode), Some(1 | 3)) {
                        bytes.extend_from_slice(format!("\x1b[?{mode}h").as_bytes());
                    }
                }
            }
        }
        if self.screen {
            if let Some((top, bottom)) = self.margins {
                let rows = Self::size().1;
                let bottom = bottom.min(rows);
                if top < bottom {
                    bytes.extend_from_slice(format!("\x1b7\x1b[{top};{bottom}r\x1b8").as_bytes());
                }
            }
        }
        // Synchronized output can predate this program. Restore it after the other output.
        if self.screen && self.modes.get(&2026) == Some(&1) {
            bytes.extend_from_slice(b"\x1b[?2026h");
        }
        bytes
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = Self::write(&self.restoration());
        unsafe {
            libc::tcsetattr(0, libc::TCSANOW, &self.original);
        }
        for (signal, action) in self.signals.iter().rev() {
            unsafe {
                libc::sigaction(*signal, action, std::ptr::null_mut());
            }
        }
    }
}

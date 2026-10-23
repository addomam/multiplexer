use amux::runtime::Terminal;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Fixture {
    terminal: Arc<Terminal>,
    directory: std::path::PathBuf,
}
impl std::ops::Deref for Fixture {
    type Target = Terminal;
    fn deref(&self) -> &Terminal {
        &self.terminal
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.terminal.close();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn terminal() -> Fixture {
    terminal_with_env(std::env::vars().collect())
}
fn terminal_with_env(mut env: BTreeMap<String, String>) -> Fixture {
    let dir = std::env::temp_dir().join(format!("amux-runtime-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join(".zshrc"), "PS1='AMUX_PROMPT> '\n").unwrap();
    env.insert("ZDOTDIR".into(), dir.display().to_string());
    env.insert("PS1".into(), "AMUX_PROMPT> ".into());
    let fixture = Fixture {
        terminal: Terminal::spawn("test".into(), &dir, &env, Arc::new(|_| {})).unwrap(),
        directory: dir,
    };
    wait(&fixture, "AMUX_PROMPT>");
    fixture
}
fn wait(terminal: &Terminal, text: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let screen = terminal.screen(0, None);
        if screen["lines"]
            .as_array()
            .unwrap()
            .iter()
            .any(|line| line.as_str().unwrap().contains(text))
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "missing {text}: {}",
            screen["lines"]
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn native_pty_unicode_alt_screen_modes_scrollback_resize_and_reply() {
    let term = terminal();
    term.input(b"printf '\\033[31mRED\\033[0m e\\u0301 \\u754c\\n'\r".to_vec())
        .unwrap();
    wait(&term, "RED é 界");
    let screen = term.screen(0, None);
    let cells = screen["cells"].as_array().unwrap();
    assert!(
        cells
            .iter()
            .flat_map(|row| row.as_array().unwrap())
            .any(|cell| cell["text"] == "é")
    );
    assert!(
        cells
            .iter()
            .flat_map(|row| row.as_array().unwrap())
            .any(|cell| cell["wide_spacer"] == true)
    );
    term.input(
        b"printf '\\033[?1049h\\033[?1h\\033[?1003h\\033[?1006h\\033[?2004hALT_SURFACE'\r".to_vec(),
    )
    .unwrap();
    wait(&term, "ALT_SURFACE");
    let alt = term.screen(0, None);
    assert_eq!(alt["modes"]["alt_screen"], true);
    assert_eq!(alt["modes"]["application_cursor"], true);
    assert_eq!(alt["modes"]["mouse_sgr"], true);
    assert_eq!(alt["modes"]["bracketed_paste"], true);
    term.input(b"printf '\\033[?1049l\\033[?1l\\033[?1003l\\033[?1006l'; for i in {1..300}; do print LINE_$i; done\r".to_vec()).unwrap();
    wait(&term, "LINE_300");
    assert_eq!(term.screen(0, None)["modes"]["alt_screen"], false);
    assert!(term.screen(0, None)["history_size"].as_u64().unwrap() > 200);
    assert!(
        term.screen(200, None)["lines"]
            .to_string()
            .contains("LINE_")
    );
    for width in [40, 100, 60, 80] {
        term.resize(width, 30).unwrap();
    }
    assert_eq!(term.screen(0, None)["cols"], 80);
    assert_eq!(term.screen(0, None)["rows"], 30);
}

#[test]
fn native_pty_origin_relative_reports_and_joined_grapheme_cells() {
    let term = terminal();
    let executable = std::env::current_exe().unwrap();
    let executable = executable.to_str().unwrap().replace('\'', "'\\''");
    term.input(
        format!(
            "AMUX_CPR_QUERY_APP=1 '{executable}' --exact raw_cpr_query_foreground_app --ignored --nocapture\r"
        )
        .into_bytes(),
    )
    .unwrap();
    wait(&term, "CPR_AND_GRAPHEME_REPLIES_OK");
    let frame = term.screen(0, None);
    assert_eq!(frame["cells"][6][0]["text"], "👩‍💻");
    assert_eq!(frame["cells"][6][0]["wide"], true);
    assert_eq!(frame["cells"][6][1]["wide_spacer"], true);
    assert_eq!(frame["cells"][6][2]["text"], "X");
    assert_eq!(frame["cells"][6][3]["text"], " ");
    term.input(b"finish\n".to_vec()).unwrap();
    wait(&term, "RAW_CPR_INPUT_OK");
}

#[test]
#[ignore = "raw foreground subprocess fixture"]
fn raw_cpr_query_foreground_app() {
    use std::io::{Read, Write};

    assert_eq!(std::env::var("AMUX_CPR_QUERY_APP").as_deref(), Ok("1"));
    struct RestoreMode(libc::termios);
    impl Drop for RestoreMode {
        fn drop(&mut self) {
            unsafe {
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.0);
            }
        }
    }
    let mut original = std::mem::MaybeUninit::<libc::termios>::uninit();
    assert_eq!(
        unsafe { libc::tcgetattr(libc::STDIN_FILENO, original.as_mut_ptr()) },
        0
    );
    let original = unsafe { original.assume_init() };
    let _restore = RestoreMode(original);
    let mut raw = original;
    unsafe { libc::cfmakeraw(&mut raw) };
    assert_eq!(
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) },
        0
    );
    fn read_bounded(expected: &[u8]) {
        let mut actual = vec![0; expected.len()];
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut input = std::io::stdin().lock();
        let mut offset = 0;
        while offset < actual.len() {
            assert!(Instant::now() < deadline, "raw CPR reply timed out");
            let mut poll = libc::pollfd {
                fd: libc::STDIN_FILENO,
                events: libc::POLLIN,
                revents: 0,
            };
            let ready = unsafe { libc::poll(&mut poll, 1, 100) };
            if ready < 0
                && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
            {
                continue;
            }
            assert!(ready >= 0, "raw CPR poll failed");
            if ready == 0 {
                continue;
            }
            let count = input.read(&mut actual[offset..]).unwrap();
            assert!(count > 0, "raw CPR input ended");
            offset += count;
        }
        assert_eq!(actual, expected, "independent raw-wire CPR mismatch");
    }
    let mut output = std::io::stdout().lock();
    for (sequence, expected) in [
        ("\x1bc\x1b[3;20r\x1b[?6h\x1b[2;4H\x1b[6n", "\x1b[2;4R"),
        ("\x1b[?6l\x1b[4;4H\x1b[6n", "\x1b[4;4R"),
        ("\x1b[?6h\x1b[?1049h\x1b[2;4H\x1b[6n", "\x1b[2;4R"),
        ("\x1b[?1049l\x1b[2;4H\x1b[6n", "\x1b[2;4R"),
        ("\x1bc\x1b[6n", "\x1b[1;1R"),
    ] {
        output.write_all(sequence.as_bytes()).unwrap();
        output.flush().unwrap();
        read_bounded(expected.as_bytes());
    }
    output.write_all(b"\x1b[7;1H").unwrap();
    // Deliberately split scalars and the joined cluster over PTY writes.
    for byte in "👩‍💻X".as_bytes() {
        output.write_all(std::slice::from_ref(byte)).unwrap();
        output.flush().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    output.write_all(b"\x1b[6n").unwrap();
    output.flush().unwrap();
    read_bounded(b"\x1b[7;4R");
    output
        .write_all(b"\x1b[10;1HCPR_AND_GRAPHEME_REPLIES_OK")
        .unwrap();
    output.flush().unwrap();
    read_bounded(b"finish\n");
    output.write_all(b"\r\nRAW_CPR_INPUT_OK\r\n").unwrap();
    output.flush().unwrap();
}

#[test]
fn native_pty_application_updated_palette_queries_match_displayed_colors() {
    let term = terminal();
    let executable = std::env::current_exe().unwrap();
    let executable = executable.to_str().unwrap().replace('\'', "'\\''");
    term.input(
        format!(
            "AMUX_COLOR_QUERY_APP=1 '{executable}' --exact raw_color_query_foreground_app --ignored --nocapture\r"
        )
        .into_bytes(),
    )
    .unwrap();
    wait(&term, "PALETTE_REPLIES_OK");

    let screen = term.screen(0, None);
    let row = screen["lines"]
        .as_array()
        .unwrap()
        .iter()
        .position(|line| line.as_str().unwrap().starts_with("I D"))
        .unwrap();
    assert_eq!(
        screen["cells"][row][0]["fg"],
        serde_json::json!([18, 52, 86])
    );
    assert_eq!(
        screen["cells"][row][0]["bg"],
        serde_json::json!([171, 205, 239])
    );
    assert_eq!(
        screen["cells"][row][2]["fg"],
        serde_json::json!([101, 67, 33])
    );
    assert_eq!(
        screen["cells"][row][2]["bg"],
        serde_json::json!([171, 205, 239])
    );

    // Replies must not consume, intercept, or gate ordinary foreground input.
    term.input(b"foreground-input\n".to_vec()).unwrap();
    wait(&term, "FOREGROUND_INPUT_OK");
}

#[test]
#[ignore = "raw foreground subprocess fixture"]
fn raw_color_query_foreground_app() {
    use std::io::{Read, Write};

    assert_eq!(std::env::var("AMUX_COLOR_QUERY_APP").as_deref(), Ok("1"));
    struct RestoreMode(libc::termios);
    impl Drop for RestoreMode {
        fn drop(&mut self) {
            unsafe {
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.0);
            }
        }
    }
    let mut original = std::mem::MaybeUninit::<libc::termios>::uninit();
    assert_eq!(
        unsafe { libc::tcgetattr(libc::STDIN_FILENO, original.as_mut_ptr()) },
        0
    );
    let original = unsafe { original.assume_init() };
    let _restore = RestoreMode(original);
    let mut raw = original;
    unsafe {
        libc::cfmakeraw(&mut raw);
    }
    assert_eq!(
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) },
        0
    );

    fn read_exact_bounded(bytes: &mut [u8]) {
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut input = std::io::stdin().lock();
        let mut offset = 0;
        while offset < bytes.len() {
            let mut poll = libc::pollfd {
                fd: libc::STDIN_FILENO,
                events: libc::POLLIN,
                revents: 0,
            };
            assert!(Instant::now() < deadline, "foreground input timed out");
            let result = unsafe { libc::poll(&mut poll, 1, 100) };
            if result < 0
                && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
            {
                continue;
            }
            assert!(result >= 0, "foreground input poll failed");
            if result == 0 {
                continue;
            }
            let count = input.read(&mut bytes[offset..]).unwrap();
            assert!(count > 0, "foreground input ended");
            offset += count;
        }
    }

    let mut output = std::io::stdout().lock();
    output
        .write_all(
            b"\x1b[2J\x1b[H\
              \x1b]4;42;rgb:12/34/56\x1b\\\
              \x1b]10;rgb:65/43/21\x1b\\\
              \x1b]11;rgb:ab/cd/ef\x1b\\\
              \x1b[38;5;42mI\x1b[39m D\r\n\
              \x1b]4;42;?\x1b\\\
              \x1b]10;?\x1b\\\
              \x1b]11;?\x07\
              \x1b]4;1;?\x1b\\",
        )
        .unwrap();
    output.flush().unwrap();
    let expected = b"\x1b]4;42;rgb:1212/3434/5656\x1b\\\
                     \x1b]10;rgb:6565/4343/2121\x1b\\\
                     \x1b]11;rgb:abab/cdcd/efef\x07\
                     \x1b]4;1;rgb:cdcd/3131/3131\x1b\\";
    let mut replies = vec![0; expected.len()];
    read_exact_bounded(&mut replies);
    assert_eq!(
        replies.as_slice(),
        expected.as_slice(),
        "color replies disagree with app palette"
    );
    output.write_all(b"PALETTE_REPLIES_OK\r\n").unwrap();
    output.flush().unwrap();

    let mut input = [0; b"foreground-input\n".len()];
    read_exact_bounded(&mut input);
    assert_eq!(&input, b"foreground-input\n");
    output.write_all(b"FOREGROUND_INPUT_OK\r\n").unwrap();
    output.flush().unwrap();
}

#[test]
fn close_terminates_foreground_job_and_retains_exit_status() {
    let term = terminal();
    term.input(b"sleep 120 & print CHILD_PID=$!; wait\r".to_vec())
        .unwrap();
    wait(&term, "CHILD_PID=");
    let deadline = Instant::now() + Duration::from_secs(5);
    let pid = loop {
        let lines = term.screen(0, None)["lines"].as_array().unwrap().clone();
        if let Some(pid) = lines
            .iter()
            .filter_map(|line| {
                line.as_str()?
                    .strip_prefix("CHILD_PID=")?
                    .trim()
                    .parse::<i32>()
                    .ok()
            })
            .next()
        {
            break pid;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    };
    term.close();
    while term.alive() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(term.status()["exit_status"].is_number());
    assert!(term.input(b"x".to_vec()).is_err());
    wait_dead(pid);
}

#[test]
fn close_terminates_term_ignoring_separate_session_descendant() {
    if let Some(path) = std::env::var_os("AMUX_TEST_DETACHED_PID_FILE") {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(path.as_bytes()).unwrap();
        unsafe {
            // The helper's child is not Zsh's foreground process-group leader.
            // Only async-signal-safe operations run in the forked test process.
            let child = libc::fork();
            assert!(child >= 0);
            if child > 0 {
                libc::waitpid(child, std::ptr::null_mut(), 0);
                libc::_exit(0);
            }
            if libc::setsid() < 0 {
                libc::_exit(2);
            }
            libc::signal(libc::SIGTERM, libc::SIG_IGN);
            let mut digits = [0u8; 12];
            let mut start = digits.len();
            let mut pid = libc::getpid();
            while pid > 0 {
                start -= 1;
                digits[start] = b'0' + (pid % 10) as u8;
                pid /= 10;
            }
            let fd = libc::open(path.as_ptr(), libc::O_CREAT | libc::O_WRONLY, 0o600);
            if fd < 0 {
                libc::_exit(3);
            }
            libc::write(fd, digits[start..].as_ptr().cast(), digits.len() - start);
            libc::close(fd);
            loop {
                libc::pause();
            }
        }
    }
    let term = terminal();
    let path = term.directory.join("detached.pid");
    let executable = std::env::current_exe().unwrap();
    term.input(format!("AMUX_TEST_DETACHED_PID_FILE='{}' '{}' --exact close_terminates_term_ignoring_separate_session_descendant --nocapture\r",
        path.display().to_string().replace('\'',"'\\''"),executable.display().to_string().replace('\'',"'\\''")).into_bytes()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "helper did not start: {}",
            term.screen(0, None)["lines"]
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let pid = std::fs::read_to_string(path)
        .unwrap()
        .parse::<i32>()
        .unwrap();
    assert_eq!(unsafe { libc::getsid(pid) }, pid);
    term.close();
    wait_dead(pid);
}

fn wait_file(path: &std::path::Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(text) = std::fs::read_to_string(path)
            && !text.is_empty()
            && text.ends_with('\n')
        {
            return text;
        }
        assert!(Instant::now() < deadline, "missing {}", path.display());
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn running(pid: i32) -> bool {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|stat| {
                stat.rsplit_once(") ")
                    .map(|(_, fields)| !fields.starts_with('Z'))
            })
            .unwrap_or(false)
    }
    #[cfg(target_os = "macos")]
    {
        let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
        unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                1,
                info.as_mut_ptr().cast(),
                size,
            ) == size
                && info.assume_init().pbi_status != libc::SZOMB
        }
    }
}

fn wait_dead(pid: i32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while running(pid) {
        assert!(Instant::now() < deadline, "process {pid} survived cleanup");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn retained_natural_exit_cleanup_is_idempotent_and_keeps_original_status() {
    let term = terminal();
    let path = term.directory.join("retained.pid");
    term.input(
        format!(
            "trap '' HUP; sleep 120 & print -r -- $! > '{}'; disown; exit 23\r",
            path.display()
        )
        .into_bytes(),
    )
    .unwrap();
    let pid = wait_file(&path).trim().parse::<i32>().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while term.alive() {
        assert!(Instant::now() < deadline, "shell did not naturally exit");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(term.status()["exit_status"], 23);
    assert!(
        running(pid),
        "natural exit unexpectedly removed retained descendant"
    );
    term.close();
    term.close();
    wait_dead(pid);
    assert_eq!(term.status()["exit_status"], 23);
}

#[test]
fn captured_removed_exports_do_not_leak_from_daemon_environment() {
    if std::env::var_os("AMUX_TEST_ENV_CAPTURE").is_some() {
        assert_eq!(
            std::env::var("AMUX_RUNTIME_REMOVED_EXPORT").unwrap(),
            "daemon-value"
        );
        let mut env: BTreeMap<String, String> = std::env::vars().collect();
        env.remove("AMUX_RUNTIME_REMOVED_EXPORT");
        env.remove("PATH");
        env.insert("AMUX_RUNTIME_CAPTURED_EXPORT".into(), "source-value".into());
        env.insert("TERM".into(), "captured-term".into());
        env.insert("COLORTERM".into(), "captured-colorterm".into());
        let term = terminal_with_env(env);
        let path = term.directory.join("environment");
        term.input(format!(
            "print -r -- \"$AMUX_RUNTIME_CAPTURED_EXPORT:${{+AMUX_RUNTIME_REMOVED_EXPORT}}:$TERM:$COLORTERM\" > '{}'\r",
            path.display(),
        ).into_bytes()).unwrap();
        assert_eq!(
            wait_file(&path),
            "source-value:0:xterm-256color:truecolor\n"
        );
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "captured_removed_exports_do_not_leak_from_daemon_environment",
            "--nocapture",
        ])
        .env("AMUX_TEST_ENV_CAPTURE", "1")
        .env("AMUX_RUNTIME_REMOVED_EXPORT", "daemon-value")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// Called only after fork: stack buffers and async-signal-safe libc operations.
unsafe fn write_pid(fd: i32, mut pid: i32) {
    let mut digits = [0u8; 12];
    let mut start = digits.len() - 1;
    digits[start] = b'\n';
    while pid > 0 {
        start -= 1;
        digits[start] = b'0' + (pid % 10) as u8;
        pid /= 10;
    }
    unsafe {
        libc::write(fd, digits[start..].as_ptr().cast(), digits.len() - start);
    }
}

#[test]
fn close_freezes_and_rescans_concurrent_fork_producers() {
    if let Some(path) = std::env::var_os("AMUX_TEST_FORK_PID_FILE") {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(path.as_bytes()).unwrap();
        unsafe {
            let child = libc::fork();
            if child < 0 {
                libc::_exit(2);
            }
            if child > 0 {
                libc::waitpid(child, std::ptr::null_mut(), 0);
                libc::_exit(0);
            }
            if libc::setsid() < 0 {
                libc::_exit(3);
            }
            libc::signal(libc::SIGTERM, libc::SIG_IGN);
            libc::signal(libc::SIGHUP, libc::SIG_IGN);
            let fd = libc::open(
                path.as_ptr(),
                libc::O_CREAT | libc::O_WRONLY | libc::O_APPEND | libc::O_TRUNC,
                0o600,
            );
            if fd < 0 {
                libc::_exit(4);
            }
            write_pid(fd, libc::getpid());
            for _ in 0..64 {
                let descendant = libc::fork();
                if descendant < 0 {
                    libc::_exit(5);
                }
                if descendant == 0 {
                    if libc::setsid() < 0 {
                        libc::_exit(6);
                    }
                    write_pid(fd, libc::getpid());
                    libc::close(fd);
                    loop {
                        libc::pause();
                    }
                }
                write_pid(fd, descendant);
                let delay = libc::timespec {
                    tv_sec: 0,
                    tv_nsec: 5_000_000,
                };
                libc::nanosleep(&delay, std::ptr::null_mut());
            }
            libc::close(fd);
            loop {
                libc::pause();
            }
        }
    }
    let term = terminal();
    let path = term.directory.join("forks.pid");
    let executable = std::env::current_exe().unwrap();
    term.input(format!(
        "AMUX_TEST_FORK_PID_FILE='{}' '{}' --exact close_freezes_and_rescans_concurrent_fork_producers --nocapture\r",
        path.display().to_string().replace('\'', "'\\''"),
        executable.display().to_string().replace('\'', "'\\''"),
    ).into_bytes()).unwrap();
    let first = wait_file(&path);
    let producer = first.lines().next().unwrap().parse::<i32>().unwrap();
    assert_eq!(unsafe { libc::getsid(producer) }, producer);
    term.close();
    let pids: std::collections::BTreeSet<i32> = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| line.parse().unwrap())
        .collect();
    assert!(pids.len() > 1, "helper did not fork any descendants");
    for pid in pids {
        wait_dead(pid);
    }
}

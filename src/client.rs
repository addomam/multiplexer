use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const MAX_FRAME: u64 = 64 * 1024 * 1024;

/// A daemon rejection, retaining the machine-readable wire code.
#[derive(Debug)]
pub struct DaemonError {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for DaemonError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for DaemonError {}

/// A stream can no longer be used safely, unlike an explicit daemon rejection.
#[derive(Debug)]
pub(crate) struct ConnectionError(anyhow::Error);

impl std::fmt::Display for ConnectionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, formatter)
    }
}

impl std::error::Error for ConnectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}

pub(crate) fn connection_lost(message: &'static str) -> anyhow::Error {
    ConnectionError(anyhow::anyhow!(message)).into()
}

pub(crate) fn is_connection_error(error: &anyhow::Error) -> bool {
    error.downcast_ref::<ConnectionError>().is_some()
}

pub(crate) fn stream_connected(stream: &UnixStream) -> bool {
    let mut state = libc::pollfd {
        fd: stream.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    if unsafe { libc::poll(&mut state, 1, 0) } < 0 {
        return std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted;
    }
    if state.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
        return false;
    }
    let mut byte = 0u8;
    let count = unsafe {
        libc::recv(
            stream.as_raw_fd(),
            (&mut byte as *mut u8).cast(),
            1,
            libc::MSG_PEEK | libc::MSG_DONTWAIT,
        )
    };
    count > 0
        || count < 0
            && matches!(
                std::io::Error::last_os_error().kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
            )
}

pub(crate) struct Connection {
    reader: BufReader<UnixStream>,
    writer: BufWriter<UnixStream>,
    response: String,
}

impl Drop for Connection {
    fn drop(&mut self) {
        // Shutdown handles may outlive this owning connection. In particular,
        // a stopped native worker must invalidate its socket even while the
        // observation loop is blocked and has not dropped its handle yet.
        let _ = self.reader.get_ref().shutdown(std::net::Shutdown::Both);
    }
}

impl Connection {
    pub(crate) fn connect(socket: &Path) -> Result<Self> {
        let stream = UnixStream::connect(socket)
            .map_err(|error| ConnectionError(error.into()))
            .with_context(|| format!("cannot connect to daemon at {}", socket.display()))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        Ok(Self {
            reader: BufReader::new(stream.try_clone()?),
            writer: BufWriter::with_capacity(16384, stream),
            response: String::new(),
        })
    }

    pub(crate) fn shutdown_handle(&self) -> Result<UnixStream> {
        Ok(self.reader.get_ref().try_clone()?)
    }

    pub(crate) fn connected(&self) -> bool {
        stream_connected(self.reader.get_ref())
    }

    pub(crate) fn request(
        &mut self,
        request: &impl serde::Serialize,
        timeout: Duration,
    ) -> Result<Value> {
        self.exchange(request, Some(timeout))
    }

    pub(crate) fn request_until_answer_or_dead(
        &mut self,
        request: &impl serde::Serialize,
    ) -> Result<Value> {
        self.exchange(request, None)
    }

    fn exchange(
        &mut self,
        request: &impl serde::Serialize,
        timeout: Option<Duration>,
    ) -> Result<Value> {
        self.request_inner(request, timeout).map_err(|error| {
            if error.downcast_ref::<DaemonError>().is_some() {
                error
            } else {
                ConnectionError(error).into()
            }
        })
    }

    fn request_inner(
        &mut self,
        request: &impl serde::Serialize,
        timeout: Option<Duration>,
    ) -> Result<Value> {
        self.reader
            .get_ref()
            .set_read_timeout(timeout)
            .context("setting control response timeout")?;
        serde_json::to_writer(&mut self.writer, request).context("encoding control request")?;
        self.writer
            .write_all(b"\n")
            .context("delimiting control request")?;
        self.writer.flush().context("sending control request")?;
        self.response.clear();
        (&mut self.reader)
            .take(MAX_FRAME)
            .read_line(&mut self.response)
            .context("reading control response")?;
        if !self.response.ends_with('\n') {
            bail!("protocol: incomplete or oversized daemon response");
        }
        let mut response: Value = serde_json::from_str(&self.response)?;
        if response["version"] != 1 {
            bail!("protocol: unsupported response version");
        }
        if response["ok"] == true {
            Ok(response["result"].take())
        } else {
            Err(DaemonError {
                code: response["error"]["code"]
                    .as_str()
                    .unwrap_or("daemon-error")
                    .to_owned(),
                message: response["error"]["message"]
                    .as_str()
                    .unwrap_or("request failed")
                    .to_owned(),
            }
            .into())
        }
    }
}

pub fn runtime_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("AMUX_RUNTIME_DIR") {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(path).join("amux");
    }
    std::env::temp_dir().join(format!("amux-{}", unsafe { libc::geteuid() }))
}
pub fn socket_path() -> PathBuf {
    std::env::var_os("AMUX_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| runtime_dir().join("control.sock"))
}
pub fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    base.join("amux/config.json")
}
pub fn private_dir(path: &Path) -> Result<()> {
    match fs::create_dir(path) {
        Ok(()) => fs::set_permissions(path, fs::Permissions::from_mode(0o700))?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(error.into()),
    }
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        bail!(
            "unsafe-runtime-directory: {} must be an owned private directory",
            path.display()
        );
    }
    Ok(())
}
pub struct Decoded {
    pub command: crate::command::Command,
    pub persistent: Option<Value>,
}

pub fn decode(value: Value) -> Result<Decoded> {
    if value["version"] != 1 {
        bail!("unsupported-version: API version 1 required");
    }
    let Value::Object(mut object) = value else {
        bail!("invalid-request: object required");
    };
    object.remove("version");
    let persistent = object.remove("persistent");
    let command = crate::command::Command::new(Value::Object(object))?;
    Ok(Decoded {
        command,
        persistent,
    })
}

pub fn wire(command: &crate::command::Command, persistent: Option<&Value>) -> Value {
    let mut value = command.to_value();
    value["version"] = 1.into();
    if let Some(persistent) = persistent {
        value["persistent"] = persistent.clone();
    }
    value
}

pub fn frame(command: &crate::command::Command) -> Value {
    let mut value = wire(command, None);
    if let Some(object) = value.as_object_mut() {
        object.retain(|key, value| !value.is_null() || key == "parent");
    }
    value
}

pub trait RequestBody {
    fn into_command(self) -> Result<crate::command::Command>;
}

impl RequestBody for &crate::command::Command {
    fn into_command(self) -> Result<crate::command::Command> {
        Ok(self.clone())
    }
}

impl RequestBody for Value {
    fn into_command(self) -> Result<crate::command::Command> {
        let mut value = self;
        if value.get("version").is_none() {
            value["version"] = 1.into();
        }
        Ok(decode(value)?.command)
    }
}

pub fn request(socket: &Path, body: impl RequestBody) -> Result<Value> {
    let command = body.into_command()?;
    let timeout = command
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(30000)
        .min(300000)
        + 5000;
    Connection::connect(socket)?.request(&frame(&command), Duration::from_millis(timeout))
}
fn readiness_wait(
    stream: &UnixStream,
    events: libc::c_short,
    deadline: Instant,
) -> std::io::Result<()> {
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "daemon readiness deadline")
            })?;
        let mut poll = libc::pollfd {
            fd: stream.as_raw_fd(),
            events,
            revents: 0,
        };
        let timeout = remaining
            .as_millis()
            .saturating_add(1)
            .min(i32::MAX as u128) as i32;
        let result = unsafe { libc::poll(&mut poll, 1, timeout) };
        if result > 0 {
            return Ok(());
        }
        if result == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "daemon readiness deadline",
            ));
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}
fn readiness_connect(socket: &Path, deadline: Instant) -> std::io::Result<UnixStream> {
    let path = socket.as_os_str().as_bytes();
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if path.len() >= address.sun_path.len() || path.contains(&0) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid daemon socket path",
        ));
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (destination, byte) in address.sun_path.iter_mut().zip(path) {
        *destination = *byte as libc::c_char;
    }
    let length =
        (std::mem::offset_of!(libc::sockaddr_un, sun_path) + path.len() + 1) as libc::socklen_t;
    #[cfg(target_os = "macos")]
    {
        address.sun_len = length as u8;
    }
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    stream.set_nonblocking(true)?;
    let result =
        unsafe { libc::connect(fd, (&address as *const libc::sockaddr_un).cast(), length) };
    if result < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINPROGRESS) {
            return Err(error);
        }
        readiness_wait(&stream, libc::POLLOUT, deadline)?;
        if let Some(error) = stream.take_error()? {
            return Err(error);
        }
    }
    Ok(stream)
}
struct ReadinessIo {
    stream: UnixStream,
    deadline: Instant,
}
impl ReadinessIo {
    fn check_deadline(&self) -> std::io::Result<()> {
        if Instant::now() >= self.deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "daemon readiness deadline",
            ));
        }
        Ok(())
    }
}
impl Read for ReadinessIo {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        loop {
            self.check_deadline()?;
            match self.stream.read(bytes) {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    readiness_wait(&self.stream, libc::POLLIN, self.deadline)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                result => return result,
            }
        }
    }
}
impl Write for ReadinessIo {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        loop {
            self.check_deadline()?;
            match self.stream.write(bytes) {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    readiness_wait(&self.stream, libc::POLLOUT, self.deadline)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                result => return result,
            }
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn probe_daemon(socket: &Path, deadline: Instant) -> Result<bool> {
    let stream = match readiness_connect(socket, deadline) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(false);
        }
        Err(error) => return Err(error).context("connecting readiness probe"),
    };
    let mut io = ReadinessIo { stream, deadline };
    // Snapshot is supported by older version-1 daemons and never mutates/replays work.
    io.write_all(b"{\"version\":1,\"op\":\"snapshot\"}\n")?;
    let mut response = String::new();
    BufReader::new(io)
        .take(MAX_FRAME)
        .read_line(&mut response)?;
    if !response.ends_with('\n') {
        bail!("protocol: incomplete or oversized readiness response");
    }
    let response: Value = serde_json::from_str(&response).context("invalid readiness JSON")?;
    let snapshot = &response["result"];
    if response["version"] != 1
        || response["ok"] != true
        || !snapshot["seq"].is_u64()
        || !snapshot["config"].is_object()
        || !snapshot["projects"].is_array()
        || !snapshot["terminals"].is_array()
    {
        bail!("protocol: readiness requires an initialized version-1 snapshot");
    }
    Ok(true)
}
fn startup_diagnostic(log: &Path, start: u64) -> String {
    (|| -> std::io::Result<String> {
        let mut file = fs::File::open(log)?;
        let offset = start.max(file.metadata()?.len().saturating_sub(16 * 1024));
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = Vec::new();
        file.take(16 * 1024).read_to_end(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
    })()
    .unwrap_or_default()
}
pub fn ensure_daemon(socket: &Path) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let parent = socket.parent().context("socket has no parent")?;
    private_dir(parent)?;
    let autostart = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(parent.join("autostart.lock"))?;
    loop {
        if unsafe { libc::flock(autostart.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            break;
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::WouldBlock {
            return Err(error).context("locking daemon autostart");
        }
        if Instant::now() >= deadline {
            bail!("daemon-start: timed out waiting for concurrent autostart");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let probe_deadline = deadline.min(Instant::now() + Duration::from_secs(2));
    if probe_daemon(socket, probe_deadline).context("daemon-readiness")? {
        return Ok(());
    }
    let log_path = parent.join("daemon.log");
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&log_path)?;
    let log_start = log.metadata()?.len();
    let mut command = Command::new(std::env::current_exe()?);
    command.arg("--socket").arg(socket).arg("daemon");
    command
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn()?;
    let mut last_error = String::new();
    loop {
        let probe_deadline = deadline.min(Instant::now() + Duration::from_millis(250));
        match probe_daemon(socket, probe_deadline) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(error) => last_error = format!("{error:#}"),
        }
        if let Some(status) = child.try_wait()? {
            let diagnostic = startup_diagnostic(&log_path, log_start);
            if diagnostic.contains("already-running:") && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(25));
                continue;
            }
            bail!(
                "daemon-start: exited {status}; inspect {}; {}",
                log_path.display(),
                diagnostic
            );
        }
        if Instant::now() >= deadline {
            // Only the child created by this caller is signalled; existing peers
            // are never killed or replaced merely because their probe failed.
            unsafe {
                libc::kill(child.id() as i32, libc::SIGTERM);
            }
            let stop_deadline = Instant::now() + Duration::from_secs(2);
            while child.try_wait()?.is_none() && Instant::now() < stop_deadline {
                std::thread::sleep(Duration::from_millis(25));
            }
            if child.try_wait()?.is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
            bail!(
                "daemon-start: initialized API did not become ready ({last_error}); inspect {}; {}",
                log_path.display(),
                startup_diagnostic(&log_path, log_start)
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[cfg(target_os = "macos")]
    use std::sync::Arc;
    #[cfg(target_os = "macos")]
    use std::sync::atomic::{AtomicBool, Ordering};

    #[cfg(target_os = "macos")]
    unsafe extern "C" {
        fn _NSGetArgc() -> *mut i32;
        fn _NSGetArgv() -> *mut *mut *mut libc::c_char;
    }

    #[cfg(target_os = "macos")]
    unsafe fn arg_is(arg: *const libc::c_char, expected: &[u8]) -> bool {
        if arg.is_null() {
            return false;
        }
        let mut index = 0;
        while index < expected.len() {
            let byte = unsafe { *arg.add(index) };
            if byte == 0 || byte != expected[index] as libc::c_char {
                return false;
            }
            index += 1;
        }
        unsafe { *arg.add(expected.len()) == 0 }
    }

    #[cfg(target_os = "macos")]
    unsafe extern "C" fn lose_spawned_daemon() {
        let key = b"AMUX_TEST_LOSE_AUTOSTART\0";
        if unsafe { libc::getenv(key.as_ptr().cast()) }.is_null() {
            return;
        }
        let argc_ptr = unsafe { _NSGetArgc() };
        let argv_ptr = unsafe { _NSGetArgv() };
        if argc_ptr.is_null() || argv_ptr.is_null() {
            return;
        }
        let argc = unsafe { *argc_ptr };
        let argv = unsafe { *argv_ptr };
        let Ok(count) = usize::try_from(argc) else {
            return;
        };
        if argv.is_null() || count == 0 {
            return;
        }
        let mut saw_socket = false;
        let mut index = 0;
        while index < count {
            if unsafe { arg_is(*argv.add(index), b"--socket") } {
                saw_socket = true;
            }
            index += 1;
        }
        let last_is_daemon = unsafe { arg_is(*argv.add(count - 1), b"daemon") };
        if !saw_socket || !last_is_daemon {
            return;
        }
        let message = b"already-running: daemon runtime is locked\n";
        unsafe {
            let _ = libc::write(2, message.as_ptr().cast(), message.len());
            libc::_exit(1);
        }
    }

    #[cfg(target_os = "macos")]
    #[used]
    #[unsafe(link_section = "__DATA,__mod_init_func")]
    static LOSE_SPAWNED_DAEMON: unsafe extern "C" fn() = lose_spawned_daemon;

    #[cfg(target_os = "macos")]
    struct ClearLoseAutostart;
    #[cfg(target_os = "macos")]
    impl Drop for ClearLoseAutostart {
        fn drop(&mut self) {
            unsafe { std::env::remove_var("AMUX_TEST_LOSE_AUTOSTART") };
        }
    }

    fn connection_pair() -> (Connection, UnixStream) {
        let (stream, peer) = UnixStream::pair().unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        (
            Connection {
                reader: BufReader::new(stream.try_clone().unwrap()),
                writer: BufWriter::with_capacity(16384, stream),
                response: String::new(),
            },
            peer,
        )
    }

    #[test]
    fn persistent_connection_preserves_buffered_response_frames() {
        let (mut connection, mut peer) = connection_pair();
        peer.write_all(
            b"{\"version\":1,\"ok\":true,\"result\":{\"seq\":1}}\n{\"version\":1,\"ok\":true,\"result\":{\"seq\":2}}\n",
        ).unwrap();
        let request = json!({"version":1,"persistent":true,"op":"snapshot"});
        assert_eq!(
            connection
                .request(&request, Duration::from_secs(2))
                .unwrap(),
            json!({"seq":1})
        );
        assert_eq!(
            connection
                .request(&request, Duration::from_secs(2))
                .unwrap(),
            json!({"seq":2})
        );
    }

    #[test]
    fn persistent_connection_exposes_errors_without_losing_next_frame() {
        let (mut connection, mut peer) = connection_pair();
        peer.write_all(
            b"{\"version\":1,\"ok\":false,\"error\":{\"code\":\"backpressure\",\"message\":\"queue full\"}}\n{\"version\":1,\"ok\":true,\"result\":true}\n",
        ).unwrap();
        let request = json!({"version":1,"persistent":true,"op":"input",
            "control":{"kind":"control","ref":"transport-fixture"}});
        let error = connection
            .request(&request, Duration::from_secs(2))
            .unwrap_err();
        let rejection = error.downcast_ref::<DaemonError>().unwrap();
        assert_eq!(rejection.code, "backpressure");
        assert_eq!(rejection.message, "queue full");
        assert_eq!(error.to_string(), "backpressure: queue full");
        assert_eq!(
            connection
                .request(&request, Duration::from_secs(2))
                .unwrap(),
            json!(true)
        );
    }

    #[test]
    fn connection_rejects_incomplete_and_unsupported_responses() {
        for (response, expected) in [
            (
                "{\"version\":1,\"ok\":true,\"result\":true}",
                "protocol: incomplete or oversized daemon response",
            ),
            (
                "{\"version\":2,\"ok\":true,\"result\":true}\n",
                "protocol: unsupported response version",
            ),
        ] {
            let (mut connection, mut peer) = connection_pair();
            peer.write_all(response.as_bytes()).unwrap();
            peer.shutdown(std::net::Shutdown::Write).unwrap();
            let error = connection
                .request(
                    &json!({"version":1,"op":"snapshot"}),
                    Duration::from_secs(2),
                )
                .unwrap_err();
            assert_eq!(error.to_string(), expected);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn already_running_child_then_ready_probe_returns_ok() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::net::UnixListener;

        let directory =
            std::path::PathBuf::from(format!("/tmp/amux-autostart-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let socket = directory.join("control.sock");
        let log_path = directory.join("daemon.log");
        let _clear = ClearLoseAutostart;
        unsafe { std::env::set_var("AMUX_TEST_LOSE_AUTOSTART", "1") };
        let served = Arc::new(AtomicBool::new(false));
        let served_flag = Arc::clone(&served);
        let server_socket = socket.clone();
        let server_log = log_path.clone();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if server_log.is_file()
                    && fs::read_to_string(&server_log)
                        .unwrap_or_default()
                        .contains("already-running:")
                {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "losing child did not record already-running"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            std::thread::sleep(Duration::from_millis(300));
            let listener = UnixListener::bind(&server_socket).unwrap();
            listener.set_nonblocking(true).unwrap();
            let accept_deadline = Instant::now() + Duration::from_secs(2);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if Instant::now() >= accept_deadline {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("accept readiness probe: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let request: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["op"], "snapshot");
            stream
                .write_all(b"{\"version\":1,\"ok\":true,\"result\":{\"seq\":1,\"config\":{},\"projects\":[],\"terminals\":[]}}\n")
                .unwrap();
            served_flag.store(true, Ordering::SeqCst);
        });
        let result = ensure_daemon(&socket);
        server.join().unwrap();
        let log = fs::read_to_string(&log_path).unwrap_or_default();
        let _ = fs::remove_dir_all(&directory);
        if let Err(error) = result {
            panic!("{error:#}");
        }
        assert!(served.load(Ordering::SeqCst), "ready probe was not served");
        assert!(
            log.contains("already-running:"),
            "child exit was not already-running: {log}"
        );
    }
}

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

pub(crate) struct Connection {
    reader: BufReader<UnixStream>,
    writer: BufWriter<UnixStream>,
    response: String,
}

impl Connection {
    pub(crate) fn connect(socket: &Path) -> Result<Self> {
        let stream = UnixStream::connect(socket)
            .with_context(|| format!("cannot connect to daemon at {}", socket.display()))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        Ok(Self {
            reader: BufReader::new(stream.try_clone()?),
            writer: BufWriter::with_capacity(16384, stream),
            response: String::new(),
        })
    }

    pub(crate) fn request(
        &mut self,
        request: &impl serde::Serialize,
        timeout: Duration,
    ) -> Result<Value> {
        self.reader
            .get_ref()
            .set_read_timeout(Some(timeout))
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
pub fn request(socket: &Path, mut request: Value) -> Result<Value> {
    if request.get("version").is_none() {
        request["version"] = 1.into();
    }
    let timeout = request
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(30000)
        .min(300000)
        + 5000;
    Connection::connect(socket)?.request(&request, Duration::from_millis(timeout))
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
            bail!(
                "daemon-start: exited {status}; inspect {}; {}",
                log_path.display(),
                startup_diagnostic(&log_path, log_start)
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
}

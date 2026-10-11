use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const MAX_FRAME: u64 = 64 * 1024 * 1024;

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
        self.reader.get_ref().set_read_timeout(Some(timeout))?;
        serde_json::to_writer(&mut self.writer, request)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        self.response.clear();
        (&mut self.reader).take(MAX_FRAME).read_line(&mut self.response)?;
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
            bail!(
                "{}: {}",
                response["error"]["code"].as_str().unwrap_or("daemon-error"),
                response["error"]["message"]
                    .as_str()
                    .unwrap_or("request failed")
            );
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
pub fn ensure_daemon(socket: &Path) -> Result<()> {
    if UnixStream::connect(socket).is_ok() {
        return Ok(());
    }
    let parent = socket.parent().context("socket has no parent")?;
    private_dir(parent)?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(parent.join("daemon.log"))?;
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
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if UnixStream::connect(socket).is_ok() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            bail!(
                "daemon-start: exited {status}; inspect {}",
                parent.join("daemon.log").display()
            );
        }
        if Instant::now() >= deadline {
            bail!(
                "daemon-start: socket did not become ready; inspect {}",
                parent.join("daemon.log").display()
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
        stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
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
            connection.request(&request, Duration::from_secs(2)).unwrap(),
            json!({"seq":1})
        );
        assert_eq!(
            connection.request(&request, Duration::from_secs(2)).unwrap(),
            json!({"seq":2})
        );
    }

    #[test]
    fn persistent_connection_exposes_errors_without_losing_next_frame() {
        let (mut connection, mut peer) = connection_pair();
        peer.write_all(
            b"{\"version\":1,\"ok\":false,\"error\":{\"code\":\"backpressure\",\"message\":\"queue full\"}}\n{\"version\":1,\"ok\":true,\"result\":true}\n",
        ).unwrap();
        let request = json!({"version":1,"persistent":true,"op":"input"});
        assert_eq!(
            connection.request(&request, Duration::from_secs(2)).unwrap_err().to_string(),
            "backpressure: queue full"
        );
        assert_eq!(
            connection.request(&request, Duration::from_secs(2)).unwrap(),
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
                .request(&json!({"version":1,"op":"snapshot"}), Duration::from_secs(2))
                .unwrap_err();
            assert_eq!(error.to_string(), expected);
        }
    }
}

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const MAX_FRAME: u64 = 64 * 1024 * 1024;
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
    let mut stream = UnixStream::connect(socket)
        .with_context(|| format!("cannot connect to daemon at {}", socket.display()))?;
    stream.set_read_timeout(Some(Duration::from_millis(timeout)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    {
        let mut writer = BufWriter::with_capacity(16384, &mut stream);
        serde_json::to_writer(&mut writer, &request)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
    }
    let mut reader = BufReader::new(stream).take(MAX_FRAME);
    let mut line = String::new();
    use std::io::Read;
    reader.read_line(&mut line)?;
    if !line.ends_with('\n') {
        bail!("protocol: incomplete or oversized daemon response");
    }
    let mut response: Value = serde_json::from_str(&line)?;
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

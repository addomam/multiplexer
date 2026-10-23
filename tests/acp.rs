use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(12);

fn exchange(socket: &Path, mut request: Value) -> std::io::Result<Value> {
    request["version"] = json!(1);
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(DEADLINE))?;
    stream.set_write_timeout(Some(DEADLINE))?;
    serde_json::to_writer(&mut stream, &request)?;
    stream.write_all(b"\n")?;
    let mut line = String::new();
    BufReader::new(stream).take(8 * 1024 * 1024).read_line(&mut line)?;
    assert!(line.ends_with('\n'), "incomplete daemon response");
    Ok(serde_json::from_str(&line)?)
}

struct Host {
    root: PathBuf,
    socket: PathBuf,
    daemon: Child,
}

impl Host {
    fn new() -> Self {
        let root = fs::canonicalize("/tmp")
            .unwrap()
            .join(format!("amux-acp-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["home", "startup", "config", "runtime", "projects"] {
            fs::create_dir(root.join(name)).unwrap();
            fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::write(root.join("startup/.zshrc"), "PS1='ACP_FIXTURE> '\n").unwrap();
        let socket = root.join("daemon.sock");
        let log = fs::File::create(root.join("daemon.log")).unwrap();
        let daemon = Command::new(env!("CARGO_BIN_EXE_amux"))
            .arg("--socket")
            .arg(&socket)
            .arg("daemon")
            .arg("--projects-dir")
            .arg(root.join("projects"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("HOME", root.join("home"))
            .env("SHELL", "/bin/zsh")
            .env("USER", "amux-fixture")
            .env("TERM", "xterm-256color")
            .env("ZDOTDIR", root.join("startup"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_RUNTIME_DIR", root.join("runtime"))
            .env("AMUX_RUNTIME_DIR", root.join("runtime"))
            .env("AMUX_SOCKET", &socket)
            .current_dir(&root)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();
        let host = Self { root, socket, daemon };
        host.until("private daemon startup", || {
            exchange(&host.socket, json!({"op":"snapshot"}))
                .ok()
                .filter(|response| response["ok"] == true)
        });
        host
    }

    fn until<T>(&self, label: &str, mut probe: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + DEADLINE;
        loop {
            if let Some(value) = probe() {
                return value;
            }
            assert!(Instant::now() < deadline, "{label}; {}", fs::read_to_string(self.root.join("daemon.log")).unwrap_or_default());
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn call(&self, request: Value) -> Value {
        let response = exchange(&self.socket, request).unwrap();
        assert_eq!(response["ok"], true, "{response}");
        response["result"].clone()
    }

    fn view(&self, id: &str) -> Option<Value> {
        self.call(json!({"op":"snapshot"}))["agent_views"]
            .as_array()?
            .iter()
            .find(|view| view["id"] == id)
            .cloned()
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = exchange(&self.socket, json!({"op":"shutdown"}));
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.daemon.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if self.daemon.try_wait().ok().flatten().is_none() {
            let _ = self.daemon.kill();
        }
        let _ = self.daemon.wait();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn e04s01_prompt_effect_and_stream_share_one_live_instance() {
    let host = Host::new();
    let project = host.call(json!({"op":"snapshot"}))["projects"][0]["id"].clone();
    let controller = host.call(json!({"op":"controller-create","role":"external","label":"ACP tracer"}))["controller"].clone();
    let peer = Path::new(env!("CARGO_BIN_EXE_amux"))
        .parent().unwrap().join("examples/acp_fixture");
    let opened = host.call(json!({"op":"acp-open","project":project,"controller":controller,
        "command":[peer,host.root],"name":"Local arithmetic peer"}));
    let id = opened["target"]["id"].as_str().unwrap();
    let view = host.until("same ACP session ready", || {
        host.view(id).filter(|view| view["lifecycle"] == "ready")
    });
    let target = json!({"kind":"acp","id":id,"instance_id":view["instance_id"],
        "connection_generation":view["connection_generation"],"session_id":view["session_id"]});
    let snapshot = host.call(json!({"op":"snapshot"}));
    assert!(snapshot["terminals"].as_array().unwrap().iter().all(|t| t["id"] != id), "ACP must not create a fake PTY");
    host.call(json!({"op":"acp-prompt","target":target,"control":opened["control"],
        "owner_generation":opened["generation"],"call_id":"arithmetic-1","text":"add 17 25"}));
    host.until("agent applied arithmetic once", || {
        let effect: Value = serde_json::from_slice(&fs::read(host.root.join("effect.json")).ok()?).ok()?;
        (effect["value"] == 42 && effect["commands"] == 1 && effect["session_id"] == target["session_id"]).then_some(())
    });
    host.until("canonical stream from that live instance", || {
        let inspected = host.call(json!({"op":"acp-inspect","target":target,"include_content":true}));
        inspected["content"]["events"].as_array()?.iter().any(|event| {
            event["kind"] == "agent-message" && event["text"] == "Total=42; commands=1"
        }).then_some(())
    });
    host.call(json!({"op":"acp-close","target":target,"control":opened["control"],
        "owner_generation":opened["generation"],"call_id":"close-arithmetic"}));
    host.until("owned peer reaped before fixture fallback", || {
        let pid: i32 = fs::read_to_string(host.root.join("peer.pid")).ok()?.parse().ok()?;
        (unsafe { libc::kill(pid, 0) } == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)).then_some(())
    });
}

use amux::client;
use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Harness {
    directory: PathBuf,
    socket: PathBuf,
    child: Child,
}

impl Harness {
    fn new() -> Self {
        let directory = fs::canonicalize("/tmp")
            .unwrap()
            .join(format!("amux-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let startup = directory.join("startup");
        fs::create_dir(&startup).unwrap();
        fs::write(
            startup.join(".zshrc"),
            "export AMUX_TEST_EXPORT=preserved\nPS1='NATIVE_PROMPT> '\nautoload -Uz add-zsh-hook\nfunction user_precmd() { :; }\nadd-zsh-hook precmd user_precmd\n",
        )
        .unwrap();
        let socket = directory.join("control.sock");
        let log = fs::File::create(directory.join("daemon.log")).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_amux"))
            .arg("--socket")
            .arg(&socket)
            .arg("daemon")
            .arg("--projects-dir")
            .arg(directory.join("projects"))
            .env("XDG_CONFIG_HOME", directory.join("config"))
            .env("ZDOTDIR", &startup)
            .current_dir(&directory)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();
        let harness = Self {
            directory,
            socket,
            child,
        };
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            if client::request(&harness.socket, json!({"op":"snapshot"})).is_ok() {
                break;
            }
            assert!(Instant::now() < deadline, "daemon start");
            std::thread::sleep(Duration::from_millis(20));
        }
        harness
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = client::request(&self.socket, json!({"op":"shutdown"}));
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn wait_document_has_seq_and_state() {
    let harness = Harness::new();
    let snapshot = client::request(&harness.socket, json!({"op":"snapshot"})).unwrap();
    let id = snapshot["terminals"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let waited = client::request(
        &harness.socket,
        json!({"op":"wait","terminal":id,"condition":"shell-ready","timeout_ms":10000}),
    )
    .unwrap();
    assert!(waited["seq"].is_number());
    assert_eq!(waited["state"]["id"], id);
    assert_eq!(waited["state"]["alive"], true);
    assert_eq!(waited["state"]["integration"]["shell"]["ready"], true);
}

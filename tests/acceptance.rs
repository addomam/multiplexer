use amux::client;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Harness {
    directory: PathBuf,
    socket: PathBuf,
    child: Child,
    controller: Value,
    controls: RefCell<HashMap<String, Value>>,
}
impl Harness {
    fn new() -> Self {
        Self::with_daemon_command(|_, _| {})
    }
    fn with_daemon_command(configure: impl FnOnce(&std::path::Path, &mut Command)) -> Self {
        let directory = fs::canonicalize("/tmp")
            .unwrap()
            .join(format!("amux-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let startup = directory.join("startup");
        fs::create_dir(&startup).unwrap();
        fs::write(startup.join(".zshrc"),"export AMUX_TEST_EXPORT=preserved\nPS1='NATIVE_PROMPT> '\nautoload -Uz add-zsh-hook\nfunction user_precmd() { :; }\nadd-zsh-hook precmd user_precmd\n").unwrap();
        let socket = directory.join("control.sock");
        let log = fs::File::create(directory.join("daemon.log")).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_amux"));
        command
            .arg("--socket")
            .arg(&socket)
            .arg("daemon")
            .arg("--projects-dir")
            .arg(directory.join("projects"))
            .env("XDG_CONFIG_HOME", directory.join("config"))
            .env("ZDOTDIR", &startup)
            .env("AMUX_REMOVABLE_EXPORT", "daemon-only")
            .current_dir(&directory)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log);
        configure(&directory, &mut command);
        let child = command.spawn().unwrap();
        let mut h = Self {
            directory,
            socket,
            child,
            controller: Value::Null,
            controls: RefCell::new(HashMap::new()),
        };
        h.until(|| h.call(json!({"op":"snapshot"})).ok(), "daemon start");
        h.controller = h
            .call(json!({"op":"controller-create","role":"external","label":"acceptance fixture"}))
            .unwrap()["controller"]
            .clone();
        h
    }
    fn call(&self, mut request: Value) -> anyhow::Result<Value> {
        if matches!(request["op"].as_str(), Some("input" | "execute")) {
            let terminal = request["terminal"].as_str().expect("mutation terminal");
            let control = self
                .controls
                .borrow()
                .get(terminal)
                .unwrap_or_else(|| {
                    panic!("fixture must acquire control before mutating {terminal}")
                })
                .clone();
            request["control"] = control;
        }
        client::request(&self.socket, request)
    }
    fn acquire(&self, id: &str) {
        if self.controls.borrow().contains_key(id) {
            return;
        }
        // Initial shell readiness can precede automatic-launch dispatch/retirement.
        // Observe completion before making exactly one explicit ownership claim.
        let state = self.until(
            || {
                let state = self.call(json!({"op":"state","terminal":id})).ok()?;
                (state["integration"]["shell"]["ready"] == true
                    && state["ownership"]["owner"].is_null()
                    && state["ownership"]["reservation"].is_null()
                    && state["ownership"]["unsettled"]
                        .as_array()
                        .is_some_and(Vec::is_empty))
                .then_some(state)
            },
            "shell ready and automatic launch authority retired",
        );
        let grant = self
            .call(json!({"op":"ownership-claim","controller":self.controller,
            "terminal":id,"observed":state["ownership"]["observed"],
            "mode":"cooperative","when":{"kind":"now"}}))
            .unwrap();
        assert_eq!(
            grant["kind"], "control",
            "fixture requires an unowned settled target: {grant}"
        );
        self.controls
            .borrow_mut()
            .insert(id.to_owned(), grant["control"].clone());
    }
    fn until<T>(&self, mut f: impl FnMut() -> Option<T>, label: &str) -> T {
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            if let Some(value) = f() {
                return value;
            }
            assert!(
                Instant::now() < deadline,
                "{label}; snapshot {:?}; main screen {}; log {}",
                self.call(json!({"op":"snapshot"})),
                self.screen(&self.main()),
                fs::read_to_string(self.directory.join("daemon.log")).unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn ready(&self, id: &str) {
        self.call(json!({"op":"wait","terminal":id,"condition":"shell-ready","timeout_ms":10000}))
            .unwrap();
        self.acquire(id);
    }
    fn main(&self) -> String {
        self.call(json!({"op":"snapshot"})).unwrap()["terminals"][0]["id"]
            .as_str()
            .unwrap()
            .into()
    }
    fn screen(&self, id: &str) -> String {
        self.call(json!({"op":"screen","terminal":id})).unwrap()["lines"].to_string()
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.call(json!({"op":"shutdown"}));
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn git(directory: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn inherited_git_common_dir_does_not_corrupt_workspace_metadata() {
    let h = Harness::with_daemon_command(|directory, command| {
        for name in ["repo-a", "repo-b"] {
            let repo = directory.join(name);
            fs::create_dir(&repo).unwrap();
            git(&repo, &["init"]);
            git(
                &repo,
                &[
                    "-c",
                    "user.name=Acceptance",
                    "-c",
                    "user.email=acceptance@example.test",
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "--allow-empty",
                    "-m",
                    name,
                ],
            );
        }
        git(
            &directory.join("repo-b"),
            &["worktree", "add", "-b", "actual-b", "../workspace-b"],
        );
        command.env("GIT_COMMON_DIR", directory.join("repo-a/.git"));
    });
    let main = h.main();
    h.ready(&main);
    let workspace = h.directory.join("workspace-b");
    fs::write(workspace.join("only-in-b"), "workspace b\n").unwrap();
    h.call(json!({"op":"execute","terminal":main,"source":format!("cd '{}'",workspace.display())}))
        .unwrap();
    h.ready(&main);
    let metadata = h.until(
        || {
            let state = h.call(json!({"op":"state","terminal":main})).ok()?;
            let metadata = state["workspace"]["git"].clone();
            (metadata["repository_root"] == workspace.display().to_string()
                && metadata["refreshed_at_ms"].is_number())
            .then_some(metadata)
        },
        "actual workspace B Git metadata",
    );
    assert_eq!(
        metadata["common_dir"],
        h.directory.join("repo-b/.git").display().to_string()
    );
    assert_eq!(
        metadata["git_dir"],
        h.directory
            .join("repo-b/.git/worktrees/workspace-b")
            .display()
            .to_string()
    );
    assert_eq!(metadata["worktree_id"], metadata["git_dir"]);
    assert_eq!(metadata["branch"], "actual-b");
    assert_eq!(metadata["isolated"], true);
    assert!(metadata["error"].is_null());
    let saved = h
        .call(json!({"op":"project-save","name":"actual-b"}))
        .unwrap();
    assert_eq!(saved["context"], workspace.display().to_string());
}

#[test]
fn project_save_immediately_after_confirmed_cd_uses_latest_workspace_root() {
    let h = Harness::new();
    let main = h.main();
    h.ready(&main);
    let repo = h.directory.join("fresh-repo");
    let nested = repo.join("nested/deeper");
    fs::create_dir_all(&nested).unwrap();
    git(&repo, &["init"]);
    let outside = h.directory.join("outside");
    fs::create_dir(&outside).unwrap();
    for (cwd, expected) in [(&nested, &repo), (&outside, &outside)] {
        h.call(json!({"op":"execute","terminal":main,"source":format!("cd '{}'",cwd.display())}))
            .unwrap();
        h.ready(&main);
        // No metadata/cache wait: shell-ready confirms the cd, and save must
        // resolve that confirmed cwd before writing the project configuration.
        let saved = h
            .call(json!({"op":"project-save","name":"latest-cwd"}))
            .unwrap();
        assert_eq!(saved["context"], expected.display().to_string());
        let config: Value =
            serde_json::from_slice(&fs::read(h.directory.join("config/amux/config.json")).unwrap())
                .unwrap();
        let persisted = config["projects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|project| project["id"] == saved["id"])
            .unwrap();
        assert_eq!(persisted["context"], expected.display().to_string());
    }
}

#[test]
fn host_context_child_partial_busy_tree_detach_and_shutdown() {
    let h = Harness::new();
    let main = h.main();
    h.ready(&main);
    let repo = h.directory.join("repo");
    fs::create_dir_all(repo.join("nested")).unwrap();
    assert!(
        Command::new("git")
            .arg("init")
            .arg(&repo)
            .output()
            .unwrap()
            .status
            .success()
    );
    h.call(
        json!({"op":"execute","terminal":main,"source":format!("unset AMUX_REMOVABLE_EXPORT; cd '{}'/nested",repo.display())}),
    )
    .unwrap();
    h.ready(&main);
    h.until(
        || {
            let snap = h.call(json!({"op":"snapshot"})).ok()?;
            (snap["projects"][0]["context"] == repo.display().to_string()).then_some(())
        },
        "repo context follows main cwd",
    );
    let child = h
        .call(json!({"op":"open","source":main,"name":"child"}))
        .unwrap();
    let id = child["id"].as_str().unwrap();
    h.ready(id);
    assert_eq!(child["parent_id"], main);
    assert_eq!(
        child["workspace"]["cwd"],
        repo.join("nested").display().to_string()
    );
    h.call(json!({"op":"execute","terminal":id,"source":"print EXPORT=$AMUX_TEST_EXPORT REMOVED=${AMUX_REMOVABLE_EXPORT-absent}"}))
        .unwrap();
    h.until(
        || {
            h.screen(id)
                .contains("EXPORT=preserved REMOVED=absent")
                .then_some(())
        },
        "captured export",
    );
    h.ready(id);
    let sibling = h
        .call(json!({"op":"open","source":id,"name":"captured-from-child"}))
        .unwrap();
    assert_eq!(sibling["parent_id"], main);
    assert_eq!(sibling["workspace"]["cwd"], child["workspace"]["cwd"]);
    h.call(json!({"op":"input","terminal":main,"text":"print DO_NOT_OVERWRITE"}))
        .unwrap();
    std::thread::sleep(Duration::from_millis(80));
    let error = h
        .call(json!({"op":"execute","terminal":main,"source":"print WRONG"}))
        .unwrap_err()
        .to_string();
    assert!(error.contains("partial-input"), "{error}");
    assert!(h.screen(&main).contains("DO_NOT_OVERWRITE"));
    h.call(json!({"op":"input","terminal":main,"key":"ctrl-c"}))
        .unwrap();
    h.ready(&main);
    h.call(json!({"op":"execute","terminal":main,"source":"sleep 30"}))
        .unwrap();
    let error = h
        .call(json!({"op":"execute","terminal":main,"source":"print WRONG"}))
        .unwrap_err()
        .to_string();
    assert!(error.contains("busy"), "{error}");
    let busy_child = h
        .call(json!({"op":"open","source":main,"name":"busy-child"}))
        .unwrap();
    assert_eq!(busy_child["source_freshness"], "stale");
    h.call(json!({"op":"execute","terminal":main,"source":"print QUEUED_SUCCESS","queue":true}))
        .unwrap();
    h.call(json!({"op":"input","terminal":main,"key":"ctrl-c"}))
        .unwrap();
    h.until(
        || h.screen(&main).contains("QUEUED_SUCCESS").then_some(()),
        "explicit queued execution",
    );
    h.ready(&main);
    let pid = h.call(json!({"op":"state","terminal":id})).unwrap()["pid"].clone();
    h.call(json!({"op":"reparent","terminal":id,"parent":null}))
        .unwrap();
    h.call(json!({"op":"reparent","terminal":main,"parent":id}))
        .unwrap();
    assert!(
        h.call(json!({"op":"reparent","terminal":id,"parent":main}))
            .unwrap_err()
            .to_string()
            .contains("cycle")
    );
    assert_eq!(
        h.call(json!({"op":"state","terminal":id})).unwrap()["pid"],
        pid
    );
    h.call(json!({"op":"detach"})).unwrap();
    assert_eq!(
        h.call(json!({"op":"state","terminal":id})).unwrap()["pid"],
        pid
    );
    h.call(json!({"op":"name","terminal":main,"name":"child"}))
        .unwrap();
    assert!(
        h.call(json!({"op":"state","terminal":"child"}))
            .unwrap_err()
            .to_string()
            .contains("ambiguous")
    );
    h.call(json!({"op":"close","terminal":id})).unwrap();
    h.call(json!({"op":"wait","terminal":id,"condition":"exited"}))
        .unwrap();
    assert!(h.call(json!({"op":"state","terminal":id})).unwrap()["exit_status"].is_number());
    h.call(json!({"op":"remove","terminal":id})).unwrap();
    h.call(json!({"op":"shutdown"})).unwrap();
    h.until(
        || (!h.socket.exists()).then_some(()),
        "shutdown removes socket",
    );
}

#[test]
fn sequenced_waits_replay_transitions_and_report_stream_gaps() {
    let h = Harness::new();
    let main = h.main();
    h.ready(&main);
    let after = h.call(json!({"op":"snapshot"})).unwrap()["seq"]
        .as_u64()
        .unwrap();
    let pid = std::process::id();
    for (activity, settled, generation) in [
        ("working", false, 1),
        ("needs-input", false, 1),
        ("idle", true, 1),
    ] {
        let lifecycle_event = if settled { "agent_end" } else { "agent_start" };
        h.call(json!({"op":"report","terminal":main,"event":{"kind":"agent","instance_id":"root","agent_kind":"main","lifecycle_event":lifecycle_event,"pid":pid,"activity":activity,"settled":settled,"generation":generation,"completed_generation":if settled{1}else{0},"conversation_id":"conversation"}})).unwrap();
    }
    let observed=h.call(json!({"op":"wait","terminal":main,"condition":"agent-working","after":after,"timeout_ms":50})).unwrap();
    assert_eq!(observed["observed"]["agent"]["activity"], "working");
    h.call(json!({"op":"report","terminal":main,"event":{"kind":"agent","instance_id":"child","agent_kind":"sub","pid":pid,"activity":"needs-input","settled":false}})).unwrap();
    assert_eq!(
        h.call(json!({"op":"state","terminal":main})).unwrap()["integration"]["agent"]["activity"],
        "idle"
    );
    assert!(
        h.call(json!({"op":"wait","terminal":main,"condition":"agent-settled","timeout_ms":50}))
            .is_ok()
    );
    let finished=h.call(json!({"op":"wait","terminal":main,"condition":"agent-settled","after":after,"timeout_ms":50})).unwrap();
    let cursor = finished["seq"].as_u64().unwrap();
    for source in ["heartbeat", "auto_compaction_end"] {
        h.call(json!({"op":"report","terminal":main,"event":{"kind":"agent","instance_id":"root","agent_kind":"main","lifecycle_event":source,"pid":pid,"activity":"idle","settled":true,"generation":1,"completed_generation":1}})).unwrap();
    }
    assert!(h.call(json!({"op":"wait","terminal":main,"condition":"agent-settled","after":cursor,"timeout_ms":100})).unwrap_err().to_string().contains("timeout"));
    for (source, activity, settled) in [
        ("agent_start", "working", false),
        ("agent_end", "idle", true),
    ] {
        h.call(json!({"op":"report","terminal":main,"event":{"kind":"agent","instance_id":"root","agent_kind":"main","lifecycle_event":source,"pid":pid,"activity":activity,"settled":settled,"generation":2,"completed_generation":if settled{2}else{0}}})).unwrap();
    }
    assert_eq!(h.call(json!({"op":"wait","terminal":main,"condition":"agent-settled","after":cursor,"timeout_ms":50})).unwrap()["observed"]["agent"]["generation"],2);
    // More events than retained; the old cursor must not imply a lossless stream.
    let live_cursor = h.call(json!({"op":"snapshot"})).unwrap()["seq"]
        .as_u64()
        .unwrap();
    let socket = h.socket.clone();
    let wait_terminal = main.clone();
    let live_wait = std::thread::spawn(move || {
        client::request(
            &socket,
            json!({"op":"wait","terminal":wait_terminal,"condition":"agent-working","after":live_cursor,"timeout_ms":30000}),
        )
    });
    std::thread::sleep(Duration::from_millis(50));
    for i in 0..2050 {
        h.call(json!({"op":"name","terminal":main,"name":format!("n{i}")}))
            .unwrap();
    }
    h.call(json!({"op":"report","terminal":main,"event":{"kind":"agent","instance_id":"root","agent_kind":"main","lifecycle_event":"agent_start","pid":pid,"activity":"working","settled":false,"generation":3,"completed_generation":2}})).unwrap();
    assert_eq!(
        live_wait.join().unwrap().unwrap()["observed"]["agent"]["generation"],
        3
    );
    let events = h
        .call(json!({"op":"events","after":after,"timeout_ms":0}))
        .unwrap();
    assert_eq!(events["gap"], true);
    assert!(events["snapshot"].is_object());
    assert!(h.call(json!({"op":"wait","terminal":main,"condition":"agent-working","after":after,"timeout_ms":10})).unwrap_err().to_string().contains("stream-gap"));
}

#[test]
fn screen_queries_slow_subscriber_and_noisy_terminal_do_not_block_input() {
    let h = Harness::new();
    let main = h.main();
    h.ready(&main);
    let noisy = h
        .call(json!({"op":"open","source":main,"name":"noisy"}))
        .unwrap();
    let noisy = noisy["id"].as_str().unwrap();
    h.ready(noisy);
    h.call(json!({"op":"execute","terminal":noisy,"source":"while true; do printf 'background noise 012345678901234567890123456789\\n'; done"})).unwrap();
    let mut slow = UnixStream::connect(&h.socket).unwrap();
    writeln!(
        slow,
        "{}",
        json!({"version":1,"op":"events","after":0,"timeout_ms":1000})
    )
    .unwrap();
    let mut measurements = Vec::new();
    for i in 0..20 {
        let start = Instant::now();
        h.call(json!({"op":"input","terminal":main,"text":format!("print INPUT_{i}\\n").replace("\\n","\r")})).unwrap();
        h.until(
            || {
                h.screen(&main)
                    .contains(&format!("INPUT_{i}"))
                    .then_some(())
            },
            "foreground echo under background load",
        );
        measurements.push(start.elapsed().as_millis());
    }
    measurements.sort_unstable();
    println!(
        "foreground input under noisy PTY: p50={}ms p95={}ms max={}ms",
        measurements[10], measurements[18], measurements[19]
    );
    assert!(
        measurements[18] < 1000,
        "foreground input starved: {measurements:?}"
    );
    h.call(json!({"op":"close","terminal":noisy})).unwrap();
    drop(slow);
}

#[test]
fn unsupported_version_is_explicit_json_error() {
    let h = Harness::new();
    let mut stream = UnixStream::connect(&h.socket).unwrap();
    stream
        .write_all(b"{\"version\":2,\"op\":\"snapshot\"}\n")
        .unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    let error: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(error["error"]["code"], "unsupported-version");
}

#[test]
fn agent_crash_is_published_without_a_final_extension_report() {
    let h = Harness::new();
    let main = h.main();
    h.ready(&main);
    let mut process = Command::new("sleep").arg("30").spawn().unwrap();
    let report=h.call(json!({"op":"report","terminal":main,"event":{"kind":"agent","instance_id":"crash-probe","agent_kind":"main","pid":process.id(),"activity":"working","generation":1,"settled":false}}));
    let cursor = h.call(json!({"op":"snapshot"})).unwrap()["seq"]
        .as_u64()
        .unwrap();
    process.kill().unwrap();
    process.wait().unwrap();
    report.unwrap();
    let mut after = cursor;
    h.until(
        || {
            let result = h
                .call(json!({"op":"events","after":after,"timeout_ms":400}))
                .unwrap();
            after = result["seq"].as_u64().unwrap();
            result["events"]
                .as_array()
                .unwrap()
                .iter()
                .find(|event| {
                    event["kind"] == "state-refreshed"
                        && event["terminal"] == main
                        && event["data"]["integration"]["agent"]["activity"] == "unknown"
                })
                .cloned()
        },
        "agent crash transition",
    );
    assert_eq!(
        h.call(json!({"op":"state","terminal":main})).unwrap()["integration"]["agent"]["settled"],
        false
    );
    assert!(h.call(json!({"op":"wait","terminal":main,"condition":"agent-settled","after":cursor,"timeout_ms":200})).unwrap_err().to_string().contains("timeout"));
}

#[test]
fn maximum_supported_screen_is_readable_through_shared_client() {
    let h = Harness::new();
    let main = h.main();
    h.ready(&main);
    h.call(json!({"op":"resize","terminal":main,"cols":500,"rows":300}))
        .unwrap();
    h.call(json!({"op":"execute","terminal":main,"source":"printf '\\033[300;498HMAX'"}))
        .unwrap();
    h.ready(&main);
    let screen = h.call(json!({"op":"screen","terminal":main})).unwrap();
    assert!(
        screen["lines"]
            .as_array()
            .unwrap()
            .iter()
            .any(|line| line.as_str().unwrap().ends_with("MAX"))
    );
    assert_eq!(screen["cols"], 500);
    assert_eq!(screen["rows"], 300);
}

#[test]
fn initial_hooks_continue_after_failure_and_concurrent_open_runs_once() {
    let h = Harness::new();
    let result = h
        .call(
            json!({"op":"project-new","name":"hooked","config":{"hooks":[
        {"source":"printf one >> hook-order; printf HOOK_OUTPUT","timeout_ms":2000},
        {"source":"printf two >> hook-order; printf HOOK_FAILURE >&2; exit 7","timeout_ms":2000},
        {"source":"printf three >> hook-order","timeout_ms":2000}
    ],"recipes":{"worker":"print RECIPE_OK"}}}),
        )
        .unwrap();
    let project = result["project"]["id"].as_str().unwrap().to_owned();
    let terminal = result["terminal"]["id"].as_str().unwrap();
    let file = PathBuf::from(result["project"]["context"].as_str().unwrap()).join("hook-order");
    h.ready(terminal);
    h.until(
        || {
            fs::read_to_string(&file)
                .ok()
                .filter(|s| s == "onetwothree")
        },
        "ordered hooks after failure",
    );
    assert!(
        h.call(json!({"op":"snapshot"})).unwrap()["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["diagnostic"]["exit_status"] == 7
                && d["diagnostic"]["stderr"] == "HOOK_FAILURE")
    );
    assert_eq!(
        h.call(json!({"op":"project-open","project":project}))
            .unwrap()["reattached"],
        true
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "onetwothree");
    h.call(json!({"op":"close","terminal":terminal})).unwrap();
    h.call(json!({"op":"wait","terminal":terminal,"condition":"exited","timeout_ms":5000}))
        .unwrap();
    h.call(json!({"op":"remove","terminal":terminal})).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let socket = h.socket.clone();
            let project = project.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                client::request(&socket, json!({"op":"project-open","project":project})).unwrap()
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(
        results.iter().filter(|r| r["reattached"] == true).count(),
        1
    );
    let opened = results.iter().find(|r| r["terminal"].is_object()).unwrap()["terminal"]["id"]
        .as_str()
        .unwrap();
    let snapshot = h.call(json!({"op":"snapshot"})).unwrap();
    let reopened = snapshot["terminals"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["project_id"] == project)
        .collect::<Vec<_>>();
    assert_eq!(
        reopened
            .iter()
            .map(|t| t["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [opened]
    );
    h.until(
        || {
            fs::read_to_string(&file)
                .ok()
                .filter(|s| s == "onetwothreeonetwothree")
        },
        "one hook run per open",
    );
}

#[test]
fn shutdown_disconnects_partial_clients_and_drains_concurrent_opens() {
    let mut h = Harness::new();
    let main = h.main();
    h.ready(&main);
    let main_pid = h.call(json!({"op":"state","terminal":main})).unwrap()["pid"]
        .as_i64()
        .unwrap() as i32;
    let pid_file = h.directory.join("spawned-pids");
    fs::write(
        h.directory.join("startup/.zshenv"),
        format!(
            "trap '' HUP TERM\nprint -r -- $$ >> '{}'\nsleep 0.1\n",
            pid_file.display()
        ),
    )
    .unwrap();
    let mut slow = UnixStream::connect(&h.socket).unwrap();
    slow.write_all(b"{\"version\":1,\"op\":\"snapshot\"")
        .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(9));
    let opens: Vec<_> = (0..8)
        .map(|i| {
            let socket = h.socket.clone();
            let main = main.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                client::request(
                    &socket,
                    json!({"op":"open","source":main,"name":format!("racing-{i}")}),
                )
            })
        })
        .collect();
    barrier.wait();
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(
        h.call(json!({"op":"shutdown"})).unwrap()["shutting_down"],
        true
    );
    let mut pids = vec![main_pid];
    for open in opens {
        if let Ok(terminal) = open.join().unwrap() {
            pids.push(terminal["pid"].as_i64().unwrap() as i32);
        }
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    while h.child.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "shutdown stuck on partial client or late open"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    if let Ok(contents) = fs::read_to_string(pid_file) {
        pids.extend(contents.lines().map(|line| line.parse::<i32>().unwrap()));
    }
    pids.sort_unstable();
    pids.dedup();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let alive: Vec<_> = pids
            .iter()
            .copied()
            .filter(|pid| unsafe { libc::kill(*pid, 0) == 0 })
            .collect();
        if alive.is_empty() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "managed shells survived shutdown: {alive:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!h.socket.exists());
}

#[test]
fn interrupted_source_records_completion_without_replacing_user_hooks() {
    let h = Harness::new();
    let main = h.main();
    h.ready(&main);
    let execution = h.call(json!({"op":"execute","terminal":main,"source":"print -r -- INTERRUPT_RUNNING; sleep 30"})).unwrap();
    let id = execution["execution_id"].as_str().unwrap();
    h.until(
        || h.screen(&main).contains("INTERRUPT_RUNNING").then_some(()),
        "source began",
    );
    h.call(json!({"op":"input","terminal":main,"key":"ctrl-c"}))
        .unwrap();
    h.ready(&main);
    let state = h.call(json!({"op":"state","terminal":main})).unwrap();
    let completed = state["integration"]["executions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|execution| execution["execution_id"] == id)
        .unwrap();
    assert_eq!(completed["status"], "completed");
    assert_eq!(completed["exit_status"], 130);
}

#[test]
fn concurrent_terminal_admission_counts_exits_and_reuses_removed_capacity() {
    let h = Harness::new();
    let main = h.main();
    h.ready(&main);
    h.call(json!({"op":"config","config":{"terminal_limit":3}}))
        .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(17));
    let opens: Vec<_> = (0..16)
        .map(|index| {
            let socket = h.socket.clone();
            let barrier = barrier.clone();
            let main = main.clone();
            std::thread::spawn(move || {
                barrier.wait();
                client::request(
                    &socket,
                    json!({"op":"open","source":main,"name":format!("capacity-{index}")}),
                )
            })
        })
        .collect();
    barrier.wait();
    let mut admitted = Vec::new();
    for open in opens {
        match open.join().unwrap() {
            Ok(terminal) => admitted.push(terminal["id"].as_str().unwrap().to_owned()),
            Err(error) => assert!(
                error.to_string().starts_with("terminal-limit:"),
                "{error:#}"
            ),
        }
    }
    assert_eq!(admitted.len(), 2);
    assert_eq!(
        h.call(json!({"op":"snapshot"})).unwrap()["terminals"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let removed = admitted.pop().unwrap();
    h.call(json!({"op":"close","terminal":removed})).unwrap();
    h.call(json!({"op":"wait","terminal":removed,"condition":"exited","timeout_ms":5000}))
        .unwrap();
    assert!(
        h.call(json!({"op":"open"}))
            .unwrap_err()
            .to_string()
            .starts_with("terminal-limit:")
    );
    assert!(
        h.call(json!({"op":"project-new","name":"capacity-project"}))
            .unwrap_err()
            .to_string()
            .starts_with("terminal-limit:")
    );
    assert!(
        h.call(json!({"op":"project-open","project":"capacity-project"}))
            .unwrap_err()
            .to_string()
            .starts_with("terminal-limit:")
    );
    h.call(json!({"op":"remove","terminal":removed})).unwrap();
    for _ in 0..4 {
        assert!(
            h.call(json!({"op":"open","cwd":h.directory.join("missing")}))
                .unwrap_err()
                .to_string()
                .starts_with("invalid-directory:")
        );
    }
    let replacement = h.call(json!({"op":"open","source":main})).unwrap();
    admitted.push(replacement["id"].as_str().unwrap().to_owned());
    admitted.push(main);
    h.call(json!({"op":"config","config":{"terminal_limit":1}}))
        .unwrap();
    assert_eq!(
        h.call(json!({"op":"snapshot"})).unwrap()["terminals"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(
        h.call(json!({"op":"open"}))
            .unwrap_err()
            .to_string()
            .starts_with("terminal-limit:")
    );
    for terminal in admitted {
        h.call(json!({"op":"close","terminal":terminal})).unwrap();
        h.call(json!({"op":"wait","terminal":terminal,"condition":"exited","timeout_ms":5000}))
            .unwrap();
        h.call(json!({"op":"remove","terminal":terminal})).unwrap();
    }
    let opened = h
        .call(json!({"op":"project-open","project":"capacity-project"}))
        .unwrap();
    assert!(opened["terminal"]["id"].is_string());
    assert_eq!(
        h.call(json!({"op":"snapshot"})).unwrap()["terminals"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn failed_terminal_preparation_refunds_capacity_without_late_shells() {
    let h = Harness::new();
    h.ready(&h.main());
    h.call(json!({"op":"config","config":{"terminal_limit":2}}))
        .unwrap();
    let integration = h.directory.join("integration");
    let saved = h.directory.join("saved-integration");
    fs::rename(&integration, &saved).unwrap();
    fs::write(&integration, b"not a directory").unwrap();
    for _ in 0..4 {
        let error = h.call(json!({"op":"open"})).unwrap_err();
        assert!(
            !error.to_string().starts_with("terminal-limit:"),
            "{error:#}"
        );
    }
    assert_eq!(
        h.call(json!({"op":"snapshot"})).unwrap()["terminals"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    fs::remove_file(&integration).unwrap();
    fs::rename(&saved, &integration).unwrap();
    let terminal = h.call(json!({"op":"open"})).unwrap();
    h.ready(terminal["id"].as_str().unwrap());
    assert_eq!(
        h.call(json!({"op":"snapshot"})).unwrap()["terminals"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn completed_hooks_and_launch_failures_share_bounded_ordered_diagnostics() {
    let h = Harness::with_daemon_command(|directory, _| {
        let path = directory.join("config/amux");
        fs::create_dir_all(&path).unwrap();
        let hooks: Vec<_> = (0..25).map(|index| {
            json!({"source":format!("printf '%20000s' x; : hook-{index:02} {}", "界".repeat(9000)),"timeout_ms":2000})
        }).collect();
        fs::write(
            path.join("config.json"),
            serde_json::to_vec(&json!({
                "version":1,"config":{"projects_dir":directory.join("projects"),"prefix":"Ctrl-a",
                    "terminal_limit":2,"daemon_start_hooks":hooks},"projects":[]
            }))
            .unwrap(),
        )
        .unwrap();
    });
    let hooks = h.until(
        || {
            let snapshot = h.call(json!({"op":"snapshot"})).ok()?;
            (snapshot["diagnostics"].as_array()?.last()?["diagnostic"]["index"] == 24)
                .then_some(snapshot)
        },
        "all startup hooks completed",
    );
    let diagnostics = hooks["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 20);
    assert_eq!(diagnostics[0]["diagnostic"]["index"], 5);
    for diagnostic in diagnostics {
        assert!(diagnostic["diagnostic"]["source"].as_str().unwrap().len() <= 16 * 1024);
        assert_eq!(diagnostic["diagnostic"]["source_truncated"], true);
        assert!(diagnostic["diagnostic"]["stdout"].as_str().unwrap().len() <= 16 * 1024);
        assert_eq!(diagnostic["diagnostic"]["output_truncated"], true);
    }
    for index in 0..25 {
        let command = format!("launch-{index:02}\0{}", "界".repeat(9000));
        let terminal = h.call(json!({"op":"open","command":command})).unwrap();
        let id = terminal["id"].as_str().unwrap();
        h.ready(id);
        h.until(
            || {
                let snapshot = h.call(json!({"op":"snapshot"})).ok()?;
                (snapshot["diagnostics"].as_array()?.last()?["terminal"] == id).then_some(())
            },
            "launch failure published",
        );
        h.call(json!({"op":"close","terminal":id})).unwrap();
        h.call(json!({"op":"wait","terminal":id,"condition":"exited","timeout_ms":5000}))
            .unwrap();
        h.call(json!({"op":"remove","terminal":id})).unwrap();
    }
    let snapshot = h.call(json!({"op":"snapshot"})).unwrap();
    let diagnostics = snapshot["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 20);
    assert!(
        diagnostics[0]["command"]
            .as_str()
            .unwrap()
            .starts_with("launch-05")
    );
    assert!(
        diagnostics[19]["command"]
            .as_str()
            .unwrap()
            .starts_with("launch-24")
    );
    assert!(serde_json::to_vec(diagnostics).unwrap().len() < 2 * 1024 * 1024);
    for diagnostic in diagnostics {
        assert!(diagnostic["command"].as_str().unwrap().len() <= 16 * 1024);
        assert_eq!(diagnostic["command_truncated"], true);
    }
    let events = h
        .call(json!({"op":"events","after":0,"timeout_ms":0}))
        .unwrap();
    let events = events["events"].as_array().unwrap();
    assert!(
        events
            .windows(2)
            .all(|pair| pair[0]["seq"].as_u64().unwrap() < pair[1]["seq"].as_u64().unwrap())
    );
    assert!(
        events
            .iter()
            .filter(|event| event["kind"] == "launch-failed")
            .all(|event| event["data"]["command"].as_str().unwrap().len() <= 16 * 1024)
    );
}

#[test]
fn hook_enqueue_failures_retain_only_twenty_bounded_command_diagnostics() {
    let h = Harness::with_daemon_command(|directory, _| {
        let path = directory.join("config/amux");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("config.json"), serde_json::to_vec(&json!({
            "version":1,"config":{"projects_dir":directory.join("projects"),"prefix":"Ctrl-a",
                "terminal_limit":2,"daemon_start_hooks":[{"source":"touch hook-running; sleep 300","timeout_ms":300000}]},"projects":[]
        })).unwrap()).unwrap();
    });
    h.until(
        || h.directory.join("hook-running").exists().then_some(()),
        "blocking hook started",
    );
    let hooks: Vec<_> = (0..25)
        .map(|index| {
            json!({
                "source":format!(": queued-{index:02} {}", "界".repeat(9000)),"timeout_ms":2000
            })
        })
        .collect();
    let opened = h
        .call(json!({"op":"project-new","name":"queued-hooks","config":{"hooks":hooks}}))
        .unwrap();
    let project = opened["project"]["id"].as_str().unwrap();
    let mut terminal = opened["terminal"]["id"].as_str().unwrap().to_owned();
    for _ in 0..65 {
        h.call(json!({"op":"close","terminal":terminal})).unwrap();
        h.call(json!({"op":"wait","terminal":terminal,"condition":"exited","timeout_ms":5000}))
            .unwrap();
        h.call(json!({"op":"remove","terminal":terminal})).unwrap();
        let opened = h
            .call(json!({"op":"project-open","project":project}))
            .unwrap();
        terminal = opened["terminal"]["id"].as_str().unwrap().to_owned();
    }
    let snapshot = h.call(json!({"op":"snapshot"})).unwrap();
    let diagnostics = snapshot["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 20);
    for (offset, diagnostic) in diagnostics.iter().enumerate() {
        assert_eq!(diagnostic["index"], offset + 5);
        assert!(
            diagnostic["message"]
                .as_str()
                .unwrap()
                .starts_with("hook-backpressure:")
        );
        assert!(diagnostic["command"].as_str().unwrap().len() <= 16 * 1024);
        assert_eq!(diagnostic["command_truncated"], true);
    }
    assert!(serde_json::to_vec(diagnostics).unwrap().len() < 2 * 1024 * 1024);
}

#[test]
fn concurrent_removals_release_one_admission_slot_without_double_refund() {
    let h = Harness::new();
    let main = h.main();
    h.ready(&main);
    h.call(json!({"op":"config","config":{"terminal_limit":2}}))
        .unwrap();
    let spare = h.call(json!({"op":"open"})).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    h.call(json!({"op":"close","terminal":spare})).unwrap();
    h.call(json!({"op":"wait","terminal":spare,"condition":"exited","timeout_ms":5000}))
        .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(9));
    let requests: Vec<_> = (0..8)
        .map(|index| {
            let socket = h.socket.clone();
            let spare = spare.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let request = if index < 2 {
                    json!({"op":"remove","terminal":spare})
                } else {
                    json!({"op":"open"})
                };
                (index, client::request(&socket, request))
            })
        })
        .collect();
    barrier.wait();
    let mut removed = 0;
    let mut opened = 0;
    for worker in requests {
        let (index, result) = worker.join().unwrap();
        match result {
            Ok(_) if index < 2 => removed += 1,
            Ok(_) => opened += 1,
            Err(error) if index < 2 => {
                assert!(error.to_string().starts_with("not-found:"), "{error:#}")
            }
            Err(error) => assert!(
                error.to_string().starts_with("terminal-limit:"),
                "{error:#}"
            ),
        }
    }
    assert_eq!(removed, 1);
    assert!(opened <= 1);
    if opened == 0 {
        h.call(json!({"op":"open"})).unwrap();
    }
    assert_eq!(
        h.call(json!({"op":"snapshot"})).unwrap()["terminals"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        h.call(json!({"op":"open"}))
            .unwrap_err()
            .to_string()
            .starts_with("terminal-limit:")
    );
}

//! Daily-use coverage drives the built native client through a controlling PTY.
use super::*;
use std::net::Shutdown;
use std::os::unix::net::UnixListener;

// A transparent transport seam, not a protocol peer: every byte still reaches
// the real daemon, broker, original fixture connection and native renderer.
pub(super) struct Relay {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    streams: Arc<parking_lot::Mutex<Vec<UnixStream>>>,
    workers: Arc<parking_lot::Mutex<Vec<std::thread::JoinHandle<()>>>>,
    accept: Option<std::thread::JoinHandle<()>>,
}
impl Relay {
    pub(super) fn new(host: &Host) -> Self {
        let path = host.root.join("native-relay.sock");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let streams = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let workers = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let daemon = host.socket.clone();
        let stop_worker = stop.clone();
        let streams_worker = streams.clone();
        let workers_worker = workers.clone();
        let accept = std::thread::spawn(move || {
            while !stop_worker.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((client, _)) => {
                        // BSD/Darwin accepts inherit the listener's nonblocking
                        // mode. The forwarding workers use blocking io::copy:
                        // WouldBlock must not be mistaken for a client EOF.
                        client.set_nonblocking(false).unwrap();
                        let server = UnixStream::connect(&daemon).unwrap();
                        streams_worker.lock().push(client.try_clone().unwrap());
                        let mut from_client = client.try_clone().unwrap();
                        let mut to_server = server.try_clone().unwrap();
                        let mut from_server = server;
                        let mut to_client = client;
                        let a = std::thread::spawn(move || {
                            let _ = io::copy(&mut from_client, &mut to_server);
                            let _ = to_server.shutdown(Shutdown::Write);
                        });
                        let b = std::thread::spawn(move || {
                            let _ = io::copy(&mut from_server, &mut to_client);
                            let _ = to_client.shutdown(Shutdown::Write);
                        });
                        workers_worker.lock().extend([a, b]);
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("native relay accept: {error}"),
                }
            }
        });
        Self {
            path,
            stop,
            streams,
            workers,
            accept: Some(accept),
        }
    }

    pub(super) fn cut(&self, connection: usize) {
        // Startup also makes short-lived readiness/selector connections. Select
        // among live streams only; never infer native provenance from a global
        // accept count or interpret the forwarded protocol bytes.
        let streams = self.streams.lock();
        let stream = streams
            .iter()
            .filter(|stream| {
                let mut state = libc::pollfd {
                    fd: stream.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                if unsafe { libc::poll(&mut state, 1, 0) } < 0
                    || state.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0
                {
                    return false;
                }
                let mut byte = 0u8;
                let n = unsafe {
                    libc::recv(
                        stream.as_raw_fd(),
                        (&mut byte as *mut u8).cast(),
                        1,
                        libc::MSG_PEEK | libc::MSG_DONTWAIT,
                    )
                };
                n > 0 || n < 0 && io::Error::last_os_error().kind() == io::ErrorKind::WouldBlock
            })
            .nth(connection)
            .expect("original native transport is live");
        stream.shutdown(Shutdown::Both).unwrap();
    }

    pub(super) fn attachment(&self, host: &mut Host, agent: &Agent) -> Attachment {
        let original = std::mem::replace(&mut host.socket, self.path.clone());
        let native = Attachment::new(host, agent);
        host.socket = original;
        native
    }
}
impl Drop for Relay {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(accept) = self.accept.take() {
            accept.join().unwrap();
        }
        for stream in self.streams.lock().iter() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        for worker in self.workers.lock().drain(..) {
            worker.join().unwrap();
        }
        let _ = fs::remove_file(&self.path);
    }
}

fn same_agent(host: &Host, agent: &Agent, exact: &Value, pid: &[u8], daemon: &Value) {
    assert_eq!(host.inspect(agent)["target"], *exact);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    let snapshot = host.call(json!({"op":"snapshot"}));
    assert_eq!(snapshot["daemon_id"], *daemon);
    assert_eq!(snapshot["agent_views"].as_array().unwrap().len(), 1);
}

fn fresh_control(host: &Host, agent: &Agent, native: &mut Attachment, old: &Value) -> Value {
    native.observer();
    native.action(6);
    native.wait("Enter Force control");
    native.send(b"\r");
    native.owner();
    let current = ownership(host, agent)["owner"]["controller_id"].clone();
    assert_ne!(current, *old, "recovery must use fresh native provenance");
    current
}

fn show_question(host: &Host, agent: &Agent, native: &mut Attachment, question: &Value) {
    acceptance::show_interaction(host, agent, native, Some(question));
}

#[derive(Clone, Copy)]
enum NativeProfileCase {
    Defaults,
    LongSettings,
    RejectedEdits,
}

#[test]
fn e06s02_native_profile_launch() {
    native_profile_launch(NativeProfileCase::Defaults);
}

#[test]
fn e06s02_native_long_model_settings_remain_readable_and_input_is_inert() {
    native_profile_launch(NativeProfileCase::LongSettings);
}

#[test]
fn e06s02_native_rejected_edits_never_launch_a_substituted_model() {
    native_profile_launch(NativeProfileCase::RejectedEdits);
}

fn native_profile_launch(case: NativeProfileCase) {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut anchor = host.open("arithmetic");
    host.ready(&mut anchor);
    let root = host.root.join("peer-profile");
    fs::create_dir(&root).unwrap();
    let peer = Path::new(env!("CARGO_BIN_EXE_amux"))
        .parent()
        .unwrap()
        .join("examples/acp_fixture");
    let (model, mode) = match case {
        NativeProfileCase::Defaults => ("fixture/cheap".to_owned(), "e06-settings"),
        NativeProfileCase::LongSettings => (
            format!("fixture/{}", "m".repeat(196)),
            "e06-settings-long-model",
        ),
        NativeProfileCase::RejectedEdits => (
            format!("fixture/{}", "b".repeat(248)),
            "e06-settings-boundary-model",
        ),
    };
    host.call(json!({"op":"config","config":{"acp_profiles":{"fixture":{
        "label":"Daily fixture","command":[peer,root,mode],
        "defaults":{"model":model,"mode":"plan","effort":"low"}}}}}));
    let mut native = native_attachment(&host, &anchor);
    native.send(PREFIX);
    native.send(b"\x1b[B"); // settle prefix; Ctrl-A is now the picker launch action
    native.send(b"\x01");
    native.wait("New ACP session");
    native.send(b"\t"); // Profile field; configured profile follows builtins alphabetically
    // Choose the explicit configured profile by its visible label, not a guessed executable.
    for _ in 0..20 {
        let current = native.text();
        if current.contains("Daily fixture") {
            break;
        }
        let before = current
            .lines()
            .find(|line| line.contains("Profile:"))
            .map(str::to_owned);
        native.send(b"\x1b[C");
        host.until("deliberate profile selection is rendered", || {
            let text = native.text();
            (text
                .lines()
                .find(|line| line.contains("Profile:"))
                .map(str::to_owned)
                != before)
                .then_some(())
        });
    }
    native.wait("Daily fixture");
    native.send(b"\t");
    native.paste("Daily profile launch");
    native.click("Advanced");
    native.click("Effort:");
    native.send(b"\x15");
    native.paste("high");
    native.wait("Effort: high");
    native.send(b"\x15");
    native.wait("Effort: [inherited: low]");
    if matches!(case, NativeProfileCase::RejectedEdits) {
        native.click("Model:");
        native.send(b"\x15");
        native.wait("Model: [inherited:");
        native.paste(&format!("{model}x"));
        rejected_native_launch(&host, &mut native, &root, 3);
        native.send(b"\x1b[H\t\t\t\t");
        native.send(b"\x15"); // A different corrected field cannot erase Model's rejection.
        native.wait("Mode: [inherited: plan]");
        native.paste("plan");
        native.wait("Mode: plan");
        rejected_native_launch(&host, &mut native, &root, 4);
        native.send(b"\x1b[H\t\t\t\t\t\t");
        let mut oversized = r#"{"fixture-detail":"brief"}"#.to_owned();
        oversized.push_str(&" ".repeat(64 * 1024 + 1 - oversized.len()));
        native.paste(&oversized);
        native.wait("Options JSON [rejected]");
        native.send(b"\x1b[Z\x1b[Z\x1b[Z");
        native.send(b"\x15"); // Correct Model, leaving only Options rejected.
        native.wait("Model: [inherited:");
        native.paste(&model);
        native.wait("fixture/");
        rejected_native_launch(&host, &mut native, &root, 3);
        native.send(b"\x1b[H\t\t\t\t\t\t");
        native.send(b"\x15");
        native.wait("Options JSON: ");
        native.paste("{\n\t\"fixture-detail\":\"brief\"\r\n}");
        native.send(b"\x1b[Z\x1b[Z\x1b[Z");
        native.send(b"\x15");
        native.wait("Model: [inherited:");
        native.paste("fixture/\ncheap"); // Invalid scalar must not be rewritten to an offered cheap ID.
        native.wait("Model [rejected]");
        rejected_native_launch(&host, &mut native, &root, 3);
        native.send(b"\x1b[H\t\t\t");
        native.send(b"\x15");
        native.wait("Model: [inherited:");
        native.paste(&model); // Explicit correction: the complete offered 256-byte ID.
        native.wait("fixture/");
    }
    native.click("Launch");
    let view = host.until("native launch publishes exactly one new agent", || {
        host.call(json!({"op":"snapshot"}))["agent_views"]
            .as_array()?
            .iter()
            .find(|view| view["name"] == "Daily profile launch" && view["lifecycle"] == "ready")
            .cloned()
    });
    let mut agent = Agent {
        opened: Value::Null,
        target: agent_target_from_view(&view),
        root,
        controller_id: ownership_for_view(&host, &view)["owner"]["controller_id"].clone(),
    };
    // Ownership/cursors can still belong to the previous ready anchor while the
    // asynchronous native launch finishes. Fence the new rendered target first.
    native.wait_header("Daily profile launch");
    native.owner();
    native.details();
    native.wait(agent.target["session_id"].as_str().unwrap());
    let projection = host.inspect(&agent);
    native.projected(&host, projection["seq"].as_u64().unwrap());
    if !matches!(case, NativeProfileCase::Defaults) {
        let original_owner = ownership(&host, &agent)["owner"].clone();
        let original_pid = fs::read(agent.root.join("peer.pid")).unwrap();
        native.send(b"\x1b[21~"); // F10: wrapped, read-only settings.
        native.wait("ACP settings (read-only)");
        native.wait("Mode metadata is not a permission guarantee.");
        let surface = native.text();
        let (reported, requested) = surface.split_once("Requested").unwrap();
        for section in [reported, requested] {
            let unwrapped = section
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>();
            assert!(unwrapped.contains("mode:plan"), "{surface}");
            assert!(unwrapped.contains("effort:low"), "{surface}");
            assert!(unwrapped.contains(&model), "{surface}");
        }
        native.resize(80, 12);
        host.until("smaller native viewport has later settings rows", || {
            let surface = native.text();
            (surface.contains("ACP settings (read-only)")
                && !surface.contains("Mode metadata is not a permission guarantee."))
            .then_some(())
        });
        for _ in 0..64 {
            if native
                .text()
                .contains("Mode metadata is not a permission guarantee.")
            {
                break;
            }
            let before = native.text();
            native.send(b"\x1b[6~");
            host.until("Details advances toward the complete warning", || {
                (native.text() != before).then_some(())
            });
        }
        native.wait("Mode metadata is not a permission guarantee.");
        native.send(b"\x1b[H"); // Home returns to current settings.
        native.paste("add 900 1");
        native.send(b"\r\x1b[19~\x03"); // Enter, F8, Ctrl-C stay local to the read-only view.
        native.resize(COLS, ROWS);
        native.send(b"\x1b[H");
        native.wait("ACP | ready");
        native.send(b"\x1b[21~");
        host.until(
            "Details closed without submitting application input",
            || (!native.text().contains("ACP settings (read-only)")).then_some(()),
        );
        assert_eq!(ownership(&host, &agent)["owner"], original_owner);
        assert_eq!(host.inspect(&agent)["target"], agent.target);
        assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), original_pid);
    }
    let configuration = host.inspect(&agent)["configuration"].clone();
    assert_eq!(configuration["requested"]["model"], model);
    assert_eq!(configuration["requested"]["effort"], "low");
    assert_eq!(configuration["reported"]["model"]["current"], model);
    assert_eq!(configuration["reported"]["mode"]["current"], "plan");
    assert_eq!(configuration["reported"]["effort"]["current"], "low");
    if matches!(case, NativeProfileCase::RejectedEdits) {
        assert_eq!(
            configuration["requested"]["options"]["fixture-detail"],
            "brief"
        );
    }
    assert_eq!(ownership(&host, &agent)["owner"]["role"], "attachment");
    let work = native_prompt(&host, &agent, &mut native, "add 17 25");
    let completed = host.completed(&agent, &work["work_id"]);
    assert_eq!(completed["controller_id"], agent.controller_id);
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(agent.effect()["session_id"], agent.target["session_id"]);
    assert_eq!(
        agent.effect()["commands"],
        1,
        "settings-view input must never be submitted or replayed"
    );
    assert_eq!(
        host.call(json!({"op":"snapshot"}))["agent_views"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    force(&host, &mut anchor);
    host.close(&anchor);
    finish(host, &mut agent, &mut native, &mut sentinel);
}

fn rejected_native_launch(host: &Host, native: &mut Attachment, root: &Path, field: usize) {
    native.send(&[b'\t'; 7][..7 - field]);
    native.send(b"\r");
    host.until(
        "rejected edits block launch until deliberately corrected",
        || {
            let snapshot = host.call(json!({"op":"snapshot"}));
            if let Some(view) = snapshot["agent_views"]
                .as_array()
                .unwrap()
                .iter()
                .find(|view| view["name"] == "Daily profile launch" && view["lifecycle"] == "ready")
            {
                panic!(
                    "invalid native edit launched a substituted configuration: {}",
                    view["configuration"]
                );
            }
            native
                .text()
                .contains("Correct rejected launch input")
                .then_some(())
        },
    );
    assert!(
        !root.join("peer.pid").exists(),
        "invalid edit must not start the selected process"
    );
}

#[test]
fn e06s02_native_profile_launch_preserves_delimited_inherited_options() {
    native_delimited_options(false);
}

#[test]
fn e06s02_native_profile_launch_explicit_delimited_options() {
    native_delimited_options(true);
}

fn native_delimited_options(explicit: bool) {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut anchor = host.open("arithmetic");
    host.ready(&mut anchor);
    let root = host.root.join("peer-delimited-profile");
    fs::create_dir(&root).unwrap();
    let peer = Path::new(env!("CARGO_BIN_EXE_amux"))
        .parent()
        .unwrap()
        .join("examples/acp_fixture");
    let defaults = json!({"options":{"fixture,detail=level":if explicit {
        "full"
    } else {
        "brief,exact=value"
    }}});
    host.call(json!({"op":"config","config":{"acp_profiles":{"delimited":{
        "label":"Delimited fixture","command":[peer,root,"e06-settings-delimited"],
        "defaults":defaults}}}}));
    let daemon = host.call(json!({"op":"snapshot"}))["daemon_id"].clone();
    let mut native = native_attachment(&host, &anchor);
    native.send(PREFIX);
    native.send(b"\x1b[B");
    native.send(b"\x01");
    native.wait("New ACP session");
    native.send(b"\t");
    for _ in 0..20 {
        let current = native.text();
        if current.contains("Delimited fixture") {
            break;
        }
        let before = current
            .lines()
            .find(|line| line.contains("Profile:"))
            .map(str::to_owned);
        native.send(b"\x1b[C");
        host.until("delimited profile selection is rendered", || {
            let text = native.text();
            (text
                .lines()
                .find(|line| line.contains("Profile:"))
                .map(str::to_owned)
                != before)
                .then_some(())
        });
    }
    native.wait("Delimited fixture");
    native.send(b"\t");
    native.paste("Inherited delimited launch");
    if explicit {
        native.click("Advanced");
        native.click("Options JSON");
        native.paste(r#"{"fixture,detail=level":"brief,exact=value"}"#);
    }
    native.click("Launch");
    let view = host.until(
        "native launch applies exact delimiter-containing settings",
        || {
            host.call(json!({"op":"snapshot"}))["agent_views"]
                .as_array()?
                .iter()
                .find(|view| {
                    view["name"] == "Inherited delimited launch" && view["lifecycle"] == "ready"
                })
                .cloned()
        },
    );
    let mut agent = Agent {
        opened: Value::Null,
        target: agent_target_from_view(&view),
        root,
        controller_id: ownership_for_view(&host, &view)["owner"]["controller_id"].clone(),
    };
    let exact = agent.target.clone();
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    native.wait_header("Inherited delimited launch");
    native.owner();
    native.details();
    native.wait(agent.target["session_id"].as_str().unwrap());
    let projection = host.inspect(&agent);
    native.projected(&host, projection["seq"].as_u64().unwrap());
    let configuration = projection["configuration"].clone();
    assert_eq!(configuration["state"], "applied");
    assert_eq!(
        configuration["requested"],
        json!({"options":{"fixture,detail=level":"brief,exact=value"}})
    );
    let option = configuration["reported"]["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["id"] == "fixture,detail=level")
        .unwrap();
    assert_eq!(option["current"], "brief,exact=value");
    assert_eq!(ownership(&host, &agent)["owner"]["role"], "attachment");
    let work = native_prompt(&host, &agent, &mut native, "add 17 25");
    let completed = host.completed(&agent, &work["work_id"]);
    assert_eq!(completed["controller_id"], agent.controller_id);
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(agent.effect()["session_id"], exact["session_id"]);
    assert_eq!(host.inspect(&agent)["target"], exact);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    let snapshot = host.call(json!({"op":"snapshot"}));
    assert_eq!(snapshot["daemon_id"], daemon);
    assert_eq!(snapshot["agent_views"].as_array().unwrap().len(), 2);
    force(&host, &mut anchor);
    host.close(&anchor);
    finish(host, &mut agent, &mut native, &mut sentinel);
}

fn agent_target_from_view(view: &Value) -> Value {
    json!({"kind":"acp","id":view["id"],"instance_id":view["instance_id"],
        "connection_generation":view["connection_generation"],"session_id":view["session_id"]})
}
fn ownership_for_view(host: &Host, view: &Value) -> Value {
    host.view(view["id"].as_str().unwrap()).unwrap()["ownership"].clone()
}

#[test]
fn e06s03_native_reconnect() {
    let mut host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("s04-complete");
    host.ready(&mut agent);
    let exact = agent.target.clone();
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let daemon = host.call(json!({"op":"snapshot"}))["daemon_id"].clone();
    host.call(json!({"op":"config","config":{"prefix":"Ctrl-a"}}));
    handle(&host, &agent.opened["control"], "release");
    let relay = Relay::new(&host);
    let mut native = relay.attachment(&mut host, &agent);
    native.owner();
    let original_owner = ownership(&host, &agent)["owner"]["controller_id"].clone();
    let work = native_prompt(&host, &agent, &mut native, "add 17 25");
    let form = pending(&host, &agent, "form");
    show_question(&host, &agent, &mut native, &form);
    native.send(b"\x1b[14~9"); // deliberately dirty original form, not submitted
    relay.cut(1); // read-only observation loss alone must fence the authority socket
    native.wait("Reconnected");
    native.observer();
    same_agent(&host, &agent, &exact, &pid, &daemon);
    assert_eq!(
        pending(&host, &agent, "form")["interaction_id"],
        form["interaction_id"]
    );
    assert!(host.work(&agent, &work["work_id"])["outcome"].is_null());
    assert!(!agent.root.join("effect.json").exists());
    let fresh = fresh_control(&host, &agent, &mut native, &original_owner);
    show_question(&host, &agent, &mut native, &form);
    native.send(b"\x1b[14~");
    native.wait("UNSET (F4)");
    native.paste("7");
    native.wait("local value");
    native.ready_action("Review (F5)");
    native.send(ACCEPT_FORM);
    native.wait("Submit (F5)"); // observe ready review after queued input drains
    assert_eq!(
        pending(&host, &agent, "form")["interaction_id"],
        form["interaction_id"]
    );
    native.send(ACCEPT_FORM);
    let permission = pending(&host, &agent, "permission");
    show_question(&host, &agent, &mut native, &permission);
    native.send(b"\x1b[B\x1b[A");
    native.wait("Local permission highlight");
    native.ready_action("[Allow this invocation [allow_once]]");
    native.send(b"\r"); // exact offered permission, original pending work
    host.completed(&agent, &work["work_id"]);
    assert_eq!(agent.effect()["value"], 49);
    assert_eq!(agent.effect()["commands"], 1);
    same_agent(&host, &agent, &exact, &pid, &daemon);
    // Cut the new original authority connection independently. No queued prompt
    // or force/claim can migrate onto the next worker or old reply channel.
    native.click("Prompt");
    native.paste("add 900 1");
    relay.cut(0);
    native.wait("Reconnected");
    native.observer();
    native.send(b"\r\x1b[120;1:2u");
    native.paste("add 700 3\r\n");
    native.send(b"\x1b[<0;16;14M\x1b[<32;17;14M\x1b[<0;17;14m");
    fresh_control(&host, &agent, &mut native, &fresh);
    assert_eq!(agent.effect()["commands"], 1);
    same_agent(&host, &agent, &exact, &pid, &daemon);
    drop(relay); // the attachment remains safely disconnected until quit
    native.wait("Disconnected");
    finish(host, &mut agent, &mut native, &mut sentinel);
}

#[test]
fn e06s03_paused_attachment_discards_draft_without_relaunch() {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("hold-prompt");
    host.ready(&mut agent);
    let exact = agent.target.clone();
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let daemon = host.call(json!({"op":"snapshot"}))["daemon_id"].clone();
    let mut native = native_attachment(&host, &agent);
    let old = ownership(&host, &agent)["owner"]["controller_id"].clone();
    let original = native_prompt(&host, &agent, &mut native, "add 17 25");
    host.barrier(&agent, "prompt");
    native.paste("add 900 1");
    native.wait("add 900 1");
    let group = -(native.child.id() as i32);
    assert_eq!(unsafe { libc::kill(group, libc::SIGSTOP) }, 0);
    // The original agent/daemon continue and settle the admitted work while the
    // real attachment process group is stopped; only the client sleeps.
    agent.release("prompt");
    let completed = host.completed(&agent, &original["work_id"]);
    s03_same_work(&completed, &original);
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(agent.effect()["commands"], 1);
    native.send(b"\r"); // held while the actual native client cannot read
    std::thread::sleep(Duration::from_millis(2300));
    assert_eq!(unsafe { libc::kill(group, libc::SIGCONT) }, 0);
    native.wait("Reconnected");
    fresh_control(&host, &agent, &mut native, &old);
    let work = native_prompt(&host, &agent, &mut native, "add 1 2");
    host.completed(&agent, &work["work_id"]);
    assert_eq!(agent.effect()["value"], 45);
    assert_eq!(agent.effect()["commands"], 2);
    same_agent(&host, &agent, &exact, &pid, &daemon);
    finish(host, &mut agent, &mut native, &mut sentinel);
}

#[test]
fn e06s03_ended_target_never_substitutes_same_name() {
    let mut host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("arithmetic");
    host.ready(&mut agent);
    host.call(json!({"op":"config","config":{"prefix":"Ctrl-a"}}));
    handle(&host, &agent.opened["control"], "release");
    let relay = Relay::new(&host);
    let mut native = relay.attachment(&mut host, &agent);
    native.owner();
    force(&host, &mut agent);
    host.close(&agent);
    let mut replacement = host.open("arithmetic");
    host.ready(&mut replacement);
    assert_ne!(replacement.target["id"], agent.target["id"]);
    relay.cut(1);
    native.wait("Ended");
    native.send(b"\r");
    native.paste("add 900 1");
    assert!(
        host.inspect(&replacement)["work"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(!replacement.root.join("effect.json").exists());
    drop(relay);
    finish(host, &mut replacement, &mut native, &mut sentinel);
}

#[test]
fn e06s03_daemon_loss_quits_without_resurrection() {
    let mut host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("arithmetic");
    host.ready(&mut agent);
    let mut native = native_attachment(&host, &agent);
    host.call(json!({"op":"shutdown"}));
    let deadline = Instant::now() + DEADLINE;
    while host.daemon.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "daemon shutdown");
        std::thread::sleep(Duration::from_millis(5));
    }
    native.wait("Disconnected");
    native.send(b"\r");
    native.paste("add 900 1");
    native.detach();
    assert!(
        !host.socket.exists(),
        "native recovery must not ensure/spawn a daemon"
    );
    host.reaped(&agent, "peer.pid");
    assert!(!agent.root.join("fixture-fallback").exists());
    sentinel.alive();
    let root = host.root.clone();
    fs::remove_dir_all(&root).unwrap();
    drop(host);
    assert!(!root.exists());
    sentinel.alive();
}

#[test]
fn e06s03_restarted_daemon_is_not_original_attachment() {
    let mut host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut original = host.open("arithmetic");
    host.ready(&mut original);
    let daemon = host.call(json!({"op":"snapshot"}))["daemon_id"].clone();
    let mut native = native_attachment(&host, &original);
    host.call(json!({"op":"shutdown"}));
    let deadline = Instant::now() + DEADLINE;
    while host.daemon.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "original daemon shutdown");
        std::thread::sleep(Duration::from_millis(5));
    }
    host.reaped(&original, "peer.pid");
    native.wait("Disconnected");
    let log = fs::File::create(host.root.join("daemon.log")).unwrap();
    host.daemon = Command::new(env!("CARGO_BIN_EXE_amux"))
        .arg("--socket")
        .arg(&host.socket)
        .arg("daemon")
        .arg("--projects-dir")
        .arg(host.root.join("projects"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("HOME", host.root.join("home"))
        .env("SHELL", "/bin/zsh")
        .env("USER", "amux-fixture")
        .env("TERM", "xterm-256color")
        .env("ZDOTDIR", host.root.join("startup"))
        .env("XDG_CONFIG_HOME", host.root.join("config"))
        .env("XDG_RUNTIME_DIR", host.root.join("runtime"))
        .env("AMUX_RUNTIME_DIR", host.root.join("runtime"))
        .env("AMUX_SOCKET", &host.socket)
        .current_dir(&host.root)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let replacement_daemon = host.until("explicit fixture daemon replacement", || {
        exchange(&host.socket, json!({"op":"snapshot"}))
            .ok()
            .filter(|value| value["ok"] == true)
            .map(|value| value["result"]["daemon_id"].clone())
    });
    assert_ne!(replacement_daemon, daemon);
    let mut replacement = host.open("arithmetic");
    host.ready(&mut replacement);
    native.wait("Ended");
    native.send(b"\r");
    native.paste("add 900 1");
    assert_ne!(replacement.target, original.target);
    assert!(
        host.inspect(&replacement)["work"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(!replacement.root.join("effect.json").exists());
    finish(host, &mut replacement, &mut native, &mut sentinel);
}

#[test]
fn e06s03_old_claim_offer_never_migrates_to_fresh_controller() {
    let mut host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("arithmetic");
    host.ready(&mut agent);
    host.call(json!({"op":"config","config":{"prefix":"Ctrl-a"}}));
    handle(&host, &agent.opened["control"], "release");
    let relay = Relay::new(&host);
    let mut native = relay.attachment(&mut host, &agent);
    native.owner();
    let original = ownership(&host, &agent)["owner"]["controller_id"].clone();
    force(&host, &mut agent);
    native.observer();
    native.action(12); // deliberate cooperative claim on the old original socket
    host.until("old native cooperative reservation", || {
        let state = ownership(&host, &agent);
        (state["reservation"]["claim_id"].is_string()
            && state["reservation"]["requester_id"] == original)
            .then_some(())
    });
    relay.cut(1);
    native.wait("Reconnected");
    native.observer();
    handle(&host, &agent.opened["control"], "release");
    native.send(b"\x1b[I");
    host.until(
        "fresh observer publishes current view without acquiring old offer",
        || {
            let state = ownership(&host, &agent);
            state["views"]
                .as_array()?
                .iter()
                .any(|view| {
                    view["controller_id"] != original
                        && view["connected"] == true
                        && view["reported"] == true
                        && view["focused"] == true
                })
                .then_some(())
        },
    );
    assert!(ownership(&host, &agent)["owner"].is_null());
    assert!(host.inspect(&agent)["work"].as_array().unwrap().is_empty());
    fresh_control(&host, &agent, &mut native, &original);
    let work = native_prompt(&host, &agent, &mut native, "add 17 25");
    host.completed(&agent, &work["work_id"]);
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(agent.effect()["commands"], 1);
    drop(relay);
    native.wait("Disconnected");
    finish(host, &mut agent, &mut native, &mut sentinel);
}

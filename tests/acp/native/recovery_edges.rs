//! Faults are placed between actual client/daemon messages, never substituted
//! Regressions exercise the built native client and live synthetic peer.
use super::*;
use std::net::Shutdown;
use std::os::unix::net::UnixListener;
use std::sync::atomic::AtomicUsize;

#[derive(Clone, Copy, PartialEq)]
enum Fault {
    ClaimPause,
    LaunchObservation,
    LaunchQueuedObservation,
    LaunchQueuedAuthority,
    RecoverySetup,
    RecoveryRegistration,
    BootstrapObservation,
    PickerResize,
}

struct Registration {
    handle: Value,
    id: Value,
    socket: UnixStream,
}
#[derive(Default)]
struct Gates {
    stop: AtomicBool,
    original: AtomicBool,
    armed: AtomicBool,
    native_held: AtomicBool,
    ui_held: AtomicBool,
    release_native: AtomicBool,
    release_ui: AtomicBool,
    native_closed: AtomicBool,
    after_ack: AtomicBool,
    cut: AtomicBool,
    claims: AtomicUsize,
    opens: AtomicUsize,
    launched: parking_lot::Mutex<Value>,
    picker_target: parking_lot::Mutex<Value>,
    setup_drops_remaining: AtomicUsize,
    setup_drops: AtomicUsize,
    registrations: parking_lot::Mutex<Vec<Registration>>,
}
impl Gates {
    fn wait(&self, flag: &AtomicBool) -> bool {
        let deadline = Instant::now() + DEADLINE;
        while !flag.load(Ordering::Acquire) {
            if self.stop.load(Ordering::Acquire) || Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        true
    }
}
struct PhaseRelay {
    path: PathBuf,
    gates: Arc<Gates>,
    streams: Arc<parking_lot::Mutex<Vec<UnixStream>>>,
    workers: Arc<parking_lot::Mutex<Vec<std::thread::JoinHandle<()>>>>,
    accept: Option<std::thread::JoinHandle<()>>,
}
impl PhaseRelay {
    fn new(host: &Host, fault: Fault) -> Self {
        let path = host.root.join("phase-relay.sock");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let gates = Arc::new(Gates::default());
        let streams = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let workers = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let state = gates.clone();
        let connections = streams.clone();
        let tasks = workers.clone();
        let daemon = host.socket.clone();
        let accept = std::thread::spawn(move || {
            while !state.stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((client, _)) => {
                        client.set_nonblocking(false).unwrap();
                        let server = UnixStream::connect(&daemon).unwrap();
                        connections
                            .lock()
                            .extend([client.try_clone().unwrap(), server.try_clone().unwrap()]);
                        let state = state.clone();
                        tasks.lock().push(std::thread::spawn(move || {
                            forward(client, server, state, fault);
                        }));
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("phase relay accept: {error}"),
                }
            }
        });
        Self {
            path,
            gates,
            streams,
            workers,
            accept: Some(accept),
        }
    }
    fn attachment(&self, host: &mut Host, agent: &Agent) -> Attachment {
        let socket = std::mem::replace(&mut host.socket, self.path.clone());
        let native = Attachment::new(host, agent);
        host.socket = socket;
        native
    }
}
fn forward(client: UnixStream, server: UnixStream, state: Arc<Gates>, fault: Fault) {
    let mut client = BufReader::new(client);
    let mut server = BufReader::new(server);
    let mut request = String::new();
    let mut response = String::new();
    let mut native = false;
    let mut previous_read = false;
    let mut previous_open = false;
    loop {
        request.clear();
        if !matches!((&mut client).take(8 * 1024 * 1024).read_line(&mut request), Ok(n) if n > 0)
            || !request.ends_with('\n')
        {
            break;
        }
        let Ok(message) = serde_json::from_str::<Value>(&request) else {
            break;
        };
        if message["op"] == "controller-create" && message["role"] == "attachment" {
            native = !state.original.swap(true, Ordering::AcqRel);
        }
        if native && previous_open {
            // The worker has parsed and published the launch receipt before it
            // sends this next request. The UI is still inside its earlier read.
            state.after_ack.store(true, Ordering::Release);
            if fault == Fault::LaunchQueuedAuthority {
                state.cut.store(true, Ordering::Release);
                break;
            }
        }
        if message["op"] == "ownership-claim" {
            state.claims.fetch_add(1, Ordering::AcqRel);
        }
        if message["op"] == "acp-open" {
            state.opens.fetch_add(1, Ordering::AcqRel);
        }
        if fault == Fault::LaunchObservation
            && !native
            && message["op"] == "snapshot"
            && state.ui_held.load(Ordering::Acquire)
            && state.after_ack.load(Ordering::Acquire)
            && !state.cut.swap(true, Ordering::AcqRel)
        {
            // This is the UI's receipt-following snapshot, not an earlier
            // periodic observation which happened to race the worker reply.
            break;
        }
        if fault == Fault::RecoverySetup
            && message["op"] == "snapshot"
            && state.original.load(Ordering::Acquire)
            && state
                .setup_drops_remaining
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |left| {
                    left.checked_sub(1)
                })
                .is_ok()
        {
            state.setup_drops.fetch_add(1, Ordering::AcqRel);
            break;
        }
        if server.get_mut().write_all(request.as_bytes()).is_err() {
            break;
        }
        response.clear();
        if !matches!((&mut server).take(8 * 1024 * 1024).read_line(&mut response), Ok(n) if n > 0)
            || !response.ends_with('\n')
        {
            break;
        }
        if fault == Fault::PickerResize
            && !native
            && message["op"] == "resize"
            && state.armed.load(Ordering::Acquire)
            && !state.cut.swap(true, Ordering::AcqRel)
        {
            // Forwarded real selected-target resize and read its actual reply.
            // Lose observation before the client can consume that reply.
            *state.picker_target.lock() = message["terminal"].clone();
            break;
        }
        if message["op"] == "controller-create" && message["role"] == "attachment" {
            let reply: Value = serde_json::from_str(&response).unwrap();
            if reply["ok"] == true {
                state.registrations.lock().push(Registration {
                    handle: reply["result"]["controller"].clone(),
                    id: reply["result"]["controller_id"].clone(),
                    socket: client.get_ref().try_clone().unwrap(),
                });
            }
            if fault == Fault::RecoveryRegistration
                && !native
                && state.armed.load(Ordering::Acquire)
                && !state.native_held.swap(true, Ordering::AcqRel)
                && !state.wait(&state.release_native)
            {
                break;
            }
        }
        if fault == Fault::BootstrapObservation
            && state.armed.load(Ordering::Acquire)
            && !native
            && message["op"] == "snapshot"
            && !state.ui_held.swap(true, Ordering::AcqRel)
            && !state.wait(&state.release_ui)
        {
            break;
        }
        if fault == Fault::ClaimPause && state.armed.load(Ordering::Acquire) {
            if native
                && message["op"] == "snapshot"
                && !previous_read
                && !state.native_held.swap(true, Ordering::AcqRel)
            {
                if !state.wait(&state.release_native) {
                    break;
                }
            } else if !native
                && state.native_held.load(Ordering::Acquire)
                && !state.ui_held.swap(true, Ordering::AcqRel)
                && !state.wait(&state.release_ui)
            {
                break;
            }
        }
        if matches!(
            fault,
            Fault::LaunchObservation
                | Fault::LaunchQueuedObservation
                | Fault::LaunchQueuedAuthority
        ) {
            if native && message["op"] == "acp-open" {
                let reply: Value = serde_json::from_str(&response).unwrap();
                assert_eq!(
                    reply["ok"], true,
                    "real launch must succeed before the fault"
                );
                *state.launched.lock() = reply["result"]["target"].clone();
                state.native_held.store(true, Ordering::Release);
                if !state.wait(&state.ui_held) {
                    break;
                }
            } else if !native
                && message["op"] == "snapshot"
                && state.native_held.load(Ordering::Acquire)
                && !state.ui_held.swap(true, Ordering::AcqRel)
            {
                if !state.wait(&state.after_ack) {
                    break;
                }
                if fault == Fault::LaunchQueuedObservation {
                    state.cut.store(true, Ordering::Release);
                    break;
                }
                if fault == Fault::LaunchQueuedAuthority && !state.wait(&state.native_closed) {
                    break;
                }
            }
        }
        if client.get_mut().write_all(response.as_bytes()).is_err() {
            break;
        }
        previous_read = message["op"] == "control-handle" && message["method"] == "read";
        previous_open = message["op"] == "acp-open";
    }
    let _ = client.get_ref().shutdown(Shutdown::Both);
    let _ = server.get_ref().shutdown(Shutdown::Both);
    if native {
        state.native_closed.store(true, Ordering::Release);
    }
}
impl Drop for PhaseRelay {
    fn drop(&mut self) {
        self.gates.stop.store(true, Ordering::Release);
        self.accept.take().unwrap().join().unwrap();
        for stream in self.streams.lock().iter() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        for worker in self.workers.lock().drain(..) {
            worker.join().unwrap();
        }
        let _ = fs::remove_file(&self.path);
    }
}
// The child remains unreaped and stopped, so its original process-group identity
// cannot be recycled. Resume also on assertion unwind before driver cleanup.
struct Paused<'a>(&'a mut Attachment);
impl<'a> Paused<'a> {
    fn new(native: &'a mut Attachment) -> Self {
        assert_eq!(
            unsafe { libc::kill(-(native.child.id() as i32), libc::SIGSTOP) },
            0
        );
        Self(native)
    }
}
impl Drop for Paused<'_> {
    fn drop(&mut self) {
        if self.0.child.try_wait().unwrap().is_none() {
            assert_eq!(
                unsafe { libc::kill(-(self.0.child.id() as i32), libc::SIGCONT) },
                0
            );
        }
    }
}

#[test]
fn e06s03_pause_during_claim_observation_never_reserves_control() {
    let mut host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("arithmetic");
    host.ready(&mut agent);
    host.call(json!({"op":"config","config":{"prefix":"Ctrl-a"}}));
    let owner = ownership(&host, &agent)["owner"].clone();
    let exact = agent.target.clone();
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let relay = PhaseRelay::new(&host, Fault::ClaimPause);
    let mut native = relay.attachment(&mut host, &agent);
    native.observer();
    relay.gates.armed.store(true, Ordering::Release);
    native.action(12);
    host.until("compound claim read and UI observation both held", || {
        (relay.gates.native_held.load(Ordering::Acquire)
            && relay.gates.ui_held.load(Ordering::Acquire))
        .then_some(())
    });
    let paused = Paused::new(&mut native);
    std::thread::sleep(Duration::from_millis(2100));
    relay.gates.release_native.store(true, Ordering::Release);
    drop(paused);
    host.until(
        "stale worker disconnects or incorrectly emits its claim",
        || {
            (relay.gates.native_closed.load(Ordering::Acquire)
                || relay.gates.claims.load(Ordering::Acquire) > 0)
                .then_some(())
        },
    );
    assert_eq!(
        relay.gates.claims.load(Ordering::Acquire),
        0,
        "a stale compound claim must never reach the daemon"
    );
    assert_eq!(ownership(&host, &agent)["owner"], owner);
    assert!(ownership(&host, &agent)["reservation"].is_null());
    relay.gates.release_ui.store(true, Ordering::Release);
    native.wait("Reconnected");
    native.observer();
    let work = host.prompt(&agent, "after-paused-claim", "add 17 25");
    host.completed(&agent, &work["work_id"]);
    native.projected(&host, host.inspect(&agent)["seq"].as_u64().unwrap());
    assert_eq!(host.inspect(&agent)["target"], exact);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(agent.effect()["value"], 42);
    finish(host, &mut agent, &mut native, &mut sentinel);
}

fn launch_survives(fault: Fault) {
    let mut host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut anchor = host.open("arithmetic");
    host.ready(&mut anchor);
    let root = host.root.join("peer-launched");
    fs::create_dir(&root).unwrap();
    let peer = Path::new(env!("CARGO_BIN_EXE_amux"))
        .parent()
        .unwrap()
        .join("examples/acp_fixture");
    host.call(
        json!({"op":"config","config":{"prefix":"Ctrl-a","acp_profiles":{"fixture":{
        "label":"Loss fixture","command":[peer,root,"arithmetic"]}}}}),
    );
    handle(&host, &anchor.opened["control"], "release");
    let relay = PhaseRelay::new(&host, fault);
    let mut native = relay.attachment(&mut host, &anchor);
    native.owner();
    native.send(PREFIX);
    native.send(b"\x1b[B");
    native.send(b"\x01");
    native.wait("New ACP session");
    native.send(b"\t");
    for _ in 0..20 {
        let before = native.text();
        if before.contains("Loss fixture") {
            break;
        }
        let old = before
            .lines()
            .find(|line| line.contains("Profile:"))
            .map(str::to_owned);
        native.send(b"\x1b[C");
        host.until("profile selection rendered", || {
            (native
                .text()
                .lines()
                .find(|line| line.contains("Profile:"))
                .map(str::to_owned)
                != old)
                .then_some(())
        });
    }
    native.wait("Loss fixture");
    native.send(b"\t");
    native.paste("Acknowledged new agent");
    native.click("Launch");
    host.until("causally ordered launch transport fault occurred", || {
        relay.gates.cut.load(Ordering::Acquire).then_some(())
    });
    native.wait("Reconnected");
    let target = relay.gates.launched.lock().clone();
    native.details();
    native.wait(target["id"].as_str().unwrap());
    native.observer();
    let mut agent = Agent {
        opened: Value::Null,
        target,
        root,
        controller_id: Value::Null,
    };
    host.ready(&mut agent);
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    force(&host, &mut agent);
    let work = host.prompt(&agent, "after-launch-loss", "add 17 25");
    host.completed(&agent, &work["work_id"]);
    native.projected(&host, host.inspect(&agent)["seq"].as_u64().unwrap());
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    assert_eq!(relay.gates.opens.load(Ordering::Acquire), 1);
    assert_eq!(
        host.call(json!({"op":"snapshot"}))["agent_views"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        ownership(&host, &agent)["owner"]["controller_id"],
        agent.controller_id
    );
    assert!(ownership(&host, &agent)["reservation"].is_null());
    force(&host, &mut anchor);
    host.close(&anchor);
    finish(host, &mut agent, &mut native, &mut sentinel);
}

#[test]
fn e06s02_acknowledged_launch_survives_followup_snapshot_loss() {
    launch_survives(Fault::LaunchObservation);
}

#[test]
fn e06s02_queued_launch_survives_observation_loss_before_consumption() {
    launch_survives(Fault::LaunchQueuedObservation);
}

#[test]
fn e06s02_queued_launch_survives_authority_loss_before_consumption() {
    launch_survives(Fault::LaunchQueuedAuthority);
}

#[test]
fn e06s03_first_session_established_while_disconnected_is_not_replacement() {
    let mut host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("hold-session");
    host.barrier(&agent, "session");
    host.call(json!({"op":"config","config":{"prefix":"Ctrl-a"}}));
    let original = agent.target.clone();
    assert!(original["session_id"].is_null());
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let relay = super::daily_use::Relay::new(&host);
    let mut native = relay.attachment(&mut host, &agent);
    native.details();
    native.wait("ACP | starting");
    let paused = Paused::new(&mut native);
    relay.cut(0);
    relay.cut(0); // both original transports, no fresh registration while stopped
    agent.release("session");
    host.ready(&mut agent);
    assert!(!agent.target["session_id"].is_null());
    drop(paused);
    native.wait("Reconnected");
    native.wait("ACP · ready");
    native.details();
    native.wait(agent.target["session_id"].as_str().unwrap());
    native.observer();
    let work = host.prompt(&agent, "first-session-recovered", "add 17 25");
    host.completed(&agent, &work["work_id"]);
    native.projected(&host, host.inspect(&agent)["seq"].as_u64().unwrap());
    for field in ["id", "instance_id", "connection_generation"] {
        assert_eq!(agent.target[field], original[field]);
    }
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(
        ownership(&host, &agent)["owner"]["controller_id"],
        agent.controller_id
    );
    assert!(ownership(&host, &agent)["reservation"].is_null());
    assert_eq!(
        host.call(json!({"op":"snapshot"}))["agent_views"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    finish(host, &mut agent, &mut native, &mut sentinel);
}

#[test]
fn e06s03_first_session_established_between_snapshot_and_inspect_stays_attached() {
    let mut host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("hold-session");
    host.barrier(&agent, "session");
    host.call(json!({"op":"config","config":{"prefix":"Ctrl-a"}}));
    let original = agent.target.clone();
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let relay = PhaseRelay::new(&host, Fault::BootstrapObservation);
    let mut native = relay.attachment(&mut host, &agent);
    native.details();
    native.wait("ACP | starting");
    relay.gates.armed.store(true, Ordering::Release);
    host.until(
        "actual pre-session snapshot retained before client inspection",
        || relay.gates.ui_held.load(Ordering::Acquire).then_some(()),
    );
    agent.release("session");
    host.ready(&mut agent);
    relay.gates.release_ui.store(true, Ordering::Release);
    native.details();
    native.wait(agent.target["session_id"].as_str().unwrap());
    native.observer();
    let work = host.prompt(&agent, "snapshot-inspect-first-session", "add 17 25");
    host.completed(&agent, &work["work_id"]);
    native.projected(&host, host.inspect(&agent)["seq"].as_u64().unwrap());
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    for field in ["id", "instance_id", "connection_generation"] {
        assert_eq!(agent.target[field], original[field]);
    }
    assert_eq!(
        ownership(&host, &agent)["owner"]["controller_id"],
        agent.controller_id
    );
    assert!(ownership(&host, &agent)["reservation"].is_null());
    assert_eq!(
        host.call(json!({"op":"snapshot"}))["agent_views"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    finish(host, &mut agent, &mut native, &mut sentinel);
}

#[test]
fn e06s03_failed_recovery_setup_and_idle_observers_do_not_leak_registrations() {
    let mut host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("arithmetic");
    host.ready(&mut agent);
    host.call(json!({"op":"config","config":{"prefix":"Ctrl-a"}}));
    let exact = agent.target.clone();
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let original_owner = ownership(&host, &agent)["owner"].clone();
    let relay = PhaseRelay::new(&host, Fault::RecoverySetup);
    let mut native = relay.attachment(&mut host, &agent);
    native.observer();
    let original_registration = relay.gates.registrations.lock()[0].handle.clone();
    relay
        .gates
        .setup_drops_remaining
        .store(3, Ordering::Release);
    relay.gates.registrations.lock()[0]
        .socket
        .shutdown(Shutdown::Both)
        .unwrap();
    host.until(
        "failed setup observations exhausted before fresh registration",
        || {
            (relay.gates.setup_drops.load(Ordering::Acquire) == 3
                && relay.gates.registrations.lock().len() == 2)
                .then_some(())
        },
    );
    native.wait("Reconnected");
    let (retired, next_id) = {
        let registrations = relay.gates.registrations.lock();
        let fresh = registrations.last().unwrap();
        (fresh.handle.clone(), fresh.id.clone())
    };
    host.until(
        "recovered observer is connected on the original target",
        || {
            ownership(&host, &agent)["views"]
                .as_array()?
                .iter()
                .any(|view| {
                    view["controller_id"] == next_id
                        && view["connected"] == true
                        && view["reported"] == true
                })
                .then_some(())
        },
    );
    relay
        .gates
        .registrations
        .lock()
        .last()
        .unwrap()
        .socket
        .shutdown(Shutdown::Both)
        .unwrap();
    host.until("second observer recovery finishes", || {
        (relay.gates.registrations.lock().len() == 3).then_some(())
    });
    // Existing guarded controller-close must reclaim the idle recovered
    // registration, not merely mark its view disconnected or release an owner.
    host.until("retired recovery capability is actually closed", || {
        let response = exchange(
            &host.socket,
            json!({"op":"control-handle","handle":retired,"method":"read"}),
        )
        .ok()?;
        (response["ok"] == false && response["error"]["code"] == "stale-handle").then_some(())
    });
    host.call(json!({"op":"control-handle","handle":original_registration,"method":"read"}));
    assert_eq!(ownership(&host, &agent)["owner"], original_owner);
    assert!(ownership(&host, &agent)["reservation"].is_null());
    let latest = relay.gates.registrations.lock().last().unwrap().id.clone();
    host.until(
        "latest fresh observer reports the exact original target",
        || {
            ownership(&host, &agent)["views"]
                .as_array()?
                .iter()
                .any(|view| {
                    view["controller_id"] == latest
                        && view["connected"] == true
                        && view["reported"] == true
                })
                .then_some(())
        },
    );
    let work = host.prompt(&agent, "after-registration-recovery", "add 17 25");
    host.completed(&agent, &work["work_id"]);
    native.projected(&host, host.inspect(&agent)["seq"].as_u64().unwrap());
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(host.inspect(&agent)["target"], exact);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    assert_eq!(relay.gates.registrations.lock().len(), 3);
    finish(host, &mut agent, &mut native, &mut sentinel);
}

#[test]
fn e06s03_registration_time_keys_never_become_fresh_ownership_gestures() {
    let mut host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("arithmetic");
    host.ready(&mut agent);
    host.call(json!({"op":"config","config":{"prefix":"Ctrl-a"}}));
    let owner = ownership(&host, &agent)["owner"].clone();
    let exact = agent.target.clone();
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let work = host.prompt(&agent, "before-registration-input-fence", "add 17 25");
    host.completed(&agent, &work["work_id"]);
    let retained_work = host.inspect(&agent)["work"].clone();
    let relay = PhaseRelay::new(&host, Fault::RecoveryRegistration);
    let mut native = relay.attachment(&mut host, &agent);
    native.observer();
    relay.gates.armed.store(true, Ordering::Release);
    relay.gates.registrations.lock()[0]
        .socket
        .shutdown(Shutdown::Both)
        .unwrap();
    host.until(
        "fresh registration reply is held after daemon creation",
        || {
            relay
                .gates
                .native_held
                .load(Ordering::Acquire)
                .then_some(())
        },
    );
    // All bytes are queued while the actual registration reply is withheld.
    // Cursor-position, device-attributes and keyboard-flags replies are real
    // internal terminal events: filtering one is not an empty-input proof.
    // The following old prefix/Ctrl-L must not become a new controller gesture.
    native.send(b"\x1b[20;10R\x1b[?1;2c\x1b[?0u\x01\x0c");
    relay.gates.release_native.store(true, Ordering::Release);
    native.wait("Reconnected");
    let fresh = relay.gates.registrations.lock().last().unwrap().id.clone();
    native.send(b"\x1b[I");
    host.until(
        "fresh focus report fences all earlier queued key handling",
        || {
            ownership(&host, &agent)["views"]
                .as_array()?
                .iter()
                .any(|view| {
                    view["controller_id"] == fresh
                        && view["connected"] == true
                        && view["reported"] == true
                        && view["focused"] == true
                })
                .then_some(())
        },
    );
    assert_eq!(
        relay.gates.claims.load(Ordering::Acquire),
        0,
        "recovery-time keys must never emit an ownership claim"
    );
    assert_eq!(ownership(&host, &agent)["owner"], owner);
    assert!(ownership(&host, &agent)["reservation"].is_null());
    native.projected(&host, host.inspect(&agent)["seq"].as_u64().unwrap());
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(host.inspect(&agent)["target"], exact);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    assert_eq!(host.inspect(&agent)["work"], retained_work);
    // Only this separately entered gesture may create the recovered claim.
    native.action(12);
    host.until(
        "fresh deliberate gesture emits its real cooperative claim",
        || {
            let state = ownership(&host, &agent);
            (relay.gates.claims.load(Ordering::Acquire) == 1
                && state["reservation"]["requester_id"] == fresh
                && state["reservation"]["claim_id"].is_string())
            .then_some(())
        },
    );
    assert_eq!(ownership(&host, &agent)["owner"], owner);
    native.wait("Control requested");
    native.send(b"c");
    native.wait("Cancel takeover?");
    native.send(b"\r");
    host.until("explicit cancellation grants nobody", || {
        let state = ownership(&host, &agent);
        (state["owner"].is_null() && state["reservation"].is_null()).then_some(())
    });
    assert_eq!(relay.gates.claims.load(Ordering::Acquire), 1);
    assert_eq!(host.inspect(&agent)["target"], exact);
    assert_eq!(host.inspect(&agent)["work"], retained_work);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    assert_eq!(relay.gates.registrations.lock().len(), 2);
    finish(host, &mut agent, &mut native, &mut sentinel);
}

fn picker_selection_survives_resize_loss(mouse: bool) {
    let mut host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("arithmetic");
    host.ready(&mut agent);
    host.call(json!({"op":"config","config":{"prefix":"Ctrl-a"}}));
    let terminal = host.call(json!({"op":"snapshot"}))["terminals"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let before = host.until("native survivor bootstrap is settled and unowned", || {
        let state = host.call(json!({"op":"state","terminal":terminal}));
        (state["integration"]["shell"]["ready"] == true
            && state["ownership"]["owner"].is_null()
            && state["ownership"]["reservation"].is_null()
            && state["ownership"]["unsettled"]
                .as_array()
                .is_some_and(Vec::is_empty))
        .then_some(state)
    });
    let exact = agent.target.clone();
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let original_owner = ownership(&host, &agent)["owner"].clone();
    let relay = PhaseRelay::new(&host, Fault::PickerResize);
    let mut native = relay.attachment(&mut host, &agent);
    native.observer();
    relay.gates.armed.store(true, Ordering::Release);
    native.send(PREFIX);
    if mouse {
        native.send(b"\x1b[B"); // settle prefix; native survivor is the first picker row
        native.send(b"\x1b[<0;4;4M\x1b[<0;4;4m");
    } else {
        native.send(b"\x1b[A\r"); // from the second ACP row to the first native row
    }
    host.until("selected native resize reply loses observation", || {
        relay.gates.cut.load(Ordering::Acquire).then_some(())
    });
    assert_eq!(*relay.gates.picker_target.lock(), terminal);
    native.wait("Reconnected");
    let fresh = relay.gates.registrations.lock().last().unwrap().id.clone();
    native.send(b"\x1b[I");
    let selected = host.until("fresh connection reports its recovered selection", || {
        let state = host.call(json!({"op":"state","terminal":terminal}));
        // Views are global original-connection reports, not evidence that the
        // state request's target is selected. Inspect their actual target.
        let view = state["ownership"]["views"]
            .as_array()?
            .iter()
            .find(|view| {
                view["controller_id"] == fresh
                    && view["connected"] == true
                    && view["reported"] == true
            })?;
        view.get("terminal")
            .or_else(|| view.get("target").and_then(|target| target.get("id")))?
            .as_str()
            .map(str::to_owned)
    });
    assert_eq!(
        selected, terminal,
        "recovery substituted the previous picker target"
    );
    let after = host.call(json!({"op":"state","terminal":terminal}));
    assert_eq!(after["pid"], before["pid"]);
    assert_eq!(
        after["ownership"]["observed"],
        before["ownership"]["observed"]
    );
    assert!(after["ownership"]["owner"].is_null());
    assert!(after["ownership"]["reservation"].is_null());
    assert_eq!(ownership(&host, &agent)["owner"], original_owner);
    assert_eq!(relay.gates.claims.load(Ordering::Acquire), 0);
    assert_eq!(relay.gates.opens.load(Ordering::Acquire), 0);
    let work = host.prompt(&agent, "original-agent-after-native-selection", "add 17 25");
    host.completed(&agent, &work["work_id"]);
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(host.inspect(&agent)["target"], exact);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    finish(host, &mut agent, &mut native, &mut sentinel);
}

#[test]
fn e06s03_picker_keyboard_selection_survives_resize_observation_loss() {
    picker_selection_survives_resize_loss(false);
}

#[test]
fn e06s03_picker_mouse_selection_survives_resize_observation_loss() {
    picker_selection_survives_resize_loss(true);
}

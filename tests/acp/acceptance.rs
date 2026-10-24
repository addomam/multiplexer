use super::*;

fn s04_metadata_is_private(host: &Host, agent: &Agent, markers: &[&str]) {
    for projection in [
        host.call(json!({"op":"snapshot"})),
        host.call(json!({"op":"events","after":0,"timeout_ms":0})),
        host.call(json!({"op":"acp-inspect","target":agent.target})),
    ] {
        let text = projection.to_string();
        for marker in markers {
            assert!(!text.contains(marker), "private marker leaked: {marker}");
        }
    }
}

fn s04_interaction(host: &Host, agent: &Agent, id: &Value) -> Value {
    host.inspect(agent)["content"]["interactions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|interaction| interaction["interaction_id"] == *id)
        .expect("exact retained interaction")
        .clone()
}

fn s04_owned_tree(host: &Host, agent: &Agent, sentinel: &mut Sentinel) -> (i32, i32) {
    let read_pid = |name: &str| {
        host.until("owned process identity published", || {
            fs::read_to_string(agent.root.join(name))
                .ok()?
                .parse::<i32>()
                .ok()
        })
    };
    let peer = read_pid("peer.pid");
    let descendant = read_pid("descendant.pid");
    assert_ne!(peer, descendant);
    assert_eq!(
        unsafe { libc::getsid(peer) },
        peer,
        "peer must own its session"
    );
    assert_eq!(
        unsafe { libc::getpgid(peer) },
        peer,
        "peer must own its group"
    );
    assert_eq!(unsafe { libc::getsid(descendant) }, peer);
    assert_eq!(unsafe { libc::getpgid(descendant) }, peer);
    assert_ne!(unsafe { libc::getpgid(sentinel.0.id() as i32) }, peer);
    sentinel.alive();
    (peer, descendant)
}

fn s04_joined(host: &Host, agent: &Agent) {
    host.reaped(agent, "peer.pid");
    if agent.root.join("descendant.pid").exists() {
        host.reaped(agent, "descendant.pid");
    }
    host.until("owned pipe workers joined before fallback", || {
        (host.inspect(agent)["resource"]["cleanup_complete"] == true).then_some(())
    });
    assert!(!agent.root.join("fixture-fallback").exists());
}

// Success-path cleanup is observed before Host::drop may enable any fallback.
// A daemon exit is also the shutdown join boundary for its owned worker threads.
fn s04_finish(mut host: Host, agents: &[&Agent], sentinel: &mut Sentinel, stopped: bool) {
    if !stopped {
        assert_eq!(host.call(json!({"op":"shutdown"}))["shutting_down"], true);
    }
    let deadline = Instant::now() + DEADLINE;
    let status = loop {
        if let Some(status) = host.daemon.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "daemon failed to exit after admitted shutdown"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success(), "private daemon exited with {status}");
    for agent in agents {
        host.reaped(agent, "peer.pid");
        if agent.root.join("descendant.pid").exists() {
            host.reaped(agent, "descendant.pid");
        }
        assert!(!agent.root.join("fixture-fallback").exists());
    }
    assert!(!host.root.join("fixture-fallback").exists());
    assert!(
        !host.socket.exists(),
        "private socket survived daemon shutdown"
    );
    assert!(pid_gone(host.daemon.id() as i32));
    sentinel.alive();
    let root = host.root.clone();
    fs::remove_dir_all(&root).unwrap();
    assert!(!root.exists(), "private root survived explicit removal");
    drop(host);
    sentinel.alive();
}

#[test]
fn e04s04_complete_forms_and_all_permission_dispositions_keep_exact_originals_across_force() {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("s04-complete");
    host.ready(&mut agent);
    let tuple = agent.target.clone();
    let (peer, descendant) = s04_owned_tree(&host, &agent, &mut sentinel);
    let mut previous_form: Option<Value> = None;
    let mut previous_permission: Option<Value> = None;
    let mut previous_prompt: Option<Value> = None;
    for (index, (text, bonus, option, expected, allows)) in [
        ("add 17 25", 7, "opaque-α", 49, true),
        ("add 17 25", 11, "opaque-γ", 49, false),
        ("add 1 2", 5, "opaque-β", 57, true),
        ("add 1 2", 99, "opaque-δ", 57, false),
    ]
    .into_iter()
    .enumerate()
    {
        let attempt = index + 1;
        let admitted = host.prompt(&agent, &format!("complete-prompt-{attempt}"), text);
        let original = host.work(&agent, &admitted["work_id"]);
        let form = pending(&host, &agent, "form");
        assert_eq!(form["scope"]["sessionId"], tuple["session_id"]);
        assert_eq!(
            host.inspect(&agent)["foreground"]["prompt_id"],
            original["prompt_id"]
        );
        if let Some(old) = &previous_form {
            assert_ne!(form["interaction_id"], old["interaction_id"]);
            host.deny(
                form_answer(
                    &agent,
                    old,
                    &format!("stale-form-{attempt}"),
                    "accept",
                    Some(json!({"bonus":97})),
                ),
                &["stale-interaction"],
            );
        }
        let answer = host.call(form_answer(
            &agent,
            &form,
            &format!("complete-answer-{attempt}"),
            "accept",
            Some(json!({"bonus":bonus})),
        ));
        let answer_work = host.work(&agent, &answer["work_id"]);
        assert_eq!(answer_work["rpc_id"], "reusable-form");
        assert_eq!(answer_work["interaction_id"], form["interaction_id"]);
        let permission = pending(&host, &agent, "permission");
        assert_eq!(permission["scope"]["sessionId"], tuple["session_id"]);
        assert!(!agent.root.join(format!("tool-effect-{attempt}")).exists());
        if let Some(old) = &previous_permission {
            assert_ne!(permission["interaction_id"], old["interaction_id"]);
            host.deny(
                permission_answer(
                    &agent,
                    old,
                    &format!("stale-permission-{attempt}"),
                    "opaque-α",
                ),
                &["stale-interaction"],
            );
        }
        if index == 0 {
            let stale = permission_answer(&agent, &permission, "revoked-permission", "opaque-β");
            let successor = s03_controller(&host, "S04 complete successor");
            let grant = s03_claim(&host, &agent, &successor, "force", Value::Null);
            host.deny(stale, &["stale-control", "stale-handle"]);
            s03_same_work(&host.work(&agent, &admitted["work_id"]), &original);
            assert_eq!(
                pending(&host, &agent, "permission")["interaction_id"],
                permission["interaction_id"]
            );
            assert!(host.work(&agent, &admitted["work_id"])["outcome"].is_null());
            agent.opened = grant;
            agent.controller_id = successor["controller_id"].clone();
        }
        let selected = host.call(permission_answer(
            &agent,
            &permission,
            &format!("complete-permission-{attempt}"),
            option,
        ));
        let selected_work = host.work(&agent, &selected["work_id"]);
        assert_eq!(selected_work["rpc_id"], 73);
        assert_eq!(
            selected_work["interaction_id"],
            permission["interaction_id"]
        );
        let completed = host.completed(&agent, &admitted["work_id"]);
        s03_same_work(&completed, &original);
        assert_eq!(
            completed["outcome"],
            json!({"kind":"response","stop_reason":"end_turn"})
        );
        if let Some(old) = &previous_prompt {
            assert_ne!(completed["rpc_id"], old["rpc_id"]);
            assert_ne!(completed["prompt_id"], old["prompt_id"]);
        }
        assert_eq!(
            agent.effect(),
            json!({
                "session_id":tuple["session_id"],"value":expected,"commands":attempt,"rpc_id":original["rpc_id"]
            })
        );
        assert_eq!(
            agent.root.join(format!("tool-effect-{attempt}")).exists(),
            allows
        );
        let answered = s04_interaction(&host, &agent, &form["interaction_id"]);
        assert_eq!(answered["selected_work_id"], answer["work_id"]);
        assert_eq!(
            answered["selection"],
            json!({"action":"accept","content":{"bonus":bonus}})
        );
        let permitted = s04_interaction(&host, &agent, &permission["interaction_id"]);
        assert_eq!(permitted["selected_work_id"], selected["work_id"]);
        assert_eq!(permitted["selection"], json!({"option_id":option}));
        assert_eq!(host.inspect(&agent)["target"], tuple);
        assert!(host.inspect(&agent)["foreground"].is_null());
        assert_eq!(unsafe { libc::getsid(peer) }, peer);
        assert_eq!(unsafe { libc::getpgid(descendant) }, peer);
        assert!(!agent.root.join("unexpected-cancel").exists());
        s04_metadata_is_private(
            &host,
            &agent,
            &[
                "Synthetic private arithmetic question",
                "Synthetic private tool",
                "bonus",
                "Total=",
                "synthetic-private-agent-label",
            ],
        );
        previous_form = Some(form);
        previous_permission = Some(permission);
        previous_prompt = Some(completed);
    }
    let projection = host.inspect(&agent);
    let prompts: Vec<_> = projection["work"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|work| work["op"] == "acp-prompt")
        .collect();
    assert_eq!(
        prompts.len(),
        4,
        "stale choices must never create extra prompts"
    );
    assert!(prompts.iter().all(|work| work["state"] == "completed"));
    assert!(!projection["work"].as_array().unwrap().iter().any(|work| {
        work["call_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("stale-") || id == "revoked-permission")
    }));
    host.close(&agent);
    s04_joined(&host, &agent);
    s04_finish(host, &[&agent], &mut sentinel, false);
}

// Retain the unread socket until actual canonical admission has been observed.
fn s04_held_prompt(host: &Host, agent: &Agent) -> Value {
    host.barrier(agent, "s04-reads");
    let mut request = agent.action("acp-prompt", "s04-held-original");
    request["version"] = json!(1);
    request["text"] = json!(format!("add 17 25{}", " ".repeat(65536 - 9)));
    let mut socket = UnixStream::connect(&host.socket).unwrap();
    socket.set_write_timeout(Some(DEADLINE)).unwrap();
    serde_json::to_writer(&mut socket, &request).unwrap();
    socket.write_all(b"\n").unwrap();
    let original = host.until("canonical held-input admission before caller loss", || {
        host.inspect(agent)["work"]
            .as_array()?
            .iter()
            .find(|work| work["call_id"] == "s04-held-original")
            .cloned()
    });
    assert_eq!(original["state"], "accepted");
    assert_eq!(original["delivery"], "not-submitted");
    assert!(original["outcome"].is_null());
    assert!(!agent.root.join("effect.json").exists());
    drop(socket);
    original
}

#[test]
fn e04s04_held_stdin_descendants_allow_parallel_snapshot_force_and_unrelated_computation() {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut held = host.open("s04-stalled-descendants");
    host.ready(&mut held);
    s04_owned_tree(&host, &held, &mut sentinel);
    let original = s04_held_prompt(&host, &held);
    let tuple = held.target.clone();
    let mut healthy = host.open("arithmetic");
    host.ready(&mut healthy);
    let successor = s03_controller(&host, "S04 held-input successor");
    let claim = json!({"op":"ownership-claim","target":{"kind":"acp","id":held.target["id"]},
        "controller":successor["controller"],"mode":"force","observed":s03_ownership(&host, &held)["observed"]});
    let mut compute = healthy.action("acp-prompt", "s04-unrelated-during-stall");
    compute["text"] = json!("add 19 23");
    let gate = std::sync::Arc::new(std::sync::Barrier::new(4));
    let jobs: Vec<_> = [json!({"op":"snapshot"}), claim, compute]
        .into_iter()
        .map(|request| {
            let gate = gate.clone();
            let socket = host.socket.clone();
            std::thread::spawn(move || {
                gate.wait();
                exchange(&socket, request).unwrap()
            })
        })
        .collect();
    gate.wait();
    let replies: Vec<_> = jobs.into_iter().map(|job| job.join().unwrap()).collect();
    for reply in &replies {
        assert_eq!(reply["ok"], true, "{reply}");
    }
    assert!(
        replies[0]["result"]["agent_views"]
            .as_array()
            .unwrap()
            .iter()
            .any(|view| view["id"] == held.target["id"] && view["lifecycle"] == "ready")
    );
    held.opened = replies[1]["result"].clone();
    let computed = host.completed(&healthy, &replies[2]["result"]["work_id"]);
    assert_eq!(computed["outcome"]["stop_reason"], "end_turn");
    assert_eq!(healthy.effect()["value"], 42);
    assert_eq!(healthy.effect()["commands"], 1);
    assert_eq!(healthy.effect()["rpc_id"], computed["rpc_id"]);
    let recovered = s03_handle(&host, &successor["controller"], "read");
    let retained = recovered["controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|control| control["control"] == held.opened["control"])
        .unwrap()["retained_work"]
        .as_array()
        .unwrap()
        .iter()
        .find(|work| work["work_id"] == original["work_id"])
        .unwrap();
    s03_same_work(retained, &original);
    let waiting = host.work(&held, &original["work_id"]);
    s03_same_work(&waiting, &original);
    assert_eq!(waiting["state"], "accepted");
    assert_eq!(waiting["delivery"], "not-submitted");
    assert!(waiting["outcome"].is_null());
    assert_eq!(host.inspect(&held)["target"], tuple);
    assert!(!held.root.join("s04-reads.release").exists());
    assert!(!held.root.join("unexpected-cancel").exists());
    host.close(&held);
    s04_joined(&host, &held);
    let ended = host.work(&held, &original["work_id"]);
    s03_same_work(&ended, &original);
    assert_eq!(ended["state"], "unknown");
    assert_eq!(ended["application"], "unknown");
    assert!(ended["outcome"].is_null());
    assert!(host.inspect(&held)["foreground"].is_null());
    assert!(!held.root.join("effect.json").exists());
    // Closing a blocked target cannot retire or interrupt its independent sibling.
    let next = host.prompt(&healthy, "s04-unrelated-after-close", "add 10 5");
    host.completed(&healthy, &next["work_id"]);
    assert_eq!(healthy.effect()["value"], 57);
    assert_eq!(healthy.effect()["commands"], 2);
    host.close(&healthy);
    s04_joined(&host, &healthy);
    s04_finish(host, &[&held, &healthy], &mut sentinel, false);
}

#[test]
fn e04s04_canonical_close_admission_precedes_shutdown_with_held_input_and_owned_tree() {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("s04-stalled-descendants");
    host.ready(&mut agent);
    s04_owned_tree(&host, &agent, &mut sentinel);
    let original = s04_held_prompt(&host, &agent);
    let socket = host.socket.clone();
    let close = agent.action("acp-close", "s04-close-before-shutdown");
    let closer = std::thread::spawn(move || exchange(&socket, close));
    let closed = host.until("exact close committed before shutdown is sent", || {
        let projection = host.inspect(&agent);
        let work = projection["work"]
            .as_array()?
            .iter()
            .find(|work| work["call_id"] == "s04-close-before-shutdown")?
            .clone();
        assert!(matches!(
            projection["lifecycle"].as_str(),
            Some("closing" | "ended")
        ));
        assert!(projection["foreground"].is_null());
        Some(work)
    });
    let unknown = host.work(&agent, &original["work_id"]);
    s03_same_work(&unknown, &original);
    assert_eq!(unknown["state"], "unknown");
    assert!(unknown["outcome"].is_null());
    assert_eq!(closed["op"], "acp-close");
    assert_eq!(closed["owner_generation"], agent.opened["generation"]);
    assert!(!agent.root.join("s04-reads.release").exists());
    assert!(!agent.root.join("effect.json").exists());
    assert_eq!(host.call(json!({"op":"shutdown"}))["shutting_down"], true);
    match closer.join().unwrap() {
        Ok(reply) => {
            assert_eq!(
                reply["ok"], true,
                "canonical close must not become an authority error: {reply}"
            );
            assert_eq!(reply["result"]["work_id"], closed["work_id"]);
        }
        Err(error) => {
            // Only loss of this already-admitted reply during confirmed shutdown
            // is classified here; no early transport/authority error is a success.
            assert!(
                matches!(
                    error.kind(),
                    std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
                ),
                "unexpected close transport failure after canonical admission: {error}"
            );
            eprintln!(
                "S04 close canonically admitted; reply unobserved during shutdown: {:?}",
                error.kind()
            );
        }
    }
    s04_finish(host, &[&agent], &mut sentinel, true);
}

#[test]
fn e04s04_shutdown_of_canonically_admitted_held_input_reaps_without_application() {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("s04-stalled-descendants");
    host.ready(&mut agent);
    s04_owned_tree(&host, &agent, &mut sentinel);
    let original = s04_held_prompt(&host, &agent);
    let waiting = host.work(&agent, &original["work_id"]);
    s03_same_work(&waiting, &original);
    assert_eq!(waiting["state"], "accepted");
    assert_eq!(waiting["delivery"], "not-submitted");
    assert!(!agent.root.join("s04-reads.release").exists());
    assert_eq!(host.call(json!({"op":"shutdown"}))["shutting_down"], true);
    host.reaped(&agent, "peer.pid");
    host.reaped(&agent, "descendant.pid");
    assert!(!agent.root.join("effect.json").exists());
    assert!(!agent.root.join("unexpected-cancel").exists());
    // The daemon is ending: do not invent a post-shutdown work projection or
    // claim an original response/cancellation that was never observed.
    s04_finish(host, &[&agent], &mut sentinel, true);
}

#[test]
fn e04s04_hostile_frames_fail_only_their_instance_while_other_session_computes() {
    for mode in ["oversized", "invalid-utf8", "malformed", "partial", "flood"] {
        let host = Host::new();
        let mut sentinel = Sentinel::new();
        let hostile = host.open(mode);
        host.barrier(&hostile, "hostile");
        let original_tuple = hostile.target.clone();
        let mut healthy = host.open("s04-complete");
        host.ready(&mut healthy);
        let (peer, descendant) = s04_owned_tree(&host, &healthy, &mut sentinel);
        let prompt = host.prompt(&healthy, "s04-pending-through-hostile-frame", "add 17 25");
        let original = host.work(&healthy, &prompt["work_id"]);
        let form = pending(&host, &healthy, "form");
        hostile.release("hostile");
        host.until(
            "hostile instance fails and joins without blocking active responder",
            || {
                let projection = host.inspect(&hostile);
                (projection["lifecycle"] == "failed"
                    && projection["resource"]["cleanup_complete"] == true)
                    .then_some(projection)
            },
        );
        host.reaped(&hostile, "peer.pid");
        assert_eq!(host.inspect(&hostile)["target"], original_tuple);
        assert!(!hostile.root.join("effect.json").exists());
        assert_eq!(
            pending(&host, &healthy, "form")["interaction_id"],
            form["interaction_id"]
        );
        s03_same_work(&host.work(&healthy, &prompt["work_id"]), &original);
        let successor = s03_controller(&host, "S04 surviving responder successor");
        healthy.opened = s03_claim(&host, &healthy, &successor, "force", Value::Null);
        host.call(form_answer(
            &healthy,
            &form,
            "s04-answer-after-hostile-frame",
            "accept",
            Some(json!({"bonus":15})),
        ));
        let permission = pending(&host, &healthy, "permission");
        host.call(permission_answer(
            &healthy,
            &permission,
            "s04-allow-after-hostile-frame",
            "opaque-α",
        ));
        let completed = host.completed(&healthy, &prompt["work_id"]);
        s03_same_work(&completed, &original);
        assert_eq!(healthy.effect()["value"], 57);
        assert_eq!(healthy.effect()["commands"], 1);
        assert_eq!(healthy.effect()["rpc_id"], original["rpc_id"]);
        assert!(healthy.root.join("tool-effect-1").exists());
        assert_eq!(unsafe { libc::getsid(peer) }, peer);
        assert_eq!(unsafe { libc::getpgid(descendant) }, peer);
        assert!(!healthy.root.join("unexpected-cancel").exists());
        s04_metadata_is_private(
            &host,
            &healthy,
            &[
                "Synthetic private arithmetic question",
                "Synthetic private tool",
                "bonus",
                "Total=",
                "synthetic-private-agent-label",
            ],
        );
        host.close(&healthy);
        s04_joined(&host, &healthy);
        s04_joined(&host, &hostile);
        s04_finish(host, &[&hostile, &healthy], &mut sentinel, false);
    }
}

#[test]
fn e04s04_provider_error_and_stderr_pressure_preserve_private_state_and_independent_effects() {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut error_peer = host.open("error-once");
    let mut noisy = host.open("stderr");
    host.ready(&mut error_peer);
    host.ready(&mut noisy);
    let error_tuple = error_peer.target.clone();
    let noise_tuple = noisy.target.clone();
    let failed = host.prompt(
        &error_peer,
        "s04-private-error-original",
        "s04-private-prompt-marker",
    );
    let original = host.work(&error_peer, &failed["work_id"]);
    let failure = host.until(
        "original provider error releases only its foreground",
        || {
            let work = host.work(&error_peer, &failed["work_id"]);
            (work["state"] == "failed").then_some(work)
        },
    );
    s03_same_work(&failure, &original);
    assert_eq!(failure["outcome"], json!({"kind":"error","code":-32603}));
    assert!(host.inspect(&error_peer)["foreground"].is_null());
    assert!(!error_peer.root.join("effect.json").exists());
    let content = host.inspect(&error_peer);
    assert!(
        content["content"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["kind"] == "user-message"
                && event["text"] == "s04-private-prompt-marker")
    );
    for private in [
        "synthetic-private-provider-error",
        "synthetic-private-error-data",
    ] {
        assert!(!content.to_string().contains(private));
    }
    host.until(
        "stderr overflow retained as truncation while child continues",
        || {
            (noisy.root.join("stderr-drained").exists()
                && host.inspect(&noisy)["resource"]["stderr_truncated"] == true)
                .then_some(())
        },
    );
    let gate = std::sync::Arc::new(std::sync::Barrier::new(3));
    let jobs: Vec<_> = [
        (&error_peer, "s04-fresh-after-error", "add 17 25"),
        (&noisy, "s04-after-stderr-overflow", "add 10 5"),
    ]
    .into_iter()
    .map(|(agent, call, text)| {
        let mut request = agent.action("acp-prompt", call);
        request["text"] = json!(text);
        let gate = gate.clone();
        let socket = host.socket.clone();
        std::thread::spawn(move || {
            gate.wait();
            exchange(&socket, request).unwrap()
        })
    })
    .collect();
    gate.wait();
    for (agent, job, expected) in [&error_peer, &noisy]
        .into_iter()
        .zip(jobs)
        .zip([42, 15])
        .map(|((agent, job), expected)| (agent, job, expected))
    {
        let receipt = job.join().unwrap();
        assert_eq!(receipt["ok"], true, "{receipt}");
        let complete = host.completed(agent, &receipt["result"]["work_id"]);
        assert_eq!(complete["outcome"]["stop_reason"], "end_turn");
        assert_eq!(agent.effect()["value"], expected);
        assert_eq!(agent.effect()["commands"], 1);
        assert_eq!(agent.effect()["rpc_id"], complete["rpc_id"]);
        s04_metadata_is_private(
            &host,
            agent,
            &[
                "s04-private-prompt-marker",
                "synthetic-private-provider-error",
                "synthetic-private-error-data",
                "synthetic-private-agent-label",
                "Total=",
                "ssssssssssssssss",
            ],
        );
    }
    assert_eq!(host.inspect(&error_peer)["target"], error_tuple);
    assert_eq!(host.inspect(&noisy)["target"], noise_tuple);
    let failed_after_success = host.work(&error_peer, &failed["work_id"]);
    s03_same_work(&failed_after_success, &original);
    assert_eq!(failed_after_success["state"], "failed");
    assert_eq!(failed_after_success["outcome"], failure["outcome"]);
    host.close(&error_peer);
    host.close(&noisy);
    s04_joined(&host, &error_peer);
    s04_joined(&host, &noisy);
    s04_finish(host, &[&error_peer, &noisy], &mut sentinel, false);
}

struct S04PipeRetainer {
    root: PathBuf,
    released: bool,
}

impl S04PipeRetainer {
    fn release(&mut self, host: &Host, agent: &Agent) {
        fs::write(
            self.root.join("retainer.release"),
            b"explicit fixture release",
        )
        .unwrap();
        host.reaped(agent, "retainer.pid");
        self.released = true;
    }
}

impl Drop for S04PipeRetainer {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        // Failure-only owned fixture cleanup. No PID/group signals or success oracle.
        let _ = fs::write(self.root.join("retainer.release"), b"failure cleanup");
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let running = fs::read_to_string(self.root.join("retainer.pid"))
                .ok()
                .and_then(|pid| pid.parse().ok())
                .is_some_and(|pid| !pid_gone(pid));
            if !running {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn e04s04_retained_orphan_stdio_cannot_pin_close_or_daemon_shutdown() {
    for close_first in [true, false] {
        let mut host = Host::new();
        let mut sentinel = Sentinel::new();
        let mut agent = host.open("s04-retained-pipes");
        let mut retainer = S04PipeRetainer {
            root: agent.root.clone(),
            released: false,
        };
        host.ready(&mut agent);
        let identity: Value = host.until("retainer reparented outside captured tree", || {
            serde_json::from_slice(&fs::read(agent.root.join("retainer.ready")).ok()?).ok()
        });
        let peer: i32 = fs::read_to_string(agent.root.join("peer.pid"))
            .unwrap()
            .parse()
            .unwrap();
        let held_pid: i32 = fs::read_to_string(agent.root.join("retainer.pid"))
            .unwrap()
            .parse()
            .unwrap();
        assert_ne!(identity["parent"], peer);
        assert_ne!(identity["parent"], identity["intermediary"]);
        assert_ne!(unsafe { libc::getsid(held_pid) }, peer);
        assert_eq!(identity["session"], unsafe { libc::getsid(held_pid) });
        assert!(!pid_gone(held_pid));
        let original = s04_held_prompt(&host, &agent);
        let mut siblings = Vec::new();
        if close_first {
            let closed = exchange(
                &host.socket,
                agent.action("acp-close", "retained-pipe-close"),
            );
            assert!(pid_gone(peer), "captured root still alive");
            assert!(
                !pid_gone(held_pid),
                "retainer must not supply accidental EOF"
            );
            sentinel.alive();
            let reply = match closed {
                Ok(reply) => reply,
                Err(error) => {
                    let before = host.inspect(&agent);
                    assert!(before["work"].as_array().unwrap().iter().any(|work| {
                        work["call_id"] == "retained-pipe-close" && work["op"] == "acp-close"
                    }));
                    assert_eq!(before["resource"]["cleanup_complete"], false);
                    retainer.release(&host, &agent);
                    host.until("only endpoint release unblocks original cleanup", || {
                        (host.inspect(&agent)["resource"]["cleanup_complete"] == true).then_some(())
                    });
                    eprintln!(
                        "retained-pipe RCA: close admitted; captured root absent; escaped retainer alive; only explicit endpoint release completed cleanup"
                    );
                    panic!(
                        "ACP close exceeded its client deadline with retained endpoints: {error}"
                    );
                }
            };
            assert_eq!(reply["ok"], true, "{reply}");
            let closed = host.inspect(&agent);
            assert_eq!(closed["lifecycle"], "failed");
            assert_eq!(closed["resource"]["failure"], "cleanup");
            assert_eq!(closed["resource"]["cleanup_complete"], false);
            let unknown = host.work(&agent, &original["work_id"]);
            s03_same_work(&unknown, &original);
            assert_eq!(unknown["state"], "unknown");
            assert_eq!(unknown["application"], "unknown");
            assert!(unknown["outcome"].is_null());
            let mut healthy = host.open("arithmetic");
            host.ready(&mut healthy);
            let prompt = host.prompt(&healthy, "after-retained-pipe-close", "add 19 23");
            host.completed(&healthy, &prompt["work_id"]);
            assert_eq!(healthy.effect()["value"], 42);
            siblings.push(healthy);
            for _ in 0..6 {
                let waiting = host.open("hold-initialize");
                host.barrier(&waiting, "initialize");
                siblings.push(waiting);
            }
            let controller = host.call(json!({"op":"controller-create","role":"external",
                "label":"Retained cleanup capacity"}));
            let project = host.call(json!({"op":"snapshot"}))["projects"][0]["id"].clone();
            let executable = Path::new(env!("CARGO_BIN_EXE_amux"))
                .parent()
                .unwrap()
                .join("examples/acp_fixture");
            host.deny(
                json!({"op":"acp-open","project":project,"controller":controller["controller"],
                "command":[executable,host.root,"arithmetic"]}),
                &["capacity-exceeded"],
            );
            for sibling in &siblings {
                host.close(sibling);
            }
        }
        assert_eq!(host.call(json!({"op":"shutdown"}))["shutting_down"], true);
        let deadline = Instant::now() + DEADLINE;
        let status = loop {
            if let Some(status) = host.daemon.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "retained pipes pinned daemon shutdown"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(status.success(), "{status}");
        assert!(pid_gone(peer));
        assert!(
            !pid_gone(held_pid),
            "daemon exit must not depend on fixture EOF"
        );
        assert!(!agent.root.join("retainer.release").exists());
        assert!(!agent.root.join("fixture-fallback").exists());
        sentinel.alive();
        retainer.release(&host, &agent);
        let mut agents: Vec<_> = siblings.iter().collect();
        agents.push(&agent);
        s04_finish(host, &agents, &mut sentinel, true);
    }
}

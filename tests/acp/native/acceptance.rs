//! Complete local two-surface acceptance; registration and cleanup support belong to parent.
use super::*;

// F9 is read-only navigation, not a retry of an application action. Advance once
// per observed rendered transition, bounded by the retained canonical choices.
fn show_interaction(host: &Host, agent: &Agent, native: &mut Attachment, wanted: Option<&Value>) {
    let projection = host.inspect(agent);
    let ids = projection["content"]["interactions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["interaction_id"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let wanted = wanted.map(|item| item["interaction_id"].as_str().unwrap());
    let visible = |text: &str| {
        text.lines()
            .find(|line| line.contains(" | revision "))
            .map(|line| {
                ids.iter()
                    .find(|id| line.contains(id.as_str()))
                    .expect("rendered interaction belongs to this exact retained session")
                    .clone()
            })
    };
    // A newly arrived question may still be awaiting the next canonical refresh.
    // One full rendered cycle plus its arrival is enough; never repeat an answer.
    for _ in 0..=ids.len() + 1 {
        let current = visible(&native.text());
        if current.as_deref() == wanted {
            return;
        }
        native.send(b"\x1b[20~");
        host.until(
            "F9 changes the rendered interaction/history selection",
            || (visible(&native.text()) != current).then_some(()),
        );
    }
    panic!(
        "requested interaction {wanted:?} not reachable:\n{}",
        native.text()
    );
}

fn dismiss_ownership(host: &Host, agent: &Agent, native: &mut Attachment) {
    let activity = ownership(host, agent)["activity_revision"]
        .as_u64()
        .unwrap();
    native.send(b"\x1b");
    host.until(
        "original owner parses Escape and dismisses ownership UI",
        || {
            let text = native.text();
            (ownership(host, agent)["activity_revision"]
                .as_u64()
                .unwrap()
                > activity
                && !text.contains("Control owned here")
                && !text.contains("Control requested")
                && !text.contains("Locked terminal"))
            .then_some(())
        },
    );
}

fn native_form(host: &Host, agent: &Agent, native: &mut Attachment, form: &Value, answer: &str) {
    // A fresh prompt can publish its question before the native projection
    // selects it. Observe that arrival before F9 can toggle it back to history.
    native.wait(form["interaction_id"].as_str().unwrap());
    show_interaction(host, agent, native, Some(form));
    native.send(b"\x1b[14~"); // F4: explicitly unset before deliberate typing.
    native.send(answer.as_bytes());
    native.send(ACCEPT_FORM);
    native.wait("REVIEW: F5 accepts");
    assert_eq!(
        pending(host, agent, "form")["interaction_id"],
        form["interaction_id"]
    );
    native.send(ACCEPT_FORM); // Only the second, deliberate F5 accepts this review.
}

fn selected_work(host: &Host, agent: &Agent, interaction: &Value, selection: Value) -> Value {
    let selected = host.until("exact canonical interaction selection", || {
        host.inspect(agent)["content"]["interactions"]
            .as_array()?
            .iter()
            .find(|item| {
                item["interaction_id"] == interaction["interaction_id"]
                    && item["state"] == "selected"
            })
            .cloned()
    });
    assert_eq!(selected["revision"], interaction["revision"]);
    assert_eq!(selected["scope"], interaction["scope"]);
    assert_eq!(selected["selection"], selection);
    let work = host.work(agent, &selected["selected_work_id"]);
    assert_eq!(work["interaction_id"], interaction["interaction_id"]);
    assert_eq!(host.inspect(agent)["target"], agent.target);
    work
}

fn completed_turn(host: &Host, agent: &Agent, original: &Value, value: i64, commands: u64) {
    let completed = host.completed(agent, &original["work_id"]);
    s03_same_work(&completed, original);
    assert_eq!(completed["outcome"]["stop_reason"], "end_turn");
    let effect = agent.effect();
    assert_eq!(effect["value"], value);
    assert_eq!(effect["commands"], commands);
    assert_eq!(effect["rpc_id"], original["rpc_id"]);
    assert_eq!(effect["session_id"], agent.target["session_id"]);
    host.until(
        "same-session arithmetic stream reaches canonical history",
        || {
            host.inspect(agent)["content"]["events"]
                .as_array()?
                .iter()
                .any(|event| {
                    event["kind"] == "agent-message"
                        && event["text"] == format!("Total={value}; commands={commands}")
                })
                .then_some(())
        },
    );
}

#[test]
fn e04s04_native_complete_two_surface_forms_permissions_and_running_handoffs() {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("s04-complete");
    host.ready(&mut agent);
    let exact = agent.target.clone();
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let descendant: i32 = host.until("owned descendant identity published", || {
        fs::read_to_string(agent.root.join("descendant.pid"))
            .ok()?
            .parse()
            .ok()
    });
    assert!(
        !pid_gone(descendant),
        "fixture must own a live descendant before cleanup"
    );
    let a_id = agent.controller_id.clone();
    let a_generation = agent.opened["generation"].clone();
    let b = external(&host);

    // API A starts the original RPC. Explicit release lets the genuine original
    // attachment request control without replacing this process/session/turn.
    let first = host.prompt(&agent, "s04-api-first", "add 17 25");
    let first_form = pending(&host, &agent, "form");
    let first_original = host.work(&agent, &first["work_id"]);
    assert_eq!(first_original["controller_id"], a_id);
    assert_eq!(first_original["owner_generation"], a_generation);
    let mut native = native_attachment(&host, &agent);
    let human_owner = ownership(&host, &agent)["owner"].clone();
    let human_id = human_owner["controller_id"].clone();
    let human_generation = ownership(&host, &agent)["observed"]["generation"].clone();
    native.send(b"\x1b[I");
    host.until("real original human reports focused exact ACP view", || {
        ownership(&host, &agent)["views"]
            .as_array()?
            .iter()
            .any(|view| {
                view["controller_id"] == human_id
                    && view["connected"] == true
                    && view["reported"] == true
                    && view["focused"] == true
                    && view["target"] == target(&agent)
            })
            .then_some(())
    });
    native.wait(first_form["interaction_id"].as_str().unwrap());
    show_interaction(&host, &agent, &mut native, Some(&first_form));

    // Each conditional policy is explicitly armed by external B. Human keep is
    // a real consumed key on the original PTY, not an API impersonation. F5
    // must not accidentally review/answer the application form underneath it.
    for disposition in ["aborted-by-owner", "withdrawn"] {
        let claim = s03_claim(
            &host,
            &agent,
            &b,
            "cooperative",
            json!({"kind":"human-idle","idle_ms":0,"warning_ms":60000}),
        );
        native.wait("Keep control: any key");
        host.until(
            "actual rendered warning receives original-connection consent",
            || {
                let state = ownership(&host, &agent);
                (state["reservation"]["claim_id"] == claim["claim_id"]
                    && state["reservation"]["conditional"]["consent"] == "recorded"
                    && state["reservation"]["conditional"]["clock"]["kind"] == "warning")
                    .then_some(())
            },
        );
        if disposition == "aborted-by-owner" {
            native.send(ACCEPT_FORM);
        } else {
            handle(&host, &claim["claim"], "withdraw");
        }
        host.until("conditional claim ends without granting requester", || {
            (handle(&host, &claim["claim"], "read")["phase"] == disposition).then_some(())
        });
        host.until("conditional warning and reservation are cleared", || {
            let state = ownership(&host, &agent);
            let text = native.text();
            (state["reservation"].is_null()
                && !text.contains("Keep control: any key")
                && !text.contains("Keeping control; awaiting acknowledgement"))
            .then_some(())
        });
        let state = ownership(&host, &agent);
        assert_eq!(state["owner"]["controller_id"], human_id);
        assert_eq!(state["owner"]["role"], "attachment");
        assert_eq!(state["observed"]["generation"], human_generation);
        assert_eq!(
            pending(&host, &agent, "form")["interaction_id"],
            first_form["interaction_id"]
        );
        s03_same_work(&host.work(&agent, &first["work_id"]), &first_original);
        assert!(host.work(&agent, &first["work_id"])["outcome"].is_null());
        assert!(!native.text().contains("REVIEW: F5 accepts"));
        assert!(!agent.root.join("effect.json").exists());
    }

    native_form(&host, &agent, &mut native, &first_form, "7");
    let first_permission = pending(&host, &agent, "permission");
    assert_eq!(
        host.inspect(&agent)["foreground"]["prompt_id"],
        first_original["prompt_id"]
    );
    assert!(host.work(&agent, &first["work_id"])["outcome"].is_null());
    let first_answer = selected_work(
        &host,
        &agent,
        &first_form,
        json!({"action":"accept","content":{"bonus":7}}),
    );
    assert_eq!(first_answer["controller_id"], human_id);
    assert_eq!(first_answer["owner_generation"], human_generation);
    assert_eq!(first_answer["rpc_id"], "reusable-form");

    // Capture a real native permission highlight, then force only authority.
    show_interaction(&host, &agent, &mut native, Some(&first_permission));
    native.send(b"\x1b[B");
    native.wait("Local permission highlight");
    let grant = s03_claim(&host, &agent, &b, "force", Value::Null);
    agent.opened = grant.clone();
    agent.controller_id = b["controller_id"].clone();
    native.observer();
    assert_eq!(
        ownership(&host, &agent)["owner"]["controller_id"],
        b["controller_id"]
    );
    assert_eq!(
        pending(&host, &agent, "permission")["interaction_id"],
        first_permission["interaction_id"]
    );
    s03_same_work(&host.work(&agent, &first["work_id"]), &first_original);
    assert!(!agent.root.join("tool-effect-1").exists());
    assert!(
        !pid_gone(descendant),
        "force must not kill owned descendants"
    );
    sentinel.alive();

    // Physical stale keys, repeat, paste, mouse tails, focus and double-prefix
    // choices traverse actual top-level routing. A final original view receipt
    // fences their parsing; no guessed delay or application-input retry.
    native.send(b"\r\x1b[13;1:2u\x1b[15~");
    native.paste("add 800 2\r\n");
    native.send(b"\x1b[<0;16;14M\x1b[<32;17;14M\x1b[<0;17;14m\x01\x01");
    let serial = ownership(&host, &agent)["views"]
        .as_array()
        .unwrap()
        .iter()
        .find(|view| view["controller_id"] == human_id)
        .unwrap()["serial"]
        .as_u64()
        .unwrap();
    native.send(b"\x1b[O");
    host.until("observer focus loss fences stale physical gestures", || {
        ownership(&host, &agent)["views"]
            .as_array()?
            .iter()
            .any(|view| {
                view["controller_id"] == human_id
                    && view["serial"].as_u64().unwrap() > serial
                    && view["focused"] == false
            })
            .then_some(())
    });
    native.send(b"\x1b[I");
    host.until(
        "observer regains genuine focus without regaining authority",
        || {
            ownership(&host, &agent)["views"]
                .as_array()?
                .iter()
                .any(|view| view["controller_id"] == human_id && view["focused"] == true)
                .then_some(())
        },
    );
    assert_eq!(
        ownership(&host, &agent)["owner"]["controller_id"],
        b["controller_id"]
    );
    assert_eq!(
        pending(&host, &agent, "permission")["interaction_id"],
        first_permission["interaction_id"]
    );
    assert!(host.work(&agent, &first["work_id"])["outcome"].is_null());
    assert!(!agent.root.join("tool-effect-1").exists());
    let first_allow = host.call(permission_answer(
        &agent,
        &first_permission,
        "s04-b-exact-allow",
        "opaque-α",
    ));
    completed_turn(&host, &agent, &first_original, 49, 1);
    assert!(agent.root.join("tool-effect-1").exists());
    let first_choice = selected_work(
        &host,
        &agent,
        &first_permission,
        json!({"option_id":"opaque-α"}),
    );
    assert_eq!(first_choice["work_id"], first_allow["work_id"]);
    assert_eq!(first_choice["controller_id"], b["controller_id"]);
    assert_eq!(first_choice["owner_generation"], grant["generation"]);
    assert_eq!(first_choice["rpc_id"], 73);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    assert_eq!(host.inspect(&agent)["target"], exact);

    let second = host.prompt(&agent, "s04-api-second", "add 17 25");
    let second_form = pending(&host, &agent, "form");
    assert_ne!(second_form["interaction_id"], first_form["interaction_id"]);
    let second_original = host.work(&agent, &second["work_id"]);
    assert_eq!(second_original["controller_id"], b["controller_id"]);
    assert_eq!(second_original["owner_generation"], grant["generation"]);
    let stale_form = form_answer(
        &agent,
        &second_form,
        "s04-b-old-form",
        "accept",
        Some(json!({"bonus":97})),
    );
    let second_answer_receipt = host.call(form_answer(
        &agent,
        &second_form,
        "s04-b-eleven",
        "accept",
        Some(json!({"bonus":11})),
    ));
    let second_permission = pending(&host, &agent, "permission");
    assert_ne!(
        second_permission["interaction_id"],
        first_permission["interaction_id"]
    );
    let stale_permission = permission_answer(
        &agent,
        &second_permission,
        "s04-b-old-permission",
        "opaque-α",
    );
    assert_eq!(
        host.inspect(&agent)["foreground"]["prompt_id"],
        second_original["prompt_id"]
    );
    assert!(host.work(&agent, &second["work_id"])["outcome"].is_null());
    let second_answer = selected_work(
        &host,
        &agent,
        &second_form,
        json!({"action":"accept","content":{"bonus":11}}),
    );
    assert_eq!(second_answer["work_id"], second_answer_receipt["work_id"]);
    assert_eq!(second_answer["controller_id"], b["controller_id"]);
    assert_eq!(second_answer["owner_generation"], grant["generation"]);
    assert_eq!(second_answer["rpc_id"], "reusable-form");

    handle(&host, &grant["control"], "release");
    assert!(ownership(&host, &agent)["owner"].is_null());
    native.action(12); // Fresh deliberate cooperative request, never auto-reacquire.
    native.owner();
    let returned = ownership(&host, &agent);
    assert_eq!(returned["owner"]["controller_id"], human_id);
    let returned_generation = returned["observed"]["generation"].clone();
    assert_ne!(returned_generation, human_generation);
    host.deny(stale_form, &["stale-control", "stale-handle"]);
    host.deny(stale_permission, &["stale-control", "stale-handle"]);
    dismiss_ownership(&host, &agent, &mut native);
    assert_eq!(
        pending(&host, &agent, "permission")["interaction_id"],
        second_permission["interaction_id"]
    );
    assert_eq!(
        agent.effect()["commands"],
        1,
        "observer choices must not replay on reacquisition"
    );
    assert!(!agent.root.join("tool-effect-2").exists());
    show_interaction(&host, &agent, &mut native, Some(&second_permission));
    // Deliberately navigate to reject_once from either end, never accept a default.
    native.send(b"\x1b[A\x1b[A\x1b[A\x1b[B\x1b[B");
    native.wait("Local permission highlight");
    native.wait("opaque-γ");
    native.send(b"\r");
    completed_turn(&host, &agent, &second_original, 49, 2);
    let second_choice = selected_work(
        &host,
        &agent,
        &second_permission,
        json!({"option_id":"opaque-γ"}),
    );
    assert_eq!(second_choice["controller_id"], human_id);
    assert_eq!(second_choice["owner_generation"], returned_generation);
    assert_eq!(second_choice["rpc_id"], 73);
    assert!(
        !agent.root.join("tool-effect-2").exists(),
        "deny must run no tool arithmetic/effect"
    );
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    assert_eq!(host.inspect(&agent)["target"], exact);

    // Separate deliberate return to the editor and its canonical idle refresh.
    // An empty Enter must not bind/replay any observer draft as a fresh prompt.
    show_interaction(&host, &agent, &mut native, None);
    native.wait("Foreground: none");
    native.wait("Local draft 0/65536 B");
    native.send(b"\r");
    let third = native_prompt(&host, &agent, &mut native, "add 1 2");
    let third_form = pending(&host, &agent, "form");
    let third_original = host.work(&agent, &third["work_id"]);
    assert_eq!(third_original["controller_id"], human_id);
    assert_eq!(third_original["owner_generation"], returned_generation);
    native_form(&host, &agent, &mut native, &third_form, "5");
    let third_permission = pending(&host, &agent, "permission");
    assert_eq!(
        host.inspect(&agent)["foreground"]["prompt_id"],
        third_original["prompt_id"]
    );
    assert!(host.work(&agent, &third["work_id"])["outcome"].is_null());
    show_interaction(&host, &agent, &mut native, Some(&third_permission));
    // Reset from either end, deliberately visit another option, then allow_once.
    native.send(b"\x1b[A\x1b[A\x1b[A\x1b[B\x1b[A");
    native.wait("Local permission highlight");
    native.wait("opaque-α");
    native.send(b"\r");
    completed_turn(&host, &agent, &third_original, 57, 3);
    assert!(agent.root.join("tool-effect-3").exists());
    let third_answer = selected_work(
        &host,
        &agent,
        &third_form,
        json!({"action":"accept","content":{"bonus":5}}),
    );
    let third_choice = selected_work(
        &host,
        &agent,
        &third_permission,
        json!({"option_id":"opaque-α"}),
    );
    for work in [&third_answer, &third_choice] {
        assert_eq!(work["controller_id"], human_id);
        assert_eq!(work["owner_generation"], returned_generation);
    }
    assert_eq!(third_answer["rpc_id"], "reusable-form");
    assert_eq!(third_choice["rpc_id"], 73);

    let projection = host.inspect(&agent);
    assert_eq!(projection["target"], exact);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    let interactions = [
        &first_form,
        &first_permission,
        &second_form,
        &second_permission,
        &third_form,
        &third_permission,
    ];
    let responses = [
        &first_answer,
        &first_choice,
        &second_answer,
        &second_choice,
        &third_answer,
        &third_choice,
    ];
    for (index, interaction) in interactions.iter().enumerate() {
        assert_eq!(interaction["scope"]["sessionId"], exact["session_id"]);
        for previous in &interactions[..index] {
            assert_ne!(interaction["interaction_id"], previous["interaction_id"]);
        }
        for previous in &responses[..index] {
            assert_ne!(responses[index]["work_id"], previous["work_id"]);
            assert_ne!(responses[index]["call_id"], previous["call_id"]);
        }
    }
    let prompts = projection["work"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|work| work["op"] == "acp-prompt")
        .collect::<Vec<_>>();
    assert_eq!(prompts.len(), 3, "no automatic stale draft submission");
    for (index, original) in [&first_original, &second_original, &third_original]
        .iter()
        .enumerate()
    {
        s03_same_work(prompts[index], original);
        for previous in &prompts[..index] {
            assert_ne!(prompts[index]["rpc_id"], previous["rpc_id"]);
            assert_ne!(prompts[index]["prompt_id"], previous["prompt_id"]);
        }
    }
    assert_eq!(
        projection["content"]["interactions"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    let canonical = projection["content"].to_string();
    assert!(!canonical.contains("add 800 2"));
    assert!(!canonical.contains("\"bonus\":97"));
    assert!(!projection["work"].as_array().unwrap().iter().any(|work| {
        work["op"] == "acp-interrupt"
            || work["call_id"] == "s04-b-old-form"
            || work["call_id"] == "s04-b-old-permission"
    }));
    for public in [
        host.call(json!({"op":"snapshot"})),
        host.call(json!({"op":"acp-inspect","target":exact})),
    ] {
        let public = public.to_string();
        for private in [
            "Synthetic private arithmetic question",
            "Synthetic private tool",
            "\"bonus\"",
            "\"selection\"",
            "add 17 25",
            "add 1 2",
            "add 800 2",
        ] {
            assert!(
                !public.contains(private),
                "private conversation data in metadata: {private}"
            );
        }
    }
    assert!(!agent.root.join("unexpected-cancel").exists());
    assert!(
        !pid_gone(descendant),
        "owned descendant must survive all authority changes"
    );
    sentinel.alive();
    // Parent extends this actual close helper to observe descendant.pid absent
    // before root deletion/fallback; it retains termios/host/drainer/daemon/socket
    // and unrelated-sentinel observations. Do not duplicate its lifecycle here.
    finish(host, &mut agent, &mut native, &mut sentinel);
}

#[test]
fn e04s04_native_grok_other_missing_notes_remains_correctable_and_cancellable() {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("s04-grok-other");
    host.ready(&mut agent);
    let mut native = native_attachment(&host, &agent);
    dismiss_ownership(&host, &agent, &mut native);
    let original_owner = ownership(&host, &agent)["owner"].clone();
    for (index, text, total) in [(1, "add 17 25", 49), (2, "add 1 2", 32)] {
        show_interaction(&host, &agent, &mut native, None);
        native.wait("Foreground: none"); // Observe completion in the actual input consumer.
        let prompt = native_prompt(&host, &agent, &mut native, text);
        let form = pending(&host, &agent, "form");
        native.wait(form["interaction_id"].as_str().unwrap());
        show_interaction(&host, &agent, &mut native, Some(&form));
        native.send(b"\x1b[B"); // Select Other, not the offered Zero adjustment.
        if index == 2 {
            native.send(b"\t");
            native.paste("x");
            native.send(b"\x7f"); // Explicitly empty notes, not merely omitted.
        }
        native.send(ACCEPT_FORM);
        native.send(ACCEPT_FORM);
        native.wait("Validation: Other requires answer notes");
        let retained = pending(&host, &agent, "form");
        assert_eq!(retained["interaction_id"], form["interaction_id"]);
        assert!(retained["selected_work_id"].is_null());
        assert!(
            !agent
                .root
                .join(format!("grok-response-{index}.json"))
                .exists()
        );
        assert_eq!(ownership(&host, &agent)["owner"], original_owner);
        assert_ne!(host.work(&agent, &prompt["work_id"])["state"], "completed");
        if index == 1 {
            native.send(b"\t");
            native.paste("7");
            native.send(ACCEPT_FORM);
            native.wait("REVIEW: F5 accepts");
            native.send(ACCEPT_FORM);
        } else {
            native.send(b"\x1b[18~"); // F7 explicitly cancels the retained question.
        }
        let completed = host.completed(&agent, &prompt["work_id"]);
        s03_same_work(&completed, &prompt);
        assert_eq!(completed["outcome"]["stop_reason"], "end_turn");
        assert_eq!(agent.effect()["value"], total);
        assert_eq!(agent.effect()["commands"], index);
        assert_eq!(agent.effect()["rpc_id"], prompt["rpc_id"]);
        let wire: Value = serde_json::from_slice(
            &fs::read(agent.root.join(format!("grok-response-{index}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(
            wire,
            if index == 1 {
                json!({"jsonrpc":"2.0","id":"grok-other","result":{"outcome":"accepted",
                "answers":{"Arithmetic adjustment":["Other"]},
                "annotations":{"Arithmetic adjustment":{"notes":"7"}}}})
            } else {
                json!({"jsonrpc":"2.0","id":"grok-other","result":{"outcome":"cancelled"}})
            }
        );
        assert_eq!(ownership(&host, &agent)["owner"], original_owner);
    }
    assert!(
        !host.inspect(&agent)["work"]
            .as_array()
            .unwrap()
            .iter()
            .any(|work| work["op"] == "acp-interrupt")
    );
    assert!(!agent.root.join("unexpected-cancel").exists());
    finish(host, &mut agent, &mut native, &mut sentinel);
}

#[test]
fn e04s04_native_grok_unrepresentable_reply_is_rejected_before_offering() {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("s04-grok-large");
    host.ready(&mut agent);
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let mut native = native_attachment(&host, &agent);
    dismiss_ownership(&host, &agent, &mut native);
    let original_owner = ownership(&host, &agent)["owner"].clone();
    let original = native_prompt(&host, &agent, &mut native, "add 17 25");
    completed_turn(&host, &agent, &original, 42, 1);
    let rejected: Value =
        serde_json::from_slice(&fs::read(agent.root.join("grok-large-rejected.json")).unwrap())
            .unwrap();
    assert_eq!(rejected["id"], "grok-large");
    assert_eq!(rejected["error"]["code"], -32602);
    assert!(
        host.inspect(&agent)["content"]["interactions"]
            .as_array()
            .unwrap()
            .is_empty(),
        "an unrepresentable question must never be offered for native capture"
    );
    assert_eq!(ownership(&host, &agent)["owner"], original_owner);
    native.wait("Foreground: none");
    let fresh = native_prompt(&host, &agent, &mut native, "add 1 2");
    let form = pending(&host, &agent, "form");
    native.wait(form["interaction_id"].as_str().unwrap());
    show_interaction(&host, &agent, &mut native, Some(&form));
    native.send(b"\x1b[B"); // Deliberately select Other on the subsequent small question.
    native.send(b"\t");
    native.paste("7");
    native.send(ACCEPT_FORM);
    native.wait("REVIEW: F5 accepts");
    native.send(ACCEPT_FORM);
    completed_turn(&host, &agent, &fresh, 52, 2);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    assert_eq!(ownership(&host, &agent)["owner"], original_owner);
    let reply: Value =
        serde_json::from_slice(&fs::read(agent.root.join("grok-response-2.json")).unwrap())
            .unwrap();
    assert_eq!(
        reply,
        json!({"jsonrpc":"2.0","id":"grok-other","result":{"outcome":"accepted",
        "answers":{"Arithmetic adjustment":["Other"]},
        "annotations":{"Arithmetic adjustment":{"notes":"7"}}}})
    );
    assert!(
        !host.inspect(&agent)["work"]
            .as_array()
            .unwrap()
            .iter()
            .any(|work| work["op"] == "acp-interrupt")
    );
    assert!(!agent.root.join("unexpected-cancel").exists());
    finish(host, &mut agent, &mut native, &mut sentinel);
}

#[test]
fn e04s04_native_grok_definite_non_admission_allows_deliberate_correction_and_cancel() {
    let host = Host::new();
    let mut sentinel = Sentinel::new();
    let mut agent = host.open("s04-grok-capacity");
    host.ready(&mut agent);
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let mut native = native_attachment(&host, &agent);
    dismiss_ownership(&host, &agent, &mut native);
    let owner = ownership(&host, &agent)["owner"].clone();
    for (index, text, total) in [(1, "add 17 25", 49), (2, "add 1 2", 32)] {
        show_interaction(&host, &agent, &mut native, None);
        native.wait("Foreground: none");
        let prompt = native_prompt(&host, &agent, &mut native, text);
        if index == 1 {
            for reservation in 0..5 {
                let barrier = format!("capacity-{reservation}");
                host.barrier(&agent, &barrier);
                host.until(
                    "reservation canonically retained before next publication",
                    || {
                        (host.inspect(&agent)["interactions"].as_array()?.len() == reservation + 1)
                            .then_some(())
                    },
                );
                agent.release(&barrier);
            }
        }
        let form = host.until("small Grok question after retained reservations", || {
            host.inspect(&agent)["content"]["interactions"]
                .as_array()?
                .iter()
                .find(|item| {
                    item["state"] == "pending" && item["schema"]["properties"]["q0"].is_object()
                })
                .cloned()
        });
        show_interaction(&host, &agent, &mut native, Some(&form));
        native.send(b"\x1b[B"); // Other.
        native.send(b"\t");
        native.paste(&"9".repeat(40_960));
        native.send(ACCEPT_FORM);
        native.wait("REVIEW: F5 accepts");
        native.send(ACCEPT_FORM);
        native.wait("capacity-exceeded");
        let rejected = host.inspect(&agent);
        let pending = rejected["interactions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["interaction_id"] == form["interaction_id"])
            .unwrap();
        assert_eq!(pending["state"], "pending");
        assert!(pending["selected_work_id"].is_null());
        assert_eq!(
            rejected["work"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|work| work["op"] == "acp-answer")
                .count(),
            index as usize - 1
        );
        assert!(
            !agent
                .root
                .join(format!("grok-response-{index}.json"))
                .exists()
        );
        assert_eq!(ownership(&host, &agent)["owner"], owner);
        if index == 1 {
            native.send(b"\x1b[14~"); // Deliberately unset retained notes, not replay.
            native.paste("7");
            native.send(ACCEPT_FORM);
            native.wait("REVIEW: F5 accepts");
            native.send(ACCEPT_FORM);
        } else {
            native.send(b"\x1b[18~"); // F7 deliberately cancels after definite rejection.
        }
        completed_turn(&host, &agent, &prompt, total, index);
        let response: Value = serde_json::from_slice(
            &fs::read(agent.root.join(format!("grok-response-{index}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(
            response,
            if index == 1 {
                json!({"jsonrpc":"2.0","id":"grok-other","result":{"outcome":"accepted",
                "answers":{"Arithmetic adjustment":["Other"]},
                "annotations":{"Arithmetic adjustment":{"notes":"7"}}}})
            } else {
                json!({"jsonrpc":"2.0","id":"grok-other","result":{"outcome":"cancelled"}})
            }
        );
        assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
        assert_eq!(ownership(&host, &agent)["owner"], owner);
    }
    assert!(
        !host.inspect(&agent)["work"]
            .as_array()
            .unwrap()
            .iter()
            .any(|work| work["op"] == "acp-interrupt")
    );
    assert!(!agent.root.join("unexpected-cancel").exists());
    finish(host, &mut agent, &mut native, &mut sentinel);
}

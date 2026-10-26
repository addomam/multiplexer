//! Source-shaped OMP ACP boundaries; deterministic peer, never provider evidence.
use super::*;

fn open(host: &Host, mode: &str, settings: Value) -> Agent {
    let root = host
        .root
        .join(format!("peer-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&root).unwrap();
    let project = host.call(json!({"op":"snapshot"}))["projects"][0]["id"].clone();
    let controller =
        host.call(json!({"op":"controller-create","role":"external","label":"OMP wire boundary"}));
    let peer = Path::new(env!("CARGO_BIN_EXE_amux"))
        .parent()
        .unwrap()
        .join("examples/acp_fixture");
    let opened = host.call(
        json!({"op":"acp-open","project":project,"controller":controller["controller"],
        "command":[peer,root,mode],"name":"OMP source-shaped peer","settings":settings}),
    );
    Agent {
        target: opened["target"].clone(),
        opened,
        root,
        controller_id: controller["controller_id"].clone(),
    }
}

fn refresh(host: &Host, agent: &mut Agent, lifecycle: &str) -> Value {
    let id = agent.target["id"].as_str().unwrap();
    let view = host.until(&format!("exact settings lifecycle {lifecycle}"), || {
        host.view(id).filter(|v| v["lifecycle"] == lifecycle)
    });
    agent.target = json!({"kind":"acp","id":id,"instance_id":view["instance_id"],
        "connection_generation":view["connection_generation"],"session_id":view["session_id"]});
    view
}

#[test]
fn e06s01_omp_settings_are_acknowledged_before_prompt_admission() {
    let host = Host::new();
    let requested = json!({"model":"fixture/cheap","mode":"plan","effort":"low","options":{"fixture-detail":"brief"}});
    let mut agent = open(&host, "e06-settings-hold", requested.clone());
    host.barrier(&agent, "settings");
    let starting = refresh(&host, &mut agent, "starting");
    assert_eq!(starting["configuration"]["state"], "applying");
    assert_eq!(starting["configuration"]["requested"], requested);
    let mut prompt = agent.action("acp-prompt", "too-early");
    prompt["text"] = json!("add 17 25");
    host.deny(prompt, &["not-ready"]);
    assert!(!agent.root.join("effect.json").exists());
    agent.release("settings");
    host.ready(&mut agent);
    let inspect = host.inspect(&agent);
    let configuration = &inspect["configuration"];
    assert_eq!(configuration["state"], "applied");
    assert_eq!(configuration["requested"], requested);
    assert_eq!(
        configuration["reported"]["model"]["current"],
        "fixture/cheap"
    );
    assert_eq!(configuration["reported"]["mode"]["current"], "plan");
    assert_eq!(configuration["reported"]["effort"]["current"], "low");
    assert_eq!(configuration["reported"]["effort"]["option_id"], "thinking");
    assert_eq!(
        host.view(agent.target["id"].as_str().unwrap()).unwrap()["configuration"],
        *configuration
    );
    let sequence: Value =
        serde_json::from_slice(&fs::read(agent.root.join("settings-sequence.json")).unwrap())
            .unwrap();
    assert_eq!(
        sequence,
        json!([
            {"method":"session/set_config_option","configId":"model","value":"fixture/cheap"},
            {"method":"session/set_mode","modeId":"plan"},
            {"method":"session/set_config_option","configId":"thinking","value":"low"},
            {"method":"session/set_config_option","configId":"fixture-detail","value":"brief"}
        ])
    );
    let work = host.prompt(&agent, "configured-effect", "add 17 25");
    host.completed(&agent, &work["work_id"]);
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(agent.effect()["session_id"], agent.target["session_id"]);
    host.close(&agent);
}

#[test]
fn e06s01_explicit_option_amid_duplicate_categories_keeps_exact_identity() {
    let host = Host::new();
    let requested = json!({"options":{"thinking":"low"}});
    let mut agent = open(&host, "e06-settings-ambiguous", requested);
    host.ready(&mut agent);
    let exact = agent.target.clone();
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let configuration = host.inspect(&agent)["configuration"].clone();
    assert_eq!(configuration["state"], "applied");
    assert!(configuration["reported"]["effort"]["option_id"].is_null());
    let options = configuration["reported"]["options"].as_array().unwrap();
    assert_eq!(
        options
            .iter()
            .find(|option| option["id"] == "thinking")
            .unwrap()["current"],
        "low"
    );
    assert_eq!(
        options
            .iter()
            .find(|option| option["id"] == "other-thinking")
            .unwrap()["current"],
        "off"
    );
    let work = host.prompt(&agent, "exact-option-effect", "add 17 25");
    host.completed(&agent, &work["work_id"]);
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(agent.effect()["session_id"], exact["session_id"]);
    assert_eq!(host.inspect(&agent)["target"], exact);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    host.close(&agent);
}

#[test]
fn e06s01_distinct_exact_options_in_one_category_apply_independently() {
    let host = Host::new();
    let requested = json!({"options":{"thinking":"low","other-thinking":"high"}});
    let mut agent = open(&host, "e06-settings-ambiguous", requested);
    host.ready(&mut agent);
    let configuration = host.inspect(&agent)["configuration"].clone();
    assert_eq!(configuration["state"], "applied");
    let options = configuration["reported"]["options"].as_array().unwrap();
    for (id, value) in [("thinking", "low"), ("other-thinking", "high")] {
        assert_eq!(
            options.iter().find(|option| option["id"] == id).unwrap()["current"],
            value
        );
    }
    let work = host.prompt(&agent, "independent-options-effect", "add 17 25");
    host.completed(&agent, &work["work_id"]);
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(agent.effect()["session_id"], agent.target["session_id"]);
    host.close(&agent);
}

#[test]
fn e06s01_semantic_and_exact_aliases_share_one_selection() {
    for requested in [
        json!({"model":"fixture/cheap","options":{"model":"fixture/cheap"}}),
        json!({"mode":"plan","options":{"mode":"plan"}}),
        json!({"effort":"low","options":{"thinking":"low"}}),
    ] {
        let host = Host::new();
        let mut agent = open(&host, "e06-settings", requested.clone());
        host.ready(&mut agent);
        let configuration = host.inspect(&agent)["configuration"].clone();
        assert_eq!(configuration["state"], "applied");
        for key in ["model", "mode", "effort"] {
            if let Some(value) = requested.get(key) {
                assert_eq!(configuration["reported"][key]["current"], *value);
            }
        }
        let work = host.prompt(&agent, "same-alias-choice-effect", "add 17 25");
        host.completed(&agent, &work["work_id"]);
        assert_eq!(agent.effect()["value"], 42);
        assert_eq!(agent.effect()["commands"], 1);
        assert_eq!(agent.effect()["session_id"], agent.target["session_id"]);
        host.close(&agent);
    }
}

#[test]
fn e06s01_unreported_agent_defaults_never_claim_requested_settings() {
    let host = Host::new();
    let mut agent = open(&host, "arithmetic", json!({}));
    host.ready(&mut agent);
    let configuration = host.inspect(&agent)["configuration"].clone();
    assert_eq!(configuration["state"], "agent-default");
    for field in ["model", "mode", "effort"] {
        assert_eq!(configuration["reported"][field]["state"], "unreported");
        assert!(configuration["reported"][field]["current"].is_null());
        assert_eq!(configuration["reported"][field]["offered"], json!([]));
    }
    host.close(&agent);
}

#[test]
fn e06s01_unsupported_and_unconfirmed_settings_end_owned_peer_without_prompt() {
    for (mode, requested, failure) in [
        (
            "e06-settings",
            json!({"model":"fixture/premium-not-offered"}),
            "unsupported-model",
        ),
        (
            "e06-settings",
            json!({"mode":"never-offered"}),
            "unsupported-mode",
        ),
        (
            "e06-settings",
            json!({"effort":"premium-not-offered"}),
            "unsupported-effort",
        ),
        (
            "e06-settings-no-effort",
            json!({"effort":"low"}),
            "unsupported-effort",
        ),
        (
            "e06-settings-ambiguous",
            json!({"effort":"low"}),
            "ambiguous-effort",
        ),
        (
            "e06-settings",
            json!({"options":{"maxTokens":"2000"}}),
            "unsupported-option",
        ),
        (
            "e06-settings-reject",
            json!({"model":"fixture/cheap"}),
            "setting-rejected",
        ),
        (
            "e06-settings-omit",
            json!({"model":"fixture/cheap"}),
            "setting-unconfirmed",
        ),
        (
            "e06-settings-drift",
            json!({"model":"fixture/cheap","effort":"low"}),
            "setting-mismatch",
        ),
        (
            "e06-settings",
            json!({"effort":"low","options":{"thinking":"high"}}),
            "conflicting-settings",
        ),
        (
            "e06-settings",
            json!({"model":"fixture/cheap","options":{"model":"fixture/other"}}),
            "conflicting-settings",
        ),
        (
            "e06-settings",
            json!({"mode":"plan","options":{"mode":"default"}}),
            "conflicting-settings",
        ),
    ] {
        eprintln!("startup settings case mode={mode} requested={requested}");
        let host = Host::new();
        let mut agent = open(&host, mode, requested.clone());
        let failed = refresh(&host, &mut agent, "failed");
        assert_eq!(failed["configuration"]["requested"], requested, "{mode}");
        assert_eq!(failed["configuration"]["state"], "failed", "{mode}");
        assert_eq!(failed["configuration"]["failure"], failure, "{mode}");
        host.reaped(&agent, "peer.pid");
        host.until(
            &format!(
                "startup settings cleanup mode={mode} requested={requested} before fixture fallback"
            ),
            || (host.inspect(&agent)["resource"]["cleanup_complete"] == true).then_some(()),
        );
        assert!(!agent.root.join("effect.json").exists());
    }
}

#[test]
fn e06s01_settings_input_is_closed_and_bounded_before_spawn() {
    let host = Host::new();
    let project = host.call(json!({"op":"snapshot"}))["projects"][0]["id"].clone();
    let controller = host
        .call(json!({"op":"controller-create","role":"external","label":"Settings validation"}));
    for settings in [
        Value::Null,
        json!([]),
        json!({"unknown":"x"}),
        json!({"model":""}),
        json!({"mode":"x\n"}),
        json!({"effort":false}),
        json!({"options":{"x":true}}),
        json!({"options":{"":"x"}}),
        json!({"model":"x".repeat(257)}),
    ] {
        host.deny(
            json!({"op":"acp-open","project":project,"controller":controller["controller"],
            "command":["/bin/false"],"settings":settings}),
            &["invalid-request"],
        );
    }
    let options: serde_json::Map<String, Value> = (0..65)
        .map(|n| (format!("option-{n}"), json!("choice")))
        .collect();
    host.deny(
        json!({"op":"acp-open","project":project,"controller":controller["controller"],
        "command":["/bin/false"],"settings":{"options":options}}),
        &["invalid-request"],
    );
    assert!(
        host.call(json!({"op":"snapshot"}))["agent_views"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["kind"] != "acp")
    );
}

#[test]
fn e06s01_omp_optional_ask_form_and_exact_permission_responses_share_original_work() {
    let host = Host::new();
    let mut agent = open(
        &host,
        "e06-omp",
        json!({"model":"fixture/cheap","effort":"low"}),
    );
    host.ready(&mut agent);
    let tuple = agent.target.clone();
    let pid = fs::read_to_string(agent.root.join("peer.pid")).unwrap();
    let work = host.prompt(&agent, "omp-shaped-turn", "add 17 25");
    let form = host.until("OMP optional oneOf question", || {
        host.inspect(&agent)["content"]["interactions"]
            .as_array()?
            .iter()
            .find(|i| i["kind"] == "form" && i["state"] == "pending")
            .cloned()
    });
    assert!(form["schema"].get("required").is_none());
    host.call(form_answer(
        &agent,
        &form,
        "omp-exact-form",
        "accept",
        Some(json!({"q0":"Proceed"})),
    ));
    let permission = host.until("OMP original permission", || {
        host.inspect(&agent)["content"]["interactions"]
            .as_array()?
            .iter()
            .find(|i| i["kind"] == "permission" && i["state"] == "pending")
            .cloned()
    });
    host.deny(
        permission_answer(&agent, &permission, "no-fabrication", "unoffered"),
        &["invalid-request"],
    );
    host.call(permission_answer(
        &agent,
        &permission,
        "omp-deny",
        "reject_once",
    ));
    host.completed(&agent, &work["work_id"]);
    let permission_effect: Value =
        serde_json::from_slice(&fs::read(agent.root.join("omp-permission.json")).unwrap()).unwrap();
    assert_eq!(permission_effect["allowed"], false);
    assert_eq!(agent.effect()["value"], 0);
    assert_eq!(agent.target, tuple);
    assert_eq!(
        fs::read_to_string(agent.root.join("peer.pid")).unwrap(),
        pid
    );
    let reported = host.inspect(&agent)["configuration"]["reported"].clone();
    assert_eq!(
        reported["mode"]["current"], "plan",
        "actual spontaneous mode update is projected"
    );
    assert_eq!(
        reported["effort"]["current"], "high",
        "actual spontaneous config update is projected"
    );
    assert_eq!(host.inspect(&agent)["configuration"]["state"], "changed");
    host.close(&agent);
}

#[test]
fn e06s01_omp_permission_cancel_request_withdraws_only_original_responder() {
    let host = Host::new();
    let mut agent = open(&host, "e06-omp-cancel", json!({}));
    host.ready(&mut agent);
    let work = host.prompt(&agent, "cancelled-original-turn", "add 17 25");
    let form = host.until("OMP form", || {
        host.inspect(&agent)["content"]["interactions"]
            .as_array()?
            .iter()
            .find(|i| i["kind"] == "form" && i["state"] == "pending")
            .cloned()
    });
    host.call(form_answer(
        &agent,
        &form,
        "before-permission-cancel",
        "accept",
        Some(json!({"q0":"Proceed"})),
    ));
    host.barrier(&agent, "omp-permission");
    let permission = host.until("OMP permission before withdrawal", || {
        host.inspect(&agent)["content"]["interactions"]
            .as_array()?
            .iter()
            .find(|i| i["kind"] == "permission" && i["state"] == "pending")
            .cloned()
    });
    agent.release("omp-permission");
    host.completed(&agent, &work["work_id"]);
    host.deny(
        permission_answer(&agent, &permission, "no-stale-allow", "allow_once"),
        &["stale-interaction"],
    );
    let inspect = host.inspect(&agent);
    let retired = inspect["content"]["interactions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["interaction_id"] == permission["interaction_id"])
        .unwrap();
    assert_eq!(retired["state"], "cancelled");
    assert!(
        retired["selection"].is_null(),
        "peer cancellation is not a human grant"
    );
    assert!(
        !inspect["work"]
            .as_array()
            .unwrap()
            .iter()
            .any(|work| work["op"] == "acp-interrupt")
    );
    assert_eq!(agent.effect()["value"], 0);
    host.close(&agent);
}

#[test]
fn e06s01_semantic_settings_use_advertised_ids_and_post_model_effort_choices() {
    for (mode, effort_id) in [
        ("e06-settings-categories", "depth"),
        ("e06-settings-model-dependent", "thinking"),
    ] {
        let host = Host::new();
        let mut agent = open(&host, mode, json!({"model":"fixture/cheap","effort":"low"}));
        host.ready(&mut agent);
        let configuration = host.inspect(&agent)["configuration"].clone();
        assert_eq!(
            configuration["reported"]["model"]["current"],
            "fixture/cheap"
        );
        assert_eq!(configuration["reported"]["effort"]["current"], "low");
        assert_eq!(configuration["reported"]["effort"]["option_id"], effort_id);
        let sequence: Value =
            serde_json::from_slice(&fs::read(agent.root.join("settings-sequence.json")).unwrap())
                .unwrap();
        assert_eq!(
            sequence[0]["configId"],
            if mode == "e06-settings-categories" {
                "engine"
            } else {
                "model"
            }
        );
        assert_eq!(sequence[1]["configId"], effort_id);
        host.close(&agent);
    }
}

#[test]
fn e06s01_close_during_unacknowledged_setting_never_admits_a_prompt_or_restarts() {
    let host = Host::new();
    let mut agent = open(&host, "e06-settings-hold", json!({"model":"fixture/cheap"}));
    host.barrier(&agent, "settings");
    refresh(&host, &mut agent, "starting");
    let tuple = agent.target.clone();
    host.close(&agent);
    assert_eq!(host.inspect(&agent)["target"], tuple);
    assert_eq!(host.inspect(&agent)["configuration"]["state"], "failed");
    assert!(!agent.root.join("effect.json").exists());
    assert!(!agent.root.join("fixture-fallback").exists());
}

#[test]
fn e06s01_raw_launch_preserves_agent_defaults_without_unsolicited_setting_rpcs() {
    let host = Host::new();
    let mut agent = open(&host, "e06-settings", json!({}));
    host.ready(&mut agent);
    let configuration = host.inspect(&agent)["configuration"].clone();
    assert_eq!(configuration["state"], "agent-default");
    assert_eq!(configuration["requested"], json!({}));
    assert_eq!(
        configuration["reported"]["model"]["current"],
        "fixture/other"
    );
    assert_eq!(configuration["reported"]["mode"]["current"], "default");
    assert_eq!(configuration["reported"]["effort"]["current"], "off");
    assert!(!agent.root.join("settings-sequence.json").exists());
    let work = host.prompt(&agent, "unchanged-raw-launch", "add 17 25");
    host.completed(&agent, &work["work_id"]);
    assert_eq!(agent.effect()["value"], 42);
    host.close(&agent);
}

#[test]
fn e06s01_legacy_mode_null_ack_reaches_ready_but_config_requires_confirmation() {
    let host = Host::new();
    let mut agent = open(&host, "e06-settings-null-ack", json!({"mode":"plan"}));
    let id = agent.target["id"].as_str().unwrap();
    let settled = host.until("legacy mode acknowledgement settles startup", || {
        host.view(id).filter(|view| view["lifecycle"] != "starting")
    });
    assert_eq!(
        settled["lifecycle"], "ready",
        "valid null legacy mode acknowledgement ended startup: {settled}"
    );
    host.ready(&mut agent);
    let exact = agent.target.clone();
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let configuration = host.inspect(&agent)["configuration"].clone();
    assert_eq!(configuration["state"], "applied");
    assert_eq!(configuration["requested"], json!({"mode":"plan"}));
    assert_eq!(configuration["reported"]["mode"]["current"], "plan");
    let work = host.prompt(&agent, "after-null-mode-ack", "add 17 25");
    host.completed(&agent, &work["work_id"]);
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(agent.effect()["session_id"], exact["session_id"]);
    assert_eq!(host.inspect(&agent)["target"], exact);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    host.close(&agent);

    // A preceding notification does not replace the config setter's required
    // full value-confirming response. The same null wire shape is not valid.
    let mut unconfirmed = open(&host, "e06-settings-null-ack", json!({"effort":"low"}));
    let failed = refresh(&host, &mut unconfirmed, "failed");
    assert_eq!(failed["configuration"]["failure"], "setting-unconfirmed");
    host.reaped(&unconfirmed, "peer.pid");
    host.until("unconfirmed config owned cleanup before fallback", || {
        (host.inspect(&unconfirmed)["resource"]["cleanup_complete"] == true).then_some(())
    });
    assert!(!unconfirmed.root.join("effect.json").exists());
}

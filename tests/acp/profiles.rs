use super::*;

fn fixture(host: &Host, mode: &str, defaults: Value) -> (PathBuf, Value) {
    let root = host
        .root
        .join(format!("peer-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&root).unwrap();
    let peer = Path::new(env!("CARGO_BIN_EXE_amux"))
        .parent()
        .unwrap()
        .join("examples/acp_fixture");
    let profile = json!({"label":"Daily fixture","command":[peer,root,mode],"defaults":defaults});
    (root, profile)
}

fn controller(host: &Host) -> Value {
    host.call(json!({"op":"controller-create","role":"external","label":"Profile consumer"}))
}

fn agent(opened: Value, root: PathBuf, controller: &Value) -> Agent {
    Agent {
        target: opened["target"].clone(),
        opened,
        root,
        controller_id: controller["controller_id"].clone(),
    }
}

#[test]
fn e06s02_cli_profile_overrides_defaults_and_resolves_project_name_on_one_instance() {
    let host = Host::new();
    let (root, profile) = fixture(
        &host,
        "e06-settings",
        json!({
        "model":"fixture/default","effort":"high","options":{"fixture-detail":"not-selected"}}),
    );
    host.call(json!({"op":"config","config":{"acp_profiles":{"fixture":profile}}}));
    let snapshot = host.call(json!({"op":"snapshot"}));
    let project = &snapshot["projects"][0];
    let controller = controller(&host);
    let output = Command::new(env!("CARGO_BIN_EXE_amux"))
        .arg("--socket")
        .arg(&host.socket)
        .args([
            "acp",
            "start",
            "fixture",
            "--project",
            project["name"].as_str().unwrap(),
            "--model",
            "fixture/cheap",
            "--mode",
            "plan",
            "--effort",
            "low",
            "--option",
            "fixture-detail=brief",
            "--controller",
        ])
        .arg(controller["controller"].to_string())
        .env_clear()
        .env("HOME", &host.root)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "explicit-controller profile CLI failed"
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["ok"], true);
    let mut agent = agent(response["result"].clone(), root, &controller);
    host.ready(&mut agent);
    let view = host.view(agent.target["id"].as_str().unwrap()).unwrap();
    assert_eq!(view["project_id"], project["id"]);
    assert_eq!(view["configuration"]["state"], "applied");
    assert_eq!(
        view["configuration"]["reported"]["model"]["current"],
        "fixture/cheap"
    );
    assert_eq!(view["configuration"]["reported"]["mode"]["current"], "plan");
    assert_eq!(
        view["configuration"]["reported"]["effort"]["current"],
        "low"
    );
    let work = host.prompt(&agent, "profile-configured-effect", "add 17 25");
    host.completed(&agent, &work["work_id"]);
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(agent.effect()["session_id"], agent.target["session_id"]);
    assert_eq!(
        host.call(json!({"op":"snapshot"}))["agent_views"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "one profile launch must not create duplicate live agents"
    );
    host.close(&agent);
    assert!(!agent.root.join("fixture-fallback").exists());
}

#[test]
fn e06s02_cli_explicit_json_options_preserve_delimited_ids_and_choices() {
    let host = Host::new();
    let (root, profile) = fixture(
        &host,
        "e06-settings-delimited",
        json!({"options":{"fixture,detail=level":"full"}}),
    );
    host.call(json!({"op":"config","config":{"acp_profiles":{"fixture":profile}}}));
    let project = host.call(json!({"op":"snapshot"}))["projects"][0]["id"].clone();
    let controller = controller(&host);
    let launch = |options: &str| {
        Command::new(env!("CARGO_BIN_EXE_amux"))
            .arg("--socket")
            .arg(&host.socket)
            .args(["acp", "start", "fixture", "--project"])
            .arg(project.as_str().unwrap())
            .args(["--options-json", options, "--controller"])
            .arg(controller["controller"].to_string())
            .env_clear()
            .env("HOME", &host.root)
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap()
    };
    let duplicate =
        launch(r#"{"fixture,detail=level":"full","fixture,detail=level":"brief,exact=value"}"#);
    assert!(!duplicate.status.success());
    assert_eq!(
        host.call(json!({"op":"snapshot"}))["agent_views"],
        json!([])
    );
    let output = launch(r#"{"fixture,detail=level":"brief,exact=value"}"#);
    assert!(
        output.status.success(),
        "lossless explicit CLI options failed"
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["ok"], true);
    let mut agent = agent(response["result"].clone(), root, &controller);
    host.ready(&mut agent);
    let exact = agent.target.clone();
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let configuration = host.inspect(&agent)["configuration"].clone();
    assert_eq!(configuration["state"], "applied");
    assert_eq!(
        configuration["requested"]["options"],
        json!({"fixture,detail=level":"brief,exact=value"})
    );
    let selected = configuration["reported"]["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["id"] == "fixture,detail=level")
        .unwrap();
    assert_eq!(selected["current"], "brief,exact=value");
    let work = host.prompt(&agent, "explicit-json-options-effect", "add 17 25");
    host.completed(&agent, &work["work_id"]);
    assert_eq!(agent.effect()["value"], 42);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(agent.effect()["session_id"], exact["session_id"]);
    assert_eq!(host.inspect(&agent)["target"], exact);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    host.close(&agent);
    assert!(!agent.root.join("fixture-fallback").exists());
}

#[test]
fn e06s02_unavailable_or_conflicting_profile_never_reserves_and_metadata_has_no_argv() {
    let host = Host::new();
    host.call(json!({"op":"config","config":{"acp_profiles":{"missing":{
        "label":"Not installed","command":[host.root.join("no-such-adapter")],"defaults":{}}}}}));
    let profiles = host.call(json!({"op":"acp-profiles"}));
    let missing = profiles["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "missing")
        .unwrap();
    assert_eq!(missing["available"], false);
    assert!(
        profiles["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .all(|profile| profile.as_object().unwrap().keys().all(|key| [
                "id",
                "label",
                "available",
                "defaults"
            ]
            .contains(&key.as_str()))),
        "profile discovery must not disclose argv or environment"
    );
    let controller = controller(&host);
    let snapshot = host.call(json!({"op":"snapshot"}));
    let project = &snapshot["projects"][0];
    for extra in [
        json!({"agent":"missing"}),
        json!({"agent":"unknown"}),
        json!({"agent":"missing","command":["/bin/false"]}),
    ] {
        let mut request =
            json!({"op":"acp-open","project":project["id"],"controller":controller["controller"]});
        request
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        host.deny(request, &["invalid-request"]);
    }
    host.deny(
        json!({"op":"acp-open","project":project["name"],"controller":controller["controller"],
        "command":["/bin/false"]}),
        &["project-not-found"],
    );
    assert_eq!(
        host.call(json!({"op":"snapshot"}))["agent_views"],
        json!([])
    );
}

#[test]
fn e06s02_rejected_profile_limit_patch_keeps_previous_profile_usable() {
    let host = Host::new();
    let (root, profile) = fixture(&host, "arithmetic", json!({}));
    host.call(json!({"op":"config","config":{"acp_profiles":{"fixture":profile}}}));
    let excessive: serde_json::Map<String, Value> = (0..17)
        .map(|id| (format!("profile-{id}"), profile.clone()))
        .collect();
    host.deny(
        json!({"op":"config","config":{"acp_profiles":excessive}}),
        &["request-failed"],
    );
    let controller = controller(&host);
    let project = host.call(json!({"op":"snapshot"}))["projects"][0]["id"].clone();
    let opened = host.call(json!({"op":"acp-open","agent":"fixture","project":project,
        "controller":controller["controller"]}));
    let mut agent = agent(opened, root, &controller);
    host.ready(&mut agent);
    let work = host.prompt(&agent, "profile-after-invalid-patch", "add 17 25");
    host.completed(&agent, &work["work_id"]);
    assert_eq!(agent.effect()["value"], 42);
    host.close(&agent);
    assert!(!agent.root.join("fixture-fallback").exists());
}

#[test]
fn e06s02_symlink_profiles_preserve_absolute_and_path_invocation_identity() {
    let root = fs::canonicalize("/tmp")
        .unwrap()
        .join(format!("amux-acp-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    for name in ["home", "startup", "config", "runtime", "projects", "bin"] {
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
        .env(
            "PATH",
            std::env::join_paths([
                root.join("bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
                PathBuf::from("/usr/sbin"),
                PathBuf::from("/sbin"),
            ])
            .unwrap(),
        )
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
    let host = Host {
        root,
        socket,
        daemon,
    };
    host.until("private profile daemon startup", || {
        exchange(&host.socket, json!({"op":"snapshot"}))
            .ok()
            .filter(|response| response["ok"] == true)
    });
    let wrapper = host.root.join("bin/basename-dispatch");
    fs::write(
        &wrapper,
        concat!(
            "#!/bin/sh\n",
            "case \"${0##*/}\" in\n",
            "  absolute-acp-alias|path-acp-alias) exec \"$@\" ;;\n",
            "  *) exit 64 ;;\n",
            "esac\n",
        ),
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).unwrap();
    let controller = controller(&host);
    let project = host.call(json!({"op":"snapshot"}))["projects"][0]["id"].clone();
    for (id, bare) in [("absolute-acp-alias", false), ("path-acp-alias", true)] {
        let alias = host.root.join("bin").join(id);
        std::os::unix::fs::symlink(&wrapper, &alias).unwrap();
        let (root, mut profile) = fixture(&host, "arithmetic", json!({}));
        profile["command"]
            .as_array_mut()
            .unwrap()
            .insert(0, if bare { json!(id) } else { json!(alias) });
        host.call(json!({"op":"config","config":{"acp_profiles":{(id):profile}}}));
        let opened = host.call(json!({"op":"acp-open","agent":id,"project":project,
            "controller":controller["controller"]}));
        let mut agent = agent(opened, root, &controller);
        host.ready(&mut agent);
        let work = host.prompt(&agent, id, "add 17 25");
        host.completed(&agent, &work["work_id"]);
        assert_eq!(agent.effect()["value"], 42, "{id}");
        assert_eq!(
            agent.effect()["session_id"],
            agent.target["session_id"],
            "{id}"
        );
        host.close(&agent);
        assert!(!agent.root.join("fixture-fallback").exists(), "{id}");
    }
}

#[test]
fn e07_independent_acp_placement_and_failed_admission_leave_source_project_unchanged() {
    let host = Host::new();
    let controller = controller(&host);
    let (root, profile) = fixture(&host, "arithmetic", json!({}));
    host.call(json!({"op":"config","config":{"acp_profiles":{"fixture":profile}}}));
    let source = host.call(json!({"op":"snapshot"}))["projects"][0].clone();
    host.call(
        json!({"op":"project-config","project":source["id"],"config":{
            "hooks":[{"source":"touch inherited-hook-must-not-run","timeout_ms":1000}],
            "recipes":{"source-only":"printf source"}
        }}),
    );
    let before = host.call(json!({"op":"snapshot"}));
    for invalid in [json!(null), json!("true"), json!(1), json!({})] {
        host.deny(
            json!({"op":"acp-open","agent":"fixture","project":source["id"],
            "new_project":invalid,"controller":controller["controller"]}),
            &["invalid-request"],
        );
    }
    let failed = exchange(
        &host.socket,
        json!({"op":"acp-open","agent":"fixture",
        "project":source["id"],"new_project":true,
        "controller":{"kind":"controller","ref":"not-an-admitted-controller"}}),
    )
    .unwrap();
    assert_eq!(failed["ok"], false, "{failed}");
    let after_failure = host.call(json!({"op":"snapshot"}));
    assert_eq!(after_failure["projects"], before["projects"]);
    assert_eq!(after_failure["agent_views"], before["agent_views"]);
    assert!(!root.join("peer.pid").exists());

    let opened = host.call(
        json!({"op":"acp-open","agent":"fixture","project":source["id"],
        "new_project":true,"controller":controller["controller"]}),
    );
    let mut independent = agent(opened, root, &controller);
    host.ready(&mut independent);
    let view = host
        .view(independent.target["id"].as_str().unwrap())
        .unwrap();
    assert_ne!(view["project_id"], source["id"]);
    let placed = host.call(json!({"op":"snapshot"}));
    let new_project = placed["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|project| project["id"] == view["project_id"])
        .unwrap();
    assert_eq!(new_project["saved"], false);
    assert_eq!(new_project["context"], source["context"]);
    assert_eq!(new_project["hooks"], json!([]));
    assert_eq!(new_project["recipes"], json!({}));
    assert!(new_project["main_terminal"].is_null());
    let retained_source = placed["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|project| project["id"] == source["id"])
        .unwrap();
    let old_source = before["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|project| project["id"] == source["id"])
        .unwrap();
    assert_eq!(retained_source, old_source);
    assert!(!host.root.join("inherited-hook-must-not-run").exists());
    assert!(
        !host
            .root
            .join("projects")
            .join(view["project_id"].as_str().unwrap())
            .exists()
    );
    let work = host.prompt(&independent, "independent-placement", "add 17 25");
    host.completed(&independent, &work["work_id"]);
    assert_eq!(independent.effect()["value"], 42);
    host.close(&independent);
}

#[test]
fn e07_saved_live_acp_only_project_reuses_original_callback_without_native_or_hooks() {
    let host = Host::new();
    let controller = controller(&host);
    let (root, profile) = fixture(&host, "form-prompt", json!({}));
    host.call(json!({"op":"config","config":{"acp_profiles":{"fixture":profile}}}));
    let source = host.call(json!({"op":"snapshot"}))["projects"][0]["id"].clone();
    let opened = host.call(json!({"op":"acp-open","agent":"fixture","project":source,
        "new_project":true,"controller":controller["controller"]}));
    let mut independent = agent(opened, root, &controller);
    host.ready(&mut independent);
    let tuple = independent.target.clone();
    let pid = fs::read(independent.root.join("peer.pid")).unwrap();
    let preview = host.call(json!({"op":"project-save-preview","terminal":tuple["id"]}));
    let saved = host.call(json!({"op":"project-save","terminal":tuple["id"],
        "name":"saved-acp-only","expected_context":preview["context"]}));
    assert_eq!(saved["id"], preview["project"]);
    assert_eq!(saved["saved"], true);
    assert!(saved["main_terminal"].is_null());
    let hook_effect = host.root.join("saved-acp-project-open");
    let hook_source = format!("printf 'opened\\n' >> '{}'", hook_effect.display());
    host.call(
        json!({"op":"project-config","project":saved["id"],"config":{
        "hooks":[{"source":hook_source,"timeout_ms":1000}]}}),
    );

    let prompt = host.prompt(&independent, "pending-before-project-open", "add 17 25");
    let form = pending(&host, &independent, "form");
    let before = host.inspect(&independent);
    let snapshot = host.call(json!({"op":"snapshot"}));
    let native_ids: Vec<_> = snapshot["terminals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|terminal| terminal["id"].clone())
        .collect();
    let view = host.view(tuple["id"].as_str().unwrap()).unwrap();
    assert_eq!(view["project_id"], saved["id"]);
    assert_eq!(snapshot["agent_views"].as_array().unwrap().len(), 1);

    for selector in [&saved["id"], &saved["name"]] {
        let reopened = host.call(json!({"op":"project-open","project":selector}));
        assert_eq!(reopened["project"]["id"], saved["id"]);
        assert_eq!(reopened["reattached"], true);
        assert!(reopened.get("terminal").is_none());
    }
    let reopened = host.call(json!({"op":"snapshot"}));
    let reopened_native_ids: Vec<_> = reopened["terminals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|terminal| terminal["id"].clone())
        .collect();
    assert_eq!(reopened_native_ids, native_ids);
    assert_eq!(reopened["agent_views"], snapshot["agent_views"]);
    let inspected = host.inspect(&independent);
    assert_eq!(inspected["target"], tuple);
    assert_eq!(inspected["foreground"], before["foreground"]);
    assert_eq!(inspected["work"], before["work"]);
    assert_eq!(inspected["interactions"], before["interactions"]);
    assert_eq!(fs::read(independent.root.join("peer.pid")).unwrap(), pid);
    assert!(!pid_gone(
        std::str::from_utf8(&pid).unwrap().parse().unwrap()
    ));
    assert!(!hook_effect.exists());
    host.call(form_answer(
        &independent,
        &form,
        "original-answer-after-project-open",
        "accept",
        Some(json!({"bonus":15})),
    ));
    let completed = host.completed(&independent, &prompt["work_id"]);
    assert_eq!(independent.effect()["rpc_id"], completed["rpc_id"]);
    assert_eq!(independent.effect()["session_id"], tuple["session_id"]);
    assert_eq!(independent.effect()["commands"], 1);
    assert_eq!(independent.effect()["value"], 57);
    host.close(&independent);
    assert!(!independent.root.join("fixture-fallback").exists());

    // A retained ended view is history, not a live represented session. The
    // original unopened-project behavior must still create a shell and run hooks.
    assert_eq!(
        host.view(tuple["id"].as_str().unwrap()).unwrap()["lifecycle"],
        "ended"
    );
    let fresh = host.call(json!({"op":"project-open","project":saved["id"]}));
    assert_ne!(fresh["reattached"], true);
    assert_eq!(fresh["project"]["id"], saved["id"]);
    assert_eq!(fresh["terminal"]["project_id"], saved["id"]);
    let diagnostics = host.until("one original project-open hook completed", || {
        let snapshot = host.call(json!({"op":"snapshot"}));
        let diagnostics: Vec<_> = snapshot["diagnostics"]
            .as_array()?
            .iter()
            .filter(|entry| {
                entry["kind"] == "project-open" && entry["diagnostic"]["source"] == hook_source
            })
            .cloned()
            .collect();
        diagnostics
            .iter()
            .any(|entry| entry["diagnostic"]["ok"] == true)
            .then_some(diagnostics)
    });
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(fs::read_to_string(hook_effect).unwrap(), "opened\n");
    let final_snapshot = host.call(json!({"op":"snapshot"}));
    assert_eq!(
        final_snapshot["terminals"].as_array().unwrap().len(),
        native_ids.len() + 1
    );
    assert_eq!(final_snapshot["agent_views"].as_array().unwrap().len(), 1);
}

#[test]
fn e07_acp_save_preview_uses_canonical_git_root_and_stale_context_is_atomic() {
    let host = Host::new();
    let workspace = host.root.join("workspace");
    let nested = workspace.join("nested");
    fs::create_dir_all(&nested).unwrap();
    let git = Command::new("git")
        .arg("init")
        .arg(&workspace)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(git.status.success());
    let controller = controller(&host);
    let (root, profile) = fixture(&host, "arithmetic", json!({}));
    host.call(json!({"op":"config","config":{"acp_profiles":{"fixture":profile}}}));
    let source = host.call(json!({"op":"snapshot"}))["projects"][0]["id"].clone();
    let opened = host.call(json!({"op":"acp-open","agent":"fixture","project":source,
        "cwd":nested,"new_project":true,"controller":controller["controller"]}));
    let mut independent = agent(opened, root, &controller);
    host.ready(&mut independent);
    let view = host
        .view(independent.target["id"].as_str().unwrap())
        .unwrap();
    let before = host.call(json!({"op":"snapshot"}));
    let disk = fs::read(host.root.join("config/amux/config.json")).unwrap();
    let preview =
        host.call(json!({"op":"project-save-preview","terminal":independent.target["id"]}));
    assert_eq!(
        preview,
        json!({"project":view["project_id"],"context":workspace})
    );
    assert_eq!(
        host.call(json!({"op":"snapshot"}))["projects"],
        before["projects"]
    );
    assert_eq!(
        fs::read(host.root.join("config/amux/config.json")).unwrap(),
        disk
    );
    host.deny(
        json!({"op":"project-save","terminal":independent.target["id"],"name":"reviewed",
        "expected_context":nested}),
        &["context-changed"],
    );
    assert_eq!(
        host.call(json!({"op":"snapshot"}))["projects"],
        before["projects"]
    );
    assert_eq!(
        fs::read(host.root.join("config/amux/config.json")).unwrap(),
        disk
    );
    let saved = host.call(
        json!({"op":"project-save","terminal":independent.target["id"],
        "project":preview["project"],"name":"reviewed","expected_context":preview["context"]}),
    );
    assert_eq!(saved["id"], preview["project"]);
    assert_eq!(saved["context"], preview["context"]);
    assert_eq!(saved["saved"], true);
    assert_eq!(host.inspect(&independent)["target"], independent.target);
    host.close(&independent);
}

#[test]
fn e07_rename_keeps_pending_original_rpc_and_snapshot_attention_without_content() {
    let host = Host::new();
    let mut agent = host.open("form-prompt");
    host.ready(&mut agent);
    let tuple = agent.target.clone();
    let pid = fs::read(agent.root.join("peer.pid")).unwrap();
    let prompt = host.prompt(&agent, "before-rename", "add 17 25");
    let form = pending(&host, &agent, "form");
    let before = host.inspect(&agent);
    host.call(json!({"op":"name","terminal":agent.target["id"],"name":"Reviewed same instance"}));
    let inspected = host.inspect(&agent);
    let view = host.view(agent.target["id"].as_str().unwrap()).unwrap();
    assert_eq!(inspected["name"], "Reviewed same instance");
    assert_eq!(view["name"], inspected["name"]);
    assert_eq!(inspected["target"], tuple);
    assert_eq!(inspected["work"], before["work"]);
    assert_eq!(inspected["interactions"], before["interactions"]);
    assert_eq!(fs::read(agent.root.join("peer.pid")).unwrap(), pid);
    assert_eq!(
        view["attention"],
        json!({"state":"needs-answer","questions":1,"permissions":0})
    );
    assert!(view.get("content").is_none());
    assert!(view.get("authentication").is_none());
    host.call(form_answer(
        &agent,
        &form,
        "answer-after-rename",
        "accept",
        Some(json!({"bonus":15})),
    ));
    let completed = host.completed(&agent, &prompt["work_id"]);
    assert_eq!(agent.effect()["rpc_id"], completed["rpc_id"]);
    assert_eq!(agent.effect()["session_id"], tuple["session_id"]);
    assert_eq!(agent.effect()["commands"], 1);
    assert_eq!(agent.effect()["value"], 57);
    assert_eq!(
        host.view(agent.target["id"].as_str().unwrap()).unwrap()["attention"],
        json!({"state":"finished","questions":0,"permissions":0})
    );
    host.close(&agent);
}

#[test]
fn e07_native_cwd_change_after_save_preview_rejects_without_bookmark_mutation() {
    let host = Host::new();
    host.call(json!({"op":"config","config":{"prefix":"C-a"}}));
    let controller = controller(&host);
    let terminal = host.call(json!({"op":"snapshot"}))["terminals"][0]["id"].clone();
    let state = host.until("native shell settled before explicit claim", || {
        let state = host.call(json!({"op":"state","terminal":terminal}));
        (state["integration"]["shell"]["ready"] == true
            && state["ownership"]["owner"].is_null()
            && state["ownership"]["reservation"].is_null()
            && state["ownership"]["unsettled"]
                .as_array()
                .is_some_and(Vec::is_empty))
        .then_some(state)
    });
    let grant = host.call(json!({"op":"ownership-claim","terminal":terminal,
        "controller":controller["controller"],"observed":state["ownership"]["observed"],
        "mode":"cooperative","when":{"kind":"now"}}));
    assert_eq!(grant["kind"], "control");
    let preview = host.call(json!({"op":"project-save-preview","terminal":terminal}));
    let changed = host.root.join("changed-after-preview");
    fs::create_dir(&changed).unwrap();
    host.call(
        json!({"op":"execute","terminal":terminal,"control":grant["control"],
        "source":format!("cd '{}'",changed.display())}),
    );
    host.until("authoritative native cwd changed", || {
        let state = host.call(json!({"op":"state","terminal":terminal}));
        (state["integration"]["shell"]["ready"] == true
            && state["workspace"]["cwd"] == json!(changed))
        .then_some(())
    });
    host.until(
        "unsaved project observation caught up before atomicity comparison",
        || {
            let snapshot = host.call(json!({"op":"snapshot"}));
            snapshot["projects"]
                .as_array()?
                .iter()
                .any(|project| {
                    project["id"] == preview["project"] && project["context"] == json!(changed)
                })
                .then_some(())
        },
    );
    let before = host.call(json!({"op":"snapshot"}));
    let config_path = host.root.join("config/amux/config.json");
    let disk = fs::read(&config_path).unwrap();
    host.deny(
        json!({"op":"project-save","terminal":terminal,"name":"stale-bookmark",
        "project":preview["project"],"expected_context":preview["context"]}),
        &["context-changed"],
    );
    assert_eq!(
        host.call(json!({"op":"snapshot"}))["projects"],
        before["projects"]
    );
    assert_eq!(fs::read(&config_path).unwrap(), disk);
    let fresh = host.call(json!({"op":"project-save-preview","terminal":terminal}));
    assert_eq!(fresh["project"], preview["project"]);
    assert_eq!(fresh["context"], json!(changed));
    let saved = host.call(
        json!({"op":"project-save","terminal":terminal,"name":"fresh-bookmark",
        "expected_context":fresh["context"]}),
    );
    assert_eq!(saved["id"], preview["project"]);
    assert_eq!(saved["context"], fresh["context"]);
}

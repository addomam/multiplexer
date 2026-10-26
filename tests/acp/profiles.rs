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

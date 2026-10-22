use loh::{
    commands::{self, Command, Kind, Outcome, Service},
    config::{Config, Paths},
};

#[test]
fn catalog_parses_all_command_families_and_rejects_extra_arguments() {
    for entry in commands::DIRECTORY {
        assert!(commands::parse(entry.example).is_ok(), "{}", entry.name);
    }
    assert!(matches!(
        commands::parse("/resume").unwrap(),
        Command::Builtin {
            kind: Kind::Resume,
            ..
        }
    ));
    assert!(matches!(
        commands::parse("/sessions").unwrap(),
        Command::Builtin {
            kind: Kind::Sessions,
            ..
        }
    ));
    for text in [
        "/status extra",
        "/budget 0",
        "/budget 1 2 3",
        "/approve",
        "/answer id",
        "/mcp add remote",
        "/config campaign 500",
        "/permissions allow all",
    ] {
        assert!(commands::parse(text).is_err(), "{text}");
    }
    assert!(matches!(
        commands::parse("/clear").unwrap(),
        Command::Builtin {
            kind: Kind::New,
            ..
        }
    ));
}

#[test]
fn local_commands_work_without_a_provider_and_preserve_configuration() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().join("state"));
    let service = Service::new(&paths, root.path(), None);
    for text in [
        "/help",
        "/doctor",
        "/config",
        "/permissions",
        "/skills",
        "/mcp",
    ] {
        assert!(
            service.execute(commands::parse(text).unwrap()).is_ok(),
            "{text}"
        );
    }
    assert!(matches!(
        service.execute(commands::parse("/new").unwrap()).unwrap(),
        Outcome::Navigate(_)
    ));
    service
        .execute(commands::parse("/config workers 3").unwrap())
        .unwrap();
    assert_eq!(Config::load(&paths.home).unwrap().workers, 3);
}

#[test]
fn skill_arguments_are_literal_and_core_names_cannot_be_shadowed() {
    let root = tempfile::tempdir().unwrap();
    for name in ["status", "explain"] {
        let folder = root.path().join(name);
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(
            folder.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Test skill\n---\nExplain the request."),
        )
        .unwrap();
    }
    let paths = Paths::new(root.path().join("state"));
    let service = Service::new(&paths, root.path(), None);
    assert!(matches!(
        commands::parse("/status").unwrap(),
        Command::Builtin {
            kind: Kind::Status,
            ..
        }
    ));
    let Outcome::Skill(invocation) = service
        .execute(commands::parse("/explain $(touch NEVER) `echo hi` $HOME").unwrap())
        .unwrap()
    else {
        panic!("skill intent")
    };
    assert_eq!(invocation.arguments, "$(touch NEVER) `echo hi` $HOME");
    assert!(!root.path().join("NEVER").exists());
    assert!(
        service
            .execute(commands::parse("/skills run status").unwrap())
            .is_ok()
    );
}

#[test]
fn same_name_skills_require_source_selection_and_keep_literal_arguments() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().join("state"));
    for folder in [
        root.path().join(".agents/skills/explain"),
        paths.home.join("skills/explain"),
    ] {
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("SKILL.md"),
            "---\nname: explain\ndescription: Explain\n---\nSelected body",
        )
        .unwrap();
    }
    let service = Service::new(&paths, root.path(), None);
    let Outcome::ChooseSkill { entries, arguments } = service
        .execute(commands::parse("/explain a \"b\" $(c)").unwrap())
        .unwrap()
    else {
        panic!("source selection")
    };
    assert!(entries.iter().any(|entry| entry.source == "project"));
    assert!(entries.iter().any(|entry| entry.source == "user"));
    let selected = entries.iter().find(|entry| entry.source == "user").unwrap();
    let Outcome::Skill(invocation) = service
        .execute(commands::parse(&format!("/skills run {} {arguments}", selected.id)).unwrap())
        .unwrap()
    else {
        panic!("explicit source")
    };
    assert_eq!(invocation.path, selected.path);
    assert_eq!(invocation.arguments, "a \"b\" $(c)");
}

#[test]
fn changing_defaults_preserves_campaign_and_mcp_and_cannot_edit_secrets_or_cap() {
    use serde_json::json;
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().join("state"));
    paths.prepare().unwrap();
    let mut config = serde_json::to_value(Config::default()).unwrap();
    config["campaign"] = json!({"name":"shared","limit":50_000_000_000u64,"prices":{"input":1,"cached":1,"output":1,"search":1,"fetch":1}});
    config["mcp"] = json!({"local":{"transport":{"type":"stdio","command":"must-not-start","args":[],"environment":[]},"effects":{}}});
    let config: Config = serde_json::from_value(config).unwrap();
    std::fs::write(
        paths.home.join("config.toml"),
        toml::to_string(&config).unwrap(),
    )
    .unwrap();
    let service = Service::new(&paths, root.path(), None);
    let saved_defaults = loh::config::defaults::Snapshot::capture(&config);
    let original = serde_json::to_value(&config).unwrap();
    for text in [
        "/config workers 4",
        "/config context_bytes 8192",
        "/config chrome /Applications/Custom Chrome/chrome",
        "/config user_skills false",
    ] {
        service.execute(commands::parse(text).unwrap()).unwrap();
    }
    let updated = serde_json::to_value(Config::load(&paths.home).unwrap()).unwrap();
    assert_eq!(updated["campaign"], original["campaign"]);
    assert_eq!(updated["mcp"], original["mcp"]);
    assert_eq!(updated["workers"], 4);
    assert_eq!(updated["chrome"], "/Applications/Custom Chrome/chrome");
    let mut resumed = Config::load(&paths.home).unwrap();
    saved_defaults.apply(&mut resumed);
    assert_eq!(resumed.workers, config.workers);
    assert_eq!(resumed.context_bytes, config.context_bytes);
    assert_eq!(resumed.chrome, config.chrome);
    assert_eq!(
        serde_json::to_value(&resumed.campaign).unwrap(),
        original["campaign"]
    );
    let before = std::fs::read(paths.home.join("config.toml")).unwrap();
    for text in [
        "/config workers 17",
        "/config context_bytes 10",
        "/config tokens 0",
    ] {
        assert!(service.execute(commands::parse(text).unwrap()).is_err());
    }
    for text in [
        "/config campaign 100",
        "/config api_key secret",
        "/config limit 100",
    ] {
        assert!(commands::parse(text).is_err());
    }
    assert_eq!(
        std::fs::read(paths.home.join("config.toml")).unwrap(),
        before
    );
}

#[test]
fn session_inspection_export_and_navigation_do_not_enqueue_controls() {
    use loh::{
        domain::EventData,
        runtime::state::{Control, Session},
        store::Store,
    };
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().join("state"));
    paths.prepare().unwrap();
    let mut state = Session::conversation(root.path().into(), "", 1000, 60);
    state.budget.reported.input = 40;
    state.budget.reported.cached = 10;
    let store = Store::open(&paths.database()).unwrap();
    store
        .commit(
            &state.id,
            &state,
            EventData::Diagnostic {
                message: "fixture".into(),
            },
        )
        .unwrap();
    for i in 0..350 {
        store
            .append(
                &state.id,
                EventData::Diagnostic {
                    message: format!("event {i}"),
                },
            )
            .unwrap();
    }
    let service = Service::new(&paths, root.path(), Some(&state.id));
    for text in [
        "/status",
        "/budget",
        "/context",
        "/usage",
        "/goal",
        "/tasks",
        "/permissions",
        "/history 搜索",
        "/tools action",
        "/copy response",
    ] {
        assert!(
            service.execute(commands::parse(text).unwrap()).is_ok(),
            "{text}"
        );
    }
    let Outcome::Local { text, .. } = service.execute(commands::parse("/usage").unwrap()).unwrap()
    else {
        panic!()
    };
    let usage: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(usage["kv_hit_ratio"], 0.25);
    let Outcome::Local { text, .. } = service
        .execute(commands::parse("/context").unwrap())
        .unwrap()
    else {
        panic!()
    };
    assert!(text.contains("not provider tokens"));
    service
        .execute(commands::parse("/export full history.json").unwrap())
        .unwrap();
    let export: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("full history.json")).unwrap())
            .unwrap();
    assert_eq!(export["events"].as_array().unwrap().len(), 351);
    assert!(
        service
            .execute(commands::parse("/export full history.json").unwrap())
            .is_err()
    );
    assert!(matches!(
        service
            .execute(commands::parse("/resume").unwrap())
            .unwrap(),
        Outcome::Control(Control::Resume)
    ));
    assert!(matches!(
        service
            .execute(commands::parse("/sessions").unwrap())
            .unwrap(),
        Outcome::Navigate(commands::Navigation::Sessions)
    ));
    assert!(store.queued(&state.id, 0).unwrap().is_empty());
    let saved: Session = store.load(&state.id).unwrap();
    assert_eq!(
        serde_json::to_value(saved).unwrap(),
        serde_json::to_value(state).unwrap()
    );
}

#[test]
fn metadata_completion_retains_argument_context() {
    assert_eq!(commands::matches("/budget 100")[0].name, "/budget");
    assert_eq!(commands::matches("/sk")[0].name, "/skills");
    assert!(commands::matches("normal text").is_empty());
    assert!(
        matches!(commands::parse("/example a  b\n  c  ").unwrap(),Command::Skill {arguments,..} if arguments=="a  b\n  c  ")
    );
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().join("state"));
    let service = Service::new(&paths, root.path(), None);
    let complete = service.complete("/i").unwrap();
    assert!(
        complete
            .iter()
            .any(|entry| entry.name == "/init" && entry.source.starts_with("bundled"))
    );
    assert!(service.complete("hello").unwrap().is_empty());
    for command in ["/init", "/review"] {
        let Outcome::Skill(invocation) =
            service.execute(commands::parse(command).unwrap()).unwrap()
        else {
            panic!("bundled workflow")
        };
        assert_eq!(invocation.source, "bundled");
        assert!(!invocation.content.is_empty());
    }
}

use loh::{
    commands::{Service, parse},
    config::Paths,
    domain::EventData,
    runtime::state::Session,
    store::Store,
};
use serde_json::json;

#[test]
fn markdown_export_keeps_full_history_tool_bytes_safe_fences_and_provenance() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().join("state"));
    paths.prepare().unwrap();
    let store = Store::open(&paths.database()).unwrap();
    let session = Session::conversation(root.path().into(), "Export this conversation", 10000, 60);
    store
        .commit(
            &session.id,
            &session,
            EventData::Created {
                objective: "Export this conversation".into(),
                workspace: root.path().display().to_string(),
            },
        )
        .unwrap();
    for i in 0..420 {
        store
            .append(
                &session.id,
                EventData::Message {
                    actor: "main".into(),
                    text: format!("answer-{i}"),
                },
            )
            .unwrap();
    }
    store
        .append(
            &session.id,
            EventData::Identity {
                actor: "worker-export".into(),
                response: "response-export".into(),
                turn: "turn-export".into(),
                goal: None,
                action: Some("action-export".into()),
            },
        )
        .unwrap();
    let started = store
        .append(
            &session.id,
            EventData::ActionStarted {
                id: "action-export".into(),
                tool: "shell".into(),
                arguments: json!({"command":"fixture"}),
            },
        )
        .unwrap();
    let raw = "# raw **literal**\n````\nnot a fence close\n".to_owned()
        + &"very long output\n".repeat(350);
    store
        .append(
            &session.id,
            EventData::ActionFinished {
                id: "action-export".into(),
                success: false,
                result: json!({"result":{"data":{"process":{"output":raw,"code":1}}}}),
            },
        )
        .unwrap();
    let service = Service::new(&paths, root.path(), Some(&session.id));
    service
        .execute(parse("/export session.md").unwrap())
        .unwrap();
    let text = std::fs::read_to_string(root.path().join("session.md")).unwrap();
    assert!(
        text.starts_with("# "),
        "Markdown export must not be unconditional JSON"
    );
    for expected in [
        "answer-0",
        "answer-419",
        "worker-export",
        "turn-export",
        "response-export",
        "action-export",
        &started.timestamp,
    ] {
        assert!(text.contains(expected), "missing {expected}");
    }
    assert!(text.contains(&raw));
    assert!(text.contains("`````text\n# raw **literal**"));
    assert!(!text.contains("已折叠"));
    assert!(
        service
            .execute(parse("/export session.md").unwrap())
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("session.md")).unwrap(),
        text
    );
    service
        .execute(parse("/export explicit.json").unwrap())
        .unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("explicit.json")).unwrap()).unwrap();
    assert_eq!(json["events"].as_array().unwrap().len(), 424);
}

#[test]
fn markdown_export_preserves_unfinished_streams_and_rejects_existing_symlink() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().join("state"));
    paths.prepare().unwrap();
    let store = Store::open(&paths.database()).unwrap();
    let session = Session::conversation(root.path().into(), "partial", 1000, 60);
    store
        .commit(
            &session.id,
            &session,
            EventData::Created {
                objective: "partial".into(),
                workspace: root.path().display().to_string(),
            },
        )
        .unwrap();
    store
        .append(
            &session.id,
            EventData::Delta {
                actor: "main".into(),
                text: "unfinished 中文 stream".into(),
            },
        )
        .unwrap();
    let service = Service::new(&paths, root.path(), Some(&session.id));
    service
        .execute(parse("/export transcript").unwrap())
        .unwrap();
    let text = std::fs::read_to_string(root.path().join("transcript")).unwrap();
    assert!(text.starts_with("# "));
    assert!(text.contains("unfinished 中文 stream"));
    std::os::unix::fs::symlink(
        root.path().join("transcript"),
        root.path().join("linked.md"),
    )
    .unwrap();
    assert!(
        service
            .execute(parse("/export linked.md").unwrap())
            .is_err()
    );
}

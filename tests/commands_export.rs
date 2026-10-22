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

#[test]
fn completed_shell_export_retains_persisted_stream_before_the_bounded_final_tail() {
    let full = "UNIQUE_PREFIX_BEFORE_TAIL\n".to_owned() + &"shell log output line\n".repeat(6000);
    assert!(full.len() > 65_536);
    let tail = full[full.len() - 65_536..].to_owned();
    assert!(!tail.contains("UNIQUE_PREFIX_BEFORE_TAIL"));
    let mut data = vec![
        EventData::Identity {
            actor: "worker-long".into(),
            response: "response-long".into(),
            turn: "turn-long".into(),
            goal: None,
            action: Some("action-long".into()),
        },
        EventData::ActionStarted {
            id: "action-long".into(),
            tool: "shell".into(),
            arguments: json!({"command":"emit large fixture output"}),
        },
    ];
    // Match process::drain: every chunk is persisted as a Delta while the final
    // ProcessOutput retains only its last 65,536 bytes.
    for chunk in full.as_bytes().chunks(4096) {
        data.push(EventData::Delta {
            actor: "action-long".into(),
            text: String::from_utf8(chunk.to_vec()).unwrap(),
        });
    }
    let process = loh::tools::process::ProcessOutput {
        pid: 4242,
        code: Some(0),
        cancelled: false,
        timed_out: false,
        output: tail,
    };
    let result = json!({"result":{"data":{"process":process}}});
    data.push(EventData::ActionFinished {
        id: "action-long".into(),
        success: true,
        result: result.clone(),
    });
    let (markdown, _) = export_events(data);
    assert!(
        markdown.contains(&full),
        "full persisted stream must survive a bounded final result"
    );
    assert_eq!(markdown.matches("UNIQUE_PREFIX_BEFORE_TAIL").count(), 1);
    assert!(
        markdown.contains(&serde_json::to_string_pretty(&result).unwrap()),
        "retain the exact final structured result as well"
    );
    assert!(markdown.contains("已持久化流输出"));
    assert!(markdown.contains("最终结果中的进程输出"));
    assert_eq!(markdown.matches("## 工具 · shell").count(), 1);
    assert!(!markdown.contains("## 工具 · shell · 未结算"));
}

#[test]
fn quiet_and_streaming_unfinished_tools_keep_start_details_and_worker_source() {
    let mut data = vec![];
    for (action, worker) in [
        ("quiet-action", "worker-quiet"),
        ("partial-action", "worker-partial"),
    ] {
        data.push(EventData::Identity {
            actor: worker.into(),
            response: format!("response-{action}"),
            turn: format!("turn-{action}"),
            goal: None,
            action: Some(action.into()),
        });
        data.push(EventData::ActionStarted {
            id: action.into(),
            tool: "shell".into(),
            arguments: json!({"command":format!("run {action}")}),
        });
    }
    data.push(EventData::Delta {
        actor: "partial-action".into(),
        text: "partial tool bytes\n".into(),
    });
    let (markdown, events) = export_events(data);
    assert_eq!(markdown.matches("## 工具 · shell · 未结算").count(), 2);
    for (action, worker) in [
        ("quiet-action", "worker-quiet"),
        ("partial-action", "worker-partial"),
    ] {
        assert!(markdown.contains(&format!("- 操作：`{action}`")));
        assert!(markdown.contains(&format!("- 来源：`{worker}`")));
        assert!(!markdown.contains(&format!("- 来源：`{action}`")));
        assert!(markdown.contains(&format!("run {action}")));
        assert!(markdown.contains(&format!("turn-{action}")));
        assert!(markdown.contains(&format!("response-{action}")));
    }
    for event in events
        .iter()
        .filter(|event| matches!(event.data, EventData::ActionStarted { .. }))
    {
        assert!(markdown.contains(&format!(
            "- 开始：{} · 事件 {}",
            event.timestamp, event.sequence
        )));
    }
    assert!(markdown.contains("尚无已持久化流输出"));
    assert!(markdown.contains("partial tool bytes\n"));
    assert!(markdown.contains("未收到最终结果"));
}

fn export_events(data: Vec<EventData>) -> (String, Vec<loh::domain::SessionEvent>) {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().join("state"));
    paths.prepare().unwrap();
    let store = Store::open(&paths.database()).unwrap();
    let session = Session::conversation(root.path().into(), "history fixture", 1000, 60);
    store
        .commit(
            &session.id,
            &session,
            EventData::Created {
                objective: "history fixture".into(),
                workspace: root.path().display().to_string(),
            },
        )
        .unwrap();
    for event in data {
        store.append(&session.id, event).unwrap();
    }
    Service::new(&paths, root.path(), Some(&session.id))
        .execute(parse("/export history.md").unwrap())
        .unwrap();
    (
        std::fs::read_to_string(root.path().join("history.md")).unwrap(),
        store.events(&session.id, 0).unwrap(),
    )
}

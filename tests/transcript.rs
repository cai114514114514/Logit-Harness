use loh::{
    cli::terminal::Transcript,
    domain::{EventData, SessionEvent},
    store::Store,
};
use serde_json::json;

fn event(sequence: u64, data: EventData) -> SessionEvent {
    SessionEvent {
        sequence,
        session: "fixture".into(),
        timestamp: "2026-10-22T10:00:00Z".into(),
        data,
    }
}

#[test]
fn raw_interleaved_tools_settle_once_in_completion_order() {
    let mut projection = Transcript::default();
    for (i, data) in [
        EventData::ActionStarted {
            id: "a".into(),
            tool: "shell".into(),
            arguments: json!({"command":"quiet"}),
        },
        EventData::Delta {
            actor: "a".into(),
            text: "# raw **literal**\n".into(),
        },
        EventData::Message {
            actor: "worker".into(),
            text: "worker done".into(),
        },
        EventData::ActionFinished {
            id: "a".into(),
            success: false,
            result: json!({"error":"# raw **literal**\nfailed"}),
        },
    ]
    .into_iter()
    .enumerate()
    {
        projection.receive(&event(i as u64 + 1, data));
    }
    let settled = projection.drain_settled(80);
    let text = settled
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("# raw **literal**"));
    assert!(text.find("worker done").unwrap() < text.find("failed").unwrap());
    assert!(projection.drain_settled(80).is_empty());
    projection.receive(&event(
        4,
        EventData::ActionFinished {
            id: "a".into(),
            success: false,
            result: json!({"error":"duplicate"}),
        },
    ));
    assert!(projection.drain_settled(80).is_empty());
}

#[test]
fn response_identity_keeps_stream_final_and_tool_only_completion_together() {
    let mut p = Transcript::default();
    for (i, data) in [
        EventData::Identity {
            actor: "main".into(),
            response: "r1".into(),
            turn: "t1".into(),
            goal: None,
            action: None,
        },
        EventData::ModelRequested {
            actor: "main".into(),
            artifact: "r1.request.json".into(),
        },
        EventData::Delta {
            actor: "main".into(),
            text: "hello".into(),
        },
        EventData::ModelFinished {
            actor: "main".into(),
            artifact: "r1.response.json".into(),
            model: "fixture".into(),
        },
        EventData::Message {
            actor: "main".into(),
            text: "hello world".into(),
        },
        EventData::Identity {
            actor: "main".into(),
            response: "r2".into(),
            turn: "t2".into(),
            goal: None,
            action: None,
        },
        EventData::ModelRequested {
            actor: "main".into(),
            artifact: "r2.request.json".into(),
        },
        EventData::ModelFinished {
            actor: "main".into(),
            artifact: "r2.response.json".into(),
            model: "fixture".into(),
        },
    ]
    .into_iter()
    .enumerate()
    {
        p.receive(&event(i as u64 + 1, data));
    }
    assert!(p.active().is_none());
    assert_eq!(
        p.entries()
            .iter()
            .filter(|e| e.body.contains("hello"))
            .count(),
        1
    );
    assert_eq!(p.entries()[0].response.as_deref(), Some("r1"));
}

#[test]
fn paginated_replay_retains_history_older_than_three_hundred_entries() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("events.sqlite")).unwrap();
    for i in 0..420 {
        store
            .append(
                "fixture",
                EventData::Message {
                    actor: "main".into(),
                    text: format!("entry-{i}"),
                },
            )
            .unwrap();
    }
    let mut projection = Transcript::default();
    let mut cursor = 0;
    loop {
        let page = store.event_page("fixture", cursor, 37).unwrap();
        if page.is_empty() {
            break;
        }
        for event in page {
            cursor = event.sequence;
            projection.receive(&event);
        }
    }
    assert_eq!(projection.entries().len(), 420);
    assert!(projection.entries().iter().any(|e| e.body == "entry-0"));
    assert!(projection.recent(80).len() <= 200);
}

#[test]
fn finished_model_does_not_commit_stream_prefix_before_final_message() {
    let mut p = Transcript::default();
    p.receive(&event(
        1,
        EventData::Identity {
            actor: "main".into(),
            response: "r".into(),
            turn: "t".into(),
            goal: None,
            action: None,
        },
    ));
    p.receive(&event(
        2,
        EventData::Delta {
            actor: "main".into(),
            text: "prefix".into(),
        },
    ));
    p.receive(&event(
        3,
        EventData::ModelFinished {
            actor: "main".into(),
            artifact: String::new(),
            model: "fixture".into(),
        },
    ));
    assert!(p.drain_settled(30).is_empty());
    assert!(p.active().is_none());
    p.receive(&event(
        4,
        EventData::Message {
            actor: "main".into(),
            text: "prefix and final".into(),
        },
    ));
    assert_eq!(
        p.drain_settled(30)
            .iter()
            .map(ToString::to_string)
            .collect::<String>()
            .matches("prefix")
            .count(),
        1
    );
}

#[test]
fn recovery_caps_wrapped_rows_and_display_cannot_emit_terminal_controls() {
    let mut p = Transcript::default();
    for i in 1..=30 {
        p.receive(&event(
            i,
            EventData::Message {
                actor: "main".into(),
                text: "中文很长的内容".repeat(500),
            },
        ));
    }
    let rows = p.recent(24);
    assert!(rows.len() <= 200);
    assert!(rows.iter().all(|line| line.width() <= 24));
    p.receive(&event(
        31,
        EventData::ActionStarted {
            id: "raw".into(),
            tool: "shell".into(),
            arguments: json!({"command":"echo raw"}),
        },
    ));
    let bytes = "\u{1b}]52;c;YXNk\u{7}# **literal**";
    p.receive(&event(
        32,
        EventData::ActionFinished {
            id: "raw".into(),
            success: true,
            result: json!({"result":{"data":{"output":bytes}}}),
        },
    ));
    assert_eq!(p.entries().last().unwrap().body, bytes);
    let rendered = p
        .drain_settled(80)
        .iter()
        .map(ToString::to_string)
        .collect::<String>();
    assert!(!rendered.contains('\u{1b}'));
    assert!(!rendered.contains('\u{7}'));
    assert!(rendered.contains("# **literal**"));
}

#[test]
fn replayed_notices_keep_original_time_and_identity_after_local_commands() {
    let notice = event(
        1,
        EventData::Diagnostic {
            message: "saved notice".into(),
        },
    );
    let mut live = Transcript::default();
    live.note("local command");
    live.receive(&notice);
    let mut replay = Transcript::default();
    replay.receive(&notice);
    let a = live.entries().last().unwrap();
    let b = replay.entries().last().unwrap();
    assert_eq!(a.id, b.id);
    assert_eq!(a.started, notice.timestamp);
    assert_eq!(a.started, b.started);
}

#[test]
fn persisted_long_tool_stream_survives_success_failure_cancellation_and_replay() {
    let stream = format!(
        "UNIQUE_EARLY_PREFIX!!\n{}END\n",
        "0123456789\n".repeat(12_000)
    );
    assert_eq!(stream.len(), 132026);
    let tail = &stream[stream.len() - 65_536..];
    assert!(!tail.contains("UNIQUE_EARLY_PREFIX"));
    for result in [
        json!({"result":{"data":{"process":{"output":tail,"code":0,"cancelled":false}}}}),
        json!({"error":"command failed after output"}),
        json!({"result":{"data":{"process":{"output":tail,"code":null,"cancelled":true}}}}),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db")).unwrap();
        let mut live = Transcript::default();
        for data in [
            EventData::Identity {
                actor: "main".into(),
                response: "response".into(),
                turn: "turn".into(),
                goal: None,
                action: Some("long".into()),
            },
            EventData::ActionStarted {
                id: "long".into(),
                tool: "shell".into(),
                arguments: json!({"command":"long fixture"}),
            },
            EventData::Delta {
                actor: "long".into(),
                text: stream.clone(),
            },
            EventData::ActionFinished {
                id: "long".into(),
                success: result["result"]["data"]["process"]["code"] == 0,
                result,
            },
        ] {
            store.append("fixture", data).unwrap();
            for event in store.events("fixture", live.sequence()).unwrap() {
                live.receive(&event);
            }
        }
        let mut replay = Transcript::default();
        for event in store.events("fixture", 0).unwrap() {
            replay.receive(&event);
        }
        for projection in [&live, &replay] {
            let entry = &projection.entries()[0];
            assert!(
                entry.body.starts_with(&stream),
                "settlement lost persisted stream prefix"
            );
            assert_eq!(entry.turn.as_deref(), Some("turn"));
            assert_eq!(entry.response.as_deref(), Some("response"));
            assert!(!entry.running);
            assert!(
                entry
                    .lines(80, true)
                    .iter()
                    .map(ToString::to_string)
                    .collect::<String>()
                    .contains("UNIQUE_EARLY_PREFIX")
            );
        }
        assert_eq!(live.entries()[0].settled, replay.entries()[0].settled);
    }
}

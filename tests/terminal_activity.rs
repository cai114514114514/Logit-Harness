use loh::{
    cli::terminal::{Recent, Screen},
    domain::{EventData, SessionEvent, Status},
    runtime::{ipc::ServerMessage, state::Session},
};
use serde_json::json;
fn event(screen: &mut Screen, sequence: u64, data: EventData) {
    screen.receive(ServerMessage::Event {
        event: SessionEvent {
            sequence,
            session: "fixture".into(),
            timestamp: (chrono::Utc::now() - chrono::Duration::seconds(12)).to_rfc3339(),
            data,
        },
    });
}
#[test]
fn workspace_owner_wait_survives_tool_only_side_response_completion() {
    let mut session = Session::conversation(std::env::temp_dir(), "hello", 10000, 60);
    session.status = Status::Waiting;
    session.owner = Some("owner-session-id".into());
    let mut screen = Screen::default();
    screen.recent.push(Recent {
        id: "owner-session-id".into(),
        objective: "Compile original workspace".into(),
        status: Status::Running,
    });
    screen.receive(ServerMessage::Snapshot {
        state: Box::new(session),
        sequence: 1,
    });
    event(
        &mut screen,
        2,
        EventData::ModelRequested {
            actor: "steer-side".into(),
            artifact: "r.request.json".into(),
        },
    );
    event(
        &mut screen,
        3,
        EventData::ModelFinished {
            actor: "steer-side".into(),
            artifact: "r.response.json".into(),
            model: "fixture".into(),
        },
    );
    let rows = screen.activity_lines().join("\n");
    assert!(rows.contains("等待工作区"));
    assert!(rows.contains("Compile original workspace"));
    assert!(rows.contains("owner-sessio"));
    assert!(rows.contains("/sessions"));
    assert!(!rows.contains("等待模型响应"));
    assert!(!rows.contains("正在理解"));
}
#[test]
fn quiet_commands_and_parallel_actors_have_measured_independent_activity() {
    let mut screen = Screen::default();
    event(
        &mut screen,
        1,
        EventData::ActionStarted {
            id: "command-a".into(),
            tool: "shell".into(),
            arguments: json!({"command":"sleep 30"}),
        },
    );
    event(
        &mut screen,
        2,
        EventData::ModelRequested {
            actor: "verify-1".into(),
            artifact: String::new(),
        },
    );
    let rows = screen.activity_lines().join("\n");
    assert!(
        screen
            .transcript
            .active_lines(110)
            .iter()
            .any(|line| line.to_string().contains("sleep 30"))
    );
    assert!(
        !rows.contains("command-a"),
        "internal action IDs belong in details"
    );
    assert!(rows.contains("独立验收"));
    assert!(rows.contains("已用12s"));
    assert!(rows.contains("距输出12s"));
    event(
        &mut screen,
        3,
        EventData::ActionFinished {
            id: "command-a".into(),
            success: false,
            result: json!({"error":"cancelled"}),
        },
    );
    let rows = screen.activity_lines().join("\n");
    assert!(!rows.contains("sleep 30"));
    assert!(rows.contains("独立验收"));
}
#[test]
fn optional_argument_commands_execute_without_erasing_literal_tails() {
    for command in ["/plan", "/goal", "/tools", "/history", "/review", "/config"] {
        let mut screen = Screen::default();
        screen.input.insert(command);
        assert!(screen.complete(), "{command}");
        assert_eq!(screen.input.text(), command);
    }
    let mut screen = Screen::default();
    screen.input.insert("/review  '$literal' `text`\n中文  ");
    assert!(screen.complete());
    assert_eq!(screen.input.text(), "/review  '$literal' `text`\n中文  ");
}

#[test]
fn cancellation_settlement_stays_visible_alongside_work_after_reconnect() {
    use loh::runtime::view::{Activity, SessionView};
    for status in [Status::Pausing, Status::Stopping] {
        for reconnect in [false, true] {
            let mut screen = Screen::default();
            let mut session = Session::conversation(std::env::temp_dir(), "cancel", 10000, 60);
            session.status = status.clone();
            let mut view = SessionView::from(session);
            view.activities.push(Activity {
                id: "draining-tool".into(),
                actor: "main".into(),
                title: "shell sleep 30".into(),
            });
            if !reconnect {
                event(
                    &mut screen,
                    1,
                    EventData::ActionStarted {
                        id: "draining-tool".into(),
                        tool: "shell".into(),
                        arguments: json!({"command":"sleep 30"}),
                    },
                );
            }
            screen.receive(ServerMessage::View {
                state: Box::new(view),
                sequence: 2,
            });
            let rows = screen.activity_lines().join("\n");
            assert!(rows.contains("等待活动结算"), "{status:?}: {rows}");
            assert!(
                rows.contains("sleep 30")
                    || screen
                        .transcript
                        .active_lines(80)
                        .iter()
                        .any(|line| line.to_string().contains("sleep 30")),
                "remaining work must stay visible: {rows}"
            );
            event(
                &mut screen,
                3,
                EventData::Status {
                    status: Status::Paused,
                    reason: "settled".into(),
                },
            );
            assert!(!screen.activity_lines().join("\n").contains("等待活动结算"));
        }
    }
}

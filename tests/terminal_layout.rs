use loh::{
    cli::terminal::{Input, Screen, render},
    domain::{EventData, SessionEvent, Status},
    runtime::{ipc::ServerMessage, state::Session},
};
use serde_json::json;

fn event(screen: &mut Screen, sequence: u64, data: EventData) {
    screen.receive(ServerMessage::Event {
        event: SessionEvent {
            sequence,
            session: "fixture".into(),
            timestamp: String::new(),
            data,
        },
    });
}

#[test]
fn input_history_returns_the_unsent_draft_and_multiline_arrows_preserve_unicode() {
    let mut input = Input::default();
    input.remember("first");
    input.remember("中文👩‍💻");
    input.insert("unsent");
    input.up();
    assert_eq!(input.text(), "中文👩‍💻");
    input.up();
    assert_eq!(input.text(), "first");
    input.down();
    input.down();
    assert_eq!(input.text(), "unsent");
    input.replace("甲乙\n中文👩‍💻");
    input.up();
    input.insert("!");
    assert_eq!(input.text(), "甲乙!\n中文👩‍💻");
    input.down();
    input.insert("?");
    assert_eq!(input.text(), "甲乙!\n中文👩‍💻?");
}

#[test]
fn command_completion_filters_and_requires_arguments_before_submission() {
    let mut screen = Screen::default();
    screen.input.insert("/bud");
    assert_eq!(screen.menu().len(), 1);
    assert!(screen.complete());
    assert_eq!(screen.input.text(), "/budget");
    let mut screen = Screen::default();
    screen.input.insert("/app");
    assert!(!screen.complete());
    assert_eq!(screen.input.text(), "/approve ");
}

#[test]
fn tool_output_stays_with_its_command_and_failure_is_visible_when_collapsed() {
    let mut screen = Screen::default();
    event(
        &mut screen,
        1,
        EventData::ActionStarted {
            id: "action".into(),
            tool: "check".into(),
            arguments: json!({"command":"cargo test"}),
        },
    );
    event(
        &mut screen,
        2,
        EventData::Delta {
            actor: "action".into(),
            text: "compile progress".into(),
        },
    );
    event(
        &mut screen,
        3,
        EventData::Message {
            actor: "main".into(),
            text: "I will inspect the failing test.".into(),
        },
    );
    event(
        &mut screen,
        4,
        EventData::ActionFinished {
            id: "action".into(),
            success: false,
            result: json!({"result":{"data":{"process":{"output":"assertion failed: expected SQLite"}}}}),
        },
    );
    let text = screen
        .transcript
        .lines(90, false)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(text.matches("检查  cargo test").count(), 1);
    assert!(text.contains("✕"));
    assert!(text.contains("expected SQLite"));
    assert!(text.find("expected SQLite").unwrap() < text.find("I will inspect").unwrap());
    assert!(!text.contains("compile progress"));
    assert!(!text.contains("工具执行成功"));
}

#[test]
fn interrupted_tool_is_never_rendered_as_a_success() {
    let mut screen = Screen::default();
    event(
        &mut screen,
        1,
        EventData::ActionStarted {
            id: "action".into(),
            tool: "shell".into(),
            arguments: json!({"command":"sleep 10"}),
        },
    );
    event(
        &mut screen,
        2,
        EventData::Status {
            status: Status::Paused,
            reason: "user pause".into(),
        },
    );
    let text = screen
        .transcript
        .lines(80, false)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!text.contains('✓'));
    assert!(text.contains('?'));
    assert!(screen.transcript.active().is_none());
}

#[test]
fn editor_cursor_stays_inside_small_and_resized_viewports() {
    let mut screen = Screen::default();
    screen.model = "deepseek-flash".into();
    screen.workspace = "/tmp/project".into();
    screen.input.insert("中文👩‍💻".repeat(40).as_str());
    for (width, height) in [(24, 10), (54, 22), (100, 30)] {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| render(frame, &screen)).unwrap();
        let position = terminal.get_cursor_position().unwrap();
        assert!(
            position.x < width && position.y < height,
            "cursor outside {width}x{height}"
        );
    }
    let session = Session::new(std::env::temp_dir(), "render", 10000, 60);
    screen.receive(ServerMessage::Snapshot {
        state: Box::new(session),
        sequence: 0,
    });
    screen.input.take();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| render(frame, &screen)).unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("KV"));
}

#[test]
fn conversation_modes_dock_the_editor_without_an_active_banner() {
    use loh::runtime::conversation::Mode;
    for (mode, placeholder) in [
        (Mode::Conversation, "继续提问"),
        (Mode::Plan, "补充只读规划"),
        // Narrow editors shorten the prompt; correcting is always offered.
        (Mode::Engineering, "纠正"),
    ] {
        for height in [18, 32, 50] {
            for width in [40, 80, 120] {
                let mut screen = Screen::default();
                let mut session = Session::new(std::env::temp_dir(), "layout", 10000, 60);
                session.mode = mode;
                screen.receive(ServerMessage::Snapshot {
                    state: Box::new(session),
                    sequence: 0,
                });
                let mut terminal =
                    ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height))
                        .unwrap();
                terminal.draw(|frame| render(frame, &screen)).unwrap();
                let buffer = terminal.backend().buffer();
                let text = buffer
                    .content()
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>();
                assert!(
                    !text.contains("████"),
                    "active conversation repeated startup brand"
                );
                assert!(
                    text.replace(' ', "").contains(placeholder),
                    "wrong prompt for {mode:?}: {text}"
                );
                let footer = (0..width)
                    .map(|x| buffer[(x, height - 1)].symbol())
                    .collect::<String>();
                assert!(footer.contains("KV"), "footer not at bottom");
                let cursor = terminal.get_cursor_position().unwrap();
                // One rule above the editor, the footer below it.
                assert_eq!(cursor.y, height - 2);
                assert!(cursor.x < width);
            }
        }
    }
}

#[test]
fn routine_diagnostics_remain_in_details_and_failures_stay_visible() {
    let mut screen = Screen::default();
    event(
        &mut screen,
        1,
        EventData::Diagnostic {
            message: "verified goal archived; ready for a new conversation turn".into(),
        },
    );
    event(
        &mut screen,
        2,
        EventData::Diagnostic {
            message: "provider failed: connection refused".into(),
        },
    );
    let compact = screen
        .transcript
        .lines(80, false)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let full = screen
        .transcript
        .lines(80, true)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!compact.contains("verified goal archived"));
    assert!(full.contains("verified goal archived"));
    assert!(compact.contains("provider failed"));
    assert!(!compact.contains("系统 · 系统"));
}

#[test]
fn narrow_blockers_show_whole_action_ids_or_an_explicit_details_route() {
    use loh::runtime::{state::Question, view::SessionView};
    let question = "12345678-1234-1234-1234-123456789012";
    let approval = "87654321-4321-4321-4321-210987654321";
    for menu in [false, true] {
        let session = Session::new(std::env::temp_dir(), "blocked", 10000, 60);
        let mut view = SessionView::from(session);
        view.questions.insert(
            question.into(),
            Question {
                target: None,
                text: "请确认接下来的工作范围".into(),
                requirements: Default::default(),
                answer: None,
            },
        );
        view.approval_actions
            .push(format!("shell · /approve {approval}"));
        let mut screen = Screen::default();
        screen.receive(ServerMessage::View {
            state: Box::new(view),
            sequence: 1,
        });
        if menu {
            screen.input.insert("/");
        }
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 18)).unwrap();
        terminal.draw(|frame| render(frame, &screen)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
            .replace(' ', "");
        for (command, id) in [("/answer", question), ("/approve", approval)] {
            if text.contains(command) {
                assert!(text.contains(id), "partial actionable command: {text}");
            } else {
                assert!(text.contains("/status"), "missing details route: {text}");
            }
        }
        assert!(!text.contains("/approveshell"));
        assert_eq!(terminal.get_cursor_position().unwrap().y, 16);
    }
}

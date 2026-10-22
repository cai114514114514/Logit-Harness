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
    assert_eq!(text.matches("Check  cargo test").count(), 1);
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

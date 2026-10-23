use loh::{
    cli::terminal::{Screen, render},
    runtime::{
        ipc::ServerMessage,
        state::Session,
        team::{MemberView, Target},
        view::{SessionView, ViewTarget},
    },
};
use ratatui::{Terminal, backend::TestBackend};

fn team() -> Screen {
    let session = Session::new(std::env::temp_dir(), "计算器修复", 1000, 60);
    let mut view = SessionView::from(session);
    view.goal.id = "current-goal".into();
    for i in 0..9 {
        view.team.members.push(MemberView {
            id: format!("member-{i}"),
            name: if i == 8 {
                "乙中文".into()
            } else {
                format!("Ada{i}")
            },
            target: Target::Member {
                id: format!("member-{i}"),
                assignment: None,
            },
            actor: None,
            attempts: vec![],
            task: Some("检查空列表".into()),
            paused: false,
            interrupted: false,
            stopping: false,
            waiting: Some("main".into()),
        });
    }
    let mut screen = Screen::default();
    screen.view = ViewTarget {
        target: view.team.members[8].target.clone(),
        goal: Some("current-goal".into()),
        readonly: false,
    };
    screen.receive(ServerMessage::View {
        state: Box::new(view),
        sequence: 1,
    });
    screen.input.insert("中文草稿");
    screen
}
fn painted(screen: &Screen, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| render(frame, screen)).unwrap();
    use unicode_width::UnicodeWidthStr;
    terminal
        .backend()
        .buffer()
        .content()
        .chunks(width as usize)
        .map(|row| {
            let mut line = String::new();
            let mut x = 0;
            while x < row.len() {
                let symbol = row[x].symbol();
                line.push_str(symbol);
                x += symbol.width().max(1);
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn member_identity_survives_overflow() {
    for (w, h) in [(110, 34), (80, 24), (60, 18), (40, 12)] {
        let output = painted(&team(), w, h);
        assert!(output.contains("乙中文"), "{w}: {output}");
        assert!(output.contains("仅控制此成员"), "{w}: {output}");
        assert!(
            output.contains('+'),
            "overflow must remain discoverable: {output}"
        );
        assert!(
            output.contains("中文草稿"),
            "editor was squeezed out: {output}"
        );
    }
}
#[test]
fn archived_member_keeps_original_team() {
    let mut screen = team();
    screen.view.goal = Some("archived-goal".into());
    screen.view.readonly = true;
    let output = painted(&screen, 110, 34);
    assert!(
        output.contains("归档只读") && output.contains("archived"),
        "{output}"
    );
    assert!(
        !output.contains("计算器修复"),
        "archive incorrectly took the new goal title"
    );
}
#[test]
fn ordinary_chat_has_no_roster_or_recent_task_wall() {
    let mut screen = Screen::default();
    screen.recent.push(loh::cli::terminal::Recent {
        id: "old".into(),
        objective: "old repeated task".into(),
        status: loh::domain::Status::Waiting,
    });
    let output = painted(&screen, 80, 24);
    assert!(!output.contains("old repeated task"));
    assert!(!output.contains("并发") && !output.contains("成员"));
    assert!(output.contains("/setup"));
}
#[test]
fn command_catalog_keeps_sources_without_repeated_tags() {
    let mut screen = Screen::default();
    screen.input.insert("/");
    let output = painted(&screen, 110, 34);
    assert!(output.contains("/"));
    assert!(output.matches("本地操作").count() <= 1, "{output}");
}

#[test]
fn long_team_title_does_not_hide_the_current_assignment_in_a_narrow_view() {
    let mut screen = team();
    screen.session.as_mut().unwrap().goal.original = "很长很长的团队目标".repeat(10);
    let output = painted(&screen, 40, 12);
    assert!(output.contains("检查空列表"), "current task lost: {output}");
}

#[test]
fn main_shows_members_waiting_when_no_actual_work_is_running() {
    let mut screen = team();
    screen.view = ViewTarget::main();
    let output = painted(&screen, 110, 34);
    assert!(
        output.contains("成员等待") && output.contains("main"),
        "wait reason lost: {output}"
    );
}

#[test]
fn unusably_small_team_view_requests_space_instead_of_overlapping_the_editor() {
    assert!(painted(&team(), 40, 5).contains("请扩大终端窗口"));
}

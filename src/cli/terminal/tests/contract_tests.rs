use super::{Action, Detail};
use crate::{
    cli::terminal::Screen,
    domain::{
        Change, Evidence, EvidenceKind,
        contract::{Acceptance, AcceptanceCheck},
    },
    runtime::{ipc::ServerMessage, state::Session},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::BTreeMap;

const ORIGINAL: &str = "把 JSON 存储改成 SQLite，保留 CLI 行为与现有用户数据。";
const CORRECTION: &str =
    "迁移要在一个事务里完成；已有的 JSON 数据要自动导入，重复启动不能重复导入。";

fn evidence(
    session: &Session,
    id: &str,
    kind: EvidenceKind,
    clauses: &[&str],
    passed: bool,
) -> Evidence {
    let versions = session.goal.active_versions();
    Evidence {
        id: id.into(),
        kind,
        requirements: clauses
            .iter()
            .map(|c| ((*c).into(), versions[*c]))
            .collect(),
        epoch: session.goal.epoch,
        steps: BTreeMap::new(),
        files: BTreeMap::new(),
        passed,
        description: "check".into(),
        artifact: None,
    }
}

/// Contract v2: r2 revised, r4 added, r3's old check stale, r4 rejected.
fn screen() -> Screen {
    let mut session = Session::new(std::env::temp_dir(), ORIGINAL, 1_000_000, 600);
    session
        .goal
        .extract(&[
            "把 JSON 存储改成 SQLite".into(),
            "保留 CLI 行为与现有用户数据".into(),
        ])
        .unwrap();
    let old = evidence(&session, "e-old", EvidenceKind::Command, &["r3"], true);
    session.evidence.push(old);
    session
        .goal
        .revise(Change {
            source: CORRECTION.into(),
            updates: [(
                "r2".into(),
                "把 JSON 存储改成 SQLite，迁移在单一事务内完成".into(),
            )]
            .into(),
            additions: vec!["已有 JSON 数据自动导入，重复启动不重复导入".into()],
            replacement: false,
        })
        .unwrap();
    let fresh = evidence(&session, "e-new", EvidenceKind::Command, &["r2"], true);
    session.evidence.push(fresh);
    session.acceptance = Some(Acceptance {
        goal: session.goal.id.clone(),
        revision: 2,
        epoch: session.goal.epoch,
        passed: false,
        checks: vec![AcceptanceCheck {
            requirement: "r4".into(),
            passed: false,
            evidence: vec![],
            reason: "迁移标记改动后没有重跑".into(),
        }],
        summary: String::new(),
        evidence: session.evidence.len(),
        versions: session.goal.active_versions(),
    });
    let mut screen = Screen::default();
    screen.receive(ServerMessage::Snapshot {
        state: Box::new(session),
        sequence: 0,
    });
    screen
}

fn rows(detail: &Detail, width: u16, height: u16) -> Vec<String> {
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| detail.render(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            let mut line = String::new();
            let mut x = 0;
            while x < width {
                let symbol = buffer[(x, y)].symbol();
                line.push_str(symbol);
                x += unicode_width::UnicodeWidthStr::width(symbol).max(1) as u16;
            }
            line.trim_end().to_owned()
        })
        .collect()
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn every_clause_shows_its_source_revisions_evidence_and_verdict() {
    let detail = Detail::open(&screen());
    let rows = rows(&detail, 80, 30);
    let text = rows.join("\n");
    assert!(rows[0].starts_with(" 契约 v2 · 4 条条款 · 修订 1 次"));
    assert!(rows[0].ends_with("Esc 返回"));
    assert!(text.contains(" ›○ r1  v1  把 JSON 存储改成 SQLite，保留 CLI 行为与现有用户数据。"));
    assert!(text.contains("来源  你的完整原话"));
    assert!(text.contains("● r2  v2  把 JSON 存储改成 SQLite，迁移在单一事务内完成"));
    assert!(text.contains("修订  v1「把 JSON 存储改成 SQLite」→ v2"));
    assert!(text.contains("证据  e2 检查  ✓"), "{text}");
    assert!(text.contains("⊘ r3  v1  保留 CLI 行为与现有用户数据"));
    assert!(text.contains("证据  e1 已过期（新增条款）"));
    assert!(text.contains("⊗ r4  v2  已有 JSON 数据自动导入，重复启动不重复导入"));
    assert!(text.contains("原话  「迁移要在一个事务里完成；"));
    assert!(text.contains("验收  未通过：迁移标记改动后没有重跑"));
    assert!(text.contains("状态按条款版本、步骤版本和 epoch 判断；文件指纹在验收时核对。"));
}

#[test]
fn selection_copies_the_users_words_and_search_narrows_the_list() {
    let mut detail = Detail::open(&screen());
    for _ in 0..3 {
        assert!(matches!(detail.key(key(KeyCode::Down)), Action::Stay));
    }
    match detail.key(key(KeyCode::Char('c'))) {
        Action::Copy(text) => assert_eq!(text, CORRECTION),
        _ => panic!("c must copy the selected clause's source"),
    }
    detail.key(key(KeyCode::Char('/')));
    for c in "保留".chars() {
        detail.key(key(KeyCode::Char(c)));
    }
    detail.key(key(KeyCode::Enter));
    let text = rows(&detail, 80, 24).join("\n");
    assert!(text.contains("r3"));
    assert!(!text.contains(" r2  v2"));
    assert!(matches!(detail.key(key(KeyCode::Esc)), Action::Close));
}

#[test]
fn enter_expands_the_selected_clauses_evidence() {
    let mut detail = Detail::open(&screen());
    detail.key(key(KeyCode::Down));
    detail.key(key(KeyCode::Enter));
    let text = rows(&detail, 80, 30).join("\n");
    assert!(text.contains("── e2 e-new ──"));
}

#[test]
fn without_a_goal_the_page_says_how_a_contract_starts() {
    let detail = Detail::open(&Screen::default());
    let text = rows(&detail, 60, 12).join("\n");
    assert!(text.contains("当前没有契约。"));
}

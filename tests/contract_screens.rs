//! Whole screens for the contract-ledger design, checked as character grids.
//!
//! Each scenario feeds real events and a real session view into `Screen`, then
//! stacks committed history above the rendered active region the way a
//! terminal shows them. Run with `--nocapture` to print the grids.
use loh::{
    cli::terminal::{Screen, render},
    domain::{
        EventData, Evidence, EvidenceKind, SessionEvent,
        contract::{ClauseRecord, ContractChanges},
    },
    runtime::{conversation::Mode, ipc::ServerMessage, state::Session},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const ORIGINAL: &str = "把 JSON 存储改成 SQLite，保留 CLI 行为与现有用户数据。";

struct Fixture {
    screen: Screen,
    sequence: u64,
    /// Seconds after the fixture's start, for event timestamps.
    clock: f64,
}

impl Fixture {
    fn new(session: Session) -> Self {
        let mut screen = Screen::default();
        screen.model = "deepseek-flash".into();
        screen.key_present = true;
        screen.connected = true;
        screen.receive(ServerMessage::Snapshot {
            state: Box::new(session),
            sequence: 0,
        });
        Self {
            screen,
            sequence: 0,
            clock: 0.,
        }
    }
    fn at(&mut self, seconds: f64) -> &mut Self {
        self.clock = seconds;
        self
    }
    fn send(&mut self, data: EventData) -> &mut Self {
        self.sequence += 1;
        let start = chrono::DateTime::parse_from_rfc3339("2026-10-26T14:02:00+08:00").unwrap();
        let stamp = start + chrono::Duration::milliseconds((self.clock * 1000.) as i64);
        self.screen.receive(ServerMessage::Event {
            event: SessionEvent {
                sequence: self.sequence,
                session: "fixture".into(),
                timestamp: stamp.to_rfc3339(),
                data,
            },
        });
        self
    }
    fn tool(
        &mut self,
        id: &str,
        tool: &str,
        arguments: Value,
        clauses: &[&str],
        seconds: (f64, f64),
        finish: Option<(bool, Value)>,
    ) -> &mut Self {
        let requirements = clauses
            .iter()
            .map(|id| ((*id).to_owned(), 1))
            .collect::<BTreeMap<_, _>>();
        self.at(seconds.0)
            .send(EventData::ActionScoped {
                id: id.into(),
                actor: "main".into(),
                requirements,
                epoch: 0,
                steps: BTreeMap::new(),
                observed: BTreeMap::new(),
            })
            .send(EventData::ActionStarted {
                id: id.into(),
                tool: tool.into(),
                arguments,
            });
        if let Some((success, result)) = finish {
            self.at(seconds.1).send(EventData::ActionFinished {
                id: id.into(),
                result,
                success,
            });
        }
        self
    }
    /// Committed history stacked above the active region, `columns × rows`.
    fn grid(&mut self, columns: u16, rows: u16) -> Vec<String> {
        let committed = self
            .screen
            .transcript
            .drain_settled(columns as usize - 1)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let active = active(&self.screen, columns, rows);
        let room = rows as usize - active.len();
        let mut grid = committed[committed.len().saturating_sub(room)..].to_vec();
        grid.extend(active);
        grid.iter().map(|l| l.trim_end().to_owned()).collect()
    }
}

/// The active region exactly as tall as the terminal would make it.
fn active(screen: &Screen, columns: u16, rows: u16) -> Vec<String> {
    let height = loh::cli::terminal::active_height(screen, columns, rows);
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(columns, height)).unwrap();
    terminal.draw(|frame| render(frame, screen)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            let mut line = String::new();
            let mut x = 0;
            while x < columns {
                let symbol = buffer[(x, y)].symbol();
                line.push_str(symbol);
                // A wide character's trailing cell is not a second character.
                x += unicode_width::UnicodeWidthStr::width(symbol).max(1) as u16;
            }
            line.trim_end().to_owned()
        })
        .collect()
}

fn print(title: &str, grid: &[String]) {
    println!("======== {title}");
    for row in grid {
        println!("|{row}");
    }
}

fn clause(id: &str, text: &str) -> ClauseRecord {
    ClauseRecord {
        id: id.into(),
        text: text.into(),
        revision: 1,
        active: true,
        source: text.into(),
    }
}

fn command(session: &Session, id: &str, clauses: &[&str], passed: bool) -> Evidence {
    let versions = session.goal.active_versions();
    Evidence {
        id: id.into(),
        kind: EvidenceKind::Command,
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

/// Frame 3 of the design: a failed run on r2 is being rerun.
fn engineering() -> Fixture {
    let mut session = Session::new(std::env::temp_dir(), ORIGINAL, 1_000_000, 600);
    session.mode = Mode::Engineering;
    session.status = loh::domain::Status::Running;
    session
        .goal
        .extract(&[
            "把 JSON 存储改成 SQLite".into(),
            "保留 CLI 行为与现有用户数据".into(),
        ])
        .unwrap();
    let evidence = [
        command(&session, "e-check", &["r3"], true),
        command(&session, "e-run", &["r2"], false),
    ];
    session.evidence.extend(evidence);
    session.budget.reported = loh::domain::Usage {
        input: 22_000,
        output: 5_000,
        cached: 18_480,
    };
    let mut fixture = Fixture::new(session);
    let lines = (1..=48).map(|i| format!("line {i}\n")).collect::<String>();
    fixture
        .send(EventData::User {
            message: ORIGINAL.into(),
            turn: Some("t1".into()),
        })
        .at(1.)
        .send(EventData::Message {
            actor: "main".into(),
            text: "我会把目标记成 3 条可核对的条款，先读存储接口和现有测试。".into(),
        })
        .send(EventData::Contract {
            revision: 1,
            epoch: 0,
            source: ORIGINAL.into(),
            cause: "extract".into(),
            clauses: vec![
                clause("r1", ORIGINAL),
                clause("r2", "把 JSON 存储改成 SQLite"),
                clause("r3", "保留 CLI 行为与现有用户数据"),
            ],
            changes: ContractChanges::default(),
        })
        .tool(
            "a1",
            "read_file",
            json!({"path":"src/storage.rs"}),
            &["r1", "r2", "r3"],
            (2., 2.1),
            Some((true, json!({"result":{"data":{"path":"src/storage.rs","content":lines}}}))),
        )
        .tool(
            "a2",
            "check",
            json!({"command":"cargo test cli"}),
            &["r3"],
            (3., 5.3),
            Some((
                true,
                json!({"result":{"data":{"process":{"code":0,"output":"running 12 tests\ntest result: ok. 12 passed; 0 failed; 0 ignored"}}}}),
            )),
        )
        .tool(
            "a3",
            "apply_patch",
            json!({"edits":[{"path":"src/storage/sqlite.rs"}]}),
            &["r2"],
            (6., 6.2),
            Some((
                true,
                json!({"result":{"data":{"changes":[{"path":"src/storage/sqlite.rs","before":"missing","after":"h","added":112,"removed":0}]}}}),
            )),
        )
        .tool(
            "a4",
            "shell",
            json!({"command":"cargo test storage::tests"}),
            &["r2"],
            (7., 11.1),
            Some((
                false,
                json!({"result":{"data":{"process":{"code":101,"output":"test migration_is_idempotent ... FAILED\nassertion failed: expected 3 rows, found 6"}}}}),
            )),
        )
        .at(12.)
        .send(EventData::Message {
            actor: "main".into(),
            text: "测试发现第二次启动重复导入。接下来在事务里写迁移标记，再重跑测试。".into(),
        })
        .tool(
            "a5",
            "shell",
            json!({"command":"cargo test storage::tests"}),
            &["r2"],
            (13., 0.),
            None,
        );
    fixture
}

#[test]
fn engineering_frame_matches_the_design() {
    let mut fixture = engineering();
    let grid = fixture.grid(80, 24);
    print("3 · 工程执行中", &grid);
    let expected = [
        "▌ 把 JSON 存储改成 SQLite，保留 CLI 行为与现有用户数据。",
        "",
        "│ 我会把目标记成 3 条可核对的条款，先读存储接口和现有测试。",
    ];
    assert_eq!(grid[..3], expected);
    assert!(grid[3].starts_with("╞══ 契约 v1 ══ 摘自你的原话 ══"));
    assert_eq!(
        grid[4],
        "│ r1  把 JSON 存储改成 SQLite，保留 CLI 行为与现有用户数据。           完整原话"
    );
    assert_eq!(
        grid[9..15],
        [
            "│ ✓ 读取  src/storage.rs                                           48 行 · 0.1s",
            "│ ✓ 检查  cargo test cli                                    12 通过 · 2.3s · r3",
            "│ ✓ 修改  src/storage/sqlite.rs                             +112 −0 · 0.2s · r2",
            "│ ✕ 运行  cargo test storage::tests                        退出 101 · 4.1s · r2",
            "│     test migration_is_idempotent ... FAILED",
            "│     assertion failed: expected 3 rows, found 6",
        ]
    );
    assert_eq!(grid[16], "");
    assert_eq!(
        grid[17],
        " 契约 v1   ○ r1   ⊗ r2   ● r3                            有证据 1/3 · /contract"
    );
    assert!(grid[19].starts_with(" ⋯ 运行 cargo test storage::tests · 服务 r2 · 已用 "));
    assert!(grid[20].starts_with("╶─ Engineering · 补充或纠正 ──"));
    assert!(grid[20].ends_with("─╴"));
    assert_eq!(grid[21], " ▸  补充要求、纠正方向，或输入 /");
    assert_eq!(
        grid[22],
        " Ctrl-O 详情 · /contract 契约 · Ctrl-T 成员     deepseek-flash · 27.0k · KV 84%"
    );
    assert_eq!(grid.len(), 23, "the last terminal row stays for the cursor");
}

const CORRECTION: &str =
    "迁移要在一个事务里完成；已有的 JSON 数据要自动导入，重复启动不能重复导入。";

/// The session after the correction: four clauses at contract v2.
fn session_v2() -> Session {
    let correction = CORRECTION;
    let mut session = Session::new(std::env::temp_dir(), ORIGINAL, 1_000_000, 600);
    session.mode = Mode::Engineering;
    session.status = loh::domain::Status::Running;
    session
        .goal
        .extract(&[
            "把 JSON 存储改成 SQLite".into(),
            "保留 CLI 行为与现有用户数据".into(),
        ])
        .unwrap();
    let old = command(&session, "e-check", &["r3"], true);
    session.evidence.push(old);
    session
        .goal
        .revise(loh::domain::Change {
            source: correction.into(),
            updates: [(
                "r2".into(),
                "把 JSON 存储改成 SQLite，迁移在单一事务内完成".into(),
            )]
            .into(),
            additions: vec!["已有 JSON 数据自动导入，重复启动不重复导入".into()],
            replacement: false,
        })
        .unwrap();
    session.budget.reported = loh::domain::Usage {
        input: 26_000,
        output: 5_400,
        cached: 21_320,
    };
    session
}

/// Frame 4: the user's correction becomes revision 2 of the contract.
fn revised() -> Fixture {
    let mut fixture = engineering();
    fixture.grid(80, 24);
    let correction = CORRECTION;
    let session = session_v2();
    let mut r2 = clause("r2", "把 JSON 存储改成 SQLite，迁移在单一事务内完成");
    r2.revision = 2;
    let mut r4 = clause("r4", "已有 JSON 数据自动导入，重复启动不重复导入");
    r4.revision = 2;
    fixture
        .at(20.)
        .send(EventData::ActionFinished {
            id: "a5".into(),
            result: json!({"result":{"data":{"process":{"code":null,"cancelled":true}}}}),
            success: false,
        })
        .send(EventData::User {
            message: correction.into(),
            turn: Some("t2".into()),
        })
        .send(EventData::Revised {
            revision: 2,
            changed: vec!["r2".into(), "r4".into()],
        })
        .send(EventData::Contract {
            revision: 2,
            epoch: 1,
            source: correction.into(),
            cause: "refine".into(),
            clauses: vec![
                clause("r1", ORIGINAL),
                r2,
                clause("r3", "保留 CLI 行为与现有用户数据"),
                r4,
            ],
            changes: ContractChanges {
                updated: vec![loh::domain::contract::ClauseUpdate {
                    id: "r2".into(),
                    before: "把 JSON 存储改成 SQLite".into(),
                    after: "把 JSON 存储改成 SQLite，迁移在单一事务内完成".into(),
                }],
                added: vec!["r4".into()],
                cancelled: vec![loh::domain::contract::CancelledAction {
                    id: "a5".into(),
                    tool: "shell".into(),
                    subject: "cargo test storage::tests".into(),
                    requirements: vec!["r2".into()],
                    settled: true,
                }],
                stale: vec![loh::domain::contract::StaleEvidence {
                    id: "e-check".into(),
                    tool: "check".into(),
                    subject: "cargo test cli".into(),
                    reason: loh::domain::contract::StaleReason::Epoch,
                }],
                ..ContractChanges::default()
            },
        })
        .at(21.)
        .send(EventData::Message {
            actor: "main".into(),
            text: "已按 v2 调整：先把迁移包进事务，再给 r4 补重复启动测试。".into(),
        });
    let sequence = fixture.sequence;
    fixture.screen.receive(ServerMessage::Snapshot {
        state: Box::new(session),
        sequence,
    });
    fixture
}

#[test]
fn a_correction_prints_a_revision_card_and_moves_the_bar_to_v2() {
    let mut fixture = revised();
    let grid = fixture.grid(80, 24);
    print("4 · 纠偏与修订卡", &grid);
    let text = grid.join("\n");
    assert!(text.contains("╞══ 修订 v2 ══ 来自你的补充 ══"));
    assert!(text.contains(
        "│ ~ r2  把 JSON 存储改成 SQLite\n│    →  把 JSON 存储改成 SQLite，迁移在单一事务内完成"
    ));
    assert!(text.contains("│ + r4  已有 JSON 数据自动导入，重复启动不重复导入"));
    assert!(text.contains("│ × 已取消  运行 cargo test storage::tests · 作用于 r2 · 已结算"));
    assert!(text.contains("│ ⊘ 1 项证据过期  e1 检查 cargo test cli · 新增条款使旧证据失效"));
    assert!(text.contains("╘══ 契约 v1 → v2 · /contract 查看原话与修订链 ══"));
    assert!(
        !text.contains("需求修订 2"),
        "the card replaces the one-line notice"
    );
    assert!(text.contains(
        " 契约 v2   ○ r1   ○ r2   ⊘ r3   ○ r4                     有证据 0/4 · /contract"
    ));
}

/// Frames 6 and 7: the verifier's own steps, then its receipt.
fn verified(passed: bool) -> Fixture {
    let mut fixture = revised();
    fixture.grid(80, 24);
    let mut session = session_v2();
    let versions = session.goal.active_versions();
    for id in ["r1", "r2", "r3", "r4"] {
        if passed || id != "r4" {
            let mut review = command(&session, &format!("review-{id}"), &[id], true);
            review.kind = EvidenceKind::Review;
            session.evidence.push(review);
        }
    }
    let check =
        |id: &str, ok: bool, subject: &str, reason: &str| loh::domain::contract::AcceptanceCheck {
            requirement: id.into(),
            passed: ok,
            evidence: if subject.is_empty() {
                vec![]
            } else {
                vec![loh::domain::contract::CitedEvidence {
                    id: format!("e-{id}"),
                    kind: EvidenceKind::Command,
                    tool: "check".into(),
                    subject: subject.into(),
                }]
            },
            reason: reason.into(),
        };
    let checks = vec![
        check("r1", true, "cargo test", ""),
        check("r2", true, "cargo test storage", ""),
        check("r3", true, "cargo test cli", ""),
        check(
            "r4",
            passed,
            if passed {
                "cargo test import_once_on_restart"
            } else {
                ""
            },
            "引用的 e9 绑定 s4 的旧版本；迁移标记改动后没有重跑",
        ),
    ];
    session.acceptance = Some(loh::domain::contract::Acceptance {
        goal: session.goal.id.clone(),
        revision: 2,
        epoch: session.goal.epoch,
        passed,
        checks: checks.clone(),
        summary: String::new(),
        evidence: session.evidence.len(),
        versions,
    });
    session.status = if passed {
        loh::domain::Status::Completed
    } else {
        loh::domain::Status::Running
    };
    for (i, (tool, arguments)) in [
        ("read_file", json!({"path":"src/storage/migrate.rs"})),
        ("read_file", json!({"path":"src/storage.rs"})),
        ("check", json!({"command":"cargo test storage"})),
        ("check", json!({"command":"cargo test cli"})),
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("v{i}");
        let start = 30. + i as f64 * 10.;
        fixture
            .at(start)
            .send(EventData::Identity {
                actor: "verify-1".into(),
                response: "verify-response".into(),
                turn: "t2".into(),
                goal: None,
                action: Some(id.clone()),
            })
            .tool(
                &id,
                tool,
                arguments,
                &[],
                (start, start + 2.),
                Some((true, json!({}))),
            );
    }
    fixture
        .at(94.)
        .send(EventData::Elapsed { seconds: 372 })
        .send(EventData::Usage {
            actor: "main".into(),
            input: 35_000,
            output: 6_200,
            cached: 29_050,
        })
        .send(EventData::Acceptance {
            revision: 2,
            passed,
            checks,
            summary: String::new(),
        });
    let sequence = fixture.sequence;
    fixture.screen.receive(ServerMessage::Snapshot {
        state: Box::new(session),
        sequence,
    });
    fixture
}

#[test]
fn a_passing_verdict_prints_a_receipt_and_stamps_every_clause() {
    let mut fixture = verified(true);
    let grid = fixture.grid(80, 24);
    print("6 · 验收回执", &grid);
    let text = grid.join("\n");
    assert!(text.contains("║ 验收 · 独立上下文 · 读取 2 个文件 · 重跑 2 组检查 · 1m04s"));
    assert!(text.contains("╞══ 验收回执 ══ 独立验收 · 契约 v2 ══"));
    assert!(text.contains("│ 4/4 通过 · 证据绑定 v2 条款版本、步骤版本与文件指纹"));
    assert!(text.contains("╘══ 完成 · 用时 6m12s · 41.2k tokens · KV 83% ══"));
    assert!(text.contains(
        " 契约 v2   ◉ r1   ◉ r2   ◉ r3   ◉ r4                   验收通过 4/4 · /contract"
    ));
    assert!(text.contains("╶─ Engineering · 已完成 ─"));
    assert!(text.contains(" ▸  有新目标或追问，直接输入"));
}

#[test]
fn a_failing_verdict_names_the_clause_and_keeps_completion_withheld() {
    let mut fixture = verified(false);
    let grid = fixture.grid(80, 24);
    print("7 · 验收未通过", &grid);
    let text = grid.join("\n");
    assert!(text.contains("╞══ 验收未通过 ══ 独立验收 · 契约 v2 ══"));
    assert!(text.contains("│       引用的 e9 绑定 s4 的旧版本；迁移标记改动后没有重跑"));
    assert!(text.contains("│ 3/4 通过 · 有条款未通过时不会标记完成"));
    assert!(text.contains("╘══ 已把 1 条失败原因交回 Logit · 继续修复 ══"));
    assert!(text.contains(
        " 契约 v2   ◉ r1   ◉ r2   ◉ r3   ⊗ r4                       验收 3/4 · /contract"
    ));
}

#[test]
fn startup_explains_the_contract_before_anything_is_said() {
    let mut screen = Screen::default();
    screen.model = "deepseek-flash".into();
    screen.key_present = true;
    let rows = active(&screen, 80, 24);
    print("1 · 启动", &rows);
    assert_eq!(
        rows[..4],
        [
            "",
            "  说出目标，Logit 会把它记成一份可核对的契约。",
            "  打招呼和提问不会建立契约；执行中随时插话，离开终端也会继续。",
            "",
        ]
    );
    assert!(rows[4].starts_with("╶─ 新对话 ─"));
    assert_eq!(rows[5], " ▸  输入消息、问题，或需要实现的目标…");
    assert_eq!(
        rows[6],
        " Enter 开始 · / 命令 · /sessions 最近会话 · Ctrl-D 退出"
    );
}

#[test]
fn plain_conversation_has_no_contract_bar() {
    let mut session = Session::conversation(std::env::temp_dir(), "", 1_000_000, 600);
    session.budget.reported = loh::domain::Usage {
        input: 1_800,
        output: 300,
        cached: 1_400,
    };
    let mut fixture = Fixture::new(session);
    fixture
        .send(EventData::User {
            message: "你好".into(),
            turn: Some("c1".into()),
        })
        .send(EventData::Message {
            actor: "main".into(),
            text: "你好！我在 ~/residual。想聊点什么，或者有要在这里完成的事，直接说。".into(),
        });
    let grid = fixture.grid(80, 24);
    print("2 · 普通对话", &grid);
    let text = grid.join("\n");
    assert!(text.contains("▌ 你好\n\n│ 你好！"));
    assert!(!text.contains("契约 v"));
    assert!(text.contains("╶─ 对话 · 等待输入 ─"));
}

fn target(member: &str, task: &str) -> loh::runtime::team::Target {
    loh::runtime::team::Target::Member {
        id: member.into(),
        assignment: Some(loh::runtime::team::AssignmentRef {
            id: task.into(),
            version: 1,
        }),
    }
}

/// Frame 5: two members work in parallel on different clauses.
fn team(view_member: Option<&str>) -> Fixture {
    use loh::runtime::{
        graph::{Step, StepStatus},
        team::{Assignment, AssignmentStatus, MemberView},
        view::{SessionView, ViewTarget},
    };
    let mut session = session_v2();
    let versions = session.goal.active_versions();
    let mut failed = command(&session, "e-import", &["r4"], false);
    failed.requirements = [("r4".into(), versions["r4"])].into();
    session.evidence.push(failed);
    for (id, clauses) in [("s2", vec!["r2", "r4"]), ("s4", vec!["r4"])] {
        session.graph.steps.insert(
            id.into(),
            Step {
                id: id.into(),
                description: id.into(),
                requirements: clauses.into_iter().map(Into::into).collect(),
                dependencies: Default::default(),
                files: Default::default(),
                revision: 0,
                status: StepStatus::Running,
            },
        );
    }
    let mut view = SessionView::from(session);
    for (member, name, task, step, actor, summary) in [
        (
            "member-a",
            "迁移",
            "task-a",
            "s2",
            "worker-a",
            "事务内迁移与迁移标记",
        ),
        (
            "member-b",
            "测试",
            "task-b",
            "s4",
            "worker-b",
            "重复启动导入测试",
        ),
    ] {
        view.team.members.push(MemberView {
            id: member.into(),
            name: name.into(),
            target: target(member, task),
            actor: Some(actor.into()),
            attempts: vec![actor.into()],
            task: Some(summary.into()),
            paused: false,
            interrupted: false,
            stopping: false,
            waiting: (member == "member-b").then(|| "迁移 修复后重跑".into()),
        });
        view.team.assignments.push(Assignment {
            id: task.into(),
            version: 1,
            member: member.into(),
            actor: actor.into(),
            step: step.into(),
            task: summary.into(),
            writable: Default::default(),
            status: AssignmentStatus::Active,
        });
    }
    let mut screen = Screen::default();
    screen.model = "deepseek-flash".into();
    screen.key_present = true;
    // Views always name the goal whose team they show.
    screen.view.goal = Some(view.goal.id.clone());
    if let Some(member) = view_member {
        screen.view = ViewTarget {
            target: target(
                member,
                if member == "member-a" {
                    "task-a"
                } else {
                    "task-b"
                },
            ),
            goal: Some(view.goal.id.clone()),
            readonly: false,
        };
    }
    screen.receive(ServerMessage::View {
        state: Box::new(view),
        sequence: 0,
    });
    screen.connected = true;
    // The client focuses its transcript whenever it switches views.
    let view = screen.view.clone();
    screen.transcript.focus(view);
    let goal = screen.session.as_ref().map(|s| s.goal.id.clone());
    let mut fixture = Fixture {
        screen,
        sequence: 0,
        clock: 0.,
    };
    for (member, task, actor) in [
        ("member-a", "task-a", "worker-a"),
        ("member-b", "task-b", "worker-b"),
    ] {
        fixture.send(EventData::ActorBound {
            actor: actor.into(),
            target: target(member, task),
            goal: goal.clone(),
        });
    }
    let steps = [
        (
            "m1",
            "worker-a",
            "apply_patch",
            json!({"edits":[{"path":"src/storage/migrate.rs"}]}),
            vec!["r2", "r4"],
            Some((
                true,
                json!({"result":{"data":{"changes":[{"path":"src/storage/migrate.rs","before":"a","after":"b","added":64,"removed":3}]}}}),
            )),
        ),
        (
            "t1",
            "worker-b",
            "apply_patch",
            json!({"edits":[{"path":"tests/import_once.rs"}]}),
            vec!["r4"],
            Some((
                true,
                json!({"result":{"data":{"changes":[{"path":"tests/import_once.rs","before":"missing","after":"b","added":41,"removed":0}]}}}),
            )),
        ),
        (
            "t2",
            "worker-b",
            "shell",
            json!({"command":"cargo test import_once"}),
            vec!["r4"],
            Some((
                false,
                json!({"result":{"data":{"process":{"code":101,"output":"assertion `left == right` failed: left: 3, right: 0"}}}}),
            )),
        ),
        (
            "m2",
            "worker-a",
            "shell",
            json!({"command":"cargo test storage"}),
            vec!["r2", "r4"],
            None,
        ),
    ];
    for (i, (id, actor, tool, arguments, clauses, finish)) in steps.into_iter().enumerate() {
        let start = 10. + i as f64 * 5.;
        fixture.at(start).send(EventData::Identity {
            actor: actor.into(),
            response: format!("{actor}-response"),
            turn: "t1".into(),
            goal: None,
            action: Some(id.into()),
        });
        let versions = clauses
            .iter()
            .map(|c| {
                (
                    (*c).to_owned(),
                    if *c == "r3" || *c == "r1" { 1 } else { 2 },
                )
            })
            .collect::<BTreeMap<_, _>>();
        fixture
            .send(EventData::ActionScoped {
                id: id.into(),
                actor: actor.into(),
                requirements: versions,
                epoch: 1,
                steps: BTreeMap::new(),
                observed: BTreeMap::new(),
            })
            .send(EventData::ActionStarted {
                id: id.into(),
                tool: tool.into(),
                arguments,
            });
        if let Some((success, result)) = finish {
            fixture.at(start + 2.2).send(EventData::ActionFinished {
                id: id.into(),
                result,
                success,
            });
        }
    }
    fixture.at(40.).send(EventData::Message {
        actor: "main".into(),
        text: "测试 的失败说明迁移标记没有写入。迁移 正在修，测试 修好后重跑。".into(),
    });
    fixture
}

#[test]
fn the_main_view_names_each_members_work_and_its_clauses() {
    let mut fixture = team(None);
    let grid = fixture.grid(110, 34);
    print("5 · 团队并行（主控视图）", &grid);
    let text = grid.join("\n");
    // Member tool streams stay in member views (the confirmed 10-23 rule);
    // the main view sees their effect on the contract and their state.
    assert!(!text.contains("src/storage/migrate.rs"));
    assert!(text.contains(" [主控]  迁移·执行  测试·等待 · Ctrl-T"));
    assert!(text.contains(" 契约 v2   ○ r1   ○ r2   ⊘ r3   ⊗ r4 "));
    assert!(text.contains(" 成员等待 · 测试 → 迁移 修复后重跑 · /team"));
    assert!(text.contains("╶─ 主控 · Engineering · 控制整队 ─"));
    assert!(text.contains("deepseek-flash · 全队 31.4k · KV 82%"));
}

#[test]
fn a_member_view_shows_only_the_clauses_it_serves() {
    let mut fixture = team(Some("member-b"));
    let grid = fixture.grid(80, 24);
    print("5b · 成员视图", &grid);
    let text = grid.join("\n");
    assert!(
        text.contains("┃ ✕ 运行  cargo test import_once"),
        "no name inside its own view"
    );
    assert!(
        !text.contains("┃ 迁移 · "),
        "other members stay out of this view"
    );
    assert!(text.contains(" 本成员服务  ⊗ r4"));
    assert!(text.contains("契约 v2 · 全部条款 /contract"));
    assert!(text.contains("╶─ 测试 · 仅此成员 ─"));
    assert!(text.contains(" ▸  给 测试 的补充，或输入 /"));
}

#[test]
fn narrow_screens_keep_identity_failures_and_the_cursor() {
    for (columns, rows) in [(60, 18), (40, 12)] {
        let mut fixture = revised();
        let grid = fixture.grid(columns, rows);
        print(&format!("8 · {columns}×{rows}"), &grid);
        for row in &grid {
            assert!(
                unicode_width::UnicodeWidthStr::width(row.as_str()) <= columns as usize,
                "{row}"
            );
        }
        let text = grid.join("\n");
        if columns == 60 {
            assert!(text.contains(" 契约 v2  ○r1 ○r2 ⊘r3 ○r4"));
            assert!(text.contains("证据 0/4"));
            assert!(text.contains("╶─ Engineering ─"));
        } else {
            // Clause numbers are the last thing given up; at 40 columns they fit.
            assert!(text.contains(" 契约 v2  ○r1 ○r2 ⊘r3 ○r4"));
            assert!(text.contains("╶─ Eng ─"));
            assert!(text.contains(" ▸  补充或纠正…"));
        }
    }
}

use super::{Card, Totals, Verifier};
use crate::domain::{
    EvidenceKind,
    contract::{
        AcceptanceCheck, CancelledAction, CitedEvidence, ClauseRecord, ClauseUpdate,
        ContractChanges, KeptStep, StaleEvidence, StaleReason,
    },
};
use unicode_width::UnicodeWidthStr;

const ORIGINAL: &str = "把 JSON 存储改成 SQLite，保留 CLI 行为与现有用户数据。";

fn clause(id: &str, text: &str, revision: u64, source: &str) -> ClauseRecord {
    ClauseRecord {
        id: id.into(),
        text: text.into(),
        revision,
        active: true,
        source: source.into(),
    }
}

fn rows(card: &Card, width: usize) -> Vec<String> {
    let lines = card.lines(width);
    for line in &lines {
        assert!(line.width() <= width, "{line} is wider than {width}");
    }
    lines
        .iter()
        .map(|line| line.to_string().trim_end().to_owned())
        .collect()
}

fn contract_v1() -> Vec<ClauseRecord> {
    vec![
        clause("r1", ORIGINAL, 1, ORIGINAL),
        clause(
            "r2",
            "把 JSON 存储改成 SQLite",
            1,
            "把 JSON 存储改成 SQLite",
        ),
        clause(
            "r3",
            "保留 CLI 行为与现有用户数据",
            1,
            "保留 CLI 行为与现有用户数据",
        ),
    ]
}

#[test]
fn the_contract_card_quotes_every_clause_and_marks_the_full_request() {
    let card = Card::Contract {
        revision: 1,
        cause: "extract".into(),
        clauses: contract_v1(),
        changes: ContractChanges::default(),
        labels: Default::default(),
    };
    let rows = rows(&card, 79);
    assert!(rows[0].starts_with("╞══ 契约 v1 ══ 摘自你的原话 ══"));
    assert_eq!(rows[0].width(), 79);
    assert_eq!(
        rows[1..4],
        [
            "│ r1  把 JSON 存储改成 SQLite，保留 CLI 行为与现有用户数据。           完整原话",
            "│ r2  把 JSON 存储改成 SQLite",
            "│ r3  保留 CLI 行为与现有用户数据",
        ]
    );
    assert!(rows[4].starts_with("╘══ 条款只能引用你的原话 · 模型不能删减 · /contract ══"));
    assert_eq!(rows.len(), 5);
}

fn revision() -> Card {
    let mut clauses = contract_v1();
    clauses[1] = clause(
        "r2",
        "把 JSON 存储改成 SQLite，迁移在单一事务内完成",
        2,
        "迁移要在一个事务里完成",
    );
    clauses.push(clause(
        "r4",
        "已有 JSON 数据自动导入，重复启动不重复导入",
        2,
        "迁移要在一个事务里完成",
    ));
    Card::Contract {
        revision: 2,
        cause: "refine".into(),
        clauses,
        changes: ContractChanges {
            updated: vec![ClauseUpdate {
                id: "r2".into(),
                before: "把 JSON 存储改成 SQLite".into(),
                after: "把 JSON 存储改成 SQLite，迁移在单一事务内完成".into(),
            }],
            added: vec!["r4".into()],
            retired: vec![],
            cancelled: vec![CancelledAction {
                id: "a1".into(),
                tool: "shell".into(),
                subject: "cargo test storage::tests".into(),
                requirements: vec!["r2".into()],
                settled: true,
            }],
            stale: vec![StaleEvidence {
                id: "e-3".into(),
                tool: "check".into(),
                subject: "cargo test cli".into(),
                reason: StaleReason::Epoch,
            }],
            kept: vec![
                KeptStep {
                    id: "s1".into(),
                    description: "梳理存储接口".into(),
                },
                KeptStep {
                    id: "s3".into(),
                    description: "CLI 兼容回归".into(),
                },
            ],
        },
        labels: [("e-3".into(), 3)].into(),
    }
}

#[test]
fn the_revision_card_shows_what_the_runtime_actually_did() {
    let rows = rows(&revision(), 79);
    assert!(rows[0].starts_with("╞══ 修订 v2 ══ 来自你的补充 ══"));
    assert_eq!(
        rows[1..7],
        [
            "│ ~ r2  把 JSON 存储改成 SQLite",
            "│    →  把 JSON 存储改成 SQLite，迁移在单一事务内完成",
            "│ + r4  已有 JSON 数据自动导入，重复启动不重复导入",
            "│ × 已取消  运行 cargo test storage::tests · 作用于 r2 · 已结算",
            "│ ⊘ 1 项证据过期  e3 检查 cargo test cli · 新增条款使旧证据失效",
            "│ = 保留 2 个步骤  s1 梳理存储接口 · s3 CLI 兼容回归 · 代码不回滚",
        ]
    );
    assert!(rows[7].starts_with("╘══ 契约 v1 → v2 · /contract 查看原话与修订链 ══"));
}

#[test]
fn narrow_revision_cards_keep_the_changes_and_shorten_the_rest() {
    let narrow = rows(&revision(), 59);
    assert_eq!(narrow[2], "│    →  …SQLite，迁移在单一事务内完成");
    assert_eq!(narrow[4], "│ × 已取消  cargo test storage::tests");
    assert_eq!(narrow[5], "│ ⊘ 1 项证据过期 · = 保留 2 个步骤");
    assert!(narrow[6].starts_with("╘══ v1 → v2 · /contract ══"));
    let tiny = rows(&revision(), 39);
    assert_eq!(tiny[5], "│ ⊘ 1 项证据过期 · = 保留 2 步");
}

fn check(id: &str, passed: bool, tool: &str, subject: &str, reason: &str) -> AcceptanceCheck {
    AcceptanceCheck {
        requirement: id.into(),
        passed,
        evidence: if subject.is_empty() {
            vec![]
        } else {
            vec![CitedEvidence {
                id: format!("e-{id}"),
                kind: if tool == "review" {
                    EvidenceKind::Review
                } else {
                    EvidenceKind::Command
                },
                tool: tool.into(),
                subject: subject.into(),
            }]
        },
        reason: reason.into(),
    }
}

fn receipt(passed: bool) -> Card {
    Card::Acceptance {
        revision: 2,
        passed,
        checks: vec![
            check("r1", true, "review", "r2–r4 均有当前证据", ""),
            check("r3", true, "check", "cargo test cli", ""),
            check(
                "r4",
                passed,
                "check",
                if passed { "cargo test import_once" } else { "" },
                "引用的 e9 绑定 s4 的旧版本；迁移标记改动后没有重跑",
            ),
        ],
        clauses: [
            ("r1".into(), ORIGINAL.into()),
            ("r3".into(), "保留 CLI 行为与现有用户数据".into()),
            (
                "r4".into(),
                "已有 JSON 数据自动导入，重复启动不重复导入".into(),
            ),
        ]
        .into(),
        verifier: Verifier {
            files: 5,
            checks: 2,
            seconds: Some(64),
        },
        totals: Totals {
            seconds: Some(372),
            tokens: 41_200,
            kv: Some(0.83),
        },
    }
}

#[test]
fn a_passing_receipt_binds_every_clause_to_its_evidence() {
    let rows = rows(&receipt(true), 79);
    assert_eq!(
        rows[0],
        "║ 验收 · 独立上下文 · 读取 5 个文件 · 重跑 2 组检查 · 1m04s"
    );
    assert!(rows[1].starts_with("╞══ 验收回执 ══ 独立验收 · 契约 v2 ══"));
    assert!(rows[2].starts_with("│ ◉ r1  把 JSON 存储改成 SQLite，保留 CLI"));
    assert!(rows[2].ends_with("评审  r2–r4 均有当前证据"));
    assert!(rows[3].ends_with("检查  cargo test cli"));
    assert_eq!(
        rows[5],
        "│ 3/3 通过 · 证据绑定 v2 条款版本、步骤版本与文件指纹"
    );
    assert!(rows[6].starts_with("╘══ 完成 · 用时 6m12s · 41.2k tokens · KV 83% ══"));
}

#[test]
fn a_failing_receipt_states_the_reason_and_the_completion_barrier() {
    let rows = rows(&receipt(false), 79);
    assert!(rows[1].starts_with("╞══ 验收未通过 ══"));
    assert!(rows[4].starts_with("│ ⊗ r4  已有 JSON 数据自动导入"));
    assert!(rows[4].ends_with("无有效证据"));
    assert_eq!(
        rows[5],
        "│       引用的 e9 绑定 s4 的旧版本；迁移标记改动后没有重跑"
    );
    assert_eq!(rows[6], "│ 2/3 通过 · 有条款未通过时不会标记完成");
    assert!(rows[7].starts_with("╘══ 已把 1 条失败原因交回 Logit · 继续修复 ══"));
}

#[test]
fn every_card_fits_the_narrowest_supported_width() {
    for card in [receipt(true), receipt(false), revision()] {
        let rows = rows(&card, 39);
        assert!(
            rows.iter()
                .any(|row| row.starts_with("╘══ ") && row.ends_with("══"))
        );
    }
}

#[test]
fn plain_text_matches_what_was_printed() {
    let card = receipt(false);
    assert!(card.plain().contains("验收未通过"));
    assert!(card.plain().lines().all(|line| line == line.trim_end()));
}

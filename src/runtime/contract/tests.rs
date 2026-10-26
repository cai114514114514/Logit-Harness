use super::{Origin, Stamp, derive, focused};
use crate::domain::{
    Change, Evidence, EvidenceKind,
    contract::{Acceptance, AcceptanceCheck, StaleReason},
};
use crate::runtime::{
    graph::{Step, StepStatus},
    state::Session,
};
use std::collections::BTreeMap;

fn session() -> Session {
    let mut session = Session::new(
        std::env::temp_dir(),
        "把 JSON 存储改成 SQLite，保留 CLI 行为。",
        10_000,
        60,
    );
    session
        .goal
        .extract(&["把 JSON 存储改成 SQLite".into(), "保留 CLI 行为".into()])
        .unwrap();
    session
}

fn evidence(session: &Session, kind: EvidenceKind, clauses: &[&str], passed: bool) -> Evidence {
    let versions = session.goal.active_versions();
    Evidence {
        id: format!("e-{}", session.evidence.len() + 1),
        kind,
        requirements: clauses
            .iter()
            .map(|id| ((*id).to_owned(), versions[*id]))
            .collect(),
        epoch: session.goal.epoch,
        steps: BTreeMap::new(),
        files: BTreeMap::new(),
        passed,
        description: "check".into(),
        artifact: None,
    }
}

fn stamps(session: &Session) -> Vec<(String, Stamp)> {
    derive(session)
        .unwrap()
        .active()
        .map(|c| (c.id.clone(), c.stamp))
        .collect()
}

fn stamp(session: &Session, id: &str) -> Stamp {
    stamps(session)
        .into_iter()
        .find(|(clause, _)| clause == id)
        .unwrap()
        .1
}

#[test]
fn reading_alone_leaves_every_clause_open() {
    let mut s = session();
    let read = evidence(&s, EvidenceKind::Read, &["r1", "r2", "r3"], true);
    s.evidence.push(read);
    assert!(stamps(&s).iter().all(|(_, stamp)| *stamp == Stamp::Open));
}

#[test]
fn latest_current_observation_decides_between_failed_and_evidenced() {
    let mut s = session();
    let pass = evidence(&s, EvidenceKind::Command, &["r2"], true);
    s.evidence.push(pass);
    assert_eq!(stamp(&s, "r2"), Stamp::Evidenced);
    let fail = evidence(&s, EvidenceKind::Command, &["r2"], false);
    s.evidence.push(fail);
    assert_eq!(stamp(&s, "r2"), Stamp::Failed);
    let fixed = evidence(&s, EvidenceKind::Command, &["r2"], true);
    s.evidence.push(fixed);
    assert_eq!(stamp(&s, "r2"), Stamp::Evidenced);
    assert_eq!(stamp(&s, "r3"), Stamp::Open);
}

#[test]
fn current_review_evidence_is_accepted() {
    let mut s = session();
    let command = evidence(&s, EvidenceKind::Command, &["r3"], true);
    s.evidence.push(command);
    let review = evidence(&s, EvidenceKind::Review, &["r3"], true);
    s.evidence.push(review);
    assert_eq!(stamp(&s, "r3"), Stamp::Accepted);
}

#[test]
fn an_added_clause_advances_the_epoch_and_stales_old_evidence() {
    let mut s = session();
    let pass = evidence(&s, EvidenceKind::Command, &["r3"], true);
    s.evidence.push(pass);
    s.goal
        .revise(Change {
            source: "重复启动不能重复导入。".into(),
            updates: BTreeMap::new(),
            additions: vec!["重复启动不能重复导入".into()],
            replacement: false,
        })
        .unwrap();
    let view = derive(&s).unwrap();
    let r3 = view.active().find(|c| c.id == "r3").unwrap();
    assert_eq!(r3.stamp, Stamp::Stale);
    assert_eq!(r3.evidence[0].stale, Some(StaleReason::Epoch));
    let r4 = view.active().find(|c| c.id == "r4").unwrap();
    assert_eq!((r4.stamp, r4.origin), (Stamp::Open, Origin::Addition));
}

#[test]
fn an_invalidated_step_stales_evidence_bound_to_it() {
    let mut s = session();
    s.graph.steps.insert(
        "s1".into(),
        Step {
            id: "s1".into(),
            description: "迁移".into(),
            requirements: ["r2".into()].into(),
            dependencies: Default::default(),
            files: Default::default(),
            // An empty graph's first invalidation produces revision 1.
            revision: 0,
            status: StepStatus::Verified,
        },
    );
    let mut pass = evidence(&s, EvidenceKind::Command, &["r2"], true);
    pass.steps = [("s1".into(), 0)].into();
    s.evidence.push(pass);
    assert_eq!(stamp(&s, "r2"), Stamp::Evidenced);
    s.graph.invalidate_steps(&["s1".into()].into());
    let view = derive(&s).unwrap();
    let r2 = view.active().find(|c| c.id == "r2").unwrap();
    assert_eq!(r2.stamp, Stamp::Stale);
    assert_eq!(r2.evidence[0].stale, Some(StaleReason::Step));
}

#[test]
fn a_rejected_clause_fails_until_new_passing_evidence_arrives() {
    let mut s = session();
    let pass = evidence(&s, EvidenceKind::Command, &["r1", "r2", "r3"], true);
    s.evidence.push(pass);
    s.acceptance = Some(Acceptance {
        goal: s.goal.id.clone(),
        revision: s.goal.revision,
        epoch: s.goal.epoch,
        passed: false,
        checks: vec![AcceptanceCheck {
            requirement: "r2".into(),
            passed: false,
            evidence: vec![],
            reason: "迁移标记没有写入".into(),
        }],
        summary: String::new(),
        evidence: s.evidence.len(),
        versions: s.goal.active_versions(),
    });
    assert_eq!(stamp(&s, "r2"), Stamp::Failed);
    assert_eq!(stamp(&s, "r3"), Stamp::Evidenced);
    let rerun = evidence(&s, EvidenceKind::Command, &["r2"], true);
    s.evidence.push(rerun);
    assert_eq!(stamp(&s, "r2"), Stamp::Evidenced);
    let view = derive(&s).unwrap();
    assert!(view.acceptance.is_some());
}

#[test]
fn a_verdict_for_an_older_goal_is_ignored() {
    let mut s = session();
    s.acceptance = Some(Acceptance {
        goal: "older".into(),
        checks: vec![AcceptanceCheck {
            requirement: "r1".into(),
            passed: false,
            evidence: vec![],
            reason: String::new(),
        }],
        versions: s.goal.active_versions(),
        ..Acceptance::default()
    });
    assert_eq!(stamp(&s, "r1"), Stamp::Open);
    assert!(derive(&s).unwrap().acceptance.is_none());
}

#[test]
fn clause_origin_order_and_wording_history_follow_the_goal() {
    let mut s = session();
    s.goal
        .revise(Change {
            source: "迁移要在一个事务里完成。".into(),
            updates: [(
                "r2".into(),
                "把 JSON 存储改成 SQLite，迁移在单一事务内完成".into(),
            )]
            .into(),
            additions: vec![],
            replacement: false,
        })
        .unwrap();
    let view = derive(&s).unwrap();
    let ids = view.active().map(|c| c.id.as_str()).collect::<Vec<_>>();
    assert_eq!(ids, ["r1", "r2", "r3"]);
    let origins = view.active().map(|c| c.origin).collect::<Vec<_>>();
    assert_eq!(origins, [Origin::Original, Origin::Addition, Origin::Quote]);
    let r2 = &view.clauses[1];
    assert_eq!(r2.history, [(1, "把 JSON 存储改成 SQLite".to_owned())]);
    assert_eq!(view.revision, 2);
}

#[test]
fn only_a_scope_narrower_than_the_goal_is_focused() {
    let s = session();
    let all = s.goal.active_versions();
    assert!(!focused(&all, &s.goal));
    assert!(!focused(&BTreeMap::new(), &s.goal));
    let one = all.into_iter().filter(|(id, _)| id == "r2").collect();
    assert!(focused(&one, &s.goal));
}

#[test]
fn a_session_without_a_goal_has_no_contract() {
    let s = Session::conversation(std::env::temp_dir(), "", 10_000, 60);
    assert!(derive(&s).is_none());
}

#[test]
fn a_contract_event_moves_the_bar_before_the_next_view() {
    let mut s = session();
    let pass = evidence(&s, EvidenceKind::Command, &["r3"], true);
    s.evidence.push(pass);
    let mut view = derive(&s).unwrap();
    assert_eq!(view.clauses[2].stamp, Stamp::Evidenced);
    let records = view
        .clauses
        .iter()
        .map(|c| crate::domain::contract::ClauseRecord {
            id: c.id.clone(),
            text: c.text.clone(),
            revision: c.revision,
            active: true,
            source: c.source.clone(),
        })
        .chain([crate::domain::contract::ClauseRecord {
            id: "r4".into(),
            text: "重复启动不能重复导入".into(),
            revision: 2,
            active: true,
            source: "重复启动不能重复导入。".into(),
        }])
        .collect::<Vec<_>>();
    // An addition advances the epoch: r3's evidence is no longer current.
    view.revise(2, s.goal.epoch + 1, &records);
    let stamps = view
        .active()
        .map(|c| (c.id.as_str(), c.stamp))
        .collect::<Vec<_>>();
    assert_eq!(
        stamps,
        [
            ("r1", Stamp::Open),
            ("r2", Stamp::Open),
            ("r3", Stamp::Stale),
            ("r4", Stamp::Open)
        ]
    );
    assert_eq!(view.revision, 2);
    s.goal
        .revise(Change {
            source: "重复启动不能重复导入。".into(),
            updates: BTreeMap::new(),
            additions: vec!["重复启动不能重复导入".into()],
            replacement: false,
        })
        .unwrap();
    let derived = derive(&s).unwrap();
    let truth = derived
        .active()
        .map(|c| (c.id.as_str(), c.stamp))
        .collect::<Vec<_>>();
    assert_eq!(stamps, truth, "the projection agrees with the next view");
}

#[test]
fn a_verdict_event_stamps_passed_and_rejected_clauses() {
    let s = session();
    let mut view = derive(&s).unwrap();
    view.verdict(
        &s.goal.id,
        false,
        &[
            AcceptanceCheck {
                requirement: "r1".into(),
                passed: true,
                evidence: vec![],
                reason: String::new(),
            },
            AcceptanceCheck {
                requirement: "r2".into(),
                passed: false,
                evidence: vec![],
                reason: "没有重跑".into(),
            },
        ],
    );
    let stamps = view.active().map(|c| c.stamp).collect::<Vec<_>>();
    assert_eq!(stamps, [Stamp::Accepted, Stamp::Failed, Stamp::Open]);
    assert!(view.acceptance.is_some());
}

#[test]
fn an_archived_goal_keeps_its_contract_for_the_detail_page() {
    let mut s = session();
    let review = evidence(&s, EvidenceKind::Review, &["r1", "r2", "r3"], true);
    s.evidence.push(review);
    s.archives.push(crate::runtime::conversation::Archive {
        team: Default::default(),
        goal: s.goal.clone(),
        status: crate::domain::Status::Completed,
        graph: s.graph.clone(),
        actors: Default::default(),
        evidence: s.evidence.clone(),
        actions: Default::default(),
        report: None,
    });
    s.goal = crate::domain::Goal::new("");
    let view = derive(&s).unwrap();
    assert!(view.archived);
    assert!(view.active().all(|c| c.stamp == Stamp::Accepted));
}

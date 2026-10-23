use super::*;
use crate::{
    model::{ModelEvent, Provider, Request, Response},
    runtime::{
        runner::Settings,
        state::{Control, Session},
        team::{MemberControl, Request as Directed, Target},
    },
    store::Store,
    tools::{ToolHost, files::Workspace},
};
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tokio_util::sync::CancellationToken;
struct Never;
#[async_trait::async_trait]
impl Provider for Never {
    async fn respond(
        &self,
        _: Request,
        _: CancellationToken,
        _: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        panic!("candidate retries must not call the provider")
    }
}

#[tokio::test]
async fn rejected_completion_keeps_memory_running_and_retries_after_harmless_control() {
    let dir = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let path = state.path().join("db");
    let store = Arc::new(Store::open(&path).unwrap());
    let client = Store::open(&path).unwrap();
    let mut session = Session::new(dir.path().into(), "build", 10000, 60);
    session.status = Status::Running;
    session.actors.get_mut("main").unwrap().phase = crate::runtime::actor::Phase::Done;
    let host = Arc::new(ToolHost::new(
        Workspace::open(dir.path()).unwrap(),
        Arc::new(RwLock::new(session.goal.clone())),
        state.path().join("logs"),
    ));
    store
        .commit(
            &session.id,
            &session,
            EventData::Diagnostic {
                message: "before".into(),
            },
        )
        .unwrap();
    let (mut runner, _, _) = Runner::new(
        session,
        store.clone(),
        Arc::new(Never),
        host,
        Settings::default(),
    );
    client
        .enqueue(
            &runner.session.id,
            &serde_json::to_value(Control::Directed {
                request: Directed {
                    id: "limit".into(),
                    client_id: "test".into(),
                    target: Target::Main,
                    action: MemberControl::Limit { limit: 1 },
                },
            })
            .unwrap(),
        )
        .unwrap();
    runner.completion_candidate("verified").unwrap();
    assert!(!runner.try_completion().unwrap());
    assert_eq!(runner.session.status, Status::Running);
    assert_eq!(
        store.load::<Session>(&runner.session.id).unwrap().status,
        Status::Running
    );
    assert!(runner.session.completion.is_some());
    runner.drain_inbox().await.unwrap();
    assert!(runner.try_completion().unwrap());
    assert_eq!(runner.session.status, Status::Completed);
    assert!(runner.session.completion.is_none());
}
#[tokio::test]
async fn archive_keeps_team_provenance_and_clears_current_members() {
    use crate::runtime::{
        graph::{Step, StepStatus},
        team::AssignmentStatus,
    };
    use std::collections::BTreeSet;
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut s = Session::new(root.path().into(), "build", 10000, 60);
    s.graph
        .install(
            vec![Step {
                id: "a".into(),
                description: "a".into(),
                requirements: BTreeSet::from(["r1".into()]),
                dependencies: BTreeSet::new(),
                files: BTreeSet::new(),
                revision: 1,
                status: StepStatus::Pending,
            }],
            &s.goal,
        )
        .unwrap();
    let a = s.assign_member(Some("Ada"), "a", "build").unwrap();
    s.finish_assignment(&a.actor, AssignmentStatus::Finished);
    s.status = Status::Completed;
    s.interactive = true;
    let host = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(RwLock::new(s.goal.clone())),
        state.path().join("logs"),
    ));
    let (mut runner, _, _) = Runner::new(
        s,
        Arc::new(Store::open(&state.path().join("db")).unwrap()),
        Arc::new(Never),
        host,
        Settings::default(),
    );
    runner.settle_boundary().await.unwrap();
    assert!(runner.session.team.members.is_empty());
    assert_eq!(runner.session.archives[0].team.members.len(), 1);
    runner.session.normalize_team();
    assert!(
        runner.session.team.members.is_empty(),
        "archived workers must not become legacy live members"
    );
    let mut old = serde_json::to_value(&runner.session.archives[0]).unwrap();
    old.as_object_mut().unwrap().remove("team");
    let archive: crate::runtime::conversation::Archive = serde_json::from_value(old).unwrap();
    assert!(archive.team.members.is_empty());
}
#[tokio::test]
async fn new_requirement_outside_old_plan_returns_to_main_before_verifying() {
    use crate::{
        domain::Change,
        runtime::{
            actor::Phase,
            graph::{Step, StepStatus},
        },
    };
    use std::collections::{BTreeMap, BTreeSet};
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut s = Session::new(root.path().into(), "build", 10000, 60);
    s.graph
        .install(
            vec![Step {
                id: "a".into(),
                description: "a".into(),
                requirements: BTreeSet::from(["r1".into()]),
                dependencies: BTreeSet::new(),
                files: BTreeSet::new(),
                revision: 1,
                status: StepStatus::Pending,
            }],
            &s.goal,
        )
        .unwrap();
    s.goal
        .revise(Change {
            source: "also export".into(),
            updates: BTreeMap::new(),
            additions: vec!["export".into()],
            replacement: false,
        })
        .unwrap();
    s.status = Status::Running;
    s.actors.get_mut("main").unwrap().phase = Phase::Done;
    s.verification_pending = true;
    s.final_report = Some("done".into());
    let host = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(RwLock::new(s.goal.clone())),
        state.path().join("logs"),
    ));
    let (mut runner, _, _) = Runner::new(
        s,
        Arc::new(Store::open(&state.path().join("db")).unwrap()),
        Arc::new(Never),
        host,
        Settings::default(),
    );
    runner.start_verifier().unwrap();
    assert_eq!(
        runner.session.actors["main"].phase,
        Phase::Ready,
        "only main can reconcile the missing plan coverage"
    );
    assert!(!runner.session.verification_pending);
}
#[tokio::test]
async fn archived_followup_stays_readonly_until_a_real_correction_is_classified() {
    for archive_first in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut s = Session::new(
            root.path().into(),
            "Build the complete original deliverable",
            10000,
            60,
        );
        let goal = s.goal.id.clone();
        s.status = Status::Completed;
        s.interactive = true;
        s.actors.get_mut("main").unwrap().phase = crate::runtime::actor::Phase::Done;
        let host = Arc::new(ToolHost::new(
            Workspace::open(root.path()).unwrap(),
            Arc::new(RwLock::new(s.goal.clone())),
            state.path().join("logs"),
        ));
        let (mut runner, _, _) = Runner::new(
            s,
            Arc::new(Store::open(&state.path().join("db")).unwrap()),
            Arc::new(Never),
            host,
            Settings::default(),
        );
        if archive_first {
            runner.settle_boundary().await.unwrap();
        }
        runner
            .handle_control(Control::Message {
                text: "hello, thanks".into(),
            })
            .await
            .unwrap();
        assert!(
            runner.frozen(),
            "completed followups need read-only classification against the archived objective"
        );
        assert!(runner.session.goal.id.is_empty());
        std::fs::remove_dir(root.path()).unwrap();
        runner.execution_owner().await.unwrap();
        assert!(
            runner.lease.is_none(),
            "a greeting must not reacquire engineering ownership"
        );
        runner.start_steering();
        let id = runner
            .session
            .actors
            .values()
            .find(|a| {
                a.role == crate::runtime::actor::Role::Steering
                    && a.phase != crate::runtime::actor::Phase::Done
            })
            .unwrap()
            .id
            .clone();
        // The project remains removed: the greeting must do no source scan.
        runner.apply_steering(&id,&serde_json::json!({"intent":"question","updates":{},"additions":[],"answer":"Hello!","reason":"greeting"})).await.unwrap();
        assert!(runner.session.goal.id.is_empty());
        assert_eq!(runner.session.archives.len(), 1);
        assert_eq!(runner.session.archives[0].goal.id, goal);
        assert!(runner.session.owner.is_none());
        std::fs::create_dir(root.path()).unwrap();
        runner
            .handle_control(Control::Directed {
                request: Directed {
                    id: "correction".into(),
                    client_id: "late".into(),
                    target: Target::Main,
                    action: MemberControl::Message {
                        text: "Also require CSV export".into(),
                    },
                },
            })
            .await
            .unwrap();
        runner.start_steering();
        let id = runner
            .session
            .actors
            .values()
            .find(|a| {
                a.role == crate::runtime::actor::Role::Steering
                    && a.phase != crate::runtime::actor::Phase::Done
            })
            .unwrap()
            .id
            .clone();
        runner.apply_steering(&id,&serde_json::json!({"intent":"refine","updates":{},"additions":["CSV export"],"answer":"","reason":"additional acceptance"})).await.unwrap();
        assert_eq!(runner.session.goal.id, goal);
        assert_eq!(
            runner.session.team.receipts["late:correction"].status,
            crate::runtime::team::ReceiptStatus::Applied
        );
        assert_eq!(
            runner.session.goal.original,
            "Build the complete original deliverable"
        );
        assert_eq!(
            runner.session.goal.requirements["r1"].text,
            "Build the complete original deliverable"
        );
        assert!(
            runner
                .session
                .goal
                .requirements
                .values()
                .any(|r| r.source == "Also require CSV export")
        );
    }
}

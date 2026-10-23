use super::*;
use crate::{
    model::{ModelEvent, Provider, Request as ModelRequest, Response},
    runtime::{
        actor::Phase,
        graph::{Step, StepStatus},
        runner::Settings,
        state::Session,
        team::{MemberControl, ReceiptStatus, Request, Target},
    },
    store::Store,
    tools::{ToolHost, files::Workspace},
};
use std::{collections::BTreeSet, sync::Arc};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
struct NoRequests;
#[async_trait::async_trait]
impl Provider for NoRequests {
    async fn respond(
        &self,
        _: ModelRequest,
        _: CancellationToken,
        _: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        panic!("no model")
    }
}
fn fixture(root: &std::path::Path) -> Runner {
    let session = Session::new(root.into(), "build", 10000, 60);
    let host = ToolHost::new(
        Workspace::open(root).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        root.join("logs"),
    );
    Runner::new(
        session,
        Arc::new(Store::open(&root.join("db")).unwrap()),
        Arc::new(NoRequests),
        Arc::new(host),
        Settings::default(),
    )
    .0
}
fn assign(runner: &mut Runner) -> Target {
    runner
        .session
        .graph
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
            &runner.session.goal,
        )
        .unwrap();
    runner
        .session
        .assign_member(Some("Ada"), "a", "work")
        .unwrap();
    runner.session.member_target("Ada").unwrap()
}
#[tokio::test]
async fn new_worker_question_and_classifier_bind_before_output_and_mail_replays_updates() {
    let root = tempfile::tempdir().unwrap();
    let mut runner = fixture(root.path());
    runner
        .emit(EventData::Diagnostic {
            message: "client already attached".into(),
        })
        .unwrap();
    let target = assign(&mut runner);
    let worker = runner.session.team.members[target.member_id()]
        .actor
        .clone()
        .unwrap();
    runner
        .emit(EventData::Delta {
            actor: worker.clone(),
            text: "worker output".into(),
        })
        .unwrap();
    runner
        .directed_control(Request {
            id: "q".into(),
            client_id: "fixture".into(),
            target: target.clone(),
            action: MemberControl::Side {
                text: "why?".into(),
            },
        })
        .await
        .unwrap();
    let question = runner.session.team.members[target.member_id()]
        .attempts
        .last()
        .unwrap()
        .clone();
    runner
        .emit(EventData::Message {
            actor: question.clone(),
            text: "because".into(),
        })
        .unwrap();
    let request = Request {
        id: "input".into(),
        client_id: "fixture".into(),
        target: target.clone(),
        action: MemberControl::Message {
            text: "re-read this file".into(),
        },
    };
    runner.directed_control(request.clone()).await.unwrap();
    runner.start_member_steering().await.unwrap();
    let classifier = runner
        .session
        .actors
        .values()
        .find(|a| a.id.starts_with("member-steer-"))
        .unwrap()
        .id
        .clone();
    runner
        .emit(EventData::Delta {
            actor: classifier.clone(),
            text: "classifying".into(),
        })
        .unwrap();
    for actor in [&worker, &question, &classifier] {
        let events = runner.store.events(&runner.session.id, 0).unwrap();
        let bound=events.iter().position(|e| matches!(&e.data,EventData::ActorBound { actor:id,target:t,goal } if id==actor && t==&target && goal.as_ref()==Some(&runner.session.goal.id))).unwrap();
        let output=events.iter().position(|e| matches!(&e.data,EventData::Delta { actor:id,.. } | EventData::Message { actor:id,.. } if id==actor)).unwrap();
        assert!(bound < output);
    }
    // Peer mail has its own durable identity and update, rather than being
    // inferred from a Diagnostic or a periodic snapshot's array order.
    runner
        .session
        .team
        .send("peer", "main", target.clone(), "peer note", false)
        .unwrap();
    runner.publish_metadata().unwrap();
    runner.session.actors.get_mut(&worker).unwrap().phase = Phase::Ready;
    runner.deliver_mail().unwrap();
    let events = runner.store.events(&runner.session.id, 0).unwrap();
    let mail = events
        .iter()
        .filter_map(|e| {
            if let EventData::Mail { mail, .. } = &e.data {
                (mail.id == "peer").then_some((e.sequence, mail))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(mail.len(), 2);
    assert!(!mail[0].1.delivered && mail[1].1.delivered);
    assert!(mail[0].0 < mail[1].0);
    let pending=events.iter().filter(|e| matches!(&e.data, EventData::RequestReceipt { receipt,.. } if receipt.request==request && receipt.status==ReceiptStatus::Pending)).count();
    assert_eq!(pending, 2);
}
#[tokio::test]
async fn stale_input_keeps_failed_receipt_and_colliding_textual_ids_are_independent() {
    let root = tempfile::tempdir().unwrap();
    let mut runner = fixture(root.path());
    let target = assign(&mut runner);
    runner
        .session
        .invalidate_step("a", "new instruction")
        .unwrap();
    let request = Request {
        id: "c".into(),
        client_id: "a:b".into(),
        target: target.clone(),
        action: MemberControl::Message {
            text: "preserve exact '$X'".into(),
        },
    };
    runner.directed_control(request.clone()).await.unwrap();
    runner.directed_control(request.clone()).await.unwrap();
    let receipt = &runner.session.team.receipts[&request.key()];
    assert_eq!(receipt.status, ReceiptStatus::Failed);
    assert_eq!(receipt.request, request);
    let mut other = request.clone();
    other.client_id = "a".into();
    other.id = "b:c".into();
    runner.directed_control(other.clone()).await.unwrap();
    assert_eq!(runner.session.team.receipts.len(), 2);
    let events = runner.store.events(&runner.session.id, 0).unwrap();
    let statuses = events
        .iter()
        .filter_map(|e| {
            if let EventData::RequestReceipt { receipt, .. } = &e.data {
                (receipt.request == request).then_some(&receipt.status)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        statuses,
        vec![&ReceiptStatus::Pending, &ReceiptStatus::Failed]
    );
}

#[tokio::test]
async fn uncertain_mail_is_quarantined_and_cannot_complete_until_user_reviews_current_goal() {
    use crate::{
        domain::Change,
        runtime::{
            completion::Candidate,
            team::{AssignmentStatus, Receipt},
        },
    };
    let root = tempfile::tempdir().unwrap();
    let mut runner = fixture(root.path());
    let target = assign(&mut runner);
    let actor = runner.session.team.members[target.member_id()]
        .actor
        .clone()
        .unwrap();
    let request = Request {
        id: "partial".into(),
        client_id: "fixture".into(),
        target: target.clone(),
        action: MemberControl::Message {
            text: "retain exact correction '$x'".into(),
        },
    };
    runner
        .session
        .team
        .send(
            &request.mail_id(),
            "user",
            target.clone(),
            "retain exact correction '$x'",
            true,
        )
        .unwrap();
    let member = runner
        .session
        .team
        .members
        .get_mut(target.member_id())
        .unwrap();
    member.interrupted = true;
    member.paused = true;
    runner.session.team.receipts.insert(
        request.key(),
        Receipt {
            applying: true,
            request: request.clone(),
            status: ReceiptStatus::Pending,
            error: None,
            pending_input: None,
        },
    );
    runner
        .session
        .goal
        .revise(Change {
            source: "already applied effect".into(),
            updates: std::collections::BTreeMap::from([(
                "r1".into(),
                "already changed delivery".into(),
            )]),
            additions: vec![],
            replacement: false,
        })
        .unwrap();
    let revision = runner.session.goal.revision;
    crate::runtime::team::recover_requests(&mut runner.session, &runner.store).unwrap();
    assert_eq!(
        runner.session.goal.revision, revision,
        "existing effects must remain"
    );
    assert!(runner.session.team.mail[0].error.is_some());
    assert!(!runner.session.team.members[target.member_id()].interrupted);
    assert!(
        runner.session.team.members[target.member_id()].paused,
        "explicit pause survives uncertainty settlement"
    );
    runner.start_member_steering().await.unwrap();
    assert!(
        !runner
            .session
            .actors
            .values()
            .any(|a| a.id.starts_with("member-steer-"))
    );
    runner
        .session
        .finish_assignment(&actor, AssignmentStatus::Finished);
    assert!(
        runner.session.team.engineering_settled(),
        "quarantined mail itself must not block forever"
    );
    runner.session.status = crate::domain::Status::Running;
    runner.session.actors.get_mut("main").unwrap().phase = Phase::Done;
    runner.session.completion = Some(Candidate {
        scope: crate::tools::Scope {
            epoch: runner.session.goal.epoch,
            requirements: runner.session.goal.active_versions(),
            steps: runner.session.graph.versions(),
            ..Default::default()
        },
        files: runner
            .tools
            .workspace
            .snapshot_for(std::iter::empty::<&String>())
            .unwrap(),
        reason: "old verdict".into(),
    });
    assert!(
        !runner.try_completion().unwrap(),
        "unreviewed accepted correction cannot be silently ignored by completion"
    );
    assert_ne!(runner.session.status, crate::domain::Status::Completed);
    let question = &runner.session.questions[&request.recovery_id()];
    assert!(question.text.contains("retain exact correction") && question.answer.is_none());
    *runner.tools.goal.write().await = runner.session.goal.clone();
    runner
        .answer_question(&request.recovery_id(), "已检查现有改动，保留新的交付约束")
        .await
        .unwrap();
    assert!(
        runner.session.questions[&request.recovery_id()]
            .answer
            .is_some()
    );
    assert!(runner.blocked_requirements().is_empty());
}

#[test]
fn accepted_async_pending_receipts_are_not_misread_as_interrupted_handlers() {
    use crate::runtime::team::Receipt;
    let root = tempfile::tempdir().unwrap();
    let mut runner = fixture(root.path());
    let target = assign(&mut runner);
    for (id, action) in [
        (
            "message",
            MemberControl::Message {
                text: "waiting classification".into(),
            },
        ),
        (
            "skill",
            MemberControl::Skill {
                id: "chosen".into(),
                hash: "hash".into(),
                arguments: "literal".into(),
            },
        ),
    ] {
        let request = Request {
            id: id.into(),
            client_id: "fixture".into(),
            target: target.clone(),
            action,
        };
        runner.session.team.receipts.insert(
            request.key(),
            Receipt {
                applying: false,
                request,
                status: ReceiptStatus::Pending,
                error: None,
                pending_input: None,
            },
        );
    }
    crate::runtime::team::recover_requests(&mut runner.session, &runner.store).unwrap();
    assert!(
        runner
            .session
            .team
            .receipts
            .values()
            .all(|r| r.status == ReceiptStatus::Pending && r.error.is_none())
    );
    assert!(runner.session.questions.is_empty());
}

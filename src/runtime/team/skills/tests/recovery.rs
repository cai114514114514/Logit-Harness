use super::*;
use crate::{
    domain::{Change, EventData},
    runtime::{recovery, state::Question, team::Receipt},
    tools::Scope,
};
use std::collections::BTreeMap;

#[tokio::test]
async fn resubmitted_skill_refreshes_host_blockers_and_keeps_unrelated_requirement_blocked() {
    let root = tempfile::tempdir().unwrap();
    let (mut runner, mut original, _) = fixture(root.path());
    let actor = runner.session.team.members[original.target.member_id()]
        .actor
        .clone()
        .unwrap();
    runner
        .session
        .goal
        .revise(Change {
            source: "independent requirement".into(),
            updates: BTreeMap::new(),
            additions: vec!["wait for independent user clarification".into()],
            replacement: false,
        })
        .unwrap();
    runner
        .session
        .finish_assignment(&actor, crate::runtime::team::AssignmentStatus::Invalidated);
    let assignment = runner
        .session
        .assign_member(Some("Ada"), "a", "repair current requirements")
        .unwrap();
    let actor = assignment.actor;
    original.target = runner.session.member_target("Ada").unwrap();
    let independent_version = runner.session.goal.requirements["r2"].revision;
    runner.session.questions.insert(
        "unrelated".into(),
        Question {
            target: None,
            text: "independent clarification".into(),
            requirements: BTreeMap::from([("r2".into(), independent_version)]),
            answer: None,
        },
    );
    runner.session.team.receipts.insert(
        original.key(),
        Receipt {
            applying: true,
            request: original.clone(),
            status: ReceiptStatus::Pending,
            error: None,
            pending_input: None,
        },
    );
    runner
        .store
        .commit(
            &runner.session.id,
            &runner.session,
            EventData::Diagnostic {
                message: "interrupted skill checkpoint".into(),
            },
        )
        .unwrap();
    runner.session = runner.store.load(&runner.session.id).unwrap();
    recovery::reconcile(
        &mut runner.session,
        &runner.store,
        &root.path().join("logs"),
    )
    .await
    .unwrap();
    *runner.tools.goal.write().await = runner.session.goal.clone();
    *runner.tools.steps.write().await = runner.session.graph.versions();
    runner.refresh_questions().await;
    assert_eq!(
        *runner.tools.blocked.read().await,
        BTreeSet::from(["r1".into(), "r2".into()])
    );
    assert_eq!(
        runner.session.team.receipts[&original.key()].status,
        ReceiptStatus::Failed
    );

    let mut repeated = original.clone();
    repeated.id = "repeated-skill".into();
    runner.directed_control(repeated.clone()).await.unwrap();
    runner.settle_member_skills().unwrap();
    assert_eq!(
        runner.session.team.receipts[&repeated.key()].status,
        ReceiptStatus::Applied
    );
    assert!(
        runner.session.questions[&original.recovery_id()]
            .answer
            .is_some()
    );
    assert!(runner.session.questions["unrelated"].answer.is_none());
    assert!(
        !runner.session.actors[&actor].retired,
        "recovered worker unexpectedly retired"
    );
    assert!(
        runner.scope_current(&runner.session.actors[&actor].scope),
        "scope {:?}; goal {:?}; graph {:?}",
        runner.session.actors[&actor].scope,
        runner.session.goal,
        runner.session.graph
    );
    let proposal = Pending {
        call: ToolCall {
            id: "recovered-write".into(),
            name: "propose_patch".into(),
            arguments: serde_json::json!({"edits":[{"path":"allowed.txt","expected":"missing","operation":"create","content":"repair accepted"}]}),
        },
        scope: runner.session.actors[&actor].scope.clone(),
    };
    runner.propose(&actor, &proposal).await.unwrap();
    let (id, result) = runner.tasks.join_next().await.unwrap().unwrap();
    runner.handle_result(id, result).await.unwrap();
    assert!(
        root.path().join("allowed.txt").exists(),
        "accepted resubmission must release the recovered requirement in the authoritative host: {:?}",
        runner.store.events(&runner.session.id, 0).unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("allowed.txt")).unwrap(),
        "repair accepted"
    );
    assert_eq!(
        *runner.tools.blocked.read().await,
        BTreeSet::from(["r2".into()])
    );

    let (output, _) = mpsc::unbounded_channel();
    let error = runner.tools.execute(ToolCall { id:"unrelated-write".into(),name:"apply_patch".into(),arguments:serde_json::json!({"edits":[{"path":"blocked.txt","expected":"missing","operation":"create","content":"must remain blocked"}]}) },Scope { epoch:runner.session.goal.epoch, requirements:BTreeMap::from([("r2".into(),1)]),..Scope::default() },CancellationToken::new(),output).await.unwrap_err();
    assert!(error.to_string().contains("unanswered user input"));
    assert!(!root.path().join("blocked.txt").exists());
}

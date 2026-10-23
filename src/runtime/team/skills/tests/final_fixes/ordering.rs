use super::*;

#[tokio::test]
async fn old_pending_receipts_and_new_requests_restore_one_durable_order() {
    let root = tempfile::tempdir().unwrap();
    let (mut runner, mut first, _) = fixture(root.path());
    first.id = "z-old-first".into();
    let second = control(&first, "a-old-second", first.action.clone());
    for request in [&first, &second] {
        runner.directed_control(request.clone()).await.unwrap();
    }
    let mut old = serde_json::to_value(&runner.session).unwrap();
    old["team"].as_object_mut().unwrap().remove("skill_queue");
    runner.session = serde_json::from_value(old).unwrap();
    let third = control(&first, "0-new-third", first.action.clone());
    runner.directed_control(third.clone()).await.unwrap();
    runner.session = runner.store.load(&runner.session.id).unwrap();
    runner.settle_member_skills().unwrap();
    assert_eq!(
        runner.session.team.receipts[&first.key()].status,
        ReceiptStatus::Applied
    );
    assert_eq!(
        runner.session.team.receipts[&second.key()].status,
        ReceiptStatus::Pending
    );
    assert_eq!(
        runner.session.team.receipts[&third.key()].status,
        ReceiptStatus::Pending
    );
    assert_eq!(
        runner.session.team.skill_queue,
        [first.key(), second.key(), third.key()]
    );
    // Ending the assignment does not turn later old-target inputs into success.
    let actor = runner.session.team.members[first.target.member_id()]
        .actor
        .clone()
        .unwrap();
    runner
        .session
        .finish_assignment(&actor, crate::runtime::team::AssignmentStatus::Finished);
    runner.settle_member_skills().unwrap();
    for request in [&second, &third] {
        let receipt = &runner.session.team.receipts[&request.key()];
        assert_eq!(receipt.status, ReceiptStatus::Failed);
        assert_eq!(&receipt.request, request);
        assert!(receipt.error.as_ref().unwrap().contains("stale"));
    }
}

#[tokio::test]
async fn paused_skill_queue_does_not_block_an_independent_member() {
    let root = tempfile::tempdir().unwrap();
    let (mut runner, first, _) = fixture(root.path());
    let mut step = runner.session.graph.steps["a"].clone();
    step.id = "b".into();
    step.status = StepStatus::Pending;
    runner.session.graph.steps.insert("b".into(), step);
    let other = runner
        .session
        .assign_member(Some("Bob"), "b", "independent task")
        .unwrap();
    let second = Request {
        id: "other".into(),
        target: runner.session.member_target("Bob").unwrap(),
        ..first.clone()
    };
    runner.directed_control(first.clone()).await.unwrap();
    runner
        .directed_control(control(&first, "pause", MemberControl::Pause))
        .await
        .unwrap();
    runner.directed_control(second.clone()).await.unwrap();
    runner.settle_member_skills().unwrap();
    assert_eq!(
        runner.session.team.receipts[&first.key()].status,
        ReceiptStatus::Pending
    );
    assert_eq!(
        runner.session.team.receipts[&second.key()].status,
        ReceiptStatus::Applied
    );
    assert!(
        runner
            .session
            .workflows
            .contains_key(&runner.session.actors[&other.actor].turn)
    );
}

#[tokio::test]
async fn missing_arrival_evidence_fails_the_old_input_without_blocking_new_work() {
    let root = tempfile::tempdir().unwrap();
    let (mut runner, first, _) = fixture(root.path());
    // An imported old snapshot has no receipt events or control rows to prove order.
    runner.session.team.receipts.insert(
        first.key(),
        crate::runtime::team::Receipt {
            applying: false,
            request: first.clone(),
            status: ReceiptStatus::Pending,
            error: None,
            pending_input: None,
        },
    );
    runner.settle_member_skills().unwrap();
    assert_eq!(
        runner.session.team.receipts[&first.key()].status,
        ReceiptStatus::Failed
    );
    assert_eq!(runner.session.team.receipts[&first.key()].request, first);
    let repeated = control(&first, "fresh", first.action.clone());
    runner.directed_control(repeated.clone()).await.unwrap();
    runner.settle_member_skills().unwrap();
    assert_eq!(
        runner.session.team.receipts[&repeated.key()].status,
        ReceiptStatus::Applied
    );
}

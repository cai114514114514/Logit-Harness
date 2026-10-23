use super::*;
use crate::{
    domain::{EventData, Status, Usage},
    runtime::state::Control,
};
use tokio::sync::oneshot;

struct Capture(mpsc::UnboundedSender<(ModelRequest, CancellationToken, oneshot::Sender<Response>)>);
#[async_trait::async_trait]
impl Provider for Capture {
    async fn respond(
        &self,
        request: ModelRequest,
        cancel: CancellationToken,
        _: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        let (send, receive) = oneshot::channel();
        self.0.send((request, cancel, send)).unwrap();
        Ok(receive.await?)
    }
}
fn response(output: Vec<serde_json::Value>) -> Response {
    Response {
        output,
        usage: Usage {
            input: 7,
            output: 3,
            cached: 0,
        },
        model: "offline fixture".into(),
    }
}
fn answer() -> Response {
    response(vec![
        serde_json::json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"settled answer"}]}),
    ])
}
async fn settle(runner: &mut Runner) {
    let (id, result) =
        tokio::time::timeout(std::time::Duration::from_secs(3), runner.tasks.join_next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    runner.handle_result(id, result).await.unwrap();
}
fn control(request: &Request, id: &str, action: MemberControl) -> Request {
    Request {
        id: id.into(),
        action,
        ..request.clone()
    }
}

#[tokio::test]
async fn deferred_skills_keep_server_order_and_each_body_after_reload_and_cancellation() {
    let root = tempfile::tempdir().unwrap();
    let (mut runner, mut first, _) = fixture(root.path());
    runner.settings.max_output = 128;
    runner.session.budget.limit = 100_000;
    runner.session.actors.get_mut("main").unwrap().phase = Phase::Done;
    let actor = runner.session.team.members[first.target.member_id()]
        .actor
        .clone()
        .unwrap();
    first.id = "z-first".into();
    let skill = root.path().join(".agents/skills/second");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: second\ndescription: fixture\n---\nSECOND DISTINCT BODY",
    )
    .unwrap();
    let invocation = Skills::discover(root.path(), &[])
        .unwrap()
        .invoke("second", "second args")
        .unwrap();
    let second = control(
        &first,
        "a-second",
        MemberControl::Skill {
            id: invocation.id,
            hash: invocation.hash,
            arguments: invocation.arguments,
        },
    );
    std::fs::write(root.path().join("allowed.txt"), "tool barrier").unwrap();
    let call = ToolCall {
        id: "barrier".into(),
        name: "read_file".into(),
        arguments: serde_json::json!({"path":"allowed.txt"}),
    };
    runner.session.actors.get_mut(&actor).unwrap().context.append(&[serde_json::json!({"type":"function_call","call_id":call.id,"name":call.name,"arguments":call.arguments.to_string()})]);
    runner
        .start_tool(
            &actor,
            Pending {
                call,
                scope: runner.session.actors[&actor].scope.clone(),
            },
        )
        .await
        .unwrap();
    for request in [&first, &second] {
        runner
            .store
            .enqueue(
                &runner.session.id,
                &serde_json::to_value(Control::Directed {
                    request: request.clone(),
                })
                .unwrap(),
            )
            .unwrap();
    }
    runner.drain_inbox().await.unwrap();
    runner.settle_member_skills().unwrap();
    assert!(
        [&first, &second]
            .iter()
            .all(|r| runner.session.team.receipts[&r.key()].status == ReceiptStatus::Pending)
    );
    runner.session = runner.store.load(&runner.session.id).unwrap();
    settle(&mut runner).await;
    runner.settle_member_skills().unwrap();
    assert_eq!(
        runner.session.team.receipts[&first.key()].status,
        ReceiptStatus::Applied
    );
    assert_eq!(
        runner.session.team.receipts[&second.key()].status,
        ReceiptStatus::Pending,
        "second invocation replaced an admitted, unscheduled workflow"
    );
    let first_turn = runner.session.actors[&actor].turn.clone();
    let (send, mut requests) = mpsc::unbounded_channel();
    runner.provider = Arc::new(Capture(send));
    runner.schedule().await.unwrap();
    let (request, cancel, release) = requests.recv().await.unwrap();
    let first_wire = serde_json::to_string(&request.input).unwrap();
    assert!(
        first_wire.contains("Repair only declared files.")
            && !first_wire.contains("SECOND DISTINCT BODY")
    );
    runner
        .directed_control(control(&first, "pause", MemberControl::Pause))
        .await
        .unwrap();
    assert!(cancel.is_cancelled());
    release.send(answer()).unwrap();
    settle(&mut runner).await;
    runner
        .store
        .commit(
            &runner.session.id,
            &runner.session,
            EventData::Diagnostic {
                message: "cancelled workflow checkpoint".into(),
            },
        )
        .unwrap();
    runner.session = runner.store.load(&runner.session.id).unwrap();
    crate::runtime::recovery::reconcile(
        &mut runner.session,
        &runner.store,
        &root.path().join("logs"),
    )
    .await
    .unwrap();
    runner
        .directed_control(control(&first, "resume", MemberControl::Resume))
        .await
        .unwrap();
    runner.settle_member_skills().unwrap();
    assert_eq!(
        runner.session.actors[&actor].turn, first_turn,
        "cancellation cannot skip the first selected body"
    );
    runner.schedule().await.unwrap();
    let (_, _, release) = requests.recv().await.unwrap();
    release.send(response(vec![serde_json::json!({"type":"function_call","call_id":"after-first","name":"read_file","arguments":"{\"path\":\"allowed.txt\"}"})])).unwrap();
    settle(&mut runner).await;
    runner.settle_member_skills().unwrap();
    assert_eq!(
        runner.session.team.receipts[&second.key()].status,
        ReceiptStatus::Pending
    );
    runner.schedule().await.unwrap();
    settle(&mut runner).await;
    runner.settle_member_skills().unwrap();
    assert_eq!(
        runner.session.team.receipts[&second.key()].status,
        ReceiptStatus::Applied
    );
    assert_ne!(runner.session.actors[&actor].turn, first_turn);
    runner.schedule().await.unwrap();
    let (request, _, release) = requests.recv().await.unwrap();
    let wire = serde_json::to_string(&request.input).unwrap();
    assert!(
        wire.contains("SECOND DISTINCT BODY") && wire.contains("Repair only declared files."),
        "earlier selected body must remain in member history"
    );
    release.send(answer()).unwrap();
    settle(&mut runner).await;
    let events = runner.store.events(&runner.session.id, 0).unwrap();
    let turns = events
        .iter()
        .filter_map(|e| match &e.data {
            EventData::Identity {
                actor: id,
                turn,
                action: None,
                ..
            } if id == &actor => Some(turn),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(turns.len(), 3);
    assert_eq!(turns[0], turns[1]);
    assert_ne!(turns[1], turns[2]);
}

#[tokio::test]
async fn paused_member_question_is_not_redispatched_after_cancelled_response() {
    paused_reply(false).await;
}
#[tokio::test]
async fn paused_member_readonly_skill_is_not_redispatched_after_cancelled_response() {
    paused_reply(true).await;
}
async fn paused_reply(skill: bool) {
    let root = tempfile::tempdir().unwrap();
    let (mut runner, mut request, _) = fixture(root.path());
    runner.settings.max_output = 128;
    runner.session.budget.limit = 100_000;
    runner.session.actors.get_mut("main").unwrap().phase = Phase::Done;
    let worker = runner.session.team.members[request.target.member_id()]
        .actor
        .clone()
        .unwrap();
    runner
        .session
        .finish_assignment(&worker, crate::runtime::team::AssignmentStatus::Finished);
    request.target = runner.session.member_target("Ada").unwrap();
    if !skill {
        request.action = MemberControl::Side {
            text: "held question".into(),
        };
    }
    runner.directed_control(request.clone()).await.unwrap();
    runner.settle_member_skills().unwrap();
    let actor = runner.session.team.members[request.target.member_id()]
        .actor
        .clone()
        .unwrap();
    let (send, mut requests) = mpsc::unbounded_channel();
    runner.provider = Arc::new(Capture(send));
    runner.schedule().await.unwrap();
    let (_, cancel, release) = requests.recv().await.unwrap();
    runner
        .directed_control(control(&request, "pause", MemberControl::Pause))
        .await
        .unwrap();
    assert!(cancel.is_cancelled());
    release.send(answer()).unwrap();
    settle(&mut runner).await;
    runner
        .store
        .commit(
            &runner.session.id,
            &runner.session,
            EventData::Diagnostic {
                message: "settled cancellation checkpoint".into(),
            },
        )
        .unwrap();
    runner.session = runner.store.load(&runner.session.id).unwrap();
    for _ in 0..3 {
        runner.schedule().await.unwrap();
    }
    assert!(
        runner.jobs.is_empty(),
        "paused read-only reply was automatically respawned"
    );
    let other = runner
        .session
        .assign_member(Some("Bob"), "a", "independent progress")
        .unwrap();
    runner.schedule().await.unwrap();
    let (other_request, _, release) = requests.recv().await.unwrap();
    assert!(other_request.instructions.starts_with("ROLE: worker"));
    assert!(runner.jobs.values().all(|job| job.actor == other.actor));
    release.send(answer()).unwrap();
    settle(&mut runner).await;
    runner.session.actors.get_mut("main").unwrap().phase = Phase::Done;
    runner
        .directed_control(control(
            &request,
            "fresh",
            MemberControl::Side {
                text: "new explicit side question".into(),
            },
        ))
        .await
        .unwrap();
    runner.schedule().await.unwrap();
    let (_, _, release) = requests.recv().await.unwrap();
    release.send(answer()).unwrap();
    settle(&mut runner).await;
    let fresh = runner.session.team.members[request.target.member_id()]
        .actor
        .clone()
        .unwrap();
    assert_eq!(runner.session.actors[&fresh].phase, Phase::Done);
    runner.session.status = Status::Paused;
    runner
        .directed_control(control(&request, "resume", MemberControl::Resume))
        .await
        .unwrap();
    runner.schedule().await.unwrap();
    assert!(
        runner.jobs.is_empty(),
        "local resume must not override the global pause for an interrupted attempt"
    );
    runner.handle_control(Control::Resume).await.unwrap();
    runner.schedule().await.unwrap();
    let (_, _, release) = requests.recv().await.unwrap();
    release.send(answer()).unwrap();
    settle(&mut runner).await;
    assert_eq!(runner.session.actors[&actor].phase, Phase::Done);
    runner.schedule().await.unwrap();
    assert!(
        runner.jobs.is_empty(),
        "completed attempts must not resurrect on resume"
    );
}

#[tokio::test]
async fn directed_main_messages_emit_their_stable_request_turn_once() {
    let root = tempfile::tempdir().unwrap();
    let (mut runner, mut first, _) = fixture(root.path());
    first.target = Target::Main;
    first.action = MemberControl::Message {
        text: "identical literal input".into(),
    };
    let second = control(&first, "second", first.action.clone());
    for request in [&first, &second, &first] {
        runner.directed_control(request.clone()).await.unwrap();
    }
    let events = runner.store.events(&runner.session.id, 0).unwrap();
    let turns = events
        .iter()
        .filter_map(|e| match &e.data {
            EventData::User { message, turn } if message == "identical literal input" => {
                Some(turn.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(turns, [Some(first.key()), Some(second.key())]);
    for request in [&first, &second] {
        assert_eq!(
            &runner.session.team.receipts[&request.key()].request,
            request
        );
    }
}

mod legacy;
mod ordering;

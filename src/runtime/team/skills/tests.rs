use super::*;
use crate::{
    extensions::{knowledge::Knowledge, skills::Skills},
    model::{ModelEvent, Provider, Request as ModelRequest, Response, ToolCall},
    runtime::{
        actor::Pending,
        graph::{Step, StepStatus},
        runner::Settings,
        state::Session,
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
        panic!("no provider")
    }
}
fn fixture(root: &std::path::Path) -> (Runner, Request, Invocation) {
    let skill = root.join(".agents/skills/repair");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: repair\ndescription: fixture\n---\nRepair only declared files.",
    )
    .unwrap();
    let mut session = Session::new(root.into(), "build", 10000, 60);
    session
        .graph
        .install(
            vec![Step {
                id: "a".into(),
                description: "a".into(),
                requirements: BTreeSet::from(["r1".into()]),
                dependencies: BTreeSet::new(),
                files: BTreeSet::from(["allowed.txt".into()]),
                revision: 1,
                status: StepStatus::Pending,
            }],
            &session.goal,
        )
        .unwrap();
    session.assign_member(Some("Ada"), "a", "repair").unwrap();
    let invocation = Skills::discover(root, &[])
        .unwrap()
        .invoke("repair", "literal '$X'\nmore")
        .unwrap();
    let request = Request {
        id: "skill".into(),
        client_id: "fixture".into(),
        target: session.member_target("Ada").unwrap(),
        action: MemberControl::Skill {
            id: invocation.id.clone(),
            hash: invocation.hash.clone(),
            arguments: invocation.arguments.clone(),
        },
    };
    let mut host = ToolHost::new(
        Workspace::open(root).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        root.join("logs"),
    );
    host.knowledge = Some(Arc::new(Knowledge::new(root.into(), vec![]).unwrap()));
    let (runner, _, _) = Runner::new(
        session,
        Arc::new(Store::open(&root.join("db")).unwrap()),
        Arc::new(NoRequests),
        Arc::new(host),
        Settings::default(),
    );
    (runner, request, invocation)
}
#[tokio::test]
async fn active_member_skill_waits_for_tool_pairs_and_preserves_assignment_authority() {
    let root = tempfile::tempdir().unwrap();
    let (mut runner, request, invocation) = fixture(root.path());
    let actor_id = runner.session.team.members[request.target.member_id()]
        .actor
        .clone()
        .unwrap();
    let scope = serde_json::to_value(&runner.session.actors[&actor_id].scope).unwrap();
    let main_turn = runner.session.actors["main"].turn.clone();
    runner
        .session
        .actors
        .get_mut(&actor_id)
        .unwrap()
        .pending
        .push_back(Pending {
            call: ToolCall {
                id: "pending".into(),
                name: "read_file".into(),
                arguments: serde_json::json!({"path":"allowed.txt"}),
            },
            scope: Default::default(),
        });
    runner.directed_control(request.clone()).await.unwrap();
    runner.settle_member_skills().unwrap();
    assert_eq!(
        runner.session.team.receipts[&request.key()].status,
        ReceiptStatus::Pending
    );
    runner
        .session
        .actors
        .get_mut(&actor_id)
        .unwrap()
        .pending
        .clear();
    runner
        .session
        .team
        .members
        .get_mut(request.target.member_id())
        .unwrap()
        .paused = true;
    runner.settle_member_skills().unwrap();
    assert_eq!(
        runner.session.team.receipts[&request.key()].status,
        ReceiptStatus::Pending
    );
    runner
        .session
        .team
        .members
        .get_mut(request.target.member_id())
        .unwrap()
        .paused = false;
    runner.session.status = crate::domain::Status::Paused;
    runner.settle_member_skills().unwrap();
    let actor = &runner.session.actors[&actor_id];
    assert_eq!(serde_json::to_value(&actor.scope).unwrap(), scope);
    assert!(actor.role == Role::Worker && !actor.retired);
    assert_eq!(runner.session.member_target("Ada").unwrap(), request.target);
    assert_eq!(runner.session.actors["main"].turn, main_turn);
    assert_ne!(actor.turn, main_turn);
    assert_eq!(runner.session.status, crate::domain::Status::Paused);
    assert_eq!(
        runner.session.workflows[&actor.turn].arguments,
        invocation.arguments
    );
    assert_eq!(
        runner.session.team.receipts[&request.key()].status,
        ReceiptStatus::Applied
    );
    // The same worker can still submit guarded proposals inside its original
    // declaration; loading a workflow grants no new paths.
    runner.session.status = crate::domain::Status::Running;
    *runner.tools.steps.write().await = runner.session.graph.versions();
    let mut proposal = Pending {
        call: ToolCall {
            id: "proposal".into(),
            name: "propose_patch".into(),
            arguments: serde_json::json!({"edits":[{"path":"outside.txt","expected":"missing","operation":"create","content":"no"}]}),
        },
        scope: runner.session.actors[&actor_id].scope.clone(),
    };
    assert!(runner.propose(&actor_id, &proposal).await.is_err());
    proposal.call.arguments["edits"][0]["path"] = serde_json::json!("allowed.txt");
    runner.propose(&actor_id, &proposal).await.unwrap();
    let (id, result) = runner.tasks.join_next().await.unwrap().unwrap();
    runner.handle_result(id, result).await.unwrap();
    assert!(
        root.path().join("allowed.txt").exists(),
        "{:?}",
        runner.store.events(&runner.session.id, 0).unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("allowed.txt")).unwrap(),
        "no"
    );
    assert!(!root.path().join("outside.txt").exists());
}
#[tokio::test]
async fn pending_skill_fails_on_assignment_change_and_preserves_literal_request() {
    let root = tempfile::tempdir().unwrap();
    let (mut runner, request, _) = fixture(root.path());
    runner.directed_control(request.clone()).await.unwrap();
    runner
        .session
        .invalidate_step("a", "re-read before repair")
        .unwrap();
    runner.settle_member_skills().unwrap();
    let receipt = &runner.session.team.receipts[&request.key()];
    assert_eq!(receipt.status, ReceiptStatus::Failed);
    assert_eq!(receipt.request, request);
    assert!(receipt.error.as_ref().unwrap().contains("stale"));
}
#[tokio::test]
async fn changed_skill_body_fails_after_selection_without_injecting_workflow() {
    let root = tempfile::tempdir().unwrap();
    let (mut runner, request, _) = fixture(root.path());
    runner.directed_control(request.clone()).await.unwrap();
    std::fs::write(
        root.path().join(".agents/skills/repair/SKILL.md"),
        "---\nname: repair\ndescription: changed\n---\nChanged body.",
    )
    .unwrap();
    runner.settle_member_skills().unwrap();
    assert_eq!(
        runner.session.team.receipts[&request.key()].status,
        ReceiptStatus::Failed
    );
    assert!(runner.session.workflows.is_empty());
}

#[tokio::test]
async fn idle_and_archived_member_skills_use_readonly_actors_without_reviving_work() {
    use crate::runtime::{conversation::Archive, team::AssignmentStatus};
    let root = tempfile::tempdir().unwrap();
    let (mut runner, mut request, _) = fixture(root.path());
    let original_target = request.target.clone();
    let previous = runner.session.team.members[request.target.member_id()]
        .actor
        .clone()
        .unwrap();
    runner
        .session
        .finish_assignment(&previous, AssignmentStatus::Finished);
    request.target = runner.session.member_target("Ada").unwrap();
    runner.directed_control(request.clone()).await.unwrap();
    runner.settle_member_skills().unwrap();
    let question = runner.session.team.members[request.target.member_id()]
        .actor
        .clone()
        .unwrap();
    assert_ne!(question, previous);
    assert!(runner.session.actors[&previous].retired);
    assert!(runner.session.actors[&question].scope.readonly);
    assert_eq!(runner.session.actors[&question].role, Role::Question);
    let goal = runner.session.goal.id.clone();
    runner.session.archives.push(Archive {
        team: std::mem::take(&mut runner.session.team),
        goal: runner.session.goal.clone(),
        status: crate::domain::Status::Completed,
        graph: runner.session.graph.clone(),
        actors: runner.session.actors.clone(),
        evidence: vec![],
        actions: Default::default(),
        report: None,
    });
    runner.session.goal = crate::domain::Goal::new("other goal");
    request.id = "archived-skill".into();
    request.target = original_target;
    runner.directed_control(request.clone()).await.unwrap();
    runner.settle_member_skills().unwrap();
    assert!(runner.session.team.members.is_empty());
    let actor = runner
        .session
        .actors
        .values()
        .find(|a| a.id != question && a.id.starts_with("member-skill-"))
        .unwrap();
    assert!(actor.scope.readonly && actor.role == Role::Question);
    assert_eq!(runner.session.actor_bindings()[&actor.id].goal, Some(goal));
    assert_eq!(
        runner.session.team.receipts[&request.key()].status,
        ReceiptStatus::Applied
    );
}

mod recovery;

mod final_fixes;

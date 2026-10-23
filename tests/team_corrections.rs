use loh::{
    domain::{Evidence, EvidenceKind},
    runtime::{
        graph::{Step, StepStatus},
        state::Session,
    },
};
use std::collections::{BTreeMap, BTreeSet};

fn session() -> Session {
    let mut s = Session::new(std::env::temp_dir(), "build", 100_000, 60);
    s.graph
        .install(
            [("a", vec![]), ("b", vec!["a"]), ("c", vec![])]
                .into_iter()
                .map(|(id, deps)| Step {
                    id: id.into(),
                    description: id.into(),
                    requirements: BTreeSet::from(["r1".into()]),
                    dependencies: deps.into_iter().map(str::to_owned).collect(),
                    files: BTreeSet::from([format!("{id}.txt")]),
                    revision: 1,
                    status: StepStatus::Pending,
                })
                .collect(),
            &s.goal,
        )
        .unwrap();
    s
}

#[test]
fn local_correction_invalidates_downstream_evidence_but_preserves_independent_work() {
    let mut s = session();
    let assignment = s.assign_member(Some("Ada"), "a", "build a").unwrap();
    let prior = s.graph.versions();
    let evidence = Evidence {
        id: "proof".into(),
        kind: EvidenceKind::Review,
        epoch: s.goal.epoch,
        requirements: s.goal.active_versions(),
        files: BTreeMap::new(),
        steps: BTreeMap::from([("b".into(), prior["b"])]),
        passed: true,
        description: "old proof".into(),
        artifact: None,
    };
    let affected = s.invalidate_step("a", "先重读文件，再改这一步").unwrap();
    assert_eq!(affected, BTreeSet::from(["a".into(), "b".into()]));
    assert_eq!(s.graph.versions()["c"], prior["c"]);
    assert!(!evidence.current_at(&s.goal, &BTreeMap::new(), &s.graph.versions()));
    assert_eq!(
        s.goal.revision, 1,
        "local execution does not replace delivery requirements"
    );
    assert_eq!(s.team.corrections[0].source, "先重读文件，再改这一步");
    assert!(s.team.assignments[&assignment.id].version > assignment.version);
}

#[test]
fn waiting_assignment_and_unknown_mail_prevent_completion_but_idle_member_does_not() {
    let mut s = session();
    let a = s.assign_member(Some("Ada"), "a", "build").unwrap();
    s.team.wait(&a.member, "main").unwrap();
    assert!(!s.team.engineering_settled());
    s.finish_assignment(&a.actor, loh::runtime::team::AssignmentStatus::Finished);
    assert!(s.team.engineering_settled());
    s.team
        .send(
            "input",
            "user",
            s.member_target("Ada").unwrap(),
            "change acceptance",
            true,
        )
        .unwrap();
    assert!(!s.team.engineering_settled());
}

#[test]
fn member_handoff_to_another_step_waits_for_its_old_write() {
    let mut s = session();
    let a = s.assign_member(Some("Ada"), "a", "build").unwrap();
    let mut scope = s.graph.scope("a", &s.goal).unwrap();
    scope.readonly = false;
    s.actions.insert(
        "old-write".into(),
        serde_json::from_value(serde_json::json!({
            "id":"old-write","actor":a.actor,"running":true,"pid":null,"approval":null,
            "pending":{"call":{"id":"call","name":"apply_patch","arguments":{}},"scope":scope}
        }))
        .unwrap(),
    );
    s.finish_assignment(&a.actor, loh::runtime::team::AssignmentStatus::Invalidated);
    assert!(
        s.assign_member(Some("Ada"), "c", "replacement").is_err(),
        "a new task must not reuse a member while its previous write is settling"
    );
    s.actions.get_mut("old-write").unwrap().running = false;
    assert!(s.assign_member(Some("Ada"), "c", "replacement").is_ok());
}

use loh::{
    domain::{EventData, Status, Usage},
    model::{ModelEvent, Provider, Request as ModelRequest, Response},
    runtime::{
        runner::{Runner, Settings},
        state::{Control, Question},
        team::{MemberControl, ReceiptStatus, Request},
    },
    store::Store,
    tools::{ToolHost, files::Workspace},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::sync::{RwLock, mpsc};
use tokio_util::sync::CancellationToken;

struct Classifier {
    replies: Mutex<Vec<Value>>,
    prompts: Mutex<Vec<ModelRequest>>,
}
#[async_trait::async_trait]
impl Provider for Classifier {
    async fn respond(
        &self,
        request: ModelRequest,
        _: CancellationToken,
        _: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        assert!(request.instructions.starts_with("ROLE: steering"));
        assert_eq!(request.tools.len(), 1);
        self.prompts.lock().unwrap().push(request);
        let arguments = self.replies.lock().unwrap().remove(0);
        Ok(Response {
            output: vec![
                json!({"type":"function_call","call_id":uuid::Uuid::new_v4().to_string(),"name":"classify_input","arguments":arguments.to_string()}),
            ],
            usage: Usage::default(),
            model: "offline".into(),
        })
    }
}

async fn classify(
    mut s: Session,
    replies: Vec<Value>,
    text: &str,
    target_name: &str,
) -> (Session, Arc<Classifier>, Vec<ReceiptStatus>) {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    s.workspace = root.path().into();
    s.status = Status::Paused;
    let target = s.member_target(target_name).unwrap();
    let id = s.id.clone();
    let host = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(RwLock::new(s.goal.clone())),
        state.path().join("logs"),
    ));
    let store = Arc::new(Store::open(&state.path().join("db")).unwrap());
    let provider = Arc::new(Classifier {
        replies: Mutex::new(replies),
        prompts: Mutex::new(vec![]),
    });
    let (runner, control, mut events) = Runner::new(
        s,
        store.clone(),
        provider.clone(),
        host,
        Settings {
            max_output: 128,
            ..Settings::default()
        },
    );
    control
        .send(Control::Directed {
            request: Request {
                id: "input".into(),
                client_id: "test".into(),
                target,
                action: MemberControl::Message { text: text.into() },
            },
        })
        .await
        .unwrap();
    let task = tokio::spawn(runner.run());
    let observed = tokio::time::timeout(std::time::Duration::from_secs(4), async {
        let mut statuses = vec![];
        loop {
            let event = events.recv().await.unwrap();
            if let EventData::Diagnostic { message } = event.data {
                if message.starts_with("directed request [\"test\",\"input\"]:") {
                    statuses.push(ReceiptStatus::Pending);
                }
                let saved = store.load::<Session>(&id).unwrap();
                if saved
                    .team
                    .receipts
                    .get("[\"test\",\"input\"]")
                    .is_some_and(|r| r.status == ReceiptStatus::Applied)
                {
                    statuses.push(ReceiptStatus::Applied);
                    break statuses;
                }
            }
        }
    })
    .await;
    control.send(Control::Stop).await.unwrap();
    let saved = tokio::time::timeout(std::time::Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    (saved, provider, observed.unwrap())
}

#[tokio::test]
async fn delivery_correction_enters_canonical_requirements_and_receipt_waits_for_application() {
    let mut s = session();
    let a = s.assign_member(Some("Ada"), "a", "build").unwrap();
    let (s, provider, receipts) = classify(
        s,
        vec![json!({"intent":"refine","updates":{},"additions":["Export CSV with headers"]})],
        "验收需要带表头的 CSV",
        "Ada",
    )
    .await;
    assert_eq!(
        receipts,
        vec![ReceiptStatus::Pending, ReceiptStatus::Applied]
    );
    assert!(
        s.goal
            .requirements
            .values()
            .any(|r| r.source == "验收需要带表头的 CSV" && r.text == "Export CSV with headers")
    );
    assert_eq!(
        s.team.assignments[&a.id].status,
        loh::runtime::team::AssignmentStatus::Invalidated
    );
    assert!(s.inbox.is_empty());
    assert_eq!(provider.prompts.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn member_side_question_restores_only_temporary_interruption() {
    for paused in [false, true] {
        let mut s = session();
        let a = s.assign_member(Some("Ada"), "a", "build").unwrap();
        s.team.members.get_mut(&a.member).unwrap().paused = paused;
        let (s, _, _) = classify(
            s,
            vec![json!({"intent":"question","answer":"当前还在等待","needs_tools":false})],
            "这一步进度如何？",
            "Ada",
        )
        .await;
        assert_eq!(s.goal.revision, 1);
        assert_eq!(s.team.members[&a.member].paused, paused);
        assert!(!s.team.members[&a.member].interrupted);
        assert!(s.inbox.is_empty());
        assert!(s.team.corrections.is_empty());
    }
}

#[tokio::test]
async fn member_answer_can_resolve_own_question_but_cannot_answer_a_peer() {
    let mut s = session();
    s.assign_member(Some("Ada"), "a", "build").unwrap();
    s.assign_member(Some("Bob"), "c", "other").unwrap();
    for (id, name) in [("own", "Ada"), ("peer", "Bob")] {
        s.questions.insert(
            id.into(),
            Question {
                target: Some(s.member_target(name).unwrap()),
                text: "format?".into(),
                requirements: s.goal.active_versions(),
                answer: None,
            },
        );
    }
    let (s, provider, _) = classify(
        s,
        vec![
            json!({"intent":"answer","question_id":"peer"}),
            json!({"intent":"answer","question_id":"own"}),
        ],
        "使用 CSV",
        "Ada",
    )
    .await;
    assert_eq!(s.questions["own"].answer.as_deref(), Some("使用 CSV"));
    assert!(s.questions["peer"].answer.is_none());
    assert_eq!(provider.prompts.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn directed_main_correction_receipt_also_waits_for_canonical_application() {
    let (s,_,statuses)=classify(session(),vec![json!({"intent":"refine","updates":{"r1":"build with CSV"},"additions":[],"answer":"","reason":"delivery"})],"require CSV","main").await;
    assert_eq!(
        statuses,
        vec![ReceiptStatus::Pending, ReceiptStatus::Applied]
    );
    assert_eq!(s.goal.requirements["r1"].source, "require CSV");
    assert!(
        s.team.receipts["[\"test\",\"input\"]"]
            .pending_input
            .is_none()
    );
    assert!(s.inbox[0].applied);
}

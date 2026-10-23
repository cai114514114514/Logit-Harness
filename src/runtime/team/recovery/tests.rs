use super::*;
use crate::{
    domain::{Change, Status},
    extensions::{knowledge::Knowledge, skills::Skills},
    model::{ModelEvent, Provider, Request as ModelRequest, Response},
    runtime::{
        conversation::{Boundary, Mode},
        recovery,
        runner::{Observer, Runner, Settings},
        team::{Receipt, Request},
    },
    tools::{ToolHost, files::Workspace},
};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
};
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
        panic!("recovery and boundary settlement must not call a provider")
    }
}
struct Checkpoint {
    store: Arc<Store>,
    key: String,
    saved: Mutex<Option<Session>>,
}
impl Observer for Checkpoint {
    fn on_event(&self, event: &crate::domain::SessionEvent) -> anyhow::Result<()> {
        let state: Session = self.store.load(&event.session)?;
        if state.boundary.is_some()
            && state
                .team
                .receipts
                .get(&self.key)
                .is_some_and(|r| r.applying)
        {
            *self.saved.lock().unwrap() = Some(state);
        }
        Ok(())
    }
}
fn runner(root: &Path, session: Session, store: Arc<Store>) -> Runner {
    let mut host = ToolHost::new(
        Workspace::open(root).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        root.join("logs"),
    );
    host.knowledge = Some(Arc::new(Knowledge::new(root.into(), vec![]).unwrap()));
    Runner::new(
        session,
        store,
        Arc::new(NoRequests),
        Arc::new(host),
        Settings::default(),
    )
    .0
}
async fn checkpoint(root: &Path, action: MemberControl) -> (Session, Request, Arc<Store>) {
    let mut session = Session::conversation(root.into(), "existing conversation", 10000, 60);
    // A busy old response makes the real Message handler defer its new turn.
    session.actors.get_mut("main").unwrap().busy = 1;
    let store = Arc::new(Store::open(&root.join("state.db")).unwrap());
    let request = Request {
        id: "uncertain".into(),
        client_id: "fixture".into(),
        target: Target::Main,
        action,
    };
    let captured = Arc::new(Checkpoint {
        store: store.clone(),
        key: request.key(),
        saved: Mutex::new(None),
    });
    let mut original = runner(root, session, store.clone()).observe(captured.clone());
    original.directed_control(request.clone()).await.unwrap();
    let saved = captured
        .saved
        .lock()
        .unwrap()
        .take()
        .expect("real handler must persist an applying receipt with its boundary");
    assert!(saved.team.receipts[&request.key()].applying);
    assert_eq!(
        saved.team.receipts[&request.key()].pending_input.as_deref(),
        Some(request.key().as_str())
    );
    (saved, request, store)
}
async fn recover(root: &Path, state: &Session, store: Arc<Store>) -> Runner {
    store
        .commit(
            &state.id,
            state,
            EventData::Diagnostic {
                message: "simulated crash checkpoint".into(),
            },
        )
        .unwrap();
    let mut reloaded: Session = store.load(&state.id).unwrap();
    recovery::reconcile(&mut reloaded, &store, &root.join("logs"))
        .await
        .unwrap();
    runner(root, reloaded, store)
}

#[tokio::test]
async fn main_message_crash_quarantines_its_saved_conversation_boundary() {
    let root = tempfile::tempdir().unwrap();
    let (saved, request, store) = checkpoint(
        root.path(),
        MemberControl::Message {
            text: "failed input must not run".into(),
        },
    )
    .await;
    assert!(
        matches!(&saved.boundary,Some(Boundary::Conversation { turn,.. }) if turn==&request.key())
    );
    let mut recovered = recover(root.path(), &saved, store).await;
    recovered.settle_boundary().await.unwrap();
    assert!(recovered.session.boundary.is_none());
    assert_ne!(
        recovered.session.actors["main"].task, "failed input must not run",
        "a failed request must not re-enter through its saved boundary"
    );
    assert_ne!(recovered.session.actors["main"].turn, request.key());
    assert_eq!(
        recovered.session.team.receipts[&request.key()].status,
        ReceiptStatus::Failed
    );
    assert_eq!(
        recovered.session.team.receipts[&request.key()].request,
        request
    );
    assert!(recovered.session.goal.id.is_empty());
}

#[tokio::test]
async fn main_init_crash_quarantines_its_saved_goal_boundary_but_preserves_unrelated_skill() {
    let root = tempfile::tempdir().unwrap();
    let init = Skills::default()
        .with_bundled()
        .invoke("bundled:init", "literal '$arg'")
        .unwrap();
    let (mut saved, request, store) = checkpoint(
        root.path(),
        MemberControl::Skill {
            id: init.id,
            hash: init.hash,
            arguments: init.arguments,
        },
    )
    .await;
    assert!(matches!(&saved.boundary,Some(Boundary::Goal { turn,.. }) if turn==&request.key()));
    let review = Skills::default()
        .with_bundled()
        .invoke("bundled:review", "independent pending workflow")
        .unwrap();
    saved
        .skill_pending
        .push(crate::extensions::skills::Request {
            id: review.id.clone(),
            hash: review.hash.clone(),
            arguments: review.arguments.clone(),
        });
    let mut recovered = recover(root.path(), &saved, store).await;
    recovered.settle_boundary().await.unwrap();
    assert_eq!(
        recovered.session.mode,
        Mode::Conversation,
        "a failed init request cannot acquire engineering ownership through an old boundary"
    );
    assert!(recovered.session.goal.id.is_empty());
    assert!(
        recovered
            .session
            .actors
            .values()
            .all(|a| a.turn != request.key())
    );
    assert_eq!(
        recovered.session.team.receipts[&request.key()].status,
        ReceiptStatus::Failed
    );
    assert!(
        recovered.session.workflows.contains_key(&request.key()),
        "passive workflow provenance remains inspectable"
    );
    assert_eq!(recovered.session.skill_pending.len(), 1);
    assert_eq!(recovered.session.skill_pending[0].id, review.id);
    assert_eq!(
        recovered.session.skill_pending[0].arguments,
        review.arguments
    );
    assert!(!root.path().join("AGENTS.md").exists());
}

#[tokio::test]
async fn uncertain_request_preserves_unrelated_boundaries_and_existing_goal_effects() {
    for turn in [Some("unrelated-turn".to_owned()), None] {
        let root = tempfile::tempdir().unwrap();
        let mut state = Session::new(root.path().into(), "keep existing deliverable", 10000, 60);
        state.status = Status::Running;
        state
            .goal
            .revise(Change {
                source: "known applied correction".into(),
                updates: BTreeMap::from([("r1".into(), "keep already revised requirement".into())]),
                additions: vec![],
                replacement: false,
            })
            .unwrap();
        let original_goal = state.goal.clone();
        std::fs::write(
            root.path().join("known-effect.txt"),
            "already applied file effect",
        )
        .unwrap();
        let request = Request {
            id: "uncertain".into(),
            client_id: "fixture".into(),
            target: Target::Main,
            action: MemberControl::Message {
                text: "uncertain later request".into(),
            },
        };
        state.team.receipts.insert(
            request.key(),
            Receipt {
                applying: true,
                request: request.clone(),
                status: ReceiptStatus::Pending,
                error: None,
                pending_input: Some(request.key()),
            },
        );
        state.boundary = Some(Boundary::Plan {
            text: Some("independently requested plan".into()),
            turn: turn.clone(),
        });
        let store = Arc::new(Store::open(&root.path().join("state.db")).unwrap());
        let mut recovered = recover(root.path(), &state, store).await;
        assert!(
            matches!(&recovered.session.boundary,Some(Boundary::Plan { turn:actual,.. }) if actual==&turn)
        );
        recovered.settle_boundary().await.unwrap();
        assert_eq!(recovered.session.mode, Mode::Plan);
        assert_eq!(recovered.session.goal.id, original_goal.id);
        assert_eq!(recovered.session.goal.revision, original_goal.revision);
        assert_eq!(
            recovered.session.goal.requirements["r1"].text,
            original_goal.requirements["r1"].text
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("known-effect.txt")).unwrap(),
            "already applied file effect"
        );
        assert_eq!(
            recovered.session.team.receipts[&request.key()].status,
            ReceiptStatus::Failed
        );
    }
}

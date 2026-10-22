#[allow(dead_code)]
#[path = "fixtures/chat.rs"]
mod chat;

use loh::{
    domain::{EventData, Status},
    model::{ModelEvent, Provider, Request, Response},
    runtime::{
        conversation::Mode,
        runner::{Runner, Settings},
        state::{Control, Session},
    },
    store::Store,
    tools::{ToolHost, files::Workspace},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;

struct Metered {
    requests: AtomicUsize,
    hold_first: bool,
    started: Semaphore,
    settle: Semaphore,
}
impl Metered {
    fn new(hold_first: bool) -> Arc<Self> {
        Arc::new(Self {
            requests: AtomicUsize::new(0),
            hold_first,
            started: Semaphore::new(0),
            settle: Semaphore::new(0),
        })
    }
}
#[async_trait::async_trait]
impl Provider for Metered {
    async fn respond(
        &self,
        _: Request,
        cancel: CancellationToken,
        _: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        let index = self.requests.fetch_add(1, Ordering::SeqCst);
        if self.hold_first && index == 0 {
            self.started.add_permits(1);
            cancel.cancelled().await;
            self.settle.acquire().await.unwrap().forget();
            anyhow::bail!("cancelled fixture request");
        }
        Ok(chat::text("Read-only continuation."))
    }
}

async fn exhausted(live: &mut chat::Live) {
    live.until(|e| {
        matches!(
            e,
            EventData::Status {
                status: Status::Exhausted,
                ..
            }
        )
    })
    .await;
}

#[tokio::test]
async fn queued_resume_cannot_dispatch_with_zero_or_expired_positive_time() {
    for mode in [Mode::Conversation, Mode::Plan] {
        for seconds in [0, 5] {
            let root = tempfile::tempdir().unwrap();
            let state = tempfile::tempdir().unwrap();
            let mut session = Session::conversation(
                root.path().into(),
                "Explain this concept",
                1_000_000,
                seconds,
            );
            session.mode = mode;
            session.elapsed = seconds;
            session.status = Status::Paused;
            let turn = session.actors["main"].turn.clone();
            let id = session.id.clone();
            let store = Arc::new(Store::open(&state.path().join("db")).unwrap());
            let provider = Metered::new(false);
            let tools = ToolHost::new(
                Workspace::open(root.path()).unwrap(),
                Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
                state.path().join("logs"),
            );
            let (runner, control, events) = Runner::new(
                session,
                store.clone(),
                provider.clone(),
                Arc::new(tools),
                Settings {
                    max_output: 1024,
                    ..Default::default()
                },
            );
            // Queue before the runner starts: its pre-control check sees Paused.
            control.send(Control::Resume).await.unwrap();
            let mut live = chat::Live {
                _state: state,
                store,
                id,
                control,
                events,
                task: tokio::spawn(runner.run()),
            };
            exhausted(&mut live).await;
            assert_eq!(
                provider.requests.load(Ordering::SeqCst),
                0,
                "{mode:?} resumed beyond {seconds}s allowance"
            );
            let paused = live.saved();
            assert!(paused.budget.reservations.is_empty());
            live.control
                .send(Control::Budget {
                    tokens: None,
                    seconds: Some(seconds + 60),
                })
                .await
                .unwrap();
            live.control.send(Control::Resume).await.unwrap();
            live.waiting().await;
            let saved = live.saved();
            assert_eq!(provider.requests.load(Ordering::SeqCst), 1);
            assert_eq!(saved.mode, mode);
            assert_eq!(saved.actors["main"].turn, turn);
            assert!(saved.goal.id.is_empty() && saved.archives.is_empty());
            live.stop().await;
        }
    }
}

#[tokio::test]
async fn early_resume_rechecks_expired_time_when_cancellation_settles() {
    for mode in [Mode::Conversation, Mode::Plan] {
        let root = tempfile::tempdir().unwrap();
        let mut session =
            Session::conversation(root.path().into(), "Explain this concept", 1_000_000, 120);
        session.mode = mode;
        // Saved work already consumed time; lowering the allowance while
        // cancelling makes it expired without introducing wall-clock sleeps.
        session.elapsed = 5;
        let turn = session.actors["main"].turn.clone();
        let provider = Metered::new(true);
        let mut live = chat::Live::start(session, provider.clone());
        provider.started.acquire().await.unwrap().forget();
        live.control.send(Control::Pause).await.unwrap();
        live.until(|e| {
            matches!(
                e,
                EventData::Status {
                    status: Status::Pausing,
                    ..
                }
            )
        })
        .await;
        live.control
            .send(Control::Budget {
                tokens: None,
                seconds: Some(5),
            })
            .await
            .unwrap();
        live.control.send(Control::Resume).await.unwrap();
        live.until(
            |e| matches!(e, EventData::Diagnostic { message } if message.contains("resume queued")),
        )
        .await;
        assert_eq!(provider.requests.load(Ordering::SeqCst), 1);
        provider.settle.add_permits(1);
        exhausted(&mut live).await;
        assert_eq!(
            provider.requests.load(Ordering::SeqCst),
            1,
            "replacement was submitted after time expired"
        );
        assert!(live.saved().budget.reservations.is_empty());
        live.control
            .send(Control::Budget {
                tokens: None,
                seconds: Some(120),
            })
            .await
            .unwrap();
        live.control.send(Control::Resume).await.unwrap();
        live.waiting().await;
        let saved = live.saved();
        assert_eq!(provider.requests.load(Ordering::SeqCst), 2);
        assert_eq!(saved.mode, mode);
        assert_eq!(saved.actors["main"].turn, turn);
        assert!(saved.goal.id.is_empty() && saved.archives.is_empty());
        live.stop().await;
    }
}

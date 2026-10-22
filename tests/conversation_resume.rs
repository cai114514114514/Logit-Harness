#[allow(dead_code)]
#[path = "fixtures/chat.rs"]
mod chat;

use loh::{
    domain::{EventData, Status},
    model::{ModelEvent, Provider, Request, Response},
    runtime::{
        conversation::Mode,
        state::{Control, Session},
    },
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;

struct Interrupted {
    requests: Mutex<Vec<Request>>,
    first: AtomicBool,
    started: Semaphore,
    settle: Semaphore,
}
impl Interrupted {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::new(vec![]),
            first: AtomicBool::new(true),
            started: Semaphore::new(0),
            settle: Semaphore::new(0),
        })
    }
}
#[async_trait::async_trait]
impl Provider for Interrupted {
    async fn respond(
        &self,
        request: Request,
        cancel: CancellationToken,
        _: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        self.requests.lock().unwrap().push(request);
        if self.first.swap(false, Ordering::SeqCst) {
            self.started.add_permits(1);
            cancel.cancelled().await;
            self.settle.acquire().await.unwrap().forget();
            anyhow::bail!("cancelled fixture request");
        }
        Ok(chat::text("Saved read-only request continued."))
    }
}

async fn paused_request(mode: Mode, early_resume: bool, recover: bool) {
    let root = tempfile::tempdir().unwrap();
    let provider = Interrupted::new();
    let mut live = chat::Live::start(
        Session::conversation(
            root.path().into(),
            if mode == Mode::Conversation {
                "Explain this concept"
            } else {
                ""
            },
            1_000_000,
            60,
        ),
        provider.clone(),
    );
    if mode == Mode::Plan {
        live.control
            .send(Control::Plan {
                text: Some("Plan the requested change".into()),
            })
            .await
            .unwrap();
    }
    provider.started.acquire().await.unwrap().forget();
    let before = live.saved();
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
    if early_resume {
        live.control.send(Control::Resume).await.unwrap();
        live.until(
            |e| matches!(e, EventData::Diagnostic { message } if message.contains("resume")),
        )
        .await;
        assert_eq!(
            provider.requests.lock().unwrap().len(),
            1,
            "replacement started before cancellation settled"
        );
    }
    provider.settle.add_permits(1);
    if !early_resume {
        live.until(|e| {
            matches!(
                e,
                EventData::Status {
                    status: Status::Paused,
                    ..
                }
            )
        })
        .await;
        if recover {
            restart(&mut live, provider.clone()).await;
        }
        live.control.send(Control::Resume).await.unwrap();
    }
    live.waiting().await;
    let after = live.saved();
    assert_eq!(after.mode, mode);
    assert!(after.active_goal().is_none() && after.archives.is_empty());
    assert_eq!(after.goal.id, before.goal.id);
    assert_eq!(after.actors["main"].turn, before.actors["main"].turn);
    assert!(after.actors["main"].context.unanswered().is_empty());
    assert!(after.budget.reservations.is_empty());
    let requests = provider.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|r| r.instructions.starts_with("ROLE: conversation"))
    );
    assert!(
        requests
            .iter()
            .all(|r| !r.tools.iter().any(|t| t["name"] == "apply_patch"))
    );
    live.stop().await;
}

#[tokio::test]
async fn resume_paused_conversation_after_settlement() {
    paused_request(Mode::Conversation, false, false).await;
}

#[tokio::test]
async fn resume_paused_plan_waits_for_inflight_cancellation() {
    paused_request(Mode::Plan, true, false).await;
}

#[tokio::test]
async fn resume_paused_plan_after_restart_preserves_original_turn() {
    paused_request(Mode::Plan, false, true).await;
}

#[tokio::test]
async fn resume_does_not_restart_idle_plan() {
    let root = tempfile::tempdir().unwrap();
    let provider = Interrupted::new();
    let mut live = chat::Live::start(
        Session::conversation(root.path().into(), "", 1_000_000, 60),
        provider.clone(),
    );
    live.control
        .send(Control::Plan { text: None })
        .await
        .unwrap();
    live.waiting().await;
    live.control.send(Control::Pause).await.unwrap();
    live.until(|e| {
        matches!(
            e,
            EventData::Status {
                status: Status::Paused,
                ..
            }
        )
    })
    .await;
    live.control.send(Control::Resume).await.unwrap();
    live.until(|e| matches!(e, EventData::Diagnostic { message } if message.contains("no paused execution"))).await;
    assert_eq!(live.saved().mode, Mode::Plan);
    assert!(provider.requests.lock().unwrap().is_empty());
    live.stop().await;
}

struct Recovering {
    requests: Mutex<Vec<Request>>,
    failures: std::sync::atomic::AtomicUsize,
    promotion: AtomicBool,
}
#[async_trait::async_trait]
impl Provider for Recovering {
    async fn respond(
        &self,
        request: Request,
        _: CancellationToken,
        _: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        self.requests.lock().unwrap().push(request);
        if self
            .failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            anyhow::bail!("fixture provider failed");
        }
        if self.promotion.swap(false, Ordering::SeqCst) {
            return Ok(chat::call("begin_goal", serde_json::json!({})));
        }
        Ok(chat::text("Continued read-only after recovery."))
    }
}

/// Restart only this fixture runner, retaining its store and saved actor state.
async fn restart(live: &mut chat::Live, provider: Arc<dyn Provider>) {
    live.task.abort();
    let _ = (&mut live.task).await;
    let session = live.saved();
    let tools = loh::tools::ToolHost::new(
        loh::tools::files::Workspace::open(&session.workspace).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        live._state.path().join("logs"),
    );
    let (runner, control, events) = loh::runtime::runner::Runner::new(
        session,
        live.store.clone(),
        provider,
        Arc::new(tools),
        loh::runtime::runner::Settings {
            max_output: 1024,
            ..Default::default()
        },
    );
    live.control = control;
    live.events = events;
    live.task = tokio::spawn(runner.recover_on_start().run());
}

#[tokio::test]
async fn failed_readonly_request_resumes_after_restart_without_goal_authorization() {
    let root = tempfile::tempdir().unwrap();
    let provider = Arc::new(Recovering {
        requests: Mutex::new(vec![]),
        failures: std::sync::atomic::AtomicUsize::new(3),
        promotion: AtomicBool::new(true),
    });
    let mut live = chat::Live::start(
        Session::conversation(root.path().into(), "Explain a concept", 1_000_000, 60),
        provider.clone(),
    );
    live.until(|e| {
        matches!(
            e,
            EventData::Status {
                status: Status::Failed,
                ..
            }
        )
    })
    .await;
    let turn = live.saved().actors["main"].turn.clone();
    restart(&mut live, provider.clone()).await;
    live.control.send(Control::Resume).await.unwrap();
    live.waiting().await;
    let saved = live.saved();
    assert_eq!(saved.mode, Mode::Conversation);
    assert!(saved.goal.id.is_empty() && saved.archives.is_empty());
    assert_eq!(saved.actors["main"].turn, turn);
    assert!(
        saved.actors["main"].context.unanswered().is_empty(),
        "rejected promotion must have a paired tool response"
    );
    assert_eq!(provider.requests.lock().unwrap().len(), 5);
    live.stop().await;
}

#[tokio::test]
async fn exhausted_plan_resume_still_enforces_budget_after_recovery() {
    let root = tempfile::tempdir().unwrap();
    let provider = Arc::new(Recovering {
        requests: Mutex::new(vec![]),
        failures: std::sync::atomic::AtomicUsize::new(0),
        promotion: AtomicBool::new(false),
    });
    let mut live = chat::Live::start(
        Session::conversation(root.path().into(), "", 0, 60),
        provider.clone(),
    );
    live.control
        .send(Control::Plan {
            text: Some("Plan this change".into()),
        })
        .await
        .unwrap();
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
    let turn = live.saved().actors["main"].turn.clone();
    restart(&mut live, provider.clone()).await;
    live.control.send(Control::Resume).await.unwrap();
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
    assert!(provider.requests.lock().unwrap().is_empty());
    live.control
        .send(Control::Budget {
            tokens: Some(1_000_000),
            seconds: None,
        })
        .await
        .unwrap();
    live.control.send(Control::Resume).await.unwrap();
    live.waiting().await;
    let saved = live.saved();
    assert_eq!(saved.mode, Mode::Plan);
    assert_eq!(saved.actors["main"].turn, turn);
    assert!(saved.goal.id.is_empty());
    assert_eq!(provider.requests.lock().unwrap().len(), 1);
    live.stop().await;
}

//! A history too long for the model is the harness's to repair: compact and
//! retry, with bounded summary requests, instead of pausing for the user.
use async_trait::async_trait;
use loh::{
    domain::{EventData, Status, Usage},
    model::{ModelEvent, Provider, Request, RequestFailure, Response},
    runtime::{
        runner::{Runner, Settings},
        state::{Control, Session},
    },
    store::Store,
    tools::{ToolHost, files::Workspace},
};
use serde_json::json;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// The provider's answer to a request over the model's context window.
fn overflow() -> anyhow::Error {
    RequestFailure::Rejected {
        status: 400,
        detail: r#"{"error":{"message":"This model's maximum context length is 1048576 tokens. However, you requested 1301918 tokens (1297822 in the messages, 4096 in the completion).","type":"invalid_request_error"}}"#.into(),
    }
    .into()
}

fn summary(text: &str) -> Response {
    Response {
        output: vec![
            json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}),
        ],
        usage: Usage {
            input: 100,
            output: 20,
            cached: 0,
        },
        model: "fixture".into(),
    }
}

#[derive(Default)]
struct Script {
    /// Overflow every summary request instead of answering it.
    summaries_overflow: bool,
    /// Roles of the requests seen, in order.
    roles: Mutex<Vec<String>>,
    /// Byte size of every summary request's input.
    summary_sizes: Mutex<Vec<usize>>,
    main: AtomicUsize,
}

#[async_trait]
impl Provider for Script {
    async fn respond(
        &self,
        request: Request,
        cancel: CancellationToken,
        _: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        let role = request.instructions.lines().next().unwrap_or("").to_owned();
        self.roles.lock().unwrap().push(role.clone());
        if role == "ROLE: compact" {
            let input = serde_json::to_vec(&request.input).unwrap();
            self.summary_sizes.lock().unwrap().push(input.len());
            assert!(
                !self.summaries_overflow || String::from_utf8_lossy(&input).contains("[omitted "),
                "an oversized history must be omitted from the summary request"
            );
            return if self.summaries_overflow {
                Err(overflow())
            } else {
                Ok(summary("已审查工作区；没有需要修改的源码。"))
            };
        }
        if self.main.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(overflow());
        }
        // Keep the session busy until the test stops it.
        cancel.cancelled().await;
        anyhow::bail!("fixture stopped")
    }
}

fn runner(
    session: Session,
    provider: Arc<Script>,
    workspace: &std::path::Path,
    state: &std::path::Path,
) -> (
    Runner,
    mpsc::Sender<Control>,
    tokio::sync::broadcast::Receiver<loh::domain::SessionEvent>,
) {
    let tools = Arc::new(ToolHost::new(
        Workspace::open(workspace).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        state.join("logs"),
    ));
    let store = Arc::new(Store::open(&state.join("state.db")).unwrap());
    Runner::new(
        session,
        store,
        provider,
        tools,
        Settings {
            linger: false,
            max_output: 1024,
            ..Settings::default()
        },
    )
}

#[tokio::test]
async fn an_overflowing_request_is_compacted_and_retried_without_pausing() {
    let workspace = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let session = Session::new(workspace.path().into(), "帮我审查一下代码", 100_000_000, 60);
    let provider = Arc::new(Script::default());
    let (runner, control, mut events) =
        runner(session, provider.clone(), workspace.path(), state.path());
    let running = tokio::spawn(runner.run());
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while provider.main.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the main agent resumes after compaction");
    control.send(Control::Stop).await.unwrap();
    let session = running.await.unwrap().unwrap();
    assert_ne!(session.status, Status::Failed);
    assert_eq!(
        *provider.roles.lock().unwrap(),
        ["ROLE: main", "ROLE: compact", "ROLE: main"]
    );
    let mut paused = false;
    while let Ok(event) = events.try_recv() {
        if let EventData::Status { status, .. } = event.data {
            paused |= matches!(status, Status::Failed | Status::Paused);
        }
    }
    assert!(
        !paused,
        "an overflow must not stop the session for the user"
    );
}

#[tokio::test]
async fn summary_requests_shrink_until_the_runtime_gives_up_with_a_reason() {
    let workspace = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut session = Session::new(workspace.path().into(), "帮我审查一下代码", 100_000_000, 60);
    // The incident: one tool result carrying thousands of fingerprints.
    let files = (0..20_000)
        .map(|i| (format!("artifacts/run/{i}.txt"), json!(format!("{i:064x}"))))
        .collect::<serde_json::Map<_, _>>();
    let main = session.actors.get_mut("main").unwrap();
    main.context.append(&[
        json!({"type":"function_call","call_id":"diff","name":"project_diff","arguments":"{}"}),
    ]);
    main.context
        .tool_result("diff", &json!({"result":{"files":files}}));
    let provider = Arc::new(Script {
        summaries_overflow: true,
        ..Script::default()
    });
    let (runner, _control, mut events) =
        runner(session, provider.clone(), workspace.path(), state.path());
    let session = tokio::time::timeout(std::time::Duration::from_secs(10), runner.run())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(session.status, Status::Failed);
    let sizes = provider.summary_sizes.lock().unwrap().clone();
    assert_eq!(
        sizes.len(),
        4,
        "one summary attempt and three tighter retries"
    );
    assert!(
        sizes[0] <= 2 * 256 * 1024,
        "first summary request is bounded: {sizes:?}"
    );
    assert!(
        provider
            .roles
            .lock()
            .unwrap()
            .iter()
            .all(|role| role == "ROLE: compact"),
        "nothing is sent unbounded while the history is oversized"
    );
    // Halting announces its reason, then settles into Failed.
    let mut reasons = vec![];
    while let Ok(event) = events.try_recv() {
        if let EventData::Status { reason, .. } = event.data {
            reasons.push(reason);
        }
    }
    assert!(
        reasons.iter().any(|reason| reason
            == "context still exceeds the model limit after compaction; progress is saved"),
        "{reasons:?}"
    );
}

//! Summary requests must leave a reasoning model room to finish, and must ask
//! for the summary explicitly rather than let it resume the latest task.
use async_trait::async_trait;
use loh::{
    domain::Usage,
    model::{ModelEvent, Provider, ReportedFailure, Request, Response},
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

const MAX_OUTPUT: u32 = 32_768;

#[derive(Default)]
struct Reasoner {
    summaries: Mutex<Vec<Request>>,
    main: AtomicUsize,
}

#[async_trait]
impl Provider for Reasoner {
    async fn respond(
        &self,
        request: Request,
        cancel: CancellationToken,
        _: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        if request.instructions.starts_with("ROLE: compact") {
            let attempt = {
                let mut summaries = self.summaries.lock().unwrap();
                summaries.push(request.clone());
                summaries.len()
            };
            if attempt == 1 {
                // The 2026-10-26 failure: every output token spent on reasoning.
                return Err(ReportedFailure {
                    reason: "provider response did not complete: \"response.incomplete\"".into(),
                    usage: Usage {
                        input: 72_055,
                        output: 4096,
                        cached: 0,
                    },
                    response: json!({"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"}}),
                }
                .into());
            }
            return Ok(Response {
                output: vec![
                    json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"已审查 runtime；发现两处问题，待修复。"}]}),
                ],
                usage: Usage {
                    input: 1000,
                    output: 200,
                    cached: 0,
                },
                model: "fixture".into(),
            });
        }
        self.main.fetch_add(1, Ordering::SeqCst);
        cancel.cancelled().await;
        anyhow::bail!("fixture stopped")
    }
}

#[tokio::test]
async fn a_summary_request_has_the_full_output_budget_and_an_explicit_last_turn() {
    let workspace = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut session = Session::new(workspace.path().into(), "帮我审查一下代码", 100_000_000, 60);
    // Just over the default 256 KB compaction threshold, ending with the task.
    let main = session.actors.get_mut("main").unwrap();
    for i in 0..30 {
        main.context
            .user(&format!("第 {i} 段审查记录：{}", "x".repeat(9_000)));
    }
    main.context.user("继续审查 runtime 目录");
    let history = main.context.items.len();
    let provider = Arc::new(Reasoner::default());
    let tools = Arc::new(ToolHost::new(
        Workspace::open(workspace.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        state.path().join("logs"),
    ));
    let (runner, control, _events) = Runner::new(
        session,
        Arc::new(Store::open(&state.path().join("db")).unwrap()),
        provider.clone(),
        tools,
        Settings {
            linger: false,
            max_output: MAX_OUTPUT,
            ..Settings::default()
        },
    );
    let running = tokio::spawn(runner.run());
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while provider.main.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the summary is retried and the task resumes");
    control.send(Control::Stop).await.unwrap();
    let session = running.await.unwrap().unwrap();
    let summaries = provider.summaries.lock().unwrap();
    assert_eq!(summaries.len(), 2);
    for request in summaries.iter() {
        assert_eq!(request.max_output, MAX_OUTPUT, "no fixed 4096 cap");
        let last = request.input.last().unwrap();
        assert_eq!(last["role"], "user");
        assert!(
            last["content"]
                .as_str()
                .unwrap()
                .starts_with("Write the summary now")
        );
    }
    let retry = serde_json::to_string(&summaries[1].input).unwrap();
    assert!(
        !retry.contains("Retry with a smaller"),
        "a failed summary leaves no tool-retry note behind"
    );
    let context = &session.actors["main"].context.items;
    assert!(
        !serde_json::to_string(context)
            .unwrap()
            .contains("Write the summary now"),
        "the summary turn is request-only"
    );
    assert!(context.len() < history, "the history was compacted");
}

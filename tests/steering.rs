use async_trait::async_trait;
use loh::{
    domain::{EventData, Usage},
    model::{ModelEvent, Provider, Request, Response},
    runtime::{
        runner::{Runner, Settings},
        state::{Control, Session},
    },
    store::Store,
    tools::{ToolHost, files::Workspace},
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct Correction {
    turns: AtomicUsize,
}

#[async_trait]
impl Provider for Correction {
    async fn respond(
        &self,
        request: Request,
        cancel: CancellationToken,
        _: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        if !request.instructions.starts_with("ROLE: steering") {
            cancel.cancelled().await;
            anyhow::bail!("fixture main cancelled");
        }
        let turn = self.turns.fetch_add(1, Ordering::SeqCst);
        if turn > 0 {
            assert!(
                request.input.iter().any(|item| {
                    item["type"] == "function_call_output"
                        && item["output"]
                            .as_str()
                            .is_some_and(|text| text.contains("replacement cannot also update"))
                }),
                "the same classifier must receive an actionable validation error"
            );
        }
        let arguments = json!({
            "intent": if turn == 0 { "replace" } else { "refine" },
            "updates": {"r1":"Keep the CLI and store data in SQLite"},
            "additions":["Run the updated caller verification"],
            "answer":"", "reason":"storage correction",
        });
        Ok(Response {
            output: vec![json!({"type":"function_call","name":"classify_input",
                "call_id":uuid::Uuid::new_v4().to_string(),"arguments":arguments.to_string()})],
            usage: Usage {
                input: 100,
                output: 50,
                cached: 0,
            },
            model: "fixture".into(),
        })
    }
}

#[tokio::test]
async fn contradictory_classification_is_retried_without_losing_the_goal() {
    let workspace = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let session = Session::new(
        workspace.path().into(),
        "Keep the CLI and store data in JSON",
        1_000_000,
        60,
    );
    let store = Arc::new(Store::open(&state.path().join("state.db")).unwrap());
    let tools = Arc::new(ToolHost::new(
        Workspace::open(workspace.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        state.path().join("logs"),
    ));
    let provider = Arc::new(Correction { turns: 0.into() });
    let (runner, control, mut events) = Runner::new(
        session,
        store,
        provider.clone(),
        tools,
        Settings {
            max_output: 1024,
            ..Settings::default()
        },
    );
    let running = tokio::spawn(runner.run());
    control
        .send(Control::Message {
            text:
                "Change storage from JSON to SQLite; keep the CLI and run the updated verification"
                    .into(),
        })
        .await
        .unwrap();
    let revised = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if let EventData::Revised { revision, .. } = events.recv().await.unwrap().data {
                break revision;
            }
        }
    })
    .await;
    control.send(Control::Stop).await.unwrap();
    let result = running.await.unwrap().unwrap();
    assert_eq!(
        revised.unwrap(),
        2,
        "rejected classification cannot advance the revision"
    );
    assert_eq!(provider.turns.load(Ordering::SeqCst), 2);
    assert_eq!(result.goal.history.len(), 1);
    assert!(result.goal.requirements["r1"].active);
    assert!(result.goal.requirements["r1"].text.contains("SQLite"));
}

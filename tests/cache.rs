//! Cache eligibility is an ordered-request property. These tests never claim a
//! provider cache hit: only DeepSeek's reported usage can establish that.
use loh::{
    domain::{Status, Usage},
    model::{ModelEvent, Provider, Request, Response},
    runtime::{
        condition::Condition,
        runner::{Runner, Settings},
        state::Session,
    },
    store::Store,
    tools::{ToolHost, files::Workspace},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[path = "fixtures/state.rs"]
mod state;

#[derive(Default)]
struct Capture {
    turns: Mutex<Vec<(Request, Response)>>,
}

#[async_trait::async_trait]
impl Provider for Capture {
    async fn respond(
        &self,
        request: Request,
        _: CancellationToken,
        _: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        let mut turns = self.turns.lock().unwrap();
        let output = match turns.len() {
            0 => tool(
                "plan",
                "set_plan",
                json!({"steps":[{"id":"inspect","description":"inspect source","requirements":["r1"]}]}),
            ),
            1 => tool("read", "read_file", json!({"path":"source.txt"})),
            2 => {
                json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"Inspected source."}]})
            }
            _ => anyhow::bail!("unexpected additional request"),
        };
        let response = Response {
            // Preserve reasoning alongside the tool history, including on the next request.
            output: vec![
                json!({"type":"reasoning","content":[{"type":"reasoning_text","text":"fixture reasoning"}]}),
                output,
            ],
            usage: Usage {
                input: 100,
                cached: 80,
                output: 10,
            },
            model: "fixture".into(),
        };
        turns.push((request, response.clone()));
        Ok(response)
    }
}

fn tool(id: &str, name: &str, arguments: Value) -> Value {
    json!({"type":"function_call","call_id":id,"name":name,"arguments":arguments.to_string()})
}

#[tokio::test]
async fn plan_and_file_observations_preserve_the_complete_previous_request_prefix() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("source.txt"), "current source").unwrap();
    let mut session = Session::new(root.path().into(), "Inspect source.txt", 1_000_000, 30);
    session.condition = Condition::Prompt;
    let host = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        state.path().join("logs"),
    ));
    let provider = Arc::new(Capture::default());
    let (runner, _, _) = Runner::new(
        session,
        Arc::new(Store::open(&state.path().join("db")).unwrap()),
        provider.clone(),
        host,
        Settings {
            linger: false,
            max_output: 1024,
            ..Settings::default()
        },
    );
    let finished = tokio::time::timeout(std::time::Duration::from_secs(10), runner.run())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(finished.status, Status::Completed);
    let turns = provider.turns.lock().unwrap();
    assert_eq!(turns.len(), 3);
    assert_eq!(
        state::current(&turns[1].0)["plan"]["steps"]["inspect"]["description"],
        "inspect source"
    );
    let current = state::current(&turns[2].0);
    assert!(current["scope"]["observed"]["source.txt"].is_string());
    assert_eq!(current["requirements"]["r1"]["text"], "Inspect source.txt");
    for pair in turns.windows(2) {
        let (before, response) = &pair[0];
        let (after, _) = &pair[1];
        assert_eq!(
            before.instructions, after.instructions,
            "changing state must not rewrite the system prefix"
        );
        assert_eq!(
            before.tools, after.tools,
            "tool definitions must retain deterministic ordering"
        );
        let mut prefix = before.input.clone();
        prefix.extend(response.output.clone());
        assert!(
            after.input.starts_with(&prefix),
            "retain previous inputs AND the complete model output unchanged"
        );
    }
}

use loh::{
    domain::{Status, Usage},
    model::{ModelEvent, Provider, Request, Response},
    runtime::{
        runner::{Runner, Settings},
        state::Session,
    },
    store::Store,
    tools::{ToolHost, files::Workspace},
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
#[path = "fixtures/identity.rs"]
mod identity;

fn call(name: &str, args: Value) -> Response {
    Response {
        output: vec![
            json!({"type":"function_call","call_id":uuid::Uuid::new_v4().to_string(),"name":name,"arguments":args.to_string()}),
        ],
        usage: Usage::default(),
        model: "fixture".into(),
    }
}
struct Stuck {
    diagnoses: AtomicUsize,
}
#[async_trait::async_trait]
impl Provider for Stuck {
    async fn respond(
        &self,
        request: Request,
        _cancel: CancellationToken,
        _events: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        if request.instructions.starts_with("ROLE: diagnosis") {
            self.diagnoses.fetch_add(1, Ordering::SeqCst);
            return Ok(call(
                "report_diagnosis",
                json!({"cause":"The file does not exist; rereading cannot create it.","strategy":"Create result.txt using a guarded create patch, then inspect it."}),
            ));
        }
        let results: Vec<Value> = request
            .input
            .iter()
            .filter_map(|v| v["output"].as_str())
            .filter_map(|s| serde_json::from_str(s).ok())
            .collect();
        if request.instructions.starts_with("ROLE: verifier") {
            if let Some(id) = results.iter().find_map(|v| v["evidence"].as_str()) {
                return Ok(call(
                    "report_verification",
                    json!({"summary":"file checked","checks":[{"requirement":"r1","passed":true,"evidence":[id],"reason":"file delivered"}]}),
                ));
            }
            return Ok(call("read_file", json!({"path":"result.txt"})));
        }
        if results
            .iter()
            .any(|v| v["result"]["data"]["changes"].is_array())
        {
            return Ok(Response {
                output: vec![
                    json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"ready"}]}),
                ],
                usage: Usage::default(),
                model: "fixture".into(),
            });
        }
        if request.input.iter().any(|v| {
            v["content"]
                .as_str()
                .is_some_and(|s| s.contains("guarded create patch"))
        }) {
            return Ok(call(
                "apply_patch",
                json!({"edits":[{"path":"result.txt","expected":"missing","operation":"create","content":"done"}]}),
            ));
        }
        Ok(call("read_file", json!({"path":"result.txt"})))
    }
}

#[tokio::test]
async fn repeated_no_information_failures_trigger_fresh_diagnosis_and_continue() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let session = Session::new(root.path().into(), "Write result.txt", 1_000_000, 60);
    let host = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        state.path().join("logs"),
    ));
    let store = Arc::new(Store::open(&state.path().join("db")).unwrap());
    let provider = Arc::new(Stuck {
        diagnoses: 0.into(),
    });
    let (runner, _, _) = Runner::new(
        session,
        store.clone(),
        provider.clone(),
        host,
        Settings {
            max_output: 1024,
            linger: false,
            ..Settings::default()
        },
    );
    let session = tokio::time::timeout(Duration::from_secs(2), runner.run())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(session.status, Status::Completed);
    identity::assert_user_turns(&session, &store);
    assert_eq!(provider.diagnoses.load(Ordering::SeqCst), 1);
}

#[test]
fn progress_uses_observed_information_not_transient_identifiers() {
    use loh::runtime::progress::Progress;
    let mut progress = Progress::default();
    assert!(!progress.observe(&json!({"error":"same"})));
    assert!(!progress.observe(&json!({"error":"same"})));
    assert!(!progress.observe(&json!({"error":"same"})));
    assert!(progress.observe(&json!({"error":"same"})));
    assert!(!progress.observe(&json!({"files":{"a":"new hash"}})));
}

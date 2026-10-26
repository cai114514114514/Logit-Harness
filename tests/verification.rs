use async_trait::async_trait;
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
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct VerdictRepair {
    main_calls: AtomicUsize,
    verifier_calls: AtomicUsize,
}

fn response(item: Value) -> Response {
    Response {
        output: vec![item],
        usage: Usage {
            input: 100,
            output: 50,
            cached: 0,
        },
        model: "fixture".into(),
    }
}

fn call(name: &str, arguments: Value) -> Response {
    response(json!({"type":"function_call","name":name,
        "call_id":uuid::Uuid::new_v4().to_string(),"arguments":arguments.to_string()}))
}

#[async_trait]
impl Provider for VerdictRepair {
    async fn respond(
        &self,
        request: Request,
        _: CancellationToken,
        _: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        if !request.instructions.starts_with("ROLE: verifier") {
            self.main_calls.fetch_add(1, Ordering::SeqCst);
            return Ok(response(
                json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"The existing result is ready for independent verification."}]}),
            ));
        }
        let turn = self.verifier_calls.fetch_add(1, Ordering::SeqCst);
        match turn {
            // A successful required check and a failing optional experiment.
            0 => Ok(call("check", json!({"command":"test -f result.txt"}))),
            1 => Ok(call("check", json!({"command":"exit 1"}))),
            _ => {
                let results: Vec<Value> = request
                    .input
                    .iter()
                    .filter(|item| item["type"] == "function_call_output")
                    .filter_map(|item| serde_json::from_str(item["output"].as_str()?).ok())
                    .collect();
                let ids: Vec<Value> = results
                    .iter()
                    .filter(|item| item["evidence"].is_string())
                    .filter(|item| turn == 2 || item["result"]["success"] == true)
                    .map(|item| item["evidence"].clone())
                    .collect();
                Ok(call(
                    "report_verification",
                    json!({"summary":"required file verified",
                    "checks":[{"requirement":"r1","passed":true,"evidence":ids,"reason":"the required file exists; optional experiment is outside the requirement"}]}),
                ))
            }
        }
    }
}

#[tokio::test]
async fn failed_support_in_a_passing_verdict_returns_to_the_verifier() {
    let workspace = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("result.txt"), "keep this deliverable").unwrap();
    let session = Session::new(
        workspace.path().into(),
        "Keep result.txt present",
        1_000_000,
        60,
    );
    let store = Arc::new(Store::open(&state.path().join("state.db")).unwrap());
    let tools = Arc::new(ToolHost::new(
        Workspace::open(workspace.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        state.path().join("logs"),
    ));
    let provider = Arc::new(VerdictRepair {
        main_calls: 0.into(),
        verifier_calls: 0.into(),
    });
    let (runner, _, _) = Runner::new(
        session,
        store,
        provider.clone(),
        tools,
        Settings {
            linger: false,
            max_output: 1024,
            ..Settings::default()
        },
    );
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), runner.run())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.status, Status::Completed);
    assert_eq!(
        provider.main_calls.load(Ordering::SeqCst),
        1,
        "a contradictory evidence citation must be repaired before asking the implementer to redo work"
    );
    assert_eq!(provider.verifier_calls.load(Ordering::SeqCst), 4);
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("result.txt")).unwrap(),
        "keep this deliverable"
    );
    // Only the accepted verdict is recorded; the contradictory one was a
    // protocol error and never reached the ledger.
    let acceptance = result.acceptance.clone().unwrap();
    assert!(acceptance.passed);
    assert_eq!(acceptance.checks.len(), 1);
    assert_eq!(acceptance.checks[0].requirement, "r1");
    assert_eq!(acceptance.checks[0].evidence[0].tool, "check");
    assert_eq!(
        acceptance.checks[0].evidence[0].subject,
        "test -f result.txt"
    );
    let contract = loh::runtime::contract::derive(&result).unwrap();
    assert_eq!(
        contract.clauses[0].stamp,
        loh::runtime::contract::Stamp::Accepted
    );
}

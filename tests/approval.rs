//! The approval returned by a tool can depend on live external state.
use loh::{
    domain::{EventData, Status, Usage},
    model::{ModelEvent, Provider, Request, Response},
    runtime::{
        runner::{Runner, Settings},
        state::{Control, Session},
    },
    store::Store,
    tools::{
        Effect, Tool, ToolContext, ToolError, ToolEvent, ToolHost, ToolResult, ToolSpec,
        files::Workspace,
    },
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

struct Bound(AtomicUsize);
#[async_trait::async_trait]
impl Tool for Bound {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "bound_action".into(),
            description: "Local approval fixture".into(),
            parameters: json!({"type":"object"}),
            effect: Effect::Write,
        }
    }
    async fn call(
        &self,
        _: Value,
        context: ToolContext,
        _: CancellationToken,
        _: mpsc::UnboundedSender<ToolEvent>,
    ) -> anyhow::Result<ToolResult> {
        let id = if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
            "origin-one"
        } else {
            "origin-two"
        };
        if !context.scope.approvals.contains(id) {
            return Err(ToolError::Approval {
                id: id.into(),
                reason: "current origin requires authorization".into(),
            }
            .into());
        }
        std::fs::write(context.workspace.root.join("result.txt"), "done")?;
        Ok(ToolResult {
            data: json!({"done":true}),
            success: true,
            files: context.workspace.fingerprints()?,
            kind: loh::domain::EvidenceKind::Command,
            images: vec![],
        })
    }
}
struct Script;
fn call(name: &str, args: Value) -> Response {
    Response {
        output: vec![
            json!({"type":"function_call","call_id":uuid::Uuid::new_v4().to_string(),"name":name,"arguments":args.to_string()}),
        ],
        usage: Usage::default(),
        model: "fixture".into(),
    }
}
#[async_trait::async_trait]
impl Provider for Script {
    async fn respond(
        &self,
        request: Request,
        _: CancellationToken,
        _: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        let observed = request
            .input
            .iter()
            .filter(|v| v["type"] == "function_call_output")
            .filter_map(|v| v["output"].as_str())
            .filter_map(|s| serde_json::from_str::<Value>(s).ok())
            .find_map(|v| v["evidence"].as_str().map(str::to_owned));
        Ok(if request.instructions.starts_with("ROLE: verifier") {
            if let Some(evidence) = observed {
                call(
                    "report_verification",
                    json!({"summary":"inspected","checks":[{"requirement":"r1","passed":true,"evidence":[evidence],"reason":"file exists"}]}),
                )
            } else {
                call("read_file", json!({"path":"result.txt"}))
            }
        } else if observed.is_none() {
            call("bound_action", json!({}))
        } else {
            Response {
                output: vec![
                    json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"ready"}]}),
                ],
                usage: Usage::default(),
                model: "fixture".into(),
            }
        })
    }
}

#[tokio::test]
async fn authorization_requeues_the_exact_pending_action_and_rechecks_live_scope() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let session = Session::new(root.path().into(), "Write result.txt", 1_000_000, 60);
    let store = Arc::new(Store::open(&state.path().join("db")).unwrap());
    let mut tools = ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        state.path().join("logs"),
    );
    tools
        .register(Arc::new(Bound(AtomicUsize::new(0))))
        .unwrap();
    let (runner, control, mut events) = Runner::new(
        session,
        store,
        Arc::new(Script),
        Arc::new(tools),
        Settings::default(),
    );
    let running = tokio::spawn(runner.run());
    let outcome = tokio::time::timeout(Duration::from_secs(3), async {
        let mut approvals = vec![];
        loop {
            match events.recv().await.unwrap().data {
                EventData::Approval { id, .. } => {
                    assert!(!root.path().join("result.txt").exists());
                    approvals.push(id.clone());
                    control.send(Control::Approve { id }).await.unwrap();
                }
                EventData::Status {
                    status: Status::Completed,
                    ..
                } => break approvals,
                _ => (),
            }
        }
    })
    .await;
    control.send(Control::Stop).await.unwrap();
    running.await.unwrap().unwrap();
    assert_eq!(outcome.unwrap(), vec!["origin-one", "origin-two"]);
}

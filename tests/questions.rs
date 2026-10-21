use loh::{
    domain::{Status, Usage},
    model::{ModelEvent, Provider, Request, Response},
    runtime::{
        runner::{Runner, Settings},
        state::{Control, Session},
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

fn call(name: &str, args: Value) -> Response {
    Response {
        output: vec![
            json!({"type":"function_call","call_id":uuid::Uuid::new_v4().to_string(),"name":name,"arguments":args.to_string()}),
        ],
        usage: Usage::default(),
        model: "fixture".into(),
    }
}
fn done() -> Response {
    Response {
        output: vec![
            json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"ready"}]}),
        ],
        usage: Usage::default(),
        model: "fixture".into(),
    }
}
fn patch(path: &str) -> Value {
    json!({"edits":[{"path":path,"expected":"missing","operation":"create","content":"done"}]})
}
struct Script {
    main: AtomicUsize,
    answered: AtomicUsize,
}
#[async_trait::async_trait]
impl Provider for Script {
    async fn respond(
        &self,
        request: Request,
        _cancel: CancellationToken,
        _events: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        let outputs: Vec<Value> = request
            .input
            .iter()
            .filter_map(|v| v["output"].as_str())
            .filter_map(|s| serde_json::from_str(s).ok())
            .collect();
        if request.instructions.starts_with("ROLE: main") {
            if request.instructions.contains("SQLite") {
                return Ok(if self.answered.fetch_add(1, Ordering::SeqCst) == 0 {
                    call("apply_patch", patch("storage.txt"))
                } else {
                    done()
                });
            }
            return Ok(match self.main.fetch_add(1, Ordering::SeqCst) {
                0 => call(
                    "extract_requirements",
                    json!({"quotes":["Choose storage.","Build interface."]}),
                ),
                1 => call(
                    "set_plan",
                    json!({"steps":[{"id":"store","description":"storage","requirements":["r2"]},{"id":"ui","description":"interface","requirements":["r3"]},{"id":"all","description":"whole task","requirements":["r1"],"dependencies":["store","ui"]}]}),
                ),
                2 => call(
                    "ask_user",
                    json!({"question":"Which storage backend?","requirements":["r2"]}),
                ),
                3 => call("delegate", json!({"step":"ui","task":"Build interface"})),
                _ => call("wait_input", json!({})),
            });
        }
        if request.instructions.starts_with("ROLE: worker") {
            return Ok(if outputs.is_empty() {
                call("propose_patch", patch("ui.txt"))
            } else {
                done()
            });
        }
        if request.instructions.starts_with("ROLE: verifier") {
            let evidence = outputs.iter().find_map(|v| v["evidence"].as_str());
            return Ok(if let Some(id) = evidence {
                call(
                    "report_verification",
                    json!({"summary":"both delivered","checks":(["r1","r2","r3"].map(|r| json!({"requirement":r,"passed":true,"evidence":[id],"reason":"checked files"})))}),
                )
            } else {
                call(
                    "check",
                    json!({"command":"test -f storage.txt && test -f ui.txt"}),
                )
            });
        }
        anyhow::bail!("unexpected role")
    }
}

#[tokio::test]
async fn missing_storage_decision_does_not_block_independent_interface_work() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let session = Session::new(
        root.path().into(),
        "Choose storage. Build interface.",
        1_000_000,
        60,
    );
    let id = session.id.clone();
    let host = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        state.path().join("logs"),
    ));
    let store = Arc::new(Store::open(&state.path().join("db")).unwrap());
    let (runner, control, _) = Runner::new(
        session,
        store.clone(),
        Arc::new(Script {
            main: 0.into(),
            answered: 0.into(),
        }),
        host,
        Settings {
            max_output: 1024,
            ..Settings::default()
        },
    );
    let running = tokio::spawn(runner.run());
    let waiting = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(session) = store.load::<Session>(&id) {
                if session.status == Status::Waiting && root.path().join("ui.txt").exists() {
                    break session;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    if waiting.is_err() {
        control.send(Control::Stop).await.unwrap();
        running.await.unwrap().unwrap();
        panic!("independent interface work did not finish while storage waited");
    }
    let waiting = waiting.unwrap();
    assert!(!root.path().join("storage.txt").exists());
    let question = waiting.questions.keys().next().unwrap().clone();
    control
        .send(Control::Answer {
            id: question,
            text: "SQLite".into(),
        })
        .await
        .unwrap();
    let completed = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if store.load::<Session>(&id).unwrap().status == Status::Completed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    control.send(Control::Stop).await.unwrap();
    running.await.unwrap().unwrap();
    assert!(completed.is_ok());
    assert!(root.path().join("storage.txt").exists());
}

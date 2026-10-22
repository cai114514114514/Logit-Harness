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
use tokio::{
    sync::{Barrier, mpsc},
    time::timeout,
};
use tokio_util::sync::CancellationToken;

#[path = "fixtures/state.rs"]
mod state;

fn response(name: &str, args: Value) -> Response {
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
            json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"scoped work is ready"}]}),
        ],
        usage: Usage::default(),
        model: "fixture".into(),
    }
}

struct Parallel {
    main: AtomicUsize,
    rendezvous: Barrier,
}
#[async_trait::async_trait]
impl Provider for Parallel {
    async fn respond(
        &self,
        request: Request,
        _cancel: CancellationToken,
        _events: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        if request.instructions.starts_with("ROLE: main") {
            return Ok(match self.main.fetch_add(1, Ordering::SeqCst) {
                0 => response(
                    "extract_requirements",
                    json!({"quotes":["Build storage.","Build interface."]}),
                ),
                1 => response(
                    "set_plan",
                    json!({"steps":[
                        {"id":"storage","description":"Build storage","requirements":["r2"]},
                        {"id":"ui","description":"Build interface","requirements":["r3"]},
                        {"id":"complete","description":"Check whole deliverable","requirements":["r1"],"dependencies":["storage","ui"]}
                    ]}),
                ),
                2 => {
                    let mut first =
                        response("delegate", json!({"step":"storage","task":"Build storage"}));
                    first.output.extend(
                        response("delegate", json!({"step":"ui","task":"Build interface"})).output,
                    );
                    first
                }
                _ => done(),
            });
        }
        if request.instructions.starts_with("ROLE: worker") {
            if request
                .input
                .iter()
                .any(|v| v["type"] == "function_call_output")
            {
                return Ok(done());
            }
            // This barrier can only open if both real worker contexts are scheduled concurrently.
            self.rendezvous.wait().await;
            let storage = state::current(&request)["task"] == "Build storage";
            return Ok(response(
                "propose_patch",
                json!({"edits":[{"path":if storage {"storage.txt"} else {"ui.txt"},"expected":"missing","operation":"create","content":"done"}]}),
            ));
        }
        if request.instructions.starts_with("ROLE: verifier") {
            let evidence = request
                .input
                .iter()
                .filter_map(|v| v["output"].as_str())
                .filter_map(|s| serde_json::from_str::<Value>(s).ok())
                .find_map(|v| v["evidence"].as_str().map(str::to_owned));
            return Ok(if let Some(evidence) = evidence {
                response(
                    "report_verification",
                    json!({"summary":"checked both workers","checks":(["r1","r2","r3"].map(|r| json!({"requirement":r,"passed":true,"evidence":[evidence],"reason":"both files exist"})))}),
                )
            } else {
                response(
                    "check",
                    json!({"command":"test -f storage.txt && test -f ui.txt"}),
                )
            });
        }
        anyhow::bail!("unexpected role")
    }
}

#[tokio::test]
async fn workers_run_concurrently_and_commit_proposals_through_the_owner() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let session = Session::new(
        root.path().into(),
        "Build storage. Build interface.",
        1_000_000,
        60,
    );
    let host = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        state.path().join("logs"),
    ));
    let store = Arc::new(Store::open(&state.path().join("db")).unwrap());
    let (runner, _, _) = Runner::new(
        session,
        store,
        Arc::new(Parallel {
            main: 0.into(),
            rendezvous: Barrier::new(2),
        }),
        host,
        Settings {
            max_output: 1024,
            linger: false,
            ..Settings::default()
        },
    );
    let session = timeout(Duration::from_secs(1), runner.run())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(session.status, Status::Completed);
    assert_eq!(
        std::fs::read_to_string(root.path().join("storage.txt")).unwrap(),
        "done"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("ui.txt")).unwrap(),
        "done"
    );
    let value = serde_json::to_value(session).unwrap();
    assert_eq!(
        value["actors"]
            .as_object()
            .unwrap()
            .values()
            .filter(|a| a["role"] == "worker")
            .count(),
        2
    );
}
struct Steered {
    inner: Parallel,
    arrivals: Arc<tokio::sync::Semaphore>,
    release: Arc<tokio::sync::Semaphore>,
    replacement: std::sync::atomic::AtomicBool,
    ui_requests: AtomicUsize,
    hold_main: bool,
}

#[async_trait::async_trait]
impl Provider for Steered {
    async fn respond(
        &self,
        request: Request,
        cancel: CancellationToken,
        events: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        if request.instructions.starts_with("ROLE: steering") {
            return Ok(response(
                "classify_input",
                json!({"intent":"refine","updates":{"r2":"Build SQLite storage."},"additions":[],"answer":"","reason":"storage correction only"}),
            ));
        }
        if self.hold_main
            && request.instructions.starts_with("ROLE: main")
            && state::current(&request)["requirements"]["r2"]["text"] != "Build SQLite storage."
            && self.inner.main.load(Ordering::SeqCst) >= 3
        {
            self.arrivals.add_permits(1);
            self.release.acquire().await.unwrap().forget();
            return Ok(done());
        }
        if request.instructions.starts_with("ROLE: main")
            && state::current(&request)["requirements"]["r2"]["text"] == "Build SQLite storage."
        {
            return Ok(if !self.replacement.swap(true, Ordering::SeqCst) {
                response(
                    "delegate",
                    json!({"step":"storage","task":"Build SQLite storage"}),
                )
            } else {
                done()
            });
        }
        if request.instructions.starts_with("ROLE: worker") {
            let task = request.input[0]["content"].as_str().unwrap();
            let ui = task.contains("interface");
            if ui {
                self.ui_requests.fetch_add(1, Ordering::SeqCst);
            }
            if request
                .input
                .iter()
                .any(|v| v["type"] == "function_call_output")
            {
                return Ok(done());
            }
            if !task.contains("SQLite") {
                self.arrivals.add_permits(1);
                // Deliberately ignore cancellation: a late response still must not execute tools.
                self.release.acquire().await.unwrap().forget();
            }
            let path = if ui {
                "ui.txt"
            } else if task.contains("SQLite") {
                "storage.txt"
            } else {
                "obsolete.txt"
            };
            return Ok(response(
                "propose_patch",
                json!({"edits":[{"path":path,"expected":"missing","operation":"create","content":"done"}]}),
            ));
        }
        self.inner.respond(request, cancel, events).await
    }
}

#[tokio::test]
async fn correction_discards_late_worker_but_keeps_unaffected_context() {
    steering_case(false).await;
}

#[tokio::test]
async fn steering_has_a_reserved_lane_when_all_work_requests_are_busy() {
    steering_case(true).await;
}

async fn steering_case(hold_main: bool) {
    use loh::runtime::state::Control;
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let session = Session::new(
        root.path().into(),
        "Build storage. Build interface.",
        1_000_000,
        60,
    );
    let host = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        state.path().join("logs"),
    ));
    let store = Arc::new(Store::open(&state.path().join("db")).unwrap());
    let provider = Arc::new(Steered {
        inner: Parallel {
            main: 0.into(),
            rendezvous: Barrier::new(2),
        },
        arrivals: Arc::new(tokio::sync::Semaphore::new(0)),
        release: Arc::new(tokio::sync::Semaphore::new(0)),
        replacement: false.into(),
        ui_requests: 0.into(),
        hold_main,
    });
    let (runner, control, mut events) = Runner::new(
        session,
        store,
        provider.clone(),
        host,
        Settings {
            max_output: 1024,
            linger: false,
            ..Settings::default()
        },
    );
    let running = tokio::spawn(runner.run());
    let count = if hold_main { 3 } else { 2 };
    timeout(
        Duration::from_secs(2),
        provider.arrivals.acquire_many(count),
    )
    .await
    .unwrap()
    .unwrap()
    .forget();
    control
        .send(Control::Message {
            text: "Change storage to SQLite; keep interface work".into(),
        })
        .await
        .unwrap();
    timeout(Duration::from_secs(2), async {
        loop {
            if matches!(
                events.recv().await.unwrap().data,
                loh::domain::EventData::Revised { .. }
            ) {
                break;
            }
        }
    })
    .await
    .unwrap();
    provider.release.add_permits(count as usize);
    let session = timeout(Duration::from_secs(2), running)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(session.status, Status::Completed);
    assert_eq!(
        provider.ui_requests.load(Ordering::SeqCst),
        2,
        "unaffected worker should keep its original context"
    );
    assert!(!root.path().join("obsolete.txt").exists());
    assert!(root.path().join("storage.txt").exists());
    assert!(root.path().join("ui.txt").exists());
}

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
use weft::{
    domain::{EventData, Goal, Status, Usage},
    model::{ModelEvent, Provider, Request, Response},
    runtime::{
        runner::{Runner, Settings},
        state::{Control, Session},
    },
    store::Store,
    tools::{ToolHost, files::Workspace},
};

struct Script {
    main: AtomicUsize,
    premature: bool,
    long_command: bool,
}
fn answer(output: Value) -> Response {
    Response {
        output: output.as_array().unwrap().clone(),
        usage: Usage {
            input: 100,
            output: 50,
            cached: 0,
        },
        model: "fixture".into(),
    }
}
fn call(name: &str, args: Value) -> Response {
    answer(
        json!([{"type":"function_call","name":name,"call_id":uuid::Uuid::new_v4().to_string(),"arguments":args.to_string()}]),
    )
}
fn text(message: &str) -> Response {
    answer(
        json!([{"type":"message","role":"assistant","content":[{"type":"output_text","text":message}]}]),
    )
}

#[async_trait::async_trait]
impl Provider for Script {
    async fn respond(
        &self,
        request: Request,
        _cancel: CancellationToken,
        _events: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        if request.instructions.starts_with("ROLE: steering") {
            return Ok(call(
                "classify_input",
                json!({"intent":"refine","updates":{"r1":"Write SQLite result.txt"},"additions":[],"answer":"","reason":"storage correction"}),
            ));
        }
        if request.instructions.starts_with("ROLE: verifier") {
            let evidence = request
                .input
                .iter()
                .rev()
                .filter(|v| v["type"] == "function_call_output")
                .filter_map(|v| v["output"].as_str())
                .filter_map(|s| serde_json::from_str::<Value>(s).ok())
                .find_map(|v| v["evidence"].as_str().map(str::to_owned));
            if let Some(evidence) = evidence {
                return Ok(call(
                    "report_verification",
                    json!({"summary":"inspected result","checks":[{"requirement":"r1","passed":true,"evidence":[evidence],"reason":"result exists"}]}),
                ));
            }
            if self.premature {
                return Ok(call(
                    "report_verification",
                    json!({"summary":"unsupported success","checks":[{"requirement":"r1","passed":true,"evidence":[],"reason":"model said done"}]}),
                ));
            }
            return Ok(call("read_file", json!({"path":"result.txt"})));
        }
        let turn = self.main.fetch_add(1, Ordering::SeqCst);
        if self.premature {
            return Ok(if turn == 0 {
                text("done")
            } else {
                call(
                    "ask_user",
                    json!({"question":"What content should result.txt contain?","requirements":["r1"]}),
                )
            });
        }
        if self.long_command && turn == 0 {
            return Ok(call(
                "shell",
                json!({"command":"printf ready; sleep 30; touch obsolete","timeout_seconds":40}),
            ));
        }
        if turn == 0 || (self.long_command && turn == 1) {
            return Ok(call(
                "apply_patch",
                json!({"edits":[{"path":"result.txt","expected":"missing","operation":"create","content":"SQLite"}]}),
            ));
        }
        Ok(text("The file is ready."))
    }
}

fn setup(
    workspace: &std::path::Path,
    state: &std::path::Path,
    script: Script,
) -> (
    Runner,
    mpsc::Sender<Control>,
    tokio::sync::broadcast::Receiver<weft::domain::SessionEvent>,
) {
    let session = Session::new(workspace.to_path_buf(), "Write result.txt", 1_000_000, 60);
    let store = Arc::new(Store::open(&state.join("state.db")).unwrap());
    let goal = Arc::new(tokio::sync::RwLock::new(Goal::new("Write result.txt")));
    let tools = Arc::new(ToolHost::new(
        Workspace::open(workspace).unwrap(),
        goal,
        state.join("logs"),
    ));
    Runner::new(
        session,
        store,
        Arc::new(script),
        tools,
        Settings {
            linger: false,
            max_output: 1024,
            ..Settings::default()
        },
    )
}

#[tokio::test]
async fn independent_verification_requires_observed_evidence() {
    let workspace = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (runner, _control, _events) = setup(
        workspace.path(),
        state.path(),
        Script {
            main: AtomicUsize::new(0),
            premature: false,
            long_command: false,
        },
    );
    let session = tokio::time::timeout(Duration::from_secs(3), runner.run())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(session.status, Status::Completed);
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("result.txt")).unwrap(),
        "SQLite"
    );
    assert!(!session.evidence.is_empty());
}

#[tokio::test]
async fn an_unsupported_done_response_does_not_complete_the_task() {
    let workspace = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (runner, _control, _events) = setup(
        workspace.path(),
        state.path(),
        Script {
            main: AtomicUsize::new(0),
            premature: true,
            long_command: false,
        },
    );
    let session = tokio::time::timeout(Duration::from_secs(3), runner.run())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(session.status, Status::Waiting);
    assert!(session.final_report.is_none());
}

#[tokio::test]
async fn correction_cancels_old_command_before_replacement_write() {
    let workspace = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let (runner, control, mut events) = setup(
        workspace.path(),
        state.path(),
        Script {
            main: AtomicUsize::new(0),
            premature: false,
            long_command: true,
        },
    );
    let running = tokio::spawn(runner.run());
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(event) = events.recv().await {
                if matches!(event.data,EventData::ActionStarted{ref tool,..}if tool=="shell") {
                    break;
                }
            }
        }
    })
    .await
    .unwrap();
    control
        .send(Control::Message {
            text: "Use SQLite instead".into(),
        })
        .await
        .unwrap();
    let session = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(session.status, Status::Completed);
    assert_eq!(session.goal.revision, 2);
    assert!(!workspace.path().join("obsolete").exists());
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("result.txt")).unwrap(),
        "SQLite"
    );
}

struct QuestionScript(Script);
#[async_trait::async_trait]
impl Provider for QuestionScript {
    async fn respond(
        &self,
        request: Request,
        cancel: CancellationToken,
        events: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        if request.instructions.starts_with("ROLE: steering") {
            Ok(call(
                "classify_input",
                json!({"intent":"question","updates":{},"additions":[],"answer":"The file is complete.","reason":"side question"}),
            ))
        } else {
            self.0.respond(request, cancel, events).await
        }
    }
}

#[tokio::test]
async fn a_question_after_completion_restores_the_completed_status() {
    let workspace = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let session = Session::new(
        workspace.path().to_owned(),
        "Write result.txt",
        1_000_000,
        60,
    );
    let store = Arc::new(Store::open(&state.path().join("state.db")).unwrap());
    let tools = Arc::new(ToolHost::new(
        Workspace::open(workspace.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        state.path().join("logs"),
    ));
    let provider = Arc::new(QuestionScript(Script {
        main: AtomicUsize::new(0),
        premature: false,
        long_command: false,
    }));
    let (runner, control, mut events) = Runner::new(
        session,
        store,
        provider,
        tools,
        Settings {
            max_output: 1024,
            ..Settings::default()
        },
    );
    let running = tokio::spawn(runner.run());
    async fn complete(events: &mut tokio::sync::broadcast::Receiver<weft::domain::SessionEvent>) {
        loop {
            let event = events.recv().await.unwrap();
            if matches!(
                event.data,
                EventData::Status {
                    status: Status::Completed,
                    ..
                }
            ) {
                break;
            }
        }
    }
    tokio::time::timeout(Duration::from_secs(3), complete(&mut events))
        .await
        .unwrap();
    control
        .send(Control::Message {
            text: "What is the status?".into(),
        })
        .await
        .unwrap();
    let restored = tokio::time::timeout(Duration::from_secs(3), complete(&mut events)).await;
    control.send(Control::Stop).await.unwrap();
    let finished = running.await.unwrap().unwrap();
    assert!(
        restored.is_ok(),
        "answering a question must not leave an already completed goal stuck in Running"
    );
    assert_eq!(finished.goal.revision, 1);
}

struct InvalidPlan {
    first: std::sync::atomic::AtomicBool,
    inner: Script,
}
#[async_trait::async_trait]
impl Provider for InvalidPlan {
    async fn respond(
        &self,
        request: Request,
        cancel: CancellationToken,
        events: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        if request.instructions.starts_with("ROLE: main")
            && !self.first.swap(true, Ordering::SeqCst)
        {
            Ok(call(
                "set_plan",
                json!({"steps":"invalid provider arguments"}),
            ))
        } else {
            self.inner.respond(request, cancel, events).await
        }
    }
}

#[tokio::test]
async fn malformed_planning_arguments_are_feedback_not_a_runner_crash() {
    let workspace = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let session = Session::new(
        workspace.path().to_owned(),
        "Write result.txt",
        1_000_000,
        60,
    );
    let store = Arc::new(Store::open(&state.path().join("state.db")).unwrap());
    let tools = Arc::new(ToolHost::new(
        Workspace::open(workspace.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        state.path().join("logs"),
    ));
    let provider = Arc::new(InvalidPlan {
        first: false.into(),
        inner: Script {
            main: AtomicUsize::new(0),
            premature: false,
            long_command: false,
        },
    });
    let (runner, _control, _events) = Runner::new(
        session,
        store,
        provider,
        tools,
        Settings {
            linger: false,
            max_output: 1024,
            ..Settings::default()
        },
    );
    let session = tokio::time::timeout(Duration::from_secs(3), runner.run())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(session.status, Status::Completed);
}

#[tokio::test]
async fn campaign_refusal_is_exhaustion_without_unknown_token_charges() {
    use weft::runtime::{
        meter::Metered,
        money::{Money, Prices},
    };
    let workspace = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let session = Session::new(
        workspace.path().to_owned(),
        "Write result.txt",
        1_000_000,
        60,
    );
    let store = Arc::new(Store::open(&state.path().join("state.db")).unwrap());
    let tools = Arc::new(ToolHost::new(
        Workspace::open(workspace.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        state.path().join("logs"),
    ));
    let prices = Prices {
        input: 2000,
        cached: 40,
        output: 8000,
        search: 5_000_000,
        fetch: 5_000_000,
    };
    let money = Arc::new(Money::open(&state.path().join("money.db"), "tiny", 1, &prices).unwrap());
    let provider = Arc::new(Metered::new(
        Arc::new(Script {
            main: AtomicUsize::new(0),
            premature: false,
            long_command: false,
        }),
        money,
        prices,
    ));
    let (runner, _control, _events) = Runner::new(
        session,
        store,
        provider,
        tools,
        Settings {
            linger: false,
            max_output: 1024,
            ..Settings::default()
        },
    );
    let session = tokio::time::timeout(Duration::from_secs(3), runner.run())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(session.status, Status::Exhausted);
    assert_eq!(session.budget.uncertain, 0);
    assert!(!workspace.path().join("result.txt").exists());
}
struct SlowCancellation;
#[async_trait::async_trait]
impl Provider for SlowCancellation {
    async fn respond(
        &self,
        _request: Request,
        cancel: CancellationToken,
        _events: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        cancel.cancelled().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        anyhow::bail!("cancelled")
    }
}

#[tokio::test]
async fn resume_during_cancellation_is_saved_and_runs_after_settlement() {
    let workspace = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let session = Session::new(workspace.path().to_owned(), "work", 1_000_000, 60);
    let id = session.id.clone();
    let store = Arc::new(Store::open(&state.path().join("state.db")).unwrap());
    let tools = Arc::new(ToolHost::new(
        Workspace::open(workspace.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        state.path().join("logs"),
    ));
    let (runner, control, mut events) = Runner::new(
        session,
        store.clone(),
        Arc::new(SlowCancellation),
        tools,
        Settings::default(),
    );
    let running = tokio::spawn(runner.run());
    while !matches!(
        events.recv().await.unwrap().data,
        EventData::ModelRequested { .. }
    ) {}
    control.send(Control::Pause).await.unwrap();
    control.send(Control::Resume).await.unwrap();
    let restarted = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if matches!(
                events.recv().await.unwrap().data,
                EventData::ModelRequested { .. }
            ) {
                break;
            }
        }
    })
    .await;
    let snapshot: Session = store.load(&id).unwrap();
    control.send(Control::Stop).await.unwrap();
    running.await.unwrap().unwrap();
    assert!(
        restarted.is_ok(),
        "resume must not be lost while the earlier model request cancels; state={:?}",
        snapshot.status
    );
}
struct ReportFormat {
    inner: Script,
    malformed: std::sync::atomic::AtomicBool,
}
#[async_trait::async_trait]
impl Provider for ReportFormat {
    async fn respond(
        &self,
        request: Request,
        cancel: CancellationToken,
        events: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        let verifier = request.instructions.starts_with("ROLE: verifier");
        let mut response = self.inner.respond(request, cancel, events).await?;
        if verifier
            && response.output[0]["name"] == "report_verification"
            && !self.malformed.swap(true, Ordering::SeqCst)
        {
            let mut arguments: Value =
                serde_json::from_str(response.output[0]["arguments"].as_str().unwrap()).unwrap();
            arguments["checks"][0]["requirement"] = json!("Write result.txt");
            response.output[0]["arguments"] = json!(arguments.to_string());
        }
        Ok(response)
    }
}

#[tokio::test]
async fn malformed_verdict_is_repaired_by_verifier_without_redoing_implementation() {
    let workspace = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let session = Session::new(
        workspace.path().to_owned(),
        "Write result.txt",
        1_000_000,
        60,
    );
    let store = Arc::new(Store::open(&state.path().join("state.db")).unwrap());
    let tools = Arc::new(ToolHost::new(
        Workspace::open(workspace.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        state.path().join("logs"),
    ));
    let provider = Arc::new(ReportFormat {
        inner: Script {
            main: AtomicUsize::new(0),
            premature: false,
            long_command: false,
        },
        malformed: false.into(),
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
    let session = tokio::time::timeout(Duration::from_secs(3), runner.run())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(session.status, Status::Completed);
    assert_eq!(
        provider.inner.main.load(Ordering::SeqCst),
        2,
        "malformed verdicts are not evidence that the implementation needs changing"
    );
}
#[tokio::test]
async fn resuming_completed_work_preserves_its_completion() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut session = Session::new(root.path().into(), "done", 10000, 60);
    session.status = Status::Completed;
    let id = session.id.clone();
    let store = Arc::new(Store::open(&state.path().join("db")).unwrap());
    let tools = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        state.path().join("logs"),
    ));
    let (runner, control, mut events) = Runner::new(
        session,
        store.clone(),
        Arc::new(SlowCancellation),
        tools,
        Settings::default(),
    );
    let running = tokio::spawn(runner.run());
    control.send(Control::Resume).await.unwrap();
    events.recv().await.unwrap();
    let snapshot: Session = store.load(&id).unwrap();
    control.send(Control::Stop).await.unwrap();
    running.await.unwrap().unwrap();
    assert_eq!(snapshot.status, Status::Completed);
}

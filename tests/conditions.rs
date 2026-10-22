//! Fixed provider fixtures establish the intended differences before paid comparisons.
use loh::{
    domain::{EventData, Status, Usage},
    model::{ModelEvent, Provider, Request, Response},
    runtime::{
        condition::Condition,
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
#[path = "fixtures/identity.rs"]
mod identity;

fn response(output: Value) -> Response {
    Response {
        output: output.as_array().unwrap().clone(),
        usage: Usage {
            input: 10,
            output: 10,
            cached: 0,
        },
        model: "condition-fixture".into(),
    }
}
fn call(name: &str, arguments: Value) -> Response {
    response(
        json!([{"type":"function_call","name":name,"call_id":uuid::Uuid::new_v4().to_string(),"arguments":arguments.to_string()}]),
    )
}
fn prose() -> Response {
    response(
        json!([{"type":"message","role":"assistant","content":[{"type":"output_text","text":"finished attempt"}]}]),
    )
}

struct Script {
    main: AtomicUsize,
    premature: bool,
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
                json!({"intent":"refine","updates":{"r1":"write revised result.txt"},"additions":[],"answer":"","reason":"scope correction"}),
            ));
        }
        if request.instructions.starts_with("ROLE: verifier") {
            let evidence = request
                .input
                .iter()
                .filter_map(|item| item["output"].as_str())
                .filter_map(|text| serde_json::from_str::<Value>(text).ok())
                .find_map(|data| data["evidence"].as_str().map(str::to_owned));
            return Ok(if let Some(evidence) = evidence {
                call(
                    "report_verification",
                    json!({"summary":"read actual file","checks":[{"requirement":"r1","passed":true,"evidence":[evidence],"reason":"actual file"}]}),
                )
            } else {
                call("read_file", json!({"path":"result.txt"}))
            });
        }
        if self.premature {
            return Ok(prose());
        }
        Ok(match self.main.fetch_add(1, Ordering::SeqCst) {
            0 => call(
                "shell",
                json!({"command":"printf ready; sleep 1; printf old > obsolete.txt"}),
            ),
            1 => call(
                "apply_patch",
                json!({"edits":[{"path":"result.txt","expected":"missing","operation":"create","content":"revised"}]}),
            ),
            _ => prose(),
        })
    }
}

#[tokio::test]
async fn prompt_baseline_can_finish_without_independent_verification() {
    let root = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    let mut session = Session::new(root.path().to_owned(), "write a real file", 100000, 30);
    session.condition = Condition::Prompt;
    let host = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        logs.path().join("logs"),
    ));
    let store = Arc::new(Store::open(&logs.path().join("state.db")).unwrap());
    let (runner, _, _) = Runner::new(
        session,
        store,
        Arc::new(Script {
            main: AtomicUsize::new(0),
            premature: true,
        }),
        host,
        Settings {
            linger: false,
            max_output: 1024,
            ..Default::default()
        },
    );
    let state = tokio::time::timeout(Duration::from_secs(3), runner.run())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.status, Status::Completed);
    assert!(!state.actors.keys().any(|key| key.starts_with("verify-")));
    assert!(
        !root.path().join("result.txt").exists(),
        "this deliberate false completion must be left for the external oracle to detect"
    );
}

struct Repeating {
    main: AtomicUsize,
    diagnosis: AtomicUsize,
}
#[async_trait::async_trait]
impl Provider for Repeating {
    async fn respond(
        &self,
        request: Request,
        _cancel: CancellationToken,
        _events: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        if request.instructions.starts_with("ROLE: diagnosis") {
            self.diagnosis.fetch_add(1, Ordering::SeqCst);
            anyhow::bail!("diagnosis must be disabled in this ablation");
        }
        let turn = self.main.fetch_add(1, Ordering::SeqCst);
        Ok(if turn < 8 {
            call("read_file", json!({"path":"missing.txt"}))
        } else {
            call(
                "ask_user",
                json!({"question":"Fixture needs an external answer","requirements":["r1"]}),
            )
        })
    }
}

#[tokio::test]
async fn no_stagnation_ablation_really_disables_the_diagnostic_actor() {
    let root = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    let mut session = Session::new(root.path().to_owned(), "read missing input", 1000000, 30);
    session.condition = Condition::NoStagnation;
    let host = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        logs.path().join("logs"),
    ));
    let store = Arc::new(Store::open(&logs.path().join("state.db")).unwrap());
    let provider = Arc::new(Repeating {
        main: AtomicUsize::new(0),
        diagnosis: AtomicUsize::new(0),
    });
    let (runner, _, _) = Runner::new(
        session,
        store,
        provider.clone(),
        host,
        Settings {
            linger: false,
            max_output: 1024,
            ..Default::default()
        },
    );
    let state = tokio::time::timeout(Duration::from_secs(3), runner.run())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.status, Status::Waiting);
    assert_eq!(provider.main.load(Ordering::SeqCst), 9);
    assert_eq!(provider.diagnosis.load(Ordering::SeqCst), 0);
}

struct AtWrite {
    store: Arc<Store>,
    delivered: std::sync::atomic::AtomicBool,
}
impl loh::runtime::runner::Observer for AtWrite {
    fn on_event(&self, event: &loh::domain::SessionEvent) -> anyhow::Result<()> {
        if matches!(event.data, EventData::ActionFinished { success: true, .. })
            && !self.delivered.swap(true, Ordering::SeqCst)
        {
            self.store.enqueue_once(
                &event.session,
                "fixture-correction",
                &json!({"type":"message","text":"Use the revised value now"}),
            )?;
        }
        Ok(())
    }
}
struct Boundary {
    turn: AtomicUsize,
}

struct QueuedChecks {
    main: AtomicUsize,
    diagnosis: AtomicUsize,
}
#[async_trait::async_trait]
impl Provider for QueuedChecks {
    async fn respond(
        &self,
        request: Request,
        _cancel: CancellationToken,
        _events: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        if request.instructions.starts_with("ROLE: steering") {
            return Ok(call(
                "classify_input",
                json!({"intent":"refine","updates":{"r1":"inspect current result.txt"},"additions":[],"answer":"","reason":"fixture correction"}),
            ));
        }
        if request.instructions.starts_with("ROLE: diagnosis") {
            self.diagnosis.fetch_add(1, Ordering::SeqCst);
            return Ok(call(
                "report_diagnosis",
                json!({"cause":"obsolete work","strategy":"read current files"}),
            ));
        }
        if request.instructions.starts_with("ROLE: verifier") {
            let evidence = request
                .input
                .iter()
                .filter_map(|item| item["output"].as_str())
                .filter_map(|text| serde_json::from_str::<Value>(text).ok())
                .find_map(|data| data["evidence"].as_str().map(str::to_owned));
            return Ok(if let Some(evidence) = evidence {
                call(
                    "report_verification",
                    json!({"summary":"actual file","checks":[{"requirement":"r1","passed":true,"evidence":[evidence],"reason":"read file"}]}),
                )
            } else {
                call("read_file", json!({"path":"result.txt"}))
            });
        }
        if self.main.fetch_add(1, Ordering::SeqCst) == 0 {
            let mut result = call(
                "apply_patch",
                json!({"edits":[{"path":"result.txt","operation":"create","expected":"missing","content":"ready"}]}),
            );
            for _ in 0..4 {
                result
                    .output
                    .extend(call("check", json!({"command":"printf obsolete"})).output);
            }
            return Ok(result);
        }
        Ok(prose())
    }
}

#[tokio::test]
async fn obsolete_queued_results_do_not_trigger_stale_diagnostic_loops() {
    let root = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    let session = Session::new(root.path().to_owned(), "write result.txt", 200000, 30);
    let host = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        logs.path().join("logs"),
    ));
    let store = Arc::new(Store::open(&logs.path().join("state.db")).unwrap());
    let observer = Arc::new(AtWrite {
        store: store.clone(),
        delivered: Default::default(),
    });
    let provider = Arc::new(QueuedChecks {
        main: AtomicUsize::new(0),
        diagnosis: AtomicUsize::new(0),
    });
    let (runner, _, _) = Runner::new(
        session,
        store,
        provider.clone(),
        host,
        Settings {
            linger: false,
            max_output: 1024,
            ..Default::default()
        },
    );
    let state = tokio::time::timeout(Duration::from_secs(3), runner.observe(observer).run())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.status, Status::Completed);
    assert_eq!(provider.diagnosis.load(Ordering::SeqCst), 0);
}
#[async_trait::async_trait]
impl Provider for Boundary {
    async fn respond(
        &self,
        request: Request,
        _cancel: CancellationToken,
        _events: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        match self.turn.fetch_add(1, Ordering::SeqCst) {
            0 => Ok(call(
                "apply_patch",
                json!({"edits":[{"path":"boundary.txt","expected":"missing","operation":"create","content":"initial"}]}),
            )),
            _ => {
                anyhow::ensure!(
                    serde_json::to_string(&request.input)?.contains("Use the revised value now"),
                    "event-boundary correction missed the next request"
                );
                Ok(prose())
            }
        }
    }
}

#[tokio::test]
async fn scripted_correction_is_durable_before_the_following_model_dispatch() {
    let root = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    let mut session = Session::new(root.path().to_owned(), "event boundary fixture", 100000, 30);
    session.condition = Condition::Prompt;
    let host = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        logs.path().join("logs"),
    ));
    let store = Arc::new(Store::open(&logs.path().join("state.db")).unwrap());
    let observer = Arc::new(AtWrite {
        store: store.clone(),
        delivered: Default::default(),
    });
    let (runner, _, _) = Runner::new(
        session,
        store,
        Arc::new(Boundary {
            turn: AtomicUsize::new(0),
        }),
        host,
        Settings {
            linger: false,
            max_output: 1024,
            ..Default::default()
        },
    );
    let state = tokio::time::timeout(Duration::from_secs(3), runner.observe(observer).run())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.status, Status::Completed);
    assert!(state.inbox[0].applied);
    assert_eq!(state.inbox.len(), 1);
}

#[tokio::test]
async fn tracking_leaves_active_work_running_but_dynamic_and_replan_cancel_it() {
    for condition in [
        Condition::Prompt,
        Condition::Tracking,
        Condition::Dynamic,
        Condition::Replan,
    ] {
        let root = tempfile::tempdir().unwrap();
        let logs = tempfile::tempdir().unwrap();
        let mut session = Session::new(
            root.path().to_owned(),
            "write original result.txt",
            100000,
            30,
        );
        session.condition = condition;
        let original_turn = session.actors["main"].turn.clone();
        let host = Arc::new(ToolHost::new(
            Workspace::open(root.path()).unwrap(),
            Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
            logs.path().join("logs"),
        ));
        let store = Arc::new(Store::open(&logs.path().join("state.db")).unwrap());
        let (runner, control, mut events) = Runner::new(
            session,
            store.clone(),
            Arc::new(Script {
                main: AtomicUsize::new(0),
                premature: false,
            }),
            host,
            Settings {
                linger: false,
                max_output: 1024,
                ..Default::default()
            },
        );
        let running = tokio::spawn(runner.run());
        tokio::time::timeout(Duration::from_secs(5), async {
            while !matches!(
                events.recv().await.unwrap().data,
                EventData::ProcessStarted { .. }
            ) {}
        })
        .await
        .unwrap();
        control
            .send(Control::Message {
                text: "Change to revised result.txt".into(),
            })
            .await
            .unwrap();
        let state = tokio::time::timeout(Duration::from_secs(6), running)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(state.status, Status::Completed, "{condition:?}");
        identity::assert_user_turns(&state, &store);
        assert_eq!(state.actors["main"].turn, original_turn);
        assert_eq!(
            root.path().join("obsolete.txt").exists(),
            matches!(condition, Condition::Prompt | Condition::Tracking),
            "{condition:?}"
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("result.txt")).unwrap(),
            "revised"
        );
        assert_eq!(state.goal.revision > 1, condition != Condition::Prompt);
    }
}

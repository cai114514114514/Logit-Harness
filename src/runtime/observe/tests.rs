use super::*;
use crate::{
    domain::EvidenceKind,
    model::{Provider, Request, Response, ToolCall},
    runtime::{
        actor::Action,
        runner::{Job, Settings},
        state::Session,
    },
    store::Store,
    tools::{ToolHost, ToolResult, files::Workspace},
};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
struct NoRequests;
#[async_trait::async_trait]
impl Provider for NoRequests {
    async fn respond(
        &self,
        _: Request,
        _: CancellationToken,
        _: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        panic!("no model request")
    }
}
#[tokio::test]
async fn joined_fast_tool_keeps_buffered_process_and_output_before_finish() {
    fast_tool(0, false).await;
}
#[tokio::test]
async fn fast_tool_does_not_drain_busy_model_backlog_before_user_pause() {
    fast_tool(10_000, false).await;
}
#[tokio::test]
async fn live_tool_prefix_is_not_duplicated_when_result_outranks_wire_tail() {
    fast_tool(0, true).await;
}
async fn fast_tool(model_backlog: usize, live_prefix: bool) {
    let root = tempfile::tempdir().unwrap();
    let session = Session::new(root.path().into(), "test", 1000, 60);
    let store = Arc::new(Store::open(&root.path().join("db")).unwrap());
    let host = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        root.path().join("logs"),
    ));
    let (mut runner, _, _) = Runner::new(
        session,
        store.clone(),
        Arc::new(NoRequests),
        host,
        Settings::default(),
    );
    let pending = Pending {
        call: ToolCall {
            id: "call".into(),
            name: "run_command".into(),
            arguments: json!({"command":"fixture"}),
        },
        scope: Scope::default(),
    };
    runner.session.actions.insert(
        "fast".into(),
        Action {
            id: "fast".into(),
            actor: "main".into(),
            pending,
            running: true,
            pid: None,
            approval: None,
        },
    );
    runner.jobs.insert(
        "fast".into(),
        Job {
            goal: runner.session.goal.id.clone(),
            turn: runner.session.actors["main"].turn.clone(),
            actor: "main".into(),
            scope: Scope::default(),
            cancel: CancellationToken::new(),
            kind: JobKind::Tool {
                action: "fast".into(),
            },
        },
    );
    let log = Arc::new(std::sync::Mutex::new(
        crate::runtime::tool_stream::Log::default(),
    ));
    log.lock().unwrap().push(ToolEvent::Process { pid: 42 });
    log.lock()
        .unwrap()
        .push(ToolEvent::Output("complete output".into()));
    runner.tool_logs.insert("fast".into(), log);
    if model_backlog > 0 {
        runner.jobs.insert(
            "noisy".into(),
            Job {
                goal: runner.session.goal.id.clone(),
                turn: runner.session.actors["main"].turn.clone(),
                actor: "main".into(),
                scope: Scope::default(),
                cancel: CancellationToken::new(),
                kind: JobKind::Model {
                    reservation: "unused".into(),
                    compact: false,
                    prior: Phase::Ready,
                },
            },
        );
        for _ in 0..model_backlog {
            runner
                .send
                .send(Wire::Model {
                    id: "noisy".into(),
                    actor: "main".into(),
                    event: ModelEvent::Text("buffered model output".into()),
                })
                .unwrap();
        }
    }
    if live_prefix {
        runner
            .handle_wire(Wire::Tool {
                id: "fast".into(),
                index: 0,
                event: ToolEvent::Process { pid: 42 },
            })
            .unwrap();
    }
    runner
        .send
        .send(Wire::Tool {
            id: "fast".into(),
            index: 0,
            event: ToolEvent::Process { pid: 42 },
        })
        .unwrap();
    runner
        .send
        .send(Wire::Tool {
            id: "fast".into(),
            index: 1,
            event: ToolEvent::Output("complete output".into()),
        })
        .unwrap();
    let started = std::time::Instant::now();
    runner
        .handle_result(
            "fast".into(),
            ResultData::Tool(Ok(ToolResult {
                data: json!({}),
                success: true,
                files: Default::default(),
                kind: EvidenceKind::Command,
                images: vec![],
            })),
        )
        .await
        .unwrap();
    let events = store.events(&runner.session.id, 0).unwrap();
    let process = events
        .iter()
        .position(|e| matches!(&e.data, EventData::ProcessStarted { id, .. } if id == "fast"))
        .expect("buffered process start missing");
    let output = events.iter().position(|e| matches!(&e.data, EventData::Delta { actor, text } if actor == "fast" && text == "complete output")).expect("buffered output missing");
    let finish = events
        .iter()
        .position(|e| matches!(&e.data, EventData::ActionFinished { id, .. } if id == "fast"))
        .unwrap();
    assert!(process < output && output < finish);
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(&e.data, EventData::ProcessStarted { id,.. } if id=="fast"))
            .count(),
        1
    );
    assert_eq!(
        runner.wire.len(),
        model_backlog + 2,
        "tool settlement must leave unrelated stream backlog for the fair runner loop"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(&e.data, EventData::Delta { actor,.. } if actor=="main"))
    );
    if model_backlog > 0 {
        runner
            .handle_control(crate::runtime::state::Control::Pause)
            .await
            .unwrap();
        assert!(runner.jobs["noisy"].cancel.is_cancelled());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "tool flush delayed user cancellation behind unrelated model output"
        );
    }
}

#[tokio::test]
async fn consumed_live_tool_batches_release_their_shared_relay_tail() {
    let root = tempfile::tempdir().unwrap();
    let session = Session::new(root.path().into(), "test", 1000, 60);
    let store = Arc::new(Store::open(&root.path().join("db")).unwrap());
    let host = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        root.path().join("logs"),
    ));
    let (mut runner, _, _) = Runner::new(
        session,
        store,
        Arc::new(NoRequests),
        host,
        Settings::default(),
    );
    runner.session.actions.insert(
        "stream".into(),
        Action {
            id: "stream".into(),
            actor: "main".into(),
            pending: Pending {
                call: ToolCall {
                    id: "stream-call".into(),
                    name: "shell".into(),
                    arguments: json!({}),
                },
                scope: Scope::default(),
            },
            running: true,
            pid: None,
            approval: None,
        },
    );
    let log = Arc::new(std::sync::Mutex::new(
        crate::runtime::tool_stream::Log::default(),
    ));
    runner.tool_logs.insert("stream".into(), log.clone());
    for batch in 0..32 {
        let mut wires = vec![];
        for item in 0..8 {
            let event = ToolEvent::Output(format!("{batch}/{item}: {}", "x".repeat(1024)));
            let index = log.lock().unwrap().push(event.clone());
            wires.push(Wire::Tool {
                id: "stream".into(),
                index,
                event,
            });
        }
        assert_eq!(
            log.lock().unwrap().len(),
            8,
            "only this unconsumed batch is retained"
        );
        for wire in wires {
            runner.handle_wire(wire).unwrap();
        }
        assert_eq!(
            log.lock().unwrap().len(),
            0,
            "durably consumed output must not accumulate for the tool's whole lifetime"
        );
    }
    let events = runner.store.events(&runner.session.id, 0).unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(&e.data,EventData::Delta { actor,.. } if actor=="stream"))
            .count(),
        256,
        "pruning must not lose durable output"
    );
}

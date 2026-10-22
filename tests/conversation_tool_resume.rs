#[allow(dead_code)]
#[path = "fixtures/chat.rs"]
mod chat;

use loh::{
    domain::{EventData, Status},
    model::{ModelEvent, Provider, Request, Response},
    runtime::{
        conversation::Mode,
        runner::{Runner, Settings},
        state::{Control, Session},
    },
    store::Store,
    tools::{
        Effect, Tool, ToolContext, ToolEvent, ToolHost, ToolResult, ToolSpec, files::Workspace,
    },
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;

struct ReadOnly {
    started: Semaphore,
}
#[async_trait::async_trait]
impl Tool for ReadOnly {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "slow_read".into(),
            description: "Fixture read".into(),
            parameters: json!({"type":"object","properties":{}}),
            effect: Effect::Read,
        }
    }
    async fn call(
        &self,
        _: Value,
        context: ToolContext,
        cancel: CancellationToken,
        events: mpsc::UnboundedSender<ToolEvent>,
    ) -> anyhow::Result<ToolResult> {
        assert!(context.scope.readonly);
        events
            .send(ToolEvent::Output("read started\n".into()))
            .unwrap();
        self.started.add_permits(1);
        cancel.cancelled().await;
        anyhow::bail!("read cancelled after streamed output");
    }
}
#[derive(Default)]
struct Reading {
    requests: Mutex<Vec<Request>>,
}
#[async_trait::async_trait]
impl Provider for Reading {
    async fn respond(
        &self,
        request: Request,
        _: CancellationToken,
        _: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        let mut requests = self.requests.lock().unwrap();
        requests.push(request);
        Ok(if requests.len() == 1 {
            chat::call("slow_read", json!({}))
        } else {
            chat::text("Continued after cancelled read.")
        })
    }
}

#[tokio::test]
async fn paused_readonly_tool_settles_once_and_resume_preserves_tool_pair_and_turn() {
    for mode in [Mode::Conversation, Mode::Plan] {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&state.path().join("db")).unwrap());
        let session = Session::conversation(
            root.path().into(),
            if mode == Mode::Conversation {
                "Read this project"
            } else {
                ""
            },
            1_000_000,
            60,
        );
        let provider = Arc::new(Reading::default());
        let reader = Arc::new(ReadOnly {
            started: Semaphore::new(0),
        });
        let mut tools = ToolHost::new(
            Workspace::open(root.path()).unwrap(),
            Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
            state.path().join("logs"),
        );
        tools.register(reader.clone()).unwrap();
        let id = session.id.clone();
        let (runner, control, events) = Runner::new(
            session,
            store.clone(),
            provider.clone(),
            Arc::new(tools),
            Settings {
                max_output: 1024,
                ..Default::default()
            },
        );
        let mut live = chat::Live {
            _state: state,
            store,
            id,
            control,
            events,
            task: tokio::spawn(runner.run()),
        };
        if mode == Mode::Plan {
            live.control
                .send(Control::Plan {
                    text: Some("Plan using project evidence".into()),
                })
                .await
                .unwrap();
        }
        reader.started.acquire().await.unwrap().forget();
        let turn = live.saved().actors["main"].turn.clone();
        live.control.send(Control::Pause).await.unwrap();
        live.until(|e| {
            matches!(
                e,
                EventData::Status {
                    status: Status::Paused,
                    ..
                }
            )
        })
        .await;
        let paused = live.saved();
        assert!(paused.actions.is_empty());
        assert!(paused.actors["main"].context.unanswered().is_empty());
        live.control.send(Control::Resume).await.unwrap();
        live.waiting().await;
        let saved = live.saved();
        assert_eq!(saved.mode, mode);
        assert_eq!(saved.actors["main"].turn, turn);
        assert!(saved.goal.id.is_empty() && saved.archives.is_empty());
        let requests = provider.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        let outputs = requests[1]
            .input
            .iter()
            .filter(|item| item["type"] == "function_call_output")
            .collect::<Vec<_>>();
        assert_eq!(outputs.len(), 1);
        assert!(
            outputs[0]["output"]
                .as_str()
                .unwrap()
                .contains("read cancelled")
        );
        let events = live.store.events(&live.id, 0).unwrap();
        let starts = events
            .iter()
            .filter_map(|e| match &e.data {
                EventData::ActionStarted { id, .. } => Some(id),
                _ => None,
            })
            .collect::<Vec<_>>();
        let finishes = events
            .iter()
            .filter_map(|e| match &e.data {
                EventData::ActionFinished {
                    id, success: false, ..
                } => Some(id),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(starts, finishes);
        assert_eq!(starts.len(), 1);
        live.stop().await;
    }
}

use loh::{
    domain::{EventData, SessionEvent, Status, Usage},
    model::{ModelEvent, Provider, Request, Response},
    runtime::{
        runner::{Runner, Settings},
        state::{Control, Session},
    },
    store::Store,
    tools::{ToolHost, files::Workspace},
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::{Semaphore, broadcast, mpsc};
use tokio_util::sync::CancellationToken;
mod state;

pub struct Engineer {
    pub requests: Mutex<Vec<Request>>,
    pub gate: Semaphore,
    pub hold: AtomicBool,
}
impl Engineer {
    pub fn new(hold: bool) -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::new(vec![]),
            gate: Semaphore::new(0),
            hold: AtomicBool::new(hold),
        })
    }
    pub fn count(&self, role: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.instructions.starts_with(&format!("ROLE: {role}\n")))
            .count()
    }
}
pub fn text(value: &str) -> Response {
    response(
        json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":value}]}),
    )
}
pub fn call(name: &str, args: Value) -> Response {
    response(
        json!({"type":"function_call","call_id":uuid::Uuid::new_v4().to_string(),"name":name,"arguments":args.to_string()}),
    )
}
fn response(output: Value) -> Response {
    Response {
        output: vec![output],
        usage: Usage {
            input: 10,
            output: 10,
            cached: 0,
        },
        model: "fixture".into(),
    }
}

#[async_trait::async_trait]
impl Provider for Engineer {
    async fn respond(
        &self,
        request: Request,
        cancel: CancellationToken,
        _: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        self.requests.lock().unwrap().push(request.clone());
        if request.instructions.starts_with("ROLE: compact") {
            return Ok(text(
                "Saved progress; preserve actual user constraints from durable state.",
            ));
        }
        if request.instructions.starts_with("ROLE: conversation") {
            let user = request
                .input
                .iter()
                .rev()
                .find(|v| v["role"] == "user")
                .and_then(|v| v["content"].as_str())
                .unwrap_or("");
            return Ok(if matches!(user, "hello" | "thanks" | "What is SQLite?") {
                text("Hello!")
            } else if user.starts_with("Plan ") {
                text(
                    "I will create the requested file after explicit implementation authorization.",
                )
            } else {
                call("begin_goal", json!({}))
            });
        }
        if request.instructions.starts_with("ROLE: question") {
            return Ok(text("Side answer; the engineering task is unchanged."));
        }
        if request.instructions.starts_with("ROLE: steering") {
            let state = state::current(&request);
            if !state["pending_implementation"].is_null() {
                if state["requirements"]
                    .as_object()
                    .is_some_and(|requirements| requirements.is_empty())
                {
                    let quotes: Vec<_> = state["planning_user_turns"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(|turn| turn.as_str())
                        .filter(|turn| turn.starts_with("Plan "))
                        .collect();
                    return Ok(call(
                        "classify_input",
                        json!({"intent":if quotes.is_empty() {"clarify"} else {"start"},"objective_quotes":quotes,"updates":{},"additions":[],"answer":"What task should I implement?","reason":"extract actual planning request only"}),
                    ));
                }
                let changed = state["planning_user_turns"]
                    .to_string()
                    .contains("SQLite instead of JSON");
                return Ok(call(
                    "classify_input",
                    json!({"intent":if changed {"refine"} else {"nudge"},"updates":if changed {json!({"r1":"Create first.txt using SQLite"})} else {json!({})},"additions":[],"answer":"","reason":"only actual planning requirement changes are applied"}),
                ));
            }
            return Ok(call(
                "classify_input",
                json!({"intent":"queue","updates":{},"additions":[],"answer":"","reason":"independent work"}),
            ));
        }
        let state = state::current(&request);
        let objective = state["objective"].as_str().unwrap_or("");
        let path = if objective.contains("second.txt") {
            "second.txt"
        } else {
            "first.txt"
        };
        let outputs: Vec<Value> = request
            .input
            .iter()
            .filter(|v| v["type"] == "function_call_output")
            .filter_map(|v| v["output"].as_str())
            .filter_map(|v| serde_json::from_str(v).ok())
            .collect();
        if request.instructions.starts_with("ROLE: main") {
            if self.hold.swap(false, Ordering::SeqCst) {
                tokio::select! { _ = cancel.cancelled() => anyhow::bail!("cancelled"), permit = self.gate.acquire() => { permit?.forget(); } }
            }
            if outputs.iter().any(|v| v.get("result").is_some()) {
                return Ok(text("Ready for independent inspection."));
            }
            return Ok(call(
                "apply_patch",
                json!({"edits":[{"path":path,"expected":"missing","operation":"create","content":"fixture delivery"}]}),
            ));
        }
        if request.instructions.starts_with("ROLE: verifier") {
            if let Some(evidence) = outputs.iter().find_map(|v| v["evidence"].as_str()) {
                return Ok(call(
                    "report_verification",
                    json!({"summary":"verified","checks":[{"requirement":"r1","passed":true,"evidence":[evidence],"reason":"actual delivered file read"}]}),
                ));
            }
            return Ok(call("read_file", json!({"path":path})));
        }
        anyhow::bail!("unexpected fixture role")
    }
}

pub struct Live {
    pub _state: tempfile::TempDir,
    pub store: Arc<Store>,
    pub id: String,
    pub control: mpsc::Sender<Control>,
    pub events: broadcast::Receiver<SessionEvent>,
    pub task: tokio::task::JoinHandle<anyhow::Result<Session>>,
}
impl Live {
    pub fn start(session: Session, provider: Arc<dyn Provider>) -> Self {
        let state = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&state.path().join("db")).unwrap());
        let id = session.id.clone();
        let tools = ToolHost::new(
            Workspace::open(&session.workspace).unwrap(),
            Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
            state.path().join("logs"),
        );
        let (runner, control, events) = Runner::new(
            session,
            store.clone(),
            provider,
            Arc::new(tools),
            Settings {
                max_output: 1024,
                ..Settings::default()
            },
        );
        Self {
            _state: state,
            store,
            id,
            control,
            events,
            task: tokio::spawn(runner.run()),
        }
    }
    pub async fn until(&mut self, predicate: impl Fn(&EventData) -> bool) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if predicate(&self.events.recv().await.unwrap().data) {
                    break;
                }
            }
        })
        .await
        .unwrap_or_else(|error| {
            panic!(
                "{error}; last events: {:?}",
                self.store
                    .events(&self.id, 0)
                    .unwrap()
                    .iter()
                    .rev()
                    .take(10)
                    .collect::<Vec<_>>()
            )
        });
    }
    pub async fn completed(&mut self) {
        self.until(|e| matches!(e, EventData::Diagnostic { message } if message.starts_with("verified goal archived"))).await;
    }
    pub fn saved(&self) -> Session {
        self.store.load(&self.id).unwrap()
    }
    pub async fn stop(self) -> Session {
        self.control.send(Control::Stop).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    }
    pub async fn waiting(&mut self) {
        self.until(|e| {
            matches!(
                e,
                EventData::Status {
                    status: Status::Waiting,
                    ..
                }
            )
        })
        .await;
    }
}

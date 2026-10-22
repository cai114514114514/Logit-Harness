use loh::{
    domain::{EventData, Usage},
    extensions::{
        knowledge::Knowledge,
        skills::{Invocation, Skills},
    },
    model::{ModelEvent, Provider, Request, Response},
    runtime::{
        conversation::Mode,
        runner::{Runner, Settings},
        state::{Control, Session},
    },
    store::Store,
    tools::{ToolHost, files::Workspace},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct Workflow {
    requests: Mutex<Vec<Request>>,
    attack: bool,
    file: String,
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
fn text() -> Response {
    response(
        json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}]}),
    )
}
fn call(name: &str, arguments: Value) -> Response {
    response(
        json!({"type":"function_call","call_id":uuid::Uuid::new_v4().to_string(),"name":name,"arguments":arguments.to_string()}),
    )
}

#[async_trait::async_trait]
impl Provider for Workflow {
    async fn respond(
        &self,
        request: Request,
        _: CancellationToken,
        _: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        self.requests.lock().unwrap().push(request.clone());
        let outputs: Vec<Value> = request
            .input
            .iter()
            .filter(|v| v["type"] == "function_call_output")
            .filter_map(|v| v["output"].as_str())
            .filter_map(|s| serde_json::from_str(s).ok())
            .collect();
        if request.instructions.starts_with("ROLE: question") {
            return Ok(if self.attack && outputs.is_empty() {
                call(
                    "apply_patch",
                    json!({"edits":[{"path":"FORBIDDEN","expected":"missing","operation":"create","content":"bad"}]}),
                )
            } else {
                text()
            });
        }
        if request.instructions.starts_with("ROLE: conversation") {
            return Ok(call("begin_goal", json!({})));
        }
        if request.instructions.starts_with("ROLE: main") {
            if outputs.is_empty() {
                return Ok(call("read_file", json!({"path":"README.md"})));
            }
            if outputs.len() == 1 {
                return Ok(call(
                    "apply_patch",
                    json!({"edits":[{"path":self.file,"expected":"missing","operation":"create","content":"Project guidance from actual files"}]}),
                ));
            }
            return Ok(text());
        }
        if request.instructions.starts_with("ROLE: verifier") {
            if let Some(evidence) = outputs.iter().find_map(|v| v["evidence"].as_str()) {
                return Ok(call(
                    "report_verification",
                    json!({"summary":"read actual file","checks":[{"requirement":"r1","passed":true,"evidence":[evidence],"reason":"source inspected"}]}),
                ));
            }
            return Ok(call("read_file", json!({"path":self.file})));
        }
        anyhow::bail!("unexpected role")
    }
}

async fn execute(
    root: &std::path::Path,
    invocation: Invocation,
    plan: bool,
    provider: Arc<Workflow>,
    engineering: bool,
) -> (Session, Vec<loh::domain::SessionEvent>) {
    let state = tempfile::tempdir().unwrap();
    let session = Session::conversation(root.into(), "", 1_000_000, 60);
    let id = session.id.clone();
    let store = Arc::new(Store::open(&state.path().join("db")).unwrap());
    let mut tools = ToolHost::new(
        Workspace::open(root).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        state.path().join("logs"),
    );
    tools.knowledge = Some(Arc::new(Knowledge::new(root.into(), vec![]).unwrap()));
    let (runner, control, mut events) = Runner::new(
        session,
        store.clone(),
        provider,
        Arc::new(tools),
        Settings {
            max_output: 1024,
            ..Settings::default()
        },
    );
    if plan {
        control.send(Control::Plan { text: None }).await.unwrap();
    }
    control.send(invocation.control()).await.unwrap();
    let task = tokio::spawn(runner.run());
    tokio::time::timeout(std::time::Duration::from_secs(5),async {
        loop {
            let event=events.recv().await.unwrap();
            if (engineering && matches!(&event.data,EventData::Diagnostic {message} if message.starts_with("verified goal archived"))) || (!engineering && matches!(&event.data,EventData::Message {actor,text} if actor.starts_with("question-") && text=="done")) {break;}
        }
    }).await.unwrap_or_else(|error|panic!("{error}: {:?}",store.events(&id,0).unwrap()));
    control.send(Control::Stop).await.unwrap();
    let saved = task.await.unwrap().unwrap();
    (saved, store.events(&id, 0).unwrap())
}

fn project() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("README.md"), "fixture project").unwrap();
    root
}
fn workflow(attack: bool, file: &str) -> Arc<Workflow> {
    Arc::new(Workflow {
        requests: Mutex::new(vec![]),
        attack,
        file: file.into(),
    })
}
fn invocation(root: &std::path::Path, name: &str) -> Invocation {
    Skills::discover(root, &[])
        .unwrap()
        .with_bundled()
        .invoke(name, "$(touch NEVER) `echo x` $HOME")
        .unwrap()
}

#[tokio::test]
async fn bundled_init_creates_missing_guidance_under_engineering_verification() {
    let root = project();
    let provider = workflow(false, "AGENTS.md");
    let (saved, events) = execute(
        root.path(),
        invocation(root.path(), "init"),
        false,
        provider.clone(),
        true,
    )
    .await;
    assert!(root.path().join("AGENTS.md").exists());
    assert_eq!(saved.archives.len(), 1);
    assert!(
        saved.archives[0]
            .actors
            .values()
            .any(|a| serde_json::to_value(&a.role).unwrap() == "verifier")
    );
    assert!(
        saved
            .workflows
            .values()
            .all(|w| w.arguments == "$(touch NEVER) `echo x` $HOME")
    );
    assert!(!root.path().join("NEVER").exists());
    assert!(events.iter().any(|event|matches!(&event.data,EventData::Diagnostic {message} if message.contains("capability changed: skill"))));
    assert!(provider.requests.lock().unwrap().iter().any(|r| {
        r.input
            .iter()
            .any(|v| v.to_string().contains("preserving existing instructions"))
    }));
}

#[tokio::test]
async fn review_and_existing_init_guidance_reject_mutation_even_if_provider_tries() {
    for name in ["review", "init"] {
        let root = project();
        std::fs::write(root.path().join("AGENTS.md"), "KEEP EXACTLY").unwrap();
        let provider = workflow(true, "AGENTS.md");
        let (saved, _) = execute(
            root.path(),
            invocation(root.path(), name),
            false,
            provider.clone(),
            false,
        )
        .await;
        assert_eq!(
            std::fs::read_to_string(root.path().join("AGENTS.md")).unwrap(),
            "KEEP EXACTLY"
        );
        assert!(!root.path().join("FORBIDDEN").exists());
        assert!(saved.goal.id.is_empty());
        assert!(provider.requests.lock().unwrap().iter().all(|r| {
            !r.tools
                .iter()
                .any(|t| matches!(t["name"].as_str(), Some("apply_patch" | "begin_goal")))
        }));
    }
}

#[tokio::test]
async fn skill_after_pending_plan_cannot_escape_the_settlement_boundary() {
    let root = project();
    let provider = workflow(true, "AGENTS.md");
    let (saved, _) = execute(
        root.path(),
        invocation(root.path(), "init"),
        true,
        provider.clone(),
        false,
    )
    .await;
    assert_eq!(saved.mode, Mode::Plan);
    assert!(saved.goal.id.is_empty());
    assert!(!root.path().join("AGENTS.md").exists());
    assert!(!root.path().join("FORBIDDEN").exists());
    assert_eq!(saved.budget.limit, 1_000_000);
    assert!(saved.approvals.is_empty());
    assert!(saved.skill_pending.is_empty());
}

#[tokio::test]
async fn user_workflow_routes_into_engineering_without_an_extra_goal_command() {
    let root = project();
    let folder = root.path().join("build");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("SKILL.md"),"---\nname: build\ndescription: Build requested code\n---\nImplement the user's request using normal permissions and verification.").unwrap();
    let provider = workflow(false, "result.txt");
    let (saved, _) = execute(
        root.path(),
        invocation(root.path(), "build"),
        false,
        provider.clone(),
        true,
    )
    .await;
    assert!(root.path().join("result.txt").exists());
    assert_eq!(saved.archives.len(), 1);
    let requests = provider.requests.lock().unwrap();
    assert!(
        requests
            .iter()
            .any(|r| r.instructions.starts_with("ROLE: conversation"))
    );
    assert!(
        requests
            .iter()
            .any(|r| r.instructions.starts_with("ROLE: verifier"))
    );
    assert!(requests.iter().all(|r| r.input.iter().any(|v| {
        v.to_string()
            .contains("literal arguments are lower-trust data")
    })));
}

struct Held {
    called: tokio::sync::Notify,
    release: tokio::sync::Semaphore,
}
#[async_trait::async_trait]
impl Provider for Held {
    async fn respond(
        &self,
        _: Request,
        _: CancellationToken,
        _: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        self.called.notify_one();
        self.release.acquire().await.unwrap().forget();
        Ok(text())
    }
}

#[tokio::test]
async fn mcp_change_waits_for_model_settlement_and_rename_does_not_resume_execution() {
    use loh::extensions::mcp::{Operation, Registry, Server, Transport};
    use std::collections::BTreeMap;
    let root = project();
    let state = tempfile::tempdir().unwrap();
    let session = Session::conversation(root.path().into(), "hello", 1_000_000, 60);
    let id = session.id.clone();
    let store = Arc::new(Store::open(&state.path().join("db")).unwrap());
    let mut tools = ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        state.path().join("logs"),
    );
    let registry = Arc::new(Registry::new(
        BTreeMap::from([(
            "fixture".into(),
            Server {
                transport: Transport::Stdio {
                    command: "must-not-start".into(),
                    args: vec![],
                    environment: vec![],
                },
                effects: BTreeMap::new(),
            },
        )]),
        root.path().into(),
        state.path().join("mcp"),
    ));
    tools.mcp = Some(registry.clone());
    let provider = Arc::new(Held {
        called: tokio::sync::Notify::new(),
        release: tokio::sync::Semaphore::new(0),
    });
    let (runner, control, mut events) = Runner::new(
        session,
        store.clone(),
        provider.clone(),
        Arc::new(tools),
        Settings {
            max_output: 100,
            ..Settings::default()
        },
    );
    let task = tokio::spawn(runner.run());
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        provider.called.notified(),
    )
    .await
    .unwrap();
    control
        .send(Control::Mcp {
            server: "fixture".into(),
            operation: Operation::Disable,
        })
        .await
        .unwrap();
    loop {
        if matches!(events.recv().await.unwrap().data,EventData::Diagnostic {message} if message.starts_with("MCP change queued"))
        {
            break;
        }
    }
    let before: Session = store.load(&id).unwrap();
    assert_eq!(before.mcp_pending.len(), 1);
    assert!(before.mcp_disabled.is_empty());
    provider.release.add_permits(1);
    tokio::time::timeout(std::time::Duration::from_secs(3),async {loop {if matches!(events.recv().await.unwrap().data,EventData::Diagnostic {message} if message.starts_with("capability changed: MCP")) {break;}}}).await.unwrap();
    let settled: Session = store.load(&id).unwrap();
    assert!(settled.mcp_pending.is_empty());
    assert!(settled.mcp_disabled.contains("fixture"));
    assert!(
        registry
            .catalog("fixture", &CancellationToken::new())
            .await
            .unwrap_err()
            .to_string()
            .contains("disabled")
    );
    control
        .send(Control::Rename {
            title: "Renamed chat".into(),
        })
        .await
        .unwrap();
    loop {
        if matches!(events.recv().await.unwrap().data,EventData::Diagnostic {message} if message=="session renamed")
        {
            break;
        }
    }
    let renamed: Session = store.load(&id).unwrap();
    assert_eq!(renamed.title, "Renamed chat");
    assert_eq!(renamed.status, loh::domain::Status::Waiting);
    control.send(Control::Stop).await.unwrap();
    task.await.unwrap().unwrap();
    let events = store.events(&id, 0).unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.data, EventData::ModelFinished { .. }))
            .count(),
        1
    );
}

use loh::{
    extensions::mcp::{McpClient, Server, Transport},
    tools::Effect,
};
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult,
        PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    },
    service::RequestContext,
    transport::streamable_http_server::{
        StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct Echo;

#[tokio::test]
async fn stdio_external_effect_stays_uncertain_after_cancellation_and_restart() {
    use loh::extensions::mcp::Registry;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let root = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let counted = count.clone();
    let app = axum::Router::new().route(
        "/",
        axum::routing::post(move || {
            let count = counted.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                "committed"
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let serving = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut server = config(Transport::Stdio {
        command: "python3".into(),
        args: vec![
            "-u".into(),
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/mcp_server.py")
                .to_string_lossy()
                .into_owned(),
        ],
        environment: vec![],
    });
    server.effects.insert("echo".into(), Effect::External);
    let config = BTreeMap::from([("remote".into(), server)]);
    let registry = Arc::new(Registry::new(
        config.clone(),
        root.path().into(),
        logs.path().into(),
    ));
    let cancel = CancellationToken::new();
    let pending = {
        let registry = registry.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            registry
                .call(
                    "remote",
                    "echo",
                    json!({"text":format!("remote:{url}")}),
                    cancel,
                )
                .await
        })
    };
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while !root.path().join("started").exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    cancel.cancel();
    assert!(pending.await.unwrap().is_err());
    let saved = registry.pending("remote").await.unwrap();
    registry.close().await.unwrap();
    let recovered = Registry::new(config, root.path().into(), logs.path().into());
    recovered.recover().await.unwrap();
    let restored = recovered.pending("remote").await.unwrap();
    recovered
        .control("remote", loh::extensions::mcp::Operation::Disable)
        .await
        .unwrap();
    assert_eq!(recovered.pending("remote").await.unwrap(), restored);
    assert!(
        recovered
            .catalog("remote", &CancellationToken::new())
            .await
            .is_err()
    );
    recovered
        .control("remote", loh::extensions::mcp::Operation::Enable)
        .await
        .unwrap();
    recovered
        .control("remote", loh::extensions::mcp::Operation::Reconnect)
        .await
        .unwrap();
    assert_eq!(recovered.pending("remote").await.unwrap(), restored);
    let retry = recovered
        .call(
            "remote",
            "echo",
            json!({"text":"retry"}),
            CancellationToken::new(),
        )
        .await;
    recovered.close().await.unwrap();
    serving.abort();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(
        !saved.is_null(),
        "stopping local transport cannot erase a remote effect"
    );
    assert_eq!(saved, restored);
    assert!(retry.is_err());
}
impl ServerHandler for Echo {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut result = ListToolsResult::default();
        result.tools.push(Tool::new("echo", "Echo actual input", Arc::new(json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}).as_object().unwrap().clone())));
        Ok(result)
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let text = request.arguments.unwrap()["text"]
            .as_str()
            .unwrap()
            .to_owned();
        Ok(CallToolResult::success(vec![ContentBlock::text(text)]).into())
    }
}

#[derive(Clone)]
struct SlowMutation(Arc<std::sync::atomic::AtomicUsize>);
impl ServerHandler for SlowMutation {
    fn get_info(&self) -> ServerConfig {
        Echo.get_info()
    }
    async fn list_tools(
        &self,
        params: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut result = Echo.list_tools(params, context).await?;
        let mut status = result.tools[0].clone();
        status.name = "status".into();
        result.tools.push(status);
        Ok(result)
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if request.name == "status" {
            return Ok(
                CallToolResult::success(vec![ContentBlock::text("one write observed")]).into(),
            );
        }
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        Ok(CallToolResult::success(vec![ContentBlock::text("committed")]).into())
    }
}

#[tokio::test]
async fn uncertain_http_mutation_remains_blocked_after_registry_restart() {
    for effect in [Effect::External, Effect::Write] {
        check_http_uncertainty(effect).await;
    }
}

async fn check_http_uncertainty(effect: Effect) {
    use loh::extensions::mcp::Registry;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let count = Arc::new(AtomicUsize::new(0));
    let server_count = count.clone();
    let service: StreamableHttpService<SlowMutation, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(SlowMutation(server_count.clone())),
            Default::default(),
            Default::default(),
        );
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let serving = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let root = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    let mut server = config(Transport::Http {
        url,
        headers: BTreeMap::new(),
    });
    server.effects.insert("echo".into(), effect);
    server.effects.insert("status".into(), Effect::Read);
    let settings = BTreeMap::from([("remote".into(), server)]);
    let registry = Arc::new(Registry::new(
        settings.clone(),
        root.path().to_owned(),
        logs.path().to_owned(),
    ));
    let cancel = CancellationToken::new();
    let task = {
        let registry = registry.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            registry
                .call("remote", "echo", json!({"text":"charge once"}), cancel)
                .await
        })
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while count.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    cancel.cancel();
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    registry.close().await.unwrap();
    let resumed = Registry::new(
        settings.clone(),
        root.path().to_owned(),
        logs.path().to_owned(),
    );
    resumed.recover().await.unwrap();
    let pending = resumed.pending("remote").await.unwrap();
    assert_eq!(pending["record"]["arguments"]["text"], "charge once");
    assert!(
        resumed
            .call("remote", "status", json!({}), CancellationToken::new())
            .await
            .unwrap()
            .success
    );
    assert_eq!(
        resumed.pending("remote").await.unwrap(),
        pending,
        "reading remote state must not clear a pending mutation"
    );
    assert!(
        resumed
            .call(
                "remote",
                "echo",
                json!({"text":"retry with different words"}),
                CancellationToken::new()
            )
            .await
            .err()
            .unwrap()
            .to_string()
            .contains("uncertain outcome")
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
    resumed.close().await.unwrap();
    let mut changed = settings;
    changed.get_mut("remote").unwrap().transport = Transport::Stdio {
        command: "must-not-start".into(),
        args: vec![],
        environment: vec![],
    };
    let changed = Registry::new(changed, root.path().into(), logs.path().into());
    changed.recover().await.unwrap();
    assert_eq!(
        changed.pending("remote").await.unwrap(),
        pending,
        "configuration changes cannot rewrite the pending effect's provenance"
    );
    serving.abort();
}
fn config(transport: Transport) -> Server {
    Server {
        transport,
        effects: BTreeMap::from([("echo".into(), Effect::Read)]),
    }
}

#[tokio::test]
async fn stdio_sdk_client_discovers_and_calls_an_actual_child_server() {
    let root = tempfile::tempdir().unwrap();
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mcp_server.py");
    let server = config(Transport::Stdio {
        command: "python3".into(),
        args: vec!["-u".into(), script.to_string_lossy().into_owned()],
        environment: vec![],
    });
    let client = McpClient::connect(&server, root.path(), root.path())
        .await
        .unwrap();
    assert_eq!(client.catalog().len(), 1);
    let result = client
        .call("echo", json!({"text":"你好 MCP"}), CancellationToken::new())
        .await
        .unwrap();
    assert!(serde_json::to_string(&result).unwrap().contains("你好 MCP"));
    client.close().await.unwrap();
}

#[tokio::test]
async fn sdk_negotiates_an_initialize_server_without_a_second_client_implementation() {
    let root = tempfile::tempdir().unwrap();
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mcp_server.py");
    let server = config(Transport::Stdio {
        command: "python3".into(),
        args: vec![
            "-u".into(),
            script.to_string_lossy().into_owned(),
            "--initialize".into(),
        ],
        environment: vec![],
    });
    let client = McpClient::connect(&server, root.path(), root.path())
        .await
        .unwrap();
    let result = client
        .call(
            "echo",
            json!({"text":"negotiated"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    client.close().await.unwrap();
    assert!(result.complete && result.success);
    assert!(result.data.to_string().contains("negotiated"));
}

#[tokio::test]
async fn streamable_http_uses_the_official_sdk_on_both_sides() {
    let service: StreamableHttpService<Echo, LocalSessionManager> =
        StreamableHttpService::new(|| Ok(Echo), Default::default(), Default::default());
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let serving = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let root = tempfile::tempdir().unwrap();
    let client = McpClient::connect(
        &config(Transport::Http {
            url,
            headers: BTreeMap::new(),
        }),
        root.path(),
        root.path(),
    )
    .await
    .unwrap();
    let result = client
        .call(
            "echo",
            json!({"text":"actual HTTP round trip"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(
        serde_json::to_string(&result)
            .unwrap()
            .contains("actual HTTP round trip")
    );
    client.close().await.unwrap();
    serving.abort();
}

#[tokio::test]
async fn readonly_worker_cannot_call_a_configured_mcp_writer() {
    use loh::{
        domain::Goal,
        extensions::mcp::Registry,
        model::ToolCall,
        tools::{Scope, ToolHost, files::Workspace},
    };
    let root = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    let mut server = config(Transport::Stdio {
        command: "must-not-run".into(),
        args: vec![],
        environment: vec![],
    });
    server.effects.insert("echo".into(), Effect::Write);
    let registry = Arc::new(Registry::new(
        BTreeMap::from([("local".into(), server)]),
        root.path().to_owned(),
        logs.path().to_owned(),
    ));
    let goal = Goal::new("work");
    let mut host = ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(goal.clone())),
        logs.path().to_owned(),
    );
    for tool in loh::extensions::mcp::tools(registry) {
        host.register(tool).unwrap();
    }
    let (events, _) = tokio::sync::mpsc::unbounded_channel();
    let result = host
        .execute(
            ToolCall {
                id: "writer".into(),
                name: "mcp_call".into(),
                arguments: json!({"server":"local","tool":"echo","arguments":{"text":"write"}}),
            },
            Scope {
                requirements: goal.active_versions(),
                readonly: true,
                ..Default::default()
            },
            CancellationToken::new(),
            events,
        )
        .await;
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("worker cannot directly mutate")
    );
}

#[tokio::test]
async fn cancellation_confirms_the_stdio_writer_exits_before_returning() {
    let root = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mcp_server.py");
    let server = config(Transport::Stdio {
        command: "python3".into(),
        args: vec!["-u".into(), script.to_string_lossy().into_owned()],
        environment: vec![],
    });
    let client = Arc::new(
        McpClient::connect(&server, root.path(), logs.path())
            .await
            .unwrap(),
    );
    let cancel = CancellationToken::new();
    let task = {
        let client = client.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move { client.call("echo", json!({"text":"slow"}), cancel).await })
    };
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while !root.path().join("started").exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let identity: loh::tools::identity::Identity =
        serde_json::from_slice(&std::fs::read(logs.path().join("process.json")).unwrap()).unwrap();
    cancel.cancel();
    assert!(task.await.unwrap().is_err());
    assert!(
        loh::tools::identity::Identity::capture(identity.pid)
            .unwrap()
            .is_none()
    );
    assert!(!root.path().join("late").exists());
}

#[tokio::test]
async fn disable_waits_for_current_call_and_preserves_unknown_mutation_journal() {
    use loh::extensions::mcp::{Operation, Registry};
    let root = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    let mut server = config(Transport::Stdio {
        command: "python3".into(),
        args: vec![
            "-u".into(),
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/mcp_server.py")
                .display()
                .to_string(),
        ],
        environment: vec![],
    });
    server.effects.insert("echo".into(), Effect::Unknown);
    let registry = Arc::new(Registry::new(
        BTreeMap::from([("test".into(), server)]),
        root.path().into(),
        logs.path().into(),
    ));
    let cancel = CancellationToken::new();
    let call = {
        let registry = registry.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            registry
                .call("test", "echo", json!({"text":"slow"}), cancel)
                .await
        })
    };
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while !root.path().join("started").exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let disable = {
        let registry = registry.clone();
        tokio::spawn(async move { registry.control("test", Operation::Disable).await })
    };
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    assert!(
        !disable.is_finished(),
        "disable must await the in-flight call, not terminate its transport"
    );
    cancel.cancel();
    assert!(call.await.unwrap().is_err());
    let result = disable.await.unwrap().unwrap();
    assert_eq!(result["enabled"], false);
    assert_eq!(result["uncertain_mutation"], true);
    let pending = registry.pending("test").await.unwrap();
    registry.control("test", Operation::Enable).await.unwrap();
    assert_eq!(registry.pending("test").await.unwrap(), pending);
    assert!(
        registry
            .call(
                "test",
                "echo",
                json!({"text":"retry"}),
                CancellationToken::new()
            )
            .await
            .is_err()
    );
    assert!(
        registry
            .control("unconfigured", Operation::Enable)
            .await
            .is_err()
    );
    registry.close().await.unwrap();
}

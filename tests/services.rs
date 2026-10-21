use loh::{
    domain::Goal,
    model::ToolCall,
    tools::{Scope, ToolHost, files::Workspace, services::Services},
};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn a_live_service_allows_source_edits_but_cannot_write_source_itself() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    std::fs::write(root.path().join("source.txt"), "user data").unwrap();
    std::fs::write(
        root.path().join("server.py"),
        format!(
            r#"from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
import os
try:
    Path('source.txt').write_text('bad')
except PermissionError:
    pass
Path(os.environ['LOH_OUTPUT_DIR'], 'built.txt').write_text('ready')
class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b'ready')
HTTPServer(('127.0.0.1', {port}), Handler).serve_forever()
"#
        ),
    )
    .unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    let goal = Goal::new("run service and edit source");
    let scope = Scope {
        requirements: goal.active_versions(),
        ..Scope::default()
    };
    let services = Arc::new(Services::new(
        workspace.clone(),
        state.path().join("services"),
    ));
    let ready = format!("http://127.0.0.1:{port}/");
    let service = services
        .start(
            "python3 -B -u server.py",
            &ready,
            scope.clone(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.path().join("source.txt")).unwrap(),
        "user data"
    );
    assert!(service.output.join("built.txt").is_file());
    let host = ToolHost::new(
        workspace,
        Arc::new(tokio::sync::RwLock::new(goal)),
        state.path().join("logs"),
    );
    let (events, _) = tokio::sync::mpsc::unbounded_channel();
    let edit = host.execute(ToolCall { id:"edit".into(), name:"apply_patch".into(), arguments:json!({"edits":[{"path":"added.txt","expected":"missing","operation":"create","content":"independent edit"}]}) }, scope, CancellationToken::new(), events);
    assert!(
        tokio::time::timeout(Duration::from_secs(1), edit)
            .await
            .unwrap()
            .unwrap()
            .success
    );
    assert_eq!(
        reqwest::get(&ready).await.unwrap().text().await.unwrap(),
        "ready"
    );
    services.stop(&service.id).await.unwrap();
    assert!(reqwest::get(&ready).await.is_err());
}

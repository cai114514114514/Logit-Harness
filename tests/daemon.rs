//! A real CLI/daemon/socket test. The only double is the remote model's HTTP response.
use loh::{
    config::Paths,
    domain::Status,
    runtime::{
        ipc::{self, ClientMessage, ServerMessage},
        state::{Control, Session},
    },
    store::Store,
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Semaphore,
};

fn model_response(request: &Value) -> Value {
    let instruction = request["instructions"].as_str().unwrap_or("");
    let history = request["input"].as_array().unwrap();
    let outputs: Vec<_> = history
        .iter()
        .filter(|item| item["type"] == "function_call_output")
        .filter_map(|item| item["output"].as_str())
        .filter_map(|text| serde_json::from_str::<Value>(text).ok())
        .collect();
    let (name, args) = if instruction.starts_with("ROLE: verifier") {
        if let Some(evidence) = outputs.iter().find_map(|out| out["evidence"].as_str()) {
            (
                "report_verification",
                json!({"summary":"verified fixture output","checks":[{"requirement":"r1","passed":true,"evidence":[evidence],"reason":"read the delivered file"}]}),
            )
        } else {
            ("read_file", json!({"path":"result.txt"}))
        }
    } else if outputs.is_empty() {
        if instruction.contains("Crash recovery fixture") {
            return json!([{"type":"function_call","call_id":"append-once","name":"shell","arguments":json!({"command":"printf x >> marker.txt; sleep 30; printf y >> marker.txt","timeout_seconds":40}).to_string()}]);
        }
        (
            "apply_patch",
            json!({"edits":[{"path":"result.txt","expected":"missing","operation":"create","content":"background delivery"}]}),
        )
    } else {
        return json!([{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ready for verification"}]}]);
    };
    json!([{"type":"function_call","call_id":uuid::Uuid::new_v4().to_string(),"name":name,"arguments":args.to_string()}])
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn crashed_daemon_reconciles_command_and_preserves_concurrent_user_edit() {
    let workspace = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let paths = Paths::new(state.path().join("state"));
    paths.prepare().unwrap();
    let _cleanup = Cleanup {
        paths: paths.clone(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    std::fs::write(
        paths.home.join("config.toml"),
        format!(
            "base_url = \"http://{}\"\nmax_output = 1024\n",
            listener.local_addr().unwrap()
        ),
    )
    .unwrap();
    let gate = Arc::new(Semaphore::new(1));
    let server = tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            tokio::spawn(respond(socket, gate.clone()));
        }
    });
    // The fixture verifier reads this independent delivered file after recovery.
    std::fs::write(workspace.path().join("result.txt"), "user-owned result").unwrap();
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_loh"))
        .args([
            "--home",
            paths.home.to_str().unwrap(),
            "--cwd",
            workspace.path().to_str().unwrap(),
            "run",
            "Crash recovery fixture",
            "--detach",
        ])
        .env("DEEPSEEK_API_KEY", "fixture")
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let info: Value = serde_json::from_slice(&output.stdout).unwrap();
    let id = info["id"].as_str().unwrap();
    let store = Store::open(&paths.database()).unwrap();
    let pid = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let session: Session = store.load(id).unwrap();
            if workspace.path().join("marker.txt").exists() {
                if let Some(pid) = session.actions.values().find_map(|a| a.pid) {
                    break pid;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let parent = std::process::Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "ppid="])
        .output()
        .unwrap();
    let daemon_pid: i32 = String::from_utf8(parent.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_ne!(daemon_pid, std::process::id() as i32);
    unsafe {
        libc::kill(daemon_pid, libc::SIGKILL);
    }
    std::fs::write(workspace.path().join("marker.txt"), "user retained x").unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while tokio::net::UnixStream::connect(paths.socket(id).unwrap())
            .await
            .is_ok()
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let resumed = tokio::process::Command::new(env!("CARGO_BIN_EXE_loh"))
        .args(["--home", paths.home.to_str().unwrap(), "resume", id])
        .env("DEEPSEEK_API_KEY", "fixture")
        .output()
        .await
        .unwrap();
    assert!(
        resumed.status.success(),
        "{}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    let completed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let session: Session = store.load(id).unwrap();
            if session.status == Status::Completed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(
        completed.is_ok(),
        "recovery did not complete: {:?}; events: {:?}",
        store.load::<Session>(id).unwrap().status,
        store
            .events(id, 0)
            .unwrap()
            .into_iter()
            .filter(|e| matches!(
                e.data,
                loh::domain::EventData::Status { .. } | loh::domain::EventData::Diagnostic { .. }
            ))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("marker.txt")).unwrap(),
        "user retained x"
    );
    assert!(store.events(id, 0).unwrap().iter().any(|e| matches!(&e.data, loh::domain::EventData::Diagnostic { message } if message.contains("Recovery reconciled"))));
    server.abort();
}

async fn respond(mut socket: tokio::net::TcpStream, gate: Arc<Semaphore>) {
    let mut bytes = vec![];
    let mut buffer = [0; 8192];
    let (head, length) = loop {
        let n = socket.read(&mut buffer).await.unwrap();
        if n == 0 {
            return;
        }
        bytes.extend_from_slice(&buffer[..n]);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]);
            let length = headers
                .lines()
                .find_map(|line| {
                    line.to_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|s| s.trim().parse::<usize>().ok())
                })
                .unwrap();
            break (end + 4, length);
        }
    };
    while bytes.len() < head + length {
        let n = socket.read(&mut buffer).await.unwrap();
        if n == 0 {
            return;
        }
        bytes.extend_from_slice(&buffer[..n]);
    }
    let request: Value = serde_json::from_slice(&bytes[head..head + length]).unwrap();
    let permit = gate.acquire().await.unwrap();
    permit.forget();
    gate.add_permits(1);
    let body = format!(
        "data: {}\n\n",
        json!({"type":"response.completed","response":{
            "status":"completed","model":"fixture","output":model_response(&request),
            "usage":{"input_tokens":100,"output_tokens":50,"input_tokens_details":{"cached_tokens":0}}
        }})
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = socket.write_all(response.as_bytes()).await;
}

struct Cleanup {
    paths: Paths,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        if !self.paths.database().exists() {
            return;
        }
        if let Ok(store) = Store::open(&self.paths.database()) {
            for (id, _) in store.snapshots().unwrap_or_default() {
                let _ = std::process::Command::new(env!("CARGO_BIN_EXE_loh"))
                    .args(["--home", self.paths.home.to_str().unwrap(), "stop", &id])
                    .output();
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn detached_cli_exits_while_daemon_finishes_and_accepts_reattach() {
    let workspace = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let paths = Paths::new(state.path().join("state"));
    paths.prepare().unwrap();
    let _cleanup = Cleanup {
        paths: paths.clone(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    std::fs::write(
        paths.home.join("config.toml"),
        format!(
            "base_url = \"http://{}\"\nmax_output = 1024\n",
            listener.local_addr().unwrap()
        ),
    )
    .unwrap();
    let gate = Arc::new(Semaphore::new(0));
    let server_gate = gate.clone();
    let server = tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            tokio::spawn(respond(socket, server_gate.clone()));
        }
    });
    let child = tokio::process::Command::new(env!("CARGO_BIN_EXE_loh"))
        .args([
            "--home",
            paths.home.to_str().unwrap(),
            "--cwd",
            workspace.path().to_str().unwrap(),
            "run",
            "Write result.txt",
            "--detach",
            "--json",
        ])
        .env("DEEPSEEK_API_KEY", "fixture-key-not-a-real-secret")
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(Duration::from_secs(5), child)
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    let id = result["id"].as_str().unwrap();
    assert!(
        !workspace.path().join("result.txt").exists(),
        "the parent must exit before inference is allowed to finish"
    );
    gate.add_permits(1);
    let socket = paths.socket(id).unwrap();
    let completed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let ServerMessage::Snapshot { state, .. } =
                ipc::request(&socket, ClientMessage::Snapshot)
                    .await
                    .unwrap()
            {
                if state.status == Status::Completed {
                    break state;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("result.txt")).unwrap(),
        "background delivery"
    );
    let mut attached = ipc::subscribe(&socket, 0).await.unwrap();
    assert!(
        matches!(ipc::receive(&mut attached).await.unwrap(),ServerMessage::Snapshot{state,..}if state.id==completed.id)
    );
    drop(attached);
    ipc::request(
        &socket,
        ClientMessage::Command {
            command: Control::Stop,
        },
    )
    .await
    .unwrap();
    let store = Store::open(&paths.database()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let state: Session = store.load(id).unwrap();
            if state.status == Status::Stopped {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    server.abort();
}

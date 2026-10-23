use loh::{
    config::{Config, Paths},
    domain::EventData,
    runtime::{
        ipc::{self, ClientMessage, Server, ServerMessage},
        state::{Control, Session},
    },
    store::Store,
};
use std::{os::unix::fs::PermissionsExt, sync::Arc};
use tokio::{
    io::AsyncBufReadExt,
    sync::{broadcast, mpsc},
};
use tokio_util::sync::CancellationToken;

#[test]
fn config_is_loaded_and_rejects_invalid_execution_limits() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("config.toml"),
        "tokens = 123456\nworkers = 1\n",
    )
    .unwrap();
    assert_eq!(Config::load(dir.path()).unwrap().tokens, 123456);
    std::fs::write(dir.path().join("config.toml"), "seconds = 0\n").unwrap();
    assert!(Config::load(dir.path()).is_err());
}

#[test]
fn state_directories_are_private_and_socket_names_fit_macos() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("long-name".repeat(10));
    let paths = Paths::new(home);
    paths.prepare().unwrap();
    assert_eq!(
        std::fs::metadata(&paths.home).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert!(
        paths
            .socket("0123456789abcdef")
            .unwrap()
            .as_os_str()
            .as_encoded_bytes()
            .len()
            < 104
    );
    assert!(paths.socket("../escape").is_err());
}

#[tokio::test]
async fn socket_routes_controls_and_replays_after_a_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path().join("state"));
    paths.prepare().unwrap();
    let store = Arc::new(Store::open(&paths.database()).unwrap());
    let session = Session::new(dir.path().to_path_buf(), "test ipc", 10000, 60);
    let first = store
        .commit(
            &session.id,
            &session,
            EventData::User {
                turn: None,
                message: "first".into(),
            },
        )
        .unwrap();
    let second = store
        .commit(
            &session.id,
            &session,
            EventData::User {
                turn: None,
                message: "second".into(),
            },
        )
        .unwrap();
    let (control, mut received) = mpsc::channel(8);
    let (events, _) = broadcast::channel(32);
    let path = paths.socket(&session.id).unwrap();
    paths.prepare().unwrap();
    let server = Server::bind(
        path.clone(),
        session.id.clone(),
        store.clone(),
        control,
        events.clone(),
    )
    .await
    .unwrap();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(server.run(cancel.clone()));
    let ack = ipc::request(
        &path,
        ClientMessage::Command {
            command: Control::Pause,
        },
    )
    .await
    .unwrap();
    assert!(matches!(ack, ServerMessage::Ack));
    assert!(matches!(received.recv().await, Some(Control::Wake)));
    let queued = store.queued(&session.id, 0).unwrap();
    assert!(matches!(
        serde_json::from_value::<Control>(queued[0].1.clone()).unwrap(),
        Control::Pause
    ));
    let stream = ipc::subscribe(&path, first.sequence).await.unwrap();
    let mut lines = stream.lines();
    let snapshot: ServerMessage =
        serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    assert!(matches!(snapshot, ServerMessage::View { .. }));
    let replay: ServerMessage =
        serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    assert!(matches!(replay,ServerMessage::Event{event} if event.sequence==second.sequence));
    drop(lines);
    // Disconnect is deliberately not translated into Pause or Stop.
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), received.recv())
            .await
            .is_err()
    );
    cancel.cancel();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn ui_status_does_not_retransmit_large_model_or_image_history() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().join("state"));
    paths.prepare().unwrap();
    let store = Arc::new(Store::open(&paths.database()).unwrap());
    let mut session = Session::new(root.path().to_owned(), "status projection", 1000, 60);
    session
        .actors
        .get_mut("main")
        .unwrap()
        .context
        .user(&"large context ".repeat(30_000));
    store
        .commit(
            &session.id,
            &session,
            EventData::Diagnostic {
                message: "fixture".into(),
            },
        )
        .unwrap();
    let (control, _) = mpsc::channel(8);
    let (events, _) = broadcast::channel(8);
    let path = paths.socket(&session.id).unwrap();
    let server = Server::bind(path.clone(), session.id.clone(), store, control, events)
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let running = tokio::spawn(server.run(cancel.clone()));
    let response = ipc::request(&path, ClientMessage::Status).await.unwrap();
    let bytes = serde_json::to_vec(&response).unwrap();
    assert!(bytes.len() < 10_000);
    assert!(!String::from_utf8_lossy(&bytes).contains("large context"));
    assert!(matches!(response, ServerMessage::View { .. }));
    cancel.cancel();
    running.await.unwrap().unwrap();
}

use loh::{
    commands::{self, Outcome, Service},
    config::Paths,
    domain::EventData,
    runtime::{
        graph::{Step, StepStatus},
        ipc::{self, ClientMessage, PROTOCOL, Server, ServerMessage},
        state::{Control, Session},
        team::{MemberControl, Request, Target},
        view::ViewTarget,
    },
    store::Store,
};
use std::{collections::BTreeSet, sync::Arc};
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

fn member(session: &mut Session, name: &str, step: &str) -> Target {
    session
        .graph
        .install(
            vec![Step {
                id: step.into(),
                description: step.into(),
                requirements: BTreeSet::from(["r1".into()]),
                dependencies: BTreeSet::new(),
                files: BTreeSet::new(),
                revision: 1,
                status: StepStatus::Pending,
            }],
            &session.goal,
        )
        .unwrap();
    session.assign_member(Some(name), step, "work").unwrap();
    session.member_target(name).unwrap()
}

#[tokio::test]
async fn addressed_ipc_retry_keys_are_tuples_and_ack_is_only_persistence() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().join("state"));
    paths.prepare().unwrap();
    let store = Arc::new(Store::open(&paths.database()).unwrap());
    let mut session = Session::new(root.path().into(), "fixture", 1000, 60);
    let target = member(&mut session, "Ada", "a");
    store
        .commit(
            &session.id,
            &session,
            EventData::User {
                turn: None,
                message: "fixture".into(),
            },
        )
        .unwrap();
    let (control, mut wakes) = mpsc::channel(8);
    let (events, _) = broadcast::channel(8);
    let socket = paths.socket(&session.id).unwrap();
    let server = Server::bind(
        socket.clone(),
        session.id.clone(),
        store.clone(),
        control,
        events,
    )
    .await
    .unwrap();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(server.run(cancel.clone()));
    let request = Request {
        client_id: "a:b".into(),
        id: "c".into(),
        target: target.clone(),
        action: MemberControl::Pause,
    };
    let send = |request| ClientMessage::Addressed { request };
    let first = ipc::request(&socket, send(request.clone())).await.unwrap();
    let ServerMessage::Accepted { cursor, .. } = first else {
        panic!("durable ack")
    };
    let retry = ipc::request(&socket, send(request.clone())).await.unwrap();
    assert!(matches!(retry, ServerMessage::Accepted { cursor: saved, .. } if saved == cursor));
    let mut changed = request.clone();
    changed.action = MemberControl::Stop;
    assert!(matches!(
        ipc::request(&socket, send(changed)).await.unwrap(),
        ServerMessage::Error { .. }
    ));
    let mut distinct = request.clone();
    distinct.client_id = "a".into();
    distinct.id = "b:c".into();
    ipc::request(&socket, send(distinct)).await.unwrap();
    assert_eq!(store.queued(&session.id, 0).unwrap().len(), 2);
    assert!(
        store
            .load::<Session>(&session.id)
            .unwrap()
            .team
            .receipts
            .is_empty()
    );
    assert!(matches!(wakes.recv().await, Some(Control::Wake)));
    let reply = ipc::request(
        &socket,
        ClientMessage::Hello {
            protocol: PROTOCOL + 1,
        },
    )
    .await
    .unwrap();
    assert!(matches!(reply, ServerMessage::Error { message } if message.contains("protocol")));
    // Raw old clients cannot bypass negotiation using the old command tag.
    use tokio::io::{AsyncWriteExt, BufReader};
    let mut legacy = tokio::net::UnixStream::connect(&socket).await.unwrap();
    legacy
        .write_all(b"{\"type\":\"command\",\"command\":{\"type\":\"stop\"}}\n")
        .await
        .unwrap();
    assert!(
        matches!(ipc::receive(&mut BufReader::new(legacy)).await.unwrap(), ServerMessage::Error { message } if message.contains("protocol"))
    );
    assert_eq!(store.queued(&session.id, 0).unwrap().len(), 2);
    cancel.cancel();
    task.await.unwrap().unwrap();
}

#[test]
fn member_commands_capture_assignment_and_use_saved_workspace() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().join("state"));
    paths.prepare().unwrap();
    let mut session = Session::new(root.path().into(), "fixture", 1000, 60);
    let target = member(&mut session, "Ada", "a");
    let view = session.resolve_view("Ada").unwrap();
    Store::open(&paths.database())
        .unwrap()
        .commit(
            &session.id,
            &session,
            EventData::User {
                turn: None,
                message: "fixture".into(),
            },
        )
        .unwrap();
    let service = Service::new(&paths, other.path(), Some(&session.id)).in_view(view.clone());
    let Outcome::Control(Control::Directed { request }) =
        service.execute(commands::parse("/pause").unwrap()).unwrap()
    else {
        panic!("directed control")
    };
    assert_eq!(request.target, target);
    assert!(
        service
            .execute(commands::parse("/goal overwrite").unwrap())
            .is_err()
    );
    assert!(
        service
            .execute(commands::parse("/budget 4000").unwrap())
            .is_err()
    );
    let Outcome::Focus {
        target: focused, ..
    } = service
        .execute(commands::parse("/agent main").unwrap())
        .unwrap()
    else {
        panic!("focus")
    };
    assert_eq!(focused, ViewTarget::main());
    let Outcome::Control(Control::Directed { request }) = service
        .execute(commands::parse("/tell Ada literal '$X'\nsecond").unwrap())
        .unwrap()
    else {
        panic!("tell")
    };
    assert_eq!(
        request.action,
        MemberControl::Message {
            text: "literal '$X'\nsecond".into()
        }
    );
    let Outcome::Local { text, .. } = service
        .execute(commands::parse("/context").unwrap())
        .unwrap()
    else {
        panic!("context")
    };
    assert!(!text.contains("\"id\": \"main\""));
}

#[tokio::test]
async fn three_target_subscribers_replay_with_independent_cursors_and_public_views() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().join("state"));
    paths.prepare().unwrap();
    let mut session = Session::new(root.path().into(), "fixture", 1000, 60);
    let target = member(&mut session, "Ada", "a");
    let view = session.resolve_view("Ada").unwrap();
    session
        .actors
        .get_mut("main")
        .unwrap()
        .context
        .user(&"private context ".repeat(30_000));
    let store = Arc::new(Store::open(&paths.database()).unwrap());
    let one = store
        .commit(
            &session.id,
            &session,
            EventData::ActorBound {
                actor: session.team.members[target.member_id()]
                    .actor
                    .clone()
                    .unwrap(),
                target,
                goal: Some(session.goal.id.clone()),
            },
        )
        .unwrap();
    let two = store
        .commit(
            &session.id,
            &session,
            EventData::User {
                turn: None,
                message: "second".into(),
            },
        )
        .unwrap();
    let (control, _) = mpsc::channel(8);
    let (events, _) = broadcast::channel(8);
    let socket = paths.socket(&session.id).unwrap();
    let server = Server::bind(
        socket.clone(),
        session.id.clone(),
        store.clone(),
        control,
        events.clone(),
    )
    .await
    .unwrap();
    let cancel = CancellationToken::new();
    let running = tokio::spawn(server.run(cancel.clone()));
    let mut readers = vec![];
    for after in [0, one.sequence, two.sequence] {
        let mut reader = ipc::subscribe_target(&socket, after, view.clone())
            .await
            .unwrap();
        let response = ipc::receive(&mut reader).await.unwrap();
        assert!(matches!(response, ServerMessage::TargetView { .. }));
        let bytes = serde_json::to_string(&response).unwrap();
        assert!(bytes.len() < 15000 && !bytes.contains("private context"));
        let replay = store.events(&session.id, after).unwrap();
        for expected in replay {
            assert!(
                matches!(ipc::receive(&mut reader).await.unwrap(), ServerMessage::Event { event } if event.sequence == expected.sequence)
            );
        }
        readers.push(reader);
    }
    let three = store
        .append(
            &session.id,
            EventData::User {
                turn: None,
                message: "third".into(),
            },
        )
        .unwrap();
    events.send(three.clone()).unwrap();
    for reader in &mut readers {
        assert!(
            matches!(ipc::receive(reader).await.unwrap(), ServerMessage::Event { event } if event.sequence == three.sequence)
        );
    }
    drop(readers.remove(1));
    let mut reconnect = ipc::subscribe_target(&socket, two.sequence, view)
        .await
        .unwrap();
    assert!(matches!(
        ipc::receive(&mut reconnect).await.unwrap(),
        ServerMessage::TargetView { .. }
    ));
    assert!(
        matches!(ipc::receive(&mut reconnect).await.unwrap(), ServerMessage::Event { event } if event.sequence == three.sequence)
    );
    let missed = store
        .append(
            &session.id,
            EventData::Diagnostic {
                message: "recovery event without a broadcast".into(),
            },
        )
        .unwrap();
    let wake = store
        .append(
            &session.id,
            EventData::Diagnostic {
                message: "later runner publication".into(),
            },
        )
        .unwrap();
    events.send(wake.clone()).unwrap();
    for reader in readers.iter_mut().chain(std::iter::once(&mut reconnect)) {
        for sequence in [missed.sequence, wake.sequence] {
            assert!(
                matches!(ipc::receive(reader).await.unwrap(), ServerMessage::Event { event } if event.sequence == sequence),
                "live publication must recover all earlier durable events before advancing its cursor"
            );
        }
    }
    cancel.cancel();
    running.await.unwrap().unwrap();
}

#[test]
fn archived_member_ids_remain_readonly_and_current_names_win() {
    use loh::runtime::{conversation::Archive, team::AssignmentStatus};
    let root = tempfile::tempdir().unwrap();
    let mut session = Session::new(root.path().into(), "fixture", 1000, 60);
    let old = member(&mut session, "Ada", "a");
    let original = session.resolve_view("Ada").unwrap();
    let actor = session.team.members[old.member_id()].actor.clone().unwrap();
    session.finish_assignment(&actor, AssignmentStatus::Finished);
    session.archives.push(Archive {
        team: std::mem::take(&mut session.team),
        goal: session.goal.clone(),
        status: loh::domain::Status::Completed,
        graph: session.graph.clone(),
        actors: session.actors.clone(),
        evidence: vec![],
        actions: Default::default(),
        report: None,
    });
    session.goal = loh::domain::Goal::new("new goal");
    let archived = session.resolve_view(old.member_id()).unwrap();
    assert!(archived.readonly);
    assert!(session.target_view(&original).unwrap().origin.readonly);
    let current = member(&mut session, "Ada", "a");
    assert_eq!(session.resolve_view("Ada").unwrap().target, current);
    assert_eq!(
        session
            .resolve_view(old.member_id())
            .unwrap()
            .target
            .member_id(),
        old.member_id()
    );
    let mut second = session.archives[0].clone();
    let old_id = old.member_id().to_string();
    let mut m = second.team.members.remove(&old_id).unwrap();
    m.id = "other-id".into();
    second.team.members.insert(m.id.clone(), m);
    second.goal.id = "another-goal".into();
    session.archives.push(second);
    session.team = Default::default();
    assert!(
        session.resolve_view("Ada").is_err(),
        "ambiguous old names must not guess"
    );
    assert!(session.resolve_view(&old_id).unwrap().readonly);
}

#[test]
fn launcher_quotes_trusted_paths_and_returns_attach_on_failure() {
    use loh::commands::window;
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("space ' $dollar");
    std::fs::create_dir_all(&workspace).unwrap();
    let paths = Paths::new(root.path().join("home ' quoted"));
    paths.prepare().unwrap();
    let session = Session::new(workspace.clone(), "fixture", 1000, 60);
    Store::open(&paths.database())
        .unwrap()
        .commit(
            &session.id,
            &session,
            EventData::User {
                turn: None,
                message: "fixture".into(),
            },
        )
        .unwrap();
    let command = window::prepare(
        &paths,
        std::path::Path::new("/trusted/loh ' app"),
        &session.id,
        &ViewTarget::main(),
    )
    .unwrap();
    assert!(command.attach.contains("'\\''"));
    assert!(command.attach.contains("'--cwd'"));
    assert!(command.attach.contains("'--home'"));
    assert!(!command.attach.contains("DEEPSEEK"));
    let result = window::launch_with(command.clone(), |program, args| {
        assert_eq!(program, std::path::Path::new("/usr/bin/osascript"));
        assert_eq!(
            args.last().unwrap(),
            &std::ffi::OsString::from(&command.attach)
        );
        assert!(!args[1].to_string_lossy().contains("$dollar"));
        anyhow::bail!("fixture denied launch")
    });
    assert!(!result.opened && result.error.unwrap().contains("fixture denied"));
    assert_eq!(result.attach, command.attach);
    assert!(window::launch_with(command, |_, _| Ok(())).opened);
}

#[tokio::test]
async fn incompatible_old_daemon_is_reported_without_replacing_its_socket() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("old.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let running = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        reader
            .get_mut()
            .write_all(b"{\"type\":\"error\",\"message\":\"unknown hello\"}\n")
            .await
            .unwrap();
        listener
    });
    let error = ipc::check_protocol(&socket).await.unwrap_err().to_string();
    assert!(error.contains("protocol") && error.contains("Keep the running daemon"));
    let listener = running.await.unwrap();
    assert!(socket.exists());
    drop(listener);
}

#[tokio::test]
async fn crash_mid_handler_retains_input_with_explicit_failed_receipt() {
    use loh::runtime::{
        recovery,
        team::{Receipt, ReceiptStatus},
    };
    let root = tempfile::tempdir().unwrap();
    let mut session = Session::new(root.path().into(), "fixture", 1000, 60);
    let request = Request {
        id: "interrupted".into(),
        client_id: "fixture".into(),
        target: Target::Main,
        action: MemberControl::Message {
            text: "keep original '$input'".into(),
        },
    };
    session.team.receipts.insert(
        request.key(),
        Receipt {
            applying: true,
            request: request.clone(),
            status: ReceiptStatus::Pending,
            error: None,
            pending_input: None,
        },
    );
    let store = Store::open(&root.path().join("db")).unwrap();
    store
        .commit(
            &session.id,
            &session,
            EventData::Diagnostic {
                message: "crash boundary".into(),
            },
        )
        .unwrap();
    recovery::reconcile(&mut session, &store, &root.path().join("logs"))
        .await
        .unwrap();
    let saved: Session = store.load(&session.id).unwrap();
    let receipt = &saved.team.receipts[&request.key()];
    assert_eq!(receipt.request, request);
    assert_eq!(receipt.status, ReceiptStatus::Failed);
    assert!(!receipt.applying);
    assert!(
        receipt
            .error
            .as_ref()
            .unwrap()
            .contains("partially applied")
    );
    assert!(store.events(&session.id,0).unwrap().iter().any(|event|matches!(&event.data,EventData::RequestReceipt { receipt,.. } if receipt.request==request && receipt.status==ReceiptStatus::Failed)));
}

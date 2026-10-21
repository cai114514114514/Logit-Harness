use loh::cli::terminal::{Input, Intent, Screen, parse, render};
use loh::{
    domain::{EventData, SessionEvent, Status},
    runtime::{
        ipc::ServerMessage,
        state::{Control, Session},
    },
};

#[test]
fn editing_keeps_cjk_combining_marks_and_emoji_clusters_intact() {
    let mut input = Input::default();
    input.insert("中e\u{301}👩‍💻");
    input.backspace();
    assert_eq!(input.text(), "中e\u{301}");
    input.left();
    input.backspace();
    assert_eq!(input.text(), "e\u{301}");
    input.insert("你好\nSQLite");
    assert_eq!(input.text(), "你好\nSQLitee\u{301}");
    assert_eq!(input.position(20), (6, 1));
}

#[test]
fn commands_distinguish_control_from_ordinary_corrections() {
    assert!(matches!(
        parse("/pause").unwrap(),
        Intent::Control(Control::Pause)
    ));
    assert!(matches!(parse("/detach").unwrap(), Intent::Detach));
    assert!(matches!(
        parse("/budget 6000000 30000").unwrap(),
        Intent::Control(Control::Budget {
            tokens: Some(6_000_000),
            seconds: Some(30_000)
        })
    ));
    assert!(parse("/budget 0").is_err());
    assert!(parse("/delete-all").is_err());
    match parse("存储改成 SQLite\n保留界面").unwrap() {
        Intent::Control(Control::Message { text }) => assert!(text.contains('\n')),
        _ => panic!("correction must stay a user message"),
    }
}

#[test]
fn a_newline_after_a_full_input_row_does_not_insert_an_extra_visual_row() {
    let mut input = Input::default();
    input.insert("中文\nA");
    assert_eq!(input.position(4), (1, 1));
}

#[test]
fn delayed_snapshot_cannot_overwrite_a_newer_live_status() {
    let mut screen = Screen::default();
    let mut session = Session::new(std::env::temp_dir(), "test", 1000, 60);
    session.status = Status::Running;
    screen.receive(ServerMessage::Snapshot {
        state: session.clone().into(),
        sequence: 10,
    });
    screen.receive(ServerMessage::Event {
        event: SessionEvent {
            sequence: 11,
            session: session.id.clone(),
            timestamp: String::new(),
            data: EventData::Status {
                status: Status::Paused,
                reason: "user pause".into(),
            },
        },
    });
    screen.receive(ServerMessage::Snapshot {
        state: session.into(),
        sequence: 10,
    });
    assert_eq!(screen.session.unwrap().status, Status::Paused);
}

#[test]
fn replayed_history_does_not_revert_the_latest_snapshot_status_or_budget() {
    let mut session = Session::new(std::env::temp_dir(), "实现中文界面", 1000, 60);
    session.status = Status::Paused;
    session.budget.used = 50;
    let mut screen = Screen::default();
    screen.receive(ServerMessage::Snapshot {
        state: session.into(),
        sequence: 9,
    });
    for (sequence, data) in [
        (
            1,
            EventData::Status {
                status: Status::Running,
                reason: "old".into(),
            },
        ),
        (
            2,
            EventData::Usage {
                actor: "main".into(),
                input: 20,
                output: 30,
                cached: 0,
            },
        ),
    ] {
        screen.receive(ServerMessage::Event {
            event: SessionEvent {
                sequence,
                session: "test".into(),
                timestamp: String::new(),
                data,
            },
        });
    }
    assert_eq!(screen.session.as_ref().unwrap().status, Status::Paused);
    assert_eq!(screen.session.as_ref().unwrap().budget.used, 50);
    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    screen.input.insert("中途纠正");
    terminal.draw(|frame| render(frame, &screen)).unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("Logit- Harness"));
    // Each CJK glyph occupies two cells; the test backend stores a blank continuation cell.
    assert!(text.replace(' ', "").contains("中途纠正"));
    assert!(text.replace(' ', "").contains("已暂停"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_pty_accepts_multiline_corrections_pauses_and_detaches_without_stopping_runner() {
    use loh::{
        config::Paths,
        runtime::ipc::{self, ClientMessage, Server},
        store::Store,
    };
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::{
        io::{Read, Write},
        sync::Arc,
        time::Duration,
    };
    use tokio::sync::{broadcast, mpsc};
    use tokio_util::sync::CancellationToken;
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().join("state"));
    paths.prepare().unwrap();
    let store = Arc::new(Store::open(&paths.database()).unwrap());
    let mut session = Session::new(root.path().to_owned(), "终端协议验收 fixture", 10000, 60);
    session.status = Status::Running;
    store
        .commit(
            &session.id,
            &session,
            EventData::Created {
                objective: session.goal.original.clone(),
                workspace: root.path().display().to_string(),
            },
        )
        .unwrap();
    let (control, _received) = mpsc::channel(8);
    let (events, _) = broadcast::channel(32);
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
    let serving = tokio::spawn(server.run(cancel.clone()));
    let pty = native_pty_system()
        .openpty(PtySize {
            rows: 28,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_loh"));
    command.args([
        "--home",
        paths.home.to_str().unwrap(),
        "attach",
        &session.id,
    ]);
    command.env("TERM", "xterm-256color");
    let mut child = pty.slave.spawn_command(command).unwrap();
    drop(pty.slave);
    let mut reader = pty.master.try_clone_reader().unwrap();
    let mut writer = pty.master.take_writer().unwrap();
    let (output, mut chunks) = mpsc::unbounded_channel();
    let reading = std::thread::spawn(move || {
        let mut bytes = [0; 4096];
        while let Ok(count) = reader.read(&mut bytes) {
            if count == 0 {
                break;
            }
            if output.send(bytes[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut screen = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !String::from_utf8_lossy(&screen).contains("Logit- Harness") {
            screen.extend(chunks.recv().await.unwrap());
        }
    })
    .await
    .unwrap();
    writer
        .write_all("\x1b[200~存储改成 SQLite\n保留界面\x1b[201~".as_bytes())
        .unwrap();
    writer.flush().unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        store.queued(&session.id, 0).unwrap().is_empty(),
        "pasting must not submit without Enter"
    );
    writer.write_all(b"\r").unwrap();
    writer.flush().unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let commands = store.queued(&session.id, 0).unwrap();
            if !commands.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    writer.write_all(b"\x03").unwrap();
    writer.flush().unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while store.queued(&session.id, 0).unwrap().len() < 2 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    writer.write_all(b"/detach\r").unwrap();
    writer.flush().unwrap();
    let exited = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(exited.success());
    let commands = store.queued(&session.id, 0).unwrap();
    assert_eq!(commands.len(), 2);
    let serialized = serde_json::to_string(&commands).unwrap();
    assert!(serialized.contains("存储改成 SQLite\\n保留界面"));
    assert!(serialized.contains("pause"));
    assert!(!serialized.contains("stop"));
    assert!(matches!(
        ipc::request(&socket, ClientMessage::Snapshot)
            .await
            .unwrap(),
        ServerMessage::Snapshot { .. }
    ));
    cancel.cancel();
    serving.await.unwrap().unwrap();
    drop(writer);
    drop(pty.master);
    reading.join().unwrap();
}

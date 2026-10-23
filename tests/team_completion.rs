use loh::{
    domain::{EventData, Status},
    runtime::state::Session,
    store::Store,
};

#[test]
fn completion_transaction_rejects_a_control_from_another_connection() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    let runner = Store::open(&path).unwrap();
    let client = Store::open(&path).unwrap();
    let mut session = Session::new(dir.path().into(), "build", 1000, 60);
    session.status = Status::Running;
    runner
        .commit(
            &session.id,
            &session,
            EventData::Diagnostic {
                message: "before".into(),
            },
        )
        .unwrap();
    let cursor = client
        .enqueue(&session.id, &serde_json::json!({"type":"pause"}))
        .unwrap();
    let mut completed = session.clone();
    completed.status = Status::Completed;
    let event = || EventData::Status {
        status: Status::Completed,
        reason: "verified".into(),
    };
    assert!(
        runner
            .commit_if_controls_current(&session.id, 0, &completed, event())
            .unwrap()
            .is_none()
    );
    assert_eq!(
        runner.load::<Session>(&session.id).unwrap().status,
        Status::Running
    );
    completed.cursor = cursor;
    assert!(
        runner
            .commit_if_controls_current(&session.id, cursor, &completed, event())
            .unwrap()
            .is_some()
    );
    let (saved, sequence) = runner.snapshot::<Session>(&session.id).unwrap();
    assert_eq!(saved.status, Status::Completed);
    assert_eq!(sequence, 2, "rejected completion must append no event");
}

#[test]
fn completion_waits_for_an_inflight_ipc_writer_then_rechecks_its_watermark() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let store = Store::open(&path).unwrap();
    let session = Session::new(dir.path().into(), "build", 1000, 60);
    let mut connection = rusqlite::Connection::open(&path).unwrap();
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    transaction
        .execute(
            "INSERT INTO controls(session,data) VALUES(?1,?2)",
            rusqlite::params![session.id, "{\"type\":\"pause\"}"],
        )
        .unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        send.send(()).unwrap();
        store
            .commit_if_controls_current(
                &session.id,
                0,
                &session,
                EventData::Status {
                    status: Status::Completed,
                    reason: "candidate".into(),
                },
            )
            .unwrap()
    });
    receive.recv().unwrap();
    transaction.commit().unwrap();
    assert!(writer.join().unwrap().is_none());
}

struct Greeting;
#[async_trait::async_trait]
impl loh::model::Provider for Greeting {
    async fn respond(
        &self,
        request: loh::model::Request,
        _: tokio_util::sync::CancellationToken,
        _: tokio::sync::mpsc::UnboundedSender<loh::model::ModelEvent>,
    ) -> anyhow::Result<loh::model::Response> {
        assert!(
            request.instructions.starts_with("ROLE: steering"),
            "greetings must dispatch no main, worker or verifier"
        );
        assert!(
            serde_json::to_string(&request.input)
                .unwrap()
                .contains("complete original objective")
        );
        Ok(loh::model::Response {
            output: vec![
                serde_json::json!({"type":"function_call","call_id":"greeting","name":"classify_input","arguments":serde_json::json!({"intent":"question","updates":{},"additions":[],"answer":"Hello!","reason":"greeting"}).to_string()}),
            ],
            usage: Default::default(),
            model: "offline".into(),
        })
    }
}

#[tokio::test]
async fn greeting_after_completion_has_no_engineering_dispatch_or_project_scan() {
    use loh::{
        runtime::{
            conversation::Mode,
            runner::{Runner, Settings},
            state::Control,
        },
        tools::{ToolHost, files::Workspace},
    };
    use std::sync::Arc;
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut s = Session::new(
        root.path().into(),
        "complete original objective",
        100_000,
        60,
    );
    s.interactive = true;
    s.status = Status::Completed;
    s.actors.get_mut("main").unwrap().phase =
        serde_json::from_value(serde_json::json!("done")).unwrap();
    let goal = s.goal.id.clone();
    let host = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(s.goal.clone())),
        state.path().join("logs"),
    ));
    std::fs::remove_dir(root.path()).unwrap();
    let (runner, control, _) = Runner::new(
        s,
        Arc::new(Store::open(&state.path().join("db")).unwrap()),
        Arc::new(Greeting),
        host,
        Settings {
            linger: false,
            max_output: 128,
            ..Settings::default()
        },
    );
    control
        .send(Control::Message {
            text: "hello, thanks".into(),
        })
        .await
        .unwrap();
    let s = tokio::time::timeout(std::time::Duration::from_secs(3), runner.run())
        .await
        .unwrap()
        .unwrap();
    assert!(s.goal.id.is_empty());
    assert_eq!(s.mode, Mode::Conversation);
    assert!(s.owner.is_none());
    assert_eq!(s.archives.len(), 1);
    assert_eq!(s.archives[0].goal.id, goal);
    assert!(s.actions.is_empty());
}

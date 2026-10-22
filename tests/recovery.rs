use loh::tools::{
    identity::Identity,
    process::{self, ProcessEvent, ProcessSpec},
};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

fn owner_path(workspace: &std::path::Path) -> std::path::PathBuf {
    let root = workspace.canonicalize().unwrap();
    let hash = blake3::hash(root.as_os_str().as_encoded_bytes());
    format!(
        "/tmp/loh-owner-{}-{}.lock",
        unsafe { libc::geteuid() },
        &hash.to_hex()[..24]
    )
    .into()
}

struct RecoveredProvider {
    writer: Option<u32>,
    calls: std::sync::atomic::AtomicUsize,
}
#[async_trait::async_trait]
impl loh::model::Provider for RecoveredProvider {
    async fn respond(
        &self,
        _: loh::model::Request,
        _: CancellationToken,
        _: tokio::sync::mpsc::UnboundedSender<loh::model::ModelEvent>,
    ) -> anyhow::Result<loh::model::Response> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Some(pid) = self.writer {
            anyhow::ensure!(
                Identity::capture(pid)?.is_none(),
                "old writer is still alive at engineering dispatch"
            );
        }
        Ok(loh::model::Response {
            output: vec![
                serde_json::json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"inspected after reconciliation"}]}),
            ],
            usage: Default::default(),
            model: "fixture".into(),
        })
    }
}

#[tokio::test]
async fn legacy_cross_session_owner_reaps_writer_before_new_engineering_dispatch() {
    use loh::{
        config::Paths,
        domain::EventData,
        runtime::{
            condition::Condition,
            runner::{Runner, Settings},
            state::Session,
        },
        store::Store,
        tools::{ToolHost, files::Workspace},
    };
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path().join("state"));
    paths.prepare().unwrap();
    let store = Arc::new(Store::open(&paths.database()).unwrap());
    let previous = Session::new(root.path().into(), "legacy writer", 100000, 60);
    store
        .commit(
            &previous.id,
            &previous,
            EventData::Diagnostic {
                message: "legacy fixture".into(),
            },
        )
        .unwrap();
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let writer = tokio::spawn(process::run(
        ProcessSpec {
            command: "printf started > marker; sleep 30; printf late >> marker".into(),
            cwd: root.path().into(),
            log: paths.logs(&previous.id).unwrap().join("writer.log"),
            timeout: Duration::from_secs(40),
            profile: None,
        },
        CancellationToken::new(),
        events,
    ));
    let pid = loop {
        if let ProcessEvent::Started { pid } = receiver.recv().await.unwrap() {
            break pid;
        }
    };
    tokio::time::timeout(Duration::from_secs(3), async {
        while !root.path().join("marker").exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    // The legacy daemon is gone: its raw lease remains, without a live flock.
    let lock = owner_path(root.path());
    std::fs::write(&lock, &previous.id).unwrap();
    let mut session = Session::new(root.path().into(), "new engineering session", 100000, 60);
    session.condition = Condition::Prompt;
    let provider = Arc::new(RecoveredProvider {
        writer: Some(pid),
        calls: 0.into(),
    });
    let tools = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        paths.logs(&session.id).unwrap(),
    ));
    let (runner, _, _) = Runner::new(
        session,
        store,
        provider.clone(),
        tools,
        Settings {
            linger: false,
            max_output: 1024,
            ..Settings::default()
        },
    );
    let result = tokio::time::timeout(Duration::from_secs(5), runner.run())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.status, loh::domain::Status::Completed);
    writer.await.unwrap().unwrap();
    assert_eq!(provider.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::read_to_string(root.path().join("marker")).unwrap(),
        "started"
    );
    assert!(std::fs::read_to_string(&lock).unwrap().is_empty());
    std::fs::remove_file(lock).unwrap();
}

#[tokio::test]
async fn unknown_legacy_owner_is_preserved_and_blocks_engineering() {
    use loh::{
        runtime::{
            runner::{Runner, Settings},
            state::Session,
        },
        store::Store,
        tools::{ToolHost, files::Workspace},
    };
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let owner = "0123456789abcdef";
    let lock = owner_path(root.path());
    std::fs::write(&lock, owner).unwrap();
    let session = Session::new(root.path().into(), "new work", 100000, 60);
    let provider = Arc::new(RecoveredProvider {
        writer: None,
        calls: 0.into(),
    });
    let tools = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        state.path().join("logs"),
    ));
    let (runner, _, _) = Runner::new(
        session,
        Arc::new(Store::open(&state.path().join("db")).unwrap()),
        provider.clone(),
        tools,
        Settings {
            linger: false,
            ..Settings::default()
        },
    );
    let error = runner.run().await.unwrap_err().to_string();
    assert!(error.contains(owner) && error.contains("original --home"));
    assert_eq!(provider.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(std::fs::read_to_string(&lock).unwrap(), owner);
    std::fs::remove_file(lock).unwrap();
}

#[tokio::test]
async fn recorded_process_identity_stops_only_its_own_process_group() {
    let dir = tempfile::tempdir().unwrap();
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(process::run(
        ProcessSpec {
            command: "sleep 30; touch late".into(),
            cwd: dir.path().into(),
            log: dir.path().join("command.log"),
            timeout: Duration::from_secs(40),
            profile: None,
        },
        CancellationToken::new(),
        events,
    ));
    let pid = loop {
        if let ProcessEvent::Started { pid } = receiver.recv().await.unwrap() {
            break pid;
        }
    };
    let identity = Identity::capture(pid).unwrap().unwrap();
    let mut wrong = identity.clone();
    wrong.started += 1;
    assert!(wrong.stop_group().await.is_err());
    assert_eq!(Identity::capture(pid).unwrap().as_ref(), Some(&identity));
    identity.stop_group().await.unwrap();
    task.await.unwrap().unwrap();
    assert!(!dir.path().join("late").exists());
    assert!(Identity::capture(pid).unwrap().is_none());
}

#[tokio::test]
async fn recovery_observes_files_and_never_replays_an_uncertain_write() {
    use loh::{
        runtime::{recovery, state::Session},
        store::Store,
    };
    let dir = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(&logs.path().join("state.sqlite")).unwrap());
    let mut session = Session::new(dir.path().into(), "Write once", 100000, 60);
    let reservation = session.budget.reserve(1000).unwrap();
    let mut state = serde_json::to_value(&session).unwrap();
    state["actors"]["main"]["busy"] = 1.into();
    state["actors"]["main"]["phase"] = "awaiting".into();
    state["actions"] = serde_json::json!({"action": {
        "id":"action", "actor":"main", "running":true, "pid":null,
        "pending":{"call":{"id":"provider-call","name":"shell","arguments":{"command":"printf x >> once"}},"scope":{"requirements":{"r1":1},"observed":{},"readonly":false,"approvals":[]}}
    }});
    session = serde_json::from_value(state).unwrap();
    std::fs::write(dir.path().join("once"), "user edit after interrupted x").unwrap();
    recovery::reconcile(&mut session, &store, logs.path())
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("once")).unwrap(),
        "user edit after interrupted x"
    );
    assert!(session.actions.is_empty());
    assert!(!session.budget.reservations.contains_key(&reservation));
    assert_eq!(session.budget.uncertain, 1000);
    let history = serde_json::to_string(&session).unwrap();
    assert!(history.contains("not replayed"));
    let saved: Session = store.load(&session.id).unwrap();
    assert!(saved.actions.is_empty());
}
#[tokio::test]
async fn recovery_closes_transcript_calls_that_were_never_dispatched() {
    use loh::{
        runtime::{recovery, state::Session},
        store::Store,
    };
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let session = Session::new(root.path().into(), "work", 100000, 60);
    let mut encoded = serde_json::to_value(session).unwrap();
    encoded["actors"]["main"]["phase"] = "busy".into();
    encoded["actors"]["main"]["context"]["items"].as_array_mut().unwrap().push(serde_json::json!({"type":"function_call","call_id":"unfinished","name":"shell","arguments":"{}"}));
    let mut session: Session = serde_json::from_value(encoded).unwrap();
    let store = Store::open(&state.path().join("db")).unwrap();
    recovery::reconcile(&mut session, &store, state.path())
        .await
        .unwrap();
    let encoded = serde_json::to_value(session).unwrap();
    assert_eq!(encoded["actors"]["main"]["phase"], "ready");
    let items = encoded["actors"]["main"]["context"]["items"]
        .as_array()
        .unwrap();
    assert!(
        items
            .iter()
            .any(|v| v["type"] == "function_call_output" && v["call_id"] == "unfinished")
    );
}

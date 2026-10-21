use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use weft::tools::{
    identity::Identity,
    process::{self, ProcessEvent, ProcessSpec},
};

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
    use weft::{
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
    use weft::{
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

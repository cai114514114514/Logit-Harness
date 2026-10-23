use loh::{
    domain::EventData,
    runtime::{
        graph::{Step, StepStatus},
        state::Session,
        team::AssignmentStatus,
    },
    store::Store,
    tools::{
        identity::Identity,
        process::{self, ProcessEvent, ProcessSpec},
    },
};
use std::{collections::BTreeSet, time::Duration};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn recovery_settles_old_member_writer_preserves_pause_and_does_not_replay() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let logs = state.path().join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let store = Store::open(&state.path().join("db")).unwrap();
    let mut s = Session::new(root.path().into(), "build", 10000, 60);
    s.graph
        .install(
            ["a", "b"]
                .into_iter()
                .map(|id| Step {
                    id: id.into(),
                    description: id.into(),
                    requirements: BTreeSet::from(["r1".into()]),
                    dependencies: BTreeSet::new(),
                    files: BTreeSet::from(["marker".into()]),
                    revision: 1,
                    status: StepStatus::Pending,
                })
                .collect(),
            &s.goal,
        )
        .unwrap();
    let a = s.assign_member(Some("Ada"), "a", "old").unwrap();
    let target = s.member_target("Ada").unwrap();
    s.team
        .send("input", "user", target, "inspect before writing", true)
        .unwrap();
    s.team.members.get_mut(&a.member).unwrap().paused = true;
    s.team.members.get_mut(&a.member).unwrap().interrupted = true;
    let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let process = tokio::spawn(process::run(
        ProcessSpec {
            command: "printf original > marker; sleep 30; printf late >> marker".into(),
            cwd: root.path().into(),
            log: logs.join("old.log"),
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
    let mut scope = s.graph.scope("a", &s.goal).unwrap();
    scope.readonly = false;
    s.actions.insert("old".into(),serde_json::from_value(serde_json::json!({"id":"old","actor":a.actor,"running":true,"pid":pid,"approval":null,"pending":{"call":{"id":"write","name":"shell","arguments":{}},"scope":scope}})).unwrap());
    s.finish_assignment(&a.actor, AssignmentStatus::Invalidated);
    store
        .commit(
            &s.id,
            &s,
            EventData::Diagnostic {
                message: "crashed during cancellation".into(),
            },
        )
        .unwrap();
    assert!(s.assign_member(Some("Ada"), "b", "new").is_err());
    loh::runtime::recovery::reconcile(&mut s, &store, &logs)
        .await
        .unwrap();
    process.await.unwrap().unwrap();
    assert!(Identity::capture(pid).unwrap().is_none());
    assert_eq!(
        std::fs::read_to_string(root.path().join("marker")).unwrap(),
        "original"
    );
    assert!(s.team.members[&a.member].paused);
    assert!(!s.team.mail[0].delivered);
    assert!(s.settled_actions.contains_key("old"));
    assert!(s.assign_member(Some("Ada"), "b", "new").is_ok());
}

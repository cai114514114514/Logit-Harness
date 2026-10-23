use loh::runtime::graph::{Step, StepStatus};
use loh::runtime::{
    state::Session,
    team::{AssignmentStatus, MemberControl, Target},
};
use std::collections::BTreeSet;

fn session() -> Session {
    let mut session = Session::new(std::env::temp_dir(), "build", 100_000, 60);
    session
        .graph
        .install(
            ["a", "b"]
                .into_iter()
                .map(|id| Step {
                    id: id.into(),
                    description: id.into(),
                    requirements: BTreeSet::from(["r1".into()]),
                    dependencies: BTreeSet::new(),
                    files: BTreeSet::from([format!("{id}.txt")]),
                    revision: 1,
                    status: StepStatus::Pending,
                })
                .collect(),
            &session.goal,
        )
        .unwrap();
    session
}

#[test]
fn member_identity_survives_fresh_attempts_and_old_targets_fail() {
    let mut s = session();
    let first = s.assign_member(Some("Ada"), "a", "first").unwrap();
    let target = s.member_target("Ada").unwrap();
    let old_actor = first.actor.clone();
    s.finish_assignment(&old_actor, AssignmentStatus::Finished);
    let second = s.assign_member(Some("Ada"), "b", "second").unwrap();
    assert_eq!(first.member, second.member);
    assert_ne!(old_actor, second.actor);
    assert!(s.team.validate(&target).is_err());
    assert!(s.actors[&old_actor].retired);
}

#[test]
fn live_assignment_has_one_owner_even_when_paused_or_waiting() {
    let mut s = session();
    let a = s.assign_member(Some("Ada"), "a", "first").unwrap();
    s.team.members.get_mut(&a.member).unwrap().paused = true;
    assert!(s.assign_member(Some("Bob"), "a", "duplicate").is_err());
    assert!(s.assign_member(Some("Ada"), "b", "steal").is_err());
}

#[test]
fn mail_is_addressed_deduplicated_and_breaks_wait_cycles() {
    let mut s = session();
    let a = s.assign_member(Some("Ada"), "a", "first").unwrap();
    let b = s.assign_member(Some("Bob"), "b", "second").unwrap();
    s.team.wait(&a.member, &b.member).unwrap();
    s.team.wait(&b.member, &a.member).unwrap();
    assert!(!s.team.wait_problem().is_empty());
    let target = s.member_target("Ada").unwrap();
    s.team
        .send("mail-1", &b.member, target.clone(), "answer", false)
        .unwrap();
    s.team
        .send("mail-1", &b.member, target, "answer", false)
        .unwrap();
    assert_eq!(s.team.mail.len(), 1);
    assert!(s.team.members[&a.member].waiting.is_none());
    assert!(s.team.wait_problem().is_empty());
    assert_eq!(s.team.mail[0].from, b.member);
}

#[test]
fn legacy_sessions_get_empty_team_without_losing_main_target() {
    let s = session();
    let mut value = serde_json::to_value(s).unwrap();
    value.as_object_mut().unwrap().remove("team");
    let s: Session = serde_json::from_value(value).unwrap();
    assert_eq!(s.team.limit, 2);
    assert_eq!(s.member_target("main").unwrap(), Target::Main);
    assert!(
        serde_json::from_value::<MemberControl>(serde_json::json!({"type":"budget", "tokens":1}))
            .is_err()
    );
}

#[test]
fn legacy_workers_keep_immutable_assignment_ownership_after_reload_and_finish() {
    let mut original = session();
    let active = original
        .assign_member(Some("Ada"), "a", "legacy active")
        .unwrap();
    let finished = original
        .assign_member(Some("Bob"), "b", "legacy done")
        .unwrap();
    original.finish_assignment(&finished.actor, AssignmentStatus::Finished);
    let mut value = serde_json::to_value(original).unwrap();
    value.as_object_mut().unwrap().remove("team");
    for actor in value["actors"].as_object_mut().unwrap().values_mut() {
        actor.as_object_mut().unwrap().remove("target");
    }
    let mut restored: Session = serde_json::from_value(value).unwrap();
    restored.normalize_team();
    for actor in [&active.actor, &finished.actor] {
        assert_eq!(restored.actor_bindings()[actor].target.member_id(), actor);
        assert!(
            matches!(&restored.actors[actor].target, Some(Target::Member { assignment: Some(reference), .. }) if reference.id == format!("legacy-{actor}"))
        );
    }
    assert!(restored.team.members[&finished.actor].assignment.is_none());
    assert!(restored.actors[&finished.actor].retired);
    let target = restored.actors[&active.actor].target.clone();
    restored.finish_assignment(&active.actor, AssignmentStatus::Finished);
    let mut reloaded: Session =
        serde_json::from_value(serde_json::to_value(restored).unwrap()).unwrap();
    reloaded.normalize_team();
    assert_eq!(reloaded.actors[&active.actor].target, target);
    assert!(reloaded.actors[&active.actor].retired);
}

#[test]
fn legacy_archive_projects_finished_worker_without_reviving_it() {
    use loh::runtime::conversation::Archive;
    let mut old = session();
    let assignment = old
        .assign_member(Some("Ada"), "a", "archived task")
        .unwrap();
    old.finish_assignment(&assignment.actor, AssignmentStatus::Finished);
    let goal = old.goal.id.clone();
    let archive = Archive {
        team: old.team,
        goal: old.goal,
        status: loh::domain::Status::Completed,
        graph: old.graph,
        actors: old.actors,
        evidence: vec![],
        actions: Default::default(),
        report: None,
    };
    let mut current = session();
    current.archives.push(archive);
    let mut saved = serde_json::to_value(current).unwrap();
    saved["archives"][0].as_object_mut().unwrap().remove("team");
    for actor in saved["archives"][0]["actors"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        actor.as_object_mut().unwrap().remove("target");
    }
    let mut restored: Session = serde_json::from_value(saved).unwrap();
    restored.normalize_team();
    let origin = restored.resolve_view(&assignment.actor).unwrap();
    assert!(origin.readonly);
    assert_eq!(origin.goal, Some(goal));
    assert!(restored.archives[0].actors[&assignment.actor].retired);
    assert!(
        restored.archives[0].team.members[&assignment.actor]
            .assignment
            .is_none()
    );
    assert!(
        matches!(&restored.actor_bindings()[&assignment.actor].target, Target::Member { assignment: Some(reference), .. } if reference.id == format!("legacy-{}", assignment.actor))
    );
    assert!(restored.team.members.is_empty());
}

use loh::{
    domain::{EventData, Status, Usage},
    model::{ModelEvent, Provider, Request as ModelRequest, Response},
    runtime::{
        runner::{Runner, Settings},
        state::Control,
        team::{ReceiptStatus, Request},
    },
    store::Store,
    tools::{ToolHost, files::Workspace},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;

struct Held {
    workers: AtomicUsize,
    questions: AtomicUsize,
    entered: Semaphore,
}
#[async_trait::async_trait]
impl Provider for Held {
    async fn respond(
        &self,
        request: ModelRequest,
        cancel: CancellationToken,
        _: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        if request.instructions.starts_with("ROLE: question")
            || request.instructions.starts_with("ROLE: compact")
        {
            assert!(!request.tools.iter().any(|t| matches!(
                t["name"].as_str(),
                Some("delegate" | "propose_patch" | "apply_patch")
            )));
            self.questions.fetch_add(1, Ordering::SeqCst);
            self.entered.add_permits(1);
            return Ok(Response {
                output: vec![
                    serde_json::json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"read-only answer"}]}),
                ],
                usage: Usage::default(),
                model: "fixture".into(),
            });
        }
        if request.instructions.starts_with("ROLE: worker") {
            self.workers.fetch_add(1, Ordering::SeqCst);
            self.entered.add_permits(1);
        }
        cancel.cancelled().await;
        anyhow::bail!("fixture cancelled")
    }
}

fn direct(target: Target, id: &str, action: MemberControl) -> Control {
    Control::Directed {
        request: Request {
            id: id.into(),
            client_id: "fixture".into(),
            target,
            action,
        },
    }
}

async fn receipt(
    events: &mut tokio::sync::broadcast::Receiver<loh::domain::SessionEvent>,
    id: &str,
) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let EventData::RequestReceipt { receipt, .. } = events.recv().await.unwrap().data
                && receipt.request.client_id == "fixture"
                && receipt.request.id == id
                && receipt.status != ReceiptStatus::Pending
            {
                break;
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn local_pause_survives_global_resume_and_stop_releases_only_after_settlement() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut s = session();
    s.workspace = root.path().into();
    let a = s.assign_member(Some("Ada"), "a", "first").unwrap();
    let target = s.member_target("Ada").unwrap();
    let session_id = s.id.clone();
    let provider = Arc::new(Held {
        workers: 0.into(),
        questions: 0.into(),
        entered: Semaphore::new(0),
    });
    let host = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(s.goal.clone())),
        state.path().join("logs"),
    ));
    let store = Arc::new(Store::open(&state.path().join("db")).unwrap());
    let (runner, control, mut events) = Runner::new(
        s,
        store.clone(),
        provider.clone(),
        host,
        Settings {
            max_output: 128,
            ..Settings::default()
        },
    );
    control
        .send(direct(target.clone(), "pause", MemberControl::Pause))
        .await
        .unwrap();
    control.send(Control::Pause).await.unwrap();
    control.send(Control::Resume).await.unwrap();
    let run = tokio::spawn(runner.run());
    receipt(&mut events, "pause").await;
    control
        .send(direct(
            Target::Main,
            "barrier",
            MemberControl::Limit { limit: 2 },
        ))
        .await
        .unwrap();
    receipt(&mut events, "barrier").await;
    assert_eq!(provider.workers.load(Ordering::SeqCst), 0);
    assert!(store.load::<Session>(&session_id).unwrap().team.members[&a.member].paused);
    control
        .send(direct(target.clone(), "resume", MemberControl::Resume))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), provider.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    control
        .send(direct(target.clone(), "stop", MemberControl::Stop))
        .await
        .unwrap();
    receipt(&mut events, "stop").await;
    control.send(Control::Stop).await.unwrap();
    let s = tokio::time::timeout(Duration::from_secs(3), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(s.team.assignments[&a.id].status, AssignmentStatus::Stopped);
    assert!(s.team.members[&a.member].assignment.is_none());
    assert!(s.actors[&a.actor].retired);
    assert_eq!(s.actors[&a.actor].busy, 0);
}

#[tokio::test]
async fn idle_side_question_uses_fresh_readonly_actor_and_stale_control_keeps_receipt() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut s = session();
    s.workspace = root.path().into();
    let assignment = s.assign_member(Some("Ada"), "a", "first").unwrap();
    let stale = s.member_target("Ada").unwrap();
    s.finish_assignment(&assignment.actor, AssignmentStatus::Finished);
    let target = s.member_target("Ada").unwrap();
    s.status = Status::Paused;
    let provider = Arc::new(Held {
        workers: 0.into(),
        questions: 0.into(),
        entered: Semaphore::new(0),
    });
    let host = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(s.goal.clone())),
        state.path().join("logs"),
    ));
    let store = Arc::new(Store::open(&state.path().join("db")).unwrap());
    let (runner, control, mut events) = Runner::new(
        s,
        store,
        provider.clone(),
        host,
        Settings {
            max_output: 128,
            ..Settings::default()
        },
    );
    control
        .send(direct(stale, "stale", MemberControl::Resume))
        .await
        .unwrap();
    let question = direct(
        target,
        "question",
        MemberControl::Side {
            text: "What did you find?".into(),
        },
    );
    control.send(question.clone()).await.unwrap();
    control.send(question).await.unwrap();
    let run = tokio::spawn(runner.run());
    receipt(&mut events, "question").await;
    tokio::time::timeout(Duration::from_secs(3), provider.entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    control.send(Control::Stop).await.unwrap();
    let s = tokio::time::timeout(Duration::from_secs(3), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        s.team.receipts["[\"fixture\",\"stale\"]"].status,
        ReceiptStatus::Failed
    );
    assert!(
        s.team.receipts["[\"fixture\",\"stale\"]"]
            .error
            .as_ref()
            .unwrap()
            .contains("stale")
    );
    assert_eq!(provider.questions.load(Ordering::SeqCst), 1);
    assert_eq!(provider.workers.load(Ordering::SeqCst), 0);
    let member = &s.team.members[&assignment.member];
    assert_eq!(member.attempts.len(), 2);
    assert_ne!(member.actor.as_ref().unwrap(), &assignment.actor);
    assert!(member.assignment.is_none());
    assert!(s.inbox.is_empty());
}

#[path = "fixtures/state.rs"]
mod fixture_state;
struct Exchange {
    first: String,
    calls: std::sync::Mutex<std::collections::BTreeMap<String, usize>>,
}
#[async_trait::async_trait]
impl Provider for Exchange {
    async fn respond(
        &self,
        request: ModelRequest,
        cancel: CancellationToken,
        _: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        if !request.instructions.starts_with("ROLE: worker") {
            cancel.cancelled().await;
            anyhow::bail!("fixture main cancelled")
        }
        let task = fixture_state::current(&request)["task"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut calls = self.calls.lock().unwrap();
        let count = calls.entry(task.clone()).or_default();
        *count += 1;
        let output = if *count == 1 {
            let peer = if task == "Ada" { "Bob" } else { "Ada" };
            let (name, args) = if task == self.first {
                ("wait_message", serde_json::json!({"from":peer}))
            } else {
                (
                    "send_message",
                    serde_json::json!({"to":peer,"text":"peer result"}),
                )
            };
            serde_json::json!({"type":"function_call","call_id":format!("{task}-mail"),"name":name,"arguments":args.to_string()})
        } else {
            if task == self.first {
                assert!(
                    request
                        .input
                        .iter()
                        .any(|item| item.to_string().contains("peer result")),
                    "waiting member must receive addressed data before waking"
                );
            }
            serde_json::json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"scoped result"}]})
        };
        Ok(Response {
            output: vec![output],
            usage: Usage::default(),
            model: "fixture".into(),
        })
    }
}

#[tokio::test]
async fn peer_wait_releases_capacity_and_mail_wakes_only_its_recipient() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut s = session();
    s.workspace = root.path().into();
    s.team.limit = 1;
    let a = s.assign_member(Some("Ada"), "a", "Ada").unwrap();
    let b = s.assign_member(Some("Bob"), "b", "Bob").unwrap();
    let first = if a.actor < b.actor { "Ada" } else { "Bob" };
    let provider = Arc::new(Exchange {
        first: first.into(),
        calls: Default::default(),
    });
    let host = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(s.goal.clone())),
        state.path().join("logs"),
    ));
    let store = Arc::new(Store::open(&state.path().join("db")).unwrap());
    let (runner, control, mut events) = Runner::new(
        s,
        store,
        provider.clone(),
        host,
        Settings {
            max_output: 128,
            ..Settings::default()
        },
    );
    let run = tokio::spawn(runner.run());
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut finished = 0;
        while finished < 2 {
            if let EventData::Worker { status, .. } = events.recv().await.unwrap().data
                && status == "finished"
            {
                finished += 1;
            }
        }
    })
    .await
    .unwrap();
    control.send(Control::Stop).await.unwrap();
    let s = tokio::time::timeout(Duration::from_secs(3), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(s.team.mail.len(), 1);
    assert!(s.team.mail[0].delivered);
    assert!(s.team.members.values().all(|m| m.assignment.is_none()));
    assert_eq!(
        provider.calls.lock().unwrap().values().sum::<usize>(),
        4,
        "waits must not poll the model"
    );
}

#[tokio::test]
async fn compact_targets_idle_member_without_reviving_its_finished_assignment() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut s = session();
    s.workspace = root.path().into();
    let assignment = s.assign_member(Some("Ada"), "a", "first").unwrap();
    s.finish_assignment(&assignment.actor, AssignmentStatus::Finished);
    // Maintenance may summarize an old attempt after its plan became obsolete.
    s.graph.invalidate(&BTreeSet::from(["r1".into()]));
    s.status = Status::Paused;
    s.team.limit = 0;
    let target = s.member_target("Ada").unwrap();
    let provider = Arc::new(Held {
        workers: 0.into(),
        questions: 0.into(),
        entered: Semaphore::new(0),
    });
    let host = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(s.goal.clone())),
        state.path().join("logs"),
    ));
    let store = Arc::new(Store::open(&state.path().join("db")).unwrap());
    let (runner, control, mut events) = Runner::new(
        s,
        store,
        provider.clone(),
        host,
        Settings {
            max_output: 128,
            ..Settings::default()
        },
    );
    control
        .send(direct(target, "compact", MemberControl::Compact))
        .await
        .unwrap();
    let run = tokio::spawn(runner.run());
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let EventData::Diagnostic { message } = events.recv().await.unwrap().data
                && message.contains("context compacted")
            {
                break;
            }
        }
    })
    .await
    .unwrap();
    control.send(Control::Stop).await.unwrap();
    let s = tokio::time::timeout(Duration::from_secs(3), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(s.actors[&assignment.actor].retired);
    assert_eq!(
        s.team.assignments[&assignment.id].status,
        AssignmentStatus::Finished
    );
    assert!(s.team.members[&assignment.member].assignment.is_none());
    assert_eq!(provider.workers.load(Ordering::SeqCst), 0);
    assert_eq!(
        s.actors[&assignment.actor].context.items[0]["content"],
        "read-only answer"
    );
}

struct WaitForStop {
    main_calls: AtomicUsize,
    resumed: Semaphore,
}

#[async_trait::async_trait]
impl Provider for WaitForStop {
    async fn respond(
        &self,
        request: ModelRequest,
        cancel: CancellationToken,
        _: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        if request.instructions.starts_with("ROLE: main") {
            if self.main_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Ok(Response {
                    output: vec![
                        serde_json::json!({"type":"function_call","call_id":"wait-for-member","name":"wait_workers","arguments":"{}"}),
                    ],
                    usage: Usage::default(),
                    model: "fixture".into(),
                });
            }
            assert!(
                request
                    .input
                    .iter()
                    .any(|item| item.to_string().contains("stopped")),
                "main needs an actionable reason for the worker disappearing"
            );
            self.resumed.add_permits(1);
        }
        cancel.cancelled().await;
        anyhow::bail!("fixture cancelled")
    }
}

#[tokio::test]
async fn stopping_last_member_wakes_main_after_wait_workers() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut s = session();
    s.workspace = root.path().into();
    let assignment = s.assign_member(Some("Ada"), "a", "first").unwrap();
    let target = s.member_target("Ada").unwrap();
    let id = s.id.clone();
    let provider = Arc::new(WaitForStop {
        main_calls: 0.into(),
        resumed: Semaphore::new(0),
    });
    let host = Arc::new(ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(s.goal.clone())),
        state.path().join("logs"),
    ));
    let store = Arc::new(Store::open(&state.path().join("db")).unwrap());
    let (runner, control, mut events) = Runner::new(
        s,
        store.clone(),
        provider.clone(),
        host,
        Settings {
            max_output: 128,
            ..Settings::default()
        },
    );
    let run = tokio::spawn(runner.run());
    // Persist observations at explicit command boundaries, never infer blocking
    // from a delay or from the model merely returning a wait tool call.
    tokio::time::timeout(Duration::from_secs(3), async {
        for sequence in 0.. {
            let request = format!("snapshot-{sequence}");
            control
                .send(direct(
                    Target::Main,
                    &request,
                    MemberControl::Limit { limit: 2 },
                ))
                .await
                .unwrap();
            receipt(&mut events, &request).await;
            let snapshot = serde_json::to_value(store.load::<Session>(&id).unwrap()).unwrap();
            if snapshot["actors"]["main"]["phase"] == "blocked" {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    control
        .send(direct(target, "stop-member", MemberControl::Stop))
        .await
        .unwrap();
    let woke = tokio::time::timeout(Duration::from_secs(3), provider.resumed.acquire()).await;
    // Always settle the runner, including RED, so a failed assertion cannot
    // leave an unjoined fixture task behind.
    control.send(Control::Stop).await.unwrap();
    let s = tokio::time::timeout(Duration::from_secs(3), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        woke.is_ok(),
        "main stayed blocked after its only member stopped"
    );
    assert_eq!(
        s.team.assignments[&assignment.id].status,
        AssignmentStatus::Stopped
    );
}

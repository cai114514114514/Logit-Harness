use super::*;
use crate::{
    domain::{EventData, SessionEvent},
    runtime::{
        team::{AssignmentRef, Target},
        view::ViewTarget,
    },
};

fn member(version: u64) -> ViewTarget {
    ViewTarget {
        target: Target::Member {
            id: "member-a".into(),
            assignment: Some(AssignmentRef {
                id: "task-a".into(),
                version,
            }),
        },
        goal: Some("goal".into()),
        readonly: false,
    }
}

#[test]
fn terminal_receipts_are_quiet_after_applied_even_when_ack_is_late() {
    use crate::runtime::team::{MemberControl, Receipt, ReceiptStatus, Request};
    let request = Request {
        id: "ui-request".into(),
        client_id: "ui-client".into(),
        target: member(1).target,
        action: MemberControl::Message {
            text: "unique instruction".into(),
        },
    };
    let mut p = Transcript::default();
    p.focus(member(1));
    p.accepted(&member(1), &request);
    assert!(
        p.pending_lines(100)
            .iter()
            .any(|r| r.to_string().contains("等待处理"))
    );
    assert!(
        !p.drain_settled(100)
            .iter()
            .any(|r| r.to_string().contains("已接收"))
    );
    let pending = Receipt {
        request: request.clone(),
        status: ReceiptStatus::Pending,
        applying: false,
        error: None,
        pending_input: None,
    };
    p.receive(&event(
        1,
        EventData::RequestReceipt {
            goal: "goal".into(),
            receipt: pending.clone(),
        },
    ));
    let input = p
        .drain_settled(100)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(input.matches("unique instruction").count(), 1);
    p.receive(&event(
        2,
        EventData::RequestReceipt {
            goal: "goal".into(),
            receipt: Receipt {
                status: ReceiptStatus::Applied,
                ..pending.clone()
            },
        },
    ));
    p.accepted(&member(1), &request);
    assert!(p.drain_settled(100).is_empty());
    assert!(p.pending_lines(100).is_empty());
    let full = p
        .lines(100, true)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(full.contains("已应用") && full.contains("ui-request"));
}

#[test]
fn pending_delivery_survives_long_history_and_stays_in_its_view() {
    use crate::runtime::team::{MemberControl, Request};
    let mut p = Transcript::default();
    p.focus(member(1));
    p.accepted(
        &member(1),
        &Request {
            id: "pending".into(),
            client_id: "client".into(),
            target: member(1).target,
            action: MemberControl::Message {
                text: "waiting instruction".into(),
            },
        },
    );
    for i in 1..=400 {
        p.receive(&event(
            i,
            EventData::Diagnostic {
                message: format!("notice {i}"),
            },
        ));
    }
    assert!(
        p.pending_lines(80)
            .iter()
            .any(|line| line.to_string().contains("waiting instruction"))
    );
    p.focus(ViewTarget::main());
    assert!(p.pending_lines(80).is_empty());
    p.focus(member(2));
    assert!(
        p.pending_lines(80)
            .iter()
            .any(|line| line.to_string().contains("旧任务"))
    );
}
fn event(sequence: u64, data: EventData) -> SessionEvent {
    SessionEvent {
        sequence,
        session: "fixture".into(),
        timestamp: "2026-10-23T00:00:00Z".into(),
        data,
    }
}
#[test]
fn ownership_precedes_first_output_and_filtered_events_advance_cursor() {
    let mut transcript = Transcript::default();
    transcript.focus(ViewTarget::main());
    transcript.receive(&event(
        1,
        EventData::ActorBound {
            actor: "worker-a".into(),
            target: member(1).target,
            goal: Some("goal".into()),
        },
    ));
    transcript.receive(&event(
        2,
        EventData::Identity {
            actor: "worker-a".into(),
            response: "response-a".into(),
            turn: "turn".into(),
            goal: Some("goal".into()),
            action: None,
        },
    ));
    transcript.receive(&event(
        3,
        EventData::Message {
            actor: "worker-a".into(),
            text: "private member detail".into(),
        },
    ));
    assert_eq!(transcript.sequence(), 3);
    assert!(
        !transcript
            .lines(100, true)
            .iter()
            .any(|l| l.to_string().contains("private member detail"))
    );
    transcript.focus(member(1));
    assert!(
        transcript
            .lines(100, true)
            .iter()
            .any(|l| l.to_string().contains("private member detail"))
    );
    transcript.receive(&event(
        4,
        EventData::Message {
            actor: "main".into(),
            text: "main detail".into(),
        },
    ));
    assert_eq!(transcript.sequence(), 4);
    assert!(
        !transcript
            .lines(100, true)
            .iter()
            .any(|l| l.to_string().contains("main detail"))
    );
}
#[test]
fn draft_binds_on_first_edit_and_survives_assignment_and_focus_changes() {
    let mut screen = Screen::default();
    screen.focus(member(1));
    screen.input.insert("旧任务中文\n第二行");
    screen.bind_draft();
    screen.view = member(2);
    assert_eq!(screen.draft_view(), member(1));
    assert!(screen.stale_draft());
    screen.focus(ViewTarget::main());
    screen.input.insert("main draft");
    screen.bind_draft();
    screen.focus(member(2));
    assert_eq!(screen.input.text(), "旧任务中文\n第二行");
    assert_eq!(screen.draft_view(), member(1));
    screen.input.take();
    screen.bind_draft();
    screen.input.insert("new intent");
    screen.bind_draft();
    assert_eq!(screen.draft_view(), member(2));
}
#[test]
fn late_failure_returns_to_origin_and_local_pause_is_addressed() {
    let mut screen = Screen::default();
    screen.focus(member(1));
    let revision = screen.input.revision();
    screen.focus(ViewTarget::main());
    screen.input.insert("main stays");
    screen.restore_origin(&member(1), "member request".into(), revision);
    assert_eq!(screen.input.text(), "main stays");
    screen.focus(member(1));
    assert_eq!(screen.input.text(), "member request");
    let control = screen.pause();
    assert!(
        matches!(control, crate::runtime::state::Control::Directed { request } if request.target == member(1).target)
    );
}

#[test]
fn mail_receipt_updates_append_input_once_and_expose_uncertain_recovery() {
    use crate::runtime::team::{Mail, MemberControl, Receipt, ReceiptStatus, Request};
    let mut transcript = Transcript::default();
    transcript.focus(member(1));
    let request = Request {
        id: "request".into(),
        client_id: "client".into(),
        target: member(1).target,
        action: MemberControl::Message {
            text: "保留 '$literal'\nsecond line".into(),
        },
    };
    let mut receipt = Receipt {
        applying: false,
        request: request.clone(),
        status: ReceiptStatus::Pending,
        error: None,
        pending_input: None,
    };
    transcript.receive(&event(
        1,
        EventData::RequestReceipt {
            goal: "goal".into(),
            receipt: receipt.clone(),
        },
    ));
    transcript.drain_settled(100);
    transcript.receive(&event(
        2,
        EventData::Mail {
            goal: "goal".into(),
            mail: Mail {
                id: request.mail_id(),
                from: "user".into(),
                target: request.target.clone(),
                text: "保留 '$literal'\nsecond line".into(),
                user: true,
                delivered: false,
                error: None,
            },
        },
    ));
    receipt.status = ReceiptStatus::Failed;
    receipt.error = Some(format!(
        "partially applied; unresolved review: /answer {} TEXT",
        request.recovery_id()
    ));
    transcript.receive(&event(
        3,
        EventData::RequestReceipt {
            goal: "goal".into(),
            receipt: receipt.clone(),
        },
    ));
    assert_eq!(
        transcript
            .entries()
            .iter()
            .filter(|e| e.kind == super::transcript::Kind::User)
            .count(),
        1
    );
    let text = transcript
        .drain_settled(200)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("应用失败")
            && text.contains("partially applied")
            && text.contains("保留 '$literal'")
    );
    assert!(text.contains("/agent main") && text.contains(&request.recovery_id()));
    transcript.receive(&event(
        4,
        EventData::RequestReceipt {
            goal: "goal".into(),
            receipt,
        },
    ));
    assert!(
        transcript.drain_settled(200).is_empty(),
        "identical update replayed a receipt"
    );
}

#[test]
fn scoped_tool_stream_and_assignment_boundaries_keep_full_history() {
    let mut transcript = Transcript::default();
    transcript.focus(member(1));
    for (sequence, data) in [
        EventData::ActorBound {
            actor: "worker-a".into(),
            target: member(1).target,
            goal: Some("goal".into()),
        },
        EventData::Identity {
            actor: "worker-a".into(),
            response: "response".into(),
            turn: "turn".into(),
            goal: Some("goal".into()),
            action: Some("tool".into()),
        },
        EventData::ActionStarted {
            id: "tool".into(),
            tool: "shell".into(),
            arguments: serde_json::json!({"command":"echo own"}),
        },
        EventData::Delta {
            actor: "tool".into(),
            text: "own-stream\n".into(),
        },
        EventData::ActionFinished {
            id: "tool".into(),
            result: serde_json::json!({"stdout":"tail"}),
            success: true,
        },
        EventData::ActorBound {
            actor: "worker-b".into(),
            target: member(2).target,
            goal: Some("goal".into()),
        },
        EventData::Message {
            actor: "worker-b".into(),
            text: "replacement-task".into(),
        },
    ]
    .into_iter()
    .enumerate()
    {
        transcript.receive(&event(sequence as u64 + 1, data));
    }
    assert_eq!(
        transcript
            .entries()
            .iter()
            .filter(|e| e.id.starts_with("assignment:"))
            .count(),
        2
    );
    assert!(
        super::history::selected_text(&transcript, "tool")
            .unwrap()
            .contains("own-stream")
    );
    transcript.focus(ViewTarget::main());
    assert!(super::history::selected_text(&transcript, "tool").is_err());
    assert!(
        !transcript
            .active_lines(100)
            .iter()
            .any(|l| l.to_string().contains("own-stream"))
    );
    transcript.focus(member(2));
    assert!(
        super::history::selected_text(&transcript, "tool").is_ok(),
        "identity view must retain older assignments"
    );
}

#[test]
fn recent_and_reconnect_are_bounded_per_view_without_replaying_settled_blocks() {
    let mut transcript = Transcript::default();
    transcript.focus(ViewTarget::main());
    transcript.receive(&event(
        1,
        EventData::ActorBound {
            actor: "worker-a".into(),
            target: member(1).target,
            goal: Some("goal".into()),
        },
    ));
    for i in 2..42 {
        transcript.receive(&event(
            i,
            EventData::Message {
                actor: "worker-a".into(),
                text: format!("item-{i}"),
            },
        ));
    }
    transcript.recent(80);
    transcript.focus(member(1));
    assert!(transcript.needs_recent());
    let recent = transcript.recent(80);
    assert!(recent.len() <= 200);
    assert!(!recent.iter().any(|l| l.to_string().trim() == "item-2"));
    transcript.focus(ViewTarget::main());
    transcript.focus(member(2));
    assert!(!transcript.needs_recent());
    assert!(transcript.drain_settled(40).is_empty());
    transcript.receive(&event(
        41,
        EventData::Message {
            actor: "worker-a".into(),
            text: "duplicate".into(),
        },
    ));
    assert!(transcript.drain_settled(80).is_empty());
    assert!(
        transcript
            .lines(80, true)
            .iter()
            .any(|l| l.to_string().trim() == "item-2"),
        "complete history stays available"
    );
}

#[tokio::test]
async fn delayed_failure_keeps_original_assignment_and_editor_after_focus_switch() {
    use super::jobs::{Context, Jobs, Reply};
    let directory = tempfile::tempdir().unwrap();
    let paths = crate::config::Paths::new(directory.path().into());
    let mut screen = Screen::default();
    screen.focus(member(1));
    screen.input.insert("old assignment input");
    screen.bind_draft();
    let mut context = Context::new(1, &paths, &screen);
    screen.input.take();
    screen.bind_draft();
    context.revision = screen.input.revision();
    let (release, wait) = std::sync::mpsc::channel();
    let mut jobs = Jobs::new();
    context.execute(&mut jobs, "old assignment input".into(), move |context| {
        wait.recv().unwrap();
        assert_eq!(context.view, member(1));
        anyhow::bail!("fixture delayed failure")
    });
    screen.view = member(2);
    screen.focus(ViewTarget::main());
    screen.input.insert("untouched main");
    screen.bind_draft();
    release.send(()).unwrap();
    let (_, reply) = jobs.join_next().await.unwrap().unwrap();
    let Reply::Failed {
        origin,
        draft,
        revision,
        ..
    } = reply.unwrap()
    else {
        panic!("expected origin-bound failure")
    };
    screen.restore_origin(&origin, draft, revision);
    assert_eq!(screen.input.text(), "untouched main");
    screen.focus(member(2));
    assert_eq!(screen.input.text(), "old assignment input");
    assert!(screen.stale_draft());
}

#[test]
fn assignment_refresh_rejects_existing_draft_before_submission() {
    let mut screen = Screen::default();
    screen.focus(member(1));
    screen.input.insert("not for replacement task");
    screen.bind_draft();
    screen.view = member(2);
    let paths = crate::config::Paths::new(std::env::temp_dir());
    let context = super::jobs::Context::new(0, &paths, &screen);
    let mut initial = super::submission::Initial::default();
    assert!(
        initial
            .prepare(
                &mut screen,
                context,
                &crate::commands::Command::Message("not for replacement task".into()),
                false
            )
            .is_none()
    );
    assert_eq!(screen.input.text(), "not for replacement task");
}

fn switch_fixture() -> Screen {
    let mut screen = Screen::default();
    screen.transcript.focus(ViewTarget::main());
    screen.transcript.receive(&event(
        1,
        EventData::ActorBound {
            actor: "worker-a".into(),
            target: member(1).target,
            goal: Some("goal".into()),
        },
    ));
    screen.transcript.receive(&event(
        2,
        EventData::Message {
            actor: "worker-a".into(),
            text: "old destination output".into(),
        },
    ));
    screen
}

#[test]
fn first_view_boundary_precedes_recovered_destination_output() {
    let mut screen = switch_fixture();
    screen.focus(member(1));
    let lines = screen.transcript.recent(100);
    let boundary = lines
        .iter()
        .position(|line| line.to_string().contains("视图 member-a"))
        .unwrap();
    let output = lines
        .iter()
        .position(|line| line.to_string().contains("old destination output"))
        .unwrap();
    assert!(
        boundary < output,
        "destination output appeared before its view boundary"
    );
    assert!(lines.len() <= 200);
    assert!(
        screen.transcript.drain_settled(40).is_empty(),
        "resize/repaint replayed the boundary or settled output"
    );
}

#[test]
fn revisited_view_boundary_precedes_only_unseen_destination_rows() {
    let mut screen = switch_fixture();
    screen.focus(member(1));
    screen.transcript.recent(100);
    screen.focus(ViewTarget::main());
    screen.transcript.recent(100);
    screen.transcript.receive(&event(
        3,
        EventData::Message {
            actor: "worker-a".into(),
            text: "new destination output".into(),
        },
    ));
    screen.focus(member(1));
    assert!(!screen.transcript.needs_recent());
    let lines = screen.transcript.drain_settled(100);
    let text = lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.find("视图 member-a").unwrap() < text.find("new destination output").unwrap());
    assert!(
        !text.contains("old destination output")
            && !text.contains("恢复最近记录")
            && !text.contains("Logit"),
        "revisit replayed old rows, recovery or startup branding"
    );
    assert!(screen.transcript.drain_settled(50).is_empty());
    assert_eq!(screen.transcript.sequence(), 3);
    assert!(
        screen
            .transcript
            .visible_entries()
            .any(|entry| entry.body == "old destination output" && entry.origin == member(1)),
        "view presentation changed complete history or its origin"
    );
}

#[test]
fn persistent_team_label_uses_view_goal_before_current_goal_and_session_fallback() {
    let mut screen = Screen::default();
    let mut session = crate::runtime::state::Session::new(std::env::temp_dir(), "task", 1000, 60);
    session.id = "session-fallback".into();
    session.goal.id = "current-goal".into();
    let collaborator = crate::runtime::team::MemberView {
        id: "member-a".into(),
        name: "Ada".into(),
        target: member(1).target,
        actor: None,
        attempts: vec![],
        task: Some("current task".into()),
        paused: false,
        interrupted: false,
        stopping: false,
        waiting: None,
    };
    let mut session: crate::runtime::view::SessionView = session.into();
    session.team.members.push(collaborator.clone());
    screen.session = Some(session);
    let main = screen.activity_lines().join("\n");
    assert!(main.contains("main · 团队 current-"));
    screen.view = member(1);
    screen.view.goal = Some("current-goal".into());
    screen.member = Some(collaborator);
    let active = screen.activity_lines().join("\n");
    assert!(
        active.contains("团队 current-")
            && active.contains("Ada [member-a]")
            && active.contains("任务 task-a v1")
    );
    screen.view.goal = Some("original-goal".into());
    screen.view.readonly = true;
    let archived = screen.activity_lines().join("\n");
    assert!(archived.contains("团队 original") && archived.contains("归档只读"));
    assert!(
        !archived.contains("团队 current-"),
        "archived member was labelled as the later team"
    );
    screen.view.goal = Some(String::new());
    assert!(screen.activity_lines()[0].contains("团队 current-"));
    screen.session.as_mut().unwrap().goal.id.clear();
    assert!(screen.activity_lines()[0].contains("团队 session-"));
}

#[test]
fn addressed_main_input_correlates_by_turn_in_live_and_replay() {
    use crate::runtime::team::{MemberControl, Receipt, ReceiptStatus, Request};
    let mut events = vec![];
    for id in ["first", "second", "first"] {
        let request = Request {
            id: id.into(),
            client_id: "client".into(),
            target: Target::Main,
            action: MemberControl::Message {
                text: "identical literal input".into(),
            },
        };
        events.push(event(
            events.len() as u64 + 1,
            EventData::RequestReceipt {
                goal: "goal".into(),
                receipt: Receipt {
                    applying: false,
                    request: request.clone(),
                    status: ReceiptStatus::Pending,
                    error: None,
                    pending_input: None,
                },
            },
        ));
        events.push(event(events.len() as u64 + 1, serde_json::from_value(serde_json::json!({"type":"user","message":"identical literal input","turn":request.key()})).unwrap()));
    }
    for live in [true, false] {
        let mut transcript = Transcript::default();
        for event in &events {
            transcript.receive(event);
            if live {
                transcript.drain_settled(100);
            }
        }
        let users = transcript
            .entries()
            .iter()
            .filter(|e| e.kind == super::transcript::Kind::User)
            .collect::<Vec<_>>();
        assert_eq!(
            users.len(),
            2,
            "same-ID retry must coalesce; distinct identical text must remain"
        );
        assert!(users.iter().all(|e| e.body == "identical literal input"));
    }
}

#[test]
fn saved_legacy_worker_history_and_usage_belong_to_its_member() {
    use crate::{
        config::Paths,
        runtime::{state::Session, view::SessionView},
        store::Store,
    };
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().into());
    std::fs::create_dir_all(paths.database().parent().unwrap()).unwrap();
    let store = Store::open(&paths.database()).unwrap();
    let mut session = Session::new(root.path().into(), "legacy", 10000, 60);
    let mut worker = serde_json::to_value(&session.actors["main"]).unwrap();
    worker["id"] = serde_json::json!("old-worker");
    worker["role"] = serde_json::json!("worker");
    worker["task"] = serde_json::json!("old task");
    session
        .actors
        .insert("old-worker".into(), serde_json::from_value(worker).unwrap());
    let mut old = serde_json::to_value(&session).unwrap();
    old.as_object_mut().unwrap().remove("team");
    for actor in old["actors"].as_object_mut().unwrap().values_mut() {
        actor.as_object_mut().unwrap().remove("target");
    }
    store
        .commit(
            &session.id,
            &old,
            EventData::Message {
                actor: "old-worker".into(),
                text: "old member output".into(),
            },
        )
        .unwrap();
    store
        .commit(
            &session.id,
            &old,
            EventData::Usage {
                actor: "old-worker".into(),
                input: 7,
                output: 3,
                cached: 0,
            },
        )
        .unwrap();
    let before = store.events(&session.id, 0).unwrap();
    let saved: Session = store.load(&session.id).unwrap();
    let view = SessionView::from(saved);
    assert_eq!(view.bindings["old-worker"].target.member_id(), "old-worker");
    let origin = ViewTarget {
        target: view.bindings["old-worker"].target.clone(),
        goal: Some(session.goal.id.clone()),
        readonly: false,
    };
    let service =
        crate::commands::Service::new(&paths, root.path(), Some(&session.id)).in_view(origin);
    let crate::commands::Outcome::Local { text, .. } = service
        .execute(crate::commands::parse("/context").unwrap())
        .unwrap()
    else {
        panic!("context output")
    };
    let context: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(context["actors"].as_array().unwrap().len(), 1);
    assert_eq!(context["actors"][0]["id"], "old-worker");
    let mut transcript = super::history::load(&paths, &session.id).unwrap();
    transcript.focus(ViewTarget {
        target: view.bindings["old-worker"].target.clone(),
        goal: Some(session.goal.id.clone()),
        readonly: false,
    });
    assert!(
        transcript
            .visible_entries()
            .any(|entry| entry.body == "old member output")
    );
    assert_eq!(
        transcript
            .ownership
            .event(&before[1].data)
            .target
            .member_id(),
        "old-worker"
    );
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(store.events(&session.id, 0).unwrap()).unwrap(),
        "projection must not rewrite durable events"
    );
    assert_eq!(transcript.usage["old-worker"].input, 7);
    assert_eq!(transcript.usage["old-worker"].output, 3);
}

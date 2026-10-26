//! Regressions from the independent whole-change review, using runtime views.
use super::*;
use crate::{
    domain::Status,
    runtime::{
        graph::{Step, StepStatus},
        state::Session,
        team::{AssignmentStatus, MemberControl, Request},
        view::ViewTarget,
    },
};
use std::collections::BTreeSet;

fn session() -> Session {
    let mut session = Session::new(std::env::temp_dir(), "team", 1000, 60);
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
                    files: BTreeSet::new(),
                    revision: 1,
                    status: StepStatus::Pending,
                })
                .collect(),
            &session.goal,
        )
        .unwrap();
    session
}
fn text(lines: Vec<ratatui::text::Line<'static>>) -> String {
    lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn finished_assignment_keeps_actor_but_not_execution_label() {
    for (status, label) in [
        (AssignmentStatus::Finished, "完成"),
        (AssignmentStatus::Stopped, "停止"),
        (AssignmentStatus::Invalidated, "失效"),
    ] {
        let mut s = session();
        let a = s.assign_member(Some("Ada"), "a", "work").unwrap();
        s.finish_assignment(&a.actor, status);
        assert_eq!(
            s.team.members[&a.member].actor.as_deref(),
            Some(a.actor.as_str())
        );
        let mut screen = Screen::default();
        screen.receive(crate::runtime::ipc::ServerMessage::View {
            state: Box::new(s.into()),
            sequence: 0,
        });
        let view = text(chrome::lines(&screen, 106));
        assert!(view.contains(&format!("Ada·{label}")), "{view}");
        assert!(!view.contains("Ada·执行"));
    }
}

#[test]
fn archived_same_name_direct_attaches_keep_distinct_member_identity() {
    use crate::runtime::conversation::Archive;
    let mut old = session();
    let a = old.assign_member(Some("Ada"), "a", "same task").unwrap();
    let b = old.assign_member(Some("Bob"), "b", "same task").unwrap();
    old.team.members.get_mut(&b.member).unwrap().name = "Ada".into();
    old.finish_assignment(&a.actor, AssignmentStatus::Finished);
    old.finish_assignment(&b.actor, AssignmentStatus::Finished);
    let mut current = session();
    current.archives.push(Archive {
        team: old.team,
        goal: old.goal,
        status: Status::Completed,
        graph: old.graph,
        actors: old.actors,
        evidence: vec![],
        actions: Default::default(),
        report: None,
    });
    let mut rendered = vec![];
    for id in [&a.member, &b.member] {
        let mut screen = Screen::default();
        screen.view = current.resolve_view(id).unwrap();
        screen.receive(crate::runtime::ipc::ServerMessage::TargetView {
            state: Box::new(current.target_view(&screen.view).unwrap()),
            sequence: 0,
        });
        let rows = chrome::lines(&screen, 36);
        let view = text(rows.clone());
        assert!(
            view.contains(&team::identity(id)),
            "missing stable member identity: {view}"
        );
        assert!(
            rows.last().unwrap().to_string().contains("Ada"),
            "current archive missing from strip: {view}"
        );
        rendered.push(view);
    }
    assert_ne!(rendered[0], rendered[1]);
}

fn pending(screen: &mut Screen, origin: &ViewTarget) {
    screen.transcript.focus(origin.clone());
    screen.transcript.accepted(
        origin,
        &Request {
            id: "pending".into(),
            client_id: "client".into(),
            target: origin.target.clone(),
            action: MemberControl::Message {
                text: "很长的待处理输入".repeat(8),
            },
        },
    );
}
#[test]
fn pending_input_cannot_hide_global_or_local_pause() {
    for global in [false, true] {
        let mut s = session();
        let a = s.assign_member(Some("Ada"), "a", "work").unwrap();
        s.team.members.get_mut(&a.member).unwrap().paused = !global;
        if global {
            s.status = Status::Paused;
        }
        let mut screen = Screen::default();
        screen.view = s.resolve_view(&a.member).unwrap();
        screen.receive(crate::runtime::ipc::ServerMessage::TargetView {
            state: Box::new(s.target_view(&screen.view).unwrap()),
            sequence: 0,
        });
        let origin = screen.view.clone();
        pending(&mut screen, &origin);
        let view = text(status::lines(&screen, 36, 3));
        assert!(view.contains("暂停"), "pending hid the blocker: {view}");
        if global {
            assert!(view.contains("整队"), "scope missing: {view}");
        }
    }
}
#[test]
fn narrow_pending_fallback_preserves_its_delivery_state() {
    let mut screen = Screen::default();
    pending(&mut screen, &ViewTarget::main());
    let rows = status::lines(&screen, 36, 1);
    let view = text(rows.clone());
    assert!(
        view.contains("已接收，等待处理") && view.contains("/status"),
        "{view}"
    );
    assert!(rows.iter().all(|r| r.width() <= 36));
}

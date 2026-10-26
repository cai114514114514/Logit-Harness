//! Measured activity and explicit blockers, independent of the last model label.
use super::{Screen, transcript::Kind};
pub(super) fn lines(screen: &Screen) -> Vec<String> {
    let mut lines = vec![];
    if let Some(session) = &screen.session {
        if !screen.view.readonly
            && matches!(
                session.status,
                crate::domain::Status::Paused
                    | crate::domain::Status::Stopped
                    | crate::domain::Status::Exhausted
                    | crate::domain::Status::Failed
            )
        {
            lines.push(format!(
                "{}{} · {}",
                if screen.view.is_main() { "" } else { "整队" },
                super::state::label(&session.status),
                if screen.view.is_main() {
                    "/resume"
                } else {
                    "/agent main"
                }
            ));
        }
        if !screen.view.is_main()
            && let Some(member) = &screen.member
            && (member.paused || member.stopping || member.waiting.is_some())
        {
            lines.push(format!(
                "{} · {}",
                super::team::member_status(member),
                progress(screen, &member.id)
            ));
        }
        if matches!(
            session.status,
            crate::domain::Status::Pausing | crate::domain::Status::Stopping
        ) {
            lines.push(format!(
                "{} · 等待活动结算 · /tasks",
                super::state::label(&session.status)
            ));
        }
        if let Some(owner) = &session.owner {
            let title = screen
                .recent
                .iter()
                .find(|s| s.id == *owner)
                .map(|s| s.objective.as_str())
                .unwrap_or("工作区拥有者");
            lines.push(format!(
                "等待工作区 · /sessions · /tasks · [{}] {}",
                owner, title
            ));
        }
        for approval in session
            .approval_actions
            .iter()
            .filter(|_| screen.view.is_main())
        {
            // SessionView already supplies the tool and exact /approve command.
            lines.push(format!("等待授权 · {approval}"));
        }
        for (id, question) in session.questions.iter().filter(|_| screen.view.is_main()) {
            if question.answer.is_none() {
                lines.push(format!(
                    "等待用户回答 · /answer {id} <答复> · {}",
                    question.text
                ));
            }
        }
    }
    let now = chrono::Utc::now();
    let active = screen
        .transcript
        .visible_entries()
        .filter(|e| e.running)
        .collect::<Vec<_>>();
    for entry in &active {
        let elapsed = age(&entry.started, now);
        let idle = age(&entry.updated, now);
        // The same verb and object as the tool line, so the two read as one step.
        let work = if entry.kind == Kind::Tool {
            entry.title.split_whitespace().collect::<Vec<_>>().join(" ")
        } else if entry.body.is_empty() {
            format!("{} · 等待模型响应", entry.title)
        } else {
            format!("{} · 正在回复", entry.title)
        };
        let serves = if entry.clauses.is_empty() {
            String::new()
        } else {
            format!(" · 服务 {}", entry.clauses.join(" "))
        };
        // In the main view each member's work is named; its own view needs no name.
        let who = if screen.view.is_main() && !entry.origin.is_main() {
            format!(
                "{} ",
                screen
                    .transcript
                    .member_name(entry.origin.target.member_id())
            )
        } else {
            String::new()
        };
        lines.push(format!(
            "{who}⋯ {work}{serves} · 已用 {elapsed} · 距输出 {idle}"
        ));
    }
    if let Some(session) = &screen.session {
        for activity in &session.activities {
            if !super::ownership::same_view(
                &screen.view,
                &screen.transcript.ownership.actor(&activity.actor),
            ) {
                continue;
            }
            if !active
                .iter()
                .any(|e| e.id == activity.id || e.source == activity.id)
            {
                lines.push(format!(
                    "⋯ {} · {} · /tasks",
                    super::transcript::actor_label(&activity.actor),
                    activity.title
                ));
            }
        }
        if lines.is_empty() && screen.view.is_main() {
            let waiting = session
                .team
                .members
                .iter()
                .filter_map(|member| {
                    member
                        .waiting
                        .as_ref()
                        .map(|reason| format!("{} → {reason}", member.name))
                })
                .collect::<Vec<_>>();
            if !waiting.is_empty() {
                // These are reported member waits, not an inferred main-agent
                // phase. A running request above always takes precedence.
                lines.push(format!("成员等待 · {} · /team", waiting.join("；")));
            }
        }
        // Idle mode and status live in the editor rule; only facts that need
        // attention take a status line of their own.
        if lines.is_empty() && session.verifying {
            lines.push("等待独立验收调度 · /tasks".into());
        }
        if !screen.connected {
            lines.push("后台未连接 · /resume".into());
        }
    }
    if !screen.activity.is_empty() {
        lines.push(screen.activity.clone());
    }
    lines.extend(screen.feedback.lines(&screen.view));
    lines
}
/// Same format as the tool line's duration, so the two never disagree.
fn age(value: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    super::tool_text::elapsed(value, &now.to_rfc3339()).unwrap_or_else(|| "未知".into())
}
pub(super) fn progress(screen: &Screen, member: &str) -> String {
    screen
        .transcript
        .entries()
        .iter()
        .filter(|entry| {
            entry.origin.target.member_id() == member
                && matches!(entry.kind, Kind::Assistant | Kind::Tool)
        })
        .map(|entry| &entry.updated)
        .max()
        .map_or_else(
            || "暂无输出记录".into(),
            |stamp| format!("距进展{}", age(stamp, chrono::Utc::now())),
        )
}

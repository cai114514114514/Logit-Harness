//! Measured activity and explicit blockers, independent of the last model label.
use super::{Screen, transcript::Kind};
pub(super) fn lines(screen: &Screen) -> Vec<String> {
    let mut lines = vec![];
    if let Some(session) = &screen.session {
        let team = team_label(screen, session);
        if screen.view.is_main() {
            if !session.team.members.is_empty() {
                lines.push(format!(
                    "main · {team} · {} 位成员 · 并发上限 {} · /team",
                    session.team.members.len(),
                    session.team.limit
                ));
                for member in session.team.members.iter().take(2) {
                    lines.push(format!(
                        "{} · {} · {} · {}",
                        member.name,
                        super::team::member_status(member),
                        member.task.as_deref().unwrap_or("无分配"),
                        progress(screen, &member.id)
                    ));
                }
            }
        } else {
            let name = screen
                .member
                .as_ref()
                .map(|m| m.name.as_str())
                .unwrap_or(screen.view.target.member_id());
            lines.push(format!(
                "{team} · {} [{}] · {}{}",
                name,
                super::team::identity(screen.view.target.member_id()),
                super::team::assignment(&screen.view.target),
                if screen.view.readonly {
                    " · 归档只读"
                } else {
                    ""
                }
            ));
            if let Some(member) = &screen.member {
                lines.push(format!(
                    "{} · {} · {}",
                    super::team::member_status(member),
                    member.task.as_deref().unwrap_or("无任务"),
                    progress(screen, &member.id)
                ));
            }
            if screen.stale_draft() {
                lines.push("草稿属于旧任务 · Ctrl+U 清空后重新输入 · 不会自动改投".into());
            }
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
        let work = if entry.kind == Kind::Assistant && entry.body.is_empty() {
            "等待模型响应"
        } else if entry.kind == Kind::Assistant {
            "正在回复"
        } else {
            "执行工具"
        };
        let title = if entry.kind == Kind::Tool {
            String::new()
        } else {
            format!("{} · ", entry.title)
        };
        lines.push(format!(
            "{work} · {title}已用{elapsed} · 距输出{idle} · /tasks"
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
                    "{} · {} · /tasks",
                    super::transcript::actor_label(&activity.actor),
                    activity.title
                ));
            }
        }
        if lines.is_empty() {
            lines.push(if session.verifying {
                "等待独立验收调度 · /tasks".into()
            } else {
                format!(
                    "{} · {}{}",
                    super::present::mode(screen),
                    super::state::label(&session.status),
                    if screen.connected {
                        ""
                    } else {
                        " · 后台未连接 · /resume"
                    }
                )
            });
        }
    }
    if !screen.activity.is_empty() {
        lines.push(screen.activity.clone());
    }
    if lines.is_empty() {
        lines.push("输入消息开始对话 · / 查看命令".into());
    }
    lines
}
fn team_label(screen: &Screen, session: &crate::runtime::view::SessionView) -> String {
    // Archived views keep their original goal even when this session owns a
    // later team. An empty goal identity falls back to the saved session.
    let id = screen
        .view
        .goal
        .as_deref()
        .filter(|id| !id.is_empty())
        .or_else(|| (!session.goal.id.is_empty()).then_some(session.goal.id.as_str()))
        .unwrap_or(&session.id);
    format!("团队 {}", super::team::identity(id))
}
fn age(value: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|t| format!("{}s", now.signed_duration_since(t).num_seconds().max(0)))
        .unwrap_or_else(|| "未知".into())
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

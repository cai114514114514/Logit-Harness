//! Measured activity and explicit blockers, independent of the last model label.
use super::{Screen, transcript::Kind};
pub(super) fn lines(screen: &Screen) -> Vec<String> {
    let mut lines = vec![];
    if let Some(session) = &screen.session {
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
                short(owner),
                title
            ));
        }
        for approval in &session.approval_actions {
            lines.push(format!("等待授权 · {approval}"));
        }
        for (id, question) in &session.questions {
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
        .entries()
        .iter()
        .filter(|e| e.running)
        .collect::<Vec<_>>();
    for entry in &active {
        let elapsed = age(&entry.started, now);
        let idle = age(&entry.updated, now);
        let work = if entry.kind == Kind::Assistant {
            "等待模型响应"
        } else {
            "执行工具"
        };
        let title = if entry.kind == Kind::Tool {
            String::new()
        } else {
            format!("{} · ", entry.title)
        };
        lines.push(format!(
            "{work} · {title}已用{elapsed} · 距输出{idle} · [{}] · /tasks {}",
            short(&entry.source),
            short(&entry.id)
        ));
    }
    if let Some(session) = &screen.session {
        for activity in &session.activities {
            if !active
                .iter()
                .any(|e| e.id == activity.id || e.source == activity.id)
            {
                lines.push(format!(
                    "{} [{}] · {} · /tasks",
                    super::transcript::actor_label(&activity.actor),
                    short(&activity.id),
                    activity.title
                ));
            }
        }
        if lines.is_empty() {
            lines.push(if session.verifying {
                "等待独立验收调度 · /tasks".into()
            } else {
                format!(
                    "{}{}",
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
fn age(value: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|t| format!("{}s", now.signed_duration_since(t).num_seconds().max(0)))
        .unwrap_or_else(|| "未知".into())
}
fn short(value: &str) -> String {
    value.chars().take(12).collect()
}

//! One presentation for command, team, session and Skill choices.
use super::{
    Screen,
    text::{self, ACCENT, MUTED},
};
use ratatui::{
    style::{Modifier, Style},
    text::Line,
};

pub(super) fn count(screen: &Screen) -> usize {
    if let Some(choice) = &screen.skill_choices {
        choice.entries.len()
    } else if screen.team_picker {
        screen.members.len() + 1
    } else if screen.picker {
        screen.recent.len()
    } else {
        screen.menu().len()
    }
}
pub(super) fn lines(screen: &Screen, width: usize, limit: usize) -> Vec<Line<'static>> {
    if count(screen) == 0 || limit == 0 {
        return vec![];
    }
    let items: Vec<(String, String)> = if let Some(choice) = &screen.skill_choices {
        choice
            .entries
            .iter()
            .map(|e| {
                (
                    format!("{}  {}", e.name, e.description),
                    format!("{} · {}", e.source, e.path.display()),
                )
            })
            .collect()
    } else if screen.team_picker {
        std::iter::once((
            "主控 · 总览与全队控制".into(),
            "Enter 聚焦 · o 新窗口 · Esc 返回".into(),
        ))
        .chain(screen.members.iter().map(|m| {
            (
                format!(
                    "{} · {} · {}",
                    m.name,
                    super::team::member_status(m),
                    m.task.as_deref().unwrap_or("无分配")
                ),
                format!(
                    "{} · {} · {} · {}",
                    m.id,
                    super::team::assignment(&m.target),
                    screen.member_usage(&m.id),
                    super::activity::progress(screen, &m.id)
                ),
            )
        }))
        .collect()
    } else if screen.picker {
        screen
            .recent
            .iter()
            .map(|r| {
                (
                    format!(
                        "{}  {}",
                        super::state::label(&r.status),
                        r.objective.lines().next().unwrap_or("")
                    ),
                    r.id.clone(),
                )
            })
            .collect()
    } else {
        screen
            .menu()
            .iter()
            .map(|c| {
                (
                    format!("{:<14} {}", c.name, c.description),
                    format!("{} · {}", c.source, super::present::timing(c.timing)),
                )
            })
            .collect()
    };
    let selected = screen.selection.min(items.len() - 1);
    let visible = limit.saturating_sub(2).clamp(1, 6);
    let first = selected.saturating_sub(visible - 1);
    let mut rows = items
        .iter()
        .enumerate()
        .skip(first)
        .take(visible)
        .map(|(i, (label, _))| {
            Line::styled(
                text::ellipsize(
                    &super::state::clean(&format!(
                        "{} {label}",
                        if i == selected { "›" } else { " " }
                    )),
                    width,
                ),
                if i == selected {
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(MUTED)
                },
            )
        })
        .collect::<Vec<_>>();
    if rows.len() < limit {
        rows.push(Line::styled(
            text::ellipsize(&super::state::clean(&items[selected].1), width),
            Style::default().fg(MUTED),
        ));
    }
    if rows.len() < limit {
        rows.push(Line::styled(
            if screen.team_picker {
                "↑↓ 选择 · Enter 聚焦 · o 新窗口 · Esc 返回"
            } else {
                "↑↓ 选择 · Enter 确认 · Tab 补全 · Esc 返回"
            },
            Style::default().fg(MUTED),
        ));
    }
    rows.into_iter()
        .map(|mut row| {
            if row.width() > width {
                row = Line::styled(text::ellipsize(&row.to_string(), width), row.style);
            }
            row
        })
        .collect()
}

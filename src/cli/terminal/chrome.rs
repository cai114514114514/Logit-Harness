//! Stable view identity and a single member strip inside the active region.
use super::{
    Screen,
    text::{self, ACCENT, MUTED},
};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

pub(super) fn lines(screen: &Screen, width: usize) -> Vec<Line<'static>> {
    let Some(session) = &screen.session else {
        return vec![];
    };
    if screen.view.is_main() && session.team.members.is_empty() {
        return vec![];
    }
    let goal = screen
        .view
        .goal
        .as_deref()
        .filter(|g| !g.is_empty())
        .or_else(|| (!session.goal.id.is_empty()).then_some(session.goal.id.as_str()))
        .unwrap_or(&session.id);
    let title = if goal == session.goal.id && !session.goal.original.is_empty() {
        text::ellipsize(
            &super::state::clean(session.goal.original.lines().next().unwrap_or("")),
            width.saturating_sub(10),
        )
    } else {
        format!("团队 {}", super::team::identity(goal))
    };
    let mut rows = vec![];
    if screen.view.is_main() {
        rows.push(Line::from(vec![
            Span::styled("┗ ", Style::default().fg(ACCENT)),
            Span::raw(title),
            Span::styled(" · 主控", Style::default().fg(MUTED)),
        ]));
    } else {
        let scope = if screen.view.readonly {
            "归档只读"
        } else {
            "仅控制此成员"
        };
        let name = screen
            .member
            .as_ref()
            .map_or(screen.view.target.member_id(), |m| m.name.as_str());
        rows.push(Line::from(vec![
            Span::styled(
                text::ellipsize(
                    &super::state::clean(name),
                    width.saturating_sub(scope.width() + 3),
                ),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(" · {scope}")),
        ]));
        let task = screen
            .member
            .as_ref()
            .and_then(|m| m.task.as_deref())
            .unwrap_or("无分配");
        rows.push(Line::styled(
            text::ellipsize(&super::state::clean(&format!("{title} · {task}")), width),
            Style::default().fg(MUTED),
        ));
    }
    rows.push(strip(screen, width));
    rows
}

fn strip(screen: &Screen, width: usize) -> Line<'static> {
    let mut members = screen.members.iter().collect::<Vec<_>>();
    if members.is_empty()
        && let Some(s) = &screen.session
    {
        members = s.team.members.iter().collect();
    }
    if screen.view.readonly {
        members.retain(|m| m.id == screen.view.target.member_id());
    }
    members.sort_by(|a, b| a.id.cmp(&b.id));
    members.dedup_by_key(|m| &m.id);
    let current = screen.view.target.member_id();
    let labels = members
        .iter()
        .map(|m| {
            let duplicates = members.iter().filter(|other| other.name == m.name).count() > 1;
            let name = if duplicates {
                format!(
                    "{}:{}",
                    m.name,
                    m.id.chars()
                        .rev()
                        .take(4)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>()
                )
            } else {
                m.name.clone()
            };
            let state = if m.stopping {
                "结算"
            } else if m.paused {
                "暂停"
            } else if m.waiting.is_some() {
                "等待"
            } else if m.interrupted {
                "补充"
            } else if m.actor.is_some() {
                "执行"
            } else {
                "空闲"
            };
            let label = format!(
                "{}·{state}",
                text::ellipsize(&super::state::clean(&name), 12)
            );
            if m.id == current {
                format!("[{label}]")
            } else {
                label
            }
        })
        .collect::<Vec<_>>();
    let main = if screen.view.is_main() {
        "[主控]"
    } else {
        "主控"
    };
    let capacity = width.saturating_sub(main.width() + 15);
    let selected = members.iter().position(|m| m.id == current).unwrap_or(0);
    let mut first = 0;
    let mut end = labels.len();
    if labels.iter().map(|s| s.width() + 2).sum::<usize>() > capacity {
        first = selected;
        end = (selected + 1).min(labels.len());
        let mut used = labels.get(selected).map_or(0, |s| s.width() + 2);
        while first > 0 && used + labels[first - 1].width() + 2 <= capacity {
            first -= 1;
            used += labels[first].width() + 2;
        }
        while end < labels.len() && used + labels[end].width() + 2 <= capacity {
            used += labels[end].width() + 2;
            end += 1;
        }
    }
    let mut spans = vec![Span::styled(
        main,
        Style::default().fg(if screen.view.is_main() { ACCENT } else { MUTED }),
    )];
    for (i, label) in labels.iter().enumerate().take(end).skip(first) {
        spans.push(Span::styled(
            format!("  {label}"),
            Style::default().fg(if members[i].id == current {
                ACCENT
            } else {
                MUTED
            }),
        ));
    }
    let hidden = labels.len().saturating_sub(end - first);
    let tail = if hidden > 0 {
        format!(" +{hidden} · Ctrl-T")
    } else {
        " · Ctrl-T".into()
    };
    spans.push(Span::styled(tail, Style::default().fg(MUTED)));
    let line = Line::from(spans);
    if line.width() <= width {
        line
    } else {
        Line::styled(
            text::ellipsize(&line.to_string(), width),
            Style::default().fg(ACCENT),
        )
    }
}

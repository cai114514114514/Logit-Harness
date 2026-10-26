//! Stable view identity and a single member strip inside the active region.
use super::{
    Screen,
    text::{self, accent, muted},
};
use crate::runtime::team::{AssignmentStatus, MemberView, Target};
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
            Span::styled("┗ ", Style::default().fg(accent())),
            Span::raw(title),
            Span::styled(" · 主控", Style::default().fg(muted())),
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
        let badge = if screen.view.readonly
            || screen.members.iter().filter(|m| m.name == name).count() > 1
        {
            format!(
                " [{}]",
                super::team::identity(screen.view.target.member_id())
            )
        } else {
            String::new()
        };
        rows.push(Line::from(vec![
            Span::styled(
                format!(
                    "{}{badge}",
                    text::ellipsize(
                        &super::state::clean(name),
                        width.saturating_sub(scope.width() + 3 + badge.width()),
                    )
                ),
                Style::default().fg(accent()).add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(" · {scope}")),
        ]));
        let task = screen
            .member
            .as_ref()
            .and_then(|m| m.task.as_deref())
            .unwrap_or("无分配");
        let subtitle = if width < 60 {
            let team = format!(" · 团队 {}", super::team::identity(goal));
            format!(
                "{}{team}",
                text::ellipsize(
                    &super::state::clean(task),
                    width.saturating_sub(team.width())
                )
            )
        } else {
            let title = text::ellipsize(&title, width / 2);
            let remaining = width.saturating_sub(title.width() + 3);
            format!(
                "{title} · {}",
                text::ellipsize(&super::state::clean(task), remaining)
            )
        };
        rows.push(Line::styled(subtitle, Style::default().fg(muted())));
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
    // Direct archive attach supplies its member separately from the current
    // team's roster. Keep the exact selected identity, including after reassignment.
    if let Some(current) = &screen.member {
        if let Some(position) = members.iter().position(|m| m.id == current.id) {
            members[position] = current;
        } else {
            members.push(current);
        }
    }
    let ambiguous = members
        .iter()
        .filter(|m| members.iter().filter(|other| other.name == m.name).count() > 1)
        .map(|m| m.id.as_str())
        .collect::<Vec<_>>();
    if screen.view.readonly {
        members.retain(|m| m.id == screen.view.target.member_id());
    }
    members.sort_by(|a, b| a.id.cmp(&b.id));
    members.dedup_by_key(|m| &m.id);
    let current = screen.view.target.member_id();
    let labels = members
        .iter()
        .map(|m| {
            let duplicates = ambiguous.contains(&m.id.as_str());
            let name = if duplicates {
                format!(
                    "{}:{}",
                    text::ellipsize(&super::state::clean(&m.name), 7),
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
            let state = phase(screen, m);
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
        Style::default().fg(if screen.view.is_main() {
            accent()
        } else {
            muted()
        }),
    )];
    for (i, label) in labels.iter().enumerate().take(end).skip(first) {
        spans.push(Span::styled(
            format!("  {label}"),
            Style::default().fg(if members[i].id == current {
                accent()
            } else {
                muted()
            }),
        ));
    }
    let hidden = labels.len().saturating_sub(end - first);
    let tail = if hidden > 0 {
        format!(" +{hidden} · Ctrl-T")
    } else {
        " · Ctrl-T".into()
    };
    spans.push(Span::styled(tail, Style::default().fg(muted())));
    let line = Line::from(spans);
    if line.width() <= width {
        line
    } else {
        Line::styled(
            text::ellipsize(&line.to_string(), width),
            Style::default().fg(accent()),
        )
    }
}

fn phase(screen: &Screen, member: &MemberView) -> &'static str {
    if screen.view.readonly {
        return "归档";
    }
    if member.stopping {
        return "结算";
    }
    if member.paused {
        return "暂停";
    }
    if member.interrupted {
        return "补充";
    }
    if member.waiting.is_some() {
        return "等待";
    }
    let running = screen
        .transcript
        .entries()
        .iter()
        .any(|e| e.running && e.origin.target.member_id() == member.id)
        || screen.session.as_ref().is_some_and(|s| {
            s.activities.iter().any(|a| {
                screen
                    .transcript
                    .ownership
                    .actor(&a.actor)
                    .target
                    .member_id()
                    == member.id
            })
        });
    if running {
        return "执行";
    }
    if matches!(
        member.target,
        Target::Member {
            assignment: Some(_),
            ..
        }
    ) {
        return "待执行";
    }
    // An actor ID survives retirement. Resolve its assignment outcome instead
    // of treating this historical identity as evidence of live work.
    let assignment = screen.session.as_ref().and_then(|s| {
        s.team
            .assignments
            .iter()
            .find(|a| a.member == member.id && Some(a.actor.as_str()) == member.actor.as_deref())
    });
    match assignment.map(|a| &a.status) {
        Some(AssignmentStatus::Finished) => "完成",
        Some(AssignmentStatus::Stopped) => "停止",
        Some(AssignmentStatus::Invalidated) => "失效",
        _ => "空闲",
    }
}

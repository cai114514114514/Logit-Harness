//! The contract bar: one stamp per clause, read straight from the session view.
//!
//! It answers "how far along is this" without /status: each stamp states one
//! evidence fact, and an underlined clause is one a running step is serving.
use super::{
    Screen,
    text::{self, accent, fail, fresh, muted, stale},
};
use crate::runtime::{
    contract::{ClauseView, ContractView, Stamp},
    conversation::Mode,
    view::SessionView,
};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

pub(super) fn symbol(stamp: Stamp) -> (&'static str, Color) {
    match stamp {
        Stamp::Open => ("○", muted()),
        Stamp::Evidenced => ("●", fresh()),
        Stamp::Accepted => ("◉", fresh()),
        Stamp::Stale => ("⊘", stale()),
        Stamp::Failed => ("⊗", fail()),
    }
}

fn stamp(clause: &ClauseView) -> Span<'static> {
    let (symbol, color) = symbol(clause.stamp);
    Span::styled(symbol, Style::default().fg(color))
}

fn id(clause: &ClauseView) -> Span<'static> {
    let style = if clause.working {
        Style::default()
            .fg(accent())
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        Style::default()
    };
    Span::styled(clause.id.clone(), style)
}

/// The verdict while it still describes this contract version, else evidence.
fn count(contract: &ContractView, clauses: &[&ClauseView], short: bool) -> String {
    let total = clauses.len();
    let verdict = contract
        .acceptance
        .as_ref()
        .filter(|a| a.revision == contract.revision);
    match verdict {
        Some(a) if a.passed && !short => format!("验收通过 {total}/{total}"),
        Some(a) => {
            let passed = a.checks.iter().filter(|c| c.passed).count();
            format!("验收 {passed}/{total}")
        }
        None => {
            let evidenced = clauses
                .iter()
                .filter(|c| matches!(c.stamp, Stamp::Evidenced | Stamp::Accepted))
                .count();
            let label = if short { "证据" } else { "有证据" };
            format!("{label} {evidenced}/{total}")
        }
    }
}

/// Left and right parts on one line, the right part flush with the edge.
fn spread(mut left: Vec<Span<'static>>, right: String, width: usize) -> Option<Line<'static>> {
    let used = left.iter().map(|s| s.content.width()).sum::<usize>();
    if used + 2 + right.width() > width {
        return None;
    }
    left.push(Span::raw(" ".repeat(width - used - right.width())));
    left.push(Span::styled(right, Style::default().fg(muted())));
    Some(Line::from(left))
}

fn wide(contract: &ContractView, clauses: &[&ClauseView], width: usize) -> Option<Line<'static>> {
    let mut left = vec![Span::raw(format!("契约 v{}", contract.revision))];
    for clause in clauses {
        left.extend([Span::raw("   "), stamp(clause), Span::raw(" "), id(clause)]);
    }
    spread(
        left,
        format!("{} · /contract", count(contract, clauses, false)),
        width,
    )
}

fn medium(contract: &ContractView, clauses: &[&ClauseView], width: usize) -> Option<Line<'static>> {
    let mut left = vec![Span::raw(format!("契约 v{} ", contract.revision))];
    for clause in clauses {
        left.extend([Span::raw(" "), stamp(clause), id(clause)]);
    }
    spread(left, count(contract, clauses, true), width)
}

/// The last resort keeps the version, every stamp and the count.
fn small(contract: &ContractView, clauses: &[&ClauseView], width: usize) -> Line<'static> {
    let mut left = vec![Span::raw(format!("v{} ", contract.revision))];
    left.extend(clauses.iter().map(|c| stamp(c)));
    let total = clauses.len();
    let right = count(contract, clauses, true);
    let right = right.rsplit(' ').next().unwrap_or(&right).to_owned();
    spread(left.clone(), format!("{right} · /contract"), width)
        .or_else(|| spread(left, right.clone(), width))
        .unwrap_or_else(|| {
            Line::raw(text::ellipsize(
                &format!("v{} {right} ({total})", contract.revision),
                width,
            ))
        })
}

/// The clauses a member's current assignment serves.
fn member_clauses<'a>(
    screen: &Screen,
    session: &SessionView,
    contract: &'a ContractView,
) -> Vec<&'a ClauseView> {
    let member = screen.view.target.member_id();
    let assignment = session
        .team
        .assignments
        .iter()
        .rev()
        .find(|a| a.member == member);
    let ids = assignment
        .and_then(|a| session.graph.steps.get(&a.step))
        .map(|step| step.requirements.clone())
        .unwrap_or_default();
    contract.active().filter(|c| ids.contains(&c.id)).collect()
}

pub(super) fn lines(screen: &Screen, width: usize) -> Vec<Line<'static>> {
    let Some(session) = &screen.session else {
        return vec![];
    };
    let Some(contract) = &session.contract else {
        return vec![];
    };
    if screen.view.readonly || session.mode == Mode::Conversation {
        return vec![];
    }
    let clauses = contract.active().collect::<Vec<_>>();
    if clauses.is_empty() {
        return vec![];
    }
    if !screen.view.is_main() {
        // Members cannot change the contract; their bar is read-only context.
        let served = member_clauses(screen, session, contract);
        let mut left = vec![Span::raw("本成员服务 ")];
        for clause in &served {
            left.extend([Span::raw(" "), stamp(clause), Span::raw(" "), id(clause)]);
        }
        if served.is_empty() {
            left.push(Span::styled(" 当前无分配", Style::default().fg(muted())));
        }
        let right = format!("契约 v{} · 全部条款 /contract", contract.revision);
        let line = spread(left.clone(), right, width)
            .or_else(|| spread(left, format!("v{} · /contract", contract.revision), width))
            .unwrap_or_else(|| small(contract, &served, width));
        return vec![line];
    }
    let line = wide(contract, &clauses, width)
        .or_else(|| medium(contract, &clauses, width))
        .unwrap_or_else(|| small(contract, &clauses, width));
    vec![line]
}

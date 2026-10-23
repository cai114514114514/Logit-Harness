//! Prioritize actionable state; verbose diagnostics remain in /status and history.
use super::{Screen, text};
use ratatui::{
    style::Style,
    text::{Line, Span},
};

fn priority(line: &str) -> u8 {
    if line.starts_with("草稿属于") {
        0
    } else if line.starts_with("等待授权") || line.starts_with("等待用户回答") {
        1
    } else if line.contains("等待活动结算") {
        2
    } else if line.starts_with("等待工作区") {
        3
    } else if line.contains("已接收，等待处理") {
        4
    } else {
        5
    }
}
pub(super) fn lines(screen: &Screen, width: usize, limit: usize) -> Vec<Line<'static>> {
    if limit == 0 {
        return vec![];
    }
    let mut facts = screen.activity_lines();
    if screen.stale_draft() {
        facts.insert(0, "草稿属于旧任务 · Ctrl-U 清空 · /history 核对".into());
    }
    facts.extend(
        screen
            .transcript
            .pending_lines(usize::MAX)
            .iter()
            .map(ToString::to_string),
    );
    facts.sort_by_key(|line| priority(line));
    let mut rows = vec![];
    for (i, fact) in facts.iter().enumerate() {
        let wrapped = text::wrap(vec![Span::raw(super::state::clean(fact))], width);
        let reserve = usize::from(i + 1 < facts.len());
        if rows.len() + wrapped.len() + reserve > limit || i > 0 && priority(fact) == 5 {
            // Facts containing an actionable command are atomic: show its full
            // ID or a complete details route, never an apparently usable prefix.
            if rows.is_empty() {
                let subject = fact.split(" · ").next().unwrap_or("当前状态");
                let route = " · /status";
                use unicode_width::UnicodeWidthStr;
                rows.push(Line::raw(format!(
                    "{}{route}",
                    text::ellipsize(subject, width.saturating_sub(route.width()))
                )));
            } else {
                rows.push(Line::raw(text::ellipsize(
                    &format!("/status · 另 {} 项状态", facts.len() - i),
                    width,
                )));
            }
            break;
        }
        rows.extend(wrapped);
    }
    for row in &mut rows {
        row.style = Style::default().fg(text::ACCENT);
    }
    rows
}

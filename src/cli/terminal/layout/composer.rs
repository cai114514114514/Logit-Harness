//! The same editor and measured usage footer serve every view.
//!
//! One rule above the editor names the mode and what input does now, so the
//! idle state needs no status line of its own.
use super::{
    Screen,
    input::wrap,
    text::{self, accent, muted},
};
use crate::{domain::Status, runtime::conversation::Mode};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

/// Columns before the editor text: ` ▸  `.
const PROMPT: u16 = 4;

/// Editor rows for the current draft, plus the rule above it.
pub(super) fn height(screen: &Screen, columns: u16) -> u16 {
    let rows = wrap(
        screen.input.text(),
        columns.saturating_sub(PROMPT + 1) as usize,
    )
    .len();
    (rows as u16 + 1).clamp(2, 5)
}

fn mode(screen: &Screen, width: usize) -> &'static str {
    let mode = screen.session.as_ref().map(|session| session.mode);
    match mode {
        Some(Mode::Engineering) if width < 50 => "Eng",
        Some(Mode::Engineering) => "Engineering",
        Some(Mode::Plan) => "Plan · 只读",
        None | Some(Mode::Conversation) => "对话",
    }
}

/// What the rule says: who input goes to and whether anything is happening.
pub(super) fn label(screen: &Screen, width: usize) -> String {
    let Some(session) = &screen.session else {
        return "新对话".into();
    };
    let name = || {
        screen.member.as_ref().map_or_else(
            || screen.view.target.member_id().to_owned(),
            |m| m.name.clone(),
        )
    };
    if screen.view.readonly {
        return format!("{} · 归档只读", super::state::clean(&name()));
    }
    if !screen.view.is_main() {
        return format!("{} · 仅此成员", super::state::clean(&name()));
    }
    let mode = mode(screen, width);
    if width < 64 {
        return mode.into();
    }
    if !session.team.members.is_empty() {
        return format!("主控 · {mode} · 控制整队");
    }
    let state = match (&session.status, session.mode) {
        (Status::Running | Status::Waiting, Mode::Engineering) => "补充或纠正",
        (Status::Running, _) => "回复中",
        (Status::Completed, Mode::Engineering) => "已完成",
        (Status::Ready | Status::Completed | Status::Waiting, _) => "等待输入",
        (status, _) => super::state::label(status),
    };
    format!("{mode} · {state}")
}

/// `╶─ label ───…╴` across the full width.
fn rule(screen: &Screen, width: usize) -> Line<'static> {
    let label = text::ellipsize(&label(screen, width), width.saturating_sub(8));
    let fill = width.saturating_sub(4 + label.width() + 2);
    Line::from(vec![
        Span::styled("╶─ ", Style::default().fg(muted())),
        Span::raw(label),
        Span::styled(
            format!(" {}╴", "─".repeat(fill)),
            Style::default().fg(muted()),
        ),
    ])
}

pub(super) fn render(frame: &mut Frame, area: Rect, screen: &Screen) {
    if area.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new(rule(screen, area.width as usize)),
        Rect::new(area.x, area.y, area.width, 1),
    );
    let editor = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(1),
    );
    let text_area = Rect::new(
        editor.x + PROMPT,
        editor.y,
        editor.width.saturating_sub(PROMPT + 1),
        editor.height,
    );
    frame.render_widget(
        Paragraph::new("▸").style(Style::default().fg(accent())),
        Rect::new(editor.x + 1, editor.y, 1, 1.min(editor.height)),
    );
    let (cursor_x, cursor_y) = screen.input.position(text_area.width as usize);
    let offset = cursor_y.saturating_sub(text_area.height.saturating_sub(1) as usize);
    let rows = if screen.input.text().is_empty() {
        vec![Line::styled(
            text::ellipsize(
                &super::present::placeholder(screen, text_area.width as usize),
                text_area.width as usize,
            ),
            Style::default().fg(muted()),
        )]
    } else {
        wrap(screen.input.text(), text_area.width as usize)
            .into_iter()
            .skip(offset)
            .map(Line::from)
            .collect()
    };
    frame.render_widget(Paragraph::new(rows), text_area);
    frame.set_cursor_position((
        text_area.x + cursor_x as u16,
        text_area.y + (cursor_y - offset) as u16,
    ));
}

fn tokens(count: u64) -> String {
    if count >= 1000 {
        format!("{:.1}k", count as f64 / 1000.)
    } else {
        count.to_string()
    }
}

/// Shortcuts in the order they are given up when space runs out.
fn hints(screen: &Screen) -> Vec<&'static str> {
    let Some(session) = &screen.session else {
        return vec!["Enter 开始", "/ 命令", "Ctrl-D 退出", "/sessions 最近会话"];
    };
    if screen.view.readonly {
        return vec!["/history 历史", "/agent main 回主控"];
    }
    if !screen.view.is_main() {
        return vec!["Ctrl-T 成员", "/agent main 回主控"];
    }
    if !session.team.members.is_empty() {
        return vec![
            "Ctrl-O 详情",
            "Ctrl-T 成员",
            "/contract 契约",
            "Enter 发送",
            "Alt+Enter 换行",
        ];
    }
    match (session.mode, &session.status) {
        (Mode::Conversation, _) => vec!["/ 命令", "Ctrl-O 详情"],
        (_, Status::Completed) => vec!["/ 命令", "Ctrl-O 详情", "/contract 契约"],
        _ => vec!["Ctrl-O 详情", "/contract 契约", "Ctrl-T 成员"],
    }
}

/// Shown hints read in one fixed order, whichever of them survived.
const READING_ORDER: [&str; 11] = [
    "Enter 开始",
    "Enter 发送",
    "Alt+Enter 换行",
    "/ 命令",
    "Ctrl-O 详情",
    "/contract 契约",
    "Ctrl-T 成员",
    "/sessions 最近会话",
    "/history 历史",
    "/agent main 回主控",
    "Ctrl-D 退出",
];

fn ordered(mut kept: Vec<&'static str>) -> String {
    kept.sort_by_key(|hint| READING_ORDER.iter().position(|h| h == hint));
    kept.join(" · ")
}

pub(super) fn footer(screen: &Screen, width: usize) -> Line<'static> {
    let hints = hints(screen);
    let Some(session) = &screen.session else {
        // No usage yet: give up the least needed shortcut first, never mid-word.
        let line = (1..=hints.len())
            .rev()
            .map(|keep| ordered(hints[..keep].to_vec()))
            .find(|line| line.width() <= width)
            .unwrap_or_else(|| text::ellipsize(hints[0], width));
        return Line::styled(line, Style::default().fg(muted()));
    };
    let model = if screen.model.is_empty() {
        "deepseek-flash"
    } else {
        &screen.model
    };
    let member = !screen.view.is_main();
    let usage = if member {
        screen.transcript.usage.get(screen.view.target.member_id())
    } else {
        Some(&session.budget.reported)
    };
    let who = if member {
        screen
            .member
            .as_ref()
            .map_or_else(|| model.to_owned(), |m| super::state::clean(&m.name))
    } else {
        model.to_owned()
    };
    let short = if member {
        who.clone()
    } else {
        model.strip_prefix("deepseek-").unwrap_or(model).to_owned()
    };
    let used = usage.filter(|u| u.tokens() > 0).map(|u| {
        let prefix = if !member && !session.team.members.is_empty() {
            "全队 "
        } else {
            ""
        };
        format!("{prefix}{}", tokens(u.tokens()))
    });
    let kv = format!(
        "KV {}",
        usage
            .and_then(|u| u.cache_hit())
            .map_or_else(|| "—".into(), |hit| format!("{:.0}%", hit * 100.))
    );
    let full = [Some(who.clone()), used.clone(), Some(kv.clone())]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    let compact = format!("{short} · {kv}");
    let essential = if member {
        vec!["Ctrl-T 成员"]
    } else if session.mode == Mode::Engineering {
        vec!["/ 命令", "/contract"]
    } else {
        vec!["/ 命令"]
    };
    // Collapse order: secondary shortcuts, then tokens, then the model name.
    let mut levels = vec![];
    for keep in (1..=hints.len()).rev() {
        levels.push((ordered(hints[..keep].to_vec()), full.clone()));
    }
    levels.push((essential.join(" · "), compact));
    levels.push((essential[0].to_owned(), kv.clone()));
    for (left, right) in &levels {
        if left.width() + 3 + right.width() <= width {
            let pad = width - left.width() - right.width();
            return Line::styled(
                format!("{left}{}{right}", " ".repeat(pad)),
                Style::default().fg(muted()),
            );
        }
    }
    Line::styled(text::ellipsize(&kv, width), Style::default().fg(muted()))
}

use super::{
    Screen,
    input::wrap,
    state::label,
    text::{ACCENT, MUTED},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
};

/// Reserve only content that is currently interactive; completed text belongs
/// to native scrollback, so idle sessions do not keep a screenful of blank rows.
pub(super) fn height(screen: &Screen, columns: u16, rows: u16) -> u16 {
    let width = columns.saturating_sub(4) as usize;
    let body = if screen.session.is_none() {
        welcome_lines(screen)
            .into_iter()
            .flat_map(|line| super::text::wrap(line.spans, width))
            .count()
    } else {
        screen.transcript.active_lines(width).len().clamp(1, 8)
    };
    let choices = if let Some(choice) = &screen.skill_choices {
        choice.entries.len()
    } else if screen.picker {
        screen.recent.len()
    } else {
        screen.menu().len()
    };
    let menu = choices.min(6) + usize::from(choices > 0);
    let composer = (wrap(screen.input.text(), width.saturating_sub(2)).len() + 2).clamp(3, 6);
    let activity = screen.activity_lines().len().clamp(1, 6);
    (3 + body + activity + menu + composer + 1)
        .clamp(10, rows.max(10) as usize)
        .min(rows as usize) as u16
}

pub fn render(frame: &mut Frame, screen: &Screen) {
    if let Some(history) = &screen.history {
        history.render(frame);
        return;
    }
    if let Some(form) = &screen.setup {
        super::setup::render(frame, screen, form);
        return;
    }
    let area = frame.area();
    if area.width < 24 || area.height < 10 {
        frame.render_widget("Logit- Harness\n请扩大终端窗口", area);
        return;
    }
    let width = area.width.saturating_sub(4) as usize;
    let input_lines = wrap(screen.input.text(), width.saturating_sub(2));
    let input_height = (input_lines.len() as u16 + 2).clamp(3, (area.height / 3).max(3));
    let menu = screen.menu();
    let choices = if let Some(choice) = &screen.skill_choices {
        choice.entries.len()
    } else if screen.picker {
        screen.recent.len()
    } else {
        menu.len()
    };
    let menu_height = (choices.min(6) as u16 + u16::from(choices > 0))
        .min(area.height.saturating_sub(input_height + 7));
    let activities = screen.activity_lines();
    let activity_height = (activities.len() as u16).clamp(1, (area.height / 3).max(1));
    let [header, body, progress, menu_area, composer, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(activity_height),
        Constraint::Length(menu_height),
        Constraint::Length(input_height),
        Constraint::Length(1),
    ])
    .areas(area);
    super::brand::render(frame, screen, inset(header, 2));

    let content = inset(body, 2);
    let mut lines = screen.transcript.active_lines(width);
    if lines.is_empty() && screen.session.is_none() {
        lines = welcome_lines(screen)
            .into_iter()
            .flat_map(|line| super::text::wrap(line.spans, width))
            .collect();
    }
    let end = lines.len();
    let start = end.saturating_sub(content.height as usize);
    frame.render_widget(Paragraph::new(lines[start..end].to_vec()), content);

    frame.render_widget(
        Paragraph::new(
            activities
                .into_iter()
                .map(|line| Line::raw(super::state::clean(&line)))
                .collect::<Vec<_>>(),
        )
        .style(Style::default().fg(ACCENT)),
        inset(progress, 2),
    );

    if choices > 0 {
        let selected = screen.selection.min(choices - 1);
        let visible = menu_area.height.saturating_sub(1) as usize;
        let first = selected.saturating_sub(visible.saturating_sub(1));
        let mut rows = vec![];
        let labels = if let Some(choice) = &screen.skill_choices {
            choice
                .entries
                .iter()
                .map(|e| format!("{} · {} · {}", e.name, e.source, e.path.display()))
                .collect()
        } else if screen.picker {
            screen
                .recent
                .iter()
                .map(|recent| {
                    format!(
                        "{} [{}]  {}",
                        label(&recent.status),
                        recent.id.chars().take(12).collect::<String>(),
                        recent.objective.lines().next().unwrap_or("")
                    )
                })
                .collect::<Vec<_>>()
        } else {
            menu.iter()
                .map(|command| {
                    format!(
                        "{:<12} {} [{} · {:?}]",
                        command.name, command.description, command.source, command.timing
                    )
                })
                .collect()
        };
        for (index, text) in labels.iter().enumerate().skip(first).take(visible) {
            let style = if index == selected {
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(MUTED)
            };
            rows.push(Line::styled(
                format!("{} {text}", if index == selected { "❯" } else { " " }),
                style,
            ));
        }
        rows.push(Line::styled(
            "  ↑↓ 选择 · Enter 确认 · Tab 补全 · Esc 返回",
            Style::default().fg(MUTED),
        ));
        frame.render_widget(Paragraph::new(rows), inset(menu_area, 2));
    }

    let composer = inset(composer, 1);
    frame.render_widget(
        Block::new()
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_style(Style::default().fg(MUTED)),
        composer,
    );
    let editor = Rect::new(
        composer.x + 1,
        composer.y + 1,
        composer.width.saturating_sub(2),
        composer.height.saturating_sub(2),
    );
    let text_area = Rect::new(
        editor.x + 2,
        editor.y,
        editor.width.saturating_sub(2),
        editor.height,
    );
    frame.render_widget(
        Paragraph::new("❯").style(Style::default().fg(ACCENT)),
        Rect::new(editor.x, editor.y, 2, 1),
    );
    let (cursor_x, cursor_y) = screen.input.position(text_area.width as usize);
    let offset = cursor_y.saturating_sub(text_area.height.saturating_sub(1) as usize);
    let rows = if screen.input.text().is_empty() {
        vec![Line::styled(
            if screen.session.is_some() {
                "补充要求、纠正方向，或输入 /"
            } else {
                "输入消息、问题，或需要实现的目标…"
            },
            Style::default().fg(MUTED),
        )]
    } else {
        input_lines
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

    let hint = if let Some(session) = &screen.session {
        let hit = session
            .budget
            .reported
            .cache_hit()
            .map(|hit| format!("{:.1}%", hit * 100.0))
            .unwrap_or_else(|| "—".into());
        if area.width >= 90 {
            format!(
                "Enter 发送 · Alt+Enter 换行 · Ctrl+O 详情   {} tokens · KV {hit} · {}s",
                compact(session.budget.used),
                session.elapsed
            )
        } else {
            format!("/ 命令 · ↑↓ 历史 · KV {hit} · /budget")
        }
    } else if area.width >= 70 {
        "Enter 开始 · Alt+Enter 换行 · ↑↓ 历史 · Ctrl+D 退出".into()
    } else {
        "Enter 开始 · / 命令 · Ctrl+D 退出".into()
    };
    frame.render_widget(
        Paragraph::new(hint).style(Style::default().fg(MUTED)),
        inset(footer, 2),
    );
}

fn welcome_lines(screen: &Screen) -> Vec<Line<'static>> {
    if screen.session.is_some() {
        return vec![Line::styled(
            "正在读取会话记录…",
            Style::default().fg(MUTED),
        )];
    }
    let mut lines = vec![
        Line::default(),
        Line::styled(
            "从一段对话开始。",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Line::from("可以打招呼、提问，或交代需要实现的目标。"),
        Line::styled(
            "执行中可以随时插话；离开终端后，任务仍在后台继续。",
            Style::default().fg(MUTED),
        ),
        Line::default(),
    ];
    if !screen.key_present {
        lines.push(Line::styled(
            "尚未配置 API Key · 输入 /setup 设置",
            Style::default().fg(ACCENT),
        ));
        lines.push(Line::styled(
            "设置后即可开始任务，也可以先浏览已有会话。",
            Style::default().fg(MUTED),
        ));
        lines.push(Line::default());
    }
    if !screen.recent.is_empty() {
        lines.push(Line::styled(
            "最近的会话  /sessions",
            Style::default().fg(MUTED),
        ));
        for recent in screen.recent.iter().take(3) {
            lines.push(Line::from(format!(
                "  {}  {}",
                label(&recent.status),
                recent.objective.lines().next().unwrap_or("")
            )));
        }
    }
    lines
}

fn inset(area: Rect, margin: u16) -> Rect {
    Rect::new(
        area.x + margin,
        area.y,
        area.width.saturating_sub(margin * 2),
        area.height,
    )
}
fn compact(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}m", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1000 {
        format!("{:.1}k", tokens as f64 / 1000.0)
    } else {
        tokens.to_string()
    }
}

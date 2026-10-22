use super::{
    Screen,
    input::wrap,
    state::label,
    text::{ACCENT, BRAND, MUTED},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

pub fn render(frame: &mut Frame, screen: &Screen) {
    let area = frame.area();
    if area.width < 24 || area.height < 10 {
        frame.render_widget("Logit- Harness\n请扩大终端窗口", area);
        return;
    }
    let width = area.width.saturating_sub(4) as usize;
    let input_lines = wrap(screen.input.text(), width.saturating_sub(2));
    let input_height = (input_lines.len() as u16 + 2).clamp(3, (area.height / 3).max(3));
    let menu = screen.menu();
    let choices = if screen.picker {
        screen.recent.len()
    } else {
        menu.len()
    };
    let menu_height = (choices.min(6) as u16 + u16::from(choices > 0))
        .min(area.height.saturating_sub(input_height + 7));
    let [header, body, progress, menu_area, composer, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(menu_height),
        Constraint::Length(input_height),
        Constraint::Length(1),
    ])
    .areas(area);
    header_view(frame, screen, inset(header, 2));

    let content = inset(body, 2);
    let mut lines = screen.transcript.lines(width, screen.expanded);
    if lines.is_empty() {
        lines = welcome_lines(screen)
            .into_iter()
            .flat_map(|line| super::text::wrap(line.spans, width))
            .collect();
    }
    let end = lines
        .len()
        .saturating_sub(screen.scroll.min(lines.len().saturating_sub(1)));
    let start = end.saturating_sub(content.height as usize);
    frame.render_widget(Paragraph::new(lines[start..end].to_vec()), content);

    let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"][(screen.frame / 2) % 10];
    let status = if screen.scroll > 0 {
        format!("↑ 历史记录 · 距最新 {} 行 · PgDn 返回", screen.scroll)
    } else if let Some(active) = screen.transcript.active() {
        format!("{spinner} {active} · Ctrl+C 暂停")
    } else if !screen.activity.is_empty() {
        format!("{spinner} {} · 随时输入补充", screen.activity)
    } else if let Some(session) = &screen.session {
        format!(
            "{}{}",
            label(&session.status),
            if screen.connected {
                ""
            } else {
                " · 连接已断开"
            }
        )
    } else {
        "输入任务开始 · / 查看命令".into()
    };
    frame.render_widget(
        Paragraph::new(status).style(Style::default().fg(ACCENT)),
        inset(progress, 2),
    );

    if choices > 0 {
        let selected = screen.selection.min(choices - 1);
        let visible = menu_area.height.saturating_sub(1) as usize;
        let first = selected.saturating_sub(visible.saturating_sub(1));
        let mut rows = vec![];
        let labels = if screen.picker {
            screen
                .recent
                .iter()
                .map(|recent| {
                    format!(
                        "{}  {}",
                        label(&recent.status),
                        recent.objective.lines().next().unwrap_or("")
                    )
                })
                .collect::<Vec<_>>()
        } else {
            menu.iter()
                .map(|command| format!("{:<12} {}", command.name, command.description))
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
                "描述你想实现或修改的内容…"
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

fn header_view(frame: &mut Frame, screen: &Screen, area: Rect) {
    let model = if screen.model.is_empty() {
        "deepseek-flash"
    } else {
        &screen.model
    };
    let directory = screen.workspace.display().to_string();
    let directory = std::env::var("HOME")
        .ok()
        .and_then(|home| directory.strip_prefix(&home).map(|rest| format!("~{rest}")))
        .unwrap_or(directory);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    " L ",
                    Style::default()
                        .fg(Color::White)
                        .bg(BRAND)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "  Logit- Harness",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  {}", env!("CARGO_PKG_VERSION")),
                    Style::default().fg(MUTED),
                ),
            ]),
            Line::styled(
                format!("   {model} · {directory}"),
                Style::default().fg(MUTED),
            ),
        ]),
        area,
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
            "从一个任务开始。",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Line::from("描述目标，Logit 会检查项目、执行修改并验证结果。"),
        Line::styled(
            "执行中可以随时插话；离开终端后，任务仍在后台继续。",
            Style::default().fg(MUTED),
        ),
        Line::default(),
    ];
    if !screen.key_present {
        lines.push(Line::styled(
            "尚未配置 DEEPSEEK_API_KEY",
            Style::default().fg(ACCENT),
        ));
        lines.push(Line::styled(
            "配置后重新打开 loh。现在仍可浏览已有会话和命令。",
            Style::default().fg(MUTED),
        ));
        lines.push(Line::default());
    }
    if !screen.recent.is_empty() {
        lines.push(Line::styled(
            "最近的任务  /sessions",
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

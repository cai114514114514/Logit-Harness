use super::{Screen, input::wrap, state::label};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

pub fn render(frame: &mut Frame, screen: &Screen) {
    let area = frame.area();
    if area.width < 12 || area.height < 8 {
        frame.render_widget("请扩大终端窗口", area);
        return;
    }
    let width = area.width.saturating_sub(2) as usize;
    let (cursor_x, cursor_y) = screen.input.position(width);
    let input_lines = wrap(screen.input.text(), width);
    let input_height = (input_lines.len() as u16 + 2).clamp(3, (area.height / 3).max(3));
    let areas = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(input_height),
        Constraint::Length(1),
    ])
    .split(area);
    let accent = Color::Cyan;
    let status = screen
        .session
        .as_ref()
        .map(|state| label(&state.status))
        .unwrap_or("连接中");
    let connection = if screen.connected {
        ""
    } else {
        " · 已断开"
    };
    let id = screen
        .session
        .as_ref()
        .map(|state| state.id.as_str())
        .unwrap_or("");
    let title = Line::from(vec![
        Span::styled("Logit- Harness", Style::default().fg(accent)),
        Span::raw(format!("   {status}{connection}   {id}")),
    ]);
    let budget = screen
        .session
        .as_ref()
        .map(|state| {
            format!(
                "{} / {} token   ·   {} 秒   ·   随时输入纠正或补充",
                state.budget.used, state.budget.limit, state.elapsed
            )
        })
        .unwrap_or_default();
    frame.render_widget(
        Paragraph::new(vec![title, Line::from(budget)])
            .block(Block::default().borders(Borders::BOTTOM)),
        areas[0],
    );

    let mut lines = vec![];
    for entry in &screen.logs {
        lines.extend(wrap(entry, width));
        lines.push(String::new());
    }
    for (actor, text) in &screen.streams {
        lines.extend(wrap(&format!("{actor} › {text}"), width));
    }
    if lines.is_empty() {
        lines.push("任务在后台运行。输入 /help 查看控制命令。".into());
    }
    let height = areas[1].height as usize;
    let end = lines
        .len()
        .saturating_sub(screen.scroll.min(lines.len().saturating_sub(1)));
    let start = end.saturating_sub(height);
    let visible = lines[start..end]
        .iter()
        .cloned()
        .map(Line::from)
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(visible), areas[1]);

    let available = areas[2].height.saturating_sub(2) as usize;
    let offset = cursor_y.saturating_sub(available.saturating_sub(1));
    let input = input_lines
        .into_iter()
        .skip(offset)
        .map(Line::from)
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(input).block(
            Block::bordered()
                .title(" 输入 · Enter 发送，Alt+Enter 换行 ")
                .border_style(Style::default().fg(accent)),
        ),
        areas[2],
    );
    frame.set_cursor_position((
        areas[2].x + 1 + cursor_x as u16,
        areas[2].y + 1 + (cursor_y - offset) as u16,
    ));
    frame.render_widget(
        Paragraph::new("Ctrl-C 暂停  ·  Ctrl-D 断开  ·  PgUp/PgDn 滚动  ·  /help")
            .style(Style::default().fg(Color::DarkGray)),
        areas[3],
    );
}

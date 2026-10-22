use super::Form;
use crate::cli::terminal::{
    Screen, brand,
    text::{ACCENT, MUTED},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::Paragraph,
};

pub(crate) fn render(frame: &mut Frame, screen: &Screen, form: &Form) {
    let area = frame.area();
    if area.width < 24 || area.height < 17 {
        frame.render_widget("API 设置\n请扩大终端窗口 · Esc 返回", area);
        return;
    }
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(6),
        Constraint::Length(2),
    ])
    .areas(area);
    brand::render(
        frame,
        screen,
        Rect::new(
            header.x + 2,
            header.y,
            header.width.saturating_sub(4),
            header.height,
        ),
    );
    let body = Rect::new(
        body.x + 2,
        body.y,
        body.width.saturating_sub(4),
        body.height,
    );
    let width = body.width.saturating_sub(2).max(1) as usize;
    let url = form.endpoint.text();
    let (column, _) = form.endpoint.position(usize::MAX);
    let offset = if form.field == 0 {
        column.saturating_sub(width - 1)
    } else {
        0
    };
    let url: String = url.chars().skip(offset).take(width).collect();
    // Use a fixed mask for a previously saved key; never load it into the editor.
    let mask = if form.secret.text().is_empty() {
        if form.configured {
            "已配置 · 留空保留".into()
        } else {
            "粘贴 API Key，输入会隐藏".into()
        }
    } else {
        "*".repeat(form.secret.text().chars().count().min(width))
    };
    let field_style = |field| Style::default().fg(if field == form.field { ACCENT } else { MUTED });
    let mut lines = vec![
        Line::styled("API 设置", Style::default().add_modifier(Modifier::BOLD)),
        Line::from(""),
        Line::styled("API Base URL", field_style(0)),
        Line::from(format!("{} {url}", if form.field == 0 { "❯" } else { " " })),
        Line::from(""),
        Line::styled("API Key", field_style(1)),
        Line::styled(
            format!("{} {mask}", if form.field == 1 { "❯" } else { " " }),
            field_style(1),
        ),
        Line::from(""),
        Line::styled(
            if form.environment {
                "环境变量 DEEPSEEK_API_KEY 优先于保存的密钥"
            } else {
                "密钥单独保存在本机 credentials.json（权限 600）"
            },
            Style::default().fg(MUTED),
        ),
        Line::styled(
            "保存仅做本地校验，不发起模型请求。",
            Style::default().fg(MUTED),
        ),
    ];
    if let Some(error) = &form.error {
        lines.push(Line::styled(error.clone(), Style::default().fg(Color::Red)));
    }
    frame.render_widget(Paragraph::new(lines), body);
    frame.render_widget(
        Paragraph::new("  Tab / ↑↓ 切换 · Enter 下一项 / 保存\n  Ctrl+U 清空当前项 · Esc 取消")
            .style(Style::default().fg(MUTED)),
        footer,
    );
    let row = if form.field == 0 { 3 } else { 6 };
    if row < body.height {
        let cursor = if form.field == 0 {
            column - offset
        } else {
            form.secret.position(usize::MAX).0.min(width - 1)
        };
        frame.set_cursor_position((body.x + 2 + cursor as u16, body.y + row));
    }
}

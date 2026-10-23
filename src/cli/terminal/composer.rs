//! The same editor and measured usage footer serve every view.
use super::{
    Screen,
    input::wrap,
    text::{self, ACCENT, MUTED},
};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Block, Borders, Paragraph},
};

pub(super) fn render(frame: &mut Frame, area: Rect, screen: &Screen) {
    let composer = inset(area, 1);
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
    let input_lines = wrap(screen.input.text(), text_area.width as usize);
    let rows = if screen.input.text().is_empty() {
        vec![Line::styled(
            super::present::placeholder(screen),
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
}

fn inset(area: Rect, margin: u16) -> Rect {
    Rect::new(
        area.x + margin,
        area.y,
        area.width.saturating_sub(margin * 2),
        area.height,
    )
}

pub(super) fn footer(screen: &Screen, width: usize) -> Line<'static> {
    let hint = if screen.session.is_none() {
        "Enter 开始 · / 命令 · Ctrl-D 退出"
    } else if width >= 100 {
        "Enter 发送 · Alt+Enter 换行 · Ctrl-O 详情 · Ctrl-T 成员"
    } else {
        "/ 命令 · Ctrl-T 成员"
    };
    let model = if screen.model.is_empty() {
        "deepseek-flash"
    } else {
        &screen.model
    };
    let usage = screen.session.as_ref().and_then(|s| {
        if screen.view.is_main() {
            Some(&s.budget.reported)
        } else {
            screen.transcript.usage.get(screen.view.target.member_id())
        }
    });
    let kv = usage
        .and_then(|u| u.cache_hit())
        .map_or_else(|| "—".into(), |hit| format!("{:.0}%", hit * 100.));
    let tokens = usage.filter(|u| u.tokens() > 0).map_or_else(
        || "用量 —".into(),
        |u| {
            let t = u.tokens();
            if t >= 1000 {
                format!("{:.1}k tokens", t as f64 / 1000.)
            } else {
                format!("{t} tokens")
            }
        },
    );
    let line = if screen.session.is_none() {
        hint.into()
    } else if width >= 100 {
        format!("{hint}   {model} · {tokens} · KV {kv}")
    } else if width >= 60 {
        format!("{hint}   {model} · KV {kv}")
    } else {
        format!("/ 命令  {} · KV {kv}", text::ellipsize(model, 14))
    };
    Line::styled(text::ellipsize(&line, width), Style::default().fg(MUTED))
}

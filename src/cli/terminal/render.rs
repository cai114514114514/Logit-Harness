//! Compose one measured active region; completed output lives in native history.
use super::{Screen, chrome, composer, input::wrap, menu, status, text};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::Line,
    widgets::Paragraph,
};

struct Content {
    chrome: Vec<Line<'static>>,
    body: Vec<Line<'static>>,
    status: Vec<Line<'static>>,
    menu: Vec<Line<'static>>,
    composer: u16,
}
impl Content {
    fn measure(screen: &Screen, columns: u16, rows: u16) -> Self {
        let width = columns.saturating_sub(4) as usize;
        let chrome = chrome::lines(screen, width);
        let max_editor = rows.saturating_sub(chrome.len() as u16 + 1).clamp(3, 6);
        let composer = ((wrap(screen.input.text(), width.saturating_sub(2)).len() + 2).clamp(3, 6)
            as u16)
            .min(max_editor);
        let available = rows.saturating_sub(composer + 1 + chrome.len() as u16) as usize;
        let statuses = status::lines(screen, width, available.min(3));
        let available = available.saturating_sub(statuses.len());
        let menu = menu::lines(screen, width, available.min(8));
        let available = available.saturating_sub(menu.len());
        let mut body = screen.transcript.active_lines(width);
        if body.is_empty() && screen.session.is_none() && !screen.entered {
            body = welcome(screen, width);
        }
        let keep = available.min(8);
        if body.len() > keep {
            body.drain(..body.len() - keep);
        }
        Self {
            chrome,
            body,
            status: statuses,
            menu,
            composer,
        }
    }
    fn height(&self) -> u16 {
        (self.chrome.len()
            + self.body.len()
            + self.status.len()
            + self.menu.len()
            + self.composer as usize
            + 1) as u16
    }
}
pub(super) fn height(screen: &Screen, columns: u16, rows: u16) -> u16 {
    if screen.setup.is_some() {
        return rows.min(18);
    }
    Content::measure(screen, columns, rows)
        .height()
        .max(5)
        .min(rows)
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
    let minimum = chrome::lines(screen, area.width.saturating_sub(4) as usize).len() as u16 + 5;
    if area.width < 24 || area.height < minimum {
        frame.render_widget("loh · 请扩大终端窗口", area);
        return;
    }
    let content = Content::measure(screen, area.width, area.height);
    let [chrome, body, status, menu, composer, footer] = Layout::vertical([
        Constraint::Length(content.chrome.len() as u16),
        Constraint::Min(0),
        Constraint::Length(content.status.len() as u16),
        Constraint::Length(content.menu.len() as u16),
        Constraint::Length(content.composer),
        Constraint::Length(1),
    ])
    .areas(area);
    frame.render_widget(Paragraph::new(content.chrome), inset(chrome));
    let shown = content.body.len().min(body.height as usize);
    let body = Rect::new(
        body.x,
        body.bottom().saturating_sub(shown as u16),
        body.width,
        shown as u16,
    );
    frame.render_widget(Paragraph::new(content.body), inset(body));
    frame.render_widget(Paragraph::new(content.status), inset(status));
    frame.render_widget(Paragraph::new(content.menu), inset(menu));
    composer::render(frame, composer, screen);
    frame.render_widget(
        Paragraph::new(composer::footer(
            screen,
            area.width.saturating_sub(4) as usize,
        )),
        inset(footer),
    );
}
fn inset(area: Rect) -> Rect {
    Rect::new(
        area.x + 2,
        area.y,
        area.width.saturating_sub(4),
        area.height,
    )
}
fn welcome(screen: &Screen, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec!["输入消息开始对话，或用 / 查看命令。".to_string()];
    if !screen.key_present {
        lines.push("尚未配置 API Key · /setup 设置".into());
    }
    lines
        .into_iter()
        .flat_map(|line| {
            text::wrap(
                vec![ratatui::text::Span::styled(
                    line,
                    Style::default().fg(text::MUTED),
                )],
                width,
            )
        })
        .collect()
}

//! Compose one measured active region; completed output lives in native history.
//!
//! Columns follow the committed history above: speaker bars start at column 0,
//! secondary rows sit one column in, and the editor rule spans the full width.
use super::{Screen, chrome, composer, contract_bar, menu, status, text};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::Paragraph,
};

struct Content {
    /// One blank row between committed history and live activity.
    gap: u16,
    chrome: Vec<Line<'static>>,
    contract: Vec<Line<'static>>,
    body: Vec<Line<'static>>,
    status: Vec<Line<'static>>,
    menu: Vec<Line<'static>>,
    composer: u16,
}
impl Content {
    fn measure(screen: &Screen, columns: u16, rows: u16) -> Self {
        let side = columns.saturating_sub(2) as usize;
        let chrome = chrome::lines(screen, side);
        let contract = contract_bar::lines(screen, side);
        let fixed = (chrome.len() + contract.len()) as u16;
        let composer = composer::height(screen, columns).min(rows.saturating_sub(fixed + 1).max(2));
        let available = rows.saturating_sub(composer + 1 + fixed) as usize;
        let statuses = status::lines(screen, side, available.min(3));
        let available = available.saturating_sub(statuses.len());
        let menu = menu::lines(screen, side, available.min(8));
        let available = available.saturating_sub(menu.len());
        let mut body = screen
            .transcript
            .active_lines(columns.saturating_sub(1) as usize);
        if body.is_empty() && screen.session.is_none() && !screen.entered {
            body = welcome(screen, columns.saturating_sub(1) as usize);
        }
        let keep = available.min(8);
        if body.len() > keep {
            body.drain(..body.len() - keep);
        }
        let live =
            !(chrome.is_empty() && contract.is_empty() && body.is_empty() && statuses.is_empty());
        let gap = u16::from(screen.session.is_some() && live && available > body.len());
        Self {
            gap,
            chrome,
            contract,
            body,
            status: statuses,
            menu,
            composer,
        }
    }
    fn height(&self) -> u16 {
        (self.gap as usize
            + self.chrome.len()
            + self.contract.len()
            + self.body.len()
            + self.status.len()
            + self.menu.len()
            + self.composer as usize
            + 1) as u16
    }
}
/// Rows the active region needs; the rest of the terminal is native history.
pub fn height(screen: &Screen, columns: u16, rows: u16) -> u16 {
    if screen.setup.is_some() {
        return rows.min(18);
    }
    Content::measure(screen, columns, rows)
        .height()
        .max(4)
        .min(rows)
}
pub fn render(frame: &mut Frame, screen: &Screen) {
    if let Some(detail) = &screen.contract {
        detail.render(frame);
        return;
    }
    if let Some(history) = &screen.history {
        history.render(frame);
        return;
    }
    if let Some(form) = &screen.setup {
        super::setup::render(frame, screen, form);
        return;
    }
    let area = frame.area();
    let minimum = chrome::lines(screen, area.width.saturating_sub(2) as usize).len() as u16 + 4;
    if area.width < 24 || area.height < minimum {
        frame.render_widget("loh · 请扩大终端窗口", area);
        return;
    }
    let content = Content::measure(screen, area.width, area.height);
    let [_, chrome, contract, body, status, menu, composer, footer] = Layout::vertical([
        Constraint::Length(content.gap),
        Constraint::Length(content.chrome.len() as u16),
        Constraint::Length(content.contract.len() as u16),
        Constraint::Min(0),
        Constraint::Length(content.status.len() as u16),
        Constraint::Length(content.menu.len() as u16),
        Constraint::Length(content.composer),
        Constraint::Length(1),
    ])
    .areas(area);
    frame.render_widget(Paragraph::new(content.chrome), inset(chrome));
    frame.render_widget(Paragraph::new(content.contract), inset(contract));
    let shown = content.body.len().min(body.height as usize);
    let body = Rect::new(
        body.x,
        body.bottom().saturating_sub(shown as u16),
        body.width.saturating_sub(1),
        shown as u16,
    );
    frame.render_widget(Paragraph::new(content.body), body);
    frame.render_widget(Paragraph::new(content.status), inset(status));
    frame.render_widget(Paragraph::new(content.menu), inset(menu));
    composer::render(frame, composer, screen);
    frame.render_widget(
        Paragraph::new(composer::footer(
            screen,
            area.width.saturating_sub(2) as usize,
        )),
        inset(footer),
    );
}
/// Secondary rows: one column in from each side.
fn inset(area: Rect) -> Rect {
    Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height,
    )
}
fn welcome(screen: &Screen, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![
        "说出目标，Logit 会把它记成一份可核对的契约。".to_string(),
        "打招呼和提问不会建立契约；执行中随时插话，离开终端也会继续。".into(),
    ];
    if !screen.key_present {
        lines.push("尚未配置 API Key · /setup 设置".into());
    }
    let mut rows = vec![Line::default()];
    rows.extend(lines.into_iter().flat_map(|line| {
        text::wrap(vec![Span::raw(line)], width.saturating_sub(2))
            .into_iter()
            .map(|mut row| {
                row.spans.insert(0, Span::raw("  "));
                row
            })
    }));
    rows.push(Line::default());
    rows
}

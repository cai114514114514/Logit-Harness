//! A terminal-native L mark: three rows, beside the product, model and project.
use super::{
    Screen,
    text::{ACCENT, MUTED},
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

// Block characters retain the silhouette when NO_COLOR disables ANSI styling.
// The half-height tip echoes the diagonal terminal of the app icon.
const MARK: [&str; 3] = ["██      ", "██      ", "██████▀ "];

pub(super) fn render(frame: &mut Frame, screen: &Screen, area: Rect) {
    let model = if screen.model.is_empty() {
        "deepseek-flash"
    } else {
        &screen.model
    };
    let directory = std::env::var_os("HOME")
        .and_then(|home| {
            screen
                .workspace
                .strip_prefix(home)
                .ok()
                .map(|path| format!("~/{}", path.display()))
        })
        .unwrap_or_else(|| screen.workspace.display().to_string());
    let compact = area.width < 34;
    let title = if compact {
        "loh".into()
    } else {
        format!("Logit- Harness  {}", env!("CARGO_PKG_VERSION"))
    };
    let details = [title, model.to_owned(), directory];
    let lines = MARK
        .iter()
        .zip(details)
        .enumerate()
        .map(|(row, (mark, detail))| {
            Line::from(vec![
                Span::styled(*mark, Style::default().fg(ACCENT)),
                Span::styled(
                    detail,
                    if row == 0 {
                        Style::default().add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(MUTED)
                    },
                ),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), area);
}

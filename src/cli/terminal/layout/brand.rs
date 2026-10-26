//! A terminal-native L mark: three rows, beside the product, model and project.
use super::{
    Screen,
    text::{accent, muted},
};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

// Block characters retain the silhouette when NO_COLOR disables ANSI styling.
// The half-height tip echoes the diagonal terminal of the app icon.
const MARK: [&str; 3] = ["██      ", "██      ", "██████▀ "];

pub(super) fn lines(screen: &Screen, width: u16) -> Vec<Line<'static>> {
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
    let compact = width < 34;
    let title = if compact {
        "loh".into()
    } else {
        format!("Logit- Harness  {}", env!("CARGO_PKG_VERSION"))
    };
    let details = [title, model.to_owned(), directory];
    MARK.iter()
        .zip(details)
        .enumerate()
        .map(|(row, (mark, detail))| {
            Line::from(vec![
                Span::styled(*mark, Style::default().fg(accent())),
                Span::styled(
                    detail,
                    if row == 0 {
                        Style::default().add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(muted())
                    },
                ),
            ])
        })
        .map(|line| {
            line.spans
                .into_iter()
                .map(|mut span| {
                    span.content = super::state::clean(&span.content).into();
                    span
                })
                .collect::<Vec<_>>()
        })
        .flat_map(|spans| super::text::wrap(spans, width as usize))
        .collect()
}

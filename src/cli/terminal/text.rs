use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub const ACCENT: Color = Color::Rgb(212, 151, 94);
pub const MUTED: Color = Color::DarkGray;

/// Small, predictable terminal Markdown: headings, fenced code, inline code and
/// emphasis. No terminal escape from model/tool output is ever interpreted.
pub fn markdown(text: &str, width: usize, base: Style) -> Vec<Line<'static>> {
    let mut code = false;
    let mut lines = vec![];
    for raw in text.lines() {
        if raw.trim_start().starts_with("```") {
            code = !code;
            if code {
                lines.push(Line::styled(
                    format!("  {}", raw.trim_start().trim_start_matches('`')),
                    Style::default().fg(MUTED),
                ));
            }
            continue;
        }
        let (text, style) = if code {
            (raw.to_owned(), base.fg(Color::Cyan))
        } else if raw.starts_with('#') {
            (
                raw.trim_start_matches('#').trim_start().to_owned(),
                base.add_modifier(Modifier::BOLD),
            )
        } else {
            (raw.to_owned(), base)
        };
        let spans = if code {
            vec![Span::styled(text, style)]
        } else {
            inline(&text, style)
        };
        lines.extend(wrap(spans, width.max(1)));
    }
    if lines.is_empty() {
        lines.push(Line::default());
    }
    lines
}

fn inline(text: &str, base: Style) -> Vec<Span<'static>> {
    let mut spans = vec![];
    let (mut bold, mut code) = (false, false);
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(next) = rest.strip_prefix("**") {
            bold = !bold;
            rest = next;
            continue;
        }
        if let Some(next) = rest.strip_prefix('`') {
            code = !code;
            rest = next;
            continue;
        }
        let end = rest
            .char_indices()
            .skip(1)
            .find(|(i, _)| rest[*i..].starts_with("**") || rest[*i..].starts_with('`'))
            .map_or(rest.len(), |(i, _)| i);
        let mut style = base;
        if bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if code {
            style = style.fg(Color::Cyan);
        }
        spans.push(Span::styled(rest[..end].to_owned(), style));
        rest = &rest[end..];
    }
    spans
}

pub(super) fn wrap(spans: Vec<Span<'static>>, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![];
    let mut line = Line::default();
    let mut column = 0;
    for span in spans {
        let mut part = String::new();
        for symbol in span.content.graphemes(true) {
            let size = if symbol == "\t" { 4 } else { symbol.width() };
            if column + size > width && column > 0 {
                line.spans
                    .push(Span::styled(std::mem::take(&mut part), span.style));
                lines.push(std::mem::take(&mut line));
                column = 0;
            }
            part.push_str(if symbol == "\t" { "    " } else { symbol });
            column += size;
        }
        line.spans.push(Span::styled(part, span.style));
    }
    lines.push(line);
    lines
}

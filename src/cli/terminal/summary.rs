//! Compress only an uncommitted adjacent batch; native history is immutable.
use super::super::{text, tool_text};
use super::{Entry, Kind};
use ratatui::{style::Style, text::Line};

fn groupable(entry: &Entry) -> bool {
    entry.kind == Kind::Tool
        && !entry.running
        && entry.success == Some(true)
        && entry.turn.is_some()
        && entry.response.is_some()
        && entry
            .result
            .as_ref()
            .and_then(tool_text::limitation)
            .is_none()
        && matches!(
            entry.tool.as_deref(),
            Some("read_file" | "search_text" | "search_files" | "list_files")
        )
}
fn together(a: &Entry, b: &Entry) -> bool {
    groupable(b)
        && a.origin == b.origin
        && a.source == b.source
        && a.turn == b.turn
        && a.response == b.response
}
pub(super) fn batch(entries: &[&Entry], width: usize) -> Vec<Line<'static>> {
    let mut rows = vec![];
    let mut cursor = 0;
    while cursor < entries.len() {
        let first = entries[cursor];
        let mut end = cursor + 1;
        if groupable(first) {
            while end < entries.len() && together(first, entries[end]) {
                end += 1;
            }
        }
        let count = end - cursor;
        let visible = if count > 3 { 2 } else { count };
        for entry in &entries[cursor..cursor + visible] {
            rows.extend(entry.lines(width, false));
        }
        if count > visible {
            rows.push(Line::styled(
                text::ellipsize(
                    &format!("  … 另 {} 个读取/搜索步骤 · /history", count - visible),
                    width,
                ),
                Style::default().fg(text::MUTED),
            ));
        }
        cursor = end;
    }
    rows
}

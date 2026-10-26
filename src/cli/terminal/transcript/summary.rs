//! Compress only an uncommitted adjacent batch; native history is immutable.
use super::super::{text, tool_text};
use super::{Entry, Kind, entry::Voice};
use ratatui::{
    style::Style,
    text::{Line, Span},
};

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

/// What the previous committed block was, so the next batch knows whether a
/// blank line belongs between them. Native history cannot be inspected later.
#[derive(Clone, Debug, Default)]
pub(super) struct Last {
    speaker: Option<String>,
    standalone: bool,
}

/// Notes and ledger cards stand apart; utterances and steps of one speaker
/// stay together, so a reply, its card and its tool steps read as one turn.
fn standalone(entry: &Entry) -> bool {
    matches!(entry.kind, Kind::Note | Kind::Ledger)
}

fn speaker(entry: &Entry, voice: &Voice) -> String {
    match entry.kind {
        Kind::Note => "note".into(),
        Kind::Ledger => entry.card().map_or("main", |card| card.speaker()).into(),
        _ => voice.speaker.clone(),
    }
}

pub(super) fn batch(
    entries: &[&Entry],
    width: usize,
    voice: &dyn Fn(&Entry) -> Voice,
    last: &mut Last,
) -> Vec<Line<'static>> {
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
            let voice = voice(entry);
            let lines = entry.compact(width, &voice);
            if lines.is_empty() {
                continue;
            }
            let who = speaker(entry, &voice);
            let apart = standalone(entry);
            // Ledger cards attach to the turn before them; only what follows a
            // card is set apart from it.
            let separate = last.speaker.as_ref().is_some_and(|previous| {
                *previous != who || last.standalone || apart && entry.kind == Kind::Note
            });
            if separate {
                rows.push(Line::default());
            }
            rows.extend(lines);
            *last = Last {
                speaker: Some(who),
                standalone: apart,
            };
        }
        if count > visible {
            let voice = voice(first);
            rows.push(Line::from(vec![
                Span::styled(format!("{} ", voice.bar), Style::default().fg(voice.color)),
                Span::styled(
                    text::ellipsize(
                        &format!("… 另 {} 个读取/搜索步骤 · /history", count - visible),
                        width.saturating_sub(2),
                    ),
                    Style::default().fg(text::muted()),
                ),
            ]));
        }
        cursor = end;
    }
    rows
}

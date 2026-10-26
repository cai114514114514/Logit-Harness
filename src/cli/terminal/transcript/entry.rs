//! Entry presentation; retained raw records are shared by history, search and copy.
//!
//! Every visual line of an utterance starts with its speaker's bar. Heading
//! lines such as `❯ 你` or `● Logit` are gone: the bar already says who speaks.
use super::super::{present, state::clean, text, tool_text};
use super::{Entry, Kind};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

/// Assistant replies longer than this fold into /history.
const REPLY_LINES: usize = 100;
/// Visual rows of a failed or running tool's output kept in the summary.
const TAIL_ROWS: usize = 4;

/// Who is speaking, drawn as a bar at the start of every visual line.
#[derive(Clone, Debug, PartialEq)]
pub struct Voice {
    pub bar: &'static str,
    pub color: Color,
    /// A member speaking in the main view is named on its first line.
    pub name: Option<String>,
    /// Spacing key: a blank line separates different speakers.
    pub speaker: String,
}

impl Voice {
    pub fn you() -> Self {
        Self {
            bar: "▌",
            color: text::accent(),
            name: None,
            speaker: "you".into(),
        }
    }
    pub fn main() -> Self {
        Self {
            bar: "│",
            color: text::muted(),
            name: None,
            speaker: "main".into(),
        }
    }
    pub fn verifier() -> Self {
        Self {
            bar: "║",
            color: text::fresh(),
            name: None,
            speaker: "verify".into(),
        }
    }
    pub fn member(id: &str, index: usize, name: Option<String>) -> Self {
        Self {
            bar: "┃",
            color: super::super::theme::member(index),
            name,
            speaker: id.into(),
        }
    }
    /// The speaker of an entry when roster names and order are unknown.
    pub fn of(entry: &Entry) -> Self {
        if entry.kind == Kind::User {
            Self::you()
        } else if !entry.origin.is_main() {
            Self::member(entry.origin.target.member_id(), 0, None)
        } else if entry.source.starts_with("verify") {
            Self::verifier()
        } else {
            Self::main()
        }
    }
    fn width(&self) -> usize {
        2 + self.name.as_ref().map_or(0, |name| name.width() + 3)
    }
    /// The bar, plus the member name on the first line or matching indent after it.
    fn prefix(&self, first: bool) -> Vec<Span<'static>> {
        let style = Style::default().fg(self.color);
        let mut spans = vec![Span::styled(format!("{} ", self.bar), style)];
        if let Some(name) = &self.name {
            spans.push(if first {
                Span::styled(format!("{name} · "), style)
            } else {
                Span::raw(" ".repeat(name.width() + 3))
            });
        }
        spans
    }
    fn line(&self, first: bool, line: Line<'static>) -> Line<'static> {
        if line.spans.iter().all(|s| s.content.trim().is_empty()) {
            // Paragraph gaps inside one utterance carry no bar.
            return Line::default();
        }
        let mut spans = self.prefix(first);
        spans.extend(line.spans);
        Line::from(spans)
    }
}

impl Entry {
    /// Search, expansion and both copy routes share the same complete record.
    /// JSON retains process flags and bounded-result provenance, even for quiet
    /// tools; the leading stream is preserved byte-for-byte without escaping.
    pub fn full_text(&self) -> String {
        if self.kind == Kind::Ledger {
            return self.body.clone();
        }
        match &self.result {
            Some(result) => {
                let mut metadata = serde_json::to_string_pretty(result).unwrap_or_default();
                if tool_text::truncated(result) {
                    metadata.push_str("\n[工具提供的结果内容已截断；以上持久化流如有则完整保留]");
                }
                if !self.body.is_empty() && self.body != metadata {
                    format!("{}\n\n[最终结果]\n{metadata}", self.body)
                } else {
                    metadata
                }
            }
            None => self.body.clone(),
        }
    }

    fn display_text(&self, expanded: bool) -> String {
        if expanded {
            return self.full_text();
        }
        if self.kind == Kind::Note {
            return present::notice(&self.body);
        }
        if let Some(result) = &self.result {
            let final_text = tool_text::result(result);
            if !final_text.is_empty() {
                // Keep the existing compact final-result/failure-tail view.
                // Full streams remain available through expansion and copy.
                return final_text;
            }
        }
        self.body.clone()
    }

    pub fn lines(&self, width: usize, expanded: bool) -> Vec<Line<'static>> {
        if expanded {
            self.details(width)
        } else {
            self.compact(width, &Voice::of(self))
        }
    }

    /// Native-history form: bars, one-line tools, folded long output.
    pub fn compact(&self, width: usize, voice: &Voice) -> Vec<Line<'static>> {
        if self.quiet || self.kind == Kind::Assistant && self.body.is_empty() {
            return vec![];
        }
        match self.kind {
            Kind::Tool => self.tool_lines(width, voice),
            Kind::Ledger => self.card().map_or_else(Vec::new, |card| card.lines(width)),
            Kind::Note => clean(&self.display_text(false))
                .split('\n')
                .flat_map(|line| text::wrap(vec![Span::raw(line.to_owned())], width))
                .collect(),
            Kind::User | Kind::Assistant => self.utterance(width, voice),
        }
    }

    pub(super) fn card(&self) -> Option<super::super::ledger::Card> {
        self.result
            .as_ref()
            .and_then(|value| serde_json::from_value(value.clone()).ok())
    }

    fn utterance(&self, width: usize, voice: &Voice) -> Vec<Line<'static>> {
        let room = width.saturating_sub(voice.width()).max(1);
        let content = clean(&self.display_text(false));
        let body = if self.kind == Kind::Assistant {
            text::markdown(&content, room, Style::default())
        } else {
            content
                .split('\n')
                .flat_map(|line| text::wrap(vec![Span::raw(line.to_owned())], room))
                .collect()
        };
        let shown = body.len().min(REPLY_LINES);
        let mut lines = body
            .iter()
            .take(shown)
            .enumerate()
            .map(|(i, line)| voice.line(i == 0, line.clone()))
            .collect::<Vec<_>>();
        if body.len() > shown {
            let notice = vec![Span::styled(
                format!("… 另 {} 行已折叠 · /history", body.len() - shown),
                Style::default().fg(text::muted()),
            )];
            lines.extend(
                text::wrap(notice, room)
                    .into_iter()
                    .map(|line| voice.line(false, line)),
            );
        }
        lines
    }

    /// `✓ 读取  path ··· 48 行 · 0.1s · r2`, with the failure or live tail below.
    fn tool_lines(&self, width: usize, voice: &Voice) -> Vec<Line<'static>> {
        let (mark, color) = match (self.running, self.success) {
            (true, _) => ("⋯", text::accent()),
            (_, Some(true)) => ("✓", text::fresh()),
            (_, Some(false)) => ("✕", text::fail()),
            _ => ("?", text::muted()),
        };
        let now = chrono::Utc::now().to_rfc3339();
        let finished = if self.running { &now } else { &self.updated };
        let right = [
            self.result.as_ref().and_then(tool_text::outcome),
            tool_text::elapsed(&self.started, finished),
            (!self.clauses.is_empty()).then(|| self.clauses.join(" ")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        let room = width.saturating_sub(voice.width());
        let title = clean(&self.title);
        // The verb and the start of the object come before measurements.
        let right = if title.width().min(16) + 2 + right.width() + 2 > room {
            String::new()
        } else {
            right
        };
        let reserved = if right.is_empty() {
            0
        } else {
            right.width() + 2
        };
        let title = text::ellipsize(&title, room.saturating_sub(2 + reserved));
        let pad = room.saturating_sub(2 + title.width() + right.width());
        let right_color = if self.success == Some(false) {
            text::fail()
        } else {
            text::muted()
        };
        let mut lines = vec![voice.line(
            true,
            Line::from(vec![
                Span::styled(format!("{mark} "), Style::default().fg(color)),
                Span::raw(title),
                Span::raw(" ".repeat(pad)),
                Span::styled(right, Style::default().fg(right_color)),
            ]),
        )];
        let indent = "    ";
        let tail_room = room.saturating_sub(indent.width());
        if self.running || self.success != Some(true) {
            // The final result's readable text wins; a live or result-less step
            // shows its stream. Structured JSON stays in details.
            let readable = self
                .result
                .as_ref()
                .map(tool_text::readable)
                .unwrap_or_default();
            let content = clean(if !readable.is_empty() {
                &readable
            } else if self.streamed || self.result.is_none() {
                &self.body
            } else {
                ""
            });
            let rows = content
                .lines()
                .flat_map(|line| text::wrap(vec![Span::raw(line.to_owned())], tail_room))
                .collect::<Vec<_>>();
            if rows.len() > TAIL_ROWS && !self.running {
                lines.push(voice.line(
                    false,
                    Line::styled(
                        text::ellipsize(&format!("{indent}/history · 完整输出"), room),
                        Style::default().fg(text::muted()),
                    ),
                ));
            }
            for row in rows.iter().skip(rows.len().saturating_sub(TAIL_ROWS)) {
                let mut spans = vec![Span::raw(indent)];
                spans.extend(row.spans.clone());
                lines.push(voice.line(false, Line::from(spans)));
            }
        }
        if let Some(notice) = self.result.as_ref().and_then(tool_text::limitation) {
            lines.push(voice.line(
                false,
                Line::styled(
                    text::ellipsize(&format!("{indent}{notice} · /history"), room),
                    Style::default().fg(text::muted()),
                ),
            ));
        }
        lines
    }

    /// Detail form for the history browser: exact attribution, time, full text.
    fn details(&self, width: usize) -> Vec<Line<'static>> {
        let (marker, color) = match self.kind {
            Kind::User => ("▌", text::accent()),
            Kind::Assistant => (Voice::of(self).bar, Color::Reset),
            Kind::Note | Kind::Ledger => ("·", text::muted()),
            Kind::Tool if self.running => ("⋯", text::accent()),
            Kind::Tool if self.success == Some(true) => ("✓", text::fresh()),
            Kind::Tool if self.success == Some(false) => ("✕", text::fail()),
            Kind::Tool => ("?", text::muted()),
        };
        let time = chrono::DateTime::parse_from_rfc3339(&self.started)
            .map(|time| {
                time.with_timezone(&chrono::Local)
                    .format("%H:%M:%S")
                    .to_string()
            })
            .unwrap_or_else(|_| self.started.clone());
        let mut lines = text::wrap(
            vec![Span::styled(
                format!(
                    "{marker} {} · {} · {time}",
                    clean(&self.title),
                    clean(&self.source)
                ),
                Style::default().fg(color),
            )],
            width,
        );
        let content = self.full_text();
        let body: Vec<Line<'static>> = if self.kind == Kind::Assistant {
            text::markdown(&clean(&content), width.saturating_sub(2), Style::default())
        } else {
            clean(&content)
                .split('\n')
                .flat_map(|line| {
                    text::wrap(vec![Span::raw(line.to_owned())], width.saturating_sub(2))
                })
                .collect()
        };
        for mut line in body {
            line.spans.insert(0, Span::raw("  "));
            lines.push(line);
        }
        lines.push(Line::default());
        lines
    }
}

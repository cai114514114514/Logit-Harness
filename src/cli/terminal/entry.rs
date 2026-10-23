//! Entry presentation; retained raw records are shared by history, search and copy.
use super::super::{present, state::clean, text, tool_text};
use super::{Entry, Kind};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

impl Entry {
    /// Search, expansion and both copy routes share the same complete record.
    /// JSON retains process flags and bounded-result provenance, even for quiet
    /// tools; the leading stream is preserved byte-for-byte without escaping.
    pub fn full_text(&self) -> String {
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
        if self.quiet && !expanded {
            return vec![];
        }
        if self.kind == Kind::Assistant && self.body.is_empty() {
            return vec![];
        }
        if self.kind == Kind::Tool && !expanded {
            return self.tool_lines(width);
        }
        let (marker, color) = match self.kind {
            Kind::User => ("❯", text::ACCENT),
            Kind::Assistant => ("●", Color::Reset),
            Kind::Note => ("·", text::MUTED),
            Kind::Tool if self.running => ("◌", text::ACCENT),
            Kind::Tool if self.success == Some(true) => ("✓", Color::Green),
            Kind::Tool if self.success == Some(false) => ("✕", Color::Red),
            Kind::Tool => ("?", text::MUTED),
        };
        let time = chrono::DateTime::parse_from_rfc3339(&self.started)
            .map(|time| {
                time.with_timezone(&chrono::Local)
                    .format("%H:%M:%S")
                    .to_string()
            })
            .unwrap_or_else(|_| self.started.clone());
        let heading = if expanded {
            format!(
                "{marker} {} · {} · {time}",
                clean(&self.title),
                clean(&self.source)
            )
        } else if self.kind == Kind::Note {
            // The notice itself supplies its subject; do not repeat 系统 twice.
            String::new()
        } else {
            format!("{marker} {}", clean(&self.title))
        };
        let heading_width = if !expanded && self.kind == Kind::Tool {
            width.saturating_sub(2)
        } else {
            width
        };
        let mut lines = text::wrap(
            vec![Span::styled(heading, Style::default().fg(color))],
            heading_width,
        );
        if !expanded && self.kind == Kind::Tool && lines.len() > 1 {
            lines.truncate(1);
            lines[0]
                .spans
                .push(Span::styled(" …", Style::default().fg(color)));
        }
        if !expanded && self.kind == Kind::Note {
            lines.clear();
        }
        let content = self.display_text(expanded);
        let body = if self.kind == Kind::Assistant {
            text::markdown(&clean(&content), width.saturating_sub(2), Style::default())
        } else {
            clean(&content)
                .split('\n')
                .flat_map(|line| {
                    text::wrap(vec![Span::raw(line.to_owned())], width.saturating_sub(2))
                })
                .collect()
        };
        let limit = if expanded || self.kind == Kind::Note {
            body.len()
        } else if self.kind == Kind::Tool {
            if self.success == Some(false) { 8 } else { 1 }
        } else {
            body.len().min(100)
        };
        let start = if self.kind == Kind::Tool && self.success == Some(false) {
            body.len().saturating_sub(limit)
        } else {
            0
        };
        if start > 0 {
            lines.push(Line::raw(format!("  … 前 {start} 行已折叠 · /history")));
        }
        for mut line in body.iter().skip(start).take(limit).cloned() {
            line.spans.insert(0, Span::raw("  "));
            lines.push(line);
        }
        if start == 0 && body.len() > limit {
            lines.push(Line::raw(format!(
                "  … 另 {} 行已折叠 · /history",
                body.len() - limit
            )));
        }
        if !expanded
            && self.kind == Kind::Tool
            && let Some(notice) = self.result.as_ref().and_then(tool_text::limitation)
        {
            lines.push(Line::styled(
                format!("  {notice} · /history"),
                Style::default().fg(text::MUTED),
            ));
        }
        lines.push(Line::default());
        lines
            .into_iter()
            .flat_map(|line| text::wrap(line.spans, width))
            .collect()
    }
    fn tool_lines(&self, width: usize) -> Vec<Line<'static>> {
        let (mark, color) = match (self.running, self.success) {
            (true, _) => ("◌", text::ACCENT),
            (_, Some(true)) => ("✓", Color::Green),
            (_, Some(false)) => ("✕", Color::Red),
            _ => ("?", text::MUTED),
        };
        let suffix = self
            .result
            .as_ref()
            .map(tool_text::suffix)
            .unwrap_or_default();
        let title = text::ellipsize(
            &format!("{mark} {}", clean(&self.title)),
            width.saturating_sub(suffix.width()),
        );
        let mut lines = vec![Line::styled(
            format!("{title}{suffix}"),
            Style::default().fg(color),
        )];
        if self.running || self.success != Some(true) {
            let content = clean(&self.display_text(false));
            let rows = content
                .lines()
                .flat_map(|line| text::wrap(vec![Span::raw(format!("  {line}"))], width))
                .collect::<Vec<_>>();
            if rows.len() > 4 && !self.running {
                lines.push(Line::styled(
                    text::ellipsize("  /history · 完整输出", width),
                    Style::default().fg(text::MUTED),
                ));
            }
            lines.extend(
                rows.into_iter()
                    .rev()
                    .take(4)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev(),
            );
        }
        if let Some(notice) = self.result.as_ref().and_then(tool_text::limitation) {
            lines.extend(text::wrap(
                vec![Span::styled(
                    format!("  {notice} · /history"),
                    Style::default().fg(text::MUTED),
                )],
                width,
            ));
        }
        lines
    }
}

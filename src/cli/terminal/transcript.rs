use super::{
    state::clean,
    text::{self, ACCENT, MUTED},
};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use serde_json::Value;
use std::collections::VecDeque;

#[derive(Clone, PartialEq)]
enum Kind {
    User,
    Assistant,
    Tool,
    Note,
}

struct Entry {
    key: String,
    kind: Kind,
    title: String,
    body: String,
    success: Option<bool>,
    running: bool,
}

#[derive(Default)]
pub struct Transcript {
    entries: VecDeque<Entry>,
}

impl Transcript {
    pub fn settle(&mut self) {
        for entry in &mut self.entries {
            entry.running = false;
        }
    }
    fn push(&mut self, entry: Entry) {
        self.entries.push_back(entry);
        // Keep bounded visible history; the complete history remains in SQLite.
        while self.entries.len() > 300
            || self
                .entries
                .iter()
                .map(|entry| entry.body.len())
                .sum::<usize>()
                > 250_000
        {
            let Some(index) = self.entries.iter().position(|entry| !entry.running) else {
                break;
            };
            self.entries.remove(index);
        }
    }
    pub fn note(&mut self, text: &str) {
        self.push(Entry {
            key: String::new(),
            kind: Kind::Note,
            title: String::new(),
            body: clean(text).chars().take(20_000).collect(),
            success: None,
            running: false,
        });
    }
    pub fn user(&mut self, text: &str) {
        self.push(Entry {
            key: String::new(),
            kind: Kind::User,
            title: "你".into(),
            body: clean(text).chars().take(20_000).collect(),
            success: None,
            running: false,
        });
    }
    pub fn message(&mut self, actor: &str, text: &str) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .rev()
            .find(|entry| entry.key == actor && entry.running)
        {
            entry.body = clean(text).chars().take(30_000).collect();
            entry.running = false;
        } else {
            self.push(Entry {
                key: actor.into(),
                kind: Kind::Assistant,
                title: actor_label(actor),
                body: clean(text).chars().take(30_000).collect(),
                success: None,
                running: false,
            });
        }
    }
    pub fn delta(&mut self, actor: &str, text: &str) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .rev()
            .find(|entry| entry.key == actor && entry.running)
        {
            entry.body.push_str(&clean(text));
            if entry.body.len() > 30_000 {
                entry.body = entry
                    .body
                    .chars()
                    .rev()
                    .take(12_000)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
            }
        } else {
            self.push(Entry {
                key: actor.into(),
                kind: Kind::Assistant,
                title: actor_label(actor),
                body: clean(text),
                success: None,
                running: true,
            });
        }
    }
    pub fn start(&mut self, id: &str, tool: &str, arguments: &Value) {
        let arguments = crate::tools::approval::redact(arguments);
        let detail = ["command", "path", "query", "url", "name"]
            .iter()
            .find_map(|key| arguments[key].as_str())
            .unwrap_or("");
        let name = match tool {
            "read_file" => "Read",
            "search_files" | "search" => "Search",
            "apply_patch" => "Edit",
            "shell" => "Run",
            "check" => "Check",
            _ => tool,
        };
        let detail = if tool == "apply_patch" {
            arguments["edits"]
                .as_array()
                .map(|edits| {
                    edits
                        .iter()
                        .filter_map(|edit| edit["path"].as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default()
        } else {
            detail.lines().next().unwrap_or("").to_owned()
        };
        self.push(Entry {
            key: id.into(),
            kind: Kind::Tool,
            title: clean(&format!("{name}  {detail}"))
                .chars()
                .take(500)
                .collect(),
            body: String::new(),
            success: None,
            running: true,
        });
    }
    pub fn finish(&mut self, id: &str, success: bool, result: &Value) {
        if let Some(entry) = self.entries.iter_mut().rev().find(|entry| entry.key == id) {
            entry.running = false;
            entry.success = Some(success);
            let data = &result["result"]["data"];
            let detail = result["error"].as_str().map(str::to_owned).or_else(|| {
                ["stdout", "stderr", "output", "content", "text"]
                    .iter()
                    .filter_map(|key| data[*key].as_str())
                    .filter(|text| !text.trim().is_empty())
                    .map(str::to_owned)
                    .reduce(|left, right| format!("{left}\n{right}"))
                    .or_else(|| data["process"]["output"].as_str().map(str::to_owned))
            });
            if let Some(detail) = detail {
                entry.body = clean(&detail).chars().take(20_000).collect();
            } else if entry.body.is_empty() && !data.is_null() {
                entry.body = clean(&data.to_string()).chars().take(2000).collect();
            }
            // Never silently swallow a failed result whose adapter supplied no text.
            if !success && entry.body.is_empty() {
                entry.body = "执行未成功；/status 查看状态，Ctrl+O 展开记录。".into();
            }
        }
    }
    pub fn active(&self) -> Option<&str> {
        self.entries
            .iter()
            .rev()
            .find(|entry| entry.running)
            .map(|entry| entry.title.as_str())
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn lines(&self, width: usize, expanded: bool) -> Vec<Line<'static>> {
        let mut lines = vec![];
        for entry in &self.entries {
            let (marker, color) = match entry.kind {
                Kind::User => ("❯", ACCENT),
                Kind::Assistant => ("●", Color::Reset),
                Kind::Tool if entry.running => ("◌", ACCENT),
                Kind::Tool if entry.success == Some(false) => ("✕", Color::Red),
                Kind::Tool if entry.success == Some(true) => ("✓", Color::Green),
                Kind::Tool => ("?", MUTED),
                Kind::Note => ("·", MUTED),
            };
            if entry.kind == Kind::Note {
                lines.extend(text::markdown(
                    &entry.body,
                    width,
                    Style::default().fg(MUTED),
                ));
            } else {
                let heading = Line::from(vec![
                    Span::styled(format!("{marker} "), Style::default().fg(color)),
                    Span::styled(
                        entry.title.clone(),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                ]);
                lines.extend(text::wrap(heading.spans, width));
                let body = text::markdown(&entry.body, width.saturating_sub(2), Style::default());
                let limit = if entry.kind == Kind::Tool && !expanded {
                    if entry.success == Some(false) { 8 } else { 3 }
                } else {
                    body.len()
                };
                if !entry.body.is_empty() {
                    let failure_tail = entry.kind == Kind::Tool
                        && entry.success == Some(false)
                        && body.len() > limit;
                    let start = if failure_tail { body.len() - limit } else { 0 };
                    if failure_tail {
                        lines.push(Line::styled(
                            "  … 前文已折叠 · Ctrl+O 展开",
                            Style::default().fg(MUTED),
                        ));
                    }
                    for mut line in body.iter().skip(start).take(limit).cloned() {
                        line.spans.insert(0, Span::raw("  "));
                        if entry.kind == Kind::Tool {
                            line = line.style(Style::default().fg(MUTED));
                        }
                        lines.push(line);
                    }
                    if body.len() > limit && !failure_tail {
                        lines.push(Line::styled(
                            format!("  … {} 行 · Ctrl+O 展开", body.len() - limit),
                            Style::default().fg(MUTED),
                        ));
                    }
                }
            }
            lines.push(Line::default());
        }
        lines
    }
}

fn actor_label(actor: &str) -> String {
    if actor == "main" {
        "Logit".into()
    } else if actor.starts_with("verify") {
        "独立验收".into()
    } else if actor.starts_with("steer") {
        "需求更新".into()
    } else if actor.starts_with("diagnos") {
        "诊断".into()
    } else if actor.starts_with("worker") {
        "工作者".into()
    } else {
        "Logit".into()
    }
}

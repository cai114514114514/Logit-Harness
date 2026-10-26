//! `/contract`: every clause with its words, revisions, evidence and verdict.
//!
//! A temporary full screen like the history browser. It is a snapshot taken when
//! opened; Esc returns without replaying history, and reopening refreshes it.
use super::{
    Input, Screen,
    history::Action,
    state::clean,
    text::{self, accent, muted},
    tool_text,
};
use crate::{
    domain::contract::StaleReason,
    runtime::contract::{ClauseView, ContractView, EvidenceView, Origin},
};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use std::collections::{BTreeMap, BTreeSet};
use unicode_width::UnicodeWidthStr;

#[cfg(test)]
#[path = "contract_tests.rs"]
mod tests;

/// Indent of a clause's detail rows, under its wording.
const INDENT: &str = "          ";
/// Rows of raw output shown per piece of evidence when a clause is expanded.
const RAW_ROWS: usize = 12;

/// A focused step running when the page was opened.
struct Running {
    title: String,
    clauses: Vec<String>,
    started: String,
}

pub struct Detail {
    contract: Option<ContractView>,
    times: BTreeMap<u64, String>,
    running: Vec<Running>,
    /// Complete records of the actions behind evidence, by evidence ID.
    raw: BTreeMap<String, String>,
    selected: usize,
    expanded: BTreeSet<String>,
    query: String,
    search: Option<Input>,
}

impl Detail {
    pub fn open(screen: &Screen) -> Self {
        let contract = screen.session.as_ref().and_then(|s| s.contract.clone());
        let transcript = &screen.transcript;
        let raw = contract
            .iter()
            .flat_map(|c| &c.clauses)
            .flat_map(|c| &c.evidence)
            .filter_map(|e| {
                // Command evidence is named after the action that produced it.
                let action = e.id.strip_prefix("e-")?;
                Some((e.id.clone(), transcript.action_text(action)?))
            })
            .collect();
        let running = transcript
            .visible_entries()
            .filter(|e| e.running && !e.clauses.is_empty())
            .map(|e| Running {
                title: e.title.split_whitespace().collect::<Vec<_>>().join(" "),
                clauses: e.clauses.clone(),
                started: e.started.clone(),
            })
            .collect();
        Self {
            contract,
            times: transcript.contract_times(),
            running,
            raw,
            selected: 0,
            expanded: BTreeSet::new(),
            query: String::new(),
            search: None,
        }
    }

    /// Clauses matching the search, active first, retired last.
    fn clauses(&self) -> Vec<&ClauseView> {
        let query = self.query.to_lowercase();
        self.contract
            .iter()
            .flat_map(|c| &c.clauses)
            .filter(|c| {
                query.is_empty()
                    || format!("{} {} {}", c.id, c.text, c.source)
                        .to_lowercase()
                        .contains(&query)
            })
            .collect()
    }

    pub fn key(&mut self, key: KeyEvent) -> Action {
        if let Some(input) = &mut self.search {
            match key.code {
                KeyCode::Esc => self.search = None,
                KeyCode::Enter => {
                    self.query = input.take();
                    self.search = None;
                    self.selected = 0;
                }
                KeyCode::Backspace => input.backspace(),
                KeyCode::Char(c) => input.insert(&c.to_string()),
                _ => (),
            }
            return Action::Stay;
        }
        let count = self.clauses().len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return Action::Close,
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => self.selected = (self.selected + 1).min(count.saturating_sub(1)),
            KeyCode::Char('/') => {
                let mut input = Input::default();
                input.insert(&self.query);
                self.search = Some(input);
            }
            KeyCode::Char('c') => {
                if let Some(clause) = self.clauses().get(self.selected) {
                    return Action::Copy(clause.source.clone());
                }
            }
            KeyCode::Enter => {
                if let Some(id) = self.clauses().get(self.selected).map(|c| c.id.clone())
                    && !self.expanded.remove(&id)
                {
                    self.expanded.insert(id);
                }
            }
            _ => (),
        }
        Action::Stay
    }

    pub fn paste(&mut self, text: &str) {
        if let Some(search) = &mut self.search {
            search.insert(text);
        }
    }

    fn evidence(&self, evidence: &EvidenceView) -> String {
        let label = format!("e{}", evidence.label);
        if let Some(reason) = &evidence.stale {
            let why = match reason {
                StaleReason::Clause => "条款已修订",
                StaleReason::Epoch => "新增条款",
                StaleReason::Step => "步骤失效",
            };
            return format!("{label} 已过期（{why}）");
        }
        let verb = match evidence.kind {
            crate::domain::EvidenceKind::Review => "评审",
            _ => tool_text::verb(&evidence.tool),
        };
        let mark = if evidence.passed { "✓" } else { "✕" };
        let subject = text::ellipsize(&clean(&evidence.subject), 32);
        let current = if evidence.current {
            ""
        } else {
            "（已过期）"
        };
        format!("{label} {verb} {subject} {mark}{current}")
    }

    /// One clause: its stamped heading, then source, revisions, evidence, verdict.
    fn clause(&self, clause: &ClauseView, selected: bool, width: usize) -> Vec<Line<'static>> {
        let (symbol, color) = if clause.active {
            super::contract_bar::symbol(clause.stamp)
        } else {
            ("−", muted())
        };
        let marker = if selected { "›" } else { " " };
        let heading = Style::default().add_modifier(if selected {
            Modifier::BOLD
        } else {
            Modifier::empty()
        });
        let wording = text::wrap(
            vec![Span::styled(clean(&clause.text), heading)],
            width.saturating_sub(INDENT.width()),
        );
        let mut lines = vec![];
        for (i, row) in wording.into_iter().enumerate() {
            let mut spans = if i == 0 {
                vec![
                    Span::styled(marker, Style::default().fg(accent())),
                    Span::styled(format!("{symbol} "), Style::default().fg(color)),
                    Span::styled(format!("{:<4}", clause.id), heading),
                    Span::styled(
                        format!("v{:<3}", clause.revision),
                        Style::default().fg(muted()),
                    ),
                ]
            } else {
                vec![Span::raw(INDENT)]
            };
            spans.extend(row.spans);
            lines.push(Line::from(spans));
        }
        let mut detail = |label: &str, value: String| {
            let rows = text::wrap(
                vec![Span::raw(value)],
                width.saturating_sub(INDENT.width() + label.width() + 2),
            );
            for (i, row) in rows.into_iter().enumerate() {
                let mut spans = vec![Span::raw(INDENT)];
                spans.push(Span::styled(
                    if i == 0 {
                        format!("{label}  ")
                    } else {
                        " ".repeat(label.width() + 2)
                    },
                    Style::default().fg(muted()),
                ));
                spans.extend(row.spans);
                lines.push(Line::from(spans));
            }
        };
        if !clause.active {
            detail("状态", "已退役".into());
            return lines;
        }
        let origin = match clause.origin {
            Origin::Original => "你的完整原话",
            Origin::Quote => "你的原话摘录",
            Origin::Addition => "你的补充",
        };
        let time = self
            .times
            .get(&clause.revision)
            .map_or_else(String::new, |t| format!(" · {t}"));
        detail("来源", format!("{origin}{time}"));
        if clause.origin == Origin::Addition {
            detail("原话", format!("「{}」", clean(&clause.source)));
        }
        for (revision, text) in &clause.history {
            detail(
                "修订",
                format!("v{revision}「{}」→ v{}", clean(text), clause.revision),
            );
        }
        // One row per piece of evidence, so a label is never split by wrapping.
        if clause.evidence.is_empty() {
            detail("证据", "暂无".into());
        }
        for (i, evidence) in clause.evidence.iter().enumerate() {
            detail(
                if i == 0 { "证据" } else { "    " },
                self.evidence(evidence),
            );
        }
        let verdict = self.contract.as_ref().and_then(|c| {
            c.acceptance
                .as_ref()
                .filter(|a| a.versions.get(&clause.id) == Some(&clause.revision))
                .and_then(|a| a.checks.iter().find(|check| check.requirement == clause.id))
        });
        if let Some(check) = verdict {
            detail(
                "验收",
                if check.passed {
                    "通过".into()
                } else {
                    format!("未通过：{}", clean(&check.reason))
                },
            );
        }
        let now = chrono::Utc::now().to_rfc3339();
        for step in self
            .running
            .iter()
            .filter(|r| r.clauses.contains(&clause.id))
        {
            let elapsed = tool_text::elapsed(&step.started, &now).unwrap_or_default();
            detail("进行", format!("{} · {elapsed}", clean(&step.title)));
        }
        if self.expanded.contains(&clause.id) {
            for evidence in &clause.evidence {
                lines.push(Line::styled(
                    format!("{INDENT}── e{} {} ──", evidence.label, evidence.id),
                    Style::default().fg(muted()),
                ));
                let raw = self
                    .raw
                    .get(&evidence.id)
                    .cloned()
                    .unwrap_or_else(|| clean(&evidence.subject));
                for row in clean(&raw).lines().take(RAW_ROWS) {
                    lines.extend(
                        text::wrap(
                            vec![Span::raw(row.to_owned())],
                            width.saturating_sub(INDENT.width()),
                        )
                        .into_iter()
                        .map(|mut line| {
                            line.spans.insert(0, Span::raw(INDENT));
                            line
                        }),
                    );
                }
            }
        }
        lines
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let width = area.width.saturating_sub(2) as usize;
        let [heading, body, footer] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .areas(area);
        let inset = |rect: ratatui::layout::Rect| {
            ratatui::layout::Rect::new(
                rect.x + 1,
                rect.y,
                rect.width.saturating_sub(2),
                rect.height,
            )
        };
        let rule = Line::styled("─".repeat(width), Style::default().fg(muted()));
        let title = self.contract.as_ref().map_or_else(
            || "契约".to_owned(),
            |c| {
                format!(
                    "契约 v{} · {} 条条款 · 修订 {} 次{}",
                    c.revision,
                    c.active().count(),
                    c.revision.saturating_sub(1),
                    if c.archived { " · 已归档" } else { "" }
                )
            },
        );
        let back = "Esc 返回";
        let title = text::ellipsize(&title, width.saturating_sub(back.width() + 2));
        let pad = width.saturating_sub(title.width() + back.width());
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(title, Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(" ".repeat(pad)),
                    Span::styled(back, Style::default().fg(muted())),
                ]),
                rule.clone(),
            ]),
            inset(heading),
        );
        let clauses = self.clauses();
        let mut lines = vec![];
        let mut start = 0;
        if self.contract.is_none() {
            lines.push(Line::raw(text::ellipsize(
                "当前没有契约。说出要完成的目标后，Logit 会把它记成契约。",
                width,
            )));
        } else if clauses.is_empty() {
            lines.push(Line::raw(format!(
                "没有匹配「{}」的条款",
                clean(&self.query)
            )));
        }
        for (i, clause) in clauses.iter().enumerate() {
            if i == self.selected {
                start = lines.len();
            }
            lines.extend(self.clause(clause, i == self.selected, width));
        }
        // Keep the selected clause's heading in view with a little context above.
        let height = body.height as usize;
        let offset = if start + 4 > height {
            start.saturating_sub(2)
        } else {
            0
        };
        frame.render_widget(
            Paragraph::new(lines.into_iter().skip(offset).collect::<Vec<_>>()),
            inset(body),
        );
        let hint = self.search.as_ref().map_or_else(
            || "↑↓ 选择 · Enter 展开证据原文 · / 搜索 · c 复制原话 · Esc 返回".to_owned(),
            |input| format!("搜索 > {} · Enter 应用 · Esc 取消", input.text()),
        );
        frame.render_widget(
            Paragraph::new(vec![
                rule,
                Line::styled(
                    text::ellipsize(
                        "状态按条款版本、步骤版本和 epoch 判断；文件指纹在验收时核对。",
                        width,
                    ),
                    Style::default().fg(muted()),
                ),
                Line::styled(text::ellipsize(&hint, width), Style::default().fg(muted())),
            ]),
            inset(footer),
        );
    }
}

//! One event projection for live output, reconnect and durable history.
use super::{state::clean, text};
use crate::domain::{EventData, SessionEvent, Status};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    User,
    Assistant,
    Tool,
    Note,
}
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub turn: Option<String>,
    pub response: Option<String>,
    pub source: String,
    pub started: String,
    pub updated: String,
    pub kind: Kind,
    pub title: String,
    pub body: String,
    pub running: bool,
    pub success: Option<bool>,
    pub settled: Option<u64>,
    printed: bool,
}
#[derive(Clone, Default)]
struct Identity {
    turn: String,
    response: String,
    actor: String,
}
#[derive(Default)]
pub struct Transcript {
    entries: Vec<Entry>,
    identities: BTreeMap<String, Identity>,
    sequence: u64,
    local: u64,
}
impl Transcript {
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    fn key(&self, actor: &str) -> String {
        self.identities
            .get(actor)
            .map(|i| i.response.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("{actor}:{}", self.sequence))
    }
    fn add(&mut self, id: String, kind: Kind, source: &str, title: String, stamp: &str) -> usize {
        let identity = self
            .identities
            .get(&id)
            .or_else(|| self.identities.get(source));
        self.entries.push(Entry {
            id,
            kind,
            title,
            source: identity.map_or(source, |i| &i.actor).into(),
            turn: identity.map(|i| i.turn.clone()),
            response: identity.map(|i| i.response.clone()),
            started: stamp.into(),
            updated: stamp.into(),
            body: String::new(),
            running: true,
            success: None,
            settled: None,
            printed: false,
        });
        self.entries.len() - 1
    }
    fn assistant(&mut self, actor: &str, stamp: &str) -> usize {
        let key = self.key(actor);
        if let Some(i) = self.entries.iter().position(|e| e.id == key) {
            return i;
        }
        if !self.identities.contains_key(actor)
            && let Some(i) = self
                .entries
                .iter()
                .rposition(|e| e.source == actor && e.kind == Kind::Assistant && e.running)
        {
            return i;
        }
        self.add(key, Kind::Assistant, actor, actor_label(actor), stamp)
    }
    fn done(&mut self, i: usize) {
        let entry = &mut self.entries[i];
        entry.running = false;
        entry.settled.get_or_insert(self.sequence);
    }
    pub fn receive(&mut self, event: &SessionEvent) {
        if event.sequence <= self.sequence {
            return;
        }
        self.sequence = event.sequence;
        let stamp = &event.timestamp;
        let first = self.entries.len();
        match &event.data {
            EventData::Identity {
                actor,
                response,
                turn,
                action,
                ..
            } => {
                self.identities.insert(
                    action.as_ref().unwrap_or(actor).clone(),
                    Identity {
                        actor: actor.clone(),
                        response: response.clone(),
                        turn: turn.clone(),
                    },
                );
            }
            EventData::ModelRequested { actor, .. } => {
                self.assistant(actor, stamp);
            }
            EventData::ModelFinished {
                actor, artifact, ..
            } => {
                let response = std::path::Path::new(artifact)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.strip_suffix(".response.json"));
                let i = response
                    .and_then(|id| self.entries.iter().position(|e| e.id == id))
                    .unwrap_or_else(|| self.assistant(actor, stamp));
                // ModelFinished precedes Message. End activity now, but commit text only
                // on Message; a tool-only response produces no empty transcript block.
                self.entries[i].running = false;
            }
            EventData::Message { actor, text } => {
                let i = self.assistant(actor, stamp);
                self.entries[i].body = text.clone();
                self.entries[i].updated = stamp.clone();
                self.done(i);
            }
            EventData::Delta { actor, text } => {
                let i = self
                    .entries
                    .iter()
                    .position(|e| e.id == *actor && e.kind == Kind::Tool)
                    .unwrap_or_else(|| self.assistant(actor, stamp));
                self.entries[i].body.push_str(text);
                self.entries[i].updated = stamp.clone();
            }
            EventData::ActionStarted {
                id,
                tool,
                arguments,
            } => {
                if self.entries.iter().any(|e| e.id == *id) {
                    return;
                }
                self.add(
                    id.clone(),
                    Kind::Tool,
                    id,
                    super::tool_text::title(tool, arguments),
                    stamp,
                );
            }
            EventData::ActionFinished {
                id,
                success,
                result,
            } => {
                let i = self
                    .entries
                    .iter()
                    .position(|e| e.id == *id)
                    .unwrap_or_else(|| {
                        self.add(
                            id.clone(),
                            Kind::Tool,
                            id,
                            "工具结果（开始记录缺失）".into(),
                            stamp,
                        )
                    });
                let body = super::tool_text::result(result);
                if !body.is_empty() {
                    self.entries[i].body = body;
                }
                self.entries[i].success = Some(*success);
                self.entries[i].updated = stamp.clone();
                if !success && self.entries[i].body.is_empty() {
                    self.entries[i].body = "执行失败；未提供结果正文".into();
                }
                self.done(i);
            }
            EventData::Created { objective, .. } | EventData::User { message: objective }
                if !objective.is_empty() =>
            {
                let i = self.add(
                    format!("user:{}", event.sequence),
                    Kind::User,
                    "user",
                    "你".into(),
                    stamp,
                );
                self.entries[i].body = objective.clone();
                self.done(i);
            }
            EventData::Status { status, reason } => {
                if matches!(
                    status,
                    Status::Paused
                        | Status::Stopped
                        | Status::Failed
                        | Status::Completed
                        | Status::Exhausted
                ) {
                    self.settle();
                }
                if *status != Status::Running {
                    self.note(&format!("{} · {reason}", super::state::label(status)));
                }
            }
            EventData::Diagnostic { message } => self.note(message),
            EventData::Approval {
                id,
                tool,
                reason,
                details,
                ..
            } => self.note(&format!(
                "需要授权 · {tool}\n{reason}\n审阅：{details}\n/approve {id}"
            )),
            EventData::Revised { revision, changed } => {
                self.note(&format!("需求修订 {revision} · {}", changed.join(", ")))
            }
            _ => (),
        }
        for entry in self
            .entries
            .iter_mut()
            .skip(first)
            .filter(|entry| entry.kind == Kind::Note)
        {
            // Persisted notices use their original event time and identity;
            // opening history must not make an old notice appear to happen now.
            entry.id = format!("event:{}", event.sequence);
            entry.started = stamp.clone();
            entry.updated = stamp.clone();
        }
    }
    pub fn note(&mut self, text: &str) {
        self.local += 1;
        let i = self.add(
            format!("note:{}:{}", self.sequence, self.local),
            Kind::Note,
            "系统",
            "系统".into(),
            &chrono::Utc::now().to_rfc3339(),
        );
        self.entries[i].body = text.into();
        self.done(i);
    }
    pub fn settle(&mut self) {
        for i in 0..self.entries.len() {
            if self.entries[i].running {
                self.done(i);
            }
        }
    }
    pub(super) fn reconcile(&mut self, activities: &[crate::runtime::view::Activity]) {
        // A current snapshot also closes failed requests that have no final
        // Message. Never manufacture a successful tool result from a snapshot.
        for entry in &mut self.entries {
            if entry.running
                && !activities.iter().any(|a| {
                    a.id == entry.id || (entry.kind == Kind::Assistant && a.id == entry.source)
                })
            {
                entry.running = false;
            }
        }
    }
    pub fn active(&self) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| e.running)
            .map(|e| e.title.as_str())
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn lines(&self, width: usize, expanded: bool) -> Vec<Line<'static>> {
        self.entries
            .iter()
            .flat_map(|e| e.lines(width, expanded))
            .collect()
    }
    pub fn active_lines(&self, width: usize) -> Vec<Line<'static>> {
        self.entries
            .iter()
            .filter(|e| e.running)
            .flat_map(|e| e.lines(width, false))
            .collect()
    }
    pub fn drain_settled(&mut self, width: usize) -> Vec<Line<'static>> {
        let mut indices = (0..self.entries.len())
            .filter(|i| {
                let e = &self.entries[*i];
                !e.printed && e.settled.is_some() && (!e.body.is_empty() || e.kind == Kind::Tool)
            })
            .collect::<Vec<_>>();
        indices.sort_by_key(|i| self.entries[*i].settled);
        let mut lines = vec![];
        for i in indices {
            lines.extend(self.entries[i].lines(width, false));
            self.entries[i].printed = true;
        }
        lines
    }
    pub fn recent(&mut self, width: usize) -> Vec<Line<'static>> {
        let mut entries = self
            .entries
            .iter_mut()
            .filter(|e| e.settled.is_some())
            .collect::<Vec<_>>();
        entries.sort_by_key(|e| e.settled);
        for e in &mut entries {
            e.printed = true;
        }
        let first = entries.len().saturating_sub(20);
        let mut lines = entries[first..]
            .iter()
            .flat_map(|e| e.lines(width, false))
            .collect::<Vec<_>>();
        let marker = text::wrap(
            vec![Span::raw(
                "── 恢复最近记录（最多20块/200行）；/history 查看完整历史 ──",
            )],
            width,
        );
        let available = 200usize.saturating_sub(marker.len());
        if lines.len() > available {
            lines.drain(..lines.len() - available);
        }
        marker.into_iter().chain(lines).collect()
    }
}
impl Entry {
    pub fn lines(&self, width: usize, expanded: bool) -> Vec<Line<'static>> {
        if self.kind == Kind::Assistant && self.body.is_empty() {
            return vec![];
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
        let body = if self.kind == Kind::Assistant {
            text::markdown(
                &clean(&self.body),
                width.saturating_sub(2),
                Style::default(),
            )
        } else {
            clean(&self.body)
                .split('\n')
                .flat_map(|line| {
                    text::wrap(vec![Span::raw(line.to_owned())], width.saturating_sub(2))
                })
                .collect()
        };
        let limit = if expanded || self.kind == Kind::Note {
            body.len()
        } else if self.kind == Kind::Tool {
            if self.success == Some(false) { 8 } else { 3 }
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
        lines.push(Line::default());
        lines
            .into_iter()
            .flat_map(|line| text::wrap(line.spans, width))
            .collect()
    }
}
pub(super) fn actor_label(actor: &str) -> String {
    if actor.starts_with("verify") {
        "独立验收"
    } else if actor.starts_with("worker") {
        "工作者"
    } else if actor.starts_with("steer") {
        "处理补充"
    } else if actor.starts_with("question") {
        "回答侧问"
    } else {
        "Logit"
    }
    .into()
}

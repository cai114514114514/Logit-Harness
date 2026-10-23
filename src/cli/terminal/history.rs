//! Temporary searchable, paginated details. Loading uses SQLite pages off the UI loop.
use super::{
    Input, Transcript,
    transcript::{Entry, Kind},
};
use crate::{config::Paths, store::Store};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    text::Line,
    widgets::Paragraph,
};
use std::collections::BTreeSet;
const PAGE: usize = 20;

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;

pub(super) fn selected_text(transcript: &Transcript, selector: &str) -> anyhow::Result<String> {
    let entries = transcript
        .visible_entries()
        .filter(|entry| entry.id.starts_with(selector))
        .collect::<Vec<_>>();
    anyhow::ensure!(
        entries.len() == 1,
        "ID 必须唯一匹配一条记录；/history 选择后按 c 复制"
    );
    Ok(entries[0].full_text())
}

pub struct Browser {
    transcript: Transcript,
    query: String,
    tools: bool,
    selected: usize,
    expanded: BTreeSet<String>,
    search: Option<Input>,
    scroll: usize,
}
pub enum Action {
    Stay,
    Close,
    Copy(String),
}
impl Browser {
    pub fn new(transcript: Transcript, query: String, tools: bool) -> Self {
        Self {
            transcript,
            query,
            tools,
            selected: 0,
            expanded: BTreeSet::new(),
            search: None,
            scroll: 0,
        }
    }
    fn matches(&self) -> Vec<&Entry> {
        let query = self.query.to_lowercase();
        self.transcript
            .visible_entries()
            .filter(|e| {
                (e.kind == Kind::Tool || !self.tools && !e.body.is_empty())
                    && format!("{} {} {} {}", e.id, e.source, e.title, e.full_text())
                        .to_lowercase()
                        .contains(&query)
            })
            .collect()
    }
    pub fn copy(&self) -> Option<String> {
        self.matches().get(self.selected).map(|e| e.full_text())
    }
    pub fn key(&mut self, key: KeyEvent) -> Action {
        if let Some(input) = &mut self.search {
            match key.code {
                KeyCode::Esc => self.search = None,
                KeyCode::Enter => {
                    self.query = input.take();
                    self.search = None;
                    self.selected = 0;
                    self.scroll = 0;
                }
                KeyCode::Backspace => input.backspace(),
                KeyCode::Char(c) => input.insert(&c.to_string()),
                _ => (),
            }
            return Action::Stay;
        }
        let count = self.matches().len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return Action::Close,
            KeyCode::Char('/') => {
                let mut input = Input::default();
                input.insert(&self.query);
                self.search = Some(input);
            }
            KeyCode::Char('c') => {
                if let Some(text) = self.copy() {
                    return Action::Copy(text);
                }
            }
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                self.scroll = 0;
            }
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(count.saturating_sub(1));
                self.scroll = 0;
            }
            KeyCode::PageUp => {
                self.selected = self.selected.saturating_sub(PAGE);
                self.scroll = 0;
            }
            KeyCode::PageDown => {
                self.selected = (self.selected + PAGE).min(count.saturating_sub(1));
                self.scroll = 0;
            }
            KeyCode::Char('j') => self.scroll += 5,
            KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(5),
            KeyCode::Enter => {
                if let Some(entry) = self.matches().get(self.selected) {
                    let id = entry.id.clone();
                    if !self.expanded.remove(&id) {
                        self.expanded.insert(id);
                    }
                    self.scroll = 0;
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
    pub fn render(&self, frame: &mut Frame) {
        let entries = self.matches();
        let page = self.selected / PAGE;
        let [heading, list, detail, footer] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(8),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .areas(frame.area());
        frame.render_widget(
            Paragraph::new(format!(
                "完整历史 · {} 条 · 第 {}/{} 页 · 搜索：{} · {}",
                entries.len(),
                page + 1,
                entries.len().div_ceil(PAGE).max(1),
                super::state::clean(&self.query),
                self.transcript.heading()
            )),
            heading,
        );
        let first = page * PAGE;
        let visible = (self.selected.saturating_sub(6)).max(first);
        let rows = entries
            .iter()
            .enumerate()
            .skip(visible)
            .take(8)
            .take_while(|(i, _)| *i < first + PAGE)
            .map(|(i, e)| {
                Line::raw(format!(
                    "{} {} · {} · {}",
                    if i == self.selected { "❯" } else { " " },
                    e.id,
                    e.source,
                    super::state::clean(&e.title)
                ))
            })
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(rows), list);
        if let Some(entry) = entries.get(self.selected) {
            let rows = entry.lines(detail.width as usize, self.expanded.contains(&entry.id));
            let offset = self
                .scroll
                .min(rows.len().saturating_sub(detail.height as usize));
            frame.render_widget(
                Paragraph::new(rows.into_iter().skip(offset).collect::<Vec<_>>()),
                detail,
            );
        }
        let hint = self.search.as_ref().map(|input| format!("搜索 > {} · Enter 应用 · Esc 取消", input.text())).unwrap_or_else(|| "↑↓ 逐项 · PgUp/PgDn 分页 · Enter 展开此项 · j/k 滚动正文\n/ 搜索 · c 复制原文 · Esc 返回对话".into());
        frame.render_widget(Paragraph::new(hint), footer);
    }
}
pub fn load(paths: &Paths, id: &str) -> anyhow::Result<Transcript> {
    let store = Store::open(&paths.database())?;
    let mut projection = Transcript::default();
    if let Ok(session) = store.load::<crate::runtime::state::Session>(id) {
        projection.ownership.bindings = session.actor_bindings();
        for archive in &session.archives {
            projection.team(&archive.team.view());
        }
        projection.team(&session.team.view());
    }
    loop {
        let events = store.event_page(id, projection.sequence(), 500)?;
        if events.is_empty() {
            break;
        }
        for event in events {
            projection.receive(&event);
        }
    }
    Ok(projection)
}

pub(super) fn load_view(
    paths: &Paths,
    id: &str,
    view: crate::runtime::view::ViewTarget,
) -> anyhow::Result<Transcript> {
    let mut projection = load(paths, id)?;
    projection.focus(view);
    Ok(projection)
}

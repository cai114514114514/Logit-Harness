//! Starting a task uses the same editor as steering one; no raw read_line detour.
use super::{Intent, Recent, Screen, edit, menu, parse, render};
use crate::{
    config::{Config, Paths},
    runtime::state::Control,
    store::Store,
};
use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, Event, EventStream, KeyCode, KeyEventKind,
    KeyModifiers,
};
use futures::StreamExt;
use std::path::Path;

pub enum Launch {
    Task(String),
    Attach(String),
    Exit,
}

pub(super) fn recent(paths: &Paths, cwd: &Path) -> anyhow::Result<Vec<Recent>> {
    if !paths.database().exists() {
        return Ok(vec![]);
    }
    Store::open(&paths.database())?
        .snapshots()?
        .into_iter()
        .filter(|(_, state)| state["workspace"].as_str() == cwd.to_str())
        .map(|(id, state)| {
            Ok(Recent {
                id,
                objective: state["goal"]["original"]
                    .as_str()
                    .unwrap_or("未命名任务")
                    .to_owned(),
                status: serde_json::from_value(state["status"].clone())?,
            })
        })
        .collect()
}

struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        let _ = crossterm::execute!(std::io::stdout(), DisableBracketedPaste);
        ratatui::restore();
    }
}

pub async fn welcome(paths: &Paths, config: &Config, cwd: &Path) -> anyhow::Result<Launch> {
    let mut screen = Screen::default();
    screen.workspace = cwd.to_owned();
    screen.model = config.model.clone();
    screen.key_present = std::env::var("DEEPSEEK_API_KEY").is_ok_and(|key| !key.trim().is_empty());
    screen.recent = recent(paths, cwd)?;
    let _restore = Restore;
    let mut terminal = super::surface::open(true)?;
    crossterm::execute!(std::io::stdout(), EnableBracketedPaste)?;
    let mut events = EventStream::new();
    loop {
        terminal.draw(|frame| render(frame, &screen))?;
        let event = tokio::select! {
            _ = tokio::signal::ctrl_c() => return Ok(Launch::Exit),
            event = events.next() => match event { Some(event) => event?, None => return Ok(Launch::Exit) },
        };
        match event {
            Event::Paste(text) => {
                screen.input.insert(&text);
                screen.menu_hidden = false;
            }
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if screen.picker && key.code == KeyCode::Enter {
                    if let Some(recent) = screen.recent.get(screen.selection) {
                        return Ok(Launch::Attach(recent.id.clone()));
                    }
                    screen.picker = false;
                    continue;
                }
                if edit::edit(&mut screen, key) {
                    continue;
                }
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(key.code, KeyCode::Char('c' | 'd'))
                {
                    return Ok(Launch::Exit);
                }
                if key.code != KeyCode::Enter || !screen.complete() {
                    continue;
                }
                let draft = screen.input.text().to_owned();
                if draft.trim().is_empty() {
                    continue;
                }
                match parse(&draft) {
                    Ok(Intent::Control(Control::Message { text })) => {
                        if screen.key_present {
                            return Ok(Launch::Task(text));
                        }
                        screen.note("尚未配置 DEEPSEEK_API_KEY。任务草稿仍在输入区；请配置环境变量后重新打开 loh。未启动模型请求。");
                    }
                    Ok(Intent::Sessions) => {
                        screen.input.take();
                        screen.selection = 0;
                        if screen.recent.is_empty() {
                            screen.note("当前目录还没有保存的会话。输入一个任务开始。");
                        } else {
                            screen.picker = true;
                        }
                    }
                    Ok(Intent::Help) => {
                        screen.input.take();
                        screen.note(menu::help());
                    }
                    Ok(Intent::Detach) => return Ok(Launch::Exit),
                    Ok(_) => {
                        screen.input.take();
                        screen.note("请先开始任务，或用 /sessions 打开已有会话。");
                    }
                    Err(error) => screen.note(error.to_string()),
                }
                screen.menu_hidden = false;
            }
            _ => (),
        }
    }
}

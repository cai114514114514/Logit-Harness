//! Saved conversations are readable without credentials or a running daemon.
use super::{Intent, Screen, edit, menu, parse, render};
use crate::{
    config::{Config, Paths},
    runtime::{
        ipc::ServerMessage,
        state::{Control, Session},
    },
    store::Store,
};
use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, Event, EventStream, KeyCode, KeyEventKind,
    KeyModifiers,
};
use futures::StreamExt;

struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        let _ = crossterm::execute!(std::io::stdout(), DisableBracketedPaste);
        ratatui::restore();
    }
}

pub(super) async fn open(paths: &Paths, id: &str, after: u64) -> anyhow::Result<bool> {
    let store = Store::open(&paths.database())?;
    let session: Session = store.load(id)?;
    let events = store.events(id, after)?;
    let sequence = events.last().map_or(after, |event| event.sequence);
    let mut screen = Screen::default();
    screen.model = Config::load(&paths.home)?.model;
    screen.receive(ServerMessage::Snapshot {
        state: Box::new(session),
        sequence,
    });
    for event in events {
        screen.receive(ServerMessage::Event { event });
    }
    screen.connected = false;
    screen.activity.clear();
    screen.transcript.settle();
    screen.note("已打开保存的记录，后台连接不可用。/resume 重新连接或恢复，Ctrl+D 离开。");
    let _restore = Restore;
    let mut terminal = super::surface::open(false)?;
    crossterm::execute!(std::io::stdout(), EnableBracketedPaste)?;
    let mut keys = EventStream::new();
    loop {
        terminal.draw(|frame| render(frame, &screen))?;
        let event = tokio::select! {
            _ = tokio::signal::ctrl_c() => return Ok(false),
            event = keys.next() => match event { Some(event) => event?, None => return Ok(false) },
        };
        match event {
            Event::Paste(text) => screen.input.insert(&text),
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if edit::edit(&mut screen, key) {
                    continue;
                }
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(key.code, KeyCode::Char('c' | 'd'))
                {
                    return Ok(false);
                }
                if key.code != KeyCode::Enter || !screen.complete() {
                    continue;
                }
                let draft = screen.input.take();
                match parse(&draft) {
                    Ok(Intent::Control(Control::Resume)) => {
                        if std::env::var("DEEPSEEK_API_KEY").is_ok_and(|key| !key.trim().is_empty())
                        {
                            return Ok(true);
                        }
                        screen.note("恢复执行需要 DEEPSEEK_API_KEY；配置后运行 loh resume <会话 ID>。记录仍然保留。");
                    }
                    Ok(Intent::Status) => screen.note(screen.status()),
                    Ok(Intent::Budget) => screen.note(screen.budget()),
                    Ok(Intent::Help) => screen.note(menu::help()),
                    Ok(Intent::Diff) => {
                        match super::actions::diff(screen.workspace.clone()).await {
                            Ok(diff) => screen.note(diff),
                            Err(error) => screen.note(error.to_string()),
                        }
                    }
                    Ok(Intent::Detach) => return Ok(false),
                    Ok(_) => {
                        screen.note("这是保存的记录。/resume 恢复执行后即可发送新要求。");
                        screen.input.insert(&draft);
                    }
                    Err(error) => {
                        screen.note(error.to_string());
                        screen.input.insert(&draft);
                    }
                }
                screen.menu_hidden = false;
            }
            _ => (),
        }
    }
}

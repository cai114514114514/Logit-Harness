use super::{
    Screen,
    actions::{self, View, request, snapshot},
    render,
};
use crate::{
    config::Paths,
    runtime::{ipc, state::Control},
};
use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste, EventStream};
use futures::StreamExt;
use std::time::Duration;
use tokio::{
    sync::mpsc,
    task::{JoinHandle, JoinSet},
};

struct TerminalGuard {
    reader: JoinHandle<()>,
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.reader.abort();
        let _ = crossterm::execute!(std::io::stdout(), DisableBracketedPaste);
        ratatui::restore();
    }
}

pub async fn attach(paths: &Paths, id: &str, after: u64) -> anyhow::Result<()> {
    let mut id = id.to_owned();
    let mut after = after;
    while let Some(next) = session(paths, &id, after).await? {
        id = next;
        after = 0;
    }
    Ok(())
}

async fn session(paths: &Paths, id: &str, after: u64) -> anyhow::Result<Option<String>> {
    let socket = paths.socket(id)?;
    let mut reader = loop {
        match ipc::subscribe(&socket, after).await {
            Ok(reader) => break reader,
            Err(_) => {
                if !super::archive::open(paths, id, after).await? {
                    return Ok(None);
                }
                crate::runtime::daemon::resume(paths, id).await?;
            }
        }
    };
    let (sender, mut incoming) = mpsc::channel(32);
    // read_line is not cancellation safe. A dedicated reader preserves partial socket frames
    // while UI input, redraw ticks and control requests race in the foreground event loop.
    let reader = tokio::spawn(async move {
        loop {
            let message = ipc::receive(&mut reader).await;
            let failed = message.is_err();
            if sender.send(message).await.is_err() || failed {
                break;
            }
        }
    });
    let mut terminal = match super::surface::open(false) {
        Ok(terminal) => terminal,
        Err(error) => {
            reader.abort();
            ratatui::restore();
            return Err(error.into());
        }
    };
    let _guard = TerminalGuard { reader };
    crossterm::execute!(std::io::stdout(), EnableBracketedPaste)?;
    let mut screen = Screen::default();
    screen.model = crate::config::Config::load(&paths.home)?.model;
    let saved: crate::runtime::state::Session =
        crate::store::Store::open(&paths.database())?.load(id)?;
    screen.recent = super::launch::recent(paths, &saved.workspace)?;
    let mut keys = EventStream::new();
    let mut jobs = JoinSet::new();
    let mut frames = tokio::time::interval(Duration::from_millis(50));
    let mut refresh = tokio::time::interval(Duration::from_secs(2));
    let mut hangup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let mut receiving = true;
    let mut dirty = true;
    loop {
        tokio::select! {
            _ = hangup.recv() => break,
            _ = tokio::signal::ctrl_c() => request(&mut jobs, &socket, Control::Pause),
            _ = frames.tick() => {
                if screen.transcript.active().is_some() || !screen.activity.is_empty() {
                    screen.frame = screen.frame.wrapping_add(1);
                    dirty |= screen.frame.is_multiple_of(2);
                }
                if dirty {
                    terminal.draw(|frame| render(frame, &screen))?;
                    dirty = false;
                }
            }
            _ = refresh.tick(), if screen.connected => {
                if jobs.is_empty() { snapshot(&mut jobs, &socket, View::Quiet); }
            }
            result = incoming.recv(), if receiving => {
                match result {
                    Some(Ok(message)) => screen.receive(message),
                    Some(Err(error)) => {
                        screen.connected = false;
                        receiving = false;
                        screen.note(format!("连接已断开：{error}。已保存的任务可用 loh resume {id} 恢复。"));
                    }
                    None => { screen.connected = false; receiving = false; }
                }
                dirty = true;
            }
            Some(result) = jobs.join_next(), if !jobs.is_empty() => {
                match result {
                    Ok(Ok(reply)) => actions::apply(reply, &mut screen),
                    Ok(Err(error)) => screen.note(format!("请求失败：{error}")),
                    Err(error) => screen.note(format!("客户端请求中断：{error}")),
                }
                dirty = true;
            }
            event = keys.next() => {
                match event {
                    None => break,
                    Some(Err(error)) => return Err(error.into()),
                    Some(Ok(event)) => {
                        if actions::handle(event, &mut screen, &mut jobs, &socket, paths) { break; }
                    }
                }
                dirty = true;
            }
        }
    }
    Ok(screen.next_session.take())
}

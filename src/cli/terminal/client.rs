//! One client loop serves welcome, live and archived sessions. The daemon owns work.
use super::{
    Screen,
    jobs::{Context, Jobs, Reply},
    render,
    submission::{self, Initial},
};
use crate::{
    commands::{Command, Navigation, Outcome},
    config::{Config, Paths},
    runtime::{
        ipc::{self, ServerMessage},
        state::Control,
    },
};
use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, Event, EventStream, KeyCode, KeyEventKind,
    KeyModifiers,
};
use futures::StreamExt;
use std::{path::Path, time::Duration};
use tokio::{sync::mpsc, task::JoinHandle};

struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        let _ = crossterm::execute!(std::io::stdout(), DisableBracketedPaste);
        ratatui::restore();
    }
}
struct Connection(Option<JoinHandle<()>>);
impl Drop for Connection {
    fn drop(&mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
        }
    }
}
impl Connection {
    fn close(&mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
        }
    }
    fn open(
        &mut self,
        paths: &Paths,
        id: &str,
        after: u64,
        view: crate::runtime::view::ViewTarget,
        generation: u64,
        sender: mpsc::Sender<(u64, anyhow::Result<ServerMessage>)>,
    ) -> anyhow::Result<()> {
        self.close();
        let socket = paths.socket(id)?;
        self.0 = Some(tokio::spawn(async move {
            let mut reader = match ipc::subscribe_target(&socket, after, view).await {
                Ok(reader) => reader,
                Err(error) => {
                    let _ = sender.send((generation, Err(error))).await;
                    return;
                }
            };
            // A dedicated reader keeps partial JSON frames across UI ticks.
            loop {
                let message = ipc::receive(&mut reader).await;
                let failed = message.is_err();
                if sender.send((generation, message)).await.is_err() || failed {
                    break;
                }
            }
        }));
        Ok(())
    }
}

pub(super) async fn run(
    paths: &Paths,
    cwd: &Path,
    id: Option<String>,
    setup_only: bool,
    view: crate::runtime::view::ViewTarget,
) -> anyhow::Result<()> {
    let config = Config::load(&paths.home)?;
    let mut screen = fresh(paths, &config, cwd)?;
    screen.view = view;
    screen.transcript.focus(screen.view.clone());
    let mut generation = 0;
    let mut jobs = Jobs::new();
    let mut initial = Initial::default();
    let mut connection = Connection(None);
    let (sender, mut incoming) = mpsc::channel(128);
    let mut opening = id.is_some();
    if let Some(id) = id {
        Context::new(generation, paths, &screen).open(&mut jobs, id);
    }
    if setup_only {
        super::setup::open(&mut screen, paths);
    }
    let _restore = Restore;
    let mut terminal = super::surface::open(true)?;
    terminal.append(&super::brand::lines(&screen, terminal.width()))?;
    crossterm::execute!(std::io::stdout(), EnableBracketedPaste)?;
    let mut keys = EventStream::new();
    let mut frames = tokio::time::interval(Duration::from_millis(80));
    let mut refresh = tokio::time::interval(Duration::from_secs(2));
    let mut hangup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let mut dirty = true;
    let mut paint = true;
    let mut catalog_loading = false;
    let mut reconnect = false;
    let mut failure_after_open: Option<(String, String, u64)> = None;
    loop {
        // Keystrokes repaint immediately; model/tool streams remain coalesced by
        // the frame timer. Typing must not wait behind its 80 ms animation tick.
        if paint && dirty {
            let modal = screen.history.is_some();
            terminal.modal(modal)?;
            if !modal {
                let (columns, rows) = crossterm::terminal::size()?;
                let width = terminal.width().saturating_sub(4) as usize;
                let lines = if screen.session.is_some() && screen.transcript.needs_recent() {
                    screen.transcript.recent(width)
                } else {
                    screen.transcript.drain_settled(width)
                };
                terminal.present(
                    super::render::height(&screen, columns, rows),
                    screen.session.is_some(),
                    &lines,
                )?;
            }
            terminal.draw(|frame| render(frame, &screen))?;
            dirty = false;
        }
        paint = false;
        tokio::select! {
            _ = hangup.recv() => break,
            _ = tokio::signal::ctrl_c() => {
                if screen.session.is_some() { Context::new(generation, paths, &screen).control(&mut jobs, screen.pause(), "/pause".into(), screen.connected); }
                else { break; }
            }
            _ = frames.tick() => {
                screen.frame = screen.frame.wrapping_add(1);
                dirty |= screen.frame.is_multiple_of(12);
                paint = true;
            }
            _ = refresh.tick(), if screen.session.is_some() && !opening => {
                if reconnect {
                    if let Some(session) = &screen.session {
                        connection.open(paths, &session.id, screen.transcript.sequence(), screen.view.clone(), generation, sender.clone())?;
                    }
                } else if screen.connected {
                    let context = Context::new(generation, paths, &screen);
                    let socket = paths.socket(&screen.session.as_ref().unwrap().id)?;
                    let target = screen.view.clone();
                    jobs.spawn(async move { (context.generation, ipc::request(&socket, ipc::ClientMessage::TargetStatus { target }).await.map(Reply::Message)) });
                }
            }
            Some((origin, result)) = incoming.recv() => {
                if origin != generation { continue; }
                match result {
                    Ok(message) => { screen.receive(message); reconnect = false; },
                    Err(error) => {
                        if screen.connected { screen.note(format!("连接断开：{error}。会话和输入已保留；正在重连，未重发输入。")); reconnect = true; }
                        else { screen.activity = "后台未连接 · /resume 恢复执行 · /history 查看记录".into(); }
                        screen.connected = false;
                    }
                }
                dirty = true;
            }
            Some(result) = jobs.join_next(), if !jobs.is_empty() => {
                let (origin, reply) = match result { Ok(reply) => reply, Err(error) => { screen.note(format!("本地操作中断：{error}")); dirty = true; continue; } };
                // Navigation increments generation. A late command/control reply must
                // never edit the fresh conversation or its unsubmitted draft.
                if origin != generation { continue; }
                match reply {
                    Err(error) => { screen.note(format!("操作失败：{error}")); screen.activity.clear(); }
                    Ok(Reply::Catalog(entries)) => { screen.completions = Some(entries); catalog_loading = false; }
                    Ok(Reply::Open { context, session, transcript, recent }) => {
                        let sequence = transcript.sequence(); let id = session.id.clone();
                        screen.activity.clear(); screen.transcript = transcript; screen.recent = recent;
                        let state = session.target_view(&screen.view)?;
                        screen.receive(ServerMessage::TargetView { state: Box::new(state), sequence }); screen.connected = false;
                        let (columns, rows) = crossterm::terminal::size()?;
                        let recent = screen.transcript.recent(columns.saturating_sub(4) as usize);
                        terminal.present(super::render::height(&screen, columns, rows), true, &recent)?;
                        if let Some((error, draft, revision)) = failure_after_open.take() {
                            screen.note(format!("后台启动失败：{error}\n会话 {id} 与输入已保存。/resume 恢复；请先核对记录，避免重复发送。"));
                            restore_draft(&mut screen, draft, revision);
                        }
                        connection.open(paths, &id, sequence, screen.view.clone(), generation, sender.clone())?;
                        initial.finish(context.revision);
                        opening = false;
                    }
                    Ok(Reply::OpenFailed { context, error }) => {
                        screen.note(format!("打开会话失败：{error}"));
                        initial.finish(context.revision); opening = false;
                    }
                    Ok(Reply::Created { context, id, error, draft, revision }) => {
                        if let Some(error) = error { failure_after_open = Some((error, draft, revision)); }
                        context.open(&mut jobs, id); opening = true;
                    }
                    Ok(Reply::Message(message)) => screen.receive(message),
                    Ok(Reply::Sent) => { screen.activity.clear(); if !screen.connected { reconnect = true; } }
                    Ok(Reply::Failed { origin, text, draft, revision }) => { screen.activity.clear(); screen.note_origin(&origin, text); screen.restore_origin(&origin, draft, revision); if initial.finish(revision) { opening = false; } }
                    Ok(Reply::History { origin, browser }) => { if super::ownership::same_view(&origin, &screen.view) { screen.history = Some(browser); } else { screen.note_origin(&origin, "历史已读取；返回原视图后 /history 查看。"); } },
                    Ok(Reply::Copied(origin)) => screen.note_origin(&origin, "已复制所选记录原文。"),
                    Ok(Reply::Accepted { origin, request }) => { screen.transcript.accepted(&origin, &request); screen.activity.clear(); if !screen.connected { reconnect = true; } },
                    Ok(Reply::Notice { origin, text }) => screen.note_origin(&origin, text),
                    Ok(Reply::Command { context, outcome, draft }) => {
                        if !matches!(&outcome, Outcome::Control(_) | Outcome::Skill(_)) {
                            initial.finish(context.revision);
                        }
                        match outcome {
                        Outcome::Local { title, text } => {
                            if draft.trim() == "/team" && super::ownership::same_view(&context.view, &screen.view) {
                                super::team::open(&mut screen, &text);
                            } else { screen.note_origin(&context.view, format!("{title}\n{text}")); }
                            if draft.trim() == "/skills" {
                                catalog_loading = true;
                                context.catalog(&mut jobs);
                            }
                        },
                        Outcome::Focus { origin, target, window } => {
                            if window { context.window(&mut jobs, target); }
                            else if super::ownership::same_view(&origin, &screen.view) {
                                screen.focus(target);
                                if let Some(session) = &screen.session { connection.open(paths, &session.id, screen.transcript.sequence(), screen.view.clone(), generation, sender.clone())?; }
                            } else { screen.note_origin(&origin, "视图已改变；原切换结果未覆盖当前焦点。"); }
                        },
                        Outcome::ScopedSkill { request, .. } => context.control(&mut jobs, Control::Directed { request: *request }, draft, screen.connected),
                        Outcome::ScopedChoice { entries, arguments, .. } => { submission::install_choice(&mut screen, context, entries, arguments, &draft); },
                        Outcome::Setup => super::setup::open(&mut screen, paths),
                        Outcome::Navigate(Navigation::Detach) => break,
                        Outcome::Navigate(Navigation::New) => {
                            generation += 1; connection.close(); initial = Initial::default(); failure_after_open = None; opening = false; reconnect = false; catalog_loading = false;
                            let cwd = screen.workspace.clone(); let entered = screen.entered;
                            screen = fresh(paths, &Config::load(&paths.home)?, &cwd)?;
                            screen.entered = entered;
                            screen.note("新对话 · 原任务继续后台；/sessions 可切回。");
                        }
                        Outcome::Navigate(Navigation::Sessions) => {
                            screen.recent = super::launch::recent(paths, &screen.workspace)?;
                            screen.selection = 0; screen.picker = true;
                            if screen.recent.is_empty() { screen.note("当前工作区还没有保存的会话。"); }
                        }
                        Outcome::Control(control) => {
                            if !initial.owns(&context) { screen.note("原请求已失去会话归属；未执行，请重新提交。"); continue; }
                            if context.session.is_none() {
                                context.clone().create(&mut jobs, control, draft); opening = true;
                            } else { context.clone().control(&mut jobs, control, draft, screen.connected); }
                            screen.activity = "正在提交（输入尚未确认处理）".into();
                        }
                        Outcome::Skill(invocation) => {
                            let control = invocation.control();
                            if !initial.owns(&context) { screen.note("原技能请求已失去会话归属；未执行，请重新提交。"); continue; }
                            if context.session.is_none() { context.clone().create(&mut jobs, control, draft); opening = true; }
                            else { context.clone().control(&mut jobs, control, draft, screen.connected); }
                        }
                        Outcome::ChooseSkill { entries, arguments } => {
                            submission::install_choice(&mut screen, context, entries, arguments, &draft);
                        }
                        Outcome::History(history) => context.history(&mut jobs, history.query, history.tools_only),
                        Outcome::Copy { selector, .. } => {
                            if selector.is_empty() { context.history(&mut jobs, String::new(), false); }
                            else { context.copy_selector(&mut jobs, selector); }
                        }
                        }
                    },
                }
                dirty = true;
            }
            event = keys.next() => {
                let event = match event { Some(Ok(event)) => event, Some(Err(error)) => return Err(error.into()), None => break };
                dirty = true;
                paint = true;
                if let Some(browser) = &mut screen.history {
                    match event {
                        Event::Paste(text) => browser.paste(&text),
                        Event::Key(key) if key.kind != KeyEventKind::Release => match browser.key(key) {
                            super::history::Action::Close => screen.history = None,
                            super::history::Action::Copy(text) => Context::new(generation, paths, &screen).copy(&mut jobs, text),
                            super::history::Action::Stay => (),
                        },
                        _ => (),
                    }
                    continue;
                }
                if super::setup::handle(&mut screen, &event, paths) {
                    if setup_only && screen.setup.is_none() { terminal.append(&screen.transcript.drain_settled(terminal.width() as usize))?; break; }
                    continue;
                }
                submission::clear_stale_choice(&mut screen);
                match event {
                    Event::Paste(text) => { screen.input.insert(&text); screen.bind_draft(); screen.menu_hidden = false; }
                    Event::Key(key) if key.kind != KeyEventKind::Release => {
                        let control = key.modifiers.contains(KeyModifiers::CONTROL);
                        if key.code == KeyCode::Char('t') && control && screen.session.is_some() {
                            let mut context = Context::new(generation, paths, &screen); context.view = screen.view.clone();
                            context.command(&mut jobs, crate::commands::parse("/team")?, "/team".into());
                            continue;
                        }
                        if (key.code == KeyCode::Char('o') && control) || key.code == KeyCode::PageUp {
                            Context::new(generation, paths, &screen).history(&mut jobs, String::new(), false); continue;
                        }
                        if screen.team_picker && (key.code == KeyCode::Enter || key.code == KeyCode::Char('o')) {
                            if let Some(member) = screen.selected_member() {
                                let window = key.code == KeyCode::Char('o');
                                let draft = format!("/agent {member}{}", if window { " --window" } else { "" });
                                let mut context = Context::new(generation, paths, &screen); context.view = screen.view.clone();
                                context.command(&mut jobs, crate::commands::parse(&draft)?, draft);
                            }
                            continue;
                        }
                        if screen.picker && key.code == KeyCode::Enter {
                            if let Some(recent) = screen.recent.get(screen.selection) {
                                let id = recent.id.clone(); let workspace = screen.workspace.clone(); let entered = screen.entered;
                                generation += 1; connection.close(); initial = Initial::default(); failure_after_open = None; screen = fresh(paths, &Config::load(&paths.home)?, &workspace)?;
                                screen.entered = entered;
                                Context::new(generation, paths, &screen).open(&mut jobs, id); opening = true; reconnect = false; catalog_loading = false;
                            }
                            continue;
                        }
                        if screen.skill_choices.is_some() && key.code == KeyCode::Enter {
                            let choice = screen.skill_choices.take().unwrap();
                            if let Some(entry) = choice.entries.get(screen.selection) {
                                let command = Command::Skill { selector: entry.id.clone(), arguments: choice.arguments.clone() };
                                let draft = format!("/skills run {} {}", entry.id, choice.arguments);
                                if let Some(context) = initial.prepare(&mut screen, choice.context, &command, opening) {
                                    context.command(&mut jobs, command, draft);
                                }
                            }
                            continue;
                        }
                        if super::edit::edit(&mut screen, key) { screen.bind_draft(); }
                        else if key.code == KeyCode::Char('d') && control { break; }
                        else if key.code == KeyCode::Char('c') && control {
                            if screen.session.is_some() { Context::new(generation, paths, &screen).control(&mut jobs, screen.pause(), "/pause".into(), screen.connected); } else { break; }
                        } else if key.code == KeyCode::Enter && screen.complete() {
                            let draft = screen.input.text().to_owned();
                            if super::setup::reject_inline(&mut screen, &draft) || draft.trim().is_empty() { continue; }
                            match crate::commands::parse(&draft) {
                                Ok(command) => {
                                    let context = Context::new(generation, paths, &screen);
                                    if let Some(context) = initial.prepare(&mut screen, context, &command, opening) {
                                        screen.input.remember(&draft); screen.menu_hidden = true;
                                        context.command(&mut jobs, command, draft);
                                    }
                                }
                                Err(error) => screen.note(error.to_string()),
                            }
                        }
                    }
                    _ => (),
                }
                submission::clear_stale_choice(&mut screen);
                if screen.input.text().starts_with('/') && screen.completions.is_none() && !catalog_loading {
                    catalog_loading = true; Context::new(generation, paths, &screen).catalog(&mut jobs);
                }
            }
        }
    }
    Ok(())
}
fn restore_draft(screen: &mut Screen, draft: String, revision: u64) {
    if draft.is_empty() {
        return;
    }
    if screen.input.text().is_empty() && screen.input.revision() == revision {
        screen.input.replace(&draft);
    } else {
        screen.note(format!("未确认送达的输入（保留）：\n{draft}"));
    }
}
fn fresh(paths: &Paths, config: &Config, cwd: &Path) -> anyhow::Result<Screen> {
    let mut screen = Screen::default();
    screen.client_id = uuid::Uuid::new_v4().to_string();
    screen.workspace = cwd.to_owned();
    screen.model = config.model.clone();
    screen.key_present = crate::config::api::configured(paths, &config.base_url)?;
    screen.recent = super::launch::recent(paths, cwd)?;
    Ok(screen)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_async_input_does_not_replace_a_revised_or_deleted_draft() {
        let mut screen = Screen::default();
        let original = screen.input.revision();
        screen.input.insert("newer draft");
        screen.input.take();
        restore_draft(&mut screen, "old request".into(), original);
        assert!(screen.input.text().is_empty());
        assert!(
            screen
                .transcript
                .lines(80, true)
                .iter()
                .any(|line| line.to_string().contains("old request"))
        );
        let current = screen.input.revision();
        restore_draft(&mut screen, "recover me".into(), current);
        assert_eq!(screen.input.text(), "recover me");
    }
}

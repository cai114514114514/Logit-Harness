use super::{Intent, Screen, parse};
use crate::runtime::{
    ipc::{self, ClientMessage, ServerMessage},
    state::Control,
};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::task::JoinSet;

pub(super) enum View {
    Quiet,
    Status,
    Budget,
}
pub(super) struct Reply {
    message: Option<ServerMessage>,
    note: Option<String>,
    draft: Option<String>,
    view: View,
}
type Jobs = JoinSet<anyhow::Result<Reply>>;

pub(super) fn apply(reply: Reply, screen: &mut Screen) {
    if reply.message.is_none() && reply.note.is_none() && reply.draft.is_none() {
        screen.activity = "已送达，等待执行器处理".into();
    }
    if let Some(message) = reply.message {
        screen.receive(message);
    }
    if let Some(note) = reply.note {
        screen.note(note);
    }
    if let Some(draft) = reply.draft {
        // Never overwrite text the user has started typing while the request was in flight.
        if screen.input.text().is_empty() {
            screen.input.insert(&draft);
        } else {
            screen.note(format!("未确认送达的输入：\n{draft}"));
        }
    }
    match reply.view {
        View::Status => screen.note(screen.status()),
        View::Budget => screen.note(screen.budget()),
        View::Quiet => (),
    }
}

pub(super) fn handle(
    event: Event,
    screen: &mut Screen,
    jobs: &mut Jobs,
    socket: &Path,
    paths: &crate::config::Paths,
) -> bool {
    if super::setup::handle(screen, &event, paths) {
        return false;
    }
    let key = match event {
        Event::Paste(text) => {
            screen.input.insert(&text);
            return false;
        }
        Event::Key(key) if key.kind != KeyEventKind::Release => key,
        _ => return false,
    };
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    if screen.picker && key.code == KeyCode::Enter {
        screen.next_session = screen
            .recent
            .get(screen.selection)
            .map(|recent| recent.id.clone());
        return screen.next_session.is_some();
    }
    if super::edit::edit(screen, key) {
        return false;
    }
    match key.code {
        KeyCode::Char('c') if control => request(jobs, socket, Control::Pause),
        KeyCode::Char('d') if control => return true,
        KeyCode::Enter => return submit(screen, jobs, socket, paths),
        _ => (),
    }
    false
}

fn submit(
    screen: &mut Screen,
    jobs: &mut Jobs,
    socket: &Path,
    paths: &crate::config::Paths,
) -> bool {
    if !screen.complete() {
        return false;
    }
    let text = screen.input.take();
    if super::setup::reject_inline(screen, &text) {
        return false;
    }
    if text.trim().is_empty() {
        return false;
    }
    match parse(&text) {
        Ok(Intent::Setup) => super::setup::open(screen, paths),
        Ok(Intent::Detach) => return true,
        Ok(Intent::Help) => screen.note(super::menu::help()),
        Ok(Intent::Sessions) => {
            screen.selection = 0;
            if screen.recent.is_empty() {
                screen.note("当前目录没有其他保存的会话。");
            } else {
                screen.picker = true;
            }
        }
        Ok(Intent::Status) => snapshot(jobs, socket, View::Status),
        Ok(Intent::Budget) => snapshot(jobs, socket, View::Budget),
        Ok(Intent::Diff) => {
            if let Some(session) = &screen.session {
                let cwd = session.workspace.clone();
                jobs.spawn(async move {
                    Ok(Reply {
                        message: None,
                        note: Some(diff(cwd).await?),
                        draft: None,
                        view: View::Quiet,
                    })
                });
            }
        }
        Ok(Intent::Control(command)) => {
            screen.activity = "正在提交…".into();
            request(jobs, socket, command);
        }
        Err(error) => {
            screen.note(error.to_string());
            screen.input.insert(&text);
        }
    }
    screen.input.remember(&text);
    screen.menu_hidden = false;
    screen.scroll = 0;
    false
}

pub(super) fn request(jobs: &mut Jobs, socket: &Path, command: Control) {
    let socket = socket.to_owned();
    jobs.spawn(async move {
        let draft = match &command {
            Control::Message { text } => Some(text.clone()),
            Control::Answer { id, text } => Some(format!("/answer {id} {text}")),
            _ => None,
        };
        match ipc::request(&socket, ClientMessage::Command { command }).await {
            Ok(ServerMessage::Ack) => Ok(Reply {
                message: None,
                note: None,
                draft: None,
                view: View::Quiet,
            }),
            Ok(message) => Ok(Reply {
                message: Some(message),
                note: None,
                draft,
                view: View::Quiet,
            }),
            Err(error) => Ok(Reply {
                message: None,
                note: Some(format!(
                    "未确认送达：{error}。请先查看会话记录，避免重复提交。"
                )),
                draft,
                view: View::Quiet,
            }),
        }
    });
}
pub(super) fn snapshot(jobs: &mut Jobs, socket: &Path, view: View) {
    let socket = socket.to_owned();
    jobs.spawn(async move {
        Ok(Reply {
            message: Some(ipc::request(&socket, ClientMessage::Status).await?),
            note: None,
            draft: None,
            view,
        })
    });
}

pub(super) async fn diff(cwd: PathBuf) -> anyhow::Result<String> {
    let mut output = String::from("当前 Git 工作树（包含已有修改）：\n");
    for arguments in [
        vec!["status", "--short"],
        vec![
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--color=never",
            "HEAD",
            "--",
        ],
    ] {
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::process::Command::new("git")
                .args(arguments)
                .current_dir(&cwd)
                .env("GIT_OPTIONAL_LOCKS", "0")
                .kill_on_drop(true)
                .output(),
        )
        .await??;
        anyhow::ensure!(
            result.status.success(),
            "Git 无法读取差异：{}",
            String::from_utf8_lossy(&result.stderr)
        );
        output.extend(String::from_utf8_lossy(&result.stdout).chars().take(80_000));
    }
    Ok(output)
}

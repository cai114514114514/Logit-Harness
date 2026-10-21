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

const HELP: &str = "/status 当前状态\n/diff 当前 Git 工作树差异（包括用户已有修改）\n/budget 查看用量；/budget <总 token 上限> [总秒数上限]\n/pause 暂停  /resume 继续  /stop 停止\n/answer <问题 ID> <答复>  /approve <授权 ID>\n/detach 断开客户端，后台继续；关闭终端同样只断开\nEnter 发送，Alt+Enter 换行；粘贴多行后仍需 Enter 才发送。";

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

pub(super) fn handle(event: Event, screen: &mut Screen, jobs: &mut Jobs, socket: &Path) -> bool {
    let key = match event {
        Event::Paste(text) => {
            screen.input.insert(&text);
            return false;
        }
        Event::Key(key) if key.kind != KeyEventKind::Release => key,
        _ => return false,
    };
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('c') if control => request(jobs, socket, Control::Pause),
        KeyCode::Char('d') if control => return true,
        KeyCode::Char('u') if control => {
            screen.input.take();
        }
        KeyCode::Char('a') if control => screen.input.home(),
        KeyCode::Char('e') if control => screen.input.end(),
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::ALT) => screen.input.insert("\n"),
        KeyCode::Enter => return submit(screen, jobs, socket),
        KeyCode::Char(character) if !control => screen.input.insert(&character.to_string()),
        KeyCode::Backspace => screen.input.backspace(),
        KeyCode::Delete => screen.input.delete(),
        KeyCode::Left => screen.input.left(),
        KeyCode::Right => screen.input.right(),
        KeyCode::Home => screen.input.home(),
        KeyCode::End => screen.input.end(),
        KeyCode::PageUp => screen.scroll = screen.scroll.saturating_add(10),
        KeyCode::PageDown => screen.scroll = screen.scroll.saturating_sub(10),
        _ => (),
    }
    false
}

fn submit(screen: &mut Screen, jobs: &mut Jobs, socket: &Path) -> bool {
    let text = screen.input.take();
    if text.trim().is_empty() {
        return false;
    }
    match parse(&text) {
        Ok(Intent::Detach) => return true,
        Ok(Intent::Help) => screen.note(HELP),
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
        Ok(Intent::Control(command)) => request(jobs, socket, command),
        Err(error) => {
            screen.note(error.to_string());
            screen.input.insert(&text);
        }
    }
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
                note: Some("控制请求已保存，等待执行器处理。".into()),
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

async fn diff(cwd: PathBuf) -> anyhow::Result<String> {
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

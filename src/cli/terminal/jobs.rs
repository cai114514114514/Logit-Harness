//! Blocking local services and asynchronous transport, bound to a client generation.
use super::{Screen, Transcript};
use crate::{
    commands::{Command, Completion, Outcome},
    config::{Config, Paths},
    domain::EventData,
    runtime::{
        daemon,
        ipc::{self, ClientMessage, ServerMessage},
        state::{Control, Session},
    },
    store::Store,
};
use std::path::PathBuf;
use tokio::task::JoinSet;

pub(super) enum Reply {
    Command {
        outcome: Outcome,
        draft: String,
        revision: u64,
    },
    Catalog(Vec<Completion>),
    Open {
        session: Box<Session>,
        transcript: Transcript,
        recent: Vec<super::Recent>,
    },
    Created {
        id: String,
        error: Option<String>,
        draft: String,
        revision: u64,
    },
    Message(ServerMessage),
    Sent,
    Failed {
        text: String,
        draft: String,
        revision: u64,
    },
    History(super::history::Browser),
    Copied,
}
pub(super) type Jobs = JoinSet<(u64, anyhow::Result<Reply>)>;
#[derive(Clone)]
pub(super) struct Context {
    pub generation: u64,
    pub revision: u64,
    pub paths: Paths,
    pub workspace: PathBuf,
    pub session: Option<String>,
}
impl Context {
    pub fn new(generation: u64, paths: &Paths, screen: &Screen) -> Self {
        Self {
            generation,
            revision: screen.input.revision(),
            paths: Paths::new(paths.home.clone()),
            workspace: screen.workspace.clone(),
            session: screen.session.as_ref().map(|s| s.id.clone()),
        }
    }
    pub fn command(self, jobs: &mut Jobs, command: Command, draft: String) {
        let generation = self.generation;
        jobs.spawn(async move {
            (
                generation,
                tokio::task::spawn_blocking(move || {
                    crate::commands::Service::new(
                        &self.paths,
                        &self.workspace,
                        self.session.as_deref(),
                    )
                    .execute(command)
                    .map(|outcome| Reply::Command {
                        outcome,
                        draft: draft.clone(),
                        revision: self.revision,
                    })
                    .or_else(|error| {
                        Ok(Reply::Failed {
                            text: error.to_string(),
                            draft,
                            revision: self.revision,
                        })
                    })
                })
                .await
                .unwrap_or_else(|e| Err(e.into())),
            )
        });
    }
    pub fn catalog(self, jobs: &mut Jobs) {
        let generation = self.generation;
        jobs.spawn(async move {
            (
                generation,
                tokio::task::spawn_blocking(move || {
                    Ok(Reply::Catalog(
                        crate::commands::Service::new(
                            &self.paths,
                            &self.workspace,
                            self.session.as_deref(),
                        )
                        .complete("/")?,
                    ))
                })
                .await
                .unwrap_or_else(|e| Err(e.into())),
            )
        });
    }
    pub fn open(self, jobs: &mut Jobs, id: String) {
        let generation = self.generation;
        jobs.spawn(async move {
            (
                generation,
                tokio::task::spawn_blocking(move || {
                    let session: Session = Store::open(&self.paths.database())?.load(&id)?;
                    let transcript = super::history::load(&self.paths, &id)?;
                    let recent = super::launch::recent(&self.paths, &session.workspace)?;
                    Ok(Reply::Open {
                        session: Box::new(session),
                        transcript,
                        recent,
                    })
                })
                .await
                .unwrap_or_else(|e| Err(e.into())),
            )
        });
    }
    pub fn create(self, jobs: &mut Jobs, control: Control, draft: String) {
        let generation = self.generation;
        jobs.spawn(async move {
            let paths = self.paths.clone();
            let saved = tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
                let config = Config::load(&paths.home)?;
                paths.prepare()?;
                let store = Store::open(&paths.database())?;
                // The initial input is durable before launch. Failure must not cause a
                // second copy of that input to be automatically sent on reconnect.
                let text = if let Control::Message { text } = &control {
                    text.as_str()
                } else {
                    ""
                };
                let mut session =
                    Session::conversation(self.workspace, text, config.tokens, config.seconds)
                        .with_defaults(&config);
                session.campaign = config.campaign.clone();
                store.commit(
                    &session.id,
                    &session,
                    EventData::Created {
                        objective: text.into(),
                        workspace: session.workspace.display().to_string(),
                    },
                )?;
                if !matches!(control, Control::Message { .. }) {
                    store.enqueue(&session.id, &serde_json::to_value(control)?)?;
                }
                Ok(session.id)
            })
            .await;
            let result = match saved {
                Ok(Ok(id)) => {
                    let error = daemon::launch(&self.paths, &id)
                        .await
                        .err()
                        .map(|e| e.to_string());
                    Ok(Reply::Created {
                        id,
                        error,
                        draft,
                        revision: self.revision,
                    })
                }
                Ok(Err(error)) => Ok(Reply::Failed {
                    text: error.to_string(),
                    draft,
                    revision: self.revision,
                }),
                Err(error) => Err(error.into()),
            };
            (generation, result)
        });
    }
    pub fn control(self, jobs: &mut Jobs, command: Control, draft: String, connected: bool) {
        let generation = self.generation;
        jobs.spawn(async move {
            let result =
                async {
                    let id = self
                        .session
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("请先打开会话"))?;
                    if !connected {
                        if matches!(command, Control::Resume) {
                            daemon::resume(&self.paths, id).await?;
                            return Ok(Reply::Sent);
                        }
                        // Persist before starting an archived owner: /pause or /plan
                        // must be applied before it can schedule old engineering work.
                        let database = self.paths.database();
                        let target = id.clone();
                        let data = serde_json::to_value(&command)?;
                        tokio::task::spawn_blocking(move || {
                            Store::open(&database)?.enqueue(&target, &data)
                        })
                        .await??;
                        daemon::launch(&self.paths, id).await.map_err(|error| anyhow::anyhow!(
                        "指令已保存，后台启动失败：{error}。用 /resume 恢复；不要重复提交原指令"
                    ))?;
                        return Ok(Reply::Sent);
                    }
                    match ipc::request(&self.paths.socket(id)?, ClientMessage::Command { command })
                        .await?
                    {
                        ServerMessage::Ack => Ok(Reply::Sent),
                        ServerMessage::Error { message } => Err(anyhow::anyhow!(message)),
                        message => Ok(Reply::Message(message)),
                    }
                }
                .await;
            (
                generation,
                result.or_else(|error: anyhow::Error| {
                    Ok(Reply::Failed {
                        text: format!("未确认送达：{error}。请先查看会话记录，避免重复提交。"),
                        draft,
                        revision: self.revision,
                    })
                }),
            )
        });
    }
    pub fn history(self, jobs: &mut Jobs, query: String, tools: bool) {
        let generation = self.generation;
        jobs.spawn(async move {
            (
                generation,
                tokio::task::spawn_blocking(move || {
                    let id = self
                        .session
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("请先打开会话"))?;
                    Ok(Reply::History(super::history::Browser::new(
                        super::history::load(&self.paths, id)?,
                        query,
                        tools,
                    )))
                })
                .await
                .unwrap_or_else(|e| Err(e.into())),
            )
        });
    }
    pub fn copy_selector(self, jobs: &mut Jobs, selector: String) {
        jobs.spawn(async move {
            (
                self.generation,
                tokio::task::spawn_blocking(move || {
                    let id = self
                        .session
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("请先打开会话"))?;
                    let transcript = super::history::load(&self.paths, id)?;
                    let entries = transcript
                        .entries()
                        .iter()
                        .filter(|e| e.id.starts_with(&selector))
                        .collect::<Vec<_>>();
                    anyhow::ensure!(
                        entries.len() == 1,
                        "ID 必须唯一匹配一条记录；/history 选择后按 c 复制"
                    );
                    super::clipboard::copy(&entries[0].body)?;
                    Ok(Reply::Copied)
                })
                .await
                .unwrap_or_else(|e| Err(e.into())),
            )
        });
    }
    pub fn copy(self, jobs: &mut Jobs, text: String) {
        jobs.spawn(async move {
            (
                self.generation,
                tokio::task::spawn_blocking(move || {
                    super::clipboard::copy(&text)?;
                    Ok(Reply::Copied)
                })
                .await
                .unwrap_or_else(|e| Err(e.into())),
            )
        });
    }
}

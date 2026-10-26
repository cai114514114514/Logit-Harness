//! Blocking local services and asynchronous transport, bound to a client generation.
use super::{Screen, Transcript};
use crate::{
    commands::{Command, Completion, Outcome},
    config::{Config, Paths},
    domain::EventData,
    runtime::{
        daemon,
        ipc::ServerMessage,
        state::{Control, Session},
    },
    store::Store,
};
use std::path::PathBuf;
use tokio::task::JoinSet;

pub(super) enum Reply {
    Command {
        context: Context,
        outcome: Outcome,
        draft: String,
    },
    Catalog(Vec<Completion>),
    Open {
        context: Context,
        session: Box<Session>,
        transcript: Transcript,
        recent: Vec<super::Recent>,
    },
    OpenFailed {
        context: Context,
        error: String,
    },
    Created {
        context: Context,
        id: String,
        error: Option<String>,
        draft: String,
        revision: u64,
    },
    Message(ServerMessage),
    Notice {
        operation: Option<String>,
        origin: crate::runtime::view::ViewTarget,
        text: String,
    },
    Accepted {
        operation: String,
        origin: crate::runtime::view::ViewTarget,
        request: crate::runtime::team::Request,
    },
    Sent(Context),
    Failed {
        operation: String,
        origin: crate::runtime::view::ViewTarget,
        text: String,
        draft: String,
        revision: u64,
    },
    History {
        origin: crate::runtime::view::ViewTarget,
        browser: super::history::Browser,
    },
    Copied(crate::runtime::view::ViewTarget),
}
pub(super) type Jobs = JoinSet<(u64, anyhow::Result<Reply>)>;
#[derive(Clone)]
pub(super) struct Context {
    pub operation: String,
    pub generation: u64,
    pub view: crate::runtime::view::ViewTarget,
    pub revision: u64,
    pub client_id: String,
    pub paths: Paths,
    pub workspace: PathBuf,
    pub session: Option<String>,
}
impl Context {
    pub fn new(generation: u64, paths: &Paths, screen: &Screen) -> Self {
        Self {
            operation: uuid::Uuid::new_v4().to_string(),
            generation,
            view: screen.draft_view(),
            client_id: screen.client_id.clone(),
            revision: screen.input.revision(),
            paths: Paths::new(paths.home.clone()),
            workspace: screen.workspace.clone(),
            session: screen.session.as_ref().map(|s| s.id.clone()),
        }
    }
    pub fn command(self, jobs: &mut Jobs, command: Command, draft: String) {
        self.execute(jobs, draft, move |context| {
            crate::commands::Service::new(
                &context.paths,
                &context.workspace,
                context.session.as_deref(),
            )
            .in_view(context.view.clone())
            .with_client(context.client_id.clone())
            .execute(command)
        });
    }
    /// Keep the same captured origin through service work and result dispatch.
    /// Tests inject delayed local work here without a provider or daemon.
    pub fn execute(
        self,
        jobs: &mut Jobs,
        draft: String,
        operation: impl FnOnce(&Context) -> anyhow::Result<Outcome> + Send + 'static,
    ) {
        let generation = self.generation;
        let revision = self.revision;
        let origin = self.view.clone();
        let operation_id = self.operation.clone();
        jobs.spawn(async move {
            let retained = draft.clone();
            let failed_origin = origin.clone();
            let result = tokio::task::spawn_blocking(move || match operation(&self) {
                Ok(outcome) => Ok(Reply::Command {
                    context: self,
                    outcome,
                    draft,
                }),
                Err(error) => Ok(Reply::Failed {
                    operation: self.operation.clone(),
                    origin: origin.clone(),
                    text: error.to_string(),
                    draft,
                    revision,
                }),
            })
            .await
            .unwrap_or_else(|error| {
                Ok(Reply::Failed {
                    operation: operation_id,
                    origin: failed_origin,
                    text: format!("本地命令中断：{error}"),
                    draft: retained,
                    revision,
                })
            });
            (generation, result)
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
            let context = self.clone();
            let loaded = tokio::task::spawn_blocking(move || {
                let mut session: Session = Store::open(&self.paths.database())?.load(&id)?;
                session.normalize_team();
                let transcript = super::history::load_view(&self.paths, &id, self.view.clone())?;
                let recent = super::launch::recent(&self.paths, &session.workspace)?;
                Ok(Reply::Open {
                    context: self,
                    session: Box::new(session),
                    transcript,
                    recent,
                })
            })
            .await
            .unwrap_or_else(|error| Err(error.into()));
            (
                generation,
                loaded.or_else(|error: anyhow::Error| {
                    Ok(Reply::OpenFailed {
                        context,
                        error: error.to_string(),
                    })
                }),
            )
        });
    }
    pub fn create(self, jobs: &mut Jobs, control: Control, draft: String) {
        let generation = self.generation;
        jobs.spawn(async move {
            let context = self.clone();
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
                        context,
                        id,
                        error,
                        draft,
                        revision: self.revision,
                    })
                }
                Ok(Err(error)) => Ok(Reply::Failed {
                    operation: self.operation.clone(),
                    origin: self.view.clone(),
                    text: error.to_string(),
                    draft,
                    revision: self.revision,
                }),
                Err(error) => Ok(Reply::Failed {
                    operation: self.operation.clone(),
                    origin: self.view.clone(),
                    text: format!("创建会话中断：{error}"),
                    draft,
                    revision: self.revision,
                }),
            };
            (generation, result)
        });
    }
    fn local(
        self,
        jobs: &mut Jobs,
        operation: impl FnOnce(Self) -> anyhow::Result<Reply> + Send + 'static,
    ) {
        let generation = self.generation;
        let origin = self.view.clone();
        jobs.spawn(async move {
            let result = tokio::task::spawn_blocking(move || operation(self))
                .await
                .unwrap_or_else(|error| Err(error.into()));
            (
                generation,
                result.or_else(|error: anyhow::Error| {
                    Ok(Reply::Notice {
                        operation: None,
                        origin,
                        text: format!("本地操作失败：{error}"),
                    })
                }),
            )
        });
    }
    pub fn history(self, jobs: &mut Jobs, query: String, tools: bool) {
        self.local(jobs, move |context| {
            let id = context
                .session
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("请先打开会话"))?;
            Ok(Reply::History {
                origin: context.view.clone(),
                browser: super::history::Browser::new(
                    super::history::load_view(&context.paths, id, context.view.clone())?,
                    query,
                    tools,
                ),
            })
        });
    }
    pub fn copy_selector(self, jobs: &mut Jobs, selector: String) {
        self.local(jobs, move |context| {
            let id = context
                .session
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("请先打开会话"))?;
            let transcript = super::history::load_view(&context.paths, id, context.view.clone())?;
            super::clipboard::copy(&super::history::selected_text(&transcript, &selector)?)?;
            Ok(Reply::Copied(context.view))
        });
    }
    pub fn copy(self, jobs: &mut Jobs, text: String) {
        self.local(jobs, move |context| {
            super::clipboard::copy(&text)?;
            Ok(Reply::Copied(context.view))
        });
    }
}

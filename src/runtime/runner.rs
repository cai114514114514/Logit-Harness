use super::{
    actor::Role,
    state::{Control, Session},
};
use crate::{
    domain::{EventData, SessionEvent, Status},
    model::{ModelEvent, Provider, Response},
    store::Store,
    tools::{Scope, ToolEvent, ToolHost, ToolResult},
};
use std::{collections::BTreeMap, sync::Arc};
use tokio::{
    sync::{broadcast, mpsc},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub struct Settings {
    pub workers: usize,
    pub max_output: u32,
    pub context_limit: usize,
    pub linger: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            workers: 2,
            max_output: 32768,
            context_limit: 256 * 1024,
            linger: true,
        }
    }
}

pub(super) enum Wire {
    Model { actor: String, event: ModelEvent },
    Tool { id: String, event: ToolEvent },
}
pub(super) enum ResultData {
    Model(anyhow::Result<Response>),
    Tool(anyhow::Result<ToolResult>),
}
pub(super) enum JobKind {
    Model { reservation: String, compact: bool },
    Tool { action: String },
}
pub(super) struct Job {
    pub actor: String,
    pub scope: Scope,
    pub cancel: CancellationToken,
    pub kind: JobKind,
}

pub struct Runner {
    pub(super) session: Session,
    pub(super) store: Arc<Store>,
    pub(super) provider: Arc<dyn Provider>,
    pub(super) tools: Arc<ToolHost>,
    pub(super) settings: Settings,
    pub(super) control: mpsc::Receiver<Control>,
    pub(super) events: broadcast::Sender<SessionEvent>,
    pub(super) wire: mpsc::UnboundedReceiver<Wire>,
    pub(super) send: mpsc::UnboundedSender<Wire>,
    pub(super) tasks: JoinSet<(String, ResultData)>,
    pub(super) jobs: BTreeMap<String, Job>,
    pub(super) target: Option<Status>,
}
impl Runner {
    pub fn new(
        session: Session,
        store: Arc<Store>,
        provider: Arc<dyn Provider>,
        tools: Arc<ToolHost>,
        settings: Settings,
    ) -> (
        Self,
        mpsc::Sender<Control>,
        broadcast::Receiver<SessionEvent>,
    ) {
        let (control, receiver) = mpsc::channel(64);
        let (events, subscriber) = broadcast::channel(1024);
        let (send, wire) = mpsc::unbounded_channel();
        (
            Self {
                session,
                store,
                provider,
                tools,
                settings,
                control: receiver,
                events,
                send,
                wire,
                tasks: JoinSet::new(),
                jobs: BTreeMap::new(),
                target: None,
            },
            control,
            subscriber,
        )
    }
    pub(super) fn emit(&self, data: EventData) -> anyhow::Result<()> {
        let event = self.store.commit(&self.session.id, &self.session, data)?;
        let _ = self.events.send(event);
        Ok(())
    }
    pub(super) fn status(&mut self, status: Status, reason: &str) -> anyhow::Result<()> {
        self.session.status = status.clone();
        self.emit(EventData::Status {
            status,
            reason: reason.into(),
        })
    }
    pub(super) fn frozen(&self) -> bool {
        self.session.inbox.iter().any(|m| !m.applied)
    }
    pub async fn run(mut self) -> anyhow::Result<Session> {
        self.status(Status::Running, "working toward the saved objective")?;
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            if self.jobs.is_empty() {
                if let Some(target) = self.target.take() {
                    self.status(target, "all active operations have settled")?;
                }
                if self.session.status == Status::Stopped
                    || (!self.settings.linger
                        && matches!(
                            self.session.status,
                            Status::Completed
                                | Status::Waiting
                                | Status::Exhausted
                                | Status::Failed
                        ))
                {
                    return Ok(self.session);
                }
            }
            if self.target.is_none()
                && matches!(self.session.status, Status::Running | Status::Waiting)
            {
                self.schedule().await?;
            }
            tokio::select! {
                biased;
                Some(control)=self.control.recv()=>self.handle_control(control).await?,
                Some(wire)=self.wire.recv()=>self.handle_wire(wire)?,
                Some(result)=self.tasks.join_next(),if !self.tasks.is_empty()=>{
                    let (id,result)=result?;self.handle_result(id,result).await?;
                }
                _=tick.tick()=>{
                    if self.session.status==Status::Running && !self.jobs.is_empty(){
                        self.session.elapsed+=1;
                        if self.session.elapsed>=self.session.seconds {self.halt(Status::Exhausted,"execution time budget reached")?;}
                    }
                }
            }
        }
    }
    pub(super) fn halt(&mut self, target: Status, reason: &str) -> anyhow::Result<()> {
        for job in self.jobs.values() {
            job.cancel.cancel();
        }
        self.target = Some(target.clone());
        self.status(
            if target == Status::Stopped {
                Status::Stopping
            } else {
                Status::Pausing
            },
            reason,
        )
    }
    pub(super) fn main_feedback(&mut self, text: &str) {
        if let Some(main) = self.session.actors.get_mut("main") {
            main.context.user(text);
            main.phase = super::actor::Phase::Ready;
        }
    }
    pub(super) fn has_workers(&self) -> bool {
        self.session
            .actors
            .values()
            .any(|a| a.role == Role::Worker && a.phase != super::actor::Phase::Done)
    }
}

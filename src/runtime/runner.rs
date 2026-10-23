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

/// Instrumentation observes the same durable events as clients. An evaluation script can
/// enqueue a correction at an exact event boundary before the following model dispatch.
pub trait Observer: Send + Sync {
    fn on_event(&self, event: &SessionEvent) -> anyhow::Result<()>;
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
    Model {
        id: String,
        actor: String,
        event: ModelEvent,
    },
    Tool {
        id: String,
        event: ToolEvent,
    },
}
pub(super) enum ResultData {
    Model(anyhow::Result<Response>),
    Tool(anyhow::Result<ToolResult>),
}
pub(super) enum JobKind {
    Model {
        reservation: String,
        compact: bool,
        prior: super::actor::Phase,
    },
    Tool {
        action: String,
    },
}
pub(super) struct Job {
    pub goal: String,
    pub turn: String,
    pub actor: String,
    pub scope: Scope,
    pub cancel: CancellationToken,
    pub kind: JobKind,
}

pub struct Runner {
    pub(super) lease: Option<super::ownership::Lease>,
    pub(super) recover: bool,
    clock: super::clock::Clock,
    observer: Option<Arc<dyn Observer>>,
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
}
impl Runner {
    pub fn new(
        mut session: Session,
        store: Arc<Store>,
        provider: Arc<dyn Provider>,
        tools: Arc<ToolHost>,
        settings: Settings,
    ) -> (
        Self,
        mpsc::Sender<Control>,
        broadcast::Receiver<SessionEvent>,
    ) {
        if session.team.members.is_empty()
            && session.team.receipts.is_empty()
            && session.team.limit == 2
        {
            session.team.limit = settings.workers;
        }
        session.normalize_team();
        if session.followup.is_none()
            && session.interactive
            && session.status == Status::Completed
            && session.mode == super::conversation::Mode::Conversation
            && session.goal.id.is_empty()
        {
            session.followup = session
                .archives
                .last()
                .filter(|a| a.status == Status::Completed)
                .map(|a| a.goal.id.clone());
        }
        let (control, receiver) = mpsc::channel(64);
        let (events, subscriber) = broadcast::channel(1024);
        let (send, wire) = mpsc::unbounded_channel();
        (
            Self {
                lease: None,
                recover: false,
                clock: super::clock::Clock::new(),
                observer: None,
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
            },
            control,
            subscriber,
        )
    }
    pub(super) fn emit(&self, data: EventData) -> anyhow::Result<()> {
        let event = if matches!(&data, EventData::Delta { .. }) {
            self.store.append(&self.session.id, data)?
        } else {
            self.store.commit(&self.session.id, &self.session, data)?
        };
        self.publish(event)
    }
    pub(super) fn publish(&self, event: SessionEvent) -> anyhow::Result<()> {
        if let Some(observer) = &self.observer {
            observer.on_event(&event)?;
        }
        let _ = self.events.send(event);
        Ok(())
    }
    pub fn observe(mut self, observer: Arc<dyn Observer>) -> Self {
        self.observer = Some(observer);
        self
    }
    pub fn recover_on_start(mut self) -> Self {
        self.recover = true;
        self
    }
    pub(super) fn status(&mut self, status: Status, reason: &str) -> anyhow::Result<()> {
        self.account_time()?;
        if status == Status::Completed
            && self.session.mode == super::conversation::Mode::Engineering
        {
            return self.completion_candidate(reason);
        }
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
        if self.recover && self.session.mode != super::conversation::Mode::Engineering {
            super::recovery::reconcile(&mut self.session, &self.store, &self.tools.logs).await?;
            self.recover = false;
        }
        self.refresh_plan().await?;
        self.refresh_questions().await;
        if self.session.status == Status::Ready {
            self.status(Status::Running, "working toward the saved objective")?;
        }
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            // Reconcile a restarted engineering owner before consuming controls
            // that might replace its actors or discard interrupted write records.
            if self.recover && !self.execution_owner().await? {
                tick.tick().await;
                continue;
            }
            self.check_time()?;
            self.drain_inbox().await?;
            while let Ok(control) = self.control.try_recv() {
                self.handle_control(control).await?;
            }
            self.settle_members()?;
            if self.jobs.is_empty() {
                self.settle_mcp().await?;
                if let Some(target) = self.session.transition.take() {
                    self.status(target, "all active operations have settled")?;
                }
                self.settle_boundary().await?;
                self.settle_skills()?;
                self.execution_owner().await?;
                self.prepare_compaction()?;
                if self.session.status == Status::Stopped
                    || (!self.settings.linger
                        && self.session.owner.is_none()
                        && !self.frozen()
                        && self.session.implementation.is_none()
                        && !self.session.actors.values().any(|a| a.compact)
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
            // Controls and settled cancellations can restore Running after the
            // loop's first clock check. Recheck at the shared scheduling boundary
            // so no resumed mode dispatches work against an expired allowance.
            self.check_time()?;
            if self.session.transition.is_none()
                && matches!(
                    self.session.status,
                    Status::Running | Status::Waiting | Status::Paused | Status::Completed
                )
                && self.schedule().await?
            {
                // Local state transitions should not wait for the wall-clock tick.
                // Yield to I/O, then drain durable and direct controls before more work.
                tokio::task::yield_now().await;
                while let Ok(control) = self.control.try_recv() {
                    self.handle_control(control).await?;
                }
                continue;
            }
            tokio::select! {
                biased;
                Some(control)=self.control.recv()=>{
                    self.check_time()?;
                    self.handle_control(control).await?;
                }
                // Settled cancellation outranks a buffered stream. Otherwise an
                // already-finished job can retain ownership behind old deltas.
                Some(result)=self.tasks.join_next(),if !self.tasks.is_empty()=>{
                    self.check_time()?;
                    let (id,result)=result?;self.handle_result(id,result).await?;
                }
                Some(wire)=self.wire.recv()=>{
                    self.check_time()?;
                    self.handle_wire(wire)?;
                }
                _=tick.tick()=>self.check_time()?,
            }
        }
    }
    fn account_time(&mut self) -> anyhow::Result<()> {
        let active = self.session.status == Status::Running
            || (self.session.status == Status::Waiting && !self.jobs.is_empty());
        let seconds = self.clock.sample(active);
        if seconds != 0 {
            self.session.elapsed = self.session.elapsed.saturating_add(seconds);
            // Delta events omit full snapshots. A long stream must still save
            // elapsed time before a crash, even without a finished model/tool.
            self.emit(EventData::Elapsed {
                seconds: self.session.elapsed,
            })?;
        }
        Ok(())
    }
    fn check_time(&mut self) -> anyhow::Result<()> {
        self.account_time()?;
        if matches!(self.session.status, Status::Running | Status::Waiting)
            && self.session.elapsed >= self.session.seconds
        {
            self.halt(Status::Exhausted, "execution time budget reached")?;
        }
        Ok(())
    }
    pub(super) fn halt(&mut self, target: Status, reason: &str) -> anyhow::Result<()> {
        for job in self.jobs.values() {
            job.cancel.cancel();
        }
        self.session.transition = Some(target.clone());
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
        self.session.completion = None;
        self.session.verification_pending = false;
        if let Some(main) = self.session.actors.get_mut("main") {
            main.step = None;
            main.scope.steps.clear();
            main.context.user(text);
            main.phase = super::actor::Phase::Ready;
        }
    }
    pub(super) fn has_workers(&self) -> bool {
        self.session
            .actors
            .values()
            .any(|a| a.role == Role::Worker && !a.retired && a.phase != super::actor::Phase::Done)
    }
}

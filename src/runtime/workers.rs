//! Workers own contexts and dependency leases; only the shared tool host commits proposals.
use super::{
    actor::{Actor, Pending, Phase, Role},
    graph::StepStatus,
    runner::Runner,
};
use crate::{domain::EventData, tools::Scope};
use serde_json::{Value, json};

impl Runner {
    pub(super) fn scope_current(&self, scope: &Scope) -> bool {
        scope.epoch == self.session.goal.epoch
            && scope.requirements.iter().all(|(id, rev)| {
                self.session
                    .goal
                    .requirements
                    .get(id)
                    .is_some_and(|r| r.active && r.revision == *rev)
            })
            && scope.steps.iter().all(|(id, rev)| {
                self.session
                    .graph
                    .steps
                    .get(id)
                    .is_some_and(|s| s.revision == *rev)
            })
    }

    pub(super) fn delegate(&mut self, caller: &str, pending: &Pending) -> anyhow::Result<()> {
        let args = &pending.call.arguments;
        let step = args["step"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("delegate needs a step ID"))?;
        let task = args["task"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("delegate needs a task"))?;
        anyhow::ensure!(
            self.scope_current(&pending.scope),
            "delegation came from an obsolete plan or requirement version"
        );
        let active: Vec<_> = self
            .session
            .actors
            .values()
            .filter(|a| a.role == Role::Worker && !a.retired && a.phase != Phase::Done)
            .collect();
        anyhow::ensure!(
            active.len() < self.settings.workers,
            "worker limit reached; continue independent work or wait_workers"
        );
        anyhow::ensure!(
            !active.iter().any(|a| a.step.as_deref() == Some(step)),
            "step already has an active worker"
        );
        let scope = self.session.graph.scope(step, &self.session.goal)?;
        anyhow::ensure!(
            !self.scope_blocked(&scope),
            "worker step is waiting for user input"
        );
        let planned = &self.session.graph.steps[step];
        anyhow::ensure!(
            planned
                .dependencies
                .iter()
                .all(|id| self.session.graph.steps[id].status == StepStatus::Verified),
            "step dependencies need finish_step evidence first"
        );
        let id = format!("worker-{}", uuid::Uuid::new_v4());
        let mut actor = Actor::new(id.clone(), Role::Worker, task.into(), scope);
        actor.step = Some(step.into());
        self.session.actors.insert(id.clone(), actor);
        self.session.graph.steps.get_mut(step).unwrap().status = StepStatus::Running;
        let main = self.session.actors.get_mut(caller).unwrap();
        main.context
            .tool_result(&pending.call.id, &json!({"worker":id,"step":step}));
        main.phase = Phase::Ready;
        self.emit(EventData::Worker {
            id,
            status: "started".into(),
            task: task.into(),
        })
    }

    pub(super) async fn propose(&mut self, actor: &str, pending: &Pending) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.session.actors[actor].retired && self.scope_current(&pending.scope),
            "worker proposal is obsolete"
        );
        let mut proposal = pending.clone();
        proposal.call.name = "apply_patch".into();
        // This is the only elevation a worker receives. The host still validates
        // requirement/step versions and every observed file under its mutation lock.
        proposal.scope.readonly = false;
        self.start_tool(actor, proposal).await
    }

    pub(super) fn finish_step(&mut self, pending: &Pending) -> anyhow::Result<Value> {
        let id = pending.call.arguments["step"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing step"))?;
        let ids: Vec<String> = serde_json::from_value(pending.call.arguments["evidence"].clone())?;
        let scope = self.session.graph.scope(id, &self.session.goal)?;
        let files = self.tools.workspace.snapshot_for(
            self.session
                .evidence
                .iter()
                .flat_map(|evidence| evidence.files.keys()),
        )?;
        let support: Vec<_> = ids
            .iter()
            .filter_map(|id| self.session.evidence.iter().find(|e| e.id == *id))
            .collect();
        anyhow::ensure!(
            !support.is_empty()
                && support.len() == ids.len()
                && support.iter().all(|e| e.current_at(
                    &self.session.goal,
                    &files,
                    &self.session.graph.versions()
                )),
            "step needs current passing evidence"
        );
        anyhow::ensure!(
            scope
                .requirements
                .keys()
                .all(|id| support.iter().any(|e| e.requirements.contains_key(id))),
            "step evidence omits dependent requirements"
        );
        self.session.graph.steps.get_mut(id).unwrap().status = StepStatus::Verified;
        Ok(json!({"step":id,"verified":true}))
    }

    pub(super) async fn refresh_plan(&mut self) -> anyhow::Result<()> {
        *self.tools.steps.write().await = self.session.graph.versions();
        if let Some(services) = &self.tools.services
            && self.session.condition.cancels_active()
        {
            services
                .invalidate(&self.session.goal, &self.session.graph.versions())
                .await?;
        }
        for job in self.jobs.values() {
            if self.session.condition.cancels_active() && !self.scope_current(&job.scope) {
                job.cancel.cancel();
            }
        }
        let obsolete: Vec<_> = self
            .session
            .actors
            .values()
            .filter(|a| {
                matches!(a.role, Role::Worker | Role::Verifier | Role::Diagnosis)
                    && !a.retired
                    && !self.scope_current(&a.scope)
            })
            .map(|a| a.id.clone())
            .collect();
        for id in obsolete {
            let actor = self.session.actors.get_mut(&id).unwrap();
            actor.retired = true;
            actor.phase = Phase::Done;
            for pending in actor.pending.drain(..) {
                actor.context.tool_result(
                    &pending.call.id,
                    &json!({"error":"cancelled before dispatch because a dependency changed"}),
                );
            }
            let task = actor.task.clone();
            self.emit(EventData::Worker {
                id,
                status: "invalidated".into(),
                task,
            })?;
        }
        Ok(())
    }

    pub(super) fn worker_finished(&mut self, id: &str, text: &str) -> anyhow::Result<()> {
        let actor = &self.session.actors[id];
        if actor.retired {
            return Ok(());
        }
        let task = actor.task.clone();
        self.main_feedback(&format!("Worker {id} finished its scoped attempt: {text}\nInspect its changes and evidence, then continue the complete objective."));
        self.emit(EventData::Worker {
            id: id.into(),
            status: "finished".into(),
            task,
        })
    }
}

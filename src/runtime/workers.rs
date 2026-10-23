//! Workers own contexts and dependency leases; only the shared tool host commits proposals.
use super::{
    actor::{Pending, Phase, Role},
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
        let scope = self.session.graph.scope(step, &self.session.goal)?;
        anyhow::ensure!(
            !self.scope_blocked(&scope),
            "worker step is waiting for user input"
        );
        let assignment = self
            .session
            .assign_member(args["member"].as_str(), step, task)?;
        let id = assignment.actor.clone();
        let main = self.session.actors.get_mut(caller).unwrap();
        main.context
            .tool_result(&pending.call.id, &json!({"worker":id,"step":step,"member":assignment.member,"assignment":assignment.id,"version":assignment.version}));
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
        let member = self
            .session
            .team
            .member_for_actor(actor)
            .ok_or_else(|| anyhow::anyhow!("proposal has no member owner"))?;
        let assignment = member
            .assignment
            .as_ref()
            .and_then(|id| self.session.team.assignments.get(id))
            .ok_or_else(|| anyhow::anyhow!("proposal assignment has settled"))?;
        anyhow::ensure!(
            assignment.actor == actor && !member.paused && !member.stopping,
            "proposal is not from the current executing owner"
        );
        let edits = pending.call.arguments["edits"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("proposal needs edits"))?;
        for edit in edits {
            let path = edit["path"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("edit path missing"))?;
            let candidate = std::path::Path::new(path);
            anyhow::ensure!(
                !candidate.is_absolute()
                    && candidate
                        .components()
                        .all(|part| matches!(part, std::path::Component::Normal(_))),
                "invalid proposal path"
            );
            anyhow::ensure!(
                assignment.writable.iter().any(|allowed| {
                    path == allowed || (allowed.ends_with('/') && path.starts_with(allowed))
                }),
                "path {path} is outside assignment; request_scope from main before writing"
            );
        }
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
            self.session
                .finish_assignment(&id, super::team::AssignmentStatus::Invalidated);
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
        self.session
            .finish_assignment(id, super::team::AssignmentStatus::Finished);
        self.main_feedback(&format!("Worker {id} finished its scoped attempt: {text}\nInspect its changes and evidence, then continue the complete objective."));
        self.emit(EventData::Worker {
            id: id.into(),
            status: "finished".into(),
            task,
        })
    }
}

use super::{Assignment, AssignmentStatus, Member, Target};
use crate::runtime::{
    actor::{Actor, Phase, Role},
    graph::StepStatus,
    runner::Runner,
    state::Session,
};

impl Session {
    pub fn member_target(&self, name: &str) -> anyhow::Result<Target> {
        self.team.target(name)
    }

    /// Main is the sole caller in the runtime. Retain ownership while paused/waiting.
    pub fn assign_member(
        &mut self,
        name: Option<&str>,
        step: &str,
        task: &str,
    ) -> anyhow::Result<Assignment> {
        anyhow::ensure!(!task.trim().is_empty(), "assignment needs a task");
        let planned = self
            .graph
            .steps
            .get(step)
            .ok_or_else(|| anyhow::anyhow!("unknown step: {step}"))?;
        anyhow::ensure!(
            planned.status != StepStatus::Verified,
            "step already verified"
        );
        anyhow::ensure!(
            planned
                .dependencies
                .iter()
                .all(|id| self.graph.steps[id].status == StepStatus::Verified),
            "step dependencies need finish_step evidence first"
        );
        anyhow::ensure!(
            !self
                .team
                .assignments
                .values()
                .any(|a| a.step == step && a.status == AssignmentStatus::Active),
            "step already has an active assignment"
        );
        // A cancelled model response has no write authority. Actual writes must
        // settle before any replacement can acquire this step's lease.
        anyhow::ensure!(
            !self.actions.values().any(|action| action.running
                && !action.pending.scope.readonly
                && action.pending.scope.steps.contains_key(step)),
            "previous assignment writes are still settling"
        );
        let selected = match name {
            Some(name) => self
                .team
                .members
                .values()
                .find(|m| m.name == name || m.id == name)
                .map(|m| m.id.clone()),
            None => self
                .team
                .members
                .values()
                .find(|m| {
                    m.assignment.is_none()
                        && !m.paused
                        && !m.stopping
                        && m.actor.as_ref().is_none_or(|id| {
                            self.actors.get(id).is_none_or(|a| a.phase == Phase::Done)
                        })
                })
                .map(|m| m.id.clone()),
        };
        if let Some(id) = &selected {
            let member = &self.team.members[id];
            anyhow::ensure!(
                member.assignment.is_none() && !member.stopping,
                "member already owns an assignment"
            );
            anyhow::ensure!(
                member
                    .actor
                    .as_ref()
                    .is_none_or(|id| self.actors.get(id).is_none_or(|a| a.phase == Phase::Done)),
                "member conversation is still active"
            );
        }
        let scope = self.graph.scope(step, &self.goal)?;
        let member_id = selected.unwrap_or_else(|| format!("member-{}", uuid::Uuid::new_v4()));
        let member_name = name.map(str::to_owned).unwrap_or_else(|| {
            let mut number = self.team.members.len() + 1;
            loop {
                let candidate = format!("member-{number}");
                if !self.team.members.values().any(|m| m.name == candidate) {
                    break candidate;
                }
                number += 1;
            }
        });
        anyhow::ensure!(
            member_name != "main" && !member_name.trim().is_empty(),
            "reserved or empty member name"
        );
        let actor_id = format!("worker-{}", uuid::Uuid::new_v4());
        let assignment = Assignment {
            id: uuid::Uuid::new_v4().to_string(),
            version: planned.revision,
            member: member_id.clone(),
            actor: actor_id.clone(),
            step: step.into(),
            task: task.into(),
            writable: planned.files.clone(),
            status: AssignmentStatus::Active,
        };
        let mut actor = Actor::new(actor_id.clone(), Role::Worker, task.into(), scope);
        actor.target = Some(Target::Member {
            id: member_id.clone(),
            assignment: Some(super::AssignmentRef {
                id: assignment.id.clone(),
                version: assignment.version,
            }),
        });
        actor.turn = self.actors["main"].turn.clone();
        actor.step = Some(step.into());
        let member = self
            .team
            .members
            .entry(member_id.clone())
            .or_insert(Member {
                id: member_id,
                name: member_name,
                assignment: None,
                actor: None,
                attempts: vec![],
                paused: false,
                stopping: false,
                waiting: None,
                mail_seen: 0,
                compact_pending: false,
            });
        member.assignment = Some(assignment.id.clone());
        member.actor = Some(actor_id.clone());
        member.attempts.push(actor_id.clone());
        member.waiting = None;
        self.actors.insert(actor_id, actor);
        self.team
            .assignments
            .insert(assignment.id.clone(), assignment.clone());
        self.graph.steps.get_mut(step).unwrap().status = StepStatus::Running;
        Ok(assignment)
    }

    pub fn finish_assignment(&mut self, actor: &str, status: AssignmentStatus) {
        let Some(assignment) = self
            .team
            .assignments
            .values_mut()
            .find(|a| a.actor == actor && a.status == AssignmentStatus::Active)
        else {
            return;
        };
        assignment.status = status;
        let member = self.team.members.get_mut(&assignment.member).unwrap();
        member.assignment = None;
        member.waiting = None;
        if let Some(actor) = self.actors.get_mut(actor) {
            actor.retired = true;
            actor.phase = Phase::Done;
        }
    }

    /// Deterministic migration: old actor IDs become member IDs, never reused actors.
    pub fn normalize_team(&mut self) {
        let legacy: Vec<_> = self
            .actors
            .values()
            .filter(|a| a.role == Role::Worker && self.team.member_for_actor(&a.id).is_none())
            .cloned()
            .collect();
        for actor in legacy {
            let id = actor.id.clone();
            let active = !actor.retired && actor.phase != Phase::Done;
            let assignment_id = format!("legacy-{id}");
            let step = actor.step.clone().unwrap_or_default();
            self.team.assignments.insert(
                assignment_id.clone(),
                Assignment {
                    id: assignment_id.clone(),
                    version: actor.scope.steps.get(&step).copied().unwrap_or(1),
                    member: id.clone(),
                    actor: id.clone(),
                    task: actor.task.clone(),
                    writable: self
                        .graph
                        .steps
                        .get(&step)
                        .map(|s| s.files.clone())
                        .unwrap_or_default(),
                    step,
                    status: if active {
                        AssignmentStatus::Active
                    } else {
                        AssignmentStatus::Finished
                    },
                },
            );
            self.team.members.insert(
                id.clone(),
                Member {
                    id: id.clone(),
                    name: id.clone(),
                    assignment: active.then_some(assignment_id),
                    actor: Some(id.clone()),
                    attempts: vec![id],
                    paused: false,
                    stopping: false,
                    waiting: None,
                    mail_seen: 0,
                    compact_pending: false,
                },
            );
        }
    }
}

impl Runner {
    pub(crate) fn settle_members(&mut self) -> anyhow::Result<()> {
        let members: Vec<_> = self.session.team.members.values().cloned().collect();
        for member in members {
            let Some(id) = &member.actor else {
                continue;
            };
            if self
                .jobs
                .values()
                .any(|j| member.attempts.contains(&j.actor))
            {
                continue;
            }
            if member.stopping {
                for actor in self
                    .session
                    .actors
                    .values_mut()
                    .filter(|a| member.attempts.contains(&a.id))
                {
                    actor.retired = true;
                    actor.phase = Phase::Done;
                    for pending in actor.pending.drain(..) {
                        actor.context.tool_result(
                            &pending.call.id,
                            &serde_json::json!({"error":"member stopped"}),
                        );
                    }
                }
                let approvals: Vec<_> = self
                    .session
                    .actions
                    .values()
                    .filter(|a| member.attempts.contains(&a.actor) && !a.running)
                    .map(|a| a.id.clone())
                    .collect();
                for action_id in approvals {
                    let action = self.session.actions.remove(&action_id).unwrap();
                    self.session
                        .actors
                        .get_mut(&action.actor)
                        .unwrap()
                        .context
                        .tool_result(
                            &action.pending.call.id,
                            &serde_json::json!({"error":"member stopped before authorization"}),
                        );
                    self.session.settled_actions.insert(action_id, action);
                }
                self.session
                    .finish_assignment(id, AssignmentStatus::Stopped);
                self.session
                    .team
                    .members
                    .get_mut(&member.id)
                    .unwrap()
                    .stopping = false;
                self.emit(crate::domain::EventData::Diagnostic {
                    message: format!("member {} stopped after operations settled", member.name),
                })?;
            } else if member.compact_pending
                && let Some(actor) = self.session.actors.get_mut(id)
                && actor.pending.is_empty()
                && actor.context.unanswered().is_empty()
            {
                actor.compact = true;
                self.session
                    .team
                    .members
                    .get_mut(&member.id)
                    .unwrap()
                    .compact_pending = false;
            }
        }
        Ok(())
    }
}

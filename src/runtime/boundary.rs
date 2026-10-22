//! Goal/mode transitions are settlement barriers, never immediate actor replacement.
use super::{
    actor::{Actor, Phase, Role},
    conversation::{Archive, Boundary, Mode, conversation_actor, empty_goal},
    runner::Runner,
};
use crate::{
    domain::{EventData, Goal, Status},
    tools::Scope,
};
use serde_json::json;

impl Runner {
    pub(super) fn request_boundary(&mut self, boundary: Boundary) -> anyhow::Result<()> {
        self.session.boundary = Some(boundary);
        let blocked: Vec<_> = self
            .session
            .actions
            .values()
            .filter(|a| !a.running)
            .map(|a| a.id.clone())
            .collect();
        for id in blocked {
            let action = self.session.actions.remove(&id).unwrap();
            let result = json!({"cancelled":"user changed execution mode before authorization"});
            if let Some(actor) = self.session.actors.get_mut(&action.actor) {
                actor.context.tool_result(&action.pending.call.id, &result);
            }
            self.session.settled_actions.insert(id.clone(), action);
            self.emit(EventData::ActionFinished {
                id,
                result,
                success: false,
            })?;
        }
        for actor in self.session.actors.values_mut() {
            // Pair queued calls before changing modes. Running calls are paired by
            // handle_result, even when the provider ignores cancellation.
            for pending in actor.pending.drain(..) {
                actor.context.tool_result(
                    &pending.call.id,
                    &json!({"cancelled":"user changed execution mode"}),
                );
            }
            actor.retired = true;
            if actor.busy == 0 {
                actor.phase = Phase::Done;
            }
        }
        self.halt(
            Status::Running,
            "settling active operations before changing execution mode",
        )
    }

    pub(super) async fn settle_boundary(&mut self) -> anyhow::Result<()> {
        if !self.jobs.is_empty() {
            return Ok(());
        }
        if self.session.boundary.is_some()
            && !matches!(
                self.session.status,
                Status::Ready | Status::Running | Status::Waiting | Status::Completed
            )
        {
            return Ok(());
        }
        if self.session.mode == Mode::Engineering
            && self.session.status == Status::Completed
            && self.session.interactive
            && self.session.boundary.is_none()
        {
            if self.session.queue.is_empty() {
                self.archive_goal();
                self.session.mode = Mode::Conversation;
                self.session.goal = empty_goal(self.session.goal.epoch.saturating_add(1));
                self.session.graph = Default::default();
                self.session.questions.clear();
                self.session.actors.insert(
                    "main".into(),
                    conversation_actor("", self.session.goal.epoch, true),
                );
                self.session.actors.get_mut("main").unwrap().phase = Phase::Done;
                *self.tools.goal.write().await = self.session.goal.clone();
                self.emit(EventData::Diagnostic {
                    message: "verified goal archived; ready for a new conversation turn".into(),
                })?;
            } else {
                let task = self.session.queue.remove(0);
                self.session.boundary = Some(Boundary::Goal {
                    text: task.text,
                    turn: task.turn,
                });
            }
        }
        let Some(boundary) = self.session.boundary.take() else {
            return Ok(());
        };
        let turn = match &boundary {
            Boundary::Plan { turn, .. } => turn.clone(),
            Boundary::Goal { turn, .. }
            | Boundary::Implement { turn, .. }
            | Boundary::ResumeGoal { turn, .. }
            | Boundary::Conversation { turn, .. } => Some(turn.clone()),
        };
        self.session.transition = None;
        let epoch = self.session.goal.epoch.saturating_add(1);
        match boundary {
            Boundary::Plan { text, .. } => {
                // Cancellation is not evidence that writes had no effect. Re-read
                // source only at this explicit project boundary, after writers exit.
                let snapshot = if self.session.mode == Mode::Engineering {
                    Some(self.tools.workspace.fingerprints()?)
                } else {
                    None
                };
                if self.session.mode == Mode::Engineering {
                    self.session.suspended = self.session.actors.get("main").cloned();
                }
                self.session.mode = Mode::Plan;
                self.session.goal.epoch = epoch;
                self.session.plan_input = text.clone().or(self.session.plan_input.take());
                if let Some(text) = &text {
                    self.session.plan_turns.push(text.clone());
                }
                let idle = text.as_ref().is_none_or(|text| text.trim().is_empty());
                let task = text.unwrap_or_default();
                self.session
                    .actors
                    .insert("main".into(), conversation_actor(&task, epoch, false));
                if let Some(snapshot) = snapshot {
                    self.session.actors.get_mut("main").unwrap().scope.observed = snapshot;
                }
                if idle {
                    self.session.actors.get_mut("main").unwrap().phase = Phase::Done;
                }
                self.session.verification_pending = false;
            }
            Boundary::Implement { text, turn } => {
                self.reconcile_plan(text, turn, epoch);
            }
            Boundary::ResumeGoal { text, .. } => {
                self.resume_planned_goal(&text, epoch)?;
            }
            Boundary::Goal { text, .. } => {
                self.archive_goal();
                self.session.mode = Mode::Engineering;
                let mut goal = Goal::new(&text);
                goal.epoch = epoch;
                self.session.goal = goal;
                self.session.graph = Default::default();
                self.session.evidence.clear();
                self.session.settled_actions.clear();
                self.session.questions.clear();
                self.session.approvals.clear();
                self.session.final_report = None;
                self.session.verification_pending = false;
                self.session.replan_pending = false;
                self.session.prompt_pending = false;
                self.session.actors.clear();
                self.session.actors.insert(
                    "main".into(),
                    Actor::new(
                        "main".into(),
                        Role::Main,
                        text,
                        Scope {
                            epoch,
                            requirements: self.session.goal.active_versions(),
                            ..Scope::default()
                        },
                    ),
                );
                self.session.suspended = None;
                self.session.plan_input = None;
                self.session.plan_turns.clear();
            }
            Boundary::Conversation { text, turn } => {
                self.session.goal.epoch = epoch;
                self.add_turn(&text, true, Some(turn));
                self.session.actors.get_mut("main").unwrap().scope.epoch = epoch;
            }
        }
        *self.tools.goal.write().await = self.session.goal.clone();
        if let Some(turn) = turn {
            self.session.actors.get_mut("main").unwrap().turn = turn;
        }
        self.refresh_plan().await?;
        self.refresh_questions().await;
        let status = if self.session.mode == Mode::Plan
            && self.session.actors["main"].phase == Phase::Done
        {
            Status::Waiting
        } else {
            Status::Running
        };
        self.status(status, "execution boundary settled")
    }

    fn archive_goal(&mut self) {
        if self.session.goal.id.is_empty()
            || self
                .session
                .archives
                .iter()
                .any(|a| a.goal.id == self.session.goal.id)
        {
            return;
        }
        self.session.archives.push(Archive {
            goal: self.session.goal.clone(),
            status: if self.session.status == Status::Completed {
                Status::Completed
            } else {
                Status::Stopped
            },
            graph: self.session.graph.clone(),
            actors: self.session.actors.clone(),
            evidence: self.session.evidence.clone(),
            report: self.session.final_report.clone(),
            actions: self.session.settled_actions.clone(),
        });
    }

    pub(super) fn prepare_compaction(&mut self) -> anyhow::Result<()> {
        if !self.session.compact_pending
            || !self.jobs.is_empty()
            || self
                .session
                .actors
                .values()
                .filter(|a| !a.retired && (a.id == "main" || a.phase != Phase::Done))
                .any(|a| !a.pending.is_empty() || !a.context.unanswered().is_empty())
        {
            return Ok(());
        }
        self.session.compact_pending = false;
        for actor in self
            .session
            .actors
            .values_mut()
            .filter(|a| !a.retired && (a.id == "main" || a.phase != Phase::Done))
        {
            actor.compact = true;
        }
        self.emit(EventData::Diagnostic { message: "compaction queued at a fully paired tool boundary; durable requirements and corrections remain authoritative".into() })
    }
}

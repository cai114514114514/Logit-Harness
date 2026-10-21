use super::{
    actor::{Action, Phase, Role},
    prompts,
    runner::{Job, JobKind, ResultData, Runner, Wire},
};
use crate::{
    domain::{EventData, Status},
    model::Request,
    tools::Effect,
};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

impl Runner {
    pub(super) async fn schedule(&mut self) -> anyhow::Result<bool> {
        let mut progressed = false;
        self.start_steering();
        if self.session.replan_pending {
            if !self.jobs.is_empty() {
                return Ok(false);
            }
            self.restart_plan();
        }
        if !self.frozen()
            && self.session.status == Status::Running
            && self.session.actors["main"].phase == Phase::Done
            && self.session.final_report.is_some()
        {
            self.start_verifier()?;
        }
        let mut ids: Vec<_> = self.session.actors.keys().cloned().collect();
        ids.sort_by_key(|id| {
            if self.session.actors[id].role == Role::Steering {
                0
            } else {
                1
            }
        });
        for id in ids {
            if self.session.actors[&id].retired || self.session.actors[&id].phase == Phase::Blocked
            {
                continue;
            }
            let role = self.session.actors[&id].role.clone();
            if self.session.status == Status::Paused
                && !matches!(role, Role::Steering | Role::Question)
            {
                continue;
            }
            if role == Role::Main
                && !self.blocked_requirements().is_empty()
                && self
                    .session
                    .goal
                    .active_versions()
                    .keys()
                    .all(|id| self.blocked_requirements().contains(id))
            {
                self.session.actors.get_mut(&id).unwrap().phase = Phase::Blocked;
                continue;
            }
            if role == Role::Worker && self.scope_blocked(&self.session.actors[&id].scope) {
                continue;
            }
            if self.frozen() && role != Role::Steering {
                continue;
            }
            if self.session.actors[&id].busy > 0 {
                continue;
            }
            if let Some(pending) = self
                .session
                .actors
                .get_mut(&id)
                .unwrap()
                .pending
                .pop_front()
            {
                progressed = true;
                match self.special_call(&id, &pending).await {
                    Ok(true) => continue,
                    Ok(false) => (),
                    Err(error) => {
                        let actor = self.session.actors.get_mut(&id).unwrap();
                        actor
                            .context
                            .tool_result(&pending.call.id, &json!({"error":error.to_string()}));
                        actor.phase = if actor.pending.is_empty() {
                            Phase::Ready
                        } else {
                            Phase::Awaiting
                        };
                        self.emit(EventData::Diagnostic {
                            message: format!("{} rejected: {error}", pending.call.name),
                        })?;
                        self.record_progress(&id, &json!({"tool":pending.call.name,"error":error.to_string(),"requirements":pending.scope.requirements,"steps":pending.scope.steps}))?;
                        continue;
                    }
                }
                self.start_tool(&id, pending).await?;
                continue;
            }
            if self.session.actors[&id].phase == Phase::Ready {
                let active = self
                    .jobs
                    .values()
                    .filter(|j| matches!(j.kind, JobKind::Model { .. }))
                    .count();
                // Steering gets one reserved request lane. Long work requests must
                // not prevent us from interpreting the user's correction.
                let capacity = self.settings.workers + 1 + usize::from(role == Role::Steering);
                if active < capacity {
                    let before = self.jobs.len();
                    self.start_model(&id)?;
                    progressed |= self.jobs.len() != before;
                }
            }
        }
        if !progressed
            && self.jobs.is_empty()
            && !self.blocked_requirements().is_empty()
            && self.session.status == Status::Running
        {
            self.status(
                Status::Waiting,
                "independent work is settled; dependent steps await user input",
            )?;
            progressed = true;
        }
        Ok(progressed)
    }
    fn start_model(&mut self, actor_id: &str) -> anyhow::Result<()> {
        let actor = self.session.actors.get_mut(actor_id).unwrap();
        if matches!(
            actor.role,
            Role::Main | Role::Verifier | Role::Steering | Role::Question
        ) {
            actor.scope.epoch = self.session.goal.epoch;
            if let Some(step) = &actor.step {
                let scoped = self.session.graph.scope(step, &self.session.goal)?;
                actor.scope.requirements = scoped.requirements;
                actor.scope.steps = scoped.steps;
            } else {
                actor.scope.requirements = self.session.goal.active_versions();
            }
        }
        let guidance = if let Some(knowledge) = &self.tools.knowledge {
            knowledge.prompt(&mut actor.scope)?
        } else {
            String::new()
        };
        let mut tools = self.tools.definitions();
        let actor = &self.session.actors[actor_id];
        if matches!(
            actor.role,
            Role::Verifier | Role::Worker | Role::Question | Role::Diagnosis
        ) {
            tools.retain(|v| {
                self.tools
                    .effect(v["name"].as_str().unwrap_or(""))
                    .is_some_and(Effect::readonly)
            });
        }
        if actor.role == Role::Steering {
            tools.clear();
        }
        tools.extend(prompts::special(&actor.role));
        if actor.role == Role::Worker {
            if let Some(mut proposal) = self
                .tools
                .definitions()
                .into_iter()
                .find(|t| t["name"] == "apply_patch")
            {
                proposal["name"] = json!("propose_patch");
                proposal["description"] = json!(
                    "Submit a patch proposal to the central owner. The owner rechecks your original requirement, step and observed file versions before committing it."
                );
                tools.push(proposal);
            }
        }
        if actor.role == Role::Verifier {
            for tool in &mut tools {
                if tool["name"] == "report_verification" {
                    tool["parameters"]["properties"]["checks"]["items"]["properties"]["requirement"]
                        ["enum"] = json!(
                        self.session
                            .goal
                            .active_versions()
                            .keys()
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
        let compact = actor.role != Role::Steering
            && serde_json::to_vec(&actor.context.items)?.len() > self.settings.context_limit;
        let request = Request {
            instructions: if compact {
                "ROLE: compact\nSummarize verified progress, failed attempts, unresolved requirements and source pointers. Do not invent completion or discard constraints.".into()
            } else {
                format!(
                    "{}{guidance}\nActor task: {}\nActor dependency scope: {}",
                    prompts::instructions(&actor.role, &self.session),
                    actor.task,
                    serde_json::to_string(&actor.scope)?
                )
            },
            input: actor.context.items.clone(),
            tools: if compact { vec![] } else { tools },
            max_output: if compact {
                4096
            } else if actor.role == Role::Steering {
                self.settings.max_output.min(8192)
            } else {
                self.settings.max_output
            },
        };
        let reservation = match self.session.budget.reserve(request.reserve_tokens()) {
            Ok(id) => id,
            Err(_) => {
                let available_after_settlement = self
                    .session
                    .budget
                    .limit
                    .saturating_sub(self.session.budget.used)
                    .saturating_sub(self.session.budget.uncertain);
                if !self.session.budget.reservations.is_empty()
                    && request.reserve_tokens() <= available_after_settlement
                {
                    return Ok(());
                }
                self.halt(
                    Status::Exhausted,
                    "token allowance cannot cover the next request",
                )?;
                return Ok(());
            }
        };
        let actor = self.session.actors.get_mut(actor_id).unwrap();
        actor.phase = Phase::Busy;
        actor.busy += 1;
        actor.turns += 1;
        let scope = actor.scope.clone();
        if actor.role == Role::Main {
            self.session.prompt_pending = false;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let artifact = self.tools.logs.join(format!("{id}.request.json"));
        std::fs::create_dir_all(&self.tools.logs)?;
        std::fs::write(
            &artifact,
            serde_json::to_vec(
                &json!({"instructions":request.instructions,"input":request.input,"tools":request.tools,"max_output":request.max_output}),
            )?,
        )?;
        self.emit(EventData::ModelRequested {
            actor: actor_id.into(),
            artifact: artifact.to_string_lossy().into_owned(),
        })?;
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        self.jobs.insert(
            id.clone(),
            Job {
                actor: actor_id.into(),
                scope,
                cancel,
                kind: JobKind::Model {
                    reservation,
                    compact,
                },
            },
        );
        let provider = self.provider.clone();
        let send = self.send.clone();
        let actor = actor_id.to_owned();
        self.tasks.spawn(async move {
            let (output, mut receiver) = mpsc::unbounded_channel();
            let relay = tokio::spawn(async move {
                while let Some(event) = receiver.recv().await {
                    let _ = send.send(Wire::Model {
                        actor: actor.clone(),
                        event,
                    });
                }
            });
            let result = provider.respond(request, token, output).await;
            let _ = relay.await;
            (id, ResultData::Model(result))
        });
        Ok(())
    }
    pub(super) async fn start_tool(
        &mut self,
        actor_id: &str,
        mut pending: super::actor::Pending,
    ) -> anyhow::Result<()> {
        pending.scope.approvals = self.session.approvals.clone();
        let id = uuid::Uuid::new_v4().to_string();
        self.session.actions.insert(
            id.clone(),
            Action {
                id: id.clone(),
                actor: actor_id.into(),
                pending: pending.clone(),
                running: true,
                pid: None,
            },
        );
        {
            let actor = self.session.actors.get_mut(actor_id).unwrap();
            actor.busy += 1;
            actor.phase = Phase::Awaiting;
        }
        self.emit(EventData::ActionStarted {
            id: id.clone(),
            tool: pending.call.name.clone(),
            arguments: pending.call.arguments.clone(),
        })?;
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let scope = pending.scope.clone();
        self.jobs.insert(
            id.clone(),
            Job {
                actor: actor_id.into(),
                scope,
                cancel,
                kind: JobKind::Tool { action: id.clone() },
            },
        );
        let host = self.tools.clone();
        let send = self.send.clone();
        let action = id.clone();
        self.tasks.spawn(async move {
            let (output, mut receiver) = mpsc::unbounded_channel();
            let relay = tokio::spawn(async move {
                while let Some(event) = receiver.recv().await {
                    let _ = send.send(Wire::Tool {
                        id: action.clone(),
                        event,
                    });
                }
            });
            // Log paths use our action ID; the provider's call ID remains in the transcript.
            let mut call = pending.call;
            call.id = id.clone();
            let result = host.execute(call, pending.scope, token, output).await;
            let _ = relay.await;
            (id, ResultData::Tool(result))
        });
        Ok(())
    }
}

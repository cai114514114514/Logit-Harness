//! Correlate the global stream before selecting a view. Tool deltas use action
//! IDs, model deltas may use response IDs, and both inherit durable actor binding.
use crate::{
    domain::EventData,
    runtime::{
        team::Target,
        view::{ActorBinding, ViewTarget},
    },
};
use std::collections::BTreeMap;

#[derive(Clone, Default)]
pub(super) struct Ownership {
    pub bindings: BTreeMap<String, ActorBinding>,
    aliases: BTreeMap<String, String>,
}
impl Ownership {
    pub fn observe(&mut self, data: &EventData) {
        match data {
            EventData::ActorBound {
                actor,
                target,
                goal,
            } => {
                self.bindings.insert(
                    actor.clone(),
                    ActorBinding {
                        actor: actor.clone(),
                        target: target.clone(),
                        goal: goal.clone(),
                    },
                );
            }
            EventData::Identity {
                actor,
                response,
                action,
                ..
            } => {
                self.aliases.insert(response.clone(), actor.clone());
                if let Some(action) = action {
                    self.aliases.insert(action.clone(), actor.clone());
                }
            }
            EventData::ActionScoped { id, actor, .. } => {
                self.aliases.insert(id.clone(), actor.clone());
            }
            _ => (),
        }
    }
    pub fn actor(&self, id: &str) -> ViewTarget {
        let actor = self.aliases.get(id).map_or(id, String::as_str);
        self.bindings
            .get(actor)
            .map(|b| ViewTarget {
                target: b.target.clone(),
                goal: b.goal.clone(),
                readonly: false,
            })
            .unwrap_or_default()
    }
    pub fn event(&self, data: &EventData) -> ViewTarget {
        match data {
            EventData::ActorBound { target, goal, .. } => ViewTarget {
                target: target.clone(),
                goal: goal.clone(),
                readonly: false,
            },
            EventData::Mail { goal, mail } => ViewTarget {
                target: mail.target.clone(),
                goal: Some(goal.clone()),
                readonly: false,
            },
            EventData::RequestReceipt { goal, receipt } => ViewTarget {
                target: receipt.request.target.clone(),
                goal: Some(goal.clone()),
                readonly: false,
            },
            EventData::Identity { actor, .. }
            | EventData::ModelRequested { actor, .. }
            | EventData::ModelFinished { actor, .. }
            | EventData::Message { actor, .. }
            | EventData::Delta { actor, .. }
            | EventData::Usage { actor, .. } => self.actor(actor),
            EventData::ActionStarted { id, .. }
            | EventData::ActionFinished { id, .. }
            | EventData::ActionScoped { id, .. }
            | EventData::ProcessStarted { id, .. } => self.actor(id),
            EventData::Diagnostic { message } => {
                let member = message.split_once(": ").map(|(id, _)| id);
                self.bindings
                    .values()
                    .find(|binding| Some(binding.target.member_id()) == member)
                    .map(|binding| ViewTarget {
                        target: binding.target.clone(),
                        goal: binding.goal.clone(),
                        readonly: false,
                    })
                    .unwrap_or_default()
            }
            _ => ViewTarget::main(),
        }
    }
}
pub(super) fn same_view(a: &ViewTarget, b: &ViewTarget) -> bool {
    a.target.member_id() == b.target.member_id() && (a.is_main() || a.goal == b.goal)
}
pub(super) fn assignment(target: &Target) -> String {
    match target {
        Target::Main => "main".into(),
        Target::Member {
            assignment: Some(a),
            ..
        } => format!("{} v{}", a.id, a.version),
        Target::Member { .. } => "空闲 · 无分配".into(),
    }
}

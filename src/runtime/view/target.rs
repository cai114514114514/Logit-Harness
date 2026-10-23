//! Immutable command origins and public archive lookup, without model contexts.
use super::SessionView;
use crate::runtime::{
    state::Session,
    team::{Assignment, MemberView, Target, Team},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewTarget {
    pub target: Target,
    pub goal: Option<String>,
    pub readonly: bool,
}
impl Default for ViewTarget {
    fn default() -> Self {
        Self::main()
    }
}
impl ViewTarget {
    pub fn main() -> Self {
        Self {
            target: Target::Main,
            goal: None,
            readonly: false,
        }
    }
    pub fn is_main(&self) -> bool {
        self.target == Target::Main
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActorBinding {
    pub actor: String,
    pub target: Target,
    pub goal: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TargetView {
    pub origin: ViewTarget,
    pub session: SessionView,
    pub member: Option<MemberView>,
    pub assignments: Vec<Assignment>,
    pub goal: Option<crate::domain::Goal>,
}

impl Session {
    pub(crate) fn archived_target(
        &self,
        target: &Target,
    ) -> Option<&crate::runtime::conversation::Archive> {
        let Target::Member { id, assignment } = target else {
            return None;
        };
        self.archives.iter().rev().find(|archive| {
            archive.team.members.contains_key(id)
                && assignment.as_ref().is_none_or(|reference| {
                    archive
                        .team
                        .assignments
                        .get(&reference.id)
                        .is_some_and(|a| a.member == *id && reference.version <= a.version)
                })
        })
    }
    pub fn resolve_view(&self, selector: &str) -> anyhow::Result<ViewTarget> {
        if selector == "main" {
            return Ok(ViewTarget::main());
        }
        // A current name takes precedence; archived names must be unambiguous.
        if self
            .team
            .members
            .values()
            .any(|m| m.id == selector || m.name == selector)
        {
            return Ok(ViewTarget {
                target: self.team.target(selector)?,
                goal: Some(self.goal.id.clone()),
                readonly: false,
            });
        }
        if let Some(archive) = self
            .archives
            .iter()
            .rev()
            .find(|a| a.team.members.contains_key(selector))
        {
            return Ok(ViewTarget {
                target: archive.team.target(selector)?,
                goal: Some(archive.goal.id.clone()),
                readonly: true,
            });
        }
        let matches = self
            .archives
            .iter()
            .filter_map(|archive| {
                archive.team.target(selector).ok().map(|target| ViewTarget {
                    target,
                    goal: Some(archive.goal.id.clone()),
                    readonly: true,
                })
            })
            .collect::<Vec<_>>();
        anyhow::ensure!(
            matches.len() == 1,
            "unknown or ambiguous archived member: {selector}; use its stable member ID"
        );
        Ok(matches[0].clone())
    }

    /// Refresh display metadata without rebinding a draft to a replacement assignment.
    pub fn target_view(&self, origin: &ViewTarget) -> anyhow::Result<TargetView> {
        let mut origin = origin.clone();
        let (team, goal): (Option<&Team>, _) = if origin.is_main() {
            (None, Some(&self.goal))
        } else if origin.goal.as_deref() == Some(&self.goal.id)
            && self.team.members.contains_key(origin.target.member_id())
        {
            (Some(&self.team), Some(&self.goal))
        } else {
            let archive = self
                .archives
                .iter()
                .find(|a| {
                    origin.goal.as_deref() == Some(&a.goal.id)
                        && a.team.members.contains_key(origin.target.member_id())
                })
                .ok_or_else(|| anyhow::anyhow!("member history is unavailable; reopen /team"))?;
            origin.readonly = true;
            (Some(&archive.team), Some(&archive.goal))
        };
        let member = team.and_then(|team| {
            team.view()
                .members
                .into_iter()
                .find(|m| m.id == origin.target.member_id())
        });
        let assignments = team
            .map(|team| {
                team.assignments
                    .values()
                    .filter(|a| a.member == origin.target.member_id())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        Ok(TargetView {
            assignments,
            origin,
            session: self.clone().into(),
            member,
            goal: goal.cloned(),
        })
    }

    pub fn actor_bindings(&self) -> std::collections::BTreeMap<String, ActorBinding> {
        let mut bindings = std::collections::BTreeMap::new();
        for archive in &self.archives {
            for actor in archive.actors.values() {
                bindings.insert(
                    actor.id.clone(),
                    ActorBinding {
                        actor: actor.id.clone(),
                        target: actor.target.clone().unwrap_or(Target::Main),
                        goal: Some(archive.goal.id.clone()),
                    },
                );
            }
        }
        for actor in self.actors.values() {
            bindings.insert(
                actor.id.clone(),
                ActorBinding {
                    actor: actor.id.clone(),
                    target: actor.target.clone().unwrap_or(Target::Main),
                    goal: actor
                        .target
                        .as_ref()
                        .and_then(|target| {
                            self.archives
                                .iter()
                                .find(|a| a.team.members.contains_key(target.member_id()))
                                .map(|a| a.goal.id.clone())
                        })
                        .or_else(|| (!self.goal.id.is_empty()).then(|| self.goal.id.clone())),
                },
            );
        }
        bindings
    }
}

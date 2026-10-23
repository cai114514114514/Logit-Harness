//! Normalize live and archived ownership in memory without changing old events.
use super::{Assignment, AssignmentRef, AssignmentStatus, Member, Target, Team};
use crate::runtime::{
    actor::{Actor, Phase, Role},
    graph::Graph,
};
use std::collections::BTreeMap;

pub(super) fn normalize(team: &mut Team, actors: &mut BTreeMap<String, Actor>, graph: &Graph) {
    for actor in actors.values_mut().filter(|a| a.role == Role::Worker) {
        let id = &actor.id;
        if team.member_for_actor(id).is_none() {
            let active = !actor.retired && actor.phase != Phase::Done;
            let assignment_id = format!("legacy-{id}");
            let step = actor.step.clone().unwrap_or_default();
            team.assignments.insert(
                assignment_id.clone(),
                Assignment {
                    id: assignment_id.clone(),
                    version: actor.scope.steps.get(&step).copied().unwrap_or(1),
                    member: id.clone(),
                    actor: id.clone(),
                    task: actor.task.clone(),
                    writable: graph
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
            team.members.insert(
                id.clone(),
                Member {
                    id: id.clone(),
                    name: id.clone(),
                    assignment: active.then_some(assignment_id),
                    actor: Some(id.clone()),
                    attempts: vec![id.clone()],
                    paused: false,
                    interrupted: false,
                    stopping: false,
                    waiting: None,
                    mail_seen: 0,
                    compact_pending: false,
                },
            );
        }
        if actor.target.is_none()
            && let Some(assignment) = team.assignments.values().find(|a| a.actor == *id)
        {
            // The member may already be idle or own a newer task. Bind this
            // attempt to its original assignment, never the member's current one.
            actor.target = Some(Target::Member {
                id: assignment.member.clone(),
                assignment: Some(AssignmentRef {
                    id: assignment.id.clone(),
                    version: actor
                        .scope
                        .steps
                        .get(&assignment.step)
                        .copied()
                        .unwrap_or(assignment.version),
                }),
            });
        }
    }
}

//! A supported verdict may wait for controls without issuing another model request.
use super::runner::Runner;
use crate::{
    domain::{EventData, Fingerprints, Status},
    tools::Scope,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Candidate {
    pub scope: Scope,
    pub files: Fingerprints,
    pub reason: String,
}

impl Runner {
    pub(super) fn completion_candidate(&mut self, reason: &str) -> anyhow::Result<()> {
        self.session.completion = Some(Candidate {
            scope: Scope {
                epoch: self.session.goal.epoch,
                requirements: self.session.goal.active_versions(),
                steps: self.session.graph.versions(),
                ..Scope::default()
            },
            files: self
                .tools
                .workspace
                .snapshot_for(self.session.evidence.iter().flat_map(|e| e.files.keys()))?,
            reason: reason.into(),
        });
        // The caller still has to pair its meta-tool response. Finalize at the
        // next scheduling boundary, after controls and that transcript settle.
        self.emit(EventData::Diagnostic {
            message: "acceptance candidate saved; checking settled controls and evidence".into(),
        })
    }

    pub(super) fn try_completion(&mut self) -> anyhow::Result<bool> {
        let Some(candidate) = self.session.completion.clone() else {
            return Ok(false);
        };
        if !self.scope_current(&candidate.scope)
            || self.tools.workspace.snapshot_for(candidate.files.keys())? != candidate.files
        {
            self.session.completion = None;
            self.session.final_report = None;
            self.main_feedback("Acceptance evidence changed while controls were settling. Inspect the current deliverable and refresh verification.");
            return Ok(true);
        }
        if self.session.status != Status::Running
            || self.frozen()
            || !self.session.team.engineering_settled()
            || !self.blocked_requirements().is_empty()
            || !self.session.actions.is_empty()
            || self.session.actors["main"].phase != super::actor::Phase::Done
            || self.session.actors["main"].busy != 0
            || !self.session.actors["main"].pending.is_empty()
            || self.session.boundary.is_some()
            || self.session.transition.is_some()
        {
            return Ok(false);
        }
        let mut completed = self.session.clone();
        completed.status = Status::Completed;
        completed.completion = None;
        completed.verification_pending = false;
        let Some(event) = self.store.commit_if_controls_current(
            &completed.id,
            completed.cursor,
            &completed,
            EventData::Status {
                status: Status::Completed,
                reason: candidate.reason,
            },
        )?
        else {
            return Ok(false);
        };
        // In-memory state changes only after SQLite has atomically accepted it.
        self.session = completed;
        self.publish(event)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests;

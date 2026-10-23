//! A message after completion can revise the full previous objective. Reading
//! that archive is not permission to reacquire engineering ownership.
use super::{
    conversation::{Archive, Mode},
    runner::Runner,
    state::Session,
};
use crate::domain::{EventData, Status};

impl Session {
    pub(crate) fn followup_goal(&self) -> Option<&Archive> {
        let id = self.followup.as_ref()?;
        self.archives
            .iter()
            .rev()
            .find(|archive| &archive.goal.id == id)
    }
}

impl Runner {
    pub(super) async fn restore_followup(&mut self) -> anyhow::Result<()> {
        let id = self
            .session
            .followup
            .take()
            .ok_or_else(|| anyhow::anyhow!("completed objective missing"))?;
        let position = self
            .session
            .archives
            .iter()
            .rposition(|a| a.goal.id == id)
            .ok_or_else(|| anyhow::anyhow!("completed objective archive missing"))?;
        let archive = self.session.archives.remove(position);
        self.session.goal = archive.goal;
        self.session.graph = archive.graph;
        let conversations = std::mem::take(&mut self.session.actors);
        self.session.actors = archive.actors;
        self.session
            .actors
            .extend(conversations.into_iter().filter(|(id, _)| id != "main"));
        let receipts = std::mem::take(&mut self.session.team.receipts);
        self.session.team = archive.team;
        self.session.team.receipts.extend(receipts);
        self.session.evidence = archive.evidence;
        self.session.settled_actions = archive.actions;
        self.session.final_report = archive.report;
        self.session.mode = Mode::Engineering;
        self.session.status = Status::Running;
        self.session.completion = None;
        self.session.verification_pending = false;
        *self.tools.goal.write().await = self.session.goal.clone();
        self.refresh_plan().await?;
        self.emit(EventData::Diagnostic {
            message: "reopened the complete saved objective for the user's correction".into(),
        })
    }
}

//! A planning conversation is context, not a list of new requirements. Reconcile
//! actual changes once, through structured steering, before restoring the owner.
use super::{
    actor::{Actor, Phase, Role},
    conversation::Task,
    runner::Runner,
    state::UserMessage,
};
use crate::tools::Scope;

impl Runner {
    pub(super) fn reconcile_plan(&mut self, text: String, turn: String) {
        // A bare /plan may precede several ordinary messages. Let structured
        // reconciliation select actual request quotes before creating any goal;
        // implementation authorization is not a fallback acceptance requirement.
        if self.session.plan_turns.last() != Some(&text) {
            self.session.plan_turns.push(text.clone());
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.session.inbox.push(UserMessage {
            id: id.clone(),
            text: self.session.plan_turns.join("\n"),
            applied: false,
            prior: self.session.status.clone(),
            revision: self.session.goal.revision,
        });
        self.session.implementation = Some(Task { id, text, turn });
        let main = self.session.actors.get_mut("main").unwrap();
        main.phase = Phase::Done;
        main.retired = false;
    }

    pub(super) fn resume_planned_goal(&mut self, text: &str, epoch: u64) -> anyhow::Result<()> {
        let planner = self.session.actors["main"].context.clone();
        let mut main = self.session.suspended.take().unwrap_or_else(|| {
            Actor::new(
                "main".into(),
                Role::Main,
                self.session.goal.original.clone(),
                Scope::default(),
            )
        });
        main.role = Role::Main;
        main.retired = false;
        main.phase = Phase::Ready;
        main.busy = 0;
        main.context.extend_history(&planner)?;
        main.context.user(text);
        main.scope = Scope {
            epoch,
            requirements: self.session.goal.active_versions(),
            ..Scope::default()
        };
        self.session.actors.insert("main".into(), main);
        self.session.mode = super::conversation::Mode::Engineering;
        self.session.goal.epoch = epoch;
        self.session.final_report = None;
        self.session.verification_pending = false;
        self.session.plan_turns.clear();
        self.session.plan_input = None;
        Ok(())
    }
}

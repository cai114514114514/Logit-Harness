//! Baseline behavior is explicit and auditable rather than hidden in prompt wording alone.
use super::{
    actor::{Phase, Role},
    runner::Runner,
};
use crate::{model::Context, tools::Scope};

impl Runner {
    pub(super) fn append_prompt_correction(&mut self, text: &str) {
        self.session.prompt_pending = true;
        self.session.verification_pending = false;
        self.session.final_report = None;
        for actor in self
            .session
            .actors
            .values_mut()
            .filter(|actor| !actor.retired && matches!(actor.role, Role::Main | Role::Worker))
        {
            actor.context.user(text);
            if actor.busy == 0 && actor.pending.is_empty() {
                actor.phase = Phase::Ready;
            }
        }
    }

    pub(super) fn restart_plan(&mut self) {
        let main = self.session.actors.get_mut("main").expect("main actor");
        // Old actions have settled before this reset. Their request/result artifacts remain
        // in the event log, while the baseline deliberately gives up contextual reuse.
        main.pending.clear();
        main.context = Context::default();
        main.context.user("Replan the entire current objective from the actual tree. Preserve existing files and user edits; obtain fresh evidence for every requirement.");
        main.scope = Scope {
            requirements: self.session.goal.active_versions(),
            epoch: self.session.goal.epoch,
            ..Default::default()
        };
        main.step = None;
        main.failures = 0;
        main.phase = Phase::Ready;
        self.session.replan_pending = false;
    }
}

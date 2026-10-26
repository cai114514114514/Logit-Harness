//! Local work is identified independently of the editor and durable receipts.
//! Finishing one operation must not clear another view's newer submission.
use super::{jobs::Context, ownership::same_view};
use crate::runtime::view::ViewTarget;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Feedback {
    pending: BTreeMap<String, ViewTarget>,
}
impl Feedback {
    pub fn start(&mut self, context: &Context) {
        self.pending
            .insert(context.operation.clone(), context.view.clone());
    }
    pub fn finish(&mut self, operation: &str) {
        self.pending.remove(operation);
    }
    pub fn lines(&self, view: &ViewTarget) -> Vec<String> {
        let count = self
            .pending
            .values()
            .filter(|origin| same_view(origin, view))
            .count();
        if count == 0 {
            return vec![];
        }
        let stale = self
            .pending
            .values()
            .filter(|origin| same_view(origin, view))
            .any(|origin| origin.target != view.target || origin.goal != view.goal);
        vec![format!(
            "正在发送 · {count} 条输入尚未确认{}",
            if stale { "（旧任务）" } else { "" }
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cli::terminal::Screen, config::Paths, runtime::team::Target};
    #[test]
    fn late_completion_only_settles_its_operation_and_original_view() {
        let mut screen = Screen::default();
        let paths = Paths::new(std::env::temp_dir());
        let main = Context::new(0, &paths, &screen);
        let second = Context::new(0, &paths, &screen);
        screen.feedback.start(&main);
        screen.feedback.start(&second);
        screen.view = ViewTarget {
            target: Target::Member {
                id: "Ada".into(),
                assignment: None,
            },
            goal: Some("goal".into()),
            readonly: false,
        };
        assert!(screen.feedback.lines(&screen.view).is_empty());
        let ada = Context::new(0, &paths, &screen);
        screen.feedback.start(&ada);
        screen.feedback.finish(&main.operation);
        assert!(screen.feedback.lines(&screen.view)[0].contains("1 条"));
        screen.feedback.finish(&main.operation); // late duplicate ack
        assert!(screen.feedback.lines(&ViewTarget::main())[0].contains("1 条"));
        screen.feedback.finish(&ada.operation);
        assert!(screen.feedback.lines(&screen.view).is_empty());
        assert!(!screen.feedback.lines(&ViewTarget::main()).is_empty());
        screen.feedback.finish(&second.operation);
        assert!(screen.feedback.lines(&ViewTarget::main()).is_empty());
    }
}

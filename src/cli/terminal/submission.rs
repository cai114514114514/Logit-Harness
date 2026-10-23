//! Ownership of the first asynchronous command and revision-bound Skill choices.
use super::{Screen, jobs::Context};
use crate::{
    commands::{Command, Timing},
    extensions::skills::Entry,
};

#[derive(Default)]
pub(super) struct Initial {
    revision: Option<u64>,
}

impl Initial {
    /// Reserve before service work starts. The later draft remains in the editor
    /// until the initial request resolves and its one session has been opened.
    pub fn prepare(
        &mut self,
        screen: &mut Screen,
        mut context: Context,
        command: &Command,
        opening: bool,
    ) -> Option<Context> {
        if screen.stale_draft()
            && !matches!(
                command,
                Command::Builtin {
                    kind: crate::commands::Kind::Agent
                        | crate::commands::Kind::Team
                        | crate::commands::Kind::History
                        | crate::commands::Kind::New,
                    ..
                }
            )
        {
            screen.note("草稿属于旧任务，已保留且未提交。Ctrl+U 清空后重新输入，明确发送给当前分配；/history 可核对旧任务。");
            return None;
        }
        let runtime = match command {
            Command::Message(_) | Command::Skill { .. } => true,
            // Browsing Skills is local, but the same catalog entry also offers
            // source-qualified invocation. Reserve conservatively until resolved.
            Command::Builtin {
                kind: crate::commands::Kind::Skills,
                ..
            } => true,
            Command::Builtin { kind, .. } => crate::commands::DIRECTORY
                .iter()
                .find(|entry| entry.kind == *kind)
                .is_some_and(|entry| entry.timing != Timing::Local),
        };
        if runtime && (opening || context.session.is_none() && self.revision.is_some()) {
            screen.note("首条请求正在处理或会话正在打开；当前草稿已保留，请稍后发送。");
            return None;
        }
        screen.input.take();
        screen.bind_draft();
        context.revision = screen.input.revision();
        if runtime && context.session.is_none() {
            self.revision = Some(context.revision);
        }
        Some(context)
    }
    pub fn owns(&self, context: &Context) -> bool {
        context.session.is_some() || self.revision == Some(context.revision)
    }
    pub fn finish(&mut self, revision: u64) -> bool {
        if self.revision != Some(revision) {
            return false;
        }
        self.revision = None;
        true
    }
}

pub(super) struct Choice {
    pub context: Context,
    pub entries: Vec<Entry>,
    pub arguments: String,
}

pub(super) fn install_choice(
    screen: &mut Screen,
    context: Context,
    entries: Vec<Entry>,
    arguments: String,
    draft: &str,
) -> bool {
    if !super::ownership::same_view(&context.view, &screen.view) {
        screen.note_origin(
            &context.view,
            format!("技能选择返回原视图，输入保留：\n{draft}"),
        );
        screen.restore_origin(&context.view, draft.into(), context.revision);
        return false;
    }
    if context.revision != screen.input.revision() {
        screen.note(format!(
            "技能来源选择已过期，当前草稿未改变。需要时重新提交：\n{draft}"
        ));
        return false;
    }
    screen.skill_choices = Some(Choice {
        context,
        entries,
        arguments,
    });
    screen.selection = 0;
    true
}

pub(super) fn clear_stale_choice(screen: &mut Screen) {
    if screen
        .skill_choices
        .as_ref()
        .is_some_and(|choice| choice.context.revision != screen.input.revision())
    {
        screen.skill_choices = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::terminal::{
        Screen,
        jobs::{Context, Jobs, Reply},
    };
    use crate::{
        commands::{Command, Outcome, parse},
        config::Paths,
        runtime::state::Control,
    };

    #[tokio::test]
    async fn initial_runtime_owner_survives_both_delayed_service_reply_orders() {
        for first in ["/slow literal '$arg'", "hello"] {
            for local_first in [true, false] {
                let root = tempfile::tempdir().unwrap();
                let paths = Paths::new(root.path().join("state"));
                let mut screen = Screen::default();
                screen.workspace = root.path().into();
                let mut initial = Initial::default();
                screen.input.insert(first);
                let command = parse(first).unwrap();
                let context = Context::new(7, &paths, &screen);
                let owner = initial
                    .prepare(&mut screen, context, &command, false)
                    .unwrap();
                let mut jobs = Jobs::new();
                let (release_owner, owner_wait) = std::sync::mpsc::channel();
                owner.clone().execute(&mut jobs, first.into(), move |_| {
                    owner_wait.recv().unwrap();
                    Ok(Outcome::Control(Control::Message {
                        text: "initial runtime input".into(),
                    }))
                });
                // A second runtime Enter is refused before taking its draft or
                // starting discovery. It cannot become an accidental second owner.
                screen.input.insert("later message");
                let second = Context::new(7, &paths, &screen);
                assert!(
                    initial
                        .prepare(
                            &mut screen,
                            second,
                            &Command::Message("later message".into()),
                            false
                        )
                        .is_none()
                );
                assert_eq!(screen.input.text(), "later message");
                // Local commands remain usable and may complete on either side
                // of the initial service result without releasing its ownership.
                screen.input.replace("/help");
                let local = Context::new(7, &paths, &screen);
                let local = initial
                    .prepare(&mut screen, local, &parse("/help").unwrap(), false)
                    .unwrap();
                let (release_local, local_wait) = std::sync::mpsc::channel();
                local.execute(&mut jobs, "/help".into(), move |_| {
                    local_wait.recv().unwrap();
                    Ok(Outcome::Local {
                        title: "help".into(),
                        text: "local".into(),
                    })
                });
                let senders = if local_first {
                    [release_local, release_owner]
                } else {
                    [release_owner, release_local]
                };
                for release in senders {
                    release.send(()).unwrap();
                    let (_, reply) = jobs.join_next().await.unwrap().unwrap();
                    let Reply::Command {
                        context, outcome, ..
                    } = reply.unwrap()
                    else {
                        panic!("expected service result")
                    };
                    assert!(
                        context.session.is_none(),
                        "reply must retain the original welcome context"
                    );
                    match outcome {
                        Outcome::Local { .. } => {
                            assert!(!initial.finish(context.revision));
                        }
                        Outcome::Control(_) => {
                            assert!(initial.owns(&context));
                            assert_eq!(context.revision, owner.revision);
                        }
                        _ => panic!("unexpected outcome"),
                    }
                    assert!(
                        initial.owns(&owner),
                        "initial owner remains reserved until its session opens"
                    );
                }
                assert!(initial.finish(owner.revision));
            }
        }
    }

    #[test]
    fn stale_skill_choice_never_intercepts_a_newer_draft() {
        let paths = Paths::new(std::env::temp_dir());
        let mut screen = Screen::default();
        screen.input.insert("/same original");
        screen.input.take();
        let context = Context::new(1, &paths, &screen);
        screen.input.insert("newer draft");
        assert!(!install_choice(
            &mut screen,
            context.clone(),
            vec![],
            "original".into(),
            "/same original"
        ));
        assert!(screen.skill_choices.is_none());
        assert_eq!(screen.input.text(), "newer draft");
        let current = Context::new(1, &paths, &screen);
        assert!(install_choice(
            &mut screen,
            current,
            vec![],
            "literal '$arg'".into(),
            "/same"
        ));
        screen.input.insert(" edited");
        clear_stale_choice(&mut screen);
        assert!(screen.skill_choices.is_none());
        assert_eq!(screen.input.text(), "newer draft edited");
    }
    #[tokio::test]
    async fn editing_before_delayed_real_skill_choice_keeps_the_new_enter_target() {
        let root = tempfile::tempdir().unwrap();
        for folder in ["one", "two"] {
            let folder = root.path().join(folder).join("task3-choice");
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(
                folder.join("SKILL.md"),
                "---\nname: task3-choice\ndescription: Fixture\n---\nRead only.",
            )
            .unwrap();
        }
        let paths = Paths::new(root.path().join("state"));
        let mut screen = Screen::default();
        screen.workspace = root.path().into();
        let draft = "/task3-choice literal '$arg'";
        screen.input.insert(draft);
        let command = parse(draft).unwrap();
        let mut initial = Initial::default();
        let context = Context::new(9, &paths, &screen);
        let context = initial
            .prepare(&mut screen, context, &command, false)
            .unwrap();
        let mut jobs = Jobs::new();
        let (release, wait) = std::sync::mpsc::channel();
        context.execute(&mut jobs, draft.into(), move |origin| {
            wait.recv().unwrap();
            crate::commands::Service::new(
                &origin.paths,
                &origin.workspace,
                origin.session.as_deref(),
            )
            .execute(command)
        });
        screen.input.insert("send this newer message");
        release.send(()).unwrap();
        let (_, reply) = jobs.join_next().await.unwrap().unwrap();
        let Reply::Command {
            context,
            outcome: Outcome::ChooseSkill { entries, arguments },
            draft,
        } = reply.unwrap()
        else {
            panic!("expected real source choice")
        };
        assert_eq!(entries.len(), 2);
        assert_eq!(arguments, "literal '$arg'");
        initial.finish(context.revision);
        assert!(!install_choice(
            &mut screen,
            context,
            entries,
            arguments,
            &draft
        ));
        assert!(screen.skill_choices.is_none());
        let command = parse(screen.input.text()).unwrap();
        assert!(matches!(&command, Command::Message(text) if text == "send this newer message"));
        let context = Context::new(9, &paths, &screen);
        assert!(
            initial
                .prepare(&mut screen, context, &command, false)
                .is_some()
        );
    }
}

use super::{Input, transcript::Transcript};
use crate::{
    domain::{EventData, Status},
    runtime::{ipc::ServerMessage, view::SessionView},
};
use std::path::PathBuf;

pub struct Recent {
    pub id: String,
    pub objective: String,
    pub status: Status,
}

#[derive(Default)]
pub struct Screen {
    pub(super) feedback: super::feedback::Feedback,
    pub(super) drafts: super::team::Drafts,
    pub(super) team_picker: bool,
    pub(super) members: Vec<crate::runtime::team::MemberView>,
    pub(super) member: Option<crate::runtime::team::MemberView>,
    pub(super) client_id: String,
    pub view: crate::runtime::view::ViewTarget,
    pub input: Input,
    pub session: Option<SessionView>,
    pub connected: bool,
    pub transcript: Transcript,
    pub workspace: PathBuf,
    pub model: String,
    pub key_present: bool,
    pub recent: Vec<Recent>,
    pub picker: bool,
    pub selection: usize,
    pub(super) frame: usize,
    pub(super) menu_hidden: bool,
    pub(super) activity: String,
    pub(super) completions: Option<Vec<crate::commands::Completion>>,
    pub(super) skill_choices: Option<super::submission::Choice>,
    pub(super) history: Option<super::history::Browser>,
    pub(super) contract: Option<super::contract::Detail>,
    pub(super) setup: Option<super::setup::Form>,
    /// Welcome copy is only for the first entry in this client. `/new` keeps
    /// the conversation layout even while no session has been created yet.
    pub(super) entered: bool,
    sequence: u64,
    snapshot: u64,
}

impl Screen {
    pub fn note(&mut self, text: impl Into<String>) {
        self.transcript.note_in(&self.view, &text.into());
    }
    pub fn menu(&self) -> Vec<crate::commands::Completion> {
        if self.menu_hidden
            || self.picker
            || self.team_picker
            || !self.input.text().starts_with('/')
            || self.input.text().chars().any(char::is_whitespace)
        {
            return vec![];
        }
        let prefix = self.input.text().split_whitespace().next().unwrap_or("/");
        if let Some(entries) = &self.completions {
            entries
                .iter()
                .filter(|e| e.name.starts_with(prefix) || e.insert.starts_with(self.input.text()))
                .cloned()
                .collect()
        } else {
            crate::commands::matches(self.input.text())
                .iter()
                .map(|e| crate::commands::Completion {
                    name: e.name.into(),
                    description: e.description.into(),
                    source: e.source.into(),
                    timing: e.timing,
                    insert: format!("{}{}", e.name, if e.arguments { " " } else { "" }),
                })
                .collect()
        }
    }
    pub fn complete(&mut self) -> bool {
        let draft = self.input.text();
        // Exact optional-argument commands execute on the first Enter. Never replace
        // a typed argument tail with a menu entry (especially literal Skill input).
        if draft.chars().any(char::is_whitespace)
            || crate::commands::DIRECTORY.iter().any(|e| e.name == draft)
        {
            return true;
        }
        let menu = self.menu();
        let Some(command) = menu.get(self.selection.min(menu.len().saturating_sub(1))) else {
            return true;
        };
        let name = command.name.clone();
        let skill = matches!(
            crate::commands::parse(&name),
            Ok(crate::commands::Command::Skill { .. })
        );
        let insertion = if skill {
            command.insert.as_str()
        } else {
            name.as_str()
        };
        let ready = crate::commands::parse(insertion).is_ok();
        self.input
            .replace(if ready { insertion } else { &command.insert });
        self.menu_hidden = true;
        self.selection = 0;
        ready
    }
    pub fn receive(&mut self, message: ServerMessage) {
        match message {
            ServerMessage::Snapshot { state, sequence } => self.receive(ServerMessage::View {
                state: Box::new((*state).into()),
                sequence,
            }),
            ServerMessage::View { state, sequence } => {
                if sequence >= self.snapshot.max(self.sequence) {
                    self.entered = true;
                    self.transcript
                        .ownership
                        .bindings
                        .extend(state.bindings.clone());
                    self.transcript.team(&state.team);
                    self.transcript.contract_view(state.contract.as_ref());
                    let archived = self
                        .members
                        .iter()
                        .filter(|member| member.name.ends_with("（归档只读）"))
                        .cloned()
                        .collect::<Vec<_>>();
                    self.members = state.team.members.clone();
                    self.members.extend(archived);
                    if let Some(member) = self
                        .members
                        .iter()
                        .find(|m| m.id == self.view.target.member_id())
                        .cloned()
                    {
                        self.bind_draft();
                        self.view.target = member.target.clone();
                        self.member = Some(member);
                    }
                    self.transcript.reconcile(&state.activities);
                    self.workspace = state.workspace.clone();
                    self.session = Some(*state);
                    self.snapshot = sequence;
                }
                self.connected = true;
            }
            ServerMessage::Event { event } => {
                if event.sequence <= self.sequence {
                    return;
                }
                self.sequence = event.sequence;
                if event.sequence > self.snapshot
                    && let Some(session) = &mut self.session
                {
                    match &event.data {
                        EventData::Status { status, .. } => session.status = status.clone(),
                        EventData::Elapsed { seconds } => session.elapsed = *seconds,
                        EventData::Usage {
                            input,
                            output,
                            cached,
                            ..
                        } => {
                            session.budget.used = session
                                .budget
                                .used
                                .saturating_add(*input)
                                .saturating_add(*output);
                            session.budget.reported.accumulate(&crate::domain::Usage {
                                input: *input,
                                output: *output,
                                cached: *cached,
                            });
                        }
                        EventData::ModelFinished { actor, .. } => {
                            session.activities.retain(|a| a.id != *actor)
                        }
                        EventData::ActionFinished { id, .. } => {
                            session.activities.retain(|a| a.id != *id)
                        }
                        // The bar follows the ledger at once; the next session
                        // view replaces this projection with derived state.
                        EventData::Contract {
                            revision,
                            epoch,
                            cause,
                            clauses,
                            ..
                        } => {
                            if let Some(contract) = &mut session.contract {
                                contract.revise(*revision, *epoch, clauses, cause == "extract");
                            }
                        }
                        EventData::Acceptance { passed, checks, .. } => {
                            let goal = session.goal.id.clone();
                            if let Some(contract) = &mut session.contract {
                                contract.verdict(&goal, *passed, checks);
                            }
                        }
                        _ => (),
                    }
                }
                if let EventData::Created {
                    objective: text, ..
                }
                | EventData::User { message: text, .. } = &event.data
                    && self.view.is_main()
                {
                    self.input.remember(text);
                }
                self.transcript.receive(&event);
                self.activity.clear();
            }
            ServerMessage::Error { message } => self.note(message),
            ServerMessage::TargetView { state, sequence } => {
                if sequence >= self.snapshot.max(self.sequence)
                    && super::ownership::same_view(&self.view, &state.origin)
                {
                    self.bind_draft();
                    self.view.readonly = state.origin.readonly;
                    if let Some(member) = &state.member {
                        self.view.target = member.target.clone();
                    }
                    self.member = state.member;
                }
                self.receive(ServerMessage::View {
                    state: Box::new(state.session),
                    sequence,
                });
            }
            ServerMessage::Ack
            | ServerMessage::Hello { .. }
            | ServerMessage::Accepted { .. }
            | ServerMessage::Receipt { .. } => (),
        }
    }
    pub fn activity_lines(&self) -> Vec<String> {
        super::activity::lines(self)
    }
    pub fn budget(&self) -> String {
        match &self.session {
            Some(session) => format!(
                "Token：已用 {} / 上限 {}，未确定 {}，预留 {}。运行时间：{} / {} 秒。\n{}\n/budget <总 token 上限> [总秒数上限]，调整后 /resume。实验总金额上限不会随此命令改变。",
                session.budget.used,
                crate::domain::limit::label(session.budget.limit),
                session.budget.uncertain,
                session.budget.reservations.values().sum::<u64>(),
                session.elapsed,
                crate::domain::limit::label(session.seconds),
                self.cache(),
            ),
            None => "正在读取会话预算…".into(),
        }
    }
    pub fn cache(&self) -> String {
        let Some(budget) = self.session.as_ref().map(|session| &session.budget) else {
            return "KV Cache Hit：等待用量数据".into();
        };
        let usage = &budget.reported;
        let Some(hit) = usage.cache_hit() else {
            return "KV Cache Hit：暂无已报告输入用量".into();
        };
        let coverage = if usage.tokens() < budget.used || budget.uncertain != 0 {
            "；不含缺失用量"
        } else {
            ""
        };
        format!(
            "KV Cache Hit：{:.2}%（命中 {} / 已报告输入 {}，未命中 {}{}）",
            hit * 100.0,
            usage.cached,
            usage.input,
            usage.input.saturating_sub(usage.cached),
            coverage,
        )
    }
    pub fn status(&self) -> String {
        match &self.session {
            Some(session) => format!(
                "{} · {}\n契约 v{} · {} 条条款，{} 个执行步骤 · /contract 查看详情。\n{}",
                session.id,
                label(&session.status),
                session.goal.revision,
                session.goal.active_versions().len(),
                session.graph.steps.len(),
                session.title
            ),
            None => "正在连接会话…".into(),
        }
    }
}

pub(super) fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}
pub(super) fn label(status: &Status) -> &'static str {
    match status {
        Status::Ready => "就绪",
        Status::Running => "执行中",
        Status::Waiting => "等待输入",
        Status::Paused => "已暂停",
        Status::Exhausted => "预算已用尽",
        Status::Completed => "验收通过",
        Status::Stopped => "已停止",
        Status::Failed => "需要恢复",
        Status::Pausing => "正在暂停",
        Status::Stopping => "正在停止",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_arguments_leave_arrows_to_the_editor() {
        let mut screen = Screen::default();
        screen.input.insert("/goal 改进输入体验");
        assert!(
            screen.menu().is_empty(),
            "argument editing reopened command picker"
        );
        screen.input.replace("/go");
        assert!(!screen.menu().is_empty());
    }

    #[test]
    fn selected_skill_completion_preserves_qualified_insertion() {
        let mut screen = Screen::default();
        screen.input.insert("/rev");
        screen.completions = Some(vec![crate::commands::Completion {
            name: "/review".into(),
            description: "review".into(),
            source: "project/second".into(),
            timing: crate::commands::Timing::Runtime,
            insert: "/skills run project-second ".into(),
        }]);
        assert!(screen.complete());
        assert_eq!(screen.input.text(), "/skills run project-second ");
    }
}

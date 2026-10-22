use super::{Input, menu, transcript::Transcript};
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
    pub input: Input,
    pub session: Option<SessionView>,
    pub scroll: usize,
    pub connected: bool,
    pub transcript: Transcript,
    pub workspace: PathBuf,
    pub model: String,
    pub key_present: bool,
    pub recent: Vec<Recent>,
    pub picker: bool,
    pub selection: usize,
    pub expanded: bool,
    pub(super) frame: usize,
    pub(super) menu_hidden: bool,
    pub(super) activity: String,
    pub(super) next_session: Option<String>,
    pub(super) setup: Option<super::setup::Form>,
    sequence: u64,
    snapshot: u64,
}

impl Screen {
    pub fn note(&mut self, text: impl Into<String>) {
        self.transcript.note(&text.into());
    }
    pub fn menu(&self) -> Vec<&'static menu::Command> {
        if self.menu_hidden || self.picker {
            vec![]
        } else {
            menu::matches(self.input.text())
        }
    }
    pub fn complete(&mut self) -> bool {
        let menu = self.menu();
        let Some(command) = menu.get(self.selection.min(menu.len().saturating_sub(1))) else {
            return true;
        };
        let text = format!(
            "{}{}",
            command.name,
            if command.arguments { " " } else { "" }
        );
        let ready = !command.arguments;
        self.input.replace(&text);
        self.menu_hidden = true;
        self.selection = 0;
        ready
    }
    pub fn receive(&mut self, message: ServerMessage) {
        match message {
            ServerMessage::Snapshot { state, sequence } => {
                self.receive(ServerMessage::View {
                    state: (*state).into(),
                    sequence,
                });
            }
            ServerMessage::View { state, sequence } => {
                if sequence >= self.snapshot.max(self.sequence) {
                    self.workspace = state.workspace.clone();
                    self.session = Some(state);
                    self.snapshot = sequence;
                }
                self.connected = true;
            }
            ServerMessage::Event { event } => {
                if event.sequence <= self.sequence {
                    return;
                }
                self.sequence = event.sequence;
                // Subscription starts with a current snapshot, then replays old history.
                // History is visible but cannot rewind live status or double-count usage.
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
                        _ => (),
                    }
                }
                match event.data {
                    EventData::Message { actor, text } => {
                        self.transcript.message(&actor, &text);
                        self.activity.clear();
                    }
                    EventData::Created { objective, .. } => {
                        self.input.remember(&objective);
                        self.transcript.user(&objective);
                    }
                    EventData::User { message } => {
                        self.input.remember(&message);
                        self.transcript.user(&message);
                    }
                    EventData::Delta { actor, text } => {
                        self.transcript.delta(&actor, &text);
                    }
                    EventData::ModelRequested { actor, .. } => {
                        self.activity = if actor.starts_with("verify") {
                            "正在独立验收"
                        } else if actor.starts_with("steer") {
                            "正在理解你的补充"
                        } else {
                            "正在思考"
                        }
                        .into();
                    }
                    EventData::ActionStarted {
                        id,
                        tool,
                        arguments,
                    } => self.transcript.start(&id, &tool, &arguments),
                    EventData::ActionFinished {
                        id,
                        success,
                        result,
                    } => {
                        self.transcript.finish(&id, success, &result);
                        self.activity.clear();
                    }
                    EventData::Status { status, reason } => {
                        if matches!(
                            status,
                            Status::Paused
                                | Status::Stopped
                                | Status::Completed
                                | Status::Failed
                                | Status::Exhausted
                        ) {
                            self.transcript.settle();
                        }
                        if status != Status::Running {
                            self.note(format!("{} · {reason}", label(&status)));
                        }
                        self.activity.clear();
                    }
                    EventData::Diagnostic { message } => self.note(format!("系统 › {message}")),
                    EventData::Approval {
                        id,
                        tool,
                        reason,
                        arguments,
                        details,
                    } => {
                        let preview = serde_json::to_string_pretty(&arguments).unwrap_or_default();
                        let shortened = preview.chars().take(10_000).collect::<String>();
                        self.note(format!("需要授权 › {tool}\n{reason}\n完整审阅文件：{details}\n/approve {id}\n请求参数：\n{shortened}"));
                        if shortened.len() < preview.len() {
                            self.note("参数预览已截断，请先查看上面的完整审阅文件。");
                        }
                    }
                    EventData::Revised { revision, changed } => {
                        self.note(format!("需求修订 {revision} › {}", changed.join(", ")))
                    }
                    EventData::Worker { id, status, task } => {
                        let _ = id;
                        self.note(format!("工作者 · {status} · {task}"))
                    }
                    _ => (),
                }
            }
            ServerMessage::Error { message } => self.note(message),
            ServerMessage::Ack => (),
        }
    }
    pub fn budget(&self) -> String {
        match &self.session {
            Some(session) => format!(
                "Token：已用 {} / 上限 {}，未确定 {}，预留 {}。运行时间：{} / {} 秒。\n{}\n/budget <总 token 上限> [总秒数上限]，调整后 /resume。实验总金额上限不会随此命令改变。",
                session.budget.used,
                session.budget.limit,
                session.budget.uncertain,
                session.budget.reservations.values().sum::<u64>(),
                session.elapsed,
                session.seconds,
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
                "{} · {}\n需求版本 {}，{} 项有效需求，{} 个执行步骤。\n{}",
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

use super::Input;
use crate::{
    domain::{EventData, Status},
    runtime::{ipc::ServerMessage, view::SessionView},
};
use std::collections::{BTreeMap, VecDeque};

#[derive(Default)]
pub struct Screen {
    pub input: Input,
    pub session: Option<SessionView>,
    pub scroll: usize,
    pub connected: bool,
    pub(super) logs: VecDeque<String>,
    pub(super) streams: BTreeMap<String, String>,
    sequence: u64,
    snapshot: u64,
}

impl Screen {
    pub fn note(&mut self, text: impl Into<String>) {
        let text = clean(&text.into());
        self.logs.push_back(text.chars().take(20_000).collect());
        while self.logs.len() > 300 || self.logs.iter().map(String::len).sum::<usize>() > 160_000 {
            self.logs.pop_front();
        }
    }
    pub fn receive(&mut self, message: ServerMessage) {
        match message {
            ServerMessage::Snapshot { state, sequence } => {
                self.receive(ServerMessage::View {
                    state: state.into(),
                    sequence,
                });
            }
            ServerMessage::View { state, sequence } => {
                if sequence >= self.snapshot.max(self.sequence) {
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
                if event.sequence > self.snapshot {
                    if let Some(session) = &mut self.session {
                        match &event.data {
                            EventData::Status { status, .. } => session.status = status.clone(),
                            EventData::Usage { input, output, .. } => {
                                session.budget.used = session
                                    .budget
                                    .used
                                    .saturating_add(*input)
                                    .saturating_add(*output)
                            }
                            _ => (),
                        }
                    }
                }
                match event.data {
                    EventData::Message { actor, text } => {
                        self.streams.remove(&actor);
                        self.note(format!("{actor} › {text}"));
                    }
                    EventData::User { message } => self.note(format!("你 › {message}")),
                    EventData::Delta { actor, text } => {
                        let stream = self.streams.entry(actor).or_default();
                        stream.push_str(&clean(&text));
                        if stream.len() > 16_000 {
                            *stream = stream
                                .chars()
                                .rev()
                                .take(4000)
                                .collect::<String>()
                                .chars()
                                .rev()
                                .collect();
                        }
                    }
                    EventData::ActionStarted { tool, .. } => self.note(format!("执行 › {tool}")),
                    EventData::ActionFinished { id, success, .. } => {
                        self.streams.remove(&id);
                        self.note(if success {
                            "检查点 › 工具执行成功"
                        } else {
                            "检查点 › 工具未成功，正在核对原因"
                        });
                    }
                    EventData::Status { status, reason } => {
                        self.note(format!("{} › {reason}", label(&status)))
                    }
                    EventData::Diagnostic { message } => self.note(format!("系统 › {message}")),
                    EventData::Approval { id, reason, .. } => {
                        self.note(format!("需要授权 › {reason}\n确认后输入 /approve {id}"))
                    }
                    EventData::Revised { revision, changed } => {
                        self.note(format!("需求修订 {revision} › {}", changed.join(", ")))
                    }
                    EventData::Worker { id, status, task } => {
                        self.note(format!("{id} · {status} › {task}"))
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
                "Token：已用 {} / 上限 {}，未确定 {}，预留 {}。运行时间：{} / {} 秒。\n/budget <总 token 上限> [总秒数上限]，调整后 /resume。实验总金额上限不会随此命令改变。",
                session.budget.used,
                session.budget.limit,
                session.budget.uncertain,
                session.budget.reservations.values().sum::<u64>(),
                session.elapsed,
                session.seconds
            ),
            None => "正在读取会话预算…".into(),
        }
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
                session.goal.original
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

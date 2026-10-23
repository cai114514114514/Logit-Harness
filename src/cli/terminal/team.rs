//! Focus is a stable member identity; a draft is an immutable assignment intent.
use super::{Input, Screen, ownership::same_view};
use crate::runtime::{
    state::Control,
    team::{MemberControl, Request},
    view::ViewTarget,
};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Drafts {
    pub origin: Option<ViewTarget>,
    saved: BTreeMap<String, (Input, Option<ViewTarget>)>,
}
fn key(view: &ViewTarget) -> String {
    format!(
        "{}:{}",
        view.goal.as_deref().unwrap_or(""),
        view.target.member_id()
    )
}
impl Screen {
    pub(super) fn bind_draft(&mut self) {
        if self.input.text().is_empty() {
            self.drafts.origin = None;
        } else if self.drafts.origin.is_none() {
            self.drafts.origin = Some(self.view.clone());
        }
    }
    pub(super) fn draft_view(&self) -> ViewTarget {
        self.drafts
            .origin
            .clone()
            .unwrap_or_else(|| self.view.clone())
    }
    pub(super) fn stale_draft(&self) -> bool {
        self.drafts.origin.as_ref().is_some_and(|origin| {
            origin.target != self.view.target || origin.goal != self.view.goal
        })
    }
    pub(super) fn focus(&mut self, view: ViewTarget) {
        self.bind_draft();
        if !same_view(&self.view, &view) {
            self.drafts.saved.insert(
                key(&self.view),
                (std::mem::take(&mut self.input), self.drafts.origin.take()),
            );
            let (input, origin) = self.drafts.saved.remove(&key(&view)).unwrap_or_default();
            self.input = input;
            self.drafts.origin = origin;
            self.skill_choices = None;
            self.history = None;
        }
        self.view = view;
        self.member = self
            .members
            .iter()
            .find(|member| member.id == self.view.target.member_id())
            .cloned();
        self.transcript.switch(self.view.clone());
        self.team_picker = false;
        self.selection = 0;
        self.menu_hidden = true;
    }
    pub(super) fn note_origin(&mut self, origin: &ViewTarget, text: impl Into<String>) {
        self.transcript.note_in(origin, &text.into());
    }
    pub(super) fn restore_origin(&mut self, origin: &ViewTarget, draft: String, revision: u64) {
        if draft.is_empty() {
            return;
        }
        let (input, binding) = if same_view(&self.view, origin) {
            (&mut self.input, &mut self.drafts.origin)
        } else {
            let saved = self.drafts.saved.entry(key(origin)).or_default();
            (&mut saved.0, &mut saved.1)
        };
        if input.text().is_empty() && input.revision() == revision {
            input.replace(&draft);
            *binding = Some(origin.clone());
        } else {
            self.note_origin(origin, format!("未确认送达的输入（保留）：\n{draft}"));
        }
    }
    pub(super) fn pause(&self) -> Control {
        if self.view.is_main() {
            Control::Pause
        } else {
            Control::Directed {
                request: Request {
                    id: uuid::Uuid::new_v4().to_string(),
                    client_id: self.client_id.clone(),
                    target: self.view.target.clone(),
                    action: MemberControl::Pause,
                },
            }
        }
    }
    pub(super) fn team_labels(&self) -> Vec<String> {
        std::iter::once("main · 总览与全局控制".into())
            .chain(self.members.iter().map(|m| {
                format!(
                    "{} [{}] · {} · {} · {} · {}",
                    m.name,
                    identity(&m.id),
                    member_status(m),
                    self.member_usage(&m.id),
                    super::activity::progress(self, &m.id),
                    m.task.as_deref().unwrap_or("无任务")
                )
            }))
            .collect()
    }
    fn member_usage(&self, member: &str) -> String {
        let Some(usage) = self.transcript.usage.get(member) else {
            return "用量未报告".into();
        };
        let hit = usage
            .cache_hit()
            .map_or_else(|| "—".into(), |hit| format!("{:.0}%", hit * 100.0));
        format!("{}t · KV {hit}", usage.tokens())
    }
    pub(super) fn selected_member(&self) -> Option<String> {
        if self.selection == 0 {
            Some("main".into())
        } else {
            self.members.get(self.selection - 1).map(|m| m.id.clone())
        }
    }
}
pub(super) fn identity(id: &str) -> String {
    id.chars()
        .take(if id.starts_with("member-") || id.starts_with("worker-") {
            15
        } else {
            8
        })
        .collect()
}
pub(super) fn assignment(target: &crate::runtime::team::Target) -> String {
    if let crate::runtime::team::Target::Member {
        assignment: Some(reference),
        ..
    } = target
    {
        format!("任务 {} v{}", identity(&reference.id), reference.version)
    } else {
        super::ownership::assignment(target)
    }
}
pub(super) fn member_status(member: &crate::runtime::team::MemberView) -> String {
    if member.stopping {
        "正在停止".into()
    } else if member.paused {
        "已暂停".into()
    } else if member.interrupted {
        "正在处理补充".into()
    } else if let Some(wait) = &member.waiting {
        format!("等待：{wait}")
    } else if matches!(
        member.target,
        crate::runtime::team::Target::Member {
            assignment: Some(_),
            ..
        }
    ) {
        "已分配".into()
    } else {
        "空闲".into()
    }
}
pub(super) fn open(screen: &mut Screen, json: &str) {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(json) {
        if let Ok(team) =
            serde_json::from_value::<crate::runtime::team::TeamView>(value["team"].clone())
        {
            screen.members = team.members;
        }
        if let Some(archives) = value["archives"].as_array() {
            for archive in archives {
                if let Ok(team) = serde_json::from_value::<crate::runtime::team::TeamView>(
                    archive["team"].clone(),
                ) {
                    for mut member in team.members {
                        member.name = format!("{}（归档只读）", member.name);
                        screen.members.push(member);
                    }
                }
            }
        }
    }
    screen.team_picker = true;
    screen.selection = 0;
}

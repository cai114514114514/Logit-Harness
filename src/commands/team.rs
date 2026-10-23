//! Team commands resolve identities once, before asynchronous client work.
use super::{Kind, Outcome, Service, parse::split};
use crate::runtime::{
    state::Control,
    team::{MemberControl, Request},
    view::ViewTarget,
};

impl Service<'_> {
    pub(super) fn team_command(&self, kind: Kind, arguments: &str) -> anyhow::Result<Outcome> {
        let state = self.load()?;
        match kind {
            Kind::Agent => {
                let (selector, flags) = split(arguments.trim());
                anyhow::ensure!(
                    !selector.is_empty() && (flags.is_empty() || flags == "--window"),
                    "用法：/agent NAME [--window]"
                );
                let target = state.resolve_view(selector)?;
                Ok(Outcome::Focus {
                    origin: self.origin.clone(),
                    target,
                    window: flags == "--window",
                })
            }
            Kind::Tell => {
                let (selector, text) = split(arguments);
                anyhow::ensure!(!text.trim().is_empty(), "用法：/tell NAME TEXT");
                let target = state.resolve_view(selector)?;
                anyhow::ensure!(!target.readonly, "归档成员只读；向 main 发送新请求");
                Ok(self.addressed(target, MemberControl::Message { text: text.into() }))
            }
            Kind::Team => match arguments.trim() {
                "" => super::service::pretty(
                    "团队",
                    serde_json::json!({
                        "origin": self.origin, "team": state.team.view(),
                        "archives": state.archives.iter().map(|a| serde_json::json!({"goal":a.goal.id,"team":a.team.view(),"readonly":true})).collect::<Vec<_>>()
                    }),
                ),
                "pause" => Ok(Outcome::Control(Control::Pause)),
                "resume" => Ok(Outcome::Control(Control::Resume)),
                "stop" => Ok(Outcome::Control(Control::Stop)),
                _ => {
                    let (operation, limit) = split(arguments.trim());
                    anyhow::ensure!(
                        operation == "limit" && !limit.is_empty(),
                        "用法：/team [pause|resume|stop|limit N]"
                    );
                    Ok(self.addressed(
                        ViewTarget::main(),
                        MemberControl::Limit {
                            limit: limit.parse()?,
                        },
                    ))
                }
            },
            _ => unreachable!("team catalog routing"),
        }
    }
    pub(super) fn addressed(&self, origin: ViewTarget, action: MemberControl) -> Outcome {
        Outcome::Control(Control::Directed {
            request: Request {
                id: uuid::Uuid::new_v4().to_string(),
                client_id: self.client_id.clone(),
                target: origin.target,
                action,
            },
        })
    }

    pub(super) fn member_control(&self, control: Control) -> anyhow::Result<Outcome> {
        let action = match control {
            Control::Pause => MemberControl::Pause,
            Control::Resume => MemberControl::Resume,
            Control::Stop => MemberControl::Stop,
            Control::Compact => MemberControl::Compact,
            Control::Message { text } => MemberControl::Message { text },
            Control::Side { text } => MemberControl::Side { text },
            _ => anyhow::bail!("此操作需要 main 视图"),
        };
        self.writable_view()?;
        Ok(self.addressed(self.origin.clone(), action))
    }
    pub(super) fn writable_view(&self) -> anyhow::Result<()> {
        let view = self.load()?.target_view(&self.origin)?;
        anyhow::ensure!(!view.origin.readonly, "归档成员只读；使用 /agent main");
        // Keep stale drafts intact for the runner's durable failed receipt. Do
        // not silently refresh an expected assignment during local discovery.
        Ok(())
    }
}

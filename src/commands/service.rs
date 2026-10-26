use super::{Command, Kind, inspect, parse::split};
use crate::{
    config::{Config, Paths},
    extensions::{
        mcp::Operation,
        skills::{Invocation, Skills},
    },
    runtime::{
        state::{Control, Session},
        team::MemberControl,
        view::ViewTarget,
    },
    store::Store,
};
use serde_json::{Value, json};
use std::os::unix::fs::OpenOptionsExt;
use std::{io::Write, path::Path};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Navigation {
    New,
    Sessions,
    Detach,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct History {
    pub origin: ViewTarget,
    pub query: String,
    pub tools_only: bool,
}

#[derive(Debug)]
pub enum Outcome {
    Focus {
        origin: ViewTarget,
        target: ViewTarget,
        window: bool,
    },
    ScopedSkill {
        origin: ViewTarget,
        invocation: Box<Invocation>,
        request: Box<crate::runtime::team::Request>,
    },
    ScopedChoice {
        origin: ViewTarget,
        entries: Vec<crate::extensions::skills::Entry>,
        arguments: String,
    },
    Local {
        title: String,
        text: String,
    },
    Control(Control),
    Skill(Invocation),
    /// Duplicates are never picked silently. The client can render these entries
    /// and call /skills run <ID> with the original literal arguments.
    ChooseSkill {
        entries: Vec<crate::extensions::skills::Entry>,
        arguments: String,
    },
    Navigate(Navigation),
    History(History),
    /// Open the contract detail from the client's current session view.
    Contract,
    Copy {
        origin: ViewTarget,
        selector: String,
    },
    Setup,
}

#[derive(Clone)]
pub struct Service<'a> {
    pub(super) paths: &'a Paths,
    workspace: &'a Path,
    session: Option<&'a str>,
    pub(super) origin: ViewTarget,
    pub(super) client_id: String,
}

impl<'a> Service<'a> {
    pub fn new(paths: &'a Paths, workspace: &'a Path, session: Option<&'a str>) -> Self {
        Self {
            paths,
            workspace,
            session,
            origin: ViewTarget::main(),
            client_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    pub fn in_view(mut self, origin: ViewTarget) -> Self {
        self.origin = origin;
        self
    }
    pub fn with_client(mut self, id: impl Into<String>) -> Self {
        self.client_id = id.into();
        self
    }
    pub fn execute(&self, command: Command) -> anyhow::Result<Outcome> {
        if self.session.is_some() {
            let session = self.load()?;
            let mut bound = self.clone();
            bound.workspace = &session.workspace;
            return bound.execute_bound(command);
        }
        self.execute_bound(command)
    }
    fn execute_bound(&self, command: Command) -> anyhow::Result<Outcome> {
        let (kind, arguments) = match command {
            Command::Message(text) => {
                return if self.origin.is_main() {
                    Ok(Outcome::Control(Control::Message { text }))
                } else {
                    self.member_control(Control::Message { text })
                };
            }
            Command::Skill {
                selector,
                arguments,
            } => return self.skill(&selector, &arguments),
            Command::Builtin { kind, arguments } => (kind, arguments),
        };
        if matches!(kind, Kind::Team | Kind::Agent | Kind::Tell) {
            return self.team_command(kind, &arguments);
        }
        if !self.origin.is_main() {
            let mutation = match kind {
                Kind::Goal | Kind::Budget => !arguments.is_empty(),
                Kind::Config => !split(&arguments).1.is_empty(),
                Kind::Mcp => matches!(split(&arguments).0, "enable" | "disable" | "reconnect"),
                Kind::Setup | Kind::Plan | Kind::Approve | Kind::Answer | Kind::Rename => true,
                _ => false,
            };
            anyhow::ensure!(!mutation, "此操作需要 main 视图");
        }
        let control = match kind {
            Kind::Pause => Some(Control::Pause),
            Kind::Resume => Some(Control::Resume),
            Kind::Stop => Some(Control::Stop),
            Kind::Compact => Some(Control::Compact),
            Kind::Plan => Some(Control::Plan {
                text: (!arguments.is_empty()).then_some(arguments.clone()),
            }),
            Kind::Goal if !arguments.is_empty() => Some(Control::Goal {
                text: arguments.clone(),
            }),
            Kind::Btw => Some(Control::Side {
                text: arguments.clone(),
            }),
            Kind::Rename => Some(Control::Rename {
                title: arguments.clone(),
            }),
            Kind::Approve => Some(Control::Approve {
                id: arguments.clone(),
            }),
            Kind::Answer => {
                let (id, text) = split(&arguments);
                Some(Control::Answer {
                    id: id.into(),
                    text: text.into(),
                })
            }
            Kind::Budget if !arguments.is_empty() => {
                let (tokens, seconds) = split(&arguments);
                let tokens = crate::domain::limit::parse(tokens)?;
                let seconds = if seconds.is_empty() {
                    (tokens == crate::domain::limit::UNLIMITED).then_some(tokens)
                } else {
                    Some(crate::domain::limit::parse(seconds)?)
                };
                Some(Control::Budget {
                    tokens: Some(tokens),
                    seconds,
                })
            }
            _ => None,
        };
        if let Some(control) = control {
            return if self.origin.is_main() {
                Ok(Outcome::Control(control))
            } else {
                self.member_control(control)
            };
        }
        match kind {
            Kind::Setup => Ok(Outcome::Setup),
            Kind::New => Ok(Outcome::Navigate(Navigation::New)),
            Kind::Sessions => Ok(Outcome::Navigate(Navigation::Sessions)),
            Kind::Detach => Ok(Outcome::Navigate(Navigation::Detach)),
            Kind::Help => Ok(local("帮助", super::help())),
            Kind::Tools => pretty(
                "工具能力",
                super::capabilities::inspect(
                    self.paths,
                    self.workspace,
                    Config::load(&self.paths.home)?,
                    self.optional_session()?.as_ref(),
                    arguments.trim(),
                )?,
            ),
            Kind::Contract => Ok(Outcome::Contract),
            Kind::History => {
                self.id()?;
                let (head, tail) = split(&arguments);
                let tools_only = head == "tools";
                Ok(Outcome::History(History {
                    origin: self.origin.clone(),
                    query: if tools_only { tail.into() } else { arguments },
                    tools_only,
                }))
            }
            Kind::Copy => {
                self.id()?;
                Ok(Outcome::Copy {
                    origin: self.origin.clone(),
                    selector: arguments,
                })
            }
            Kind::Doctor => pretty(
                "本地诊断",
                inspect::doctor(self.paths, &Config::load(&self.paths.home)?)?,
            ),
            Kind::Config => self.config(&arguments),
            Kind::Permissions => pretty(
                "权限",
                inspect::permissions(self.optional_session()?.as_ref()),
            ),
            Kind::Skills if !arguments.is_empty() => {
                let (_, tail) = split(&arguments);
                let (selector, args) = split(tail);
                self.skill(selector, args)
            }
            Kind::Skills => {
                let catalog = self.skills()?;
                pretty(
                    "技能目录",
                    json!({"skills":catalog.entries.iter().map(|e|json!({"id":e.id,"name":e.name,"source":e.source,"path":e.path,"description":e.description,"core_conflict":super::DIRECTORY.iter().any(|c|c.name.strip_prefix('/') == Some(e.name.as_str()))})).collect::<Vec<_>>(),"warnings":catalog.warnings,"invoke":"/skills run <ID> [literal arguments]"}),
                )
            }
            Kind::Mcp => self.mcp(&arguments),
            Kind::Diff => pretty("改动", crate::extensions::skills::inspect(self.workspace)?),
            Kind::Export => self.export(&arguments),
            Kind::Status => {
                let state = crate::runtime::view::SessionView::from(self.load()?);
                let mut value = serde_json::to_value(&state)?;
                value["budget"]["limit"] = crate::domain::limit::value(state.budget.limit);
                value["seconds"] = crate::domain::limit::value(state.seconds);
                pretty("状态", value)
            }
            Kind::Budget => {
                let s = self.load()?;
                pretty("当前预算", {
                    let mut budget = serde_json::to_value(&s.budget)?;
                    budget["limit"] = crate::domain::limit::value(s.budget.limit);
                    json!({"budget":budget,"seconds":crate::domain::limit::value(s.seconds),"elapsed":s.elapsed,"campaign":s.campaign})
                })
            }
            Kind::Usage => pretty("已报告用量", inspect::usage(&self.load()?)),
            Kind::Context => pretty("上下文", inspect::context_in(&self.load()?, &self.origin)?),
            Kind::Goal => {
                let s = self.load()?;
                pretty(
                    "目标",
                    json!({"mode":s.mode,"active":s.active_goal(),"suspended":(s.mode == crate::runtime::conversation::Mode::Plan).then_some(&s.goal),"queue":s.queue,"archived_goals":s.archives.iter().map(|a|&a.goal).collect::<Vec<_>>()}),
                )
            }
            Kind::Tasks => {
                let mut value = inspect::tasks(&self.load()?, &arguments);
                value["services"] = self.services(&arguments)?;
                pretty("活动与待办", value)
            }
            _ => anyhow::bail!("命令分发缺少处理器"),
        }
    }

    fn id(&self) -> anyhow::Result<&str> {
        self.session.ok_or_else(|| anyhow::anyhow!("请先打开会话"))
    }
    pub(super) fn load(&self) -> anyhow::Result<Session> {
        let id = self.id()?;
        crate::config::validate_id(id)?;
        anyhow::ensure!(self.paths.database().exists(), "会话数据库不存在");
        let mut session: Session = Store::open(&self.paths.database())?.load(id)?;
        session.normalize_team();
        Ok(session)
    }
    fn optional_session(&self) -> anyhow::Result<Option<Session>> {
        self.session.map(|_| self.load()).transpose()
    }
    pub(super) fn skills(&self) -> anyhow::Result<Skills> {
        let mut config = Config::load(&self.paths.home)?;
        if let Some(session) = self.optional_session()?
            && let Some(defaults) = session.defaults
        {
            defaults.apply(&mut config);
        }
        let mut roots = vec![];
        if config.user_skills {
            roots.push(self.paths.home.join("skills"));
            if let Some(user) = directories::BaseDirs::new() {
                roots.push(user.home_dir().join(".agents/skills"));
            }
        }
        let workspace = self.optional_session()?.map(|s| s.workspace);
        Ok(
            Skills::discover(workspace.as_deref().unwrap_or(self.workspace), &roots)?
                .with_bundled(),
        )
    }
    fn skill(&self, selector: &str, arguments: &str) -> anyhow::Result<Outcome> {
        let skills = self.skills()?;
        let entries = skills.candidates(selector);
        if !self.origin.is_main() {
            self.load()?.target_view(&self.origin)?;
        }
        if entries.len() > 1 && !self.origin.is_main() {
            return Ok(Outcome::ScopedChoice {
                origin: self.origin.clone(),
                entries: entries.into_iter().cloned().collect(),
                arguments: arguments.into(),
            });
        }
        if entries.len() > 1 {
            return Ok(Outcome::ChooseSkill {
                entries: entries.into_iter().cloned().collect(),
                arguments: arguments.into(),
            });
        }
        let invocation = skills.invoke(selector, arguments)?;
        if self.origin.is_main() {
            return Ok(Outcome::Skill(invocation));
        }
        let Outcome::Control(Control::Directed { request }) = self.addressed(
            self.origin.clone(),
            MemberControl::Skill {
                id: invocation.id.clone(),
                hash: invocation.hash.clone(),
                arguments: invocation.arguments.clone(),
            },
        ) else {
            unreachable!()
        };
        Ok(Outcome::ScopedSkill {
            origin: self.origin.clone(),
            invocation: Box::new(invocation),
            request: Box::new(request),
        })
    }
    fn config(&self, arguments: &str) -> anyhow::Result<Outcome> {
        let (key, value) = split(arguments);
        if !value.is_empty() {
            return pretty(
                "默认值已保存（/new 新会话生效）",
                crate::config::defaults::save(self.paths, key, value)?,
            );
        }
        let values = crate::config::defaults::values(&Config::load(&self.paths.home)?);
        pretty(
            "默认值（当前预算用 /budget，密钥用 /setup）",
            if key.is_empty() {
                values
            } else {
                json!({key:values[key]})
            },
        )
    }
    fn mcp(&self, arguments: &str) -> anyhow::Result<Outcome> {
        let config = Config::load(&self.paths.home)?;
        let (operation, server) = split(arguments);
        if !server.is_empty() {
            anyhow::ensure!(config.mcp.contains_key(server), "未知 MCP 服务：{server}");
        }
        if matches!(operation, "enable" | "disable" | "reconnect") {
            let operation = match operation {
                "enable" => Operation::Enable,
                "disable" => Operation::Disable,
                _ => Operation::Reconnect,
            };
            return Ok(Outcome::Control(Control::Mcp {
                server: server.into(),
                operation,
            }));
        }
        let state = self.optional_session()?;
        let servers=config.mcp.iter().filter(|(name,_)|server.is_empty() || name.as_str()==server).map(|(name,config)| {
            json!({"name":name,"transport":match config.transport { crate::extensions::mcp::Transport::Stdio {..}=>"stdio",crate::extensions::mcp::Transport::Http {..}=>"http" },"enabled":state.as_ref().is_none_or(|s|!s.mcp_disabled.contains(name)),"effects":config.effects})
        }).collect::<Vec<_>>();
        let pending = if let Some(id) = self.session {
            let logs = self.paths.logs(id)?.join("mcp");
            config
                .mcp
                .keys()
                .filter(|name| server.is_empty() || name.as_str() == server)
                .map(|name| {
                    let path = logs
                        .join(blake3::hash(name.as_bytes()).to_hex().as_str())
                        .join("pending.json");
                    json!({"server":name,"uncertain_mutation":path.exists(),"record":path})
                })
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        pretty(
            "MCP（仅配置服务，未连接探测）",
            json!({"servers":servers,"pending":pending,"changes":state.as_ref().map(|s|&s.mcp_pending)}),
        )
    }
    fn export(&self, destination: &str) -> anyhow::Result<Outcome> {
        let id = self.id()?;
        let store = Store::open(&self.paths.database())?;
        let (state, sequence) = store.snapshot::<Session>(id)?;
        let events = store
            .events(id, 0)?
            .into_iter()
            .filter(|e| e.sequence <= sequence)
            .collect::<Vec<_>>();
        let path = self.workspace.join(destination);
        // create_new rejects existing files and symlinks; never truncate a user's export.
        let data = if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        {
            serde_json::to_vec_pretty(
                &json!({"state":state,"through_sequence":sequence,"events":events}),
            )?
        } else {
            super::export::markdown(&state, sequence, &events).into_bytes()
        };
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(&data)?;
        file.sync_all()?;
        Ok(local("导出完成", path.display().to_string()))
    }
    fn services(&self, filter: &str) -> anyhow::Result<Value> {
        let folder = self.paths.logs(self.id()?)?.join("services");
        let mut values = vec![];
        if folder.is_dir() {
            for entry in std::fs::read_dir(folder)? {
                let path = entry?.path();
                if path.extension().is_some_and(|v| v == "json") {
                    let record: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
                    if record["info"].is_object()
                        && (filter.is_empty()
                            || record["info"]["id"]
                                .as_str()
                                .is_some_and(|id| id.contains(filter)))
                    {
                        values.push(record["info"].clone());
                    }
                }
            }
        }
        values.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        Ok(
            json!({"records":values,"note":"saved service records; live state is owned by the session daemon"}),
        )
    }
}
fn local(title: &str, text: String) -> Outcome {
    Outcome::Local {
        title: title.into(),
        text,
    }
}
pub(super) fn pretty(title: &str, value: Value) -> anyhow::Result<Outcome> {
    Ok(local(title, serde_json::to_string_pretty(&value)?))
}

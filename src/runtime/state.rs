use super::actor::{Action, Actor, Role};
use super::graph::Graph;
use super::money::Prices;
use crate::domain::{Budget, Evidence, Goal, Status};
use crate::tools::Scope;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Campaign {
    pub name: String,
    pub limit: u64,
    pub prices: Prices,
}
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserMessage {
    pub id: String,
    pub text: String,
    pub applied: bool,
    pub prior: Status,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub workspace: PathBuf,
    pub goal: Goal,
    pub status: Status,
    /// Durable destination after every cancelled operation has actually settled.
    pub transition: Option<Status>,
    pub graph: Graph,
    pub actors: BTreeMap<String, Actor>,
    pub actions: BTreeMap<String, Action>,
    pub questions: BTreeMap<String, String>,
    pub budget: Budget,
    pub evidence: Vec<Evidence>,
    pub inbox: Vec<UserMessage>,
    pub approvals: BTreeSet<String>,
    pub created: String,
    pub seconds: u64,
    pub elapsed: u64,
    pub final_report: Option<String>,
    pub cursor: u64,
    pub campaign: Option<Campaign>,
}

impl Session {
    pub fn new(workspace: PathBuf, objective: &str, tokens: u64, seconds: u64) -> Self {
        let goal = Goal::new(objective);
        let main = Actor::new(
            "main".into(),
            Role::Main,
            objective.into(),
            Scope {
                requirements: goal.active_versions(),
                ..Scope::default()
            },
        );
        Self {
            id: uuid::Uuid::new_v4().simple().to_string()[..16].into(),
            workspace,
            goal,
            status: Status::Ready,
            transition: None,
            graph: Graph::default(),
            actors: BTreeMap::from([("main".into(), main)]),
            actions: BTreeMap::new(),
            questions: BTreeMap::new(),
            budget: Budget::new(tokens),
            evidence: vec![],
            inbox: vec![],
            approvals: BTreeSet::new(),
            created: chrono::Utc::now().to_rfc3339(),
            seconds,
            elapsed: 0,
            final_report: None,
            cursor: 0,
            campaign: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Control {
    #[serde(skip)]
    Wake,
    Message {
        text: String,
    },
    Pause,
    Resume,
    Stop,
    Budget {
        tokens: Option<u64>,
        seconds: Option<u64>,
    },
    Approve {
        id: String,
    },
}

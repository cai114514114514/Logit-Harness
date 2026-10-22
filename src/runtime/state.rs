use super::actor::{Action, Actor, Role};
use super::graph::Graph;
use super::money::Prices;
use crate::domain::{Budget, Evidence, Goal, Status};
use crate::tools::Scope;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

/// Session ids are truncated UUID hex strings; this is the only place the
/// length is chosen. Socket names and display formats build on it.
const ID_LENGTH: usize = 16;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Campaign {
    pub name: String,
    pub limit: u64,
    pub prices: Prices,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserMessage {
    pub id: String,
    pub text: String,
    pub applied: bool,
    /// Session status when the message arrived; answering a side question
    /// restores it instead of blindly resuming.
    pub prior: Status,
    #[serde(default)]
    pub revision: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Question {
    pub text: String,
    pub requirements: BTreeMap<String, u64>,
    pub answer: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    /// Missing fields in historical sessions retain explicit engineering behavior.
    #[serde(default)]
    pub mode: super::conversation::Mode,
    #[serde(default)]
    pub interactive: bool,
    #[serde(default)]
    pub archives: Vec<super::conversation::Archive>,
    #[serde(default)]
    pub queue: Vec<super::conversation::Task>,
    #[serde(default)]
    pub boundary: Option<super::conversation::Boundary>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub owner_status: Option<Status>,
    #[serde(default)]
    pub compact_pending: bool,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub plan_input: Option<String>,
    #[serde(default)]
    pub plan_turns: Vec<String>,
    #[serde(default)]
    pub implementation: Option<super::conversation::Task>,
    #[serde(default)]
    pub suspended: Option<Actor>,
    #[serde(default)]
    pub turns: Vec<super::conversation::Turn>,
    #[serde(default)]
    pub evaluation: Option<crate::eval::trial::Spec>,
    #[serde(default)]
    pub condition: super::condition::Condition,
    #[serde(default)]
    pub prompt_pending: bool,
    #[serde(default)]
    pub replan_pending: bool,
    pub id: String,
    pub workspace: PathBuf,
    pub goal: Goal,
    /// Current durable status. While `transition` is set this holds the
    /// intermediate Stopping/Pausing state; the target lands only after every
    /// in-flight job has settled.
    pub status: Status,
    /// Durable destination after every cancelled operation has actually settled.
    pub transition: Option<Status>,
    pub graph: Graph,
    pub actors: BTreeMap<String, Actor>,
    pub actions: BTreeMap<String, Action>,
    #[serde(default)]
    pub settled_actions: BTreeMap<String, Action>,
    pub questions: BTreeMap<String, Question>,
    pub budget: Budget,
    pub evidence: Vec<Evidence>,
    pub inbox: Vec<UserMessage>,
    pub approvals: BTreeSet<String>,
    pub created: String,
    pub seconds: u64,
    /// Whole seconds in Running, measured with a monotonic clock. Pauses and
    /// waiting for external input are excluded. Not a cost measure.
    pub elapsed: u64,
    pub final_report: Option<String>,
    #[serde(default)]
    pub verification_pending: bool,
    /// Id of the last control row consumed from the store's controls queue;
    /// replay starts strictly after it. Advancing it before handling the
    /// control is deliberate (see drain_inbox).
    pub cursor: u64,
    pub campaign: Option<Campaign>,
}

impl Session {
    /// tokens/seconds of 0 are accepted without validation: the CLI's Config
    /// rejects zero execution limits before this is constructed. A zero-token
    /// session created directly would exhaust on the first model request, and
    /// zero seconds halts at the first tick — degrading to Exhausted is the
    /// current, intentional behaviour.
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
        let turns = vec![super::conversation::Turn {
            id: main.turn.clone(),
            text: objective.into(),
        }];
        Self {
            mode: Default::default(),
            interactive: false,
            archives: vec![],
            queue: vec![],
            boundary: None,
            owner: None,
            owner_status: None,
            compact_pending: false,
            title: objective.into(),
            plan_input: None,
            plan_turns: vec![],
            implementation: None,
            suspended: None,
            turns,
            evaluation: None,
            condition: Default::default(),
            prompt_pending: false,
            replan_pending: false,
            // `simple()` is ASCII hex, so slicing at ID_LENGTH never splits a
            // multi-byte character.
            id: uuid::Uuid::new_v4().simple().to_string()[..ID_LENGTH].into(),
            workspace,
            goal,
            status: Status::Ready,
            transition: None,
            graph: Graph::default(),
            actors: BTreeMap::from([("main".into(), main)]),
            actions: BTreeMap::new(),
            settled_actions: BTreeMap::new(),
            questions: BTreeMap::new(),
            budget: Budget::new(tokens),
            evidence: vec![],
            inbox: vec![],
            approvals: BTreeSet::new(),
            created: chrono::Utc::now().to_rfc3339(),
            seconds,
            elapsed: 0,
            final_report: None,
            verification_pending: false,
            cursor: 0,
            campaign: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Control {
    Plan {
        text: Option<String>,
    },
    Goal {
        text: String,
    },
    Side {
        text: String,
    },
    Compact,
    #[serde(skip)]
    Wake,
    Message {
        text: String,
    },
    Answer {
        id: String,
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

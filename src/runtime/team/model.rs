use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Team {
    pub limit: usize,
    pub members: BTreeMap<String, Member>,
    pub assignments: BTreeMap<String, Assignment>,
    pub mail: Vec<Mail>,
    pub receipts: BTreeMap<String, Receipt>,
    pub wait_notice: String,
    #[serde(default)]
    pub corrections: Vec<Correction>,
}
impl Default for Team {
    fn default() -> Self {
        Self {
            limit: 2,
            members: BTreeMap::new(),
            assignments: BTreeMap::new(),
            mail: vec![],
            receipts: BTreeMap::new(),
            wait_notice: String::new(),
            corrections: vec![],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Member {
    pub id: String,
    pub name: String,
    pub assignment: Option<String>,
    pub actor: Option<String>,
    pub attempts: Vec<String>,
    pub paused: bool,
    /// Temporary interruption by unknown input is independent of explicit Pause.
    #[serde(default)]
    pub interrupted: bool,
    pub stopping: bool,
    pub waiting: Option<String>,
    #[serde(default)]
    pub mail_seen: usize,
    pub compact_pending: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Assignment {
    pub id: String,
    pub version: u64,
    pub member: String,
    pub actor: String,
    pub step: String,
    pub task: String,
    pub writable: BTreeSet<String>,
    pub status: AssignmentStatus,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentStatus {
    Active,
    Finished,
    Stopped,
    Invalidated,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Correction {
    #[serde(default)]
    pub target: Option<Target>,
    pub source: String,
    pub steps: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignmentRef {
    pub id: String,
    pub version: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Target {
    Main,
    Member {
        id: String,
        assignment: Option<AssignmentRef>,
    },
}
impl Target {
    pub fn member_id(&self) -> &str {
        match self {
            Self::Main => "main",
            Self::Member { id, .. } => id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    pub client_id: String,
    pub target: Target,
    pub action: MemberControl,
}

/// Deliberately excludes permissions, budget, goal and execution-mode changes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MemberControl {
    Pause,
    Resume,
    Stop,
    Compact,
    Message { text: String },
    Side { text: String },
    Limit { limit: usize },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub request: Request,
    pub status: ReceiptStatus,
    pub error: Option<String>,
    #[serde(default)]
    pub pending_input: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptStatus {
    Pending,
    Applied,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mail {
    pub id: String,
    pub from: String,
    pub target: Target,
    pub text: String,
    pub user: bool,
    pub delivered: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TeamView {
    pub limit: usize,
    pub members: Vec<MemberView>,
    pub assignments: Vec<Assignment>,
    pub mail: Vec<Mail>,
    pub receipts: Vec<Receipt>,
    #[serde(default)]
    pub targets: BTreeMap<String, Target>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemberView {
    pub id: String,
    pub name: String,
    pub target: Target,
    pub actor: Option<String>,
    pub attempts: Vec<String>,
    pub task: Option<String>,
    pub paused: bool,
    #[serde(default)]
    pub interrupted: bool,
    pub stopping: bool,
    pub waiting: Option<String>,
}
impl Team {
    pub fn engineering_settled(&self) -> bool {
        !self
            .assignments
            .values()
            .any(|a| a.status == AssignmentStatus::Active)
            && !self.members.values().any(|m| m.stopping || m.interrupted)
            && !self.mail.iter().any(|m| !m.delivered && m.error.is_none())
            && !self
                .receipts
                .values()
                .any(|r| r.status == ReceiptStatus::Pending)
    }
    pub fn target(&self, name: &str) -> anyhow::Result<Target> {
        if name == "main" {
            return Ok(Target::Main);
        }
        let member = self
            .members
            .values()
            .find(|m| m.id == name || m.name == name)
            .ok_or_else(|| anyhow::anyhow!("unknown member: {name}"))?;
        Ok(Target::Member {
            id: member.id.clone(),
            assignment: member.assignment.as_ref().map(|id| {
                let assignment = &self.assignments[id];
                AssignmentRef {
                    id: id.clone(),
                    version: assignment.version,
                }
            }),
        })
    }
    pub fn validate(&self, target: &Target) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.target(target.member_id())? == *target,
            "stale member assignment; input preserved, refresh target before retrying"
        );
        Ok(())
    }
    pub fn member_for_actor(&self, actor: &str) -> Option<&Member> {
        self.members
            .values()
            .find(|m| m.attempts.iter().any(|id| id == actor))
    }
    pub fn view(&self) -> TeamView {
        TeamView {
            limit: self.limit,
            members: self
                .members
                .values()
                .map(|m| MemberView {
                    id: m.id.clone(),
                    name: m.name.clone(),
                    target: self.target(&m.id).expect("member exists"),
                    actor: m.actor.clone(),
                    attempts: m.attempts.clone(),
                    task: m
                        .assignment
                        .as_ref()
                        .map(|id| self.assignments[id].task.clone()),
                    paused: m.paused,
                    interrupted: m.interrupted,
                    stopping: m.stopping,
                    waiting: m.waiting.clone(),
                })
                .collect(),
            assignments: self.assignments.values().cloned().collect(),
            mail: self.mail.clone(),
            receipts: self.receipts.values().cloned().collect(),
            targets: BTreeMap::new(),
        }
    }
}

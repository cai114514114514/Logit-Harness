//! Durable records behind the terminal's contract ledger.
//!
//! These are presentation facts derived from runtime decisions that already
//! happened: a revision that was applied, jobs that were cancelled, evidence that
//! stopped being current, an acceptance verdict. Nothing here feeds back into
//! scheduling or the completion gate.
use super::EvidenceKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One clause as it stood when the contract event was written.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ClauseRecord {
    pub id: String,
    pub text: String,
    pub revision: u64,
    pub active: bool,
    /// The user's words this clause came from.
    pub source: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ContractChanges {
    #[serde(default)]
    pub updated: Vec<ClauseUpdate>,
    #[serde(default)]
    pub added: Vec<String>,
    #[serde(default)]
    pub retired: Vec<String>,
    #[serde(default)]
    pub cancelled: Vec<CancelledAction>,
    #[serde(default)]
    pub stale: Vec<StaleEvidence>,
    #[serde(default)]
    pub kept: Vec<KeptStep>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ClauseUpdate {
    pub id: String,
    pub before: String,
    pub after: String,
}

/// A running job the revision cancelled. `settled` is whether its action had
/// already left the running set when the record was written; cancellation is
/// asynchronous, so this is usually false and the card must say so.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CancelledAction {
    pub id: String,
    pub tool: String,
    pub subject: String,
    pub requirements: Vec<String>,
    pub settled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StaleEvidence {
    pub id: String,
    pub tool: String,
    pub subject: String,
    pub reason: StaleReason,
}

/// Why evidence that was current before a revision is no longer current.
/// The first matching cause wins, in this order.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StaleReason {
    /// A clause it covers was revised or retired.
    Clause,
    /// Added or replaced clauses advanced the goal epoch.
    Epoch,
    /// A step it depends on was invalidated.
    Step,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct KeptStep {
    pub id: String,
    pub description: String,
}

/// Evidence cited by a verdict, resolved to something a person can read.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CitedEvidence {
    pub id: String,
    pub kind: EvidenceKind,
    pub tool: String,
    pub subject: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AcceptanceCheck {
    pub requirement: String,
    pub passed: bool,
    #[serde(default)]
    pub evidence: Vec<CitedEvidence>,
    pub reason: String,
}

/// The latest independent verdict, kept on the session so a reconnecting client
/// does not have to rescan events to colour the contract bar.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Acceptance {
    pub goal: String,
    pub revision: u64,
    pub epoch: u64,
    /// Whether the runtime actually accepted completion, not just whether every
    /// check said passed: the completion gate has further conditions.
    pub passed: bool,
    pub checks: Vec<AcceptanceCheck>,
    pub summary: String,
    /// Length of the evidence list when the verdict was given. Passing evidence
    /// recorded after this point supersedes a failed check for that clause.
    pub evidence: usize,
    /// Clause versions the verdict was bound to.
    #[serde(default)]
    pub versions: BTreeMap<String, u64>,
}

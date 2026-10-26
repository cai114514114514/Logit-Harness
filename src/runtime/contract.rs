//! The contract ledger's view of a session: what each clause's evidence says now,
//! and what a revision actually changed.
//!
//! Everything here is derived from the same goal, evidence and step versions the
//! completion gate uses. It is presentation only and never feeds back into
//! scheduling or acceptance.
use super::{actor::Action, runner::Runner, state::Session};
use crate::domain::{
    EventData, Evidence, EvidenceKind, Goal, Requirement,
    contract::{
        Acceptance, AcceptanceCheck, CancelledAction, CitedEvidence, ClauseRecord, ClauseUpdate,
        ContractChanges, KeptStep, StaleEvidence, StaleReason,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// What the evidence says about one clause. One symbol states one fact; whether
/// someone is working on the clause is shown separately (`ClauseView::working`).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stamp {
    /// No current passing evidence beyond reading.
    Open,
    /// Current passing command or browser evidence, not yet independently reviewed.
    Evidenced,
    /// Current review evidence written by the verifier.
    Accepted,
    /// Passing evidence existed, but none of it is current any more.
    Stale,
    /// The latest current observation failed, or the latest verdict rejected the
    /// clause and nothing has passed since.
    Failed,
}

/// Where a clause's words came from.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// The user's complete original request.
    Original,
    /// An exact quote extracted from the original request.
    Quote,
    /// A later correction or answer from the user.
    Addition,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvidenceView {
    /// 1-based position in the current goal's evidence list, for display only.
    pub label: usize,
    pub id: String,
    pub kind: EvidenceKind,
    pub tool: String,
    pub subject: String,
    pub passed: bool,
    pub current: bool,
    /// Why passing evidence is no longer current.
    pub stale: Option<StaleReason>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClauseView {
    pub id: String,
    pub text: String,
    pub revision: u64,
    pub active: bool,
    pub source: String,
    pub origin: Origin,
    pub stamp: Stamp,
    /// A running action is scoped to this clause more narrowly than to the whole goal.
    pub working: bool,
    /// Earlier wordings, oldest first, as `(revision, text)`.
    pub history: Vec<(u64, String)>,
    pub evidence: Vec<EvidenceView>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContractView {
    pub revision: u64,
    pub epoch: u64,
    /// Active clauses in numeric order, then retired ones.
    pub clauses: Vec<ClauseView>,
    /// The latest verdict, only while it is bound to this goal and epoch.
    pub acceptance: Option<Acceptance>,
    /// The goal was verified and archived; this is its final record.
    #[serde(default)]
    pub archived: bool,
}

impl ContractView {
    pub fn active(&self) -> impl Iterator<Item = &ClauseView> {
        self.clauses.iter().filter(|c| c.active)
    }

    /// Apply a contract event before the next session view arrives. Evidence
    /// bound to a changed clause, or to any clause when the epoch advanced, is no
    /// longer current, so the same stamp rules give Stale or Open exactly; step
    /// invalidation of other clauses waits for the next view.
    pub fn revise(&mut self, revision: u64, epoch: u64, records: &[ClauseRecord], extract: bool) {
        // Extraction for the same goal keeps what is known about r1; a new goal
        // (after an archive, or with a different full request) starts clean.
        let current = self.clauses.iter().find(|c| c.id == "r1").map(|c| &c.text);
        let next = records.iter().find(|c| c.id == "r1").map(|c| &c.text);
        let fresh = extract && (self.archived || current != next);
        self.archived = false;
        if fresh {
            // A new goal's first contract reuses clause IDs (r1, r2, …) but none of
            // the previous goal's evidence or verdict.
            self.clauses = records
                .iter()
                .map(|record| ClauseView {
                    id: record.id.clone(),
                    text: record.text.clone(),
                    revision: record.revision,
                    active: record.active,
                    source: record.source.clone(),
                    origin: if record.id == "r1" {
                        Origin::Original
                    } else {
                        Origin::Quote
                    },
                    stamp: Stamp::Open,
                    working: false,
                    history: vec![],
                    evidence: vec![],
                })
                .collect();
            self.clauses
                .sort_by_key(|c| (!c.active, clause_order(&c.id)));
            self.revision = revision;
            self.epoch = epoch;
            self.acceptance = None;
            return;
        }
        let advanced = epoch != self.epoch;
        let mut clauses = records
            .iter()
            .map(|record| {
                let previous = self.clauses.iter().find(|c| c.id == record.id);
                let unchanged = !advanced
                    && previous.is_some_and(|c| {
                        c.revision == record.revision && c.active == record.active
                    });
                let mut clause = previous.cloned().unwrap_or_else(|| ClauseView {
                    id: record.id.clone(),
                    text: record.text.clone(),
                    revision: record.revision,
                    active: record.active,
                    source: record.source.clone(),
                    origin: Origin::Addition,
                    stamp: Stamp::Open,
                    working: false,
                    history: vec![],
                    evidence: vec![],
                });
                if !unchanged {
                    if clause.text != record.text {
                        clause.history.push((clause.revision, clause.text.clone()));
                    }
                    for evidence in &mut clause.evidence {
                        evidence.current = false;
                    }
                    clause.stamp = if clause.evidence.iter().any(|e| e.passed) {
                        Stamp::Stale
                    } else {
                        Stamp::Open
                    };
                    clause.text = record.text.clone();
                    clause.revision = record.revision;
                    clause.active = record.active;
                    clause.source = record.source.clone();
                    clause.working = false;
                }
                clause
            })
            .collect::<Vec<_>>();
        clauses.sort_by_key(|c| (!c.active, clause_order(&c.id)));
        self.clauses = clauses;
        self.revision = revision;
        self.epoch = epoch;
        self.acceptance = None;
    }

    /// Apply a verdict before the next session view: the verifier writes review
    /// evidence exactly for the checks it passes.
    pub fn verdict(&mut self, goal: &str, passed: bool, checks: &[AcceptanceCheck]) {
        for clause in self.clauses.iter_mut().filter(|c| c.active) {
            if let Some(check) = checks.iter().find(|c| c.requirement == clause.id) {
                clause.stamp = if check.passed {
                    Stamp::Accepted
                } else {
                    Stamp::Failed
                };
            }
        }
        self.acceptance = Some(Acceptance {
            goal: goal.into(),
            revision: self.revision,
            epoch: self.epoch,
            passed,
            checks: checks.to_vec(),
            summary: String::new(),
            evidence: 0,
            versions: self.active().map(|c| (c.id.clone(), c.revision)).collect(),
        });
    }
}

/// Order clause IDs as people count them: r2 before r10.
pub fn clause_order(id: &str) -> (u64, String) {
    (
        id.strip_prefix('r')
            .and_then(|n| n.parse().ok())
            .unwrap_or(u64::MAX),
        id.to_owned(),
    )
}

/// Whether a scope is narrower than the whole set of active clauses. Unfocused
/// actions serve every clause, and labelling them with all of them is noise.
pub fn focused(scope: &BTreeMap<String, u64>, goal: &Goal) -> bool {
    let active = goal.active_versions();
    !scope.is_empty() && scope.keys().ne(active.keys())
}

/// Human-readable object of a tool call: the command, path, query or URL.
pub fn subject(tool: &str, arguments: &Value) -> String {
    let arguments = crate::tools::approval::redact(arguments);
    if tool == "apply_patch" {
        return arguments["edits"]
            .as_array()
            .map(|edits| {
                edits
                    .iter()
                    .filter_map(|e| e["path"].as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
    }
    ["command", "path", "query", "url", "name"]
        .iter()
        .find_map(|k| arguments[*k].as_str())
        .and_then(|s| s.lines().next())
        .unwrap_or("")
        .to_owned()
}

/// The tool and object behind a piece of evidence. Review evidence has no action;
/// its description is the verifier's reason.
fn describe(session: &Session, evidence: &Evidence) -> (String, String) {
    describe_in(&session.settled_actions, &session.actions, evidence)
}

fn describe_in(
    settled: &BTreeMap<String, Action>,
    running: &BTreeMap<String, Action>,
    evidence: &Evidence,
) -> (String, String) {
    let action = evidence
        .artifact
        .as_ref()
        .and_then(|id| settled.get(id).or_else(|| running.get(id)));
    match action {
        Some(action) => (
            action.pending.call.name.clone(),
            subject(&action.pending.call.name, &action.pending.call.arguments),
        ),
        None if evidence.kind == EvidenceKind::Review => {
            ("review".into(), evidence.description.clone())
        }
        None => (evidence.description.clone(), String::new()),
    }
}

/// Resolve evidence IDs cited by a verdict. Unknown IDs are kept as-is so the
/// receipt never hides that the verifier cited something that does not exist.
pub(super) fn cited(session: &Session, ids: &[String]) -> Vec<CitedEvidence> {
    ids.iter()
        .map(|id| match session.evidence.iter().find(|e| e.id == *id) {
            Some(evidence) => {
                let (tool, subject) = describe(session, evidence);
                CitedEvidence {
                    id: id.clone(),
                    kind: evidence.kind.clone(),
                    tool,
                    subject,
                }
            }
            None => CitedEvidence {
                id: id.clone(),
                kind: EvidenceKind::Review,
                tool: String::new(),
                subject: String::new(),
            },
        })
        .collect()
}

fn stale_reason(
    evidence: &Evidence,
    goal: &Goal,
    steps: &BTreeMap<String, u64>,
) -> Option<StaleReason> {
    if evidence.requirements.iter().any(|(id, revision)| {
        !goal
            .requirements
            .get(id)
            .is_some_and(|r| r.active && r.revision == *revision)
    }) {
        Some(StaleReason::Clause)
    } else if evidence.epoch != goal.epoch {
        Some(StaleReason::Epoch)
    } else if evidence
        .steps
        .iter()
        .any(|(id, revision)| steps.get(id) != Some(revision))
    {
        Some(StaleReason::Step)
    } else {
        None
    }
}

fn origin(requirement: &Requirement, goal: &Goal) -> Origin {
    if requirement.revision == 1 && requirement.source == goal.original {
        if requirement.text == goal.original {
            Origin::Original
        } else {
            Origin::Quote
        }
    } else if requirement.revision == 1 && requirement.source == requirement.text {
        Origin::Quote
    } else {
        Origin::Addition
    }
}

/// The stamp rules from the design, in priority order.
fn stamp(
    id: &str,
    revision: u64,
    evidence: &[Evidence],
    goal: &Goal,
    steps: &BTreeMap<String, u64>,
    acceptance: Option<&Acceptance>,
) -> Stamp {
    let relevant = evidence
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind != EvidenceKind::Read && e.requirements.contains_key(id))
        .collect::<Vec<_>>();
    let matching = relevant
        .iter()
        .filter(|(_, e)| e.matches_versions(goal, steps))
        .collect::<Vec<_>>();
    if matching.last().is_some_and(|(_, e)| !e.passed) {
        return Stamp::Failed;
    }
    let rejected = acceptance.is_some_and(|a| {
        a.versions.get(id) == Some(&revision)
            && a.checks.iter().any(|c| c.requirement == id && !c.passed)
            && !relevant
                .iter()
                .any(|(i, e)| *i >= a.evidence && e.current_version(goal, steps))
    });
    if rejected {
        Stamp::Failed
    } else if matching
        .iter()
        .any(|(_, e)| e.passed && e.kind == EvidenceKind::Review)
    {
        Stamp::Accepted
    } else if matching.iter().any(|(_, e)| e.passed) {
        Stamp::Evidenced
    } else if relevant.iter().any(|(_, e)| e.passed) {
        Stamp::Stale
    } else {
        Stamp::Open
    }
}

/// Earlier wordings of a clause, from the goal's revision snapshots.
fn history(id: &str, goal: &Goal) -> Vec<(u64, String)> {
    let mut wordings: Vec<(u64, String)> = vec![];
    for snapshot in &goal.history {
        if let Some(r) = snapshot.requirements.get(id)
            && wordings.last().is_none_or(|(_, text)| *text != r.text)
        {
            wordings.push((r.revision, r.text.clone()));
        }
    }
    if let Some(current) = goal.requirements.get(id) {
        wordings.retain(|(_, text)| *text != current.text);
    }
    wordings
}

/// One goal's records: the current goal, or the latest archived one.
struct Source<'a> {
    goal: &'a Goal,
    evidence: &'a [Evidence],
    steps: BTreeMap<String, u64>,
    settled: &'a BTreeMap<String, Action>,
    running: &'a BTreeMap<String, Action>,
    archived: bool,
}

/// Contract state for a session: the current goal's, or once a verified goal
/// has been archived, that goal's, so `/contract` can still show what was
/// accepted. `None` before any goal exists.
pub fn derive(session: &Session) -> Option<ContractView> {
    let none = BTreeMap::new();
    let source = if !session.goal.original.is_empty() && !session.goal.requirements.is_empty() {
        Source {
            goal: &session.goal,
            evidence: &session.evidence,
            steps: session.graph.versions(),
            settled: &session.settled_actions,
            running: &session.actions,
            archived: false,
        }
    } else {
        let archive = session.archives.last()?;
        Source {
            goal: &archive.goal,
            evidence: &archive.evidence,
            steps: archive.graph.versions(),
            settled: &archive.actions,
            running: &none,
            archived: true,
        }
    };
    view(&source, session.acceptance.as_ref())
}

fn view(source: &Source, acceptance: Option<&Acceptance>) -> Option<ContractView> {
    let goal = source.goal;
    if goal.original.is_empty() || goal.requirements.is_empty() {
        return None;
    }
    let steps = &source.steps;
    let acceptance = acceptance.filter(|a| a.goal == goal.id && a.epoch == goal.epoch);
    let working = source
        .running
        .values()
        .filter(|a| a.running && focused(&a.pending.scope.requirements, goal))
        .flat_map(|a| a.pending.scope.requirements.keys().cloned())
        .collect::<BTreeSet<_>>();
    let mut clauses = goal
        .requirements
        .values()
        .map(|r| ClauseView {
            id: r.id.clone(),
            text: r.text.clone(),
            revision: r.revision,
            active: r.active,
            source: r.source.clone(),
            origin: origin(r, goal),
            stamp: stamp(&r.id, r.revision, source.evidence, goal, steps, acceptance),
            working: r.active && working.contains(&r.id),
            history: history(&r.id, goal),
            evidence: source
                .evidence
                .iter()
                .enumerate()
                .filter(|(_, e)| e.kind != EvidenceKind::Read && e.requirements.contains_key(&r.id))
                .map(|(i, e)| {
                    let (tool, subject) = describe_in(source.settled, source.running, e);
                    let current = e.current_version(goal, steps);
                    EvidenceView {
                        label: i + 1,
                        id: e.id.clone(),
                        kind: e.kind.clone(),
                        tool,
                        subject,
                        passed: e.passed,
                        current,
                        stale: (e.passed && !current)
                            .then(|| stale_reason(e, goal, steps))
                            .flatten(),
                    }
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    clauses.sort_by_key(|c| (!c.active, clause_order(&c.id)));
    Some(ContractView {
        revision: goal.revision,
        epoch: goal.epoch,
        clauses,
        acceptance: acceptance.cloned(),
        archived: source.archived,
    })
}

/// State captured just before a revision, so the contract event can say what
/// that revision actually did rather than what it might have done.
pub(super) struct Before {
    revision: u64,
    epoch: u64,
    clauses: BTreeMap<String, Requirement>,
    steps: BTreeMap<String, u64>,
    current: Vec<usize>,
    running: Vec<String>,
}

impl Runner {
    pub(super) fn contract_before(&self) -> Before {
        let goal = &self.session.goal;
        let steps = self.session.graph.versions();
        Before {
            revision: goal.revision,
            epoch: goal.epoch,
            clauses: goal.requirements.clone(),
            current: self
                .session
                .evidence
                .iter()
                .enumerate()
                .filter(|(_, e)| e.kind != EvidenceKind::Read && e.current_version(goal, &steps))
                .map(|(i, _)| i)
                .collect(),
            steps,
            running: self
                .jobs
                .iter()
                .filter(|(_, job)| {
                    matches!(job.kind, super::runner::JobKind::Tool { .. })
                        && !job.cancel.is_cancelled()
                })
                .map(|(id, _)| id.clone())
                .collect(),
        }
    }

    /// Record the contract after extraction (`before = None`) or a revision.
    /// A revision that changed nothing writes nothing.
    pub(super) fn emit_contract(
        &mut self,
        cause: &str,
        source: &str,
        before: Option<Before>,
    ) -> anyhow::Result<()> {
        let goal = &self.session.goal;
        if before
            .as_ref()
            .is_some_and(|b| b.revision == goal.revision && b.epoch == goal.epoch)
        {
            return Ok(());
        }
        let changes = before.map_or_else(ContractChanges::default, |b| self.changes(b));
        let mut clauses = goal
            .requirements
            .values()
            .map(|r| ClauseRecord {
                id: r.id.clone(),
                text: r.text.clone(),
                revision: r.revision,
                active: r.active,
                source: r.source.clone(),
            })
            .collect::<Vec<_>>();
        clauses.sort_by_key(|c| clause_order(&c.id));
        self.emit(EventData::Contract {
            revision: goal.revision,
            epoch: goal.epoch,
            source: source.into(),
            cause: cause.into(),
            clauses,
            changes,
        })
    }

    /// Keep the verdict for the contract bar and write it to the ledger. `passed`
    /// is the runtime's completion decision, which has conditions beyond the checks.
    pub(super) fn record_acceptance(
        &mut self,
        passed: bool,
        mut checks: Vec<AcceptanceCheck>,
        summary: &str,
        versions: &BTreeMap<String, u64>,
    ) -> anyhow::Result<()> {
        checks.sort_by_key(|c| clause_order(&c.requirement));
        let goal = &self.session.goal;
        let revision = goal.revision;
        self.session.acceptance = Some(Acceptance {
            goal: goal.id.clone(),
            revision,
            epoch: goal.epoch,
            passed,
            checks: checks.clone(),
            summary: summary.into(),
            evidence: self.session.evidence.len(),
            versions: versions.clone(),
        });
        self.emit(EventData::Acceptance {
            revision,
            passed,
            checks,
            summary: summary.into(),
        })
    }

    fn changes(&self, before: Before) -> ContractChanges {
        let goal = &self.session.goal;
        let steps = self.session.graph.versions();
        let mut changes = ContractChanges::default();
        for (id, old) in before.clauses.iter().filter(|(_, r)| r.active) {
            match goal.requirements.get(id) {
                Some(new) if new.active && new.text != old.text => {
                    changes.updated.push(ClauseUpdate {
                        id: id.clone(),
                        before: old.text.clone(),
                        after: new.text.clone(),
                    })
                }
                Some(new) if !new.active => changes.retired.push(id.clone()),
                _ => {}
            }
        }
        changes.added = goal
            .requirements
            .keys()
            .filter(|id| !before.clauses.contains_key(*id))
            .cloned()
            .collect();
        changes.added.sort_by_key(|id| clause_order(id));
        changes.updated.sort_by_key(|u| clause_order(&u.id));
        changes.retired.sort_by_key(|id| clause_order(id));
        for id in before.running {
            let Some(job) = self.jobs.get(&id).filter(|job| job.cancel.is_cancelled()) else {
                continue;
            };
            let action = self
                .session
                .actions
                .get(&id)
                .or_else(|| self.session.settled_actions.get(&id));
            let (tool, subject) = action.map_or_else(Default::default, |a| {
                (
                    a.pending.call.name.clone(),
                    subject(&a.pending.call.name, &a.pending.call.arguments),
                )
            });
            changes.cancelled.push(CancelledAction {
                id,
                tool,
                subject,
                requirements: job.scope.requirements.keys().cloned().collect(),
                settled: !action.is_some_and(|a| a.running),
            });
        }
        for index in before.current {
            let evidence = &self.session.evidence[index];
            if evidence.current_version(goal, &steps) {
                continue;
            }
            let (tool, subject) = describe(&self.session, evidence);
            changes.stale.push(StaleEvidence {
                id: evidence.id.clone(),
                tool,
                subject,
                reason: stale_reason(evidence, goal, &steps).unwrap_or(StaleReason::Step),
            });
        }
        changes.kept = self
            .session
            .graph
            .steps
            .values()
            .filter(|step| {
                before.steps.get(&step.id) == Some(&step.revision)
                    && step.status != super::graph::StepStatus::Pending
            })
            .map(|step| KeptStep {
                id: step.id.clone(),
                description: step.description.clone(),
            })
            .collect();
        changes
    }
}

#[cfg(test)]
#[path = "contract/tests.rs"]
mod tests;

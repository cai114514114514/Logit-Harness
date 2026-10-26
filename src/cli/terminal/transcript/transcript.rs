//! One event projection for live output, reconnect and durable history.
use super::text;
#[path = "entry.rs"]
mod entry;
#[path = "receipts.rs"]
pub(super) mod receipts;
#[path = "summary.rs"]
mod summary;
use crate::domain::{EventData, SessionEvent, Status};
use ratatui::text::{Line, Span};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    User,
    Assistant,
    Tool,
    Note,
    /// A contract, revision or acceptance card.
    Ledger,
}
#[derive(Clone, Debug)]
pub struct Entry {
    pub origin: crate::runtime::view::ViewTarget,
    pub id: String,
    pub turn: Option<String>,
    pub response: Option<String>,
    pub source: String,
    pub started: String,
    pub updated: String,
    pub kind: Kind,
    pub title: String,
    pub tool: Option<String>,
    pub body: String,
    /// Full persisted deltas stay in `body`; the bounded final result must never
    /// replace them. Keep structured outcome/error metadata independently.
    pub result: Option<serde_json::Value>,
    streamed: bool,
    pub running: bool,
    pub success: Option<bool>,
    pub settled: Option<u64>,
    /// Clauses a focused edit, run or check served; empty when it served all.
    pub clauses: Vec<String>,
    printed: BTreeSet<String>,
    quiet: bool,
}
#[derive(Clone, Default)]
struct Identity {
    turn: String,
    response: String,
    actor: String,
}
#[derive(Default)]
pub struct Transcript {
    entries: Vec<Entry>,
    identities: BTreeMap<String, Identity>,
    sequence: u64,
    local: u64,
    pub(super) ownership: super::ownership::Ownership,
    view: Option<crate::runtime::view::ViewTarget>,
    origin: crate::runtime::view::ViewTarget,
    boundaries: BTreeMap<String, crate::runtime::team::Target>,
    pub(super) usage: BTreeMap<String, crate::domain::Usage>,
    visited: BTreeSet<String>,
    names: BTreeMap<String, String>,
    /// A view marker belongs before the next presentation batch, not at the
    /// current event sequence after older, unseen destination entries.
    boundary: Option<String>,
    deliveries: BTreeMap<String, receipts::Delivery>,
    /// Action scopes arrive just before the action starts.
    scopes: BTreeMap<String, BTreeMap<String, u64>>,
    /// Wording of the active clauses, from the latest contract.
    clauses: BTreeMap<String, String>,
    /// Display numbers of evidence, from the latest session view.
    labels: BTreeMap<String, usize>,
    /// Running seconds reported by the runtime.
    elapsed: Option<u64>,
    /// Member IDs in roster order; their position picks the bar colour.
    members: Vec<String>,
    /// The last committed block, for spacing across batches.
    last: summary::Last,
}
impl Transcript {
    pub(crate) fn bind(&mut self, state: &crate::runtime::view::SessionView) {
        self.ownership.bindings.extend(state.bindings.clone());
        self.team(&state.team);
        self.contract_view(state.contract.as_ref());
    }
    /// Clause wording and evidence numbers from the latest session view.
    pub(super) fn contract_view(
        &mut self,
        contract: Option<&crate::runtime::contract::ContractView>,
    ) {
        let Some(contract) = contract else {
            return;
        };
        self.clauses = contract
            .active()
            .map(|c| (c.id.clone(), c.text.clone()))
            .collect();
        self.labels = contract
            .clauses
            .iter()
            .flat_map(|c| c.evidence.iter().map(|e| (e.id.clone(), e.label)))
            .collect();
    }
    pub(super) fn team(&mut self, team: &crate::runtime::team::TeamView) {
        for member in &team.members {
            if !self.members.contains(&member.id) {
                self.members.push(member.id.clone());
                self.members.sort();
            }
            self.names.insert(member.id.clone(), member.name.clone());
            for actor in &member.attempts {
                self.names.insert(actor.clone(), member.name.clone());
            }
        }
    }
    /// The bar and colour for an entry in the current view.
    pub(super) fn voice(&self, entry: &Entry) -> entry::Voice {
        if entry.kind == Kind::User || entry.origin.is_main() {
            return entry::Voice::of(entry);
        }
        let member = entry.origin.target.member_id();
        let index = self.members.iter().position(|id| id == member).unwrap_or(0);
        // Inside a member's own view the bar alone identifies it.
        let named = self.view.as_ref().is_none_or(|view| view.is_main());
        entry::Voice::member(member, index, named.then(|| self.name(member)))
    }
    /// Tags for a focused edit, run or check. Unknown contracts tag nothing.
    fn tags(&self, tool: &str, scope: Option<BTreeMap<String, u64>>) -> Vec<String> {
        let Some(scope) = scope.filter(|_| super::tool_text::serves_clauses(tool)) else {
            return vec![];
        };
        if self.clauses.is_empty() || scope.is_empty() || scope.keys().eq(self.clauses.keys()) {
            return vec![];
        }
        let mut ids = scope.into_keys().collect::<Vec<_>>();
        ids.sort_by_key(|id| crate::runtime::contract::clause_order(id));
        ids
    }
    /// Local `HH:MM` at which each contract version was recorded.
    pub(super) fn contract_times(&self) -> BTreeMap<u64, String> {
        self.entries
            .iter()
            .filter_map(|entry| match entry.card()? {
                super::ledger::Card::Contract { revision, .. } => {
                    let time = chrono::DateTime::parse_from_rfc3339(&entry.started).ok()?;
                    Some((
                        revision,
                        time.with_timezone(&chrono::Local)
                            .format("%H:%M")
                            .to_string(),
                    ))
                }
                super::ledger::Card::Acceptance { .. } => None,
            })
            .collect()
    }
    /// The complete record of one action, for expanding the evidence it produced.
    pub(super) fn action_text(&self, action: &str) -> Option<String> {
        self.entries
            .iter()
            .find(|entry| entry.id == action)
            .map(Entry::full_text)
    }
    pub(super) fn member_name(&self, id: &str) -> String {
        self.name(id)
    }
    fn name(&self, id: &str) -> String {
        self.names
            .get(id)
            .cloned()
            .unwrap_or_else(|| super::team::identity(id))
    }
    fn view_key(&self) -> String {
        self.view.as_ref().map_or_else(
            || "all".into(),
            |v| {
                format!(
                    "{}:{}",
                    v.goal.as_deref().unwrap_or(""),
                    v.target.member_id()
                )
            },
        )
    }
    pub(super) fn needs_recent(&self) -> bool {
        !self.visited.contains(&self.view_key())
    }
    pub fn focus(&mut self, view: crate::runtime::view::ViewTarget) {
        self.view = Some(view);
        self.boundary = None;
    }
    pub(super) fn switch(&mut self, view: crate::runtime::view::ViewTarget) {
        let boundary = format!(
            "── 视图 {} · {} · /history 完整历史 ──",
            super::team::identity(view.target.member_id()),
            super::team::assignment(&view.target)
        );
        self.focus(view);
        self.boundary = Some(boundary);
    }
    fn take_boundary(&mut self, width: usize) -> Vec<Line<'static>> {
        self.boundary.take().map_or_else(Vec::new, |boundary| {
            let mut lines = text::wrap(vec![Span::raw(boundary)], width);
            lines.push(Line::default());
            lines
        })
    }
    pub(super) fn heading(&self) -> String {
        self.view.as_ref().map_or_else(
            || "完整会话".into(),
            |view| {
                format!(
                    "{} · {}",
                    self.name(view.target.member_id()),
                    super::team::assignment(&view.target)
                )
            },
        )
    }
    pub fn visible(&self, entry: &Entry) -> bool {
        self.view.as_ref().is_none_or(|view| {
            super::ownership::same_view(view, &entry.origin)
                || view.is_main() && entry.source == "receipt"
        })
    }
    pub fn visible_entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| self.visible(e))
    }
    pub(super) fn note_in(&mut self, origin: &crate::runtime::view::ViewTarget, text: &str) {
        let previous = std::mem::replace(&mut self.origin, origin.clone());
        self.note(text);
        self.origin = previous;
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    fn key(&self, actor: &str) -> String {
        self.identities
            .get(actor)
            .map(|i| i.response.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("{actor}:{}", self.sequence))
    }
    fn add(&mut self, id: String, kind: Kind, source: &str, title: String, stamp: &str) -> usize {
        let identity = self
            .identities
            .get(&id)
            .or_else(|| self.identities.get(source));
        self.entries.push(Entry {
            origin: self.origin.clone(),
            id,
            kind,
            title,
            tool: None,
            source: identity.map_or(source, |i| &i.actor).into(),
            turn: identity.map(|i| i.turn.clone()),
            response: identity.map(|i| i.response.clone()),
            started: stamp.into(),
            updated: stamp.into(),
            body: String::new(),
            result: None,
            streamed: false,
            running: true,
            success: None,
            settled: None,
            clauses: vec![],
            printed: BTreeSet::new(),
            quiet: false,
        });
        self.entries.len() - 1
    }
    fn assistant(&mut self, actor: &str, stamp: &str) -> usize {
        let key = self.key(actor);
        if let Some(i) = self.entries.iter().position(|e| e.id == key) {
            return i;
        }
        if !self.identities.contains_key(actor)
            && let Some(i) = self
                .entries
                .iter()
                .rposition(|e| e.source == actor && e.kind == Kind::Assistant && e.running)
        {
            return i;
        }
        self.add(key, Kind::Assistant, actor, actor_label(actor), stamp)
    }
    fn done(&mut self, i: usize) {
        let entry = &mut self.entries[i];
        entry.running = false;
        entry.settled.get_or_insert(self.sequence);
    }
    pub fn receive(&mut self, event: &SessionEvent) {
        if event.sequence <= self.sequence {
            return;
        }
        self.sequence = event.sequence;
        self.ownership.observe(&event.data);
        self.origin = self.ownership.event(&event.data);
        if let EventData::Usage {
            input,
            output,
            cached,
            ..
        } = &event.data
        {
            self.usage
                .entry(self.origin.target.member_id().into())
                .or_default()
                .accumulate(&crate::domain::Usage {
                    input: *input,
                    output: *output,
                    cached: *cached,
                });
        }
        if !self.origin.is_main()
            && matches!(
                event.data,
                EventData::ModelRequested { .. }
                    | EventData::Message { .. }
                    | EventData::ActionStarted { .. }
            )
        {
            let member = self.origin.target.member_id().to_owned();
            if self.boundaries.get(&member) != Some(&self.origin.target) {
                let i = self.add(
                    format!("assignment:{}", event.sequence),
                    Kind::Note,
                    &member,
                    "分配".into(),
                    &event.timestamp,
                );
                self.entries[i].body = format!(
                    "── {} · {} ──",
                    self.name(&member),
                    super::team::assignment(&self.origin.target)
                );
                self.entries[i].result = Some(
                    serde_json::json!({"target": self.origin.target, "goal": self.origin.goal}),
                );
                self.done(i);
                self.boundaries.insert(member, self.origin.target.clone());
            }
        }
        let stamp = &event.timestamp;
        let first = self.entries.len();
        match &event.data {
            EventData::Mail { mail, .. } => self.mail(mail, stamp),
            EventData::RequestReceipt { receipt, .. } => self.receipt(receipt, stamp),
            EventData::Identity {
                actor,
                response,
                turn,
                action,
                ..
            } => {
                self.identities.insert(
                    action.as_ref().unwrap_or(actor).clone(),
                    Identity {
                        actor: actor.clone(),
                        response: response.clone(),
                        turn: turn.clone(),
                    },
                );
            }
            EventData::ModelRequested { actor, .. } => {
                self.assistant(actor, stamp);
            }
            EventData::ModelFinished {
                actor, artifact, ..
            } => {
                let response = std::path::Path::new(artifact)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.strip_suffix(".response.json"));
                let i = response
                    .and_then(|id| self.entries.iter().position(|e| e.id == id))
                    .unwrap_or_else(|| self.assistant(actor, stamp));
                // ModelFinished precedes Message. End activity now, but commit text only
                // on Message; a tool-only response produces no empty transcript block.
                self.entries[i].running = false;
            }
            EventData::Message { actor, text } => {
                let i = self.assistant(actor, stamp);
                self.entries[i].body = text.clone();
                self.entries[i].updated = stamp.clone();
                self.done(i);
            }
            EventData::Delta { actor, text } => {
                let i = self
                    .entries
                    .iter()
                    .position(|e| e.id == *actor && e.kind == Kind::Tool)
                    .unwrap_or_else(|| self.assistant(actor, stamp));
                self.entries[i].body.push_str(text);
                self.entries[i].streamed = true;
                self.entries[i].updated = stamp.clone();
            }
            EventData::ActionStarted {
                id,
                tool,
                arguments,
            } => {
                if self.entries.iter().any(|e| e.id == *id) {
                    return;
                }
                let i = self.add(
                    id.clone(),
                    Kind::Tool,
                    id,
                    super::tool_text::title(tool, arguments),
                    stamp,
                );
                self.entries[i].tool = Some(tool.clone());
                let scope = self.scopes.remove(id);
                self.entries[i].clauses = self.tags(tool, scope);
            }
            EventData::ActionScoped {
                id, requirements, ..
            } => {
                self.scopes.insert(id.clone(), requirements.clone());
            }
            EventData::Elapsed { seconds } => self.elapsed = Some(*seconds),
            EventData::Contract {
                revision,
                cause,
                clauses,
                changes,
                ..
            } => self.contract(*revision, cause, clauses, changes, stamp),
            EventData::Acceptance {
                revision,
                passed,
                checks,
                ..
            } => self.acceptance(*revision, *passed, checks, stamp),
            EventData::ActionFinished {
                id,
                success,
                result,
            } => {
                let i = self
                    .entries
                    .iter()
                    .position(|e| e.id == *id)
                    .unwrap_or_else(|| {
                        self.add(
                            id.clone(),
                            Kind::Tool,
                            id,
                            "工具结果（开始记录缺失）".into(),
                            stamp,
                        )
                    });
                let body = super::tool_text::result(result);
                if !self.entries[i].streamed && !body.is_empty() {
                    self.entries[i].body = body;
                }
                self.entries[i].result = Some(result.clone());
                self.entries[i].success = Some(*success);
                self.entries[i].updated = stamp.clone();
                if !success && self.entries[i].body.is_empty() {
                    self.entries[i].body = "执行失败；未提供结果正文".into();
                }
                self.done(i);
            }
            EventData::Created { objective, .. }
            | EventData::User {
                message: objective, ..
            } if !objective.is_empty() => {
                let id = match &event.data {
                    EventData::User {
                        turn: Some(turn), ..
                    } => format!("user:{turn}"),
                    _ => format!("user:{}", event.sequence),
                };
                if self.entries.iter().any(|entry| entry.id == id) {
                    return;
                }
                let i = self.add(id, Kind::User, "user", "你".into(), stamp);
                self.entries[i].body = objective.clone();
                self.done(i);
            }
            EventData::Status { status, reason } => {
                if matches!(
                    status,
                    Status::Paused
                        | Status::Stopped
                        | Status::Failed
                        | Status::Completed
                        | Status::Exhausted
                ) {
                    self.settle();
                }
                if *status != Status::Running {
                    self.note(&format!("{} · {reason}", super::state::label(status)));
                    // Normal lifecycle transitions already have a live status
                    // above the composer. Keep their raw reason in details only.
                    if reason == "response complete; waiting for user input"
                        || reason == "execution boundary settled"
                            && matches!(status, Status::Ready | Status::Waiting)
                    {
                        self.entries.last_mut().unwrap().quiet = true;
                    }
                }
            }
            EventData::Diagnostic { message } => {
                let display = if !self.origin.is_main() {
                    message
                        .split_once(": ")
                        .map(|(_, action)| {
                            format!(
                                "{} · {}",
                                self.name(self.origin.target.member_id()),
                                match action {
                                    "wait_message" => "等待成员消息",
                                    "send_message" => "成员消息已发送",
                                    "request_scope" => "已请求调整范围",
                                    _ => action,
                                }
                            )
                        })
                        .unwrap_or_else(|| message.clone())
                } else {
                    message.clone()
                };
                self.note(&display);
                if display != *message {
                    self.entries.last_mut().unwrap().result =
                        Some(serde_json::json!({"diagnostic": message}));
                }
                self.entries.last_mut().unwrap().quiet = super::present::routine(message);
            }
            EventData::Approval {
                id,
                tool,
                reason,
                details,
                ..
            } => self.note(&format!(
                "需要授权 · {tool}\n{reason}\n审阅：{details}\n/approve {id}"
            )),
            EventData::Revised { revision, changed } => {
                self.note(&format!("需求修订 {revision} · {}", changed.join(", ")))
            }
            EventData::Worker { id, status, task } => {
                let label = match status.as_str() {
                    "started" => "开始任务",
                    "completed" | "finished" => "任务已结算",
                    "failed" => "任务失败",
                    "invalidated" => "任务已失效",
                    "diagnosing" => "正在诊断",
                    _ => status,
                };
                self.note(&format!("成员进展 {} · {label} · {task}", self.name(id)));
                self.entries.last_mut().unwrap().result =
                    Some(serde_json::json!({"actor":id,"status":status,"task":task}));
            }
            _ => (),
        }
        for entry in self
            .entries
            .iter_mut()
            .skip(first)
            .filter(|entry| entry.kind == Kind::Note && entry.id.starts_with("note:"))
        {
            // Persisted notices use their original event time and identity;
            // opening history must not make an old notice appear to happen now.
            entry.id = format!("event:{}", event.sequence);
            entry.started = stamp.clone();
            entry.updated = stamp.clone();
        }
    }
    fn card(&mut self, card: super::ledger::Card, stamp: &str) {
        let id = format!("ledger:{}", self.sequence);
        let title = match &card {
            super::ledger::Card::Contract { revision, .. } => format!("契约 v{revision}"),
            super::ledger::Card::Acceptance { .. } => "验收回执".into(),
        };
        let source = card.speaker();
        let i = self.add(id, Kind::Ledger, source, title, stamp);
        self.entries[i].body = card.plain();
        self.entries[i].result = serde_json::to_value(&card).ok();
        self.done(i);
    }
    fn contract(
        &mut self,
        revision: u64,
        cause: &str,
        clauses: &[crate::domain::contract::ClauseRecord],
        changes: &crate::domain::contract::ContractChanges,
        stamp: &str,
    ) {
        self.clauses = clauses
            .iter()
            .filter(|c| c.active)
            .map(|c| (c.id.clone(), c.text.clone()))
            .collect();
        // The card supersedes the one-line revision notice written just before
        // it. A notice already in native history stays; it is not wrong.
        let notice = format!("需求修订 {revision} · ");
        if let Some(entry) =
            self.entries.iter_mut().rev().find(|e| {
                e.kind == Kind::Note && e.printed.is_empty() && e.body.starts_with(&notice)
            })
        {
            entry.quiet = true;
        }
        self.card(
            super::ledger::Card::Contract {
                revision,
                cause: cause.into(),
                clauses: clauses.to_vec(),
                changes: changes.clone(),
                labels: self.labels.clone(),
            },
            stamp,
        );
    }
    fn acceptance(
        &mut self,
        revision: u64,
        passed: bool,
        checks: &[crate::domain::contract::AcceptanceCheck],
        stamp: &str,
    ) {
        // The verifier's own steps since the previous verdict.
        let since = self
            .entries
            .iter()
            .rposition(|e| e.kind == Kind::Ledger && e.title == "验收回执")
            .map_or(0, |i| i + 1);
        let steps = self.entries[since..]
            .iter()
            .filter(|e| e.kind == Kind::Tool && e.source.starts_with("verify"))
            .collect::<Vec<_>>();
        let files = steps
            .iter()
            .filter(|e| e.tool.as_deref() == Some("read_file"))
            .map(|e| e.title.as_str())
            .collect::<BTreeSet<_>>()
            .len();
        let runs = steps
            .iter()
            .filter(|e| matches!(e.tool.as_deref(), Some("check" | "shell")))
            .count();
        let seconds = steps.first().and_then(|first| {
            let start = chrono::DateTime::parse_from_rfc3339(&first.started).ok()?;
            let end = chrono::DateTime::parse_from_rfc3339(stamp).ok()?;
            Some(end.signed_duration_since(start).num_seconds().max(0) as u64)
        });
        let mut total = crate::domain::Usage::default();
        for usage in self.usage.values() {
            total.accumulate(usage);
        }
        self.card(
            super::ledger::Card::Acceptance {
                revision,
                passed,
                checks: checks.to_vec(),
                clauses: self.clauses.clone(),
                verifier: super::ledger::Verifier {
                    files,
                    checks: runs,
                    seconds,
                },
                totals: super::ledger::Totals {
                    seconds: self.elapsed,
                    tokens: total.tokens(),
                    kv: total.cache_hit(),
                },
            },
            stamp,
        );
    }
    pub fn note(&mut self, text: &str) {
        self.local += 1;
        let i = self.add(
            format!("note:{}:{}", self.sequence, self.local),
            Kind::Note,
            "系统",
            "系统".into(),
            &chrono::Utc::now().to_rfc3339(),
        );
        self.entries[i].body = text.into();
        self.done(i);
    }
    pub fn settle(&mut self) {
        for i in 0..self.entries.len() {
            if self.entries[i].running {
                self.done(i);
            }
        }
    }
    pub(super) fn reconcile(&mut self, activities: &[crate::runtime::view::Activity]) {
        // A current snapshot also closes failed requests that have no final
        // Message. Never manufacture a successful tool result from a snapshot.
        for entry in &mut self.entries {
            if entry.running
                && !activities.iter().any(|a| {
                    a.id == entry.id || (entry.kind == Kind::Assistant && a.id == entry.source)
                })
            {
                entry.running = false;
            }
        }
    }
    pub fn active(&self) -> Option<&str> {
        self.visible_entries()
            .find(|e| e.running)
            .map(|e| e.title.as_str())
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn lines(&self, width: usize, expanded: bool) -> Vec<Line<'static>> {
        self.visible_entries()
            .flat_map(|e| e.lines(width, expanded))
            .collect()
    }
    pub fn active_lines(&self, width: usize) -> Vec<Line<'static>> {
        self.visible_entries()
            .filter(|e| e.running)
            .flat_map(|e| e.compact(width, &self.voice(e)))
            .collect()
    }
    pub fn drain_settled(&mut self, width: usize) -> Vec<Line<'static>> {
        let mut lines = self.take_boundary(width);
        let key = self.view_key();
        let mut indices = (0..self.entries.len())
            .filter(|i| {
                let e = &self.entries[*i];
                self.visible(e)
                    && !e.printed.contains(&key)
                    && e.settled.is_some()
                    && (!e.body.is_empty() || e.kind == Kind::Tool)
            })
            .collect::<Vec<_>>();
        indices.sort_by_key(|i| self.entries[*i].settled);
        let mut last = std::mem::take(&mut self.last);
        lines.extend(summary::batch(
            &indices
                .iter()
                .map(|i| &self.entries[*i])
                .collect::<Vec<_>>(),
            width,
            &|e| self.voice(e),
            &mut last,
        ));
        self.last = last;
        for i in indices {
            self.entries[i].printed.insert(key.clone());
        }
        lines
    }
    pub fn recent(&mut self, width: usize) -> Vec<Line<'static>> {
        let mut prefix = self.take_boundary(width);
        let view = self.view.clone();
        let key = self.view_key();
        self.visited.insert(key.clone());
        let mut indices = (0..self.entries.len())
            .filter(|i| {
                let e = &self.entries[*i];
                e.settled.is_some()
                    && view.as_ref().is_none_or(|view| {
                        super::ownership::same_view(view, &e.origin)
                            || view.is_main() && e.source == "receipt"
                    })
            })
            .collect::<Vec<_>>();
        indices.sort_by_key(|i| self.entries[*i].settled);
        for i in &indices {
            self.entries[*i].printed.insert(key.clone());
        }
        let first = indices.len().saturating_sub(20);
        // A restore starts a fresh run of native history after its marker.
        let mut last = summary::Last::default();
        let mut lines = summary::batch(
            &indices[first..]
                .iter()
                .map(|i| &self.entries[*i])
                .collect::<Vec<_>>(),
            width,
            &|e| self.voice(e),
            &mut last,
        );
        self.last = last;
        let marker = text::wrap(
            vec![Span::raw(
                "── 恢复最近记录（最多20块/200行）；/history 查看完整历史 ──",
            )],
            width,
        );
        prefix.extend(marker);
        prefix.truncate(200);
        let available = 200usize.saturating_sub(prefix.len());
        if lines.len() > available {
            lines.drain(..lines.len() - available);
        }
        prefix.into_iter().chain(lines).collect()
    }
}
pub(super) fn actor_label(actor: &str) -> String {
    if actor.starts_with("verify") {
        "独立验收"
    } else if actor.starts_with("worker") {
        "工作者"
    } else if actor.starts_with("steer") {
        "处理补充"
    } else if actor.starts_with("question") {
        "回答侧问"
    } else {
        "Logit"
    }
    .into()
}

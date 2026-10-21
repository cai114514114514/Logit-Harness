use super::stats::mean;
pub use super::stats::paired;
use super::{files, manifest::Frozen, run::Study, trial::Trial};
use crate::{
    config::Paths,
    domain::{EventData, SessionEvent},
    runtime::{
        money::{Ledger, RMB},
        state::Session,
    },
    store::Store,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Metrics {
    pub id: String,
    pub task: String,
    pub condition: String,
    pub repetition: u32,
    pub finished: bool,
    pub passed: Option<bool>,
    pub fidelity: Option<f64>,
    pub interventions: usize,
    pub false_completions: usize,
    pub tokens: Option<u64>,
    pub known_rmb: f64,
    pub uncertain_rmb: f64,
    pub reserved_rmb: f64,
    pub wall_seconds: Option<f64>,
    pub correction_applied: bool,
    pub correction_trigger: Option<String>,
    pub correction_latency_ms: Option<f64>,
    pub spanning_actions: usize,
    pub post_correction_command_ms: f64,
    pub models: Vec<String>,
}

pub fn write(root: &Path, frozen: &Frozen, paths: &Paths) -> anyhow::Result<()> {
    let study: Study = serde_json::from_slice(&std::fs::read(root.join("study.json"))?)?;
    anyhow::ensure!(
        study.corpus_hash == frozen.digest,
        "report corpus differs from study"
    );
    let store = Store::open(&paths.database())?;
    anyhow::ensure!(
        study.home == paths.home,
        "use the study's original state directory for reporting"
    );
    let ledger = Ledger::open(&paths.home.join("money.sqlite"), &study.account)?;
    let mut rows = vec![];
    for entry in &study.schedule {
        let directory = Trial::directory(root, &entry.id);
        if !directory.join("trial.json").exists() {
            continue;
        }
        let trial = Trial::load(&directory)?;
        let state = store.load::<Session>(&trial.session).ok();
        let events = store.events(&trial.session, 0)?;
        let receipt = super::correction::Correction::new(
            trial.spec()?,
            trial.session.clone(),
            std::sync::Arc::new(Store::open(&paths.database())?),
        )
        .receipt()?;
        let cost = ledger.session_balance(&trial.session)?;
        let mut row = metrics(&trial, &events, &receipt);
        row.tokens = state.as_ref().map(|state| state.budget.used);
        row.known_rmb = cost.used as f64 / RMB as f64;
        row.uncertain_rmb = cost.uncertain as f64 / RMB as f64;
        row.reserved_rmb = cost.reserved as f64 / RMB as f64;
        row.correction_applied = state.as_ref().is_some_and(|state| {
            receipt.control.is_some_and(|id| state.cursor >= id)
                && state.inbox.iter().any(|message| {
                    message.applied
                        && message.text
                            == trial.spec().map(|spec| spec.correction).unwrap_or_default()
                })
        });
        rows.push(row);
    }
    let mut summaries = BTreeMap::new();
    for condition in frozen
        .protocol
        .conditions
        .iter()
        .chain(&frozen.protocol.ablations)
    {
        let completed: Vec<_> = rows
            .iter()
            .filter(|row| &row.condition == condition && row.finished)
            .collect();
        let count = completed.len();
        summaries.insert(condition.clone(), serde_json::json!({
            "finished":count,
            "acceptance":mean(completed.iter().filter_map(|row| row.passed.map(f64::from))),
            "fidelity":mean(completed.iter().filter_map(|row| row.fidelity)),
            "interventions":mean(completed.iter().map(|row| row.interventions as f64)),
            "tokens":mean(completed.iter().filter_map(|row| row.tokens.map(|tokens| tokens as f64))),
            "known_rmb":completed.iter().map(|row| row.known_rmb).sum::<f64>(),
            "uncertain_rmb":completed.iter().map(|row| row.uncertain_rmb).sum::<f64>()
        }));
    }
    let mut comparisons = vec![];
    for baseline in ["prompt", "replan", "tracking", "no_stagnation"] {
        comparisons.push(serde_json::json!({
            "baseline":baseline,
            "fidelity_difference":paired(&rows,baseline,|row|row.fidelity,frozen),
            "intervention_difference":paired(&rows,baseline,|row|Some(row.interventions as f64),frozen),
            "post_correction_command_ms_difference":paired(&rows,baseline,|row|Some(row.post_correction_command_ms),frozen)
        }));
    }
    let finished = rows.iter().filter(|row| row.finished).count();
    let diagnostics = super::gate::assess(&rows, frozen, &study.split);
    let result = serde_json::json!({
        "study":study, "scheduled":study.schedule.len(), "finished":finished,
        "not_finished":study.schedule.len()-finished, "summaries":summaries,
        "paired_comparisons":comparisons, "trials":rows,
        "quantitative_diagnostics":diagnostics,
        "campaign":{
            "account":study.account, "balance_nano_rmb":ledger.balance()?,
            "prices_nano_rmb":ledger.prices()?,
            "basis":"Conservative configured rate accounting, not an invoice; includes every probe and study sharing this account."
        },
        "research_gate":"not_established", "human_review":"pending",
        "limits":[
            "Authored controlled CLI corpus; not a representative engineering workload sample.",
            "Scripted interventions are not human labor measurements.",
            "Commands spanning a correction are a timing proxy, not proof that all their work was useless.",
            "Incomplete schedules and missing human review cannot establish the registered research gate."
        ]
    });
    files::save(&root.join("report.json"), &result)?;
    let mut markdown = format!(
        "# Logit- Harness evaluation\n\n{} / {} trials finished. This is a {} report.\n\n| Condition | Finished | Acceptance | Fidelity | Mean interventions |\n|---|---:|---:|---:|---:|\n",
        finished,
        study.schedule.len(),
        study.split
    );
    for (name, summary) in summaries {
        markdown.push_str(&format!(
            "| {name} | {} | {} | {} | {} |\n",
            summary["finished"],
            summary["acceptance"],
            summary["fidelity"],
            summary["interventions"]
        ));
    }
    markdown.push_str("\nResearch effectiveness is not established. Inspect report.json for paired task-bootstrap intervals, missing results, uncertainty and raw trial metadata. Human review remains separate. Do not interpret post-correction command time as measured useless work.\n");
    files::write(&root.join("report.md"), markdown.as_bytes())?;
    let review = root.join("human-review.csv");
    {
        let mut csv = if review.exists() {
            std::fs::read_to_string(&review)?
        } else {
            "blind_id,workspace,modularity,comments,correction_fidelity,honesty,reviewer,notes\n"
                .to_owned()
        };
        let saved: std::collections::BTreeSet<_> = csv
            .lines()
            .filter_map(|line| line.split(',').next().map(str::to_owned))
            .collect();
        for row in rows.iter().filter(|row| row.finished) {
            let id = blake3::hash(row.id.as_bytes()).to_hex()[..16].to_owned();
            if saved.contains(&id) {
                continue;
            }
            let directory = Trial::directory(root, &row.id);
            csv.push_str(&format!(
                "{id},\"{}\",,,,,,\n",
                directory
                    .join("workspace")
                    .display()
                    .to_string()
                    .replace('"', "\"\"")
            ));
        }
        files::write(&review, csv.as_bytes())?;
    }
    Ok(())
}

pub fn metrics(
    trial: &Trial,
    events: &[SessionEvent],
    receipt: &super::correction::Receipt,
) -> Metrics {
    let correction = receipt.delivered.as_deref().and_then(time);
    let mut starts = BTreeMap::new();
    let mut processes = BTreeMap::new();
    let mut spanning = 0;
    let mut after = 0.0;
    let mut models = std::collections::BTreeSet::new();
    let mut applied = None;
    for event in events {
        let Some(at) = time(&event.timestamp) else {
            continue;
        };
        match &event.data {
            EventData::ActionStarted { id, .. } => {
                starts.insert(id.clone(), at);
            }
            EventData::ProcessStarted { id, .. } => {
                processes.insert(id.clone(), at);
            }
            EventData::ActionFinished { id, .. } => {
                if let Some(correction) = correction {
                    if starts.get(id).is_some_and(|start| *start <= correction) && at > correction {
                        spanning += 1;
                    }
                    if let Some(start) = processes.get(id)
                        && *start <= correction
                        && at > correction
                    {
                        after += at - correction;
                    }
                }
            }
            EventData::ModelFinished { model, .. } => {
                models.insert(model.clone());
            }
            EventData::Revised { .. }
                if correction.is_some_and(|correction| at >= correction) && applied.is_none() =>
            {
                applied = Some(at)
            }
            EventData::Diagnostic { message }
                if message == "user input classified and applied"
                    && correction.is_some_and(|correction| at >= correction)
                    && applied.is_none() =>
            {
                applied = Some(at)
            }
            _ => (),
        }
    }
    let wall = trial
        .started
        .as_deref()
        .and_then(time)
        .zip(trial.finished.as_deref().and_then(time))
        .map(|(a, b)| (b - a) / 1000.0);
    let complete = trial.finished.is_some();
    Metrics {
        id: trial.id.clone(),
        task: trial.task.clone(),
        condition: serde_json::to_value(trial.condition)
            .unwrap()
            .as_str()
            .unwrap()
            .into(),
        repetition: trial.repetition,
        finished: complete,
        passed: if complete {
            trial.attempts.last().map(|attempt| attempt.passed)
        } else {
            None
        },
        fidelity: if complete {
            trial.attempts.last().map(|attempt| attempt.fidelity)
        } else {
            None
        },
        interventions: usize::from(receipt.control.is_some()) + trial.rescues as usize,
        false_completions: trial
            .attempts
            .iter()
            .filter(|attempt| attempt.status == "Completed" && !attempt.passed)
            .count(),
        tokens: None,
        known_rmb: 0.0,
        uncertain_rmb: 0.0,
        reserved_rmb: 0.0,
        wall_seconds: wall,
        correction_applied: false,
        correction_trigger: receipt.control.map(|_| receipt.trigger.clone()),
        correction_latency_ms: correction.zip(applied).map(|(a, b)| b - a),
        spanning_actions: spanning,
        post_correction_command_ms: after,
        models: models.into_iter().collect(),
    }
}
fn time(text: &str) -> Option<f64> {
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.timestamp_millis() as f64)
}

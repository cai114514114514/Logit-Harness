//! Quantitative diagnostics do not substitute for the preregistered human review.
use super::{
    manifest::Frozen,
    report::Metrics,
    stats::{mean, paired},
};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub fn assess(rows: &[Metrics], frozen: &Frozen, split: &str) -> Value {
    let tasks: BTreeSet<_> = frozen
        .tasks
        .iter()
        .filter(|task| task.split == split)
        .map(|task| task.id.as_str())
        .collect();
    let conditions: BTreeSet<_> = frozen
        .protocol
        .conditions
        .iter()
        .chain(&frozen.protocol.ablations)
        .map(String::as_str)
        .collect();
    let mut identities = BTreeSet::new();
    let mut valid = true;
    for row in rows.iter().filter(|row| row.finished) {
        valid &= tasks.contains(row.task.as_str())
            && conditions.contains(row.condition.as_str())
            && (1..=frozen.protocol.repetitions).contains(&row.repetition)
            && row.passed.is_some()
            && row
                .fidelity
                .is_some_and(|value| (0.0..=1.0).contains(&value))
            && identities.insert((&row.task, &row.condition, row.repetition));
    }
    let complete = valid
        && identities.len()
            == tasks.len() * conditions.len() * frozen.protocol.repetitions as usize;
    let score = |condition: &str| {
        let group: Vec<_> = rows
            .iter()
            .filter(|row| row.finished && row.condition == condition)
            .collect();
        mean(group.iter().filter_map(|row| row.passed.map(f64::from)))
            .zip(mean(group.iter().filter_map(|row| row.fidelity)))
    };
    let strongest = ["prompt", "replan", "tracking"]
        .into_iter()
        .filter_map(|name| score(name).map(|score| (name, score)))
        .max_by(|(left_name, left), (right_name, right)| {
            left.0
                .total_cmp(&right.0)
                .then(left.1.total_cmp(&right.1))
                .then_with(|| right_name.cmp(left_name))
        })
        .map(|(name, _)| name);
    let Some(baseline) = strongest else {
        return json!({"complete":false,"numerical_gate":null,"reason":"no finished baseline observations"});
    };
    let fidelity = paired(rows, baseline, |row| row.fidelity, frozen);
    let interventions = paired(rows, baseline, |row| Some(row.interventions as f64), frozen);
    let average = |condition: &str| {
        mean(
            rows.iter()
                .filter(|row| row.finished && row.condition == condition)
                .map(|row| row.interventions as f64),
        )
    };
    let reduction = average("dynamic")
        .zip(average(baseline))
        .and_then(|(a, b)| (b > 0.0).then_some(1.0 - a / b));
    let fidelity_supported = fidelity["interval_95"][0]
        .as_f64()
        .is_some_and(|low| low >= -frozen.protocol.fidelity_margin);
    let reduction_supported = reduction
        .is_some_and(|value| value >= frozen.protocol.reduction_target)
        && interventions["interval_95"][1]
            .as_f64()
            .is_some_and(|high| high < 0.0);
    json!({
        "complete":complete,
        "strongest_observed_baseline":baseline,
        "selection_is_provisional":!complete || split != "holdout",
        "fidelity_difference":fidelity,
        "scripted_intervention_difference":interventions,
        "scripted_intervention_reduction":reduction,
        "fidelity_threshold_supported":fidelity_supported,
        "intervention_threshold_supported":reduction_supported,
        "numerical_gate":if complete && split == "holdout" {Some(fidelity_supported && reduction_supported)} else {None},
        "human_review":"separate; an actual reviewer must assess the blinded rubric",
        "measured_useless_work":null,
        "interpretation":"Incomplete/development data are exploratory. All baseline comparisons remain reported. Timing proxies cannot satisfy the measured-waste alternative."
    })
}

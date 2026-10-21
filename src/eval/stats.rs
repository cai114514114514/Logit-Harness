use super::{manifest::Frozen, report::Metrics};
use std::collections::BTreeMap;

#[derive(Default)]
struct Pair {
    dynamic: BTreeMap<u32, f64>,
    baseline: BTreeMap<u32, f64>,
}

pub(super) fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let values: Vec<_> = values.collect();
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

pub fn paired(
    rows: &[Metrics],
    baseline: &str,
    value: impl Fn(&Metrics) -> Option<f64>,
    frozen: &Frozen,
) -> serde_json::Value {
    let mut groups: BTreeMap<String, Pair> = BTreeMap::new();
    let mut invalid = std::collections::BTreeSet::new();
    for row in rows
        .iter()
        .filter(|row| row.finished && (row.condition == "dynamic" || row.condition == baseline))
    {
        if let Some(value) = value(row) {
            if !value.is_finite() || !(1..=frozen.protocol.repetitions).contains(&row.repetition) {
                invalid.insert(row.task.clone());
                continue;
            }
            let pair = groups.entry(row.task.clone()).or_default();
            let previous = if row.condition == "dynamic" {
                pair.dynamic.insert(row.repetition, value)
            } else {
                pair.baseline.insert(row.repetition, value)
            };
            if previous.is_some() {
                invalid.insert(row.task.clone());
            }
        }
    }
    let differences: Vec<_> = groups
        .iter()
        .filter(|(task, pair)| {
            !invalid.contains(*task)
                && pair.dynamic.len() == frozen.protocol.repetitions as usize
                && pair.baseline.len() == frozen.protocol.repetitions as usize
        })
        .map(|(_, pair)| {
            let (a, b) = (&pair.dynamic, &pair.baseline);
            a.values().sum::<f64>() / a.len() as f64 - b.values().sum::<f64>() / b.len() as f64
        })
        .collect();
    if differences.len() < 2 {
        return serde_json::json!({"paired_tasks":differences.len(),"difference":mean(differences.into_iter()),"interval_95":null});
    }
    let mut state = frozen.protocol.seed;
    let mut samples = Vec::with_capacity(frozen.protocol.bootstrap_samples);
    for _ in 0..frozen.protocol.bootstrap_samples {
        let mut total = 0.0;
        for _ in &differences {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            total += differences[state as usize % differences.len()];
        }
        samples.push(total / differences.len() as f64);
    }
    samples.sort_by(f64::total_cmp);
    serde_json::json!({"paired_tasks":differences.len(),"difference":mean(differences.into_iter()),"interval_95":[samples[samples.len()*25/1000],samples[samples.len()*975/1000]]})
}

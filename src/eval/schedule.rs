use super::manifest::Frozen;
use crate::runtime::condition::Condition;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub task: String,
    pub repetition: u32,
    pub condition: Condition,
}

pub fn schedule(frozen: &Frozen, split: &str) -> anyhow::Result<Vec<Entry>> {
    anyhow::ensure!(
        matches!(split, "development" | "holdout"),
        "split must be development or holdout"
    );
    let mut tasks: Vec<_> = frozen
        .tasks
        .iter()
        .filter(|task| task.split == split)
        .collect();
    tasks.sort_by_key(|task| key(frozen.protocol.seed, &task.id));
    let conditions: Vec<Condition> = frozen
        .protocol
        .conditions
        .iter()
        .chain(&frozen.protocol.ablations)
        .map(|name| serde_json::from_value(serde_json::json!(name)))
        .collect::<Result<_, _>>()?;
    let mut entries = vec![];
    for task in tasks {
        for repetition in 1..=frozen.protocol.repetitions {
            let mut pair = vec![];
            for condition in &conditions {
                let name = serde_json::to_value(condition)?
                    .as_str()
                    .unwrap()
                    .to_owned();
                pair.push(Entry {
                    id: format!("{}-{repetition}-{name}", task.id),
                    task: task.id.clone(),
                    repetition,
                    condition: *condition,
                });
            }
            pair.sort_by_key(|entry| key(frozen.protocol.seed, &entry.id));
            entries.extend(pair);
        }
    }
    Ok(entries)
}
fn key(seed: u64, text: &str) -> String {
    blake3::hash(format!("{seed}:{text}").as_bytes())
        .to_hex()
        .to_string()
}

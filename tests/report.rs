//! Synthetic statistical fixtures; these are not provider quality results.
use loh::eval::{
    manifest::Frozen,
    report::{Metrics, paired},
};

#[test]
fn bootstrap_pairs_tasks_after_averaging_their_repetitions() {
    let frozen = Frozen::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("eval/registered.json"),
    )
    .unwrap();
    let mut rows = vec![];
    for task in ["synthetic-a", "synthetic-b"] {
        for repetition in 1..=3 {
            for (condition, fidelity) in [("dynamic", 1.0), ("tracking", 0.75)] {
                rows.push(Metrics {
                    id: format!("{task}-{repetition}-{condition}"),
                    task: task.into(),
                    condition: condition.into(),
                    repetition,
                    finished: true,
                    fidelity: Some(fidelity),
                    ..Default::default()
                });
            }
        }
    }
    let result = paired(&rows, "tracking", |row| row.fidelity, &frozen);
    assert_eq!(result["paired_tasks"], 2);
    assert_eq!(result["difference"], 0.25);
    assert_eq!(result["interval_95"], serde_json::json!([0.25, 0.25]));
    rows.pop();
    let incomplete = paired(&rows, "tracking", |row| row.fidelity, &frozen);
    assert_eq!(incomplete["paired_tasks"], 1);
    assert!(incomplete["interval_95"].is_null());
    // A duplicate cannot stand in for the missing third repetition.
    let duplicate = rows
        .iter()
        .find(|row| row.task == "synthetic-b" && row.condition == "tracking")
        .unwrap()
        .clone();
    rows.push(duplicate);
    assert_eq!(
        paired(&rows, "tracking", |row| row.fidelity, &frozen)["paired_tasks"],
        1
    );
}

#[test]
fn research_thresholds_require_a_complete_holdout_and_supported_direction() {
    let frozen = Frozen::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("eval/registered.json"),
    )
    .unwrap();
    let mut rows = vec![];
    for task in frozen.tasks.iter().filter(|task| task.split == "holdout") {
        for repetition in 1..=3 {
            for condition in ["dynamic", "prompt", "replan", "tracking", "no_stagnation"] {
                rows.push(Metrics {
                    id: format!("{}-{repetition}-{condition}", task.id),
                    task: task.id.clone(),
                    repetition,
                    condition: condition.into(),
                    finished: true,
                    passed: Some(true),
                    fidelity: Some(match condition {
                        "replan" => 1.0,
                        "dynamic" => 0.98,
                        _ => 0.9,
                    }),
                    interventions: if condition == "dynamic" { 1 } else { 2 },
                    ..Default::default()
                });
            }
        }
    }
    let report = loh::eval::gate::assess(&rows, &frozen, "holdout");
    assert_eq!(report["strongest_observed_baseline"], "replan");
    assert_eq!(report["numerical_gate"], true);
    assert_eq!(report["scripted_intervention_reduction"], 0.5);
    rows.pop();
    assert!(loh::eval::gate::assess(&rows, &frozen, "holdout")["numerical_gate"].is_null());
}

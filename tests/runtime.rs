use std::collections::{BTreeMap, BTreeSet};
use weft::domain::{Change, Goal};
use weft::runtime::graph::{Graph, Step, StepStatus};

fn set(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|s| s.to_string()).collect()
}
fn step(id: &str, requirements: &[&str], deps: &[&str]) -> Step {
    Step {
        id: id.into(),
        description: id.into(),
        requirements: set(requirements),
        dependencies: set(deps),
        files: set(&[]),
        revision: 1,
        status: StepStatus::Verified,
    }
}

#[test]
fn revision_invalidates_transitive_dependents_but_preserves_unrelated_work() {
    let mut goal = Goal::new("notes app");
    goal.revise(Change {
        source: "JSON storage and a keyboard shortcut".into(),
        updates: BTreeMap::new(),
        additions: vec!["storage".into(), "shortcut".into()],
        replacement: false,
    })
    .unwrap();
    let mut graph = Graph::default();
    graph
        .install(
            vec![
                step("root", &["r1"], &[]),
                step("storage", &["r2"], &[]),
                step("export", &["r2"], &["storage"]),
                step("shortcut", &["r3"], &[]),
            ],
            &goal,
        )
        .unwrap();
    for step in graph.steps.values_mut() {
        step.status = StepStatus::Verified;
    }
    let changed = graph.invalidate(&set(&["r2"]));
    assert_eq!(changed, set(&["storage", "export"]));
    assert_eq!(graph.steps["storage"].status, StepStatus::Pending);
    assert_eq!(graph.steps["shortcut"].status, StepStatus::Verified);
}

#[test]
fn cyclic_or_incomplete_plan_is_rejected_atomically() {
    let goal = Goal::new("deliver");
    let mut graph = Graph::default();
    graph
        .install(vec![step("valid", &["r1"], &[])], &goal)
        .unwrap();
    assert!(
        graph
            .install(
                vec![step("a", &["r1"], &["b"]), step("b", &["r1"], &["a"])],
                &goal
            )
            .is_err()
    );
    assert!(graph.steps.contains_key("valid"));
    assert!(graph.install(vec![], &goal).is_err());
}

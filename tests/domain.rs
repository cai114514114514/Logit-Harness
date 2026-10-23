use loh::domain::{Budget, EventData, Evidence, EvidenceKind, Usage};
use loh::domain::{Change, Goal};
use loh::store::Store;
use std::collections::BTreeMap;

#[test]
fn initial_request_cannot_disappear_from_acceptance() {
    let goal = Goal::new("Build an offline notes app");
    assert_eq!(goal.requirements.len(), 1);
    assert_eq!(goal.requirements["r1"].source, goal.original);
}

#[test]
fn correction_changes_only_affected_requirement() {
    let mut goal = Goal::new("Build notes");
    goal.revise(Change {
        source: "Use JSON storage and a keyboard shortcut".into(),
        updates: BTreeMap::new(),
        additions: vec!["JSON storage".into(), "keyboard shortcut".into()],
        replacement: false,
    })
    .unwrap();
    let shortcut_revision = goal.requirements["r3"].revision;
    let impact = goal
        .revise(Change {
            source: "Change storage to SQLite".into(),
            updates: BTreeMap::from([("r2".into(), "SQLite storage".into())]),
            additions: vec![],
            replacement: false,
        })
        .unwrap();
    assert_eq!(impact.changed.into_iter().collect::<Vec<_>>(), vec!["r2"]);
    assert_eq!(goal.requirements["r3"].revision, shortcut_revision);
    assert!(goal.requirements["r1"].active);
}

#[test]
fn invalid_correction_is_atomic() {
    let mut goal = Goal::new("Build notes");
    let before = serde_json::to_string(&goal).unwrap();
    assert!(
        goal.revise(Change {
            source: "oops".into(),
            updates: BTreeMap::from([("missing".into(), "something".into())]),
            additions: vec![],
            replacement: false,
        })
        .is_err()
    );
    assert_eq!(serde_json::to_string(&goal).unwrap(), before);
}

#[test]
fn replacement_cannot_update_requirements_it_would_deactivate() {
    let mut goal = Goal::new("Keep the CLI and use JSON storage");
    let before = serde_json::to_string(&goal).unwrap();
    let result = goal.revise(Change {
        source: "Change storage to SQLite; keep the CLI and verify it".into(),
        updates: BTreeMap::from([("r1".into(), "Keep the CLI and use SQLite storage".into())]),
        additions: vec!["Verify the SQLite implementation".into()],
        replacement: true,
    });
    assert!(
        result.is_err(),
        "mixed replacement/update must not silently drop the updated requirement"
    );
    assert_eq!(serde_json::to_string(&goal).unwrap(), before);
}

#[test]
fn a_replacement_retains_the_complete_user_objective_for_acceptance() {
    let mut goal = Goal::new("Build a todo application");
    let replacement =
        "Cancel the todo application. Build an offline calculator with keyboard input.";
    goal.revise(Change {
        source: replacement.into(),
        updates: BTreeMap::new(),
        additions: vec!["Build an offline calculator".into()],
        replacement: true,
    })
    .unwrap();
    assert!(!goal.requirements["r1"].active);
    assert!(
        goal.requirements.values().any(|requirement| {
            requirement.active
                && requirement.text == replacement
                && requirement.source == replacement
        }),
        "derived replacement requirements must not omit the complete verbatim user request"
    );
}

#[test]
fn passing_result_does_not_cover_an_omitted_requirement() {
    let goal = Goal::new("Store notes offline and support keyboard shortcuts");
    assert_eq!(goal.missing_evidence(&[], &BTreeMap::new()), vec!["r1"]);
}

#[test]
fn evidence_expires_when_code_or_requirement_changes() {
    let mut goal = Goal::new("Store notes as JSON");
    let files = BTreeMap::from([("src/storage.rs".into(), "before".into())]);
    let evidence = Evidence {
        steps: Default::default(),
        epoch: goal.epoch,
        id: "check1".into(),
        kind: EvidenceKind::Command,
        requirements: goal.active_versions(),
        files: files.clone(),
        passed: true,
        description: "round trip test passed".into(),
        artifact: None,
    };
    assert!(evidence.current(&goal, &files));
    let changed = BTreeMap::from([("src/storage.rs".into(), "after".into())]);
    assert!(!evidence.current(&goal, &changed));
    goal.revise(Change {
        source: "Use SQLite instead".into(),
        updates: BTreeMap::from([("r1".into(), "Store notes as SQLite".into())]),
        additions: vec![],
        replacement: false,
    })
    .unwrap();
    assert!(!evidence.current(&goal, &files));
}

#[test]
fn reading_source_is_not_validation() {
    let goal = Goal::new("fix search");
    let observation = Evidence {
        steps: Default::default(),
        epoch: goal.epoch,
        id: "read1".into(),
        kind: EvidenceKind::Read,
        requirements: goal.active_versions(),
        files: BTreeMap::new(),
        passed: true,
        description: "read source".into(),
        artifact: None,
    };
    assert_eq!(
        goal.missing_evidence(&[observation], &BTreeMap::new()),
        vec!["r1"]
    );
}

#[test]
fn workers_cannot_spend_the_same_remaining_budget() {
    let mut budget = Budget::new(100);
    let first = budget.reserve(70).unwrap();
    assert!(budget.reserve(40).is_err());
    budget
        .settle(
            &first,
            Some(&Usage {
                input: 15,
                output: 5,
                cached: 0,
            }),
        )
        .unwrap();
    assert_eq!(budget.remaining(), 80);
    assert!(budget.settle(&first, Some(&Usage::default())).is_err());
}

#[test]
fn interrupted_request_keeps_unknown_usage_reserved_as_liability() {
    let mut budget = Budget::new(100);
    let id = budget.reserve(70).unwrap();
    budget.settle(&id, None).unwrap();
    assert_eq!(budget.remaining(), 30);
    assert_eq!(budget.uncertain, 70);
}

#[test]
fn state_and_events_survive_reopen_with_monotonic_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    let store = Store::open(&path).unwrap();
    let first = store
        .commit(
            "s1",
            &serde_json::json!({"revision":1}),
            EventData::User {
                turn: None,
                message: "build".into(),
            },
        )
        .unwrap();
    let second = store
        .commit(
            "s1",
            &serde_json::json!({"revision":2}),
            EventData::User {
                turn: None,
                message: "correct".into(),
            },
        )
        .unwrap();
    assert!(second.sequence > first.sequence);
    drop(store);
    let reopened = Store::open(&path).unwrap();
    let state: serde_json::Value = reopened.load("s1").unwrap();
    assert_eq!(state["revision"], 2);
    let events = reopened.events("s1", first.sequence).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].sequence, second.sequence);
    assert!(reopened.events("different-session", 0).unwrap().is_empty());
}

#[test]
fn deletion_evidence_expires_if_the_file_returns() {
    let goal = Goal::new("remove obsolete file");
    let evidence = Evidence {
        steps: Default::default(),
        epoch: goal.epoch,
        id: "delete-check".into(),
        kind: EvidenceKind::Review,
        requirements: goal.active_versions(),
        files: BTreeMap::from([("obsolete.txt".into(), "missing".into())]),
        passed: true,
        description: "absent".into(),
        artifact: None,
    };
    assert!(evidence.current(&goal, &BTreeMap::new()));
    assert!(!evidence.current(
        &goal,
        &BTreeMap::from([("obsolete.txt".into(), "restored".into())])
    ));
}
#[test]
fn decomposition_retains_original_and_requires_exact_user_provenance() {
    let mut goal = loh::domain::Goal::new("Use JSON storage. Render a blue interface.");
    let ids = goal
        .extract(&[
            "Use JSON storage.".into(),
            "Render a blue interface.".into(),
        ])
        .unwrap();
    assert_eq!(ids.len(), 2);
    assert!(goal.active_versions().contains_key("r1"));
    assert!(goal.extract(&["Publish the application".into()]).is_err());
    let first = ids[0].clone();
    goal.revise(loh::domain::Change {
        source: "Use SQLite instead".into(),
        updates: std::collections::BTreeMap::from([(first.clone(), "Use SQLite storage.".into())]),
        additions: vec![],
        replacement: false,
    })
    .unwrap();
    assert_eq!(goal.history[0].requirements["r1"].text, goal.original);
    assert!(goal.history.iter().any(|revision| {
        revision
            .requirements
            .get(&first)
            .is_some_and(|r| r.text == "Use JSON storage.")
    }));
    assert_eq!(goal.requirements[&ids[1]].revision, 1);
}
#[test]
fn a_new_requirement_with_unknown_dependencies_expires_prior_evidence() {
    let mut goal = Goal::new("Build a tool");
    let proof = Evidence {
        id: "proof".into(),
        kind: EvidenceKind::Command,
        requirements: goal.active_versions(),
        steps: Default::default(),
        epoch: goal.epoch,
        files: Default::default(),
        passed: true,
        description: "old check".into(),
        artifact: None,
    };
    goal.revise(Change {
        source: "Also support encryption everywhere".into(),
        updates: Default::default(),
        additions: vec!["Support encryption everywhere".into()],
        replacement: false,
    })
    .unwrap();
    assert!(!proof.current(&goal, &Default::default()));
}

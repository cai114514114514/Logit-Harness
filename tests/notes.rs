use loh::domain::{
    Change, Goal,
    notes::{NoteInput, NoteKind},
};

#[test]
fn model_notes_preserve_user_sources_and_expire_on_correction() {
    let mut goal = Goal::new("Store tasks in JSON; preserve their titles.");
    let before = goal.active_versions();
    goal.annotate(
        NoteInput {
            kind: NoteKind::Assumption,
            text: "Single local user".into(),
            requirements: ["r1".into()].into(),
            quotes: vec![],
        },
        "main:turn-1",
    )
    .unwrap();
    goal.annotate(
        NoteInput {
            kind: NoteKind::Acceptance,
            text: "Reload preserves titles".into(),
            requirements: ["r1".into()].into(),
            quotes: vec!["preserve their titles".into()],
        },
        "main:turn-2",
    )
    .unwrap();
    assert_eq!(
        before,
        goal.active_versions(),
        "annotations cannot alter the user's goal"
    );
    assert_eq!(goal.notes[1].author, "main:turn-2");
    assert!(goal.notes.iter().all(|n| n.current(&goal)));
    assert!(
        goal.annotate(
            NoteInput {
                kind: NoteKind::Acceptance,
                text: "Discard titles".into(),
                requirements: ["r1".into()].into(),
                quotes: vec!["titles are optional".into()]
            },
            "main"
        )
        .is_err()
    );
    assert_eq!(goal.notes.len(), 2);
    goal.revise(Change {
        source: "Use SQLite instead".into(),
        updates: [(
            "r1".into(),
            "Store tasks in SQLite; preserve their titles".into(),
        )]
        .into(),
        additions: vec![],
        replacement: false,
    })
    .unwrap();
    assert!(goal.notes.iter().all(|n| !n.current(&goal)));
    assert!(goal.history[0].requirements["r1"].source.contains("JSON"));
}

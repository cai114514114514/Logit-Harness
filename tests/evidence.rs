use loh::{
    domain::{Evidence, EvidenceKind, Goal},
    tools::files::Workspace,
};

#[test]
fn explicit_build_artifact_evidence_tracks_content_and_deletion() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("target")).unwrap();
    let path = root.path().join("target/report.txt");
    std::fs::write(&path, "passed").unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    let goal = Goal::new("produce a report");
    let evidence = Evidence {
        id: "report".into(),
        kind: EvidenceKind::Command,
        requirements: goal.active_versions(),
        epoch: 0,
        steps: Default::default(),
        files: [(
            "target/report.txt".into(),
            workspace.read("target/report.txt", 100).unwrap().hash,
        )]
        .into(),
        passed: true,
        description: "report inspected".into(),
        artifact: None,
    };
    assert!(evidence.current(
        &goal,
        &workspace.snapshot_for(evidence.files.keys()).unwrap()
    ));
    std::fs::write(&path, "failed").unwrap();
    assert!(!evidence.current(
        &goal,
        &workspace.snapshot_for(evidence.files.keys()).unwrap()
    ));
    std::fs::remove_file(path).unwrap();
    assert!(!evidence.current(
        &goal,
        &workspace.snapshot_for(evidence.files.keys()).unwrap()
    ));
}

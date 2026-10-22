use loh::{
    extensions::{instructions::Instructions, knowledge::Knowledge, skills},
    tools::{Scope, ToolContext, files::Workspace},
};
use serde_json::json;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

fn project() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("nested")).unwrap();
    std::fs::write(root.path().join("AGENTS.md"), "root guidance").unwrap();
    std::fs::write(root.path().join("nested/AGENTS.md"), "nested guidance").unwrap();
    std::fs::write(root.path().join("nested/file.rs"), "").unwrap();
    root
}

#[tokio::test]
async fn instruction_tool_accepts_workspace_absolute_paths_and_directory_scopes() {
    let root = project();
    let knowledge = Arc::new(Knowledge::new(root.path().into(), vec![]).unwrap());
    let tool = skills::tools(knowledge)
        .into_iter()
        .find(|t| t.spec().name == "project_instructions")
        .unwrap();
    for (path, expected) in [
        (root.path().display().to_string(), 1),
        (".".into(), 1),
        ("nested".into(), 2),
        (root.path().join("nested").display().to_string(), 2),
        (root.path().join("nested/file.rs").display().to_string(), 2),
        ("nested/not-yet-created.rs".into(), 2),
        (
            root.path()
                .join("nested/future/new.rs")
                .display()
                .to_string(),
            2,
        ),
    ] {
        let (events, _) = tokio::sync::mpsc::unbounded_channel();
        let result = tool
            .call(
                json!({"paths":[path]}),
                ToolContext {
                    workspace: Workspace::open(root.path()).unwrap(),
                    logs: root.path().join("logs"),
                    id: "instructions".into(),
                    scope: Scope {
                        readonly: true,
                        ..Scope::default()
                    },
                },
                CancellationToken::new(),
                events,
            )
            .await
            .unwrap();
        let instructions = result.data["instructions"].as_array().unwrap();
        assert_eq!(instructions.len(), expected, "scope {path}");
        assert_eq!(instructions[0]["content"], "root guidance");
        if expected == 2 {
            assert_eq!(instructions[1]["content"], "nested guidance");
        }
    }
}

#[test]
fn instructions_reject_outside_scopes_and_symlink_escapes() {
    let root = project();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.rs"), "secret").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
    let instructions = Instructions::new(root.path().into());
    for path in [
        "../outside.rs".into(),
        outside.path().join("secret.rs").display().to_string(),
        "escape/secret.rs".into(),
        "escape/missing.rs".into(),
        "escape/../nested/file.rs".into(),
    ] {
        assert!(
            instructions.for_paths(std::slice::from_ref(&path)).is_err(),
            "accepted {path}"
        );
    }
    std::fs::remove_file(root.path().join("nested/AGENTS.md")).unwrap();
    std::os::unix::fs::symlink(
        root.path().join("AGENTS.md"),
        root.path().join("nested/AGENTS.md"),
    )
    .unwrap();
    assert!(instructions.for_paths(&["nested/file.rs".into()]).is_err());
}

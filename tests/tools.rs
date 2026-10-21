use serde_json::json;
use std::time::Duration;
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use weft::domain::{Change, Goal};
use weft::model::ToolCall;
use weft::tools::files::{EditOperation, FileEdit, MISSING, Workspace};
use weft::tools::policy::{self, Decision};
use weft::tools::process::{self, ProcessEvent, ProcessSpec};
use weft::tools::{Scope, ToolHost};

#[test]
fn stale_patch_never_overwrites_an_external_edit() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("code.rs"), "original").unwrap();
    let workspace = Workspace::open(dir.path()).unwrap();
    let observed = workspace.read("code.rs", 1000).unwrap();
    std::fs::write(dir.path().join("code.rs"), "user changed this").unwrap();
    assert!(
        workspace
            .apply(&[FileEdit {
                path: "code.rs".into(),
                expected: observed.hash,
                operation: EditOperation::Replace {
                    old: "original".into(),
                    new: "agent change".into()
                }
            }])
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("code.rs")).unwrap(),
        "user changed this"
    );
}

#[test]
fn create_replace_delete_and_snapshot_track_actual_files() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(dir.path()).unwrap();
    workspace
        .apply(&[FileEdit {
            path: "src/main.rs".into(),
            expected: MISSING.into(),
            operation: EditOperation::Create {
                content: "fn main() { println!(\"hello\"); }".into(),
            },
        }])
        .unwrap();
    let first = workspace.read("src/main.rs", 1000).unwrap();
    let before = workspace.fingerprints().unwrap();
    workspace
        .apply(&[FileEdit {
            path: "src/main.rs".into(),
            expected: first.hash,
            operation: EditOperation::Replace {
                old: "hello".into(),
                new: "你好".into(),
            },
        }])
        .unwrap();
    assert!(
        workspace
            .read("src/main.rs", 1000)
            .unwrap()
            .content
            .contains("你好")
    );
    assert_ne!(
        before["$workspace"],
        workspace.fingerprints().unwrap()["$workspace"]
    );
    let second = workspace.read("src/main.rs", 1000).unwrap();
    workspace
        .apply(&[FileEdit {
            path: "src/main.rs".into(),
            expected: second.hash,
            operation: EditOperation::Delete,
        }])
        .unwrap();
    assert!(!dir.path().join("src/main.rs").exists());
}

#[test]
fn whole_patch_is_validated_before_any_file_changes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a"), "one").unwrap();
    std::fs::write(dir.path().join("b"), "two").unwrap();
    let workspace = Workspace::open(dir.path()).unwrap();
    let hash = workspace.read("a", 100).unwrap().hash;
    let patch = [
        FileEdit {
            path: "a".into(),
            expected: hash,
            operation: EditOperation::Replace {
                old: "one".into(),
                new: "changed".into(),
            },
        },
        FileEdit {
            path: "b".into(),
            expected: "stale".into(),
            operation: EditOperation::Delete,
        },
    ];
    assert!(workspace.apply(&patch).is_err());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a")).unwrap(),
        "one"
    );
}

#[test]
fn workspace_rejects_parent_paths_and_external_symlinks() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("private"), "secret fixture").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    assert!(workspace.resolve("../outside").is_err());
    assert!(workspace.read("link/private", 100).is_err());
    assert!(workspace.resolve(".git/config").is_err());
}

#[test]
fn normal_development_is_automatic_but_publication_requires_authorization() {
    assert_eq!(policy::shell("cargo test"), Decision::Allow);
    assert_eq!(policy::shell("npm install"), Decision::Allow);
    for command in [
        "git push origin main",
        "npm publish",
        "echo ok && git push",
        "curl -X POST https://api.example.test/change",
    ] {
        assert!(
            matches!(policy::shell(command), Decision::Ask(_)),
            "{command}"
        );
    }
}

fn spec(dir: &std::path::Path, command: &str) -> ProcessSpec {
    ProcessSpec {
        command: command.into(),
        cwd: dir.into(),
        log: dir.join("output.log"),
        timeout: Duration::from_secs(10),
        profile: None,
    }
}

#[tokio::test]
async fn command_has_a_real_terminal_and_persistent_output() {
    let dir = tempfile::tempdir().unwrap();
    let (events, _) = mpsc::unbounded_channel();
    let result = process::run(
        spec(dir.path(), "test -t 1 && printf 'terminal-ok'"),
        CancellationToken::new(),
        events,
    )
    .await
    .unwrap();
    assert_eq!(result.code, Some(0));
    assert!(result.output.contains("terminal-ok"));
    assert!(
        std::fs::read_to_string(dir.path().join("output.log"))
            .unwrap()
            .contains("terminal-ok")
    );
}

#[tokio::test]
async fn cancelled_process_has_exited_before_run_returns() {
    let dir = tempfile::tempdir().unwrap();
    let (events, mut received) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();
    let child = cancel.clone();
    let run = tokio::spawn(process::run(
        spec(dir.path(), "printf ready; sleep 30; touch obsolete"),
        child,
        events,
    ));
    let pid = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(ProcessEvent::Started { pid }) = received.recv().await {
                break pid;
            }
        }
    })
    .await
    .unwrap();
    cancel.cancel();
    let output = tokio::time::timeout(Duration::from_secs(5), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(output.cancelled);
    assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
    assert!(!dir.path().join("obsolete").exists());
}

#[tokio::test]
async fn old_requirement_version_cannot_authorize_a_write() {
    let dir = tempfile::tempdir().unwrap();
    let goal = Arc::new(tokio::sync::RwLock::new(Goal::new("write JSON")));
    let host = ToolHost::new(
        Workspace::open(dir.path()).unwrap(),
        goal.clone(),
        dir.path().join("logs"),
    );
    let scope = Scope {
        requirements: goal.read().await.active_versions(),
        ..Scope::default()
    };
    let (events, _) = mpsc::unbounded_channel();
    let call = ToolCall {
        id: "new".into(),
        name: "apply_patch".into(),
        arguments: json!({"edits":[
            {"path":"first.txt","expected":"missing","operation":"create","content":"first"}
        ]}),
    };
    host.execute(
        call,
        scope.clone(),
        CancellationToken::new(),
        events.clone(),
    )
    .await
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("first.txt")).unwrap(),
        "first"
    );
    goal.write()
        .await
        .revise(Change {
            source: "use SQLite".into(),
            updates: BTreeMap::from([("r1".into(), "write SQLite".into())]),
            additions: vec![],
            replacement: false,
        })
        .unwrap();
    let call = ToolCall {
        id: "old".into(),
        name: "apply_patch".into(),
        arguments: json!({"edits":[
            {"path":"obsolete.txt","expected":"missing","operation":"create","content":"JSON"}
        ]}),
    };
    assert!(
        host.execute(call, scope, CancellationToken::new(), events)
            .await
            .unwrap_err()
            .to_string()
            .contains("stale")
    );
    assert!(!dir.path().join("obsolete.txt").exists());
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn service_can_write_build_output_but_not_source() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("source.rs"), "protected").unwrap();
    std::fs::create_dir(root.join("generated")).unwrap();
    let profile = weft::tools::service::profile(&root, &[root.join("generated")]).unwrap();
    let mut process = spec(
        &root,
        "printf nope > source.rs; printf allowed > generated/result",
    );
    process.profile = Some(profile);
    let (events, _) = mpsc::unbounded_channel();
    process::run(process, CancellationToken::new(), events)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("source.rs")).unwrap(),
        "protected"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("generated/result")).unwrap(),
        "allowed"
    );
}

#[tokio::test]
async fn source_search_reports_actual_locations_and_hashes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("module.rs"),
        "fn one() {}\nfn target() {}\n",
    )
    .unwrap();
    let goal = Arc::new(tokio::sync::RwLock::new(Goal::new("find target")));
    let host = ToolHost::new(
        Workspace::open(dir.path()).unwrap(),
        goal,
        dir.path().join("logs"),
    );
    let (events, _) = mpsc::unbounded_channel();
    let result = host
        .execute(
            ToolCall {
                id: "s".into(),
                name: "search_text".into(),
                arguments: json!({"pattern":"target"}),
            },
            Scope::default(),
            CancellationToken::new(),
            events,
        )
        .await
        .unwrap();
    assert_eq!(result.data["matches"][0]["path"], "module.rs");
    assert_eq!(result.data["matches"][0]["line"], 2);
    assert!(!result.files["module.rs"].is_empty());
    assert!(
        result.files.contains_key("$workspace"),
        "negative search conclusions need a repository fingerprint too"
    );
}

#[tokio::test]
async fn agents_own_edits_refresh_prior_workspace_observations() {
    let dir = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    let goal = Arc::new(tokio::sync::RwLock::new(Goal::new("create two files")));
    let workspace = Workspace::open(dir.path()).unwrap();
    let mut scope = Scope {
        requirements: goal.read().await.active_versions(),
        observed: workspace.fingerprints().unwrap(),
        ..Scope::default()
    };
    let host = ToolHost::new(workspace, goal, logs.path().to_owned());
    let (events, _) = mpsc::unbounded_channel();
    for path in ["one", "two"] {
        let call = ToolCall {
            id: path.into(),
            name: "apply_patch".into(),
            arguments: json!({"edits":[{"path":path,"expected":"missing","operation":"create","content":"ok"}]}),
        };
        let result = host
            .execute(
                call,
                scope.clone(),
                CancellationToken::new(),
                events.clone(),
            )
            .await
            .unwrap();
        scope.observed.extend(result.files);
    }
    assert!(dir.path().join("two").exists());
}
#[tokio::test]
async fn independent_checks_can_execute_but_cannot_modify_source() {
    use std::sync::Arc;
    use weft::tools::{Scope, ToolHost};
    use weft::{domain::Goal, model::ToolCall};
    let root = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("answer.py"), "value = 42\n").unwrap();
    let goal = Goal::new("Deliver answer.py");
    let scope = Scope {
        requirements: goal.active_versions(),
        readonly: true,
        ..Scope::default()
    };
    let host = ToolHost::new(
        Workspace::open(root.path()).unwrap(),
        Arc::new(tokio::sync::RwLock::new(goal)),
        logs.path().to_owned(),
    );
    let (events, _) = tokio::sync::mpsc::unbounded_channel();
    for (command, expected) in [
        (
            "python3 -B -c 'from answer import value; assert value == 42'",
            true,
        ),
        ("printf broken > answer.py", false),
    ] {
        let result = host
            .execute(
                ToolCall {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: "check".into(),
                    arguments: serde_json::json!({"command":command}),
                },
                scope.clone(),
                CancellationToken::new(),
                events.clone(),
            )
            .await
            .unwrap();
        assert_eq!(result.success, expected);
    }
    assert_eq!(
        std::fs::read_to_string(root.path().join("answer.py")).unwrap(),
        "value = 42\n"
    );
}

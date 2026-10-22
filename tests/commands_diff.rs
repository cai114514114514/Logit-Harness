use loh::{
    commands::{self, Outcome, Service},
    config::Paths,
};
use std::{path::Path, process::Command};

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("/usr/bin/git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn diff(root: &Path) -> String {
    let paths = Paths::new(root.join("unused-state"));
    let Outcome::Local { text, .. } = Service::new(&paths, root, None)
        .execute(commands::parse("/diff").unwrap())
        .unwrap()
    else {
        panic!("local diff")
    };
    text
}

#[test]
fn diff_command_includes_staged_unstaged_and_untracked_in_unborn_and_committed_repositories() {
    for committed in [false, true] {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "--quiet"]);
        if committed {
            git(
                root.path(),
                &[
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.test",
                    "commit",
                    "--allow-empty",
                    "-qm",
                    "initial",
                ],
            );
        }
        std::fs::write(root.path().join("staged.txt"), "STAGED_SENTINEL\n").unwrap();
        git(root.path(), &["add", "staged.txt"]);
        std::fs::write(root.path().join("untracked.txt"), "untracked\n").unwrap();
        let staged = diff(root.path());
        assert!(
            staged.contains("STAGED_SENTINEL"),
            "staged content missing: {staged}"
        );
        assert!(staged.contains("untracked.txt"));
        std::fs::write(root.path().join("staged.txt"), "UNSTAGED_SENTINEL\n").unwrap();
        let both = diff(root.path());
        assert!(both.contains("STAGED_SENTINEL") && both.contains("UNSTAGED_SENTINEL"));
    }
}

#[test]
fn diff_command_never_runs_configured_helpers_or_changes_original_index() {
    use std::os::unix::fs::PermissionsExt;
    for helper in ["filter", "process", "fsmonitor", "textconv", "external"] {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "--quiet"]);
        std::fs::write(root.path().join("file.txt"), "before\n").unwrap();
        git(root.path(), &["add", "file.txt"]);
        let marker = outside.path().join("executed");
        let script = outside.path().join("helper");
        std::fs::write(
            &script,
            format!("#!/bin/sh\ntouch '{}'\ncat\n", marker.display()),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let script = script.to_str().unwrap();
        match helper {
            "filter" => {
                std::fs::write(root.path().join(".gitattributes"), "*.txt filter=trap\n").unwrap();
                git(root.path(), &["config", "filter.trap.clean", script]);
            }
            "process" => {
                std::fs::write(root.path().join(".gitattributes"), "*.txt filter=trap\n").unwrap();
                git(root.path(), &["config", "filter.trap.process", script]);
            }
            "fsmonitor" => git(root.path(), &["config", "core.fsmonitor", script]),
            "textconv" => {
                std::fs::write(root.path().join(".gitattributes"), "*.txt diff=trap\n").unwrap();
                git(root.path(), &["config", "diff.trap.textconv", script]);
            }
            _ => git(root.path(), &["config", "diff.external", script]),
        }
        std::fs::write(root.path().join("file.txt"), "after\n").unwrap();
        let index = std::fs::read(root.path().join(".git/index")).unwrap();
        let result = diff(root.path());
        assert!(!marker.exists(), "configured {helper} executed");
        assert!(result.contains("+after"));
        assert_eq!(
            std::fs::read(root.path().join(".git/index")).unwrap(),
            index
        );
    }
}

#[test]
fn diff_command_caps_output_and_declares_inspection_limits() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "--quiet"]);
    std::fs::write(root.path().join("large.txt"), "long-line\n".repeat(50_000)).unwrap();
    git(root.path(), &["add", "large.txt"]);
    let text = diff(root.path());
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["staged"]["truncated"], true);
    assert!(value["staged"]["text"].as_str().unwrap().len() <= 256 * 1024);
    assert!(value["limits"].as_str().unwrap().contains("submodule"));
}

#[test]
fn diff_command_rejects_oversized_metadata_and_unsupported_formats() {
    for unsupported in [false, true] {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "--quiet"]);
        if unsupported {
            git(
                root.path(),
                &["config", "extensions.refStorage", "reftable"],
            );
        } else {
            std::fs::File::create(root.path().join(".git/index"))
                .unwrap()
                .set_len(33 * 1024 * 1024)
                .unwrap();
        }
        let paths = Paths::new(root.path().join("unused-state"));
        let error = Service::new(&paths, root.path(), None)
            .execute(commands::parse("/diff").unwrap())
            .unwrap_err();
        assert!(
            error.to_string().contains(if unsupported {
                "does not support"
            } else {
                "exceeds inspection limit"
            }),
            "{error}"
        );
    }
}

#[tokio::test]
async fn diff_command_can_run_from_an_async_caller_without_nested_runtime_panics() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "--quiet"]);
    std::fs::write(root.path().join("untracked.txt"), "fixture").unwrap();
    assert!(diff(root.path()).contains("untracked.txt"));
}

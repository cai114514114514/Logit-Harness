use loh::{
    domain::Goal,
    model::ToolCall,
    tools::{
        AssignmentLease, Scope, ToolHost, WriteGrant,
        files::{MISSING, Workspace},
    },
};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use tokio::sync::{RwLock, mpsc};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn queued_patch_rechecks_owner_after_the_mutation_lock() {
    for change in ["revoke", "reassign", "cancel", "user_edit"] {
        let dir = tempfile::tempdir().unwrap();
        let goal = Goal::new("build");
        let host = Arc::new(ToolHost::new(
            Workspace::open(dir.path()).unwrap(),
            Arc::new(RwLock::new(goal.clone())),
            dir.path().join("logs"),
        ));
        let lease = AssignmentLease {
            id: "assignment".into(),
            version: 1,
            actor: "worker".into(),
        };
        host.assignments.write().await.insert(
            lease.id.clone(),
            WriteGrant {
                lease: lease.clone(),
                writable: BTreeSet::from(["src/".into()]),
            },
        );
        let scope = Scope {
            epoch: goal.epoch,
            requirements: goal.active_versions(),
            assignment: Some(lease),
            ..Scope::default()
        };
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let lock = host.mutation.lock().await;
        let execute = {
            let host = host.clone();
            tokio::spawn(async move {
                host.execute(ToolCall { id:"patch".into(), name:"apply_patch".into(), arguments:json!({"edits":[{"path":"src/new.rs","expected":MISSING,"operation":"create","content":"created"}]}) },scope,token,mpsc::unbounded_channel().0).await
            })
        };
        tokio::task::yield_now().await;
        match change {
            "revoke" => host.assignments.write().await.clear(),
            "reassign" => {
                host.assignments
                    .write()
                    .await
                    .get_mut("assignment")
                    .unwrap()
                    .lease
                    .version += 1
            }
            "cancel" => cancel.cancel(),
            "user_edit" => {
                std::fs::create_dir(dir.path().join("src")).unwrap();
                std::fs::write(dir.path().join("src/new.rs"), "user content").unwrap();
            }
            _ => unreachable!(),
        }
        drop(lock);
        assert!(execute.await.unwrap().is_err());
        if change == "user_edit" {
            assert_eq!(
                std::fs::read_to_string(dir.path().join("src/new.rs")).unwrap(),
                "user content"
            );
        } else {
            assert!(!dir.path().join("src/new.rs").exists());
        }
    }
}

#[tokio::test]
async fn assignment_paths_cover_new_files_without_allowing_traversal_or_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::create_dir(dir.path().join("other")).unwrap();
    std::os::unix::fs::symlink("../other", dir.path().join("src/link")).unwrap();
    let goal = Goal::new("build");
    let host = ToolHost::new(
        Workspace::open(dir.path()).unwrap(),
        Arc::new(RwLock::new(goal.clone())),
        dir.path().join("logs"),
    );
    let lease = AssignmentLease {
        id: "one".into(),
        version: 1,
        actor: "worker".into(),
    };
    *host.assignments.write().await = BTreeMap::from([(
        lease.id.clone(),
        WriteGrant {
            lease: lease.clone(),
            writable: BTreeSet::from(["src/".into()]),
        },
    )]);
    for (path, allowed) in [
        ("other/new.rs", false),
        ("src/../other/new.rs", false),
        ("src/link/new.rs", false),
        ("src/new.rs", true),
    ] {
        let result=host.execute(ToolCall {id:"patch".into(),name:"apply_patch".into(),arguments:json!({"edits":[{"path":path,"expected":MISSING,"operation":"create","content":"created"}]})},Scope {requirements:goal.active_versions(),assignment:Some(lease.clone()),..Scope::default()},CancellationToken::new(),mpsc::unbounded_channel().0).await;
        assert_eq!(result.is_ok(), allowed, "{path}: {result:?}");
    }
}

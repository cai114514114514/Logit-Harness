use loh::tools::identity::Identity;
use std::time::Duration;

#[tokio::test]
async fn a_reused_process_identity_does_not_authorize_signals() {
    let mut child = tokio::process::Command::new("python3")
        .args(["-c", "import time; time.sleep(30)"])
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let owned = Identity::capture(child.id().unwrap()).unwrap().unwrap();
    let mut stale = owned.clone();
    stale.started = stale.started.saturating_sub(1);
    let result = stale.stop_group().await;
    let untouched = child.try_wait().unwrap().is_none();
    owned.stop_group().await.unwrap();
    child.wait().await.unwrap();
    assert!(result.unwrap_err().to_string().contains("reused PID"));
    assert!(untouched);
}

#[tokio::test]
async fn termination_kills_and_confirms_descendants_after_the_leader_exits() {
    let root = tempfile::tempdir().unwrap();
    let mut child = tokio::process::Command::new("python3")
        .arg(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/stubborn_group.py"),
        )
        .current_dir(root.path())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let identity = Identity::capture(child.id().unwrap()).unwrap().unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !root.path().join("descendant.pid").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let descendant = std::fs::read_to_string(root.path().join("descendant.pid"))
        .unwrap()
        .parse()
        .unwrap();
    let result = identity.stop_group().await;
    let alive = Identity::capture(descendant).unwrap().is_some();
    // Cleanup must run even for the expected RED outcome.
    unsafe {
        libc::kill(-(identity.group as i32), libc::SIGKILL);
    }
    child.wait().await.unwrap();
    assert!(result.is_ok(), "{result:?}");
    assert!(
        !alive,
        "settlement cannot leave a TERM-resistant writer alive"
    );
}

#[tokio::test]
async fn recovery_identifies_orphaned_descendants_from_persisted_ownership() {
    use loh::tools::group::{Group, OWNER_ENV};
    let root = tempfile::tempdir().unwrap();
    let token = uuid::Uuid::new_v4().to_string();
    let mut child = tokio::process::Command::new("python3")
        .arg(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/stubborn_group.py"),
        )
        .env(OWNER_ENV, &token)
        .current_dir(root.path())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let leader = Identity::capture(child.id().unwrap()).unwrap().unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !root.path().join("descendant.pid").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let descendant = std::fs::read_to_string(root.path().join("descendant.pid"))
        .unwrap()
        .parse()
        .unwrap();
    std::fs::write(root.path().join("leader-exit"), "exit").unwrap();
    child.wait().await.unwrap();
    assert!(Identity::capture(leader.pid).unwrap().is_none());
    let wrong = Group {
        leader: leader.clone(),
        token: Some("unrelated".into()),
    }
    .stop()
    .await;
    let untouched = Identity::capture(descendant).unwrap().is_some();
    let saved = serde_json::to_vec(&Group {
        leader: leader.clone(),
        token: Some(token),
    })
    .unwrap();
    let recovered: Group = serde_json::from_slice(&saved).unwrap();
    let outcome = recovered.stop().await;
    let alive = Identity::capture(descendant).unwrap().is_some();
    unsafe {
        libc::kill(-(leader.group as i32), libc::SIGKILL);
    }
    assert!(
        wrong.is_err() && untouched,
        "unknown ownership must not authorize a signal"
    );
    assert!(outcome.is_ok(), "{outcome:?}");
    assert!(!alive);
}

#[tokio::test]
async fn normal_command_completion_confirms_all_background_writers_exited() {
    use loh::tools::process::{ProcessRecord, ProcessSpec, run};
    let root = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("leader-exit"), "exit").unwrap();
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stubborn_group.py");
    let (events, _) = tokio::sync::mpsc::unbounded_channel();
    let output = run(
        ProcessSpec {
            command: format!("python3 '{}'", script.display()),
            cwd: root.path().into(),
            log: logs.path().join("run.log"),
            timeout: Duration::from_secs(10),
            profile: None,
        },
        tokio_util::sync::CancellationToken::new(),
        events,
    )
    .await;
    let descendant = std::fs::read_to_string(root.path().join("descendant.pid"))
        .unwrap()
        .parse()
        .unwrap();
    let alive = Identity::capture(descendant).unwrap().is_some();
    if alive {
        unsafe {
            libc::kill(descendant as i32, libc::SIGKILL);
        }
    }
    assert!(output.is_ok(), "{output:?}");
    assert!(!alive);
    let record: ProcessRecord =
        serde_json::from_slice(&std::fs::read(logs.path().join("run.process.json")).unwrap())
            .unwrap();
    assert!(record.output.is_some());
    assert!(record.group_token.is_some());
}

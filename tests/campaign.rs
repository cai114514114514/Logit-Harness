use loh::eval::{manifest::Frozen, schedule::schedule, trial::Trial};
use std::path::Path;

#[test]
fn schedule_is_deterministic_paired_and_balanced() {
    let frozen =
        Frozen::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("eval/registered.json")).unwrap();
    let first = schedule(&frozen, "development").unwrap();
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(schedule(&frozen, "development").unwrap()).unwrap()
    );
    assert_eq!(first.len(), 180);
    for pair in first.chunks(5) {
        assert!(
            pair.iter()
                .all(|item| item.task == pair[0].task && item.repetition == pair[0].repetition)
        );
        assert_eq!(
            pair.iter()
                .map(|item| format!("{:?}", item.condition))
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            5
        );
    }
    assert_eq!(schedule(&frozen, "holdout").unwrap().len(), 360);
}

#[tokio::test]
async fn preparing_again_reuses_the_trial_and_preserves_generated_work() {
    let project = Path::new(env!("CARGO_MANIFEST_DIR"));
    let frozen = Frozen::load(&project.join("eval/registered.json")).unwrap();
    let root = tempfile::tempdir().unwrap();
    let item = schedule(&frozen, "development")
        .unwrap()
        .into_iter()
        .find(|item| item.task == "dev-ledger")
        .unwrap();
    let first = Trial::prepare(
        root.path(),
        &project.join("eval/corpus"),
        &frozen,
        &item,
        "fixture-binary",
    )
    .await
    .unwrap();
    std::fs::write(
        first.workspace.join("main.py"),
        "# user/agent work must survive\n",
    )
    .unwrap();
    let resumed = Trial::prepare(
        root.path(),
        &project.join("eval/corpus"),
        &frozen,
        &item,
        "fixture-binary",
    )
    .await
    .unwrap();
    assert_eq!(resumed.session, first.session);
    assert_eq!(
        std::fs::read_to_string(resumed.workspace.join("main.py")).unwrap(),
        "# user/agent work must survive\n"
    );
    assert!(resumed.workspace.join("verify.py").is_file());
    assert!(resumed.workspace.join("NOTES.txt").is_file());
}

#[tokio::test]
async fn correction_is_once_only_and_preserves_evidence_of_protected_file_changes() {
    use loh::{
        domain::{EventData, SessionEvent},
        eval::correction::Correction,
        runtime::runner::Observer,
        store::Store,
    };
    use std::sync::Arc;
    let project = Path::new(env!("CARGO_MANIFEST_DIR"));
    let frozen = Frozen::load(&project.join("eval/registered.json")).unwrap();
    let root = tempfile::tempdir().unwrap();
    let entry = schedule(&frozen, "development")
        .unwrap()
        .into_iter()
        .find(|entry| entry.task == "dev-ledger")
        .unwrap();
    let trial = Trial::prepare(
        root.path(),
        &project.join("eval/corpus"),
        &frozen,
        &entry,
        "fixture",
    )
    .await
    .unwrap();
    let store = Arc::new(Store::open(&root.path().join("state.db")).unwrap());
    std::fs::write(trial.workspace.join("main.py"), "print('implementation')\n").unwrap();
    std::fs::write(
        trial.workspace.join("verify.py"),
        "print('agent weakened verification')\n",
    )
    .unwrap();
    let observer = Correction::new(trial.spec().unwrap(), trial.session.clone(), store.clone());
    let event = |sequence, data| SessionEvent {
        sequence,
        session: trial.session.clone(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        data,
    };
    observer
        .on_event(&event(
            1,
            EventData::ActionStarted {
                id: "write".into(),
                tool: "apply_patch".into(),
                arguments: serde_json::json!({}),
            },
        ))
        .unwrap();
    observer
        .on_event(&event(
            2,
            EventData::ActionFinished {
                id: "write".into(),
                result: serde_json::json!({}),
                success: true,
            },
        ))
        .unwrap();
    let receipt = observer.receipt().unwrap();
    assert!(receipt.control.is_some());
    assert!(receipt.violations.contains("verify.py"));
    assert_eq!(
        std::fs::read_to_string(trial.workspace.join("verify.py")).unwrap(),
        trial.spec().unwrap().final_script
    );
    let resumed = Correction::new(trial.spec().unwrap(), trial.session.clone(), store.clone());
    resumed.inject("recovery", 3).unwrap();
    assert_eq!(store.queued(&trial.session, 0).unwrap().len(), 1);
    assert_eq!(resumed.receipt().unwrap().control, receipt.control);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_daemon_trial_grades_rescues_and_resumes_without_duplicate_model_calls() {
    use loh::{
        config::{Config, Paths},
        domain::EventData,
        eval::run::{Engine, Study},
        runtime::{
            money::{Money, Prices, RMB},
            state::{Campaign, Session},
        },
        store::Store,
    };
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let counted = counted.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                let mut buffer = [0; 8192];
                loop {
                    let count = socket.read(&mut buffer).await.unwrap();
                    if count == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..end]);
                        let length = header
                            .lines()
                            .find_map(|line| {
                                line.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|value| value.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                counted.fetch_add(1, Ordering::SeqCst);
                let event = serde_json::json!({"type":"response.completed","response":{"status":"completed","model":"fixture","usage":{"input_tokens":10,"output_tokens":5},"output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"fixture claims completion without implementation"}]}]}});
                let body = format!("data: {event}\n\n");
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            });
        }
    });
    let project = Path::new(env!("CARGO_MANIFEST_DIR"));
    let frozen = Arc::new(Frozen::load(&project.join("eval/registered.json")).unwrap());
    let root = tempfile::tempdir().unwrap();
    let paths = Paths::new(root.path().join("state"));
    paths.prepare().unwrap();
    let prices = Prices {
        input: 2000,
        cached: 40,
        output: 8000,
        search: 5_000_000,
        fetch: 5_000_000,
    };
    let campaign = Campaign {
        name: "fixture-only".into(),
        limit: 50 * RMB,
        prices: prices.clone(),
    };
    let config = Config {
        base_url: base,
        campaign: Some(campaign.clone()),
        ..Default::default()
    };
    std::fs::write(
        paths.home.join("config.toml"),
        toml::to_string(&config).unwrap(),
    )
    .unwrap();
    let binary = std::path::PathBuf::from(env!("CARGO_BIN_EXE_loh"));
    let hash = loh::tools::files::fingerprint(&binary).unwrap();
    let entries = schedule(&frozen, "development").unwrap();
    let entry = entries
        .iter()
        .find(|entry| {
            entry.task == "dev-ledger"
                && entry.condition == loh::runtime::condition::Condition::Prompt
        })
        .unwrap();
    let output = root.path().join("study");
    let trial = Trial::prepare(&output, &project.join("eval/corpus"), &frozen, entry, &hash)
        .await
        .unwrap();
    let store = Store::open(&paths.database()).unwrap();
    let mut state: Session = serde_json::from_slice(
        &std::fs::read(trial.directory.join("initial-session.json")).unwrap(),
    )
    .unwrap();
    state.campaign = Some(campaign);
    store
        .commit(
            &state.id,
            &state,
            EventData::Created {
                objective: state.goal.original.clone(),
                workspace: state.workspace.display().to_string(),
            },
        )
        .unwrap();
    let mut daemon = tokio::process::Command::new(&binary)
        .args(["--home", paths.home.to_str().unwrap(), "_serve", &state.id])
        .env("DEEPSEEK_API_KEY", "fixture-key")
        .kill_on_drop(true)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if tokio::net::UnixStream::connect(paths.socket(&state.id).unwrap())
                .await
                .is_ok()
            {
                break;
            }
            assert!(daemon.try_wait().unwrap().is_none());
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let study = Study {
        created: chrono::Utc::now().to_rfc3339(),
        corpus_hash: frozen.digest.clone(),
        source_hash: "fixture".into(),
        binary_hash: hash,
        binary,
        split: "development".into(),
        account: "fixture-only".into(),
        home: paths.home.clone(),
        schedule: entries,
    };
    std::fs::write(
        output.join("study.json"),
        serde_json::to_vec(&study).unwrap(),
    )
    .unwrap();
    let engine = Engine {
        paths: paths.clone(),
        config,
        corpus: project.join("eval/corpus"),
        root: output.canonicalize().unwrap(),
        frozen: frozen.clone(),
        study,
        money: Arc::new(
            Money::open(
                &paths.home.join("money.sqlite"),
                "fixture-only",
                50 * RMB,
                &prices,
            )
            .unwrap(),
        ),
    };
    let result = tokio::time::timeout(Duration::from_secs(20), engine.execute(trial))
        .await
        .unwrap()
        .unwrap();
    assert!(result.finished.is_some());
    assert_eq!(result.rescues, 2);
    assert_eq!(result.attempts.len(), 3);
    assert!(result.attempts.iter().all(|attempt| !attempt.passed));
    let count = calls.load(Ordering::SeqCst);
    assert!(count >= 4);
    engine
        .execute(Trial::load(&result.directory).unwrap())
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), count);
    loh::eval::report::write(&output, &frozen, &paths).unwrap();
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["finished"], 1);
    assert_eq!(report["not_finished"], 179);
    assert_eq!(report["human_review"], "pending");
    assert!(engine.money.session_balance(&result.session).unwrap().used > 0);
    tokio::time::timeout(Duration::from_secs(5), daemon.wait())
        .await
        .unwrap()
        .unwrap();
    rusqlite::Connection::open(paths.database())
        .unwrap()
        .execute("DELETE FROM sessions WHERE id=?1", [&result.session])
        .unwrap();
    let mut missing = result.clone();
    missing.finished = None;
    assert!(
        engine
            .execute(missing)
            .await
            .unwrap_err()
            .to_string()
            .contains("missing saved session")
    );
    assert_eq!(calls.load(Ordering::SeqCst), count);
    server.abort();
}

use super::{correction::Correction, files, manifest::Frozen, trial::Trial};
use crate::store::Store;
use serde_json::Value;
use std::{path::Path, sync::Arc};

pub async fn evaluate(
    corpus: &Path,
    frozen: &Frozen,
    trial: &Trial,
    store: Arc<Store>,
    sequence: u64,
) -> anyhow::Result<(Value, std::path::PathBuf)> {
    frozen.verify(corpus)?;
    let directory = trial.directory.join("acceptance");
    std::fs::create_dir_all(&directory)?;
    let path = directory.join(format!("{sequence}.json"));
    if path.exists() {
        return Ok((serde_json::from_slice(&std::fs::read(&path)?)?, path));
    }
    // Compile/run a source snapshot: the external grader must not create Cargo.lock or other
    // generated files in the agent's live tree and thereby change its next attempt.
    let archive = directory.join(format!("{sequence}-source"));
    let snapshot = archive.join("workspace");
    if !archive.exists() {
        let temporary = tempfile::Builder::new()
            .prefix(".snapshot-")
            .tempdir_in(&directory)?;
        let staging = temporary.path().join("workspace");
        std::fs::create_dir(&staging)?;
        let source = crate::tools::files::Workspace::open(&trial.workspace)?;
        let fingerprints = source.fingerprints()?;
        for name in fingerprints
            .keys()
            .filter(|name| name.as_str() != crate::tools::files::WORKSPACE_FINGERPRINT_KEY)
        {
            let input = source.resolve(name)?;
            if !input.is_file() {
                continue;
            }
            let target = staging.join(name);
            std::fs::create_dir_all(target.parent().unwrap())?;
            std::fs::copy(input, &target)?;
            std::fs::File::open(target)?.sync_all()?;
        }
        files::save(&temporary.path().join("fingerprints.json"), &fingerprints)?;
        let prepared = temporary.keep();
        std::fs::rename(prepared, &archive)?;
        std::fs::File::open(&directory)?.sync_all()?;
    }
    anyhow::ensure!(
        archive.join("fingerprints.json").is_file(),
        "incomplete acceptance snapshot requires reconciliation"
    );
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(90),
        tokio::process::Command::new("python3")
            .arg("-B")
            .arg(corpus.join("accept.py"))
            .arg(corpus.join("tasks").join(format!("{}.json", trial.task)))
            .arg(&snapshot)
            .arg("--build")
            .arg(trial.directory.join("build"))
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    anyhow::ensure!(
        result.status.success(),
        "external oracle failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let mut report: Value = serde_json::from_slice(&result.stdout)?;
    let receipt = Correction::new(trial.spec()?, trial.session.clone(), store).receipt()?;
    if !receipt.violations.is_empty() {
        report["groups"]["preserved"] = Value::Bool(false);
        report["passed"] = Value::Bool(false);
        report["protected_file_history"] = serde_json::to_value(receipt.violations)?;
        let satisfied = report["groups"]
            .as_object()
            .unwrap()
            .values()
            .filter(|value| **value == true)
            .count();
        report["fidelity"] = serde_json::json!(satisfied as f64 / 4.0);
    }
    files::save(&path, &report)?;
    Ok((report, path))
}

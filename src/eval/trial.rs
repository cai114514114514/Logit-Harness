use super::{
    files,
    manifest::{Frozen, Protocol, Task},
    schedule::Entry,
};
use crate::{
    domain::Fingerprints,
    runtime::{condition::Condition, state::Session},
    tools::files::Workspace,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Contains public correction material only, never private oracle checks.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Spec {
    pub directory: PathBuf,
    pub workspace: PathBuf,
    pub task: String,
    pub correction: String,
    pub trigger: String,
    pub initial: Fingerprints,
    pub initial_script: String,
    pub final_script: String,
    pub notes: String,
    pub protocol: Protocol,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attempt {
    pub sequence: u64,
    pub status: String,
    pub report: PathBuf,
    pub passed: bool,
    pub fidelity: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Feedback {
    pub key: String,
    pub command: crate::runtime::state::Control,
    pub control: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trial {
    pub id: String,
    pub task: String,
    pub split: String,
    pub condition: Condition,
    pub repetition: u32,
    pub session: String,
    pub directory: PathBuf,
    pub workspace: PathBuf,
    pub binary_hash: String,
    pub corpus_hash: String,
    pub started: Option<String>,
    pub finished: Option<String>,
    pub rescues: u32,
    pub attempts: Vec<Attempt>,
    pub error: Option<String>,
    pub feedback: Option<Feedback>,
}
impl Trial {
    pub async fn prepare(
        root: &Path,
        corpus: &Path,
        frozen: &Frozen,
        entry: &Entry,
        binary_hash: &str,
    ) -> anyhow::Result<Self> {
        std::fs::create_dir_all(root.join("trials"))?;
        let root = root.canonicalize()?;
        let directory = Self::directory(&root, &entry.id);
        if directory.join("trial.json").exists() {
            let trial = Self::load(&directory)?;
            anyhow::ensure!(
                trial.corpus_hash == frozen.digest && trial.binary_hash == binary_hash,
                "saved trial belongs to a different corpus or executable"
            );
            anyhow::ensure!(
                trial.id == entry.id
                    && trial.task == entry.task
                    && trial.condition == entry.condition
                    && trial.repetition == entry.repetition
                    && trial.directory == directory
                    && trial.workspace == directory.join("workspace"),
                "saved trial does not match its frozen schedule entry"
            );
            return Ok(trial);
        }
        anyhow::ensure!(
            !directory.exists(),
            "refusing to replace an incomplete/unrelated trial directory"
        );
        let task = frozen
            .tasks
            .iter()
            .find(|task| task.id == entry.task)
            .ok_or_else(|| anyhow::anyhow!("task missing from frozen corpus"))?;
        let temporary = tempfile::Builder::new()
            .prefix(".prepare-")
            .tempdir_in(root.join("trials"))?;
        let working = temporary.path().join("workspace");
        std::fs::create_dir(&working)?;
        if task.kind == "existing" {
            copy_tree(&corpus.join("fixtures").join(&task.id), &working)?;
        }
        let initial_script = public_script(corpus, task, &working, false).await?;
        let final_script = public_script(corpus, task, &working, true).await?;
        files::write(&working.join("verify.py"), initial_script.as_bytes())?;
        files::write(&working.join("NOTES.txt"), task.notes.as_bytes())?;
        let workspace = directory.join("workspace");
        let spec = Spec {
            directory: directory.clone(),
            workspace: workspace.clone(),
            task: task.id.clone(),
            correction: task.correction.clone(),
            trigger: task.trigger.clone(),
            initial: Workspace::open(&working)?.fingerprints()?,
            initial_script,
            final_script,
            notes: task.notes.clone(),
            protocol: frozen.protocol.clone(),
        };
        let mut session = Session::new(
            workspace.clone(),
            &task.prompt(),
            frozen.protocol.tokens,
            frozen.protocol.seconds,
        );
        session.condition = entry.condition;
        session.evaluation = Some(spec.clone());
        let trial = Self {
            id: entry.id.clone(),
            task: entry.task.clone(),
            split: task.split.clone(),
            condition: entry.condition,
            repetition: entry.repetition,
            session: session.id.clone(),
            directory: directory.clone(),
            workspace,
            binary_hash: binary_hash.into(),
            corpus_hash: frozen.digest.clone(),
            started: None,
            finished: None,
            rescues: 0,
            attempts: vec![],
            error: None,
            feedback: None,
        };
        files::save(&temporary.path().join("spec.json"), &spec)?;
        files::save(&temporary.path().join("initial-session.json"), &session)?;
        files::save(&temporary.path().join("trial.json"), &trial)?;
        let prepared = temporary.keep();
        std::fs::rename(&prepared, &directory)?;
        std::fs::File::open(root.join("trials"))?.sync_all()?;
        Ok(trial)
    }
    pub fn load(directory: &Path) -> anyhow::Result<Self> {
        Ok(serde_json::from_slice(&std::fs::read(
            directory.join("trial.json"),
        )?)?)
    }
    pub fn directory(root: &Path, id: &str) -> PathBuf {
        root.join("trials")
            .join(&blake3::hash(id.as_bytes()).to_hex()[..24])
    }
    pub fn save(&self) -> anyhow::Result<()> {
        files::save(&self.directory.join("trial.json"), self)
    }
    pub fn spec(&self) -> anyhow::Result<Spec> {
        Ok(serde_json::from_slice(&std::fs::read(
            self.directory.join("spec.json"),
        )?)?)
    }
}

async fn public_script(
    corpus: &Path,
    task: &Task,
    workspace: &Path,
    final_phase: bool,
) -> anyhow::Result<String> {
    let mut command = tokio::process::Command::new("python3");
    command
        .arg("-B")
        .arg(corpus.join("accept.py"))
        .arg(corpus.join("tasks").join(format!("{}.json", task.id)))
        .arg(workspace)
        .arg("--public");
    if !final_phase {
        command.arg("--initial");
    }
    let result = command.output().await?;
    anyhow::ensure!(
        result.status.success(),
        "public oracle preparation failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(String::from_utf8(result.stdout)?)
}
fn copy_tree(source: &Path, target: &Path) -> anyhow::Result<()> {
    for entry in walkdir::WalkDir::new(source).follow_links(false) {
        let entry = entry?;
        anyhow::ensure!(
            !entry.file_type().is_symlink(),
            "fixture contains a symlink"
        );
        let path = target.join(entry.path().strip_prefix(source)?);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(path)?;
        } else {
            std::fs::copy(entry.path(), path)?;
        }
    }
    Ok(())
}

use super::{
    correction::Correction,
    files, grade,
    manifest::Frozen,
    schedule::{self, Entry},
    trial::{Attempt, Trial},
};
use crate::{
    config::{Config, Paths},
    domain::{EventData, Status},
    runtime::{
        daemon,
        money::{Money, RMB},
        state::{Control, Session},
    },
    store::Store,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

#[derive(Clone, Debug, clap::Args)]
pub struct Options {
    #[arg(long, default_value = "eval/corpus")]
    pub corpus: PathBuf,
    #[arg(long, default_value = "eval/registered.json")]
    pub frozen: PathBuf,
    #[arg(long, default_value = "artifacts/evaluation/development")]
    pub output: PathBuf,
    #[arg(long,default_value="development",value_parser=["development","holdout"])]
    pub split: String,
    #[arg(long, default_value_t = 2)]
    pub jobs: usize,
    /// Maximum unfinished trials to run this invocation; no completed trial is rerun.
    #[arg(long)]
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Study {
    pub created: String,
    pub corpus_hash: String,
    pub source_hash: String,
    pub binary_hash: String,
    pub binary: PathBuf,
    pub split: String,
    pub account: String,
    pub home: PathBuf,
    pub schedule: Vec<Entry>,
}

#[derive(Clone)]
pub struct Engine {
    pub paths: Paths,
    pub config: Config,
    pub corpus: PathBuf,
    pub root: PathBuf,
    pub frozen: Arc<Frozen>,
    pub study: Study,
    pub money: Arc<Money>,
}

pub async fn run(options: Options, paths: Paths, config: Config) -> anyhow::Result<()> {
    anyhow::ensure!((1..=4).contains(&options.jobs), "jobs must be 1..4");
    anyhow::ensure!(
        config.base_url.trim_end_matches('/') == "https://api.deepseek.com"
            && config.model == "deepseek-flash",
        "the registered study uses the official deepseek-flash endpoint"
    );
    anyhow::ensure!(
        std::env::var("DEEPSEEK_API_KEY").is_ok_and(|key| !key.trim().is_empty()),
        "DEEPSEEK_API_KEY is missing"
    );
    let campaign = config.campaign.as_ref().ok_or_else(|| {
        anyhow::anyhow!("evaluation requires the existing shared monetary campaign")
    })?;
    anyhow::ensure!(
        campaign.limit <= 50 * RMB,
        "study exceeds the authorized 50 RMB cap"
    );
    paths.prepare()?;
    std::fs::create_dir_all(&options.output)?;
    let root = options.output.canonicalize()?;
    let _owner = files::lock(&root.join("driver.lock"), false)?;
    let corpus = options.corpus.canonicalize()?;
    let frozen = Arc::new(Frozen::load(&options.frozen)?);
    frozen.verify(&corpus)?;
    let study = pin(&root, &frozen, &options.split, &paths, &campaign.name)?;
    let money = Arc::new(Money::open(
        &paths.home.join("money.sqlite"),
        &campaign.name,
        campaign.limit,
        &campaign.prices,
    )?);
    let engine = Engine {
        paths,
        config,
        corpus,
        root,
        frozen,
        study,
        money,
    };
    let mut pending = Vec::new();
    for entry in &engine.study.schedule {
        let directory = Trial::directory(&engine.root, &entry.id);
        if directory.join("trial.json").exists() && Trial::load(&directory)?.finished.is_some() {
            continue;
        }
        pending.push(entry.clone());
    }
    pending.truncate(options.limit.unwrap_or(usize::MAX));
    let mut jobs = tokio::task::JoinSet::new();
    let mut entries = pending.into_iter();
    let mut exhausted = false;
    loop {
        while jobs.len() < options.jobs && !exhausted {
            let Some(entry) = entries.next() else {
                break;
            };
            let directory = Trial::directory(&engine.root, &entry.id);
            let started =
                directory.join("trial.json").exists() && Trial::load(&directory)?.started.is_some();
            let balance = engine.money.balance()?;
            let reserve = engine
                .config
                .campaign
                .as_ref()
                .unwrap()
                .prices
                .reserve(1_048_576, u64::from(engine.frozen.protocol.max_output));
            if !started
                && balance
                    .limit
                    .saturating_sub(balance.used)
                    .saturating_sub(balance.uncertain)
                    < reserve
            {
                exhausted = true;
                println!(
                    "{}",
                    serde_json::json!({"campaign":"budget_limited","balance":balance})
                );
                break;
            }
            let engine = engine.clone();
            jobs.spawn(async move {
                let trial = Trial::prepare(
                    &engine.root,
                    &engine.corpus,
                    &engine.frozen,
                    &entry,
                    &engine.study.binary_hash,
                )
                .await?;
                engine.execute(trial).await
            });
        }
        if jobs.is_empty() {
            break;
        }
        let result = jobs.join_next().await.unwrap()?;
        match result {
            Ok(trial) => println!(
                "{}",
                serde_json::json!({"trial":trial.id,"session":trial.session,"finished":trial.finished,"rescues":trial.rescues,"result":trial.attempts.last()})
            ),
            Err(error) => {
                println!(
                    "{}",
                    serde_json::json!({"campaign":"interrupted","error":error.to_string()})
                );
                exhausted = true;
            }
        }
    }
    super::report::write(&engine.root, &engine.frozen, &engine.paths)?;
    Ok(())
}

fn pin(
    root: &Path,
    frozen: &Frozen,
    split: &str,
    paths: &Paths,
    account: &str,
) -> anyhow::Result<Study> {
    let path = root.join("study.json");
    if path.exists() {
        let study: Study = serde_json::from_slice(&std::fs::read(path)?)?;
        anyhow::ensure!(
            study.corpus_hash == frozen.digest
                && study.split == split
                && study.account == account
                && study.home == paths.home,
            "study identity changed; use its original account, split and corpus"
        );
        anyhow::ensure!(
            files::hash(&study.binary)? == study.binary_hash,
            "pinned study executable changed"
        );
        anyhow::ensure!(
            files::hash(&std::env::current_exe()?)? == study.binary_hash,
            "resume this study using its pinned executable: {}",
            study.binary.display()
        );
        return Ok(study);
    }
    let binary = root.join("loh");
    anyhow::ensure!(
        !binary.exists(),
        "unrecorded study executable requires reconciliation"
    );
    std::fs::copy(std::env::current_exe()?, &binary)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700))?;
    let study = Study {
        created: chrono::Utc::now().to_rfc3339(),
        corpus_hash: frozen.digest.clone(),
        source_hash: env!("LOH_SOURCE_HASH").into(),
        binary_hash: files::hash(&binary)?,
        binary,
        split: split.into(),
        account: account.into(),
        home: paths.home.clone(),
        schedule: schedule::schedule(frozen, split)?,
    };
    files::save(&path, &study)?;
    Ok(study)
}

impl Engine {
    pub async fn execute(&self, mut trial: Trial) -> anyhow::Result<Trial> {
        if trial.finished.is_some() {
            return Ok(trial);
        }
        let result = self.drive(&mut trial).await;
        if let Err(error) = result {
            trial.error = Some(error.to_string());
            trial.save()?;
            return Err(error);
        }
        Ok(trial)
    }
    async fn drive(&self, trial: &mut Trial) -> anyhow::Result<()> {
        anyhow::ensure!(
            files::hash(&self.study.binary)? == trial.binary_hash,
            "trial executable no longer matches its pin"
        );
        let store = Arc::new(Store::open(&self.paths.database())?);
        if !store.has_session(&trial.session)? {
            anyhow::ensure!(
                trial.started.is_none(),
                "missing saved session for an already-started trial; refusing to recreate billed work"
            );
            let balance = self.money.session_balance(&trial.session)?;
            anyhow::ensure!(
                balance.used == 0 && balance.uncertain == 0 && balance.reserved == 0,
                "trial has prior charges but no saved session"
            );
            let mut session: Session = serde_json::from_slice(&std::fs::read(
                trial.directory.join("initial-session.json"),
            )?)?;
            session.campaign = self.config.campaign.clone();
            store.commit(
                &session.id,
                &session,
                EventData::Created {
                    objective: session.goal.original.clone(),
                    workspace: session.workspace.display().to_string(),
                },
            )?;
        }
        if trial.started.is_none() {
            trial.started = Some(chrono::Utc::now().to_rfc3339());
            trial.save()?;
        }
        let saved: Session = store.load(&trial.session)?;
        let settled_stop = saved.status == Status::Stopped
            && saved.actions.values().all(|action| !action.running)
            && saved.actors.values().all(|actor| actor.busy == 0);
        if !settled_stop {
            daemon::launch_with(&self.paths, &trial.session, &self.study.binary).await?;
        }
        let correction = Correction::new(trial.spec()?, trial.session.clone(), store.clone());
        let mut silence = tokio::time::Instant::now();
        let mut last_sequence = 0;
        let mut stopping = false;
        loop {
            let (state, sequence): (Session, u64) = store.snapshot(&trial.session)?;
            if let Some(feedback) = &mut trial.feedback {
                if feedback.control.is_none() {
                    feedback.control = Some(store.enqueue_once(
                        &trial.session,
                        &feedback.key,
                        &serde_json::to_value(&feedback.command)?,
                    )?);
                    trial.save()?;
                }
                if state.cursor < trial.feedback.as_ref().unwrap().control.unwrap() {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
                trial.feedback = None;
                trial.save()?;
            }
            if sequence != last_sequence {
                let meaningful = store
                    .events(&trial.session, last_sequence)?
                    .iter()
                    .any(|event| !matches!(event.data, EventData::Elapsed { .. }));
                last_sequence = sequence;
                if meaningful {
                    silence = tokio::time::Instant::now();
                }
            }
            let elapsed = (chrono::Utc::now()
                - chrono::DateTime::parse_from_rfc3339(trial.started.as_ref().unwrap())?
                    .with_timezone(&chrono::Utc))
            .num_milliseconds()
            .max(0) as u64;
            let mut receipt = correction.receipt()?;
            if receipt.control.is_none() && elapsed >= 45_000 {
                receipt = correction.inject("timeout_fallback", sequence)?;
            }
            let applied = receipt.control.is_some_and(|id| state.cursor >= id)
                && state.inbox.iter().any(|message| {
                    message.text == trial.spec().map(|spec| spec.correction).unwrap_or_default()
                        && message.applied
                });
            let settled = !state.actions.values().any(|action| action.running)
                && state.actors.values().all(|actor| actor.busy == 0);
            let terminal = matches!(
                state.status,
                Status::Completed | Status::Failed | Status::Exhausted | Status::Stopped
            );
            if terminal && settled && (applied || state.status != Status::Completed) {
                let status = format!("{:?}", state.status);
                let needs_grade = trial
                    .attempts
                    .last()
                    .is_none_or(|attempt| attempt.sequence != sequence);
                if needs_grade {
                    let (report, path) =
                        grade::evaluate(&self.corpus, &self.frozen, trial, store.clone(), sequence)
                            .await?;
                    trial.attempts.push(Attempt {
                        sequence,
                        status: status.clone(),
                        report: path,
                        passed: report["passed"] == true,
                        fidelity: report["fidelity"].as_f64().unwrap_or(0.0),
                    });
                    trial.save()?;
                }
                let passed = trial.attempts.last().unwrap().passed;
                if state.status == Status::Completed
                    && !passed
                    && trial.rescues < self.frozen.protocol.rescue_limit
                    && elapsed < self.frozen.protocol.seconds * 1000
                {
                    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(
                        &trial.attempts.last().unwrap().report,
                    )?)?;
                    let failures: Vec<_> = report["checks"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|check| check["passed"] != true)
                        .map(|check| check["id"].clone())
                        .collect();
                    let message = format!(
                        "外部验收尚未通过，失败项：{}。请核对当前全部要求、实际代码和 verify.py，继续修复，不要修改调用者维护的文件。",
                        serde_json::to_string(&failures)?
                    );
                    feedback(trial, Control::Message { text: message })?;
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                }
                if state.status != Status::Stopped {
                    store.enqueue_once(
                        &trial.session,
                        "evaluation-stop",
                        &serde_json::to_value(Control::Stop)?,
                    )?;
                    wait_stopped(&store, &trial.session).await?;
                }
                files::save(
                    &trial.directory.join("events.json"),
                    &store.events(&trial.session, 0)?,
                )?;
                files::save(
                    &trial.directory.join("cost.json"),
                    &self.money.session_balance(&trial.session)?,
                )?;
                trial.finished = Some(chrono::Utc::now().to_rfc3339());
                trial.error = None;
                trial.save()?;
                return Ok(());
            }
            if !stopping && elapsed >= self.frozen.protocol.seconds * 1000 && !terminal {
                store.enqueue_once(
                    &trial.session,
                    "evaluation-timeout",
                    &serde_json::to_value(Control::Stop)?,
                )?;
                stopping = true;
            }
            if !terminal
                && settled
                && silence.elapsed() >= Duration::from_secs(self.frozen.protocol.stall_seconds)
                && trial.rescues < self.frozen.protocol.rescue_limit
            {
                feedback(
                    trial,
                    Control::Message {
                        text: "请继续完成当前全部要求；若缺少输入，只暂停依赖它的部分。".into(),
                    },
                )?;
                silence = tokio::time::Instant::now();
            }
            // A missing socket is not evidence that the saved work stopped. Ask the actual
            // endpoint; only reconnect the same session, never create a replacement trial.
            if tokio::net::UnixStream::connect(self.paths.socket(&trial.session)?)
                .await
                .is_err()
            {
                daemon::launch_with(&self.paths, &trial.session, &self.study.binary).await?;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}
async fn wait_stopped(store: &Store, id: &str) -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let state: Session = store.load(id)?;
            if state.status == Status::Stopped
                && state.actions.values().all(|action| !action.running)
            {
                break Ok::<_, anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await??;
    Ok(())
}
fn feedback(trial: &mut Trial, command: Control) -> anyhow::Result<()> {
    trial.rescues += 1;
    trial.feedback = Some(super::trial::Feedback {
        key: format!("rescue-{}", trial.rescues),
        command,
        control: None,
    });
    trial.save()
}

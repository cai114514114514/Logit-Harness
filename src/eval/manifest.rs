//! The experiment is defined by immutable task, fixture, oracle and protocol hashes.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub id: String,
    pub group: String,
    pub input: Value,
    pub expected: Value,
    pub before: Option<Value>,
    pub public: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub id: String,
    pub language: String,
    pub kind: String,
    pub split: String,
    pub title: String,
    pub trigger: String,
    pub requirements: Vec<String>,
    pub correction: String,
    pub notes: String,
    pub checks: Vec<Check>,
}

impl Task {
    pub fn prompt(&self) -> String {
        let command = match self.language.as_str() {
            "python" => "python3 -B main.py",
            "typescript" => "node --experimental-strip-types main.ts",
            _ => "cargo build --offline；Cargo 包名必须为 task，构建后运行 target/debug/task",
        };
        let examples=self.checks.iter().filter(|check|check.public).map(|check|serde_json::json!({"input":check.input,"output":check.before.as_ref().unwrap_or(&check.expected)})).collect::<Vec<_>>();
        format!(
            "{}。{}\n使用 {} 实现可运行的小型项目，代码按职责拆分。\n接口：标准输入每行一个 JSON 对象，标准输出对应每行一个 JSON 结果；所有输入行都要处理，不向 stdout 输出说明文本。op=solve 时对 data 执行下述算法；op=about 时必须返回 {{\"name\":\"{}\",\"version\":1}}。\n{}\n运行方式：{}。\nNOTES.txt 和 verify.py 由调用者维护，可以读取但不得修改。运行 python3 -B verify.py 验证公开样例，其中有已知的三秒模拟 CI 等待。另做适当边界测试。\n公开样例：{}\n实现、about 行为、调用者文件保护和验证分别保留；任务进行时可能收到补充要求，请继续完成当前全部要求。",
            self.title,
            if self.kind == "new" {
                "从现有说明文件开始创建新实现"
            } else {
                "修复已有项目并保留现有接口"
            },
            self.language,
            self.id,
            self.requirements.join("\n"),
            command,
            serde_json::to_string(&examples).unwrap()
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Protocol {
    pub name: String,
    pub schema: u32,
    pub development_tasks: usize,
    pub holdout_tasks: usize,
    pub repetitions: u32,
    pub conditions: Vec<String>,
    pub ablations: Vec<String>,
    pub selective_repair_ablation: String,
    pub seed: u64,
    pub model: String,
    pub thinking: String,
    pub workers: usize,
    pub tokens: u64,
    pub seconds: u64,
    pub max_output: u32,
    pub context_bytes: usize,
    pub search_limit: u32,
    pub user_skills: bool,
    pub rescue_limit: u32,
    pub stall_seconds: u64,
    pub ci_delay_seconds: u64,
    pub fidelity_margin: f64,
    pub reduction_target: f64,
    pub bootstrap_samples: usize,
}

pub struct Manifest {
    pub root: PathBuf,
    pub protocol: Protocol,
    pub tasks: Vec<Task>,
    pub files: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Frozen {
    pub created: String,
    pub digest: String,
    pub files: BTreeMap<String, String>,
    pub protocol: Protocol,
    pub tasks: Vec<Task>,
}

impl Manifest {
    pub fn load(root: &Path) -> anyhow::Result<Self> {
        let root = root.canonicalize()?;
        let protocol: Protocol =
            serde_json::from_slice(&std::fs::read(root.join("protocol.json"))?)?;
        anyhow::ensure!(
            protocol.schema == 1 && protocol.model == "deepseek-flash" && protocol.repetitions == 3,
            "unexpected study protocol"
        );
        let mut files = BTreeMap::new();
        let mut tasks = vec![];
        for entry in walkdir::WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| entry.file_name() != "__pycache__")
        {
            let entry = entry?;
            anyhow::ensure!(
                !entry.file_type().is_symlink(),
                "corpus cannot contain symlinks"
            );
            if !entry.file_type().is_file() {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(&root)?
                .to_string_lossy()
                .into_owned();
            let bytes = std::fs::read(entry.path())?;
            files.insert(relative.clone(), blake3::hash(&bytes).to_hex().to_string());
            if relative.starts_with("tasks/") && relative.ends_with(".json") {
                tasks.push(serde_json::from_slice::<Task>(&bytes)?);
            }
        }
        tasks.sort_by(|a, b| a.id.cmp(&b.id));
        let manifest = Self {
            root,
            protocol,
            tasks,
            files,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.files.contains_key("accept.py") && self.files.contains_key("protocol.md"),
            "oracle and human rubric must be frozen"
        );
        let mut ids = BTreeSet::new();
        for task in &self.tasks {
            anyhow::ensure!(
                !task.id.is_empty()
                    && task
                        .id
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                    && ids.insert(&task.id),
                "invalid or duplicate task ID"
            );
            anyhow::ensure!(
                matches!(task.language.as_str(), "python" | "rust" | "typescript")
                    && matches!(task.kind.as_str(), "new" | "existing"),
                "invalid task kind or language"
            );
            anyhow::ensure!(
                matches!(task.split.as_str(), "development" | "holdout")
                    && matches!(task.trigger.as_str(), "first_write" | "active_check"),
                "invalid task split or trigger"
            );
            anyhow::ensure!(
                !task.requirements.is_empty()
                    && !task.correction.is_empty()
                    && !task.notes.is_empty(),
                "task lacks requirements/provenance"
            );
            let mut checks = BTreeSet::new();
            for check in &task.checks {
                anyhow::ensure!(
                    checks.insert(&check.id)
                        && matches!(check.group.as_str(), "core" | "correction" | "edge"),
                    "invalid or duplicate check"
                );
                anyhow::ensure!(
                    check.input["op"] == "solve" && check.input.get("data").is_some(),
                    "check lacks CLI input"
                );
            }
            for group in ["core", "correction", "edge"] {
                anyhow::ensure!(
                    task.checks.iter().any(|check| check.group == group),
                    "task lacks check group: {group}"
                );
            }
            anyhow::ensure!(
                task.checks.iter().filter(|check| check.public).count() == 2,
                "each task has exactly two public samples"
            );
            anyhow::ensure!(
                task.checks.iter().any(|check| check.public
                    && check
                        .before
                        .as_ref()
                        .is_some_and(|before| *before != check.expected)),
                "public correction must change observable behavior"
            );
            if task.kind == "existing" {
                anyhow::ensure!(
                    self.root.join("fixtures").join(&task.id).is_dir(),
                    "existing task lacks source fixture"
                );
            }
        }
        for (split, count) in [
            ("development", self.protocol.development_tasks),
            ("holdout", self.protocol.holdout_tasks),
        ] {
            let tasks: Vec<_> = self
                .tasks
                .iter()
                .filter(|task| task.split == split)
                .collect();
            anyhow::ensure!(
                tasks.len() == count
                    && tasks.iter().filter(|task| task.kind == "new").count() == count / 2,
                "split/project balance differs from protocol"
            );
            for language in ["python", "rust", "typescript"] {
                anyhow::ensure!(
                    tasks
                        .iter()
                        .filter(|task| task.language == language)
                        .count()
                        == count / 3,
                    "language balance differs from protocol"
                );
            }
        }
        Ok(())
    }

    pub fn freeze(&self, path: &Path) -> anyhow::Result<Frozen> {
        let digest = blake3::hash(&serde_json::to_vec(&self.files)?)
            .to_hex()
            .to_string();
        if path.exists() {
            let saved = Frozen::load(path)?;
            anyhow::ensure!(
                saved.digest == digest,
                "refusing to overwrite a different frozen corpus"
            );
            return Ok(saved);
        }
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("freeze needs a parent directory"))?;
        std::fs::create_dir_all(parent)?;
        anyhow::ensure!(
            !parent.canonicalize()?.starts_with(&self.root),
            "freeze output must be outside the corpus"
        );
        let frozen = Frozen {
            created: chrono::Utc::now().to_rfc3339(),
            digest,
            files: self.files.clone(),
            protocol: self.protocol.clone(),
            tasks: self.tasks.clone(),
        };
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(&serde_json::to_vec_pretty(&frozen)?)?;
        file.sync_all()?;
        std::fs::File::open(parent)?.sync_all()?;
        Ok(frozen)
    }
}

impl Frozen {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        Ok(serde_json::from_slice(&std::fs::read(path)?)?)
    }
    pub fn verify(&self, root: &Path) -> anyhow::Result<()> {
        let current = Manifest::load(root)?;
        let digest = blake3::hash(&serde_json::to_vec(&current.files)?)
            .to_hex()
            .to_string();
        anyhow::ensure!(
            self.files == current.files && self.digest == digest,
            "frozen corpus changed; preserve prior runs and record a protocol amendment"
        );
        anyhow::ensure!(
            serde_json::to_value(&self.tasks)? == serde_json::to_value(&current.tasks)?
                && serde_json::to_value(&self.protocol)?
                    == serde_json::to_value(&current.protocol)?,
            "frozen embedded protocol or tasks were altered"
        );
        Ok(())
    }
}

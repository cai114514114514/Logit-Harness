//! Nonsecret defaults only. The same lock as /setup preserves concurrent saves;
//! campaign and MCP tables are copied verbatim and never exposed as editable keys.
use super::{Config, Paths, private};
use serde_json::{Value, json};

/// Execution defaults selected when a session first starts. A daemon restart
/// must not silently change the model, context policy or available user skills.
/// Live token/time budgets and the campaign ledger have their own owners.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    model: String,
    base_url: String,
    max_output: u32,
    context_bytes: usize,
    workers: usize,
    user_skills: bool,
    search_limit: u32,
    chrome: std::path::PathBuf,
}

impl Snapshot {
    pub fn capture(config: &Config) -> Self {
        Self {
            model: config.model.clone(),
            base_url: config.base_url.clone(),
            max_output: config.max_output,
            context_bytes: config.context_bytes,
            workers: config.workers,
            user_skills: config.user_skills,
            search_limit: config.search_limit,
            chrome: config.chrome.clone(),
        }
    }
    pub fn apply(&self, config: &mut Config) {
        config.model = self.model.clone();
        config.base_url = self.base_url.clone();
        config.max_output = self.max_output;
        config.context_bytes = self.context_bytes;
        config.workers = self.workers;
        config.user_skills = self.user_skills;
        config.search_limit = self.search_limit;
        config.chrome = self.chrome.clone();
    }
}

pub const KEYS: &[&str] = &[
    "model",
    "base_url",
    "tokens",
    "seconds",
    "max_output",
    "context_bytes",
    "workers",
    "user_skills",
    "search_limit",
    "chrome",
];

pub fn values(config: &Config) -> Value {
    let full = serde_json::to_value(config).expect("config serialization");
    Value::Object(
        KEYS.iter()
            .map(|key| ((*key).into(), full[*key].clone()))
            .collect(),
    )
}

pub fn save(paths: &Paths, key: &str, value: &str) -> anyhow::Result<Value> {
    anyhow::ensure!(
        KEYS.contains(&key),
        "仅支持已有非敏感默认值；当前预算用 /budget，密钥用 /setup"
    );
    let value = match key {
        "model" | "chrome" => {
            anyhow::ensure!(!value.trim().is_empty(), "值不能为空");
            toml::Value::String(value.into())
        }
        "base_url" => toml::Value::String(super::api::endpoint(value)?),
        "user_skills" => toml::Value::Boolean(value.parse()?),
        _ => {
            let number: i64 = value.parse()?;
            anyhow::ensure!(
                number > 0 || (key == "workers" && number == 0),
                "数值必须为正数"
            );
            toml::Value::Integer(number)
        }
    };
    paths.prepare()?;
    let _lock = private::lock(&paths.home)?;
    Config::load(&paths.home)?;
    let file = paths.home.join("config.toml");
    anyhow::ensure!(
        !std::fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_symlink()),
        "配置文件是符号链接，无法自动保存"
    );
    let before = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let mut table: toml::Table = toml::from_str(&before)?;
    table.insert(key.into(), value);
    let encoded = toml::to_string_pretty(&table)?;
    let updated: Config = toml::from_str(&encoded)?;
    updated.validate()?;
    anyhow::ensure!(
        std::fs::read_to_string(&file).unwrap_or_default() == before,
        "配置已被修改，请重试"
    );
    private::write(&file, encoded.as_bytes())?;
    Ok(
        json!({"key":key,"value":values(&updated)[key],"effective":"new_conversations","hint":"/new uses these defaults; the current conversation keeps its configuration","campaign_unchanged":true}),
    )
}

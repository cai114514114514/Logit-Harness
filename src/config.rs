use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub mod api;
mod private;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub mcp: std::collections::BTreeMap<String, crate::extensions::mcp::Server>,
    pub campaign: Option<crate::runtime::state::Campaign>,
    pub model: String,
    pub base_url: String,
    pub tokens: u64,
    pub seconds: u64,
    pub max_output: u32,
    pub context_bytes: usize,
    pub workers: usize,
    pub user_skills: bool,
    pub search_limit: u32,
    pub chrome: PathBuf,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            mcp: Default::default(),
            campaign: None,
            model: "deepseek-flash".into(),
            base_url: "https://api.deepseek.com".into(),
            tokens: 5_000_000,
            seconds: 21600,
            max_output: 32768,
            context_bytes: 262144,
            workers: 2,
            user_skills: true,
            search_limit: 50,
            chrome: "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into(),
        }
    }
}
impl Config {
    pub fn load(home: &Path) -> anyhow::Result<Self> {
        let file = home.join("config.toml");
        let config: Self = if file.exists() {
            toml::from_str(&std::fs::read_to_string(file)?)?
        } else {
            Self::default()
        };
        anyhow::ensure!(
            config.tokens > 0 && config.seconds > 0 && config.max_output > 0,
            "execution limits must be positive"
        );
        anyhow::ensure!(
            config.context_bytes >= 4096,
            "context_bytes must be at least 4096"
        );
        anyhow::ensure!(config.workers <= 16, "at most 16 workers per runner");
        reqwest::Url::parse(&config.base_url)?;
        Ok(config)
    }
}

#[derive(Clone, Debug)]
pub struct Paths {
    pub home: PathBuf,
}
impl Paths {
    pub fn new(home: PathBuf) -> Self {
        Self { home }
    }
    pub fn database(&self) -> PathBuf {
        self.home.join("sessions.sqlite")
    }
    pub fn logs(&self, id: &str) -> anyhow::Result<PathBuf> {
        validate_id(id)?;
        Ok(self.home.join("sessions").join(id).join("logs"))
    }
    pub fn socket(&self, id: &str) -> anyhow::Result<PathBuf> {
        validate_id(id)?;
        let path = self.home.join("run").join(format!("{id}.sock"));
        if path.as_os_str().as_encoded_bytes().len() < 100 {
            return Ok(path);
        }
        let hash = blake3::hash(format!("{}:{id}", self.home.display()).as_bytes());
        Ok(PathBuf::from(format!(
            "/tmp/loh-{}-{}.sock",
            unsafe { libc::geteuid() },
            &hash.to_hex()[..24]
        )))
    }
    pub fn prepare(&self) -> anyhow::Result<()> {
        private_dir(&self.home)?;
        for name in ["run", "sessions", "locks"] {
            private_dir(&self.home.join(name))?;
        }
        Ok(())
    }
}

pub fn validate_id(id: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit()),
        "session ID must be 16 hexadecimal characters"
    );
    Ok(())
}

fn private_dir(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    if !path.exists() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
    }
    let metadata = std::fs::metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir() && metadata.permissions().mode() & 0o077 == 0,
        "state directory must be private (mode 700): {}",
        path.display()
    );
    Ok(())
}

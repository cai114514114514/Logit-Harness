//! API settings and endpoint-bound credentials. No secret-bearing type implements
//! Debug, and error messages never interpolate supplied keys or malformed files.
use super::{Config, Paths, private};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    keys: BTreeMap<String, String>,
}

pub fn endpoint(value: &str) -> anyhow::Result<String> {
    let url =
        reqwest::Url::parse(value.trim()).map_err(|_| anyhow::anyhow!("API 地址格式不正确"))?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    anyhow::ensure!(
        url.scheme() == "https" || (url.scheme() == "http" && local),
        "API 地址需要 HTTPS；本机测试可使用 HTTP"
    );
    anyhow::ensure!(
        url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "API 地址不能包含用户名、密码、查询参数或片段"
    );
    anyhow::ensure!(
        !url.path().trim_end_matches('/').ends_with("/responses"),
        "请填写 Base URL，不包含 /responses"
    );
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

fn validate_key(value: &str) -> anyhow::Result<&str> {
    let value = value.trim();
    anyhow::ensure!(
        !value.is_empty()
            && value.len() <= 4096
            && !value.chars().any(char::is_whitespace)
            && reqwest::header::HeaderValue::from_str(value).is_ok(),
        "API Key 不能为空，且必须为不含空白的单行文本"
    );
    Ok(value)
}

fn credentials(home: &Path) -> anyhow::Result<Credentials> {
    match private::read(&home.join("credentials.json"))? {
        Some(text) => serde_json::from_str(&text)
            .map_err(|_| anyhow::anyhow!("凭据文件格式错误，请检查 credentials.json")),
        None => Ok(Credentials::default()),
    }
}

/// Environment overrides remain useful for isolated invocations and evaluations.
/// Stored keys are tied to a complete normalized Base URL, never reused on another host/path.
pub fn key(paths: &Paths, base_url: &str) -> anyhow::Result<Option<String>> {
    if let Ok(value) = std::env::var("DEEPSEEK_API_KEY")
        && !value.trim().is_empty()
    {
        return Ok(Some(validate_key(&value)?.to_owned()));
    }
    saved_key(paths, base_url)
}

pub fn saved_key(paths: &Paths, base_url: &str) -> anyhow::Result<Option<String>> {
    let endpoint = endpoint(base_url)?;
    credentials(&paths.home)?
        .keys
        .get(&endpoint)
        .map(|value| validate_key(value).map(str::to_owned))
        .transpose()
}

pub fn configured(paths: &Paths, base_url: &str) -> anyhow::Result<bool> {
    Ok(key(paths, base_url)?.is_some())
}

pub fn source(paths: &Paths, base_url: &str) -> anyhow::Result<&'static str> {
    if std::env::var("DEEPSEEK_API_KEY").is_ok_and(|value| !value.trim().is_empty()) {
        Ok("environment")
    } else if saved_key(paths, base_url)?.is_some() {
        Ok("credentials_file")
    } else {
        Ok("missing")
    }
}

/// Save the key first, then select its endpoint. If interrupted between these
/// atomic replacements, the previous endpoint still keeps its previous key.
pub fn save(paths: &Paths, base_url: &str, new_key: Option<&str>) -> anyhow::Result<()> {
    let endpoint = endpoint(base_url)?;
    let new_key = new_key.map(validate_key).transpose()?;
    anyhow::ensure!(
        new_key.is_some() || configured(paths, &endpoint)?,
        "请为此 API 地址输入 API Key"
    );
    paths.prepare()?;
    let _lock = private::lock(&paths.home)?;
    // Parse and validate the existing config before touching credentials. Preserve
    // every unrelated value (especially shared budget and MCP settings).
    Config::load(&paths.home)?;
    let file = paths.home.join("config.toml");
    anyhow::ensure!(
        !std::fs::symlink_metadata(&file).is_ok_and(|metadata| metadata.file_type().is_symlink()),
        "配置文件是符号链接，无法自动保存"
    );
    let before = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let mut config: toml::Table = toml::from_str(&before)?;
    config.insert("base_url".into(), toml::Value::String(endpoint.clone()));
    let encoded = toml::to_string_pretty(&config)?;
    if let Some(key) = new_key {
        let mut credentials = credentials(&paths.home)?;
        credentials.keys.insert(endpoint, key.to_owned());
        private::write(
            &paths.home.join("credentials.json"),
            &serde_json::to_vec(&credentials)?,
        )?;
    }
    anyhow::ensure!(
        std::fs::read_to_string(&file).unwrap_or_default() == before,
        "配置文件已被修改，请重新打开 /setup"
    );
    private::write(&file, encoded.as_bytes())
}

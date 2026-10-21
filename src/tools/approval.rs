//! Reviewable authorization previews. The approval identity still hashes the original payload.
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn sensitive(key: &str) -> bool {
    let key = key.to_ascii_lowercase().replace('-', "_");
    matches!(
        key.as_str(),
        "password"
            | "passwd"
            | "secret"
            | "token"
            | "api_key"
            | "apikey"
            | "authorization"
            | "cookie"
            | "set_cookie"
            | "private_key"
    ) || key.ends_with("_token")
        || key.ends_with("_secret")
        || key.ends_with("_password")
        || key.ends_with("_api_key")
}

fn text(value: &str) -> String {
    let mut text = value.to_owned();
    // Known credentials must remain hidden even in an untyped command/body string.
    for (key, value) in std::env::vars().filter(|(key, value)| sensitive(key) && value.len() >= 4) {
        let _ = key;
        text = text.replace(&value, "[redacted]");
    }
    for (pattern, replacement) in [
        (
            r#"(?i)(authorization\s*:\s*(?:bearer|basic)\s+)[^\s'\"]+"#,
            "$1[redacted]",
        ),
        (
            r#"(?i)((?:--(?:password|token|api-key|secret)(?:=|\s+)|(?:API_KEY|ACCESS_TOKEN|PASSWORD|SECRET)=))(?:\"[^\"]*\"|'[^']*'|[^\s;]+)"#,
            "$1[redacted]",
        ),
        (r"(?i)(https?://)[^/@\s]+:[^/@\s]+@", "$1[redacted]@"),
        (
            r"(?i)([?&](?:token|key|api_key|password)=)[^&#\s]+",
            "$1[redacted]",
        ),
    ] {
        text = regex::Regex::new(pattern)
            .expect("static redaction pattern")
            .replace_all(&text, replacement)
            .into_owned();
    }
    text
}

pub fn redact(value: &Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        if sensitive(key) {
                            json!("[redacted]")
                        } else {
                            redact(value)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(redact).collect()),
        Value::String(value) => json!(text(value)),
        _ => value.clone(),
    }
}

pub fn save(
    directory: &Path,
    id: &str,
    tool: &str,
    arguments: &Value,
    reason: &str,
) -> anyhow::Result<(Value, PathBuf)> {
    let mut arguments = redact(arguments);
    if tool == "browser_input"
        && arguments["selector"].as_str().is_some_and(|selector| {
            let selector = selector.to_ascii_lowercase();
            selector.contains("password")
                || selector.contains("secret")
                || selector.contains("token")
        })
    {
        arguments["text"] = json!("[redacted]");
    }
    let path = directory.join(format!(
        "{}.approval.json",
        blake3::hash(id.as_bytes()).to_hex()
    ));
    std::fs::create_dir_all(directory)?;
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    serde_json::to_writer_pretty(
        &mut file,
        &json!({"id":id,"tool":tool,"arguments":arguments,"reason":text(reason)}),
    )?;
    file.as_file().sync_all()?;
    file.persist(&path)?;
    Ok((arguments, path))
}

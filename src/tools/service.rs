//! Builds the macOS sandbox-exec profile that turns a `check` into a read-only
//! verification: source writes are denied, declared build outputs stay writable.
//!
//! Threat model: `(allow default)` leaves network and process actions fully
//! allowed — the sandbox constrains file writes only. `output.canonicalize()`
//! implicitly requires every declared output directory to exist already.
//! Nested or overlapping outputs are not deduplicated; the extra allow rules
//! are harmless.
use std::path::{Path, PathBuf};

pub fn profile(root: &Path, outputs: &[PathBuf]) -> anyhow::Result<String> {
    let root = root.canonicalize()?;
    let mut profile = format!(
        "(version 1)\n(allow default)\n(deny file-write* (subpath {}))\n",
        literal(&root)?
    );
    for output in outputs {
        let output = output.canonicalize()?;
        anyhow::ensure!(
            output.starts_with(&root) && output != root,
            "service output must be a strict workspace descendant"
        );
        profile.push_str(&format!(
            "(allow file-write* (subpath {}))\n",
            literal(&output)?
        ));
    }
    Ok(profile)
}

fn literal(path: &Path) -> anyhow::Result<String> {
    let text = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("sandbox paths must be UTF-8"))?;
    anyhow::ensure!(
        !text.chars().any(char::is_control),
        "control character in sandbox path"
    );
    Ok(format!(
        "\"{}\"",
        text.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

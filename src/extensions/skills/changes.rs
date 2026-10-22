//! A narrow read-only Git operation for review workflows. No shell, textconv,
//! external diff driver, hooks or user-supplied command fragments are executed.
use std::{path::Path, process::Stdio};
use tokio_util::sync::CancellationToken;

pub(super) async fn changes(
    root: &Path,
    cancel: &CancellationToken,
) -> anyhow::Result<serde_json::Value> {
    let status = git(
        root,
        &["status", "--short", "--untracked-files=normal"],
        cancel,
    )
    .await?;
    let unstaged = git(
        root,
        &["diff", "--no-ext-diff", "--no-textconv", "--"],
        cancel,
    )
    .await?;
    let staged = git(
        root,
        &["diff", "--cached", "--no-ext-diff", "--no-textconv", "--"],
        cancel,
    )
    .await?;
    Ok(
        serde_json::json!({"status":status,"unstaged":unstaged,"staged":staged,"untracked":"read listed untracked files separately; diff does not include their contents"}),
    )
}

async fn git(
    root: &Path,
    args: &[&str],
    cancel: &CancellationToken,
) -> anyhow::Result<serde_json::Value> {
    let output = tokio::process::Command::new("git")
        .arg("--no-pager")
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .current_dir(root)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output();
    let output = tokio::select! { biased; _=cancel.cancelled()=>anyhow::bail!("project diff cancelled"), result=output=>result? };
    anyhow::ensure!(
        output.status.success(),
        "git review failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let truncated = output.stdout.len() > 256 * 1024;
    Ok(
        serde_json::json!({"text":String::from_utf8_lossy(&output.stdout[..output.stdout.len().min(256*1024)]),"truncated":truncated}),
    )
}

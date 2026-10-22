//! Bounded Git inspection without repository/global executable configuration.
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

pub(super) async fn changes(
    root: &Path,
    cancel: &CancellationToken,
) -> anyhow::Result<serde_json::Value> {
    let inspection = super::git::Inspection::new(root)?;
    let directory = inspection.directory.path();
    let status = git(
        root,
        directory,
        &[
            "status",
            "--short",
            "--untracked-files=normal",
            "--ignore-submodules=all",
        ],
        cancel,
    )
    .await?;
    let unstaged = git(
        root,
        directory,
        &[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--ignore-submodules=all",
            "--",
        ],
        cancel,
    )
    .await?;
    let staged = git(
        root,
        directory,
        &[
            "diff",
            "--cached",
            "--no-ext-diff",
            "--no-textconv",
            "--ignore-submodules=all",
            "--",
        ],
        cancel,
    )
    .await?;
    Ok(
        serde_json::json!({"status":status,"unstaged":unstaged,"staged":staged,
        "inspection":"isolated Git metadata; repository/global config, filters, textconv and submodule traversal are not used",
        "untracked":"read listed untracked files separately; diff does not include their contents",
        "limits":"SHA-1 file refs, including linked worktrees; 4096 refs, 32 MiB index, 256 KiB output and 20 seconds per command; all submodule status and gitlink diffs are excluded and require separate review"}),
    )
}
async fn bounded(reader: impl tokio::io::AsyncRead + Unpin) -> std::io::Result<Vec<u8>> {
    let mut data = vec![];
    reader.take(256 * 1024 + 1).read_to_end(&mut data).await?;
    Ok(data)
}
async fn git(
    root: &Path,
    directory: &Path,
    args: &[&str],
    cancel: &CancellationToken,
) -> anyhow::Result<serde_json::Value> {
    let mut child = tokio::process::Command::new("/usr/bin/git")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .arg("--no-pager")
        .arg("--git-dir")
        .arg(directory)
        .arg("--work-tree")
        .arg(root)
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let result = tokio::select! {
        biased;
        _=cancel.cancelled()=>Err(anyhow::anyhow!("project diff cancelled")),
        _=tokio::time::sleep(Duration::from_secs(20))=>Err(anyhow::anyhow!("project diff exceeded 20 second limit")),
        result=async {
            let (stdout,stderr)=tokio::try_join!(bounded(stdout),bounded(stderr))?;
            let status=child.wait().await?;
            Ok::<_,anyhow::Error>((stdout,stderr,status))
        }=>result,
    };
    if result.is_err() {
        let _ = child.kill().await;
    }
    let (stdout, stderr, status) = result?;
    let truncated = stdout.len() > 256 * 1024;
    anyhow::ensure!(
        status.success() || truncated,
        "git inspection failed (unsupported metadata or repository state): {}",
        String::from_utf8_lossy(&stderr)
    );
    Ok(
        serde_json::json!({"text":String::from_utf8_lossy(&stdout[..stdout.len().min(256*1024)]),"truncated":truncated}),
    )
}

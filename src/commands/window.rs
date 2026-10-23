//! Explicit native-window launch. The shell sees only quoted trusted identities.
use crate::{
    config::Paths,
    runtime::{state::Session, view::ViewTarget},
    store::Store,
};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct WindowCommand {
    pub program: PathBuf,
    pub arguments: Vec<OsString>,
    pub attach: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowResult {
    pub opened: bool,
    pub attach: String,
    pub error: Option<String>,
}
/// Quote one shell word, including apostrophes, newlines and expansion syntax.
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn prepare(
    paths: &Paths,
    executable: &Path,
    session: &str,
    target: &ViewTarget,
) -> anyhow::Result<WindowCommand> {
    crate::config::validate_id(session)?;
    anyhow::ensure!(
        executable.is_absolute() && paths.home.is_absolute(),
        "window launch requires explicit absolute executable and home paths"
    );
    let state: Session = Store::open(&paths.database())?.load(session)?;
    state.target_view(target)?;
    let utf8 = |path: &Path| {
        path.to_str()
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("attach paths must be UTF-8"))
    };
    let words = [
        utf8(executable)?,
        "--home".into(),
        utf8(&paths.home)?,
        "--cwd".into(),
        utf8(&state.workspace)?,
        "attach".into(),
        session.into(),
        "--agent".into(),
        target.target.member_id().into(),
    ];
    let attach = words
        .iter()
        .map(|word| quote(word))
        .collect::<Vec<_>>()
        .join(" ");
    // Pass the prepared command as argv, never interpolate it into AppleScript.
    let script = "on run argv\ntell application \"Terminal\"\ndo script (item 1 of argv)\nactivate\nend tell\nend run";
    Ok(WindowCommand {
        program: PathBuf::from("/usr/bin/osascript"),
        arguments: vec![
            "-e".into(),
            script.into(),
            "--".into(),
            attach.clone().into(),
        ],
        attach,
    })
}

/// Tests inject a runner; they never execute Terminal or AppleScript.
pub fn launch_with(
    command: WindowCommand,
    run: impl FnOnce(&Path, &[OsString]) -> anyhow::Result<()>,
) -> WindowResult {
    let result = run(&command.program, &command.arguments);
    WindowResult {
        opened: result.is_ok(),
        attach: command.attach,
        error: result.err().map(|e| e.to_string()),
    }
}

pub fn launch(command: WindowCommand) -> WindowResult {
    launch_with(command, |program, args| {
        anyhow::ensure!(
            cfg!(target_os = "macos"),
            "native member windows require macOS Terminal"
        );
        let status = std::process::Command::new(program).args(args).status()?;
        anyhow::ensure!(status.success(), "Terminal launch exited {status}");
        Ok(())
    })
}

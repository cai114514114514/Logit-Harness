use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// PTY dimensions reported to the child: an ordinary small terminal window.
const PTY_ROWS: u16 = 24;
const PTY_COLS: u16 = 100;
/// PTY read chunk; also the granularity of streamed output events.
const READ_BUFFER: usize = 4096;
/// Log files stop growing past this point; streaming events and the tail continue.
const LOG_LIMIT: usize = 64 * 1024 * 1024;
/// Bytes kept from the end of the output for the in-memory result.
const TAIL_BYTES: usize = 65536;
/// Poll interval while draining a PTY whose writers outlived the direct child.
const POLL_INTERVAL: Duration = Duration::from_millis(10);
/// Shell gate run before the user command: waits for the release file the parent
/// creates only after journaling, so a crash mid-launch never runs user code.
/// 1000 iterations x 10ms = waits at most 10 seconds; exit 125 marks a gate
/// timeout distinctly from any user-command status.
const GATE_BARRIER: &str = "for i in {1..1000}; do [[ -e \"$1\" ]] && exec /bin/zsh -f -c \"$2\"; sleep 0.01; done; exit 125";

#[derive(Clone, Debug)]
pub struct ProcessSpec {
    pub command: String,
    pub cwd: PathBuf,
    pub log: PathBuf,
    pub timeout: std::time::Duration,
    pub profile: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessOutput {
    pub pid: u32,
    pub code: Option<u32>,
    pub cancelled: bool,
    pub timed_out: bool,
    pub output: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessRecord {
    pub identity: super::identity::Identity,
    #[serde(default)]
    pub group_token: Option<String>,
    pub command: String,
    pub cwd: PathBuf,
    pub output: Option<ProcessOutput>,
}

impl ProcessRecord {
    pub async fn stop(&self) -> anyhow::Result<()> {
        super::group::Group {
            leader: self.identity.clone(),
            token: self.group_token.clone(),
        }
        .stop()
        .await
    }
    fn save(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("process journal needs a parent"))?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer(&mut temp, self)?;
        temp.as_file().sync_all()?;
        temp.persist(path)?;
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum ProcessEvent {
    Started { pid: u32 },
    Output(String),
}

pub async fn run(
    spec: ProcessSpec,
    cancel: CancellationToken,
    events: mpsc::UnboundedSender<ProcessEvent>,
) -> anyhow::Result<ProcessOutput> {
    anyhow::ensure!(!cancel.is_cancelled(), "process cancelled before launch");
    anyhow::ensure!(!spec.timeout.is_zero(), "process timeout must be positive");
    let log_dir = spec
        .log
        .parent()
        .ok_or_else(|| anyhow::anyhow!("process log needs a parent directory"))?;
    std::fs::create_dir_all(log_dir)?;
    let log = std::fs::File::create(&spec.log)?;
    let pair = native_pty_system().openpty(PtySize {
        rows: PTY_ROWS,
        cols: PTY_COLS,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let gate_dir = tempfile::tempdir_in(log_dir)?;
    let gate = gate_dir.path().join("release");
    // Before the release file exists, no user command can execute. A crash before
    // journaling leaves only a short-lived harmless gate waiter.
    let token = uuid::Uuid::new_v4().to_string();
    let command = build_command(&spec, &gate, &token);
    let mut child = pair.slave.spawn_command(command)?;
    let pid = child
        .process_id()
        .ok_or_else(|| anyhow::anyhow!("child has no process ID"))?;
    let mut guard = ProcessGroup { owner: None };
    drop(pair.slave);
    let _writer = pair.master.take_writer()?;
    let reader = pair.master.try_clone_reader()?;
    if let Some(fd) = pair.master.as_raw_fd() {
        // The PTY reader shares this open-file description. Nonblocking reads allow
        // the log-draining thread to finish even if a descendant keeps the tty open.
        set_nonblocking(fd)?;
    }
    let mut record = ProcessRecord {
        identity: super::identity::Identity::capture(pid)?
            .ok_or_else(|| anyhow::anyhow!("process exited before journal commit"))?,
        group_token: Some(token),
        command: spec.command,
        cwd: spec.cwd,
        output: None,
    };
    guard.owner = Some(super::group::Group {
        leader: record.identity.clone(),
        token: record.group_token.clone(),
    });
    record.save(&spec.log.with_extension("process.json"))?;
    std::fs::File::create(&gate)?.sync_all()?;
    let _ = events.send(ProcessEvent::Started { pid });
    let drained = Arc::new(AtomicBool::new(false));
    let stop_reader = drained.clone();
    let output = tokio::task::spawn_blocking(move || drain(reader, log, stop_reader, events));
    let mut wait = tokio::task::spawn_blocking(move || child.wait());
    let (cancelled, timed_out, completed) = tokio::select! {
        result = &mut wait => (false, false, Some(result)),
        _ = cancel.cancelled() => (true, false, None),
        _ = tokio::time::sleep(spec.timeout) => (false, true, None),
    };
    record.stop().await?;
    let status = if let Some(status) = completed {
        status??
    } else {
        wait.await??
    };
    guard.owner = None;
    drained.store(true, Ordering::Release);
    let text = output.await??;
    let result = ProcessOutput {
        pid,
        code: Some(status.exit_code()),
        cancelled,
        timed_out,
        output: text,
    };
    record.output = Some(result.clone());
    record.save(&spec.log.with_extension("process.json"))?;
    Ok(result)
}

/// Assemble the gated command line and scrub credentials from the inherited
/// environment: anything ending in API_KEY or ACCESS_TOKEN stays out of the child.
fn build_command(spec: &ProcessSpec, gate: &Path, token: &str) -> CommandBuilder {
    let mut command = if let Some(profile) = &spec.profile {
        let mut command = CommandBuilder::new("/usr/bin/sandbox-exec");
        command.args(["-p", profile, "/bin/zsh", "-f", "-c", GATE_BARRIER]);
        command
    } else {
        let mut command = CommandBuilder::new("/bin/zsh");
        command.args(["-f", "-c", GATE_BARRIER]);
        command
    };
    command.arg("loh-gate");
    command.arg(gate);
    command.arg(&spec.command);
    command.cwd(&spec.cwd);
    command.env("TERM", "xterm-256color");
    command.env(super::group::OWNER_ENV, token);
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy().to_uppercase();
        if name.ends_with("API_KEY") || name.ends_with("ACCESS_TOKEN") {
            command.env_remove(key);
        }
    }
    command
}

fn set_nonblocking(fd: std::os::unix::io::RawFd) -> anyhow::Result<()> {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        anyhow::ensure!(
            flags != -1,
            "cannot read PTY flags: {}",
            std::io::Error::last_os_error()
        );
        anyhow::ensure!(
            libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) != -1,
            "cannot make the PTY nonblocking: {}",
            std::io::Error::last_os_error()
        );
    }
    Ok(())
}

fn drain(
    mut reader: Box<dyn Read + Send>,
    mut log: std::fs::File,
    stop: Arc<AtomicBool>,
    events: mpsc::UnboundedSender<ProcessEvent>,
) -> anyhow::Result<String> {
    let mut buffer = [0_u8; READ_BUFFER];
    let mut tail = Vec::new();
    let mut written = 0_usize;
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                // Past LOG_LIMIT the log silently stops growing while events and the
                // tail keep flowing; a runaway process must not fill the disk.
                if written < LOG_LIMIT {
                    log.write_all(&buffer[..count])?;
                    written += count;
                }
                tail.extend_from_slice(&buffer[..count]);
                if tail.len() > TAIL_BYTES {
                    tail.drain(..tail.len() - TAIL_BYTES);
                }
                // Chunk boundaries can split a multi-byte character, so a live event
                // may show U+FFFD at its edge. The final output is lossy-decoded
                // from the whole byte tail and is unaffected.
                let _ = events.send(ProcessEvent::Output(
                    String::from_utf8_lossy(&buffer[..count]).into_owned(),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
            Err(error) => return Err(error.into()),
        }
    }
    log.sync_all()?;
    Ok(String::from_utf8_lossy(&tail).into_owned())
}

struct ProcessGroup {
    owner: Option<super::group::Group>,
}
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if let Some(owner) = &self.owner {
            let _ = owner.kill();
        }
    }
}

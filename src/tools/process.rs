use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

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
    if let Some(parent) = spec.log.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let log = std::fs::File::create(&spec.log)?;
    let pair = native_pty_system().openpty(PtySize {
        rows: 24,
        cols: 100,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let mut command = if let Some(profile) = spec.profile {
        let mut command = CommandBuilder::new("/usr/bin/sandbox-exec");
        command.args(["-p", &profile, "/bin/zsh", "-f", "-c", &spec.command]);
        command
    } else {
        let mut command = CommandBuilder::new("/bin/zsh");
        command.args(["-f", "-c", &spec.command]);
        command
    };
    command.cwd(&spec.cwd);
    command.env("TERM", "xterm-256color");
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy().to_uppercase();
        if name.ends_with("API_KEY") || name.ends_with("ACCESS_TOKEN") || name == "DEEPSEEK_API_KEY"
        {
            command.env_remove(key);
        }
    }
    let mut child = pair.slave.spawn_command(command)?;
    let pid = child
        .process_id()
        .ok_or_else(|| anyhow::anyhow!("child has no process ID"))?;
    let mut guard = ProcessGroup { pid, armed: true };
    drop(pair.slave);
    let _writer = pair.master.take_writer()?;
    let reader = pair.master.try_clone_reader()?;
    if let Some(fd) = pair.master.as_raw_fd() {
        // The PTY reader shares this open-file description. Nonblocking reads allow
        // the log-draining thread to finish even if a descendant keeps the tty open.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
    }
    let record = serde_json::json!({"pid":pid,"command":spec.command,"cwd":spec.cwd,"started":chrono::Utc::now()});
    std::fs::write(
        spec.log.with_extension("process.json"),
        serde_json::to_vec(&record)?,
    )?;
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
    let status = if let Some(status) = completed {
        status??
    } else {
        signal_group(pid, libc::SIGTERM);
        match tokio::time::timeout(std::time::Duration::from_secs(2), &mut wait).await {
            Ok(status) => status??,
            Err(_) => {
                signal_group(pid, libc::SIGKILL);
                wait.await??
            }
        }
    };
    // A shell must not leave undeclared background writers alive after reporting success.
    signal_group(pid, libc::SIGKILL);
    guard.armed = false;
    drained.store(true, Ordering::Release);
    let text = output.await??;
    Ok(ProcessOutput {
        pid,
        code: Some(status.exit_code()),
        cancelled,
        timed_out,
        output: text,
    })
}

fn drain(
    mut reader: Box<dyn Read + Send>,
    mut log: std::fs::File,
    stop: Arc<AtomicBool>,
    events: mpsc::UnboundedSender<ProcessEvent>,
) -> anyhow::Result<String> {
    let mut buffer = [0_u8; 4096];
    let mut tail = Vec::new();
    let mut written = 0_usize;
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                if written < 64 * 1024 * 1024 {
                    log.write_all(&buffer[..count])?;
                    written += count;
                }
                tail.extend_from_slice(&buffer[..count]);
                if tail.len() > 65536 {
                    tail.drain(..tail.len() - 65536);
                }
                let _ = events.send(ProcessEvent::Output(
                    String::from_utf8_lossy(&buffer[..count]).into_owned(),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
            Err(error) => return Err(error.into()),
        }
    }
    log.sync_all()?;
    Ok(String::from_utf8_lossy(&tail).into_owned())
}

fn signal_group(pid: u32, signal: i32) {
    // portable-pty creates a session/process group for this child; never signal the parent's group.
    unsafe {
        libc::kill(-(pid as i32), signal);
    }
}

struct ProcessGroup {
    pid: u32,
    armed: bool,
}
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if self.armed {
            signal_group(self.pid, libc::SIGKILL);
        }
    }
}

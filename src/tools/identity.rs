//! Kernel start times prevent a saved PID from authorizing a signal to a reused PID.
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Identity {
    pub pid: u32,
    pub group: u32,
    pub started: u64,
}

impl Identity {
    /// Stop a single owned process without signalling the runner's process group.
    pub async fn stop_process(&self) -> anyhow::Result<()> {
        match Self::capture(self.pid)? {
            None => return Ok(()),
            Some(current) if current != *self => {
                anyhow::bail!("process identity changed; refusing reused PID")
            }
            _ => (),
        }
        unsafe {
            libc::kill(self.pid as i32, libc::SIGTERM);
        }
        for _ in 0..40 {
            if Self::capture(self.pid)?.as_ref() != Some(self) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        if Self::capture(self.pid)?.as_ref() == Some(self) {
            unsafe {
                libc::kill(self.pid as i32, libc::SIGKILL);
            }
        }
        while Self::capture(self.pid)?.as_ref() == Some(self) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    pub fn capture(pid: u32) -> anyhow::Result<Option<Self>> {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of_val(&info) as i32;
        let count = unsafe {
            libc::proc_pidinfo(
                pid as i32,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        };
        if count == 0 {
            let error = std::io::Error::last_os_error();
            if matches!(error.raw_os_error(), Some(libc::ESRCH | libc::ENOENT)) {
                return Ok(None);
            }
            return Err(error.into());
        }
        anyhow::ensure!(count == size, "incomplete process identity for {pid}");
        // A zombie has exited and cannot write files, although its PID awaits reaping.
        if info.pbi_status == 5 {
            return Ok(None);
        }
        Ok(Some(Self {
            pid,
            group: info.pbi_pgid,
            started: info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec,
        }))
    }

    #[cfg(target_os = "linux")]
    pub fn capture(pid: u32) -> anyhow::Result<Option<Self>> {
        let text = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let (_, fields) = text
            .rsplit_once(')')
            .ok_or_else(|| anyhow::anyhow!("invalid process stat"))?;
        let fields: Vec<_> = fields.split_whitespace().collect();
        // A process exiting between open and read can leave a truncated record;
        // state/pgrp/starttime sit at fixed offsets, so refuse short input before
        // indexing instead of panicking on a race.
        anyhow::ensure!(fields.len() >= 20, "short process stat for {pid}");
        if fields[0] == "Z" {
            return Ok(None);
        }
        Ok(Some(Self {
            pid,
            group: fields[2].parse()?,
            started: fields[19].parse()?,
        }))
    }

    /// An in-memory group observation also tracks descendants that survive TERM.
    pub async fn stop_group(&self) -> anyhow::Result<()> {
        super::group::Group {
            leader: self.clone(),
            token: None,
        }
        .stop()
        .await
    }
}

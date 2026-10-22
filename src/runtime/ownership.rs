//! OS ownership covers engineering execution, not the lifetime of a chat daemon.
use super::{conversation::Mode, runner::Runner};
use crate::{
    domain::{EventData, Status},
    tools::process::ProcessRecord,
};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
struct Owner {
    session: String,
    logs: PathBuf,
}

pub(super) struct Lease(std::fs::File);
impl Lease {
    fn release(self) -> anyhow::Result<()> {
        // Empty means clean handover. A crash leaves the previous journal pointer
        // so the successor can reap orphan writers before starting any model.
        self.0.set_len(0)?;
        self.0.sync_all()?;
        Ok(())
    }
}

impl Runner {
    pub(super) async fn execution_owner(&mut self) -> anyhow::Result<bool> {
        let needs_owner = self.recover
            || (self.session.mode == Mode::Engineering
                && matches!(
                    self.session.status,
                    Status::Ready | Status::Running | Status::Waiting
                )
                && (self.session.status != Status::Waiting
                    || self.session.owner.is_some()
                    || !self
                        .session
                        .actors
                        .values()
                        .filter(|a| !a.retired)
                        .all(|a| {
                            matches!(
                                a.phase,
                                super::actor::Phase::Done | super::actor::Phase::Blocked
                            )
                        })));
        if !needs_owner {
            if self.jobs.is_empty() {
                if let Some(lease) = self.lease.take() {
                    lease.release()?;
                }
                self.session.owner = None;
                self.session.owner_status = None;
            }
            return Ok(true);
        }
        if self.lease.is_some() {
            return Ok(true);
        }
        let mut file = owner_file(&self.session.workspace)?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let error = std::io::Error::last_os_error();
            anyhow::ensure!(
                error.raw_os_error() == Some(libc::EWOULDBLOCK),
                "workspace lock: {error}"
            );
            let mut content = String::new();
            file.read_to_string(&mut content)?;
            let owner = serde_json::from_str::<Owner>(&content)
                .map(|o| o.session)
                .unwrap_or_else(|_| content.trim().to_owned());
            // A new message/steering result can set Running without acquiring
            // the lease. The OS lock remains authoritative even when its owner
            // has not changed; retain the original handover destination.
            if self.session.owner.as_deref() != Some(&owner)
                || self.session.status != Status::Waiting
            {
                if self.session.owner_status.is_none() {
                    self.session.owner_status = Some(
                        self.session
                            .transition
                            .clone()
                            .unwrap_or_else(|| self.session.status.clone()),
                    );
                }
                self.session.owner = Some(owner.clone());
                self.status(
                    Status::Waiting,
                    &format!("waiting for workspace execution owner {owner}"),
                )?;
            }
            return Ok(false);
        }
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        if let Some(previous) = self.previous_owner(&content)? {
            reap(&previous.logs).await?;
            self.emit(EventData::Diagnostic {
                message: format!(
                    "reconciled orphan processes from previous workspace owner {}",
                    previous.session
                ),
            })?;
        }
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&serde_json::to_vec(&Owner {
            session: self.session.id.clone(),
            logs: self.tools.logs.clone(),
        })?)?;
        file.sync_all()?;
        self.lease = Some(Lease(file));
        if self.recover {
            super::recovery::reconcile(&mut self.session, &self.store, &self.tools.logs).await?;
            self.recover = false;
        }
        let destination = self.session.owner_status.take().unwrap_or(Status::Running);
        if self.session.owner.take().is_some() && self.session.status == Status::Waiting {
            self.status(destination, "workspace execution ownership acquired")?;
        }
        Ok(true)
    }

    fn previous_owner(&self, content: &str) -> anyhow::Result<Option<Owner>> {
        let content = content.trim();
        if content.is_empty() {
            return Ok(None);
        }
        if let Ok(owner) = serde_json::from_str::<Owner>(content) {
            return Ok(Some(owner));
        }

        // Older daemons stored only a session ID. Resolve it exclusively inside
        // this known state home and verify its workspace before touching journals.
        // Never erase an unknown nonempty lease: its orphan writers are unproven.
        let unresolved = || {
            anyhow::anyhow!(
                "cannot reconcile legacy workspace owner {content:?}; resume that session using its original --home to settle its process journals before starting another task"
            )
        };
        crate::config::validate_id(content).map_err(|_| unresolved())?;
        let current = self.tools.logs.parent().ok_or_else(unresolved)?;
        let sessions = current.parent().ok_or_else(unresolved)?;
        anyhow::ensure!(
            self.tools
                .logs
                .file_name()
                .is_some_and(|name| name == "logs")
                && current
                    .file_name()
                    .is_some_and(|name| name == self.session.id.as_str())
                && sessions.file_name().is_some_and(|name| name == "sessions"),
            "{}",
            unresolved()
        );
        let previous: super::state::Session = self.store.load(content).map_err(|_| unresolved())?;
        anyhow::ensure!(
            previous.workspace.canonicalize()? == self.session.workspace.canonicalize()?,
            "{}",
            unresolved()
        );
        let logs = sessions.join(content).join("logs");
        anyhow::ensure!(logs.is_dir(), "{}", unresolved());
        Ok(Some(Owner {
            session: content.into(),
            logs,
        }))
    }
}

async fn reap(logs: &Path) -> anyhow::Result<()> {
    if !logs.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(logs)? {
        let path = entry?.path();
        if path.to_string_lossy().ends_with(".process.json") {
            let record: ProcessRecord = serde_json::from_slice(&std::fs::read(path)?)?;
            if record.output.is_none() {
                record.stop().await?;
            }
        }
    }
    Ok(())
}

fn owner_file(workspace: &Path) -> anyhow::Result<std::fs::File> {
    let path = workspace.canonicalize()?;
    let hash = blake3::hash(path.as_os_str().as_encoded_bytes());
    let path = PathBuf::from(format!(
        "/tmp/loh-owner-{}-{}.lock",
        unsafe { libc::geteuid() },
        &hash.to_hex()[..24]
    ));
    Ok(std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?)
}

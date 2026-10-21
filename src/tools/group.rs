//! Process-group ownership survives the leader. The inherited nonce identifies
//! orphaned members during recovery without treating a recycled PGID as ownership.
use super::identity::Identity;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};

pub const OWNER_ENV: &str = "LOH_GROUP";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Group {
    #[serde(flatten)]
    pub leader: Identity,
    #[serde(default)]
    pub token: Option<String>,
}

impl Group {
    pub async fn stop(&self) -> anyhow::Result<()> {
        let mut known = BTreeMap::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        loop {
            let members = self.members(&mut known)?;
            if members.is_empty() {
                // Confirm a second observation; a descendant can be forked while
                // the first process inventory is being collected.
                tokio::time::sleep(Duration::from_millis(20)).await;
                if self.members(&mut known)?.is_empty() {
                    return Ok(());
                }
                continue;
            }
            let signal = if tokio::time::Instant::now() < deadline {
                libc::SIGTERM
            } else {
                libc::SIGKILL
            };
            for member in &members {
                signal_member(member, signal)?;
            }
            // Signal acceptance is not exit. Never release an owned writer merely
            // because a grace period elapsed or the original leader was reaped.
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Last-resort cleanup when an owning future is dropped. Normal paths use
    /// stop() and await confirmation before returning a result.
    pub fn kill(&self) -> anyhow::Result<()> {
        for member in self.members(&mut BTreeMap::new())? {
            signal_member(&member, libc::SIGKILL)?;
        }
        Ok(())
    }

    fn members(&self, known: &mut BTreeMap<u32, Identity>) -> anyhow::Result<Vec<Identity>> {
        anyhow::ensure!(
            self.leader.pid == self.leader.group,
            "saved process was not its own group leader"
        );
        let anchor = Identity::capture(self.leader.pid)?;
        anyhow::ensure!(
            anchor
                .as_ref()
                .is_none_or(|current| current == &self.leader),
            "process identity changed; refusing reused PID"
        );
        let inventory = std::process::Command::new("/bin/ps")
            .args(["-axo", "pid=,pgid="])
            .output()?;
        anyhow::ensure!(inventory.status.success(), "cannot inspect process groups");
        let mut members = vec![];
        for line in String::from_utf8(inventory.stdout)?.lines() {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() != 2 || fields[1].parse::<u32>() != Ok(self.leader.group) {
                continue;
            }
            let pid: u32 = fields[0].parse()?;
            let Some(member) = Identity::capture(pid)? else {
                continue;
            };
            if member.group != self.leader.group {
                continue;
            }
            let owned = anchor.as_ref() == Some(&self.leader)
                || known.get(&pid) == Some(&member)
                || self
                    .token
                    .as_ref()
                    .is_some_and(|token| has_token(pid, token).unwrap_or(false));
            anyhow::ensure!(
                owned,
                "cannot establish ownership of process {pid}; reconciliation required"
            );
            known.insert(pid, member.clone());
            members.push(member);
        }
        Ok(members)
    }
}

fn signal_member(identity: &Identity, signal: i32) -> anyhow::Result<()> {
    if Identity::capture(identity.pid)?.as_ref() == Some(identity) {
        let result = unsafe { libc::kill(identity.pid as i32, signal) };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            anyhow::ensure!(
                error.raw_os_error() == Some(libc::ESRCH),
                "cannot signal owned process: {error}"
            );
        }
    }
    Ok(())
}

fn has_token(pid: u32, token: &str) -> anyhow::Result<bool> {
    let bytes = arguments(pid)?;
    let expected = format!("{OWNER_ENV}={token}");
    // Inspect only members of the saved group; never log the argument/environment
    // buffer, which can contain unrelated credentials inherited by that child.
    Ok(bytes
        .split(|byte| *byte == 0)
        .any(|entry| entry == expected.as_bytes()))
}

#[cfg(target_os = "macos")]
fn arguments(pid: u32) -> anyhow::Result<Vec<u8>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as i32];
    let mut size = 0;
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    anyhow::ensure!(
        result == 0 && size <= 16 * 1024 * 1024,
        "cannot inspect owned process environment"
    );
    let mut bytes = vec![0; size];
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            bytes.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    anyhow::ensure!(result == 0, "cannot inspect owned process environment");
    bytes.truncate(size);
    Ok(bytes)
}

#[cfg(target_os = "linux")]
fn arguments(pid: u32) -> anyhow::Result<Vec<u8>> {
    Ok(std::fs::read(format!("/proc/{pid}/environ"))?)
}

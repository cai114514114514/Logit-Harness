//! Credential files are never handled by the session/event store.
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        io::AsRawFd,
    },
    path::Path,
};

pub(super) fn read(path: &Path) -> anyhow::Result<Option<String>> {
    let mut file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => anyhow::bail!("无法安全读取凭据文件"),
    };
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.permissions().mode() & 0o077 == 0,
        "凭据文件必须属于当前用户且权限为 600"
    );
    anyhow::ensure!(metadata.len() <= 1024 * 1024, "凭据文件过大");
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|_| anyhow::anyhow!("凭据文件无法读取"))?;
    Ok(Some(text))
}

pub(super) fn write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("缺少配置目录"))?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|_| anyhow::anyhow!("无法保存配置文件"))?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

/// Separate clients may configure different endpoints concurrently. Keep their
/// credentials and non-secret config updates within the same local lock.
pub(super) fn lock(home: &Path) -> anyhow::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(home.join("config.lock"))?;
    anyhow::ensure!(
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0,
        "无法锁定配置"
    );
    Ok(file)
}

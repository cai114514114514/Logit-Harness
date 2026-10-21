use serde::Serialize;
use std::{
    io::{Read, Write},
    os::fd::AsRawFd,
    path::Path,
};

pub fn save(path: &Path, value: &impl Serialize) -> anyhow::Result<()> {
    write(path, &serde_json::to_vec_pretty(value)?)
}
pub fn write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("artifact path needs a parent"))?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}
pub fn hash(path: &Path) -> anyhow::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut buffer = [0; 65536];
    let mut hash = blake3::Hasher::new();
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hash.finalize().to_hex().to_string())
}
pub fn lock(path: &Path, wait: bool) -> anyhow::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let flags = libc::LOCK_EX | if wait { 0 } else { libc::LOCK_NB };
    anyhow::ensure!(
        unsafe { libc::flock(file.as_raw_fd(), flags) } == 0,
        "another process owns {}",
        path.display()
    );
    Ok(file)
}

// SPDX-License-Identifier: AGPL-3.0-only
use anyhow::Result;
use std::{fs, io::Write, path::Path};
use yappers_netkit::new_secret;

/// Keep the lock for the entire read–modify–write operation, including retries.
pub(crate) struct PrivateFileLock {
    file: fs::File,
}
impl PrivateFileLock {
    pub(crate) fn acquire(lock: &Path) -> Result<Self> {
        if let Some(parent) = lock.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let mut options = fs::OpenOptions::new();
        options.create(true).write(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(lock)?;
        file.try_lock()?;
        Ok(Self { file })
    }
}
impl Drop for PrivateFileLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
pub(crate) fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("{}.tmp", &new_secret()[..16]));
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> Result<()> {
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    let _ = fs::remove_file(temporary);
    result
}

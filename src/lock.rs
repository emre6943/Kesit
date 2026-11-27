//! Advisory file locks: one daemon per user, one selection overlay at a time.
//!
//! The OS releases a lock when its file handle is dropped or the process
//! dies, so a crash never leaves a stale lock behind.

use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Held for as long as the value lives.
#[derive(Debug)]
pub struct Lock {
    _file: File,
}

/// `<runtime or cache dir>/kesit`. Both are per-user directories; a shared
/// one such as `/tmp` is never used, so another user cannot plant the lock.
fn lock_dir() -> Result<PathBuf> {
    dirs::runtime_dir()
        .or_else(dirs::cache_dir)
        .map(|dir| dir.join("kesit"))
        .context("no runtime or cache directory for the lock files")
}

pub fn daemon_lock_path() -> Result<PathBuf> {
    Ok(lock_dir()?.join("daemon.lock"))
}

/// Held by `kesit capture` while it grabs the screen and shows the overlay;
/// released before the editor opens, so editors never block a new capture.
pub fn overlay_lock_path() -> Result<PathBuf> {
    Ok(lock_dir()?.join("overlay.lock"))
}

/// Takes an exclusive lock on `path` without waiting. `Ok(None)` means
/// another holder has it. Writes the holder's PID into the file.
pub fn try_lock(path: &Path) -> Result<Option<Lock>> {
    if let Some(dir) = path.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .with_context(|| format!("creating {}", dir.display()))?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    match file.try_lock() {
        Ok(()) => {
            file.set_len(0).ok();
            let _ = writeln!(file, "{}", std::process::id());
            Ok(Some(Lock { _file: file }))
        }
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(err)) => {
            Err(err).with_context(|| format!("locking {}", path.display()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn second_holder_is_refused_until_release() {
        let dir = std::env::temp_dir().join(format!("kesit-lock-{}", std::process::id()));
        let path = dir.join("overlay.lock");
        let first = try_lock(&path).expect("lock").expect("free");
        assert!(try_lock(&path).expect("lock").is_none(), "held twice");
        drop(first);
        let again = try_lock(&path).expect("lock");
        assert!(again.is_some(), "released on drop");
        drop(again);
        let mode = fs::metadata(&path).expect("metadata").permissions().mode();
        assert_eq!(mode & 0o077, 0, "lock file is private: {mode:o}");
        let _ = fs::remove_dir_all(&dir);
    }
}

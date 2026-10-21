//! Single-instance protection: an exclusive OS file lock per channel. The
//! lock is released automatically when the process dies.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{EngineError, Result};

pub struct InstanceLock {
    _file: File,
    path: PathBuf,
}

impl InstanceLock {
    /// Acquire `<state_dir>/channel.lock` or fail if another process holds it.
    pub fn acquire(state_dir: &Path, channel: &str) -> Result<Self> {
        std::fs::create_dir_all(state_dir)?;
        let path = state_dir.join("channel.lock");
        let mut file = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(&path)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(EngineError::AlreadyRunning(channel.to_string()));
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
        }
        let _ = file.set_len(0);
        let _ = writeln!(file, "{}", std::process::id());
        Ok(InstanceLock { _file: file, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_lock_fails() {
        let dir = tempfile::tempdir().unwrap();
        let a = InstanceLock::acquire(dir.path(), "main").unwrap();
        assert!(matches!(InstanceLock::acquire(dir.path(), "main"), Err(EngineError::AlreadyRunning(_))));
        drop(a);
        assert!(InstanceLock::acquire(dir.path(), "main").is_ok());
    }
}

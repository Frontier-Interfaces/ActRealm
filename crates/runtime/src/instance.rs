use crate::fsutil::ensure_private_directory;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InstanceError {
    #[error("another Flow Agent runtime already owns {0}")]
    AlreadyRunning(PathBuf),
    #[error("runtime lock I/O failed: {0}")]
    Io(#[from] io::Error),
}

pub struct RuntimeInstanceGuard {
    file: File,
    path: PathBuf,
}

impl RuntimeInstanceGuard {
    pub fn acquire(path: impl Into<PathBuf>) -> Result<Self, InstanceError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            let _ = ensure_private_directory(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;

        // SAFETY: flock receives the live descriptor owned by `file` and does
        // not retain it. The guard keeps the descriptor open for its lifetime.
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result != 0 {
            let error = io::Error::last_os_error();
            if error
                .raw_os_error()
                .is_some_and(|code| code == libc::EWOULDBLOCK || code == libc::EAGAIN)
            {
                return Err(InstanceError::AlreadyRunning(path));
            }
            return Err(InstanceError::Io(error));
        }

        file.set_len(0)?;
        writeln!(file, "{}", std::process::id())?;
        file.sync_data()?;
        Ok(Self { file, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for RuntimeInstanceGuard {
    fn drop(&mut self) {
        // SAFETY: the descriptor remains valid until this drop returns.
        let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

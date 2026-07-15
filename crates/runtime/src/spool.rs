use crate::fsutil::ensure_private_directory;
use flow_agent_core::BridgeRequest;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use thiserror::Error;

const DEFAULT_MAX_FILES: usize = 500;
const DEFAULT_MAX_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum SpoolError {
    #[error("permission requests must never be spooled")]
    PermissionRequest,
    #[error("spool I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("spool serialization failed: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct EventSpool {
    path: PathBuf,
    max_files: usize,
    max_bytes: u64,
}

pub fn default_spool_path() -> PathBuf {
    if let Some(root) = env::var_os("FLOW_AGENT_HOME") {
        return PathBuf::from(root).join("spool");
    }
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".flow-agent/spool")
}

impl Default for EventSpool {
    fn default() -> Self {
        Self::new(default_spool_path())
    }
}

impl EventSpool {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            max_files: DEFAULT_MAX_FILES,
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }

    pub fn with_limits(path: impl Into<PathBuf>, max_files: usize, max_bytes: u64) -> Self {
        Self {
            path: path.into(),
            max_files,
            max_bytes,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, request: &BridgeRequest) -> Result<(), SpoolError> {
        if request.needs_reply {
            return Err(SpoolError::PermissionRequest);
        }
        self.ensure_directory()?;
        let bytes = serde_json::to_vec(request)?;
        let final_path = self
            .path
            .join(format!("{:020}-{}.json", request.received_at, request.id));
        let temporary_path = self
            .path
            .join(format!(".{}.{}.tmp", std::process::id(), request.id));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temporary_path)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary_path, &final_path)?;
        fs::set_permissions(&final_path, fs::Permissions::from_mode(0o600))?;
        self.prune()?;
        Ok(())
    }

    pub fn drain<F>(&self, mut ingest: F) -> Result<usize, SpoolError>
    where
        F: FnMut(BridgeRequest) -> bool,
    {
        if !self.path.exists() {
            return Ok(0);
        }
        let mut drained = 0;
        for path in self.entries()? {
            let bytes = fs::read(&path)?;
            let request = match serde_json::from_slice::<BridgeRequest>(&bytes) {
                Ok(request) if !request.needs_reply => request,
                Ok(_) | Err(_) => {
                    fs::remove_file(path)?;
                    continue;
                }
            };
            if !ingest(request) {
                break;
            }
            fs::remove_file(path)?;
            drained += 1;
        }
        Ok(drained)
    }

    pub fn len(&self) -> Result<usize, SpoolError> {
        Ok(self.entries()?.len())
    }

    pub fn is_empty(&self) -> Result<bool, SpoolError> {
        Ok(self.entries()?.is_empty())
    }

    fn ensure_directory(&self) -> Result<(), io::Error> {
        let _ = ensure_private_directory(&self.path)?;
        Ok(())
    }

    fn entries(&self) -> Result<Vec<PathBuf>, io::Error> {
        let mut entries = Vec::new();
        if !self.path.exists() {
            return Ok(entries);
        }
        for item in fs::read_dir(&self.path)? {
            let item = item?;
            let path = item.path();
            if path.extension().and_then(|value| value.to_str()) == Some("json") {
                entries.push(path);
            }
        }
        entries.sort();
        Ok(entries)
    }

    fn prune(&self) -> Result<(), io::Error> {
        let mut entries = self.entries()?;
        let mut bytes = entries.iter().try_fold(0_u64, |total, path| {
            fs::metadata(path).map(|metadata| total.saturating_add(metadata.len()))
        })?;
        while entries.len() > self.max_files || bytes > self.max_bytes {
            let oldest = entries.remove(0);
            bytes = bytes.saturating_sub(fs::metadata(&oldest)?.len());
            fs::remove_file(oldest)?;
        }
        Ok(())
    }
}

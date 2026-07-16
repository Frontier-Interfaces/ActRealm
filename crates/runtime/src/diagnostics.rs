use crate::fsutil::ensure_private_directory;
use flow_agent_core::BridgeRequest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

const CONFIG_SCHEMA_VERSION: u16 = 1;
const MAX_CONFIG_BYTES: u64 = 4 * 1024;
pub const MAX_DIAGNOSTIC_CAPTURE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Error)]
pub enum DiagnosticCaptureError {
    #[error("diagnostic capture duration must be between 1 and 60 minutes")]
    InvalidDuration,
    #[error("unsafe symbolic link refused: {0}")]
    SymlinkRefused(PathBuf),
    #[error("diagnostic path is not a private directory: {0}")]
    UnsafeDirectory(PathBuf),
    #[error("diagnostic configuration is invalid: {0}")]
    InvalidConfig(String),
    #[error("diagnostic I/O failed for {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticCaptureStatus {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    pub path: PathBuf,
    pub bytes: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CaptureConfig {
    schema_version: u16,
    expires_at: u64,
}

#[derive(Debug, Clone)]
pub struct DiagnosticCapture {
    root: PathBuf,
}

impl DiagnosticCapture {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn enable(
        &self,
        minutes: u64,
        now: u64,
    ) -> Result<DiagnosticCaptureStatus, DiagnosticCaptureError> {
        if !(1..=60).contains(&minutes) {
            return Err(DiagnosticCaptureError::InvalidDuration);
        }
        self.ensure_private_root()?;
        let config = CaptureConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            expires_at: now.saturating_add(minutes.saturating_mul(60_000)),
        };
        let bytes = serde_json::to_vec_pretty(&config)
            .map_err(|error| DiagnosticCaptureError::InvalidConfig(error.to_string()))?;
        self.atomic_write(&self.config_path(), &bytes)?;
        self.status(now)
    }

    pub fn status(&self, now: u64) -> Result<DiagnosticCaptureStatus, DiagnosticCaptureError> {
        let Some(config) = self.read_config()? else {
            return Ok(DiagnosticCaptureStatus {
                enabled: false,
                expires_at: None,
                path: self.events_path(),
                bytes: 0,
            });
        };
        if config.expires_at <= now {
            self.clear()?;
            return Ok(DiagnosticCaptureStatus {
                enabled: false,
                expires_at: None,
                path: self.events_path(),
                bytes: 0,
            });
        }
        let bytes = self.safe_file_len(&self.events_path())?.unwrap_or(0);
        Ok(DiagnosticCaptureStatus {
            enabled: true,
            expires_at: Some(config.expires_at),
            path: self.events_path(),
            bytes,
        })
    }

    pub fn capture(&self, request: &BridgeRequest, now: u64) -> Result<(), DiagnosticCaptureError> {
        let Some(config) = self.read_config()? else {
            return Ok(());
        };
        if config.expires_at <= now {
            self.clear()?;
            return Ok(());
        }
        self.ensure_private_root()?;
        let payload_bytes = serde_json::to_vec(&request.raw)
            .map(|bytes| bytes.len())
            .unwrap_or(0);
        let mut line = serde_json::to_vec(&json!({
            "schemaVersion": 1,
            "capturedAt": now,
            "provider": request.provider,
            "event": stable_event_name(request.event_name()),
            "needsReply": request.needs_reply,
            "payloadBytes": payload_bytes,
        }))
        .map_err(|error| DiagnosticCaptureError::InvalidConfig(error.to_string()))?;
        line.push(b'\n');
        let path = self.events_path();
        let current = self.safe_file_len(&path)?.unwrap_or(0);
        let truncate = current.saturating_add(line.len() as u64) > MAX_DIAGNOSTIC_CAPTURE_BYTES;
        let mut options = OpenOptions::new();
        options
            .create(true)
            .write(true)
            .append(!truncate)
            .truncate(truncate)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW);
        let mut file = options
            .open(&path)
            .map_err(|source| io_error(&path, source))?;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|source| io_error(&path, source))?;
        file.write_all(&line)
            .map_err(|source| io_error(&path, source))?;
        file.flush().map_err(|source| io_error(&path, source))
    }

    pub fn clear(&self) -> Result<(), DiagnosticCaptureError> {
        if !self.validate_existing_root()? {
            return Ok(());
        }
        for path in [self.events_path(), self.config_path()] {
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(DiagnosticCaptureError::SymlinkRefused(path))
                }
                Ok(metadata) if !metadata.is_file() => {
                    return Err(DiagnosticCaptureError::UnsafeDirectory(path))
                }
                Ok(_) => fs::remove_file(&path).map_err(|source| io_error(&path, source))?,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(io_error(&path, error)),
            }
        }
        match fs::remove_dir(&self.root) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io_error(&self.root, error)),
        }
    }

    fn config_path(&self) -> PathBuf {
        self.root.join("config.json")
    }

    fn events_path(&self) -> PathBuf {
        self.root.join("events.jsonl")
    }

    fn ensure_private_root(&self) -> Result<(), DiagnosticCaptureError> {
        if self.validate_existing_root()? {
            return Ok(());
        }
        ensure_private_directory(&self.root).map_err(|source| io_error(&self.root, source))?;
        Ok(())
    }

    fn validate_existing_root(&self) -> Result<bool, DiagnosticCaptureError> {
        match fs::symlink_metadata(&self.root) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                Err(DiagnosticCaptureError::SymlinkRefused(self.root.clone()))
            }
            Ok(metadata) if !metadata.is_dir() => {
                Err(DiagnosticCaptureError::UnsafeDirectory(self.root.clone()))
            }
            Ok(metadata) if metadata.permissions().mode() & 0o077 != 0 => {
                Err(DiagnosticCaptureError::UnsafeDirectory(self.root.clone()))
            }
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(io_error(&self.root, error)),
        }
    }

    fn read_config(&self) -> Result<Option<CaptureConfig>, DiagnosticCaptureError> {
        if !self.validate_existing_root()? {
            return Ok(None);
        }
        let path = self.config_path();
        let Some(length) = self.safe_file_len(&path)? else {
            return Ok(None);
        };
        if length > MAX_CONFIG_BYTES {
            return Err(DiagnosticCaptureError::InvalidConfig(
                "configuration exceeds 4 KiB".to_owned(),
            ));
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .map_err(|source| io_error(&path, source))?;
        let mut bytes = Vec::with_capacity(length as usize);
        file.take(MAX_CONFIG_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| io_error(&path, source))?;
        let config: CaptureConfig = serde_json::from_slice(&bytes)
            .map_err(|error| DiagnosticCaptureError::InvalidConfig(error.to_string()))?;
        if config.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(DiagnosticCaptureError::InvalidConfig(
                "unsupported schema version".to_owned(),
            ));
        }
        Ok(Some(config))
    }

    fn safe_file_len(&self, path: &Path) -> Result<Option<u64>, DiagnosticCaptureError> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                Err(DiagnosticCaptureError::SymlinkRefused(path.to_path_buf()))
            }
            Ok(metadata) if !metadata.is_file() => {
                Err(DiagnosticCaptureError::UnsafeDirectory(path.to_path_buf()))
            }
            Ok(metadata) => Ok(Some(metadata.len())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(io_error(path, error)),
        }
    }

    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> Result<(), DiagnosticCaptureError> {
        if fs::symlink_metadata(path)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(DiagnosticCaptureError::SymlinkRefused(path.to_path_buf()));
        }
        let temporary = self.root.join(format!(".config-{}.tmp", Uuid::now_v7()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&temporary)
                .map_err(|source| io_error(&temporary, source))?;
            file.write_all(bytes)
                .map_err(|source| io_error(&temporary, source))?;
            file.sync_all()
                .map_err(|source| io_error(&temporary, source))?;
            fs::rename(&temporary, path).map_err(|source| io_error(path, source))?;
            File::open(&self.root)
                .and_then(|directory| directory.sync_all())
                .map_err(|source| io_error(&self.root, source))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn stable_event_name(name: Option<&str>) -> &'static str {
    match name {
        Some("SessionStart") => "SessionStart",
        Some("SessionEnd") => "SessionEnd",
        Some("UserPromptSubmit") => "UserPromptSubmit",
        Some("PreToolUse") => "PreToolUse",
        Some("PostToolUse") => "PostToolUse",
        Some("PostToolUseFailure") => "PostToolUseFailure",
        Some("PermissionRequest") => "PermissionRequest",
        Some("Notification") => "Notification",
        Some("SubagentStart") => "SubagentStart",
        Some("SubagentStop") => "SubagentStop",
        Some("TaskCreate") => "TaskCreate",
        Some("TaskCompleted") => "TaskCompleted",
        Some("PreCompact") => "PreCompact",
        Some("Stop") => "Stop",
        Some("StopFailure") => "StopFailure",
        _ => "unknown",
    }
}

fn io_error(path: &Path, source: io::Error) -> DiagnosticCaptureError {
    DiagnosticCaptureError::Io {
        path: path.to_path_buf(),
        source,
    }
}

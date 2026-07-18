use actrealm_core::{BridgeRequest, BridgeResponse, ReplyAction, MAX_HOOK_PAYLOAD_BYTES};
use std::env;
use std::fs::{self, DirBuilder};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::time::Instant;
use thiserror::Error;
use uuid::Uuid;

pub const MAX_BRIDGE_FRAME_BYTES: usize = MAX_HOOK_PAYLOAD_BYTES + 64 * 1024;

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("bridge I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("bridge protocol failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("bridge frame exceeds {maximum} bytes")]
    FrameTooLarge { maximum: usize },
    #[error("bridge closed without a response")]
    MissingResponse,
    #[error("bridge response id mismatch: expected {expected}, received {received}")]
    MismatchedRequestId { expected: Uuid, received: Uuid },
    #[error("bridge runtime instance changed while waiting")]
    RuntimeChanged,
    #[error("bridge response deadline exceeded")]
    DeadlineExceeded,
    #[error("Unix socket path is {actual} bytes; maximum supported length is {maximum}: {path}")]
    SocketPathTooLong {
        path: PathBuf,
        actual: usize,
        maximum: usize,
    },
}

pub fn default_socket_path() -> PathBuf {
    if let Some(root) = env::var_os("ACTREALM_HOME") {
        return PathBuf::from(root).join("run/bridge.sock");
    }
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".actrealm/run/bridge.sock")
}

pub fn unix_socket_path_limit() -> usize {
    // SAFETY: sockaddr_un is a plain C data structure. A zeroed value is valid
    // for measuring the fixed sun_path array and is never passed to the OS.
    let address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_path.len().saturating_sub(1)
}

pub fn validate_socket_path(path: &Path) -> Result<(), BridgeError> {
    let actual = path.as_os_str().as_bytes().len();
    let maximum = unix_socket_path_limit();
    if actual > maximum {
        return Err(BridgeError::SocketPathTooLong {
            path: path.to_path_buf(),
            actual,
            maximum,
        });
    }
    Ok(())
}

pub struct BridgeClient {
    socket_path: PathBuf,
}

impl BridgeClient {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    pub fn send(
        &self,
        request: &BridgeRequest,
        timeout: Duration,
    ) -> Result<Option<BridgeResponse>, BridgeError> {
        validate_socket_path(&self.socket_path)?;
        let mut stream = UnixStream::connect(&self.socket_path)?;
        stream.set_write_timeout(Some(Duration::from_millis(200)))?;
        serde_json::to_writer(&mut stream, request)?;
        stream.write_all(b"\n")?;
        stream.flush()?;

        if !request.needs_reply {
            return Ok(None);
        }

        let expected = request.request_id.ok_or(BridgeError::MissingResponse)?;
        let deadline = Instant::now() + timeout;
        let mut reader = BufReader::new(stream);
        let mut runtime_instance_id = None;
        loop {
            if reader.buffer().is_empty() {
                let now = Instant::now();
                if now >= deadline {
                    return Err(BridgeError::DeadlineExceeded);
                }
                let wait = deadline
                    .saturating_duration_since(now)
                    .min(Duration::from_secs(2));
                let timeout_ms = wait.as_millis().max(1).min(i32::MAX as u128) as i32;
                let mut descriptor = libc::pollfd {
                    fd: reader.get_ref().as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                // SAFETY: poll receives one live UnixStream descriptor and does not
                // retain the pointer after returning.
                let ready = unsafe { libc::poll(&mut descriptor, 1, timeout_ms) };
                if ready == 0 {
                    if Instant::now() >= deadline {
                        return Err(BridgeError::DeadlineExceeded);
                    }
                    continue;
                }
                if ready < 0 {
                    return Err(BridgeError::Io(io::Error::last_os_error()));
                }
            }
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => return Err(BridgeError::MissingResponse),
                Ok(_) => {}
                Err(error) => return Err(BridgeError::Io(error)),
            }
            if line.trim().is_empty() {
                return Err(BridgeError::MissingResponse);
            }
            let response: BridgeResponse = serde_json::from_str(&line)?;
            if response.request_id != expected {
                return Err(BridgeError::MismatchedRequestId {
                    expected,
                    received: response.request_id,
                });
            }
            if let Some(incoming_instance_id) = response.runtime_instance_id {
                if runtime_instance_id.is_some_and(|known| known != incoming_instance_id) {
                    return Err(BridgeError::RuntimeChanged);
                }
                runtime_instance_id = Some(incoming_instance_id);
            }
            if response.action == ReplyAction::Ping {
                continue;
            }
            return Ok(Some(response));
        }
    }
}

pub struct BridgeListener {
    listener: UnixListener,
    socket_path: PathBuf,
}

impl BridgeListener {
    pub fn bind(socket_path: impl Into<PathBuf>) -> Result<Self, BridgeError> {
        let socket_path = socket_path.into();
        validate_socket_path(&socket_path)?;
        if let Some(parent) = socket_path.parent() {
            let created_parent = !parent.exists();
            if created_parent {
                let mut builder = DirBuilder::new();
                builder.recursive(true).mode(0o700).create(parent)?;
            }
            // Never chmod an arbitrary existing directory supplied via --socket
            // (for example /private/tmp). Directories created by actrealm are
            // private from their first usable moment.
            if created_parent {
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
            }
        }
        if socket_path.exists() {
            fs::remove_file(&socket_path)?;
        }
        let listener = UnixListener::bind(&socket_path)?;
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;
        Ok(Self {
            listener,
            socket_path,
        })
    }

    pub fn incoming(&self) -> impl Iterator<Item = io::Result<UnixStream>> + '_ {
        self.listener.incoming()
    }

    pub fn read_request(stream: &mut UnixStream) -> Result<BridgeRequest, BridgeError> {
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut frame = Vec::new();
        loop {
            let available = reader.fill_buf()?;
            if available.is_empty() {
                break;
            }
            let take = available
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(available.len(), |position| position + 1);
            if frame.len().saturating_add(take) > MAX_BRIDGE_FRAME_BYTES {
                return Err(BridgeError::FrameTooLarge {
                    maximum: MAX_BRIDGE_FRAME_BYTES,
                });
            }
            frame.extend_from_slice(&available[..take]);
            reader.consume(take);
            if frame.last() == Some(&b'\n') {
                break;
            }
        }
        if frame.iter().all(u8::is_ascii_whitespace) {
            return Err(BridgeError::MissingResponse);
        }
        Ok(serde_json::from_slice(&frame)?)
    }

    pub fn write_response(
        stream: &mut UnixStream,
        response: &BridgeResponse,
    ) -> Result<(), BridgeError> {
        serde_json::to_writer(&mut *stream, response)?;
        stream.write_all(b"\n")?;
        stream.flush()?;
        Ok(())
    }
}

impl Drop for BridgeListener {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket_path);
    }
}

#[cfg(test)]
fn socket_is_private(path: &Path) -> io::Result<bool> {
    Ok(fs::metadata(path)?.permissions().mode() & 0o777 == 0o600)
}

#[cfg(test)]
mod tests {
    use super::*;
    use actrealm_core::{Decision, Provider};
    use std::thread;

    fn temp_socket(name: &str) -> PathBuf {
        env::temp_dir().join(format!("actrealm-{name}-{}.sock", std::process::id()))
    }

    #[test]
    fn round_trips_a_blocking_request() {
        let path = temp_socket("roundtrip");
        let listener = BridgeListener::bind(&path).unwrap();
        assert!(socket_is_private(&path).unwrap());

        let request = BridgeRequest::from_hook(
            Provider::Claude,
            serde_json::json!({
                "hook_event_name": "PermissionRequest",
                "session_id": "session-1"
            }),
        );
        let expected_id = request.request_id.unwrap();
        let server = thread::spawn(move || {
            let mut stream = listener.incoming().next().unwrap().unwrap();
            let request = BridgeListener::read_request(&mut stream).unwrap();
            BridgeListener::write_response(
                &mut stream,
                &BridgeResponse::decided(request.request_id.unwrap(), Decision::Allow),
            )
            .unwrap();
        });

        let response = BridgeClient::new(path)
            .send(&request, Duration::from_secs(1))
            .unwrap()
            .unwrap();
        server.join().unwrap();

        assert_eq!(response.request_id, expected_id);
        assert_eq!(response.decision(), Some(Decision::Allow));
    }
}

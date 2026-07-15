use flow_agent_core::{BridgeRequest, BridgeResponse, ReplyAction};
use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("bridge I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("bridge protocol failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("bridge closed without a response")]
    MissingResponse,
    #[error("bridge response id mismatch: expected {expected}, received {received}")]
    MismatchedRequestId { expected: Uuid, received: Uuid },
    #[error("bridge runtime instance changed while waiting")]
    RuntimeChanged,
    #[error("bridge response deadline exceeded")]
    DeadlineExceeded,
}

pub fn default_socket_path() -> PathBuf {
    if let Some(root) = env::var_os("FLOW_AGENT_HOME") {
        return PathBuf::from(root).join("run/bridge.sock");
    }
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".flow-agent/run/bridge.sock")
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
        if let Some(parent) = socket_path.parent() {
            let created_parent = !parent.exists();
            fs::create_dir_all(parent)?;
            // Never chmod an arbitrary existing directory supplied via --socket
            // (for example /private/tmp). Directories created by flow-agent are
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
        let mut line = String::new();
        BufReader::new(stream.try_clone()?).read_line(&mut line)?;
        if line.trim().is_empty() {
            return Err(BridgeError::MissingResponse);
        }
        Ok(serde_json::from_str(&line)?)
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
    use flow_agent_core::{Decision, Provider};
    use std::thread;

    fn temp_socket(name: &str) -> PathBuf {
        env::temp_dir().join(format!("flow-agent-{name}-{}.sock", std::process::id()))
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

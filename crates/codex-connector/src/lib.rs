use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
#[cfg(target_os = "macos")]
use std::fs;
use std::io::{BufRead, BufReader, Write};
#[cfg(target_os = "macos")]
use std::os::unix::fs::FileTypeExt;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, Weak};
use std::thread;
use std::time::Duration;
use thiserror::Error;

const RPC_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub enum ConnectorError {
    #[error("Codex app-server process failed: {0}")]
    Process(String),
    #[error("Codex app-server transport failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Codex app-server protocol failed: {0}")]
    Protocol(String),
    #[error("Codex app-server request timed out")]
    Timeout,
    #[error("Codex app-server channel closed")]
    Closed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServerRequest {
    pub id: Value,
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServerNotification {
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexThread {
    pub id: String,
    pub name: Option<String>,
    pub cwd: Option<String>,
    pub status: String,
    pub active_flags: Vec<String>,
    pub updated_at: Option<u64>,
    #[serde(default)]
    pub approval_policy: Option<String>,
    #[serde(default)]
    pub approvals_reviewer: Option<String>,
    #[serde(default)]
    pub sandbox_mode: Option<String>,
}

pub struct ConnectorChannels {
    pub requests: mpsc::Receiver<ServerRequest>,
    pub notifications: mpsc::Receiver<ServerNotification>,
}

#[derive(Clone)]
pub struct CodexConnector {
    inner: Arc<Inner>,
}

struct Inner {
    writer: Mutex<ChildStdin>,
    child: Mutex<Option<Child>>,
    pending: Mutex<HashMap<String, mpsc::Sender<Result<Value, String>>>>,
    next_id: AtomicU64,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Ok(Some(child)) = self.child.get_mut().map(Option::as_mut) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[derive(Clone)]
pub struct WeakCodexConnector {
    inner: Weak<Inner>,
}

impl WeakCodexConnector {
    pub fn upgrade(&self) -> Option<CodexConnector> {
        self.inner.upgrade().map(|inner| CodexConnector { inner })
    }
}

impl CodexConnector {
    pub fn connect(
        executable: &Path,
        socket_path: &Path,
    ) -> Result<(Self, ConnectorChannels), ConnectorError> {
        cleanup_legacy_socket_listeners(socket_path);
        Self::connect_stdio(executable)
    }

    fn connect_stdio(executable: &Path) -> Result<(Self, ConnectorChannels), ConnectorError> {
        // Codex app-server natively supports stdio. Keeping it as the direct
        // child of Runtime avoids a detached Unix-socket listener: if Runtime
        // is killed, the OS closes stdin and app-server exits on EOF instead
        // of surviving as a PPID-1 orphan.
        let mut child = Command::new(executable)
            .args(["app-server", "--stdio"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let writer = child
            .stdin
            .take()
            .ok_or_else(|| ConnectorError::Process("proxy stdin unavailable".to_owned()))?;
        let reader = child
            .stdout
            .take()
            .ok_or_else(|| ConnectorError::Process("proxy stdout unavailable".to_owned()))?;
        let (request_sender, requests) = mpsc::channel();
        let (notification_sender, notifications) = mpsc::channel();
        let connector = Self {
            inner: Arc::new(Inner {
                writer: Mutex::new(writer),
                child: Mutex::new(Some(child)),
                pending: Mutex::new(HashMap::new()),
                next_id: AtomicU64::new(1),
            }),
        };
        let reader_connector = connector.downgrade();
        thread::Builder::new()
            .name("actrealm-codex-reader".to_owned())
            .spawn(move || {
                for line in BufReader::new(reader).lines() {
                    let Ok(line) = line else { break };
                    let Ok(message) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    let method = message.get("method").and_then(Value::as_str);
                    let id = message.get("id").cloned();
                    match (method, id) {
                        (Some(method), Some(id)) => {
                            let _ = request_sender.send(ServerRequest {
                                id,
                                method: method.to_owned(),
                                params: message.get("params").cloned().unwrap_or(Value::Null),
                            });
                        }
                        (Some(method), None) => {
                            let _ = notification_sender.send(ServerNotification {
                                method: method.to_owned(),
                                params: message.get("params").cloned().unwrap_or(Value::Null),
                            });
                        }
                        (None, Some(id)) => {
                            let Some(connector) = reader_connector.upgrade() else {
                                break;
                            };
                            connector.complete_response(id, message);
                        }
                        (None, None) => {}
                    }
                }
                if let Some(connector) = reader_connector.upgrade() {
                    connector.fail_all_pending("transport_closed");
                }
            })
            .map_err(|error| ConnectorError::Process(error.to_string()))?;
        connector.initialize()?;
        Ok((
            connector,
            ConnectorChannels {
                requests,
                notifications,
            },
        ))
    }

    pub fn downgrade(&self) -> WeakCodexConnector {
        WeakCodexConnector {
            inner: Arc::downgrade(&self.inner),
        }
    }

    #[cfg(test)]
    fn managed_child_id(&self) -> u32 {
        self.inner
            .child
            .lock()
            .expect("child lock")
            .as_ref()
            .expect("managed app-server")
            .id()
    }

    fn initialize(&self) -> Result<(), ConnectorError> {
        self.call(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "actrealm",
                    "title": "ActRealm",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "experimentalApi": true,
                    "mcpServerOpenaiFormElicitation": true
                }
            }),
            RPC_TIMEOUT,
        )?;
        self.notify("initialized", json!({}))
    }

    pub fn list_threads(&self) -> Result<Vec<CodexThread>, ConnectorError> {
        let result = self.call(
            "thread/list",
            json!({"limit": 100, "sortKey": "updated_at", "sortDirection": "desc"}),
            RPC_TIMEOUT,
        )?;
        let threads = result
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| ConnectorError::Protocol("thread/list omitted data".to_owned()))?;
        Ok(threads.iter().filter_map(parse_thread).collect())
    }

    pub fn resume_thread(&self, thread_id: &str) -> Result<CodexThread, ConnectorError> {
        if thread_id.is_empty() || thread_id.len() > 256 {
            return Err(ConnectorError::Protocol("invalid thread id".to_owned()));
        }
        let result = self.call(
            "thread/resume",
            json!({"threadId": thread_id, "excludeTurns": true}),
            RPC_TIMEOUT,
        )?;
        let mut thread = parse_thread(result.get("thread").unwrap_or(&Value::Null))
            .ok_or_else(|| ConnectorError::Protocol("thread/resume omitted thread".to_owned()))?;
        thread.approval_policy = protocol_variant(result.get("approvalPolicy"));
        thread.approvals_reviewer = protocol_variant(result.get("approvalsReviewer"));
        thread.sandbox_mode = protocol_variant(result.get("sandbox"));
        Ok(thread)
    }

    pub fn call(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, ConnectorError> {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::channel();
        self.inner
            .pending
            .lock()
            .map_err(|_| ConnectorError::Closed)?
            .insert(id.to_string(), sender);
        if let Err(error) = self.write(&json!({"id": id, "method": method, "params": params})) {
            if let Ok(mut pending) = self.inner.pending.lock() {
                pending.remove(&id.to_string());
            }
            return Err(error);
        }
        match receiver.recv_timeout(timeout) {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => Err(ConnectorError::Protocol(error)),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Ok(mut pending) = self.inner.pending.lock() {
                    pending.remove(&id.to_string());
                }
                Err(ConnectorError::Timeout)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(ConnectorError::Closed),
        }
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), ConnectorError> {
        self.write(&json!({"method": method, "params": params}))
    }

    pub fn respond(&self, id: Value, result: Value) -> Result<(), ConnectorError> {
        self.write(&json!({"id": id, "result": result}))
    }

    pub fn respond_error(&self, id: Value, code: i64, message: &str) -> Result<(), ConnectorError> {
        self.write(&json!({"id": id, "error": {"code": code, "message": message}}))
    }

    fn write(&self, value: &Value) -> Result<(), ConnectorError> {
        let mut writer = self
            .inner
            .writer
            .lock()
            .map_err(|_| ConnectorError::Closed)?;
        serde_json::to_writer(&mut *writer, value)
            .map_err(|error| ConnectorError::Protocol(error.to_string()))?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        Ok(())
    }

    fn complete_response(&self, id: Value, message: Value) {
        let sender = self
            .inner
            .pending
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(&id_key(&id)));
        let Some(sender) = sender else { return };
        if let Some(error) = message.get("error") {
            let _ = sender.send(Err(error.to_string()));
        } else {
            let _ = sender.send(Ok(message.get("result").cloned().unwrap_or(Value::Null)));
        }
    }

    fn fail_all_pending(&self, reason: &str) {
        if let Ok(mut pending) = self.inner.pending.lock() {
            for (_, sender) in pending.drain() {
                let _ = sender.send(Err(reason.to_owned()));
            }
        }
    }
}

/// Releases listeners created by pre-stdio ActRealm builds. The private
/// ActRealm socket path is the ownership boundary: ordinary Codex CLI/Desktop
/// sessions never bind it, and no process with another command name is
/// touched. Cleanup is best-effort so a missing `lsof` cannot disable Codex.
#[cfg(target_os = "macos")]
fn cleanup_legacy_socket_listeners(socket_path: &Path) {
    if let Ok(output) = Command::new("/usr/sbin/lsof")
        .args(["-n", "-P", "-U", "-F", "pcn"])
        .output()
    {
        let fields = String::from_utf8_lossy(&output.stdout);
        for pid in legacy_listener_pids(&fields, socket_path) {
            if pid != std::process::id() {
                let _ = Command::new("/bin/kill")
                    .args(["-TERM", &pid.to_string()])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
    }

    if fs::symlink_metadata(socket_path).is_ok_and(|metadata| metadata.file_type().is_socket()) {
        let _ = fs::remove_file(socket_path);
    }
}

#[cfg(not(target_os = "macos"))]
fn cleanup_legacy_socket_listeners(_socket_path: &Path) {}

#[cfg(target_os = "macos")]
fn legacy_listener_pids(fields: &str, socket_path: &Path) -> Vec<u32> {
    let expected_path = socket_path.to_string_lossy();
    let mut current_pid = None;
    let mut current_command = None;
    let mut matches = Vec::new();
    for field in fields.lines() {
        let Some((kind, value)) = field.split_at_checked(1) else {
            continue;
        };
        match kind {
            "p" => {
                current_pid = value.parse::<u32>().ok();
                current_command = None;
            }
            "c" => current_command = Some(value),
            "n" if value == expected_path && current_command == Some("codex") => {
                if let Some(pid) = current_pid {
                    matches.push(pid);
                }
            }
            _ => {}
        }
    }
    matches.sort_unstable();
    matches.dedup();
    matches
}

fn id_key(id: &Value) -> String {
    id.as_u64()
        .map(|value| value.to_string())
        .or_else(|| id.as_i64().map(|value| value.to_string()))
        .or_else(|| id.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| id.to_string())
}

pub fn parse_thread(value: &Value) -> Option<CodexThread> {
    let status = value.get("status")?;
    Some(CodexThread {
        id: value.get("id")?.as_str()?.to_owned(),
        name: value
            .get("name")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        cwd: value
            .get("cwd")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        status: status.get("type")?.as_str()?.to_owned(),
        active_flags: status
            .get("activeFlags")
            .and_then(Value::as_array)
            .map(|flags| {
                flags
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        updated_at: value.get("updatedAt").and_then(Value::as_u64),
        approval_policy: protocol_variant(value.get("approvalPolicy")),
        approvals_reviewer: protocol_variant(value.get("approvalsReviewer")),
        sandbox_mode: protocol_variant(value.get("sandbox")),
    })
}

fn protocol_variant(value: Option<&Value>) -> Option<String> {
    let value = value?;
    value
        .as_str()
        .or_else(|| value.get("type").and_then(Value::as_str))
        .map(ToOwned::to_owned)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Instant;

    #[test]
    fn persistent_stdio_initializes_lists_resumes_and_routes_server_requests() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-codex-connector-{}-{}",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("fake-codex");
        fs::write(
            &executable,
            r#"#!/bin/sh
if [ "$2" = "daemon" ]; then
  exit 0
fi
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"id":1,"result":{"codexHome":"/tmp/codex","platformFamily":"unix","platformOs":"macos","userAgent":"fake"}}'
      ;;
    *'"method":"thread/list"'*)
      printf '%s\n' '{"id":2,"result":{"data":[{"id":"thread-1","sessionId":"session-1","cwd":"/tmp/project","name":"Recovered thread","status":{"type":"active","activeFlags":["waitingOnUserInput"]},"turns":[],"updatedAt":100}]}}'
      ;;
    *'"method":"thread/resume"'*)
      printf '%s\n' '{"id":3,"result":{"thread":{"id":"thread-1","sessionId":"session-1","cwd":"/tmp/project","name":"Recovered thread","status":{"type":"active","activeFlags":["waitingOnUserInput"]},"turns":[],"updatedAt":100}}}'
      printf '%s\n' '{"id":"server-request-1","method":"item/tool/requestUserInput","params":{"threadId":"thread-1","turnId":"turn-1","itemId":"item-1","questions":[{"id":"choice","header":"Choice","question":"Continue?","isOther":false,"isSecret":false,"options":null}]}}'
      ;;
    *'"id":"server-request-1"'*)
      printf '%s\n' '{"method":"test/responseSeen","params":{"threadId":"thread-1"}}'
      ;;
  esac
done
"#,
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();

        let (connector, channels) = CodexConnector::connect_stdio(&executable).unwrap();
        let weak_connector = connector.downgrade();
        let child_id = connector.managed_child_id();
        let threads = connector.list_threads().unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].status, "active");
        assert_eq!(threads[0].active_flags, vec!["waitingOnUserInput"]);
        let resumed = connector.resume_thread("thread-1").unwrap();
        assert_eq!(resumed.name.as_deref(), Some("Recovered thread"));
        let request = channels
            .requests
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        assert_eq!(request.method, "item/tool/requestUserInput");
        connector
            .respond(
                request.id,
                json!({"answers":{"choice":{"answers":["yes"]}}}),
            )
            .unwrap();
        let notification = channels
            .notifications
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        assert_eq!(notification.method, "test/responseSeen");
        drop(connector);
        assert!(weak_connector.upgrade().is_none());
        assert!(
            !Command::new("/bin/kill")
                .args(["-0", &child_id.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success(),
            "managed app-server child must be reaped when the connector drops"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn legacy_listener_parser_only_accepts_codex_on_the_private_socket() {
        let socket = Path::new("/Users/test/.actrealm/run/codex-app-server.sock");
        let fields = "p100\nccodex\nn/Users/test/.actrealm/run/codex-app-server.sock\n\
p101\nccodex\nn/Users/test/.codex/app-server.sock\n\
p102\ncother\nn/Users/test/.actrealm/run/codex-app-server.sock\n\
p100\nccodex\nn/Users/test/.actrealm/run/codex-app-server.sock\n";
        assert_eq!(legacy_listener_pids(fields, socket), vec![100]);
    }
}

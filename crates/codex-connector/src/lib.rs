use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};
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
    protocol: Mutex<ProtocolSupport>,
    next_id: AtomicU64,
    shared_connection: bool,
    /// Set once the app-server rejects `account/rateLimits/read` parameters
    /// (releases before `GetAccountRateLimitsParams`).
    rate_limit_params_rejected: AtomicBool,
}

#[derive(Debug, Clone, Default)]
struct ProtocolSupport {
    server_user_agent: Option<String>,
    managed_approvals: bool,
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
        let shared_socket = shared_control_socket();
        Self::connect_transport(executable, shared_socket.as_deref())
    }

    /// Reads account quota through a short-lived app-server process. Runtime
    /// uses this only when its persistent task connector is unavailable, so
    /// quota polling keeps working after sleep or an early connector failure
    /// without requiring the user to start a Codex task.
    pub fn read_rate_limits_once(executable: &Path) -> Result<Value, ConnectorError> {
        let (connector, _channels) = Self::connect_stdio(executable)?;
        let result = connector.read_rate_limits();
        connector.shutdown();
        result
    }

    fn connect_stdio(executable: &Path) -> Result<(Self, ConnectorChannels), ConnectorError> {
        Self::connect_transport(executable, None)
    }

    pub fn is_shared_connection(&self) -> bool {
        self.inner.shared_connection
    }

    fn connect_transport(
        executable: &Path,
        shared_socket: Option<&Path>,
    ) -> Result<(Self, ConnectorChannels), ConnectorError> {
        // The official proxy joins the existing service; killing our proxy
        // never terminates its daemon or another client's active writer.
        let mut command = Command::new(executable);
        command.arg("app-server");
        if let Some(socket) = shared_socket {
            command.arg("proxy").arg("--sock").arg(socket);
        } else {
            command.arg("--stdio");
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let writer = child
            .stdin
            .take()
            .ok_or_else(|| ConnectorError::Process("proxy stdin unavailable".to_owned()))?;
        #[cfg(unix)]
        set_nonblocking(&writer)?;
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
                protocol: Mutex::new(ProtocolSupport::default()),
                next_id: AtomicU64::new(1),
                shared_connection: shared_socket.is_some(),
                rate_limit_params_rejected: AtomicBool::new(false),
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
        let result = self.call(
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
        let user_agent = result
            .get("userAgent")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let managed_approvals = user_agent
            .as_deref()
            .and_then(codex_version)
            .is_some_and(managed_approval_schema_supported);
        if let Ok(mut protocol) = self.inner.protocol.lock() {
            protocol.server_user_agent = user_agent;
            protocol.managed_approvals = managed_approvals;
        }
        self.notify("initialized", json!({}))
    }

    pub fn supports_managed_approvals(&self) -> bool {
        self.inner
            .protocol
            .lock()
            .map(|protocol| protocol.managed_approvals)
            .unwrap_or(false)
    }

    pub fn server_user_agent(&self) -> Option<String> {
        self.inner
            .protocol
            .lock()
            .ok()
            .and_then(|protocol| protocol.server_user_agent.clone())
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

    /// Reads Codex account rate limits without starting or resuming a task.
    /// The app-server owns authentication; ActRealm receives only the
    /// allowlisted usage snapshot returned by the provider protocol.
    ///
    /// Quota reads are background polls, so they ask the app-server to skip
    /// its separate reset-credit detail lookup (the available count is still
    /// returned). An app-server that predates that parameter gets the plain
    /// request instead.
    pub fn read_rate_limits(&self) -> Result<Value, ConnectorError> {
        if !self
            .inner
            .rate_limit_params_rejected
            .load(Ordering::Relaxed)
        {
            match self.call(
                "account/rateLimits/read",
                json!({"excludeResetCreditDetails": true}),
                RPC_TIMEOUT,
            ) {
                Err(ConnectorError::Protocol(_)) => {
                    self.inner
                        .rate_limit_params_rejected
                        .store(true, Ordering::Relaxed);
                }
                result => return result,
            }
        }
        self.call("account/rateLimits/read", json!({}), RPC_TIMEOUT)
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
        let request = json!({"id": id, "method": method, "params": params});
        if let Err(error) = self.write_before(&request, timeout) {
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

    pub fn shutdown(&self) {
        self.fail_all_pending("connector_shutdown");
        if let Ok(mut managed) = self.inner.child.lock() {
            if let Some(mut child) = managed.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    fn write(&self, value: &Value) -> Result<(), ConnectorError> {
        self.write_before(value, RPC_TIMEOUT)
    }

    /// Writes one message line. An app-server that stops reading its input
    /// would otherwise block every writer, and the quota loop with them, once
    /// the pipe fills; after `timeout` without progress it is shut down.
    fn write_before(&self, value: &Value, timeout: Duration) -> Result<(), ConnectorError> {
        let mut line = serde_json::to_vec(value)
            .map_err(|error| ConnectorError::Protocol(error.to_string()))?;
        line.push(b'\n');
        let mut writer = self
            .inner
            .writer
            .lock()
            .map_err(|_| ConnectorError::Closed)?;
        match write_all_while_read(&mut writer, &line, timeout) {
            Err(error) if error.kind() == io::ErrorKind::TimedOut => {
                self.shutdown();
                Err(ConnectorError::Timeout)
            }
            result => Ok(result?),
        }
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

fn shared_control_socket() -> Option<PathBuf> {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")))?;
    let path = home.join("app-server-control/app-server-control.sock");
    trusted_control_socket(&path).then_some(path)
}

#[cfg(unix)]
fn trusted_control_socket(path: &Path) -> bool {
    let owner = unsafe { libc::geteuid() };
    let Some(parent) = path.parent() else {
        return false;
    };
    let Ok(directory) = fs::symlink_metadata(parent) else {
        return false;
    };
    let Ok(socket) = fs::symlink_metadata(path) else {
        return false;
    };
    directory.is_dir()
        && directory.uid() == owner
        && directory.mode() & 0o022 == 0
        && socket.file_type().is_socket()
        && socket.uid() == owner
        && socket.mode() & 0o022 == 0
}

#[cfg(not(unix))]
fn trusted_control_socket(_path: &Path) -> bool {
    false
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

/// Only our end of the pipe: the app-server still reads its input blocking.
#[cfg(unix)]
fn set_nonblocking(writer: &ChildStdin) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let fd = writer.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Writes all of `bytes`. Fails only when the reader takes nothing for
/// `stall`: a large message to a busy but reading app-server still goes out.
#[cfg(unix)]
fn write_all_while_read(
    writer: &mut ChildStdin,
    mut bytes: &[u8],
    stall: Duration,
) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let mut deadline = Instant::now() + stall;
    while !bytes.is_empty() {
        match writer.write(bytes) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(written) => {
                bytes = &bytes[written..];
                deadline = Instant::now() + stall;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(io::ErrorKind::TimedOut.into());
                }
                let mut ready = libc::pollfd {
                    fd: writer.as_raw_fd(),
                    events: libc::POLLOUT,
                    revents: 0,
                };
                let millis = left.as_millis().clamp(1, i32::MAX as u128) as libc::c_int;
                if unsafe { libc::poll(&mut ready, 1, millis) } == -1 {
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::Interrupted {
                        return Err(error);
                    }
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn write_all_while_read(writer: &mut ChildStdin, bytes: &[u8], _stall: Duration) -> io::Result<()> {
    writer.write_all(bytes)?;
    writer.flush()
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

fn codex_version(user_agent: &str) -> Option<(u64, u64, u64)> {
    user_agent
        .split(|character: char| !(character.is_ascii_digit() || character == '.'))
        .find_map(|candidate| {
            let mut parts = candidate.split('.');
            let major = parts.next()?.parse().ok()?;
            let minor = parts.next()?.parse().ok()?;
            let patch = parts.next()?.parse().ok()?;
            Some((major, minor, patch))
        })
}

fn managed_approval_schema_supported(version: (u64, u64, u64)) -> bool {
    // These request/response shapes are generated and verified against the
    // installed 0.144 app-server schema. A later minor must be sampled before
    // controls are exposed; unsupported versions degrade to truthful
    // observation instead of guessing a response contract.
    ((0, 144, 5)..(0, 145, 0)).contains(&version)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Instant;

    #[test]
    fn shared_control_socket_rejects_links_and_writable_endpoints() {
        use std::os::unix::net::UnixListener;
        let root =
            std::env::temp_dir().join(format!("actrealm-shared-trust-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let path = root.join("control.sock");
        let listener = UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(trusted_control_socket(&path));
        let link = root.join("link.sock");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(!trusted_control_socket(&link));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
        assert!(!trusted_control_socket(&path));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(!trusted_control_socket(&path));
        drop(listener);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn shared_transport_uses_official_proxy_arguments_and_only_owns_its_child() {
        let root =
            std::env::temp_dir().join(format!("actrealm-shared-proxy-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("fake-codex");
        fs::write(&executable, r#"#!/bin/sh
[ "$1" = app-server ] && [ "$2" = proxy ] && [ "$3" = --sock ] && [ "$4" = /fixture/control.sock ] || exit 1
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*) printf '%s\n' '{"id":1,"result":{"userAgent":"codex_cli_rs/0.144.6"}}' ;;
  esac
done
"#).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(Command::new(&executable)
            .args(["app-server", "proxy", "--sock", "/fixture/control.sock"])
            .stdin(Stdio::null())
            .status()
            .unwrap()
            .success());
        let (connector, _channels) = CodexConnector::connect_transport(
            &executable,
            Some(Path::new("/fixture/control.sock")),
        )
        .unwrap();
        assert!(connector.is_shared_connection());
        let weak = connector.downgrade();
        connector.shutdown();
        drop(connector);
        assert!(weak.upgrade().is_none());
        fs::remove_dir_all(root).unwrap();
    }

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
  request_id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"id":%s,"result":{"codexHome":"/tmp/codex","platformFamily":"unix","platformOs":"macos","userAgent":"codex_cli_rs/0.144.6"}}\n' "$request_id"
      ;;
    *'"method":"thread/list"'*)
      printf '{"id":%s,"result":{"data":[{"id":"thread-1","sessionId":"session-1","cwd":"/tmp/project","name":"Recovered thread","status":{"type":"active","activeFlags":["waitingOnUserInput"]},"turns":[],"updatedAt":100}]}}\n' "$request_id"
      ;;
    *'"method":"thread/resume"'*)
      printf '{"id":%s,"result":{"thread":{"id":"thread-1","sessionId":"session-1","cwd":"/tmp/project","name":"Recovered thread","status":{"type":"active","activeFlags":["waitingOnUserInput"]},"turns":[],"updatedAt":100}}}\n' "$request_id"
      printf '%s\n' '{"id":"server-request-1","method":"item/tool/requestUserInput","params":{"threadId":"thread-1","turnId":"turn-1","itemId":"item-1","questions":[{"id":"choice","header":"Choice","question":"Continue?","isOther":false,"isSecret":false,"options":null}]}}'
      ;;
    *'"method":"account/rateLimits/read"'*'"excludeResetCreditDetails"'*)
      printf '{"id":%s,"error":{"code":-32602,"message":"Invalid params"}}\n' "$request_id"
      ;;
    *'"method":"account/rateLimits/read"'*)
      printf '{"id":%s,"result":{"rateLimits":{"limitId":"codex","primary":{"usedPercent":12,"windowDurationMins":300,"resetsAt":1784140000}}}}\n' "$request_id"
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
        assert!(connector.supports_managed_approvals());
        assert_eq!(
            connector.server_user_agent().as_deref(),
            Some("codex_cli_rs/0.144.6")
        );
        let weak_connector = connector.downgrade();
        let child_id = connector.managed_child_id();
        let threads = connector.list_threads().unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].status, "active");
        assert_eq!(threads[0].active_flags, vec!["waitingOnUserInput"]);
        let resumed = connector.resume_thread("thread-1").unwrap();
        assert_eq!(resumed.name.as_deref(), Some("Recovered thread"));
        // This app-server predates the background-poll parameter: the read
        // falls back to the plain request and keeps using it.
        let rate_limits = connector.read_rate_limits().unwrap();
        assert_eq!(rate_limits["rateLimits"]["primary"]["usedPercent"], 12);
        assert!(connector
            .inner
            .rate_limit_params_rejected
            .load(Ordering::Relaxed));
        let again = connector.read_rate_limits().unwrap();
        assert_eq!(again["rateLimits"]["primary"]["usedPercent"], 12);

        let one_shot = CodexConnector::read_rate_limits_once(&executable).unwrap();
        assert_eq!(one_shot["rateLimits"]["primary"]["usedPercent"], 12);
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
        connector.shutdown();
        assert!(
            !Command::new("/bin/kill")
                .args(["-0", &child_id.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success(),
            "explicit shutdown must reap the managed app-server child"
        );
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

    #[test]
    fn managed_approval_gate_tracks_versioned_app_server_schema() {
        assert_eq!(codex_version("codex_cli_rs/0.144.6"), Some((0, 144, 6)));
        assert_eq!(codex_version("codex-cli 1.2.3"), Some((1, 2, 3)));
        assert_eq!(codex_version("fake"), None);
        assert!(managed_approval_schema_supported((0, 144, 6)));
        assert!(!managed_approval_schema_supported((0, 144, 4)));
        assert!(!managed_approval_schema_supported((0, 145, 0)));
    }

    #[test]
    fn app_server_that_stops_reading_is_shut_down_instead_of_blocking_writers() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-codex-stuck-reader-{}-{}",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("fake-codex");
        // Answers initialize, then keeps its input open without reading it.
        fs::write(
            &executable,
            "#!/bin/sh\nIFS= read -r line || exit 0\nprintf '%s\\n' '{\"id\":1,\"result\":{\"userAgent\":\"codex_cli_rs/0.162.0\"}}'\nexec sleep 30\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(Command::new(&executable)
            .stdin(Stdio::null())
            .status()
            .unwrap()
            .success());
        let (connector, _channels) = CodexConnector::connect_stdio(&executable).unwrap();
        let pid = connector.managed_child_id() as i32;
        let (sender, receiver) = mpsc::channel();
        let caller = connector.clone();
        thread::spawn(move || {
            let filler = json!({"pad": "a".repeat(256 * 1024)});
            let _ = sender.send(caller.call("x", filler, Duration::from_secs(1)));
        });
        let result = receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("a write the app-server never reads must give up");
        assert!(matches!(result, Err(ConnectorError::Timeout)));
        // The app-server was shut down, so the next request fails at once
        // and quota polling can fall back to a one-shot process.
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        let started = Instant::now();
        assert!(connector.read_rate_limits().is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(connector);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn app_server_that_reads_slowly_keeps_its_connection() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-codex-slow-reader-{}-{}",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("fake-codex");
        // Answers initialize, then takes 8 KiB every 0.2 s and never replies.
        fs::write(
            &executable,
            "#!/bin/sh\nIFS= read -r line || exit 0\nprintf '%s\\n' '{\"id\":1,\"result\":{\"userAgent\":\"codex_cli_rs/0.162.0\"}}'\nwhile :; do dd bs=8192 count=1 of=/dev/null 2>/dev/null; sleep 0.2; done\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(Command::new(&executable)
            .stdin(Stdio::null())
            .status()
            .unwrap()
            .success());
        let (connector, _channels) = CodexConnector::connect_stdio(&executable).unwrap();
        let pid = connector.managed_child_id() as i32;
        // Twice the pipe: writing it takes longer than the timeout, but the
        // reader keeps taking some, so only the reply times out.
        let large = json!({"pad": "a".repeat(128 * 1024)});
        let started = Instant::now();
        assert!(matches!(
            connector.call("x", large, Duration::from_secs(1)),
            Err(ConnectorError::Timeout)
        ));
        assert!(started.elapsed() > Duration::from_secs(1));
        assert_eq!(unsafe { libc::kill(pid, 0) }, 0);
        connector.shutdown();
        drop(connector);
        fs::remove_dir_all(root).unwrap();
    }
}

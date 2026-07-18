use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE, COOKIE, HOST, ORIGIN, SET_COOKIE,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use flow_agent_codex_connector::{
    parse_thread, CodexConnector, CodexThread, ConnectorChannels, ServerNotification, ServerRequest,
};
use flow_agent_core::{BridgeRequest, Provider, ReplyAction, ReplyPayload};
use flow_agent_installer::{
    discover_provider_availability, BinaryHealth, ClaudeStatuslineStatus, CodexTrustStatus,
    ConfigHealth, HookProvider, InstallIntent, InstallOptions, InstallPaths, Installer,
};
use flow_agent_quota::{QuotaCollector, QuotaEntry, QuotaPaths};
use flow_agent_runtime::{
    ApprovalAction, AttentionAction, CommandState, MetricEvent, QuotaRecord, RuntimeStore,
    SessionRecord, SessionUsageRecord, StoreError, WaiterError, WaiterRegistry,
};
use flow_agent_usage::{UsageCollector, UsagePaths, UsageRecord};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::os::unix::net::UnixStream;
use std::path::{Path as FilePath, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::sync::oneshot;
use uuid::Uuid;

const SESSION_COOKIE: &str = "flow_agent_session";
const CSRF_HEADER: &str = "x-flow-agent-csrf";
const SETTINGS_KEY: &str = "ui_settings";
const MANAGED_CODEX_THREADS_KEY: &str = "managed_codex_threads";
const MAX_MANAGED_CODEX_THREADS: usize = 32;
const TASK_CARD_DISPLAY_FIELDS: &[&str] = &[
    "project",
    "task",
    "model",
    "activity",
    "plan",
    "tokens",
    "context",
    "tool",
    "permissionMode",
    "subagents",
    "environment",
    "recovery",
    "control",
    "jump",
    "titleSource",
    "sessionId",
    "providerSessionId",
    "providerTurnId",
    "lastEventAt",
];
const SESSION_LIST_RETENTION_MS: u64 = 30 * 60 * 1_000;
const USAGE_POLL_INTERVAL: Duration = Duration::from_secs(1);
const INDEX_HTML: &str = include_str!("../../../web/index.html");
const APP_CSS: &str = include_str!("../../../web/app.css");
const APP_JS: &str = include_str!("../../../web/app.js");
const CLAUDE_ICON: &[u8] = include_bytes!("../../../web/assets/claude.png");
const CODEX_ICON: &[u8] = include_bytes!("../../../web/assets/codex.png");

#[derive(Debug, Clone)]
pub struct ApiServerConfig {
    pub bind: SocketAddr,
    pub commit_delay: Duration,
    pub snapshot_interval: Duration,
    pub quota_poll_interval: Duration,
    pub enable_claude_oauth_quota: bool,
    pub install_paths: Option<InstallPaths>,
    pub enable_codex_connector: bool,
    pub runtime_restart: Option<RuntimeRestartHandle>,
}

impl Default for ApiServerConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            commit_delay: Duration::from_secs(3),
            snapshot_interval: Duration::from_millis(100),
            quota_poll_interval: Duration::from_secs(60),
            enable_claude_oauth_quota: false,
            install_paths: None,
            enable_codex_connector: false,
            runtime_restart: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeRestartHandle {
    sender: std_mpsc::Sender<std_mpsc::Sender<Result<(), String>>>,
    socket_path: PathBuf,
}

impl RuntimeRestartHandle {
    pub fn new(
        sender: std_mpsc::Sender<std_mpsc::Sender<Result<(), String>>>,
        socket_path: PathBuf,
    ) -> Self {
        Self {
            sender,
            socket_path,
        }
    }

    fn request(&self) -> Result<std_mpsc::Receiver<Result<(), String>>, String> {
        let (response_sender, response_receiver) = std_mpsc::channel();
        self.sender
            .send(response_sender)
            .map_err(|_| "Runtime 控制通道不可用".to_owned())?;
        UnixStream::connect(&self.socket_path)
            .map_err(|error| format!("无法唤醒 bridge.sock：{error}"))?;
        Ok(response_receiver)
    }
}

#[derive(Debug, Error)]
pub enum ApiServerError {
    #[error("API listener failed: {0}")]
    Io(#[from] io::Error),
    #[error("API runtime thread failed: {0}")]
    Thread(String),
    #[error("setup service failed: {0}")]
    Setup(String),
}

pub struct ApiServer {
    address: SocketAddr,
    bootstrap_token: String,
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl ApiServer {
    pub fn start(
        store: RuntimeStore,
        waiters: WaiterRegistry,
        config: ApiServerConfig,
    ) -> Result<Self, ApiServerError> {
        let listener = TcpListener::bind(config.bind)?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        if !address.ip().is_loopback() {
            return Err(ApiServerError::Io(io::Error::new(
                io::ErrorKind::AddrNotAvailable,
                "Flow Agent API must bind to loopback",
            )));
        }
        let bootstrap_token = Uuid::now_v7().to_string();
        let install_paths = match config.install_paths.clone() {
            Some(paths) => paths,
            None => InstallPaths::discover()
                .map_err(|error| ApiServerError::Setup(error.to_string()))?,
        };
        let source_binary =
            std::env::current_exe().map_err(|error| ApiServerError::Setup(error.to_string()))?;
        let quota_paths = QuotaPaths {
            flow_home: install_paths.flow_home.clone(),
            codex_sessions: install_paths
                .codex_config
                .parent()
                .unwrap_or_else(|| FilePath::new("."))
                .join("sessions"),
        };
        let mut usage_paths = UsagePaths::discover();
        usage_paths.flow_home = install_paths.flow_home.clone();
        let codex_home = install_paths
            .codex_config
            .parent()
            .unwrap_or_else(|| FilePath::new("."));
        usage_paths.codex_sessions = vec![
            codex_home.join("sessions"),
            codex_home.join("archived_sessions"),
        ];
        let data_paths = DataPaths {
            cache: install_paths.flow_home.join("cache"),
            spool: install_paths.flow_home.join("spool"),
            diagnostics: install_paths.flow_home.join("diagnostics"),
        };
        let codex_socket = install_paths.flow_home.join("run/codex-app-server.sock");
        let codex = if config.enable_codex_connector {
            CodexManager::start(store.clone(), waiters.clone(), &codex_socket)
        } else {
            CodexManager::disabled(store.clone())
        };
        let state = AppState {
            store,
            waiters,
            auth: Arc::new(Mutex::new(AuthState {
                bootstrap_token: Some(bootstrap_token.clone()),
                session_token: None,
                csrf_token: None,
            })),
            expected_host: address.to_string(),
            expected_origin: format!("http://{address}"),
            commit_delay: config.commit_delay,
            snapshot_interval: config.snapshot_interval,
            quota_poll_interval: config.quota_poll_interval,
            claude_oauth_quota: config.enable_claude_oauth_quota,
            installer: Arc::new(Installer::new(install_paths, source_binary)),
            quota: Arc::new(Mutex::new(QuotaState {
                collector: QuotaCollector::new(quota_paths),
                entries: Vec::new(),
                refreshed_at: None,
                claude_cache_modified_at: None,
                oauth_refresh_in_progress: false,
            })),
            usage: Arc::new(Mutex::new(UsageState {
                collector: UsageCollector::new(usage_paths),
                refreshed_at: None,
            })),
            data_paths,
            codex,
            runtime_restart: config.runtime_restart,
        };
        let router = router(state);
        let (shutdown, shutdown_receiver) = oneshot::channel();
        let api_thread = thread::Builder::new()
            .name("flow-agent-api".to_owned())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                let Ok(runtime) = runtime else { return };
                runtime.block_on(async move {
                    let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                        return;
                    };
                    let _ = axum::serve(listener, router)
                        .with_graceful_shutdown(async {
                            let _ = shutdown_receiver.await;
                        })
                        .await;
                });
            })
            .map_err(|error| ApiServerError::Thread(error.to_string()))?;
        Ok(Self {
            address,
            bootstrap_token,
            shutdown: Some(shutdown),
            thread: Some(api_thread),
        })
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn origin(&self) -> String {
        format!("http://{}", self.address)
    }

    pub fn bootstrap_token(&self) -> &str {
        &self.bootstrap_token
    }

    pub fn bootstrap_url(&self) -> String {
        format!("{}/#bootstrap={}", self.origin(), self.bootstrap_token)
    }
}

impl Drop for ApiServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Clone)]
struct AppState {
    store: RuntimeStore,
    waiters: WaiterRegistry,
    auth: Arc<Mutex<AuthState>>,
    expected_host: String,
    expected_origin: String,
    commit_delay: Duration,
    snapshot_interval: Duration,
    quota_poll_interval: Duration,
    claude_oauth_quota: bool,
    installer: Arc<Installer>,
    quota: Arc<Mutex<QuotaState>>,
    usage: Arc<Mutex<UsageState>>,
    data_paths: DataPaths,
    codex: CodexManager,
    runtime_restart: Option<RuntimeRestartHandle>,
}

#[derive(Clone)]
struct CodexManager {
    connector: Option<CodexConnector>,
    state: Arc<Mutex<CodexManagerState>>,
    store: RuntimeStore,
    waiters: WaiterRegistry,
}

#[derive(Default)]
struct CodexManagerState {
    status: String,
    error: Option<String>,
    threads: HashMap<String, CodexThread>,
    managed: HashSet<String>,
    resume_failed: HashSet<String>,
    native_waiting: HashMap<String, bool>,
    native_synced: HashSet<String>,
}

struct QuotaState {
    collector: QuotaCollector,
    entries: Vec<QuotaEntry>,
    refreshed_at: Option<Instant>,
    claude_cache_modified_at: Option<SystemTime>,
    oauth_refresh_in_progress: bool,
}

struct UsageState {
    collector: UsageCollector,
    refreshed_at: Option<Instant>,
}

#[derive(Clone)]
struct DataPaths {
    cache: PathBuf,
    spool: PathBuf,
    diagnostics: PathBuf,
}

struct AuthState {
    bootstrap_token: Option<String>,
    session_token: Option<String>,
    csrf_token: Option<String>,
}

impl CodexManager {
    fn disabled(store: RuntimeStore) -> Self {
        Self {
            connector: None,
            state: Arc::new(Mutex::new(CodexManagerState {
                status: "disabled".to_owned(),
                ..CodexManagerState::default()
            })),
            store,
            waiters: WaiterRegistry::default(),
        }
    }

    fn start(store: RuntimeStore, waiters: WaiterRegistry, socket_path: &FilePath) -> Self {
        let state = Arc::new(Mutex::new(CodexManagerState {
            status: "unavailable".to_owned(),
            ..CodexManagerState::default()
        }));
        let executable = discover_provider_availability(HookProvider::Codex)
            .version_executable()
            .map(ToOwned::to_owned);
        let Some(executable) = executable else {
            if let Ok(mut current) = state.lock() {
                current.error = Some("未找到支持 app-server 的 Codex 客户端".to_owned());
            }
            return Self {
                connector: None,
                state,
                store,
                waiters,
            };
        };
        let (connector, channels) = match CodexConnector::connect(&executable, socket_path) {
            Ok(connection) => connection,
            Err(error) => {
                if let Ok(mut current) = state.lock() {
                    current.error = Some(error.to_string());
                }
                return Self {
                    connector: None,
                    state,
                    store,
                    waiters,
                };
            }
        };
        let managed = store
            .read_setting(MANAGED_CODEX_THREADS_KEY)
            .ok()
            .flatten()
            .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|id| !id.is_empty() && id.len() <= 256)
            .take(MAX_MANAGED_CODEX_THREADS)
            .collect::<HashSet<_>>();
        let listed = connector.list_threads().unwrap_or_default();
        let listed_for_sync = listed.clone();
        if let Ok(mut current) = state.lock() {
            current.status = "connected".to_owned();
            current.error = None;
            current.managed = managed.clone();
            current.threads = listed
                .into_iter()
                .map(|thread| (thread.id.clone(), thread))
                .collect();
        }
        for thread in &listed_for_sync {
            let waiting = thread_waiting_on_approval(thread);
            if let Ok(mut current) = state.lock() {
                current.native_waiting.insert(thread.id.clone(), waiting);
            }
            sync_codex_native_attention(
                &state,
                &store,
                &waiters,
                &thread.id,
                waiting,
                thread.status == "active",
            );
        }
        for thread_id in &managed {
            if let Ok(thread) = connector.resume_thread(thread_id) {
                let waiting = thread_waiting_on_approval(&thread);
                if let Ok(mut current) = state.lock() {
                    current.resume_failed.remove(thread_id);
                    current.native_waiting.insert(thread.id.clone(), waiting);
                    current.threads.insert(thread.id.clone(), thread);
                }
                let active = state
                    .lock()
                    .ok()
                    .and_then(|current| {
                        current
                            .threads
                            .get(thread_id)
                            .map(|thread| thread.status == "active")
                    })
                    .unwrap_or(false);
                sync_codex_native_attention(&state, &store, &waiters, thread_id, waiting, active);
            } else if let Ok(mut current) = state.lock() {
                current.resume_failed.insert(thread_id.clone());
            }
        }
        spawn_codex_handlers(
            connector.clone(),
            channels,
            Arc::clone(&state),
            store.clone(),
            waiters.clone(),
        );
        Self {
            connector: Some(connector),
            state,
            store,
            waiters,
        }
    }

    fn attach(&self, thread_id: &str) -> Result<CodexThread, String> {
        let connector = self
            .connector
            .as_ref()
            .ok_or_else(|| "Codex app-server Connector 当前不可用".to_owned())?;
        let thread = connector
            .resume_thread(thread_id)
            .map_err(|error| error.to_string())?;
        let waiting = thread_waiting_on_approval(&thread);
        let active = thread.status == "active";
        let managed = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "Connector 状态不可用".to_owned())?;
            state.managed.insert(thread_id.to_owned());
            state.resume_failed.remove(thread_id);
            state.native_waiting.insert(thread.id.clone(), waiting);
            state.threads.insert(thread.id.clone(), thread.clone());
            let mut managed = state.managed.iter().cloned().collect::<Vec<_>>();
            managed.sort();
            managed
        };
        self.store
            .write_setting(
                MANAGED_CODEX_THREADS_KEY,
                serde_json::to_string(&managed).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
        sync_codex_native_attention(
            &self.state,
            &self.store,
            &self.waiters,
            thread_id,
            waiting,
            active,
        );
        Ok(thread)
    }

    fn capability_value(&self) -> Value {
        let Ok(state) = self.state.lock() else {
            return json!({"status":"unavailable","error":"Connector 状态不可用"});
        };
        json!({
            "status": state.status,
            "error": state.error,
            "managedThreads": state.managed.len(),
            "protocol": "app-server",
            "experimentalUserInput": true
        })
    }

    fn retry_unsynced_native_approvals(&self) {
        let pending = self
            .state
            .lock()
            .map(|state| {
                state
                    .native_waiting
                    .iter()
                    .filter_map(|(thread_id, waiting)| {
                        (*waiting && !state.native_synced.contains(thread_id))
                            .then_some(thread_id.clone())
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for thread_id in pending {
            sync_codex_native_attention(
                &self.state,
                &self.store,
                &self.waiters,
                &thread_id,
                true,
                true,
            );
        }
    }

    fn recovery_for(
        &self,
        thread_id: &str,
        exec_state: &str,
    ) -> (String, String, bool, Option<String>) {
        let Ok(state) = self.state.lock() else {
            return (
                "external_hook".to_owned(),
                "waiting_for_event".to_owned(),
                false,
                None,
            );
        };
        let managed = state.managed.contains(thread_id);
        let status = state
            .threads
            .get(thread_id)
            .map(|thread| thread.status.clone());
        if managed {
            if state.status != "connected" || state.resume_failed.contains(thread_id) {
                return (
                    "managed".to_owned(),
                    "lost_control".to_owned(),
                    false,
                    status,
                );
            }
            let recovery = match status.as_deref() {
                Some("active") => "controllable",
                Some("idle") => "ended",
                Some("systemError") => "lost_control",
                Some("notLoaded") | None => "waiting_for_event",
                Some(_) => "waiting_for_event",
            };
            return ("managed".to_owned(), recovery.to_owned(), false, status);
        }
        let ended = matches!(exec_state, "idle" | "response_finished" | "failed");
        (
            "external_hook".to_owned(),
            if ended { "ended" } else { "observing" }.to_owned(),
            state.status == "connected",
            status,
        )
    }
}

fn spawn_codex_handlers(
    connector: CodexConnector,
    channels: ConnectorChannels,
    state: Arc<Mutex<CodexManagerState>>,
    store: RuntimeStore,
    waiters: WaiterRegistry,
) {
    let request_connector = connector.clone();
    let request_state = Arc::clone(&state);
    let request_store = store.clone();
    let request_waiters = waiters.clone();
    thread::spawn(move || {
        for request in channels.requests {
            handle_codex_server_request(
                request_connector.clone(),
                Arc::clone(&request_state),
                request_store.clone(),
                request_waiters.clone(),
                request,
            );
        }
        if let Ok(mut current) = request_state.lock() {
            current.status = "unavailable".to_owned();
            current.error = Some("Codex app-server 请求通道已断开".to_owned());
        }
    });
    let notification_store = store.clone();
    thread::spawn(move || {
        for notification in channels.notifications {
            update_codex_notification(&state, &notification_store, &waiters, notification);
        }
        if let Ok(mut current) = state.lock() {
            current.status = "unavailable".to_owned();
            current.error = Some("Codex app-server 通知通道已断开".to_owned());
        }
    });
}

fn handle_codex_server_request(
    connector: CodexConnector,
    state: Arc<Mutex<CodexManagerState>>,
    store: RuntimeStore,
    waiters: WaiterRegistry,
    request: ServerRequest,
) {
    if request.method != "item/tool/requestUserInput" {
        let _ = connector.respond_error(
            request.id,
            -32601,
            "Unsupported Flow Agent connector request",
        );
        return;
    }
    let Some(bridge_request) = BridgeRequest::codex_user_input_at(request.params, now_millis())
    else {
        let _ = connector.respond_error(request.id, -32602, "Invalid requestUserInput params");
        return;
    };
    let Some(thread_id) = bridge_request.provider_session_id.clone() else {
        let _ = connector.respond_error(request.id, -32602, "Missing threadId");
        return;
    };
    let request_id = bridge_request.request_id.unwrap_or(bridge_request.id);
    let registration = match waiters.register_at(&bridge_request, now_millis()) {
        Ok(registration) => registration,
        Err(_) => {
            let _ = connector.respond_error(request.id, -32000, "Question waiter unavailable");
            return;
        }
    };
    if store.ingest(bridge_request.clone()).is_err() {
        let _ = waiters.pass_through(request_id, "runtime_error");
        let _ = connector.respond_error(request.id, -32000, "Flow Agent storage unavailable");
        return;
    }
    let managed = if let Ok(mut current) = state.lock() {
        current.managed.insert(thread_id.clone());
        current.resume_failed.remove(&thread_id);
        current
            .threads
            .entry(thread_id.clone())
            .and_modify(|thread| {
                thread.status = "active".to_owned();
                if !thread
                    .active_flags
                    .iter()
                    .any(|flag| flag == "waitingOnUserInput")
                {
                    thread.active_flags.push("waitingOnUserInput".to_owned());
                }
            });
        let mut managed = current.managed.iter().cloned().collect::<Vec<_>>();
        managed.sort();
        Some(managed)
    } else {
        None
    };
    if let Some(managed) = managed.and_then(|value| serde_json::to_string(&value).ok()) {
        let _ = store.write_setting(MANAGED_CODEX_THREADS_KEY, managed);
    }
    thread::spawn(move || {
        let wait_for = bridge_request
            .deadline_at
            .map(|deadline| Duration::from_millis(deadline.saturating_sub(now_millis())))
            .unwrap_or(Duration::from_secs(60));
        match registration.ticket.recv_timeout(wait_for) {
            Ok(response) => match (response.action, response.payload) {
                (ReplyAction::Answer, Some(ReplyPayload::CodexUserInput { answers })) => {
                    let answers = answers
                        .into_iter()
                        .map(|(id, answers)| (id, json!({"answers": answers})))
                        .collect::<serde_json::Map<_, _>>();
                    let _ = connector.respond(request.id, json!({"answers": answers}));
                }
                _ => {
                    let _ =
                        connector.respond_error(request.id, -32001, "Question was not answered");
                }
            },
            Err(_) => {
                let _ = waiters.pass_through(request_id, "deadline");
                let _ = store.expire_approval(request_id, "deadline", now_millis());
                let _ = connector.respond_error(request.id, -32001, "Question expired");
            }
        }
    });
}

fn update_codex_notification(
    state: &Arc<Mutex<CodexManagerState>>,
    store: &RuntimeStore,
    waiters: &WaiterRegistry,
    notification: ServerNotification,
) {
    let started_thread = (notification.method == "thread/started")
        .then(|| notification.params.get("thread").and_then(parse_thread))
        .flatten();
    let thread_id = started_thread
        .as_ref()
        .map(|thread| thread.id.clone())
        .or_else(|| {
            notification
                .params
                .get("threadId")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        });
    let Some(thread_id) = thread_id else { return };

    let (waiting, active) = {
        let Ok(mut current) = state.lock() else {
            return;
        };
        current.resume_failed.remove(&thread_id);
        if let Some(thread) = started_thread {
            current.threads.insert(thread_id.clone(), thread);
        } else if notification.method == "thread/status/changed" {
            let Some(status) = notification
                .params
                .pointer("/status/type")
                .and_then(Value::as_str)
            else {
                return;
            };
            let flags = notification
                .params
                .pointer("/status/activeFlags")
                .and_then(Value::as_array)
                .map(|flags| {
                    flags
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToOwned::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            current
                .threads
                .entry(thread_id.clone())
                .and_modify(|thread| {
                    thread.status = status.to_owned();
                    thread.active_flags.clone_from(&flags);
                })
                .or_insert_with(|| CodexThread {
                    id: thread_id.clone(),
                    name: None,
                    cwd: None,
                    status: status.to_owned(),
                    active_flags: flags,
                    updated_at: None,
                });
        } else if matches!(
            notification.method.as_str(),
            "turn/completed" | "thread/closed"
        ) {
            if let Some(thread) = current.threads.get_mut(&thread_id) {
                thread.status = "idle".to_owned();
                thread.active_flags.clear();
            }
        } else if notification.method == "turn/started" {
            if let Some(thread) = current.threads.get_mut(&thread_id) {
                thread.status = "active".to_owned();
                thread.active_flags.clear();
            }
        } else {
            return;
        }
        let waiting = current
            .threads
            .get(&thread_id)
            .is_some_and(thread_waiting_on_approval);
        let changed = current.native_waiting.insert(thread_id.clone(), waiting) != Some(waiting);
        if !changed && (!waiting || current.native_synced.contains(&thread_id)) {
            return;
        }
        let active = current
            .threads
            .get(&thread_id)
            .is_some_and(|thread| thread.status == "active");
        (waiting, active)
    };
    sync_codex_native_attention(state, store, waiters, &thread_id, waiting, active);
}

fn thread_waiting_on_approval(thread: &CodexThread) -> bool {
    thread.status == "active"
        && thread
            .active_flags
            .iter()
            .any(|flag| flag == "waitingOnApproval")
}

fn sync_codex_native_attention(
    state: &Arc<Mutex<CodexManagerState>>,
    store: &RuntimeStore,
    waiters: &WaiterRegistry,
    thread_id: &str,
    waiting: bool,
    active: bool,
) {
    let Ok(result) = store.sync_native_approval(
        Provider::Codex,
        thread_id.to_owned(),
        waiting,
        active,
        now_millis(),
    ) else {
        return;
    };
    for request_id in result.resolved_request_ids {
        let _ = waiters.pass_through(request_id, "provider_handled");
    }
    let Ok(mut current) = state.lock() else {
        return;
    };
    if waiting && result.session_found {
        current.native_synced.insert(thread_id.to_owned());
    } else if !waiting {
        current.native_synced.remove(thread_id);
    }
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.css", get(styles))
        .route("/app.js", get(script))
        .route("/assets/claude.png", get(claude_icon))
        .route("/assets/codex.png", get(codex_icon))
        .route("/api/v1/health", get(health))
        .route("/api/v1/runtime/restart", post(restart_runtime))
        .route("/api/v1/bootstrap", post(bootstrap))
        .route("/api/v1/snapshot", get(snapshot))
        .route("/api/v1/setup", get(setup).post(change_setup))
        .route("/api/v1/settings", get(settings).put(update_settings))
        .route("/api/v1/quota/claude-bridge", post(change_claude_bridge))
        .route("/api/v1/export", get(export_data))
        .route("/api/v1/metrics", post(record_metric))
        .route("/api/v1/metrics/export", get(export_metrics))
        .route("/api/v1/data/clear", post(clear_data))
        .route("/api/v1/commands", post(command))
        .route("/api/v1/questions/{id}/answer", post(answer_question))
        .route("/api/v1/commands/{id}/undo", post(undo))
        .route("/api/v1/sessions/{id}/jump", post(jump_session))
        .route("/api/v1/sessions/{id}/manage", post(manage_session))
        .route("/api/v1/ws", get(websocket))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .with_state(state)
}

async fn index() -> Response {
    static_response("text/html; charset=utf-8", INDEX_HTML)
}

async fn styles() -> Response {
    static_response("text/css; charset=utf-8", APP_CSS)
}

async fn script() -> Response {
    static_response("text/javascript; charset=utf-8", APP_JS)
}

async fn claude_icon() -> Response {
    static_binary_response("image/png", CLAUDE_ICON)
}

async fn codex_icon() -> Response {
    static_binary_response("image/png", CODEX_ICON)
}

fn static_response(content_type: &'static str, body: &'static str) -> Response {
    (
        [
            (CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        body,
    )
        .into_response()
}

fn static_binary_response(content_type: &'static str, body: &'static [u8]) -> Response {
    (
        [
            (CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        Body::from(body),
    )
        .into_response()
}

async fn health(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !valid_host(&state, &headers) {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_HOST");
    }
    Json(json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "protocolVersion": 1
    }))
    .into_response()
}

async fn restart_runtime(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    let Some(handle) = state.runtime_restart.as_ref() else {
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "RUNTIME_RESTART_UNAVAILABLE",
        );
    };
    let receiver = match handle.request() {
        Ok(receiver) => receiver,
        Err(error) => {
            return api_error_detail(
                StatusCode::SERVICE_UNAVAILABLE,
                "RUNTIME_RESTART_FAILED",
                &error,
            )
        }
    };
    match receiver.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(())) => Json(json!({ "ok": true, "bridge": "restarted" })).into_response(),
        Ok(Err(error)) => api_error_detail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "RUNTIME_RESTART_FAILED",
            &error,
        ),
        Err(_) => api_error(StatusCode::GATEWAY_TIMEOUT, "RUNTIME_RESTART_TIMED_OUT"),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapRequest {
    token: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapResponse {
    csrf_token: String,
}

async fn bootstrap(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<BootstrapRequest>,
) -> Response {
    if !valid_same_origin(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "INVALID_ORIGIN");
    }
    let Ok(mut auth) = state.auth.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "AUTH_UNAVAILABLE");
    };
    if auth.bootstrap_token.as_deref() != Some(request.token.as_str()) {
        return api_error(StatusCode::UNAUTHORIZED, "INVALID_BOOTSTRAP");
    }
    auth.bootstrap_token = None;
    let session_token = Uuid::now_v7().to_string();
    let csrf_token = Uuid::now_v7().to_string();
    auth.session_token = Some(session_token.clone());
    auth.csrf_token = Some(csrf_token.clone());
    let cookie = format!("{SESSION_COOKIE}={session_token}; HttpOnly; SameSite=Strict; Path=/");
    let mut response = Json(BootstrapResponse { csrf_token }).into_response();
    if let Ok(cookie) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(SET_COOKIE, cookie);
    }
    response
}

async fn snapshot(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return api_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED");
    }
    snapshot_value(&state)
        .map(Json)
        .map(IntoResponse::into_response)
        .unwrap_or_else(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR"))
}

async fn setup(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return api_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED");
    }
    setup_value(&state)
        .map(Json)
        .map(IntoResponse::into_response)
        .unwrap_or_else(|error| {
            api_error_detail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "SETUP_INSPECTION_FAILED",
                &error,
            )
        })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetupChangeRequest {
    provider: String,
    action: String,
}

async fn change_setup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SetupChangeRequest>,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    let provider = match request.provider.as_str() {
        "claude" => HookProvider::Claude,
        "codex" => HookProvider::Codex,
        _ => return api_error(StatusCode::BAD_REQUEST, "UNKNOWN_PROVIDER"),
    };
    if request.action != "uninstall" && !discover_provider_availability(provider).is_available() {
        return api_error(StatusCode::CONFLICT, "PROVIDER_CLIENT_MISSING");
    }
    let enhanced_codex_activity = if provider == HookProvider::Codex {
        match load_ui_settings(&state) {
            Ok(settings) => settings.codex_enhanced_activity,
            Err(error) => {
                return api_error_detail(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "SETTINGS_READ_FAILED",
                    &error.to_string(),
                )
            }
        }
    } else {
        false
    };
    let options = InstallOptions {
        enhanced_codex_activity,
    };
    let changed = match request.action.as_str() {
        "install" => state.installer.install(provider, options).map(|_| ()),
        "repair" => state.installer.repair(provider, options).map(|_| ()),
        "uninstall" => state.installer.uninstall(provider).map(|_| ()),
        _ => return api_error(StatusCode::BAD_REQUEST, "UNKNOWN_SETUP_ACTION"),
    };
    if let Err(error) = changed {
        return api_error_detail(
            StatusCode::CONFLICT,
            "SETUP_CHANGE_FAILED",
            &error.to_string(),
        );
    }
    setup_value(&state)
        .map(Json)
        .map(IntoResponse::into_response)
        .unwrap_or_else(|error| {
            api_error_detail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "SETUP_INSPECTION_FAILED",
                &error,
            )
        })
}

fn setup_value(state: &AppState) -> Result<Value, String> {
    let runtime = state.store.snapshot().map_err(|error| error.to_string())?;
    let mut providers = Vec::new();
    for provider in [HookProvider::Claude, HookProvider::Codex] {
        let inspection = state
            .installer
            .inspect(provider)
            .map_err(|error| error.to_string())?;
        let availability = discover_provider_availability(provider);
        let provider_available = availability.is_available();
        let cli_installed = availability.cli_path.is_some();
        let desktop_installed = availability.desktop_app_path.is_some();
        let real_event_verified =
            inspection
                .installed_definition_changed_at_ms
                .is_some_and(|installed_at| {
                    runtime.sessions.iter().any(|session| {
                        session.provider == provider.as_str()
                            && session.last_event_at >= installed_at
                    })
                });
        let status = if !provider_available {
            "provider_missing"
        } else if inspection.config_health == ConfigHealth::Malformed
            || inspection.codex_config_error.is_some()
        {
            "error"
        } else if !inspection.codex_inline_events.is_empty() {
            "inline_conflict"
        } else if inspection.definition_matches_manifest
            && inspection.binary_health == BinaryHealth::Executable
        {
            if real_event_verified
                && (provider != HookProvider::Codex
                    || inspection.codex_trust_status == Some(CodexTrustStatus::TrustedStatePresent))
            {
                "connected"
            } else if provider == HookProvider::Codex
                && inspection.codex_trust_status == Some(CodexTrustStatus::ReviewRequired)
            {
                "needs_trust"
            } else {
                "installed_unverified"
            }
        } else if inspection.owned_handlers > 0 {
            "needs_reinstall"
        } else {
            "not_installed"
        };
        providers.push(json!({
            "provider": provider.as_str(),
            "status": status,
            "cliInstalled": cli_installed,
            "desktopInstalled": desktop_installed,
            "desktopAppPath": availability.desktop_app_path,
            "reviewCommand": if provider == HookProvider::Codex {
                availability.codex_review_command()
            } else {
                None
            },
            "intent": inspection.intent,
            "configPath": inspection.config_path,
            "ownedHandlers": inspection.owned_handlers,
            "expectedHandlers": inspection.expected_handlers,
            "binaryHealth": inspection.binary_health,
            "trustStatus": inspection.codex_trust_status,
            "featureStatus": inspection.codex_feature_status,
            "inlineEvents": inspection.codex_inline_events,
            "canRepair": inspection.definition_matches_manifest
                && inspection.binary_health != BinaryHealth::Executable,
            "realEventVerified": real_event_verified,
        }));
    }
    let first_run = providers.iter().all(|provider| {
        matches!(
            provider.get("status").and_then(Value::as_str),
            Some("not_installed") | Some("provider_missing") | Some("cli_missing")
        )
    });
    Ok(json!({
        "schemaVersion": 1,
        "firstRun": first_run,
        "providers": providers,
        "safety": {
            "backsUpBeforeWrite": true,
            "codexTrustIsManual": true,
            "repairRespectsRemoval": true
        }
    }))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
struct UiSettings {
    notification_rules: NotificationRules,
    sound_enabled: bool,
    provider_muted: ProviderMuted,
    codex_enhanced_activity: bool,
    retention_days: u32,
    display_profile: String,
    task_card_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NotificationRules {
    approval: String,
    question: String,
    error: String,
    completion: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProviderMuted {
    claude: bool,
    codex: bool,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            notification_rules: NotificationRules {
                approval: "list".to_owned(),
                question: "list".to_owned(),
                error: "list".to_owned(),
                completion: "list".to_owned(),
            },
            sound_enabled: true,
            provider_muted: ProviderMuted {
                claude: false,
                codex: false,
            },
            codex_enhanced_activity: true,
            retention_days: 90,
            display_profile: "detailed".to_owned(),
            task_card_fields: vec![
                "project".to_owned(),
                "task".to_owned(),
                "model".to_owned(),
                "activity".to_owned(),
                "plan".to_owned(),
                "tokens".to_owned(),
                "context".to_owned(),
                "tool".to_owned(),
                "subagents".to_owned(),
                "environment".to_owned(),
                "recovery".to_owned(),
                "control".to_owned(),
                "jump".to_owned(),
            ],
        }
    }
}

impl UiSettings {
    fn validate(&self) -> Result<(), &'static str> {
        for mode in [
            self.notification_rules.approval.as_str(),
            self.notification_rules.question.as_str(),
            self.notification_rules.error.as_str(),
            self.notification_rules.completion.as_str(),
        ] {
            if !matches!(mode, "banner" | "list" | "ignore") {
                return Err("notification mode must be banner, list, or ignore");
            }
        }
        if !matches!(self.retention_days, 0 | 30 | 90 | 180 | 365) {
            return Err("retentionDays must be 0, 30, 90, 180, or 365");
        }
        if !matches!(
            self.display_profile.as_str(),
            "concise" | "detailed" | "developer"
        ) {
            return Err("displayProfile must be concise, detailed, or developer");
        }
        if self.task_card_fields.len() > TASK_CARD_DISPLAY_FIELDS.len() {
            return Err("taskCardFields contains too many fields");
        }
        let mut unique = HashSet::new();
        for field in &self.task_card_fields {
            if !TASK_CARD_DISPLAY_FIELDS.contains(&field.as_str()) {
                return Err("taskCardFields contains an unsupported field");
            }
            if !unique.insert(field.as_str()) {
                return Err("taskCardFields contains duplicate fields");
            }
        }
        Ok(())
    }
}

async fn settings(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return api_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED");
    }
    settings_value(&state)
        .map(Json)
        .map(IntoResponse::into_response)
        .unwrap_or_else(|error| {
            api_error_detail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "SETTINGS_READ_FAILED",
                &error,
            )
        })
}

async fn update_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(next): Json<UiSettings>,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    if let Err(reason) = next.validate() {
        return api_error_detail(StatusCode::BAD_REQUEST, "INVALID_SETTINGS", reason);
    }
    let current = match load_ui_settings(&state) {
        Ok(settings) => settings,
        Err(error) => {
            return api_error_detail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "SETTINGS_READ_FAILED",
                &error.to_string(),
            )
        }
    };
    if current.codex_enhanced_activity != next.codex_enhanced_activity {
        let inspection = match state.installer.inspect(HookProvider::Codex) {
            Ok(inspection) => inspection,
            Err(error) => {
                return api_error_detail(
                    StatusCode::CONFLICT,
                    "CODEX_REINSTALL_FAILED",
                    &error.to_string(),
                )
            }
        };
        if inspection.intent == InstallIntent::Installed {
            if !discover_provider_availability(HookProvider::Codex).is_available() {
                return api_error(StatusCode::CONFLICT, "PROVIDER_CLIENT_MISSING");
            }
            if let Err(error) = state.installer.install(
                HookProvider::Codex,
                InstallOptions {
                    enhanced_codex_activity: next.codex_enhanced_activity,
                },
            ) {
                return api_error_detail(
                    StatusCode::CONFLICT,
                    "CODEX_REINSTALL_FAILED",
                    &error.to_string(),
                );
            }
        }
    }
    let encoded = match serde_json::to_string(&next) {
        Ok(encoded) => encoded,
        Err(_) => return api_error(StatusCode::BAD_REQUEST, "INVALID_SETTINGS"),
    };
    if state.store.write_setting(SETTINGS_KEY, encoded).is_err() {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR");
    }
    if state
        .store
        .prune_events(next.retention_days, now_millis())
        .is_err()
    {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "RETENTION_FAILED");
    }
    settings_value(&state)
        .map(Json)
        .map(IntoResponse::into_response)
        .unwrap_or_else(|error| {
            api_error_detail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "SETTINGS_READ_FAILED",
                &error,
            )
        })
}

fn load_ui_settings(state: &AppState) -> Result<UiSettings, StoreError> {
    match state.store.read_setting(SETTINGS_KEY)? {
        Some(value) => serde_json::from_str::<UiSettings>(&value)
            .map_err(|error| StoreError::Storage(format!("settings JSON is invalid: {error}"))),
        None => Ok(UiSettings::default()),
    }
}

fn settings_value(state: &AppState) -> Result<Value, String> {
    let settings = load_ui_settings(state).map_err(|error| error.to_string())?;
    let bridge = state
        .installer
        .inspect_claude_statusline()
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "settings": settings,
        "displayCatalog": [
            { "id": "project", "label": "项目名", "level": "detailed" },
            { "id": "task", "label": "任务内容", "level": "concise" },
            { "id": "model", "label": "模型", "level": "detailed" },
            { "id": "activity", "label": "实时状态与本轮时间", "level": "concise" },
            { "id": "plan", "label": "计划进度", "level": "detailed" },
            { "id": "tokens", "label": "会话 Token 与估算价格", "level": "detailed" },
            { "id": "context", "label": "上下文占用", "level": "detailed" },
            { "id": "tool", "label": "当前工具", "level": "detailed" },
            { "id": "permissionMode", "label": "权限模式", "level": "detailed" },
            { "id": "subagents", "label": "运行中的子 Agent", "level": "detailed" },
            { "id": "environment", "label": "运行环境", "level": "detailed" },
            { "id": "recovery", "label": "恢复状态", "level": "detailed" },
            { "id": "control", "label": "控制能力", "level": "detailed" },
            { "id": "jump", "label": "跳转能力", "level": "detailed" },
            { "id": "titleSource", "label": "标题来源", "level": "developer" },
            { "id": "sessionId", "label": "Flow Agent Session ID", "level": "developer" },
            { "id": "providerSessionId", "label": "Provider Session ID", "level": "developer" },
            { "id": "providerTurnId", "label": "Provider Turn ID", "level": "developer" },
            { "id": "lastEventAt", "label": "最后事件时间", "level": "developer" }
        ],
        "claudeQuotaBridge": {
            "status": bridge.status,
            "configPath": bridge.config_path,
            "helperPath": bridge.helper_path,
            "customConflict": bridge.status == ClaudeStatuslineStatus::CustomConflict,
        }
    }))
}

#[derive(Debug, Deserialize)]
struct BridgeChangeRequest {
    action: String,
}

async fn change_claude_bridge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<BridgeChangeRequest>,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    if matches!(request.action.as_str(), "install" | "wrap")
        && discover_provider_availability(HookProvider::Claude)
            .cli_path
            .is_none()
    {
        return api_error(StatusCode::CONFLICT, "PROVIDER_CLI_REQUIRED");
    }
    let result = match request.action.as_str() {
        "install" => state.installer.install_claude_statusline().map(|_| ()),
        "wrap" => state
            .installer
            .install_claude_statusline_wrapper()
            .map(|_| ()),
        "uninstall" => state.installer.uninstall_claude_statusline().map(|_| ()),
        _ => return api_error(StatusCode::BAD_REQUEST, "UNKNOWN_BRIDGE_ACTION"),
    };
    if let Err(error) = result {
        return api_error_detail(
            StatusCode::CONFLICT,
            "CLAUDE_BRIDGE_CHANGE_FAILED",
            &error.to_string(),
        );
    }
    invalidate_quota(&state);
    settings_value(&state)
        .map(Json)
        .map(IntoResponse::into_response)
        .unwrap_or_else(|error| {
            api_error_detail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "SETTINGS_READ_FAILED",
                &error,
            )
        })
}

async fn export_data(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return api_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED");
    }
    let export = match state.store.export_json(now_millis()) {
        Ok(export) => export,
        Err(_) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "EXPORT_FAILED"),
    };
    let body = match serde_json::to_vec_pretty(&export) {
        Ok(body) => body,
        Err(_) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "EXPORT_FAILED"),
    };
    (
        [
            (CONTENT_TYPE, HeaderValue::from_static("application/json")),
            (
                CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment; filename=flow-agent-export.json"),
            ),
            (CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        body,
    )
        .into_response()
}

async fn export_metrics(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return api_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED");
    }
    let export = match state.store.export_metrics_json(now_millis()) {
        Ok(export) => export,
        Err(_) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "EXPORT_FAILED"),
    };
    let body = match serde_json::to_vec_pretty(&export) {
        Ok(body) => body,
        Err(_) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "EXPORT_FAILED"),
    };
    (
        [
            (CONTENT_TYPE, HeaderValue::from_static("application/json")),
            (
                CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment; filename=flow-agent-metrics.json"),
            ),
            (CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        body,
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum UiMetricEvent {
    AppOpened,
    BannerShown,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MetricRequest {
    event: UiMetricEvent,
}

async fn record_metric(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<MetricRequest>,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    let event = match request.event {
        UiMetricEvent::AppOpened => MetricEvent::AppOpened,
        UiMetricEvent::BannerShown => MetricEvent::BannerShown,
    };
    match state.store.record_metric(event, now_millis()) {
        Ok(()) => Json(json!({"recorded": true})).into_response(),
        Err(_) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "METRIC_RECORD_FAILED"),
    }
}

#[derive(Debug, Deserialize)]
struct ClearDataRequest {
    confirmation: String,
}

async fn clear_data(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ClearDataRequest>,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    if request.confirmation != "DELETE" {
        return api_error(StatusCode::BAD_REQUEST, "DELETE_CONFIRMATION_REQUIRED");
    }
    for path in [
        &state.data_paths.cache,
        &state.data_paths.spool,
        &state.data_paths.diagnostics,
    ] {
        if let Err(error) = removable_owned_tree(path) {
            return api_error_detail(StatusCode::CONFLICT, "UNSAFE_DATA_PATH", &error);
        }
    }
    if state.store.clear_data().is_err() {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "CLEAR_FAILED");
    }
    for path in [
        &state.data_paths.cache,
        &state.data_paths.spool,
        &state.data_paths.diagnostics,
    ] {
        if path.exists() && fs::remove_dir_all(path).is_err() {
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, "CLEAR_FAILED");
        }
    }
    invalidate_quota(&state);
    Json(json!({ "cleared": true })).into_response()
}

fn removable_owned_tree(path: &FilePath) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(format!("refusing symbolic link {}", path.display()))
        }
        Ok(metadata) if !metadata.is_dir() => Err(format!("{} is not a directory", path.display())),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn invalidate_quota(state: &AppState) {
    if let Ok(mut quota) = state.quota.lock() {
        quota.entries.clear();
        quota.refreshed_at = None;
        quota.claude_cache_modified_at = None;
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandRequest {
    id: Uuid,
    attention_id: String,
    request_id: Option<Uuid>,
    action: String,
}

async fn command(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CommandRequest>,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    let Ok(snapshot) = state.store.snapshot() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR");
    };
    let Some(attention) = snapshot
        .attention
        .iter()
        .find(|item| item.id == request.attention_id)
    else {
        return api_error(StatusCode::CONFLICT, "STALE_ATTENTION");
    };
    if attention.request_id != request.request_id {
        return api_error(StatusCode::CONFLICT, "REQUEST_MISMATCH");
    }
    let normalized_request_action = if request.action == "dismiss" && attention.kind == "approval" {
        "pass_through"
    } else {
        request.action.as_str()
    };
    if let Some(existing) = snapshot
        .commands
        .iter()
        .find(|command| command.id == request.id)
    {
        if existing.attention_id != request.attention_id
            || existing.request_id != request.request_id
            || existing.action != normalized_request_action
        {
            return api_error(StatusCode::CONFLICT, "COMMAND_MISMATCH");
        }
        let status = if existing.state == "pending_commit" {
            StatusCode::ACCEPTED
        } else {
            StatusCode::OK
        };
        return command_state_response(status, request.id, &existing.state);
    }
    let now = now_millis();
    match request.action.as_str() {
        "approve" | "deny" => {
            let Some(request_id) = request.request_id else {
                return api_error(StatusCode::CONFLICT, "MISSING_REQUEST_ID");
            };
            if attention.kind != "approval" || !state.waiters.is_active(request_id).unwrap_or(false)
            {
                let _ = state.store.expire_approval(request_id, "stale_waiter", now);
                return api_error(StatusCode::CONFLICT, "STALE_APPROVAL");
            }
            let action = if request.action == "approve" {
                ApprovalAction::Approve
            } else {
                ApprovalAction::Deny
            };
            let claim = match state
                .store
                .claim_approval(request.id, request_id, action, now)
            {
                Ok(claim) => claim,
                Err(error) => return store_error_response(error),
            };
            if claim.created {
                let Some(commit_due_at) = claim.commit_due_at else {
                    return api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR");
                };
                schedule_decision(state.clone(), request.id, request_id, action, commit_due_at);
            }
            command_response(StatusCode::ACCEPTED, request.id, claim.state)
        }
        "pass_through" | "dismiss" if attention.kind == "approval" => {
            let Some(request_id) = request.request_id else {
                return api_error(StatusCode::CONFLICT, "MISSING_REQUEST_ID");
            };
            if attention.kind != "approval" || !state.waiters.is_active(request_id).unwrap_or(false)
            {
                let _ = state.store.expire_approval(request_id, "stale_waiter", now);
                return api_error(StatusCode::CONFLICT, "STALE_APPROVAL");
            }
            let claim = match state.store.claim_approval(
                request.id,
                request_id,
                ApprovalAction::PassThrough,
                now,
            ) {
                Ok(claim) => claim,
                Err(error) => return store_error_response(error),
            };
            if claim.created && state.waiters.pass_through(request_id, "user").is_err() {
                return api_error(StatusCode::CONFLICT, "STALE_APPROVAL");
            }
            command_response(StatusCode::OK, request.id, claim.state)
        }
        "ack" | "snooze" | "dismiss" => {
            if attention.kind == "approval" {
                return api_error(StatusCode::CONFLICT, "INVALID_ACTION");
            }
            let action = match request.action.as_str() {
                "ack" => AttentionAction::Ack,
                "snooze" => AttentionAction::Snooze,
                _ => AttentionAction::Dismiss,
            };
            match state
                .store
                .act_on_attention(request.id, &request.attention_id, action, now)
            {
                Ok(command_state) => command_response(StatusCode::OK, request.id, command_state),
                Err(error) => store_error_response(error),
            }
        }
        _ => api_error(StatusCode::BAD_REQUEST, "UNKNOWN_ACTION"),
    }
}

async fn answer_question(
    State(state): State<AppState>,
    Path(request_id): Path<Uuid>,
    headers: HeaderMap,
    Json(submission): Json<Value>,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    if !submission.is_object() {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_ANSWER");
    }
    let snapshot = match state.store.snapshot() {
        Ok(snapshot) => snapshot,
        Err(_) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR"),
    };
    let Some(attention) = snapshot.attention.iter().find(|item| {
        item.request_id == Some(request_id) && item.kind == "question" && item.state == "open"
    }) else {
        return api_error(StatusCode::CONFLICT, "QUESTION_EXPIRED");
    };
    let native_provider_ui = submission.get("action").and_then(Value::as_str) == Some("native");
    let result = if native_provider_ui {
        state.waiters.pass_through(request_id, "native_provider_ui")
    } else {
        state.waiters.answer(request_id, &submission)
    };
    match result {
        Ok(()) => {}
        Err(WaiterError::InvalidAnswer | WaiterError::NotInteractive) => {
            return api_error(StatusCode::BAD_REQUEST, "INVALID_ANSWER")
        }
        Err(WaiterError::NotActive) => {
            let _ = state
                .store
                .expire_approval(request_id, "stale_waiter", now_millis());
            return api_error(StatusCode::CONFLICT, "QUESTION_EXPIRED");
        }
        Err(WaiterError::Poisoned | WaiterError::NotBlocking) => {
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, "ANSWER_FAILED")
        }
    }
    let command_id = Uuid::now_v7();
    if state
        .store
        .act_on_attention(
            command_id,
            &attention.id,
            AttentionAction::Ack,
            now_millis(),
        )
        .is_err()
    {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR");
    }
    Json(json!({
        "requestId": request_id,
        "state": if native_provider_ui { "passed_through" } else { "answered" }
    }))
    .into_response()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum JumpTarget {
    CodexThread(String),
    ITermSession,
    TerminalTty,
    AppBundle(&'static str),
    Unsupported,
}

async fn jump_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    let Ok(snapshot) = state.store.snapshot() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR");
    };
    let Some(session) = snapshot
        .sessions
        .iter()
        .find(|session| session.id == session_id)
    else {
        return api_error(StatusCode::NOT_FOUND, "SESSION_NOT_FOUND");
    };
    let target = jump_target(session);
    if target == JumpTarget::Unsupported {
        return api_error(StatusCode::CONFLICT, "JUMP_UNSUPPORTED");
    }
    match run_jump_target(&target, session) {
        Ok(true) => Json(json!({
            "success": true,
            "capability": session.jump_capability,
            "label": session.jump_label,
        }))
        .into_response(),
        Ok(false) | Err(_) => api_error_detail(
            StatusCode::CONFLICT,
            "JUMP_FAILED",
            "系统没有找到目标窗口，或尚未授予应用控制权限",
        ),
    }
}

#[derive(Debug, Deserialize)]
struct ManageSessionRequest {
    action: String,
}

async fn manage_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<ManageSessionRequest>,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    if request.action != "attach" {
        return api_error(StatusCode::BAD_REQUEST, "UNKNOWN_MANAGE_ACTION");
    }
    let snapshot = match state.store.snapshot() {
        Ok(snapshot) => snapshot,
        Err(_) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR"),
    };
    let Some(session) = snapshot
        .sessions
        .iter()
        .find(|session| session.id == session_id)
    else {
        return api_error(StatusCode::NOT_FOUND, "SESSION_NOT_FOUND");
    };
    if session.provider != "codex" {
        return api_error(StatusCode::CONFLICT, "MANAGED_CONNECTOR_UNSUPPORTED");
    }
    match state.codex.attach(&session.provider_session_id) {
        Ok(thread) => Json(json!({
            "attached": true,
            "threadId": thread.id,
            "status": thread.status,
            "controlCapability": "managed"
        }))
        .into_response(),
        Err(error) => api_error_detail(StatusCode::CONFLICT, "CONNECTOR_ATTACH_FAILED", &error),
    }
}

fn jump_target(session: &SessionRecord) -> JumpTarget {
    if session.jump_capability == "exact_conversation"
        && session.provider == "codex"
        && session.term_surface.as_deref() == Some("codex_app")
        && Uuid::parse_str(&session.provider_session_id).is_ok()
    {
        return JumpTarget::CodexThread(session.provider_session_id.clone());
    }
    let app = session
        .term_app
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let bundle = session
        .term_bundle_id
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if session.jump_capability == "terminal" {
        if (app.contains("iterm") || bundle == "com.googlecode.iterm2")
            && session.term_session_id.as_deref().is_some_and(safe_locator)
        {
            return JumpTarget::ITermSession;
        }
        if (app == "apple_terminal" || bundle == "com.apple.terminal")
            && session.term_tty.as_deref().is_some_and(safe_locator)
        {
            return JumpTarget::TerminalTty;
        }
    }
    if session.jump_capability == "app_only" {
        if session.term_surface.as_deref() == Some("codex_app") || bundle == "com.openai.codex" {
            return JumpTarget::AppBundle("com.openai.codex");
        }
        if session.term_surface.as_deref() == Some("claude_app")
            || bundle == "com.anthropic.claudefordesktop"
        {
            return JumpTarget::AppBundle("com.anthropic.claudefordesktop");
        }
        if app.contains("iterm") || bundle == "com.googlecode.iterm2" {
            return JumpTarget::AppBundle("com.googlecode.iterm2");
        }
        if app == "apple_terminal" || bundle == "com.apple.terminal" {
            return JumpTarget::AppBundle("com.apple.Terminal");
        }
        if app == "vscode" || bundle == "com.microsoft.vscode" {
            return JumpTarget::AppBundle("com.microsoft.VSCode");
        }
        if app.contains("warp") || bundle.starts_with("dev.warp.") {
            return JumpTarget::AppBundle("dev.warp.Warp-Stable");
        }
    }
    JumpTarget::Unsupported
}

fn safe_locator(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '/' | '_' | '.' | ':' | '-')
        })
}

fn run_jump_target(target: &JumpTarget, session: &SessionRecord) -> io::Result<bool> {
    let status = match target {
        JumpTarget::CodexThread(thread_id) => ProcessCommand::new("/usr/bin/open")
            .arg(format!("codex://threads/{thread_id}"))
            .status()?,
        JumpTarget::ITermSession => ProcessCommand::new("/usr/bin/osascript")
            .arg("-e")
            .arg(ITERM_JUMP_SCRIPT)
            .env(
                "FLOW_AGENT_JUMP_SESSION",
                session.term_session_id.as_deref().unwrap_or_default(),
            )
            .env(
                "FLOW_AGENT_JUMP_TTY",
                session.term_tty.as_deref().unwrap_or_default(),
            )
            .status()?,
        JumpTarget::TerminalTty => ProcessCommand::new("/usr/bin/osascript")
            .arg("-e")
            .arg(TERMINAL_JUMP_SCRIPT)
            .env(
                "FLOW_AGENT_JUMP_TTY",
                session.term_tty.as_deref().unwrap_or_default(),
            )
            .status()?,
        JumpTarget::AppBundle(bundle) => ProcessCommand::new("/usr/bin/open")
            .args(["-b", *bundle])
            .status()?,
        JumpTarget::Unsupported => return Ok(false),
    };
    Ok(status.success())
}

const ITERM_JUMP_SCRIPT: &str = r#"
set targetSession to system attribute "FLOW_AGENT_JUMP_SESSION"
set targetTty to system attribute "FLOW_AGENT_JUMP_TTY"
tell application "iTerm2"
  repeat with candidateWindow in windows
    repeat with candidateTab in tabs of candidateWindow
      repeat with candidateSession in sessions of candidateTab
        if (unique ID of candidateSession is targetSession) or (targetTty is not "" and tty of candidateSession is targetTty) then
          select candidateSession
          activate
          return
        end if
      end repeat
    end repeat
  end repeat
end tell
error "target session not found"
"#;

const TERMINAL_JUMP_SCRIPT: &str = r#"
set targetTty to system attribute "FLOW_AGENT_JUMP_TTY"
tell application "Terminal"
  repeat with candidateWindow in windows
    repeat with candidateTab in tabs of candidateWindow
      if tty of candidateTab is targetTty then
        set selected tab of candidateWindow to candidateTab
        set index of candidateWindow to 1
        activate
        return
      end if
    end repeat
  end repeat
end tell
error "target tab not found"
"#;

fn schedule_decision(
    state: AppState,
    command_id: Uuid,
    request_id: Uuid,
    action: ApprovalAction,
    commit_due_at: u64,
) {
    tokio::spawn(async move {
        tokio::time::sleep(state.commit_delay).await;
        let waiter_active = state.waiters.is_active(request_id).unwrap_or(false);
        let result = state.store.commit(command_id, commit_due_at, waiter_active);
        let Ok(committed) = result else { return };
        let Some(decision) = action.decision() else {
            return;
        };
        if state.waiters.decide(request_id, decision).is_err() {
            let _ = state.store.expire_approval(
                committed.request_id,
                "waiter_delivery_failed",
                now_millis(),
            );
        }
    });
}

async fn undo(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    let Ok(command_id) = Uuid::parse_str(&id) else {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_COMMAND_ID");
    };
    match state.store.undo(command_id, now_millis()) {
        Ok(command_state) => command_response(StatusCode::OK, command_id, command_state),
        Err(error) => store_error_response(error),
    }
}

#[derive(Debug, Deserialize)]
struct WebSocketQuery {
    csrf: String,
}

async fn websocket(
    State(state): State<AppState>,
    Query(query): Query<WebSocketQuery>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !authorized(&state, &headers)
        || !valid_same_origin(&state, &headers)
        || !valid_csrf_value(&state, &query.csrf)
    {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_WEBSOCKET");
    }
    upgrade
        .on_upgrade(move |socket| websocket_loop(socket, state))
        .into_response()
}

async fn websocket_loop(mut socket: WebSocket, state: AppState) {
    let mut last_payload = String::new();
    while let Ok(snapshot) = snapshot_value(&state) {
        let payload = json!({ "type": "snapshot", "snapshot": snapshot }).to_string();
        if payload != last_payload {
            if socket
                .send(Message::Text(payload.clone().into()))
                .await
                .is_err()
            {
                break;
            }
            last_payload = payload;
        }
        match tokio::time::timeout(state.snapshot_interval, socket.recv()).await {
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Ok(Some(Err(_))) => break,
            Ok(Some(Ok(_))) | Err(_) => {}
        }
    }
}

fn snapshot_value(state: &AppState) -> Result<Value, StoreError> {
    let now = now_millis();
    state.codex.retry_unsynced_native_approvals();
    refresh_session_usage(state, now)?;
    let mut snapshot = state.store.snapshot()?;
    let visible_attention_sessions = snapshot
        .attention
        .iter()
        .filter(|item| {
            matches!(
                item.state.as_str(),
                "open" | "committing" | "decision_sent" | "snoozed"
            )
        })
        .map(|item| item.session_id.as_str())
        .collect::<HashSet<_>>();
    let cutoff = now.saturating_sub(SESSION_LIST_RETENTION_MS);
    snapshot.sessions.retain(|session| {
        let active = !matches!(
            session.exec_state.as_str(),
            "idle" | "response_finished" | "failed"
        );
        active
            || session.last_event_at >= cutoff
            || visible_attention_sessions.contains(session.id.as_str())
    });
    let mut sessions = serde_json::to_value(&snapshot.sessions)
        .map_err(|error| StoreError::Storage(error.to_string()))?;
    if let Some(items) = sessions.as_array_mut() {
        for (index, session) in snapshot.sessions.iter().enumerate() {
            let (control, mut recovery, can_manage, connector_status) =
                if session.provider == "codex" {
                    state
                        .codex
                        .recovery_for(&session.provider_session_id, &session.exec_state)
                } else {
                    let ended = matches!(
                        session.exec_state.as_str(),
                        "idle" | "response_finished" | "failed"
                    );
                    (
                        "external_hook".to_owned(),
                        if ended { "ended" } else { "observing" }.to_owned(),
                        false,
                        None,
                    )
                };
            if control == "external_hook"
                && !matches!(
                    session.exec_state.as_str(),
                    "idle" | "response_finished" | "failed"
                )
            {
                recovery = match session.provider_pid {
                    Some(pid) if process_alive(pid) => "observing".to_owned(),
                    Some(_) => "lost_control".to_owned(),
                    None => "waiting_for_event".to_owned(),
                };
            }
            if let Some(object) = items.get_mut(index).and_then(Value::as_object_mut) {
                object.insert("controlCapability".to_owned(), Value::String(control));
                object.insert("recoveryState".to_owned(), Value::String(recovery));
                object.insert("canManage".to_owned(), Value::Bool(can_manage));
                if let Some(status) = connector_status {
                    object.insert("connectorThreadStatus".to_owned(), Value::String(status));
                }
            }
        }
    }
    let mut attention = serde_json::to_value(&snapshot.attention)
        .map_err(|error| StoreError::Storage(error.to_string()))?;
    if let Some(items) = attention.as_array_mut() {
        for (index, item) in snapshot.attention.iter().enumerate() {
            let Some(request_id) = item.request_id else {
                continue;
            };
            let Ok(Some(interaction)) = state.waiters.interactive_prompt(request_id) else {
                continue;
            };
            if let Some(object) = items.get_mut(index).and_then(Value::as_object_mut) {
                object.insert(
                    "interaction".to_owned(),
                    serde_json::to_value(interaction)
                        .map_err(|error| StoreError::Storage(error.to_string()))?,
                );
            }
        }
    }
    let quota = quota_entries(state)?;
    Ok(json!({
        "sessions": sessions,
        "attention": attention,
        "commands": snapshot.commands,
        "quota": quota,
        "stats": {
            "eventCount": snapshot.event_count,
            "metrics": snapshot.metrics
        },
        "capabilities": {
            "codexConnector": state.codex.capability_value()
        }
    }))
}

fn refresh_session_usage(state: &AppState, now: u64) -> Result<(), StoreError> {
    let records = {
        let mut usage = state
            .usage
            .lock()
            .map_err(|_| StoreError::Storage("usage collector lock is poisoned".to_owned()))?;
        if usage
            .refreshed_at
            .is_some_and(|instant| instant.elapsed() < USAGE_POLL_INTERVAL)
        {
            return Ok(());
        }
        let records = usage.collector.collect(now);
        usage.refreshed_at = Some(Instant::now());
        records
    };
    state
        .store
        .upsert_session_usages(records.into_iter().map(runtime_usage_record).collect())
}

fn runtime_usage_record(record: UsageRecord) -> SessionUsageRecord {
    SessionUsageRecord {
        provider: record.provider,
        provider_session_id: record.provider_session_id,
        input_tokens: record.input_tokens,
        output_tokens: record.output_tokens,
        cache_read_tokens: record.cache_read_tokens,
        cache_creation_tokens: record.cache_creation_tokens,
        reasoning_tokens: record.reasoning_tokens,
        token_total: record.token_total,
        last_turn_tokens: record.last_turn_tokens,
        context_used_tokens: record.context_used_tokens,
        context_window_tokens: record.context_window_tokens,
        context_used_percent: record.context_used_percent,
        estimated_cost_usd_micros: record.estimated_cost_usd_micros,
        cost_kind: record.cost_kind,
        pricing_source: record.pricing_source,
        usage_source: record.usage_source,
        usage_quality: record.usage_quality,
        captured_at: record.captured_at,
    }
}

fn process_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    if pid <= 1 {
        return false;
    }
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

fn quota_entries(state: &AppState) -> Result<Vec<QuotaEntry>, StoreError> {
    let mut quota = state
        .quota
        .lock()
        .map_err(|_| StoreError::Storage("quota collector lock is poisoned".to_owned()))?;
    let claude_cache_modified_at = fs::metadata(quota.collector.paths().claude_cache())
        .and_then(|metadata| metadata.modified())
        .ok();
    let claude_cache_changed = claude_cache_modified_at != quota.claude_cache_modified_at;
    let refresh = claude_cache_changed
        || quota
            .refreshed_at
            .is_none_or(|instant| instant.elapsed() >= state.quota_poll_interval);
    let start_oauth_refresh =
        refresh && state.claude_oauth_quota && !quota.oauth_refresh_in_progress;
    if start_oauth_refresh {
        quota.oauth_refresh_in_progress = true;
    }
    if refresh {
        let now = now_millis();
        quota.entries = quota.collector.collect(now);
        quota.refreshed_at = Some(Instant::now());
        quota.claude_cache_modified_at = claude_cache_modified_at;
        let persisted = quota.entries.iter().filter_map(quota_record).collect();
        state.store.replace_quota_snapshots(persisted)?;
    }
    let entries = quota.entries.clone();
    let paths = start_oauth_refresh.then(|| quota.collector.paths().clone());
    drop(quota);
    if let Some(paths) = paths {
        spawn_oauth_quota_refresh(state.quota.clone(), state.store.clone(), paths);
    }
    Ok(entries)
}

fn spawn_oauth_quota_refresh(
    quota_state: Arc<Mutex<QuotaState>>,
    store: RuntimeStore,
    paths: QuotaPaths,
) {
    let state_on_failure = quota_state.clone();
    let result = thread::Builder::new()
        .name("flow-agent-claude-quota".to_owned())
        .spawn(move || {
            let now = now_millis();
            let mut collector = QuotaCollector::new(paths);
            let result = collector.refresh_claude_oauth(now).map(|mut entries| {
                entries.extend(collector.collect_codex(now));
                entries
            });
            let Ok(mut quota) = quota_state.lock() else {
                return;
            };
            quota.oauth_refresh_in_progress = false;
            let Ok(entries) = result else { return };
            quota.entries = entries.clone();
            quota.refreshed_at = Some(Instant::now());
            quota.claude_cache_modified_at = fs::metadata(quota.collector.paths().claude_cache())
                .and_then(|metadata| metadata.modified())
                .ok();
            drop(quota);
            let persisted = entries.iter().filter_map(quota_record).collect::<Vec<_>>();
            let _ = store.replace_quota_snapshots(persisted);
        });
    if result.is_err() {
        if let Ok(mut quota) = state_on_failure.lock() {
            quota.oauth_refresh_in_progress = false;
        }
    }
}

fn quota_record(entry: &QuotaEntry) -> Option<QuotaRecord> {
    Some(QuotaRecord {
        provider: entry.provider.clone(),
        window: entry.window.clone(),
        used_pct: entry.used_pct?,
        resets_at: entry.resets_at?,
        source: entry.source.clone(),
        captured_at: entry.captured_at?,
    })
}

fn command_response(status: StatusCode, id: Uuid, state: CommandState) -> Response {
    (status, Json(json!({ "id": id, "state": state }))).into_response()
}

fn command_state_response(status: StatusCode, id: Uuid, state: &str) -> Response {
    (status, Json(json!({ "id": id, "state": state }))).into_response()
}

fn store_error_response(error: StoreError) -> Response {
    match error {
        StoreError::StaleApproval | StoreError::NotUndoable | StoreError::CommandNotFound => {
            api_error(StatusCode::CONFLICT, "STALE_APPROVAL")
        }
        StoreError::CommitTooEarly => api_error(StatusCode::CONFLICT, "COMMIT_TOO_EARLY"),
        StoreError::Storage(_) | StoreError::Provider(_) | StoreError::WriterStopped => {
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR")
        }
    }
}

fn api_error(status: StatusCode, code: &'static str) -> Response {
    (status, Json(json!({ "error": { "code": code } }))).into_response()
}

fn api_error_detail(status: StatusCode, code: &'static str, detail: &str) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "detail": detail } })),
    )
        .into_response()
}

fn valid_host(state: &AppState, headers: &HeaderMap) -> bool {
    headers.get(HOST).and_then(|value| value.to_str().ok()) == Some(state.expected_host.as_str())
}

fn valid_same_origin(state: &AppState, headers: &HeaderMap) -> bool {
    valid_host(state, headers)
        && headers.get(ORIGIN).and_then(|value| value.to_str().ok())
            == Some(state.expected_origin.as_str())
}

fn authorized(state: &AppState, headers: &HeaderMap) -> bool {
    if !valid_host(state, headers) {
        return false;
    }
    let Some(cookie) = headers
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|header| cookie_value(header, SESSION_COOKIE))
    else {
        return false;
    };
    let Ok(auth) = state.auth.lock() else {
        return false;
    };
    auth.session_token.as_deref() == Some(cookie)
}

fn authorized_mutation(state: &AppState, headers: &HeaderMap) -> bool {
    authorized(state, headers)
        && valid_same_origin(state, headers)
        && headers
            .get(CSRF_HEADER)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| valid_csrf_value(state, value))
}

fn valid_csrf_value(state: &AppState, value: &str) -> bool {
    let Ok(auth) = state.auth.lock() else {
        return false;
    };
    auth.csrf_token.as_deref() == Some(value)
}

fn cookie_value<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        (key == name).then_some(value)
    })
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use serde_json::Value;
    use std::fs;
    use tower::ServiceExt;

    fn authorized_request(method: &str, uri: &str, body: Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(HOST, "127.0.0.1:43111")
            .header(ORIGIN, "http://127.0.0.1:43111")
            .header(COOKIE, "flow_agent_session=test-session")
            .header(CSRF_HEADER, "test-csrf")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    async fn json_body(response: Response) -> Value {
        let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn test_state(store: RuntimeStore, root: &FilePath) -> AppState {
        let paths = InstallPaths {
            flow_home: root.join("flow-home"),
            claude_settings: root.join("home/.claude/settings.json"),
            codex_hooks: root.join("codex/hooks.json"),
            codex_config: root.join("codex/config.toml"),
        };
        let quota_paths = QuotaPaths {
            flow_home: paths.flow_home.clone(),
            codex_sessions: root.join("codex/sessions"),
        };
        let usage_paths = UsagePaths {
            flow_home: paths.flow_home.clone(),
            claude_projects: vec![root.join("home/.claude/projects")],
            codex_sessions: vec![
                root.join("codex/sessions"),
                root.join("codex/archived_sessions"),
            ],
        };
        let codex = CodexManager::disabled(store.clone());
        AppState {
            store,
            waiters: WaiterRegistry::default(),
            auth: Arc::new(Mutex::new(AuthState {
                bootstrap_token: Some("one-time-token".to_owned()),
                session_token: None,
                csrf_token: None,
            })),
            expected_host: "127.0.0.1:43111".to_owned(),
            expected_origin: "http://127.0.0.1:43111".to_owned(),
            commit_delay: Duration::from_secs(3),
            snapshot_interval: Duration::from_millis(250),
            quota_poll_interval: Duration::from_secs(300),
            claude_oauth_quota: false,
            installer: Arc::new(Installer::new(paths, std::env::current_exe().unwrap())),
            quota: Arc::new(Mutex::new(QuotaState {
                collector: QuotaCollector::new(quota_paths),
                entries: Vec::new(),
                refreshed_at: None,
                claude_cache_modified_at: None,
                oauth_refresh_in_progress: false,
            })),
            usage: Arc::new(Mutex::new(UsageState {
                collector: UsageCollector::new(usage_paths),
                refreshed_at: None,
            })),
            data_paths: DataPaths {
                cache: root.join("flow-home/cache"),
                spool: root.join("flow-home/spool"),
                diagnostics: root.join("flow-home/diagnostics"),
            },
            codex,
            runtime_restart: None,
        }
    }

    #[test]
    fn jump_capabilities_map_only_to_targets_the_runtime_can_really_open() {
        let root = std::env::temp_dir().join(format!(
            "flow-agent-server-jump-targets-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let codex_id = Uuid::now_v7().to_string();
        let cases = [
            (
                flow_agent_core::Provider::Codex,
                codex_id.as_str(),
                flow_agent_core::TermContext {
                    app: None,
                    session_id: None,
                    tty: None,
                    title: None,
                    bundle_id: Some("com.openai.codex".to_owned()),
                    surface: Some("codex_app".to_owned()),
                    provider_pid: None,
                },
            ),
            (
                flow_agent_core::Provider::Claude,
                "iterm-session",
                flow_agent_core::TermContext {
                    app: Some("iTerm.app".to_owned()),
                    session_id: Some("w0t0p0:ABC-123".to_owned()),
                    tty: None,
                    title: None,
                    bundle_id: Some("com.googlecode.iterm2".to_owned()),
                    surface: Some("terminal".to_owned()),
                    provider_pid: None,
                },
            ),
            (
                flow_agent_core::Provider::Claude,
                "claude-app-session",
                flow_agent_core::TermContext {
                    app: None,
                    session_id: None,
                    tty: None,
                    title: None,
                    bundle_id: Some("com.anthropic.claudefordesktop".to_owned()),
                    surface: Some("claude_app".to_owned()),
                    provider_pid: None,
                },
            ),
        ];
        for (provider, session_id, term) in cases {
            let mut request = flow_agent_core::BridgeRequest::from_hook_at(
                provider,
                json!({
                    "hook_event_name":"UserPromptSubmit",
                    "session_id":session_id,
                    "prompt":"jump test"
                }),
                now_millis(),
            );
            request.term = Some(term);
            store.ingest(request).unwrap();
        }

        let snapshot = store.snapshot().unwrap();
        let exact = snapshot
            .sessions
            .iter()
            .find(|session| session.provider_session_id == codex_id)
            .unwrap();
        assert_eq!(exact.jump_label, "精确打开对话");
        assert!(matches!(jump_target(exact), JumpTarget::CodexThread(_)));
        let terminal = snapshot
            .sessions
            .iter()
            .find(|session| session.provider_session_id == "iterm-session")
            .unwrap();
        assert_eq!(terminal.jump_label, "打开对应终端");
        assert_eq!(jump_target(terminal), JumpTarget::ITermSession);
        let app = snapshot
            .sessions
            .iter()
            .find(|session| session.provider_session_id == "claude-app-session")
            .unwrap();
        assert_eq!(app.jump_label, "只能打开应用");
        assert_eq!(
            jump_target(app),
            JumpTarget::AppBundle("com.anthropic.claudefordesktop")
        );
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn auth_contract_rejects_missing_cookie_forged_origin_and_missing_csrf() {
        let root = std::env::temp_dir().join(format!(
            "flow-agent-server-unit-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let app = router(test_state(store, &root));
            let unauthorized = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/snapshot")
                        .header(HOST, "127.0.0.1:43111")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

            let forged = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/bootstrap")
                        .header(HOST, "127.0.0.1:43111")
                        .header(ORIGIN, "http://malicious.invalid")
                        .header(CONTENT_TYPE, "application/json")
                        .body(Body::from(r#"{"token":"one-time-token"}"#))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(forged.status(), StatusCode::FORBIDDEN);

            let bootstrap = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/bootstrap")
                        .header(HOST, "127.0.0.1:43111")
                        .header(ORIGIN, "http://127.0.0.1:43111")
                        .header(CONTENT_TYPE, "application/json")
                        .body(Body::from(r#"{"token":"one-time-token"}"#))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(bootstrap.status(), StatusCode::OK);
            let cookie = bootstrap
                .headers()
                .get(SET_COOKIE)
                .unwrap()
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .to_owned();
            let bytes = to_bytes(bootstrap.into_body(), 4096).await.unwrap();
            let payload: Value = serde_json::from_slice(&bytes).unwrap();
            assert!(payload["csrfToken"].as_str().is_some());

            let setup = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/setup")
                        .header(HOST, "127.0.0.1:43111")
                        .header(COOKIE, cookie.clone())
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(setup.status(), StatusCode::OK);
            let setup_bytes = to_bytes(setup.into_body(), 16 * 1024).await.unwrap();
            let setup_payload: Value = serde_json::from_slice(&setup_bytes).unwrap();
            assert_eq!(setup_payload["schemaVersion"], 1);
            assert_eq!(setup_payload["providers"].as_array().unwrap().len(), 2);

            let setup_without_csrf = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/setup")
                        .header(HOST, "127.0.0.1:43111")
                        .header(ORIGIN, "http://127.0.0.1:43111")
                        .header(COOKIE, cookie.clone())
                        .header(CONTENT_TYPE, "application/json")
                        .body(Body::from(
                            r#"{"provider":"claude","action":"install"}"#,
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(setup_without_csrf.status(), StatusCode::FORBIDDEN);

            let missing_csrf = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/commands")
                        .header(HOST, "127.0.0.1:43111")
                        .header(ORIGIN, "http://127.0.0.1:43111")
                        .header(COOKIE, cookie)
                        .header(CONTENT_TYPE, "application/json")
                        .body(Body::from(
                            r#"{"id":"00000000-0000-0000-0000-000000000000","attentionId":"missing","requestId":null,"action":"ack"}"#,
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(missing_csrf.status(), StatusCode::FORBIDDEN);
        });
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn agent_list_keeps_recent_and_attention_sessions_but_hides_older_idle_history() {
        let root = std::env::temp_dir().join(format!(
            "flow-agent-server-visible-sessions-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let state = test_state(store.clone(), &root);
        let now = now_millis();
        for (session, event, at) in [
            (
                "old-idle",
                json!({"hook_event_name":"SessionStart","session_id":"old-idle"}),
                now.saturating_sub(31 * 60 * 1_000),
            ),
            (
                "recent-idle",
                json!({"hook_event_name":"SessionStart","session_id":"recent-idle"}),
                now.saturating_sub(29 * 60 * 1_000),
            ),
            (
                "old-attention",
                json!({
                    "hook_event_name":"PermissionRequest",
                    "session_id":"old-attention",
                    "tool_name":"Bash",
                    "tool_input":{"command":"cargo test"}
                }),
                now.saturating_sub(31 * 60 * 1_000),
            ),
        ] {
            let request = flow_agent_core::BridgeRequest::from_hook_at(
                flow_agent_core::Provider::Claude,
                event,
                at,
            );
            store
                .ingest(request)
                .unwrap_or_else(|error| panic!("failed to ingest {session}: {error}"));
        }

        let value = snapshot_value(&state).unwrap();
        let sessions = value["sessions"].as_array().unwrap();
        let provider_ids = sessions
            .iter()
            .filter_map(|session| session["providerSessionId"].as_str())
            .collect::<HashSet<_>>();
        assert!(!provider_ids.contains("old-idle"));
        assert!(provider_ids.contains("recent-idle"));
        assert!(provider_ids.contains("old-attention"));
        drop(state);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn new_claude_cache_refreshes_immediately_after_an_unavailable_snapshot() {
        let root = std::env::temp_dir().join(format!(
            "flow-agent-server-quota-cache-refresh-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let state = test_state(store.clone(), &root);

        let before = snapshot_value(&state).unwrap();
        assert!(before["quota"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["provider"] == "claude")
            .all(|entry| entry["status"] == "unavailable"));

        let cache = state.quota.lock().unwrap().collector.paths().claude_cache();
        flow_agent_quota::capture_claude_statusline(
            br#"{"rate_limits":{"five_hour":{"used_percentage":2,"resets_at":1784193000},"seven_day":{"used_percentage":0,"resets_at":1784563200}}}"#,
            &cache,
            now_millis(),
        )
        .unwrap();

        let after = snapshot_value(&state).unwrap();
        let claude = after["quota"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["provider"] == "claude")
            .collect::<Vec<_>>();
        assert_eq!(claude.len(), 2);
        assert!(claude.iter().all(|entry| entry["status"] == "available"));
        assert_eq!(claude[0]["usedPct"], 2.0);
        assert_eq!(claude[1]["usedPct"], 0.0);

        drop(state);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn m4_settings_quota_export_and_clear_follow_the_authenticated_ui_path() {
        let root = std::env::temp_dir().join(format!(
            "flow-agent-server-m4-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let state = test_state(store.clone(), &root);
        {
            let mut auth = state.auth.lock().unwrap();
            auth.bootstrap_token = None;
            auth.session_token = Some("test-session".to_owned());
            auth.csrf_token = Some("test-csrf".to_owned());
        }
        let claude_settings = state.installer.paths().claude_settings.clone();
        fs::create_dir_all(claude_settings.parent().unwrap()).unwrap();
        fs::write(
            &claude_settings,
            br#"{"statusLine":{"type":"command","command":"~/.claude/custom.sh"},"keep":true}"#,
        )
        .unwrap();
        let cache = state.quota.lock().unwrap().collector.paths().claude_cache();
        flow_agent_quota::capture_claude_statusline(
            br#"{"rate_limits":{"five_hour":{"used_percentage":25,"resets_at":1784140000}}}"#,
            &cache,
            now_millis(),
        )
        .unwrap();
        fs::create_dir_all(&state.data_paths.spool).unwrap();
        fs::write(state.data_paths.spool.join("offline.json"), b"sanitized").unwrap();
        fs::create_dir_all(&state.data_paths.diagnostics).unwrap();
        fs::write(
            state.data_paths.diagnostics.join("events.jsonl"),
            b"sanitized diagnostics",
        )
        .unwrap();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let app = router(state.clone());
            let settings_update = json!({
                "notificationRules": {
                    "approval":"list", "question":"banner", "error":"ignore", "completion":"banner"
                },
                "soundEnabled": false,
                "providerMuted": {"claude": true, "codex": false},
                "codexEnhancedActivity": true,
                "retentionDays": 30
            });
            let updated = app
                .clone()
                .oneshot(authorized_request(
                    "PUT",
                    "/api/v1/settings",
                    settings_update,
                ))
                .await
                .unwrap();
            assert_eq!(updated.status(), StatusCode::OK);
            let updated = json_body(updated).await;
            assert_eq!(updated["settings"]["retentionDays"], 30);
            assert_eq!(updated["settings"]["notificationRules"]["approval"], "list");
            assert_eq!(updated["claudeQuotaBridge"]["status"], "custom_conflict");

            let snapshot = app
                .clone()
                .oneshot(authorized_request("GET", "/api/v1/snapshot", Value::Null))
                .await
                .unwrap();
            assert_eq!(snapshot.status(), StatusCode::OK);
            let snapshot = json_body(snapshot).await;
            let claude = snapshot["quota"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["provider"] == "claude")
                .unwrap();
            assert_eq!(claude["status"], "available");
            assert_eq!(claude["remainingPct"], 75.0);

            let bridge = app
                .clone()
                .oneshot(authorized_request(
                    "POST",
                    "/api/v1/quota/claude-bridge",
                    json!({"action":"install"}),
                ))
                .await
                .unwrap();
            assert_eq!(bridge.status(), StatusCode::CONFLICT);
            assert_eq!(
                serde_json::from_slice::<Value>(&fs::read(&claude_settings).unwrap()).unwrap()
                    ["keep"],
                true
            );

            let wrapped = app
                .clone()
                .oneshot(authorized_request(
                    "POST",
                    "/api/v1/quota/claude-bridge",
                    json!({"action":"wrap"}),
                ))
                .await
                .unwrap();
            assert_eq!(wrapped.status(), StatusCode::OK);
            let wrapped = json_body(wrapped).await;
            assert_eq!(wrapped["claudeQuotaBridge"]["status"], "installed");
            let wrapped_settings =
                serde_json::from_slice::<Value>(&fs::read(&claude_settings).unwrap()).unwrap();
            assert_eq!(wrapped_settings["keep"], true);
            assert_eq!(
                wrapped_settings["_flowAgentOriginalStatusLine"]["command"],
                "~/.claude/custom.sh"
            );

            let exported = app
                .clone()
                .oneshot(authorized_request("GET", "/api/v1/export", Value::Null))
                .await
                .unwrap();
            assert_eq!(exported.status(), StatusCode::OK);
            assert_eq!(
                exported.headers()[CONTENT_DISPOSITION],
                "attachment; filename=flow-agent-export.json"
            );
            let exported = json_body(exported).await;
            assert_eq!(exported["tables"]["settings"].as_array().unwrap().len(), 1);

            let wrong_confirmation = app
                .clone()
                .oneshot(authorized_request(
                    "POST",
                    "/api/v1/data/clear",
                    json!({"confirmation":"delete"}),
                ))
                .await
                .unwrap();
            assert_eq!(wrong_confirmation.status(), StatusCode::BAD_REQUEST);
            assert!(cache.exists());

            let cleared = app
                .oneshot(authorized_request(
                    "POST",
                    "/api/v1/data/clear",
                    json!({"confirmation":"DELETE"}),
                ))
                .await
                .unwrap();
            assert_eq!(cleared.status(), StatusCode::OK);
        });
        assert!(store.snapshot().unwrap().sessions.is_empty());
        assert_eq!(store.read_setting(SETTINGS_KEY).unwrap(), None);
        assert!(!state.data_paths.cache.exists());
        assert!(!state.data_paths.spool.exists());
        assert!(!state.data_paths.diagnostics.exists());
        assert!(claude_settings.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corrupt_settings_block_reconfiguration_before_provider_files_are_touched() {
        let root = std::env::temp_dir().join(format!(
            "flow-agent-server-m4-corrupt-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let state = test_state(store.clone(), &root);
        {
            let mut auth = state.auth.lock().unwrap();
            auth.bootstrap_token = None;
            auth.session_token = Some("test-session".to_owned());
            auth.csrf_token = Some("test-csrf".to_owned());
        }
        let hooks = state.installer.paths().codex_hooks.clone();
        fs::create_dir_all(hooks.parent().unwrap()).unwrap();
        fs::write(&hooks, b"provider file must stay unchanged").unwrap();
        store
            .write_setting(SETTINGS_KEY, "{broken-json".to_owned())
            .unwrap();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let response = router(state)
                .oneshot(authorized_request(
                    "PUT",
                    "/api/v1/settings",
                    json!({
                        "notificationRules": {
                            "approval":"banner", "question":"banner",
                            "error":"banner", "completion":"banner"
                        },
                        "soundEnabled": true,
                        "providerMuted": {"claude": false, "codex": false},
                        "codexEnhancedActivity": true,
                        "retentionDays": 90
                    }),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        });
        assert_eq!(
            fs::read(&hooks).unwrap(),
            b"provider file must stay unchanged"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn m5_ui_metrics_are_authenticated_local_and_visible_in_snapshot_and_export() {
        let root = std::env::temp_dir().join(format!(
            "flow-agent-server-m5-metrics-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let state = test_state(store.clone(), &root);
        {
            let mut auth = state.auth.lock().unwrap();
            auth.bootstrap_token = None;
            auth.session_token = Some("test-session".to_owned());
            auth.csrf_token = Some("test-csrf".to_owned());
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let app = router(state);
            for event in ["app_opened", "banner_shown", "banner_shown"] {
                let response = app
                    .clone()
                    .oneshot(authorized_request(
                        "POST",
                        "/api/v1/metrics",
                        json!({"event":event}),
                    ))
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
            }
            let snapshot = app
                .clone()
                .oneshot(authorized_request("GET", "/api/v1/snapshot", Value::Null))
                .await
                .unwrap();
            let snapshot = json_body(snapshot).await;
            assert_eq!(snapshot["stats"]["metrics"]["appOpened"], 1);
            assert_eq!(snapshot["stats"]["metrics"]["bannersShown"], 2);

            let export = app
                .clone()
                .oneshot(authorized_request("GET", "/api/v1/export", Value::Null))
                .await
                .unwrap();
            let export = json_body(export).await;
            assert_eq!(export["tables"]["metrics_daily"][0]["app_opened"], 1);

            let metrics_export = app
                .oneshot(authorized_request(
                    "GET",
                    "/api/v1/metrics/export",
                    Value::Null,
                ))
                .await
                .unwrap();
            assert_eq!(metrics_export.status(), StatusCode::OK);
            assert_eq!(
                metrics_export.headers()[CONTENT_DISPOSITION],
                "attachment; filename=flow-agent-metrics.json"
            );
            let metrics_export = json_body(metrics_export).await;
            assert_eq!(metrics_export["scope"], "metrics_only");
            assert!(metrics_export.get("tables").is_none());
        });
        assert!(INDEX_HTML.contains("我的使用统计"));
        assert!(INDEX_HTML.contains("导出统计"));
        assert!(APP_JS.contains("widgetApprovals"));
        assert!(APP_JS.contains("banner_shown"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn m5_api_rejects_bodies_over_64_kib_before_deserialization() {
        let root = std::env::temp_dir().join(format!(
            "flow-agent-server-m5-body-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let state = test_state(store, &root);
        {
            let mut auth = state.auth.lock().unwrap();
            auth.bootstrap_token = None;
            auth.session_token = Some("test-session".to_owned());
            auth.csrf_token = Some("test-csrf".to_owned());
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let response = router(state)
                .oneshot(authorized_request(
                    "PUT",
                    "/api/v1/settings",
                    json!({"padding":"x".repeat(65 * 1024)}),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        });
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn m10_display_settings_migrate_and_reject_fields_outside_the_safe_catalog() {
        let legacy = serde_json::from_value::<UiSettings>(json!({
            "notificationRules": {
                "approval": "banner",
                "question": "list",
                "error": "banner",
                "completion": "list"
            },
            "soundEnabled": false,
            "providerMuted": { "claude": false, "codex": true },
            "codexEnhancedActivity": true,
            "retentionDays": 90
        }))
        .unwrap();
        assert_eq!(legacy.display_profile, "detailed");
        assert!(legacy.task_card_fields.contains(&"activity".to_owned()));
        legacy.validate().unwrap();

        for unsafe_field in ["raw", "payload", "fullCommand", "transcript"] {
            let unsafe_settings = UiSettings {
                task_card_fields: vec![unsafe_field.to_owned()],
                ..UiSettings::default()
            };
            assert_eq!(
                unsafe_settings.validate(),
                Err("taskCardFields contains an unsupported field")
            );
        }

        let encoded = serde_json::to_string(&UiSettings::default()).unwrap();
        assert!(!encoded.contains("raw"));
        assert!(!encoded.contains("payload"));
        assert!(!encoded.contains("command"));
    }

    #[test]
    fn interactive_question_is_ephemeral_answerable_and_removed_from_snapshot() {
        let root = std::env::temp_dir().join(format!(
            "flow-agent-server-question-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let state = test_state(store.clone(), &root);
        {
            let mut auth = state.auth.lock().unwrap();
            auth.bootstrap_token = None;
            auth.session_token = Some("test-session".to_owned());
            auth.csrf_token = Some("test-csrf".to_owned());
        }
        let request = flow_agent_core::BridgeRequest::from_hook_at(
            flow_agent_core::Provider::Claude,
            json!({
                "hook_event_name":"Elicitation",
                "session_id":"question-session",
                "message":"enter private key",
                "requested_schema":{
                    "type":"object",
                    "required":["key"],
                    "properties":{"key":{"type":"string","format":"password"}}
                }
            }),
            now_millis(),
        );
        let registration = state.waiters.register_at(&request, now_millis()).unwrap();
        let request_id = request.request_id.unwrap();
        store.ingest(request).unwrap();
        let before = snapshot_value(&state).unwrap();
        let interactive = before["attention"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|item| item.get("interaction"))
            .unwrap();
        assert_eq!(interactive["questions"][0]["isSecret"], true);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let response = router(state.clone())
                .oneshot(authorized_request(
                    "POST",
                    &format!("/api/v1/questions/{request_id}/answer"),
                    json!({"action":"accept","answers":{"key":"secret-48291"}}),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        });
        let response = registration
            .ticket
            .recv_timeout(Duration::from_millis(50))
            .unwrap();
        assert_eq!(response.action, flow_agent_core::ReplyAction::Answer);
        let after = snapshot_value(&state).unwrap();
        assert!(after["attention"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item.get("interaction").is_none()));
        let exported = serde_json::to_string(&store.export_json(now_millis()).unwrap()).unwrap();
        assert!(!exported.contains("secret-48291"));
        drop(state);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_snapshot_distinguishes_observing_lost_and_managed_sessions() {
        let root = std::env::temp_dir().join(format!(
            "flow-agent-server-recovery-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        for (session, pid) in [
            ("live-external", std::process::id()),
            ("dead-external", u32::MAX - 1),
        ] {
            let mut request = flow_agent_core::BridgeRequest::from_hook_at(
                flow_agent_core::Provider::Claude,
                json!({
                    "hook_event_name":"UserPromptSubmit",
                    "session_id":session,
                    "prompt":"recovery test"
                }),
                now_millis(),
            );
            request.term.as_mut().unwrap().provider_pid = Some(pid);
            store.ingest(request).unwrap();
        }
        let mut managed = flow_agent_core::BridgeRequest::from_hook_at(
            flow_agent_core::Provider::Codex,
            json!({
                "hook_event_name":"UserPromptSubmit",
                "session_id":"managed-thread",
                "turn_id":"turn-1",
                "prompt":"managed recovery"
            }),
            now_millis(),
        );
        managed.term = None;
        store.ingest(managed).unwrap();
        let mut state = test_state(store.clone(), &root);
        state.codex.state = Arc::new(Mutex::new(CodexManagerState {
            status: "connected".to_owned(),
            managed: HashSet::from(["managed-thread".to_owned()]),
            threads: HashMap::from([(
                "managed-thread".to_owned(),
                CodexThread {
                    id: "managed-thread".to_owned(),
                    name: Some("Managed".to_owned()),
                    cwd: None,
                    status: "active".to_owned(),
                    active_flags: vec![],
                    updated_at: None,
                },
            )]),
            error: None,
            resume_failed: HashSet::new(),
            ..CodexManagerState::default()
        }));
        let value = snapshot_value(&state).unwrap();
        let sessions = value["sessions"].as_array().unwrap();
        let recovery = |id: &str| {
            sessions
                .iter()
                .find(|session| session["providerSessionId"] == id)
                .unwrap()["recoveryState"]
                .as_str()
                .unwrap()
        };
        assert_eq!(recovery("live-external"), "observing");
        assert_eq!(recovery("dead-external"), "lost_control");
        assert_eq!(recovery("managed-thread"), "controllable");
        let managed_session = sessions
            .iter()
            .find(|session| session["providerSessionId"] == "managed-thread")
            .unwrap();
        assert_eq!(managed_session["controlCapability"], "managed");
        drop(state);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_native_waiting_flag_opens_and_resolves_attention() {
        let root = std::env::temp_dir().join(format!(
            "flow-agent-server-native-approval-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        store
            .ingest(flow_agent_core::BridgeRequest::from_hook_at(
                flow_agent_core::Provider::Codex,
                json!({
                    "hook_event_name":"UserPromptSubmit",
                    "session_id":"thread-native",
                    "turn_id":"turn-1",
                    "prompt":"在桌面建立空白文件夹"
                }),
                1_000,
            ))
            .unwrap();
        let state = Arc::new(Mutex::new(CodexManagerState::default()));
        let waiters = WaiterRegistry::default();

        update_codex_notification(
            &state,
            &store,
            &waiters,
            ServerNotification {
                method: "thread/started".to_owned(),
                params: json!({
                    "thread": {
                        "id":"thread-native",
                        "status": {
                            "type":"active",
                            "activeFlags":["waitingOnApproval"]
                        }
                    }
                }),
            },
        );
        let waiting = store.snapshot().unwrap();
        assert_eq!(waiting.attention.len(), 1);
        assert_eq!(waiting.attention[0].kind, "native_approval");
        assert_eq!(waiting.attention[0].state, "open");
        assert_eq!(waiting.sessions[0].exec_state, "awaiting_approval");
        assert_eq!(
            waiting.sessions[0].approval_owner.as_deref(),
            Some("terminal")
        );
        assert!(state
            .lock()
            .unwrap()
            .native_synced
            .contains("thread-native"));

        update_codex_notification(
            &state,
            &store,
            &waiters,
            ServerNotification {
                method: "thread/status/changed".to_owned(),
                params: json!({
                    "threadId":"thread-native",
                    "status": {"type":"active", "activeFlags":[]}
                }),
            },
        );
        let resumed = store.snapshot().unwrap();
        assert_eq!(resumed.attention[0].state, "resolved");
        assert_eq!(
            resumed.attention[0].resolution.as_deref(),
            Some("provider_handled")
        );
        assert_eq!(resumed.sessions[0].exec_state, "thinking");
        assert_eq!(resumed.sessions[0].approval_owner, None);
        assert!(!state
            .lock()
            .unwrap()
            .native_synced
            .contains("thread-native"));

        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_native_resolution_releases_a_competing_hook_waiter() {
        let root = std::env::temp_dir().join(format!(
            "flow-agent-server-hook-release-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let request = flow_agent_core::BridgeRequest::from_hook_at(
            flow_agent_core::Provider::Codex,
            json!({
                "hook_event_name":"PermissionRequest",
                "session_id":"thread-hook-release",
                "turn_id":"turn-1",
                "tool_name":"Bash",
                "tool_input":{"command":"mkdir Desktop/example"}
            }),
            now_millis(),
        );
        let request_id = request.request_id.unwrap();
        let waiters = WaiterRegistry::default();
        let registration = waiters.register_at(&request, now_millis()).unwrap();
        store.ingest(request).unwrap();
        let state = Arc::new(Mutex::new(CodexManagerState::default()));

        update_codex_notification(
            &state,
            &store,
            &waiters,
            ServerNotification {
                method: "thread/started".to_owned(),
                params: json!({
                    "thread": {
                        "id":"thread-hook-release",
                        "status": {
                            "type":"active",
                            "activeFlags":["waitingOnApproval"]
                        }
                    }
                }),
            },
        );
        let waiting = store.snapshot().unwrap();
        assert_eq!(waiting.attention.len(), 1);
        assert_eq!(waiting.attention[0].kind, "approval");
        assert!(waiters.is_active(request_id).unwrap());

        update_codex_notification(
            &state,
            &store,
            &waiters,
            ServerNotification {
                method: "thread/status/changed".to_owned(),
                params: json!({
                    "threadId":"thread-hook-release",
                    "status":{"type":"active", "activeFlags":[]}
                }),
            },
        );
        let response = registration
            .ticket
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        assert_eq!(response.action, ReplyAction::PassThrough);
        assert_eq!(response.reason.as_deref(), Some("provider_handled"));
        assert!(!waiters.is_active(request_id).unwrap());
        let resolved = store.snapshot().unwrap();
        assert_eq!(resolved.attention[0].state, "resolved");
        assert_eq!(
            resolved.attention[0].resolution.as_deref(),
            Some("provider_handled")
        );
        assert_eq!(resolved.sessions[0].exec_state, "thinking");
        assert_eq!(resolved.sessions[0].approval_owner, None);

        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}

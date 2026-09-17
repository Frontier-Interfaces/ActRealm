use actrealm_codex_connector::{
    parse_thread, CodexConnector, CodexThread, ConnectorChannels, ServerNotification, ServerRequest,
};
use actrealm_core::{
    provider_capability_matrix, BridgeRequest, Provider, ProviderCapabilityFeature,
    ProviderCapabilityStatus, ReplyAction, ReplyPayload,
};
use actrealm_installer::{
    discover_provider_availability, BinaryHealth, ClaudeStatuslineStatus, CodexTrustStatus,
    ConfigHealth, HookProvider, InstallIntent, InstallOptions, InstallPaths, Installer,
};
use actrealm_quota::{
    codex_app_server_entries, QuotaCollector, QuotaEntry, QuotaError, QuotaPaths,
    CODEX_APP_SERVER_SOURCE,
};
use actrealm_runtime::{
    ApprovalAction, AttentionAction, CommandState, MetricEvent, QuotaRecord,
    ReviewBaselineCandidate, ReviewBaselineInput, ReviewBaselineRecord, RuntimeStore,
    SessionRecord, SessionUsageRecord, StoreError, TaskCheckpointInput, TaskCheckpointRecord,
    TaskHistoryMutation, TimelineEventKind, WaiterError, WaiterRegistry,
};
use actrealm_usage::{PricingStatus, UsageCollector, UsagePaths, UsageRecord};
use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::header::{
    AUTHORIZATION, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE, COOKIE, HOST, ORIGIN,
    SEC_WEBSOCKET_PROTOCOL, SET_COOKIE,
};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Component, Path as FilePath, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
#[cfg(test)]
use std::sync::Barrier;
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::codex_questions::{
    self, Observation, QuestionBatch, QuestionRegistry, QuestionScanner, ReplyRoute,
};
use crate::status_messages;
#[cfg(test)]
#[path = "native_client_tests.rs"]
mod native_client_tests;
#[path = "native_clients.rs"]
mod native_clients;

const SESSION_COOKIE: &str = "actrealm_session";
const CSRF_HEADER: &str = "x-actrealm-csrf";
const WS_PROTOCOL_PREFIX: &str = "actrealm.";
const WS_TICKET_TTL: Duration = Duration::from_secs(10);
const MAX_WS_TICKETS: usize = 16;
const COMPANION_PAIRING_TTL_MS: u64 = 5 * 60 * 1_000;
const COMPANION_SCHEMA_VERSION: u32 = 1;
const COMPANION_SCOPE_SNAPSHOT: &str = "snapshot.read";
const COMPANION_SCOPE_JUMP: &str = "session.jump";
const COMPANION_SCOPE_RESPOND: &str = "attention.respond";
const PUBLIC_PROTOCOL_VERSION: u32 = 7;
const RUNTIME_GIT_COMMIT: &str = match option_env!("ACTREALM_GIT_COMMIT") {
    Some(commit) => commit,
    None => "unknown",
};

fn runtime_git_commit() -> &'static str {
    if RUNTIME_GIT_COMMIT.len() == 40
        && RUNTIME_GIT_COMMIT
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        RUNTIME_GIT_COMMIT
    } else {
        "unknown"
    }
}
const SETTINGS_KEY: &str = "ui_settings";
const MANAGED_CODEX_THREADS_KEY: &str = "managed_codex_threads";
const MAX_MANAGED_CODEX_THREADS: usize = 32;
const TASK_CARD_DISPLAY_FIELDS: &[&str] = &[
    "project",
    "task",
    "model",
    "activity",
    "plan",
    "sessionTokens",
    "turnTokens",
    "inputOutputTokens",
    "cacheTokens",
    "reasoningTokens",
    "cost",
    "context",
    "tool",
    "currentTarget",
    "permissionMode",
    "subagents",
    "environment",
    "recovery",
    "control",
    "jump",
    "taskFlow",
    "workflow",
    "titleSource",
    "sessionId",
    "providerSessionId",
    "providerTurnId",
    "lastEventAt",
];
const SESSION_LIST_RETENTION_MS: u64 = 30 * 60 * 1_000;
const USAGE_COLLECTION_MIN_INTERVAL: Duration = Duration::from_secs(1);
const USAGE_BACKFILL_POLL_INTERVAL: Duration = Duration::from_secs(2);
const USAGE_LIVE_POLL_INTERVAL: Duration = Duration::from_secs(5);
const CODEX_CREDENTIAL_REFRESH_FIRST_USAGE_GRACE_MS: u64 = 30 * 60 * 1_000;
const UI_STATUS_TIMESTAMP_GRANULARITY_MS: u64 = 30_000;
const USAGE_FAILURE_BACKOFF: Duration = Duration::from_millis(100);
const RESTART_REQUEST_TTL: Duration = Duration::from_secs(3);
const RESTART_REQUEST_PENDING: u8 = 0;
const RESTART_REQUEST_ACCEPTED: u8 = 1;
const RESTART_REQUEST_CANCELLED: u8 = 2;
const INDEX_HTML: &str = include_str!("../../../web/index.html");
const APP_CSS: &str = include_str!("../../../web/app.css");
const I18N_JS: &str = include_str!("../../../web/i18n.js");
const AGENT_STATE_JS: &str = include_str!("../../../web/agent-state.js");
const AGENT_DETAIL_JS: &str = include_str!("../../../web/agent-detail.js");
const APP_JS: &str = include_str!("../../../web/app.js");
const CLAUDE_ICON: &[u8] = include_bytes!("../../../web/assets/claude.png");
const CODEX_ICON: &[u8] = include_bytes!("../../../web/assets/codex.png");
const FACT_METADATA_SCHEMA_VERSION: u16 = 1;
const FACT_LIVE_MAX_AGE_MS: u64 = 30_000;
const FACT_DELAYED_MAX_AGE_MS: u64 = 2 * 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum FactSourceKind {
    Authoritative,
    Observed,
    Derived,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum FactFreshness {
    Live,
    Delayed,
    Stale,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum FactVerification {
    Verified,
    Partial,
    Unverified,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum FactAbsenceReason {
    ProviderNotSupplied,
    NotSupported,
    CapabilityUnconfirmed,
    NoCurrentTurn,
    NoCurrentActivity,
    NoCurrentTool,
    CurrentToolHasNoTarget,
    TaskNotCompleted,
    SourceStale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum FactCapability {
    Direct,
    ReturnToProvider,
    ObserveOnly,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct FactMetadata {
    schema_version: u16,
    source_kind: FactSourceKind,
    source_id: Option<String>,
    captured_at: Option<u64>,
    freshness: FactFreshness,
    verification: FactVerification,
    absence_reason: Option<FactAbsenceReason>,
    capability: FactCapability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionFacts {
    schema_version: u16,
    plan: FactMetadata,
    activity: FactMetadata,
    current_target: FactMetadata,
    completion: FactMetadata,
    control: FactMetadata,
}

#[derive(Debug, Clone)]
pub struct ApiServerConfig {
    pub enable_native_clients: bool,
    pub bind: SocketAddr,
    pub bootstrap_token: Option<String>,
    pub initial_session_token: Option<String>,
    pub initial_csrf_token: Option<String>,
    pub runtime_started_at: u64,
    pub restart_count: u32,
    pub commit_delay: Duration,
    pub snapshot_interval: Duration,
    pub heartbeat_interval: Duration,
    pub quota_poll_interval: Duration,
    pub enable_claude_oauth_quota: bool,
    pub enable_live_usage_pricing: bool,
    pub install_paths: Option<InstallPaths>,
    pub enable_codex_connector: bool,
    pub runtime_restart: Option<RuntimeRestartHandle>,
}

impl Default for ApiServerConfig {
    fn default() -> Self {
        Self {
            enable_native_clients: false,
            bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            bootstrap_token: None,
            initial_session_token: None,
            initial_csrf_token: None,
            runtime_started_at: now_millis(),
            restart_count: 0,
            commit_delay: Duration::from_secs(3),
            // A 250 ms cadence keeps local Attention comfortably below the
            // one-second product target without rebuilding the full snapshot
            // ten times per second while the user is only observing.
            snapshot_interval: Duration::from_millis(250),
            heartbeat_interval: Duration::from_secs(10),
            quota_poll_interval: Duration::from_secs(60),
            enable_claude_oauth_quota: false,
            enable_live_usage_pricing: false,
            install_paths: None,
            enable_codex_connector: false,
            runtime_restart: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeRestartHandle {
    sender: std_mpsc::Sender<RuntimeRestartRequest>,
    socket_path: PathBuf,
}

#[derive(Debug)]
pub struct RuntimeRestartRequest {
    pub bootstrap_token: String,
    pub api_bind: SocketAddr,
    pub session_token: Option<String>,
    pub csrf_token: Option<String>,
    coordination: Arc<AtomicUsize>,
    expires_at: Instant,
    response_sender: std_mpsc::Sender<Result<(), String>>,
}

impl RuntimeRestartRequest {
    pub fn try_accept(&self) -> bool {
        if Instant::now() >= self.expires_at {
            let _ = self.coordination.compare_exchange(
                RESTART_REQUEST_PENDING.into(),
                RESTART_REQUEST_CANCELLED.into(),
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            return false;
        }
        self.coordination
            .compare_exchange(
                RESTART_REQUEST_PENDING.into(),
                RESTART_REQUEST_ACCEPTED.into(),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    pub fn respond(self, result: Result<(), String>) {
        let _ = self.response_sender.send(result);
    }
}

struct RuntimeRestartWaiter {
    receiver: std_mpsc::Receiver<Result<(), String>>,
    coordination: Arc<AtomicUsize>,
}

impl RuntimeRestartWaiter {
    fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<Result<(), String>, std_mpsc::RecvTimeoutError> {
        match self.receiver.recv_timeout(timeout) {
            Ok(result) => Ok(result),
            Err(std_mpsc::RecvTimeoutError::Timeout) => {
                if self
                    .coordination
                    .compare_exchange(
                        RESTART_REQUEST_PENDING.into(),
                        RESTART_REQUEST_CANCELLED.into(),
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    return Err(std_mpsc::RecvTimeoutError::Timeout);
                }
                if self.coordination.load(Ordering::Acquire)
                    == usize::from(RESTART_REQUEST_ACCEPTED)
                {
                    return self
                        .receiver
                        .recv()
                        .map_err(|_| std_mpsc::RecvTimeoutError::Disconnected);
                }
                Err(std_mpsc::RecvTimeoutError::Timeout)
            }
            Err(error) => Err(error),
        }
    }
}

impl RuntimeRestartHandle {
    pub fn new(sender: std_mpsc::Sender<RuntimeRestartRequest>, socket_path: PathBuf) -> Self {
        Self {
            sender,
            socket_path,
        }
    }

    fn request(
        &self,
        bootstrap_token: String,
        api_bind: SocketAddr,
        session_token: Option<String>,
        csrf_token: Option<String>,
    ) -> Result<RuntimeRestartWaiter, String> {
        let (response_sender, response_receiver) = std_mpsc::channel();
        let coordination = Arc::new(AtomicUsize::new(RESTART_REQUEST_PENDING.into()));
        self.sender
            .send(RuntimeRestartRequest {
                bootstrap_token,
                api_bind,
                session_token,
                csrf_token,
                coordination: coordination.clone(),
                expires_at: Instant::now() + RESTART_REQUEST_TTL,
                response_sender,
            })
            .map_err(|_| "Runtime control channel is unavailable".to_owned())?;
        if let Err(error) = UnixStream::connect(&self.socket_path) {
            if coordination
                .compare_exchange(
                    RESTART_REQUEST_PENDING.into(),
                    RESTART_REQUEST_CANCELLED.into(),
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                return Err(format!("Could not wake bridge.sock: {error}"));
            }
        }
        Ok(RuntimeRestartWaiter {
            receiver: response_receiver,
            coordination,
        })
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
    #[error("secure random generator unavailable")]
    SecureRandom,
}

pub struct ApiServer {
    native_listener: Option<crate::native_transport::NativeListener>,
    address: SocketAddr,
    bootstrap_token: String,
    shutdown: Option<oneshot::Sender<()>>,
    shutdown_flag: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    usage_thread: Option<thread::JoinHandle<()>>,
    review_thread: Option<thread::JoinHandle<()>>,
    usage_worker_failures: Arc<AtomicUsize>,
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
                "ActRealm API must bind to loopback",
            )));
        }
        let bootstrap_token = match config.bootstrap_token.clone() {
            Some(token) => token,
            None => generate_secret().map_err(|_| ApiServerError::SecureRandom)?,
        };
        let (initial_session_token, initial_csrf_token) = validated_preserved_auth(
            config.initial_session_token.clone(),
            config.initial_csrf_token.clone(),
        )
        .map_err(ApiServerError::Setup)?;
        let instance_id = Uuid::now_v7().to_string();
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let websocket_connections = Arc::new(AtomicUsize::new(0));
        let install_paths = match config.install_paths.clone() {
            Some(paths) => paths,
            None => InstallPaths::discover()
                .map_err(|error| ApiServerError::Setup(error.to_string()))?,
        };
        let source_binary =
            std::env::current_exe().map_err(|error| ApiServerError::Setup(error.to_string()))?;
        let quota_paths = QuotaPaths {
            actrealm_home: install_paths.actrealm_home.clone(),
            codex_sessions: install_paths
                .codex_config
                .parent()
                .unwrap_or_else(|| FilePath::new("."))
                .join("sessions"),
        };
        let mut usage_paths = UsagePaths::discover();
        usage_paths.actrealm_home = install_paths.actrealm_home.clone();
        let codex_home = install_paths
            .codex_config
            .parent()
            .unwrap_or_else(|| FilePath::new("."));
        usage_paths.codex_sessions = vec![
            codex_home.join("sessions"),
            codex_home.join("archived_sessions"),
        ];
        let data_paths = DataPaths {
            cache: install_paths.actrealm_home.join("cache"),
            spool: install_paths.actrealm_home.join("spool"),
            diagnostics: install_paths.actrealm_home.join("diagnostics"),
            companion_auth: install_paths.actrealm_home.join("companion-auth.json"),
            companion_discovery: install_paths
                .actrealm_home
                .join("run/companion-endpoint.json"),
        };
        persist_companion_discovery(
            &data_paths.companion_discovery,
            &format!("http://{address}"),
            &instance_id,
        )
        .map_err(|error| ApiServerError::Setup(error.to_string()))?;
        let companion_state = load_companion_state(&data_paths.companion_auth)
            .map_err(|error| ApiServerError::Setup(error.to_string()))?;
        let codex_socket = install_paths
            .actrealm_home
            .join("run/codex-app-server.sock");
        let codex_auth = codex_home.join("auth.json");
        let codex = if config.enable_codex_connector {
            CodexManager::start(store.clone(), waiters.clone(), &codex_socket, &codex_auth)
        } else {
            CodexManager::disabled(store.clone())
        };
        let mut usage_collector = UsageCollector::new(usage_paths);
        if config.enable_live_usage_pricing {
            usage_collector.enable_live_pricing();
        }
        let state = AppState {
            store,
            waiters,
            auth: Arc::new(Mutex::new(AuthState {
                bootstrap_token: Some(bootstrap_token.clone()),
                session_token: initial_session_token,
                csrf_token: initial_csrf_token,
                websocket_tickets: Vec::new(),
                native_sessions: Vec::new(),
            })),
            companions: Arc::new(Mutex::new(companion_state)),
            expected_host: address.to_string(),
            expected_origin: format!("http://{address}"),
            api_address: address,
            instance_id,
            runtime_started_at: config.runtime_started_at,
            restart_count: config.restart_count,
            websocket_connections,
            shutdown_flag: shutdown_flag.clone(),
            commit_delay: config.commit_delay,
            snapshot_interval: config.snapshot_interval,
            heartbeat_interval: config.heartbeat_interval,
            quota_poll_interval: config.quota_poll_interval,
            claude_oauth_quota: config.enable_claude_oauth_quota,
            installer: Arc::new(Installer::new(install_paths, source_binary)),
            quota: Arc::new(Mutex::new(QuotaState {
                collector: QuotaCollector::new(quota_paths),
                entries: Vec::new(),
                refreshed_at: None,
                claude_cache_modified_at: None,
                codex_rate_limits_captured_at: None,
                oauth_refresh_in_progress: false,
                oauth_next_poll_at: 0,
                oauth_last_result: None,
            })),
            pricing_status: Arc::new(Mutex::new(usage_collector.pricing_status())),
            pricing_refresh_requested: Arc::new(AtomicBool::new(false)),
            usage: Arc::new(Mutex::new(UsageState {
                collector: usage_collector,
                refreshed_at: None,
            })),
            live_codex_usage: Arc::new(Mutex::new(HashMap::new())),
            usage_worker_failures: Arc::new(AtomicUsize::new(0)),
            usage_consecutive_failures: Arc::new(AtomicUsize::new(0)),
            usage_collection_in_progress: Arc::new(AtomicBool::new(false)),
            usage_collection_ready: Arc::new(AtomicBool::new(false)),
            usage_history_complete: Arc::new(AtomicBool::new(false)),
            usage_last_success_at: Arc::new(AtomicU64::new(0)),
            review_collection_in_progress: Arc::new(AtomicBool::new(false)),
            review_collection_ready: Arc::new(AtomicBool::new(false)),
            review_consecutive_failures: Arc::new(AtomicUsize::new(0)),
            review_last_success_at: Arc::new(AtomicU64::new(0)),
            #[cfg(test)]
            usage_test_control: None,
            data_paths,
            codex,
            runtime_restart: config.runtime_restart,
        };
        let mut native_tls = None;
        let native_listener = if config.enable_native_clients {
            let tls = crate::native_tls::NativeTls::new().map_err(ApiServerError::Io)?;
            let path = state
                .data_paths
                .companion_discovery
                .with_file_name("native-clients.sock");
            let mut native_state = state.clone();
            native_state.expected_host = tls.address.to_string();
            native_state.expected_origin = format!("https://{}", tls.address);
            let mut registrar = native_clients::Registrar::new(native_state.clone())
                .map_err(|error| ApiServerError::Setup(error.to_string()))?;
            registrar.certificate_sha256 = Some(tls.certificate_sha256.clone());
            native_tls = Some((tls, router(native_state)));
            Some(
                crate::native_transport::NativeListener::start(path, move |identity, request| {
                    registrar.connect(identity, request)
                })
                .map_err(ApiServerError::Io)?,
            )
        } else {
            None
        };
        let quota_scheduler_state = state.clone();
        let usage_scheduler_state = state.clone();
        let usage_worker_failures = state.usage_worker_failures.clone();
        let router = router(state);
        let (shutdown, shutdown_receiver) = oneshot::channel();
        let api_thread = thread::Builder::new()
            .name("actrealm-api".to_owned())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                let Ok(runtime) = runtime else { return };
                runtime.block_on(async move {
                    let tls_handle = axum_server::Handle::new();
                    let tls_task = native_tls.map(|(tls, router)| {
                        let handle = tls_handle.clone();
                        tokio::spawn(async move { tls.serve(router, handle).await })
                    });
                    let quota_scheduler = tokio::spawn(quota_refresh_loop(quota_scheduler_state));
                    let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                        quota_scheduler.abort();
                        return;
                    };
                    let _ = axum::serve(listener, router)
                        .with_graceful_shutdown(async {
                            let _ = shutdown_receiver.await;
                        })
                        .await;
                    tls_handle.shutdown();
                    if let Some(task) = tls_task {
                        let _ = task.await;
                    }
                    quota_scheduler.abort();
                });
            })
            .map_err(|error| ApiServerError::Thread(error.to_string()))?;
        let usage_thread = match thread::Builder::new()
            .name("actrealm-usage".to_owned())
            .spawn(move || usage_refresh_loop(usage_scheduler_state))
        {
            Ok(thread) => thread,
            Err(error) => {
                shutdown_flag.store(true, Ordering::Release);
                let _ = shutdown.send(());
                let _ = api_thread.join();
                return Err(ApiServerError::Thread(error.to_string()));
            }
        };
        Ok(Self {
            native_listener,
            address,
            bootstrap_token,
            shutdown: Some(shutdown),
            shutdown_flag,
            thread: Some(api_thread),
            usage_thread: Some(usage_thread),
            review_thread: None,
            usage_worker_failures,
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

    /// Counts local usage worker failures without exposing provider content.
    pub fn usage_worker_failure_count(&self) -> usize {
        self.usage_worker_failures.load(Ordering::Acquire)
    }
}

impl Drop for ApiServer {
    fn drop(&mut self) {
        self.native_listener.take();
        self.shutdown_flag.store(true, Ordering::Release);
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.usage_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.review_thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Clone)]
struct AppState {
    store: RuntimeStore,
    waiters: WaiterRegistry,
    auth: Arc<Mutex<AuthState>>,
    companions: Arc<Mutex<CompanionState>>,
    expected_host: String,
    expected_origin: String,
    api_address: SocketAddr,
    instance_id: String,
    runtime_started_at: u64,
    restart_count: u32,
    websocket_connections: Arc<AtomicUsize>,
    shutdown_flag: Arc<AtomicBool>,
    commit_delay: Duration,
    snapshot_interval: Duration,
    heartbeat_interval: Duration,
    quota_poll_interval: Duration,
    claude_oauth_quota: bool,
    installer: Arc<Installer>,
    quota: Arc<Mutex<QuotaState>>,
    usage: Arc<Mutex<UsageState>>,
    live_codex_usage: Arc<Mutex<HashMap<String, LiveCodexUsage>>>,
    pricing_status: Arc<Mutex<PricingStatus>>,
    pricing_refresh_requested: Arc<AtomicBool>,
    usage_worker_failures: Arc<AtomicUsize>,
    usage_consecutive_failures: Arc<AtomicUsize>,
    usage_collection_in_progress: Arc<AtomicBool>,
    usage_collection_ready: Arc<AtomicBool>,
    usage_history_complete: Arc<AtomicBool>,
    usage_last_success_at: Arc<AtomicU64>,
    review_collection_in_progress: Arc<AtomicBool>,
    review_collection_ready: Arc<AtomicBool>,
    review_consecutive_failures: Arc<AtomicUsize>,
    review_last_success_at: Arc<AtomicU64>,
    #[cfg(test)]
    usage_test_control: Option<UsageRefreshTestControl>,
    data_paths: DataPaths,
    codex: CodexManager,
    runtime_restart: Option<RuntimeRestartHandle>,
}

#[cfg(test)]
#[derive(Clone)]
struct UsageRefreshTestControl {
    action: Arc<Mutex<Option<UsageRefreshTestAction>>>,
}

#[cfg(test)]
#[derive(Clone)]
enum UsageRefreshTestAction {
    Panic {
        used: Arc<AtomicBool>,
    },
    Block {
        started: Arc<Barrier>,
        release: Arc<Barrier>,
        used: Arc<AtomicBool>,
    },
}

#[cfg(test)]
impl UsageRefreshTestControl {
    fn panic_once() -> Self {
        Self {
            action: Arc::new(Mutex::new(Some(UsageRefreshTestAction::Panic {
                used: Arc::new(AtomicBool::new(false)),
            }))),
        }
    }

    fn block_once() -> Self {
        Self {
            action: Arc::new(Mutex::new(Some(UsageRefreshTestAction::Block {
                started: Arc::new(Barrier::new(2)),
                release: Arc::new(Barrier::new(2)),
                used: Arc::new(AtomicBool::new(false)),
            }))),
        }
    }

    fn wait_until_collecting(&self) {
        let action = self
            .action
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let Some(UsageRefreshTestAction::Block { started, .. }) = action {
            started.wait();
        }
    }

    fn release_collection(&self) {
        let action = self
            .action
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let Some(UsageRefreshTestAction::Block { release, .. }) = action {
            release.wait();
        }
    }

    fn before_collect(&self) {
        let action = self
            .action
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        match action {
            Some(UsageRefreshTestAction::Panic { used }) if !used.swap(true, Ordering::AcqRel) => {
                panic!("usage refresh test panic");
            }
            Some(UsageRefreshTestAction::Block {
                started,
                release,
                used,
            }) if !used.swap(true, Ordering::AcqRel) => {
                started.wait();
                release.wait();
            }
            None => {}
            _ => {}
        }
    }
}

#[derive(Clone)]
struct CodexManager {
    connector: Option<CodexConnector>,
    executable: Option<PathBuf>,
    state: Arc<Mutex<CodexManagerState>>,
    store: RuntimeStore,
    waiters: WaiterRegistry,
    auth_path: Option<PathBuf>,
    auth_stamp: Option<CredentialStamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CredentialStamp {
    device: u64,
    inode: u64,
    modified: SystemTime,
    modified_nanos: i64,
    changed: i64,
    changed_nanos: i64,
    length: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexQuotaRefresh {
    Refreshed,
    Failed,
    CredentialsChanged,
}

#[derive(Default)]
struct CodexManagerState {
    native_activities: HashMap<String, Vec<crate::codex_questions::ObservedActivity>>,
    async_questions: QuestionRegistry,
    status: String,
    error: Option<String>,
    last_notification_method: Option<String>,
    last_notification_at: Option<u64>,
    last_plan_skip_reason: Option<String>,
    last_plan_field_keys: Vec<String>,
    threads: HashMap<String, CodexThread>,
    managed: HashSet<String>,
    resume_failed: HashSet<String>,
    native_waiting: HashMap<String, bool>,
    native_synced: HashSet<String>,
    auto_reviewing: HashSet<String>,
    auto_review_escalated: HashSet<String>,
    managed_request_ids: HashMap<String, (Uuid, String)>,
    rate_limits: Option<Value>,
    rate_limits_captured_at: Option<u64>,
    rate_limits_error: Option<String>,
    credential_change_handled: bool,
}

fn begin_codex_credential_reconnect(state: &mut CodexManagerState) -> bool {
    if state.credential_change_handled {
        return false;
    }
    state.credential_change_handled = true;
    true
}

fn take_codex_pending_requests(state: &mut CodexManagerState) -> (Vec<Uuid>, Vec<String>) {
    let mut request_ids = state
        .managed_request_ids
        .drain()
        .map(|(_, (request_id, _))| request_id)
        .collect::<Vec<_>>();
    request_ids.sort_unstable();
    request_ids.dedup();
    let mut thread_ids = state.native_waiting.keys().cloned().collect::<Vec<_>>();
    thread_ids.extend(state.threads.keys().cloned());
    thread_ids.sort();
    thread_ids.dedup();
    state.native_waiting.clear();
    state.native_synced.clear();
    (request_ids, thread_ids)
}

struct QuotaState {
    collector: QuotaCollector,
    entries: Vec<QuotaEntry>,
    refreshed_at: Option<Instant>,
    claude_cache_modified_at: Option<SystemTime>,
    codex_rate_limits_captured_at: Option<u64>,
    oauth_refresh_in_progress: bool,
    oauth_next_poll_at: u64,
    oauth_last_result: Option<Result<u64, (&'static str, String)>>,
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
    companion_auth: PathBuf,
    companion_discovery: PathBuf,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NativeSessionKind {
    Full,
    SetupReadOnly,
    SetupReadWrite,
}

struct NativeSession {
    token: String,
    csrf: String,
    kind: NativeSessionKind,
    companion_id: Option<String>,
}

struct AuthState {
    bootstrap_token: Option<String>,
    session_token: Option<String>,
    csrf_token: Option<String>,
    websocket_tickets: Vec<(String, Instant)>,
    native_sessions: Vec<NativeSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CompanionRegistration {
    id: String,
    client_name: String,
    token_hash: String,
    scopes: Vec<String>,
    created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CompanionAuthFile {
    schema_version: u32,
    registrations: Vec<CompanionRegistration>,
}

#[derive(Debug, Clone)]
struct CompanionPairingGrant {
    code: String,
    client_name: String,
    scopes: Vec<String>,
    expires_at: u64,
}

#[derive(Debug, Default)]
struct CompanionState {
    pairing: Option<CompanionPairingGrant>,
    registrations: Vec<CompanionRegistration>,
}

#[derive(Debug, Clone)]
struct CompanionAuthorization {
    id: String,
    scopes: Vec<String>,
}

impl CompanionAuthorization {
    fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|candidate| candidate == scope)
    }
}

fn load_companion_state(path: &FilePath) -> io::Result<CompanionState> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(CompanionState::default())
        }
        Err(error) => return Err(error),
    };
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "companion authorization file must be private",
        ));
    }
    let decoded: CompanionAuthFile = serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if decoded.schema_version != COMPANION_SCHEMA_VERSION
        || decoded.registrations.len() > 16
        || decoded.registrations.iter().any(|registration| {
            Uuid::parse_str(&registration.id).is_err()
                || !valid_companion_client_name(&registration.client_name)
                || !valid_secret_hash(&registration.token_hash)
                || registration.scopes.is_empty()
                || registration
                    .scopes
                    .iter()
                    .any(|scope| !is_companion_scope(scope))
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid companion authorization file",
        ));
    }
    Ok(CompanionState {
        pairing: None,
        registrations: decoded.registrations,
    })
}

fn persist_companion_state(path: &FilePath, state: &CompanionState) -> io::Result<()> {
    let Some(parent) = path.parent() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "companion authorization path has no parent",
        ));
    };
    fs::create_dir_all(parent)?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    let encoded = serde_json::to_vec_pretty(&CompanionAuthFile {
        schema_version: COMPANION_SCHEMA_VERSION,
        registrations: state.registrations.clone(),
    })
    .map_err(io::Error::other)?;
    let temporary = path.with_extension(format!("tmp-{}", Uuid::now_v7()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&temporary)?;
    file.write_all(&encoded)?;
    file.sync_all()?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

fn persist_companion_discovery(
    path: &FilePath,
    endpoint: &str,
    instance_id: &str,
) -> io::Result<()> {
    let encoded = serde_json::to_vec_pretty(&json!({
        "schemaVersion": COMPANION_SCHEMA_VERSION,
        "endpoint": endpoint,
        "instanceId": instance_id,
    }))
    .map_err(io::Error::other)?;
    let Some(parent) = path.parent() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "companion discovery path has no parent",
        ));
    };
    fs::create_dir_all(parent)?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    let temporary = path.with_extension(format!("tmp-{}", Uuid::now_v7()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&temporary)?;
    file.write_all(&encoded)?;
    file.sync_all()?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

fn valid_companion_client_name(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 64
        && !trimmed.chars().any(char::is_control)
        && trimmed == value
}

fn is_companion_scope(scope: &str) -> bool {
    matches!(
        scope,
        COMPANION_SCOPE_SNAPSHOT | COMPANION_SCOPE_JUMP | COMPANION_SCOPE_RESPOND
    )
}

fn secret_hash(secret: &str) -> String {
    digest(&SHA256, secret.as_bytes())
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn valid_secret_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn companion_authorization(
    state: &AppState,
    headers: &HeaderMap,
) -> Option<CompanionAuthorization> {
    if !valid_host(state, headers) {
        return None;
    }
    let token = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())?
        .strip_prefix("Bearer ")?;
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let candidate = secret_hash(token);
    let companions = state.companions.lock().ok()?;
    companions
        .registrations
        .iter()
        .find(|registration| constant_time_eq(&registration.token_hash, &candidate))
        .map(|registration| CompanionAuthorization {
            id: registration.id.clone(),
            scopes: registration.scopes.clone(),
        })
}

impl CodexManager {
    fn disabled(store: RuntimeStore) -> Self {
        Self {
            connector: None,
            executable: None,
            state: Arc::new(Mutex::new(CodexManagerState {
                status: "disabled".to_owned(),
                ..CodexManagerState::default()
            })),
            store,
            waiters: WaiterRegistry::default(),
            auth_path: None,
            auth_stamp: None,
        }
    }

    fn start(
        store: RuntimeStore,
        waiters: WaiterRegistry,
        socket_path: &FilePath,
        auth_path: &FilePath,
    ) -> Self {
        // Capture the credential identity before connector initialization. If the
        // account changes while app-server starts, the first poll must detect it.
        let auth_stamp = credential_stamp(auth_path);
        let state = Arc::new(Mutex::new(CodexManagerState {
            status: "unavailable".to_owned(),
            ..CodexManagerState::default()
        }));
        let executable = discover_provider_availability(HookProvider::Codex)
            .app_server_executable()
            .map(ToOwned::to_owned);
        let Some(executable) = executable else {
            if let Ok(mut current) = state.lock() {
                current.error =
                    Some("No Codex client with app-server support was found".to_owned());
            }
            return Self {
                connector: None,
                executable: None,
                state,
                store,
                waiters,
                auth_path: Some(auth_path.to_path_buf()),
                auth_stamp,
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
                    executable: Some(executable),
                    state,
                    store,
                    waiters,
                    auth_path: Some(auth_path.to_path_buf()),
                    auth_stamp,
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
        if let Ok(mut current) = state.lock() {
            current.status = "connected".to_owned();
            current.error = None;
            current.managed = managed.clone();
        }
        spawn_codex_handlers(
            connector.clone(),
            channels,
            Arc::clone(&state),
            store.clone(),
            waiters.clone(),
        );
        spawn_codex_initial_sync(
            connector.clone(),
            Arc::clone(&state),
            store.clone(),
            waiters.clone(),
            managed,
        );
        Self {
            connector: Some(connector),
            executable: Some(executable),
            state,
            store,
            waiters,
            auth_path: Some(auth_path.to_path_buf()),
            auth_stamp,
        }
    }

    fn attach(&self, thread_id: &str) -> Result<CodexThread, String> {
        let connector = self
            .connector
            .as_ref()
            .ok_or_else(|| "The Codex app-server Connector is unavailable".to_owned())?;
        let thread = connector
            .resume_thread(thread_id)
            .map_err(|error| error.to_string())?;
        let managed = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "Connector state is unavailable".to_owned())?;
            state.managed.insert(thread_id.to_owned());
            state.resume_failed.remove(thread_id);
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
        // An explicit attach is a positive status observation. Reassert an
        // active turn immediately, but never use a possibly stale idle
        // snapshot to end work owned by the original Codex window.
        if thread.status == "active" {
            let _ =
                self.store
                    .sync_provider_execution(Provider::Codex, thread_id, true, now_millis());
        }
        sync_initial_codex_native_attention(&self.state, &self.store, &self.waiters, &thread);
        Ok(thread)
    }

    fn capability_value(&self) -> Value {
        let Ok(state) = self.state.lock() else {
            return json!({"status":"unavailable","error":"Connector state is unavailable"});
        };
        json!({
            "status": state.status,
            "error": state.error,
            "rateLimitsError": state.rate_limits_error,
            "lastNotificationMethod": state.last_notification_method,
            "lastNotificationAt": state.last_notification_at,
            "lastPlanSkipReason": state.last_plan_skip_reason,
            "lastPlanFieldKeys": state.last_plan_field_keys,
            "managedThreads": state.managed.len(),
            "protocol": "app-server",
            "experimentalUserInput": true,
            "managedApprovals": self
                .connector
                .as_ref()
                .is_some_and(CodexConnector::supports_managed_approvals),
            "serverUserAgent": self
                .connector
                .as_ref()
                .and_then(CodexConnector::server_user_agent)
        })
    }

    fn refresh_rate_limits(&self) -> CodexQuotaRefresh {
        let current_stamp = self.auth_path.as_deref().and_then(credential_stamp);
        if credential_stamp_changed(&self.auth_stamp, &current_stamp) {
            let should_reconnect = self.state.lock().is_ok_and(|mut state| {
                if !begin_codex_credential_reconnect(&mut state) {
                    return false;
                }
                state.rate_limits_error =
                    Some("Codex account credentials changed; reconnecting".to_owned());
                true
            });
            if !should_reconnect {
                return CodexQuotaRefresh::Failed;
            }
            if let Some(connector) = self.connector.as_ref() {
                connector.shutdown();
            }
            return CodexQuotaRefresh::CredentialsChanged;
        }
        self.refresh_rate_limits_quota_only()
    }

    /// Refreshes the read-only account quota without changing connector or
    /// credential lifecycle. The first usage-ledger scan may defer a Runtime
    /// restart, but it must not make official Codex quota disappear meanwhile.
    fn refresh_rate_limits_quota_only(&self) -> CodexQuotaRefresh {
        let rate_limits = self
            .connector
            .as_ref()
            .and_then(|connector| connector.read_rate_limits().ok())
            .or_else(|| {
                self.executable
                    .as_deref()
                    .and_then(|executable| CodexConnector::read_rate_limits_once(executable).ok())
            });
        let Some(rate_limits) = rate_limits else {
            if let Ok(mut state) = self.state.lock() {
                state.rate_limits_error = Some("Codex account quota refresh failed".to_owned());
            }
            return CodexQuotaRefresh::Failed;
        };
        let Ok(mut state) = self.state.lock() else {
            return CodexQuotaRefresh::Failed;
        };
        state.rate_limits = Some(rate_limits);
        state.rate_limits_captured_at = Some(now_millis());
        state.rate_limits_error = None;
        CodexQuotaRefresh::Refreshed
    }

    fn mark_credential_restart_failed(&self) {
        let (request_ids, thread_ids) = if let Ok(mut state) = self.state.lock() {
            let detail =
                "Codex account credentials changed; automatic reconnect failed. Restart ActRealm manually.";
            state.status = "unavailable".to_owned();
            state.error = Some(detail.to_owned());
            state.rate_limits_error = Some(detail.to_owned());
            take_codex_pending_requests(&mut state)
        } else {
            (Vec::new(), Vec::new())
        };
        for request_id in request_ids {
            let _ = self
                .waiters
                .pass_through(request_id, "connector_restart_failed");
            let _ =
                self.store
                    .expire_approval(request_id, "connector_restart_failed", now_millis());
        }
        for thread_id in thread_ids {
            let _ = self.store.sync_native_approval(
                Provider::Codex,
                &thread_id,
                false,
                false,
                now_millis(),
            );
            let _ =
                self.store
                    .sync_provider_execution(Provider::Codex, thread_id, false, now_millis());
        }
    }

    fn rate_limit_entries(&self) -> Option<(Vec<QuotaEntry>, u64)> {
        let state = self.state.lock().ok()?;
        let snapshot = state.rate_limits.as_ref()?;
        let captured_at = state.rate_limits_captured_at?;
        let entries = codex_app_server_entries(snapshot, captured_at);
        (!entries.is_empty()).then_some((entries, captured_at))
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
        let ended = terminal_execution_state(exec_state);
        (
            "external_hook".to_owned(),
            if ended { "ended" } else { "observing" }.to_owned(),
            state.status == "connected",
            status,
        )
    }
}

fn terminal_execution_state(exec_state: &str) -> bool {
    matches!(exec_state, "idle" | "response_finished" | "failed")
}

fn recovery_for_execution(exec_state: &str, recovery: String) -> String {
    if !terminal_execution_state(exec_state) && recovery == "ended" {
        // A connector snapshot can be stale or omit a currently executing
        // turn. Execution remains Runtime-owned truth, so recovery must wait
        // for another Provider event instead of declaring the work ended.
        return "waiting_for_event".to_owned();
    }
    recovery
}

/// Provider account reads, thread listing, and persisted managed-thread
/// restoration may each wait for an app-server timeout. They must never delay
/// the loopback API or the bootstrap line consumed by the native app.
fn spawn_codex_initial_sync(
    connector: CodexConnector,
    state: Arc<Mutex<CodexManagerState>>,
    store: RuntimeStore,
    waiters: WaiterRegistry,
    managed: HashSet<String>,
) {
    let failure_state = Arc::clone(&state);
    let result = thread::Builder::new()
        .name("actrealm-codex-initial-sync".to_owned())
        .spawn(move || {
            match connector.read_rate_limits() {
                Ok(rate_limits) => {
                    if let Ok(mut current) = state.lock() {
                        current.rate_limits = Some(rate_limits);
                        current.rate_limits_captured_at = Some(now_millis());
                        current.rate_limits_error = None;
                    }
                }
                Err(_) => {
                    if let Ok(mut current) = state.lock() {
                        current.rate_limits_error =
                            Some("Codex account quota refresh failed".to_owned());
                    }
                }
            }

            if let Ok(listed) = connector.list_threads() {
                for thread in listed {
                    // An independent app-server lists Desktop threads as
                    // notLoaded/idle. That is not a terminal event from the
                    // process which actually owns the turn.
                    if initial_codex_execution_is_authoritative(&thread) {
                        let _ = store.sync_provider_execution(
                            Provider::Codex,
                            &thread.id,
                            thread.status == "active",
                            now_millis(),
                        );
                    }
                    sync_initial_codex_native_attention(&state, &store, &waiters, &thread);
                    if let Ok(mut current) = state.lock() {
                        current.threads.insert(thread.id.clone(), thread);
                    }
                }
            }

            let mut managed = managed.into_iter().collect::<Vec<_>>();
            managed.sort();
            for thread_id in managed {
                if let Ok(thread) = connector.resume_thread(&thread_id) {
                    sync_initial_codex_native_attention(&state, &store, &waiters, &thread);
                    if let Ok(mut current) = state.lock() {
                        current.resume_failed.remove(&thread_id);
                        current.threads.insert(thread.id.clone(), thread);
                    }
                } else if let Ok(mut current) = state.lock() {
                    current.resume_failed.insert(thread_id);
                }
            }
        });
    if let Err(error) = result {
        if let Ok(mut current) = failure_state.lock() {
            current.rate_limits_error = Some(format!(
                "Codex initial synchronization could not start: {error}"
            ));
        }
    }
}

fn spawn_codex_handlers(
    connector: CodexConnector,
    channels: ConnectorChannels,
    state: Arc<Mutex<CodexManagerState>>,
    store: RuntimeStore,
    waiters: WaiterRegistry,
) {
    let request_connector = connector.downgrade();
    let request_state = Arc::clone(&state);
    let request_store = store.clone();
    let request_waiters = waiters.clone();
    thread::spawn(move || {
        for request in channels.requests {
            let Some(connector) = request_connector.upgrade() else {
                break;
            };
            handle_codex_server_request(
                connector,
                Arc::clone(&request_state),
                request_store.clone(),
                request_waiters.clone(),
                request,
            );
        }
        if let Ok(mut current) = request_state.lock() {
            current.status = "unavailable".to_owned();
            current.error = Some("Codex app-server request channel disconnected".to_owned());
        }
    });
    let notification_store = store.clone();
    thread::spawn(move || {
        for notification in channels.notifications {
            update_codex_notification(&state, &notification_store, &waiters, notification);
        }
        if let Ok(mut current) = state.lock() {
            current.status = "unavailable".to_owned();
            current.error = Some("Codex app-server notification channel disconnected".to_owned());
        }
    });
}

fn initial_codex_execution_is_authoritative(thread: &CodexThread) -> bool {
    // A persisted attachment is not evidence that this new connector owns
    // the live turn. Inactivity must come from an explicit lifecycle event.
    thread.status == "active"
}

fn handle_codex_server_request(
    connector: CodexConnector,
    state: Arc<Mutex<CodexManagerState>>,
    store: RuntimeStore,
    waiters: WaiterRegistry,
    request: ServerRequest,
) {
    if matches!(
        request.method.as_str(),
        "item/commandExecution/requestApproval"
            | "item/fileChange/requestApproval"
            | "item/permissions/requestApproval"
    ) {
        handle_codex_approval_request(connector, state, store, waiters, request);
        return;
    }
    if request.method != "item/tool/requestUserInput" {
        let _ =
            connector.respond_error(request.id, -32601, "Unsupported ActRealm connector request");
        return;
    }
    if request.params.get("isBlocking").and_then(Value::as_bool) == Some(false) {
        let thread_id = request
            .params
            .get("threadId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let batch = codex_question_batch(&store, &request.params, true, now_millis());
        let Some(mut batch) = batch else {
            let _ = connector.respond_error(request.id, -32602, "Invalid asynchronous questions");
            return;
        };
        let question_ids = request.params["questions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|q| q["id"].as_str().map(ToOwned::to_owned))
            .collect::<Vec<_>>();
        if question_ids.len() != batch.questions.len()
            || question_ids.iter().collect::<HashSet<_>>().len() != question_ids.len()
        {
            let _ = connector.respond_error(request.id, -32602, "Invalid question IDs");
            return;
        }
        batch.route = ReplyRoute::Rpc {
            id: request.id,
            question_ids,
        };
        batch.can_answer = true;
        if let Ok(mut current) = state.lock() {
            current.managed.insert(thread_id.to_owned());
            current.async_questions.observe(batch);
        }
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
    let rpc_key = rpc_id_key(&request.id);
    let Ok(mut current) = state.lock() else {
        let _ = connector.respond_error(request.id, -32000, "Connector state unavailable");
        return;
    };
    if current.credential_change_handled {
        let _ = connector.respond_error(request.id, -32001, "Codex connector is restarting");
        return;
    }
    let registration = match waiters.register_at(&bridge_request, now_millis()) {
        Ok(registration) => registration,
        Err(_) => {
            let _ = connector.respond_error(request.id, -32000, "Question waiter unavailable");
            return;
        }
    };
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
    current
        .managed_request_ids
        .insert(rpc_key.clone(), (request_id, thread_id.clone()));
    let mut managed = current.managed.iter().cloned().collect::<Vec<_>>();
    managed.sort();
    let _ = store.sync_native_approval(Provider::Codex, &thread_id, false, true, now_millis());
    match store.ingest(bridge_request.clone()) {
        Ok(result) if result.suppressed => {
            current.managed_request_ids.remove(&rpc_key);
            drop(current);
            let _ = waiters.pass_through(request_id, "provider_internal");
            let _ = connector.respond_error(
                request.id,
                -32001,
                "Internal Codex session is not exposed to ActRealm",
            );
            return;
        }
        Ok(_) => drop(current),
        Err(_) => {
            current.managed_request_ids.remove(&rpc_key);
            drop(current);
            let _ = waiters.pass_through(request_id, "runtime_error");
            let _ = connector.respond_error(request.id, -32000, "ActRealm storage unavailable");
            return;
        }
    }
    if let Ok(managed) = serde_json::to_string(&managed) {
        let _ = store.write_setting(MANAGED_CODEX_THREADS_KEY, managed);
    }
    thread::spawn(move || {
        let wait_for = bridge_request
            .deadline_at
            .map(|deadline| Duration::from_millis(deadline.saturating_sub(now_millis())))
            .unwrap_or(Duration::from_secs(60));
        let response = registration.ticket.recv_timeout(wait_for);
        if let Ok(mut current) = state.lock() {
            current.managed_request_ids.remove(&rpc_key);
        }
        match response {
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

fn handle_codex_approval_request(
    connector: CodexConnector,
    state: Arc<Mutex<CodexManagerState>>,
    store: RuntimeStore,
    waiters: WaiterRegistry,
    request: ServerRequest,
) {
    if !connector.supports_managed_approvals() {
        let _ = connector.respond_error(
            request.id,
            -32601,
            "Codex app-server version is not enabled for managed approvals",
        );
        return;
    }
    let Some(bridge_request) =
        BridgeRequest::codex_approval_at(&request.method, request.params.clone(), now_millis())
    else {
        let _ = connector.respond_error(request.id, -32602, "Invalid approval params");
        return;
    };
    let Some(thread_id) = bridge_request.provider_session_id.clone() else {
        let _ = connector.respond_error(request.id, -32602, "Missing threadId");
        return;
    };
    let request_id = bridge_request.request_id.unwrap_or(bridge_request.id);
    let rpc_key = rpc_id_key(&request.id);
    let Ok(mut current) = state.lock() else {
        let _ = connector.respond_error(request.id, -32000, "Connector state unavailable");
        return;
    };
    if current.credential_change_handled {
        let _ = connector.respond_error(request.id, -32001, "Codex connector is restarting");
        return;
    }
    if !current.managed.contains(&thread_id) {
        let _ = connector.respond_error(
            request.id,
            -32001,
            "Thread is not explicitly managed by ActRealm",
        );
        return;
    }
    let registration = match waiters.register_at(&bridge_request, now_millis()) {
        Ok(registration) => registration,
        Err(_) => {
            let _ = connector.respond_error(request.id, -32000, "Approval waiter unavailable");
            return;
        }
    };
    current
        .managed_request_ids
        .insert(rpc_key.clone(), (request_id, thread_id));
    match store.ingest(bridge_request.clone()) {
        Ok(result) if result.suppressed => {
            current.managed_request_ids.remove(&rpc_key);
            drop(current);
            let _ = waiters.pass_through(request_id, "provider_internal");
            let _ = connector.respond_error(
                request.id,
                -32001,
                "Internal Codex session is not exposed to ActRealm",
            );
            return;
        }
        Ok(_) => drop(current),
        Err(_) => {
            current.managed_request_ids.remove(&rpc_key);
            drop(current);
            let _ = waiters.pass_through(request_id, "runtime_error");
            let _ = connector.respond_error(request.id, -32000, "ActRealm storage unavailable");
            return;
        }
    }
    thread::spawn(move || {
        let wait_for = bridge_request
            .deadline_at
            .map(|deadline| Duration::from_millis(deadline.saturating_sub(now_millis())))
            .unwrap_or(Duration::from_secs(60));
        match registration.ticket.recv_timeout(wait_for) {
            Ok(response) => {
                let result =
                    codex_approval_response(&request.method, &request.params, response.action);
                let sent = match result {
                    Some(result) => connector.respond(request.id, result),
                    None => connector.respond_error(
                        request.id,
                        -32001,
                        "Approval was returned to the provider without a decision",
                    ),
                };
                if sent.is_err() {
                    if let Ok(mut current) = state.lock() {
                        current.managed_request_ids.remove(&rpc_key);
                    }
                    let _ = store.expire_approval(
                        request_id,
                        "connector_response_failed",
                        now_millis(),
                    );
                }
            }
            Err(_) => {
                if let Ok(mut current) = state.lock() {
                    current.managed_request_ids.remove(&rpc_key);
                }
                let _ = waiters.pass_through(request_id, "deadline");
                let _ = store.expire_approval(request_id, "deadline", now_millis());
                let _ = connector.respond_error(request.id, -32001, "Approval expired");
            }
        }
    });
}

fn codex_approval_response(method: &str, params: &Value, action: ReplyAction) -> Option<Value> {
    match (method, action) {
        (
            "item/commandExecution/requestApproval" | "item/fileChange/requestApproval",
            ReplyAction::Allow,
        ) => Some(json!({"decision":"accept"})),
        (
            "item/commandExecution/requestApproval" | "item/fileChange/requestApproval",
            ReplyAction::Deny,
        ) => Some(json!({"decision":"decline"})),
        ("item/permissions/requestApproval", ReplyAction::Allow) => Some(json!({
            "permissions": granted_permission_profile(params),
            "scope": "turn"
        })),
        ("item/permissions/requestApproval", ReplyAction::Deny) => Some(json!({
            "permissions": {},
            "scope": "turn"
        })),
        _ => None,
    }
}

fn granted_permission_profile(params: &Value) -> Value {
    let Some(requested) = params.get("permissions").and_then(Value::as_object) else {
        return json!({});
    };
    Value::Object(
        requested
            .iter()
            .filter(|(key, value)| {
                matches!(key.as_str(), "network" | "fileSystem") && !value.is_null()
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )
}

fn rpc_id_key(id: &Value) -> String {
    id.as_u64()
        .map(|value| value.to_string())
        .or_else(|| id.as_i64().map(|value| value.to_string()))
        .or_else(|| id.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| id.to_string())
}

fn ingest_codex_provider_events(store: &RuntimeStore, notification: &ServerNotification) {
    for request in codex_notification_events(notification, now_millis()) {
        let _ = store.ingest(request);
    }
}

fn codex_notification_events(
    notification: &ServerNotification,
    received_at: u64,
) -> Vec<BridgeRequest> {
    let Some(thread_id) = notification
        .params
        .get("threadId")
        .and_then(Value::as_str)
        .or_else(|| {
            notification
                .params
                .get("thread")
                .and_then(|thread| thread.get("id"))
                .and_then(Value::as_str)
        })
    else {
        return Vec::new();
    };
    let explicit_turn_id = notification
        .params
        .get("turnId")
        .and_then(Value::as_str)
        .or_else(|| {
            notification
                .params
                .get("turn")
                .and_then(|turn| turn.get("id"))
                .and_then(Value::as_str)
        });
    // Some app-server plan notifications omit turnId. Preserve that absence:
    // Runtime can bind the plan to the currently open turn, whereas inventing
    // a new Provider turn ID would make a previous plan look current.
    let turn_id = explicit_turn_id;
    let event_name = match notification.method.as_str() {
        "turn/started" => Some("UserPromptSubmit"),
        "turn/plan/updated" => Some("PlanUpdated"),
        "item/autoApprovalReview/started" => Some("AutoApprovalReviewStarted"),
        "item/autoApprovalReview/completed" => Some("AutoApprovalReviewCompleted"),
        "turn/completed" => match notification
            .params
            .pointer("/turn/status")
            .and_then(Value::as_str)
        {
            Some("completed") => Some("Stop"),
            Some("interrupted") => Some("TurnInterrupted"),
            Some("failed") => Some("StopFailure"),
            _ => None,
        },
        _ => None,
    };
    let event_payload = if notification.method == "turn/plan/updated" {
        normalized_codex_plan_payload(&notification.params)
    } else {
        notification.params.clone()
    };
    let mut requests = event_name
        .map(|event_name| {
            BridgeRequest::from_provider_event_at(
                Provider::Codex,
                event_name,
                thread_id,
                turn_id,
                event_payload,
                received_at,
            )
        })
        .into_iter()
        .collect::<Vec<_>>();
    if matches!(
        notification.method.as_str(),
        "item/started" | "item/completed" | "turn/completed"
    ) {
        append_codex_subagent_events(
            &mut requests,
            thread_id,
            turn_id,
            &notification.params,
            received_at,
        );
    }
    requests
}

fn normalized_codex_plan_payload(params: &Value) -> Value {
    let mut payload = params.clone();
    let Some(object) = payload.as_object_mut() else {
        return payload;
    };
    if !object.contains_key("plan") {
        if let Some(plan) = object.get("steps").or_else(|| object.get("items")).cloned() {
            object.insert("plan".to_owned(), plan);
        }
    }
    payload
}

fn codex_plan_diagnostic(params: &Value) -> (Option<&'static str>, Vec<String>) {
    let has_thread = params
        .get("threadId")
        .and_then(Value::as_str)
        .or_else(|| params.pointer("/thread/id").and_then(Value::as_str))
        .is_some();
    if !has_thread {
        return (Some("missing_thread_id"), Vec::new());
    }
    let has_plan = ["plan", "steps", "items"]
        .into_iter()
        .any(|key| params.get(key).and_then(Value::as_array).is_some());
    if has_plan {
        return (None, Vec::new());
    }
    let mut keys = params
        .as_object()
        .into_iter()
        .flat_map(|object| object.keys())
        .filter(|key| {
            !key.is_empty()
                && key.len() <= 64
                && key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        })
        .take(32)
        .cloned()
        .collect::<Vec<_>>();
    keys.sort();
    (Some("missing_plan_array"), keys)
}

fn append_codex_subagent_events(
    requests: &mut Vec<BridgeRequest>,
    thread_id: &str,
    turn_id: Option<&str>,
    params: &Value,
    received_at: u64,
) {
    let items = params
        .get("item")
        .into_iter()
        .chain(
            params
                .get("turn")
                .and_then(|turn| turn.get("items"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        )
        .collect::<Vec<_>>();
    for item in items {
        match item.get("type").and_then(Value::as_str) {
            Some("collabAgentToolCall") => {
                let model = item.get("model").and_then(Value::as_str);
                let item_id = item.get("id").and_then(Value::as_str);
                let Some(states) = item.get("agentsStates").and_then(Value::as_object) else {
                    continue;
                };
                for (agent_id, state) in states {
                    let status = state
                        .get("status")
                        .and_then(Value::as_str)
                        .or_else(|| state.as_str());
                    let event_name = match status {
                        Some("pendingInit" | "running") => "SubagentStart",
                        Some("interrupted" | "completed" | "errored" | "shutdown" | "notFound") => {
                            "SubagentStop"
                        }
                        _ => continue,
                    };
                    requests.push(BridgeRequest::from_provider_event_at(
                        Provider::Codex,
                        event_name,
                        thread_id,
                        turn_id,
                        json!({
                            "agent_id": agent_id,
                            "agent_type": model.unwrap_or("codex_collab_agent"),
                            "agent_status": status,
                            "item_id": item_id,
                            "source": "codex_app_server"
                        }),
                        received_at,
                    ));
                }
            }
            Some("subAgentActivity") => {
                let Some(agent_id) = item.get("agentThreadId").and_then(Value::as_str) else {
                    continue;
                };
                let kind = item.get("kind").and_then(Value::as_str);
                let event_name = match kind {
                    Some("started" | "interacted") => "SubagentStart",
                    Some("interrupted") => "SubagentStop",
                    _ => continue,
                };
                requests.push(BridgeRequest::from_provider_event_at(
                    Provider::Codex,
                    event_name,
                    thread_id,
                    turn_id,
                    json!({
                        "agent_id": agent_id,
                        "agent_type": "codex_subagent",
                        "agent_path": item.get("agentPath"),
                        "agent_status": kind,
                        "item_id": item.get("id"),
                        "source": "codex_app_server"
                    }),
                    received_at,
                ));
            }
            _ => {}
        }
    }
}

fn update_codex_notification(
    state: &Arc<Mutex<CodexManagerState>>,
    store: &RuntimeStore,
    waiters: &WaiterRegistry,
    notification: ServerNotification,
) {
    let observed_at = now_millis();
    if let Ok(mut current) = state.lock() {
        current.last_notification_method = Some(
            notification
                .method
                .chars()
                .take(128)
                .filter(|character| !character.is_control())
                .collect(),
        );
        current.last_notification_at = Some(observed_at);
        if notification.method == "turn/plan/updated" {
            let (reason, keys) = codex_plan_diagnostic(&notification.params);
            current.last_plan_skip_reason = reason.map(ToOwned::to_owned);
            current.last_plan_field_keys = keys;
        }
    }
    if notification.method == "account/rateLimits/updated" {
        update_codex_rate_limits(state, &notification.params, observed_at);
        return;
    }
    ingest_codex_provider_events(store, &notification);
    if notification.method == "serverRequest/resolved" {
        if let Some(id) = notification.params.get("requestId") {
            if let Ok(mut current) = state.lock() {
                current.async_questions.resolve_rpc(id);
            }
        }
        let local_request_id = notification
            .params
            .get("requestId")
            .map(rpc_id_key)
            .and_then(|key| {
                state
                    .lock()
                    .ok()
                    .and_then(|mut current| current.managed_request_ids.remove(&key))
                    .map(|(request_id, _)| request_id)
            });
        if let Some(request_id) = local_request_id {
            let _ = waiters.pass_through(request_id, "provider_resolved");
            let _ = store.resolve_managed_request(request_id, now_millis());
        }
        return;
    }
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

    if notification.method == "item/completed" {
        let item = &notification.params["item"];
        if item["type"] == "agentMessage" && item["phase"] == "final_answer" {
            if let Some(text) = item["text"].as_str() {
                if let Ok(snapshot) = store.snapshot() {
                    if let Some(session) = snapshot
                        .sessions
                        .iter()
                        .find(|s| s.provider == "codex" && s.provider_session_id == thread_id)
                    {
                        let cwd = store
                            .local_review_context(&session.id)
                            .ok()
                            .flatten()
                            .and_then(|c| c.working_directory);
                        store.observe_result(
                            &session.id,
                            text,
                            cwd.as_deref().map(FilePath::new),
                            observed_at,
                            "codex:app_server_final",
                        );
                    }
                }
            }
        }
        if item["type"] == "agentMessage" && item["questions"].is_array() {
            let params = json!({
                "threadId": thread_id,
                "itemId": item["id"],
                "questions": item["questions"],
            });
            if let Some(mut batch) = codex_question_batch(store, &params, false, observed_at) {
                if let Ok(mut current) = state.lock() {
                    if current.managed.contains(&thread_id)
                        && current
                            .threads
                            .get(&thread_id)
                            .is_some_and(|t| t.status == "active")
                    {
                        if let Some(turn_id) = notification.params["turnId"].as_str() {
                            batch.route = ReplyRoute::Steer {
                                turn_id: turn_id.to_owned(),
                            };
                            batch.can_answer = true;
                        }
                    }
                    current.async_questions.observe(batch);
                }
            }
        } else if item["type"] == "userMessage" {
            if let Ok(mut current) = state.lock() {
                current
                    .async_questions
                    .clear_thread(&thread_id, observed_at);
            }
        }
    }
    if matches!(
        notification.method.as_str(),
        "turn/completed" | "thread/closed"
    ) {
        if let Ok(mut current) = state.lock() {
            current.async_questions.end_turn(&thread_id);
        }
    }
    if notification.method == "turn/started" {
        if let Ok(mut current) = state.lock() {
            current
                .async_questions
                .clear_thread(&thread_id, observed_at);
        }
    }

    let (waiting, active, sync_native) = {
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
                    approval_policy: None,
                    approvals_reviewer: None,
                    sandbox_mode: None,
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
            current
                .threads
                .entry(thread_id.clone())
                .and_modify(|thread| {
                    thread.status = "active".to_owned();
                    thread.active_flags.clear();
                })
                .or_insert_with(|| CodexThread {
                    id: thread_id.clone(),
                    name: None,
                    cwd: None,
                    status: "active".to_owned(),
                    active_flags: Vec::new(),
                    updated_at: None,
                    approval_policy: None,
                    approvals_reviewer: None,
                    sandbox_mode: None,
                });
        } else if notification.method == "item/autoApprovalReview/started" {
            current.auto_reviewing.insert(thread_id.clone());
            current.auto_review_escalated.remove(&thread_id);
        } else if notification.method == "item/autoApprovalReview/completed" {
            current.auto_reviewing.remove(&thread_id);
            let status = notification
                .params
                .pointer("/review/status")
                .and_then(Value::as_str);
            if matches!(status, Some("timedOut" | "aborted")) {
                current.auto_review_escalated.insert(thread_id.clone());
            } else {
                current.auto_review_escalated.remove(&thread_id);
            }
        } else if notification.method == "thread/settings/updated" {
            if let Some(thread) = current.threads.get_mut(&thread_id) {
                update_codex_thread_settings(thread, &notification.params);
            }
        } else {
            return;
        }
        let waiting = current
            .threads
            .get(&thread_id)
            .is_some_and(|thread| thread_waiting_for_user_approval(&current, &thread_id, thread));
        let changed = current.native_waiting.insert(thread_id.clone(), waiting) != Some(waiting);
        let active = current
            .threads
            .get(&thread_id)
            .is_some_and(|thread| thread.status == "active");
        let sync_native = changed || (waiting && !current.native_synced.contains(&thread_id));
        (waiting, active, sync_native)
    };
    let _ = store.sync_provider_execution(Provider::Codex, &thread_id, active, observed_at);
    if sync_native {
        sync_codex_native_attention(state, store, waiters, &thread_id, waiting, active);
    }
}

fn codex_question_batch(
    store: &RuntimeStore,
    params: &Value,
    rpc: bool,
    now: u64,
) -> Option<QuestionBatch> {
    let thread_id = params["threadId"].as_str()?;
    let session_id = store
        .snapshot()
        .ok()?
        .sessions
        .into_iter()
        .find(|s| s.provider == "codex" && s.provider_session_id == thread_id)?
        .id;
    Some(QuestionBatch {
        id: Uuid::nil(),
        session_id,
        thread_id: thread_id.to_owned(),
        item_id: params["itemId"].as_str()?.to_owned(),
        created_at: now,
        questions: codex_questions::questions(&params["questions"], rpc)?,
        can_answer: false,
        route: ReplyRoute::Observe,
        submitting: false,
    })
}

fn update_codex_rate_limits(
    state: &Arc<Mutex<CodexManagerState>>,
    params: &Value,
    observed_at: u64,
) {
    let Some(update) = params.get("rateLimits") else {
        return;
    };
    let Ok(mut current) = state.lock() else {
        return;
    };
    let response = current
        .rate_limits
        .get_or_insert_with(|| json!({"rateLimits": {}}));
    if !response.is_object() {
        *response = json!({"rateLimits": {}});
    }
    if let Some(target) = response.get_mut("rateLimits") {
        merge_non_null_json(target, update);
    }
    if let Some(limit_id) = update.get("limitId").and_then(Value::as_str) {
        if let Some(bucket) = response
            .get_mut("rateLimitsByLimitId")
            .and_then(Value::as_object_mut)
            .and_then(|buckets| buckets.get_mut(limit_id))
        {
            merge_non_null_json(bucket, update);
        }
    }
    current.rate_limits_captured_at = Some(observed_at);
    current.rate_limits_error = None;
}

/// Codex rolling updates are sparse. Missing and null fields do not clear a
/// value from the most recent full account/rateLimits/read response.
fn merge_non_null_json(target: &mut Value, update: &Value) {
    if update.is_null() {
        return;
    }
    match (target, update) {
        (Value::Object(target), Value::Object(update)) => {
            for (key, value) in update {
                if value.is_null() {
                    continue;
                }
                if let Some(existing) = target.get_mut(key) {
                    merge_non_null_json(existing, value);
                } else {
                    target.insert(key.clone(), value.clone());
                }
            }
        }
        (target, update) => *target = update.clone(),
    }
}

fn thread_waiting_on_approval(thread: &CodexThread) -> bool {
    thread_has_waiting_approval_flag(thread)
        && !matches!(thread.approval_policy.as_deref(), Some("never"))
        && !matches!(
            thread.approvals_reviewer.as_deref(),
            Some("auto_review" | "guardian_subagent")
        )
}

fn thread_waiting_for_user_approval(
    state: &CodexManagerState,
    thread_id: &str,
    thread: &CodexThread,
) -> bool {
    if state
        .managed_request_ids
        .values()
        .any(|(_, managed_thread_id)| managed_thread_id == thread_id)
    {
        return false;
    }
    if !thread_has_waiting_approval_flag(thread) {
        return false;
    }
    if state.auto_review_escalated.contains(thread_id) {
        return true;
    }
    if state.auto_reviewing.contains(thread_id) {
        return false;
    }
    thread_waiting_on_approval(thread)
}

fn thread_has_waiting_approval_flag(thread: &CodexThread) -> bool {
    thread.status == "active"
        && thread
            .active_flags
            .iter()
            .any(|flag| flag == "waitingOnApproval")
}

fn update_codex_thread_settings(thread: &mut CodexThread, params: &Value) {
    let settings = params.get("settings").unwrap_or(params);
    if let Some(value) = protocol_setting(settings.get("approvalPolicy")) {
        thread.approval_policy = Some(value);
    }
    if let Some(value) = protocol_setting(settings.get("approvalsReviewer")) {
        thread.approvals_reviewer = Some(value);
    }
    if let Some(value) = protocol_setting(settings.get("sandbox")) {
        thread.sandbox_mode = Some(value);
    }
}

fn protocol_setting(value: Option<&Value>) -> Option<String> {
    let value = value?;
    value
        .as_str()
        .or_else(|| value.get("type").and_then(Value::as_str))
        .map(ToOwned::to_owned)
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

/// A fresh app-server listing is a snapshot, not an observed transition. In
/// particular, an independently spawned app-server can list a Codex Desktop
/// Thread without seeing the already-open native permission sheet. Therefore
/// startup/attach may affirm `waitingOnApproval`, but absence of that flag must
/// not clear a Hook-observed OUTBOX request. Once a waiting=true baseline is
/// synced, later live notifications use `sync_codex_native_attention` to
/// resolve the request on an authoritative true -> false transition.
fn sync_initial_codex_native_attention(
    state: &Arc<Mutex<CodexManagerState>>,
    store: &RuntimeStore,
    waiters: &WaiterRegistry,
    thread: &CodexThread,
) {
    let waiting = thread_waiting_on_approval(thread);
    if let Ok(mut current) = state.lock() {
        current.native_waiting.insert(thread.id.clone(), waiting);
    }
    if waiting {
        sync_codex_native_attention(
            state,
            store,
            waiters,
            &thread.id,
            true,
            thread.status == "active",
        );
    }
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.css", get(styles))
        .route("/i18n.js", get(i18n_script))
        .route("/agent-state.js", get(agent_state_script))
        .route("/agent-detail.js", get(agent_detail_script))
        .route("/app.js", get(script))
        .route("/assets/claude.png", get(claude_icon))
        .route("/assets/codex.png", get(codex_icon))
        .route("/api/v1/health", get(health))
        .route("/api/v1/native/session", get(native_session_health))
        .route("/api/v1/runtime/status", get(runtime_status))
        .route("/api/v1/runtime/restart", post(restart_runtime))
        .route("/api/v1/bootstrap", post(bootstrap))
        .route("/api/v1/companions/pairing", post(create_companion_pairing))
        .route("/api/v1/companions", get(list_companions))
        .route("/api/v1/companions/{id}", delete(revoke_companion))
        .route("/api/v1/companion/enroll", post(enroll_companion))
        .route("/api/v1/companion/snapshot", get(companion_snapshot))
        .route(
            "/api/v1/companion/settings/completion",
            get(companion_completion_settings).put(update_companion_completion_settings),
        )
        .route(
            "/api/v1/companion/sessions/{id}/activity",
            get(companion_session_activity),
        )
        .route(
            "/api/v1/companion/sessions/{id}/review",
            get(companion_session_review),
        )
        .route(
            "/api/v1/companion/sessions/{id}/result",
            get(companion_session_result),
        )
        .route(
            "/api/v1/companion/sessions/{id}/artifacts/{artifact}/reveal",
            post(companion_reveal_artifact),
        )
        .route(
            "/api/v1/companion/sessions/{id}/jump",
            post(companion_jump_session),
        )
        .route("/api/v1/companion/commands", post(companion_command))
        .route("/api/v1/companion/commands/{id}/undo", post(companion_undo))
        .route(
            "/api/v1/companion/questions/{id}/answer",
            post(companion_answer_question),
        )
        .route(
            "/api/v1/companion/async-questions/{id}/answer",
            post(companion_answer_async_question),
        )
        .route(
            "/api/v1/companion/pricing/refresh",
            post(companion_refresh_pricing),
        )
        .route("/api/v1/snapshot", get(snapshot))
        .route("/api/v1/history", get(task_history))
        .route("/api/v1/setup", get(setup).post(change_setup))
        .route("/api/v1/settings", get(settings).put(update_settings))
        .route("/api/v1/quota/claude-bridge", post(change_claude_bridge))
        .route("/api/v1/quota/refresh", post(refresh_quota))
        .route("/api/v1/quota/refresh-now", post(refresh_quota_now))
        .route("/api/v1/export", get(export_data))
        .route("/api/v1/token-usage/export", get(export_token_usage_json))
        .route(
            "/api/v1/token-usage/export.csv",
            get(export_token_usage_csv),
        )
        .route("/api/v1/metrics", post(record_metric))
        .route("/api/v1/metrics/export", get(export_metrics))
        .route("/api/v1/data/clear", post(clear_data))
        .route("/api/v1/backups/clear", post(clear_backups))
        .route("/api/v1/commands", post(command))
        .route("/api/v1/questions/{id}/answer", post(answer_question))
        .route("/api/v1/commands/{id}/undo", post(undo))
        .route("/api/v1/sessions/{id}/timeline", get(session_timeline))
        .route("/api/v1/sessions/{id}/review", get(session_review))
        .route(
            "/api/v1/sessions/{id}/review/diff",
            get(session_review_diff),
        )
        .route(
            "/api/v1/sessions/{id}/checkpoints",
            get(session_checkpoints).post(create_session_checkpoint),
        )
        .route(
            "/api/v1/checkpoints/{id}",
            get(task_checkpoint).delete(delete_checkpoint),
        )
        .route(
            "/api/v1/checkpoints/{id}/preflight",
            get(checkpoint_preflight),
        )
        .route("/api/v1/checkpoints/{id}/actions", post(checkpoint_action))
        .route("/api/v1/sessions/{id}/jump", post(jump_session))
        .route("/api/v1/sessions/{id}/archive", post(archive_task))
        .route("/api/v1/sessions/{id}/history", delete(delete_task_history))
        .route("/api/v1/sessions/{id}/manage", post(manage_session))
        .route("/api/v1/ws-ticket", post(issue_websocket_ticket))
        .route("/api/v1/ws", get(websocket))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::map_response(security_headers))
        .with_state(state)
}

async fn security_headers(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(
            "default-src 'self'; connect-src 'self' ws://127.0.0.1:* ws://[::1]:*; img-src 'self' data:; style-src 'self'; script-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'",
        ),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    headers.insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("DENY"),
    );
    headers.insert(
        HeaderName::from_static("cross-origin-resource-policy"),
        HeaderValue::from_static("same-origin"),
    );
    response
}

async fn index() -> Response {
    static_response("text/html; charset=utf-8", INDEX_HTML)
}

async fn styles() -> Response {
    static_response("text/css; charset=utf-8", APP_CSS)
}

async fn i18n_script() -> Response {
    static_response("text/javascript; charset=utf-8", I18N_JS)
}

async fn agent_state_script() -> Response {
    static_response("text/javascript; charset=utf-8", AGENT_STATE_JS)
}

async fn agent_detail_script() -> Response {
    static_response("text/javascript; charset=utf-8", AGENT_DETAIL_JS)
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

async fn refresh_quota(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    let reset = state.quota.lock().map(|mut quota| {
        quota.refreshed_at = None;
        quota.claude_cache_modified_at = None;
    });
    if reset.is_err() {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "QUOTA_STATE_UNAVAILABLE");
    }
    start_oauth_quota_refresh(&state, true);
    match quota_entries(&state) {
        Ok(entries) => Json(json!({
            "accepted": true,
            "entries": entries.len(),
            "oauthRefreshInProgress": state
                .quota
                .lock()
                .map(|quota| quota.oauth_refresh_in_progress)
                .unwrap_or(false)
        }))
        .into_response(),
        Err(_) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "QUOTA_REFRESH_FAILED"),
    }
}

async fn refresh_quota_now(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    if !state.claude_oauth_quota {
        return api_error_detail(
            StatusCode::CONFLICT,
            "CLAUDE_OAUTH_DISABLED",
            "Claude OAuth quota sync is disabled",
        );
    }
    // Join the same job used by automatic/wake refresh. A click during the
    // background poll must not fail with "already running" or start a second
    // Claude process against the same rotating credential chain.
    start_oauth_quota_refresh(&state, true);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
    loop {
        let result = match state.quota.lock() {
            Ok(quota) if quota.oauth_refresh_in_progress => None,
            Ok(quota) => quota.oauth_last_result.clone(),
            Err(_) => {
                return api_error(StatusCode::INTERNAL_SERVER_ERROR, "QUOTA_STATE_UNAVAILABLE")
            }
        };
        if let Some(result) = result {
            return match result {
                Ok(captured_at) => Json(json!({
                    "accepted": true, "completed": true, "claudeCapturedAt": captured_at
                }))
                .into_response(),
                Err((code, detail)) => {
                    api_error_detail(StatusCode::SERVICE_UNAVAILABLE, code, &detail)
                }
            };
        }
        if tokio::time::Instant::now() >= deadline {
            return api_error(StatusCode::CONFLICT, "QUOTA_REFRESH_IN_PROGRESS");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn quota_refresh_error_code(error: &QuotaError) -> &'static str {
    match error {
        QuotaError::OAuthUnavailable => "CLAUDE_SIGN_IN_REQUIRED",
        QuotaError::OAuthRequest(message) if message == "credential was rejected" => {
            "CLAUDE_AUTH_REFRESH_FAILED"
        }
        QuotaError::OAuthRequest(message) if message == "temporarily rate limited" => {
            "CLAUDE_QUOTA_RATE_LIMITED"
        }
        _ => "CLAUDE_QUOTA_REFRESH_FAILED",
    }
}

fn quota_refresh_error_detail(error: &QuotaError) -> String {
    match error {
        QuotaError::OAuthUnavailable => {
            "Claude Code is not signed in or its credentials are not readable. Sign in to Claude Code once, then refresh. No conversation is needed.".to_owned()
        }
        QuotaError::OAuthRequest(message) if message == "credential was rejected" => {
            "Claude rejected the credential after automatic renewal. Check the Claude Code sign-in state and sign in again if required.".to_owned()
        }
        QuotaError::OAuthRequest(message) if message == "temporarily rate limited" => {
            "The Claude quota endpoint is temporarily rate limited. Try again later.".to_owned()
        }
        QuotaError::OAuthRequest(message) => {
            format!("The Claude quota endpoint is temporarily unavailable: {message}")
        }
        _ => "The local quota cache could not be refreshed. Check the Claude sign-in state and try again.".to_owned(),
    }
}

async fn quota_refresh_loop(state: AppState) {
    loop {
        if state.shutdown_flag.load(Ordering::Acquire) {
            return;
        }
        let _ = quota_entries(&state);
        // A multi-gigabyte first usage rebuild is intentionally bounded and
        // can outlive Codex credential rotation. Do not let the quota
        // connector restart the whole Runtime before that rebuild has written
        // its first canonical generation and private scan checkpoint. Quota
        // cache projection remains available during this bounded grace.
        if should_defer_codex_credential_refresh(&state, now_millis()) {
            let codex = state.codex.clone();
            let _ = tokio::task::spawn_blocking(move || codex.refresh_rate_limits_quota_only())
                .await
                .unwrap_or(CodexQuotaRefresh::Failed);
        } else {
            let codex = state.codex.clone();
            let codex_refresh = tokio::task::spawn_blocking(move || codex.refresh_rate_limits())
                .await
                .unwrap_or(CodexQuotaRefresh::Failed);
            if codex_refresh == CodexQuotaRefresh::CredentialsChanged {
                let restart_state = state.clone();
                let restarted = tokio::task::spawn_blocking(move || {
                    restart_after_codex_credential_change(&restart_state)
                })
                .await
                .unwrap_or(false);
                if restarted {
                    return;
                }
                state.codex.mark_credential_restart_failed();
            }
        }
        // Quota freshness must not depend on a healthy WebSocket client.
        // Sleep/wake can suspend that client while Runtime remains alive.
        let _ = quota_entries(&state);
        tokio::time::sleep(state.quota_poll_interval).await;
    }
}

fn should_defer_codex_credential_refresh(state: &AppState, now: u64) -> bool {
    !state.usage_collection_ready.load(Ordering::Acquire)
        && now.saturating_sub(state.runtime_started_at)
            < CODEX_CREDENTIAL_REFRESH_FIRST_USAGE_GRACE_MS
}

fn restart_after_codex_credential_change(state: &AppState) -> bool {
    let Some(handle) = state.runtime_restart.as_ref() else {
        return false;
    };
    let restart_token = Uuid::now_v7().to_string();
    let preserved_auth = state
        .auth
        .lock()
        .ok()
        .and_then(|auth| auth.session_token.clone().zip(auth.csrf_token.clone()));
    let (session_token, csrf_token) = preserved_auth
        .map(|(session, csrf)| (Some(session), Some(csrf)))
        .unwrap_or((None, None));
    let Ok(receiver) = handle.request(restart_token, state.api_address, session_token, csrf_token)
    else {
        return false;
    };
    matches!(receiver.recv_timeout(Duration::from_secs(3)), Ok(Ok(())))
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

async fn setup(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authorized_setup(&state, &headers, false) {
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

async fn health(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !valid_host(&state, &headers) {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_HOST");
    }
    Json(json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "protocolVersion": PUBLIC_PROTOCOL_VERSION,
        "instanceId": state.instance_id,
    }))
    .into_response()
}

async fn runtime_status(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return api_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED");
    }
    let snapshot = match state.store.snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return api_error_detail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "STORAGE_ERROR",
                &error.to_string(),
            )
        }
    };
    let active_sessions = snapshot
        .sessions
        .iter()
        .filter(|session| {
            !matches!(
                session.exec_state.as_str(),
                "idle" | "response_finished" | "failed"
            )
        })
        .count();
    let pending_attention = snapshot
        .attention
        .iter()
        .filter(|item| {
            matches!(
                item.state.as_str(),
                "open" | "committing" | "decision_sent" | "snoozed"
            )
        })
        .count();
    let last_hook_event_at = snapshot
        .sessions
        .iter()
        .map(|session| session.last_event_at)
        .max();
    let generated_at = now_millis();
    let snapshot_freshness = match last_hook_event_at {
        Some(last_event_at) if last_event_at > generated_at.saturating_add(30_000) => "invalid",
        Some(last_event_at)
            if generated_at.saturating_sub(last_event_at) <= FACT_LIVE_MAX_AGE_MS =>
        {
            "live"
        }
        Some(last_event_at)
            if generated_at.saturating_sub(last_event_at) <= FACT_DELAYED_MAX_AGE_MS =>
        {
            "delayed"
        }
        Some(_) => "stale",
        None => "unavailable",
    };
    let waiter_count = state
        .waiters
        .active_request_ids()
        .map(|ids| ids.len())
        .unwrap_or_default();
    let (bridge_status, bridge_name, bridge_private) = state
        .runtime_restart
        .as_ref()
        .map(|handle| {
            let name = handle
                .socket_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("bridge.sock")
                .to_owned();
            match fs::symlink_metadata(&handle.socket_path) {
                Ok(metadata) if metadata.file_type().is_socket() => {
                    let private = metadata.permissions().mode() & 0o077 == 0;
                    (if private { "ready" } else { "insecure" }, name, private)
                }
                Ok(_) => ("invalid", name, false),
                Err(error) if error.kind() == io::ErrorKind::NotFound => ("missing", name, false),
                Err(_) => ("unavailable", name, false),
            }
        })
        .unwrap_or(("unavailable", "bridge.sock".to_owned(), false));
    let storage_value = match state.store.storage_diagnostics() {
        Ok(diagnostics) => json!({
            "status": if diagnostics.integrity == "ok"
                && diagnostics.schema_version == diagnostics.expected_schema_version
            {
                "ready"
            } else {
                "degraded"
            },
            "eventCount": snapshot.event_count,
            "schemaVersion": diagnostics.schema_version,
            "expectedSchemaVersion": diagnostics.expected_schema_version,
            "integrity": diagnostics.integrity,
            "checkedAt": generated_at,
        }),
        Err(_) => json!({
            "status": "unavailable",
            "eventCount": snapshot.event_count,
            "schemaVersion": Value::Null,
            "expectedSchemaVersion": Value::Null,
            "integrity": "unavailable",
            "checkedAt": generated_at,
        }),
    };
    let usage_quality = usage_collection_quality(&state);
    let (companion_status, companion_count, companion_scopes) = state
        .companions
        .lock()
        .map(|companions| {
            let mut scopes = companions
                .registrations
                .iter()
                .flat_map(|registration| registration.scopes.iter().cloned())
                .collect::<Vec<_>>();
            scopes.sort();
            scopes.dedup();
            ("ready", companions.registrations.len(), scopes)
        })
        .unwrap_or_else(|_| ("unavailable", 0, Vec::new()));
    Json(json!({
        "schemaVersion": 2,
        "generatedAt": generated_at,
        "instanceId": state.instance_id,
        "pid": std::process::id(),
        "version": env!("CARGO_PKG_VERSION"),
        "commit": runtime_git_commit(),
        "protocolVersion": PUBLIC_PROTOCOL_VERSION,
        "startedAt": state.runtime_started_at,
        "uptimeMs": generated_at.saturating_sub(state.runtime_started_at),
        "api": {
            "status": "ready",
            "address": state.api_address.to_string(),
        },
        "websocket": {
            "status": "ready",
            "connections": state.websocket_connections.load(Ordering::Acquire),
        },
        "hook": {
            "status": bridge_status,
            "name": bridge_name,
            "private": bridge_private,
            "lastEventAt": last_hook_event_at,
        },
        "sessions": {
            "active": active_sessions,
            "total": snapshot.sessions.len(),
        },
        "snapshot": {
            "revision": snapshot.event_count,
            "revisionSource": "runtime:sqlite_event_count",
            "lastEventAt": last_hook_event_at,
            "freshness": snapshot_freshness,
        },
        "attention": {
            "pending": pending_attention,
            "waiters": waiter_count,
        },
        "storage": storage_value,
        "collectors": {
            "review": {
                "status": "parked",
                "source": "product_scope:parked",
                "gitCheck": "disabled",
                "pendingBaselines": Value::Null,
                "inProgress": false,
                "consecutiveFailures": 0,
                "lastSuccessfulAt": Value::Null,
            },
            "token": {
                "status": usage_quality.collection_state,
                "source": "runtime:canonical_session_ledger",
                "dataQuality": usage_quality.data_quality,
                "inProgress": usage_quality.in_progress,
                "historyComplete": usage_quality.history_complete,
                "consecutiveFailures": usage_quality.failures,
                "lastSuccessfulAt": if usage_quality.last_success > 0 {
                    Some(usage_quality.last_success)
                } else {
                    None
                },
            },
        },
        "companion": {
            "status": companion_status,
            "protocolVersion": PUBLIC_PROTOCOL_VERSION,
            "registrations": companion_count,
            "scopes": companion_scopes,
        },
        "conditional": {
            "claudeCowork": {
                "status": "unsupported",
                "countsAsFault": false,
                "reason": "no_verified_event_source",
            },
        },
        "restart": {
            "count": state.restart_count,
            "lastResult": if state.restart_count > 0 { "recovered" } else { "not_restarted" },
        }
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RestartRuntimeRequest {
    restart_token: String,
}

async fn restart_runtime(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RestartRuntimeRequest>,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    if Uuid::parse_str(&request.restart_token).is_err() {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_RESTART_TOKEN");
    }
    let Some(handle) = state.runtime_restart.as_ref() else {
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "RUNTIME_RESTART_UNAVAILABLE",
        );
    };
    let bootstrap_token = request.restart_token;
    let receiver = match handle.request(bootstrap_token.clone(), state.api_address, None, None) {
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
        Ok(Ok(())) => Json(json!({
            "ok": true,
            "restartToken": bootstrap_token,
            "previousInstanceId": state.instance_id,
        }))
        .into_response(),
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
    if !auth
        .bootstrap_token
        .as_deref()
        .is_some_and(|token| constant_time_eq(token, &request.token))
    {
        return api_error(StatusCode::UNAUTHORIZED, "INVALID_BOOTSTRAP");
    }
    let Ok(session_token) = generate_secret() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "AUTH_UNAVAILABLE");
    };
    let Ok(csrf_token) = generate_secret() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "AUTH_UNAVAILABLE");
    };
    auth.bootstrap_token = None;
    auth.session_token = Some(session_token.clone());
    auth.csrf_token = Some(csrf_token.clone());
    auth.websocket_tickets.clear();
    let cookie = format!("{SESSION_COOKIE}={session_token}; HttpOnly; SameSite=Strict; Path=/");
    let mut response = Json(BootstrapResponse { csrf_token }).into_response();
    if let Ok(cookie) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(SET_COOKIE, cookie);
    }
    response
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateCompanionPairingRequest {
    client_name: String,
    allow_control: bool,
}

async fn create_companion_pairing(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateCompanionPairingRequest>,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    if !valid_companion_client_name(&request.client_name) {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_CLIENT_NAME");
    }
    let Ok(secret) = generate_secret() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "AUTH_UNAVAILABLE");
    };
    let code = format!("AR1:{}:{secret}", state.api_address.port());
    let expires_at = now_millis().saturating_add(COMPANION_PAIRING_TTL_MS);
    let mut scopes = vec![
        COMPANION_SCOPE_SNAPSHOT.to_owned(),
        COMPANION_SCOPE_JUMP.to_owned(),
    ];
    if request.allow_control {
        scopes.push(COMPANION_SCOPE_RESPOND.to_owned());
    }
    let Ok(mut companions) = state.companions.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "AUTH_UNAVAILABLE");
    };
    companions.pairing = Some(CompanionPairingGrant {
        code: code.clone(),
        client_name: request.client_name,
        scopes: scopes.clone(),
        expires_at,
    });
    Json(json!({
        "enrollmentCode": code,
        "expiresAt": expires_at,
        "scopes": scopes,
        "endpoint": state.expected_origin,
    }))
    .into_response()
}

async fn list_companions(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return api_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED");
    }
    let Ok(companions) = state.companions.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "AUTH_UNAVAILABLE");
    };
    let connections = companions
        .registrations
        .iter()
        .map(|registration| {
            json!({
                "id": registration.id,
                "clientName": registration.client_name,
                "scopes": registration.scopes,
                "createdAt": registration.created_at,
            })
        })
        .collect::<Vec<_>>();
    Json(json!({ "connections": connections })).into_response()
}

async fn revoke_companion(
    State(state): State<AppState>,
    Path(companion_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    if Uuid::parse_str(&companion_id).is_err() {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_COMPANION_ID");
    }
    let Ok(mut companions) = state.companions.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "AUTH_UNAVAILABLE");
    };
    let mut next = companions.registrations.clone();
    next.retain(|registration| registration.id != companion_id);
    if next.len() == companions.registrations.len() {
        return api_error(StatusCode::NOT_FOUND, "COMPANION_NOT_FOUND");
    }
    let candidate = CompanionState {
        pairing: companions.pairing.clone(),
        registrations: next,
    };
    if persist_companion_state(&state.data_paths.companion_auth, &candidate).is_err() {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "AUTH_PERSIST_FAILED");
    }
    *companions = candidate;
    Json(json!({ "revoked": true, "id": companion_id })).into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EnrollCompanionRequest {
    enrollment_code: String,
}

async fn enroll_companion(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<EnrollCompanionRequest>,
) -> Response {
    if !valid_host(&state, &headers) {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_HOST");
    }
    let now = now_millis();
    let Ok(mut companions) = state.companions.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "AUTH_UNAVAILABLE");
    };
    let Some(pairing) = companions.pairing.clone() else {
        return api_error(StatusCode::CONFLICT, "PAIRING_UNAVAILABLE");
    };
    if pairing.expires_at <= now {
        companions.pairing = None;
        return api_error(StatusCode::CONFLICT, "PAIRING_EXPIRED");
    }
    if !constant_time_eq(&pairing.code, &request.enrollment_code) {
        return api_error(StatusCode::UNAUTHORIZED, "INVALID_PAIRING_CODE");
    }
    let Ok(token) = generate_secret() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "AUTH_UNAVAILABLE");
    };
    let registration = CompanionRegistration {
        id: Uuid::now_v7().to_string(),
        client_name: pairing.client_name,
        token_hash: secret_hash(&token),
        scopes: pairing.scopes,
        created_at: now,
    };
    let mut registrations = companions.registrations.clone();
    registrations.retain(|candidate| candidate.client_name != registration.client_name);
    registrations.push(registration.clone());
    if registrations.len() > 16 {
        registrations.remove(0);
    }
    let candidate = CompanionState {
        pairing: None,
        registrations,
    };
    if persist_companion_state(&state.data_paths.companion_auth, &candidate).is_err() {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "AUTH_PERSIST_FAILED");
    }
    *companions = candidate;
    Json(json!({
        "companionId": registration.id,
        "clientName": registration.client_name,
        "token": token,
        "scopes": registration.scopes,
        "endpoint": state.expected_origin,
        "discoveryPath": state.data_paths.companion_discovery,
    }))
    .into_response()
}

async fn companion_snapshot(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(authorization) = companion_authorization(&state, &headers) else {
        return api_error(StatusCode::UNAUTHORIZED, "COMPANION_UNAUTHORIZED");
    };
    if !authorization.has_scope(COMPANION_SCOPE_SNAPSHOT) {
        return api_error(StatusCode::FORBIDDEN, "COMPANION_SCOPE_REQUIRED");
    }
    companion_snapshot_value(&state, &authorization)
        .map(Json)
        .map(IntoResponse::into_response)
        .unwrap_or_else(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR"))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CompanionCompletionSettingsRequest {
    mode: String,
    minutes: u32,
}

fn companion_completion_settings_value(settings: &UiSettings) -> Value {
    json!({
        "mode": settings.completion_task_hide_mode,
        "minutes": settings.completion_auto_hide_minutes,
    })
}

async fn companion_completion_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let Some(authorization) = companion_authorization(&state, &headers) else {
        return api_error(StatusCode::UNAUTHORIZED, "COMPANION_UNAUTHORIZED");
    };
    if !authorization.has_scope(COMPANION_SCOPE_SNAPSHOT) {
        return api_error(StatusCode::FORBIDDEN, "COMPANION_SCOPE_REQUIRED");
    }
    match load_ui_settings(&state) {
        Ok(settings) => Json(companion_completion_settings_value(&settings)).into_response(),
        Err(error) => api_error_detail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "SETTINGS_READ_FAILED",
            &error.to_string(),
        ),
    }
}

async fn update_companion_completion_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CompanionCompletionSettingsRequest>,
) -> Response {
    let Some(authorization) = companion_authorization(&state, &headers) else {
        return api_error(StatusCode::UNAUTHORIZED, "COMPANION_UNAUTHORIZED");
    };
    if !authorization.has_scope(COMPANION_SCOPE_RESPOND) {
        return api_error(StatusCode::FORBIDDEN, "COMPANION_SCOPE_REQUIRED");
    }
    let mut settings = match load_ui_settings(&state) {
        Ok(settings) => settings,
        Err(error) => {
            return api_error_detail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "SETTINGS_READ_FAILED",
                &error.to_string(),
            )
        }
    };
    settings.completion_task_hide_mode = request.mode;
    settings.completion_auto_hide_minutes = request.minutes;
    if let Err(reason) = settings.validate() {
        return api_error_detail(StatusCode::BAD_REQUEST, "INVALID_SETTINGS", reason);
    }
    let encoded = match serde_json::to_string(&settings) {
        Ok(encoded) => encoded,
        Err(_) => return api_error(StatusCode::BAD_REQUEST, "INVALID_SETTINGS"),
    };
    if state
        .store
        .write_ui_settings(encoded, now_millis())
        .is_err()
    {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR");
    }
    Json(companion_completion_settings_value(&settings)).into_response()
}

async fn companion_session_activity(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Query(query): Query<TimelineQuery>,
    headers: HeaderMap,
) -> Response {
    let Some(authorization) = companion_authorization(&state, &headers) else {
        return api_error(StatusCode::UNAUTHORIZED, "COMPANION_UNAUTHORIZED");
    };
    if !authorization.has_scope(COMPANION_SCOPE_SNAPSHOT) {
        return api_error(StatusCode::FORBIDDEN, "COMPANION_SCOPE_REQUIRED");
    }
    if session_id.is_empty() || session_id.len() > 256 {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_SESSION_ID");
    }
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let result = match query.after_ingest_sequence {
        Some(after) => state.store.timeline(&session_id, Some(after), limit),
        None => state.store.latest_current_timeline(&session_id, limit),
    };
    match result {
        Ok(Some(page)) => Json(page).into_response(),
        Ok(None) => api_error(StatusCode::NOT_FOUND, "SESSION_NOT_FOUND"),
        Err(_) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR"),
    }
}

async fn companion_session_review(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(authorization) = companion_authorization(&state, &headers) else {
        return api_error(StatusCode::UNAUTHORIZED, "COMPANION_UNAUTHORIZED");
    };
    if !authorization.has_scope(COMPANION_SCOPE_SNAPSHOT) {
        return api_error(StatusCode::FORBIDDEN, "COMPANION_SCOPE_REQUIRED");
    }
    if session_id.is_empty() || session_id.len() > 256 {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_SESSION_ID");
    }
    let context = match state.store.local_review_context(&session_id) {
        Ok(Some(context)) => context,
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "SESSION_NOT_FOUND"),
        Err(_) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR"),
    };
    let timeline = state
        .store
        .latest_current_local_timeline(&session_id, 100)
        .ok()
        .flatten();
    let mut limitations = Vec::new();
    let baseline = context
        .turn_id
        .as_deref()
        .and_then(|turn_id| state.store.review_baseline(turn_id).ok().flatten());
    let mut repository = if let Some(baseline) = baseline.as_ref() {
        inspect_resolved_git_repository(
            &baseline.repository_root,
            context.concurrent_active_sessions,
            &mut limitations,
        )
    } else {
        match context.working_directory.as_deref() {
            Some(working_directory) => inspect_git_repository(
                working_directory,
                context.concurrent_active_sessions,
                &mut limitations,
            ),
            None => {
                limitations.push("working_directory_unavailable".to_owned());
                unavailable_review_repository("working_directory_unavailable")
            }
        }
    };
    if let Some(baseline) = baseline.as_ref() {
        apply_review_baseline(
            &mut repository,
            baseline,
            context.concurrent_active_sessions,
            &mut limitations,
        );
    }
    let validations = timeline
        .as_ref()
        .map(|page| review_validations(&page.events))
        .unwrap_or_default();
    let last_meaningful_action = timeline
        .as_ref()
        .and_then(|page| page.events.iter().rev().find_map(review_last_action));
    let outcome_state = match context.exec_state.as_str() {
        "response_finished" => "completed",
        "failed" => "failed",
        "idle" => "idle",
        _ => "running",
    };
    let outcome_verification = if matches!(outcome_state, "completed" | "failed") {
        "verified"
    } else {
        "not_applicable"
    };
    Json(TaskReviewSnapshot {
        schema_version: REVIEW_SCHEMA_VERSION,
        session_id: context.session_id,
        provider: context.provider,
        project_label: context.project,
        generated_at: now_millis(),
        turn_started_at: context.turn_started_at,
        turn_ended_at: context.turn_ended_at,
        outcome: ReviewOutcome {
            state: outcome_state.to_owned(),
            source: "runtime:terminal_reducer".to_owned(),
            verification: outcome_verification.to_owned(),
            observed_at: context.last_event_at,
        },
        repository,
        validations,
        last_meaningful_action,
        limitations,
    })
    .into_response()
}

async fn companion_session_result(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(auth) = companion_authorization(&state, &headers) else {
        return api_error(StatusCode::UNAUTHORIZED, "COMPANION_UNAUTHORIZED");
    };
    if !auth.has_scope(COMPANION_SCOPE_SNAPSHOT) {
        return api_error(StatusCode::FORBIDDEN, "COMPANION_SCOPE_REQUIRED");
    }
    if session_id.is_empty() || session_id.len() > 256 {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_SESSION_ID");
    }
    let Ok(Some(context)) = state.store.local_review_context(&session_id) else {
        return api_error(StatusCode::NOT_FOUND, "SESSION_NOT_FOUND");
    };
    let result = if (matches!(context.exec_state.as_str(), "response_finished" | "failed")
        || (context.exec_state == "idle" && context.turn_ended_at.is_some()))
    {
        state.store.session_result(
            &session_id,
            context.turn_started_at.unwrap_or(0),
            now_millis(),
        )
    } else {
        None
    };
    let result = result.map(|mut r| {
        for artifact in &mut r.artifacts {
            artifact.can_reveal &= auth.has_scope(COMPANION_SCOPE_JUMP);
        }
        r
    });
    let activities = state
        .codex
        .state
        .lock()
        .ok()
        .and_then(|s| s.native_activities.get(&session_id).cloned())
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.started_at >= context.turn_started_at.unwrap_or(0))
        .collect::<Vec<_>>();
    Json(json!({ "schemaVersion": 1, "sessionId": session_id, "result": result, "activities": activities })).into_response()
}

#[derive(Default, Deserialize)]
struct ArtifactRevealQuery {
    #[serde(default)]
    native: bool,
}

async fn companion_reveal_artifact(
    State(state): State<AppState>,
    Path((session_id, artifact)): Path<(String, String)>,
    Query(query): Query<ArtifactRevealQuery>,
    headers: HeaderMap,
) -> Response {
    let Some(auth) = companion_authorization(&state, &headers) else {
        return api_error(StatusCode::UNAUTHORIZED, "COMPANION_UNAUTHORIZED");
    };
    if !auth.has_scope(COMPANION_SCOPE_JUMP) {
        return api_error(StatusCode::FORBIDDEN, "COMPANION_SCOPE_REQUIRED");
    }
    if session_id.is_empty() || session_id.len() > 256 {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_SESSION_ID");
    }
    let Ok(Some(context)) = state.store.local_review_context(&session_id) else {
        return api_error(StatusCode::NOT_FOUND, "SESSION_NOT_FOUND");
    };
    if !(matches!(context.exec_state.as_str(), "response_finished" | "failed")
        || (context.exec_state == "idle" && context.turn_ended_at.is_some()))
    {
        return api_error(StatusCode::CONFLICT, "ARTIFACT_UNAVAILABLE");
    }
    let Some(path) = state.store.result_artifact_path(
        &session_id,
        &artifact,
        context.turn_started_at.unwrap_or(0),
        now_millis(),
    ) else {
        return api_error(StatusCode::NOT_FOUND, "ARTIFACT_UNAVAILABLE");
    };
    // Only a user-initiated, jump-scoped native request may receive this
    // transient local target. Paths remain absent from all overview snapshots.
    if query.native {
        return Json(json!({"ok": true, "localPath": path})).into_response();
    }
    // Reveal in Finder; never execute, open or evaluate Provider-linked artifacts.
    #[cfg(target_os = "macos")]
    let revealed = tokio::task::spawn_blocking(move || {
        let mut command = ProcessCommand::new("/usr/bin/open");
        command.arg("-R").arg(path);
        wait_for_artifact_reveal(command, Duration::from_secs(3))
    })
    .await
    .unwrap_or(false);
    #[cfg(not(target_os = "macos"))]
    let revealed = {
        let _ = path;
        false
    };
    if revealed {
        Json(json!({"ok":true})).into_response()
    } else {
        api_error(StatusCode::CONFLICT, "ARTIFACT_REVEAL_FAILED")
    }
}

// Starting `open` is not success: Launch Services can reject the request later.
// Keep this bounded and off the HTTP executor, and reap children on every path.
#[cfg(any(target_os = "macos", test))]
fn wait_for_artifact_reveal(mut command: ProcessCommand, timeout: Duration) -> bool {
    let Ok(mut child) = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

async fn companion_jump_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(authorization) = companion_authorization(&state, &headers) else {
        return api_error(StatusCode::UNAUTHORIZED, "COMPANION_UNAUTHORIZED");
    };
    if !authorization.has_scope(COMPANION_SCOPE_JUMP) {
        return api_error(StatusCode::FORBIDDEN, "COMPANION_SCOPE_REQUIRED");
    }
    process_jump_session(&state, &session_id)
}

async fn companion_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CommandRequest>,
) -> Response {
    let Some(authorization) = companion_authorization(&state, &headers) else {
        return api_error(StatusCode::UNAUTHORIZED, "COMPANION_UNAUTHORIZED");
    };
    if !authorization.has_scope(COMPANION_SCOPE_RESPOND) {
        return api_error(StatusCode::FORBIDDEN, "COMPANION_SCOPE_REQUIRED");
    }
    process_command(&state, request)
}

async fn companion_undo(
    State(state): State<AppState>,
    Path(command_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(authorization) = companion_authorization(&state, &headers) else {
        return api_error(StatusCode::UNAUTHORIZED, "COMPANION_UNAUTHORIZED");
    };
    if !authorization.has_scope(COMPANION_SCOPE_RESPOND) {
        return api_error(StatusCode::FORBIDDEN, "COMPANION_SCOPE_REQUIRED");
    }
    process_undo(&state, &command_id)
}

async fn companion_answer_question(
    State(state): State<AppState>,
    Path(request_id): Path<Uuid>,
    headers: HeaderMap,
    Json(submission): Json<Value>,
) -> Response {
    let Some(authorization) = companion_authorization(&state, &headers) else {
        return api_error(StatusCode::UNAUTHORIZED, "COMPANION_UNAUTHORIZED");
    };
    if !authorization.has_scope(COMPANION_SCOPE_RESPOND) {
        return api_error(StatusCode::FORBIDDEN, "COMPANION_SCOPE_REQUIRED");
    }
    process_answer_question(&state, request_id, submission)
}

async fn companion_refresh_pricing(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(auth) = companion_authorization(&state, &headers) else {
        return api_error(StatusCode::UNAUTHORIZED, "COMPANION_UNAUTHORIZED");
    };
    if !auth.has_scope(COMPANION_SCOPE_RESPOND) {
        return api_error(StatusCode::FORBIDDEN, "COMPANION_SCOPE_REQUIRED");
    }
    let Ok(mut status) = state.pricing_status.lock() else {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "STORAGE_ERROR");
    };
    if !status.updating {
        state
            .pricing_refresh_requested
            .store(true, Ordering::Release);
        status.updating = true;
    }
    (StatusCode::ACCEPTED, Json(json!({"state":"scheduled"}))).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AsyncQuestionAnswer {
    answers: Vec<String>,
}

async fn companion_answer_async_question(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(submission): Json<AsyncQuestionAnswer>,
) -> Response {
    let Some(auth) = companion_authorization(&state, &headers) else {
        return api_error(StatusCode::UNAUTHORIZED, "COMPANION_UNAUTHORIZED");
    };
    if !auth.has_scope(COMPANION_SCOPE_RESPOND) {
        return api_error(StatusCode::FORBIDDEN, "COMPANION_SCOPE_REQUIRED");
    }
    if submission.answers.is_empty()
        || submission.answers.len() > 20
        || submission
            .answers
            .iter()
            .any(|a| a.trim().is_empty() || a.chars().count() > 4000 || a.contains('\0'))
    {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_ANSWER");
    }
    tokio::task::spawn_blocking(move || process_async_answer(&state, id, submission.answers))
        .await
        .unwrap_or_else(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "ANSWER_FAILED"))
}

fn process_async_answer(state: &AppState, id: Uuid, answers: Vec<String>) -> Response {
    let Some(connector) = state.codex.connector.as_ref() else {
        return api_error(StatusCode::CONFLICT, "QUESTION_EXPIRED");
    };
    let batch = {
        let Ok(mut current) = state.codex.state.lock() else {
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, "ANSWER_FAILED");
        };
        let pending = current.async_questions.snapshot(now_millis());
        let Some(batch) = pending.iter().find(|q| q.id == id) else {
            return api_error(StatusCode::CONFLICT, "QUESTION_EXPIRED");
        };
        if batch.questions.len() != answers.len() {
            return api_error(StatusCode::BAD_REQUEST, "INVALID_ANSWER");
        }
        if current.credential_change_handled || !current.managed.contains(&batch.thread_id) {
            return api_error(StatusCode::CONFLICT, "QUESTION_EXPIRED");
        }
        let Some(batch) = current.async_questions.claim(id, now_millis()) else {
            return api_error(StatusCode::CONFLICT, "QUESTION_EXPIRED");
        };
        batch
    };
    let sent = match batch.route {
        ReplyRoute::Rpc { id, question_ids } => {
            let answers = question_ids
                .into_iter()
                .zip(answers)
                .map(|(id, a)| (id, json!({"answers": [a]})))
                .collect::<serde_json::Map<_, _>>();
            connector.respond(id, json!({"answers": answers})).is_ok()
        }
        ReplyRoute::Steer { turn_id } => {
            let text = batch
                .questions
                .iter()
                .zip(answers)
                .map(|(q, a)| format!("{}\n{}", q.title, a))
                .collect::<Vec<_>>()
                .join("\n\n");
            connector
                .call(
                    "turn/steer",
                    json!({
                        "threadId": batch.thread_id, "expectedTurnId": turn_id,
                        "input": [{"type": "text", "text": text}]
                    }),
                    Duration::from_secs(5),
                )
                .is_ok()
        }
        ReplyRoute::Observe => false,
    };
    if let Ok(mut current) = state.codex.state.lock() {
        current.async_questions.finish(id, sent);
    }
    if sent {
        Json(json!({"state": "answer_sent"})).into_response()
    } else {
        api_error(StatusCode::CONFLICT, "QUESTION_EXPIRED")
    }
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

#[derive(Debug, Deserialize)]
struct TaskHistoryQuery {
    limit: Option<usize>,
}

async fn task_history(
    State(state): State<AppState>,
    Query(query): Query<TaskHistoryQuery>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&state, &headers) {
        return api_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED");
    }
    let limit = query.limit.unwrap_or(300);
    if !(1..=500).contains(&limit) {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_HISTORY_LIMIT");
    }
    let now = now_millis();
    match state
        .store
        .task_history(now.saturating_sub(SESSION_LIST_RETENTION_MS), limit)
    {
        Ok(tasks) => Json(json!({
            "schemaVersion": 1,
            "generatedAt": now,
            "tasks": tasks,
        }))
        .into_response(),
        Err(_) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR"),
    }
}

async fn archive_task(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    if session_id.is_empty() || session_id.len() > 256 {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_SESSION_ID");
    }
    match state.store.archive_task(&session_id, now_millis()) {
        Ok(TaskHistoryMutation::Applied) => Json(json!({
            "sessionId": session_id,
            "archived": true,
        }))
        .into_response(),
        Ok(TaskHistoryMutation::NotFound) => api_error(StatusCode::NOT_FOUND, "SESSION_NOT_FOUND"),
        Ok(TaskHistoryMutation::Active) => api_error(StatusCode::CONFLICT, "TASK_STILL_ACTIVE"),
        Err(_) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR"),
    }
}

async fn delete_task_history(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    if session_id.is_empty() || session_id.len() > 256 {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_SESSION_ID");
    }
    match state.store.delete_task_history(&session_id, now_millis()) {
        Ok(TaskHistoryMutation::Applied) => Json(json!({
            "sessionId": session_id,
            "deleted": true,
            "gitChanged": false,
            "providerStopped": false,
        }))
        .into_response(),
        Ok(TaskHistoryMutation::NotFound) => api_error(StatusCode::NOT_FOUND, "SESSION_NOT_FOUND"),
        Ok(TaskHistoryMutation::Active) => api_error(StatusCode::CONFLICT, "TASK_STILL_ACTIVE"),
        Err(_) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR"),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimelineQuery {
    after_ingest_sequence: Option<u64>,
    before_ingest_sequence: Option<u64>,
    limit: Option<usize>,
    latest: Option<bool>,
    current_turn: Option<bool>,
}

async fn session_timeline(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Query(query): Query<TimelineQuery>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&state, &headers) {
        return api_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED");
    }
    if session_id.is_empty() || session_id.len() > 256 {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_SESSION_ID");
    }
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    if query.after_ingest_sequence.is_some() && query.before_ingest_sequence.is_some() {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_TIMELINE_CURSOR");
    }
    let result = if let Some(before) = query.before_ingest_sequence {
        if !query.current_turn.unwrap_or(false) {
            return api_error(StatusCode::BAD_REQUEST, "CURRENT_TURN_REQUIRED");
        }
        state
            .store
            .latest_current_local_timeline_before(&session_id, before, limit)
    } else if query.latest.unwrap_or(false) && query.after_ingest_sequence.is_none() {
        if query.current_turn.unwrap_or(false) {
            state
                .store
                .latest_current_local_timeline(&session_id, limit)
        } else {
            state.store.latest_local_timeline(&session_id, limit)
        }
    } else {
        state
            .store
            .local_timeline(&session_id, query.after_ingest_sequence, limit)
    };
    match result {
        Ok(Some(page)) => Json(page).into_response(),
        Ok(None) => api_error(StatusCode::NOT_FOUND, "SESSION_NOT_FOUND"),
        Err(_) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR"),
    }
}

const REVIEW_SCHEMA_VERSION: u16 = 1;
const REVIEW_GIT_TIMEOUT: Duration = Duration::from_millis(750);
const REVIEW_GIT_MAX_OUTPUT_BYTES: u64 = 1_048_576;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskReviewSnapshot {
    schema_version: u16,
    session_id: String,
    provider: String,
    project_label: Option<String>,
    generated_at: u64,
    turn_started_at: Option<u64>,
    turn_ended_at: Option<u64>,
    outcome: ReviewOutcome,
    repository: ReviewRepository,
    validations: Vec<ReviewValidationRun>,
    last_meaningful_action: Option<ReviewLastAction>,
    limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewOutcome {
    state: String,
    source: String,
    verification: String,
    observed_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewRepository {
    state: String,
    baseline_state: String,
    baseline_captured_at: Option<u64>,
    baseline_head: Option<String>,
    commit_count: Option<u64>,
    branch: Option<String>,
    head: Option<String>,
    worktree_kind: Option<String>,
    dirty: Option<bool>,
    changed_files: Option<u64>,
    staged_files: Option<u64>,
    unstaged_files: Option<u64>,
    untracked_files: Option<u64>,
    insertions: Option<u64>,
    deletions: Option<u64>,
    binary_files: Option<u64>,
    attribution: String,
    attribution_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewValidationRun {
    id: String,
    kind: String,
    state: String,
    source: String,
    tool_name: Option<String>,
    observed_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewLastAction {
    kind: String,
    state: String,
    tool_name: Option<String>,
    observed_at: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewDiffQuery {
    path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewDiffResponse {
    schema_version: u16,
    session_id: String,
    base: Option<String>,
    attribution: String,
    files: Vec<ReviewDiffFile>,
    selected: Option<ReviewDiffPatch>,
    limitation: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewDiffFile {
    path: String,
    state: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewDiffPatch {
    path: String,
    patch: String,
    truncated: bool,
}

#[derive(Debug)]
enum GitReviewError {
    Unavailable,
    TimedOut,
    Failed,
    OutputTooLarge,
}

#[derive(Debug, Default)]
struct GitStatusCounts {
    changed_files: u64,
    staged_files: u64,
    unstaged_files: u64,
    untracked_files: u64,
}

#[derive(Debug, Default)]
struct GitNumstat {
    files: u64,
    insertions: u64,
    deletions: u64,
    binary_files: u64,
}

async fn session_review(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&state, &headers) {
        return api_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED");
    }
    if session_id.is_empty() || session_id.len() > 256 {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_SESSION_ID");
    }
    let context = match state.store.local_review_context(&session_id) {
        Ok(Some(context)) => context,
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "SESSION_NOT_FOUND"),
        Err(_) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR"),
    };
    let timeline = state
        .store
        .latest_current_local_timeline(&session_id, 100)
        .ok()
        .flatten();
    let mut limitations = Vec::new();
    let baseline = context
        .turn_id
        .as_deref()
        .and_then(|turn_id| state.store.review_baseline(turn_id).ok().flatten());
    let mut repository = if let Some(baseline) = baseline.as_ref() {
        inspect_resolved_git_repository(
            &baseline.repository_root,
            context.concurrent_active_sessions,
            &mut limitations,
        )
    } else {
        match context.working_directory.as_deref() {
            Some(working_directory) => inspect_git_repository(
                working_directory,
                context.concurrent_active_sessions,
                &mut limitations,
            ),
            None => {
                limitations.push("working_directory_unavailable".to_owned());
                unavailable_review_repository("working_directory_unavailable")
            }
        }
    };
    if let Some(baseline) = baseline.as_ref() {
        apply_review_baseline(
            &mut repository,
            baseline,
            context.concurrent_active_sessions,
            &mut limitations,
        );
    }
    let validations = timeline
        .as_ref()
        .map(|page| review_validations(&page.events))
        .unwrap_or_default();
    let last_meaningful_action = timeline
        .as_ref()
        .and_then(|page| page.events.iter().rev().find_map(review_last_action));
    let outcome_state = match context.exec_state.as_str() {
        "response_finished" => "completed",
        "failed" => "failed",
        "idle" => "idle",
        _ => "running",
    };
    let outcome_verification = if matches!(outcome_state, "completed" | "failed") {
        "verified"
    } else {
        "not_applicable"
    };
    Json(TaskReviewSnapshot {
        schema_version: REVIEW_SCHEMA_VERSION,
        session_id: context.session_id,
        provider: context.provider,
        project_label: context.project,
        generated_at: now_millis(),
        turn_started_at: context.turn_started_at,
        turn_ended_at: context.turn_ended_at,
        outcome: ReviewOutcome {
            state: outcome_state.to_owned(),
            source: "runtime:terminal_reducer".to_owned(),
            verification: outcome_verification.to_owned(),
            observed_at: context.last_event_at,
        },
        repository,
        validations,
        last_meaningful_action,
        limitations,
    })
    .into_response()
}

async fn session_review_diff(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Query(query): Query<ReviewDiffQuery>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&state, &headers) {
        return api_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED");
    }
    if session_id.is_empty() || session_id.len() > 256 {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_SESSION_ID");
    }
    let context = match state.store.local_review_context(&session_id) {
        Ok(Some(context)) => context,
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "SESSION_NOT_FOUND"),
        Err(_) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR"),
    };
    let baseline = context
        .turn_id
        .as_deref()
        .and_then(|turn_id| state.store.review_baseline(turn_id).ok().flatten());
    let mut limitations = Vec::new();
    let repository_root = if let Some(baseline) = baseline.as_ref() {
        if review_repository_identity(&baseline.repository_root).as_deref()
            == Some(baseline.repository_identity.as_str())
        {
            Some(baseline.repository_root.clone())
        } else {
            limitations.push("repository_identity_changed".to_owned());
            None
        }
    } else {
        context
            .working_directory
            .as_deref()
            .and_then(|working_directory| {
                resolve_review_repository_root(working_directory, &mut limitations)
            })
    };
    let Some(repository_root) = repository_root else {
        return Json(ReviewDiffResponse {
            schema_version: REVIEW_SCHEMA_VERSION,
            session_id,
            base: None,
            attribution: "unavailable".to_owned(),
            files: Vec::new(),
            selected: None,
            limitation: limitations.last().cloned(),
        })
        .into_response();
    };
    let base = baseline
        .as_ref()
        .and_then(|baseline| baseline.head.clone())
        .or_else(|| full_git_head(&repository_root));
    let mut repository = inspect_resolved_git_repository(
        &repository_root,
        context.concurrent_active_sessions,
        &mut limitations,
    );
    if let Some(baseline) = baseline.as_ref() {
        apply_review_baseline(
            &mut repository,
            baseline,
            context.concurrent_active_sessions,
            &mut limitations,
        );
    }
    let (files, file_limitation) = review_diff_files(&repository_root, base.as_deref());
    if let Some(limitation) = file_limitation {
        limitations.push(limitation);
    }
    let selected = query.path.as_deref().and_then(|path| {
        let file = files.iter().find(|file| file.path == path)?;
        if file.state == "untracked" {
            limitations.push("untracked_patch_not_read".to_owned());
            return None;
        }
        let base = base.as_deref()?;
        match review_diff_patch(&repository_root, base, path) {
            Ok(patch) => Some(patch),
            Err(error) => {
                limitations.push(error);
                None
            }
        }
    });
    Json(ReviewDiffResponse {
        schema_version: REVIEW_SCHEMA_VERSION,
        session_id,
        base: base.map(|value| value.chars().take(12).collect()),
        attribution: repository.attribution,
        files,
        selected,
        limitation: limitations.last().cloned(),
    })
    .into_response()
}

fn review_diff_files(
    repository_root: &FilePath,
    base: Option<&str>,
) -> (Vec<ReviewDiffFile>, Option<String>) {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    if let Some(base) = base {
        if let Ok(output) = run_git(
            repository_root,
            &["diff", "--name-only", "-z", "--no-ext-diff", base, "--"],
        ) {
            for raw_path in output.split(|byte| *byte == 0) {
                let Some(path) = safe_review_relative_path(raw_path) else {
                    continue;
                };
                if seen.insert(path.clone()) {
                    files.push(ReviewDiffFile {
                        path,
                        state: "tracked".to_owned(),
                    });
                }
            }
        }
    }
    if let Ok(output) = run_git(
        repository_root,
        &["status", "--porcelain=v2", "-z", "--untracked-files=normal"],
    ) {
        for raw_path in parse_untracked_review_paths(&output) {
            if seen.insert(raw_path.clone()) {
                files.push(ReviewDiffFile {
                    path: raw_path,
                    state: "untracked".to_owned(),
                });
            }
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    if files.len() > 100 {
        files.truncate(100);
        (files, Some("diff_file_limit".to_owned()))
    } else {
        (files, None)
    }
}

fn parse_untracked_review_paths(output: &[u8]) -> Vec<String> {
    output
        .split(|byte| *byte == 0)
        .filter_map(|record| record.strip_prefix(b"? "))
        .filter_map(safe_review_relative_path)
        .collect()
}

fn safe_review_relative_path(value: &[u8]) -> Option<String> {
    let value = std::str::from_utf8(value).ok()?;
    if value.is_empty() || value.len() > 1_024 || value.chars().any(char::is_control) {
        return None;
    }
    let path = FilePath::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return None;
    }
    Some(value.to_owned())
}

fn review_diff_patch(
    repository_root: &FilePath,
    base: &str,
    path: &str,
) -> Result<ReviewDiffPatch, String> {
    let output = run_git(
        repository_root,
        &[
            "diff",
            "--no-ext-diff",
            "--no-color",
            "--unified=3",
            base,
            "--",
            path,
        ],
    )
    .map_err(|error| git_review_error_code(&error).to_owned())?;
    const PATCH_LIMIT: usize = 256 * 1_024;
    let truncated = output.len() > PATCH_LIMIT;
    let bounded = &output[..output.len().min(PATCH_LIMIT)];
    Ok(ReviewDiffPatch {
        path: path.to_owned(),
        patch: String::from_utf8_lossy(bounded).into_owned(),
        truncated,
    })
}

fn apply_review_baseline(
    repository: &mut ReviewRepository,
    baseline: &ReviewBaselineRecord,
    concurrent_active_sessions: u32,
    limitations: &mut Vec<String>,
) {
    limitations.retain(|value| {
        !matches!(
            value.as_str(),
            "turn_baseline_unavailable" | "current_worktree_unattributed"
        )
    });
    repository.baseline_captured_at = Some(baseline.captured_at);
    repository.baseline_head = baseline
        .head
        .as_deref()
        .map(|value| value.chars().take(12).collect());
    if review_repository_identity(&baseline.repository_root).as_deref()
        != Some(baseline.repository_identity.as_str())
    {
        repository.baseline_state = "invalid".to_owned();
        repository.attribution = "unavailable".to_owned();
        repository.attribution_reason = "repository_identity_changed".to_owned();
        limitations.push("repository_identity_changed".to_owned());
        return;
    }
    repository.baseline_state = "available".to_owned();
    let captured_before_first_tool = baseline
        .first_tool_at
        .is_none_or(|first_tool_at| baseline.captured_at <= first_tool_at);
    let (attribution, reason) = if concurrent_active_sessions > 0 {
        (
            "concurrent_changes",
            "multiple_active_sessions_same_worktree",
        )
    } else if !captured_before_first_tool {
        ("bounded_window", "baseline_captured_after_first_tool")
    } else if baseline.dirty {
        ("bounded_window", "baseline_started_dirty")
    } else if baseline.worktree_kind == "linked" {
        ("exact", "independent_clean_worktree_baseline")
    } else {
        ("bounded_window", "clean_turn_baseline")
    };
    repository.attribution = attribution.to_owned();
    repository.attribution_reason = reason.to_owned();
    if attribution != "exact" {
        limitations.push(reason.to_owned());
    }
    let Some(baseline_head) = baseline.head.as_deref() else {
        limitations.push("baseline_head_unavailable".to_owned());
        return;
    };
    if let Ok(output) = run_git(
        &baseline.repository_root,
        &["diff", "--numstat", "--no-ext-diff", baseline_head, "--"],
    ) {
        let delta = parse_git_numstat(&output);
        repository.changed_files = Some(
            delta
                .files
                .saturating_add(repository.untracked_files.unwrap_or_default()),
        );
        repository.insertions = Some(delta.insertions);
        repository.deletions = Some(delta.deletions);
        repository.binary_files = Some(delta.binary_files);
    } else {
        limitations.push("baseline_diff_unavailable".to_owned());
    }
    repository.commit_count = git_commit_count_since(&baseline.repository_root, baseline_head);
}

fn git_commit_count_since(repository_root: &FilePath, baseline_head: &str) -> Option<u64> {
    let range = format!("{baseline_head}..HEAD");
    let output = run_git(repository_root, &["rev-list", "--count", &range]).ok()?;
    let value = String::from_utf8(output).ok()?;
    value.trim().parse::<u64>().ok()
}

fn unavailable_review_repository(reason: &str) -> ReviewRepository {
    ReviewRepository {
        state: "unavailable".to_owned(),
        baseline_state: "unavailable".to_owned(),
        baseline_captured_at: None,
        baseline_head: None,
        commit_count: None,
        branch: None,
        head: None,
        worktree_kind: None,
        dirty: None,
        changed_files: None,
        staged_files: None,
        unstaged_files: None,
        untracked_files: None,
        insertions: None,
        deletions: None,
        binary_files: None,
        attribution: "unavailable".to_owned(),
        attribution_reason: reason.to_owned(),
    }
}

fn inspect_git_repository(
    working_directory: &FilePath,
    concurrent_active_sessions: u32,
    limitations: &mut Vec<String>,
) -> ReviewRepository {
    let Some(repository_root) = resolve_review_repository_root(working_directory, limitations)
    else {
        return ReviewRepository {
            state: "not_git".to_owned(),
            attribution_reason: limitations
                .last()
                .cloned()
                .unwrap_or_else(|| "not_git_repository".to_owned()),
            ..unavailable_review_repository("repository_unavailable")
        };
    };
    inspect_resolved_git_repository(&repository_root, concurrent_active_sessions, limitations)
}

fn resolve_review_repository_root(
    working_directory: &FilePath,
    limitations: &mut Vec<String>,
) -> Option<PathBuf> {
    let Ok(working_directory) = fs::canonicalize(working_directory) else {
        limitations.push("working_directory_unavailable".to_owned());
        return None;
    };
    if !working_directory.is_dir() || working_directory == FilePath::new("/") {
        limitations.push("working_directory_unavailable".to_owned());
        return None;
    }
    let direct_repository = match run_git(&working_directory, &["rev-parse", "--show-toplevel"]) {
        Ok(output) => String::from_utf8(output)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
        Err(GitReviewError::Failed) => None,
        Err(error) => {
            limitations.push(git_review_error_code(&error).to_owned());
            return None;
        }
    };
    let repository_root = direct_repository
        .map(PathBuf::from)
        .or_else(|| select_bounded_nested_repository(&working_directory, limitations));
    if repository_root.is_none()
        && !limitations
            .iter()
            .any(|value| value == "repository_ambiguous")
    {
        limitations.push("not_git_repository".to_owned());
    }
    repository_root
}

fn inspect_resolved_git_repository(
    repository_root: &FilePath,
    concurrent_active_sessions: u32,
    limitations: &mut Vec<String>,
) -> ReviewRepository {
    if !repository_root.is_dir() {
        limitations.push("working_directory_unavailable".to_owned());
        return unavailable_review_repository("working_directory_unavailable");
    }
    let status_output = match run_git(
        repository_root,
        &["status", "--porcelain=v2", "-z", "--untracked-files=normal"],
    ) {
        Ok(output) => output,
        Err(error) => {
            limitations.push(git_review_error_code(&error).to_owned());
            return unavailable_review_repository(git_review_error_code(&error));
        }
    };
    let status = parse_git_status(&status_output);
    let branch = run_git(repository_root, &["branch", "--show-current"])
        .ok()
        .and_then(|output| String::from_utf8(output).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty() && value.len() <= 160);
    let full_head = full_git_head(repository_root);
    let head = full_head
        .as_deref()
        .map(|value| value.chars().take(12).collect::<String>());
    let worktree_kind = git_worktree_kind(repository_root);
    let numstat = if full_head.is_some() {
        match run_git(
            repository_root,
            &["diff", "--numstat", "--no-ext-diff", "HEAD", "--"],
        ) {
            Ok(output) => Some(parse_git_numstat(&output)),
            Err(error) => {
                limitations.push(git_review_error_code(&error).to_owned());
                None
            }
        }
    } else {
        limitations.push("git_head_unavailable".to_owned());
        None
    };
    let dirty = status.changed_files > 0;
    let (attribution, attribution_reason) = if !dirty {
        ("no_changes", "working_tree_clean")
    } else if concurrent_active_sessions > 0 {
        (
            "concurrent_changes",
            "multiple_active_sessions_same_worktree",
        )
    } else {
        ("current_worktree_unattributed", "turn_baseline_unavailable")
    };
    if attribution != "no_changes" {
        limitations.push(attribution_reason.to_owned());
    }
    ReviewRepository {
        state: "available".to_owned(),
        baseline_state: "unavailable".to_owned(),
        baseline_captured_at: None,
        baseline_head: None,
        commit_count: None,
        branch,
        head,
        worktree_kind: Some(worktree_kind),
        dirty: Some(dirty),
        changed_files: Some(status.changed_files),
        staged_files: Some(status.staged_files),
        unstaged_files: Some(status.unstaged_files),
        untracked_files: Some(status.untracked_files),
        insertions: numstat.as_ref().map(|value| value.insertions),
        deletions: numstat.as_ref().map(|value| value.deletions),
        binary_files: numstat.as_ref().map(|value| value.binary_files),
        attribution: attribution.to_owned(),
        attribution_reason: attribution_reason.to_owned(),
    }
}

fn full_git_head(repository_root: &FilePath) -> Option<String> {
    run_git(repository_root, &["rev-parse", "--verify", "HEAD"])
        .ok()
        .and_then(|output| String::from_utf8(output).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn git_worktree_kind(repository_root: &FilePath) -> String {
    run_git(
        repository_root,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-dir",
            "--git-common-dir",
        ],
    )
    .ok()
    .and_then(|output| String::from_utf8(output).ok())
    .map(|output| {
        let mut lines = output.lines().map(str::trim);
        match (lines.next(), lines.next()) {
            (Some(git_dir), Some(common_dir)) if git_dir == common_dir => "primary",
            (Some(_), Some(_)) => "linked",
            _ => "unknown",
        }
        .to_owned()
    })
    .unwrap_or_else(|| "unknown".to_owned())
}

fn review_repository_identity(repository_root: &FilePath) -> Option<String> {
    let root = fs::canonicalize(repository_root).ok()?;
    let common = run_git(
        &root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .ok()?;
    let mut input = root.to_string_lossy().as_bytes().to_vec();
    input.push(0);
    input.extend_from_slice(common.strip_suffix(b"\n").unwrap_or(&common));
    Some(
        digest(&SHA256, &input)
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
}

fn select_bounded_nested_repository(
    working_directory: &FilePath,
    limitations: &mut Vec<String>,
) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    let mut frontier = vec![(working_directory.to_path_buf(), 0_u8)];
    let mut visited = 0_usize;
    while let Some((directory, depth)) = frontier.pop() {
        if depth > 2 || visited >= 128 {
            break;
        }
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            visited = visited.saturating_add(1);
            if visited > 128 {
                break;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() || file_type.is_symlink() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.')
                || matches!(
                    name.as_ref(),
                    "node_modules" | "target" | "outputs" | "build" | "dist"
                )
            {
                continue;
            }
            let path = entry.path();
            if path.join(".git").exists() {
                candidates.push(path.clone());
                if candidates.len() > 16 {
                    limitations.push("repository_scan_limit".to_owned());
                    return None;
                }
            }
            if depth < 2 {
                frontier.push((path, depth.saturating_add(1)));
            }
        }
    }
    if candidates.len() == 1 {
        limitations.push("repository_selected_as_only_nested_git".to_owned());
        return candidates.pop();
    }
    if candidates.is_empty() {
        return None;
    }
    let mut dirty = candidates
        .into_iter()
        .filter(|candidate| {
            run_git(
                candidate,
                &["status", "--porcelain=v2", "-z", "--untracked-files=normal"],
            )
            .ok()
            .is_some_and(|output| parse_git_status(&output).changed_files > 0)
        })
        .collect::<Vec<_>>();
    if dirty.len() == 1 {
        limitations.push("repository_selected_by_unique_dirty_worktree".to_owned());
        dirty.pop()
    } else {
        limitations.push("repository_ambiguous".to_owned());
        None
    }
}

fn git_review_error_code(error: &GitReviewError) -> &'static str {
    match error {
        GitReviewError::Unavailable => "git_unavailable",
        GitReviewError::TimedOut => "git_timed_out",
        GitReviewError::Failed => "git_command_failed",
        GitReviewError::OutputTooLarge => "git_output_too_large",
    }
}

fn run_git(working_directory: &FilePath, arguments: &[&str]) -> Result<Vec<u8>, GitReviewError> {
    let mut child = ProcessCommand::new("/usr/bin/git")
        .arg("-C")
        .arg(working_directory)
        .args(arguments)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| GitReviewError::Unavailable)?;
    let stdout = child.stdout.take().ok_or(GitReviewError::Unavailable)?;
    let reader = thread::spawn(move || {
        let mut output = Vec::new();
        let result = stdout
            .take(REVIEW_GIT_MAX_OUTPUT_BYTES.saturating_add(1))
            .read_to_end(&mut output);
        (result, output)
    });
    let deadline = Instant::now() + REVIEW_GIT_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(GitReviewError::TimedOut);
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(GitReviewError::Failed);
            }
        }
    };
    let (read_result, output) = reader.join().map_err(|_| GitReviewError::Failed)?;
    read_result.map_err(|_| GitReviewError::Failed)?;
    if !status.success() {
        return Err(GitReviewError::Failed);
    }
    if u64::try_from(output.len()).unwrap_or(u64::MAX) > REVIEW_GIT_MAX_OUTPUT_BYTES {
        return Err(GitReviewError::OutputTooLarge);
    }
    Ok(output)
}

fn run_git_with_input(
    working_directory: &FilePath,
    arguments: &[&str],
    input: &[u8],
) -> Result<Vec<u8>, GitReviewError> {
    if u64::try_from(input.len()).unwrap_or(u64::MAX) > REVIEW_GIT_MAX_OUTPUT_BYTES {
        return Err(GitReviewError::OutputTooLarge);
    }
    let mut child = ProcessCommand::new("/usr/bin/git")
        .arg("-C")
        .arg(working_directory)
        .args(arguments)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| GitReviewError::Unavailable)?;
    let mut stdin = child.stdin.take().ok_or(GitReviewError::Unavailable)?;
    stdin.write_all(input).map_err(|_| GitReviewError::Failed)?;
    drop(stdin);
    let stdout = child.stdout.take().ok_or(GitReviewError::Unavailable)?;
    let reader = thread::spawn(move || {
        let mut output = Vec::new();
        let result = stdout
            .take(REVIEW_GIT_MAX_OUTPUT_BYTES.saturating_add(1))
            .read_to_end(&mut output);
        (result, output)
    });
    let deadline = Instant::now() + REVIEW_GIT_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(GitReviewError::TimedOut);
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(GitReviewError::Failed);
            }
        }
    };
    let (read_result, output) = reader.join().map_err(|_| GitReviewError::Failed)?;
    read_result.map_err(|_| GitReviewError::Failed)?;
    if !status.success() {
        return Err(GitReviewError::Failed);
    }
    if u64::try_from(output.len()).unwrap_or(u64::MAX) > REVIEW_GIT_MAX_OUTPUT_BYTES {
        return Err(GitReviewError::OutputTooLarge);
    }
    Ok(output)
}

fn parse_git_status(output: &[u8]) -> GitStatusCounts {
    let records = output.split(|byte| *byte == 0).collect::<Vec<_>>();
    let mut counts = GitStatusCounts::default();
    let mut index = 0;
    while index < records.len() {
        let record = records[index];
        if record.starts_with(b"? ") {
            counts.changed_files = counts.changed_files.saturating_add(1);
            counts.untracked_files = counts.untracked_files.saturating_add(1);
        } else if (record.starts_with(b"1 ") || record.starts_with(b"2 ")) && record.len() >= 4 {
            counts.changed_files = counts.changed_files.saturating_add(1);
            if record[2] != b'.' {
                counts.staged_files = counts.staged_files.saturating_add(1);
            }
            if record[3] != b'.' {
                counts.unstaged_files = counts.unstaged_files.saturating_add(1);
            }
            if record.starts_with(b"2 ") {
                index = index.saturating_add(1);
            }
        } else if record.starts_with(b"u ") {
            counts.changed_files = counts.changed_files.saturating_add(1);
            counts.staged_files = counts.staged_files.saturating_add(1);
            counts.unstaged_files = counts.unstaged_files.saturating_add(1);
        }
        index = index.saturating_add(1);
    }
    counts
}

fn parse_git_numstat(output: &[u8]) -> GitNumstat {
    let mut totals = GitNumstat::default();
    for line in output.split(|byte| *byte == b'\n') {
        let mut fields = line.split(|byte| *byte == b'\t');
        let Some(insertions) = fields.next() else {
            continue;
        };
        let Some(deletions) = fields.next() else {
            continue;
        };
        totals.files = totals.files.saturating_add(1);
        if insertions == b"-" || deletions == b"-" {
            totals.binary_files = totals.binary_files.saturating_add(1);
            continue;
        }
        totals.insertions = totals
            .insertions
            .saturating_add(parse_ascii_u64(insertions));
        totals.deletions = totals.deletions.saturating_add(parse_ascii_u64(deletions));
    }
    totals
}

fn parse_ascii_u64(value: &[u8]) -> u64 {
    if value.is_empty() || value.iter().any(|byte| !byte.is_ascii_digit()) {
        return 0;
    }
    value.iter().fold(0_u64, |result, byte| {
        result
            .saturating_mul(10)
            .saturating_add(u64::from(byte.saturating_sub(b'0')))
    })
}

fn review_validations(
    events: &[actrealm_runtime::TimelineEventRecord],
) -> Vec<ReviewValidationRun> {
    let mut runs = HashMap::<String, ReviewValidationRun>::new();
    for event in events {
        let kind = match event.tool_category.as_deref() {
            Some("test") => "test",
            Some("build") => "build",
            _ => continue,
        };
        let state = match event.kind {
            TimelineEventKind::ToolCompleted => event
                .validation_status
                .as_deref()
                .filter(|value| matches!(*value, "passed" | "failed" | "unverifiable"))
                .unwrap_or("unverifiable"),
            TimelineEventKind::ToolFailed => "failed",
            TimelineEventKind::ToolStarted => "running",
            _ => continue,
        };
        let key = match event.tool_call_id.as_deref() {
            Some(tool_call_id) => tool_call_id.to_owned(),
            None if matches!(event.kind, TimelineEventKind::ToolStarted) => continue,
            None => event.event_id.clone(),
        };
        runs.insert(
            key,
            ReviewValidationRun {
                id: event.event_id.clone(),
                kind: kind.to_owned(),
                state: state.to_owned(),
                source: "runtime:structured_tool_lifecycle".to_owned(),
                tool_name: event.tool_name.clone(),
                observed_at: event.occurred_at,
            },
        );
    }
    let mut runs = runs.into_values().collect::<Vec<_>>();
    runs.sort_by_key(|run| run.observed_at);
    let drain = runs.len().saturating_sub(10);
    runs.drain(0..drain);
    runs
}

fn review_last_action(event: &actrealm_runtime::TimelineEventRecord) -> Option<ReviewLastAction> {
    if matches!(event.kind, TimelineEventKind::PlanUpdated) {
        return None;
    }
    Some(ReviewLastAction {
        kind: review_event_kind(event.kind).to_owned(),
        state: review_event_state(event.kind).to_owned(),
        tool_name: event.tool_name.clone(),
        observed_at: event.occurred_at,
    })
}

fn review_event_kind(kind: TimelineEventKind) -> &'static str {
    match kind {
        TimelineEventKind::SessionStarted => "session.started",
        TimelineEventKind::SessionEnded => "session.ended",
        TimelineEventKind::TurnStarted => "turn.started",
        TimelineEventKind::TurnCompleted => "turn.completed",
        TimelineEventKind::TurnInterrupted => "turn.interrupted",
        TimelineEventKind::TurnFailed => "turn.failed",
        TimelineEventKind::ToolStarted => "tool.started",
        TimelineEventKind::ToolCompleted => "tool.completed",
        TimelineEventKind::ToolFailed => "tool.failed",
        TimelineEventKind::ApprovalRequested => "approval.requested",
        TimelineEventKind::ApprovalResolved => "approval.resolved",
        TimelineEventKind::QuestionRequested => "question.requested",
        TimelineEventKind::ElicitationRequested => "elicitation.requested",
        TimelineEventKind::SubagentStarted => "subagent.started",
        TimelineEventKind::SubagentCompleted => "subagent.completed",
        TimelineEventKind::TaskCreated => "task.created",
        TimelineEventKind::TaskCompleted => "task.completed",
        TimelineEventKind::PlanUpdated => "plan.updated",
        TimelineEventKind::SessionCompacting => "session.compacting",
    }
}

fn review_event_state(kind: TimelineEventKind) -> &'static str {
    match kind {
        TimelineEventKind::ToolFailed | TimelineEventKind::TurnFailed => "failed",
        TimelineEventKind::ToolCompleted
        | TimelineEventKind::TurnCompleted
        | TimelineEventKind::TaskCompleted
        | TimelineEventKind::SessionEnded
        | TimelineEventKind::ApprovalResolved
        | TimelineEventKind::SubagentCompleted => "completed",
        TimelineEventKind::ApprovalRequested
        | TimelineEventKind::QuestionRequested
        | TimelineEventKind::ElicitationRequested => "requested",
        TimelineEventKind::TurnInterrupted => "interrupted",
        TimelineEventKind::PlanUpdated => "updated",
        _ => "running",
    }
}

const CHECKPOINT_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateCheckpointRequest {
    kind: String,
    label: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CheckpointPreflightQuery {
    action: String,
}

#[derive(Debug, Deserialize)]
struct CheckpointActionRequest {
    action: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckpointRepositoryView {
    state: String,
    branch: Option<String>,
    head: Option<String>,
    worktree_kind: Option<String>,
    dirty: Option<bool>,
    changed_files: Option<u64>,
    staged_files: Option<u64>,
    unstaged_files: Option<u64>,
    untracked_files: Option<u64>,
    git_snapshot: bool,
    git_object: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskCheckpointView {
    schema_version: u16,
    id: String,
    session_id: String,
    turn_id: String,
    label: Option<String>,
    kind: String,
    provider: String,
    provider_resume_capability: String,
    created_at: u64,
    repository: CheckpointRepositoryView,
    validations: Vec<ReviewValidationRun>,
    validation_is_historical: bool,
    limitations: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckpointPreflightView {
    schema_version: u16,
    checkpoint_id: String,
    action: String,
    allowed: bool,
    blockers: Vec<String>,
    warnings: Vec<String>,
    current_branch: Option<String>,
    current_head: Option<String>,
    current_dirty: Option<bool>,
    current_changed_files: Option<u64>,
    validation_is_historical: bool,
}

struct CheckpointPreflight {
    view: CheckpointPreflightView,
    patch: Option<Vec<u8>>,
}

async fn session_checkpoints(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&state, &headers) {
        return api_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED");
    }
    if session_id.is_empty() || session_id.len() > 256 {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_SESSION_ID");
    }
    match state.store.task_checkpoints(&session_id) {
        Ok(checkpoints) => Json(json!({
            "schemaVersion": CHECKPOINT_SCHEMA_VERSION,
            "sessionId": session_id,
            "checkpoints": checkpoints.iter().map(checkpoint_view).collect::<Vec<_>>()
        }))
        .into_response(),
        Err(error) => store_error_response(error),
    }
}

async fn task_checkpoint(
    State(state): State<AppState>,
    Path(checkpoint_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&state, &headers) {
        return api_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED");
    }
    match state.store.task_checkpoint(&checkpoint_id) {
        Ok(Some(checkpoint)) => Json(checkpoint_view(&checkpoint)).into_response(),
        Ok(None) => api_error(StatusCode::NOT_FOUND, "CHECKPOINT_NOT_FOUND"),
        Err(error) => store_error_response(error),
    }
}

async fn create_session_checkpoint(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<CreateCheckpointRequest>,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    if !matches!(request.kind.as_str(), "metadata" | "git_snapshot") {
        return api_error(StatusCode::BAD_REQUEST, "CHECKPOINT_INVALID");
    }
    if request.label.as_ref().is_some_and(|label| {
        label.trim().is_empty() || label.chars().count() > 80 || label.chars().any(char::is_control)
    }) {
        return api_error(StatusCode::BAD_REQUEST, "CHECKPOINT_INVALID");
    }
    let context = match state.store.local_review_context(&session_id) {
        Ok(Some(context)) => context,
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "SESSION_NOT_FOUND"),
        Err(error) => return store_error_response(error),
    };
    let Some(turn_id) = context.turn_id.clone() else {
        return api_error(StatusCode::CONFLICT, "CURRENT_TURN_REQUIRED");
    };
    let checkpoint_id = Uuid::now_v7().to_string();
    let timeline = state
        .store
        .latest_current_local_timeline(&session_id, 100)
        .ok()
        .flatten();
    let validations = timeline
        .as_ref()
        .map(|page| review_validations(&page.events))
        .unwrap_or_default();
    let validation_json = match serde_json::to_string(&validations) {
        Ok(value) => value,
        Err(_) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR"),
    };
    let baseline = state.store.review_baseline(&turn_id).ok().flatten();
    let repository_root = baseline
        .as_ref()
        .map(|baseline| baseline.repository_root.clone())
        .or_else(|| {
            context
                .working_directory
                .as_deref()
                .and_then(|working_directory| {
                    resolve_review_repository_root(working_directory, &mut Vec::new())
                })
        });
    let mut repository_identity = None;
    let mut branch = None;
    let mut head = None;
    let mut worktree_kind = None;
    let mut dirty = None;
    let mut changed_files = None;
    let mut staged_files = None;
    let mut unstaged_files = None;
    let mut untracked_files = None;
    let mut git_object_id = None;
    let mut git_ref = None;
    let mut patch_digest = None;
    if let Some(root) = repository_root.as_ref() {
        repository_identity = review_repository_identity(root);
        branch = run_git(root, &["branch", "--show-current"])
            .ok()
            .and_then(|output| String::from_utf8(output).ok())
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        head = full_git_head(root);
        worktree_kind = Some(git_worktree_kind(root));
        if let Ok(output) = run_git(
            root,
            &["status", "--porcelain=v2", "-z", "--untracked-files=normal"],
        ) {
            let status = parse_git_status(&output);
            dirty = Some(status.changed_files > 0);
            changed_files = Some(status.changed_files);
            staged_files = Some(status.staged_files);
            unstaged_files = Some(status.unstaged_files);
            untracked_files = Some(status.untracked_files);
        }
        if request.kind == "git_snapshot" {
            let Some(base_head) = head.as_deref() else {
                return api_error(StatusCode::CONFLICT, "CHECKPOINT_GIT_FAILED");
            };
            let snapshot = match run_git(
                root,
                &[
                    "stash",
                    "create",
                    &format!("ActRealm checkpoint {checkpoint_id}"),
                ],
            ) {
                Ok(output) => String::from_utf8(output)
                    .ok()
                    .map(|value| value.trim().to_owned())
                    .filter(|value| {
                        value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
                    }),
                Err(_) => None,
            };
            let Some(snapshot) = snapshot else {
                return api_error_detail(
                    StatusCode::CONFLICT,
                    "CHECKPOINT_GIT_FAILED",
                    "No tracked Git changes were available for a snapshot",
                );
            };
            let checkpoint_ref = format!("refs/actrealm/checkpoints/{checkpoint_id}");
            if run_git(root, &["update-ref", &checkpoint_ref, &snapshot]).is_err() {
                return api_error(StatusCode::CONFLICT, "CHECKPOINT_GIT_FAILED");
            }
            let patch = match run_git(root, &["diff", "--binary", base_head, &snapshot, "--"]) {
                Ok(patch) if !patch.is_empty() => patch,
                _ => {
                    let _ = run_git(root, &["update-ref", "-d", &checkpoint_ref, &snapshot]);
                    return api_error(StatusCode::CONFLICT, "CHECKPOINT_GIT_FAILED");
                }
            };
            patch_digest = Some(sha256_hex(&patch));
            git_object_id = Some(snapshot);
            git_ref = Some(checkpoint_ref);
        }
    } else if request.kind == "git_snapshot" {
        return api_error(StatusCode::CONFLICT, "CHECKPOINT_GIT_FAILED");
    }
    let cleanup_repository_root = repository_root.clone();
    let input = TaskCheckpointInput {
        id: checkpoint_id.clone(),
        session_id: session_id.clone(),
        turn_id,
        label: request.label,
        kind: request.kind,
        provider: context.provider,
        provider_session_id: context.provider_session_id,
        provider_resume_capability: context.jump_capability,
        repository_root,
        repository_identity,
        branch,
        head,
        worktree_kind,
        dirty,
        changed_files,
        staged_files,
        unstaged_files,
        untracked_files,
        git_object_id: git_object_id.clone(),
        git_ref: git_ref.clone(),
        patch_digest,
        validation_json,
        review_baseline_captured_at: baseline.map(|baseline| baseline.captured_at),
        created_at: now_millis(),
    };
    match state.store.create_task_checkpoint(input) {
        Ok(checkpoint) => Json(checkpoint_view(&checkpoint)).into_response(),
        Err(error) => {
            if let (Some(root), Some(reference), Some(object)) = (
                cleanup_repository_root,
                git_ref.as_deref(),
                git_object_id.as_deref(),
            ) {
                let _ = run_git(&root, &["update-ref", "-d", reference, object]);
            }
            store_error_response(error)
        }
    }
}

async fn checkpoint_preflight(
    State(state): State<AppState>,
    Path(checkpoint_id): Path<String>,
    Query(query): Query<CheckpointPreflightQuery>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&state, &headers) {
        return api_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED");
    }
    let checkpoint = match state.store.task_checkpoint(&checkpoint_id) {
        Ok(Some(checkpoint)) => checkpoint,
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "CHECKPOINT_NOT_FOUND"),
        Err(error) => return store_error_response(error),
    };
    match checkpoint_preflight_for(&checkpoint, &query.action) {
        Ok(mut preflight) => {
            apply_checkpoint_session_availability(&state, &checkpoint, &mut preflight);
            Json(preflight.view).into_response()
        }
        Err(_) => api_error(StatusCode::BAD_REQUEST, "CHECKPOINT_INVALID"),
    }
}

async fn checkpoint_action(
    State(state): State<AppState>,
    Path(checkpoint_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<CheckpointActionRequest>,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    let checkpoint = match state.store.task_checkpoint(&checkpoint_id) {
        Ok(Some(checkpoint)) => checkpoint,
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "CHECKPOINT_NOT_FOUND"),
        Err(error) => return store_error_response(error),
    };
    if request.action == "resume_session" {
        let mut preflight = match checkpoint_preflight_for(&checkpoint, &request.action) {
            Ok(preflight) => preflight,
            Err(_) => return api_error(StatusCode::BAD_REQUEST, "CHECKPOINT_INVALID"),
        };
        apply_checkpoint_session_availability(&state, &checkpoint, &mut preflight);
        if !preflight.view.allowed {
            return api_error_detail(
                StatusCode::CONFLICT,
                "CHECKPOINT_PREFLIGHT_FAILED",
                &preflight.view.blockers.join(","),
            );
        }
        return process_jump_session(&state, &checkpoint.session_id);
    }
    let preflight = match checkpoint_preflight_for(&checkpoint, &request.action) {
        Ok(preflight) => preflight,
        Err(_) => return api_error(StatusCode::BAD_REQUEST, "CHECKPOINT_INVALID"),
    };
    if !preflight.view.allowed {
        return api_error_detail(
            StatusCode::CONFLICT,
            "CHECKPOINT_PREFLIGHT_FAILED",
            &preflight.view.blockers.join(","),
        );
    }
    let Some(root) = checkpoint.repository_root.as_deref() else {
        return api_error(StatusCode::CONFLICT, "CHECKPOINT_PREFLIGHT_FAILED");
    };
    let Some(patch) = preflight.patch.as_deref() else {
        return api_error(StatusCode::CONFLICT, "CHECKPOINT_PREFLIGHT_FAILED");
    };
    let arguments: &[&str] = match request.action.as_str() {
        "restore_code" => &["apply", "--binary"],
        "rollback_code" => &["apply", "--binary", "--reverse"],
        _ => return api_error(StatusCode::BAD_REQUEST, "CHECKPOINT_INVALID"),
    };
    if run_git_with_input(root, arguments, patch).is_err() {
        return api_error(StatusCode::CONFLICT, "CHECKPOINT_GIT_FAILED");
    }
    let status = run_git(
        root,
        &["status", "--porcelain=v2", "-z", "--untracked-files=normal"],
    )
    .ok()
    .map(|output| parse_git_status(&output));
    Json(json!({
        "success": true,
        "checkpointId": checkpoint.id,
        "action": request.action,
        "repository": status.map(|status| json!({
            "dirty": status.changed_files > 0,
            "changedFiles": status.changed_files,
            "stagedFiles": status.staged_files,
            "unstagedFiles": status.unstaged_files,
            "untrackedFiles": status.untracked_files,
        })),
        "validationState": "historical"
    }))
    .into_response()
}

async fn delete_checkpoint(
    State(state): State<AppState>,
    Path(checkpoint_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    let checkpoint = match state.store.task_checkpoint(&checkpoint_id) {
        Ok(Some(checkpoint)) => checkpoint,
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "CHECKPOINT_NOT_FOUND"),
        Err(error) => return store_error_response(error),
    };
    match state.store.delete_task_checkpoint(&checkpoint_id) {
        Ok(true) => {
            if let (Some(root), Some(reference), Some(object)) = (
                checkpoint.repository_root.as_deref(),
                checkpoint.git_ref.as_deref(),
                checkpoint.git_object_id.as_deref(),
            ) {
                let _ = run_git(root, &["update-ref", "-d", reference, object]);
            }
            Json(json!({ "deleted": true, "checkpointId": checkpoint_id })).into_response()
        }
        Ok(false) => api_error(StatusCode::NOT_FOUND, "CHECKPOINT_NOT_FOUND"),
        Err(error) => store_error_response(error),
    }
}

fn checkpoint_view(checkpoint: &TaskCheckpointRecord) -> TaskCheckpointView {
    let validations = serde_json::from_str::<Vec<ReviewValidationRun>>(&checkpoint.validation_json)
        .unwrap_or_default();
    let mut limitations = Vec::new();
    if checkpoint.repository_root.is_none() {
        limitations.push("repository_unavailable".to_owned());
    }
    if checkpoint.kind == "git_snapshot" && checkpoint.untracked_files.unwrap_or_default() > 0 {
        limitations.push("untracked_not_captured".to_owned());
    }
    if checkpoint.provider_resume_capability == "unsupported" {
        limitations.push("provider_resume_unsupported".to_owned());
    }
    TaskCheckpointView {
        schema_version: CHECKPOINT_SCHEMA_VERSION,
        id: checkpoint.id.clone(),
        session_id: checkpoint.session_id.clone(),
        turn_id: checkpoint.turn_id.clone(),
        label: checkpoint.label.clone(),
        kind: checkpoint.kind.clone(),
        provider: checkpoint.provider.clone(),
        provider_resume_capability: checkpoint.provider_resume_capability.clone(),
        created_at: checkpoint.created_at,
        repository: CheckpointRepositoryView {
            state: if checkpoint.repository_root.is_some() {
                "available".to_owned()
            } else {
                "unavailable".to_owned()
            },
            branch: checkpoint.branch.clone(),
            head: checkpoint
                .head
                .as_deref()
                .map(|value| value.chars().take(12).collect()),
            worktree_kind: checkpoint.worktree_kind.clone(),
            dirty: checkpoint.dirty,
            changed_files: checkpoint.changed_files,
            staged_files: checkpoint.staged_files,
            unstaged_files: checkpoint.unstaged_files,
            untracked_files: checkpoint.untracked_files,
            git_snapshot: checkpoint.git_object_id.is_some(),
            git_object: checkpoint
                .git_object_id
                .as_deref()
                .map(|value| value.chars().take(12).collect()),
        },
        validations,
        validation_is_historical: true,
        limitations,
    }
}

fn checkpoint_preflight_for(
    checkpoint: &TaskCheckpointRecord,
    action: &str,
) -> Result<CheckpointPreflight, ()> {
    if action == "resume_session" {
        let blockers = if checkpoint.provider_resume_capability == "unsupported" {
            vec!["provider_resume_unsupported".to_owned()]
        } else {
            Vec::new()
        };
        return Ok(CheckpointPreflight {
            view: CheckpointPreflightView {
                schema_version: CHECKPOINT_SCHEMA_VERSION,
                checkpoint_id: checkpoint.id.clone(),
                action: action.to_owned(),
                allowed: blockers.is_empty(),
                blockers,
                warnings: vec!["code_state_not_changed".to_owned()],
                current_branch: None,
                current_head: None,
                current_dirty: None,
                current_changed_files: None,
                validation_is_historical: true,
            },
            patch: None,
        });
    }
    if !matches!(action, "restore_code" | "rollback_code") {
        return Err(());
    }
    let mut blockers = Vec::new();
    let mut warnings = vec!["validation_is_historical".to_owned()];
    if checkpoint.untracked_files.unwrap_or_default() > 0 {
        warnings.push("untracked_not_captured".to_owned());
    }
    let Some(root) = checkpoint.repository_root.as_deref() else {
        blockers.push("repository_unavailable".to_owned());
        return Ok(empty_checkpoint_preflight(
            checkpoint, action, blockers, warnings,
        ));
    };
    if !root.is_dir() {
        blockers.push("repository_unavailable".to_owned());
    }
    if review_repository_identity(root).as_deref() != checkpoint.repository_identity.as_deref() {
        blockers.push("repository_identity_changed".to_owned());
    }
    let current_branch = run_git(root, &["branch", "--show-current"])
        .ok()
        .and_then(|output| String::from_utf8(output).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let current_head = full_git_head(root);
    if current_branch != checkpoint.branch {
        blockers.push("branch_changed".to_owned());
    }
    if current_head != checkpoint.head {
        blockers.push("head_changed".to_owned());
    }
    let status = run_git(
        root,
        &["status", "--porcelain=v2", "-z", "--untracked-files=normal"],
    )
    .ok()
    .map(|output| parse_git_status(&output));
    let current_dirty = status.as_ref().map(|status| status.changed_files > 0);
    let current_changed_files = status.as_ref().map(|status| status.changed_files);
    let (Some(base_head), Some(git_object), Some(expected_digest)) = (
        checkpoint.head.as_deref(),
        checkpoint.git_object_id.as_deref(),
        checkpoint.patch_digest.as_deref(),
    ) else {
        blockers.push("git_snapshot_unavailable".to_owned());
        return Ok(CheckpointPreflight {
            view: CheckpointPreflightView {
                schema_version: CHECKPOINT_SCHEMA_VERSION,
                checkpoint_id: checkpoint.id.clone(),
                action: action.to_owned(),
                allowed: false,
                blockers,
                warnings,
                current_branch,
                current_head,
                current_dirty,
                current_changed_files,
                validation_is_historical: true,
            },
            patch: None,
        });
    };
    let patch = run_git(root, &["diff", "--binary", base_head, git_object, "--"]).ok();
    if patch.as_deref().map(sha256_hex).as_deref() != Some(expected_digest) {
        blockers.push("git_snapshot_changed".to_owned());
    }
    if action == "restore_code" {
        if current_dirty == Some(true) {
            blockers.push("working_tree_dirty".to_owned());
        }
        if blockers.is_empty()
            && patch.as_deref().is_none_or(|patch| {
                run_git_with_input(root, &["apply", "--check", "--binary"], patch).is_err()
            })
        {
            blockers.push("patch_conflict".to_owned());
        }
    } else {
        if status
            .as_ref()
            .is_some_and(|status| status.untracked_files > 0)
        {
            blockers.push("untracked_changes_present".to_owned());
        }
        let current_patch = run_git(root, &["diff", "--binary", base_head, "--"]).ok();
        if current_patch.as_deref().map(sha256_hex).as_deref() != Some(expected_digest) {
            blockers.push("working_tree_not_checkpoint".to_owned());
        }
        if blockers.is_empty()
            && patch.as_deref().is_none_or(|patch| {
                run_git_with_input(root, &["apply", "--check", "--binary", "--reverse"], patch)
                    .is_err()
            })
        {
            blockers.push("patch_conflict".to_owned());
        }
    }
    blockers.sort();
    blockers.dedup();
    Ok(CheckpointPreflight {
        view: CheckpointPreflightView {
            schema_version: CHECKPOINT_SCHEMA_VERSION,
            checkpoint_id: checkpoint.id.clone(),
            action: action.to_owned(),
            allowed: blockers.is_empty(),
            blockers,
            warnings,
            current_branch,
            current_head,
            current_dirty,
            current_changed_files,
            validation_is_historical: true,
        },
        patch,
    })
}

fn apply_checkpoint_session_availability(
    state: &AppState,
    checkpoint: &TaskCheckpointRecord,
    preflight: &mut CheckpointPreflight,
) {
    if preflight.view.action != "resume_session" {
        return;
    }
    let available = state.store.snapshot().is_ok_and(|snapshot| {
        snapshot
            .sessions
            .iter()
            .any(|session| session.id == checkpoint.session_id)
    });
    if !available {
        preflight
            .view
            .blockers
            .push("provider_session_unavailable".to_owned());
        preflight.view.allowed = false;
    }
}

fn empty_checkpoint_preflight(
    checkpoint: &TaskCheckpointRecord,
    action: &str,
    blockers: Vec<String>,
    warnings: Vec<String>,
) -> CheckpointPreflight {
    CheckpointPreflight {
        view: CheckpointPreflightView {
            schema_version: CHECKPOINT_SCHEMA_VERSION,
            checkpoint_id: checkpoint.id.clone(),
            action: action.to_owned(),
            allowed: false,
            blockers,
            warnings,
            current_branch: None,
            current_head: None,
            current_dirty: None,
            current_changed_files: None,
            validation_is_historical: true,
        },
        patch: None,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    digest(&SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
    if !authorized_setup(&state, &headers, true) {
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
    display_fields_version: u32,
    quota_display_mode: String,
    token_usage_display_mode: String,
    token_usage_components_visible: bool,
    token_usage_heatmap_visible: bool,
    token_usage_cost_visible: bool,
    token_usage_observed_time_visible: bool,
    token_usage_execution_time_visible: bool,
    token_usage_unit_style: String,
    token_usage_task_project_visible: bool,
    token_usage_burn_rate_visible: bool,
    token_usage_anomaly_visible: bool,
    token_threshold_notifications_enabled: bool,
    token_threshold_tokens_per_minute: u64,
    completion_task_hide_mode: String,
    completion_auto_hide_minutes: u32,
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
                "sessionTokens".to_owned(),
                "turnTokens".to_owned(),
                "inputOutputTokens".to_owned(),
                "cacheTokens".to_owned(),
                "reasoningTokens".to_owned(),
                "cost".to_owned(),
                "context".to_owned(),
                "tool".to_owned(),
                "currentTarget".to_owned(),
                "subagents".to_owned(),
                "environment".to_owned(),
                "recovery".to_owned(),
                "control".to_owned(),
                "jump".to_owned(),
                "taskFlow".to_owned(),
                "workflow".to_owned(),
            ],
            display_fields_version: 5,
            quota_display_mode: "standard".to_owned(),
            token_usage_display_mode: "standard".to_owned(),
            token_usage_components_visible: true,
            token_usage_heatmap_visible: true,
            token_usage_cost_visible: true,
            token_usage_observed_time_visible: true,
            token_usage_execution_time_visible: true,
            token_usage_unit_style: "automatic".to_owned(),
            token_usage_task_project_visible: true,
            token_usage_burn_rate_visible: true,
            token_usage_anomaly_visible: true,
            token_threshold_notifications_enabled: false,
            token_threshold_tokens_per_minute: 250_000,
            completion_task_hide_mode: "afterConfirmation".to_owned(),
            completion_auto_hide_minutes: 30,
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
            "concise" | "detailed" | "developer" | "custom"
        ) {
            return Err("displayProfile must be concise, detailed, developer, or custom");
        }
        if !matches!(
            self.quota_display_mode.as_str(),
            "standard" | "twoLine" | "compact"
        ) {
            return Err("quotaDisplayMode must be standard, twoLine, or compact");
        }
        if !matches!(
            self.token_usage_display_mode.as_str(),
            "standard" | "compact" | "hidden"
        ) {
            return Err("tokenUsageDisplayMode must be standard, compact, or hidden");
        }
        if !matches!(
            self.token_usage_unit_style.as_str(),
            "automatic" | "western" | "eastAsian"
        ) {
            return Err("tokenUsageUnitStyle must be automatic, western, or eastAsian");
        }
        if !matches!(
            self.token_threshold_tokens_per_minute,
            50_000 | 100_000 | 250_000 | 500_000 | 1_000_000
        ) {
            return Err(
                "tokenThresholdTokensPerMinute must be 50000, 100000, 250000, 500000, or 1000000",
            );
        }
        if !matches!(
            self.completion_task_hide_mode.as_str(),
            "afterConfirmation" | "afterDelay" | "manual"
        ) {
            return Err("completionTaskHideMode must be afterConfirmation, afterDelay, or manual");
        }
        if !matches!(self.completion_auto_hide_minutes, 5 | 15 | 30 | 60) {
            return Err("completionAutoHideMinutes must be 5, 15, 30, or 60");
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
    Json(mut next): Json<UiSettings>,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    let source_display_fields_version = next.display_fields_version;
    migrate_display_fields(&mut next, Some(source_display_fields_version));
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
    if state
        .store
        .write_ui_settings(encoded, now_millis())
        .is_err()
    {
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
        Some(value) => decode_ui_settings(&value),
        None => Ok(UiSettings::default()),
    }
}

fn decode_ui_settings(encoded: &str) -> Result<UiSettings, StoreError> {
    let raw = serde_json::from_str::<Value>(encoded)
        .map_err(|error| StoreError::Storage(format!("settings JSON is invalid: {error}")))?;
    let source_version = raw.get("displayFieldsVersion").and_then(Value::as_u64);
    let mut settings = serde_json::from_value::<UiSettings>(raw)
        .map_err(|error| StoreError::Storage(format!("settings JSON is invalid: {error}")))?;

    migrate_display_fields(&mut settings, source_version.map(|version| version as u32));

    Ok(settings)
}

fn migrate_display_fields(settings: &mut UiSettings, source_version: Option<u32>) {
    if source_version.unwrap_or(1) < 2 {
        if let Some(token_index) = settings
            .task_card_fields
            .iter()
            .position(|field| field == "tokens")
        {
            if !settings
                .task_card_fields
                .iter()
                .any(|field| field == "cost")
            {
                settings
                    .task_card_fields
                    .insert(token_index.saturating_add(1), "cost".to_owned());
            }
        }
    }

    if let Some(token_index) = settings
        .task_card_fields
        .iter()
        .position(|field| field == "tokens")
    {
        settings.task_card_fields.remove(token_index);
        let replacement_fields = [
            "sessionTokens",
            "turnTokens",
            "inputOutputTokens",
            "cacheTokens",
            "reasoningTokens",
        ]
        .into_iter()
        .filter(|field| !settings.task_card_fields.iter().any(|item| item == field))
        .map(str::to_owned)
        .collect::<Vec<_>>();
        for (offset, field) in replacement_fields.into_iter().enumerate() {
            settings
                .task_card_fields
                .insert(token_index.saturating_add(offset), field);
        }
    }

    if source_version.unwrap_or(1) < 4 {
        if !settings
            .task_card_fields
            .iter()
            .any(|field| field == "taskFlow")
            && settings
                .task_card_fields
                .iter()
                .any(|field| field == "plan")
        {
            settings.task_card_fields.push("taskFlow".to_owned());
        }
        if !settings
            .task_card_fields
            .iter()
            .any(|field| field == "workflow")
        {
            settings.task_card_fields.push("workflow".to_owned());
        }
    }

    if source_version.unwrap_or(1) < 5 && settings.display_profile != "custom" {
        if let Some(tool_index) = settings
            .task_card_fields
            .iter()
            .position(|field| field == "tool")
        {
            if !settings
                .task_card_fields
                .iter()
                .any(|field| field == "currentTarget")
            {
                settings
                    .task_card_fields
                    .insert(tool_index.saturating_add(1), "currentTarget".to_owned());
            }
        }
    }

    settings.display_fields_version = 5;
}

fn settings_value(state: &AppState) -> Result<Value, String> {
    let settings = load_ui_settings(state).map_err(|error| error.to_string())?;
    let bridge = state
        .installer
        .inspect_claude_statusline()
        .map_err(|error| error.to_string())?;
    let backups = state
        .installer
        .backup_summary()
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "settings": settings,
        "displayCatalog": [
            { "id": "task", "label": "Task title and summary", "level": "concise", "placement": "headline", "description": "Primary title; the summary appears below when it differs from the Provider title" },
            { "id": "activity", "label": "Live status", "level": "concise", "placement": "headline", "description": "Run phase, waiting state, and elapsed time shown beside the title" },
            { "id": "project", "label": "Project", "level": "concise", "placement": "subtitle", "description": "Project name in the subtitle" },
            { "id": "model", "label": "Model", "level": "concise", "placement": "subtitle", "description": "Model name in the subtitle" },
            { "id": "plan", "label": "Plan progress", "level": "concise", "placement": "subtitle", "description": "Completed step count and progress bar in the collapsed card" },
            { "id": "sessionTokens", "label": "Session Token total", "level": "concise", "placement": "overview", "description": "Usage chip in the collapsed card and total in expanded details" },
            { "id": "context", "label": "Context usage", "level": "concise", "placement": "overview", "description": "Current context percentage in the collapsed card" },
            { "id": "cost", "label": "Estimated API price", "level": "detailed", "placement": "overview", "description": "Estimated public API price; not a subscription bill" },
            { "id": "turnTokens", "label": "Turn Tokens", "level": "detailed", "placement": "details", "description": "Most recent turn Token count in expanded details" },
            { "id": "inputOutputTokens", "label": "Input / output Tokens", "level": "detailed", "placement": "details", "description": "Input and output Token breakdown in expanded details" },
            { "id": "cacheTokens", "label": "Cache read / write Tokens", "level": "detailed", "placement": "details", "description": "Cache read and creation Token breakdown in expanded details" },
            { "id": "reasoningTokens", "label": "Reasoning Tokens", "level": "detailed", "placement": "details", "description": "Provider reasoning usage in expanded details" },
            { "id": "tool", "label": "Current action", "level": "detailed", "placement": "details", "description": "Semantic category and bounded Provider tool name" },
            { "id": "currentTarget", "label": "Current file / target", "level": "detailed", "placement": "details", "description": "A basename from an explicit Provider path field; command text is never parsed" },
            { "id": "permissionMode", "label": "Permission mode", "level": "detailed", "placement": "details", "description": "Provider permission policy in expanded details" },
            { "id": "subagents", "label": "Running subagents", "level": "detailed", "placement": "details", "description": "Active subagent count and list in expanded details" },
            { "id": "environment", "label": "Environment", "level": "detailed", "placement": "details", "description": "Workspace or client environment in expanded details" },
            { "id": "recovery", "label": "Recovery state", "level": "detailed", "placement": "details", "description": "Reconnect and control recovery state in expanded details" },
            { "id": "control", "label": "Control capability", "level": "detailed", "placement": "details", "description": "Hook or Connector capability in expanded details" },
            { "id": "jump", "label": "Open application", "level": "detailed", "placement": "details", "description": "Entry point for returning to the original application" },
            { "id": "taskFlow", "label": "Task flow", "level": "concise", "placement": "details", "description": "Current Turn plan steps shown after expanding a task" },
            { "id": "workflow", "label": "Workflow", "level": "concise", "placement": "details", "description": "Current Turn live tool activity shown after expanding a task" },
            { "id": "titleSource", "label": "Title source", "level": "developer", "placement": "developer", "description": "Title parsing source in expanded details" },
            { "id": "sessionId", "label": "ActRealm Session ID", "level": "developer", "placement": "developer", "description": "Internal ActRealm session identifier" },
            { "id": "providerSessionId", "label": "Provider Session ID", "level": "developer", "placement": "developer", "description": "Original Provider session identifier" },
            { "id": "providerTurnId", "label": "Provider Turn ID", "level": "developer", "placement": "developer", "description": "Current Provider turn identifier" },
            { "id": "lastEventAt", "label": "Last event time", "level": "developer", "placement": "developer", "description": "Local time of the most recent Runtime event" }
        ],
        "claudeQuotaBridge": {
            "status": bridge.status,
            "configPath": bridge.config_path,
            "helperPath": bridge.helper_path,
            "customConflict": bridge.status == ClaudeStatuslineStatus::CustomConflict,
        },
        "backups": backups
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
    let claude_available = discover_provider_availability(HookProvider::Claude).is_available();
    let existing_claude_config = state.installer.paths().claude_settings.exists();
    if matches!(request.action.as_str(), "install" | "wrap")
        && !claude_available
        && !existing_claude_config
    {
        return api_error(StatusCode::CONFLICT, "PROVIDER_INSTALL_REQUIRED");
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
                HeaderValue::from_static("attachment; filename=actrealm-export.json"),
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
                HeaderValue::from_static("attachment; filename=actrealm-metrics.json"),
            ),
            (CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        body,
    )
        .into_response()
}

async fn export_token_usage_json(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return api_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED");
    }
    let mut export = match state.store.export_token_usage_json(now_millis()) {
        Ok(export) => export,
        Err(_) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "EXPORT_FAILED"),
    };
    if let Some(object) = export.as_object_mut() {
        let quality = usage_collection_quality(&state);
        let has_suspect_anomaly = object
            .get("suspectCount")
            .and_then(Value::as_u64)
            .is_some_and(|count| count > 0);
        let data_quality = if quality.ready && quality.history_complete && has_suspect_anomaly {
            "suspect"
        } else {
            quality.data_quality
        };
        object.insert(
            "collectionState".to_owned(),
            Value::String(quality.collection_state.to_owned()),
        );
        object.insert(
            "dataQuality".to_owned(),
            Value::String(data_quality.to_owned()),
        );
        if quality.last_success > 0 {
            object.insert(
                "lastSuccessfulAt".to_owned(),
                Value::Number(quality.last_success.into()),
            );
        }
        if quality.ready && quality.history_complete && quality.last_success > 0 {
            object.insert(
                "lastAuditedAt".to_owned(),
                Value::Number(quality.last_success.into()),
            );
        }
    }
    let body = match serde_json::to_vec_pretty(&export) {
        Ok(body) => body,
        Err(_) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "EXPORT_FAILED"),
    };
    (
        [
            (CONTENT_TYPE, HeaderValue::from_static("application/json")),
            (
                CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment; filename=actrealm-token-usage.json"),
            ),
            (CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        body,
    )
        .into_response()
}

async fn export_token_usage_csv(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return api_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED");
    }
    let export = match state.store.export_token_usage_csv(now_millis()) {
        Ok(export) => export,
        Err(_) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "EXPORT_FAILED"),
    };
    (
        [
            (
                CONTENT_TYPE,
                HeaderValue::from_static("text/csv; charset=utf-8"),
            ),
            (
                CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment; filename=actrealm-token-usage.csv"),
            ),
            (CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        export,
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClearBackupsRequest {
    confirmation: String,
}

async fn clear_backups(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ClearBackupsRequest>,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    if request.confirmation != "DELETE BACKUPS" {
        return api_error(
            StatusCode::BAD_REQUEST,
            "BACKUP_DELETE_CONFIRMATION_REQUIRED",
        );
    }
    match state.installer.clear_backups() {
        Ok(report) => Json(json!({
            "removedCount": report.removed_count,
            "removedBytes": report.removed_bytes,
            "backups": {
                "count": 0,
                "totalBytes": 0
            }
        }))
        .into_response(),
        Err(error) => api_error_detail(
            StatusCode::CONFLICT,
            "BACKUP_CLEAR_FAILED",
            &error.to_string(),
        ),
    }
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
    undo_delay_ms: Option<u64>,
}

async fn command(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CommandRequest>,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    process_command(&state, request)
}

fn process_command(state: &AppState, request: CommandRequest) -> Response {
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
    let default_delay_ms = u64::try_from(state.commit_delay.as_millis()).unwrap_or(3_000);
    if request
        .undo_delay_ms
        .is_some_and(|undo_delay_ms| !matches!(undo_delay_ms, 0 | 3_000))
    {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_SETTINGS");
    }
    let undo_delay_ms = request.undo_delay_ms.unwrap_or(default_delay_ms);
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
            // Use the same capability boundary as allowedActions in both
            // local and Companion requests. Unknown reply shapes can be
            // rejected, but cannot acquire allow permission via a direct POST.
            if request.action == "approve" && !attention.remote_actionable {
                return api_error(StatusCode::FORBIDDEN, "INVALID_ACTION");
            }
            let action = if request.action == "approve" {
                ApprovalAction::Approve
            } else {
                ApprovalAction::Deny
            };
            let claim = match state.store.claim_approval_with_delay(
                request.id,
                request_id,
                action,
                now,
                undo_delay_ms,
            ) {
                Ok(claim) => claim,
                Err(error) => return store_error_response(error),
            };
            if claim.created {
                let Some(commit_due_at) = claim.commit_due_at else {
                    return api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR");
                };
                if undo_delay_ms == 0 {
                    let waiter_active = state.waiters.is_active(request_id).unwrap_or(false);
                    let committed = match state.store.commit(request.id, now, waiter_active) {
                        Ok(committed) => committed,
                        Err(error) => return store_error_response(error),
                    };
                    let Some(decision) = action.decision() else {
                        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR");
                    };
                    if state.waiters.decide(request_id, decision).is_err() {
                        let _ = state.store.expire_approval(
                            committed.request_id,
                            "waiter_delivery_failed",
                            now_millis(),
                        );
                        return api_error(StatusCode::CONFLICT, "STALE_APPROVAL");
                    }
                    return command_response(
                        StatusCode::OK,
                        request.id,
                        CommandState::DecisionSent,
                    );
                }
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
    process_answer_question(&state, request_id, submission)
}

fn process_answer_question(state: &AppState, request_id: Uuid, submission: Value) -> Response {
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
}

async fn jump_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    process_jump_session(&state, &session_id)
}

fn process_jump_session(state: &AppState, session_id: &str) -> Response {
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
    let targets = jump_targets(session);
    if targets.is_empty() {
        return api_error(StatusCode::CONFLICT, "JUMP_UNSUPPORTED");
    }
    for target in &targets {
        if run_jump_target(target, session).is_ok_and(|success| success) {
            let capability = jump_target_capability(target);
            return Json(json!({
                "success": true,
                "capability": capability,
                "label": jump_target_label(target),
                "labelMessage": status_messages::jump(capability),
            }))
            .into_response();
        }
    }
    api_error_detail(
        StatusCode::CONFLICT,
        "JUMP_FAILED",
        "The original task source and every safe fallback target were unavailable",
    )
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

#[cfg(test)]
fn jump_target(session: &SessionRecord) -> Option<JumpTarget> {
    jump_targets(session).into_iter().next()
}

fn jump_targets(session: &SessionRecord) -> Vec<JumpTarget> {
    let mut targets = Vec::new();
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
    let codex_thread = (session.provider == "codex"
        && Uuid::parse_str(&session.provider_session_id).is_ok())
    .then(|| JumpTarget::CodexThread(session.provider_session_id.clone()));
    let codex_app =
        session.term_surface.as_deref() == Some("codex_app") || bundle == "com.openai.codex";
    let iterm = app.contains("iterm") || bundle == "com.googlecode.iterm2";
    let terminal = app == "apple_terminal" || bundle == "com.apple.terminal";
    let vscode = app == "vscode" || bundle == "com.microsoft.vscode";
    let warp = app.contains("warp") || bundle.starts_with("dev.warp.");

    if codex_app {
        if let Some(target) = codex_thread.clone() {
            targets.push(target);
        }
        targets.push(JumpTarget::AppBundle("com.openai.codex"));
    } else if iterm {
        if session.term_session_id.as_deref().is_some_and(safe_locator) {
            targets.push(JumpTarget::ITermSession);
        }
        targets.push(JumpTarget::AppBundle("com.googlecode.iterm2"));
    } else if terminal {
        if session.term_tty.as_deref().is_some_and(safe_locator) {
            targets.push(JumpTarget::TerminalTty);
        }
        targets.push(JumpTarget::AppBundle("com.apple.Terminal"));
    } else if vscode {
        targets.push(JumpTarget::AppBundle("com.microsoft.VSCode"));
    } else if warp {
        targets.push(JumpTarget::AppBundle("dev.warp.Warp-Stable"));
    } else if session.term_surface.as_deref() == Some("claude_app")
        || bundle == "com.anthropic.claudefordesktop"
    {
        targets.push(JumpTarget::AppBundle("com.anthropic.claudefordesktop"));
    }

    if let Some(target) = codex_thread {
        if !targets.contains(&target) {
            targets.push(target);
        }
    }
    targets
}

fn jump_target_capability(target: &JumpTarget) -> &'static str {
    match target {
        JumpTarget::CodexThread(_) => "exact_conversation",
        JumpTarget::ITermSession | JumpTarget::TerminalTty => "terminal",
        JumpTarget::AppBundle(_) => "app_only",
    }
}

fn jump_target_label(target: &JumpTarget) -> &'static str {
    match target {
        JumpTarget::CodexThread(_) => "Open exact conversation",
        JumpTarget::ITermSession => "Return to iTerm",
        JumpTarget::TerminalTty => "Return to Terminal",
        JumpTarget::AppBundle("com.microsoft.VSCode") => "Return to VS Code",
        JumpTarget::AppBundle("com.googlecode.iterm2") => "Open iTerm",
        JumpTarget::AppBundle("com.apple.Terminal") => "Open Terminal",
        JumpTarget::AppBundle("com.anthropic.claudefordesktop") => "Open Claude",
        JumpTarget::AppBundle(_) => "Open task source",
    }
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
                "ACTREALM_JUMP_SESSION",
                session.term_session_id.as_deref().unwrap_or_default(),
            )
            .env(
                "ACTREALM_JUMP_TTY",
                session.term_tty.as_deref().unwrap_or_default(),
            )
            .status()?,
        JumpTarget::TerminalTty => ProcessCommand::new("/usr/bin/osascript")
            .arg("-e")
            .arg(TERMINAL_JUMP_SCRIPT)
            .env(
                "ACTREALM_JUMP_TTY",
                session.term_tty.as_deref().unwrap_or_default(),
            )
            .status()?,
        JumpTarget::AppBundle(bundle) => ProcessCommand::new("/usr/bin/open")
            .args(["-b", *bundle])
            .status()?,
    };
    Ok(status.success())
}

const ITERM_JUMP_SCRIPT: &str = r#"
set targetSession to system attribute "ACTREALM_JUMP_SESSION"
set targetTty to system attribute "ACTREALM_JUMP_TTY"
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
set targetTty to system attribute "ACTREALM_JUMP_TTY"
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
        tokio::time::sleep(Duration::from_millis(
            commit_due_at.saturating_sub(now_millis()),
        ))
        .await;
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
    process_undo(&state, &id)
}

fn process_undo(state: &AppState, id: &str) -> Response {
    let Ok(command_id) = Uuid::parse_str(id) else {
        return api_error(StatusCode::BAD_REQUEST, "INVALID_COMMAND_ID");
    };
    match state.store.undo(command_id, now_millis()) {
        Ok(command_state) => command_response(StatusCode::OK, command_id, command_state),
        Err(error) => store_error_response(error),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebSocketTicketResponse {
    ticket: String,
    expires_in_ms: u64,
}

async fn issue_websocket_ticket(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authorized_mutation(&state, &headers) {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_MUTATION");
    }
    let Ok(ticket) = generate_secret() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "AUTH_UNAVAILABLE");
    };
    let Ok(mut auth) = state.auth.lock() else {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, "AUTH_UNAVAILABLE");
    };
    let now = Instant::now();
    auth.websocket_tickets
        .retain(|(_, expires_at)| *expires_at > now);
    if auth.websocket_tickets.len() >= MAX_WS_TICKETS {
        auth.websocket_tickets.remove(0);
    }
    auth.websocket_tickets
        .push((ticket.clone(), now + WS_TICKET_TTL));
    Json(WebSocketTicketResponse {
        ticket,
        expires_in_ms: WS_TICKET_TTL.as_millis() as u64,
    })
    .into_response()
}

async fn websocket(
    State(state): State<AppState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let Some(protocol) = websocket_protocol(&headers) else {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_WEBSOCKET");
    };
    let Some(ticket) = protocol.strip_prefix(WS_PROTOCOL_PREFIX) else {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_WEBSOCKET");
    };
    if !authorized(&state, &headers)
        || !valid_same_origin(&state, &headers)
        || !consume_websocket_ticket(&state, ticket)
    {
        return api_error(StatusCode::FORBIDDEN, "UNAUTHORIZED_WEBSOCKET");
    }
    upgrade
        .protocols([protocol])
        .on_upgrade(move |socket| websocket_loop(socket, state))
        .into_response()
}

struct WebSocketConnectionGuard(Arc<AtomicUsize>);

impl WebSocketConnectionGuard {
    fn new(connections: Arc<AtomicUsize>) -> Self {
        connections.fetch_add(1, Ordering::AcqRel);
        Self(connections)
    }
}

impl Drop for WebSocketConnectionGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

async fn websocket_loop(mut socket: WebSocket, state: AppState) {
    let _connection = WebSocketConnectionGuard::new(state.websocket_connections.clone());
    let mut last_payload = String::new();
    let mut last_heartbeat = Instant::now();
    while !state.shutdown_flag.load(Ordering::Acquire) {
        let Ok(snapshot) = snapshot_value(&state) else {
            break;
        };
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
        if last_heartbeat.elapsed() >= state.heartbeat_interval {
            let heartbeat = json!({
                "type": "heartbeat",
                "serverTime": now_millis(),
            })
            .to_string();
            if socket.send(Message::Text(heartbeat.into())).await.is_err() {
                break;
            }
            last_heartbeat = Instant::now();
        }
        match tokio::time::timeout(state.snapshot_interval, socket.recv()).await {
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Ok(Some(Err(_))) => break,
            Ok(Some(Ok(_))) | Err(_) => {}
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct UsageCollectionQuality {
    collection_state: &'static str,
    data_quality: &'static str,
    in_progress: bool,
    ready: bool,
    history_complete: bool,
    failures: usize,
    last_success: u64,
}

fn usage_collection_quality(state: &AppState) -> UsageCollectionQuality {
    let in_progress = state.usage_collection_in_progress.load(Ordering::Acquire);
    let ready = state.usage_collection_ready.load(Ordering::Acquire);
    let history_complete = state.usage_history_complete.load(Ordering::Acquire);
    let failures = state.usage_consecutive_failures.load(Ordering::Acquire);
    let last_success = state.usage_last_success_at.load(Ordering::Acquire);
    let collection_state = if failures > 0 && !ready {
        "unavailable"
    } else if failures > 0 || (ready && !history_complete) {
        "partial"
    } else if !ready && (in_progress || last_success > 0) {
        "scanning"
    } else if ready {
        "ready"
    } else {
        "pending"
    };
    let data_quality = match collection_state {
        "scanning" => "rebuilding",
        "ready" => "verified",
        "partial" => "partial",
        "unavailable" => "unavailable",
        _ => "pending",
    };
    UsageCollectionQuality {
        collection_state,
        data_quality,
        in_progress,
        ready,
        history_complete,
        failures,
        last_success,
    }
}

fn fact_metadata(
    source_kind: FactSourceKind,
    source_id: Option<&str>,
    captured_at: Option<u64>,
    freshness: FactFreshness,
    verification: FactVerification,
    absence_reason: Option<FactAbsenceReason>,
    capability: FactCapability,
) -> FactMetadata {
    FactMetadata {
        schema_version: FACT_METADATA_SCHEMA_VERSION,
        source_kind,
        source_id: source_id.map(str::to_owned),
        captured_at,
        freshness,
        verification,
        absence_reason,
        capability,
    }
}

fn fact_freshness(captured_at: Option<u64>, now: u64) -> FactFreshness {
    let Some(captured_at) = captured_at else {
        return FactFreshness::Stale;
    };
    let age = now.saturating_sub(captured_at);
    if age <= FACT_LIVE_MAX_AGE_MS {
        FactFreshness::Live
    } else if age <= FACT_DELAYED_MAX_AGE_MS {
        FactFreshness::Delayed
    } else {
        FactFreshness::Stale
    }
}

fn unavailable_provider_fact(
    status: ProviderCapabilityStatus,
    captured_at: Option<u64>,
    now: u64,
    supported_absence: FactAbsenceReason,
) -> FactMetadata {
    let absence = match status {
        ProviderCapabilityStatus::Supported => supported_absence,
        ProviderCapabilityStatus::Unsupported => FactAbsenceReason::NotSupported,
        ProviderCapabilityStatus::Unknown => FactAbsenceReason::CapabilityUnconfirmed,
    };
    fact_metadata(
        FactSourceKind::Unavailable,
        None,
        captured_at,
        fact_freshness(captured_at, now),
        FactVerification::Unverified,
        Some(absence),
        FactCapability::Unavailable,
    )
}

fn session_facts(
    session: &SessionRecord,
    control: &str,
    blocking_attention_kind: Option<&str>,
    attention_created_at: Option<u64>,
    direct_attention: bool,
    now: u64,
) -> SessionFacts {
    let provider_matrix = provider_capability_matrix();
    let plan_capability =
        provider_matrix.capability(&session.provider, ProviderCapabilityFeature::Plan);
    let current_target_capability =
        provider_matrix.capability(&session.provider, ProviderCapabilityFeature::CurrentTarget);
    let no_current_turn = terminal_execution_state(&session.exec_state);

    let plan = if no_current_turn {
        fact_metadata(
            FactSourceKind::Unavailable,
            None,
            session.turn_ended_at.or(Some(session.last_event_at)),
            FactFreshness::Expired,
            FactVerification::NotApplicable,
            Some(FactAbsenceReason::NoCurrentTurn),
            FactCapability::ObserveOnly,
        )
    } else if !session.plan_steps.is_empty() {
        fact_metadata(
            FactSourceKind::Authoritative,
            plan_capability.source.as_deref(),
            Some(session.last_event_at),
            fact_freshness(Some(session.last_event_at), now),
            FactVerification::Verified,
            None,
            FactCapability::ObserveOnly,
        )
    } else {
        unavailable_provider_fact(
            plan_capability.status,
            Some(session.last_event_at),
            now,
            FactAbsenceReason::ProviderNotSupplied,
        )
    };

    let activity = if no_current_turn {
        fact_metadata(
            FactSourceKind::Unavailable,
            None,
            session.turn_ended_at.or(Some(session.last_event_at)),
            FactFreshness::Expired,
            FactVerification::NotApplicable,
            Some(FactAbsenceReason::NoCurrentTurn),
            FactCapability::ObserveOnly,
        )
    } else if blocking_attention_kind.is_some() {
        fact_metadata(
            FactSourceKind::Authoritative,
            Some("runtime:attention_state"),
            attention_created_at.or(Some(session.last_event_at)),
            FactFreshness::Live,
            FactVerification::Verified,
            None,
            FactCapability::ObserveOnly,
        )
    } else if session.activity.is_some() || session.current_tool.is_some() {
        let source_id = if session.current_tool.is_some() {
            "provider:tool_lifecycle"
        } else {
            "runtime:execution_reducer"
        };
        let captured_at = session.activity_since.or(Some(session.last_event_at));
        let freshness = fact_freshness(captured_at, now);
        if freshness == FactFreshness::Stale {
            fact_metadata(
                FactSourceKind::Unavailable,
                Some(source_id),
                captured_at,
                freshness,
                FactVerification::Unverified,
                Some(FactAbsenceReason::SourceStale),
                FactCapability::ObserveOnly,
            )
        } else {
            fact_metadata(
                if session.current_tool.is_some() {
                    FactSourceKind::Observed
                } else {
                    FactSourceKind::Derived
                },
                Some(source_id),
                captured_at,
                freshness,
                if session.current_tool.is_some() {
                    FactVerification::Verified
                } else {
                    FactVerification::Partial
                },
                None,
                FactCapability::ObserveOnly,
            )
        }
    } else {
        fact_metadata(
            FactSourceKind::Unavailable,
            None,
            Some(session.last_event_at),
            fact_freshness(Some(session.last_event_at), now),
            FactVerification::Unverified,
            Some(FactAbsenceReason::NoCurrentActivity),
            FactCapability::ObserveOnly,
        )
    };

    let current_target = if no_current_turn {
        fact_metadata(
            FactSourceKind::Unavailable,
            None,
            session.turn_ended_at.or(Some(session.last_event_at)),
            FactFreshness::Expired,
            FactVerification::NotApplicable,
            Some(FactAbsenceReason::NoCurrentTurn),
            FactCapability::ObserveOnly,
        )
    } else if session.current_target.is_some() {
        fact_metadata(
            FactSourceKind::Observed,
            current_target_capability.source.as_deref(),
            Some(session.last_event_at),
            fact_freshness(Some(session.last_event_at), now),
            FactVerification::Verified,
            None,
            FactCapability::ObserveOnly,
        )
    } else if session.exec_state != "tool_running" {
        unavailable_provider_fact(
            current_target_capability.status,
            Some(session.last_event_at),
            now,
            FactAbsenceReason::NoCurrentTool,
        )
    } else {
        unavailable_provider_fact(
            current_target_capability.status,
            Some(session.last_event_at),
            now,
            FactAbsenceReason::CurrentToolHasNoTarget,
        )
    };

    let completion = if matches!(session.exec_state.as_str(), "response_finished" | "failed") {
        fact_metadata(
            FactSourceKind::Derived,
            Some("runtime:terminal_reducer"),
            session.turn_ended_at.or(Some(session.last_event_at)),
            fact_freshness(session.turn_ended_at.or(Some(session.last_event_at)), now),
            FactVerification::Verified,
            None,
            FactCapability::ObserveOnly,
        )
    } else {
        fact_metadata(
            FactSourceKind::Unavailable,
            None,
            Some(session.last_event_at),
            fact_freshness(Some(session.last_event_at), now),
            FactVerification::NotApplicable,
            Some(FactAbsenceReason::TaskNotCompleted),
            FactCapability::ObserveOnly,
        )
    };

    let control = if direct_attention {
        fact_metadata(
            FactSourceKind::Authoritative,
            Some("runtime:live_reply_waiter"),
            attention_created_at,
            FactFreshness::Live,
            FactVerification::Verified,
            None,
            FactCapability::Direct,
        )
    } else if blocking_attention_kind.is_some() {
        fact_metadata(
            FactSourceKind::Observed,
            Some("provider:attention_observation"),
            attention_created_at.or(Some(session.last_event_at)),
            FactFreshness::Live,
            FactVerification::Verified,
            None,
            FactCapability::ReturnToProvider,
        )
    } else if no_current_turn {
        fact_metadata(
            FactSourceKind::Unavailable,
            None,
            session.turn_ended_at.or(Some(session.last_event_at)),
            FactFreshness::Expired,
            FactVerification::NotApplicable,
            Some(FactAbsenceReason::NoCurrentTurn),
            FactCapability::Unavailable,
        )
    } else {
        fact_metadata(
            FactSourceKind::Observed,
            Some(if control == "managed" {
                "connector:attached_thread"
            } else {
                "hook:session_observation"
            }),
            Some(session.last_event_at),
            fact_freshness(Some(session.last_event_at), now),
            FactVerification::Partial,
            None,
            FactCapability::ObserveOnly,
        )
    };

    SessionFacts {
        schema_version: FACT_METADATA_SCHEMA_VERSION,
        plan,
        activity,
        current_target,
        completion,
        control,
    }
}

/// Numeric-only, in-memory fallback while the first atomic ledger generation
/// is being built. Never enters SQLite, exports, daily totals or spool files.
#[derive(Clone)]
struct LiveCodexUsage {
    model: Option<String>,
    captured_at: u64,
    fields: serde_json::Map<String, Value>,
}

fn live_codex_metrics(record: &UsageRecord) -> LiveCodexUsage {
    let value = json!({
        "inputTokens":record.input_tokens,"outputTokens":record.output_tokens,
        "cacheReadTokens":record.cache_read_tokens,"reasoningTokens":record.reasoning_tokens,
        "tokenTotal":record.token_total,"lastTurnTokens":record.last_turn_tokens,
        "contextUsedTokens":record.context_used_tokens,"contextWindowTokens":record.context_window_tokens,
        "contextUsedPercent":record.context_used_percent,
        "estimatedCostUsdMicros":record.estimated_cost_usd_micros,
    });
    let mut fields = value.as_object().cloned().unwrap_or_default();
    fields.retain(|_, value| !value.is_null());
    LiveCodexUsage {
        model: record.model.clone(),
        captured_at: record.captured_at,
        fields,
    }
}

fn apply_live_codex_metrics(
    session: &SessionRecord,
    object: &mut serde_json::Map<String, Value>,
    live: &HashMap<String, LiveCodexUsage>,
) {
    if session.provider != "codex" || session.usage_quality.as_deref() == Some("suspect") {
        return;
    }
    let Some(usage) = live.get(&session.provider_session_id) else {
        return;
    };
    if matches!((&session.model,&usage.model),(Some(current),Some(observed)) if current != observed)
    {
        return;
    }
    let mut filled = false;
    for (key, value) in &usage.fields {
        if object.get(key).is_none_or(Value::is_null) {
            object.insert(key.clone(), value.clone());
            filled = true;
        }
    }
    if filled {
        object.insert("usageSource".into(), json!("codex_rollout_during_indexing"));
        object.insert("usageQuality".into(), json!("partial"));
        object.insert(
            "usageCapturedAt".into(),
            json!(coarse_ui_timestamp(usage.captured_at)),
        );
    }
}

fn snapshot_value(state: &AppState) -> Result<Value, StoreError> {
    let now = now_millis();
    state.codex.retry_unsynced_native_approvals();
    let snapshot = state
        .store
        .ui_snapshot(now.saturating_sub(SESSION_LIST_RETENTION_MS))?;
    let mut sessions = serde_json::to_value(&snapshot.sessions)
        .map_err(|error| StoreError::Storage(error.to_string()))?;
    let live_codex_usage = state
        .live_codex_usage
        .lock()
        .map(|value| value.clone())
        .unwrap_or_default();
    if let Some(items) = sessions.as_array_mut() {
        for (index, session) in snapshot.sessions.iter().enumerate() {
            let (mut control, mut recovery, can_manage, connector_status) =
                if session.provider == "codex" {
                    state
                        .codex
                        .recovery_for(&session.provider_session_id, &session.exec_state)
                } else {
                    let ended = terminal_execution_state(&session.exec_state);
                    (
                        "external_hook".to_owned(),
                        if ended { "ended" } else { "observing" }.to_owned(),
                        false,
                        None,
                    )
                };
            // A failed attachment belongs to our independent connector, not
            // the Desktop process currently producing Hook events. Continue
            // observing that live Provider without claiming managed control.
            if control == "managed" && recovery == "lost_control" && session.provider == "codex" {
                if let Some(pid) = session.provider_pid {
                    let executable = process_executable_path(pid);
                    if detached_codex_can_be_observed(
                        &session.exec_state,
                        process_alive(pid),
                        executable.as_deref(),
                    ) {
                        control = "external_hook".to_owned();
                        recovery = "observing".to_owned();
                    }
                }
            }
            if control == "external_hook" && !terminal_execution_state(&session.exec_state) {
                recovery = match session.provider_pid {
                    Some(pid) => external_process_recovery_state(
                        &session.provider,
                        pid,
                        session.term_bundle_id.as_deref(),
                        session.term_surface.as_deref(),
                    )
                    .to_owned(),
                    None => "waiting_for_event".to_owned(),
                };
            }
            recovery = recovery_for_execution(&session.exec_state, recovery);
            recovery = recovery_from_persisted_process(
                &session.provider,
                &session.exec_state,
                recovery,
                session.provider_pid,
                session.term_bundle_id.as_deref(),
                session.term_surface.as_deref(),
            );
            if let Some(object) = items.get_mut(index).and_then(Value::as_object_mut) {
                if execution_is_unconfirmed(
                    &session.exec_state,
                    &recovery,
                    session.last_event_at,
                    now,
                ) {
                    // Compare the observed event watermark inside the writer so a
                    // concurrent fresh Hook cannot be overwritten by this snapshot.
                    if state
                        .store
                        .mark_execution_unconfirmed(&session.id, session.last_event_at)
                        .unwrap_or(false)
                    {
                        object.insert("execState".into(), json!("waiting_for_event"));
                        object.insert("activity".into(), Value::Null);
                        object.insert("currentTool".into(), Value::Null);
                        object.insert("currentTarget".into(), Value::Null);
                    }
                }
                apply_live_codex_metrics(session, object, &live_codex_usage);
                let blocking_attention = snapshot.attention.iter().find(|item| {
                    item.session_id == session.id
                        && matches!(item.state.as_str(), "open" | "committing" | "decision_sent")
                        && matches!(
                            item.kind.as_str(),
                            "approval" | "native_approval" | "question"
                        )
                });
                let blocking_attention_kind = blocking_attention.map(|item| item.kind.as_str());
                let direct_attention = blocking_attention.is_some_and(|item| {
                    item.kind == "approval"
                        && item.remote_actionable
                        && item.state == "open"
                        && item.expires_at.is_some_and(|expires_at| expires_at > now)
                        && item.request_id.is_some_and(|request_id| {
                            state.waiters.is_active(request_id).unwrap_or(false)
                        })
                });
                object.insert(
                    "activityMessage".to_owned(),
                    status_messages::session_activity(session, blocking_attention_kind),
                );
                object.insert(
                    "jumpMessage".to_owned(),
                    status_messages::jump(&session.jump_capability),
                );
                object.insert(
                    "controlCapability".to_owned(),
                    Value::String(control.clone()),
                );
                object.insert("recoveryState".to_owned(), Value::String(recovery));
                object.insert("canManage".to_owned(), Value::Bool(can_manage));
                if let Some(status) = connector_status {
                    object.insert("connectorThreadStatus".to_owned(), Value::String(status));
                }
                object.insert(
                    "facts".to_owned(),
                    serde_json::to_value(session_facts(
                        session,
                        &control,
                        blocking_attention_kind,
                        blocking_attention.map(|item| item.created_at),
                        direct_attention,
                        now,
                    ))
                    .map_err(|error| StoreError::Storage(error.to_string()))?,
                );
            }
        }
    }
    let mut attention = serde_json::to_value(&snapshot.attention)
        .map_err(|error| StoreError::Storage(error.to_string()))?;
    if let Some(items) = attention.as_array_mut() {
        for (index, item) in snapshot.attention.iter().enumerate() {
            let Some(object) = items.get_mut(index).and_then(Value::as_object_mut) else {
                continue;
            };
            object.insert(
                "titleMessage".to_owned(),
                status_messages::attention_title(item),
            );
            if let Some(message) = status_messages::attention_detail(item) {
                object.insert("detailMessage".to_owned(), message);
            }
            object.insert(
                "riskMessages".to_owned(),
                Value::Array(status_messages::attention_risks(item)),
            );
            if let Some(request_id) = item.request_id {
                if let Ok(Some(interaction)) = state.waiters.interactive_prompt(request_id) {
                    object.insert(
                        "interaction".to_owned(),
                        serde_json::to_value(interaction)
                            .map_err(|error| StoreError::Storage(error.to_string()))?,
                    );
                }
            }
            let reply_channel_active = item.kind == "approval"
                && item.state == "open"
                && item.expires_at.is_some_and(|expires_at| expires_at > now)
                && item
                    .request_id
                    .is_some_and(|request_id| state.waiters.is_active(request_id).unwrap_or(false));
            let remote_actionable = item.remote_actionable && reply_channel_active;
            object.insert(
                "remoteActionable".to_owned(),
                Value::Bool(remote_actionable),
            );
            object.insert(
                "allowedActions".to_owned(),
                Value::Array(if remote_actionable {
                    vec![
                        Value::String("approve".to_owned()),
                        Value::String("deny".to_owned()),
                    ]
                } else if reply_channel_active {
                    // An unreviewed future tool can always be rejected from
                    // a trusted local companion, but it must not gain an
                    // allow capability until its reply schema is reviewed.
                    vec![Value::String("deny".to_owned())]
                } else {
                    Vec::new()
                }),
            );
        }
    }
    let quota_entries = quota_entries(state)?;
    let mut quota = serde_json::to_value(&quota_entries)
        .map_err(|error| StoreError::Storage(error.to_string()))?;
    if let Some(items) = quota.as_array_mut() {
        for (index, entry) in quota_entries.iter().enumerate() {
            let Some(object) = items.get_mut(index).and_then(Value::as_object_mut) else {
                continue;
            };
            if let Some(message) = status_messages::quota_window(entry) {
                object.insert("windowMessage".to_owned(), message);
            }
            if let Some(message) = status_messages::quota_reason(entry) {
                object.insert("reasonMessage".to_owned(), message);
            }
            object.insert(
                "quotaKind".to_owned(),
                Value::String(quota_kind(entry).to_owned()),
            );
            if let Some(captured_at) = entry.captured_at {
                object.insert(
                    "capturedAt".to_owned(),
                    Value::Number(coarse_ui_timestamp(captured_at).into()),
                );
            }
        }
    }
    let mut token_usage = serde_json::to_value(&snapshot.token_usage)
        .map_err(|error| StoreError::Storage(error.to_string()))?;
    if let Some(object) = token_usage.as_object_mut() {
        let quality = usage_collection_quality(state);
        let data_quality = if quality.ready
            && quality.history_complete
            && snapshot.token_usage.suspect_count > 0
        {
            "suspect"
        } else {
            quality.data_quality
        };
        object.insert(
            "collectionState".to_owned(),
            Value::String(quality.collection_state.to_owned()),
        );
        object.insert(
            "dataQuality".to_owned(),
            Value::String(data_quality.to_owned()),
        );
        object.insert(
            "collectionInProgress".to_owned(),
            // Keep the UI state stable across each bounded worker iteration.
            // The authenticated diagnostics endpoint still exposes the raw
            // in-progress bit; the workspace only needs to know whether the
            // first scan has reached a complete published generation.
            Value::Bool(!quality.ready),
        );
        object.insert(
            "consecutiveFailures".to_owned(),
            Value::Number(u64::try_from(quality.failures).unwrap_or(u64::MAX).into()),
        );
        if quality.last_success > 0 {
            object.insert(
                "lastSuccessfulAt".to_owned(),
                Value::Number(coarse_ui_timestamp(quality.last_success).into()),
            );
        }
        if quality.ready && quality.history_complete && quality.last_success > 0 {
            object.insert(
                "lastAuditedAt".to_owned(),
                Value::Number(coarse_ui_timestamp(quality.last_success).into()),
            );
        }
    }
    let ui_settings = load_ui_settings(state)?;
    let mut token_decision = serde_json::to_value(
        state.store.token_usage_decision(
            now,
            ui_settings
                .token_threshold_notifications_enabled
                .then_some(ui_settings.token_threshold_tokens_per_minute),
        )?,
    )
    .map_err(|error| StoreError::Storage(error.to_string()))?;
    if let Some(object) = token_decision.as_object_mut() {
        object.insert(
            "generatedAt".to_owned(),
            Value::Number(coarse_ui_timestamp(now).into()),
        );
    }
    Ok(json!({
        "sessions": sessions,
        "attention": attention,
        "asyncQuestions": state.codex.state.lock().ok().map(|mut s| s.async_questions.snapshot(now)).unwrap_or_default(),
        "commands": snapshot.commands,
        "quota": quota,
        "stats": {
            "eventCount": snapshot.event_count,
            "metrics": snapshot.metrics
        },
        "tokenUsage": token_usage,
        "tokenDecision": token_decision,
        "capabilities": {
            "codexConnector": state.codex.capability_value(),
            "providerMatrix": provider_capability_matrix()
        }
    }))
}

fn coarse_ui_timestamp(value: u64) -> u64 {
    value - value % UI_STATUS_TIMESTAMP_GRANULARITY_MS
}

fn companion_snapshot_value(
    state: &AppState,
    authorization: &CompanionAuthorization,
) -> Result<Value, StoreError> {
    let source = snapshot_value(state)?;
    let sessions = project_array_fields(
        source.get("sessions"),
        &[
            "id",
            "provider",
            "project",
            "title",
            "providerTitle",
            "providerTitleSource",
            "model",
            "execState",
            "approvalOwner",
            "activity",
            "activityMessage",
            "activitySince",
            "planDone",
            "planTotal",
            "planSteps",
            "turnStartedAt",
            "turnEndedAt",
            "tokenTotal",
            "inputTokens",
            "outputTokens",
            "cacheReadTokens",
            "cacheCreationTokens",
            "reasoningTokens",
            "lastTurnTokens",
            "contextWindowTokens",
            "contextUsedTokens",
            "contextUsedPercent",
            "estimatedCostUsdMicros",
            "currentTool",
            "currentToolCategory",
            "currentTarget",
            "activeSubagents",
            "subagents",
            "environment",
            "jumpCapability",
            "jumpLabel",
            "jumpMessage",
            "controlCapability",
            "recoveryState",
            "connectorThreadStatus",
            "facts",
            "lastEventAt",
        ],
    );
    let mut sessions = sessions;
    if let Ok(native) = state.codex.state.lock() {
        for row in &mut sessions {
            let latest = row["id"]
                .as_str()
                .and_then(|id| native.native_activities.get(id))
                .and_then(|events| {
                    events
                        .iter()
                        .filter(|event| {
                            event.started_at >= row["turnStartedAt"].as_u64().unwrap_or(0)
                        })
                        .map(|event| event.occurred_at)
                        .max()
                });
            if let Some(at) = latest {
                row["lastEventAt"] = json!(at.max(row["lastEventAt"].as_u64().unwrap_or(0)));
            }
        }
    }
    let can_respond = authorization.has_scope(COMPANION_SCOPE_RESPOND);
    let mut async_questions = source
        .get("asyncQuestions")
        .cloned()
        .unwrap_or_else(|| json!([]));
    if !can_respond {
        for q in async_questions.as_array_mut().into_iter().flatten() {
            q["canAnswer"] = json!(false);
        }
    }
    let mut attention = project_array_fields(
        source.get("attention"),
        &[
            "id",
            "sessionId",
            "provider",
            "project",
            "requestId",
            "kind",
            "title",
            "titleMessage",
            "detail",
            "detailMessage",
            "state",
            "risk",
            "riskNotes",
            "riskMessages",
            "primaryCategory",
            "riskCodes",
            "commandPreview",
            "expiresAt",
            "autoHideAt",
            "reminderAcknowledgedAt",
            "reminderResolution",
            "retainAfterAck",
            "createdAt",
            "resolution",
            "interaction",
            "remoteActionable",
            "allowedActions",
        ],
    );
    if !can_respond {
        for item in &mut attention {
            if let Some(object) = item.as_object_mut() {
                object.insert("remoteActionable".to_owned(), Value::Bool(false));
                object.insert("allowedActions".to_owned(), Value::Array(Vec::new()));
            }
        }
    }
    let quota = project_array_fields(
        source.get("quota"),
        &[
            "provider",
            "window",
            "status",
            "usedPct",
            "remainingPct",
            "resetsAt",
            "resetSource",
            "resetCapturedAt",
            "source",
            "windowMinutes",
            "limitId",
            "limitName",
            "quotaKind",
            "planType",
            "capturedAt",
            "windowMessage",
            "reasonMessage",
        ],
    );
    let commands = project_array_fields(
        source.get("commands"),
        &[
            "id",
            "attentionId",
            "requestId",
            "action",
            "state",
            "createdAt",
        ],
    );
    Ok(json!({
        "schemaVersion": 1,
        "instanceId": state.instance_id,
        "capturedAt": now_millis(),
        "sessions": sessions,
        "attention": attention,
        "commands": commands,
        "asyncQuestions": async_questions,
        "pricing": state.pricing_status.lock().ok().map(|s| s.clone()),
        "quota": quota,
        "tokenUsage": source.get("tokenUsage").cloned().unwrap_or(Value::Null),
        "stats": {
            "eventCount": source.pointer("/stats/eventCount").cloned().unwrap_or(Value::Null),
        },
        "capabilities": {
            "companionId": authorization.id,
            "scopes": authorization.scopes,
            "canJump": authorization.has_scope(COMPANION_SCOPE_JUMP),
            "canRespond": can_respond,
            "providerMatrix": source.pointer("/capabilities/providerMatrix").cloned().unwrap_or(Value::Null),
            "codexConnector": source.pointer("/capabilities/codexConnector").cloned().unwrap_or(Value::Null),
        }
    }))
}

fn quota_kind(entry: &QuotaEntry) -> &'static str {
    if entry.provider != "codex" {
        return "standard";
    }
    let is_spark = [entry.limit_id.as_deref(), entry.limit_name.as_deref()]
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_ascii_lowercase())
        .any(|value| {
            matches!(
                value.as_str(),
                "codex_bengalfox" | "codex_spark" | "codex-spark" | "codex spark"
            )
        });
    if is_spark {
        "spark"
    } else {
        "standard"
    }
}

fn project_array_fields(source: Option<&Value>, fields: &[&str]) -> Vec<Value> {
    source
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .map(|object| {
            let projected = fields
                .iter()
                .filter_map(|field| {
                    object
                        .get(*field)
                        .map(|value| ((*field).to_owned(), value.clone()))
                })
                .collect();
            Value::Object(projected)
        })
        .collect()
}

fn refresh_session_usage(state: &AppState, now: u64) -> Result<(bool, bool), StoreError> {
    let records = {
        let mut usage = match state.usage.lock() {
            Ok(usage) => usage,
            Err(poisoned) => {
                let mut usage = poisoned.into_inner();
                let paths = usage.collector.paths().clone();
                let live_pricing = usage.collector.live_pricing_enabled();
                *usage = UsageState {
                    collector: UsageCollector::new(paths),
                    refreshed_at: None,
                };
                if live_pricing {
                    usage.collector.enable_live_pricing();
                }
                state.usage.clear_poison();
                usage
            }
        };
        if usage
            .refreshed_at
            .is_some_and(|instant| instant.elapsed() < USAGE_COLLECTION_MIN_INTERVAL)
        {
            return Ok((
                usage.collector.is_caught_up(),
                usage.collector.is_history_complete(),
            ));
        }
        #[cfg(test)]
        if let Some(control) = state.usage_test_control.as_ref() {
            control.before_collect();
        }
        if state
            .pricing_refresh_requested
            .swap(false, Ordering::AcqRel)
        {
            usage.collector.request_pricing_refresh();
        }
        let records = usage
            .collector
            .collect_until_shutdown(now, &state.shutdown_flag);
        if let Ok(mut status) = state.pricing_status.lock() {
            *status = usage.collector.pricing_status();
        }
        let caught_up = usage.collector.is_caught_up();
        let history_complete = usage.collector.is_history_complete();
        usage.refreshed_at = Some(Instant::now());
        let ready = usage.collector.ready_codex_session_ids();
        (records, caught_up, history_complete, ready)
    };
    if state.shutdown_flag.load(Ordering::Acquire) {
        return Ok((false, false));
    }
    if let Ok(mut live) = state.live_codex_usage.lock() {
        let present: HashSet<&str> = records
            .0
            .iter()
            .filter(|r| r.provider == "codex")
            .map(|r| r.provider_session_id.as_str())
            .collect();
        live.retain(|id, _| present.contains(id.as_str()) && records.3.contains(id));
        for record in records
            .0
            .iter()
            .filter(|r| r.provider == "codex" && records.3.contains(&r.provider_session_id))
        {
            if record.model.as_ref().is_some_and(|model| model.len() > 256) {
                continue;
            }
            live.insert(
                record.provider_session_id.clone(),
                live_codex_metrics(record),
            );
        }
    }
    // The in-memory collector is the shadow generation. Keep serving the
    // previous committed ledger while a bounded historical scan is incomplete;
    // publishing each intermediate prefix makes totals wobble across restarts.
    if !records.1 {
        return Ok((false, records.2));
    }
    let generation = state.store.begin_usage_collection_generation()?;
    state
        .store
        .replace_session_usages_for_generation(
            records.0.into_iter().map(runtime_usage_record).collect(),
            now,
            generation,
        )
        .map(|_| {
            if let Ok(mut live) = state.live_codex_usage.lock() {
                live.clear();
            }
            (records.1, records.2)
        })
}

fn run_usage_refresh_iteration(state: &AppState) -> bool {
    state
        .usage_collection_in_progress
        .store(true, Ordering::Release);
    let attempted_at = now_millis();
    let refreshed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        refresh_session_usage(state, attempted_at)
    }));
    state
        .usage_collection_in_progress
        .store(false, Ordering::Release);
    match refreshed {
        Ok(Ok((caught_up, history_complete))) => {
            if caught_up {
                state.usage_collection_ready.store(true, Ordering::Release);
                state
                    .usage_history_complete
                    .store(history_complete, Ordering::Release);
            } else if !state.usage_collection_ready.load(Ordering::Acquire) {
                // Before the first complete generation the collector remains
                // in its initial indexing state. After readiness, a growing
                // hot file is only an incremental lag and must not send the UI
                // back to “first scan”. The previous atomic generation stays
                // authoritative until the next caught-up replacement.
                state.usage_history_complete.store(false, Ordering::Release);
            }
            state
                .usage_last_success_at
                .store(attempted_at, Ordering::Release);
            state.usage_consecutive_failures.store(0, Ordering::Release);
            true
        }
        _ => {
            state.usage_worker_failures.fetch_add(1, Ordering::AcqRel);
            state
                .usage_consecutive_failures
                .fetch_add(1, Ordering::AcqRel);
            false
        }
    }
}

fn usage_refresh_loop(state: AppState) {
    let mut question_scanner = QuestionScanner::default();
    while !state.shutdown_flag.load(Ordering::Acquire) {
        let success = run_usage_refresh_iteration(&state);
        observe_codex_questions(&state, &mut question_scanner);
        let delay = usage_refresh_delay(
            success,
            state.usage_collection_ready.load(Ordering::Acquire),
        );
        let steps = (delay.as_millis() / 100).max(1);
        for step in 0..steps {
            if state.shutdown_flag.load(Ordering::Acquire) {
                return;
            }
            thread::sleep(Duration::from_millis(100));
            if step % 10 == 9 {
                observe_codex_questions(&state, &mut question_scanner);
            }
        }
    }
}

fn observe_codex_questions(state: &AppState, scanner: &mut QuestionScanner) {
    let files = match state.usage.try_lock() {
        Ok(s) => s.collector.codex_source_files(),
        Err(_) => return,
    };
    let Ok(snapshot) = state
        .store
        .ui_snapshot(now_millis().saturating_sub(SESSION_LIST_RETENTION_MS))
    else {
        return;
    };
    let sessions = snapshot
        .sessions
        .into_iter()
        .filter(|s| s.provider == "codex")
        .map(|s| (s.provider_session_id, s.id))
        .collect();
    let observations = scanner.poll(&files, &sessions, now_millis());
    if let Ok(mut current) = state.codex.state.lock() {
        current
            .native_activities
            .retain(|id, _| sessions.values().any(|s| s == id));
        for observation in observations {
            match observation {
                Observation::TurnEnded {
                    thread_id,
                    turn_id,
                    event,
                    at,
                } => {
                    if state
                        .store
                        .observe_codex_turn_end(&thread_id, &turn_id, event, at)
                        .unwrap_or(false)
                    {
                        current.async_questions.clear_thread(&thread_id, at);
                        if let Some(session) = sessions.get(&thread_id) {
                            current.native_activities.remove(session);
                        }
                    }
                }
                Observation::Question(batch) => current.async_questions.observe(batch),
                Observation::Activity(event) => {
                    let entries = current
                        .native_activities
                        .entry(event.session_id.clone())
                        .or_default();
                    if !entries.iter().any(|e| e.event_id == event.event_id) {
                        entries.push(event);
                        let drop_count = entries.len().saturating_sub(64);
                        entries.drain(..drop_count);
                    }
                }
                Observation::Result {
                    session_id,
                    text,
                    cwd,
                    at,
                } => {
                    state.store.observe_result(
                        &session_id,
                        &text,
                        cwd.as_deref(),
                        at,
                        "codex:final_response",
                    );
                }
                Observation::Clear { thread_id, at } => {
                    current.async_questions.clear_thread(&thread_id, at);
                    if let Some(session) = sessions.get(&thread_id) {
                        state.store.clear_result(session, at);
                        current.native_activities.remove(session);
                    }
                }
            }
        }
    }
}

fn usage_refresh_delay(success: bool, caught_up: bool) -> Duration {
    if !success {
        USAGE_FAILURE_BACKOFF
    } else if caught_up {
        USAGE_LIVE_POLL_INTERVAL
    } else {
        USAGE_BACKFILL_POLL_INTERVAL
    }
}

#[allow(dead_code)]
fn review_baseline_loop(state: AppState) {
    while !state.shutdown_flag.load(Ordering::Acquire) {
        state
            .review_collection_in_progress
            .store(true, Ordering::Release);
        match state.store.pending_review_baselines(8) {
            Ok(candidates) => {
                state.review_collection_ready.store(true, Ordering::Release);
                state
                    .review_consecutive_failures
                    .store(0, Ordering::Release);
                state
                    .review_last_success_at
                    .store(now_millis(), Ordering::Release);
                for candidate in candidates {
                    if state.shutdown_flag.load(Ordering::Acquire) {
                        return;
                    }
                    if let Some(baseline) = capture_review_baseline(&candidate) {
                        let _ = state.store.write_review_baseline(baseline);
                    }
                }
            }
            Err(_) => {
                state
                    .review_consecutive_failures
                    .fetch_add(1, Ordering::AcqRel);
            }
        }
        state
            .review_collection_in_progress
            .store(false, Ordering::Release);
        for _ in 0..5 {
            if state.shutdown_flag.load(Ordering::Acquire) {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

#[allow(dead_code)]
fn capture_review_baseline(candidate: &ReviewBaselineCandidate) -> Option<ReviewBaselineInput> {
    let working_directory = candidate.working_directory.as_deref()?;
    let mut limitations = Vec::new();
    let repository_root = resolve_review_repository_root(working_directory, &mut limitations)?;
    let status = run_git(
        &repository_root,
        &["status", "--porcelain=v2", "-z", "--untracked-files=normal"],
    )
    .ok()
    .map(|output| parse_git_status(&output))?;
    let branch = run_git(&repository_root, &["branch", "--show-current"])
        .ok()
        .and_then(|output| String::from_utf8(output).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty() && value.len() <= 160);
    let head = full_git_head(&repository_root);
    let worktree_kind = git_worktree_kind(&repository_root);
    let numstat = head.as_ref().and_then(|_| {
        run_git(
            &repository_root,
            &["diff", "--numstat", "--no-ext-diff", "HEAD", "--"],
        )
        .ok()
        .map(|output| parse_git_numstat(&output))
    });
    Some(ReviewBaselineInput {
        session_id: candidate.session_id.clone(),
        turn_id: candidate.turn_id.clone(),
        repository_identity: review_repository_identity(&repository_root)?,
        repository_root,
        branch,
        head,
        worktree_kind,
        dirty: status.changed_files > 0,
        changed_files: status.changed_files,
        staged_files: status.staged_files,
        unstaged_files: status.unstaged_files,
        untracked_files: status.untracked_files,
        insertions: numstat.as_ref().map(|value| value.insertions),
        deletions: numstat.as_ref().map(|value| value.deletions),
        binary_files: numstat.as_ref().map(|value| value.binary_files),
        turn_started_at: candidate.turn_started_at,
        first_tool_at: candidate.first_tool_at,
        captured_at: now_millis(),
    })
}

fn runtime_usage_record(record: UsageRecord) -> SessionUsageRecord {
    let daily_usage = record
        .daily_usage
        .into_iter()
        .map(|day| actrealm_runtime::SessionUsageDailyRecord {
            day: day.day,
            model: day.model,
            input_tokens: day.input_tokens,
            output_tokens: day.output_tokens,
            cache_read_tokens: day.cache_read_tokens,
            cache_creation_tokens: day.cache_creation_tokens,
            reasoning_tokens: day.reasoning_tokens,
            token_total: day.token_total,
            estimated_cost_usd_micros: day.estimated_cost_usd_micros,
            cost_kind: day.cost_kind,
            pricing_source: day.pricing_source,
            message_count: day.message_count,
        })
        .collect();
    SessionUsageRecord {
        provider: record.provider,
        provider_session_id: record.provider_session_id,
        project_id: record.project_id,
        project_label: record.project_label,
        parent_provider_session_id: record.parent_provider_session_id,
        model: record.model,
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
        daily_usage,
    }
}

fn execution_is_unconfirmed(
    exec_state: &str,
    recovery: &str,
    last_event_at: u64,
    now: u64,
) -> bool {
    matches!(exec_state, "thinking" | "tool_running" | "compacting")
        && (recovery == "lost_control"
            || (recovery == "waiting_for_event" && now.saturating_sub(last_event_at) > 30_000))
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

fn external_process_recovery_state(
    provider: &str,
    pid: u32,
    bundle_id: Option<&str>,
    surface: Option<&str>,
) -> &'static str {
    if !process_alive(pid) {
        return "lost_control";
    }
    match direct_app_identity_matches(
        provider,
        bundle_id,
        surface,
        process_executable_path(pid).as_deref(),
    ) {
        Some(true) | None => "observing",
        Some(false) => "lost_control",
    }
}

fn detached_codex_can_be_observed(exec_state: &str, alive: bool, executable: Option<&str>) -> bool {
    !terminal_execution_state(exec_state)
        && alive
        && executable
            .and_then(|path| FilePath::new(path).file_name())
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("codex"))
}

/// A managed connector may restore an active persisted turn before it has
/// reloaded the matching thread. The Hook's recorded Provider PID is still
/// useful liveness evidence: keep a live process observable, and reject a dead
/// or reused process, instead of leaving both as `waiting_for_event`.
fn recovery_from_persisted_process(
    provider: &str,
    exec_state: &str,
    recovery: String,
    provider_pid: Option<u32>,
    bundle_id: Option<&str>,
    surface: Option<&str>,
) -> String {
    if terminal_execution_state(exec_state) || recovery != "waiting_for_event" {
        return recovery;
    }
    let Some(pid) = provider_pid else {
        return recovery;
    };
    external_process_recovery_state(provider, pid, bundle_id, surface).to_owned()
}

/// Returns `None` when the Hook only identifies a terminal surface. A terminal
/// can legitimately host many shells and provider launchers, so its bundle ID
/// is not executable identity evidence. Desktop app surfaces are direct
/// evidence and must match the live PID path before ActRealm claims observing.
fn direct_app_identity_matches(
    provider: &str,
    bundle_id: Option<&str>,
    surface: Option<&str>,
    executable_path: Option<&str>,
) -> Option<bool> {
    let provider = provider.to_ascii_lowercase();
    let bundle = bundle_id.unwrap_or_default().to_ascii_lowercase();
    let surface = surface.unwrap_or_default().to_ascii_lowercase();
    let (expected_provider, expected): (&str, &[&str]) = if surface == "codex_app"
        || matches!(bundle.as_str(), "com.openai.chat" | "com.openai.codex")
    {
        ("codex", &["codex", "chatgpt"])
    } else if surface == "claude_app" || bundle.contains("anthropic.claude") {
        ("claude", &["claude"])
    } else {
        return None;
    };
    if provider != expected_provider {
        return Some(false);
    }
    let Some(path) = executable_path else {
        return Some(false);
    };
    let path = path.to_ascii_lowercase();
    Some(expected.iter().any(|token| path.contains(token)))
}

#[cfg(target_os = "macos")]
fn process_executable_path(pid: u32) -> Option<String> {
    unsafe extern "C" {
        fn proc_pidpath(
            pid: libc::c_int,
            buffer: *mut libc::c_void,
            buffersize: u32,
        ) -> libc::c_int;
    }
    let pid = i32::try_from(pid).ok()?;
    let mut buffer = vec![0_u8; 4096];
    let count = unsafe {
        proc_pidpath(
            pid,
            buffer.as_mut_ptr().cast::<libc::c_void>(),
            u32::try_from(buffer.len()).ok()?,
        )
    };
    if count <= 0 {
        return None;
    }
    buffer.truncate(usize::try_from(count).ok()?);
    String::from_utf8(buffer).ok()
}

#[cfg(not(target_os = "macos"))]
fn process_executable_path(pid: u32) -> Option<String> {
    fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

fn quota_entries(state: &AppState) -> Result<Vec<QuotaEntry>, StoreError> {
    let official_codex = state.codex.rate_limit_entries();
    let mut quota = state
        .quota
        .lock()
        .map_err(|_| StoreError::Storage("quota collector lock is poisoned".to_owned()))?;
    let claude_cache_modified_at = fs::metadata(quota.collector.paths().claude_cache())
        .and_then(|metadata| metadata.modified())
        .ok();
    let claude_cache_changed = claude_cache_modified_at != quota.claude_cache_modified_at;
    let codex_rate_limits_changed = official_codex
        .as_ref()
        .is_some_and(|(_, captured_at)| Some(*captured_at) != quota.codex_rate_limits_captured_at);
    let refresh = claude_cache_changed
        || codex_rate_limits_changed
        || quota
            .refreshed_at
            .is_none_or(|instant| instant.elapsed() >= state.quota_poll_interval);
    if refresh {
        let now = now_millis();
        let mut entries = quota.collector.collect_claude(now);
        if quota.oauth_last_result.as_ref().is_some_and(Result::is_err) {
            mark_claude_quota_stale(&mut entries);
        }
        let fallback_codex = if official_codex
            .as_ref()
            .is_none_or(|(entries, _)| !has_standard_codex_quota(entries))
        {
            quota.collector.collect_codex(now)
        } else {
            Vec::new()
        };
        let (codex_entries, codex_captured_at) = select_codex_quota_entries(
            &quota.entries,
            quota.codex_rate_limits_captured_at,
            official_codex,
            fallback_codex,
            now,
        );
        entries.extend(codex_entries);
        quota.codex_rate_limits_captured_at = codex_captured_at;
        quota.entries = entries;
        quota.refreshed_at = Some(Instant::now());
        quota.claude_cache_modified_at = claude_cache_modified_at;
        let persisted = quota.entries.iter().filter_map(quota_record).collect();
        state.store.replace_quota_snapshots(persisted)?;
    }
    let entries = quota.entries.clone();
    drop(quota);
    start_oauth_quota_refresh(state, false);
    Ok(entries)
}

fn mark_claude_quota_stale(entries: &mut [QuotaEntry]) {
    for entry in entries
        .iter_mut()
        .filter(|entry| entry.provider == "claude")
    {
        *entry = entry.clone().mark_stale(
            "quota.reason.claude_refresh_failed",
            "Claude quota refresh failed; showing the last captured value.",
        );
    }
}

fn oauth_poll_due(in_progress: bool, next_poll_at: u64, now: u64, force: bool) -> bool {
    !in_progress && (force || now >= next_poll_at)
}

fn start_oauth_quota_refresh(state: &AppState, force: bool) {
    if !state.claude_oauth_quota {
        return;
    }
    let now = now_millis();
    let mut collector = {
        let Ok(mut quota) = state.quota.lock() else {
            return;
        };
        if !oauth_poll_due(
            quota.oauth_refresh_in_progress,
            quota.oauth_next_poll_at,
            now,
            force,
        ) {
            return;
        }
        quota.oauth_refresh_in_progress = true;
        // Keep the previous failure visible until a successful response replaces it.
        quota.oauth_next_poll_at = now.saturating_add(state.quota_poll_interval.as_millis() as u64);
        // Keep credential-refresh cooldown and HTTP backoff across polls.
        // Snapshot reads use an independent cache-only placeholder meanwhile.
        let placeholder = QuotaCollector::new(quota.collector.paths().clone());
        std::mem::replace(&mut quota.collector, placeholder)
    };
    let quota_state = state.quota.clone();
    let store = state.store.clone();
    let codex = state.codex.clone();
    let result = thread::Builder::new()
        .name("actrealm-claude-quota".to_owned())
        .spawn(move || {
            let result = collector.refresh_claude_oauth(now);
            let official_codex = codex.rate_limit_entries();
            let fallback_codex = if official_codex.as_ref()
                .is_none_or(|(entries, _)| !has_standard_codex_quota(entries)) {
                collector.collect_codex(now_millis())
            } else { Vec::new() };
            let Ok(mut quota) = quota_state.lock() else { return };
            quota.collector = collector;
            match result {
                Ok(mut entries) => {
                    let captured_at = entries.iter().filter_map(|entry| entry.captured_at).max().unwrap_or(now);
                    let (codex_entries, codex_captured_at) = select_codex_quota_entries(
                        &quota.entries, quota.codex_rate_limits_captured_at,
                        official_codex, fallback_codex, now_millis(),
                    );
                    entries.extend(codex_entries);
                    quota.entries = entries;
                    quota.codex_rate_limits_captured_at = codex_captured_at;
                    quota.claude_cache_modified_at = fs::metadata(quota.collector.paths().claude_cache())
                        .and_then(|metadata| metadata.modified()).ok();
                    quota.refreshed_at = Some(Instant::now());
                    quota.oauth_last_result = Some(Ok(captured_at));
                }
                Err(error) => {
                    mark_claude_quota_stale(&mut quota.entries);
                    // Network may still be resuming after wake. Retry promptly,
                    // while the collector continues to enforce HTTP 429 backoff.
                    if matches!(&error, QuotaError::OAuthRequest(message)
                        if message != "credential was rejected" && message != "temporarily rate limited") {
                        quota.oauth_next_poll_at = now_millis().saturating_add(10_000);
                    }
                    quota.oauth_last_result = Some(Err((quota_refresh_error_code(&error), quota_refresh_error_detail(&error))));
                }
            }
            let persisted = quota.entries.iter().filter_map(quota_record).collect::<Vec<_>>();
            if store.replace_quota_snapshots(persisted).is_err() {
                quota.oauth_last_result = Some(Err(("QUOTA_PERSIST_FAILED", "Could not persist the quota snapshot".into())));
            }
            quota.oauth_refresh_in_progress = false;
        });
    if result.is_err() {
        if let Ok(mut quota) = state.quota.lock() {
            quota.oauth_refresh_in_progress = false;
            quota.oauth_last_result = Some(Err((
                "CLAUDE_QUOTA_REFRESH_FAILED",
                "Could not start quota refresh".into(),
            )));
        }
    }
}

fn has_standard_codex_quota(entries: &[QuotaEntry]) -> bool {
    entries.iter().any(|entry| {
        entry.provider == "codex"
            && quota_kind(entry) == "standard"
            && (entry.used_pct.is_some() || entry.remaining_pct.is_some())
    })
}

fn select_codex_quota_entries(
    previous: &[QuotaEntry],
    previous_captured_at: Option<u64>,
    official: Option<(Vec<QuotaEntry>, u64)>,
    fallback: Vec<QuotaEntry>,
    now_ms: u64,
) -> (Vec<QuotaEntry>, Option<u64>) {
    if let Some((entries, captured_at)) = official.as_ref() {
        if has_standard_codex_quota(entries) {
            return (entries.clone(), Some(*captured_at));
        }
    }

    let retained = previous
        .iter()
        .filter(|entry| {
            entry.provider == "codex"
                && entry.source == CODEX_APP_SERVER_SOURCE
                && entry
                    .resets_at
                    .is_some_and(|reset| reset.saturating_mul(1_000) > now_ms)
        })
        .cloned()
        .collect::<Vec<_>>();
    if has_standard_codex_quota(&retained) {
        let marker = official
            .as_ref()
            .map(|(_, captured_at)| *captured_at)
            .or(previous_captured_at);
        return (
            retained
                .into_iter()
                .map(|entry| {
                    entry.mark_stale(
                        "quota.reason.codex_refresh_failed",
                        "Codex quota refresh failed. The last successfully captured values are shown.",
                    )
                })
                .collect(),
            marker,
        );
    }

    (fallback, None)
}

fn quota_record(entry: &QuotaEntry) -> Option<QuotaRecord> {
    Some(QuotaRecord {
        provider: entry.provider.clone(),
        window: entry.window.clone(),
        limit_id: entry.limit_id.clone(),
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

fn api_error_detail(status: StatusCode, code: &'static str, _detail: &str) -> Response {
    // Provider, filesystem and process errors may contain local paths, command
    // arguments or credentials. The UI maps stable codes to actionable copy;
    // raw internal detail remains out of the HTTP boundary.
    api_error(status, code)
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
    auth.session_token
        .as_deref()
        .is_some_and(|token| constant_time_eq(token, cookie))
        || auth.native_sessions.iter().any(|session| {
            session.kind == NativeSessionKind::Full && constant_time_eq(&session.token, cookie)
        })
}

fn authorized_mutation(state: &AppState, headers: &HeaderMap) -> bool {
    if !valid_same_origin(state, headers) {
        return false;
    }
    let Some(cookie) = headers
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| cookie_value(value, SESSION_COOKIE))
    else {
        return false;
    };
    let Some(csrf) = headers
        .get(CSRF_HEADER)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Ok(auth) = state.auth.lock() else {
        return false;
    };
    (auth
        .session_token
        .as_deref()
        .is_some_and(|token| constant_time_eq(token, cookie))
        && auth
            .csrf_token
            .as_deref()
            .is_some_and(|token| constant_time_eq(token, csrf)))
        || auth.native_sessions.iter().any(|session| {
            session.kind == NativeSessionKind::Full
                && constant_time_eq(&session.token, cookie)
                && constant_time_eq(&session.csrf, csrf)
        })
}

fn authorized_setup(state: &AppState, headers: &HeaderMap, mutation: bool) -> bool {
    if if mutation {
        authorized_mutation(state, headers)
    } else {
        authorized(state, headers)
    } {
        return true;
    }
    if !valid_host(state, headers) || (mutation && !valid_same_origin(state, headers)) {
        return false;
    }
    let Some(cookie) = headers
        .get(COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| cookie_value(v, SESSION_COOKIE))
    else {
        return false;
    };
    let csrf = headers.get(CSRF_HEADER).and_then(|v| v.to_str().ok());
    let Ok(auth) = state.auth.lock() else {
        return false;
    };
    let Ok(companions) = state.companions.lock() else {
        return false;
    };
    auth.native_sessions.iter().any(|session| {
        constant_time_eq(&session.token, cookie)
            && companions.registrations.iter().any(|entry| {
                Some(&entry.id) == session.companion_id.as_ref()
                    && (!mutation
                        || entry
                            .scopes
                            .iter()
                            .any(|scope| scope == COMPANION_SCOPE_RESPOND))
            })
            && (!mutation
                || (session.kind == NativeSessionKind::SetupReadWrite
                    && csrf.is_some_and(|value| constant_time_eq(&session.csrf, value))))
    })
}

async fn native_session_health(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authorized_setup(&state, &headers, false) {
        return api_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED");
    }
    Json(json!({"ok": true, "protocolVersion": PUBLIC_PROTOCOL_VERSION, "instanceId": state.instance_id})).into_response()
}

fn websocket_protocol(headers: &HeaderMap) -> Option<String> {
    headers
        .get(SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .split(',')
                .map(str::trim)
                .find(|protocol| protocol.starts_with(WS_PROTOCOL_PREFIX))
        })
        .map(ToOwned::to_owned)
}

fn consume_websocket_ticket(state: &AppState, candidate: &str) -> bool {
    let Ok(mut auth) = state.auth.lock() else {
        return false;
    };
    let now = Instant::now();
    auth.websocket_tickets
        .retain(|(_, expires_at)| *expires_at > now);
    let Some(index) = auth
        .websocket_tickets
        .iter()
        .position(|(ticket, _)| constant_time_eq(ticket, candidate))
    else {
        return false;
    };
    auth.websocket_tickets.swap_remove(index);
    true
}

fn generate_secret() -> Result<String, getrandom::Error> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)?;
    let mut encoded = String::with_capacity(bytes.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(encoded)
}

fn validated_preserved_auth(
    session_token: Option<String>,
    csrf_token: Option<String>,
) -> Result<(Option<String>, Option<String>), String> {
    match (session_token, csrf_token) {
        (None, None) => Ok((None, None)),
        (Some(session), Some(csrf))
            if valid_preserved_secret(&session) && valid_preserved_secret(&csrf) =>
        {
            Ok((Some(session), Some(csrf)))
        }
        _ => Err("preserved Runtime authentication is invalid".to_owned()),
    }
}

fn valid_preserved_secret(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let maximum = left.len().max(right.len());
    let mut difference = left.len() ^ right.len();
    for index in 0..maximum {
        let left_byte = left.get(index).copied().unwrap_or_default();
        let right_byte = right.get(index).copied().unwrap_or_default();
        difference |= usize::from(left_byte ^ right_byte);
    }
    difference == 0
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

fn credential_stamp(path: &FilePath) -> Option<CredentialStamp> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() {
        return None;
    }
    Some(CredentialStamp {
        device: metadata.dev(),
        inode: metadata.ino(),
        modified: metadata.modified().ok()?,
        modified_nanos: metadata.mtime_nsec(),
        changed: metadata.ctime(),
        changed_nanos: metadata.ctime_nsec(),
        length: metadata.len(),
    })
}

fn credential_stamp_changed(
    connected: &Option<CredentialStamp>,
    current: &Option<CredentialStamp>,
) -> bool {
    connected != current
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use serde_json::Value;
    use std::fs::{self, File};
    use std::os::unix::fs::PermissionsExt;
    use tower::ServiceExt;

    fn authorized_request(method: &str, uri: &str, body: Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(HOST, "127.0.0.1:43111")
            .header(ORIGIN, "http://127.0.0.1:43111")
            .header(COOKIE, "actrealm_session=test-session")
            .header(CSRF_HEADER, "test-csrf")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn companion_request(method: &str, uri: &str, token: &str, body: Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(HOST, "127.0.0.1:43111")
            .header(AUTHORIZATION, format!("Bearer {token}"))
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

    #[test]
    fn artifact_reveal_checks_exit_status_and_times_out() {
        assert!(wait_for_artifact_reveal(
            ProcessCommand::new("/usr/bin/true"),
            Duration::from_secs(1)
        ));
        assert!(!wait_for_artifact_reveal(
            ProcessCommand::new("/usr/bin/false"),
            Duration::from_secs(1)
        ));
        assert!(!wait_for_artifact_reveal(
            ProcessCommand::new("/nonexistent/actrealm-reveal"),
            Duration::from_secs(1)
        ));
        let mut delayed = ProcessCommand::new("/bin/sleep");
        delayed.arg("10");
        let start = Instant::now();
        assert!(!wait_for_artifact_reveal(
            delayed,
            Duration::from_millis(20)
        ));
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn companion_results_are_current_turn_only_and_reveal_requires_jump_scope() {
        let root = std::env::temp_dir().join(format!("actrealm-result-api-{}", Uuid::now_v7()));
        fs::create_dir_all(&root).unwrap();
        let artifact = root.join("report.html");
        fs::write(&artifact, "test report").unwrap();
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let state = test_state(store.clone(), &root);
        let token = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        state
            .companions
            .lock()
            .unwrap()
            .registrations
            .push(CompanionRegistration {
                id: "result-client".into(),
                client_name: "Result test".into(),
                token_hash: secret_hash(token),
                scopes: vec![COMPANION_SCOPE_SNAPSHOT.into()],
                created_at: now_millis(),
            });
        let at = now_millis();
        let session = store.ingest(BridgeRequest::from_hook_at(Provider::Codex, json!({
            "hook_event_name":"UserPromptSubmit", "session_id":"result-api", "turn_id":"t1", "cwd":root
        }), at)).unwrap().session_id;
        store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Codex,
                json!({
                    "hook_event_name":"Stop", "session_id":"result-api", "turn_id":"t1", "cwd":root,
                    "last_assistant_message":format!("结果可验收 [报告]({})", artifact.display())
                }),
                at + 1,
            ))
            .unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let uri = format!("/api/v1/companion/sessions/{session}/result");
            let denied = router(state.clone()).oneshot(companion_request("GET", &uri, "wrong", Value::Null)).await.unwrap();
            assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
            let response = router(state.clone()).oneshot(companion_request("GET", &uri, token, Value::Null)).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body = json_body(response).await;
            assert!(body["result"]["summary"].as_str().unwrap().contains("结果可验收"));
            assert_eq!(body["result"]["artifacts"][0]["canReveal"], false);
            assert!(!body.to_string().contains(&root.to_string_lossy().to_string()));
            let artifact_id = body["result"]["artifacts"][0]["id"].as_str().unwrap();
            let reveal = format!("/api/v1/companion/sessions/{session}/artifacts/{artifact_id}/reveal");
            let denied = router(state.clone()).oneshot(companion_request("POST", &reveal, token, Value::Null)).await.unwrap();
            assert_eq!(denied.status(), StatusCode::FORBIDDEN);
            let native = format!("{reveal}?native=true");
            let denied = router(state.clone()).oneshot(companion_request("POST", &native, token, Value::Null)).await.unwrap();
            assert_eq!(denied.status(), StatusCode::FORBIDDEN);
            state.companions.lock().unwrap().registrations[0].scopes.push(COMPANION_SCOPE_JUMP.into());
            let target = router(state.clone()).oneshot(companion_request("POST", &native, token, Value::Null)).await.unwrap();
            assert_eq!(target.status(), StatusCode::OK);
            assert_eq!(json_body(target).await["localPath"], artifact.to_string_lossy().as_ref());
            fs::remove_file(&artifact).unwrap();
            let missing = router(state.clone()).oneshot(companion_request("POST", &native, token, Value::Null)).await.unwrap();
            assert_eq!(missing.status(), StatusCode::NOT_FOUND);
            let response = router(state.clone()).oneshot(companion_request("POST", &reveal, token, Value::Null)).await.unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            store.ingest(BridgeRequest::from_hook_at(Provider::Codex, json!({"hook_event_name":"UserPromptSubmit", "session_id":"result-api", "turn_id":"t2"}), at+2)).unwrap();
            let response = router(state.clone()).oneshot(companion_request("GET", &uri, token, Value::Null)).await.unwrap();
            assert!(json_body(response).await["result"].is_null());
        });
        drop(state);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn companion_pairing_is_explicit_scoped_persistent_and_revocable() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-companion-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let state = test_state(store.clone(), &root);
        persist_companion_discovery(
            &state.data_paths.companion_discovery,
            "http://127.0.0.1:43111",
            "runtime-instance",
        )
        .unwrap();
        let discovery = fs::read_to_string(&state.data_paths.companion_discovery).unwrap();
        assert_eq!(
            fs::metadata(&state.data_paths.companion_discovery)
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
        assert!(discovery.contains("http://127.0.0.1:43111"));
        assert!(discovery.contains("runtime-instance"));
        assert!(!discovery.contains("token"));
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
            let pairing = router(state.clone())
                .oneshot(authorized_request(
                    "POST",
                    "/api/v1/companions/pairing",
                    json!({"clientName":"Display Companion","allowControl":true}),
                ))
                .await
                .unwrap();
            assert_eq!(pairing.status(), StatusCode::OK);
            let pairing = json_body(pairing).await;
            assert_eq!(
                pairing["scopes"],
                json!(["snapshot.read", "session.jump", "attention.respond"])
            );
            let code = pairing["enrollmentCode"].as_str().unwrap();
            assert!(code.starts_with("AR1:43111:"));

            let enrollment = Request::builder()
                .method("POST")
                .uri("/api/v1/companion/enroll")
                .header(HOST, "127.0.0.1:43111")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"enrollmentCode":code}).to_string()))
                .unwrap();
            let enrollment = router(state.clone()).oneshot(enrollment).await.unwrap();
            assert_eq!(enrollment.status(), StatusCode::OK);
            let enrollment = json_body(enrollment).await;
            let token = enrollment["token"].as_str().unwrap();
            let companion_id = enrollment["companionId"].as_str().unwrap();

            store
                .ingest(BridgeRequest::from_hook_at(
                    Provider::Claude,
                    json!({
                        "hook_event_name": "UserPromptSubmit",
                        "session_id": "companion-activity-session",
                        "cwd": "/tmp/companion",
                        "prompt": "Inspect the local companion projection",
                        "session_title": "Companion projection audit"
                    }),
                    1_000,
                ))
                .unwrap();
            store
                .ingest(BridgeRequest::from_hook_at(
                    Provider::Claude,
                    json!({
                        "hook_event_name": "PreToolUse",
                        "session_id": "companion-activity-session",
                        "cwd": "/tmp/companion",
                        "tool_name": "Read",
                        "tool_use_id": "read-1",
                        "tool_input": {"file_path": "/tmp/companion/private.txt"}
                    }),
                    1_001,
                ))
                .unwrap();

            let snapshot = router(state.clone())
                .oneshot(companion_request(
                    "GET",
                    "/api/v1/companion/snapshot",
                    token,
                    Value::Null,
                ))
                .await
                .unwrap();
            assert_eq!(snapshot.status(), StatusCode::OK);
            let snapshot = json_body(snapshot).await;
            assert_eq!(snapshot["schemaVersion"], 1);
            assert_eq!(snapshot["capabilities"]["canRespond"], true);
            assert_eq!(snapshot["capabilities"]["canJump"], true);
            assert_eq!(
                snapshot["sessions"][0]["providerTitle"],
                "Companion projection audit"
            );
            assert_eq!(
                snapshot["sessions"][0]["title"],
                "Inspect the local companion projection"
            );
            assert_eq!(snapshot["sessions"][0]["currentTarget"], "private.txt");
            assert_eq!(snapshot["sessions"][0]["facts"]["schemaVersion"], 1);
            assert_eq!(
                snapshot["sessions"][0]["facts"]["currentTarget"]["sourceId"],
                "hook:tool_input/allowlisted_basename"
            );
            assert!(!snapshot.to_string().contains("/tmp/companion/private.txt"));
            let session_id = snapshot["sessions"][0]["id"].as_str().unwrap();
            let activity = router(state.clone())
                .oneshot(companion_request(
                    "GET",
                    &format!("/api/v1/companion/sessions/{session_id}/activity?limit=50"),
                    token,
                    Value::Null,
                ))
                .await
                .unwrap();
            assert_eq!(activity.status(), StatusCode::OK);
            let activity = json_body(activity).await;
            assert_eq!(activity["events"].as_array().unwrap().len(), 2);
            assert_eq!(activity["events"][1]["kind"], "tool.started");
            assert_eq!(activity["events"][1]["toolName"], "Read");
            assert!(!activity.to_string().contains("private.txt"));

            let review = router(state.clone())
                .oneshot(companion_request(
                    "GET",
                    &format!("/api/v1/companion/sessions/{session_id}/review"),
                    token,
                    Value::Null,
                ))
                .await
                .unwrap();
            assert_eq!(review.status(), StatusCode::OK);
            let review = json_body(review).await;
            assert_eq!(review["schemaVersion"], REVIEW_SCHEMA_VERSION);
            assert_eq!(review["sessionId"], session_id);
            assert!(review.get("repository").is_some());
            assert!(!review.to_string().contains("/tmp/companion"));
            assert!(!review.to_string().contains("private.txt"));

            let completion_settings = router(state.clone())
                .oneshot(companion_request(
                    "GET",
                    "/api/v1/companion/settings/completion",
                    token,
                    Value::Null,
                ))
                .await
                .unwrap();
            assert_eq!(completion_settings.status(), StatusCode::OK);
            let completion_settings = json_body(completion_settings).await;
            assert_eq!(completion_settings["mode"], "afterConfirmation");

            let updated_settings = router(state.clone())
                .oneshot(companion_request(
                    "PUT",
                    "/api/v1/companion/settings/completion",
                    token,
                    json!({"mode":"manual","minutes":15}),
                ))
                .await
                .unwrap();
            assert_eq!(updated_settings.status(), StatusCode::OK);
            let updated_settings = json_body(updated_settings).await;
            assert_eq!(updated_settings["mode"], "manual");
            assert_eq!(
                load_ui_settings(&state).unwrap().completion_task_hide_mode,
                "manual"
            );

            let file = fs::read_to_string(&state.data_paths.companion_auth).unwrap();
            assert!(!file.contains(token));
            assert_eq!(
                fs::metadata(&state.data_paths.companion_auth)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o077,
                0
            );

            let revoke = router(state.clone())
                .oneshot(authorized_request(
                    "DELETE",
                    &format!("/api/v1/companions/{companion_id}"),
                    Value::Null,
                ))
                .await
                .unwrap();
            assert_eq!(revoke.status(), StatusCode::OK);

            let revoked = router(state.clone())
                .oneshot(companion_request(
                    "GET",
                    "/api/v1/companion/snapshot",
                    token,
                    Value::Null,
                ))
                .await
                .unwrap();
            assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);
        });
        drop(state);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sparse_codex_rate_limit_updates_merge_without_clearing_known_fields() {
        let state = Arc::new(Mutex::new(CodexManagerState {
            rate_limits: Some(json!({
                "rateLimits": {
                    "limitId": "codex",
                    "planType": "plus",
                    "primary": {
                        "usedPercent": 10,
                        "windowDurationMins": 300,
                        "resetsAt": 1_000
                    },
                    "secondary": {
                        "usedPercent": 20,
                        "windowDurationMins": 43_800,
                        "resetsAt": 2_000
                    }
                }
            })),
            ..CodexManagerState::default()
        }));
        update_codex_rate_limits(
            &state,
            &json!({
                "rateLimits": {
                    "limitId": "codex",
                    "planType": null,
                    "primary": {"usedPercent": 35, "resetsAt": null}
                }
            }),
            42,
        );
        let current = state.lock().unwrap();
        let snapshot = current.rate_limits.as_ref().unwrap();
        assert_eq!(snapshot["rateLimits"]["primary"]["usedPercent"], 35);
        assert_eq!(snapshot["rateLimits"]["primary"]["resetsAt"], 1_000);
        assert_eq!(snapshot["rateLimits"]["planType"], "plus");
        assert_eq!(
            snapshot["rateLimits"]["secondary"]["windowDurationMins"],
            43_800
        );
        assert_eq!(current.rate_limits_captured_at, Some(42));
        assert_eq!(current.rate_limits_error, None);
    }

    #[test]
    fn codex_credential_change_triggers_only_one_reconnect_attempt() {
        let mut state = CodexManagerState::default();
        assert!(begin_codex_credential_reconnect(&mut state));
        assert!(!begin_codex_credential_reconnect(&mut state));
    }

    #[test]
    fn restart_timeout_cancels_request_before_a_later_wake_can_accept_it() {
        let coordination = Arc::new(AtomicUsize::new(RESTART_REQUEST_PENDING.into()));
        let (response_sender, response_receiver) = std_mpsc::channel();
        let request = RuntimeRestartRequest {
            bootstrap_token: Uuid::now_v7().to_string(),
            api_bind: "127.0.0.1:43121".parse().unwrap(),
            session_token: None,
            csrf_token: None,
            coordination: coordination.clone(),
            expires_at: Instant::now() + Duration::from_secs(1),
            response_sender,
        };
        let waiter = RuntimeRestartWaiter {
            receiver: response_receiver,
            coordination,
        };

        assert!(matches!(
            waiter.recv_timeout(Duration::from_millis(1)),
            Err(std_mpsc::RecvTimeoutError::Timeout)
        ));
        assert!(!request.try_accept());
    }

    #[test]
    fn accepted_restart_request_is_not_reported_as_a_timeout() {
        let coordination = Arc::new(AtomicUsize::new(RESTART_REQUEST_PENDING.into()));
        let (response_sender, response_receiver) = std_mpsc::channel();
        let request = RuntimeRestartRequest {
            bootstrap_token: Uuid::now_v7().to_string(),
            api_bind: "127.0.0.1:43121".parse().unwrap(),
            session_token: None,
            csrf_token: None,
            coordination: coordination.clone(),
            expires_at: Instant::now() + Duration::from_secs(1),
            response_sender,
        };
        let waiter = RuntimeRestartWaiter {
            receiver: response_receiver,
            coordination,
        };

        assert!(request.try_accept());
        let responder = thread::spawn(move || {
            thread::sleep(Duration::from_millis(5));
            request.respond(Ok(()));
        });
        assert!(matches!(
            waiter.recv_timeout(Duration::from_millis(1)),
            Ok(Ok(()))
        ));
        responder.join().unwrap();
    }

    #[test]
    fn codex_restart_failure_drains_managed_and_native_waiting_state() {
        let first = Uuid::now_v7();
        let second = Uuid::now_v7();
        let mut state = CodexManagerState::default();
        state
            .managed_request_ids
            .insert("rpc-1".to_owned(), (first, "thread-1".to_owned()));
        state
            .managed_request_ids
            .insert("rpc-2".to_owned(), (second, "thread-2".to_owned()));
        state.native_waiting.insert("thread-3".to_owned(), true);
        state.native_synced.insert("thread-3".to_owned());

        let (request_ids, thread_ids) = take_codex_pending_requests(&mut state);

        assert_eq!(request_ids.len(), 2);
        assert!(request_ids.contains(&first));
        assert!(request_ids.contains(&second));
        assert_eq!(thread_ids, vec!["thread-3"]);
        assert!(state.managed_request_ids.is_empty());
        assert!(state.native_waiting.is_empty());
        assert!(state.native_synced.is_empty());
    }

    #[test]
    fn codex_auth_stamp_detects_account_file_replacement() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-codex-auth-stamp-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        fs::create_dir_all(&root).unwrap();
        let auth = root.join("auth.json");
        fs::write(&auth, b"old").unwrap();
        let connected = credential_stamp(&auth);
        fs::write(&auth, b"new-account").unwrap();
        let current = credential_stamp(&auth);
        assert!(credential_stamp_changed(&connected, &current));
        assert!(!credential_stamp_changed(&current, &current));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preserved_restart_auth_requires_a_complete_valid_pair() {
        let session = "a".repeat(64);
        let csrf = "b".repeat(64);
        assert_eq!(
            validated_preserved_auth(Some(session.clone()), Some(csrf.clone())).unwrap(),
            (Some(session), Some(csrf))
        );
        assert!(validated_preserved_auth(Some("a".repeat(64)), None).is_err());
        assert!(
            validated_preserved_auth(Some("not-hex".to_owned()), Some("b".repeat(64))).is_err()
        );
    }

    #[test]
    fn codex_auth_stamp_detects_same_size_same_mtime_replacement() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-codex-auth-content-stamp-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        fs::create_dir_all(&root).unwrap();
        let reference = root.join("reference");
        let auth = root.join("auth.json");
        fs::write(&reference, b"reference").unwrap();
        fs::write(&auth, b"account-a").unwrap();
        assert!(ProcessCommand::new("touch")
            .args(["-r", reference.to_str().unwrap(), auth.to_str().unwrap()])
            .status()
            .unwrap()
            .success());
        let connected = credential_stamp(&auth);

        fs::write(&auth, b"account-b").unwrap();
        assert!(ProcessCommand::new("touch")
            .args(["-r", reference.to_str().unwrap(), auth.to_str().unwrap()])
            .status()
            .unwrap()
            .success());
        let current = credential_stamp(&auth);

        assert!(credential_stamp_changed(&connected, &current));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn old_claude_quota_remains_explicitly_stale_across_background_repolls() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-claude-stale-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let cache = root.join("actrealm-home/cache/claude-rl.json");
        fs::create_dir_all(cache.parent().unwrap()).unwrap();
        fs::write(
            &cache,
            br#"{"schemaVersion":1,"provider":"claude","source":"oauth_usage","capturedAt":100,"windows":[{"window":"5h","usedPct":25.0,"resetsAt":200}]}"#,
        )
        .unwrap();
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let state = test_state(store, &root);
        let first = quota_entries(&state).unwrap();
        assert_eq!(first[0].status, "stale");
        state.quota.lock().unwrap().refreshed_at = None;
        let second = quota_entries(&state).unwrap();
        assert_eq!(second[0].status, "stale");
        assert_eq!(second[0].remaining_pct, Some(75.0));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn incomplete_codex_refresh_keeps_the_last_official_pro_snapshot() {
        let previous = codex_app_server_entries(
            &json!({
                "rateLimits": {
                    "limitId": "codex",
                    "planType": "pro",
                    "primary": {
                        "usedPercent": 45,
                        "windowDurationMins": 10080,
                        "resetsAt": 2_000
                    }
                },
                "rateLimitsByLimitId": {
                    "codex": {
                        "limitId": "codex",
                        "planType": "pro",
                        "primary": {
                            "usedPercent": 45,
                            "windowDurationMins": 10080,
                            "resetsAt": 2_000
                        }
                    },
                    "codex_bengalfox": {
                        "limitId": "codex_bengalfox",
                        "limitName": "GPT-5.3-Codex-Spark",
                        "planType": "pro",
                        "primary": {
                            "usedPercent": 0,
                            "windowDurationMins": 300,
                            "resetsAt": 1_500
                        }
                    }
                }
            }),
            900_000,
        );
        let incomplete = codex_app_server_entries(
            &json!({
                "rateLimits": {
                    "limitId": "codex_bengalfox",
                    "limitName": "GPT-5.3-Codex-Spark",
                    "primary": {
                        "usedPercent": 0,
                        "windowDurationMins": 300,
                        "resetsAt": 1_500
                    }
                }
            }),
            1_000_000,
        );

        let (selected, marker) = select_codex_quota_entries(
            &previous,
            Some(900_000),
            Some((incomplete.clone(), 1_000_000)),
            incomplete,
            1_000_000,
        );

        assert_eq!(marker, Some(1_000_000));
        assert!(has_standard_codex_quota(&selected));
        assert!(selected.iter().all(|entry| entry.status == "stale"));
        assert!(selected.iter().any(|entry| {
            entry.limit_id.as_deref() == Some("codex")
                && entry.plan_type.as_deref() == Some("pro")
                && entry.remaining_pct == Some(55.0)
                && entry.reason_code.as_deref() == Some("quota.reason.codex_refresh_failed")
        }));
    }

    #[test]
    fn spark_only_fallback_does_not_invent_a_pro_account() {
        let fallback = codex_app_server_entries(
            &json!({
                "rateLimits": {
                    "limitId": "codex_bengalfox",
                    "limitName": "GPT-5.3-Codex-Spark",
                    "primary": {
                        "usedPercent": 0,
                        "windowDurationMins": 300,
                        "resetsAt": 1_500
                    }
                }
            }),
            1_000_000,
        );

        let (selected, marker) =
            select_codex_quota_entries(&[], None, None, fallback.clone(), 1_000_000);

        assert_eq!(selected, fallback);
        assert_eq!(marker, None);
        assert!(!has_standard_codex_quota(&selected));
    }

    #[test]
    fn async_question_answers_target_the_attached_turn_and_observation_stays_readonly() {
        let root = std::env::temp_dir().join(format!("actrealm-async-answer-{}", Uuid::now_v7()));
        fs::create_dir_all(&root).unwrap();
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let mut state = test_state(store.clone(), &root);
        let executable = root.join("fake-codex");
        fs::write(&executable, r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{"id":%s,"result":{"userAgent":"codex_cli_rs/0.153.4"}}\n' "$id" ;;
    *'"method":"turn/steer"'*'"expectedTurnId":"turn-async"'*'"text":"Layout?\nWide"'*) printf '{"id":%s,"result":{"turnId":"turn-async"}}\n' "$id" ;;
    *'"method":"turn/steer"'*) printf '{"id":%s,"error":{"code":-32602,"message":"wrong turn or answer"}}\n' "$id" ;;
  esac
done
"#).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let (connector, _channels) =
            CodexConnector::connect(&executable, &root.join("test.sock")).unwrap();
        state.codex.connector = Some(connector.clone());
        store.ingest(BridgeRequest::from_hook_at(Provider::Codex, json!({
            "hook_event_name":"UserPromptSubmit", "session_id":"async-thread", "turn_id":"turn-async", "prompt":"Test asynchronous question"
        }), now_millis())).unwrap();
        {
            let mut current = state.codex.state.lock().unwrap();
            current.managed.insert("async-thread".to_owned());
            current.threads.insert(
                "async-thread".to_owned(),
                CodexThread {
                    id: "async-thread".to_owned(),
                    name: None,
                    cwd: None,
                    status: "active".to_owned(),
                    active_flags: vec![],
                    updated_at: None,
                    approval_policy: None,
                    approvals_reviewer: None,
                    sandbox_mode: None,
                },
            );
        }
        update_codex_notification(
            &state.codex.state,
            &store,
            &state.waiters,
            ServerNotification {
                method: "item/completed".into(),
                params: json!({"threadId":"async-thread","turnId":"turn-async","item":{
                    "type":"agentMessage", "id":"question-item", "questions":[{"title":"Layout?","options":["Wide","Compact"]}]
                }}),
            },
        );
        let readonly = companion_snapshot_value(
            &state,
            &CompanionAuthorization {
                id: "test".into(),
                scopes: vec![COMPANION_SCOPE_SNAPSHOT.into()],
            },
        )
        .unwrap();
        assert_eq!(readonly["asyncQuestions"][0]["canAnswer"], false);
        assert!(readonly["asyncQuestions"][0].get("route").is_none());
        assert!(store.snapshot().unwrap().attention.is_empty());
        let batch = state
            .codex
            .state
            .lock()
            .unwrap()
            .async_questions
            .snapshot(now_millis())[0]
            .clone();
        assert!(batch.can_answer);
        assert_eq!(
            process_async_answer(&state, batch.id, vec!["Wide".into()]).status(),
            StatusCode::OK
        );
        assert_eq!(
            process_async_answer(&state, batch.id, vec!["Wide".into()]).status(),
            StatusCode::CONFLICT
        );
        assert!(state
            .codex
            .state
            .lock()
            .unwrap()
            .async_questions
            .snapshot(now_millis())
            .is_empty());
        assert!(!serde_json::to_string(&store.snapshot().unwrap())
            .unwrap()
            .contains("Layout?"));
        connector.shutdown();
        drop(state);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn independent_connector_listing_does_not_finish_a_desktop_task() {
        let mut thread = CodexThread {
            id: "desktop-thread".to_owned(),
            name: None,
            cwd: None,
            status: "notLoaded".to_owned(),
            active_flags: vec![],
            updated_at: None,
            approval_policy: None,
            approvals_reviewer: None,
            sandbox_mode: None,
        };
        assert!(!initial_codex_execution_is_authoritative(&thread));
        thread.status = "idle".to_owned();
        assert!(!initial_codex_execution_is_authoritative(&thread));
        thread.status = "active".to_owned();
        assert!(initial_codex_execution_is_authoritative(&thread));
    }

    #[test]
    fn failed_attach_does_not_hide_a_running_desktop_provider() {
        assert!(detached_codex_can_be_observed(
            "thinking",
            true,
            Some("/Applications/ChatGPT.app/Contents/Resources/codex")
        ));
        assert!(!detached_codex_can_be_observed(
            "thinking",
            true,
            Some("/usr/libexec/corespeechd")
        ));
        assert!(!detached_codex_can_be_observed(
            "thinking",
            false,
            Some("/Applications/ChatGPT.app/Contents/Resources/codex")
        ));
        assert!(!detached_codex_can_be_observed(
            "response_finished",
            true,
            Some("/Applications/ChatGPT.app/Contents/Resources/codex")
        ));
    }

    pub(super) fn test_state(store: RuntimeStore, root: &FilePath) -> AppState {
        let paths = InstallPaths {
            actrealm_home: root.join("actrealm-home"),
            claude_settings: root.join("home/.claude/settings.json"),
            codex_hooks: root.join("codex/hooks.json"),
            codex_config: root.join("codex/config.toml"),
        };
        let quota_paths = QuotaPaths {
            actrealm_home: paths.actrealm_home.clone(),
            codex_sessions: root.join("codex/sessions"),
        };
        let usage_paths = UsagePaths {
            actrealm_home: paths.actrealm_home.clone(),
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
                websocket_tickets: Vec::new(),
                native_sessions: Vec::new(),
            })),
            companions: Arc::new(Mutex::new(CompanionState::default())),
            expected_host: "127.0.0.1:43111".to_owned(),
            expected_origin: "http://127.0.0.1:43111".to_owned(),
            api_address: "127.0.0.1:43111".parse().unwrap(),
            instance_id: "test-instance".to_owned(),
            runtime_started_at: now_millis(),
            restart_count: 0,
            websocket_connections: Arc::new(AtomicUsize::new(0)),
            shutdown_flag: Arc::new(AtomicBool::new(false)),
            commit_delay: Duration::from_secs(3),
            snapshot_interval: Duration::from_millis(250),
            heartbeat_interval: Duration::from_secs(10),
            quota_poll_interval: Duration::from_secs(300),
            claude_oauth_quota: false,
            installer: Arc::new(Installer::new(paths, std::env::current_exe().unwrap())),
            quota: Arc::new(Mutex::new(QuotaState {
                collector: QuotaCollector::new(quota_paths),
                entries: Vec::new(),
                refreshed_at: None,
                claude_cache_modified_at: None,
                codex_rate_limits_captured_at: None,
                oauth_refresh_in_progress: false,
                oauth_next_poll_at: 0,
                oauth_last_result: None,
            })),
            pricing_status: Arc::new(Mutex::new(PricingStatus::default())),
            pricing_refresh_requested: Arc::new(AtomicBool::new(false)),
            usage: Arc::new(Mutex::new(UsageState {
                collector: UsageCollector::new(usage_paths),
                refreshed_at: None,
            })),
            live_codex_usage: Arc::new(Mutex::new(HashMap::new())),
            usage_worker_failures: Arc::new(AtomicUsize::new(0)),
            usage_consecutive_failures: Arc::new(AtomicUsize::new(0)),
            usage_collection_in_progress: Arc::new(AtomicBool::new(false)),
            usage_collection_ready: Arc::new(AtomicBool::new(false)),
            usage_history_complete: Arc::new(AtomicBool::new(false)),
            usage_last_success_at: Arc::new(AtomicU64::new(0)),
            review_collection_in_progress: Arc::new(AtomicBool::new(false)),
            review_collection_ready: Arc::new(AtomicBool::new(false)),
            review_consecutive_failures: Arc::new(AtomicUsize::new(0)),
            review_last_success_at: Arc::new(AtomicU64::new(0)),
            usage_test_control: None,
            data_paths: DataPaths {
                cache: root.join("actrealm-home/cache"),
                spool: root.join("actrealm-home/spool"),
                diagnostics: root.join("actrealm-home/diagnostics"),
                companion_auth: root.join("actrealm-home/companion-auth.json"),
                companion_discovery: root.join("actrealm-home/run/companion-endpoint.json"),
            },
            codex,
            runtime_restart: None,
        }
    }

    #[test]
    fn usage_refresh_cadence_is_bounded_and_adapts_after_backfill() {
        assert_eq!(usage_refresh_delay(false, false), USAGE_FAILURE_BACKOFF);
        assert_eq!(
            usage_refresh_delay(true, false),
            USAGE_BACKFILL_POLL_INTERVAL
        );
        assert_eq!(usage_refresh_delay(true, true), USAGE_LIVE_POLL_INTERVAL);
        assert!(USAGE_LIVE_POLL_INTERVAL > USAGE_BACKFILL_POLL_INTERVAL);
        assert_eq!(
            ApiServerConfig::default().snapshot_interval,
            Duration::from_millis(250)
        );
        assert_eq!(coarse_ui_timestamp(29_999), 0);
        assert_eq!(coarse_ui_timestamp(30_000), 30_000);
        assert_eq!(coarse_ui_timestamp(59_999), 30_000);
    }

    #[test]
    fn usage_worker_recovers_after_a_collector_panic() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-usage-panic-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let mut state = test_state(store.clone(), &root);
        let control = UsageRefreshTestControl::panic_once();
        state.usage_test_control = Some(control);

        assert!(!run_usage_refresh_iteration(&state));
        assert_eq!(state.usage_worker_failures.load(Ordering::Acquire), 1);
        assert!(
            state.usage.lock().is_err(),
            "panic must poison the old state"
        );

        assert!(run_usage_refresh_iteration(&state));
        assert_eq!(state.usage_worker_failures.load(Ordering::Acquire), 1);
        assert!(
            state.usage.lock().is_ok(),
            "the next iteration must rebuild and clear poisoned collector state"
        );
        drop(state);
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cold_start_projects_ready_codex_metrics_while_other_history_is_indexing() {
        let root = std::env::temp_dir().join(format!("actrealm-cold-usage-{}", Uuid::now_v7()));
        let sessions = root.join("codex/sessions");
        fs::create_dir_all(&sessions).unwrap();
        let at = now_millis();
        let history = File::create(sessions.join("history.jsonl")).unwrap();
        history.set_len(32 * 1024 * 1024).unwrap();
        history
            .set_modified(SystemTime::now() - Duration::from_secs(60))
            .unwrap();
        let live = sessions.join("live.jsonl");
        fs::write(&live, concat!(
            "{\"timestamp\":\"2026-09-09T08:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"cold-desktop\"}}\n",
            "{\"timestamp\":\"2026-09-09T08:00:00Z\",\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-6-astra\"}}\n",
            "{\"timestamp\":\"2026-09-09T08:00:01Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":12000,\"cached_input_tokens\":10000,\"output_tokens\":800,\"reasoning_output_tokens\":200,\"total_tokens\":12800},\"last_token_usage\":{\"input_tokens\":12000,\"output_tokens\":800,\"total_tokens\":12800},\"model_context_window\":200000}}}\n"
        )).unwrap();
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let id = store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Codex,
                json!({
                    "hook_event_name":"UserPromptSubmit", "session_id":"cold-desktop", "cwd":root,
                    "turn_id":"current", "model":"gpt-6-astra"
                }),
                at,
            ))
            .unwrap()
            .session_id;
        let state = test_state(store.clone(), &root);
        assert!(run_usage_refresh_iteration(&state));
        assert!(!state.usage_collection_ready.load(Ordering::Acquire));
        assert_eq!(
            store.snapshot().unwrap().token_usage.total,
            0,
            "partial history must not be committed as the complete ledger"
        );
        let view = snapshot_value(&state).unwrap();
        let session = view["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .unwrap();
        assert_eq!(
            session["tokenTotal"], 12800,
            "a fully-read live session should not wait for unrelated history"
        );
        assert_eq!(session["lastTurnTokens"], 12800);
        assert_eq!(session["contextUsedTokens"], 12000);
        assert_eq!(session["contextUsedPercent"], 6);
        assert_eq!(session["contextWindowTokens"], 200000);
        let companion = companion_snapshot_value(
            &state,
            &CompanionAuthorization {
                id: "cold-client".into(),
                scopes: vec![COMPANION_SCOPE_SNAPSHOT.into()],
            },
        )
        .unwrap();
        let projected = companion["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .unwrap();
        assert_eq!(projected["tokenTotal"], 12800);
        assert_eq!(projected["contextUsedPercent"], 6);
        assert_eq!(view["tokenUsage"]["collectionState"], "scanning");
        let stored = store
            .snapshot()
            .unwrap()
            .sessions
            .into_iter()
            .find(|s| s.id == id)
            .unwrap();
        assert_eq!(
            stored.token_total, None,
            "live fallback must not enter SQLite"
        );
        let mut object = serde_json::to_value(&stored)
            .unwrap()
            .as_object()
            .unwrap()
            .clone();
        object.insert("tokenTotal".into(), json!(42000));
        let live = state.live_codex_usage.lock().unwrap().clone();
        apply_live_codex_metrics(&stored, &mut object, &live);
        assert_eq!(
            object["tokenTotal"], 42000,
            "never overwrite committed fields"
        );
        let mut mismatch = live.clone();
        mismatch.get_mut("cold-desktop").unwrap().model = Some("different-model".into());
        let mut object = serde_json::to_value(&stored)
            .unwrap()
            .as_object()
            .unwrap()
            .clone();
        apply_live_codex_metrics(&stored, &mut object, &mismatch);
        assert!(
            object["tokenTotal"].is_null(),
            "never pair another model's context with the current model"
        );
        OpenOptions::new()
            .append(true)
            .open(sessions.join("history.jsonl"))
            .unwrap()
            .write_all(b"\n")
            .unwrap();
        for _ in 0..6 {
            state.usage.lock().unwrap().refreshed_at = None;
            assert!(run_usage_refresh_iteration(&state));
            if state.usage_collection_ready.load(Ordering::Acquire) {
                break;
            }
        }
        assert!(state.usage_collection_ready.load(Ordering::Acquire));
        assert!(state.live_codex_usage.lock().unwrap().is_empty());
        assert_eq!(store.snapshot().unwrap().token_usage.total, 12800);
        drop(state);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn incomplete_history_scan_keeps_the_previous_ledger_generation_visible() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-usage-shadow-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        store
            .upsert_session_usage(SessionUsageRecord {
                provider: "codex".to_owned(),
                provider_session_id: "committed-session".to_owned(),
                project_id: None,
                project_label: None,
                parent_provider_session_id: None,
                model: Some("gpt-5.6-sol".to_owned()),
                input_tokens: Some(80),
                output_tokens: Some(20),
                cache_read_tokens: Some(0),
                cache_creation_tokens: Some(0),
                reasoning_tokens: Some(0),
                token_total: Some(100),
                last_turn_tokens: Some(100),
                context_used_tokens: None,
                context_window_tokens: None,
                context_used_percent: None,
                estimated_cost_usd_micros: None,
                cost_kind: None,
                pricing_source: None,
                usage_source: "test".to_owned(),
                usage_quality: "official_local".to_owned(),
                captured_at: 1,
                daily_usage: vec![actrealm_runtime::SessionUsageDailyRecord {
                    day: "2026-08-17".to_owned(),
                    model: Some("gpt-5.6-sol".to_owned()),
                    input_tokens: 80,
                    output_tokens: 20,
                    cache_read_tokens: 0,
                    cache_creation_tokens: 0,
                    reasoning_tokens: 0,
                    token_total: 100,
                    estimated_cost_usd_micros: None,
                    cost_kind: None,
                    pricing_source: None,
                    message_count: 1,
                }],
            })
            .unwrap();
        let sessions = root.join("codex/sessions");
        fs::create_dir_all(&sessions).unwrap();
        File::create(sessions.join("unfinished.jsonl"))
            .unwrap()
            .set_len(12 * 1_024 * 1_024)
            .unwrap();
        let state = test_state(store.clone(), &root);

        assert!(run_usage_refresh_iteration(&state));
        assert!(!state.usage_collection_ready.load(Ordering::Acquire));
        assert_eq!(store.snapshot().unwrap().token_usage.total, 100);

        drop(state);
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn codex_credential_refresh_waits_for_the_first_usage_generation() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-usage-credential-grace-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let state = test_state(store.clone(), &root);

        assert!(should_defer_codex_credential_refresh(
            &state,
            state.runtime_started_at + 60_000
        ));
        state.usage_collection_ready.store(true, Ordering::Release);
        assert!(!should_defer_codex_credential_refresh(
            &state,
            state.runtime_started_at + 60_000
        ));
        state.usage_collection_ready.store(false, Ordering::Release);
        assert!(!should_defer_codex_credential_refresh(
            &state,
            state.runtime_started_at + CODEX_CREDENTIAL_REFRESH_FIRST_USAGE_GRACE_MS
        ));

        drop(state);
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn incremental_tail_lag_does_not_restart_the_first_scan_state() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-usage-sticky-ready-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let sessions = root.join("codex/sessions");
        fs::create_dir_all(&sessions).unwrap();
        let path = sessions.join("live.jsonl");
        fs::write(&path, b"{\"type\":\"noise\"}\n").unwrap();
        let state = test_state(store.clone(), &root);

        assert!(run_usage_refresh_iteration(&state));
        assert!(state.usage_collection_ready.load(Ordering::Acquire));
        assert!(state.usage_history_complete.load(Ordering::Acquire));

        let source = OpenOptions::new().append(true).open(&path).unwrap();
        source.set_len(12 * 1_024 * 1_024).unwrap();
        assert!(run_usage_refresh_iteration(&state));
        assert!(
            state.usage_collection_ready.load(Ordering::Acquire),
            "a growing source must not restart the first-scan state"
        );
        assert!(
            state.usage_history_complete.load(Ordering::Acquire),
            "the previous committed generation remains authoritative"
        );

        drop(state);
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn snapshot_stays_responsive_while_usage_collection_is_in_flight() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-usage-isolation-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let mut state = test_state(store.clone(), &root);
        let control = UsageRefreshTestControl::block_once();
        state.usage_test_control = Some(control.clone());
        let worker_state = state.clone();
        let worker = thread::spawn(move || run_usage_refresh_iteration(&worker_state));
        control.wait_until_collecting();

        let started = Instant::now();
        let snapshot = snapshot_value(&state).expect("snapshot while collection is blocked");
        assert!(started.elapsed() < Duration::from_millis(300));
        assert!(snapshot.get("sessions").is_some());
        assert_eq!(snapshot["tokenUsage"]["dataQuality"], "rebuilding");

        control.release_collection();
        assert!(worker.join().expect("usage worker join"));
        let completed = snapshot_value(&state).expect("snapshot after collection");
        assert_eq!(completed["tokenUsage"]["dataQuality"], "verified");
        assert!(completed["tokenUsage"]["lastAuditedAt"].is_number());
        drop(state);
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn snapshot_does_not_call_a_successful_partial_rebuild_verified() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-usage-quality-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let state = test_state(store.clone(), &root);
        state
            .usage_last_success_at
            .store(123_456, Ordering::Release);

        let rebuilding = snapshot_value(&state).expect("snapshot during historical rebuild");
        assert_eq!(rebuilding["tokenUsage"]["collectionState"], "scanning");
        assert_eq!(rebuilding["tokenUsage"]["dataQuality"], "rebuilding");
        assert_eq!(rebuilding["tokenUsage"]["lastSuccessfulAt"], json!(120_000));
        assert_eq!(rebuilding["tokenUsage"]["collectionInProgress"], true);
        assert!(rebuilding["tokenUsage"].get("lastAuditedAt").is_none());

        state.usage_collection_ready.store(true, Ordering::Release);
        let partial = snapshot_value(&state).expect("snapshot after partial history reaches EOF");
        assert_eq!(partial["tokenUsage"]["collectionState"], "partial");
        assert_eq!(partial["tokenUsage"]["dataQuality"], "partial");
        assert_eq!(partial["tokenUsage"]["collectionInProgress"], false);
        assert!(partial["tokenUsage"].get("lastAuditedAt").is_none());

        state.usage_history_complete.store(true, Ordering::Release);
        let verified = snapshot_value(&state).expect("snapshot after historical rebuild");
        assert_eq!(verified["tokenUsage"]["collectionState"], "ready");
        assert_eq!(verified["tokenUsage"]["dataQuality"], "verified");
        assert_eq!(verified["tokenUsage"]["lastAuditedAt"], json!(120_000));
        drop(state);
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_snapshot_exposes_token_decisions_but_companion_does_not_receive_task_history() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-token-decision-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let now = now_millis();
        store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Codex,
                json!({
                    "hook_event_name":"UserPromptSubmit",
                    "session_id":"token-decision-session",
                    "turn_id":"token-decision-turn",
                    "cwd":"/tmp/token-decision-project",
                    "prompt":"Token attribution task"
                }),
                now,
            ))
            .unwrap();
        store
            .upsert_session_usage(SessionUsageRecord {
                provider: "codex".to_owned(),
                provider_session_id: "token-decision-session".to_owned(),
                project_id: None,
                project_label: None,
                parent_provider_session_id: None,
                model: Some("gpt-test".to_owned()),
                input_tokens: Some(80),
                output_tokens: Some(20),
                cache_read_tokens: Some(0),
                cache_creation_tokens: Some(0),
                reasoning_tokens: Some(0),
                token_total: Some(100),
                last_turn_tokens: Some(100),
                context_used_tokens: None,
                context_window_tokens: None,
                context_used_percent: None,
                estimated_cost_usd_micros: None,
                cost_kind: None,
                pricing_source: None,
                usage_source: "test".to_owned(),
                usage_quality: "official_local".to_owned(),
                captured_at: now,
                daily_usage: vec![actrealm_runtime::SessionUsageDailyRecord {
                    day: "2026-08-18".to_owned(),
                    model: Some("gpt-test".to_owned()),
                    input_tokens: 80,
                    output_tokens: 20,
                    cache_read_tokens: 0,
                    cache_creation_tokens: 0,
                    reasoning_tokens: 0,
                    token_total: 100,
                    estimated_cost_usd_micros: None,
                    cost_kind: None,
                    pricing_source: None,
                    message_count: 1,
                }],
            })
            .unwrap();
        let state = test_state(store.clone(), &root);
        let local = snapshot_value(&state).unwrap();
        assert_eq!(local["tokenDecision"]["schemaVersion"], 2);
        assert_eq!(local["tokenDecision"]["totalTokens"], 100);
        assert_eq!(local["tokenDecision"]["attributedTokens"], 100);
        assert_eq!(local["tokenDecision"]["projectAttributedTokens"], 100);
        assert_eq!(local["tokenDecision"]["taskAttributedTokens"], 100);
        assert_eq!(
            local["tokenDecision"]["projectTotals"][0]["project"],
            "token-decision-project"
        );
        assert!(!local.to_string().contains("/tmp/token-decision-project"));

        let companion = companion_snapshot_value(
            &state,
            &CompanionAuthorization {
                id: "companion".to_owned(),
                scopes: vec![COMPANION_SCOPE_SNAPSHOT.to_owned()],
            },
        )
        .unwrap();
        assert!(companion.get("tokenDecision").is_none());
        assert!(!companion.to_string().contains("canonical_session_ledger"));
        drop(state);
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn verified_collection_with_a_future_usage_day_is_marked_suspect() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-usage-suspect-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        store
            .upsert_session_usage(SessionUsageRecord {
                provider: "codex".to_owned(),
                provider_session_id: "future-session".to_owned(),
                project_id: None,
                project_label: None,
                parent_provider_session_id: None,
                model: Some("future-model".to_owned()),
                input_tokens: Some(80),
                output_tokens: Some(20),
                cache_read_tokens: Some(0),
                cache_creation_tokens: Some(0),
                reasoning_tokens: Some(0),
                token_total: Some(100),
                last_turn_tokens: Some(100),
                context_used_tokens: None,
                context_window_tokens: None,
                context_used_percent: None,
                estimated_cost_usd_micros: None,
                cost_kind: None,
                pricing_source: None,
                usage_source: "test".to_owned(),
                usage_quality: "official_local".to_owned(),
                captured_at: 1,
                daily_usage: vec![actrealm_runtime::SessionUsageDailyRecord {
                    day: "2999-01-01".to_owned(),
                    model: Some("future-model".to_owned()),
                    input_tokens: 80,
                    output_tokens: 20,
                    cache_read_tokens: 0,
                    cache_creation_tokens: 0,
                    reasoning_tokens: 0,
                    token_total: 100,
                    estimated_cost_usd_micros: None,
                    cost_kind: None,
                    pricing_source: None,
                    message_count: 1,
                }],
            })
            .unwrap();
        let state = test_state(store.clone(), &root);
        state.usage_collection_ready.store(true, Ordering::Release);
        state.usage_history_complete.store(true, Ordering::Release);
        state
            .usage_last_success_at
            .store(123_456, Ordering::Release);

        let snapshot = snapshot_value(&state).expect("audited suspect snapshot");
        assert_eq!(snapshot["tokenUsage"]["collectionState"], "ready");
        assert_eq!(snapshot["tokenUsage"]["dataQuality"], "suspect");
        assert_eq!(snapshot["tokenUsage"]["suspectCount"], 1);
        assert_eq!(
            snapshot["tokenUsage"]["anomalies"][0]["code"],
            "future_usage_day"
        );
        assert_eq!(snapshot["tokenUsage"]["peakDay"], Value::Null);
        assert_eq!(snapshot["tokenUsage"]["lastAuditedAt"], 120_000);

        drop(state);
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn snapshot_exposes_the_bundled_provider_capability_matrix() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-provider-capabilities-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let state = test_state(store.clone(), &root);

        let snapshot = snapshot_value(&state).unwrap();
        assert_eq!(
            snapshot["capabilities"]["providerMatrix"]["schemaVersion"],
            json!(actrealm_core::PROVIDER_CAPABILITY_SCHEMA_VERSION)
        );
        assert_eq!(
            snapshot["capabilities"]["providerMatrix"]["providers"]["claude"]["subagents"]
                ["status"],
            "supported"
        );
        assert_eq!(
            snapshot["capabilities"]["providerMatrix"]["providers"]["codex"]["subagents"]["status"],
            "unknown"
        );

        drop(state);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn snapshot_projects_bounded_fact_metadata_without_private_values() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-session-facts-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let observed_at = now_millis();
        store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Claude,
                json!({
                    "hook_event_name":"UserPromptSubmit",
                    "session_id":"fact-session",
                    "prompt":"PRIVATE PROMPT"
                }),
                observed_at,
            ))
            .unwrap();
        store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Claude,
                json!({
                    "hook_event_name":"PreToolUse",
                    "session_id":"fact-session",
                    "tool_name":"Read",
                    "tool_input":{"file_path":"/Users/alice/private/Secret.swift"}
                }),
                observed_at.saturating_add(1),
            ))
            .unwrap();
        let state = test_state(store.clone(), &root);

        let snapshot = snapshot_value(&state).unwrap();
        let session = snapshot["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|session| session["providerSessionId"] == "fact-session")
            .unwrap();
        assert_eq!(session["facts"]["schemaVersion"], 1);
        assert_eq!(session["facts"]["plan"]["sourceKind"], "unavailable");
        assert_eq!(
            session["facts"]["plan"]["absenceReason"],
            "provider_not_supplied"
        );
        assert_eq!(session["facts"]["activity"]["sourceKind"], "observed");
        assert_eq!(
            session["facts"]["activity"]["sourceId"],
            "provider:tool_lifecycle"
        );
        assert_eq!(
            session["facts"]["currentTarget"]["sourceId"],
            "hook:tool_input/allowlisted_basename"
        );
        assert_eq!(session["facts"]["control"]["capability"], "observe_only");
        assert_eq!(
            session["facts"]["completion"]["absenceReason"],
            "task_not_completed"
        );
        let encoded = serde_json::to_string(&session["facts"]).unwrap();
        assert!(!encoded.contains("PRIVATE PROMPT"));
        assert!(!encoded.contains("/Users/alice/private"));

        drop(state);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn review_endpoint_reports_current_git_and_structured_validation_without_paths() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-review-v1-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let repository = root.join("private-repository");
        fs::create_dir_all(&repository).unwrap();
        assert!(ProcessCommand::new("/usr/bin/git")
            .args(["init", "-b", "main"])
            .arg(&repository)
            .status()
            .unwrap()
            .success());
        fs::write(repository.join("private-file.txt"), b"private contents").unwrap();
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let first = store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Codex,
                json!({
                    "hook_event_name":"UserPromptSubmit",
                    "session_id":"review-session",
                    "turn_id":"review-turn",
                    "cwd":root.to_string_lossy(),
                    "prompt":"PRIVATE REVIEW PROMPT"
                }),
                10_000,
            ))
            .unwrap();
        for (at, event) in [(10_010, "PreToolUse"), (10_020, "PostToolUse")] {
            store
                .ingest(BridgeRequest::from_hook_at(
                    Provider::Codex,
                    json!({
                        "hook_event_name":event,
                        "session_id":"review-session",
                        "turn_id":"review-turn",
                        "cwd":root.to_string_lossy(),
                        "tool_name":"Bash",
                        "tool_use_id":"review-test",
                        "tool_input":{
                            "command":"cargo test --workspace --offline",
                            "workdir":repository.to_string_lossy()
                        }
                    }),
                    at,
                ))
                .unwrap();
        }
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
            let response = router(state)
                .oneshot(authorized_request(
                    "GET",
                    &format!("/api/v1/sessions/{}/review", first.session_id),
                    Value::Null,
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let review = json_body(response).await;
            assert_eq!(review["schemaVersion"], 1);
            assert_eq!(review["repository"]["state"], "available");
            assert_eq!(review["repository"]["branch"], "main");
            assert_eq!(review["repository"]["dirty"], true);
            assert_eq!(review["repository"]["untrackedFiles"], 1);
            assert_eq!(
                review["repository"]["attribution"],
                "current_worktree_unattributed"
            );
            assert!(review["validations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|run| { run["kind"] == "test" && run["state"] == "unverifiable" }));
            let encoded = review.to_string();
            assert!(!encoded.contains("PRIVATE REVIEW PROMPT"));
            assert!(!encoded.contains(repository.to_string_lossy().as_ref()));
            assert!(!encoded.contains("private-file.txt"));
            assert!(!encoded.contains("cargo test"));
        });
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn review_endpoint_reports_non_git_and_no_validation_without_inventing_evidence() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-review-non-git-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let working_directory = root.join("private-non-git-workspace");
        fs::create_dir_all(&working_directory).unwrap();
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let first = store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Claude,
                json!({
                    "hook_event_name":"UserPromptSubmit",
                    "session_id":"review-non-git-session",
                    "cwd":working_directory,
                    "prompt":"PRIVATE NON GIT PROMPT"
                }),
                20_000,
            ))
            .unwrap();
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
            let response = router(state)
                .oneshot(authorized_request(
                    "GET",
                    &format!("/api/v1/sessions/{}/review", first.session_id),
                    Value::Null,
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let review = json_body(response).await;
            assert_eq!(review["repository"]["state"], "not_git");
            assert_eq!(review["repository"]["attribution"], "unavailable");
            assert_eq!(
                review["repository"]["attributionReason"],
                "not_git_repository"
            );
            assert!(review["validations"].as_array().unwrap().is_empty());
            let encoded = review.to_string();
            assert!(!encoded.contains("PRIVATE NON GIT PROMPT"));
            assert!(!encoded.contains(working_directory.to_string_lossy().as_ref()));
        });
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn review_diff_is_local_bounded_and_rejects_path_traversal() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-review-diff-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let repository = root.join("repository");
        fs::create_dir_all(&repository).unwrap();
        assert!(ProcessCommand::new("/usr/bin/git")
            .args(["init", "-b", "main"])
            .arg(&repository)
            .status()
            .unwrap()
            .success());
        fs::write(repository.join("tracked.txt"), b"before\n").unwrap();
        assert!(ProcessCommand::new("/usr/bin/git")
            .args(["-C"])
            .arg(&repository)
            .args(["add", "tracked.txt"])
            .status()
            .unwrap()
            .success());
        assert!(ProcessCommand::new("/usr/bin/git")
            .args(["-C"])
            .arg(&repository)
            .args([
                "-c",
                "user.name=ActRealm Test",
                "-c",
                "user.email=actrealm@example.invalid",
                "commit",
                "-m",
                "baseline",
            ])
            .status()
            .unwrap()
            .success());
        fs::write(repository.join("tracked.txt"), b"before\nafter\n").unwrap();
        fs::write(
            repository.join("untracked.txt"),
            b"never read automatically",
        )
        .unwrap();
        fs::write(root.join("outside.txt"), b"outside secret").unwrap();
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let first = store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Codex,
                json!({
                    "hook_event_name":"UserPromptSubmit",
                    "session_id":"review-diff-session",
                    "turn_id":"review-diff-turn",
                    "cwd":repository,
                    "prompt":"PRIVATE"
                }),
                1_000,
            ))
            .unwrap();
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
            let list = app
                .clone()
                .oneshot(authorized_request(
                    "GET",
                    &format!("/api/v1/sessions/{}/review/diff", first.session_id),
                    Value::Null,
                ))
                .await
                .unwrap();
            assert_eq!(list.status(), StatusCode::OK);
            let list = json_body(list).await;
            assert!(list["files"]
                .as_array()
                .unwrap()
                .iter()
                .any(|file| { file["path"] == "tracked.txt" && file["state"] == "tracked" }));
            assert!(list["files"]
                .as_array()
                .unwrap()
                .iter()
                .any(|file| { file["path"] == "untracked.txt" && file["state"] == "untracked" }));
            assert!(!list
                .to_string()
                .contains(repository.to_string_lossy().as_ref()));

            let selected = app
                .clone()
                .oneshot(authorized_request(
                    "GET",
                    &format!(
                        "/api/v1/sessions/{}/review/diff?path=tracked.txt",
                        first.session_id
                    ),
                    Value::Null,
                ))
                .await
                .unwrap();
            let selected = json_body(selected).await;
            assert_eq!(selected["selected"]["path"], "tracked.txt");
            assert!(selected["selected"]["patch"]
                .as_str()
                .unwrap()
                .contains("+after"));

            let traversal = app
                .oneshot(authorized_request(
                    "GET",
                    &format!(
                        "/api/v1/sessions/{}/review/diff?path=..%2Foutside.txt",
                        first.session_id
                    ),
                    Value::Null,
                ))
                .await
                .unwrap();
            let traversal = json_body(traversal).await;
            assert_eq!(traversal["selected"], Value::Null);
            assert!(!traversal.to_string().contains("outside secret"));
        });
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn checkpoint_git_snapshot_restores_and_rolls_back_only_after_clean_preflight() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-checkpoint-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let repository = root.join("repository");
        fs::create_dir_all(&repository).unwrap();
        assert!(ProcessCommand::new("/usr/bin/git")
            .args(["init", "-b", "main"])
            .arg(&repository)
            .status()
            .unwrap()
            .success());
        for (key, value) in [
            ("user.name", "ActRealm Test"),
            ("user.email", "actrealm@example.invalid"),
        ] {
            assert!(ProcessCommand::new("/usr/bin/git")
                .args(["-C"])
                .arg(&repository)
                .args(["config", key, value])
                .status()
                .unwrap()
                .success());
        }
        fs::write(repository.join("tracked.txt"), b"before\n").unwrap();
        assert!(ProcessCommand::new("/usr/bin/git")
            .args(["-C"])
            .arg(&repository)
            .args(["add", "tracked.txt"])
            .status()
            .unwrap()
            .success());
        assert!(ProcessCommand::new("/usr/bin/git")
            .args(["-C"])
            .arg(&repository)
            .args(["commit", "-m", "baseline"])
            .status()
            .unwrap()
            .success());
        fs::write(repository.join("tracked.txt"), b"before\nafter\n").unwrap();
        fs::write(repository.join("untracked.txt"), b"not captured\n").unwrap();

        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let ingested = store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Codex,
                json!({
                    "hook_event_name":"UserPromptSubmit",
                    "session_id":"checkpoint-session",
                    "turn_id":"checkpoint-turn",
                    "cwd":repository,
                    "prompt":"Create a safe checkpoint"
                }),
                1_000,
            ))
            .unwrap();
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
            let created = app
                .clone()
                .oneshot(authorized_request(
                    "POST",
                    &format!("/api/v1/sessions/{}/checkpoints", ingested.session_id),
                    json!({"kind":"git_snapshot","label":"Before restore"}),
                ))
                .await
                .unwrap();
            assert_eq!(created.status(), StatusCode::OK);
            let created = json_body(created).await;
            assert_eq!(created["kind"], "git_snapshot");
            assert_eq!(created["repository"]["gitSnapshot"], true);
            assert!(created["limitations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == "untracked_not_captured"));
            assert!(!created
                .to_string()
                .contains(repository.to_string_lossy().as_ref()));
            let checkpoint_id = created["id"].as_str().unwrap();

            let dirty_preflight = app
                .clone()
                .oneshot(authorized_request(
                    "GET",
                    &format!("/api/v1/checkpoints/{checkpoint_id}/preflight?action=restore_code"),
                    Value::Null,
                ))
                .await
                .unwrap();
            let dirty_preflight = json_body(dirty_preflight).await;
            assert_eq!(dirty_preflight["allowed"], false);
            assert!(dirty_preflight["blockers"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == "working_tree_dirty"));

            fs::write(repository.join("tracked.txt"), b"before\n").unwrap();
            fs::remove_file(repository.join("untracked.txt")).unwrap();
            let restore_preflight = app
                .clone()
                .oneshot(authorized_request(
                    "GET",
                    &format!("/api/v1/checkpoints/{checkpoint_id}/preflight?action=restore_code"),
                    Value::Null,
                ))
                .await
                .unwrap();
            let restore_preflight = json_body(restore_preflight).await;
            assert_eq!(restore_preflight["allowed"], true);

            let restored = app
                .clone()
                .oneshot(authorized_request(
                    "POST",
                    &format!("/api/v1/checkpoints/{checkpoint_id}/actions"),
                    json!({"action":"restore_code"}),
                ))
                .await
                .unwrap();
            assert_eq!(restored.status(), StatusCode::OK);
            assert_eq!(
                fs::read_to_string(repository.join("tracked.txt")).unwrap(),
                "before\nafter\n"
            );

            fs::write(
                repository.join("tracked.txt"),
                b"before\nafter\nuser change\n",
            )
            .unwrap();
            let protected = app
                .clone()
                .oneshot(authorized_request(
                    "GET",
                    &format!("/api/v1/checkpoints/{checkpoint_id}/preflight?action=rollback_code"),
                    Value::Null,
                ))
                .await
                .unwrap();
            let protected = json_body(protected).await;
            assert_eq!(protected["allowed"], false);
            assert!(protected["blockers"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == "working_tree_not_checkpoint"));
            assert_eq!(
                fs::read_to_string(repository.join("tracked.txt")).unwrap(),
                "before\nafter\nuser change\n"
            );
            fs::write(repository.join("tracked.txt"), b"before\nafter\n").unwrap();

            let rollback_preflight = app
                .clone()
                .oneshot(authorized_request(
                    "GET",
                    &format!("/api/v1/checkpoints/{checkpoint_id}/preflight?action=rollback_code"),
                    Value::Null,
                ))
                .await
                .unwrap();
            let rollback_preflight = json_body(rollback_preflight).await;
            assert_eq!(rollback_preflight["allowed"], true);
            let rolled_back = app
                .clone()
                .oneshot(authorized_request(
                    "POST",
                    &format!("/api/v1/checkpoints/{checkpoint_id}/actions"),
                    json!({"action":"rollback_code"}),
                ))
                .await
                .unwrap();
            assert_eq!(rolled_back.status(), StatusCode::OK);
            assert_eq!(
                fs::read_to_string(repository.join("tracked.txt")).unwrap(),
                "before\n"
            );

            run_git(&repository, &["checkout", "-b", "other-branch"]).unwrap();
            let branch_drift = app
                .clone()
                .oneshot(authorized_request(
                    "GET",
                    &format!("/api/v1/checkpoints/{checkpoint_id}/preflight?action=restore_code"),
                    Value::Null,
                ))
                .await
                .unwrap();
            let branch_drift = json_body(branch_drift).await;
            assert_eq!(branch_drift["allowed"], false);
            assert!(branch_drift["blockers"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == "branch_changed"));
            run_git(&repository, &["checkout", "main"]).unwrap();

            let deleted = app
                .oneshot(authorized_request(
                    "DELETE",
                    &format!("/api/v1/checkpoints/{checkpoint_id}"),
                    Value::Null,
                ))
                .await
                .unwrap();
            assert_eq!(deleted.status(), StatusCode::OK);
            assert!(run_git(
                &repository,
                &[
                    "show-ref",
                    "--verify",
                    &format!("refs/actrealm/checkpoints/{checkpoint_id}")
                ]
            )
            .is_err());
        });
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn metadata_checkpoint_supports_non_git_without_inventing_code_recovery() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-checkpoint-non-git-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let workspace = root.join("private-workspace");
        fs::create_dir_all(&workspace).unwrap();
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let ingested = store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Claude,
                json!({
                    "hook_event_name":"UserPromptSubmit",
                    "session_id":"checkpoint-non-git-session",
                    "cwd":workspace,
                    "prompt":"Metadata only"
                }),
                1_000,
            ))
            .unwrap();
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
            let created = app
                .clone()
                .oneshot(authorized_request(
                    "POST",
                    &format!("/api/v1/sessions/{}/checkpoints", ingested.session_id),
                    json!({"kind":"metadata"}),
                ))
                .await
                .unwrap();
            assert_eq!(created.status(), StatusCode::OK);
            let created = json_body(created).await;
            assert_eq!(created["repository"]["state"], "unavailable");
            assert_eq!(created["repository"]["gitSnapshot"], false);
            assert!(!created.to_string().contains("private-workspace"));

            let git_snapshot = app
                .oneshot(authorized_request(
                    "POST",
                    &format!("/api/v1/sessions/{}/checkpoints", ingested.session_id),
                    json!({"kind":"git_snapshot"}),
                ))
                .await
                .unwrap();
            assert_eq!(git_snapshot.status(), StatusCode::CONFLICT);
        });
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn review_git_parsers_count_states_without_reading_path_text() {
        let status = parse_git_status(
            b"1 .M N... 100644 100644 100644 a b tracked\0\
              1 M. N... 100644 100644 100644 a b staged\0\
              2 R. N... 100644 100644 100644 a b R100 renamed\0old-name\0\
              ? private-untracked\0",
        );
        assert_eq!(status.changed_files, 4);
        assert_eq!(status.staged_files, 2);
        assert_eq!(status.unstaged_files, 1);
        assert_eq!(status.untracked_files, 1);

        let numstat = parse_git_numstat(b"10\t2\tprivate-a\n-\t-\tprivate-b\n4\t1\tprivate-c\n");
        assert_eq!(numstat.insertions, 14);
        assert_eq!(numstat.deletions, 3);
        assert_eq!(numstat.binary_files, 1);
    }

    #[test]
    fn review_baseline_upgrades_current_worktree_to_bounded_or_exact_attribution() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-review-baseline-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        fs::create_dir_all(&root).unwrap();
        assert!(ProcessCommand::new("/usr/bin/git")
            .args(["init", "-b", "main"])
            .arg(&root)
            .status()
            .unwrap()
            .success());
        fs::write(root.join("review.txt"), b"before\n").unwrap();
        assert!(ProcessCommand::new("/usr/bin/git")
            .args(["-C"])
            .arg(&root)
            .args(["add", "review.txt"])
            .status()
            .unwrap()
            .success());
        assert!(ProcessCommand::new("/usr/bin/git")
            .args(["-C"])
            .arg(&root)
            .args([
                "-c",
                "user.name=ActRealm Test",
                "-c",
                "user.email=actrealm@example.invalid",
                "commit",
                "-m",
                "baseline",
            ])
            .status()
            .unwrap()
            .success());
        let head = full_git_head(&root).unwrap();
        let identity = review_repository_identity(&root).unwrap();
        fs::write(root.join("review.txt"), b"before\nafter\n").unwrap();
        let mut limitations = Vec::new();
        let mut repository = inspect_resolved_git_repository(&root, 0, &mut limitations);
        let baseline = ReviewBaselineRecord {
            session_id: "session".to_owned(),
            turn_id: "turn".to_owned(),
            repository_root: root.clone(),
            repository_identity: identity,
            branch: Some("main".to_owned()),
            head: Some(head),
            worktree_kind: "primary".to_owned(),
            dirty: false,
            changed_files: 0,
            staged_files: 0,
            unstaged_files: 0,
            untracked_files: 0,
            insertions: Some(0),
            deletions: Some(0),
            binary_files: Some(0),
            turn_started_at: 1_000,
            first_tool_at: Some(1_200),
            captured_at: 1_100,
        };
        apply_review_baseline(&mut repository, &baseline, 0, &mut limitations);
        assert_eq!(repository.baseline_state, "available");
        assert_eq!(repository.attribution, "bounded_window");
        assert_eq!(repository.attribution_reason, "clean_turn_baseline");
        assert_eq!(repository.changed_files, Some(1));
        assert_eq!(repository.insertions, Some(1));

        let mut linked = baseline.clone();
        linked.worktree_kind = "linked".to_owned();
        let mut exact = inspect_resolved_git_repository(&root, 0, &mut Vec::new());
        apply_review_baseline(&mut exact, &linked, 0, &mut Vec::new());
        assert_eq!(exact.attribution, "exact");

        let mut started_dirty = baseline.clone();
        started_dirty.dirty = true;
        let mut dirty = inspect_resolved_git_repository(&root, 0, &mut Vec::new());
        apply_review_baseline(&mut dirty, &started_dirty, 0, &mut Vec::new());
        assert_eq!(dirty.attribution, "bounded_window");
        assert_eq!(dirty.attribution_reason, "baseline_started_dirty");

        let mut captured_late = baseline.clone();
        captured_late.captured_at = 1_300;
        let mut late = inspect_resolved_git_repository(&root, 0, &mut Vec::new());
        apply_review_baseline(&mut late, &captured_late, 0, &mut Vec::new());
        assert_eq!(late.attribution, "bounded_window");
        assert_eq!(
            late.attribution_reason,
            "baseline_captured_after_first_tool"
        );

        let mut concurrent = inspect_resolved_git_repository(&root, 1, &mut Vec::new());
        apply_review_baseline(&mut concurrent, &baseline, 1, &mut Vec::new());
        assert_eq!(concurrent.attribution, "concurrent_changes");
        assert_eq!(
            concurrent.attribution_reason,
            "multiple_active_sessions_same_worktree"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn timeline_route_is_authenticated_cursor_bounded_and_contains_no_raw_payload() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-timeline-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let first = store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Claude,
                json!({
                    "hook_event_name":"UserPromptSubmit",
                    "session_id":"timeline-session",
                    "prompt":"PRIVATE PROMPT MUST NOT LEAK"
                }),
                1_000,
            ))
            .unwrap();
        store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Claude,
                json!({
                    "hook_event_name":"PreToolUse",
                    "session_id":"timeline-session",
                    "tool_name":"PrivateProviderTool",
                    "tool_input":{
                        "secret":"PRIVATE TOOL INPUT",
                        "file_path":"/tmp/private/LanesSection.swift"
                    }
                }),
                2_000,
            ))
            .unwrap();
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
            let unauthorized = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/v1/sessions/{}/timeline", first.session_id))
                        .header(HOST, "127.0.0.1:43111")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

            let response = app
                .clone()
                .oneshot(authorized_request(
                    "GET",
                    &format!("/api/v1/sessions/{}/timeline?limit=1", first.session_id),
                    Value::Null,
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body = json_body(response).await;
            assert_eq!(body["events"].as_array().unwrap().len(), 1);
            assert_eq!(body["events"][0]["kind"], "turn.started");
            assert!(body["events"][0].get("summaryCode").is_none());
            assert_eq!(body["hasMore"], true);
            let serialized = body.to_string();
            assert!(!serialized.contains("PRIVATE PROMPT"));
            assert!(!serialized.contains("PRIVATE TOOL INPUT"));

            let latest = app
                .clone()
                .oneshot(authorized_request(
                    "GET",
                    &format!(
                        "/api/v1/sessions/{}/timeline?latest=true&limit=1",
                        first.session_id
                    ),
                    Value::Null,
                ))
                .await
                .unwrap();
            assert_eq!(latest.status(), StatusCode::OK);
            let latest_body = json_body(latest).await;
            assert_eq!(latest_body["events"].as_array().unwrap().len(), 1);
            assert_eq!(latest_body["events"][0]["kind"], "tool.started");
            assert_eq!(latest_body["events"][0]["toolTarget"], "LanesSection.swift");
            assert_eq!(latest_body["hasMore"], true);
            assert!(!latest_body.to_string().contains("/tmp/private"));

            let missing = app
                .oneshot(authorized_request(
                    "GET",
                    "/api/v1/sessions/missing/timeline",
                    Value::Null,
                ))
                .await
                .unwrap();
            assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        });
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn jump_capabilities_map_only_to_targets_the_runtime_can_really_open() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-jump-targets-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let codex_id = Uuid::now_v7().to_string();
        let codex_vscode_id = Uuid::now_v7().to_string();
        let cases = [
            (
                actrealm_core::Provider::Codex,
                codex_id.as_str(),
                actrealm_core::TermContext {
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
                actrealm_core::Provider::Codex,
                codex_vscode_id.as_str(),
                actrealm_core::TermContext {
                    app: Some("vscode".to_owned()),
                    session_id: None,
                    tty: None,
                    title: None,
                    bundle_id: Some("com.microsoft.VSCode".to_owned()),
                    surface: Some("editor".to_owned()),
                    provider_pid: None,
                },
            ),
            (
                actrealm_core::Provider::Claude,
                "iterm-session",
                actrealm_core::TermContext {
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
                actrealm_core::Provider::Claude,
                "claude-app-session",
                actrealm_core::TermContext {
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
            let mut request = actrealm_core::BridgeRequest::from_hook_at(
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
        assert_eq!(exact.jump_label, "Open exact conversation");
        assert!(matches!(
            jump_target(exact),
            Some(JumpTarget::CodexThread(_))
        ));
        let vscode_codex = snapshot
            .sessions
            .iter()
            .find(|session| session.provider_session_id == codex_vscode_id)
            .unwrap();
        assert_eq!(vscode_codex.jump_label, "Open application");
        assert!(matches!(
            jump_target(vscode_codex),
            Some(JumpTarget::AppBundle("com.microsoft.VSCode"))
        ));
        let terminal = snapshot
            .sessions
            .iter()
            .find(|session| session.provider_session_id == "iterm-session")
            .unwrap();
        assert_eq!(terminal.jump_label, "Open terminal");
        assert_eq!(jump_target(terminal), Some(JumpTarget::ITermSession));
        let app = snapshot
            .sessions
            .iter()
            .find(|session| session.provider_session_id == "claude-app-session")
            .unwrap();
        assert_eq!(app.jump_label, "Open application");
        assert_eq!(
            jump_target(app),
            Some(JumpTarget::AppBundle("com.anthropic.claudefordesktop"))
        );
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn auth_contract_rejects_missing_cookie_forged_origin_and_missing_csrf() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-unit-{}-{}",
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
            "actrealm-server-visible-sessions-{}-{}",
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
                json!({
                    "hook_event_name":"UserPromptSubmit",
                    "session_id":"recent-idle",
                    "prompt":"recent meaningful task"
                }),
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
            let request = actrealm_core::BridgeRequest::from_hook_at(
                actrealm_core::Provider::Claude,
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
        let attention_session = sessions
            .iter()
            .find(|session| session["providerSessionId"] == "old-attention")
            .unwrap();
        assert_eq!(
            attention_session["activityMessage"]["code"],
            "session.activity.awaiting_approval"
        );
        let attention = value["attention"].as_array().unwrap().first().unwrap();
        assert_eq!(
            attention["titleMessage"]["code"],
            "attention.approval.title"
        );
        assert_eq!(
            attention["detailMessage"]["code"],
            "attention.approval.detail"
        );
        assert!(attention["riskMessages"].as_array().is_some_and(|items| {
            items
                .iter()
                .all(|message| message["code"].as_str().is_some())
        }));
        drop(state);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn new_claude_cache_refreshes_immediately_after_an_unavailable_snapshot() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-quota-cache-refresh-{}-{}",
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
        assert!(before["quota"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["provider"] == "claude")
            .all(|entry| entry["reasonMessage"]["code"] == "quota.reason.cache_missing"));

        let cache = state.quota.lock().unwrap().collector.paths().claude_cache();
        actrealm_quota::capture_claude_statusline(
            &serde_json::to_vec(&json!({"rate_limits":{"five_hour":{"used_percentage":2,"resets_at":now_millis()/1000+3600},"seven_day":{"used_percentage":0,"resets_at":now_millis()/1000+604800}}})).unwrap(),
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
        assert_eq!(claude[0]["windowMessage"]["code"], "quota.window.hours");
        assert_eq!(claude[0]["windowMessage"]["args"]["count"], "5");

        drop(state);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_quota_refresh_is_authenticated_and_invalidates_the_runtime_cache() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-quota-wake-refresh-{}-{}",
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
        {
            let mut quota = state.quota.lock().unwrap();
            quota.refreshed_at = Some(Instant::now());
            quota.claude_cache_modified_at = Some(SystemTime::UNIX_EPOCH);
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let response = router(state.clone())
                .oneshot(authorized_request(
                    "POST",
                    "/api/v1/quota/refresh",
                    Value::Null,
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body = json_body(response).await;
            assert_eq!(body["accepted"], true);
        });
        let quota = state.quota.lock().unwrap();
        assert!(quota.refreshed_at.is_some());
        assert_ne!(quota.claude_cache_modified_at, Some(SystemTime::UNIX_EPOCH));
        drop(quota);
        drop(state);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manual_quota_refresh_requires_authentication_and_enabled_oauth() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-quota-manual-auth-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let state = test_state(store.clone(), &root);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let unauthorized = Request::builder()
                .method("POST")
                .uri("/api/v1/quota/refresh-now")
                .body(Body::empty())
                .unwrap();
            let response = router(state.clone()).oneshot(unauthorized).await.unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);

            {
                let mut auth = state.auth.lock().unwrap();
                auth.bootstrap_token = None;
                auth.session_token = Some("test-session".to_owned());
                auth.csrf_token = Some("test-csrf".to_owned());
            }
            let response = router(state.clone())
                .oneshot(authorized_request(
                    "POST",
                    "/api/v1/quota/refresh-now",
                    Value::Null,
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::CONFLICT);
            let body = json_body(response).await;
            assert_eq!(body["error"]["code"], "CLAUDE_OAUTH_DISABLED");
        });
        drop(state);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn m4_settings_quota_export_and_clear_follow_the_authenticated_ui_path() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-m4-{}-{}",
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
        actrealm_quota::capture_claude_statusline(
            &serde_json::to_vec(&json!({"rate_limits":{"five_hour":{"used_percentage":25,"resets_at":now_millis()/1000+3600}}})).unwrap(),
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
                "retentionDays": 30,
                "tokenUsageHeatmapVisible": false,
                "tokenUsageCostVisible": false,
                "tokenUsageObservedTimeVisible": false,
                "tokenUsageExecutionTimeVisible": false,
                "completionTaskHideMode": "afterDelay",
                "completionAutoHideMinutes": 15
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
            assert_eq!(updated["settings"]["tokenUsageHeatmapVisible"], false);
            assert_eq!(updated["settings"]["tokenUsageCostVisible"], false);
            assert_eq!(updated["settings"]["tokenUsageObservedTimeVisible"], false);
            assert_eq!(updated["settings"]["tokenUsageExecutionTimeVisible"], false);
            assert_eq!(updated["settings"]["completionTaskHideMode"], "afterDelay");
            assert_eq!(updated["settings"]["completionAutoHideMinutes"], 15);
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
            let wrapped_status = wrapped.status();
            let wrapped = json_body(wrapped).await;
            assert_eq!(wrapped_status, StatusCode::OK, "{wrapped}");
            assert_eq!(wrapped["claudeQuotaBridge"]["status"], "installed");
            let wrapped_settings =
                serde_json::from_slice::<Value>(&fs::read(&claude_settings).unwrap()).unwrap();
            assert_eq!(wrapped_settings["keep"], true);
            assert_eq!(
                wrapped_settings["_actRealmOriginalStatusLine"]["command"],
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
                "attachment; filename=actrealm-export.json"
            );
            let exported = json_body(exported).await;
            assert_eq!(exported["tables"]["settings"].as_array().unwrap().len(), 1);

            let token_json = app
                .clone()
                .oneshot(authorized_request(
                    "GET",
                    "/api/v1/token-usage/export",
                    Value::Null,
                ))
                .await
                .unwrap();
            assert_eq!(token_json.status(), StatusCode::OK);
            assert_eq!(
                token_json.headers()[CONTENT_DISPOSITION],
                "attachment; filename=actrealm-token-usage.json"
            );
            let token_json = json_body(token_json).await;
            assert_eq!(token_json["scope"], "token_usage_numeric");
            assert_eq!(token_json["dataQuality"], "pending");
            assert!(token_json["daily"].is_array());

            let token_csv = app
                .clone()
                .oneshot(authorized_request(
                    "GET",
                    "/api/v1/token-usage/export.csv",
                    Value::Null,
                ))
                .await
                .unwrap();
            assert_eq!(token_csv.status(), StatusCode::OK);
            assert_eq!(
                token_csv.headers()[CONTENT_DISPOSITION],
                "attachment; filename=actrealm-token-usage.csv"
            );
            assert_eq!(token_csv.headers()[CONTENT_TYPE], "text/csv; charset=utf-8");
            let token_csv = to_bytes(token_csv.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap();
            assert!(token_csv.starts_with(b"day,provider,model,input_tokens"));

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
            "actrealm-server-m4-corrupt-{}-{}",
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
            "actrealm-server-m5-metrics-{}-{}",
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
                "attachment; filename=actrealm-metrics.json"
            );
            let metrics_export = json_body(metrics_export).await;
            assert_eq!(metrics_export["scope"], "metrics_only");
            assert!(metrics_export.get("tables").is_none());
        });
        assert!(INDEX_HTML.contains("使用统计"));
        assert!(INDEX_HTML.contains("导出统计"));
        assert!(APP_JS.contains("widgetApprovals"));
        assert!(APP_JS.contains("banner_shown"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn m5_api_rejects_bodies_over_64_kib_before_deserialization() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-m5-body-{}-{}",
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
        assert_eq!(legacy.quota_display_mode, "standard");
        assert_eq!(legacy.completion_task_hide_mode, "afterConfirmation");
        assert_eq!(legacy.completion_auto_hide_minutes, 30);
        assert!(legacy.task_card_fields.contains(&"activity".to_owned()));
        legacy.validate().unwrap();

        let migrated =
            decode_ui_settings(r#"{"displayProfile":"custom","taskCardFields":["tokens"]}"#)
                .unwrap();
        assert_eq!(
            migrated.task_card_fields,
            vec![
                "sessionTokens".to_owned(),
                "turnTokens".to_owned(),
                "inputOutputTokens".to_owned(),
                "cacheTokens".to_owned(),
                "reasoningTokens".to_owned(),
                "cost".to_owned(),
                "workflow".to_owned(),
            ]
        );
        assert_eq!(migrated.display_fields_version, 5);

        let version_two = decode_ui_settings(
            r#"{"displayProfile":"custom","displayFieldsVersion":2,"taskCardFields":["tokens"]}"#,
        )
        .unwrap();
        assert_eq!(
            version_two.task_card_fields,
            vec![
                "sessionTokens".to_owned(),
                "turnTokens".to_owned(),
                "inputOutputTokens".to_owned(),
                "cacheTokens".to_owned(),
                "reasoningTokens".to_owned(),
                "workflow".to_owned(),
            ]
        );

        let independent = decode_ui_settings(
            r#"{"displayProfile":"custom","displayFieldsVersion":3,"taskCardFields":["turnTokens"]}"#,
        )
        .unwrap();
        assert_eq!(
            independent.task_card_fields,
            vec!["turnTokens".to_owned(), "workflow".to_owned()]
        );

        let mut stale_client = UiSettings {
            display_profile: "custom".to_owned(),
            task_card_fields: vec!["tokens".to_owned()],
            display_fields_version: 3,
            ..UiSettings::default()
        };
        migrate_display_fields(&mut stale_client, Some(3));
        assert_eq!(
            stale_client.task_card_fields,
            vec![
                "sessionTokens".to_owned(),
                "turnTokens".to_owned(),
                "inputOutputTokens".to_owned(),
                "cacheTokens".to_owned(),
                "reasoningTokens".to_owned(),
                "workflow".to_owned(),
            ]
        );

        let custom = UiSettings {
            display_profile: "custom".to_owned(),
            task_card_fields: vec!["project".to_owned(), "activity".to_owned()],
            ..UiSettings::default()
        };
        custom.validate().unwrap();

        let compact_quota = UiSettings {
            quota_display_mode: "compact".to_owned(),
            ..UiSettings::default()
        };
        compact_quota.validate().unwrap();
        let two_line_quota = UiSettings {
            quota_display_mode: "twoLine".to_owned(),
            ..UiSettings::default()
        };
        two_line_quota.validate().unwrap();
        let invalid_quota = UiSettings {
            quota_display_mode: "dense".to_owned(),
            ..UiSettings::default()
        };
        assert_eq!(
            invalid_quota.validate(),
            Err("quotaDisplayMode must be standard, twoLine, or compact")
        );
        let hidden_token_usage = UiSettings {
            token_usage_display_mode: "hidden".to_owned(),
            ..UiSettings::default()
        };
        hidden_token_usage.validate().unwrap();
        let invalid_token_usage = UiSettings {
            token_usage_display_mode: "dense".to_owned(),
            ..UiSettings::default()
        };
        assert_eq!(
            invalid_token_usage.validate(),
            Err("tokenUsageDisplayMode must be standard, compact, or hidden")
        );
        let east_asian_units = UiSettings {
            token_usage_unit_style: "eastAsian".to_owned(),
            ..UiSettings::default()
        };
        east_asian_units.validate().unwrap();
        let invalid_units = UiSettings {
            token_usage_unit_style: "decimal".to_owned(),
            ..UiSettings::default()
        };
        assert_eq!(
            invalid_units.validate(),
            Err("tokenUsageUnitStyle must be automatic, western, or eastAsian")
        );
        let automatic_completion_hide = UiSettings {
            completion_task_hide_mode: "afterDelay".to_owned(),
            completion_auto_hide_minutes: 30,
            ..UiSettings::default()
        };
        automatic_completion_hide.validate().unwrap();
        UiSettings {
            completion_task_hide_mode: "manual".to_owned(),
            ..UiSettings::default()
        }
        .validate()
        .unwrap();
        let invalid_completion_hide = UiSettings {
            completion_task_hide_mode: "idleTimeout".to_owned(),
            ..UiSettings::default()
        };
        assert_eq!(
            invalid_completion_hide.validate(),
            Err("completionTaskHideMode must be afterConfirmation, afterDelay, or manual")
        );
        let invalid_completion_delay = UiSettings {
            completion_auto_hide_minutes: 10,
            ..UiSettings::default()
        };
        assert_eq!(
            invalid_completion_delay.validate(),
            Err("completionAutoHideMinutes must be 5, 15, 30, or 60")
        );

        for independent_usage_field in [
            "sessionTokens",
            "turnTokens",
            "inputOutputTokens",
            "cacheTokens",
            "reasoningTokens",
            "cost",
        ] {
            let custom = UiSettings {
                display_profile: "custom".to_owned(),
                task_card_fields: vec![independent_usage_field.to_owned()],
                ..UiSettings::default()
            };
            custom.validate().unwrap();
        }
        assert!(UiSettings::default()
            .task_card_fields
            .contains(&"cost".to_owned()));
        assert_eq!(UiSettings::default().display_fields_version, 5);
        assert_eq!(UiSettings::default().quota_display_mode, "standard");
        assert_eq!(UiSettings::default().token_usage_display_mode, "standard");
        assert!(UiSettings::default().token_usage_components_visible);
        assert!(UiSettings::default().token_usage_execution_time_visible);
        assert_eq!(UiSettings::default().token_usage_unit_style, "automatic");
        assert!(UiSettings::default()
            .task_card_fields
            .contains(&"taskFlow".to_owned()));
        assert!(UiSettings::default()
            .task_card_fields
            .contains(&"workflow".to_owned()));
        assert!(UiSettings::default()
            .task_card_fields
            .contains(&"currentTarget".to_owned()));
        assert!(!UiSettings::default()
            .task_card_fields
            .contains(&"tokens".to_owned()));

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
        assert!(encoded.contains(r#""quotaDisplayMode":"standard""#));
        assert!(encoded.contains(r#""tokenUsageDisplayMode":"standard""#));
        assert!(encoded.contains(r#""tokenUsageComponentsVisible":true"#));
        assert!(encoded.contains(r#""tokenUsageUnitStyle":"automatic""#));
        assert!(!encoded.contains("raw"));
        assert!(!encoded.contains("payload"));
        assert!(!encoded.contains("command"));
    }

    #[test]
    fn interactive_question_is_ephemeral_answerable_and_removed_from_snapshot() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-question-{}-{}",
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
        let request = actrealm_core::BridgeRequest::from_hook_at(
            actrealm_core::Provider::Claude,
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
        assert_eq!(response.action, actrealm_core::ReplyAction::Answer);
        let after = snapshot_value(&state).unwrap();
        assert!(after["attention"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item.get("interaction").is_none()));
        let recovered = store.snapshot().unwrap();
        assert_eq!(recovered.sessions[0].exec_state, "waiting_for_event");
        assert_eq!(recovered.sessions[0].approval_owner, None);
        assert_eq!(
            recovered.sessions[0].activity.as_deref(),
            Some("Attention item resolved; waiting for the Agent's next event")
        );
        let exported = serde_json::to_string(&store.export_json(now_millis()).unwrap()).unwrap();
        assert!(!exported.contains("secret-48291"));
        drop(state);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn remote_approval_capability_requires_a_live_waiter() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-remote-capability-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let state = test_state(store.clone(), &root);
        let request = actrealm_core::BridgeRequest::from_hook_at(
            actrealm_core::Provider::Claude,
            json!({
                "hook_event_name":"PermissionRequest",
                "session_id":"remote-capability",
                "tool_name":"Bash",
                "tool_input":{"command":"git push origin main"}
            }),
            now_millis(),
        );
        let request_id = request.request_id.unwrap();
        let registration = state.waiters.register_at(&request, now_millis()).unwrap();
        store.ingest(request).unwrap();

        let undeclared = actrealm_core::BridgeRequest::from_hook_at(
            actrealm_core::Provider::Claude,
            json!({
                "hook_event_name":"PermissionRequest",
                "session_id":"remote-capability-undeclared",
                "tool_name":"FutureAutonomousTool",
                "tool_input":{}
            }),
            now_millis(),
        );
        let undeclared_id = undeclared.request_id.unwrap();
        let undeclared_registration = state
            .waiters
            .register_at(&undeclared, now_millis())
            .unwrap();
        store.ingest(undeclared).unwrap();

        let codex = actrealm_core::BridgeRequest::from_hook_at(
            actrealm_core::Provider::Codex,
            json!({
                "hook_event_name":"PermissionRequest",
                "session_id":"remote-capability-codex",
                "tool_name":"exec_command",
                "tool_input":{"command":"sudo true"}
            }),
            now_millis(),
        );
        let codex_id = codex.request_id.unwrap();
        let codex_registration = state.waiters.register_at(&codex, now_millis()).unwrap();
        store.ingest(codex).unwrap();

        let active = snapshot_value(&state).unwrap();
        let attention = active["attention"].as_array().unwrap();
        let declared = attention
            .iter()
            .find(|item| item["requestId"] == request_id.to_string())
            .unwrap();
        let default_denied = attention
            .iter()
            .find(|item| item["requestId"] == undeclared_id.to_string())
            .unwrap();
        let codex_declared = attention
            .iter()
            .find(|item| item["requestId"] == codex_id.to_string())
            .unwrap();
        assert_eq!(declared["remoteActionable"], true);
        assert_eq!(declared["risk"], "high");
        assert_eq!(declared["allowedActions"], json!(["approve", "deny"]));
        assert_eq!(default_denied["remoteActionable"], false);
        assert_eq!(default_denied["allowedActions"], json!(["deny"]));
        assert_eq!(codex_declared["risk"], "high");
        assert_eq!(codex_declared["allowedActions"], json!(["approve", "deny"]));

        let companion = companion_snapshot_value(
            &state,
            &CompanionAuthorization {
                id: "display".to_owned(),
                scopes: vec![
                    COMPANION_SCOPE_SNAPSHOT.to_owned(),
                    COMPANION_SCOPE_RESPOND.to_owned(),
                ],
            },
        )
        .unwrap();
        let companion_declared = companion["attention"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["requestId"] == request_id.to_string())
            .unwrap();
        assert_eq!(
            companion_declared["allowedActions"],
            json!(["approve", "deny"])
        );

        let read_only_companion = companion_snapshot_value(
            &state,
            &CompanionAuthorization {
                id: "display-read-only".to_owned(),
                scopes: vec![COMPANION_SCOPE_SNAPSHOT.to_owned()],
            },
        )
        .unwrap();
        assert!(read_only_companion["attention"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["allowedActions"] == json!([])));

        state
            .waiters
            .pass_through(request_id, "test_waiter_closed")
            .unwrap();
        let _ = registration.ticket.recv_timeout(Duration::from_secs(1));
        let stale = snapshot_value(&state).unwrap();
        let declared = stale["attention"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["requestId"] == request_id.to_string())
            .unwrap();
        assert_eq!(declared["remoteActionable"], false);
        assert_eq!(declared["allowedActions"], json!([]));

        state
            .waiters
            .pass_through(undeclared_id, "test_waiter_closed")
            .unwrap();
        let _ = undeclared_registration
            .ticket
            .recv_timeout(Duration::from_secs(1));
        state
            .waiters
            .pass_through(codex_id, "test_waiter_closed")
            .unwrap();
        let _ = codex_registration
            .ticket
            .recv_timeout(Duration::from_secs(1));

        drop(state);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_and_companion_cannot_approve_a_deny_only_request() {
        let root = std::env::temp_dir().join(format!("actrealm-deny-only-{}", Uuid::now_v7()));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let state = test_state(store.clone(), &root);
        let request = BridgeRequest::from_hook_at(
            Provider::Claude,
            json!({
                "hook_event_name":"PermissionRequest", "session_id":"unknown-tool",
                "tool_name":"FutureAutonomousTool", "tool_input":{}
            }),
            now_millis(),
        );
        let request_id = request.request_id.unwrap();
        let registration = state.waiters.register_at(&request, now_millis()).unwrap();
        store.ingest(request).unwrap();
        let attention = store.snapshot().unwrap().attention[0].clone();
        let snapshot = snapshot_value(&state).unwrap();
        assert_eq!(snapshot["attention"][0]["allowedActions"], json!(["deny"]));
        let rejected = process_command(
            &state,
            CommandRequest {
                id: Uuid::now_v7(),
                attention_id: attention.id.clone(),
                request_id: Some(request_id),
                action: "approve".into(),
                undo_delay_ms: Some(0),
            },
        );
        assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
        assert!(store.snapshot().unwrap().commands.is_empty());
        assert!(state.waiters.is_active(request_id).unwrap());
        let denied = process_command(
            &state,
            CommandRequest {
                id: Uuid::now_v7(),
                attention_id: attention.id,
                request_id: Some(request_id),
                action: "deny".into(),
                undo_delay_ms: Some(0),
            },
        );
        assert_eq!(denied.status(), StatusCode::OK);
        assert_eq!(
            registration
                .ticket
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .action,
            ReplyAction::Deny
        );
        drop(state);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn verified_high_risk_approval_can_commit_immediately_when_undo_is_disabled() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-immediate-approval-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let state = test_state(store.clone(), &root);
        let request = actrealm_core::BridgeRequest::from_hook_at(
            actrealm_core::Provider::Claude,
            json!({
                "hook_event_name":"PermissionRequest",
                "session_id":"immediate-high-risk",
                "tool_name":"Bash",
                "tool_input":{"command":"git push origin main"}
            }),
            now_millis(),
        );
        let request_id = request.request_id.unwrap();
        let registration = state.waiters.register_at(&request, now_millis()).unwrap();
        store.ingest(request).unwrap();
        let attention = store.snapshot().unwrap().attention[0].clone();
        assert_eq!(attention.risk, "high");

        let command_id = Uuid::now_v7();
        let response = process_command(
            &state,
            CommandRequest {
                id: command_id,
                attention_id: attention.id,
                request_id: Some(request_id),
                action: "approve".to_owned(),
                undo_delay_ms: Some(0),
            },
        );
        assert_eq!(response.status(), StatusCode::OK);
        let directive = registration
            .ticket
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        assert_eq!(directive.action, ReplyAction::Allow);
        assert_eq!(store.snapshot().unwrap().commands[0].state, "decision_sent");
        assert_eq!(
            store.undo(command_id, now_millis()),
            Err(StoreError::NotUndoable)
        );

        drop(state);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_snapshot_distinguishes_observing_lost_and_managed_sessions() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-recovery-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        for (session, pid) in [
            ("live-external", std::process::id()),
            ("dead-external", u32::MAX - 1),
        ] {
            let mut request = actrealm_core::BridgeRequest::from_hook_at(
                actrealm_core::Provider::Claude,
                json!({
                    "hook_event_name":"UserPromptSubmit",
                    "session_id":session,
                    "prompt":"recovery test"
                }),
                now_millis(),
            );
            let term = request.term.as_mut().unwrap();
            term.provider_pid = Some(pid);
            term.bundle_id = None;
            term.surface = None;
            store.ingest(request).unwrap();
        }
        let mut managed = actrealm_core::BridgeRequest::from_hook_at(
            actrealm_core::Provider::Codex,
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
                    approval_policy: None,
                    approvals_reviewer: None,
                    sandbox_mode: None,
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
    fn reused_desktop_pid_identity_never_projects_observing() {
        assert_eq!(
            direct_app_identity_matches(
                "codex",
                Some("com.openai.chat"),
                Some("codex_app"),
                Some("/Applications/Other.app/Contents/MacOS/Other"),
            ),
            Some(false)
        );
        assert_eq!(
            direct_app_identity_matches(
                "codex",
                Some("com.openai.chat"),
                Some("codex_app"),
                Some("/Applications/ChatGPT.app/Contents/MacOS/ChatGPT"),
            ),
            Some(true)
        );
        assert_eq!(
            direct_app_identity_matches(
                "claude",
                Some("com.apple.Terminal"),
                Some("terminal"),
                Some("/bin/zsh"),
            ),
            None
        );
    }

    #[test]
    fn managed_waiting_recovery_uses_persisted_process_liveness() {
        // The test runner is a live process, not a verified desktop Provider.
        // Exercise liveness through a terminal surface; app identity is tested
        // separately with fixed executable paths above.
        assert_eq!(
            recovery_from_persisted_process(
                "codex",
                "tool_running",
                "waiting_for_event".to_owned(),
                Some(std::process::id()),
                Some("com.apple.Terminal"),
                Some("terminal"),
            ),
            "observing"
        );
        assert_eq!(
            recovery_from_persisted_process(
                "codex",
                "tool_running",
                "waiting_for_event".to_owned(),
                Some(u32::MAX - 1),
                Some("com.openai.codex"),
                Some("codex_app"),
            ),
            "lost_control"
        );
        assert_eq!(
            recovery_from_persisted_process(
                "codex",
                "tool_running",
                "waiting_for_event".to_owned(),
                None,
                None,
                None,
            ),
            "waiting_for_event"
        );
    }

    #[test]
    fn running_execution_never_projects_recovery_as_ended() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-recovery-precedence-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        store
            .ingest(actrealm_core::BridgeRequest::from_hook_at(
                actrealm_core::Provider::Codex,
                json!({
                    "hook_event_name":"PreToolUse",
                    "session_id":"managed-live-thread",
                    "tool_name":"Bash",
                    "tool_input":{}
                }),
                now_millis(),
            ))
            .unwrap();
        assert_eq!(
            store.snapshot().unwrap().sessions[0].exec_state,
            "tool_running"
        );

        let mut state = test_state(store.clone(), &root);
        state.codex.state = Arc::new(Mutex::new(CodexManagerState {
            status: "connected".to_owned(),
            managed: HashSet::from(["managed-live-thread".to_owned()]),
            threads: HashMap::from([(
                "managed-live-thread".to_owned(),
                CodexThread {
                    id: "managed-live-thread".to_owned(),
                    name: None,
                    cwd: None,
                    status: "idle".to_owned(),
                    active_flags: vec![],
                    updated_at: None,
                    approval_policy: None,
                    approvals_reviewer: None,
                    sandbox_mode: None,
                },
            )]),
            ..CodexManagerState::default()
        }));

        let snapshot = snapshot_value(&state).unwrap();
        let session = snapshot["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|session| session["providerSessionId"] == "managed-live-thread")
            .unwrap();
        assert_eq!(session["execState"], "tool_running");
        assert_eq!(
            session["recoveryState"], "observing",
            "the live persisted Provider PID is stronger evidence than the stale idle thread row"
        );

        drop(state);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_native_waiting_flag_opens_and_resolves_attention() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-native-approval-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        store
            .ingest(actrealm_core::BridgeRequest::from_hook_at(
                actrealm_core::Provider::Codex,
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
    fn initial_codex_listing_without_waiting_flag_preserves_hook_native_approval() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-native-restart-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Codex,
                json!({
                    "hook_event_name":"PreToolUse",
                    "session_id":"thread-native-restart",
                    "turn_id":"turn-1",
                    "tool_name":"request_permissions",
                    "tool_use_id":"native-restart-request",
                    "tool_input":{"reason":"等待 Codex 原界面决定"}
                }),
                1_000,
            ))
            .unwrap();
        let state = Arc::new(Mutex::new(CodexManagerState::default()));
        let waiters = WaiterRegistry::default();
        let listed = CodexThread {
            id: "thread-native-restart".to_owned(),
            name: Some("restart fixture".to_owned()),
            cwd: Some("/tmp/example".to_owned()),
            status: "active".to_owned(),
            active_flags: Vec::new(),
            updated_at: None,
            approval_policy: None,
            approvals_reviewer: None,
            sandbox_mode: None,
        };
        state
            .lock()
            .unwrap()
            .threads
            .insert(listed.id.clone(), listed.clone());

        // A fresh app-server process may not project the already-running
        // Desktop Turn. Absence of waitingOnApproval in this first listing is
        // not proof that the native sheet was handled.
        sync_initial_codex_native_attention(&state, &store, &waiters, &listed);
        let preserved = store.snapshot().unwrap();
        assert_eq!(preserved.attention.len(), 1);
        assert_eq!(preserved.attention[0].kind, "native_approval");
        assert_eq!(preserved.attention[0].state, "open");
        assert_eq!(preserved.sessions[0].exec_state, "awaiting_approval");
        assert_eq!(
            preserved.sessions[0].approval_owner.as_deref(),
            Some("terminal")
        );
        assert!(!state
            .lock()
            .unwrap()
            .native_synced
            .contains("thread-native-restart"));

        // After the Connector has positively observed waiting=true, its later
        // false transition is authoritative and may resolve the request.
        update_codex_notification(
            &state,
            &store,
            &waiters,
            ServerNotification {
                method: "thread/status/changed".to_owned(),
                params: json!({
                    "threadId":"thread-native-restart",
                    "status":{"type":"active","activeFlags":["waitingOnApproval"]}
                }),
            },
        );
        assert!(state
            .lock()
            .unwrap()
            .native_synced
            .contains("thread-native-restart"));
        update_codex_notification(
            &state,
            &store,
            &waiters,
            ServerNotification {
                method: "thread/status/changed".to_owned(),
                params: json!({
                    "threadId":"thread-native-restart",
                    "status":{"type":"active","activeFlags":[]}
                }),
            },
        );
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

    #[test]
    fn codex_native_resolution_releases_a_competing_hook_waiter() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-hook-release-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let request = actrealm_core::BridgeRequest::from_hook_at(
            actrealm_core::Provider::Codex,
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

    #[test]
    fn codex_connector_plan_completion_and_subagents_reach_runtime() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-connector-events-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Codex,
                json!({
                    "hook_event_name":"SessionStart",
                    "session_id":"connector-events",
                    "turn_id":"turn-1"
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
                method: "turn/started".to_owned(),
                params: json!({
                    "threadId":"connector-events",
                    "turn":{"id":"turn-1"}
                }),
            },
        );
        assert_eq!(store.snapshot().unwrap().sessions[0].exec_state, "thinking");

        update_codex_notification(
            &state,
            &store,
            &waiters,
            ServerNotification {
                method: "turn/plan/updated".to_owned(),
                params: json!({
                    "threadId":"connector-events",
                    "steps":[
                        {"text":"分析", "state":"completed"},
                        {"text":"验证", "state":"inProgress"}
                    ],
                    "explanation":"来自 Codex app-server"
                }),
            },
        );
        update_codex_notification(
            &state,
            &store,
            &waiters,
            ServerNotification {
                method: "item/started".to_owned(),
                params: json!({
                    "threadId":"connector-events",
                    "turnId":"turn-1",
                    "item":{
                        "type":"collabAgentToolCall",
                        "id":"collab-1",
                        "model":"gpt-5.6-sol",
                        "agentsStates":{"child-1":{"status":"running"}}
                    }
                }),
            },
        );
        let running = store.snapshot().unwrap();
        assert_eq!(running.sessions[0].active_subagents, 1);
        assert_eq!(
            running.sessions[0].subagents[0].agent_type.as_deref(),
            Some("gpt-5.6-sol")
        );
        assert_eq!(
            (
                running.sessions[0].plan_done,
                running.sessions[0].plan_total
            ),
            (Some(1), Some(2))
        );
        assert_eq!(running.sessions[0].plan_steps[1].text, "验证");
        let connector = state.lock().unwrap();
        assert_eq!(
            connector.last_notification_method.as_deref(),
            Some("item/started")
        );
        assert_eq!(connector.last_plan_skip_reason, None);
        drop(connector);

        update_codex_notification(
            &state,
            &store,
            &waiters,
            ServerNotification {
                method: "turn/completed".to_owned(),
                params: json!({
                    "threadId":"connector-events",
                    "turn":{"id":"turn-1", "status":"completed", "items":[]}
                }),
            },
        );
        let completed = store.snapshot().unwrap();
        assert_eq!(completed.sessions[0].exec_state, "response_finished");
        assert_eq!(completed.sessions[0].active_subagents, 0);
        assert!(completed
            .attention
            .iter()
            .any(|item| item.kind == "completion" && item.state == "open"));
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_plan_diagnostics_are_bounded_and_plan_aliases_are_canonicalized() {
        let missing_thread = json!({"steps":[]});
        assert_eq!(
            codex_plan_diagnostic(&missing_thread),
            (Some("missing_thread_id"), Vec::new())
        );

        let missing_plan = json!({
            "threadId":"thread",
            "z_future_field":true,
            "unsafe key":true
        });
        assert_eq!(
            codex_plan_diagnostic(&missing_plan),
            (
                Some("missing_plan_array"),
                vec!["threadId".to_owned(), "z_future_field".to_owned()]
            )
        );

        let notification = ServerNotification {
            method: "turn/plan/updated".to_owned(),
            params: json!({
                "thread":{"id":"thread"},
                "items":[{"content":"Review", "state":"pending"}]
            }),
        };
        assert_eq!(
            codex_plan_diagnostic(&notification.params),
            (None, Vec::new())
        );
        let events = codex_notification_events(&notification, 42);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0]
                .raw
                .pointer("/plan/0/content")
                .and_then(Value::as_str),
            Some("Review")
        );
        assert_eq!(events[0].provider_turn_id, None);
    }

    #[test]
    fn lifecycle_only_sessions_are_hidden_until_meaningful_activity_arrives() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-session-visibility-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let state = test_state(store.clone(), &root);
        let now = now_millis();
        store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Claude,
                json!({
                    "hook_event_name":"SessionStart",
                    "session_id":"history-only"
                }),
                now,
            ))
            .unwrap();
        let lifecycle = snapshot_value(&state).unwrap();
        assert!(lifecycle["sessions"].as_array().unwrap().is_empty());

        store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Claude,
                json!({
                    "hook_event_name":"UserPromptSubmit",
                    "session_id":"history-only",
                    "prompt":"现在开始真实任务"
                }),
                now.saturating_add(1),
            ))
            .unwrap();
        let active = snapshot_value(&state).unwrap();
        let sessions = active["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["providerSessionId"], "history-only");
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_auto_review_and_full_access_do_not_impersonate_user_approval() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-auto-review-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        for session in ["auto-review", "full-access"] {
            store
                .ingest(BridgeRequest::from_hook_at(
                    Provider::Codex,
                    json!({
                        "hook_event_name":"UserPromptSubmit",
                        "session_id":session,
                        "turn_id":"turn-1",
                        "prompt":"需要权限"
                    }),
                    1_000,
                ))
                .unwrap();
        }
        let state = Arc::new(Mutex::new(CodexManagerState::default()));
        let waiters = WaiterRegistry::default();

        update_codex_notification(
            &state,
            &store,
            &waiters,
            ServerNotification {
                method: "item/autoApprovalReview/started".to_owned(),
                params: json!({
                    "threadId":"auto-review",
                    "turnId":"turn-1",
                    "reviewId":"review-1",
                    "review":{"status":"inProgress"}
                }),
            },
        );
        update_codex_notification(
            &state,
            &store,
            &waiters,
            ServerNotification {
                method: "thread/status/changed".to_owned(),
                params: json!({
                    "threadId":"auto-review",
                    "status":{"type":"active", "activeFlags":["waitingOnApproval"]}
                }),
            },
        );
        update_codex_notification(
            &state,
            &store,
            &waiters,
            ServerNotification {
                method: "thread/started".to_owned(),
                params: json!({
                    "thread":{
                        "id":"full-access",
                        "status":{"type":"active", "activeFlags":["waitingOnApproval"]},
                        "approvalPolicy":"never"
                    }
                }),
            },
        );
        let provider_owned = store.snapshot().unwrap();
        assert!(!provider_owned
            .attention
            .iter()
            .any(|item| { item.kind == "native_approval" && item.state == "open" }));

        update_codex_notification(
            &state,
            &store,
            &waiters,
            ServerNotification {
                method: "item/autoApprovalReview/completed".to_owned(),
                params: json!({
                    "threadId":"auto-review",
                    "turnId":"turn-1",
                    "reviewId":"review-1",
                    "review":{"status":"timedOut"}
                }),
            },
        );
        let escalated = store.snapshot().unwrap();
        assert!(escalated
            .attention
            .iter()
            .any(|item| { item.kind == "native_approval" && item.state == "open" }));
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn managed_codex_approval_round_trip_is_scoped_and_resolved() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-managed-approval-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("fake-codex");
        fs::write(
            &executable,
            r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"id":1,"result":{"userAgent":"codex_cli_rs/0.144.6"}}'
      ;;
    *'"id":"approval-1"'*)
      printf '%s\n' '{"method":"serverRequest/resolved","params":{"threadId":"thread-1","requestId":"approval-1"}}'
      ;;
  esac
done
"#,
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let socket = root.join("unused.sock");
        let (connector, channels) = CodexConnector::connect(&executable, &socket).unwrap();
        assert!(connector.supports_managed_approvals());
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let waiters = WaiterRegistry::default();
        let state = Arc::new(Mutex::new(CodexManagerState {
            managed: HashSet::from(["thread-1".to_owned()]),
            ..CodexManagerState::default()
        }));
        handle_codex_server_request(
            connector.clone(),
            Arc::clone(&state),
            store.clone(),
            waiters.clone(),
            ServerRequest {
                id: json!("detached-approval"),
                method: "item/fileChange/requestApproval".to_owned(),
                params: json!({
                    "threadId":"not-managed",
                    "turnId":"turn-1",
                    "itemId":"item-detached",
                    "startedAtMs":1_000,
                    "grantRoot":"/tmp/project"
                }),
            },
        );
        assert!(store.snapshot().unwrap().attention.is_empty());
        handle_codex_server_request(
            connector.clone(),
            Arc::clone(&state),
            store.clone(),
            waiters.clone(),
            ServerRequest {
                id: json!("approval-1"),
                method: "item/commandExecution/requestApproval".to_owned(),
                params: json!({
                    "threadId":"thread-1",
                    "turnId":"turn-1",
                    "itemId":"item-1",
                    "startedAtMs":1_000,
                    "command":"cargo test",
                    "cwd":"/tmp/project"
                }),
            },
        );
        let request_id = store.snapshot().unwrap().attention[0].request_id.unwrap();
        waiters
            .decide(request_id, actrealm_core::Decision::Allow)
            .unwrap();
        let notification = channels
            .notifications
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        update_codex_notification(&state, &store, &waiters, notification);
        let resolved = store.snapshot().unwrap();
        assert_eq!(resolved.attention[0].state, "resolved");
        assert_eq!(
            resolved.attention[0].resolution.as_deref(),
            Some("provider_resolved")
        );

        let granted = codex_approval_response(
            "item/permissions/requestApproval",
            &json!({
                "permissions":{
                    "network":{"enabled":true},
                    "fileSystem":null,
                    "unknownFuturePermission":{"enabled":true}
                }
            }),
            ReplyAction::Allow,
        )
        .unwrap();
        assert_eq!(
            granted.pointer("/permissions/network/enabled"),
            Some(&json!(true))
        );
        assert!(granted.pointer("/permissions/fileSystem").is_none());
        assert!(granted
            .pointer("/permissions/unknownFuturePermission")
            .is_none());
        let denied = codex_approval_response(
            "item/permissions/requestApproval",
            &json!({"permissions":{"network":{"enabled":true}}}),
            ReplyAction::Deny,
        )
        .unwrap();
        assert_eq!(denied["permissions"], json!({}));
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn claude_poll_uses_wall_clock_and_coalesces_wake_and_manual_requests() {
        assert!(!oauth_poll_due(false, 60_000, 59_999, false));
        assert!(oauth_poll_due(false, 60_000, 3_600_000, false));
        assert!(oauth_poll_due(false, 60_000, 1_000, true));
        assert!(!oauth_poll_due(true, 0, 3_600_000, true));
        assert!(!oauth_poll_due(true, 0, 3_600_000, false));
    }

    #[test]
    fn manual_refresh_joins_background_job_and_returns_its_actual_outcome() {
        let root = std::env::temp_dir().join(format!("actrealm-quota-join-{}", Uuid::now_v7()));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let mut state = test_state(store.clone(), &root);
        state.claude_oauth_quota = true;
        {
            let mut auth = state.auth.lock().unwrap();
            auth.session_token = Some("test-session".into());
            auth.csrf_token = Some("test-csrf".into());
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            for outcome in [
                Ok(42_000),
                Err(("CLAUDE_SIGN_IN_REQUIRED", "Sign in once".to_owned())),
            ] {
                {
                    let mut quota = state.quota.lock().unwrap();
                    quota.oauth_refresh_in_progress = true;
                    quota.oauth_last_result = None;
                }
                let completed = state.quota.clone();
                let success = outcome.is_ok();
                let worker = tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    let mut quota = completed.lock().unwrap();
                    quota.oauth_last_result = Some(outcome);
                    quota.oauth_refresh_in_progress = false;
                });
                let response = router(state.clone())
                    .oneshot(authorized_request(
                        "POST",
                        "/api/v1/quota/refresh-now",
                        Value::Null,
                    ))
                    .await
                    .unwrap();
                assert_eq!(
                    response.status(),
                    if success {
                        StatusCode::OK
                    } else {
                        StatusCode::SERVICE_UNAVAILABLE
                    }
                );
                let body = json_body(response).await;
                if success {
                    assert_eq!(body["completed"], true);
                    assert_eq!(body["claudeCapturedAt"], 42_000);
                } else {
                    assert_eq!(body["error"]["code"], "CLAUDE_SIGN_IN_REQUIRED");
                }
                worker.await.unwrap();
                // A joined request never reserves another network/CLI attempt.
                assert_eq!(state.quota.lock().unwrap().oauth_next_poll_at, 0);
            }
        });
        drop(state);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manual_quota_refresh_errors_are_actionable_without_exposing_credentials() {
        assert_eq!(
            quota_refresh_error_detail(&QuotaError::OAuthUnavailable),
            "Claude Code is not signed in or its credentials are not readable. Sign in to Claude Code once, then refresh. No conversation is needed."
        );
        assert_eq!(
            quota_refresh_error_detail(&QuotaError::OAuthRequest(
                "credential was rejected".to_owned()
            )),
            "Claude rejected the credential after automatic renewal. Check the Claude Code sign-in state and sign in again if required."
        );
        assert_eq!(
            quota_refresh_error_detail(&QuotaError::OAuthRequest(
                "temporarily rate limited".to_owned()
            )),
            "The Claude quota endpoint is temporarily rate limited. Try again later."
        );
    }

    #[test]
    fn quota_only_codex_refresh_works_without_persistent_connector_or_usage_scan() {
        let root = std::env::temp_dir().join(format!(
            "actrealm-server-codex-quota-only-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("fake-codex");
        fs::write(
            &executable,
            r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"id":1,"result":{"userAgent":"codex_cli_rs/0.144.6"}}'
      ;;
    *'"method":"account/rateLimits/read"'*)
      printf '%s\n' '{"id":2,"result":{"rateLimits":{"limitId":"codex","planType":"pro","primary":{"usedPercent":26,"windowDurationMins":10080,"resetsAt":1788757309}},"rateLimitsByLimitId":{"codex":{"limitId":"codex","planType":"pro","primary":{"usedPercent":26,"windowDurationMins":10080,"resetsAt":1788757309}},"codex_bengalfox":{"limitId":"codex_bengalfox","limitName":"GPT-5.3-Codex-Spark","planType":"pro","primary":{"usedPercent":0,"windowDurationMins":300,"resetsAt":1788276439}}}}}'
      ;;
  esac
done
"#,
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let manager = CodexManager {
            connector: None,
            executable: Some(executable),
            state: Arc::new(Mutex::new(CodexManagerState::default())),
            store: store.clone(),
            waiters: WaiterRegistry::default(),
            auth_path: None,
            auth_stamp: None,
        };

        assert_eq!(
            manager.refresh_rate_limits_quota_only(),
            CodexQuotaRefresh::Refreshed
        );
        let (entries, _) = manager.rate_limit_entries().unwrap();
        assert!(entries.iter().any(|entry| {
            entry.limit_id.as_deref() == Some("codex")
                && entry.plan_type.as_deref() == Some("pro")
                && entry.used_pct == Some(26.0)
        }));
        assert!(entries.iter().any(|entry| {
            entry.limit_id.as_deref() == Some("codex_bengalfox")
                && entry.plan_type.as_deref() == Some("pro")
        }));
        drop(manager);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn auth_secrets_and_error_boundary_do_not_expose_internal_detail() {
        let first = generate_secret().unwrap();
        let second = generate_secret().unwrap();
        assert_eq!(first.len(), 64);
        assert_eq!(second.len(), 64);
        assert_ne!(first, second);
        assert!(constant_time_eq(&first, &first));
        assert!(!constant_time_eq(&first, &second));
        assert!(!constant_time_eq(&first, &first[..63]));

        let response = api_error_detail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL_FAILURE",
            "/Users/private/.claude/credentials: bearer-secret",
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let body = runtime.block_on(json_body(response));
        assert_eq!(body["error"]["code"], "INTERNAL_FAILURE");
        assert!(body["error"].get("detail").is_none());
        assert!(!body.to_string().contains("/Users/private"));
        assert!(!body.to_string().contains("bearer-secret"));
    }
}

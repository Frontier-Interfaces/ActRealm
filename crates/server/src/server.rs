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
use flow_agent_installer::{
    BinaryHealth, ClaudeStatuslineStatus, CodexTrustStatus, ConfigHealth, HookProvider,
    InstallIntent, InstallOptions, InstallPaths, Installer,
};
use flow_agent_quota::{QuotaCollector, QuotaEntry, QuotaPaths};
use flow_agent_runtime::{
    ApprovalAction, AttentionAction, CommandState, MetricEvent, QuotaRecord, RuntimeStore,
    StoreError, WaiterRegistry,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path as FilePath, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::sync::oneshot;
use uuid::Uuid;

const SESSION_COOKIE: &str = "flow_agent_session";
const CSRF_HEADER: &str = "x-flow-agent-csrf";
const SETTINGS_KEY: &str = "ui_settings";
const SESSION_LIST_RETENTION_MS: u64 = 30 * 60 * 1_000;
const SESSION_SWEEP_INTERVAL: Duration = Duration::from_secs(30);
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
    pub install_paths: Option<InstallPaths>,
}

impl Default for ApiServerConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            commit_delay: Duration::from_secs(3),
            snapshot_interval: Duration::from_millis(100),
            quota_poll_interval: Duration::from_secs(5 * 60),
            install_paths: None,
        }
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
        let data_paths = DataPaths {
            cache: install_paths.flow_home.join("cache"),
            spool: install_paths.flow_home.join("spool"),
            diagnostics: install_paths.flow_home.join("diagnostics"),
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
            installer: Arc::new(Installer::new(install_paths, source_binary)),
            quota: Arc::new(Mutex::new(QuotaState {
                collector: QuotaCollector::new(quota_paths),
                entries: Vec::new(),
                refreshed_at: None,
                claude_cache_modified_at: None,
            })),
            session_sweep: Arc::new(Mutex::new(None)),
            data_paths,
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
    installer: Arc<Installer>,
    quota: Arc<Mutex<QuotaState>>,
    session_sweep: Arc<Mutex<Option<Instant>>>,
    data_paths: DataPaths,
}

struct QuotaState {
    collector: QuotaCollector,
    entries: Vec<QuotaEntry>,
    refreshed_at: Option<Instant>,
    claude_cache_modified_at: Option<SystemTime>,
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

fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.css", get(styles))
        .route("/app.js", get(script))
        .route("/assets/claude.png", get(claude_icon))
        .route("/assets/codex.png", get(codex_icon))
        .route("/api/v1/health", get(health))
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
        .route("/api/v1/commands/{id}/undo", post(undo))
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
    if request.action != "uninstall" && !provider_cli_available(provider) {
        return api_error(StatusCode::CONFLICT, "PROVIDER_CLI_MISSING");
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
        let cli_installed = provider_cli_available(provider);
        let real_event_verified =
            inspection
                .installed_definition_changed_at_ms
                .is_some_and(|installed_at| {
                    runtime.sessions.iter().any(|session| {
                        session.provider == provider.as_str()
                            && session.last_event_at >= installed_at
                    })
                });
        let status = if !cli_installed {
            "cli_missing"
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
            Some("not_installed") | Some("cli_missing")
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

fn provider_cli_available(provider: HookProvider) -> bool {
    let executable = provider.as_str();
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|directory| {
            let candidate = directory.join(executable);
            std::fs::metadata(candidate)
                .map(|metadata| {
                    use std::os::unix::fs::PermissionsExt;
                    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
                })
                .unwrap_or(false)
        })
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UiSettings {
    notification_rules: NotificationRules,
    sound_enabled: bool,
    provider_muted: ProviderMuted,
    codex_enhanced_activity: bool,
    retention_days: u32,
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
                approval: "banner".to_owned(),
                question: "banner".to_owned(),
                error: "banner".to_owned(),
                completion: "list".to_owned(),
            },
            sound_enabled: true,
            provider_muted: ProviderMuted {
                claude: false,
                codex: false,
            },
            codex_enhanced_activity: true,
            retention_days: 90,
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
        if !matches!(self.retention_days, 30 | 90 | 365) {
            return Err("retentionDays must be 30, 90, or 365");
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
            if !provider_cli_available(HookProvider::Codex) {
                return api_error(StatusCode::CONFLICT, "PROVIDER_CLI_MISSING");
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
        && !provider_cli_available(HookProvider::Claude)
    {
        return api_error(StatusCode::CONFLICT, "PROVIDER_CLI_MISSING");
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
    if let Some(existing) = snapshot
        .commands
        .iter()
        .find(|command| command.id == request.id)
    {
        if existing.attention_id != request.attention_id
            || existing.request_id != request.request_id
            || existing.action != request.action
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
        "pass_through" => {
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
        "ack" | "snooze" => {
            if attention.kind == "approval" {
                return api_error(StatusCode::CONFLICT, "INVALID_ACTION");
            }
            let action = if request.action == "ack" {
                AttentionAction::Ack
            } else {
                AttentionAction::Snooze
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
    reconcile_sessions_if_due(state, now)?;
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
        session.last_event_at >= cutoff || visible_attention_sessions.contains(session.id.as_str())
    });
    let quota = quota_entries(state)?;
    Ok(json!({
        "sessions": snapshot.sessions,
        "attention": snapshot.attention,
        "commands": snapshot.commands,
        "quota": quota,
        "stats": {
            "eventCount": snapshot.event_count,
            "metrics": snapshot.metrics
        }
    }))
}

fn reconcile_sessions_if_due(state: &AppState, now: u64) -> Result<(), StoreError> {
    let should_sweep = {
        let mut last = state
            .session_sweep
            .lock()
            .map_err(|_| StoreError::Storage("session sweep lock is poisoned".to_owned()))?;
        if last.is_some_and(|instant| instant.elapsed() < SESSION_SWEEP_INTERVAL) {
            false
        } else {
            *last = Some(Instant::now());
            true
        }
    };
    if should_sweep {
        state
            .store
            .reconcile_session_liveness(Vec::new(), now, SESSION_LIST_RETENTION_MS)?;
    }
    Ok(())
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
    if refresh {
        quota.entries = quota.collector.collect(now_millis());
        quota.refreshed_at = Some(Instant::now());
        quota.claude_cache_modified_at = claude_cache_modified_at;
        let persisted = quota
            .entries
            .iter()
            .filter_map(|entry| {
                Some(QuotaRecord {
                    provider: entry.provider.clone(),
                    window: entry.window.clone(),
                    used_pct: entry.used_pct?,
                    resets_at: entry.resets_at?,
                    source: entry.source.clone(),
                    captured_at: entry.captured_at?,
                })
            })
            .collect();
        state.store.replace_quota_snapshots(persisted)?;
    }
    Ok(quota.entries.clone())
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
            installer: Arc::new(Installer::new(paths, std::env::current_exe().unwrap())),
            quota: Arc::new(Mutex::new(QuotaState {
                collector: QuotaCollector::new(quota_paths),
                entries: Vec::new(),
                refreshed_at: None,
                claude_cache_modified_at: None,
            })),
            session_sweep: Arc::new(Mutex::new(None)),
            data_paths: DataPaths {
                cache: root.join("flow-home/cache"),
                spool: root.join("flow-home/spool"),
                diagnostics: root.join("flow-home/diagnostics"),
            },
        }
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
        assert!(APP_JS.contains("todayWidgetDecisions"));
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
}

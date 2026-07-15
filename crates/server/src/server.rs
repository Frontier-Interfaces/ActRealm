use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, COOKIE, HOST, ORIGIN, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use flow_agent_runtime::{
    ApprovalAction, AttentionAction, CommandState, RuntimeStore, StoreError, WaiterRegistry,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::sync::oneshot;
use uuid::Uuid;

const SESSION_COOKIE: &str = "flow_agent_session";
const CSRF_HEADER: &str = "x-flow-agent-csrf";
const INDEX_HTML: &str = include_str!("../../../web/index.html");
const APP_CSS: &str = include_str!("../../../web/app.css");
const APP_JS: &str = include_str!("../../../web/app.js");

#[derive(Debug, Clone)]
pub struct ApiServerConfig {
    pub bind: SocketAddr,
    pub commit_delay: Duration,
    pub snapshot_interval: Duration,
}

impl Default for ApiServerConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            commit_delay: Duration::from_secs(3),
            snapshot_interval: Duration::from_millis(250),
        }
    }
}

#[derive(Debug, Error)]
pub enum ApiServerError {
    #[error("API listener failed: {0}")]
    Io(#[from] io::Error),
    #[error("API runtime thread failed: {0}")]
    Thread(String),
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
        .route("/api/v1/health", get(health))
        .route("/api/v1/bootstrap", post(bootstrap))
        .route("/api/v1/snapshot", get(snapshot))
        .route("/api/v1/commands", post(command))
        .route("/api/v1/commands/{id}/undo", post(undo))
        .route("/api/v1/ws", get(websocket))
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
    let snapshot = state.store.snapshot()?;
    Ok(json!({
        "sessions": snapshot.sessions,
        "attention": snapshot.attention,
        "commands": snapshot.commands,
        "quota": [],
        "stats": { "eventCount": snapshot.event_count }
    }))
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

    fn test_state(store: RuntimeStore) -> AppState {
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
            let app = router(test_state(store));
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
}

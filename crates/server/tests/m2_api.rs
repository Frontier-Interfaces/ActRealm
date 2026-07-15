#![cfg(unix)]

use flow_agent_core::{BridgeRequest, Decision, Provider};
use flow_agent_runtime::{RuntimeStore, WaiterRegistry};
use flow_agent_server::{ApiServer, ApiServerConfig};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use uuid::Uuid;

const INDEX_HTML: &str = include_str!("../../../web/index.html");
const APP_CSS: &str = include_str!("../../../web/app.css");
const APP_JS: &str = include_str!("../../../web/app.js");

struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Value,
}

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "flow-agent-m2-{name}-{}-{}",
        std::process::id(),
        Uuid::now_v7()
    ))
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn event(
    provider: Provider,
    name: &str,
    session: &str,
    turn: &str,
    command: Option<&str>,
) -> BridgeRequest {
    let mut raw = json!({
        "hook_event_name": name,
        "session_id": session,
        "cwd": "/tmp/real-project",
        "turn_id": turn,
        "prompt_id": turn
    });
    if let Some(command) = command {
        raw["tool_name"] = Value::String("Bash".to_owned());
        raw["tool_input"] = json!({ "command": command });
    }
    BridgeRequest::from_hook_at(provider, raw, now_millis())
}

fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&Value>,
) -> HttpResponse {
    let payload = body.map(Value::to_string).unwrap_or_default();
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: {}\r\n",
        payload.len()
    )
    .unwrap();
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n").unwrap();
    }
    if body.is_some() {
        write!(stream, "Content-Type: application/json\r\n").unwrap();
    }
    write!(stream, "\r\n{payload}").unwrap();
    stream.flush().unwrap();

    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    let marker = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    let head = String::from_utf8(response[..marker].to_vec()).unwrap();
    let mut lines = head.lines();
    let status = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let response_headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let bytes = &response[marker + 4..];
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(bytes).unwrap()
    };
    HttpResponse {
        status,
        headers: response_headers,
        body,
    }
}

fn authenticate(server: &ApiServer) -> (String, String) {
    let origin = server.origin();
    let response = request(
        server.address(),
        "POST",
        "/api/v1/bootstrap",
        &[("Origin", &origin)],
        Some(&json!({ "token": server.bootstrap_token() })),
    );
    assert_eq!(response.status, 200);
    let cookie = response
        .headers
        .iter()
        .find(|(name, _)| name == "set-cookie")
        .unwrap()
        .1
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let csrf = response.body["csrfToken"].as_str().unwrap().to_owned();
    (cookie, csrf)
}

fn auth_headers<'a>(origin: &'a str, cookie: &'a str, csrf: &'a str) -> [(&'a str, &'a str); 3] {
    [
        ("Origin", origin),
        ("Cookie", cookie),
        ("X-Flow-Agent-CSRF", csrf),
    ]
}

fn start(name: &str) -> (PathBuf, RuntimeStore, WaiterRegistry, ApiServer) {
    let root = temp_root(name);
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let waiters = WaiterRegistry::default();
    let server = ApiServer::start(
        store.clone(),
        waiters.clone(),
        ApiServerConfig {
            commit_delay: Duration::from_millis(60),
            snapshot_interval: Duration::from_millis(20),
            ..ApiServerConfig::default()
        },
    )
    .unwrap();
    (root, store, waiters, server)
}

fn ingest_waiting(
    store: &RuntimeStore,
    waiters: &WaiterRegistry,
    event: BridgeRequest,
) -> flow_agent_runtime::RegisterResult {
    store.ingest(event.clone()).unwrap();
    waiters.register_at(&event, now_millis()).unwrap()
}

#[test]
fn embedded_ui_contract_is_small_honest_and_complete() {
    for asset in [INDEX_HTML, APP_CSS, APP_JS] {
        assert!(
            asset.len() < 100 * 1024,
            "embedded UI asset exceeds 100 KiB"
        );
    }
    assert!(APP_CSS.contains("grid-template-columns"));
    assert!(APP_CSS.contains("min-height: 48px"));
    assert!(INDEX_HTML.contains("待处理"));
    assert!(INDEX_HTML.contains("Agent 任务"));
    assert!(INDEX_HTML.contains("额度"));
    for action in ["approve", "deny", "pass_through", "ack", "snooze"] {
        assert!(APP_JS.contains(action), "missing UI action {action}");
    }
    assert!(APP_JS.contains("undoCommand"));
    for state in [
        "pending_commit",
        "decision_sent",
        "confirmed",
        "passed_through",
        "expired",
    ] {
        assert!(APP_JS.contains(state), "missing rendered state {state}");
    }
    assert!(!APP_JS.contains("innerHTML"));
    assert!(!INDEX_HTML.contains('$'));
    assert!(APP_JS.contains("暂无可靠额度数据"));
    assert!(APP_JS.contains("不会用估算值冒充真实额度"));
}

#[test]
fn authenticated_api_controls_approval_and_preserves_three_second_undo_semantics() {
    let (root, store, waiters, server) = start("commands");
    let address = server.address();
    let origin = server.origin();

    assert_eq!(
        request(address, "GET", "/api/v1/snapshot", &[], None).status,
        401
    );
    let forged = request(
        address,
        "POST",
        "/api/v1/bootstrap",
        &[("Origin", "http://malicious.invalid")],
        Some(&json!({ "token": server.bootstrap_token() })),
    );
    assert_eq!(forged.status, 403);

    let (cookie, csrf) = authenticate(&server);
    let reused = request(
        address,
        "POST",
        "/api/v1/bootstrap",
        &[("Origin", &origin)],
        Some(&json!({ "token": server.bootstrap_token() })),
    );
    assert_eq!(reused.status, 401);
    let missing_csrf = request(
        address,
        "POST",
        "/api/v1/commands",
        &[("Origin", &origin), ("Cookie", &cookie)],
        Some(&json!({
            "id": Uuid::now_v7(),
            "attentionId": "missing",
            "requestId": null,
            "action": "ack"
        })),
    );
    assert_eq!(missing_csrf.status, 403);
    let headers = auth_headers(&origin, &cookie, &csrf);
    let permission = event(
        Provider::Claude,
        "PermissionRequest",
        "claude-session",
        "turn-1",
        Some("cargo test"),
    );
    let request_id = permission.request_id.unwrap();
    let registration = ingest_waiting(&store, &waiters, permission);
    let attention_id = store.snapshot().unwrap().attention[0].id.clone();

    let approve_id = Uuid::now_v7();
    let approve = request(
        address,
        "POST",
        "/api/v1/commands",
        &headers,
        Some(&json!({
            "id": approve_id,
            "attentionId": attention_id,
            "requestId": request_id,
            "action": "approve"
        })),
    );
    assert_eq!(approve.status, 202);
    assert_eq!(approve.body["state"], "pending_commit");
    let undo = request(
        address,
        "POST",
        &format!("/api/v1/commands/{approve_id}/undo"),
        &headers,
        None,
    );
    assert_eq!(undo.status, 200);
    assert_eq!(undo.body["state"], "undone");
    assert!(registration
        .ticket
        .recv_timeout(Duration::from_millis(100))
        .is_err());

    let deny_id = Uuid::now_v7();
    let deny = request(
        address,
        "POST",
        "/api/v1/commands",
        &headers,
        Some(&json!({
            "id": deny_id,
            "attentionId": attention_id,
            "requestId": request_id,
            "action": "deny"
        })),
    );
    assert_eq!(deny.status, 202);
    let response = registration
        .ticket
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    assert_eq!(response.decision(), Some(Decision::Deny));
    let saved = store.snapshot().unwrap();
    assert_eq!(saved.attention[0].state, "decision_sent");
    assert_eq!(saved.commands.last().unwrap().state, "decision_sent");

    let repeated = request(
        address,
        "POST",
        "/api/v1/commands",
        &headers,
        Some(&json!({
            "id": deny_id,
            "attentionId": attention_id,
            "requestId": request_id,
            "action": "deny"
        })),
    );
    assert_eq!(repeated.status, 200);
    assert_eq!(repeated.body["state"], "decision_sent");

    drop(server);
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn pass_through_ack_snooze_and_websocket_snapshot_are_real() {
    let (root, store, waiters, server) = start("snapshot");
    let origin = server.origin();
    let (cookie, csrf) = authenticate(&server);
    let headers = auth_headers(&origin, &cookie, &csrf);

    let permission = event(
        Provider::Codex,
        "PermissionRequest",
        "codex-session",
        "turn-1",
        Some("git status"),
    );
    let request_id = permission.request_id.unwrap();
    let registration = ingest_waiting(&store, &waiters, permission);
    let approval = store.snapshot().unwrap().attention[0].clone();
    let pass = request(
        server.address(),
        "POST",
        "/api/v1/commands",
        &headers,
        Some(&json!({
            "id": Uuid::now_v7(),
            "attentionId": approval.id,
            "requestId": request_id,
            "action": "pass_through"
        })),
    );
    assert_eq!(pass.status, 200);
    assert!(registration
        .ticket
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .decision()
        .is_none());

    store
        .ingest(event(
            Provider::Claude,
            "StopFailure",
            "error-session",
            "turn-error",
            None,
        ))
        .unwrap();
    store
        .ingest(event(
            Provider::Codex,
            "StopFailure",
            "snooze-session",
            "turn-snooze",
            None,
        ))
        .unwrap();
    let items = store.snapshot().unwrap().attention;
    let error = items.iter().find(|item| item.provider == "claude").unwrap();
    let snooze = items
        .iter()
        .find(|item| item.provider == "codex" && item.kind == "error")
        .unwrap();
    let ack = request(
        server.address(),
        "POST",
        "/api/v1/commands",
        &headers,
        Some(&json!({
            "id": Uuid::now_v7(),
            "attentionId": error.id,
            "requestId": null,
            "action": "ack"
        })),
    );
    assert_eq!(ack.status, 200);
    let snoozed = request(
        server.address(),
        "POST",
        "/api/v1/commands",
        &headers,
        Some(&json!({
            "id": Uuid::now_v7(),
            "attentionId": snooze.id,
            "requestId": null,
            "action": "snooze"
        })),
    );
    assert_eq!(snoozed.status, 200);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut ws_request = format!("ws://{}/api/v1/ws?csrf={csrf}", server.address())
            .into_client_request()
            .unwrap();
        ws_request
            .headers_mut()
            .insert("Origin", HeaderValue::from_str(&origin).unwrap());
        ws_request
            .headers_mut()
            .insert("Cookie", HeaderValue::from_str(&cookie).unwrap());
        let (mut websocket, response) = tokio_tungstenite::connect_async(ws_request).await.unwrap();
        assert_eq!(response.status(), 101);
        let frame = websocket.next().await.unwrap().unwrap();
        let payload: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
        assert_eq!(payload["type"], "snapshot");
        assert!(payload["snapshot"]["sessions"].as_array().unwrap().len() >= 3);
        assert_eq!(payload["snapshot"]["quota"], json!([]));
        websocket.close(None).await.unwrap();
    });

    let snapshot = store.snapshot().unwrap();
    assert!(snapshot
        .attention
        .iter()
        .any(|item| item.state == "resolved"));
    assert!(snapshot
        .attention
        .iter()
        .any(|item| item.state == "snoozed"));
    assert!(snapshot
        .attention
        .iter()
        .any(|item| item.state == "passed_through"));
    drop(server);
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

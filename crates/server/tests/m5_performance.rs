#![cfg(unix)]

use flow_agent_core::{BridgeRequest, Provider};
use flow_agent_runtime::{RuntimeStore, WaiterRegistry};
use flow_agent_server::{ApiServer, ApiServerConfig};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use uuid::Uuid;

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn authenticate(server: &ApiServer) -> (String, String) {
    let body = json!({"token":server.bootstrap_token()}).to_string();
    let mut stream = TcpStream::connect(server.address()).unwrap();
    write!(
        stream,
        "POST /api/v1/bootstrap HTTP/1.1\r\nHost: {}\r\nOrigin: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        server.address(),
        server.origin(),
        body.len(),
        body
    )
    .unwrap();
    stream.flush().unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    let cookie = headers
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("set-cookie:"))
        .unwrap()
        .split_once(':')
        .unwrap()
        .1
        .trim()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let body: Value = serde_json::from_str(body).unwrap();
    (cookie, body["csrfToken"].as_str().unwrap().to_owned())
}

fn p95(samples: &mut [Duration]) -> Duration {
    samples.sort_unstable();
    let index = ((samples.len() * 95).div_ceil(100)).saturating_sub(1);
    samples[index]
}

#[test]
fn event_to_websocket_render_entry_p95_is_below_300_ms() {
    let root = PathBuf::from("/tmp").join(format!(
        "flow-agent-m5-ui-perf-{}-{}",
        std::process::id(),
        Uuid::now_v7()
    ));
    fs::create_dir_all(&root).unwrap();
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let server = ApiServer::start(
        store.clone(),
        WaiterRegistry::default(),
        ApiServerConfig {
            install_paths: Some(flow_agent_installer::InstallPaths {
                flow_home: root.join("flow-home"),
                claude_settings: root.join("home/.claude/settings.json"),
                codex_hooks: root.join("home/.codex/hooks.json"),
                codex_config: root.join("home/.codex/config.toml"),
            }),
            ..ApiServerConfig::default()
        },
    )
    .unwrap();
    let (cookie, csrf) = authenticate(&server);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut samples = runtime.block_on(async {
        let mut request = format!("ws://{}/api/v1/ws?csrf={csrf}", server.address())
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("Origin", HeaderValue::from_str(&server.origin()).unwrap());
        request
            .headers_mut()
            .insert("Cookie", HeaderValue::from_str(&cookie).unwrap());
        let (mut websocket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let _ = websocket.next().await.unwrap().unwrap();
        let mut samples = Vec::new();
        for index in 0..20 {
            let started = Instant::now();
            store
                .ingest(BridgeRequest::from_hook_at(
                    Provider::Claude,
                    json!({
                        "hook_event_name":"SessionStart",
                        "session_id":format!("performance-{index}")
                    }),
                    now_millis(),
                ))
                .unwrap();
            let expected = index + 1;
            loop {
                let frame = tokio::time::timeout(Duration::from_secs(1), websocket.next())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                let payload: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
                if payload["snapshot"]["stats"]["eventCount"].as_u64() == Some(expected) {
                    break;
                }
            }
            samples.push(started.elapsed());
        }
        websocket.close(None).await.unwrap();
        samples
    });
    let p95 = p95(&mut samples);
    eprintln!(
        "event_to_websocket_p95_ms={:.3}",
        p95.as_secs_f64() * 1_000.0
    );
    drop(server);
    drop(store);
    fs::remove_dir_all(root).unwrap();
    assert!(
        p95 < Duration::from_millis(300),
        "event-to-websocket p95 was {p95:?}"
    );
}

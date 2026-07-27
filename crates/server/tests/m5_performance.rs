#![cfg(unix)]

use actrealm_core::{BridgeRequest, Provider};
use actrealm_runtime::{RuntimeStore, WaiterRegistry};
use actrealm_server::{ApiServer, ApiServerConfig};
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

fn websocket_ticket(server: &ApiServer, cookie: &str, csrf: &str) -> String {
    let mut stream = TcpStream::connect(server.address()).unwrap();
    write!(
        stream,
        "POST /api/v1/ws-ticket HTTP/1.1\r\nHost: {}\r\nOrigin: {}\r\nCookie: {}\r\nx-actrealm-csrf: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        server.address(),
        server.origin(),
        cookie,
        csrf,
    )
    .unwrap();
    stream.flush().unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    assert!(headers.starts_with("HTTP/1.1 200"), "{headers}");
    let body: Value = serde_json::from_str(body).unwrap();
    body["ticket"].as_str().unwrap().to_owned()
}

fn p95(samples: &mut [Duration]) -> Duration {
    samples.sort_unstable();
    let index = ((samples.len() * 95).div_ceil(100)).saturating_sub(1);
    samples[index]
}

#[test]
fn event_to_websocket_render_entry_p95_is_below_300_ms() {
    let root = PathBuf::from("/tmp").join(format!(
        "actrealm-m5-ui-perf-{}-{}",
        std::process::id(),
        Uuid::now_v7()
    ));
    fs::create_dir_all(&root).unwrap();
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let server = ApiServer::start(
        store.clone(),
        WaiterRegistry::default(),
        ApiServerConfig {
            install_paths: Some(actrealm_installer::InstallPaths {
                actrealm_home: root.join("actrealm-home"),
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
        let ticket = websocket_ticket(&server, &cookie, &csrf);
        let protocol = format!("actrealm.{ticket}");
        let mut request = format!("ws://{}/api/v1/ws", server.address())
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("Origin", HeaderValue::from_str(&server.origin()).unwrap());
        request
            .headers_mut()
            .insert("Cookie", HeaderValue::from_str(&cookie).unwrap());
        request.headers_mut().insert(
            "Sec-WebSocket-Protocol",
            HeaderValue::from_str(&protocol).unwrap(),
        );
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

#[test]
fn bounded_five_thousand_session_snapshot_stays_within_server_budget() {
    let root = PathBuf::from("/tmp").join(format!(
        "actrealm-m5-bounded-snapshot-{}-{}",
        std::process::id(),
        Uuid::now_v7()
    ));
    fs::create_dir_all(&root).unwrap();
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let expired_at = now_millis().saturating_sub(31 * 60 * 1_000);
    for index in 0..5_000 {
        store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Claude,
                json!({
                    "hook_event_name":"SessionStart",
                    "session_id":format!("bounded-performance-{index}")
                }),
                expired_at,
            ))
            .unwrap();
    }
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Claude,
            json!({
                "hook_event_name":"UserPromptSubmit",
                "session_id":"bounded-performance-recent"
            }),
            now_millis(),
        ))
        .unwrap();
    let server = ApiServer::start(
        store.clone(),
        WaiterRegistry::default(),
        ApiServerConfig {
            install_paths: Some(actrealm_installer::InstallPaths {
                actrealm_home: root.join("actrealm-home"),
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
    let elapsed = runtime.block_on(async {
        let ticket = websocket_ticket(&server, &cookie, &csrf);
        let protocol = format!("actrealm.{ticket}");
        let mut request = format!("ws://{}/api/v1/ws", server.address())
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("Origin", HeaderValue::from_str(&server.origin()).unwrap());
        request
            .headers_mut()
            .insert("Cookie", HeaderValue::from_str(&cookie).unwrap());
        request.headers_mut().insert(
            "Sec-WebSocket-Protocol",
            HeaderValue::from_str(&protocol).unwrap(),
        );
        let started = Instant::now();
        let (mut websocket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let frame = tokio::time::timeout(Duration::from_secs(1), websocket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let payload: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
        assert_eq!(payload["snapshot"]["sessions"].as_array().unwrap().len(), 1);
        websocket.close(None).await.unwrap();
        started.elapsed()
    });
    eprintln!(
        "bounded_five_thousand_snapshot_ms={:.3}",
        elapsed.as_secs_f64() * 1_000.0
    );
    drop(server);
    drop(store);
    fs::remove_dir_all(root).unwrap();
    assert!(
        elapsed < Duration::from_millis(300),
        "bounded 5,000-session snapshot was {elapsed:?}"
    );
}

#[test]
fn usage_refresh_cannot_hold_websocket_snapshot_past_server_budget() {
    let root = PathBuf::from("/tmp").join(format!(
        "actrealm-m5-usage-refresh-{}-{}",
        std::process::id(),
        Uuid::now_v7()
    ));
    let sessions = root.join("home/.codex/sessions/load");
    fs::create_dir_all(&sessions).unwrap();
    let large_rollout = sessions.join("a-large-rollout.jsonl");
    fs::write(&large_rollout, b"not-json\n").unwrap();
    // Sparse 512 MiB file: below the first-read skip threshold, so a synchronous
    // snapshot refresh would still enter the bounded line reader without allocating it.
    fs::OpenOptions::new()
        .write(true)
        .open(&large_rollout)
        .unwrap()
        .set_len(512 * 1_024 * 1_024)
        .unwrap();
    let old = SystemTime::now()
        .checked_sub(Duration::from_secs(24 * 60 * 60 + 1))
        .unwrap();
    for index in 0..600 {
        let path = sessions.join(format!("b-old-{index:04}.jsonl"));
        fs::write(&path, b"{}\n").unwrap();
        fs::File::open(&path).unwrap().set_modified(old).unwrap();
    }

    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let server = ApiServer::start(
        store.clone(),
        WaiterRegistry::default(),
        ApiServerConfig {
            install_paths: Some(actrealm_installer::InstallPaths {
                actrealm_home: root.join("actrealm-home"),
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
    let snapshot_result = runtime.block_on(async {
        let ticket = websocket_ticket(&server, &cookie, &csrf);
        let protocol = format!("actrealm.{ticket}");
        let mut request = format!("ws://{}/api/v1/ws", server.address())
            .into_client_request()
            .map_err(|error| error.to_string())?;
        request.headers_mut().insert(
            "Origin",
            HeaderValue::from_str(&server.origin()).map_err(|error| error.to_string())?,
        );
        request.headers_mut().insert(
            "Cookie",
            HeaderValue::from_str(&cookie).map_err(|error| error.to_string())?,
        );
        request.headers_mut().insert(
            "Sec-WebSocket-Protocol",
            HeaderValue::from_str(&protocol).map_err(|error| error.to_string())?,
        );
        let started = Instant::now();
        let (mut websocket, _) = tokio_tungstenite::connect_async(request)
            .await
            .map_err(|error| error.to_string())?;
        let frame = tokio::time::timeout(Duration::from_millis(300), websocket.next())
            .await
            .map_err(|_| "usage refresh blocked the websocket snapshot".to_owned())?
            .ok_or_else(|| "websocket closed before snapshot".to_owned())?
            .map_err(|error| error.to_string())?;
        let payload: Value =
            serde_json::from_str(frame.to_text().map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        websocket
            .close(None)
            .await
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((payload, started.elapsed()))
    });
    let (payload, elapsed) = snapshot_result.expect("websocket snapshot result");
    assert_eq!(payload["type"], "snapshot");
    eprintln!(
        "usage_refresh_websocket_snapshot_ms={:.3}",
        elapsed.as_secs_f64() * 1_000.0
    );
    drop(server);
    drop(store);
    fs::remove_dir_all(root).unwrap();
    assert!(
        elapsed < Duration::from_millis(300),
        "usage refresh held the websocket snapshot for {elapsed:?}"
    );
}

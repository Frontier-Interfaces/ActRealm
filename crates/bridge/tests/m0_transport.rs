#![cfg(unix)]

use flow_agent_bridge::{BridgeClient, BridgeError};
use flow_agent_core::{BridgeRequest, BridgeResponse, Decision, Provider};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

fn temp_socket(name: &str) -> PathBuf {
    PathBuf::from(format!("/tmp/fa-{name}-{}.sock", std::process::id()))
}

#[test]
fn mismatched_request_id_is_rejected() {
    let path = temp_socket("mismatch");
    let listener = UnixListener::bind(&path).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut line)
            .unwrap();
        let request: BridgeRequest = serde_json::from_str(&line).unwrap();
        let wrong = BridgeResponse::decided(Uuid::now_v7(), Decision::Allow);
        assert_ne!(wrong.request_id, request.request_id.unwrap());
        serde_json::to_writer(&mut stream, &wrong).unwrap();
        stream.write_all(b"\n").unwrap();
    });

    let request = BridgeRequest::from_hook_at(
        Provider::Claude,
        serde_json::json!({
            "hook_event_name": "PermissionRequest",
            "session_id": "session"
        }),
        1,
    );
    let error = BridgeClient::new(path.clone())
        .send(&request, Duration::from_secs(1))
        .unwrap_err();
    assert!(matches!(error, BridgeError::MismatchedRequestId { .. }));
    server.join().unwrap();
    let _ = fs::remove_file(path);
}

#[test]
fn missing_runtime_fails_open_within_connection_budget() {
    let path = temp_socket("missing");
    let request = BridgeRequest::from_hook_at(
        Provider::Codex,
        serde_json::json!({
            "hook_event_name": "PermissionRequest",
            "session_id": "session",
            "turn_id": "turn"
        }),
        1,
    );
    let started = Instant::now();
    assert!(BridgeClient::new(path)
        .send(&request, Duration::from_secs(60))
        .is_err());
    assert!(started.elapsed() < Duration::from_millis(200));
}

#[test]
fn silent_runtime_honors_the_hook_owned_deadline() {
    let path = temp_socket("deadline");
    let listener = UnixListener::bind(&path).unwrap();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut line)
            .unwrap();
        thread::sleep(Duration::from_millis(250));
    });
    let request = BridgeRequest::from_hook_at(
        Provider::Codex,
        serde_json::json!({
            "hook_event_name": "PermissionRequest",
            "session_id": "session",
            "turn_id": "turn"
        }),
        1,
    );
    let started = Instant::now();
    let error = BridgeClient::new(path.clone())
        .send(&request, Duration::from_millis(40))
        .unwrap_err();
    assert!(matches!(error, BridgeError::DeadlineExceeded));
    assert!(started.elapsed() < Duration::from_millis(200));
    server.join().unwrap();
    let _ = fs::remove_file(path);
}

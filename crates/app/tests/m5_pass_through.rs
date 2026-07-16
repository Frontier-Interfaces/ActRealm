#![cfg(unix)]

use flow_agent_core::{BridgeRequest, BridgeResponse};
use serde_json::json;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

fn socket(name: &str) -> PathBuf {
    PathBuf::from("/tmp").join(format!(
        "fa-m5-pass-{name}-{}-{}.sock",
        std::process::id(),
        Uuid::now_v7()
    ))
}

fn run_hook(path: &Path, timeout_ms: u64) -> Output {
    let payload = json!({
        "hook_event_name":"PermissionRequest",
        "session_id":"pass-through-session",
        "turn_id":"pass-through-turn",
        "tool_name":"Bash",
        "tool_input":{"command":"cargo test"}
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-agent"))
        .args([
            "hook",
            "--provider",
            "codex",
            "--socket",
            path.to_str().unwrap(),
        ])
        .env("FLOW_AGENT_HOOK_REPLY_TIMEOUT_MS", timeout_ms.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    serde_json::to_writer(child.stdin.as_mut().unwrap(), &payload).unwrap();
    drop(child.stdin.take());
    child.wait_with_output().unwrap()
}

fn read_request(stream: &mut std::os::unix::net::UnixStream) -> BridgeRequest {
    let mut line = String::new();
    BufReader::new(stream.try_clone().unwrap())
        .read_line(&mut line)
        .unwrap();
    serde_json::from_str(&line).unwrap()
}

fn assert_terminal_usable(output: Output) {
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn every_transport_failure_and_explicit_handoff_leaves_provider_terminal_usable() {
    let absent = socket("absent");
    assert_terminal_usable(run_hook(&absent, 100));

    let explicit = socket("explicit");
    let listener = UnixListener::bind(&explicit).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request(&mut stream);
        serde_json::to_writer(
            &mut stream,
            &BridgeResponse::pass_through(request.request_id.unwrap(), "user"),
        )
        .unwrap();
        stream.write_all(b"\n").unwrap();
    });
    assert_terminal_usable(run_hook(&explicit, 1_000));
    server.join().unwrap();
    let _ = fs::remove_file(explicit);

    let mismatch = socket("mismatch");
    let listener = UnixListener::bind(&mismatch).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_request(&mut stream);
        serde_json::to_writer(
            &mut stream,
            &BridgeResponse::pass_through(Uuid::now_v7(), "mismatch"),
        )
        .unwrap();
        stream.write_all(b"\n").unwrap();
    });
    assert_terminal_usable(run_hook(&mismatch, 1_000));
    server.join().unwrap();
    let _ = fs::remove_file(mismatch);

    let malformed = socket("malformed");
    let listener = UnixListener::bind(&malformed).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_request(&mut stream);
        stream.write_all(b"not-json\n").unwrap();
    });
    assert_terminal_usable(run_hook(&malformed, 1_000));
    server.join().unwrap();
    let _ = fs::remove_file(malformed);

    let eof = socket("eof");
    let listener = UnixListener::bind(&eof).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_request(&mut stream);
    });
    let started = Instant::now();
    assert_terminal_usable(run_hook(&eof, 1_000));
    assert!(started.elapsed() < Duration::from_millis(250));
    server.join().unwrap();
    let _ = fs::remove_file(eof);

    let deadline = socket("deadline");
    let listener = UnixListener::bind(&deadline).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_request(&mut stream);
        thread::sleep(Duration::from_millis(150));
    });
    let started = Instant::now();
    assert_terminal_usable(run_hook(&deadline, 30));
    assert!(started.elapsed() < Duration::from_millis(120));
    server.join().unwrap();
    let _ = fs::remove_file(deadline);
}

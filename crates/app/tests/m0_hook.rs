#![cfg(unix)]

use flow_agent_core::{BridgeRequest, BridgeResponse, Decision};
use serde_json::json;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn temp_socket(name: &str) -> PathBuf {
    PathBuf::from(format!("/tmp/fa-app-{name}-{}.sock", std::process::id()))
}

fn run_hook(path: &Path, provider: &str, payload: serde_json::Value, timeout_ms: u64) -> Vec<u8> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-agent"))
        .args([
            "hook",
            "--provider",
            provider,
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
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    output.stdout
}

fn read_request(stream: &mut std::os::unix::net::UnixStream) -> BridgeRequest {
    let mut line = String::new();
    BufReader::new(stream.try_clone().unwrap())
        .read_line(&mut line)
        .unwrap();
    serde_json::from_str(&line).unwrap()
}

fn wait_for_socket(path: &Path) {
    let started = Instant::now();
    while !path.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "runtime socket did not become ready"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn codex_allow_writes_only_the_official_minimal_shape() {
    let path = temp_socket("allow");
    let listener = UnixListener::bind(&path).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request(&mut stream);
        let response = BridgeResponse::decided(request.request_id.unwrap(), Decision::Allow);
        serde_json::to_writer(&mut stream, &response).unwrap();
        stream.write_all(b"\n").unwrap();
    });

    let stdout = run_hook(
        &path,
        "codex",
        json!({
            "hook_event_name": "PermissionRequest",
            "session_id": "session",
            "turn_id": "turn",
            "tool_name": "Bash",
            "tool_input": { "command": "cargo test" }
        }),
        1_000,
    );
    server.join().unwrap();
    let output: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(
        output,
        json!({
            "hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": { "behavior": "allow" }
            }
        })
    );
    let _ = fs::remove_file(path);
}

#[test]
fn explicit_pass_through_keeps_stdout_empty() {
    let path = temp_socket("pass");
    let listener = UnixListener::bind(&path).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request(&mut stream);
        let response = BridgeResponse::pass_through(request.request_id.unwrap(), "user");
        serde_json::to_writer(&mut stream, &response).unwrap();
        stream.write_all(b"\n").unwrap();
    });

    let stdout = run_hook(
        &path,
        "claude",
        json!({
            "hook_event_name": "PermissionRequest",
            "session_id": "session",
            "tool_name": "Bash",
            "tool_input": { "command": "cargo test" }
        }),
        1_000,
    );
    server.join().unwrap();
    assert!(stdout.is_empty());
    let _ = fs::remove_file(path);
}

#[test]
fn deadline_and_socket_eof_both_fail_open() {
    for (name, hold_ms, timeout_ms) in [("deadline", 250, 40), ("eof", 0, 1_000)] {
        let path = temp_socket(name);
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_request(&mut stream);
            thread::sleep(Duration::from_millis(hold_ms));
        });
        let started = Instant::now();
        let stdout = run_hook(
            &path,
            "codex",
            json!({
                "hook_event_name": "PermissionRequest",
                "session_id": "session",
                "turn_id": "turn"
            }),
            timeout_ms,
        );
        assert!(stdout.is_empty());
        if name == "eof" {
            assert!(started.elapsed() < Duration::from_millis(200));
        }
        server.join().unwrap();
        let _ = fs::remove_file(path);
    }
}

#[test]
fn prompt_decision_can_be_undone_without_reaching_provider() {
    let path = temp_socket("undo");
    let mut runtime = Command::new(env!("CARGO_BIN_EXE_flow-agent"))
        .args([
            "serve",
            "--approval",
            "prompt",
            "--socket",
            path.to_str().unwrap(),
        ])
        .env("FLOW_AGENT_COMMIT_DELAY_MS", "250")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_for_socket(&path);

    // Propose allow, undo during the commit window, then explicitly return
    // ownership to the provider terminal. The hook must emit no old decision.
    runtime
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"y\nu\nt\n")
        .unwrap();
    let stdout = run_hook(
        &path,
        "codex",
        json!({
            "hook_event_name": "PermissionRequest",
            "session_id": "session",
            "turn_id": "turn",
            "tool_name": "Bash",
            "tool_input": { "command": "cargo test" }
        }),
        1_000,
    );

    assert!(stdout.is_empty());
    runtime.kill().unwrap();
    runtime.wait().unwrap();
    let _ = fs::remove_file(path);
}

#[test]
fn skip_hooks_exits_before_parsing_provider_input() {
    let output = Command::new(env!("CARGO_BIN_EXE_flow-agent"))
        .args([
            "hook",
            "--provider",
            "codex",
            "--socket",
            "/tmp/flow-agent-unused.sock",
        ])
        .env("FLOW_AGENT_SKIP_HOOKS", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn stdin_that_never_closes_fails_open_within_its_own_budget() {
    let path = temp_socket("stdin-open");
    let warmup = Command::new(env!("CARGO_BIN_EXE_flow-agent"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(warmup.status.success());
    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-agent"))
        .args([
            "hook",
            "--provider",
            "codex",
            "--socket",
            path.to_str().unwrap(),
        ])
        .env("FLOW_AGENT_STDIN_TIMEOUT_MS", "40")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    serde_json::to_writer(
        &mut stdin,
        &json!({
            "hook_event_name": "PermissionRequest",
            "session_id": "session",
            "turn_id": "turn"
        }),
    )
    .unwrap();
    stdin.flush().unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_millis(500),
        "stdin fail-open took {elapsed:?}"
    );
    drop(stdin);
}

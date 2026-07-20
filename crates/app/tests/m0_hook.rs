#![cfg(unix)]

use actrealm_core::{BridgeRequest, BridgeResponse, Decision, ReplyPayload};
use serde_json::json;
use std::collections::BTreeMap;
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
    run_hook_with_env(path, provider, payload, timeout_ms, &[])
}

fn run_hook_with_env(
    path: &Path,
    provider: &str,
    payload: serde_json::Value,
    timeout_ms: u64,
    environment: &[(&str, &str)],
) -> Vec<u8> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_actrealm"));
    command
        .args([
            "hook",
            "--provider",
            provider,
            "--socket",
            path.to_str().unwrap(),
        ])
        .env("ACTREALM_HOOK_REPLY_TIMEOUT_MS", timeout_ms.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in environment {
        command.env(key, value);
    }
    let mut child = command.spawn().unwrap();
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
fn codex_auto_review_is_forwarded_as_observation_without_blocking_or_directive() {
    let path = temp_socket("auto-review");
    let listener = UnixListener::bind(&path).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request(&mut stream);
        assert!(request.provider_handles_approval);
        assert!(!request.needs_reply);
        assert_eq!(request.request_id, None);
    });

    let started = Instant::now();
    let stdout = run_hook(
        &path,
        "codex",
        json!({
            "hook_event_name":"PermissionRequest",
            "session_id":"auto-review-session",
            "turn_id":"turn-1",
            "tool_name":"Bash",
            "approvals_reviewer":"auto_review",
            "permission_mode":"default"
        }),
        1_000,
    );
    server.join().unwrap();
    assert!(stdout.is_empty());
    // This is a process-level integration test, so retain enough startup
    // budget for a loaded CI host. The dedicated Hook performance gate owns
    // the sub-50 ms latency contract.
    assert!(started.elapsed() < Duration::from_secs(5));
    let _ = fs::remove_file(path);
}

#[test]
fn codex_auto_review_profile_config_is_used_when_the_hook_omits_the_reviewer() {
    let path = temp_socket("auto-review-profile");
    let listener = UnixListener::bind(&path).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request(&mut stream);
        assert!(request.provider_handles_approval);
        assert!(!request.needs_reply);
        assert_eq!(request.request_id, None);
    });
    let codex_home = std::env::temp_dir().join(format!(
        "actrealm-auto-review-config-{}",
        std::process::id()
    ));
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(
        codex_home.join("config.toml"),
        "profile = \"work\"\napprovals_reviewer = \"user\"\n\n[profiles.work]\napprovals_reviewer = \"auto_review\"\n",
    )
    .unwrap();

    let stdout = run_hook_with_env(
        &path,
        "codex",
        json!({
            "hook_event_name":"PermissionRequest",
            "session_id":"auto-review-profile-session",
            "turn_id":"turn-1",
            "tool_name":"Bash",
            "permission_mode":"default"
        }),
        1_000,
        &[("CODEX_HOME", codex_home.to_str().unwrap())],
    );
    server.join().unwrap();
    assert!(stdout.is_empty());
    let _ = fs::remove_file(path);
    fs::remove_dir_all(codex_home).unwrap();
}

#[test]
fn claude_ask_user_question_writes_updated_input_with_answers() {
    let path = temp_socket("claude-question");
    let listener = UnixListener::bind(&path).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request(&mut stream);
        let response = BridgeResponse::answered(
            request.request_id.unwrap(),
            ReplyPayload::ClaudeQuestion {
                answers: BTreeMap::from([("继续部署？".to_owned(), "继续".to_owned())]),
            },
        );
        serde_json::to_writer(&mut stream, &response).unwrap();
        stream.write_all(b"\n").unwrap();
    });

    let stdout = run_hook(
        &path,
        "claude",
        json!({
            "hook_event_name":"PreToolUse",
            "session_id":"question-session",
            "tool_name":"AskUserQuestion",
            "tool_input":{
                "questions":[{"question":"继续部署？","header":"确认","options":[],"multiSelect":false}],
                "keep":"original"
            }
        }),
        1_000,
    );
    server.join().unwrap();
    let output: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(
        output.pointer("/hookSpecificOutput/updatedInput/answers/继续部署？"),
        Some(&json!("继续"))
    );
    assert_eq!(
        output.pointer("/hookSpecificOutput/updatedInput/keep"),
        Some(&json!("original"))
    );
    let _ = fs::remove_file(path);
}

#[test]
fn claude_elicitation_writes_the_official_accept_shape() {
    let path = temp_socket("claude-elicitation");
    let listener = UnixListener::bind(&path).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request(&mut stream);
        let response = BridgeResponse::answered(
            request.request_id.unwrap(),
            ReplyPayload::ClaudeElicitation {
                action: "accept".to_owned(),
                content: Some(json!({"name":"Ada"})),
            },
        );
        serde_json::to_writer(&mut stream, &response).unwrap();
        stream.write_all(b"\n").unwrap();
    });
    let stdout = run_hook(
        &path,
        "claude",
        json!({
            "hook_event_name":"Elicitation",
            "session_id":"elicitation-session",
            "message":"Name",
            "requested_schema":{"type":"object","properties":{"name":{"type":"string"}}}
        }),
        1_000,
    );
    server.join().unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&stdout).unwrap(),
        json!({
            "hookSpecificOutput":{
                "hookEventName":"Elicitation",
                "action":"accept",
                "content":{"name":"Ada"}
            }
        })
    );
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
    let mut runtime = Command::new(env!("CARGO_BIN_EXE_actrealm"))
        .args([
            "serve",
            "--approval",
            "prompt",
            "--socket",
            path.to_str().unwrap(),
        ])
        .env("ACTREALM_COMMIT_DELAY_MS", "250")
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
    let output = Command::new(env!("CARGO_BIN_EXE_actrealm"))
        .args([
            "hook",
            "--provider",
            "codex",
            "--socket",
            "/tmp/actrealm-unused.sock",
        ])
        .env("ACTREALM_SKIP_HOOKS", "1")
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
    let warmup = Command::new(env!("CARGO_BIN_EXE_actrealm"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(warmup.status.success());
    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_actrealm"))
        .args([
            "hook",
            "--provider",
            "codex",
            "--socket",
            path.to_str().unwrap(),
        ])
        .env("ACTREALM_STDIN_TIMEOUT_MS", "40")
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

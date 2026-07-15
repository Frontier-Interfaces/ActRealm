#![cfg(unix)]

use flow_agent_runtime::RuntimeStore;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

fn temp_root(name: &str) -> PathBuf {
    PathBuf::from("/tmp").join(format!(
        "fa-m1a-{name}-{}-{}",
        std::process::id(),
        Uuid::now_v7()
    ))
}

fn warm_binary() {
    let output = Command::new(env!("CARGO_BIN_EXE_flow-agent"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
}

fn wait_until(description: &str, condition: impl Fn() -> bool) {
    let started = Instant::now();
    while !condition() {
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "timed out waiting for {description}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn write_payload(child: &mut Child, payload: &Value) {
    serde_json::to_writer(child.stdin.as_mut().unwrap(), payload).unwrap();
    drop(child.stdin.take());
}

fn spawn_hook(socket: &Path, payload: &Value) -> Child {
    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-agent"))
        .args([
            "hook",
            "--provider",
            "codex",
            "--socket",
            socket.to_str().unwrap(),
        ])
        .env("FLOW_AGENT_HOOK_REPLY_TIMEOUT_MS", "2000")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    write_payload(&mut child, payload);
    child
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn non_permission_event_spools_offline_and_replays_once_on_startup() {
    warm_binary();
    let home = temp_root("spool-replay");
    let socket = home.join("run/bridge.sock");
    let payload = json!({
        "hook_event_name": "Stop",
        "session_id": "spooled-session",
        "turn_id": "spooled-turn",
        "cwd": "/tmp/example-project"
    });
    let mut hook = Command::new(env!("CARGO_BIN_EXE_flow-agent"))
        .args(["hook", "--provider", "codex"])
        .env("FLOW_AGENT_HOME", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    write_payload(&mut hook, &payload);
    let output = hook.wait_with_output().unwrap();
    assert_success(&output);
    assert!(output.stdout.is_empty());
    wait_until("one spool file", || {
        fs::read_dir(home.join("spool"))
            .map(|entries| entries.filter_map(Result::ok).count() == 1)
            .unwrap_or(false)
    });

    let mut runtime = Command::new(env!("CARGO_BIN_EXE_flow-agent"))
        .args(["serve", "--approval", "pass-through"])
        .env("FLOW_AGENT_HOME", &home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_until("runtime socket", || socket.exists());
    wait_until("spool drain", || {
        fs::read_dir(home.join("spool"))
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false)
    });
    runtime.kill().unwrap();
    runtime.wait().unwrap();

    let store = RuntimeStore::open(home.join("data.sqlite")).unwrap();
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.event_count, 1);
    assert_eq!(snapshot.sessions[0].provider_session_id, "spooled-session");
    drop(store);
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn duplicate_permission_passes_old_waiter_and_decides_only_the_new_one() {
    warm_binary();
    let root = temp_root("duplicate");
    fs::create_dir_all(&root).unwrap();
    let socket = root.join("bridge.sock");
    let mut runtime = Command::new(env!("CARGO_BIN_EXE_flow-agent"))
        .args([
            "serve",
            "--approval",
            "allow",
            "--socket",
            socket.to_str().unwrap(),
        ])
        .env("FLOW_AGENT_COMMIT_DELAY_MS", "250")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_until("runtime socket", || socket.exists());

    let payload = json!({
        "hook_event_name": "PermissionRequest",
        "session_id": "duplicate-session",
        "turn_id": "same-turn",
        "tool_name": "Bash",
        "tool_input": { "command": "cargo test" },
        "cwd": "/tmp/example-project"
    });
    let first = spawn_hook(&socket, &payload);
    thread::sleep(Duration::from_millis(40));
    let second = spawn_hook(&socket, &payload);
    let first_output = first.wait_with_output().unwrap();
    let second_output = second.wait_with_output().unwrap();
    assert_success(&first_output);
    assert_success(&second_output);
    assert!(first_output.stdout.is_empty());
    let directive: Value = serde_json::from_slice(&second_output.stdout).unwrap();
    assert_eq!(
        directive.pointer("/hookSpecificOutput/decision/behavior"),
        Some(&Value::String("allow".to_owned()))
    );

    runtime.kill().unwrap();
    runtime.wait().unwrap();
    let store = RuntimeStore::open(socket.with_extension("sqlite")).unwrap();
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.attention.len(), 2);
    assert_eq!(
        snapshot
            .attention
            .iter()
            .filter(|item| item.state == "expired")
            .count(),
        1
    );
    assert_eq!(
        snapshot
            .attention
            .iter()
            .filter(|item| item.state == "decision_sent")
            .count(),
        1
    );
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn second_runtime_cannot_replace_the_live_instance_or_its_socket() {
    warm_binary();
    let root = temp_root("single-instance");
    fs::create_dir_all(&root).unwrap();
    let socket = root.join("bridge.sock");
    let mut first = Command::new(env!("CARGO_BIN_EXE_flow-agent"))
        .args([
            "serve",
            "--approval",
            "pass-through",
            "--socket",
            socket.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_until("first runtime socket", || socket.exists());

    let second = Command::new(env!("CARGO_BIN_EXE_flow-agent"))
        .args([
            "serve",
            "--approval",
            "allow",
            "--socket",
            socket.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("already owns"));
    assert!(socket.exists());

    let payload = json!({
        "hook_event_name": "PermissionRequest",
        "session_id": "single-instance-session",
        "turn_id": "turn-1",
        "tool_name": "Bash",
        "tool_input": { "command": "cargo test" },
        "cwd": "/tmp/example-project"
    });
    let output = spawn_hook(&socket, &payload).wait_with_output().unwrap();
    assert_success(&output);
    assert!(output.stdout.is_empty());

    first.kill().unwrap();
    first.wait().unwrap();
    fs::remove_dir_all(root).unwrap();
}

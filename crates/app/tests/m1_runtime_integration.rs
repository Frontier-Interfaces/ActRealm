#![cfg(unix)]

use actrealm_runtime::RuntimeStore;
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
    let output = Command::new(env!("CARGO_BIN_EXE_actrealm"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
}

fn wait_until(description: &str, condition: impl Fn() -> bool) {
    // A freshly linked test binary can take several seconds to launch its
    // child Runtime on a busy CI or developer machine. Lifecycle assertions
    // still use short polling; only the outer startup budget is relaxed.
    wait_until_for(description, Duration::from_secs(10), condition);
}

fn wait_until_for(description: &str, timeout: Duration, condition: impl Fn() -> bool) {
    let started = Instant::now();
    while !condition() {
        assert!(
            started.elapsed() < timeout,
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
    spawn_provider_hook(socket, "codex", payload)
}

fn spawn_provider_hook(socket: &Path, provider: &str, payload: &Value) -> Child {
    spawn_provider_hook_with_timeout(socket, provider, payload, 2_000)
}

fn spawn_provider_hook_with_timeout(
    socket: &Path,
    provider: &str,
    payload: &Value,
    timeout_ms: u64,
) -> Child {
    let mut child = Command::new(env!("CARGO_BIN_EXE_actrealm"))
        .args([
            "hook",
            "--provider",
            provider,
            "--socket",
            socket.to_str().unwrap(),
        ])
        .env("ACTREALM_HOOK_REPLY_TIMEOUT_MS", timeout_ms.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    write_payload(&mut child, payload);
    child
}

fn session_attention_state(database: &Path, provider_session_id: &str) -> Option<String> {
    let store = RuntimeStore::open(database).ok()?;
    let snapshot = store.snapshot().ok()?;
    let session = snapshot
        .sessions
        .iter()
        .find(|session| session.provider_session_id == provider_session_id)?;
    snapshot
        .attention
        .iter()
        .find(|item| item.session_id == session.id)
        .map(|item| item.state.clone())
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
    let mut hook = Command::new(env!("CARGO_BIN_EXE_actrealm"))
        .args(["hook", "--provider", "codex"])
        .env("ACTREALM_HOME", &home)
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

    let mut runtime = Command::new(env!("CARGO_BIN_EXE_actrealm"))
        .args(["serve", "--approval", "pass-through"])
        .env("ACTREALM_HOME", &home)
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
    let mut runtime = Command::new(env!("CARGO_BIN_EXE_actrealm"))
        .args([
            "serve",
            "--approval",
            "allow",
            "--socket",
            socket.to_str().unwrap(),
        ])
        .env("ACTREALM_COMMIT_DELAY_MS", "250")
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
    let mut first = Command::new(env!("CARGO_BIN_EXE_actrealm"))
        .args([
            "serve",
            "--approval",
            "pass-through",
            "--socket",
            socket.to_str().unwrap(),
        ])
        .env("ACTREALM_HOME", &root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_until("first runtime socket", || socket.exists());

    let second = Command::new(env!("CARGO_BIN_EXE_actrealm"))
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

#[test]
fn provider_side_tool_progress_releases_widget_waiters_for_claude_and_codex() {
    warm_binary();
    let root = temp_root("provider-resolution");
    fs::create_dir_all(&root).unwrap();
    let socket = root.join("bridge.sock");
    let database = socket.with_extension("sqlite");
    let mut runtime = Command::new(env!("CARGO_BIN_EXE_actrealm"))
        .args([
            "serve",
            "--approval",
            "widget",
            "--socket",
            socket.to_str().unwrap(),
        ])
        .env("ACTREALM_HOME", &root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_until("runtime socket", || socket.exists());

    for provider in ["claude", "codex"] {
        let session = format!("{provider}-provider-resolution");
        let permission = json!({
            "hook_event_name":"PermissionRequest",
            "session_id":session,
            "turn_id":"turn-1",
            "tool_name":"Bash",
            "tool_input":{"command":"cargo test"},
            "cwd":"/tmp/example-project"
        });
        let waiting = spawn_provider_hook_with_timeout(&socket, provider, &permission, 10_000);
        wait_until_for("open provider approval", Duration::from_secs(10), || {
            session_attention_state(&database, &session).as_deref() == Some("open")
        });

        let progress = json!({
            "hook_event_name":"PostToolUse",
            "session_id":session,
            "turn_id":"turn-1",
            "tool_name":"Bash",
            "tool_input":{"command":"cargo test"},
            "cwd":"/tmp/example-project"
        });
        let progress_output = spawn_provider_hook(&socket, provider, &progress)
            .wait_with_output()
            .unwrap();
        assert_success(&progress_output);
        assert!(progress_output.stdout.is_empty());

        let waiting_output = waiting.wait_with_output().unwrap();
        assert_success(&waiting_output);
        assert!(waiting_output.stdout.is_empty());
        wait_until("resolved provider approval", || {
            session_attention_state(&database, &session).as_deref() == Some("resolved")
        });
    }

    runtime.kill().unwrap();
    runtime.wait().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_native_permission_hook_lifecycle_is_observed_neutrally_for_five_rounds() {
    warm_binary();
    let root = temp_root("codex-native-permission");
    fs::create_dir_all(&root).unwrap();
    let socket = root.join("bridge.sock");
    let database = socket.with_extension("sqlite");
    let mut runtime = Command::new(env!("CARGO_BIN_EXE_actrealm"))
        .args([
            "serve",
            "--approval",
            "widget",
            "--socket",
            socket.to_str().unwrap(),
        ])
        .env("ACTREALM_HOME", &root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_until("runtime socket", || socket.exists());

    let provider_session_id = "codex-desktop-native-permission";
    for round in 1..=5 {
        let tool_use_id = format!("request-permissions-{round}");
        let started = json!({
            "hook_event_name":"PreToolUse",
            "session_id":provider_session_id,
            "turn_id":"desktop-turn",
            "cwd":"/tmp/example-project",
            "permission_mode":"default",
            "tool_name":"request_permissions",
            "tool_use_id":tool_use_id,
            "tool_input":{
                "permissions":{"network":{"enabled":true}},
                "reason":format!("第 {round} 轮：请求终端访问网络。")
            }
        });
        let output = spawn_hook(&socket, &started).wait_with_output().unwrap();
        assert_success(&output);
        assert!(
            output.stdout.is_empty(),
            "native observation must never reply"
        );

        wait_until("native permission attention", || {
            let Ok(store) = RuntimeStore::open(&database) else {
                return false;
            };
            let Ok(snapshot) = store.snapshot() else {
                return false;
            };
            let Some(session) = snapshot
                .sessions
                .iter()
                .find(|session| session.provider_session_id == provider_session_id)
            else {
                return false;
            };
            session.exec_state == "awaiting_approval"
                && session.approval_owner.as_deref() == Some("terminal")
                && snapshot.attention.iter().any(|item| {
                    item.session_id == session.id
                        && item.kind == "native_approval"
                        && item.state == "open"
                        && item.request_id.is_none()
                })
        });

        // Like Open Island's reducer, an incidental running update must not
        // overwrite a live actionable request.
        let activity = json!({
            "hook_event_name":"Notification",
            "session_id":provider_session_id,
            "turn_id":"desktop-turn",
            "message":"still waiting in Codex"
        });
        let output = spawn_hook(&socket, &activity).wait_with_output().unwrap();
        assert_success(&output);
        assert!(output.stdout.is_empty());
        let store = RuntimeStore::open(&database).unwrap();
        let snapshot = store.snapshot().unwrap();
        let session = snapshot
            .sessions
            .iter()
            .find(|session| session.provider_session_id == provider_session_id)
            .unwrap();
        assert_eq!(session.exec_state, "awaiting_approval");
        assert_eq!(session.approval_owner.as_deref(), Some("terminal"));
        drop(store);

        let handled = json!({
            "hook_event_name":"PostToolUse",
            "session_id":provider_session_id,
            "turn_id":"desktop-turn",
            "cwd":"/tmp/example-project",
            "tool_name":"request_permissions",
            "tool_use_id":tool_use_id,
            "tool_response":{"status":"handled"}
        });
        let output = spawn_hook(&socket, &handled).wait_with_output().unwrap();
        assert_success(&output);
        assert!(output.stdout.is_empty());

        let stopped = json!({
            "hook_event_name":"Stop",
            "session_id":provider_session_id,
            "turn_id":"desktop-turn",
            "cwd":"/tmp/example-project"
        });
        let output = spawn_hook(&socket, &stopped).wait_with_output().unwrap();
        assert_success(&output);
        assert!(output.stdout.is_empty());

        // Codex Desktop can emit both events above while its native sheet is
        // still open. The end-to-end transport must preserve the request just
        // like the reducer-level regression test and packaged UI acceptance.
        wait_until(
            "native permission preserved after PostToolUse and Stop",
            || {
                let Ok(store) = RuntimeStore::open(&database) else {
                    return false;
                };
                let Ok(snapshot) = store.snapshot() else {
                    return false;
                };
                let Some(session) = snapshot
                    .sessions
                    .iter()
                    .find(|session| session.provider_session_id == provider_session_id)
                else {
                    return false;
                };
                session.exec_state == "awaiting_approval"
                    && session.approval_owner.as_deref() == Some("terminal")
                    && snapshot.attention.iter().any(|item| {
                        item.session_id == session.id
                            && item.kind == "native_approval"
                            && item.state == "open"
                    })
            },
        );

        // A later, different real tool is authoritative evidence that Codex
        // left the native permission sheet and continued the Turn.
        let continued = json!({
            "hook_event_name":"PreToolUse",
            "session_id":provider_session_id,
            "turn_id":"desktop-turn",
            "cwd":"/tmp/example-project",
            "tool_name":"Bash",
            "tool_use_id":format!("continued-tool-{round}"),
            "tool_input":{"command":"printf local-regression"}
        });
        let output = spawn_hook(&socket, &continued).wait_with_output().unwrap();
        assert_success(&output);
        assert!(output.stdout.is_empty());

        wait_until("native permission authoritative resolution", || {
            let Ok(store) = RuntimeStore::open(&database) else {
                return false;
            };
            let Ok(snapshot) = store.snapshot() else {
                return false;
            };
            let Some(session) = snapshot
                .sessions
                .iter()
                .find(|session| session.provider_session_id == provider_session_id)
            else {
                return false;
            };
            session.approval_owner.is_none()
                && !snapshot.attention.iter().any(|item| {
                    item.session_id == session.id
                        && item.kind == "native_approval"
                        && matches!(item.state.as_str(), "open" | "snoozed")
                })
                && snapshot.attention.iter().any(|item| {
                    item.session_id == session.id
                        && item.kind == "native_approval"
                        && item.state == "resolved"
                        && item.resolution.as_deref() == Some("provider_advanced")
                })
        });
    }

    runtime.kill().unwrap();
    runtime.wait().unwrap();
    let store = RuntimeStore::open(&database).unwrap();
    let snapshot = store.snapshot().unwrap();
    assert_eq!(
        snapshot
            .attention
            .iter()
            .filter(|item| item.kind == "native_approval")
            .count(),
        5
    );
    assert!(snapshot
        .attention
        .iter()
        .filter(|item| item.kind == "native_approval")
        .all(|item| item.state == "resolved"
            && item.resolution.as_deref() == Some("provider_advanced")));
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

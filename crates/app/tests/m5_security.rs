#![cfg(unix)]

use flow_agent_core::MAX_HOOK_PAYLOAD_BYTES;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

struct Root(PathBuf);

impl Root {
    fn new(name: &str) -> Self {
        let path = PathBuf::from("/tmp").join(format!(
            "flow-agent-m5-security-{name}-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_flow-agent"));
        command
            .env("HOME", self.0.join("home"))
            .env("FLOW_AGENT_HOME", self.0.join("flow-home"))
            .env("CODEX_HOME", self.0.join("codex-home"));
        command
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn wait_for(path: &Path) {
    let started = Instant::now();
    while !path.exists() {
        assert!(started.elapsed() < Duration::from_secs(3));
        thread::sleep(Duration::from_millis(10));
    }
}

fn raw_hook(root: &Root, socket: &Path, bytes: &[u8]) -> Output {
    let mut child = root
        .command()
        .args([
            "hook",
            "--provider",
            "claude",
            "--socket",
            socket.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn diagnostic_cli_requires_explicit_bounded_enable_and_can_clear_everything() {
    let root = Root::new("diagnostics-cli");
    let status = root
        .command()
        .args(["diagnostics", "status"])
        .output()
        .unwrap();
    assert!(status.status.success());
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["enabled"], false);
    assert!(!root.0.join("flow-home/diagnostics").exists());

    let enabled = root
        .command()
        .args(["diagnostics", "enable", "--minutes", "1"])
        .output()
        .unwrap();
    assert!(enabled.status.success());
    let enabled: Value = serde_json::from_slice(&enabled.stdout).unwrap();
    assert_eq!(enabled["enabled"], true);
    let config = root.0.join("flow-home/diagnostics/config.json");
    assert_eq!(
        fs::metadata(config).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let cleared = root
        .command()
        .args(["diagnostics", "clear"])
        .output()
        .unwrap();
    assert!(cleared.status.success());
    assert!(!root.0.join("flow-home/diagnostics").exists());
}

#[test]
fn metrics_export_command_contains_only_aggregate_daily_counts() {
    let root = Root::new("metrics-export");
    let output = root.command().arg("export-metrics").output().unwrap();
    assert!(output.status.success());
    let export: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(export["scope"], "metrics_only");
    assert!(export["metricsDaily"].is_array());
    assert!(export.get("tables").is_none());
}

#[test]
fn oversized_and_deep_hook_json_fail_open_without_spooling_or_output() {
    let root = Root::new("hostile-json");
    let socket = root.0.join("absent.sock");
    let oversized = vec![b'{'; MAX_HOOK_PAYLOAD_BYTES + 1];
    let oversized = raw_hook(&root, &socket, &oversized);
    assert!(oversized.status.success());
    assert!(oversized.stdout.is_empty());
    assert!(oversized.stderr.is_empty());

    let mut deep =
        String::from(r#"{"hook_event_name":"SessionStart","session_id":"private","nested":"#);
    deep.push_str(&"[".repeat(200));
    deep.push('0');
    deep.push_str(&"]".repeat(200));
    deep.push('}');
    let deep = raw_hook(&root, &socket, deep.as_bytes());
    assert!(deep.status.success());
    assert!(deep.stdout.is_empty());
    assert!(deep.stderr.is_empty());
    assert!(!root.0.join("flow-home/spool").exists());
}

#[test]
fn default_runtime_output_never_contains_hook_session_path_prompt_or_command() {
    let root = Root::new("default-log");
    let socket = root.0.join("bridge.sock");
    let mut runtime = root
        .command()
        .args([
            "serve",
            "--approval",
            "pass-through",
            "--socket",
            socket.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_for(&socket);
    let payload = br#"{
      "hook_event_name":"SessionStart",
      "session_id":"do-not-log-session",
      "cwd":"/private/do-not-log-path",
      "prompt":"do not log this prompt",
      "tool_input":{"command":"echo do-not-log-command"}
    }"#;
    let hook = raw_hook(&root, &socket, payload);
    assert!(hook.status.success());
    thread::sleep(Duration::from_millis(80));
    runtime.kill().unwrap();
    let output = runtime.wait_with_output().unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for secret in [
        "do-not-log-session",
        "/private/do-not-log-path",
        "do not log this prompt",
        "do-not-log-command",
    ] {
        assert!(!combined.contains(secret), "default output leaked {secret}");
    }
}

#![cfg(unix)]

use actrealm_core::{BridgeRequest, Provider};
use actrealm_runtime::{DiagnosticCapture, MAX_DIAGNOSTIC_CAPTURE_BYTES};
use serde_json::json;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::PathBuf;
use uuid::Uuid;

struct Root(PathBuf);

impl Root {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "actrealm-m5-diagnostics-{name}-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn private_request(now: u64) -> BridgeRequest {
    BridgeRequest::from_hook_at(
        Provider::Claude,
        json!({
            "hook_event_name":"PermissionRequest",
            "session_id":"secret-session-id",
            "cwd":"/private/customer/project",
            "transcript_path":"/private/transcript.jsonl",
            "prompt":"do not persist this prompt",
            "tool_name":"Bash",
            "tool_input":{"command":"curl -H 'Authorization: Bearer sk-ant-private' https://user:pass@example.test"}
        }),
        now,
    )
}

#[test]
fn capture_is_explicit_sanitized_private_bounded_and_expires() {
    let root = Root::new("lifecycle");
    let capture = DiagnosticCapture::new(root.0.join("diagnostics"));
    let now = 1_784_130_000_000;

    let status = capture.status(now).unwrap();
    assert!(!status.enabled);
    assert!(!root.0.join("diagnostics").exists());
    assert!(capture.enable(0, now).is_err());
    assert!(capture.enable(61, now).is_err());

    let enabled = capture.enable(5, now).unwrap();
    assert!(enabled.enabled);
    assert_eq!(enabled.expires_at, Some(now + 5 * 60_000));
    assert_eq!(
        fs::metadata(root.0.join("diagnostics"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );

    let request = private_request(now + 1);
    capture.capture(&request, now + 1).unwrap();
    let events_path = root.0.join("diagnostics/events.jsonl");
    let saved = fs::read_to_string(&events_path).unwrap();
    assert!(saved.contains("PermissionRequest"));
    assert!(saved.contains("claude"));
    for secret in [
        "secret-session-id",
        "/private/customer/project",
        "/private/transcript.jsonl",
        "do not persist this prompt",
        "sk-ant-private",
        "user:pass",
        "curl",
    ] {
        assert!(!saved.contains(secret), "diagnostic leaked {secret}");
    }
    assert_eq!(
        fs::metadata(&events_path).unwrap().permissions().mode() & 0o777,
        0o600
    );

    for offset in 2..8_000 {
        capture.capture(&request, now + offset).unwrap();
    }
    assert!(fs::metadata(&events_path).unwrap().len() <= MAX_DIAGNOSTIC_CAPTURE_BYTES);

    let expired = capture.status(now + 5 * 60_000 + 1).unwrap();
    assert!(!expired.enabled);
    assert!(!events_path.exists());
}

#[test]
fn capture_refuses_a_symbolic_link_without_touching_its_target() {
    let root = Root::new("symlink");
    let capture = DiagnosticCapture::new(root.0.join("diagnostics"));
    let now = 1_784_130_000_000;
    capture.enable(5, now).unwrap();
    let target = root.0.join("target.txt");
    fs::write(&target, b"keep-me").unwrap();
    symlink(&target, root.0.join("diagnostics/events.jsonl")).unwrap();

    assert!(capture.capture(&private_request(now + 1), now + 1).is_err());
    assert!(capture.clear().is_err());
    assert_eq!(fs::read(&target).unwrap(), b"keep-me");
}

#[test]
fn capture_refuses_a_symbolic_link_root_without_reading_its_target() {
    let root = Root::new("root-symlink");
    let target = root.0.join("outside");
    fs::create_dir(&target).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(
        target.join("config.json"),
        br#"{"schemaVersion":1,"expiresAt":9999999999999}"#,
    )
    .unwrap();
    symlink(&target, root.0.join("diagnostics")).unwrap();
    let capture = DiagnosticCapture::new(root.0.join("diagnostics"));

    assert!(capture.status(1_784_130_000_000).is_err());
    assert!(capture
        .capture(&private_request(1_784_130_000_001), 1_784_130_000_001)
        .is_err());
    assert!(capture.clear().is_err());
    assert!(target.join("config.json").exists());
}

use actrealm_core::{BridgeRequest, Provider};
use actrealm_runtime::RuntimeStore;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

#[test]
fn persisted_command_previews_are_categories_not_truncated_raw_commands() {
    let root = PathBuf::from("/tmp").join(format!(
        "actrealm-m5-redaction-{}-{}",
        std::process::id(),
        Uuid::now_v7()
    ));
    fs::create_dir_all(&root).unwrap();
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let commands = [
        "git status",
        "git push --token super-secret origin main",
        "curl -H 'Authorization: Bearer sk-ant-private' https://user:pass@example.test?a=b",
        "OPENAI_API_KEY=sk-private npm test",
        "/usr/bin/python /private/customer/script.py --password hunter2",
    ];
    for (index, command) in commands.iter().enumerate() {
        store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Claude,
                json!({
                    "hook_event_name":"PermissionRequest",
                    "session_id":format!("redaction-{index}"),
                    "tool_name":"Bash",
                    "tool_input":{"command":command}
                }),
                1_000 + index as u64,
            ))
            .unwrap();
    }
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Claude,
            json!({
                "hook_event_name":"StopFailure",
                "session_id":"redacted-error",
                "error":"sk-ant-error-detail must not persist"
            }),
            2_000,
        ))
        .unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Claude,
            json!({
                "hook_event_name":"Unknown-sk-ant-event",
                "session_id":"redacted-unknown",
                "tool_name":"sk-ant-tool-name"
            }),
            2_001,
        ))
        .unwrap();
    let snapshot = store.snapshot().unwrap();
    let encoded = serde_json::to_string(&snapshot).unwrap();
    for secret in [
        "super-secret",
        "sk-ant-private",
        "user:pass",
        "sk-private",
        "/private/customer/script.py",
        "hunter2",
        "Authorization",
        "sk-ant-error-detail",
        "sk-ant-event",
        "sk-ant-tool-name",
    ] {
        assert!(!encoded.contains(secret), "snapshot leaked {secret}");
    }
    for preview in snapshot
        .attention
        .iter()
        .filter_map(|item| item.command_preview.as_deref())
    {
        assert!(preview.len() <= 80);
    }
    assert!(snapshot
        .attention
        .iter()
        .any(|item| item.command_preview.as_deref() == Some("git status")));
    assert!(snapshot
        .attention
        .iter()
        .filter_map(|item| item.command_preview.as_deref())
        .any(|preview| preview.contains("<redacted>")));
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

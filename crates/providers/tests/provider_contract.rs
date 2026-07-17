use flow_agent_core::{EventKind, Provider};
use flow_agent_providers::parse_hook;
use serde_json::json;

fn fixture(contents: &str) -> serde_json::Value {
    serde_json::from_str(contents).unwrap()
}

#[test]
fn claude_permission_request_does_not_require_tool_use_id() {
    let event = parse_hook(
        Provider::Claude,
        json!({
            "hook_event_name": "PermissionRequest",
            "session_id": "claude-session",
            "cwd": "/tmp/project",
            "tool_name": "Bash",
            "tool_input": { "command": "cargo test" },
            "future_field": { "is": "ignored" }
        }),
    )
    .unwrap();

    assert_eq!(event.kind, EventKind::PermissionRequested);
    assert_eq!(event.provider_session_id, "claude-session");
    assert_eq!(event.provider_turn_id, None);
    assert_eq!(event.tool_name.as_deref(), Some("Bash"));
}

#[test]
fn codex_permission_request_preserves_turn_id() {
    let event = parse_hook(
        Provider::Codex,
        json!({
            "hook_event_name": "PermissionRequest",
            "session_id": "codex-session",
            "turn_id": "turn-7",
            "cwd": "/tmp/project",
            "tool_name": "Bash",
            "tool_input": { "command": "git push" }
        }),
    )
    .unwrap();

    assert_eq!(event.kind, EventKind::PermissionRequested);
    assert_eq!(event.provider_turn_id.as_deref(), Some("turn-7"));
}

#[test]
fn codex_request_permissions_tool_is_an_observed_native_request() {
    let event = parse_hook(
        Provider::Codex,
        json!({
            "hook_event_name": "PreToolUse",
            "session_id": "codex-desktop-session",
            "turn_id": "turn-native",
            "tool_name": "request_permissions",
            "tool_use_id": "call-native",
            "tool_input": {
                "permissions": {"network": {"enabled": true}},
                "reason": "Allow terminal commands to access the network."
            }
        }),
    )
    .unwrap();

    assert_eq!(event.kind, EventKind::PermissionRequested);
    assert_eq!(event.tool_name.as_deref(), Some("request_permissions"));
    assert_eq!(event.provider_turn_id.as_deref(), Some("turn-native"));
}

#[test]
fn supported_lifecycle_events_map_to_normalized_kinds() {
    let cases = [
        ("SessionStart", EventKind::SessionStarted),
        ("UserPromptSubmit", EventKind::PromptSubmitted),
        ("PreToolUse", EventKind::ToolStarted),
        ("PostToolUse", EventKind::ToolFinished),
        ("PermissionRequest", EventKind::PermissionRequested),
        ("PermissionDenied", EventKind::PermissionDenied),
        ("TaskCreated", EventKind::TaskCreated),
        ("TaskCompleted", EventKind::TaskCompleted),
        ("Stop", EventKind::Stopped),
    ];

    for (name, expected) in cases {
        let event = parse_hook(
            Provider::Codex,
            json!({
                "hook_event_name": name,
                "session_id": "session",
                "turn_id": "turn"
            }),
        )
        .unwrap();
        assert_eq!(event.kind, expected);
    }
}

#[test]
fn unknown_event_is_visible_and_missing_session_is_an_error() {
    let unknown = parse_hook(
        Provider::Claude,
        json!({
            "hook_event_name": "FutureHookEvent",
            "session_id": "session"
        }),
    )
    .unwrap();
    assert_eq!(unknown.kind, EventKind::Unknown);

    assert!(parse_hook(
        Provider::Claude,
        json!({ "hook_event_name": "SessionStart" })
    )
    .is_err());
}

#[test]
fn versioned_fixture_sets_match_provider_contracts() {
    let claude_permission = fixture(include_str!(
        "../../../fixtures/claude/2.1.210/permission-request.json"
    ));
    let codex_permission = fixture(include_str!(
        "../../../fixtures/codex/0.144.4/permission-request.json"
    ));
    assert!(claude_permission.get("tool_use_id").is_none());
    assert!(claude_permission.get("turn_id").is_none());
    assert!(codex_permission.get("tool_use_id").is_none());
    assert_eq!(
        codex_permission
            .get("turn_id")
            .and_then(|value| value.as_str()),
        Some("fixture-codex-turn")
    );

    for (provider, raw) in [
        (Provider::Claude, claude_permission),
        (Provider::Codex, codex_permission),
    ] {
        assert_eq!(
            parse_hook(provider, raw).unwrap().kind,
            EventKind::PermissionRequested
        );
    }

    for fixture in [
        include_str!("../../../fixtures/claude/2.1.210/task-created.json"),
        include_str!("../../../fixtures/claude/2.1.210/task-completed.json"),
    ] {
        let raw = serde_json::from_str::<serde_json::Value>(fixture).unwrap();
        assert!(raw
            .get("task_id")
            .and_then(|value| value.as_str())
            .is_some());
        assert!(matches!(
            parse_hook(Provider::Claude, raw).unwrap().kind,
            EventKind::TaskCreated | EventKind::TaskCompleted
        ));
    }
}

#[test]
fn m0_fixture_sets_must_be_confirmed_by_live_probes() {
    let claude = fixture(include_str!(
        "../../../fixtures/claude/2.1.210/fixture-set.json"
    ));
    let codex = fixture(include_str!(
        "../../../fixtures/codex/0.144.4/fixture-set.json"
    ));
    assert_eq!(claude["captureStatus"], "live_probe_confirmed");
    assert_eq!(codex["captureStatus"], "live_probe_confirmed");
}

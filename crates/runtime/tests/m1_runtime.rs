#![cfg(unix)]

use actrealm_core::{BridgeRequest, Decision, Provider, ReplyAction, TermContext};
use actrealm_runtime::{
    ApprovalAction, AttentionAction, CommandState, EventSpool, InstanceError, RuntimeInstanceGuard,
    RuntimeStore, SessionUsageRecord, SpoolError, StoreError, WaiterRegistry,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "actrealm-m1-{name}-{}-{}",
        std::process::id(),
        Uuid::now_v7()
    ))
}

fn request_at(
    provider: Provider,
    event: &str,
    session: &str,
    turn: Option<&str>,
    command: Option<&str>,
    at: u64,
) -> BridgeRequest {
    let mut raw = json!({
        "hook_event_name": event,
        "session_id": session,
        "cwd": "/tmp/example-project"
    });
    if let Some(turn) = turn {
        raw["turn_id"] = Value::String(turn.to_owned());
        raw["prompt_id"] = Value::String(turn.to_owned());
    }
    if let Some(command) = command {
        raw["tool_name"] = Value::String("Bash".to_owned());
        raw["tool_input"] = json!({ "command": command });
    }
    BridgeRequest::from_hook_at(provider, raw, at)
}

#[test]
fn existing_v1_database_adds_task_titles_without_losing_sessions() {
    let root = temp_root("schema-v2-title");
    fs::create_dir_all(&root).unwrap();
    let database = root.join("data.sqlite");
    let connection = Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE sessions (
               id TEXT PRIMARY KEY, provider TEXT NOT NULL,
               provider_session_id TEXT NOT NULL,
               cwd TEXT, project TEXT, model TEXT, permission_mode TEXT,
               term_app TEXT, term_session_id TEXT, term_tty TEXT, term_title TEXT,
               exec_state TEXT NOT NULL DEFAULT 'idle',
               approval_owner TEXT, activity TEXT, activity_since INTEGER,
               plan_done INTEGER, plan_total INTEGER,
               started_at INTEGER NOT NULL, last_event_at INTEGER NOT NULL,
               ended_at INTEGER,
               UNIQUE(provider, provider_session_id)
             );
             INSERT INTO sessions(
               id, provider, provider_session_id, exec_state, started_at, last_event_at
             ) VALUES ('old-id', 'claude', 'old-session', 'idle', 1, 1);
             CREATE TABLE session_usage (
               provider TEXT NOT NULL,
               provider_session_id TEXT NOT NULL,
               input_tokens INTEGER, output_tokens INTEGER,
               cache_read_tokens INTEGER, cache_creation_tokens INTEGER,
               reasoning_tokens INTEGER, token_total INTEGER,
               last_turn_tokens INTEGER, context_used_tokens INTEGER,
               context_window_tokens INTEGER, context_used_percent INTEGER,
               estimated_cost_usd_micros INTEGER,
               cost_kind TEXT, pricing_source TEXT,
               usage_source TEXT NOT NULL, usage_quality TEXT NOT NULL,
               captured_at INTEGER NOT NULL,
               PRIMARY KEY(provider, provider_session_id)
             );
             PRAGMA user_version = 1;",
        )
        .unwrap();
    drop(connection);

    let store = RuntimeStore::open(&database).unwrap();
    let before = store.snapshot().unwrap();
    assert_eq!(before.sessions.len(), 1);
    assert_eq!(before.sessions[0].title, None);
    assert_eq!(before.sessions[0].provider_title, None);
    assert_eq!(before.sessions[0].provider_title_source, None);
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Claude,
            json!({
                "hook_event_name":"UserPromptSubmit",
                "session_id":"old-session",
                "prompt":"迁移后显示当前任务标题"
            }),
            2,
        ))
        .unwrap();
    assert_eq!(
        store.snapshot().unwrap().sessions[0].title.as_deref(),
        Some("迁移后显示当前任务标题")
    );
    drop(store);
    let connection = Connection::open(&database).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        8
    );
    let usage_columns = connection
        .prepare("PRAGMA table_info(session_usage)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(usage_columns.iter().any(|column| column == "model"));
    drop(connection);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_usage_snapshots_join_existing_sessions_without_prompt_content() {
    let root = temp_root("session-usage");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"UserPromptSubmit",
                "session_id":"usage-session",
                "prompt":"this text must not enter session_usage"
            }),
            1,
        ))
        .unwrap();
    store
        .upsert_session_usage(SessionUsageRecord {
            provider: "codex".to_owned(),
            provider_session_id: "usage-session".to_owned(),
            model: Some("gpt-5.6-sol".to_owned()),
            input_tokens: Some(700),
            output_tokens: Some(100),
            cache_read_tokens: Some(400),
            cache_creation_tokens: None,
            reasoning_tokens: Some(20),
            token_total: Some(800),
            last_turn_tokens: Some(300),
            context_used_tokens: Some(250),
            context_window_tokens: Some(1_000),
            context_used_percent: Some(25),
            estimated_cost_usd_micros: Some(4_250),
            cost_kind: Some("estimated_api_price".to_owned()),
            pricing_source: Some("test_snapshot".to_owned()),
            usage_source: "codex_rollout".to_owned(),
            usage_quality: "official_local".to_owned(),
            captured_at: 2,
        })
        .unwrap();
    let session = store.snapshot().unwrap().sessions.remove(0);
    assert_eq!(session.token_total, Some(800));
    assert_eq!(session.model.as_deref(), Some("gpt-5.6-sol"));
    assert_eq!(session.input_tokens, Some(700));
    assert_eq!(session.context_used_tokens, Some(250));
    assert_eq!(session.context_used_percent, Some(25));
    assert_eq!(session.estimated_cost_usd_micros, Some(4_250));
    assert_eq!(session.usage_source.as_deref(), Some("codex_rollout"));
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn provider_titles_are_separate_from_the_live_task_and_follow_claude_updates() {
    let root = temp_root("provider-title");
    let database = root.join("data.sqlite");
    let transcript = root
        .join(".claude/projects/demo")
        .join("title-session.jsonl");
    fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    fs::write(
        &transcript,
        "{\"type\":\"ai-title\",\"aiTitle\":\"客户端 AI 标题\"}\n",
    )
    .unwrap();
    let store = RuntimeStore::open(&database).unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Claude,
            json!({
                "hook_event_name":"SessionStart",
                "session_id":"title-session",
                "cwd":"/tmp/demo",
                "transcript_path":transcript,
                "session_title":"Claude 当前官方标题"
            }),
            1_000,
        ))
        .unwrap();

    let initial = store.snapshot().unwrap().sessions.remove(0);
    assert_eq!(
        initial.provider_title.as_deref(),
        Some("Claude 当前官方标题")
    );
    assert_eq!(
        initial.provider_title_source.as_deref(),
        Some("claude_session_title")
    );
    assert_eq!(initial.title, None);

    fs::write(
        &transcript,
        concat!(
            "{\"type\":\"ai-title\",\"aiTitle\":\"客户端 AI 标题\"}\n",
            "{\"type\":\"custom-title\",\"customTitle\":\"用户重命名后的标题\"}\n"
        ),
    )
    .unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Claude,
            json!({
                "hook_event_name":"UserPromptSubmit",
                "session_id":"title-session",
                "cwd":"/tmp/demo",
                "transcript_path":transcript,
                "prompt":"继续处理实时状态同步"
            }),
            2_000,
        ))
        .unwrap();

    let updated = store.snapshot().unwrap().sessions.remove(0);
    assert_eq!(
        updated.provider_title.as_deref(),
        Some("用户重命名后的标题")
    );
    assert_eq!(
        updated.provider_title_source.as_deref(),
        Some("claude_custom_title")
    );
    assert_eq!(updated.title.as_deref(), Some("继续处理实时状态同步"));
    let public = serde_json::to_value(updated).unwrap();
    assert_eq!(public["providerTitle"], "用户重命名后的标题");
    assert_eq!(public["title"], "继续处理实时状态同步");
    assert!(!public.to_string().contains("transcript_path"));

    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn current_task_title_is_a_bounded_prompt_summary_with_live_activity_time() {
    let root = temp_root("task-title");
    let database = root.join("data.sqlite");
    let store = RuntimeStore::open(&database).unwrap();
    let private_tail = "PRIVATE_TAIL_MUST_NOT_BE_PERSISTED";
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"UserPromptSubmit",
                "session_id":"title-session",
                "cwd":"/tmp/example-project",
                "prompt":format!(
                    "修复额度、实时进程和任务列表，并保证标题来自当前任务而不是用户名，继续补充足够长的说明 {private_tail}"
                )
            }),
            9_000,
        ))
        .unwrap();

    let snapshot = store.snapshot().unwrap();
    assert!(!serde_json::to_string(&snapshot)
        .unwrap()
        .contains(private_tail));
    let session = snapshot.sessions[0].clone();
    let title = session.title.unwrap();
    assert!(title.starts_with("修复额度、实时进程和任务列表"));
    assert!(title.chars().count() <= 65);
    assert_eq!(session.activity_since, Some(9_000));
    assert_eq!(session.turn_started_at, Some(9_000));
    assert_eq!(session.turn_ended_at, None);
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn normalized_usage_is_exposed_only_when_provider_supplies_real_token_fields() {
    let root = temp_root("token-usage");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"UserPromptSubmit",
                "session_id":"usage-session",
                "turn_id":"turn-1",
                "cwd":"/tmp/token-project",
                "prompt":"Measure the current turn"
            }),
            40_000,
        ))
        .unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"Notification",
                "session_id":"usage-session",
                "turn_id":"turn-1",
                "cwd":"/tmp/token-project",
                "tokenUsage": { "totalTokens": 12_345 },
                "contextWindow": 200_000
            }),
            40_500,
        ))
        .unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"Stop",
                "session_id":"usage-session",
                "turn_id":"turn-1",
                "cwd":"/tmp/token-project"
            }),
            41_000,
        ))
        .unwrap();

    let session = store.snapshot().unwrap().sessions.remove(0);
    assert_eq!(session.token_total, Some(12_345));
    assert_eq!(session.context_window_tokens, Some(200_000));
    assert_eq!(session.turn_started_at, Some(40_000));
    assert_eq!(session.turn_ended_at, Some(41_000));
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn restart_restores_running_session_and_keeps_private_jump_locator_out_of_json() {
    let root = temp_root("restart-running-jump");
    let database = root.join("data.sqlite");
    let provider_session_id = Uuid::now_v7().to_string();
    {
        let store = RuntimeStore::open(&database).unwrap();
        let mut request = BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"UserPromptSubmit",
                "session_id":provider_session_id,
                "turn_id":"turn-1",
                "cwd":"/tmp/restart-project",
                "prompt":"Continue after ActRealm restarts"
            }),
            50_000,
        );
        request.term = Some(TermContext {
            app: None,
            session_id: Some("private-window-id".to_owned()),
            tty: Some("/dev/ttys999".to_owned()),
            title: Some("private title".to_owned()),
            bundle_id: Some("com.openai.codex".to_owned()),
            surface: Some("codex_app".to_owned()),
            provider_pid: None,
        });
        store.ingest(request).unwrap();
        let before = store.snapshot().unwrap();
        assert_eq!(before.sessions[0].jump_capability, "exact_conversation");
        let serialized = serde_json::to_string(&before).unwrap();
        assert!(serialized.contains("精确打开对话"));
        assert!(!serialized.contains("private-window-id"));
        assert!(!serialized.contains("/dev/ttys999"));
        assert!(!serialized.contains("com.openai.codex"));
    }

    let reopened = RuntimeStore::open(&database).unwrap();
    let session = &reopened.snapshot().unwrap().sessions[0];
    assert_eq!(session.exec_state, "thinking");
    assert_eq!(session.turn_started_at, Some(50_000));
    assert_eq!(session.jump_capability, "exact_conversation");
    assert_eq!(session.jump_label, "精确打开对话");
    drop(reopened);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn versioned_fixtures_replay_idempotently_into_wal() {
    let root = temp_root("fixtures");
    let database = root.join("data.sqlite");
    let store = RuntimeStore::open(&database).unwrap();
    let inputs = [
        (
            Provider::Claude,
            include_str!("../../../fixtures/claude/2.1.210/session-start.json"),
        ),
        (
            Provider::Claude,
            include_str!("../../../fixtures/claude/2.1.210/user-prompt-submit.json"),
        ),
        (
            Provider::Claude,
            include_str!("../../../fixtures/claude/2.1.210/pre-tool-use.json"),
        ),
        (
            Provider::Claude,
            include_str!("../../../fixtures/claude/2.1.210/permission-request.json"),
        ),
        (
            Provider::Claude,
            include_str!("../../../fixtures/claude/2.1.210/task-created.json"),
        ),
        (
            Provider::Claude,
            include_str!("../../../fixtures/claude/2.1.210/task-completed.json"),
        ),
        (
            Provider::Claude,
            include_str!("../../../fixtures/claude/2.1.210/stop.json"),
        ),
        (
            Provider::Codex,
            include_str!("../../../fixtures/codex/0.144.4/session-start.json"),
        ),
        (
            Provider::Codex,
            include_str!("../../../fixtures/codex/0.144.4/user-prompt-submit.json"),
        ),
        (
            Provider::Codex,
            include_str!("../../../fixtures/codex/0.144.4/pre-tool-use.json"),
        ),
        (
            Provider::Codex,
            include_str!("../../../fixtures/codex/0.144.4/permission-request.json"),
        ),
        (
            Provider::Codex,
            include_str!("../../../fixtures/codex/0.144.4/post-tool-use.json"),
        ),
        (
            Provider::Codex,
            include_str!("../../../fixtures/codex/0.144.4/stop.json"),
        ),
    ];
    let envelopes: Vec<_> = inputs
        .iter()
        .enumerate()
        .map(|(index, (provider, fixture))| {
            BridgeRequest::from_hook_at(
                *provider,
                serde_json::from_str(fixture).unwrap(),
                10_000 + index as u64,
            )
        })
        .collect();

    for envelope in &envelopes {
        assert!(store.ingest(envelope.clone()).unwrap().inserted);
        assert!(!store.ingest(envelope.clone()).unwrap().inserted);
    }

    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.event_count, envelopes.len() as u64);
    assert_eq!(snapshot.sessions.len(), 2);
    assert_eq!(snapshot.attention.len(), 2);
    assert!(snapshot
        .sessions
        .iter()
        .all(|session| session.exec_state == "response_finished"));
    let claude = snapshot
        .sessions
        .iter()
        .find(|session| session.provider == "claude")
        .unwrap();
    assert_eq!((claude.plan_done, claude.plan_total), (Some(1), Some(1)));
    drop(store);

    let connection = Connection::open(&database).unwrap();
    let journal: String = connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    assert_eq!(journal.to_ascii_lowercase(), "wal");
    assert_eq!(
        fs::metadata(&database).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o700
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn approval_race_has_exactly_one_transactional_winner() {
    let root = temp_root("race");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let permission = request_at(
        Provider::Codex,
        "PermissionRequest",
        "race-session",
        Some("turn-1"),
        Some("cargo test"),
        10_000,
    );
    let request_id = permission.request_id.unwrap();
    store.ingest(permission).unwrap();

    let barrier = Arc::new(Barrier::new(4));
    let mut workers = Vec::new();
    for action in [
        ApprovalAction::Approve,
        ApprovalAction::Deny,
        ApprovalAction::PassThrough,
    ] {
        let store = store.clone();
        let barrier = Arc::clone(&barrier);
        workers.push(thread::spawn(move || {
            let command_id = Uuid::now_v7();
            barrier.wait();
            store.claim_approval(command_id, request_id, action, 11_000)
        }));
    }
    barrier.wait();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(StoreError::StaleApproval)))
            .count(),
        2
    );
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.commands.len(), 1);
    assert!(matches!(
        snapshot.attention[0].state.as_str(),
        "committing" | "passed_through"
    ));
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn delayed_commit_undo_and_provider_confirmation_are_honest() {
    let root = temp_root("commands");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let permission = request_at(
        Provider::Codex,
        "PermissionRequest",
        "command-session",
        Some("turn-1"),
        Some("cargo test"),
        10_000,
    );
    let request_id = permission.request_id.unwrap();
    store.ingest(permission).unwrap();

    let first = Uuid::now_v7();
    let claim = store
        .claim_approval(first, request_id, ApprovalAction::Approve, 11_000)
        .unwrap();
    assert_eq!(claim.state, CommandState::PendingCommit);
    assert_eq!(
        store.commit(first, 13_999, true),
        Err(StoreError::CommitTooEarly)
    );
    assert_eq!(store.undo(first, 13_999).unwrap(), CommandState::Undone);

    let second = Uuid::now_v7();
    store
        .claim_approval(second, request_id, ApprovalAction::Approve, 15_000)
        .unwrap();
    let committed = store.commit(second, 18_000, true).unwrap();
    assert_eq!(committed.state, CommandState::DecisionSent);
    assert_eq!(committed.action.decision(), Some(Decision::Allow));

    let stop = request_at(
        Provider::Codex,
        "Stop",
        "command-session",
        Some("turn-1"),
        None,
        18_001,
    );
    store.ingest(stop).unwrap();
    assert_eq!(store.snapshot().unwrap().commands[1].state, "decision_sent");

    let post = request_at(
        Provider::Codex,
        "PostToolUse",
        "command-session",
        Some("turn-1"),
        None,
        18_002,
    );
    store.ingest(post).unwrap();
    // The late tool event cannot revive a finished turn or retroactively
    // confirm it after Stop.
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.commands[1].state, "decision_sent");
    assert_eq!(snapshot.sessions[0].exec_state, "response_finished");
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn tool_completion_before_stop_confirms_only_an_allow() {
    let root = temp_root("confirmation");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let permission = request_at(
        Provider::Codex,
        "PermissionRequest",
        "confirmation-session",
        Some("turn-1"),
        Some("cargo test"),
        1_000,
    );
    let request_id = permission.request_id.unwrap();
    store.ingest(permission).unwrap();
    let command_id = Uuid::now_v7();
    store
        .claim_approval(command_id, request_id, ApprovalAction::Approve, 2_000)
        .unwrap();
    store.commit(command_id, 5_000, true).unwrap();
    let confirmation = store
        .ingest(request_at(
            Provider::Codex,
            "PostToolUse",
            "confirmation-session",
            Some("turn-1"),
            None,
            5_001,
        ))
        .unwrap();
    assert_eq!(confirmation.resolved_request_ids, vec![request_id]);
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.commands[0].state, "confirmed");
    assert_eq!(snapshot.attention[0].state, "resolved");
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_auto_review_is_observed_without_creating_a_competing_waiter() {
    let root = temp_root("codex-auto-review");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let request = BridgeRequest::from_hook_at(
        Provider::Codex,
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
    assert!(!request.needs_reply);
    let result = store.ingest(request).unwrap();
    assert_eq!(result.attention_id, None);
    let snapshot = store.snapshot().unwrap();
    assert!(snapshot.attention.is_empty());
    assert_eq!(snapshot.sessions[0].exec_state, "thinking");
    assert_eq!(
        snapshot.sessions[0].approval_owner.as_deref(),
        Some("provider")
    );
    assert_eq!(
        snapshot.sessions[0].activity.as_deref(),
        Some("Codex 正在自动审批")
    );
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_provider_approval_is_visible_and_resolves_with_the_provider_state() {
    let root = temp_root("native-provider-approval");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(request_at(
            Provider::Codex,
            "UserPromptSubmit",
            "native-session",
            Some("turn-1"),
            None,
            1_000,
        ))
        .unwrap();

    assert!(
        store
            .sync_native_approval(Provider::Codex, "native-session", true, true, 1_100)
            .unwrap()
            .session_found
    );
    let waiting = store.snapshot().unwrap();
    assert_eq!(waiting.attention.len(), 1);
    assert_eq!(waiting.attention[0].kind, "native_approval");
    assert_eq!(waiting.attention[0].state, "open");
    assert_eq!(waiting.sessions[0].exec_state, "awaiting_approval");
    assert_eq!(
        waiting.sessions[0].approval_owner.as_deref(),
        Some("terminal")
    );

    assert!(
        store
            .sync_native_approval(Provider::Codex, "native-session", false, true, 1_200)
            .unwrap()
            .session_found
    );
    let resolved = store.snapshot().unwrap();
    assert_eq!(resolved.attention[0].state, "resolved");
    assert_eq!(
        resolved.attention[0].resolution.as_deref(),
        Some("provider_handled")
    );
    assert_eq!(resolved.sessions[0].exec_state, "thinking");
    assert_eq!(resolved.sessions[0].approval_owner, None);

    store
        .sync_native_approval(Provider::Codex, "native-session", true, true, 1_300)
        .unwrap();
    store
        .sync_native_approval(Provider::Codex, "native-session", false, false, 1_400)
        .unwrap();
    let ended = store.snapshot().unwrap();
    assert_eq!(ended.sessions[0].exec_state, "response_finished");
    assert_eq!(ended.sessions[0].approval_owner, None);
    assert_eq!(ended.attention[1].state, "resolved");
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_request_permissions_hook_tracks_native_request_without_claiming_a_decision() {
    let root = temp_root("codex-request-permissions-native");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let start = BridgeRequest::from_hook_at(
        Provider::Codex,
        json!({
            "hook_event_name":"PreToolUse",
            "session_id":"codex-desktop-native",
            "turn_id":"turn-native",
            "transcript_path":"/tmp/rollout-native.jsonl",
            "cwd":"/tmp/example-project",
            "permission_mode":"default",
            "tool_name":"request_permissions",
            "tool_use_id":"call-native",
            "tool_input":{
                "permissions":{"network":{"enabled":true}},
                "reason":"允许本任务后续运行的终端命令访问互联网。"
            }
        }),
        100_000,
    );
    assert!(!start.needs_reply);
    let ingested = store.ingest(start).unwrap();
    assert_eq!(ingested.kind, actrealm_core::EventKind::PermissionRequested);

    let waiting = store.snapshot().unwrap();
    assert_eq!(waiting.attention.len(), 1);
    assert_eq!(waiting.attention[0].kind, "native_approval");
    assert_eq!(waiting.attention[0].title, "Codex 正在请求批准");
    assert_eq!(
        waiting.attention[0].detail.as_deref(),
        Some("允许本任务后续运行的终端命令访问互联网。")
    );
    assert_eq!(waiting.attention[0].request_id, None);
    assert_eq!(waiting.sessions[0].exec_state, "awaiting_approval");
    assert_eq!(
        waiting.sessions[0].approval_owner.as_deref(),
        Some("terminal")
    );

    // Open Island's reducer deliberately protects actionable state from an
    // incidental running update. ActRealm must do the same.
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"Notification",
                "session_id":"codex-desktop-native",
                "turn_id":"turn-native",
                "message":"still waiting"
            }),
            100_001,
        ))
        .unwrap();
    let preserved = store.snapshot().unwrap();
    assert_eq!(preserved.sessions[0].exec_state, "awaiting_approval");
    assert_eq!(
        preserved.sessions[0].approval_owner.as_deref(),
        Some("terminal")
    );
    assert_eq!(preserved.attention[0].state, "open");

    // PostToolUse means only that Codex finished handling the request. It does
    // not reveal whether the user approved or denied it.
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"PostToolUse",
                "session_id":"codex-desktop-native",
                "turn_id":"turn-native",
                "tool_name":"request_permissions",
                "tool_use_id":"call-native",
                "tool_response":{"status":"handled"}
            }),
            100_002,
        ))
        .unwrap();
    let handled = store.snapshot().unwrap();
    assert_eq!(handled.attention[0].state, "resolved");
    assert_eq!(
        handled.attention[0].resolution.as_deref(),
        Some("provider_handled")
    );
    assert_eq!(handled.sessions[0].exec_state, "thinking");
    assert_eq!(handled.sessions[0].approval_owner, None);
    assert!(handled.sessions[0]
        .activity
        .as_deref()
        .is_some_and(|activity| activity.contains("Codex 原界面处理")));
    assert!(!handled.sessions[0]
        .activity
        .as_deref()
        .is_some_and(|activity| activity.contains("批准") || activity.contains("拒绝")));

    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_provider_signal_does_not_duplicate_a_live_hook_approval() {
    let root = temp_root("native-hook-dedupe");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(request_at(
            Provider::Codex,
            "PermissionRequest",
            "dedupe-session",
            Some("turn-1"),
            Some("cargo test"),
            1_000,
        ))
        .unwrap();
    assert!(
        store
            .sync_native_approval(Provider::Codex, "dedupe-session", true, true, 1_100)
            .unwrap()
            .session_found
    );
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.attention.len(), 1);
    assert_eq!(snapshot.attention[0].kind, "approval");
    assert_eq!(
        snapshot.sessions[0].approval_owner.as_deref(),
        Some("widget")
    );
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn restart_expires_every_approval_without_a_live_waiter() {
    let root = temp_root("restart");
    let database = root.join("data.sqlite");
    let permission = request_at(
        Provider::Claude,
        "PermissionRequest",
        "restart-session",
        Some("prompt-1"),
        Some("git status"),
        10_000,
    );
    let question = BridgeRequest::from_hook_at(
        Provider::Claude,
        json!({
            "hook_event_name":"PreToolUse",
            "session_id":"restart-question-session",
            "prompt_id":"prompt-2",
            "tool_name":"AskUserQuestion",
            "tool_input":{"questions":[{"question":"Continue?","header":"Confirm","options":[],"multiSelect":false}]}
        }),
        10_001,
    );
    {
        let store = RuntimeStore::open(&database).unwrap();
        store.ingest(permission).unwrap();
        store.ingest(question).unwrap();
    }
    let reopened = RuntimeStore::open(&database).unwrap();
    assert_eq!(
        reopened.reconcile_orphaned_approvals(Vec::new(), 20_000),
        Ok(2)
    );
    assert!(reopened
        .snapshot()
        .unwrap()
        .attention
        .iter()
        .all(|item| item.state == "expired"));
    drop(reopened);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn stale_waiter_can_never_commit_a_persisted_decision() {
    let root = temp_root("stale-waiter");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let permission = request_at(
        Provider::Claude,
        "PermissionRequest",
        "stale-session",
        Some("prompt-1"),
        Some("cargo test"),
        1_000,
    );
    let request_id = permission.request_id.unwrap();
    store.ingest(permission).unwrap();
    let command_id = Uuid::now_v7();
    store
        .claim_approval(command_id, request_id, ApprovalAction::Deny, 2_000)
        .unwrap();
    assert_eq!(
        store.commit(command_id, 5_000, false),
        Err(StoreError::StaleApproval)
    );
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.attention[0].state, "expired");
    assert_eq!(snapshot.commands[0].state, "failed");
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn stop_is_turn_end_until_process_liveness_marks_the_session_idle() {
    let root = temp_root("liveness");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(request_at(
            Provider::Codex,
            "UserPromptSubmit",
            "live-session",
            Some("turn-1"),
            None,
            1_000,
        ))
        .unwrap();
    store
        .ingest(request_at(
            Provider::Codex,
            "Stop",
            "live-session",
            Some("turn-1"),
            None,
            1_100,
        ))
        .unwrap();
    assert_eq!(
        store.snapshot().unwrap().sessions[0].exec_state,
        "response_finished"
    );
    assert_eq!(
        store
            .reconcile_session_liveness(
                vec![(Provider::Codex, "live-session".to_owned())],
                10_000,
                1_000,
            )
            .unwrap(),
        0
    );
    assert_eq!(
        store.snapshot().unwrap().sessions[0].exec_state,
        "response_finished"
    );
    assert_eq!(
        store
            .reconcile_session_liveness(Vec::new(), 10_000, 1_000)
            .unwrap(),
        1
    );
    assert_eq!(store.snapshot().unwrap().sessions[0].exec_state, "idle");

    // A pending human approval is never idled merely because discovery missed
    // the process; native-control ownership must be resolved first.
    store
        .ingest(request_at(
            Provider::Claude,
            "PermissionRequest",
            "approval-session",
            Some("prompt-1"),
            Some("cargo test"),
            20_000,
        ))
        .unwrap();
    assert_eq!(
        store
            .reconcile_session_liveness(Vec::new(), 30_000, 1_000)
            .unwrap(),
        0
    );
    let approval_session = store
        .snapshot()
        .unwrap()
        .sessions
        .into_iter()
        .find(|session| session.provider_session_id == "approval-session")
        .unwrap();
    assert_eq!(approval_session.exec_state, "awaiting_approval");
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn previews_are_redacted_and_risk_never_uses_history_to_downgrade() {
    let root = temp_root("risk");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(request_at(
            Provider::Claude,
            "PermissionRequest",
            "risk-session",
            Some("prompt-1"),
            Some("git push --token super-secret origin main"),
            1_000,
        ))
        .unwrap();
    let attention = store.snapshot().unwrap().attention.remove(0);
    assert_eq!(attention.risk, "high");
    let preview = attention.command_preview.unwrap();
    assert!(preview.contains("<redacted>"));
    assert!(!preview.contains("super-secret"));
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn duplicate_waiter_replaces_old_continuation_and_resolution_has_one_winner() {
    let registry = WaiterRegistry::default();
    let first = request_at(
        Provider::Codex,
        "PermissionRequest",
        "waiter-session",
        Some("turn-1"),
        Some("cargo test"),
        1_000,
    );
    let second = request_at(
        Provider::Codex,
        "PermissionRequest",
        "waiter-session",
        Some("turn-1"),
        Some("cargo test"),
        1_001,
    );
    let first_registration = registry.register_at(&first, 1_100).unwrap();
    let second_registration = registry.register_at(&second, 1_100).unwrap();
    assert_eq!(second_registration.replaced_request_id, first.request_id);
    let old_response = first_registration
        .ticket
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    assert_eq!(old_response.action, ReplyAction::PassThrough);
    assert_eq!(old_response.reason.as_deref(), Some("duplicate_replaced"));

    let request_id = second.request_id.unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for decision in [Decision::Allow, Decision::Deny] {
        let registry = registry.clone();
        let barrier = Arc::clone(&barrier);
        workers.push(thread::spawn(move || {
            barrier.wait();
            registry.decide(request_id, decision)
        }));
    }
    barrier.wait();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let response = second_registration
        .ticket
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    assert!(matches!(
        response.action,
        ReplyAction::Allow | ReplyAction::Deny
    ));
    assert!(registry.raw(request_id).unwrap().is_none());
}

#[test]
fn waiter_deadline_passes_through_and_releases_raw_payload() {
    let registry = WaiterRegistry::default();
    let request = request_at(
        Provider::Codex,
        "PermissionRequest",
        "deadline-session",
        Some("turn-1"),
        Some("cargo test"),
        1_000,
    );
    let deadline = request.deadline_at.unwrap();
    let registration = registry.register_at(&request, 1_000).unwrap();
    assert_eq!(registry.expire_at(deadline).unwrap(), 1);
    let response = registration
        .ticket
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    assert_eq!(response.action, ReplyAction::PassThrough);
    assert_eq!(response.reason.as_deref(), Some("deadline"));
    assert!(registry.raw(request.request_id.unwrap()).unwrap().is_none());
}

#[test]
fn independent_sessions_can_wait_and_resolve_concurrently() {
    let registry = WaiterRegistry::default();
    let claude = request_at(
        Provider::Claude,
        "PermissionRequest",
        "claude-session",
        Some("prompt-1"),
        Some("cargo test"),
        1_000,
    );
    let codex = request_at(
        Provider::Codex,
        "PermissionRequest",
        "codex-session",
        Some("turn-1"),
        Some("git status"),
        1_000,
    );
    let claude_ticket = registry.register_at(&claude, 1_100).unwrap().ticket;
    let codex_ticket = registry.register_at(&codex, 1_100).unwrap().ticket;
    assert_eq!(registry.active_request_ids().unwrap().len(), 2);
    registry
        .decide(claude.request_id.unwrap(), Decision::Allow)
        .unwrap();
    registry
        .pass_through(codex.request_id.unwrap(), "user")
        .unwrap();
    assert_eq!(
        claude_ticket
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .action,
        ReplyAction::Allow
    );
    assert_eq!(
        codex_ticket
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .action,
        ReplyAction::PassThrough
    );
    assert!(registry.active_request_ids().unwrap().is_empty());
}

#[test]
fn different_commands_in_the_same_turn_are_not_false_duplicates() {
    let registry = WaiterRegistry::default();
    let first = request_at(
        Provider::Codex,
        "PermissionRequest",
        "same-session",
        Some("same-turn"),
        Some("cargo test"),
        1_000,
    );
    let second = request_at(
        Provider::Codex,
        "PermissionRequest",
        "same-session",
        Some("same-turn"),
        Some("git status"),
        1_001,
    );
    let first_registration = registry.register_at(&first, 1_100).unwrap();
    let second_registration = registry.register_at(&second, 1_100).unwrap();
    assert_eq!(first_registration.replaced_request_id, None);
    assert_eq!(second_registration.replaced_request_id, None);
    assert_eq!(registry.active_request_ids().unwrap().len(), 2);
    registry
        .pass_through(first.request_id.unwrap(), "test")
        .unwrap();
    registry
        .pass_through(second.request_id.unwrap(), "test")
        .unwrap();
}

#[test]
fn spool_is_bounded_replayed_and_never_accepts_permissions() {
    let root = temp_root("spool");
    let spool = EventSpool::with_limits(&root, 2, 1024 * 1024);
    let mut written = Vec::new();
    for index in 0..3 {
        let request = request_at(
            Provider::Codex,
            "Stop",
            "spool-session",
            Some("turn-1"),
            None,
            1_000 + index,
        );
        written.push(request.id);
        spool.append(&request).unwrap();
    }
    assert_eq!(spool.len().unwrap(), 2);
    assert_eq!(
        fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let mut replayed = Vec::new();
    assert_eq!(
        spool
            .drain(|request| {
                replayed.push(request.id);
                true
            })
            .unwrap(),
        2
    );
    assert_eq!(replayed, written[1..]);
    assert_eq!(spool.len().unwrap(), 0);

    let permission = request_at(
        Provider::Codex,
        "PermissionRequest",
        "spool-session",
        Some("turn-2"),
        Some("cargo test"),
        2_000,
    );
    assert!(matches!(
        spool.append(&permission),
        Err(SpoolError::PermissionRequest)
    ));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn runtime_lock_allows_only_one_instance_and_is_reusable_after_drop() {
    let root = temp_root("instance");
    let lock = root.join("runtime.lock");
    let first = RuntimeInstanceGuard::acquire(&lock).unwrap();
    assert!(matches!(
        RuntimeInstanceGuard::acquire(&lock),
        Err(InstanceError::AlreadyRunning(_))
    ));
    assert_eq!(
        fs::metadata(&lock).unwrap().permissions().mode() & 0o777,
        0o600
    );
    drop(first);
    RuntimeInstanceGuard::acquire(&lock).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn claude_task_progress_uses_only_stable_task_ids_and_never_invents_a_percentage() {
    let root = temp_root("task-progress");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let event = |name: &str, task_id: Option<&str>, at: u64| {
        let mut raw = json!({
            "hook_event_name": name,
            "session_id": "task-session",
            "cwd": "/tmp/example-project"
        });
        if let Some(task_id) = task_id {
            raw["task_id"] = Value::String(task_id.to_owned());
            raw["task_subject"] = Value::String("fact-only subject".to_owned());
        }
        BridgeRequest::from_hook_at(Provider::Claude, raw, at)
    };

    store
        .ingest(event("TaskCreated", Some("task-1"), 1_000))
        .unwrap();
    store
        .ingest(event("TaskCreated", Some("task-2"), 1_001))
        .unwrap();
    store
        .ingest(event("TaskCreated", Some("task-1"), 1_002))
        .unwrap();
    let created = store.snapshot().unwrap().sessions.remove(0);
    assert_eq!((created.plan_done, created.plan_total), (Some(0), Some(2)));
    assert_eq!(created.activity.as_deref(), Some("计划进度 0/2"));

    store
        .ingest(event("TaskCompleted", Some("task-1"), 1_003))
        .unwrap();
    store
        .ingest(event("TaskCompleted", Some("task-1"), 1_004))
        .unwrap();
    let half = store.snapshot().unwrap().sessions.remove(0);
    assert_eq!((half.plan_done, half.plan_total), (Some(1), Some(2)));

    store.ingest(event("TaskCompleted", None, 1_005)).unwrap();
    let missing_identity = store.snapshot().unwrap().sessions.remove(0);
    assert_eq!(
        (missing_identity.plan_done, missing_identity.plan_total),
        (Some(1), Some(2)),
        "a task event without task_id must not alter factual progress"
    );

    store
        .ingest(event("TaskCompleted", Some("task-2"), 1_006))
        .unwrap();
    let completed = store.snapshot().unwrap().sessions.remove(0);
    assert_eq!(
        (completed.plan_done, completed.plan_total),
        (Some(2), Some(2))
    );
    assert_eq!(completed.activity.as_deref(), Some("计划进度 2/2"));
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn subagent_counts_and_background_stop_state_are_fact_based() {
    let root = temp_root("subagents-background");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let subagent = |event: &str, agent_id: Option<&str>, at: u64| {
        let mut raw = json!({
            "hook_event_name": event,
            "session_id": "subagent-session",
            "cwd": "/tmp/example-project"
        });
        if let Some(agent_id) = agent_id {
            raw["agent_id"] = Value::String(agent_id.to_owned());
            raw["agent_type"] = Value::String("Explore".to_owned());
        }
        BridgeRequest::from_hook_at(Provider::Claude, raw, at)
    };
    store
        .ingest(subagent("SubagentStart", Some("agent-1"), 2_000))
        .unwrap();
    store
        .ingest(subagent("SubagentStart", Some("agent-2"), 2_001))
        .unwrap();
    store
        .ingest(subagent("SubagentStart", Some("agent-1"), 2_002))
        .unwrap();
    assert_eq!(
        store.snapshot().unwrap().sessions[0].activity.as_deref(),
        Some("派了 2 个子 Agent")
    );
    store
        .ingest(subagent("SubagentStop", Some("agent-1"), 2_003))
        .unwrap();
    assert_eq!(
        store.snapshot().unwrap().sessions[0].activity.as_deref(),
        Some("派了 1 个子 Agent")
    );

    let tool = BridgeRequest::from_hook_at(
        Provider::Claude,
        json!({
            "hook_event_name": "PreToolUse",
            "session_id": "background-session",
            "cwd": "/tmp/example-project",
            "tool_name": "Write",
            "tool_input": {"file_path": "/tmp/example-project/file.txt"}
        }),
        3_000,
    );
    store.ingest(tool).unwrap();
    let stop = BridgeRequest::from_hook_at(
        Provider::Claude,
        json!({
            "hook_event_name": "Stop",
            "session_id": "background-session",
            "cwd": "/tmp/example-project",
            "background_tasks": [{"id": "bg-1", "type": "shell", "status": "running"}],
            "session_crons": []
        }),
        3_001,
    );
    store.ingest(stop).unwrap();
    let snapshot = store.snapshot().unwrap();
    let background = snapshot
        .sessions
        .iter()
        .find(|session| session.provider_session_id == "background-session")
        .unwrap();
    assert_eq!(background.exec_state, "tool_running");
    assert_eq!(background.activity.as_deref(), Some("后台任务仍在运行 · 1"));
    assert!(snapshot
        .attention
        .iter()
        .all(|attention| attention.session_id != background.id));
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn factual_error_question_and_completion_attention_support_local_actions() {
    let root = temp_root("attention-kinds");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    let error = BridgeRequest::from_hook_at(
        Provider::Claude,
        json!({
            "hook_event_name": "StopFailure",
            "session_id": "attention-session",
            "prompt_id": "turn-1",
            "cwd": "/tmp/example-project",
            "error": "request failed token=super-secret"
        }),
        now,
    );
    store.ingest(error).unwrap();
    let question = BridgeRequest::from_hook_at(
        Provider::Claude,
        json!({
            "hook_event_name": "Notification",
            "session_id": "question-session",
            "prompt_id": "turn-2",
            "cwd": "/tmp/example-project",
            "notification_type": "question",
            "notification_id": "question-1",
            "message": "Which database should I use?"
        }),
        now + 1,
    );
    store.ingest(question).unwrap();

    for (offset, event, tool_name) in [
        (2, "UserPromptSubmit", None),
        (3, "PreToolUse", Some("Write")),
        (4, "PostToolUse", Some("Write")),
        (5, "Stop", None),
    ] {
        let mut raw = json!({
            "hook_event_name": event,
            "session_id": "completion-session",
            "turn_id": "turn-3",
            "cwd": "/tmp/example-project"
        });
        if let Some(tool_name) = tool_name {
            raw["tool_name"] = Value::String(tool_name.to_owned());
            raw["tool_input"] = json!({ "file_path": "/tmp/example-project/file.rs" });
        }
        store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Codex,
                raw,
                now + offset,
            ))
            .unwrap();
    }

    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.attention.len(), 3);
    let error = snapshot
        .attention
        .iter()
        .find(|item| item.kind == "error")
        .unwrap();
    assert!(!error.detail.as_deref().unwrap().contains("super-secret"));
    let error_id = error.id.clone();
    let question_id = snapshot
        .attention
        .iter()
        .find(|item| item.kind == "question")
        .unwrap()
        .id
        .clone();
    assert_eq!(
        snapshot
            .attention
            .iter()
            .find(|item| item.kind == "question")
            .and_then(|item| item.detail.as_deref()),
        Some("Which database should I use?")
    );
    assert!(snapshot
        .attention
        .iter()
        .any(|item| item.kind == "completion"));

    assert_eq!(
        store
            .act_on_attention(Uuid::now_v7(), error_id, AttentionAction::Ack, now + 10)
            .unwrap(),
        CommandState::Confirmed
    );
    assert_eq!(
        store
            .act_on_attention(
                Uuid::now_v7(),
                question_id.clone(),
                AttentionAction::Snooze,
                now + 10,
            )
            .unwrap(),
        CommandState::Confirmed
    );
    let snapshot = store.snapshot().unwrap();
    assert!(snapshot
        .attention
        .iter()
        .any(|item| item.kind == "error" && item.state == "resolved"));
    assert!(snapshot
        .attention
        .iter()
        .any(|item| item.kind == "question" && item.state == "snoozed"));
    store
        .act_on_attention(
            Uuid::now_v7(),
            question_id,
            AttentionAction::Dismiss,
            now + 11,
        )
        .unwrap();
    assert!(store
        .snapshot()
        .unwrap()
        .attention
        .iter()
        .any(|item| item.kind == "question" && item.state == "dismissed"));
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn new_activity_resolves_stale_completion_and_error_without_hiding_running_state() {
    let root = temp_root("superseded-nonblocking-attention");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();

    for (at, event, turn, tool_name) in [
        (1_000, "UserPromptSubmit", "turn-1", None),
        (1_100, "PreToolUse", "turn-1", Some("Write")),
        (1_200, "Stop", "turn-1", None),
    ] {
        let mut raw = json!({
            "hook_event_name": event,
            "session_id": "resumed-session",
            "turn_id": turn,
            "cwd": "/tmp/example-project"
        });
        if let Some(tool_name) = tool_name {
            raw["tool_name"] = Value::String(tool_name.to_owned());
            raw["tool_input"] = json!({ "file_path": "/tmp/example-project/file.rs" });
        }
        store
            .ingest(BridgeRequest::from_hook_at(Provider::Codex, raw, at))
            .unwrap();
    }
    let stopped = store.snapshot().unwrap();
    let completion = stopped
        .attention
        .iter()
        .find(|item| item.kind == "completion")
        .unwrap();
    assert_eq!(completion.state, "open");
    assert_eq!(stopped.sessions[0].exec_state, "response_finished");

    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name": "UserPromptSubmit",
                "session_id": "resumed-session",
                "turn_id": "turn-2",
                "cwd": "/tmp/example-project",
                "prompt": "继续检查实时状态"
            }),
            2_000,
        ))
        .unwrap();
    let resumed = store.snapshot().unwrap();
    let completion = resumed
        .attention
        .iter()
        .find(|item| item.kind == "completion")
        .unwrap();
    assert_eq!(completion.state, "resolved");
    assert_eq!(
        completion.resolution.as_deref(),
        Some("superseded_by_activity")
    );
    assert_eq!(resumed.sessions[0].exec_state, "thinking");

    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name": "StopFailure",
                "session_id": "resumed-session",
                "turn_id": "turn-2",
                "cwd": "/tmp/example-project",
                "error": "temporary failure"
            }),
            2_100,
        ))
        .unwrap();
    assert!(store
        .snapshot()
        .unwrap()
        .attention
        .iter()
        .any(|item| item.kind == "error" && item.state == "open"));

    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name": "UserPromptSubmit",
                "session_id": "resumed-session",
                "turn_id": "turn-3",
                "cwd": "/tmp/example-project",
                "prompt": "失败后继续"
            }),
            2_200,
        ))
        .unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name": "PreToolUse",
                "session_id": "resumed-session",
                "turn_id": "turn-3",
                "cwd": "/tmp/example-project",
                "tool_name": "Read",
                "tool_input": { "file_path": "/tmp/example-project/file.rs" }
            }),
            2_300,
        ))
        .unwrap();
    let recovered = store.snapshot().unwrap();
    let error = recovered
        .attention
        .iter()
        .find(|item| item.kind == "error")
        .unwrap();
    assert_eq!(error.state, "resolved");
    assert_eq!(error.resolution.as_deref(), Some("superseded_by_activity"));
    assert_eq!(recovered.sessions[0].exec_state, "tool_running");

    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn provider_handled_approval_resolves_attention_and_session_waiting_state() {
    let root = temp_root("provider-handled-approval");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let now = 20_000;
    store
        .ingest(request_at(
            Provider::Claude,
            "UserPromptSubmit",
            "external-session",
            Some("turn-1"),
            None,
            now,
        ))
        .unwrap();
    store
        .ingest(request_at(
            Provider::Claude,
            "PermissionRequest",
            "external-session",
            Some("turn-1"),
            Some("cargo test"),
            now + 1,
        ))
        .unwrap();
    let waiting = store.snapshot().unwrap();
    assert_eq!(waiting.sessions[0].exec_state, "awaiting_approval");
    assert_eq!(waiting.attention[0].state, "open");
    assert!(waiting.attention[0]
        .detail
        .as_deref()
        .is_some_and(|detail| detail.contains("终端命令")));

    store
        .ingest(request_at(
            Provider::Claude,
            "PostToolUse",
            "external-session",
            Some("turn-1"),
            Some("cargo test"),
            now + 2,
        ))
        .unwrap();
    let resolved = store.snapshot().unwrap();
    assert_eq!(resolved.sessions[0].exec_state, "thinking");
    assert_eq!(resolved.sessions[0].approval_owner, None);
    assert_eq!(resolved.attention[0].state, "resolved");
    assert_eq!(
        resolved.attention[0].resolution.as_deref(),
        Some("provider_approved")
    );

    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn provider_denied_event_resolves_unhandled_attention() {
    let root = temp_root("provider-denied");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(request_at(
            Provider::Claude,
            "PermissionRequest",
            "denied-session",
            Some("turn-1"),
            Some("git push"),
            30_000,
        ))
        .unwrap();
    store
        .ingest(request_at(
            Provider::Claude,
            "PermissionDenied",
            "denied-session",
            Some("turn-1"),
            None,
            30_001,
        ))
        .unwrap();

    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.attention[0].state, "resolved");
    assert_eq!(
        snapshot.attention[0].resolution.as_deref(),
        Some("provider_denied")
    );
    assert_eq!(snapshot.sessions[0].exec_state, "thinking");
    assert_eq!(snapshot.sessions[0].approval_owner, None);
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_new_prompt_closes_attention_left_open_by_the_previous_turn() {
    let root = temp_root("new-prompt-closes-attention");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(request_at(
            Provider::Claude,
            "PermissionRequest",
            "next-turn-session",
            Some("turn-1"),
            Some("cargo test"),
            60_000,
        ))
        .unwrap();
    store
        .ingest(request_at(
            Provider::Claude,
            "UserPromptSubmit",
            "next-turn-session",
            Some("turn-2"),
            None,
            60_001,
        ))
        .unwrap();

    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.attention[0].state, "resolved");
    assert_eq!(
        snapshot.attention[0].resolution.as_deref(),
        Some("provider_closed")
    );
    assert_eq!(snapshot.sessions[0].exec_state, "thinking");
    assert_eq!(snapshot.sessions[0].approval_owner, None);
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

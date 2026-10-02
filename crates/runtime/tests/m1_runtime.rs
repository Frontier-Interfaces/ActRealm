#![cfg(unix)]

use actrealm_core::{
    AttentionRiskCode, BridgeRequest, Decision, OperationCategory, Provider, ReplyAction,
    TermContext,
};
use actrealm_runtime::{
    ApprovalAction, AttentionAction, CommandState, EventSpool, InstanceError, ReviewBaselineInput,
    RuntimeInstanceGuard, RuntimeStore, SessionUsageDailyRecord, SessionUsageRecord, SpoolError,
    StoreError, TaskCheckpointInput, WaiterRegistry,
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
        38
    );
    let usage_columns = connection
        .prepare("PRAGMA table_info(session_usage)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(usage_columns.iter().any(|column| column == "model"));
    let session_columns = connection
        .prepare("PRAGMA table_info(sessions)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(session_columns
        .iter()
        .any(|column| column == "current_target"));
    assert!(session_columns
        .iter()
        .any(|column| column == "task_hidden_at"));
    assert!(session_columns
        .iter()
        .any(|column| column == "task_hidden_reason"));
    let event_columns = connection
        .prepare("PRAGMA table_info(events)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(event_columns.iter().any(|column| column == "tool_target"));
    let event_indexes = connection
        .prepare("PRAGMA index_list(events)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(event_indexes
        .iter()
        .any(|index| index == "events_turn_occurred_at"));
    let attention_columns = connection
        .prepare("PRAGMA table_info(attention_items)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(attention_columns
        .iter()
        .any(|column| column == "remote_actionable"));
    assert!(attention_columns
        .iter()
        .any(|column| column == "reminder_acknowledged_at"));
    assert!(attention_columns
        .iter()
        .any(|column| column == "reminder_resolution"));
    drop(connection);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn review_baseline_queue_is_persistent_idempotent_and_records_late_first_tool() {
    let root = temp_root("review-baseline");
    fs::create_dir_all(&root).unwrap();
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"UserPromptSubmit",
                "session_id":"review-baseline-session",
                "turn_id":"review-baseline-turn",
                "cwd":"/tmp/review-baseline"
            }),
            1_000,
        ))
        .unwrap();
    let candidate = store.pending_review_baselines(4).unwrap().remove(0);
    assert_eq!(candidate.first_tool_at, None);
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"PreToolUse",
                "session_id":"review-baseline-session",
                "turn_id":"review-baseline-turn",
                "tool_name":"Read",
                "tool_use_id":"review-tool"
            }),
            1_100,
        ))
        .unwrap();
    let input = ReviewBaselineInput {
        session_id: candidate.session_id.clone(),
        turn_id: candidate.turn_id.clone(),
        repository_root: PathBuf::from("/tmp/review-baseline"),
        repository_identity: "a".repeat(64),
        branch: Some("main".to_owned()),
        head: Some("b".repeat(40)),
        worktree_kind: "primary".to_owned(),
        dirty: false,
        changed_files: 0,
        staged_files: 0,
        unstaged_files: 0,
        untracked_files: 0,
        insertions: Some(0),
        deletions: Some(0),
        binary_files: Some(0),
        turn_started_at: candidate.turn_started_at,
        first_tool_at: candidate.first_tool_at,
        captured_at: 1_200,
    };
    assert!(store.write_review_baseline(input.clone()).unwrap());
    assert!(!store.write_review_baseline(input).unwrap());
    assert!(store.pending_review_baselines(4).unwrap().is_empty());
    let baseline = store.review_baseline(candidate.turn_id).unwrap().unwrap();
    assert_eq!(baseline.first_tool_at, Some(1_100));
    assert_eq!(baseline.captured_at, 1_200);
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn review_baseline_rechecks_first_tool_when_event_arrives_after_capture() {
    let root = temp_root("review-baseline-delayed-tool");
    fs::create_dir_all(&root).unwrap();
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"UserPromptSubmit",
                "session_id":"review-delayed-tool-session",
                "turn_id":"review-delayed-tool-turn",
                "cwd":"/tmp/review-delayed-tool"
            }),
            1_000,
        ))
        .unwrap();
    let candidate = store.pending_review_baselines(4).unwrap().remove(0);
    let input = ReviewBaselineInput {
        session_id: candidate.session_id.clone(),
        turn_id: candidate.turn_id.clone(),
        repository_root: PathBuf::from("/tmp/review-delayed-tool"),
        repository_identity: "a".repeat(64),
        branch: Some("main".to_owned()),
        head: Some("b".repeat(40)),
        worktree_kind: "primary".to_owned(),
        dirty: false,
        changed_files: 0,
        staged_files: 0,
        unstaged_files: 0,
        untracked_files: 0,
        insertions: Some(0),
        deletions: Some(0),
        binary_files: Some(0),
        turn_started_at: candidate.turn_started_at,
        first_tool_at: candidate.first_tool_at,
        captured_at: 1_200,
    };
    assert!(store.write_review_baseline(input).unwrap());
    assert_eq!(
        store
            .review_baseline(&candidate.turn_id)
            .unwrap()
            .unwrap()
            .first_tool_at,
        None
    );

    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"PreToolUse",
                "session_id":"review-delayed-tool-session",
                "turn_id":"review-delayed-tool-turn",
                "tool_name":"Read",
                "tool_use_id":"review-delayed-tool"
            }),
            1_100,
        ))
        .unwrap();

    let baseline = store.review_baseline(candidate.turn_id).unwrap().unwrap();
    assert_eq!(baseline.first_tool_at, Some(1_100));
    assert!(baseline.captured_at > baseline.first_tool_at.unwrap());
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn task_checkpoint_metadata_is_bounded_persistent_and_deletes_without_session_loss() {
    let root = temp_root("task-checkpoint");
    fs::create_dir_all(&root).unwrap();
    let database = root.join("data.sqlite");
    let store = RuntimeStore::open(&database).unwrap();
    let ingested = store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"UserPromptSubmit",
                "session_id":"checkpoint-session",
                "turn_id":"checkpoint-turn",
                "cwd":"/tmp/checkpoint-project"
            }),
            1_000,
        ))
        .unwrap();
    let checkpoint_id = Uuid::now_v7().to_string();
    let turn_id = store
        .local_review_context(&ingested.session_id)
        .unwrap()
        .unwrap()
        .turn_id
        .unwrap();
    let checkpoint = store
        .create_task_checkpoint(TaskCheckpointInput {
            id: checkpoint_id.clone(),
            session_id: ingested.session_id.clone(),
            turn_id,
            label: Some("Before recovery".to_owned()),
            kind: "metadata".to_owned(),
            provider: "codex".to_owned(),
            provider_session_id: "checkpoint-session".to_owned(),
            provider_resume_capability: "unsupported".to_owned(),
            repository_root: Some(PathBuf::from("/tmp/checkpoint-project")),
            repository_identity: Some("a".repeat(64)),
            branch: Some("main".to_owned()),
            head: Some("b".repeat(40)),
            worktree_kind: Some("primary".to_owned()),
            dirty: Some(true),
            changed_files: Some(2),
            staged_files: Some(0),
            unstaged_files: Some(1),
            untracked_files: Some(1),
            git_object_id: None,
            git_ref: None,
            patch_digest: None,
            validation_json: "[]".to_owned(),
            review_baseline_captured_at: Some(900),
            created_at: 1_100,
        })
        .unwrap();
    assert_eq!(checkpoint.id, checkpoint_id);
    assert_eq!(checkpoint.label.as_deref(), Some("Before recovery"));
    assert_eq!(
        store.task_checkpoints(&ingested.session_id).unwrap().len(),
        1
    );
    let export = store.export_json(1_200).unwrap();
    assert_eq!(
        export["tables"]["task_checkpoints"][0]["repository_root"],
        "<redacted>"
    );
    assert_eq!(
        export["tables"]["task_checkpoints"][0]["repository_identity"],
        "<redacted>"
    );
    drop(store);

    let reopened = RuntimeStore::open(&database).unwrap();
    assert_eq!(
        reopened
            .task_checkpoint(&checkpoint_id)
            .unwrap()
            .unwrap()
            .changed_files,
        Some(2)
    );
    assert!(reopened.delete_task_checkpoint(&checkpoint_id).unwrap());
    assert!(reopened.task_checkpoint(&checkpoint_id).unwrap().is_none());
    assert_eq!(reopened.snapshot().unwrap().sessions.len(), 1);
    drop(reopened);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_internal_background_sessions_are_discarded_without_visibility_state() {
    fn codex_app_event(
        event: &str,
        session_id: &str,
        prompt: Option<&str>,
        at: u64,
    ) -> BridgeRequest {
        let mut raw = json!({
            "hook_event_name": event,
            "session_id": session_id,
            "cwd": "/",
            "model": "gpt-test"
        });
        if let Some(prompt) = prompt {
            raw["prompt"] = Value::String(prompt.to_owned());
        }
        if event == "PreToolUse" {
            raw["tool_name"] = Value::String("Read".to_owned());
        }
        let mut request = BridgeRequest::from_hook_at(Provider::Codex, raw, at);
        request.term = Some(TermContext {
            app: None,
            session_id: None,
            tty: None,
            title: None,
            bundle_id: Some("com.openai.codex".to_owned()),
            surface: Some("codex_app".to_owned()),
            provider_pid: Some(42),
        });
        request
    }

    let root = temp_root("codex-internal-background");
    fs::create_dir_all(&root).unwrap();
    let database = root.join("data.sqlite");
    let overview_session = "019f7eb6-6f5c-7240-bffa-feef46eaaf69";
    let store = RuntimeStore::open(&database).unwrap();

    let started = store
        .ingest(codex_app_event(
            "SessionStart",
            overview_session,
            None,
            1_000,
        ))
        .unwrap();
    assert!(started.inserted);
    assert!(!started.suppressed);
    assert_eq!(store.snapshot().unwrap().metrics.sessions_observed, 1);

    let overview = store
        .ingest(codex_app_event(
            "UserPromptSubmit",
            overview_session,
            Some("# Overview\nGenerate 0 to 3 hyperpersonalized suggestions for what to do next"),
            1_001,
        ))
        .unwrap();
    assert!(!overview.inserted);
    assert!(overview.suppressed);
    let snapshot = store.snapshot().unwrap();
    assert!(snapshot.sessions.is_empty());
    assert_eq!(snapshot.event_count, 0);
    assert_eq!(snapshot.metrics.sessions_observed, 0);

    let tool = store
        .ingest(codex_app_event("PreToolUse", overview_session, None, 1_002))
        .unwrap();
    assert!(tool.suppressed);
    drop(store);

    let store = RuntimeStore::open(&database).unwrap();
    let stop = store
        .ingest(codex_app_event("Stop", overview_session, None, 1_003))
        .unwrap();
    assert!(stop.suppressed);
    assert!(store.snapshot().unwrap().sessions.is_empty());

    let safety_session = "019f7eb8-d444-7f42-bd55-3ceffa0ae86b";
    store
        .ingest(codex_app_event("SessionStart", safety_session, None, 2_000))
        .unwrap();
    let safety = store
        .ingest(codex_app_event(
            "UserPromptSubmit",
            safety_session,
            Some("You are an expert at upholding safety and compliance standards for tool calls"),
            2_001,
        ))
        .unwrap();
    assert!(safety.suppressed);

    let metadata_free_session = "019f7eb9-3aa1-7d18-a1ad-a6be93b75058";
    let mut metadata_free = codex_app_event(
        "UserPromptSubmit",
        metadata_free_session,
        Some("# Overview Generate 0 to 3 hyperpersonalized suggestions for what to do next"),
        2_500,
    );
    metadata_free.term = None;
    metadata_free.raw["cwd"] = Value::String("/Users/test/project".to_owned());
    let metadata_free = store.ingest(metadata_free).unwrap();
    assert!(metadata_free.suppressed);

    let executable_probe_session = "01a03d36-180a-7c12-b9e1-b0ac764daea7";
    store
        .ingest(codex_app_event(
            "SessionStart",
            executable_probe_session,
            None,
            2_600,
        ))
        .unwrap();
    let executable_probe = store
        .ingest(codex_app_event(
            "UserPromptSubmit",
            executable_probe_session,
            Some("/opt/homebrew/bin/codex"),
            2_601,
        ))
        .unwrap();
    assert!(executable_probe.suppressed);

    let hooks_command_session = "01a03d79-e01e-7aae-bcbf-d409133a39a8";
    let hooks_command = store
        .ingest(codex_app_event(
            "UserPromptSubmit",
            hooks_command_session,
            Some("/hooks"),
            2_700,
        ))
        .unwrap();
    assert!(hooks_command.suppressed);

    let markdown_hooks_session = "01a03d4c-7f7d-7541-8bf8-dfd182ae5f39";
    let markdown_hooks = store
        .ingest(codex_app_event(
            "UserPromptSubmit",
            markdown_hooks_session,
            Some("`/hooks`"),
            2_800,
        ))
        .unwrap();
    assert!(markdown_hooks.suppressed);

    let real_session = "real-codex-app-root-session";
    let real = store
        .ingest(codex_app_event(
            "UserPromptSubmit",
            real_session,
            Some("Help me inspect the current project"),
            3_000,
        ))
        .unwrap();
    assert!(real.inserted);
    assert!(!real.suppressed);
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.sessions.len(), 1);
    assert_eq!(snapshot.sessions[0].provider_session_id, real_session);
    assert_eq!(snapshot.metrics.sessions_observed, 1);
    drop(store);

    let connection = Connection::open(&database).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM ignored_provider_sessions",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        6
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'visibility'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    connection
        .execute(
            "INSERT INTO sessions(
               id, provider, provider_session_id, cwd, title,
               exec_state, started_at, last_event_at
             ) VALUES (
               'legacy-internal-id', 'codex', 'legacy-internal-session', '/Users/test/project',
               '# Overview Generate 0 to 3 hyperpersonalized suggestions for what to do next',
               'response_finished', 4000, 4100
             )",
            [],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE metrics_daily
             SET sessions_observed = sessions_observed + 1",
            [],
        )
        .unwrap();
    drop(connection);

    let store = RuntimeStore::open(&database).unwrap();
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.sessions.len(), 1);
    assert_eq!(snapshot.sessions[0].provider_session_id, real_session);
    assert_eq!(snapshot.metrics.sessions_observed, 1);
    drop(store);

    let connection = Connection::open(&database).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM ignored_provider_sessions",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        7
    );
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
            project_id: None,
            project_label: None,
            parent_provider_session_id: None,
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
            daily_usage: Vec::new(),
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
fn model_switch_uses_current_turn_usage_and_new_hook_metadata_for_both_providers() {
    for (provider, key, old, new) in [
        (Provider::Codex, "codex", "gpt-5.6-sol", "gpt-6-astra"),
        (
            Provider::Claude,
            "claude",
            "claude-sonnet-4",
            "claude-opus-4-6",
        ),
    ] {
        let root = temp_root(&format!("model-switch-{key}"));
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let event = |name: &str, turn: &str, model: &str, at| {
            BridgeRequest::from_hook_at(
                provider,
                json!({
                    "hook_event_name": name, "session_id": "model-switch-session", "turn_id": turn,
                    "model": model, "prompt": "Continue model switch test", "tool_name": "Bash", "tool_input": {}
                }),
                at,
            )
        };
        store
            .ingest(event("UserPromptSubmit", "turn-1", old, 100))
            .unwrap();
        assert_eq!(
            store.snapshot().unwrap().sessions[0].model.as_deref(),
            Some(old)
        );
        let mut usage = usage_record("model-switch-session", 200, 100, Some(500), "derived");
        usage.provider = key.to_owned();
        usage.model = Some(new.to_owned());
        store.upsert_session_usage(usage).unwrap();
        store
            .ingest(event("PreToolUse", "turn-1", old, 250))
            .unwrap();
        assert_eq!(
            store.snapshot().unwrap().sessions[0].model.as_deref(),
            Some(new)
        );
        store.ingest(event("Stop", "turn-1", old, 300)).unwrap();
        assert_eq!(
            store.snapshot().unwrap().sessions[0].model.as_deref(),
            Some(new)
        );
        store
            .ingest(event("UserPromptSubmit", "turn-2", "future-model", 400))
            .unwrap();
        // Old usage must not override a newer turn before its first usage sample.
        assert_eq!(
            store.snapshot().unwrap().sessions[0].model.as_deref(),
            Some("future-model")
        );
        let mut usage = usage_record("model-switch-session", 450, 150, None, "derived");
        usage.provider = key.to_owned();
        usage.model = Some("future-model".to_owned());
        store.upsert_session_usage(usage).unwrap();
        assert_eq!(
            store.snapshot().unwrap().sessions[0].model.as_deref(),
            Some("future-model")
        );
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn token_decision_attributes_ledger_and_uses_only_live_sample_deltas_for_burn_rate() {
    let root = temp_root("token-decision");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"UserPromptSubmit",
                "session_id":"decision-session",
                "turn_id":"decision-turn",
                "cwd":"/tmp/decision-project",
                "prompt":"Measure this task without persisting the private prompt"
            }),
            now,
        ))
        .unwrap();
    store
        .upsert_session_usage(usage_record(
            "decision-session",
            now,
            1_000,
            None,
            "official_local",
        ))
        .unwrap();
    let project_id = format!("sha256:{}", "a".repeat(64));
    let mut child = usage_record("child-history", now, 500, None, "derived");
    child.project_id = Some(project_id.clone());
    child.project_label = Some("decision-project".to_owned());
    child.parent_provider_session_id = Some("decision-session".to_owned());
    store.upsert_session_usage(child).unwrap();
    let mut grandchild = usage_record("grandchild-history", now, 50, None, "derived");
    grandchild.project_id = Some(project_id.clone());
    grandchild.project_label = Some("decision-project".to_owned());
    grandchild.parent_provider_session_id = Some("child-history".to_owned());
    store.upsert_session_usage(grandchild).unwrap();
    let mut project_history = usage_record("project-history", now, 250, None, "derived");
    project_history.project_id = Some(project_id);
    project_history.project_label = Some("decision-project".to_owned());
    store.upsert_session_usage(project_history).unwrap();
    store
        .upsert_session_usage(usage_record(
            "unassigned-history",
            now,
            100,
            None,
            "partial",
        ))
        .unwrap();
    let collecting = store.token_usage_decision(now, Some(50_000)).unwrap();
    assert_eq!(collecting.schema_version, 2);
    assert_eq!(collecting.total_tokens, 1_900);
    assert_eq!(collecting.attributed_tokens, 1_550);
    assert_eq!(collecting.unattributed_tokens, 350);
    assert_eq!(collecting.attribution_coverage_basis_points, 8_157);
    assert_eq!(collecting.task_attributed_tokens, 1_550);
    assert_eq!(collecting.task_unattributed_tokens, 350);
    assert_eq!(collecting.task_attribution_coverage_basis_points, 8_157);
    assert_eq!(collecting.project_attributed_tokens, 1_800);
    assert_eq!(collecting.project_unattributed_tokens, 100);
    assert_eq!(collecting.project_attribution_coverage_basis_points, 9_473);
    assert_eq!(collecting.project_totals[0].project, "decision-project");
    assert_eq!(collecting.project_totals[0].total, 1_800);
    assert_eq!(collecting.project_totals[0].task_count, 1);
    assert_eq!(collecting.project_totals[0].session_count, 4);
    assert_eq!(collecting.task_totals[0].total, 1_550);
    assert_eq!(collecting.burn_rates[0].state, "collecting");
    assert!(!collecting.burn_rates[0].threshold_exceeded);

    store
        .upsert_session_usage(usage_record(
            "decision-session",
            now + 6_000,
            7_000,
            None,
            "official_local",
        ))
        .unwrap();
    let decision = store
        .token_usage_decision(now + 6_000, Some(50_000))
        .unwrap();
    assert_eq!(decision.total_tokens, 7_900);
    assert_eq!(decision.task_totals[0].total, 7_550);
    assert_eq!(decision.project_totals[0].total, 7_800);
    assert_eq!(decision.burn_rates[0].window_seconds, 6);
    assert_eq!(decision.burn_rates[0].token_delta, 6_000);
    assert_eq!(decision.burn_rates[0].tokens_per_minute, 60_000);
    assert_eq!(decision.burn_rates[0].sample_count, 2);
    assert_eq!(decision.burn_rates[0].state, "normal");
    assert!(decision.burn_rates[0].threshold_exceeded);
    let encoded = serde_json::to_string(&decision).unwrap();
    assert!(!encoded.contains("/tmp/"));
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

fn usage_record(
    provider_session_id: &str,
    captured_at: u64,
    token_total: u64,
    estimated_cost_usd_micros: Option<u64>,
    usage_quality: &str,
) -> SessionUsageRecord {
    SessionUsageRecord {
        provider: "codex".to_owned(),
        provider_session_id: provider_session_id.to_owned(),
        project_id: None,
        project_label: None,
        parent_provider_session_id: None,
        model: Some("gpt-5.6-sol".to_owned()),
        input_tokens: Some(token_total),
        output_tokens: None,
        cache_read_tokens: None,
        cache_creation_tokens: None,
        reasoning_tokens: None,
        token_total: Some(token_total),
        last_turn_tokens: Some(token_total),
        context_used_tokens: Some(token_total),
        context_window_tokens: Some(1_000),
        context_used_percent: Some(10),
        estimated_cost_usd_micros,
        cost_kind: estimated_cost_usd_micros.map(|_| "computed".to_owned()),
        pricing_source: estimated_cost_usd_micros.map(|_| "test_pricing".to_owned()),
        usage_source: "codex_rollout".to_owned(),
        usage_quality: usage_quality.to_owned(),
        captured_at,
        daily_usage: Vec::new(),
    }
}

#[test]
fn token_usage_archive_counts_only_monotonic_session_deltas() {
    let root = temp_root("token-usage-archive");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    store
        .upsert_session_usage(usage_record("token-ledger", now, 100, None, "derived"))
        .unwrap();
    let mut observed_claude = usage_record("claude-observed", now, 0, None, "official");
    observed_claude.provider = "claude".to_owned();
    observed_claude.model = Some("claude-sonnet".to_owned());
    observed_claude.token_total = None;
    observed_claude.last_turn_tokens = None;
    observed_claude.context_used_tokens = None;
    store.upsert_session_usage(observed_claude).unwrap();
    let first = store.snapshot().unwrap().token_usage;
    assert_eq!((first.today, first.month, first.total), (100, 100, 100));
    assert_eq!(first.recorded_from, Some(now));
    assert_eq!(first.by_provider.len(), 2);
    assert_eq!(first.by_provider[0].provider, "codex");
    assert_eq!(first.by_provider[0].today, 100);
    assert_eq!(first.by_provider[0].month, 100);
    assert_eq!(first.by_provider[0].total, 100);
    assert_eq!(first.by_provider[1].provider, "claude");
    assert_eq!(first.by_provider[1].total, 0);
    assert_eq!(first.active_days, 1);
    assert_eq!(first.current_streak, 1);
    assert_eq!(first.by_model.len(), 1);
    assert_eq!(first.by_model[0].model, "gpt-5.6-sol");
    assert_eq!(first.by_model[0].total, 100);
    assert_eq!(first.recent_days.len(), 1);
    assert_eq!(first.recent_days[0].total, 100);
    assert_eq!(first.recent_days[0].by_provider[0].provider, "codex");
    assert_eq!(first.recent_days[0].by_model[0].model, "gpt-5.6-sol");
    assert_eq!(first.detail_recorded_from, Some(now));

    store
        .upsert_session_usage(usage_record("token-ledger", now + 1, 100, None, "derived"))
        .unwrap();
    store.replace_session_usages(vec![], now + 2).unwrap();
    store
        .upsert_session_usage(usage_record("token-ledger", now + 3, 20, None, "partial"))
        .unwrap();
    assert_eq!(store.snapshot().unwrap().token_usage.total, 100);

    store
        .upsert_session_usage(usage_record("token-ledger", now + 4, 175, None, "derived"))
        .unwrap();
    let final_totals = store.snapshot().unwrap().token_usage;
    assert_eq!(
        (final_totals.today, final_totals.month, final_totals.total),
        (175, 175, 175)
    );
    assert_eq!(final_totals.captured_at, Some(now + 4));
    assert_eq!(final_totals.recent_days[0].by_model[0].total, 175);

    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_token_cursor_baselines_new_detail_fields_without_backfilling_history() {
    let root = temp_root("token-detail-migration");
    fs::create_dir_all(&root).unwrap();
    let database = root.join("data.sqlite");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE token_usage_cursors (
               provider TEXT NOT NULL,
               provider_session_id TEXT NOT NULL,
               token_total INTEGER NOT NULL,
               captured_at INTEGER NOT NULL,
               PRIMARY KEY(provider, provider_session_id)
             );
             CREATE TABLE token_usage_daily (
               day TEXT NOT NULL,
               provider TEXT NOT NULL,
               token_total INTEGER NOT NULL DEFAULT 0,
               captured_at INTEGER NOT NULL,
               PRIMARY KEY(day, provider)
             );
             INSERT INTO token_usage_cursors VALUES ('codex', 'legacy', 100, 1000);",
        )
        .unwrap();
    drop(connection);

    let store = RuntimeStore::open(&database).unwrap();
    store
        .upsert_session_usage(usage_record("legacy", 1_001, 100, None, "derived"))
        .unwrap();
    assert!(store.snapshot().unwrap().token_usage.recent_days.is_empty());

    store
        .upsert_session_usage(usage_record("legacy", 1_002, 150, None, "derived"))
        .unwrap();
    let totals = store.snapshot().unwrap().token_usage;
    assert_eq!(totals.total, 50);
    assert_eq!(totals.recent_days[0].by_model[0].total, 50);
    assert_eq!(
        totals.recent_days[0].by_model[0].estimated_cost_usd_micros,
        None
    );

    drop(store);
    let connection = rusqlite::Connection::open(&database).unwrap();
    let cursor_columns = connection
        .prepare("PRAGMA table_info(token_usage_cursors)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    for expected in ["project_id", "project_label", "parent_provider_session_id"] {
        assert!(cursor_columns.iter().any(|column| column == expected));
    }
    drop(connection);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn session_day_history_is_idempotent_and_drives_dashboard_breakdowns() {
    let root = temp_root("token-session-day-history");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let mut record = usage_record("daily-history", now, 150, Some(750), "derived");
    record.daily_usage = vec![SessionUsageDailyRecord {
        day: "2026-08-10".to_owned(),
        model: Some("gpt-5.6-sol".to_owned()),
        input_tokens: 100,
        output_tokens: 20,
        cache_read_tokens: 30,
        cache_creation_tokens: 0,
        reasoning_tokens: 5,
        token_total: 150,
        estimated_cost_usd_micros: Some(750),
        cost_kind: Some("computed".to_owned()),
        pricing_source: Some("openai_standard_2026-07-20".to_owned()),
        message_count: 2,
    }];
    store.upsert_session_usage(record.clone()).unwrap();
    store.upsert_session_usage(record).unwrap();

    let totals = store.snapshot().unwrap().token_usage;
    assert_eq!(totals.active_days, 1);
    assert_eq!(totals.message_count, 2);
    assert_eq!(totals.peak_day.as_deref(), Some("2026-08-10"));
    assert_eq!(totals.peak_day_total, 150);
    assert_eq!(totals.recent_days.len(), 1);
    assert_eq!(totals.recent_days[0].estimated_cost_usd_micros, Some(750));
    assert_eq!(totals.recent_days[0].message_count, 2);
    assert_eq!(totals.recent_days[0].by_provider[0].message_count, 2);
    assert_eq!(totals.recent_days[0].by_model[0].input_tokens, Some(100));
    assert_eq!(totals.recent_days[0].by_model[0].reasoning_tokens, Some(5));
    assert_eq!(totals.by_model[0].total, 150);

    let mut corrected = usage_record("daily-history", now + 1, 90, Some(450), "derived");
    corrected.daily_usage = vec![SessionUsageDailyRecord {
        day: "2026-08-10".to_owned(),
        model: Some("gpt-5.6-terra".to_owned()),
        input_tokens: 60,
        output_tokens: 10,
        cache_read_tokens: 20,
        cache_creation_tokens: 0,
        reasoning_tokens: 3,
        token_total: 90,
        estimated_cost_usd_micros: Some(450),
        cost_kind: Some("computed".to_owned()),
        pricing_source: Some("openai_standard_2026-07-20".to_owned()),
        message_count: 1,
    }];
    store.upsert_session_usage(corrected).unwrap();
    let corrected_totals = store.snapshot().unwrap().token_usage;
    assert_eq!(corrected_totals.total, 90);
    assert_eq!(corrected_totals.message_count, 1);
    assert_eq!(
        corrected_totals.recent_days[0].estimated_cost_usd_micros,
        Some(450)
    );
    assert_eq!(corrected_totals.recent_days[0].by_model.len(), 1);
    assert_eq!(
        corrected_totals.recent_days[0].by_model[0].model,
        "gpt-5.6-terra"
    );
    assert_eq!(corrected_totals.by_model.len(), 1);

    let mut unpriced = usage_record("unpriced-history", now + 2, 10, None, "derived");
    unpriced.daily_usage = vec![SessionUsageDailyRecord {
        day: "2026-08-10".to_owned(),
        model: Some("future-model".to_owned()),
        input_tokens: 8,
        output_tokens: 2,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        reasoning_tokens: 0,
        token_total: 10,
        estimated_cost_usd_micros: None,
        cost_kind: None,
        pricing_source: None,
        message_count: 1,
    }];
    store.upsert_session_usage(unpriced).unwrap();
    let partial_cost = store.snapshot().unwrap().token_usage;
    assert_eq!(partial_cost.total, 100);
    assert_eq!(partial_cost.priced_tokens, 90);
    assert_eq!(partial_cost.unpriced_tokens, 10);
    assert_eq!(partial_cost.estimated_cost_usd_micros, Some(450));
    assert_eq!(partial_cost.recent_days[0].priced_tokens, 90);
    assert_eq!(partial_cost.recent_days[0].unpriced_tokens, 10);
    assert_eq!(
        partial_cost.recent_days[0].estimated_cost_usd_micros,
        Some(450)
    );

    let numeric_json = store.export_token_usage_json(now + 3).unwrap();
    assert_eq!(numeric_json["scope"], "token_usage_numeric");
    assert_eq!(numeric_json["dataQuality"], "not_evaluated");
    assert_eq!(numeric_json["totals"]["tokenTotal"], 100);
    assert_eq!(numeric_json["totals"]["pricedTokens"], 90);
    assert_eq!(numeric_json["totals"]["unpricedTokens"], 10);
    assert_eq!(numeric_json["totals"]["estimatedCostUsdMicros"], 450);
    assert_eq!(numeric_json["totals"]["costStatus"], "partial");
    assert_eq!(numeric_json["totals"]["agentExecutionTimeSeconds"], 0);
    assert_eq!(
        numeric_json["semantics"]["agentExecutionTime"],
        "Runtime intervals in thinking, tool_running, or compacting states only; user waiting is excluded and concurrent Agents are summed."
    );
    assert_eq!(numeric_json["daily"].as_array().unwrap().len(), 2);
    let encoded_numeric = serde_json::to_string(&numeric_json).unwrap();
    for private_value in ["daily-history", "unpriced-history"] {
        assert!(!encoded_numeric.contains(private_value));
    }
    for row in numeric_json["daily"].as_array().unwrap() {
        let object = row.as_object().unwrap();
        for private_field in [
            "sessionId",
            "prompt",
            "path",
            "command",
            "toolContent",
            "response",
        ] {
            assert!(!object.contains_key(private_field));
        }
    }

    let numeric_csv = store.export_token_usage_csv(now + 3).unwrap();
    assert!(numeric_csv.starts_with("day,provider,model,input_tokens"));
    assert!(numeric_csv.contains("2026-08-10,codex,future-model,8,2"));
    assert!(numeric_csv.contains("2026-08-10,codex,gpt-5.6-terra,60,10"));
    assert!(!numeric_csv.contains("daily-history"));
    assert!(!numeric_csv.contains("unpriced-history"));

    let connection = Connection::open(root.join("data.sqlite")).unwrap();
    let reconciled: (i64, i64, i64) = connection
        .query_row(
            "SELECT
               (SELECT SUM(token_total) FROM token_usage_daily),
               (SELECT SUM(token_total) FROM token_usage_session_days),
               (SELECT SUM(token_total) FROM token_usage_daily_models)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(reconciled, (100, 100, 100));
    drop(connection);

    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn historical_computed_cost_is_frozen_until_facts_or_provider_cost_changes() {
    let root = temp_root("token-price-freeze");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let daily = |cost, kind: &str, source: &str| SessionUsageDailyRecord {
        day: "2026-08-11".to_owned(),
        model: Some("gpt-5.6-sol".to_owned()),
        input_tokens: 80,
        output_tokens: 20,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        reasoning_tokens: 4,
        token_total: 100,
        estimated_cost_usd_micros: Some(cost),
        cost_kind: Some(kind.to_owned()),
        pricing_source: Some(source.to_owned()),
        message_count: 1,
    };

    let mut original = usage_record("price-freeze", now, 100, Some(500), "derived");
    original.daily_usage = vec![daily(500, "computed", "price_snapshot_v1")];
    store.upsert_session_usage(original).unwrap();

    let mut silently_repriced = usage_record("price-freeze", now + 1, 100, Some(900), "derived");
    silently_repriced.daily_usage = vec![daily(900, "computed", "price_snapshot_v2")];
    store.upsert_session_usage(silently_repriced).unwrap();
    let frozen = store.snapshot().unwrap().token_usage;
    assert_eq!(frozen.estimated_cost_usd_micros, Some(500));
    assert_eq!(frozen.pricing_sources.len(), 1);
    assert_eq!(frozen.pricing_sources[0].source, "price_snapshot_v1");

    let mut provider_reported = usage_record("price-freeze", now + 2, 100, Some(700), "derived");
    provider_reported.daily_usage =
        vec![daily(700, "provider_estimate", "provider_transcript_cost")];
    store.upsert_session_usage(provider_reported).unwrap();
    let upgraded = store.snapshot().unwrap().token_usage;
    assert_eq!(upgraded.estimated_cost_usd_micros, Some(700));
    assert_eq!(upgraded.pricing_sources[0].cost_kind, "provider_estimate");
    assert_eq!(
        upgraded.pricing_sources[0].source,
        "provider_transcript_cost"
    );

    let mut zero = usage_record("zero-price-source", now + 3, 0, Some(0), "derived");
    zero.daily_usage = vec![SessionUsageDailyRecord {
        day: "2026-08-11".to_owned(),
        model: Some("claude-sonnet-5".to_owned()),
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        reasoning_tokens: 0,
        token_total: 0,
        estimated_cost_usd_micros: Some(0),
        cost_kind: Some("provider_estimate".to_owned()),
        pricing_source: Some("zero_value_source".to_owned()),
        message_count: 1,
    }];
    store.upsert_session_usage(zero).unwrap();
    assert_eq!(
        store.snapshot().unwrap().token_usage.pricing_sources.len(),
        1
    );

    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn newly_known_catalog_price_fills_only_previously_unpriced_history() {
    let root = temp_root("token-price-backfill");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let daily = |cost: Option<u64>, source: Option<&str>| SessionUsageDailyRecord {
        day: "2026-08-12".to_owned(),
        model: Some("claude-opus-5".to_owned()),
        input_tokens: 80,
        output_tokens: 20,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        reasoning_tokens: 0,
        token_total: 100,
        estimated_cost_usd_micros: cost,
        cost_kind: cost.map(|_| "computed".to_owned()),
        pricing_source: source.map(ToOwned::to_owned),
        message_count: 1,
    };

    let mut unknown = usage_record("price-backfill", now, 100, None, "derived");
    unknown.daily_usage = vec![daily(None, None)];
    store.upsert_session_usage(unknown).unwrap();
    assert_eq!(store.snapshot().unwrap().token_usage.unpriced_tokens, 100);

    let mut newly_priced = usage_record("price-backfill", now + 1, 100, Some(900), "derived");
    newly_priced.daily_usage = vec![daily(Some(900), Some("models_dev_api_fixture"))];
    store.upsert_session_usage(newly_priced).unwrap();

    let backfilled = store.snapshot().unwrap().token_usage;
    assert_eq!(backfilled.priced_tokens, 100);
    assert_eq!(backfilled.unpriced_tokens, 0);
    assert_eq!(backfilled.estimated_cost_usd_micros, Some(900));
    assert_eq!(
        backfilled.pricing_sources[0].source,
        "models_dev_api_fixture"
    );

    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_daily_cost_migration_marks_the_original_source_as_unknown_and_frozen() {
    let root = temp_root("token-price-source-migration");
    fs::create_dir_all(&root).unwrap();
    let database = root.join("data.sqlite");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE token_usage_session_days (
               provider TEXT NOT NULL,
               provider_session_id TEXT NOT NULL,
               day TEXT NOT NULL,
               model TEXT NOT NULL,
               input_tokens INTEGER NOT NULL DEFAULT 0,
               output_tokens INTEGER NOT NULL DEFAULT 0,
               cache_read_tokens INTEGER NOT NULL DEFAULT 0,
               cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
               reasoning_tokens INTEGER NOT NULL DEFAULT 0,
               token_total INTEGER NOT NULL DEFAULT 0,
               estimated_cost_usd_micros INTEGER,
               message_count INTEGER NOT NULL DEFAULT 0,
               captured_at INTEGER NOT NULL,
               PRIMARY KEY(provider, provider_session_id, day, model)
             );
             INSERT INTO token_usage_session_days VALUES (
               'codex', 'legacy-session', '2026-08-12', 'gpt-5.6-sol',
               80, 20, 0, 0, 4, 100, 500, 1, 1000
             );
             PRAGMA user_version = 28;",
        )
        .unwrap();
    drop(connection);

    let store = RuntimeStore::open(&database).unwrap();
    let totals = store.snapshot().unwrap().token_usage;
    assert_eq!(totals.estimated_cost_usd_micros, Some(500));
    assert_eq!(totals.pricing_sources[0].cost_kind, "legacy_unclassified");
    assert_eq!(totals.pricing_sources[0].source, "legacy_pre_v29");

    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn token_audit_explains_suspect_days_and_excludes_them_from_peak_claims() {
    let root = temp_root("token-audit-suspect-days");
    let database = root.join("data.sqlite");
    let store = RuntimeStore::open(&database).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let mut record = usage_record("audit-days", now, 2_000_001_700, None, "derived");
    record.daily_usage = (1..=7)
        .map(|day| SessionUsageDailyRecord {
            day: format!("2026-08-{day:02}"),
            model: Some("future-model".to_owned()),
            input_tokens: 80,
            output_tokens: 20,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            reasoning_tokens: 0,
            token_total: 100,
            estimated_cost_usd_micros: None,
            cost_kind: None,
            pricing_source: None,
            message_count: 1,
        })
        .chain(std::iter::once(SessionUsageDailyRecord {
            day: "2026-08-08".to_owned(),
            model: Some("future-model".to_owned()),
            input_tokens: 2_000_000_000,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            reasoning_tokens: 0,
            token_total: 2_000_000_000,
            estimated_cost_usd_micros: None,
            cost_kind: None,
            pricing_source: None,
            message_count: 1,
        }))
        .chain(std::iter::once(SessionUsageDailyRecord {
            day: "2999-01-01".to_owned(),
            model: Some("future-model".to_owned()),
            input_tokens: 1_000,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            reasoning_tokens: 0,
            token_total: 1_000,
            estimated_cost_usd_micros: None,
            cost_kind: None,
            pricing_source: None,
            message_count: 1,
        }))
        .collect();
    store.upsert_session_usage(record).unwrap();

    let audited = store.snapshot().unwrap().token_usage;
    assert_eq!(audited.suspect_count, 2);
    assert!(audited
        .anomalies
        .iter()
        .any(|anomaly| anomaly.code == "extreme_daily_jump"));
    assert!(audited.anomalies.iter().any(|anomaly| {
        anomaly.code == "future_usage_day" && anomaly.day.as_deref() == Some("2999-01-01")
    }));
    assert_eq!(audited.peak_day.as_deref(), Some("2026-08-07"));
    assert_eq!(audited.peak_day_total, 100);

    drop(store);
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute(
            "UPDATE token_usage_daily SET token_total = token_total + 1
             WHERE day = '2026-08-01'",
            [],
        )
        .unwrap();
    drop(connection);
    let store = RuntimeStore::open(&database).unwrap();
    let mismatched = store.snapshot().unwrap().token_usage;
    assert!(mismatched
        .anomalies
        .iter()
        .any(|anomaly| anomaly.code == "daily_projection_mismatch"));

    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn agent_time_uses_observed_turn_activity_instead_of_wall_clock_age() {
    let root = temp_root("token-agent-time");
    fs::create_dir_all(&root).unwrap();
    let database = root.join("data.sqlite");
    RuntimeStore::open(&database).unwrap();

    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "INSERT INTO sessions(
               id, provider, provider_session_id, exec_state, started_at, last_event_at
             ) VALUES ('session', 'codex', 'provider-session', 'thinking', 1000, 61000);
             INSERT INTO turns(
               id, session_id, provider_turn_id, ordinal, state, started_at
             ) VALUES ('turn', 'session', 'provider-turn', 1, 'running', 1000);
             INSERT INTO events(
               id, session_id, turn_id, provider, type, occurred_at, ingest_seq
             ) VALUES ('event', 'session', 'turn', 'codex', 'tool_started', 61000, 1);",
        )
        .unwrap();
    drop(connection);

    let store = RuntimeStore::open(&database).unwrap();
    assert_eq!(
        store.snapshot().unwrap().token_usage.active_time_seconds,
        60
    );
    let period_totals = store.snapshot().unwrap().token_usage;
    assert_eq!(period_totals.today_active_time_seconds, 0);
    assert_eq!(period_totals.month_active_time_seconds, 0);

    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn agent_execution_time_excludes_user_waiting_and_keeps_tool_time_continuous() {
    let root = temp_root("agent-execution-time");
    let database = root.join("data.sqlite");
    let store = RuntimeStore::open(&database).unwrap();
    for (event, tool, at) in [
        ("UserPromptSubmit", None, 1_000),
        ("PermissionRequest", Some("git push"), 11_000),
        ("PermissionDenied", None, 51_000),
        ("PreToolUse", Some("cargo test"), 61_000),
        ("Stop", None, 71_000),
    ] {
        store
            .ingest(request_at(
                Provider::Claude,
                event,
                "execution-session",
                Some("turn-1"),
                tool,
                at,
            ))
            .unwrap();
    }

    let totals = store.snapshot().unwrap().token_usage;
    assert_eq!(totals.active_time_seconds, 70);
    assert_eq!(totals.execution_time_seconds, 30);
    assert!(totals.execution_time_seconds < totals.active_time_seconds);

    drop(store);
    let connection = Connection::open(&database).unwrap();
    let intervals = connection
        .prepare(
            "SELECT started_at, ended_at FROM agent_execution_intervals
             ORDER BY started_at",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(intervals, vec![(1_000, 11_000), (51_000, 71_000)]);
    connection
        .execute_batch(
            "DROP TABLE agent_execution_intervals;
             PRAGMA user_version = 29;",
        )
        .unwrap();
    drop(connection);

    let migrated = RuntimeStore::open(&database).unwrap();
    assert_eq!(
        migrated
            .snapshot()
            .unwrap()
            .token_usage
            .execution_time_seconds,
        30
    );
    drop(migrated);
    let connection = Connection::open(&database).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        38
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM agent_execution_intervals
                 WHERE start_reason = 'historical_event_rebuild'
                   AND end_reason = 'historical_boundary'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        2
    );
    drop(connection);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn partial_usage_refresh_clears_persisted_complete_cost_and_unavailable_rows() {
    let root = temp_root("usage-partial-invalidation");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({"hook_event_name":"SessionStart","session_id":"usage-partial"}),
            1,
        ))
        .unwrap();
    store
        .upsert_session_usage(usage_record(
            "usage-partial",
            10,
            1_000,
            Some(9_999),
            "official_local",
        ))
        .unwrap();
    store
        .replace_session_usages(
            vec![usage_record("usage-partial", 11, 100, None, "partial")],
            11,
        )
        .unwrap();
    let partial = store.snapshot().unwrap().sessions.remove(0);
    assert_eq!(partial.token_total, Some(100));
    assert_eq!(partial.estimated_cost_usd_micros, None);
    assert_eq!(partial.cost_kind, None);
    assert_eq!(partial.pricing_source, None);
    assert_eq!(partial.usage_quality.as_deref(), Some("partial"));

    store.replace_session_usages(vec![], 12).unwrap();
    let unavailable = store.snapshot().unwrap().sessions.remove(0);
    assert_eq!(unavailable.usage_source, None);
    assert_eq!(unavailable.usage_quality, None);
    assert_eq!(unavailable.estimated_cost_usd_micros, None);
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn usage_refresh_rejects_stale_results_and_cannot_recreate_retained_rows() {
    let root = temp_root("usage-refresh-ordering");
    let database = root.join("data.sqlite");
    let store = RuntimeStore::open(&database).unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({"hook_event_name":"SessionStart","session_id":"usage-guard"}),
            1,
        ))
        .unwrap();
    let stale_generation = store.begin_usage_collection_generation().unwrap();
    let current_generation = store.begin_usage_collection_generation().unwrap();
    assert!(store
        .replace_session_usages_for_generation(
            vec![usage_record(
                "usage-guard",
                1_000,
                1_000,
                Some(1_000),
                "official_local",
            )],
            1_000,
            current_generation,
        )
        .unwrap());
    assert!(!store
        .replace_session_usages_for_generation(
            vec![usage_record("usage-guard", 1_000, 99, None, "partial")],
            1_000,
            stale_generation,
        )
        .unwrap());
    let guarded = store.snapshot().unwrap().sessions.remove(0);
    assert_eq!(guarded.token_total, Some(1_000));
    assert_eq!(guarded.estimated_cost_usd_micros, Some(1_000));

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let expired_at = now.saturating_sub(31 * 24 * 60 * 60 * 1_000);
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({"hook_event_name":"SessionStart","session_id":"retained-usage"}),
            expired_at,
        ))
        .unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({"hook_event_name":"Stop","session_id":"retained-usage"}),
            expired_at.saturating_add(1),
        ))
        .unwrap();
    store.prune_events(30, now).unwrap();
    let generation = store.begin_usage_collection_generation().unwrap();
    assert!(store
        .replace_session_usages_for_generation(
            vec![usage_record(
                "retained-usage",
                now,
                500,
                Some(500),
                "official_local",
            )],
            now,
            generation,
        )
        .unwrap());
    drop(store);

    let connection = Connection::open(&database).unwrap();
    let orphan_count = connection
        .query_row(
            "SELECT COUNT(*) FROM session_usage WHERE provider_session_id = 'retained-usage'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(orphan_count, 0);
    drop(connection);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn usage_refresh_generation_orders_equal_millisecond_collections() {
    let root = temp_root("usage-generation-ordering");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({"hook_event_name":"SessionStart","session_id":"generation-usage"}),
            1,
        ))
        .unwrap();

    let older = store.begin_usage_collection_generation().unwrap();
    let newer = store.begin_usage_collection_generation().unwrap();
    assert!(store
        .replace_session_usages_for_generation(
            vec![usage_record(
                "generation-usage",
                1_000,
                200,
                Some(200),
                "complete"
            )],
            1_000,
            newer,
        )
        .unwrap());
    assert!(!store
        .replace_session_usages_for_generation(
            vec![usage_record(
                "generation-usage",
                1_000,
                100,
                None,
                "partial"
            )],
            1_000,
            older,
        )
        .unwrap());
    let current = store.snapshot().unwrap().sessions.remove(0);
    assert_eq!(current.token_total, Some(200));
    assert_eq!(current.estimated_cost_usd_micros, Some(200));

    let latest = store.begin_usage_collection_generation().unwrap();
    assert!(store
        .replace_session_usages_for_generation(
            vec![usage_record("generation-usage", 1, 300, None, "partial")],
            1_000,
            latest,
        )
        .unwrap());
    let latest_snapshot = store.snapshot().unwrap().sessions.remove(0);
    assert_eq!(latest_snapshot.token_total, Some(300));
    assert_eq!(latest_snapshot.estimated_cost_usd_micros, None);
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn caught_up_generation_replaces_inflated_partial_daily_rows() {
    let root = temp_root("usage-partial-generation-replacement");
    let database = root.join("data.sqlite");
    let store = RuntimeStore::open(&database).unwrap();
    let daily = |day: &str, tokens: u64| SessionUsageDailyRecord {
        day: day.to_owned(),
        model: Some("gpt-5.6-sol".to_owned()),
        input_tokens: tokens,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        reasoning_tokens: 0,
        token_total: tokens,
        estimated_cost_usd_micros: None,
        cost_kind: None,
        pricing_source: None,
        message_count: 1,
    };
    let mut inflated = usage_record("partial-rebuild", 1_000, 600, None, "partial");
    inflated.daily_usage = vec![daily("2026-08-25", 100), daily("2026-08-26", 500)];
    store.replace_session_usages(vec![inflated], 1_000).unwrap();
    assert_eq!(store.snapshot().unwrap().token_usage.total, 600);

    let mut corrected = usage_record("partial-rebuild", 2_000, 20, None, "partial");
    corrected.daily_usage = vec![daily("2026-08-26", 20)];
    let generation = store.begin_usage_collection_generation().unwrap();
    assert!(store
        .replace_session_usages_for_generation(vec![corrected], 2_000, generation)
        .unwrap());
    let totals = store.snapshot().unwrap().token_usage;
    assert_eq!(totals.total, 20);
    assert_eq!(totals.recent_days.len(), 1);
    assert_eq!(totals.recent_days[0].day, "2026-08-26");

    drop(store);
    let connection = Connection::open(&database).unwrap();
    let canonical: (i64, i64) = connection
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(token_total), 0)
             FROM token_usage_session_days",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(canonical, (1, 20));
    drop(connection);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_usage_upsert_preserves_nullable_values_and_allows_orphans() {
    let root = temp_root("legacy-usage-upsert");
    let database = root.join("data.sqlite");
    let store = RuntimeStore::open(&database).unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({"hook_event_name":"SessionStart","session_id":"legacy-usage"}),
            1,
        ))
        .unwrap();
    store
        .upsert_session_usage(usage_record(
            "legacy-usage",
            1_000,
            1_000,
            Some(1_000),
            "official_local",
        ))
        .unwrap();
    let mut partial = usage_record("legacy-usage", 999, 1, None, "partial");
    partial.input_tokens = None;
    partial.token_total = None;
    partial.last_turn_tokens = None;
    partial.context_used_tokens = None;
    partial.context_window_tokens = None;
    partial.context_used_percent = None;
    store.upsert_session_usage(partial).unwrap();
    store
        .upsert_session_usage(usage_record("legacy-orphan", 1, 7, None, "partial"))
        .unwrap();
    drop(store);

    let connection = Connection::open(&database).unwrap();
    let (tokens, cost, source, quality, captured_at): (Option<i64>, Option<i64>, String, String, i64) =
        connection
            .query_row(
                "SELECT token_total, estimated_cost_usd_micros, usage_source, usage_quality, captured_at
                 FROM session_usage WHERE provider_session_id = 'legacy-usage'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .unwrap();
    assert_eq!(tokens, Some(1_000));
    assert_eq!(cost, Some(1_000));
    assert_eq!(source, "codex_rollout");
    assert_eq!(quality, "partial");
    assert_eq!(captured_at, 1_000);
    let orphan_count = connection
        .query_row(
            "SELECT COUNT(*) FROM session_usage WHERE provider_session_id = 'legacy-orphan'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(orphan_count, 1);
    drop(connection);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn atomic_usage_replacement_removes_orphan_when_no_row_is_affected() {
    let root = temp_root("replacement-orphan-invalidation");
    let database = root.join("data.sqlite");
    let store = RuntimeStore::open(&database).unwrap();
    store
        .upsert_session_usage(usage_record(
            "deleted-session",
            1_000,
            100,
            Some(100),
            "complete",
        ))
        .unwrap();
    let generation = store.begin_usage_collection_generation().unwrap();
    assert!(store
        .replace_session_usages_for_generation(
            vec![usage_record(
                "deleted-session",
                1_000,
                200,
                Some(200),
                "complete"
            )],
            1_000,
            generation,
        )
        .unwrap());
    drop(store);

    let connection = Connection::open(&database).unwrap();
    let orphan_count = connection
        .query_row(
            "SELECT COUNT(*) FROM session_usage WHERE provider_session_id = 'deleted-session'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(orphan_count, 0);
    drop(connection);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_usage_generation_rolls_back_canonical_ledger_and_projections_together() {
    let root = temp_root("usage-generation-atomic-rollback");
    let database = root.join("data.sqlite");
    let store = RuntimeStore::open(&database).unwrap();
    let mut baseline = usage_record("a-baseline", 1_000, 100, Some(500), "complete");
    baseline.daily_usage = vec![SessionUsageDailyRecord {
        day: "2026-08-17".to_owned(),
        model: Some("gpt-5.6-sol".to_owned()),
        input_tokens: 80,
        output_tokens: 20,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        reasoning_tokens: 4,
        token_total: 100,
        estimated_cost_usd_micros: Some(500),
        cost_kind: Some("computed".to_owned()),
        pricing_source: Some("test_pricing".to_owned()),
        message_count: 1,
    }];
    store
        .replace_session_usages(vec![baseline.clone()], 1_000)
        .unwrap();

    let mut corrected = baseline;
    corrected.token_total = Some(200);
    corrected.input_tokens = Some(200);
    corrected.daily_usage[0].input_tokens = 160;
    corrected.daily_usage[0].output_tokens = 40;
    corrected.daily_usage[0].token_total = 200;
    let mut invalid = usage_record("z-invalid", 2_000, 50, None, "complete");
    invalid.daily_usage = vec![SessionUsageDailyRecord {
        day: "not-a-day".to_owned(),
        model: Some("gpt-5.6-sol".to_owned()),
        input_tokens: 50,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        reasoning_tokens: 0,
        token_total: 50,
        estimated_cost_usd_micros: None,
        cost_kind: None,
        pricing_source: None,
        message_count: 1,
    }];
    let generation = store.begin_usage_collection_generation().unwrap();
    assert!(store
        .replace_session_usages_for_generation(vec![corrected, invalid], 2_000, generation,)
        .is_err());
    drop(store);

    let connection = Connection::open(&database).unwrap();
    for table in [
        "token_usage_session_days",
        "token_usage_daily",
        "token_usage_daily_models",
    ] {
        assert_eq!(
            connection
                .query_row(
                    &format!("SELECT SUM(token_total) FROM {table}"),
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            100,
            "{table} must remain on the previous committed generation"
        );
    }
    drop(connection);
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
fn delegated_task_title_projects_only_the_user_request_without_internal_ids() {
    let root = temp_root("delegated-task-title");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let private_thread = "private-source-thread-must-not-project";
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"UserPromptSubmit",
                "session_id":"delegated-title-session",
                "cwd":"/tmp/delegated-project",
                "prompt":format!(
                    "<codex_delegation><source_thread_id>{private_thread}</source_thread_id><input>Review H2.4 and README.md. Then finish.</input></codex_delegation><environment_context>private host context</environment_context>"
                )
            }),
            10_000,
        ))
        .unwrap();

    let snapshot = store.snapshot().unwrap();
    assert_eq!(
        snapshot.sessions[0].title.as_deref(),
        Some("Review H2.4 and README.md.")
    );
    let encoded = serde_json::to_string(&snapshot).unwrap();
    assert!(!encoded.contains(private_thread));
    assert!(!encoded.contains("codex_delegation"));
    assert!(!encoded.contains("environment_context"));
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
        assert!(serialized.contains("Open exact conversation"));
        assert!(!serialized.contains("private-window-id"));
        assert!(!serialized.contains("/dev/ttys999"));
        assert!(!serialized.contains("com.openai.codex"));
    }

    let reopened = RuntimeStore::open(&database).unwrap();
    let session = &reopened.snapshot().unwrap().sessions[0];
    assert_eq!(session.exec_state, "thinking");
    assert_eq!(session.turn_started_at, Some(50_000));
    assert_eq!(session.jump_capability, "exact_conversation");
    assert_eq!(session.jump_label, "Open exact conversation");
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
    assert_eq!(snapshot.attention.len(), 4);
    assert_eq!(
        snapshot
            .attention
            .iter()
            .filter(|item| item.kind == "completion" && item.state == "open")
            .count(),
        2
    );
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
fn provider_specific_undo_delay_can_commit_immediately_or_remain_undoable() {
    let root = temp_root("provider-undo-delay");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();

    let immediate = request_at(
        Provider::Claude,
        "PermissionRequest",
        "immediate-session",
        Some("turn-1"),
        Some("git push origin main"),
        1_000,
    );
    let immediate_request = immediate.request_id.unwrap();
    store.ingest(immediate).unwrap();
    let immediate_command = Uuid::now_v7();
    let claim = store
        .claim_approval_with_delay(
            immediate_command,
            immediate_request,
            ApprovalAction::Approve,
            2_000,
            0,
        )
        .unwrap();
    assert_eq!(claim.commit_due_at, Some(2_000));
    assert_eq!(
        store.commit(immediate_command, 2_000, true).unwrap().state,
        CommandState::DecisionSent
    );
    assert_eq!(
        store.undo(immediate_command, 2_000),
        Err(StoreError::NotUndoable)
    );

    let delayed = request_at(
        Provider::Codex,
        "PermissionRequest",
        "delayed-session",
        Some("turn-1"),
        Some("sudo true"),
        3_000,
    );
    let delayed_request = delayed.request_id.unwrap();
    store.ingest(delayed).unwrap();
    let delayed_command = Uuid::now_v7();
    let claim = store
        .claim_approval_with_delay(
            delayed_command,
            delayed_request,
            ApprovalAction::Deny,
            4_000,
            3_000,
        )
        .unwrap();
    assert_eq!(claim.commit_due_at, Some(7_000));
    assert_eq!(
        store.undo(delayed_command, 6_999).unwrap(),
        CommandState::Undone
    );

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
        Some("Codex is reviewing permissions automatically")
    );
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_noninteractive_modes_are_not_labeled_as_auto_approval() {
    let root = temp_root("codex-full-access");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    for (index, mode) in [
        "danger-full-access",
        "bypassPermissions",
        "fullAccess",
        "full_access",
        "never",
        "dontAsk",
    ]
    .into_iter()
    .enumerate()
    {
        store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Codex,
                json!({
                    "hook_event_name":"PermissionRequest",
                    "session_id":format!("noninteractive-{index}"),
                    "turn_id":"turn-1",
                    "tool_name":"Bash",
                    "permission_mode":mode
                }),
                1_000 + index as u64,
            ))
            .unwrap();
    }
    let snapshot = store.snapshot().unwrap();
    assert!(snapshot.attention.is_empty());
    assert!(snapshot
        .sessions
        .iter()
        .all(|session| session.approval_owner.is_none()));
    for session in snapshot
        .sessions
        .iter()
        .filter(|session| session.permission_mode.as_deref() != Some("dontAsk"))
    {
        assert_eq!(
            session.activity.as_deref(),
            Some("Codex full-access mode does not require user approval")
        );
    }
    assert_eq!(
        snapshot
            .sessions
            .iter()
            .find(|session| session.permission_mode.as_deref() == Some("dontAsk"))
            .and_then(|session| session.activity.as_deref()),
        Some("Codex non-interactive mode will not request user approval")
    );
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn managed_codex_approval_replaces_observation_only_native_attention() {
    let root = temp_root("managed-codex-native-dedupe");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"SessionStart",
                "session_id":"managed-thread"
            }),
            1_000,
        ))
        .unwrap();
    store
        .sync_native_approval(Provider::Codex, "managed-thread", true, true, 1_100)
        .unwrap();
    store
        .ingest(
            BridgeRequest::codex_approval_at(
                "item/fileChange/requestApproval",
                json!({
                    "threadId":"managed-thread",
                    "turnId":"turn-1",
                    "itemId":"item-1",
                    "startedAtMs":1_200,
                    "reason":"write a fixture",
                    "grantRoot":"/tmp/project"
                }),
                1_200,
            )
            .unwrap(),
        )
        .unwrap();
    let snapshot = store.snapshot().unwrap();
    assert_eq!(
        snapshot
            .attention
            .iter()
            .filter(|item| item.kind == "approval" && item.state == "open")
            .count(),
        1
    );
    assert!(snapshot.attention.iter().any(|item| {
        item.kind == "native_approval"
            && item.state == "resolved"
            && item.resolution.as_deref() == Some("direct_channel_available")
    }));
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn repeated_native_approval_and_usage_polling_do_not_advance_session_event_time() {
    for provider in [Provider::Codex, Provider::Claude] {
        let root = temp_root("native-approval-event-time");
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        store
            .ingest(request_at(
                provider,
                "UserPromptSubmit",
                "event-time",
                Some("turn-1"),
                None,
                1_000,
            ))
            .unwrap();
        store
            .sync_native_approval(provider, "event-time", true, true, 1_100)
            .unwrap();
        let waiting = store.snapshot().unwrap();
        for at in [1_200, 1_300] {
            store
                .sync_native_approval(provider, "event-time", true, true, at)
                .unwrap();
            store
                .sync_provider_execution(provider, "event-time", true, at)
                .unwrap();
            let mut usage = usage_record("event-time", at, at, None, "derived");
            usage.provider = provider.to_string();
            store.upsert_session_usage(usage).unwrap();
            let polled = store.snapshot().unwrap();
            assert_eq!(polled.sessions[0].last_event_at, 1_100);
            assert_eq!(
                polled.sessions[0].activity_since,
                waiting.sessions[0].activity_since
            );
            assert_eq!(polled.attention[0].id, waiting.attention[0].id);
            assert_eq!(polled.sessions[0].token_total, Some(at));
        }
        store
            .sync_native_approval(provider, "event-time", false, true, 1_400)
            .unwrap();
        assert_eq!(store.snapshot().unwrap().sessions[0].last_event_at, 1_400);
        store
            .sync_native_approval(provider, "event-time", false, true, 1_500)
            .unwrap();
        assert_eq!(store.snapshot().unwrap().sessions[0].last_event_at, 1_400);
        store
            .ingest(request_at(
                provider,
                "PreToolUse",
                "event-time",
                Some("turn-1"),
                Some("Read"),
                1_600,
            ))
            .unwrap();
        assert_eq!(store.snapshot().unwrap().sessions[0].last_event_at, 1_600);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
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
    assert_eq!(waiting.attention[0].title, "Codex is requesting approval");
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

    // Real Codex Desktop sends PostToolUse(request_permissions) and Stop while
    // its permission sheet is still visible. Neither event is a user decision.
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
    let post_tool = store.snapshot().unwrap();
    assert_eq!(post_tool.attention[0].state, "open");
    assert_eq!(post_tool.sessions[0].exec_state, "awaiting_approval");
    assert_eq!(
        post_tool.sessions[0].approval_owner.as_deref(),
        Some("terminal")
    );

    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"Stop",
                "session_id":"codex-desktop-native",
                "turn_id":"turn-native"
            }),
            100_003,
        ))
        .unwrap();
    let stopped_but_waiting = store.snapshot().unwrap();
    assert_eq!(stopped_but_waiting.attention[0].state, "open");
    assert_eq!(
        stopped_but_waiting.sessions[0].exec_state,
        "awaiting_approval"
    );
    assert!(!stopped_but_waiting
        .attention
        .iter()
        .any(|item| item.kind == "completion" && item.state == "open"));

    // Only the explicit Provider waiting flag ending resolves the neutral
    // observation. An inactive Thread then produces the normal completion.
    store
        .sync_native_approval(
            Provider::Codex,
            "codex-desktop-native",
            false,
            false,
            100_004,
        )
        .unwrap();
    let handled = store.snapshot().unwrap();
    let native = handled
        .attention
        .iter()
        .find(|item| item.kind == "native_approval")
        .unwrap();
    assert_eq!(native.state, "resolved");
    assert_eq!(native.resolution.as_deref(), Some("provider_handled"));
    assert_eq!(handled.sessions[0].exec_state, "response_finished");
    assert_eq!(handled.sessions[0].approval_owner, None);
    assert!(handled.sessions[0]
        .activity
        .as_deref()
        .is_some_and(|activity| activity.contains("permission request was handled in Codex")));
    assert!(!handled.sessions[0]
        .activity
        .as_deref()
        .is_some_and(|activity| activity.contains("approved") || activity.contains("denied")));
    assert!(handled
        .attention
        .iter()
        .any(|item| item.kind == "completion" && item.state == "open"));

    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_plugin_install_hook_surfaces_provider_owned_attention_until_codex_advances() {
    let root = temp_root("codex-plugin-install-native");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let start = BridgeRequest::from_hook_at(
        Provider::Codex,
        json!({
            "hook_event_name":"PreToolUse",
            "session_id":"codex-plugin-session",
            "turn_id":"turn-plugin",
            "cwd":"/tmp/example-project",
            "tool_name":"request_plugin_install",
            "tool_use_id":"plugin-install-github",
            "tool_input":{
                "plugin_id":"github@openai-curated-remote",
                "suggest_reason":"GitHub 插件可连接仓库、PR 和工作流。"
            }
        }),
        110_000,
    );
    assert!(!start.needs_reply);
    let ingested = store.ingest(start).unwrap();
    assert_eq!(ingested.kind, actrealm_core::EventKind::PermissionRequested);

    let waiting = store.snapshot().unwrap();
    let native = waiting
        .attention
        .iter()
        .find(|item| item.kind == "native_approval")
        .unwrap();
    assert_eq!(
        native.title,
        "Codex requests installation or connection of GitHub"
    );
    assert_eq!(
        native.detail.as_deref(),
        Some(
            "GitHub 插件可连接仓库、PR 和工作流。 Return to the Codex interface to confirm or cancel."
        )
    );
    assert_eq!(native.request_id, None);
    assert_eq!(waiting.sessions[0].exec_state, "awaiting_approval");
    assert_eq!(
        waiting.sessions[0].approval_owner.as_deref(),
        Some("terminal")
    );
    assert_eq!(
        waiting.sessions[0].activity.as_deref(),
        Some("Waiting for you to confirm installation or connection of GitHub in Codex")
    );
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"Notification",
                "session_id":"codex-plugin-session",
                "turn_id":"turn-plugin",
                "message":"waiting for the native plugin dialog"
            }),
            110_001,
        ))
        .unwrap();
    let still_waiting = store.snapshot().unwrap();
    assert_eq!(still_waiting.sessions[0].exec_state, "awaiting_approval");
    assert!(still_waiting
        .attention
        .iter()
        .any(|item| item.kind == "native_approval" && item.state == "open"));

    // Codex emits the plugin function output only after the native dialog
    // resolves. A declined request remains neutral: ActRealm observes that it
    // was handled, but never invents an approve or deny decision.
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"PostToolUse",
                "session_id":"codex-plugin-session",
                "turn_id":"turn-plugin",
                "tool_name":"request_plugin_install",
                "tool_use_id":"plugin-install-github",
                "tool_response":{
                    "completed":false,
                    "user_confirmed":false,
                    "tool_name":"GitHub"
                }
            }),
            110_002,
        ))
        .unwrap();
    let handled = store.snapshot().unwrap();
    assert_eq!(handled.sessions[0].exec_state, "thinking");
    assert_eq!(handled.sessions[0].approval_owner, None);
    assert_eq!(
        handled.sessions[0].activity.as_deref(),
        Some("The request was handled in Codex; continuing")
    );
    assert!(!handled.sessions[0]
        .activity
        .as_deref()
        .is_some_and(|activity| activity.contains("approved") || activity.contains("denied")));
    assert!(handled.attention.iter().any(|item| {
        item.kind == "native_approval"
            && item.state == "resolved"
            && item.resolution.as_deref() == Some("provider_advanced")
    }));

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
    let recovered = reopened.snapshot().unwrap();
    assert!(recovered
        .attention
        .iter()
        .all(|item| item.state == "expired"));
    assert!(recovered.sessions.iter().all(|session| {
        session.exec_state == "waiting_for_event" && session.approval_owner.is_none()
    }));
    assert!(recovered.sessions.iter().all(|session| {
        session.activity.as_deref()
            == Some(
                "Runtime restarted and the old reply channel expired; waiting for a new Agent event"
            )
    }));
    drop(reopened);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_waiting_session_without_live_blocker_is_normalized_on_open() {
    let root = temp_root("legacy-local-waiting");
    let database = root.join("data.sqlite");
    RuntimeStore::open(&database).unwrap();

    let connection = Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "INSERT INTO sessions(
                id, provider, provider_session_id, exec_state, approval_owner, started_at, last_event_at
              ) VALUES
                ('stale-local', 'claude', 'stale-local', 'awaiting_approval', 'widget', 1, 1),
                ('native-observation', 'codex', 'native-observation', 'awaiting_approval', 'terminal', 1, 1);
              INSERT INTO attention_items(
                id, session_id, provider, kind, title, risk, risk_notes, dedupe_key, state, created_at
              ) VALUES (
                'native-attention', 'native-observation', 'codex', 'native_approval',
                'Provider-native approval', 'unknown', '[]', 'native-observation', 'open', 1
              );",
        )
        .unwrap();
    drop(connection);

    let reopened = RuntimeStore::open(&database).unwrap();
    let snapshot = reopened.snapshot().unwrap();
    let stale = snapshot
        .sessions
        .iter()
        .find(|session| session.provider_session_id == "stale-local")
        .unwrap();
    assert_eq!(stale.exec_state, "waiting_for_event");
    assert_eq!(stale.approval_owner, None);

    let native = snapshot
        .sessions
        .iter()
        .find(|session| session.provider_session_id == "native-observation")
        .unwrap();
    assert_eq!(native.exec_state, "awaiting_approval");
    assert_eq!(native.approval_owner.as_deref(), Some("terminal"));
    assert!(snapshot.attention.iter().any(|item| {
        item.session_id == native.id && item.kind == "native_approval" && item.state == "open"
    }));

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
    assert_eq!(attention.primary_category, Some(OperationCategory::GitPush));
    assert_eq!(
        attention.risk_codes,
        [
            AttentionRiskCode::HighImpact,
            AttentionRiskCode::Irreversible
        ]
    );
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
fn provider_retry_identity_deduplicates_only_events_with_a_strong_stable_id() {
    let root = temp_root("stable-provider-event");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let event = json!({
        "hook_event_name":"PreToolUse",
        "session_id":"stable-event-session",
        "prompt_id":"prompt-1",
        "tool_use_id":"tool-use-1",
        "tool_name":"Bash",
        "tool_input":{"command":"cargo test"}
    });
    assert!(
        store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Claude,
                event.clone(),
                1_000,
            ))
            .unwrap()
            .inserted
    );
    assert!(
        !store
            .ingest(BridgeRequest::from_hook_at(Provider::Claude, event, 2_000,))
            .unwrap()
            .inserted
    );

    let distinct = json!({
        "hook_event_name":"PreToolUse",
        "session_id":"stable-event-session",
        "prompt_id":"prompt-1",
        "tool_use_id":"tool-use-2",
        "tool_name":"Bash",
        "tool_input":{"command":"cargo test"}
    });
    assert!(
        store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Claude,
                distinct,
                3_000,
            ))
            .unwrap()
            .inserted
    );

    let without_stable_id = json!({
        "hook_event_name":"PermissionRequest",
        "session_id":"stable-event-session",
        "prompt_id":"prompt-1",
        "tool_name":"Bash",
        "tool_input":{"command":"cargo test"}
    });
    assert!(
        store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Claude,
                without_stable_id.clone(),
                4_000,
            ))
            .unwrap()
            .inserted
    );
    assert!(
        store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Claude,
                without_stable_id,
                5_000,
            ))
            .unwrap()
            .inserted
    );
    assert_eq!(store.snapshot().unwrap().event_count, 4);
    drop(store);
    fs::remove_dir_all(root).unwrap();
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
    assert_eq!(created.activity.as_deref(), Some("Plan progress 0/2"));
    assert_eq!(
        created
            .plan_steps
            .iter()
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>(),
        vec!["task-1", "task-2"],
        "batched snapshots must preserve every stable Claude task"
    );

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
    assert_eq!(completed.activity.as_deref(), Some("Plan progress 2/2"));
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
        Some("2 subagents running")
    );
    store
        .ingest(subagent("SubagentStop", Some("agent-1"), 2_003))
        .unwrap();
    assert_eq!(
        store.snapshot().unwrap().sessions[0].activity.as_deref(),
        Some("1 subagents running")
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
    assert_eq!(
        background.activity.as_deref(),
        Some("1 background tasks still running")
    );
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
fn a_tool_start_reopens_an_automatically_continued_codex_turn() {
    let root = temp_root("tool-reopens-completed-turn");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();

    for (at, event, tool_name) in [
        (1_000, "UserPromptSubmit", None),
        (1_100, "PreToolUse", Some("Write")),
        (1_150, "PostToolUse", Some("Write")),
        (1_200, "Stop", None),
    ] {
        let mut raw = json!({
            "hook_event_name": event,
            "session_id": "automatic-continuation",
            "turn_id": "turn-1",
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
    assert_eq!(stopped.sessions[0].exec_state, "response_finished");
    assert!(stopped.sessions[0].turn_ended_at.is_some());
    assert!(stopped
        .attention
        .iter()
        .any(|item| item.kind == "completion" && item.state == "open"));

    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name": "PreToolUse",
                "session_id": "automatic-continuation",
                "turn_id": "turn-1",
                "cwd": "/tmp/example-project",
                "tool_name": "Bash",
                "tool_input": { "command": "cargo test" }
            }),
            1_300,
        ))
        .unwrap();

    let resumed = store.snapshot().unwrap();
    assert_eq!(resumed.sessions[0].exec_state, "tool_running");
    assert!(resumed.sessions[0].turn_ended_at.is_none());
    assert!(resumed.attention.iter().any(|item| {
        item.kind == "completion"
            && item.state == "resolved"
            && item.resolution.as_deref() == Some("superseded_by_activity")
    }));

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
        .is_some_and(|detail| detail.contains("terminal command")));

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

#[test]
fn successful_turn_without_write_tools_still_creates_completion_attention() {
    let root = temp_root("read-only-completion");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(request_at(
            Provider::Codex,
            "UserPromptSubmit",
            "read-only-session",
            Some("turn-1"),
            None,
            70_000,
        ))
        .unwrap();
    store
        .ingest(request_at(
            Provider::Codex,
            "Stop",
            "read-only-session",
            Some("turn-1"),
            None,
            70_001,
        ))
        .unwrap();

    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.sessions[0].exec_state, "response_finished");
    let completion = snapshot
        .attention
        .iter()
        .find(|item| item.kind == "completion")
        .unwrap();
    assert_eq!(completion.state, "open");
    assert_eq!(completion.title, "Task completed; waiting for confirmation");
    assert_eq!(completion.auto_hide_at, None);
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn auto_hide_policy_assigns_a_runtime_owned_completion_deadline() {
    let root = temp_root("completion-auto-hide-deadline");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    store
        .write_ui_settings(
            r#"{"completionTaskHideMode":"afterDelay","completionAutoHideMinutes":30}"#,
            now,
        )
        .unwrap();
    store
        .ingest(request_at(
            Provider::Codex,
            "UserPromptSubmit",
            "auto-hide-session",
            Some("turn-1"),
            None,
            now.saturating_add(1),
        ))
        .unwrap();
    store
        .ingest(request_at(
            Provider::Codex,
            "Stop",
            "auto-hide-session",
            Some("turn-1"),
            None,
            now.saturating_add(2),
        ))
        .unwrap();

    let snapshot = store.snapshot().unwrap();
    let completion = snapshot
        .attention
        .iter()
        .find(|item| item.kind == "completion")
        .unwrap();
    assert_eq!(completion.state, "open");
    assert_eq!(
        completion.auto_hide_at,
        Some(now.saturating_add(2 + 30 * 60 * 1000))
    );
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn hidden_completed_task_reappears_only_after_new_meaningful_activity() {
    let root = temp_root("completion-visibility-reactivation");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(request_at(
            Provider::Codex,
            "UserPromptSubmit",
            "reactivated-session",
            Some("turn-1"),
            None,
            90_000,
        ))
        .unwrap();
    store
        .ingest(request_at(
            Provider::Codex,
            "Stop",
            "reactivated-session",
            Some("turn-1"),
            None,
            90_001,
        ))
        .unwrap();
    let completion = store
        .snapshot()
        .unwrap()
        .attention
        .into_iter()
        .find(|item| item.kind == "completion")
        .unwrap();
    store
        .act_on_attention(Uuid::now_v7(), &completion.id, AttentionAction::Ack, 90_002)
        .unwrap();
    assert!(store.ui_snapshot(0).unwrap().sessions.is_empty());
    assert_eq!(store.snapshot().unwrap().sessions.len(), 1);

    store
        .ingest(request_at(
            Provider::Codex,
            "UserPromptSubmit",
            "reactivated-session",
            Some("turn-2"),
            None,
            90_003,
        ))
        .unwrap();
    let reactivated = store.ui_snapshot(0).unwrap();
    assert_eq!(reactivated.sessions.len(), 1);
    assert_eq!(reactivated.sessions[0].exec_state, "thinking");

    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn lifecycle_replay_does_not_create_a_task_completion_or_meaningful_activity() {
    let root = temp_root("lifecycle-only");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    for (event, at) in [("SessionStart", 80_000), ("SessionEnd", 80_001)] {
        store
            .ingest(request_at(
                Provider::Claude,
                event,
                "historical-session",
                None,
                None,
                at,
            ))
            .unwrap();
    }
    let snapshot = store.snapshot().unwrap();
    assert!(snapshot.attention.is_empty());
    assert_eq!(snapshot.sessions[0].last_meaningful_activity_at, None);
    assert_eq!(
        snapshot.sessions[0].activity.as_deref(),
        Some("Session ended")
    );
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn opening_an_old_database_normalizes_inconsistent_subagent_rows() {
    let root = temp_root("subagent-normalization");
    let database = root.join("data.sqlite");
    let store = RuntimeStore::open(&database).unwrap();
    store
        .ingest(request_at(
            Provider::Claude,
            "UserPromptSubmit",
            "subagent-migration",
            Some("turn-1"),
            None,
            90_000,
        ))
        .unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Claude,
            json!({
                "hook_event_name":"SubagentStart",
                "session_id":"subagent-migration",
                "agent_id":"child-1",
                "agent_type":"Explore"
            }),
            90_001,
        ))
        .unwrap();
    drop(store);

    let connection = Connection::open(&database).unwrap();
    connection
        .execute(
            "UPDATE session_subagents SET active = 0, status = 'running', stopped_at = NULL",
            [],
        )
        .unwrap();
    drop(connection);

    let store = RuntimeStore::open(&database).unwrap();
    drop(store);
    let connection = Connection::open(&database).unwrap();
    let (active, status, stopped_at) = connection
        .query_row(
            "SELECT active, status, stopped_at FROM session_subagents WHERE agent_id = 'child-1'",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(active, 0);
    assert_eq!(status, "completed");
    assert!(stopped_at.is_some());
    drop(connection);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_plan_updates_persist_real_steps_and_progress() {
    let root = temp_root("codex-plan");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(BridgeRequest::from_provider_event_at(
            Provider::Codex,
            "PlanUpdated",
            "plan-session",
            Some("turn-plan"),
            json!({
                "plan": [
                    {"step":"读取现有实现", "status":"completed"},
                    {"step":"修复通知语义", "status":"inProgress"},
                    {"step":"运行门禁", "status":"pending"}
                ],
                "explanation":"以 Provider 官方事件为事实来源"
            }),
            71_000,
        ))
        .unwrap();

    let snapshot = store.snapshot().unwrap();
    let session = &snapshot.sessions[0];
    assert_eq!((session.plan_done, session.plan_total), (Some(1), Some(3)));
    assert_eq!(session.plan_steps.len(), 3);
    assert_eq!(session.plan_steps[1].text, "修复通知语义");
    assert_eq!(session.plan_steps[1].status, "in_progress");
    assert_eq!(
        session.plan_steps[0].detail.as_deref(),
        Some("以 Provider 官方事件为事实来源")
    );
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_update_plan_hook_persists_the_allowlisted_nested_plan() {
    let root = temp_root("codex-hook-plan");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name": "PreToolUse",
                "session_id": "hook-plan-session",
                "turn_id": "hook-plan-turn",
                "tool_name": "update_plan",
                "tool_use_id": "plan-call",
                "tool_input": {
                    "explanation": "bounded explanation",
                    "plan": [
                        {"step": "Inspect", "status": "completed"},
                        {"step": "Implement", "status": "in_progress"},
                        {"step": "Verify", "status": "pending"}
                    ]
                }
            }),
            71_500,
        ))
        .unwrap();

    let snapshot = store.snapshot().unwrap();
    let session = &snapshot.sessions[0];
    assert_eq!((session.plan_done, session.plan_total), (Some(1), Some(3)));
    assert_eq!(session.plan_steps[1].text, "Implement");
    assert_eq!(session.plan_steps[1].status, "in_progress");

    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name": "Stop",
                "session_id": "hook-plan-session",
                "turn_id": "hook-plan-turn"
            }),
            71_600,
        ))
        .unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name": "PreToolUse",
                "session_id": "hook-plan-session",
                "turn_id": "hook-plan-turn",
                "tool_name": "update_plan",
                "tool_use_id": "plan-call-after-stop",
                "tool_input": {
                    "plan": [
                        {"step": "Inspect", "status": "completed"},
                        {"step": "Implement", "status": "completed"},
                        {"step": "Verify", "status": "in_progress"}
                    ]
                }
            }),
            71_700,
        ))
        .unwrap();

    let after_stop = store.snapshot().unwrap();
    let session = &after_stop.sessions[0];
    assert_eq!(session.exec_state, "response_finished");
    assert_eq!((session.plan_done, session.plan_total), (Some(2), Some(3)));
    assert_eq!(session.plan_steps[1].status, "completed");
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_new_codex_turn_does_not_inherit_the_previous_turn_plan() {
    let root = temp_root("codex-turn-scoped-plan");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name": "UserPromptSubmit",
                "session_id": "turn-scoped-plan-session",
                "turn_id": "turn-1"
            }),
            80_000,
        ))
        .unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name": "PreToolUse",
                "session_id": "turn-scoped-plan-session",
                "turn_id": "turn-1",
                "tool_name": "update_plan",
                "tool_input": {
                    "plan": [
                        {"step":"Old task", "status":"in_progress"},
                        {"step":"Old verification", "status":"pending"}
                    ]
                }
            }),
            80_010,
        ))
        .unwrap();
    assert_eq!(store.snapshot().unwrap().sessions[0].plan_total, Some(2));

    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name": "Stop",
                "session_id": "turn-scoped-plan-session",
                "turn_id": "turn-1"
            }),
            80_020,
        ))
        .unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name": "UserPromptSubmit",
                "session_id": "turn-scoped-plan-session",
                "turn_id": "turn-2"
            }),
            80_030,
        ))
        .unwrap();

    let current = store.snapshot().unwrap().sessions.remove(0);
    assert_eq!((current.plan_done, current.plan_total), (None, None));
    assert!(current.plan_steps.is_empty());
    let workflow = store
        .latest_current_local_timeline(current.id, 100)
        .unwrap()
        .unwrap();
    assert_eq!(workflow.events.len(), 1);
    assert_eq!(
        workflow.events[0].kind,
        actrealm_runtime::TimelineEventKind::TurnStarted
    );
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn workflow_projects_safe_semantic_tool_categories() {
    let root = temp_root("workflow-tool-category");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    for (at, event) in [(81_000, "PreToolUse"), (81_010, "PostToolUse")] {
        store
            .ingest(BridgeRequest::from_hook_at(
                Provider::Codex,
                json!({
                    "hook_event_name": event,
                    "session_id": "category-session",
                    "turn_id": "category-turn",
                    "tool_name": "Bash",
                    "tool_input": {"command": "cargo test --workspace --offline"}
                }),
                at,
            ))
            .unwrap();
    }
    let workflow = store
        .latest_current_local_timeline(store.snapshot().unwrap().sessions[0].id.clone(), 10)
        .unwrap()
        .unwrap();
    assert_eq!(workflow.events.len(), 2);
    assert!(workflow
        .events
        .iter()
        .all(|event| event.tool_category.as_deref() == Some("test")));
    assert!(!serde_json::to_string(&workflow)
        .unwrap()
        .contains("cargo test"));
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_provider_owned_permission_never_opens_a_user_approval_item() {
    let root = temp_root("provider-owned-pretool");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"PreToolUse",
                "session_id":"auto-review-session",
                "turn_id":"turn-1",
                "tool_name":"request_permissions",
                "_approvals_reviewer":"auto_review"
            }),
            72_000,
        ))
        .unwrap();

    let snapshot = store.snapshot().unwrap();
    assert!(snapshot.attention.is_empty());
    assert_eq!(snapshot.sessions[0].exec_state, "thinking");
    assert_eq!(
        snapshot.sessions[0].approval_owner.as_deref(),
        Some("provider")
    );
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn ui_snapshot_reads_only_recent_or_actionable_sessions() {
    let root = temp_root("bounded-ui-snapshot");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let now = 1_784_130_000_000;
    let cutoff = now - 30 * 60 * 1_000;

    for index in 0..500 {
        store
            .ingest(request_at(
                Provider::Claude,
                "SessionStart",
                &format!("expired-{index}"),
                None,
                None,
                cutoff - 1,
            ))
            .unwrap();
    }
    store
        .ingest(request_at(
            Provider::Claude,
            "UserPromptSubmit",
            "recent",
            Some("recent-turn"),
            None,
            now,
        ))
        .unwrap();
    store
        .ingest(request_at(
            Provider::Claude,
            "UserPromptSubmit",
            "old-but-active",
            Some("active-turn"),
            None,
            cutoff - 10_000,
        ))
        .unwrap();
    for (session, stop_at) in [
        ("recently-inactive", cutoff + 1),
        ("expired-inactive", cutoff - 1),
    ] {
        store
            .ingest(request_at(
                Provider::Claude,
                "UserPromptSubmit",
                session,
                Some("terminal-turn"),
                None,
                cutoff - 20_000,
            ))
            .unwrap();
        assert!(store
            .sync_provider_execution(Provider::Claude, session, false, stop_at)
            .unwrap());
    }
    store
        .ingest(request_at(
            Provider::Claude,
            "PermissionRequest",
            "actionable-expired",
            Some("actionable-turn"),
            Some("cargo test"),
            cutoff - 1,
        ))
        .unwrap();

    let full_snapshot = store.snapshot().unwrap();
    assert!(full_snapshot
        .sessions
        .iter()
        .filter(|session| {
            session
                .provider_session_id
                .strip_prefix("expired-")
                .is_some_and(|suffix| suffix.parse::<usize>().is_ok())
        })
        .all(|session| session.exec_state == "idle"));

    let snapshot = store.ui_snapshot(cutoff).unwrap();
    let session_ids = snapshot
        .sessions
        .iter()
        .map(|session| session.provider_session_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        session_ids,
        std::collections::HashSet::from([
            "recent",
            "old-but-active",
            "recently-inactive",
            "actionable-expired"
        ])
    );

    let export = store.export_json(now).unwrap();
    assert_eq!(export["tables"]["sessions"].as_array().unwrap().len(), 505);

    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn provider_connector_execution_state_reactivates_and_finishes_a_session() {
    let root = temp_root("provider-execution-sync");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(request_at(
            Provider::Codex,
            "SessionStart",
            "connector-thread",
            None,
            None,
            1_000,
        ))
        .unwrap();

    assert!(store
        .sync_provider_execution(Provider::Codex, "connector-thread", true, 2_000)
        .unwrap());
    let active = store.snapshot().unwrap();
    assert_eq!(active.sessions[0].exec_state, "thinking");
    assert_eq!(active.sessions[0].activity_since, Some(2_000));

    assert!(store
        .sync_provider_execution(Provider::Codex, "connector-thread", false, 3_000)
        .unwrap());
    let inactive = store.snapshot().unwrap();
    assert_eq!(inactive.sessions[0].exec_state, "response_finished");
    assert_eq!(inactive.sessions[0].activity_since, Some(3_000));

    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_compaction_session_start_preserves_the_active_turn() {
    let root = temp_root("codex-compaction-session-start");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(request_at(
            Provider::Codex,
            "UserPromptSubmit",
            "compacting-thread",
            Some("turn-1"),
            None,
            1_000,
        ))
        .unwrap();

    let mut compact = request_at(
        Provider::Codex,
        "SessionStart",
        "compacting-thread",
        Some("turn-1"),
        None,
        2_000,
    );
    compact.raw["source"] = Value::String("compact".to_owned());
    store.ingest(compact).unwrap();

    let active = store.snapshot().unwrap();
    assert_eq!(active.sessions[0].exec_state, "thinking");
    assert_eq!(active.sessions[0].activity.as_deref(), Some("Thinking"));

    store
        .ingest(request_at(
            Provider::Codex,
            "PreToolUse",
            "compacting-thread",
            Some("turn-1"),
            Some("cargo test"),
            2_100,
        ))
        .unwrap();
    let mut compact_during_tool = request_at(
        Provider::Codex,
        "SessionStart",
        "compacting-thread",
        Some("turn-1"),
        None,
        2_200,
    );
    compact_during_tool.raw["source"] = Value::String("compact".to_owned());
    store.ingest(compact_during_tool).unwrap();
    assert_eq!(
        store.snapshot().unwrap().sessions[0].exec_state,
        "tool_running"
    );

    store
        .ingest(request_at(
            Provider::Codex,
            "Stop",
            "compacting-thread",
            Some("turn-1"),
            None,
            3_000,
        ))
        .unwrap();
    assert_eq!(
        store.snapshot().unwrap().sessions[0].exec_state,
        "response_finished"
    );

    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn ui_snapshot_hides_recent_claude_lifecycle_replay_until_meaningful_activity() {
    let root = temp_root("ui-snapshot-lifecycle-replay");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();

    for index in 0..50 {
        for (event, offset) in [("SessionStart", 0), ("SessionEnd", 1)] {
            store
                .ingest(request_at(
                    Provider::Claude,
                    event,
                    &format!("history-only-{index}"),
                    None,
                    None,
                    1_000 + index * 2 + offset,
                ))
                .unwrap();
        }
    }

    let full_snapshot = store.snapshot().unwrap();
    assert_eq!(full_snapshot.sessions.len(), 50);
    assert!(full_snapshot
        .sessions
        .iter()
        .all(|session| session.last_meaningful_activity_at.is_none()));
    assert!(store.ui_snapshot(0).unwrap().sessions.is_empty());

    store
        .ingest(request_at(
            Provider::Claude,
            "UserPromptSubmit",
            "history-only-17",
            Some("real-turn"),
            None,
            2_000,
        ))
        .unwrap();
    let visible = store.ui_snapshot(0).unwrap();
    assert_eq!(visible.sessions.len(), 1);
    assert_eq!(
        visible.sessions[0].provider_session_id.as_str(),
        "history-only-17"
    );

    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn snapshot_batches_plan_steps_and_subagents() {
    let root = temp_root("batched-ui-snapshot");
    let database = root.join("data.sqlite");
    RuntimeStore::open(&database).unwrap();

    let mut connection = Connection::open(&database).unwrap();
    let transaction = connection.transaction().unwrap();
    for index in 0..500 {
        let session_id = format!("visible-{index}");
        transaction
            .execute(
                "INSERT INTO sessions(
                    id, provider, provider_session_id, exec_state, started_at, last_event_at
                 ) VALUES (?1, 'claude', ?1, 'thinking', 1, 1)",
                [&session_id],
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO turns(
                    id, session_id, provider_turn_id, ordinal, state, started_at
                 ) VALUES (?1 || '-turn', ?1, 'turn', 1, 'running', 1)",
                [&session_id],
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO session_plan_steps(
                    session_id, provider_turn_id, step_index, step, detail, status, source, updated_at
                 ) VALUES (?1, 'turn', 0, 'Run checks', NULL, 'pending', 'connector', 1)",
                [&session_id],
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO session_subagents(
                    session_id, agent_id, agent_type, status, source, active, started_at
                 ) VALUES (?1, 'child', 'Explore', 'running', 'hook', 1, 1)",
                [&session_id],
            )
            .unwrap();
    }
    transaction.commit().unwrap();
    drop(connection);

    let store = RuntimeStore::open(&database).unwrap();
    let (snapshot, query_count) = store.ui_snapshot_with_query_count(0).unwrap();
    assert_eq!(snapshot.sessions.len(), 500);
    assert!(snapshot.sessions.iter().all(|session| {
        session.plan_steps.len() == 1
            && session.plan_steps[0].text == "Run checks"
            && session.subagents.len() == 1
            && session.subagents[0].id == "child"
    }));
    assert!(
        query_count <= 10,
        "UI snapshot issued {query_count} SQL statements for 500 sessions"
    );

    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn ui_snapshot_cache_does_not_cross_cutoffs() {
    let root = temp_root("ui-snapshot-cache-cutoff");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    for event in ["UserPromptSubmit", "SessionEnd"] {
        store
            .ingest(request_at(
                Provider::Claude,
                event,
                "cutoff-boundary",
                Some("turn"),
                None,
                1_000,
            ))
            .unwrap();
    }

    assert_eq!(store.ui_snapshot(1_000).unwrap().sessions.len(), 1);
    assert!(store.ui_snapshot(1_001).unwrap().sessions.is_empty());

    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn ui_snapshot_cache_reuses_moving_cutoffs_without_returning_expired_sessions() {
    let root = temp_root("ui-snapshot-moving-cutoff-cache");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    for (session, at) in [("crosses-cutoff", 1_000), ("stays-visible", 1_002)] {
        for event in ["UserPromptSubmit", "SessionEnd"] {
            store
                .ingest(request_at(
                    Provider::Claude,
                    event,
                    session,
                    Some("turn"),
                    None,
                    at,
                ))
                .unwrap();
        }
    }

    let (first, first_query_count) = store.ui_snapshot_with_query_count(1_000).unwrap();
    assert_eq!(first.sessions.len(), 2);
    assert!(first_query_count > 0);

    let (second, second_query_count) = store.ui_snapshot_with_query_count(1_001).unwrap();
    assert_eq!(
        second
            .sessions
            .iter()
            .map(|session| session.provider_session_id.as_str())
            .collect::<Vec<_>>(),
        vec!["stays-visible"]
    );
    assert_eq!(
        second_query_count, 0,
        "a moving cutoff inside the cache window should reuse the bounded snapshot"
    );

    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unchanged_usage_refresh_keeps_the_ui_snapshot_cache_hot() {
    let root = temp_root("unchanged-usage-snapshot-cache");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({"hook_event_name":"SessionStart","session_id":"stable-usage"}),
            1,
        ))
        .unwrap();
    let record = usage_record("stable-usage", 2_000, 100, Some(250), "derived");
    let first_generation = store.begin_usage_collection_generation().unwrap();
    assert!(store
        .replace_session_usages_for_generation(vec![record.clone()], 2_000, first_generation,)
        .unwrap());
    let (_, first_query_count) = store.ui_snapshot_with_query_count(0).unwrap();
    assert!(first_query_count > 0);

    let second_generation = store.begin_usage_collection_generation().unwrap();
    assert!(store
        .replace_session_usages_for_generation(vec![record], 2_001, second_generation)
        .unwrap());
    let (snapshot, second_query_count) = store.ui_snapshot_with_query_count(0).unwrap();
    assert_eq!(snapshot.token_usage.total, 100);
    assert_eq!(
        second_query_count, 0,
        "an unchanged one-second usage poll must not rebuild the SQLite snapshot"
    );

    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn ui_snapshot_query_measurement_is_independent_across_runtime_stores() {
    let root = temp_root("ui-snapshot-query-counter-concurrency");
    let databases = [root.join("first.sqlite"), root.join("second.sqlite")];
    let stores = databases
        .iter()
        .map(|database| {
            RuntimeStore::open(database).unwrap();
            let mut connection = Connection::open(database).unwrap();
            let transaction = connection.transaction().unwrap();
            for index in 0..2_000 {
                let session_id = format!("visible-{index}");
                transaction
                    .execute(
                        "INSERT INTO sessions(
                            id, provider, provider_session_id, exec_state, started_at, last_event_at
                         ) VALUES (?1, 'claude', ?1, 'thinking', 1, 1)",
                        [&session_id],
                    )
                    .unwrap();
            }
            transaction.commit().unwrap();
            drop(connection);
            RuntimeStore::open(database).unwrap()
        })
        .collect::<Vec<_>>();
    let barrier = Arc::new(Barrier::new(3));
    let readers = stores
        .iter()
        .cloned()
        .map(|store| {
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                store.ui_snapshot_with_query_count(0).unwrap().1
            })
        })
        .collect::<Vec<_>>();

    barrier.wait();
    let query_counts = readers
        .into_iter()
        .map(|reader| reader.join().unwrap())
        .collect::<Vec<_>>();
    assert!(query_counts.iter().all(|count| (1..=10).contains(count)));

    drop(stores);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn provider_result_excerpts_are_transient_and_clear_on_the_next_turn() {
    for provider in [Provider::Codex, Provider::Claude] {
        let root = temp_root("transient-result");
        let path = root.join("data.sqlite");
        let store = RuntimeStore::open(&path).unwrap();
        let start = store
            .ingest(request_at(
                provider,
                "UserPromptSubmit",
                "result-session",
                Some("t1"),
                None,
                100,
            ))
            .unwrap();
        let stop = BridgeRequest::from_hook_at(
            provider,
            json!({
                "hook_event_name":"Stop", "session_id":"result-session", "turn_id":"t1",
                "last_assistant_message":"独立结果标记：计算结果为 15129。"
            }),
            200,
        );
        store.ingest(stop.clone()).unwrap();
        assert!(store
            .session_result(&start.session_id, 100, 250)
            .unwrap()
            .summary
            .contains("15129"));
        assert!(!store
            .export_json(250)
            .unwrap()
            .to_string()
            .contains("独立结果标记"));
        let spool = EventSpool::new(root.join("spool"));
        spool.append(&stop).unwrap();
        spool
            .drain(|r| {
                assert!(r.raw.get("last_assistant_message").is_none());
                true
            })
            .unwrap();
        store
            .ingest(request_at(
                provider,
                "UserPromptSubmit",
                "result-session",
                Some("t2"),
                None,
                300,
            ))
            .unwrap();
        assert!(store.session_result(&start.session_id, 300, 350).is_none());
        drop(store);
        let store = RuntimeStore::open(&path).unwrap();
        assert!(store.session_result(&start.session_id, 0, 400).is_none());
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn codex_rollout_terminal_events_repair_only_the_matching_current_turn() {
    let root = temp_root("rollout-terminal");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(request_at(
            Provider::Codex,
            "UserPromptSubmit",
            "thread",
            Some("turn-1"),
            None,
            1000,
        ))
        .unwrap();
    assert!(!store
        .observe_codex_turn_end("thread", "older-turn", "StopFailure", 2000)
        .unwrap());
    assert!(!store
        .observe_codex_turn_end("thread", "turn-1", "StopFailure", 999)
        .unwrap());
    assert!(store
        .observe_codex_turn_end("thread", "turn-1", "StopFailure", 2000)
        .unwrap());
    let failed = &store.snapshot().unwrap().sessions[0];
    assert_eq!(failed.exec_state, "failed");
    assert_eq!(failed.turn_ended_at, Some(2000));
    assert!(!store
        .observe_codex_turn_end("thread", "turn-1", "StopFailure", 2000)
        .unwrap());
    store
        .ingest(request_at(
            Provider::Codex,
            "UserPromptSubmit",
            "thread",
            Some("turn-2"),
            None,
            3000,
        ))
        .unwrap();
    assert!(!store
        .observe_codex_turn_end("thread", "turn-1", "Stop", 4000)
        .unwrap());
    assert_eq!(store.snapshot().unwrap().sessions[0].exec_state, "thinking");
    assert!(store
        .observe_codex_turn_end("thread", "turn-2", "Stop", 4000)
        .unwrap());
    assert_eq!(
        store.snapshot().unwrap().sessions[0].exec_state,
        "response_finished"
    );
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn missing_execution_evidence_never_completes_work_or_overwrites_a_new_event() {
    let root = temp_root("unconfirmed-execution");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    store
        .ingest(request_at(
            Provider::Codex,
            "UserPromptSubmit",
            "thread",
            Some("turn-1"),
            None,
            1000,
        ))
        .unwrap();
    let id = store.snapshot().unwrap().sessions[0].id.clone();
    assert!(!store.mark_execution_unconfirmed(&id, 999).unwrap());
    assert!(store.mark_execution_unconfirmed(&id, 1000).unwrap());
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.sessions[0].exec_state, "waiting_for_event");
    assert_eq!(snapshot.sessions[0].turn_ended_at, None);
    assert!(!snapshot
        .attention
        .iter()
        .any(|item| item.kind == "completion"));
    store
        .ingest(request_at(
            Provider::Codex,
            "PreToolUse",
            "thread",
            Some("turn-1"),
            Some("Read"),
            2000,
        ))
        .unwrap();
    assert!(!store.mark_execution_unconfirmed(&id, 1000).unwrap());
    assert_eq!(
        store.snapshot().unwrap().sessions[0].exec_state,
        "tool_running"
    );
    store
        .ingest(request_at(
            Provider::Codex,
            "PermissionRequest",
            "thread",
            Some("turn-1"),
            Some("Bash"),
            3000,
        ))
        .unwrap();
    assert!(!store.mark_execution_unconfirmed(&id, 3000).unwrap());
    assert_eq!(
        store.snapshot().unwrap().sessions[0].exec_state,
        "awaiting_approval"
    );
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_internal_memory_maintenance_is_not_a_user_task() {
    let root = temp_root("internal-memory");
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let internal = store.ingest(BridgeRequest::from_hook_at(Provider::Codex, json!({
        "hook_event_name": "UserPromptSubmit", "session_id": "internal",
        "prompt": "## Memory Writing Agent: Phase 2 (Consolidation)\nYou are a Memory Writing Agent."
    }), 1000)).unwrap();
    assert!(internal.suppressed);
    assert!(
        store
            .ingest(request_at(
                Provider::Codex,
                "PreToolUse",
                "internal",
                None,
                Some("Read"),
                1100
            ))
            .unwrap()
            .suppressed
    );
    let user = store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name": "UserPromptSubmit", "session_id": "user",
                "prompt": "Review the memory writing code and fix its cache."
            }),
            1200,
        ))
        .unwrap();
    assert!(!user.suppressed);
    assert_eq!(store.snapshot().unwrap().sessions.len(), 1);
    drop(store);
    fs::remove_dir_all(root).unwrap();
}

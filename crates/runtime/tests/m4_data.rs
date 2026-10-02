use actrealm_core::{BridgeRequest, Provider};
use actrealm_runtime::{QuotaRecord, RuntimeStore};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

static ID: AtomicU64 = AtomicU64::new(0);

struct Database {
    root: PathBuf,
    path: PathBuf,
}

impl Database {
    fn new(name: &str) -> Self {
        let root = PathBuf::from("/tmp").join(format!(
            "actrealm-m4-data-{name}-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("data.sqlite");
        Self { root, path }
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn event(session: &str, received_at: u64) -> BridgeRequest {
    BridgeRequest::from_hook_at(
        Provider::Claude,
        json!({
            "hook_event_name":"SessionStart",
            "session_id":session,
            "cwd":"/tmp/private-project",
            "prompt":"raw prompt must not persist"
        }),
        received_at,
    )
}

#[test]
fn storage_diagnostics_report_current_schema_and_integrity_without_a_path() {
    let database = Database::new("diagnostics");
    let store = RuntimeStore::open(&database.path).unwrap();
    let diagnostics = store.storage_diagnostics().unwrap();

    assert_eq!(
        diagnostics.schema_version,
        diagnostics.expected_schema_version
    );
    assert_eq!(diagnostics.integrity, "ok");
    let encoded = serde_json::to_string(&diagnostics).unwrap();
    assert!(!encoded.contains(database.path.to_str().unwrap()));
}

#[test]
fn settings_quota_pruning_and_export_are_transactional_and_local() {
    let database = Database::new("export");
    let store = RuntimeStore::open(&database.path).unwrap();
    let now = 1_784_130_000_000;
    store
        .ingest(event("old-session", now - 100 * 86_400_000))
        .unwrap();
    store.ingest(event("new-session", now)).unwrap();
    store
        .write_setting("ui_settings", r#"{"retentionDays":90}"#)
        .unwrap();
    store
        .replace_quota_snapshots(vec![QuotaRecord {
            provider: "claude".to_owned(),
            window: "5h".to_owned(),
            limit_id: None,
            used_pct: 23.5,
            resets_at: 1_784_140_000,
            source: "statusline".to_owned(),
            captured_at: now,
        }])
        .unwrap();

    assert_eq!(store.prune_events(90, now).unwrap(), 1);
    assert_eq!(
        store.read_setting("ui_settings").unwrap().as_deref(),
        Some(r#"{"retentionDays":90}"#)
    );
    let export = store.export_json(now).unwrap();
    assert_eq!(export["schemaVersion"], 1);
    assert_eq!(export["tables"]["events"].as_array().unwrap().len(), 1);
    assert_eq!(export["tables"]["quota_snapshots"][0]["used_pct"], 23.5);
    let encoded = serde_json::to_string(&export).unwrap();
    assert!(!encoded.contains("raw prompt must not persist"));
}

#[test]
fn destructive_clear_recreates_the_database_and_removes_every_record() {
    let database = Database::new("clear");
    let store = RuntimeStore::open(&database.path).unwrap();
    store.ingest(event("clear-session", 10)).unwrap();
    store.write_setting("secret", "local-only").unwrap();
    let before = fs::metadata(&database.path).unwrap().modified().unwrap();

    store.clear_data().unwrap();

    assert!(database.path.exists());
    assert!(store.snapshot().unwrap().sessions.is_empty());
    assert_eq!(store.read_setting("secret").unwrap(), None);
    let export: Value = store.export_json(20).unwrap();
    assert!(export["tables"]["events"].as_array().unwrap().is_empty());
    assert!(fs::metadata(&database.path).unwrap().modified().unwrap() >= before);
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({"hook_event_name":"SessionStart","session_id":Uuid::now_v7()}),
            30,
        ))
        .unwrap();
    assert_eq!(store.snapshot().unwrap().sessions.len(), 1);
}

#[test]
fn retention_and_quota_validation_reject_ambiguous_values() {
    let database = Database::new("validation");
    let store = RuntimeStore::open(&database.path).unwrap();
    assert!(store.prune_events(7, 100).is_err());
    assert!(store
        .replace_quota_snapshots(vec![QuotaRecord {
            provider: "codex".to_owned(),
            window: "5h".to_owned(),
            limit_id: None,
            used_pct: 101.0,
            resets_at: 1,
            source: "rollout_experimental".to_owned(),
            captured_at: 1,
        }])
        .is_err());
}

#[test]
fn quota_schema_migrates_and_keeps_same_window_limit_buckets_distinct() {
    let database = Database::new("quota-limit-identity");
    let connection = Connection::open(&database.path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE quota_snapshots (
               provider TEXT NOT NULL, window TEXT NOT NULL,
               used_pct REAL, resets_at INTEGER, source TEXT,
               captured_at INTEGER NOT NULL, PRIMARY KEY(provider, window)
             );
             INSERT INTO quota_snapshots(
               provider, window, used_pct, resets_at, source, captured_at
             ) VALUES ('claude', '5h', 25, 100, 'statusline', 10);
             PRAGMA user_version = 18;",
        )
        .unwrap();
    drop(connection);

    let store = RuntimeStore::open(&database.path).unwrap();
    let migrated = store.export_json(20).unwrap();
    assert_eq!(migrated["tables"]["quota_snapshots"][0]["limit_id"], "");

    store
        .replace_quota_snapshots(vec![
            QuotaRecord {
                provider: "codex".to_owned(),
                window: "10080m".to_owned(),
                limit_id: Some("codex".to_owned()),
                used_pct: 1.0,
                resets_at: 1_787_209_188,
                source: "codex_app_server".to_owned(),
                captured_at: 1_786_608_000_000,
            },
            QuotaRecord {
                provider: "codex".to_owned(),
                window: "10080m".to_owned(),
                limit_id: Some("codex_bengalfox".to_owned()),
                used_pct: 0.0,
                resets_at: 1_787_213_254,
                source: "codex_app_server".to_owned(),
                captured_at: 1_786_608_000_000,
            },
        ])
        .unwrap();

    let export = store.export_json(30).unwrap();
    let rows = export["tables"]["quota_snapshots"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let mut limit_ids = rows
        .iter()
        .filter_map(|row| row["limit_id"].as_str())
        .collect::<Vec<_>>();
    limit_ids.sort_unstable();
    assert_eq!(limit_ids, vec!["codex", "codex_bengalfox"]);
}

#[test]
fn design_retention_options_support_180_days_and_forever() {
    let database = Database::new("design-retention");
    let store = RuntimeStore::open(&database.path).unwrap();
    let now = 1_784_130_000_000;
    store
        .ingest(event("older-than-180-days", now - 181 * 86_400_000))
        .unwrap();
    store.ingest(event("current", now)).unwrap();

    assert_eq!(store.prune_events(0, now).unwrap(), 0);
    assert_eq!(
        store.export_json(now).unwrap()["tables"]["events"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(store.prune_events(180, now).unwrap(), 1);
}

#[test]
fn retention_prunes_closed_expired_session_graph_but_preserves_actionable_rows() {
    let database = Database::new("retention-graph");
    RuntimeStore::open(&database.path).unwrap();
    let now = 1_784_130_000_000_i64;
    let expired_at = now - 31 * 86_400_000;

    let mut connection = Connection::open(&database.path).unwrap();
    let transaction = connection.transaction().unwrap();
    for (session_id, attention_id, attention_state) in [
        ("closed-expired", "closed-attention", "resolved"),
        ("actionable-expired", "actionable-attention", "open"),
    ] {
        transaction
            .execute(
                "INSERT INTO sessions(
                    id, provider, provider_session_id, exec_state, started_at, last_event_at
                 ) VALUES (?1, 'claude', ?1, 'response_finished', ?2, ?2)",
                (session_id, expired_at),
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO turns(id, session_id, ordinal, started_at, ended_at)
                 VALUES (?1 || '-turn', ?1, 1, ?2, ?2)",
                (session_id, expired_at),
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO events(id, session_id, turn_id, provider, type, occurred_at, ingest_seq)
                 VALUES (?1 || '-event', ?1, ?1 || '-turn', 'claude', 'turn.stopped', ?2, ?2)",
                (session_id, expired_at),
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO session_tasks(session_id, task_id, subject, completed, created_at)
                 VALUES (?1, 'task', 'Retained task metadata', 1, ?2)",
                (session_id, expired_at),
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO session_plan_steps(
                    session_id, provider_turn_id, step_index, step, status, source, updated_at
                 ) VALUES (?1, 'turn', 0, 'Run retention', 'completed', 'connector', ?2)",
                (session_id, expired_at),
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO session_subagents(
                    session_id, agent_id, status, active, started_at, stopped_at
                 ) VALUES (?1, 'child', 'completed', 0, ?2, ?2)",
                (session_id, expired_at),
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO attention_items(
                    id, session_id, provider, turn_id, kind, title, risk, risk_notes,
                    dedupe_key, state, created_at
                 ) VALUES (?2, ?1, 'claude', ?1 || '-turn', 'approval',
                    'Review requested action', 'unknown', '[]', ?2, ?3, ?4)",
                (session_id, attention_id, attention_state, expired_at),
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO commands(id, attention_id, action, state, created_at)
                 VALUES (?1 || '-command', ?1, 'allow', 'confirmed', ?2)",
                (attention_id, expired_at),
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO session_usage(
                    provider, provider_session_id, usage_source, usage_quality, captured_at
                 ) VALUES ('claude', ?1, 'test', 'known', ?2)",
                (session_id, expired_at),
            )
            .unwrap();
    }
    transaction
        .execute(
            "INSERT INTO quota_snapshots(provider, window, captured_at)
             VALUES ('claude', '5h', ?1)",
            [now],
        )
        .unwrap();
    transaction
        .execute(
            "INSERT INTO settings(key, value) VALUES ('ui_settings', '{\"retentionDays\":30}')",
            [],
        )
        .unwrap();
    transaction.commit().unwrap();
    drop(connection);

    let store = RuntimeStore::open(&database.path).unwrap();
    assert_eq!(store.prune_events(30, now as u64).unwrap(), 1);
    let export = store.export_json(now as u64).unwrap();
    for table in [
        "sessions",
        "turns",
        "events",
        "session_tasks",
        "session_plan_steps",
        "session_subagents",
        "attention_items",
        "commands",
        "session_usage",
    ] {
        assert_eq!(
            export["tables"][table].as_array().unwrap().len(),
            1,
            "{table}"
        );
    }
    assert_eq!(
        export["tables"]["sessions"][0]["provider_session_id"],
        "actionable-expired"
    );
    assert_eq!(export["tables"]["attention_items"][0]["state"], "open");
    assert_eq!(
        export["tables"]["quota_snapshots"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        export["tables"]["settings"][0]["value"],
        "{\"retentionDays\":30}"
    );
}

#[test]
fn retention_reclaims_free_pages_at_a_controlled_boundary() {
    let database = Database::new("retention-compaction");
    RuntimeStore::open(&database.path).unwrap();
    let now = 1_784_130_000_000_i64;
    let expired_at = now - 31 * 86_400_000;

    let mut connection = Connection::open(&database.path).unwrap();
    let transaction = connection.transaction().unwrap();
    for index in 0..2_000 {
        let session_id = format!("expired-{index}");
        transaction
            .execute(
                "INSERT INTO sessions(
                    id, provider, provider_session_id, exec_state, started_at, last_event_at
                 ) VALUES (?1, 'claude', ?1, 'idle', ?2, ?2)",
                (&session_id, expired_at),
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO events(id, session_id, provider, type, occurred_at, ingest_seq)
                 VALUES (?1 || '-event', ?1, 'claude', 'session.started', ?2, ?3)",
                (&session_id, expired_at, index),
            )
            .unwrap();
    }
    transaction.commit().unwrap();
    let pages_before = connection
        .query_row("PRAGMA page_count", [], |row| row.get::<_, i64>(0))
        .unwrap();
    drop(connection);

    let store = RuntimeStore::open(&database.path).unwrap();
    assert_eq!(store.prune_events(30, now as u64).unwrap(), 2_000);
    drop(store);

    let connection = Connection::open(&database.path).unwrap();
    let pages_after = connection
        .query_row("PRAGMA page_count", [], |row| row.get::<_, i64>(0))
        .unwrap();
    let freelist_after = connection
        .query_row("PRAGMA freelist_count", [], |row| row.get::<_, i64>(0))
        .unwrap();
    assert!(
        pages_after < pages_before,
        "pages: {pages_before} -> {pages_after}"
    );
    assert_eq!(freelist_after, 0);
    drop(connection);

    let reopened = RuntimeStore::open(&database.path).unwrap();
    assert!(reopened.snapshot().unwrap().sessions.is_empty());
}

#[test]
fn interactive_question_schema_and_secret_answers_never_enter_persistent_export() {
    let database = Database::new("interactive-privacy");
    let store = RuntimeStore::open(&database.path).unwrap();
    let request = BridgeRequest::from_hook_at(
        Provider::Claude,
        json!({
            "hook_event_name":"Elicitation",
            "session_id":"private-question",
            "message":"private form prompt 347819",
            "requested_schema":{
                "type":"object",
                "required":["password"],
                "properties":{
                    "password":{"type":"string","format":"password","description":"secret field description 347819"}
                }
            }
        }),
        100,
    );
    store.ingest(request).unwrap();
    let encoded = serde_json::to_string(&store.export_json(101).unwrap()).unwrap();
    assert!(!encoded.contains("private form prompt 347819"));
    assert!(!encoded.contains("secret field description 347819"));
    assert!(!encoded.contains("requested_schema"));
    assert!(encoded.contains("Claude needs more information"));
}

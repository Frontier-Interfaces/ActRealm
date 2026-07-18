use flow_agent_core::{BridgeRequest, Provider};
use flow_agent_runtime::{QuotaRecord, RuntimeStore};
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
            "flow-agent-m4-data-{name}-{}-{}",
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
            used_pct: 101.0,
            resets_at: 1,
            source: "rollout_experimental".to_owned(),
            captured_at: 1,
        }])
        .is_err());
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
    assert!(encoded.contains("Claude 需要补充信息"));
}

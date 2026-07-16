use flow_agent_core::{BridgeRequest, Provider, PERMISSION_COMMIT_DELAY_MS};
use flow_agent_runtime::{ApprovalAction, MetricEvent, RuntimeStore};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

struct Database {
    root: PathBuf,
}

impl Database {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "flow-agent-m5-metrics-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn open(&self) -> RuntimeStore {
        RuntimeStore::open(self.root.join("data.sqlite")).unwrap()
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn event(name: &str, session: &str, received_at: u64) -> BridgeRequest {
    BridgeRequest::from_hook_at(
        Provider::Claude,
        json!({
            "hook_event_name": name,
            "session_id": session,
            "turn_id": "turn-metrics",
            "tool_name": "Bash",
            "tool_input": {"command": "cargo test"}
        }),
        received_at,
    )
}

#[test]
fn factual_metrics_are_idempotent_and_export_the_plan_definitions() {
    let database = Database::new();
    let store = database.open();
    let now = now_millis();

    let started = event("SessionStart", "metrics-session", now);
    assert!(store.ingest(started.clone()).unwrap().inserted);
    assert!(!store.ingest(started).unwrap().inserted);

    let first = event("PermissionRequest", "metrics-session", now + 100);
    let first_request = first.request_id.unwrap();
    assert!(store.ingest(first.clone()).unwrap().inserted);
    assert!(!store.ingest(first).unwrap().inserted);

    let undone = Uuid::now_v7();
    store
        .claim_approval(undone, first_request, ApprovalAction::Approve, now + 600)
        .unwrap();
    store.undo(undone, now + 700).unwrap();

    let denial = Uuid::now_v7();
    store
        .claim_approval(denial, first_request, ApprovalAction::Deny, now + 1_100)
        .unwrap();
    store
        .commit(denial, now + 1_100 + PERMISSION_COMMIT_DELAY_MS, true)
        .unwrap();
    store
        .commit(denial, now + 1_100 + PERMISSION_COMMIT_DELAY_MS, true)
        .unwrap();

    let second = event("PermissionRequest", "metrics-session", now + 2_000);
    let second_request = second.request_id.unwrap();
    store.ingest(second).unwrap();
    let pass = Uuid::now_v7();
    store
        .claim_approval(
            pass,
            second_request,
            ApprovalAction::PassThrough,
            now + 2_500,
        )
        .unwrap();
    assert!(
        !store
            .claim_approval(
                pass,
                second_request,
                ApprovalAction::PassThrough,
                now + 2_500,
            )
            .unwrap()
            .created
    );

    let third = event("PermissionRequest", "metrics-session", now + 3_000);
    let third_request = third.request_id.unwrap();
    store.ingest(third).unwrap();
    assert!(store
        .expire_approval(third_request, "deadline", now + 4_000)
        .unwrap());
    assert!(!store
        .expire_approval(third_request, "deadline", now + 4_000)
        .unwrap());

    store.record_metric(MetricEvent::AppOpened, now).unwrap();
    store.record_metric(MetricEvent::BannerShown, now).unwrap();
    store.record_metric(MetricEvent::BannerShown, now).unwrap();

    let metrics = store.snapshot().unwrap().metrics;
    assert_eq!(metrics.active_days, 1);
    assert_eq!(metrics.approval_requests, 3);
    assert_eq!(metrics.widget_approvals, 0);
    assert_eq!(metrics.widget_denials, 1);
    assert_eq!(metrics.pass_through_manual, 1);
    assert_eq!(metrics.pass_through_timeout, 1);
    assert_eq!(metrics.decision_response_ms_total, 1_500);
    assert_eq!(metrics.decision_response_count, 2);
    assert_eq!(metrics.banners_shown, 2);
    assert_eq!(metrics.sessions_observed, 1);
    assert_eq!(metrics.app_opened, 1);
    assert_eq!(metrics.today_widget_decisions, 1);

    let export: Value = store.export_json(now + 5_000).unwrap();
    let rows = export["tables"]["metrics_daily"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["approval_requests"], 3);
    assert_eq!(rows[0]["widget_denials"], 1);
    assert_eq!(rows[0]["pass_through_manual"], 1);
    assert_eq!(rows[0]["pass_through_timeout"], 1);
    assert_eq!(rows[0]["decision_response_ms_total"], 1_500);
    assert_eq!(rows[0]["decision_response_count"], 2);

    let metrics_only = store.export_metrics_json(now + 5_000).unwrap();
    assert_eq!(metrics_only["scope"], "metrics_only");
    assert_eq!(metrics_only["metricsDaily"][0]["approvalRequests"], 3);
    let encoded = serde_json::to_string(&metrics_only).unwrap();
    for forbidden in [
        "\"sessions\":",
        "\"events\":",
        "\"attention\":",
        "\"commands\":",
        "\"project\":",
        "\"path\":",
    ] {
        assert!(
            !encoded.contains(forbidden),
            "metrics export leaked {forbidden}"
        );
    }
}

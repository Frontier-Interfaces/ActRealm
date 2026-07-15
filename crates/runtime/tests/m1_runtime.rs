#![cfg(unix)]

use flow_agent_core::{BridgeRequest, Decision, Provider, ReplyAction};
use flow_agent_runtime::{
    ApprovalAction, CommandState, EventSpool, InstanceError, RuntimeInstanceGuard, RuntimeStore,
    SpoolError, StoreError, WaiterRegistry,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "flow-agent-m1-{name}-{}-{}",
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
    store
        .ingest(request_at(
            Provider::Codex,
            "PostToolUse",
            "confirmation-session",
            Some("turn-1"),
            None,
            5_001,
        ))
        .unwrap();
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.commands[0].state, "confirmed");
    assert_eq!(snapshot.attention[0].state, "resolved");
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
    {
        let store = RuntimeStore::open(&database).unwrap();
        store.ingest(permission).unwrap();
    }
    let reopened = RuntimeStore::open(&database).unwrap();
    assert_eq!(
        reopened.reconcile_orphaned_approvals(Vec::new(), 20_000),
        Ok(1)
    );
    assert_eq!(reopened.snapshot().unwrap().attention[0].state, "expired");
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

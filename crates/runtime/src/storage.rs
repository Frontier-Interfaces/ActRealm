use crate::fsutil::ensure_private_directory;
use crate::title::{
    resolve_codex_session_titles, resolve_event_title, resolve_session_title, ProviderTitle,
};
use actrealm_core::{BridgeRequest, Decision, EventKind, Provider, PERMISSION_COMMIT_DELAY_MS};
use actrealm_providers::parse_hook;
use rusqlite::types::ValueRef;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Number, Value};
use std::collections::HashSet;
use std::env;
use std::fs::{self, OpenOptions};
use std::io;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use uuid::Uuid;

const SCHEMA_VERSION: i64 = 7;
const MAX_TASK_TITLE_CHARS: usize = 64;
const PROVIDER_TITLE_REFRESH_INTERVAL_MS: u64 = 2_000;
const PROVIDER_TITLE_ACTIVE_WINDOW_MS: u64 = 30 * 60 * 1_000;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StoreError {
    #[error("storage failed: {0}")]
    Storage(String),
    #[error("provider payload failed validation: {0}")]
    Provider(String),
    #[error("runtime storage writer stopped")]
    WriterStopped,
    #[error("approval is stale or already claimed")]
    StaleApproval,
    #[error("command was not found")]
    CommandNotFound,
    #[error("command can no longer be undone")]
    NotUndoable,
    #[error("command commit delay has not elapsed")]
    CommitTooEarly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalAction {
    Approve,
    Deny,
    PassThrough,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionAction {
    Ack,
    Snooze,
    Dismiss,
}

impl AttentionAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ack => "ack",
            Self::Snooze => "snooze",
            Self::Dismiss => "dismiss",
        }
    }
}

impl ApprovalAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Deny => "deny",
            Self::PassThrough => "pass_through",
        }
    }

    fn parse(value: &str) -> Result<Self, StoreError> {
        match value {
            "approve" => Ok(Self::Approve),
            "deny" => Ok(Self::Deny),
            "pass_through" => Ok(Self::PassThrough),
            other => Err(StoreError::Storage(format!(
                "invalid approval action {other}"
            ))),
        }
    }

    pub fn decision(self) -> Option<Decision> {
        match self {
            Self::Approve => Some(Decision::Allow),
            Self::Deny => Some(Decision::Deny),
            Self::PassThrough => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandState {
    PendingCommit,
    DecisionSent,
    Confirmed,
    PassedThrough,
    Undone,
    Failed,
}

impl CommandState {
    fn parse(value: &str) -> Result<Self, StoreError> {
        match value {
            "pending_commit" => Ok(Self::PendingCommit),
            "decision_sent" => Ok(Self::DecisionSent),
            "confirmed" => Ok(Self::Confirmed),
            "passed_through" => Ok(Self::PassedThrough),
            "undone" => Ok(Self::Undone),
            "failed" => Ok(Self::Failed),
            other => Err(StoreError::Storage(format!(
                "invalid command state {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestResult {
    pub inserted: bool,
    pub session_id: String,
    pub attention_id: Option<String>,
    pub kind: EventKind,
    pub resolved_request_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeApprovalSyncResult {
    pub session_found: bool,
    pub resolved_request_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimResult {
    pub created: bool,
    pub command_id: Uuid,
    pub attention_id: String,
    pub request_id: Uuid,
    pub action: ApprovalAction,
    pub state: CommandState,
    pub commit_due_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitResult {
    pub command_id: Uuid,
    pub request_id: Uuid,
    pub action: ApprovalAction,
    pub state: CommandState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub id: String,
    pub provider: String,
    pub provider_session_id: String,
    pub project: Option<String>,
    pub title: Option<String>,
    pub provider_title: Option<String>,
    pub provider_title_source: Option<String>,
    pub model: Option<String>,
    pub exec_state: String,
    pub approval_owner: Option<String>,
    pub activity: Option<String>,
    pub activity_since: Option<u64>,
    pub plan_done: Option<u32>,
    pub plan_total: Option<u32>,
    pub turn_started_at: Option<u64>,
    pub turn_ended_at: Option<u64>,
    pub token_total: Option<u64>,
    pub context_window_tokens: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub last_turn_tokens: Option<u64>,
    pub context_used_tokens: Option<u64>,
    pub context_used_percent: Option<u32>,
    pub estimated_cost_usd_micros: Option<u64>,
    pub cost_kind: Option<String>,
    pub pricing_source: Option<String>,
    pub usage_source: Option<String>,
    pub usage_quality: Option<String>,
    pub usage_captured_at: Option<u64>,
    pub permission_mode: Option<String>,
    pub current_tool: Option<String>,
    pub active_subagents: u32,
    pub provider_turn_id: Option<String>,
    pub environment: Option<String>,
    pub jump_capability: String,
    pub jump_label: String,
    #[serde(skip)]
    pub term_app: Option<String>,
    #[serde(skip)]
    pub term_session_id: Option<String>,
    #[serde(skip)]
    pub term_tty: Option<String>,
    #[serde(skip)]
    pub term_bundle_id: Option<String>,
    #[serde(skip)]
    pub term_surface: Option<String>,
    #[serde(skip)]
    pub provider_pid: Option<u32>,
    pub last_event_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttentionRecord {
    pub id: String,
    pub session_id: String,
    pub provider: String,
    pub project: Option<String>,
    pub request_id: Option<Uuid>,
    pub kind: String,
    pub title: String,
    pub detail: Option<String>,
    pub state: String,
    pub risk: String,
    pub risk_notes: Vec<String>,
    pub command_preview: Option<String>,
    pub expires_at: Option<u64>,
    pub created_at: u64,
    pub resolution: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandRecord {
    pub id: Uuid,
    pub attention_id: String,
    pub request_id: Option<Uuid>,
    pub action: String,
    pub state: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreSnapshot {
    pub sessions: Vec<SessionRecord>,
    pub attention: Vec<AttentionRecord>,
    pub commands: Vec<CommandRecord>,
    pub event_count: u64,
    pub metrics: MetricsSummary,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsSummary {
    pub active_days: u64,
    pub approval_requests: u64,
    pub widget_approvals: u64,
    pub widget_denials: u64,
    pub pass_through_manual: u64,
    pub pass_through_timeout: u64,
    pub decision_response_ms_total: u64,
    pub decision_response_count: u64,
    pub banners_shown: u64,
    pub sessions_observed: u64,
    pub app_opened: u64,
    pub today_widget_decisions: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricEvent {
    AppOpened,
    BannerShown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaRecord {
    pub provider: String,
    pub window: String,
    pub used_pct: f64,
    pub resets_at: u64,
    pub source: String,
    pub captured_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionUsageRecord {
    pub provider: String,
    pub provider_session_id: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub token_total: Option<u64>,
    pub last_turn_tokens: Option<u64>,
    pub context_used_tokens: Option<u64>,
    pub context_window_tokens: Option<u64>,
    pub context_used_percent: Option<u32>,
    pub estimated_cost_usd_micros: Option<u64>,
    pub cost_kind: Option<String>,
    pub pricing_source: Option<String>,
    pub usage_source: String,
    pub usage_quality: String,
    pub captured_at: u64,
}

#[derive(Clone)]
pub struct RuntimeStore {
    inner: Arc<StoreInner>,
}

struct StoreInner {
    sender: mpsc::Sender<StoreMessage>,
    writer: Mutex<Option<thread::JoinHandle<()>>>,
}

enum StoreMessage {
    Ingest {
        request: Box<BridgeRequest>,
        reply: mpsc::SyncSender<Result<IngestResult, StoreError>>,
    },
    Claim {
        command_id: Uuid,
        request_id: Uuid,
        action: ApprovalAction,
        now: u64,
        reply: mpsc::SyncSender<Result<ClaimResult, StoreError>>,
    },
    Undo {
        command_id: Uuid,
        now: u64,
        reply: mpsc::SyncSender<Result<CommandState, StoreError>>,
    },
    Commit {
        command_id: Uuid,
        now: u64,
        waiter_active: bool,
        reply: mpsc::SyncSender<Result<CommitResult, StoreError>>,
    },
    ActAttention {
        command_id: Uuid,
        attention_id: String,
        action: AttentionAction,
        now: u64,
        reply: mpsc::SyncSender<Result<CommandState, StoreError>>,
    },
    Reconcile {
        active_request_ids: Vec<Uuid>,
        now: u64,
        reply: mpsc::SyncSender<Result<usize, StoreError>>,
    },
    ExpireApproval {
        request_id: Uuid,
        reason: String,
        now: u64,
        reply: mpsc::SyncSender<Result<bool, StoreError>>,
    },
    ReconcileSessions {
        active_sessions: Vec<(Provider, String)>,
        now: u64,
        idle_after_ms: u64,
        reply: mpsc::SyncSender<Result<usize, StoreError>>,
    },
    SyncNativeApproval {
        provider: Provider,
        provider_session_id: String,
        waiting: bool,
        active: bool,
        now: u64,
        reply: mpsc::SyncSender<Result<NativeApprovalSyncResult, StoreError>>,
    },
    Snapshot {
        reply: mpsc::SyncSender<Result<StoreSnapshot, StoreError>>,
    },
    ReadSetting {
        key: String,
        reply: mpsc::SyncSender<Result<Option<String>, StoreError>>,
    },
    WriteSetting {
        key: String,
        value: String,
        reply: mpsc::SyncSender<Result<(), StoreError>>,
    },
    ReplaceQuota {
        entries: Vec<QuotaRecord>,
        reply: mpsc::SyncSender<Result<(), StoreError>>,
    },
    UpsertSessionUsage {
        records: Vec<SessionUsageRecord>,
        reply: mpsc::SyncSender<Result<(), StoreError>>,
    },
    RecordMetric {
        event: MetricEvent,
        now: u64,
        reply: mpsc::SyncSender<Result<(), StoreError>>,
    },
    PruneEvents {
        retention_days: u32,
        now: u64,
        reply: mpsc::SyncSender<Result<usize, StoreError>>,
    },
    Export {
        now: u64,
        reply: mpsc::SyncSender<Result<Value, StoreError>>,
    },
    ExportMetrics {
        now: u64,
        reply: mpsc::SyncSender<Result<Value, StoreError>>,
    },
    ClearData {
        reply: mpsc::SyncSender<Result<(), StoreError>>,
    },
    Shutdown,
}

pub fn default_database_path() -> PathBuf {
    if let Some(root) = env::var_os("ACTREALM_HOME") {
        return PathBuf::from(root).join("data.sqlite");
    }
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".actrealm/data.sqlite")
}

impl RuntimeStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let path = path.into();
        prepare_database_file(&path)?;
        let connection = Connection::open(&path).map_err(storage_error)?;
        initialize(&connection)?;
        let (sender, receiver) = mpsc::channel();
        let writer = thread::Builder::new()
            .name("actrealm-sqlite-writer".to_owned())
            .spawn(move || writer_loop(connection, path, receiver))
            .map_err(|error| StoreError::Storage(error.to_string()))?;
        Ok(Self {
            inner: Arc::new(StoreInner {
                sender,
                writer: Mutex::new(Some(writer)),
            }),
        })
    }

    pub fn ingest(&self, request: BridgeRequest) -> Result<IngestResult, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::Ingest {
            request: Box::new(request),
            reply,
        })?;
        receive(receiver)
    }

    pub fn claim_approval(
        &self,
        command_id: Uuid,
        request_id: Uuid,
        action: ApprovalAction,
        now: u64,
    ) -> Result<ClaimResult, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::Claim {
            command_id,
            request_id,
            action,
            now,
            reply,
        })?;
        receive(receiver)
    }

    pub fn undo(&self, command_id: Uuid, now: u64) -> Result<CommandState, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::Undo {
            command_id,
            now,
            reply,
        })?;
        receive(receiver)
    }

    pub fn commit(
        &self,
        command_id: Uuid,
        now: u64,
        waiter_active: bool,
    ) -> Result<CommitResult, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::Commit {
            command_id,
            now,
            waiter_active,
            reply,
        })?;
        receive(receiver)
    }

    pub fn act_on_attention(
        &self,
        command_id: Uuid,
        attention_id: impl Into<String>,
        action: AttentionAction,
        now: u64,
    ) -> Result<CommandState, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::ActAttention {
            command_id,
            attention_id: attention_id.into(),
            action,
            now,
            reply,
        })?;
        receive(receiver)
    }

    pub fn reconcile_orphaned_approvals(
        &self,
        active_request_ids: Vec<Uuid>,
        now: u64,
    ) -> Result<usize, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::Reconcile {
            active_request_ids,
            now,
            reply,
        })?;
        receive(receiver)
    }

    pub fn expire_approval(
        &self,
        request_id: Uuid,
        reason: impl Into<String>,
        now: u64,
    ) -> Result<bool, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::ExpireApproval {
            request_id,
            reason: reason.into(),
            now,
            reply,
        })?;
        receive(receiver)
    }

    pub fn reconcile_session_liveness(
        &self,
        active_sessions: Vec<(Provider, String)>,
        now: u64,
        idle_after_ms: u64,
    ) -> Result<usize, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::ReconcileSessions {
            active_sessions,
            now,
            idle_after_ms,
            reply,
        })?;
        receive(receiver)
    }

    pub fn sync_native_approval(
        &self,
        provider: Provider,
        provider_session_id: impl Into<String>,
        waiting: bool,
        active: bool,
        now: u64,
    ) -> Result<NativeApprovalSyncResult, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::SyncNativeApproval {
            provider,
            provider_session_id: provider_session_id.into(),
            waiting,
            active,
            now,
            reply,
        })?;
        receive(receiver)
    }

    pub fn snapshot(&self) -> Result<StoreSnapshot, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::Snapshot { reply })?;
        receive(receiver)
    }

    pub fn read_setting(&self, key: impl Into<String>) -> Result<Option<String>, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::ReadSetting {
            key: key.into(),
            reply,
        })?;
        receive(receiver)
    }

    pub fn write_setting(
        &self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<(), StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::WriteSetting {
            key: key.into(),
            value: value.into(),
            reply,
        })?;
        receive(receiver)
    }

    pub fn replace_quota_snapshots(&self, entries: Vec<QuotaRecord>) -> Result<(), StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::ReplaceQuota { entries, reply })?;
        receive(receiver)
    }

    pub fn upsert_session_usage(&self, record: SessionUsageRecord) -> Result<(), StoreError> {
        self.upsert_session_usages(vec![record])
    }

    pub fn upsert_session_usages(
        &self,
        records: Vec<SessionUsageRecord>,
    ) -> Result<(), StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::UpsertSessionUsage { records, reply })?;
        receive(receiver)
    }

    pub fn record_metric(&self, event: MetricEvent, now: u64) -> Result<(), StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::RecordMetric { event, now, reply })?;
        receive(receiver)
    }

    pub fn prune_events(&self, retention_days: u32, now: u64) -> Result<usize, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::PruneEvents {
            retention_days,
            now,
            reply,
        })?;
        receive(receiver)
    }

    pub fn export_json(&self, now: u64) -> Result<Value, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::Export { now, reply })?;
        receive(receiver)
    }

    pub fn export_metrics_json(&self, now: u64) -> Result<Value, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::ExportMetrics { now, reply })?;
        receive(receiver)
    }

    pub fn clear_data(&self) -> Result<(), StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::ClearData { reply })?;
        receive(receiver)
    }

    fn send(&self, message: StoreMessage) -> Result<(), StoreError> {
        self.inner
            .sender
            .send(message)
            .map_err(|_| StoreError::WriterStopped)
    }
}

impl Drop for StoreInner {
    fn drop(&mut self) {
        let _ = self.sender.send(StoreMessage::Shutdown);
        if let Ok(writer) = self.writer.get_mut() {
            if let Some(writer) = writer.take() {
                let _ = writer.join();
            }
        }
    }
}

fn receive<T>(receiver: mpsc::Receiver<Result<T, StoreError>>) -> Result<T, StoreError> {
    receiver.recv().map_err(|_| StoreError::WriterStopped)?
}

fn writer_loop(
    mut connection: Connection,
    database_path: PathBuf,
    receiver: mpsc::Receiver<StoreMessage>,
) {
    let mut last_provider_title_refresh_at = 0;
    while let Ok(message) = receiver.recv() {
        match message {
            StoreMessage::Ingest { request, reply } => {
                let _ = reply.send(ingest_transaction(&mut connection, *request));
            }
            StoreMessage::Claim {
                command_id,
                request_id,
                action,
                now,
                reply,
            } => {
                let _ = reply.send(claim_transaction(
                    &mut connection,
                    command_id,
                    request_id,
                    action,
                    now,
                ));
            }
            StoreMessage::Undo {
                command_id,
                now,
                reply,
            } => {
                let _ = reply.send(undo_transaction(&mut connection, command_id, now));
            }
            StoreMessage::Commit {
                command_id,
                now,
                waiter_active,
                reply,
            } => {
                let _ = reply.send(commit_transaction(
                    &mut connection,
                    command_id,
                    now,
                    waiter_active,
                ));
            }
            StoreMessage::ActAttention {
                command_id,
                attention_id,
                action,
                now,
                reply,
            } => {
                let _ = reply.send(act_attention_transaction(
                    &mut connection,
                    command_id,
                    &attention_id,
                    action,
                    now,
                ));
            }
            StoreMessage::Reconcile {
                active_request_ids,
                now,
                reply,
            } => {
                let _ = reply.send(reconcile_transaction(
                    &mut connection,
                    active_request_ids,
                    now,
                ));
            }
            StoreMessage::ExpireApproval {
                request_id,
                reason,
                now,
                reply,
            } => {
                let _ = reply.send(expire_approval_transaction(
                    &mut connection,
                    request_id,
                    &reason,
                    now,
                ));
            }
            StoreMessage::ReconcileSessions {
                active_sessions,
                now,
                idle_after_ms,
                reply,
            } => {
                let _ = reply.send(reconcile_sessions_transaction(
                    &mut connection,
                    active_sessions,
                    now,
                    idle_after_ms,
                ));
            }
            StoreMessage::SyncNativeApproval {
                provider,
                provider_session_id,
                waiting,
                active,
                now,
                reply,
            } => {
                let _ = reply.send(sync_native_approval_transaction(
                    &mut connection,
                    provider,
                    &provider_session_id,
                    waiting,
                    active,
                    now,
                ));
            }
            StoreMessage::Snapshot { reply } => {
                let now = now_millis();
                let result = reopen_due_snoozed(&mut connection, now).and_then(|_| {
                    if now.saturating_sub(last_provider_title_refresh_at)
                        >= PROVIDER_TITLE_REFRESH_INTERVAL_MS
                    {
                        refresh_provider_titles(&mut connection, now)?;
                        last_provider_title_refresh_at = now;
                    }
                    read_snapshot(&connection)
                });
                let _ = reply.send(result);
            }
            StoreMessage::ReadSetting { key, reply } => {
                let result = connection
                    .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                        row.get::<_, String>(0)
                    })
                    .optional()
                    .map_err(storage_error);
                let _ = reply.send(result);
            }
            StoreMessage::WriteSetting { key, value, reply } => {
                let result = connection
                    .execute(
                        "INSERT INTO settings(key, value) VALUES (?1, ?2)
                         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                        params![key, value],
                    )
                    .map(|_| ())
                    .map_err(storage_error);
                let _ = reply.send(result);
            }
            StoreMessage::ReplaceQuota { entries, reply } => {
                let _ = reply.send(replace_quota_transaction(&mut connection, entries));
            }
            StoreMessage::UpsertSessionUsage { records, reply } => {
                let _ = reply.send(upsert_session_usages(&mut connection, records));
            }
            StoreMessage::RecordMetric { event, now, reply } => {
                let _ = reply.send(record_metric_transaction(&mut connection, event, now));
            }
            StoreMessage::PruneEvents {
                retention_days,
                now,
                reply,
            } => {
                let _ = reply.send(prune_events_transaction(
                    &mut connection,
                    retention_days,
                    now,
                ));
            }
            StoreMessage::Export { now, reply } => {
                let _ = reply.send(export_database(&connection, now));
            }
            StoreMessage::ExportMetrics { now, reply } => {
                let _ = reply.send(export_metrics_database(&connection, now));
            }
            StoreMessage::ClearData { reply } => {
                let _ = reply.send(reset_database(&mut connection, &database_path));
            }
            StoreMessage::Shutdown => break,
        }
    }
}

fn upsert_session_usages(
    connection: &mut Connection,
    records: Vec<SessionUsageRecord>,
) -> Result<(), StoreError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    for record in records {
        upsert_session_usage_row(&transaction, record)?;
    }
    transaction.commit().map_err(storage_error)
}

fn upsert_session_usage_row(
    connection: &Connection,
    record: SessionUsageRecord,
) -> Result<(), StoreError> {
    if record.provider_session_id.trim().is_empty() || record.provider_session_id.len() > 128 {
        return Err(StoreError::Storage(
            "usage session identifier is invalid".to_owned(),
        ));
    }
    connection
        .execute(
            "INSERT INTO session_usage(
               provider, provider_session_id,
               input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
               reasoning_tokens, token_total, last_turn_tokens, context_used_tokens,
               context_window_tokens, context_used_percent, estimated_cost_usd_micros,
               cost_kind, pricing_source, usage_source, usage_quality, captured_at
             ) VALUES (
               ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
               ?14, ?15, ?16, ?17, ?18
             )
             ON CONFLICT(provider, provider_session_id) DO UPDATE SET
               input_tokens = COALESCE(excluded.input_tokens, input_tokens),
               output_tokens = COALESCE(excluded.output_tokens, output_tokens),
               cache_read_tokens = COALESCE(excluded.cache_read_tokens, cache_read_tokens),
               cache_creation_tokens = COALESCE(excluded.cache_creation_tokens, cache_creation_tokens),
               reasoning_tokens = COALESCE(excluded.reasoning_tokens, reasoning_tokens),
               token_total = COALESCE(excluded.token_total, token_total),
               last_turn_tokens = COALESCE(excluded.last_turn_tokens, last_turn_tokens),
               context_used_tokens = COALESCE(excluded.context_used_tokens, context_used_tokens),
               context_window_tokens = COALESCE(excluded.context_window_tokens, context_window_tokens),
               context_used_percent = COALESCE(excluded.context_used_percent, context_used_percent),
               estimated_cost_usd_micros = COALESCE(excluded.estimated_cost_usd_micros, estimated_cost_usd_micros),
               cost_kind = COALESCE(excluded.cost_kind, cost_kind),
               pricing_source = COALESCE(excluded.pricing_source, pricing_source),
               usage_source = excluded.usage_source,
               usage_quality = excluded.usage_quality,
               captured_at = MAX(captured_at, excluded.captured_at)",
            params![
                record.provider,
                record.provider_session_id,
                record.input_tokens.map(to_i64),
                record.output_tokens.map(to_i64),
                record.cache_read_tokens.map(to_i64),
                record.cache_creation_tokens.map(to_i64),
                record.reasoning_tokens.map(to_i64),
                record.token_total.map(to_i64),
                record.last_turn_tokens.map(to_i64),
                record.context_used_tokens.map(to_i64),
                record.context_window_tokens.map(to_i64),
                record.context_used_percent.map(i64::from),
                record.estimated_cost_usd_micros.map(to_i64),
                record.cost_kind,
                record.pricing_source,
                record.usage_source,
                record.usage_quality,
                to_i64(record.captured_at),
            ],
        )
        .map(|_| ())
        .map_err(storage_error)
}

fn replace_quota_transaction(
    connection: &mut Connection,
    entries: Vec<QuotaRecord>,
) -> Result<(), StoreError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    transaction
        .execute("DELETE FROM quota_snapshots", [])
        .map_err(storage_error)?;
    for entry in entries {
        if !entry.used_pct.is_finite() || !(0.0..=100.0).contains(&entry.used_pct) {
            return Err(StoreError::Storage(
                "quota percentage is outside 0..=100".to_owned(),
            ));
        }
        transaction
            .execute(
                "INSERT INTO quota_snapshots(
                   provider, window, used_pct, resets_at, source, captured_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    entry.provider,
                    entry.window,
                    entry.used_pct,
                    to_i64(entry.resets_at),
                    entry.source,
                    to_i64(entry.captured_at),
                ],
            )
            .map_err(storage_error)?;
    }
    transaction.commit().map_err(storage_error)
}

fn record_metric_transaction(
    connection: &mut Connection,
    event: MetricEvent,
    now: u64,
) -> Result<(), StoreError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    ensure_metric_day(&transaction, now)?;
    let column = match event {
        MetricEvent::AppOpened => "app_opened",
        MetricEvent::BannerShown => "banners_shown",
    };
    transaction
        .execute(
            &format!("UPDATE metrics_daily SET {column} = {column} + 1 WHERE day = ?1"),
            [metric_day(now)],
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)
}

fn ensure_metric_day(transaction: &Transaction<'_>, now: u64) -> Result<(), StoreError> {
    transaction
        .execute(
            "INSERT INTO metrics_daily(day) VALUES (?1)
             ON CONFLICT(day) DO NOTHING",
            [metric_day(now)],
        )
        .map(|_| ())
        .map_err(storage_error)
}

fn increment_ingest_metrics(
    transaction: &Transaction<'_>,
    now: u64,
    new_session: bool,
    approval_request: bool,
) -> Result<(), StoreError> {
    if !new_session && !approval_request {
        return Ok(());
    }
    ensure_metric_day(transaction, now)?;
    transaction
        .execute(
            "UPDATE metrics_daily SET
               approval_requests = approval_requests + ?2,
               sessions_observed = sessions_observed + ?3
             WHERE day = ?1",
            params![
                metric_day(now),
                i64::from(approval_request),
                i64::from(new_session)
            ],
        )
        .map(|_| ())
        .map_err(storage_error)
}

fn increment_decision_metrics(
    transaction: &Transaction<'_>,
    action: ApprovalAction,
    response_ms: u64,
    now: u64,
) -> Result<(), StoreError> {
    ensure_metric_day(transaction, now)?;
    let column = match action {
        ApprovalAction::Approve => "widget_approvals",
        ApprovalAction::Deny => "widget_denials",
        ApprovalAction::PassThrough => "pass_through_manual",
    };
    transaction
        .execute(
            &format!(
                "UPDATE metrics_daily SET
                   {column} = {column} + 1,
                   decision_response_ms_total = decision_response_ms_total + ?2,
                   decision_response_count = decision_response_count + 1
                 WHERE day = ?1"
            ),
            params![metric_day(now), to_i64(response_ms)],
        )
        .map(|_| ())
        .map_err(storage_error)
}

fn metric_day(now: u64) -> String {
    let seconds: libc::time_t = (now / 1_000).try_into().unwrap_or(libc::time_t::MAX);
    let mut local = std::mem::MaybeUninit::<libc::tm>::zeroed();
    // SAFETY: local points to writable tm storage and seconds remains alive for the call.
    let result = unsafe { libc::localtime_r(&seconds, local.as_mut_ptr()) };
    if result.is_null() {
        return "1970-01-01".to_owned();
    }
    // SAFETY: localtime_r returned non-null and initialized the tm value.
    let local = unsafe { local.assume_init() };
    format!(
        "{:04}-{:02}-{:02}",
        local.tm_year + 1900,
        local.tm_mon + 1,
        local.tm_mday
    )
}

fn prune_events_transaction(
    connection: &mut Connection,
    retention_days: u32,
    now: u64,
) -> Result<usize, StoreError> {
    if !matches!(retention_days, 0 | 30 | 90 | 180 | 365) {
        return Err(StoreError::Storage(
            "retention days must be 0, 30, 90, 180, or 365".to_owned(),
        ));
    }
    if retention_days == 0 {
        return Ok(0);
    }
    let cutoff = now.saturating_sub(u64::from(retention_days) * 86_400_000);
    connection
        .execute(
            "DELETE FROM events WHERE occurred_at < ?1",
            [to_i64(cutoff)],
        )
        .map_err(storage_error)
}

fn export_database(connection: &Connection, now: u64) -> Result<Value, StoreError> {
    let mut names_statement = connection
        .prepare(
            "SELECT name FROM sqlite_schema
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
             ORDER BY name",
        )
        .map_err(storage_error)?;
    let table_names = names_statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    drop(names_statement);
    let mut tables = Map::new();
    for table in table_names {
        if !table
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            return Err(StoreError::Storage(
                "database contains an unsafe table name".to_owned(),
            ));
        }
        let mut statement = connection
            .prepare(&format!("SELECT * FROM \"{table}\""))
            .map_err(storage_error)?;
        let columns = statement
            .column_names()
            .into_iter()
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        let rows = statement
            .query_map([], |row| {
                let mut object = Map::new();
                for (index, column) in columns.iter().enumerate() {
                    object.insert(column.clone(), sqlite_value(row.get_ref(index)?));
                }
                Ok(Value::Object(object))
            })
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        tables.insert(table, Value::Array(rows));
    }
    Ok(Value::Object(Map::from_iter([
        ("schemaVersion".to_owned(), Value::Number(1.into())),
        ("exportedAt".to_owned(), Value::Number(now.into())),
        ("tables".to_owned(), Value::Object(tables)),
    ])))
}

fn export_metrics_database(connection: &Connection, now: u64) -> Result<Value, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT day, approval_requests, widget_approvals, widget_denials,
                    pass_through_manual, pass_through_timeout,
                    decision_response_ms_total, decision_response_count,
                    banners_shown, sessions_observed, app_opened
             FROM metrics_daily ORDER BY day",
        )
        .map_err(storage_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok(json!({
                "day": row.get::<_, String>(0)?,
                "approvalRequests": from_i64(row.get(1)?),
                "widgetApprovals": from_i64(row.get(2)?),
                "widgetDenials": from_i64(row.get(3)?),
                "passThroughManual": from_i64(row.get(4)?),
                "passThroughTimeout": from_i64(row.get(5)?),
                "decisionResponseMsTotal": from_i64(row.get(6)?),
                "decisionResponseCount": from_i64(row.get(7)?),
                "bannersShown": from_i64(row.get(8)?),
                "sessionsObserved": from_i64(row.get(9)?),
                "appOpened": from_i64(row.get(10)?),
            }))
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    Ok(json!({
        "schemaVersion": 1,
        "appVersion": env!("CARGO_PKG_VERSION"),
        "exportedAt": now,
        "scope": "metrics_only",
        "definitions": {
            "panelHandlingRate": "(widgetApprovals + widgetDenials) / approvalRequests",
            "terminalReturnRate": "(passThroughManual + passThroughTimeout) / approvalRequests",
            "averageResponseMs": "decisionResponseMsTotal / decisionResponseCount"
        },
        "metricsDaily": rows
    }))
}

fn sqlite_value(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(value) => Value::Number(value.into()),
        ValueRef::Real(value) => Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        ValueRef::Text(value) => Value::String(String::from_utf8_lossy(value).into_owned()),
        ValueRef::Blob(value) => Value::String(format!("hex:{}", hex(value))),
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

fn reset_database(connection: &mut Connection, path: &Path) -> Result<(), StoreError> {
    let placeholder = Connection::open_in_memory().map_err(storage_error)?;
    let old = std::mem::replace(connection, placeholder);
    if let Err((old, error)) = old.close() {
        *connection = old;
        return Err(storage_error(error));
    }
    for candidate in database_files(path) {
        match fs::remove_file(&candidate) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                let reopened = Connection::open(path).map_err(storage_error)?;
                initialize(&reopened)?;
                *connection = reopened;
                return Err(StoreError::Storage(format!(
                    "failed to remove {}: {error}",
                    candidate.display()
                )));
            }
        }
    }
    prepare_database_file(path)?;
    let fresh = Connection::open(path).map_err(storage_error)?;
    initialize(&fresh)?;
    *connection = fresh;
    Ok(())
}

fn database_files(path: &Path) -> [PathBuf; 3] {
    let mut wal = path.as_os_str().to_os_string();
    wal.push("-wal");
    let mut shm = path.as_os_str().to_os_string();
    shm.push("-shm");
    [path.to_path_buf(), PathBuf::from(wal), PathBuf::from(shm)]
}

fn prepare_database_file(path: &Path) -> Result<(), StoreError> {
    let Some(parent) = path.parent() else {
        return Err(StoreError::Storage(
            "database path has no parent".to_owned(),
        ));
    };
    let _ =
        ensure_private_directory(parent).map_err(|error| StoreError::Storage(error.to_string()))?;
    OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| StoreError::Storage(error.to_string()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| StoreError::Storage(error.to_string()))?;
    Ok(())
}

fn initialize(connection: &Connection) -> Result<(), StoreError> {
    connection
        .execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
            PRAGMA synchronous = NORMAL;
            PRAGMA busy_timeout = 1000;
            CREATE TABLE IF NOT EXISTS sessions (
              id TEXT PRIMARY KEY,
              provider TEXT NOT NULL,
              provider_session_id TEXT NOT NULL,
              cwd TEXT, project TEXT, title TEXT,
              provider_title TEXT, provider_title_source TEXT,
              model TEXT, permission_mode TEXT,
              term_app TEXT, term_session_id TEXT, term_tty TEXT, term_title TEXT,
              term_bundle_id TEXT, term_surface TEXT, provider_pid INTEGER,
              exec_state TEXT NOT NULL DEFAULT 'idle',
              approval_owner TEXT, activity TEXT, activity_since INTEGER,
              plan_done INTEGER, plan_total INTEGER,
              token_total INTEGER, context_window_tokens INTEGER,
              started_at INTEGER NOT NULL, last_event_at INTEGER NOT NULL,
              ended_at INTEGER,
              UNIQUE(provider, provider_session_id)
            );
            CREATE TABLE IF NOT EXISTS turns (
              id TEXT PRIMARY KEY,
              session_id TEXT NOT NULL,
              provider_turn_id TEXT, prompt_id TEXT, ordinal INTEGER NOT NULL,
              state TEXT NOT NULL DEFAULT 'running',
              started_at INTEGER NOT NULL, ended_at INTEGER,
              UNIQUE(session_id, ordinal),
              FOREIGN KEY(session_id) REFERENCES sessions(id)
            );
            CREATE TABLE IF NOT EXISTS events (
              id TEXT PRIMARY KEY, request_id TEXT,
              session_id TEXT NOT NULL, turn_id TEXT, provider TEXT NOT NULL,
              type TEXT NOT NULL, tool_name TEXT, summary TEXT,
              occurred_at INTEGER NOT NULL, ingest_seq INTEGER NOT NULL,
              FOREIGN KEY(session_id) REFERENCES sessions(id),
              FOREIGN KEY(turn_id) REFERENCES turns(id)
            );
            CREATE TABLE IF NOT EXISTS session_tasks (
              session_id TEXT NOT NULL,
              task_id TEXT NOT NULL,
              completed INTEGER NOT NULL DEFAULT 0,
              created_at INTEGER NOT NULL,
              completed_at INTEGER,
              PRIMARY KEY(session_id, task_id),
              FOREIGN KEY(session_id) REFERENCES sessions(id)
            );
            CREATE TABLE IF NOT EXISTS session_subagents (
              session_id TEXT NOT NULL,
              agent_id TEXT NOT NULL,
              active INTEGER NOT NULL DEFAULT 1,
              started_at INTEGER NOT NULL,
              stopped_at INTEGER,
              PRIMARY KEY(session_id, agent_id),
              FOREIGN KEY(session_id) REFERENCES sessions(id)
            );
            CREATE TABLE IF NOT EXISTS attention_items (
              id TEXT PRIMARY KEY,
              session_id TEXT NOT NULL, provider TEXT NOT NULL, project TEXT,
              turn_id TEXT, request_id TEXT UNIQUE,
              kind TEXT NOT NULL, title TEXT NOT NULL, detail TEXT,
              command_preview TEXT, risk TEXT NOT NULL, risk_notes TEXT,
              dedupe_key TEXT UNIQUE NOT NULL, state TEXT NOT NULL DEFAULT 'open',
              expires_at INTEGER, created_at INTEGER NOT NULL,
              resolved_at INTEGER, resolution TEXT,
              FOREIGN KEY(session_id) REFERENCES sessions(id),
              FOREIGN KEY(turn_id) REFERENCES turns(id)
            );
            CREATE TABLE IF NOT EXISTS commands (
              id TEXT PRIMARY KEY,
              attention_id TEXT NOT NULL, request_id TEXT,
              action TEXT NOT NULL, state TEXT NOT NULL,
              created_at INTEGER NOT NULL, sent_at INTEGER, confirmed_at INTEGER,
              error_code TEXT,
              FOREIGN KEY(attention_id) REFERENCES attention_items(id)
            );
            CREATE TABLE IF NOT EXISTS approval_stats (
              project TEXT NOT NULL, risk_class TEXT NOT NULL, category TEXT NOT NULL,
              approve_count INTEGER DEFAULT 0, deny_count INTEGER DEFAULT 0,
              last_at INTEGER, PRIMARY KEY(project, category, risk_class)
            );
            CREATE TABLE IF NOT EXISTS quota_snapshots (
              provider TEXT NOT NULL, window TEXT NOT NULL,
              used_pct REAL, resets_at INTEGER, source TEXT,
              captured_at INTEGER NOT NULL, PRIMARY KEY(provider, window)
            );
            CREATE TABLE IF NOT EXISTS session_usage (
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
            CREATE TABLE IF NOT EXISTS metrics_daily (
              day TEXT PRIMARY KEY,
              approval_requests INTEGER DEFAULT 0,
              widget_approvals INTEGER DEFAULT 0, widget_denials INTEGER DEFAULT 0,
              pass_through_manual INTEGER DEFAULT 0,
              pass_through_timeout INTEGER DEFAULT 0,
              decision_response_ms_total INTEGER DEFAULT 0,
              decision_response_count INTEGER DEFAULT 0,
              banners_shown INTEGER DEFAULT 0,
              sessions_observed INTEGER DEFAULT 0, app_opened INTEGER DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS settings (
              key TEXT PRIMARY KEY, value TEXT
            );
            CREATE INDEX IF NOT EXISTS events_session_time
              ON events(session_id, occurred_at);
            CREATE INDEX IF NOT EXISTS attention_state
              ON attention_items(state, created_at);
            "#,
        )
        .map_err(storage_error)?;
    ensure_session_title_column(connection)?;
    ensure_session_provider_title_columns(connection)?;
    ensure_session_usage_columns(connection)?;
    ensure_session_locator_columns(connection)?;
    connection
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(storage_error)?;
    Ok(())
}

fn ensure_session_title_column(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(sessions)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    if !columns.iter().any(|column| column == "title") {
        connection
            .execute("ALTER TABLE sessions ADD COLUMN title TEXT", [])
            .map_err(storage_error)?;
    }
    Ok(())
}

fn ensure_session_provider_title_columns(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(sessions)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    for column in ["provider_title", "provider_title_source"] {
        if !columns.iter().any(|existing| existing == column) {
            connection
                .execute(
                    &format!("ALTER TABLE sessions ADD COLUMN {column} TEXT"),
                    [],
                )
                .map_err(storage_error)?;
        }
    }
    Ok(())
}

fn ensure_session_usage_columns(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(sessions)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    for column in ["token_total", "context_window_tokens"] {
        if !columns.iter().any(|existing| existing == column) {
            connection
                .execute(
                    &format!("ALTER TABLE sessions ADD COLUMN {column} INTEGER"),
                    [],
                )
                .map_err(storage_error)?;
        }
    }
    Ok(())
}

fn ensure_session_locator_columns(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(sessions)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    for column in ["term_bundle_id", "term_surface", "provider_pid"] {
        if !columns.iter().any(|existing| existing == column) {
            connection
                .execute(
                    &format!(
                        "ALTER TABLE sessions ADD COLUMN {column} {}",
                        if column == "provider_pid" {
                            "INTEGER"
                        } else {
                            "TEXT"
                        }
                    ),
                    [],
                )
                .map_err(storage_error)?;
        }
    }
    Ok(())
}

fn ingest_transaction(
    connection: &mut Connection,
    request: BridgeRequest,
) -> Result<IngestResult, StoreError> {
    let parsed = parse_hook(request.provider, request.raw.clone())
        .map_err(|error| StoreError::Provider(error.to_string()))?;
    let transaction = connection.transaction().map_err(storage_error)?;
    if let Some(session_id) = transaction
        .query_row(
            "SELECT session_id FROM events WHERE id = ?1",
            [request.id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(storage_error)?
    {
        return Ok(IngestResult {
            inserted: false,
            session_id,
            attention_id: request
                .request_id
                .and_then(|id| attention_id_for_request(&transaction, id).ok().flatten()),
            kind: parsed.kind,
            resolved_request_ids: Vec::new(),
        });
    }

    let occurred_at = to_i64(request.received_at);
    let task_title = task_title(&request.raw, parsed.kind);
    let provider_title = resolve_event_title(
        request.provider,
        &request.raw,
        &parsed.provider_session_id,
        parsed.cwd.as_deref(),
    );
    let (token_total, context_window_tokens) = normalized_token_usage(&request.raw);
    let provider = request.provider.to_string();
    let existing = transaction
        .query_row(
            "SELECT id, exec_state, approval_owner, activity, last_event_at FROM sessions
             WHERE provider = ?1 AND provider_session_id = ?2",
            params![provider, parsed.provider_session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?;
    let new_session = existing.is_none();
    let (session_id, current_state, current_owner, current_activity, last_event_at) = if let Some(
        existing,
    ) =
        existing
    {
        existing
    } else {
        let session_id = Uuid::now_v7().to_string();
        let cwd = parsed.cwd.as_deref();
        let project = cwd.and_then(project_name);
        transaction
            .execute(
                "INSERT INTO sessions (
                   id, provider, provider_session_id, cwd, project, title,
                   provider_title, provider_title_source, model,
                   permission_mode, term_app, term_session_id, term_tty, term_title,
                   term_bundle_id, term_surface, provider_pid,
                   exec_state, started_at, last_event_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, 'idle', ?18, ?18)",
                params![
                    session_id,
                    provider,
                    parsed.provider_session_id,
                    cwd,
                    project,
                    task_title.as_deref(),
                    provider_title.as_ref().map(|value| value.title.as_str()),
                    provider_title.as_ref().map(|value| value.source),
                    parsed.model,
                    parsed.permission_mode,
                    request.term.as_ref().and_then(|value| value.app.as_deref()),
                    request
                        .term
                        .as_ref()
                        .and_then(|value| value.session_id.as_deref()),
                    request.term.as_ref().and_then(|value| value.tty.as_deref()),
                    request
                        .term
                        .as_ref()
                        .and_then(|value| value.title.as_deref()),
                    request
                        .term
                        .as_ref()
                        .and_then(|value| value.bundle_id.as_deref()),
                    request
                        .term
                        .as_ref()
                        .and_then(|value| value.surface.as_deref()),
                    request
                        .term
                        .as_ref()
                        .and_then(|value| value.provider_pid)
                        .map(i64::from),
                    occurred_at,
                ],
            )
            .map_err(storage_error)?;
        (session_id, "idle".to_owned(), None, None, occurred_at)
    };

    let terminal = matches!(current_state.as_str(), "response_finished" | "failed");
    let turn_id = select_or_create_turn(
        &transaction,
        &session_id,
        &parsed,
        occurred_at,
        parsed.kind,
        terminal,
    )?;
    let sequence: i64 = transaction
        .query_row(
            "SELECT COALESCE(MAX(ingest_seq), 0) + 1 FROM events",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "INSERT INTO events (
               id, request_id, session_id, turn_id, provider, type, tool_name,
               summary, occurred_at, ingest_seq
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                request.id.to_string(),
                request.request_id.map(|value| value.to_string()),
                session_id,
                turn_id,
                provider,
                event_type(parsed.kind),
                parsed.tool_name.as_deref().map(sanitized_tool_name),
                event_summary(&request.raw, parsed.kind),
                occurred_at,
                sequence,
            ],
        )
        .map_err(storage_error)?;

    let stopped_after_write =
        if parsed.kind == EventKind::Stopped && !has_background_work(&request.raw) {
            turn_id
                .as_deref()
                .map(|turn| turn_has_write_tool(&transaction, turn))
                .transpose()?
                .unwrap_or(false)
        } else {
            false
        };
    let plan_progress = update_task_progress(
        &transaction,
        &session_id,
        parsed.kind,
        &request.raw,
        occurred_at,
    )?;
    let active_subagents = update_subagent_activity(
        &transaction,
        &session_id,
        parsed.kind,
        &request.raw,
        occurred_at,
    )?;
    let observed_native_permission = is_observed_codex_permission_start(&request);
    let attention_id = match parsed.kind {
        EventKind::PermissionRequested if observed_native_permission => {
            Some(insert_native_permission_attention(
                &transaction,
                &session_id,
                turn_id.as_deref(),
                &request,
            )?)
        }
        EventKind::PermissionRequested if !request.provider_handles_approval => {
            Some(insert_approval_attention(
                &transaction,
                &session_id,
                turn_id.as_deref(),
                &request,
                parsed.tool_name.as_deref(),
            )?)
        }
        EventKind::QuestionRequested | EventKind::ElicitationRequested => {
            Some(insert_nonapproval_attention(
                &transaction,
                &session_id,
                turn_id.as_deref(),
                &request,
                NonApprovalSpec {
                    kind: "question",
                    title: if request.provider == Provider::Codex {
                        "Codex 正在询问"
                    } else if parsed.kind == EventKind::ElicitationRequested {
                        "Claude 需要补充信息"
                    } else {
                        "Claude 正在询问"
                    },
                    detail: Some("可直接在 ActRealm 回答；答案不会写入本地历史。"),
                    dedupe_key: request
                        .request_id
                        .map(|id| format!("interactive:{id}"))
                        .unwrap_or_else(|| format!("interactive:{}", request.id)),
                },
            )?)
        }
        EventKind::Failed => Some(insert_nonapproval_attention(
            &transaction,
            &session_id,
            turn_id.as_deref(),
            &request,
            NonApprovalSpec {
                kind: "error",
                title: "Agent 运行失败",
                detail: request.raw.get("error").and_then(Value::as_str),
                dedupe_key: format!(
                    "{}:{}:error:{}",
                    session_id,
                    turn_id.as_deref().unwrap_or("none"),
                    request.id
                ),
            },
        )?),
        EventKind::Notification if is_structured_question(&request.raw) => {
            Some(insert_nonapproval_attention(
                &transaction,
                &session_id,
                turn_id.as_deref(),
                &request,
                NonApprovalSpec {
                    kind: "question",
                    title: "Agent 有问题",
                    detail: request
                        .raw
                        .get("message")
                        .or_else(|| request.raw.get("prompt"))
                        .and_then(Value::as_str),
                    dedupe_key: format!(
                        "{}:question:{}",
                        session_id,
                        request
                            .raw
                            .get("notification_id")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned)
                            .unwrap_or_else(|| request.id.to_string())
                    ),
                },
            )?)
        }
        EventKind::Stopped if stopped_after_write => Some(insert_nonapproval_attention(
            &transaction,
            &session_id,
            turn_id.as_deref(),
            &request,
            NonApprovalSpec {
                kind: "completion",
                title: "任务已完成，等你确认",
                detail: None,
                dedupe_key: format!(
                    "{}:{}:completion",
                    session_id,
                    turn_id.as_deref().unwrap_or("none")
                ),
            },
        )?),
        _ => None,
    };

    let native_permission_resolved = reconcile_observed_native_permission(
        &transaction,
        &session_id,
        turn_id.as_deref(),
        &request,
        parsed.kind,
        occurred_at,
    )?;

    let resolved_request_ids = reconcile_provider_handled_approval(
        &transaction,
        &session_id,
        turn_id.as_deref(),
        parsed.kind,
        parsed.tool_name.as_deref(),
        occurred_at,
    )?;

    let may_update = occurred_at >= last_event_at
        && (!terminal
            || matches!(
                parsed.kind,
                EventKind::PromptSubmitted | EventKind::SessionStarted | EventKind::SessionEnded
            ));
    if may_update {
        let preserves_native_request = current_state == "awaiting_approval"
            && current_owner.as_deref() == Some("terminal")
            && !native_permission_resolved
            && !matches!(
                parsed.kind,
                EventKind::PromptSubmitted
                    | EventKind::Stopped
                    | EventKind::Failed
                    | EventKind::SessionEnded
            );
        let (next_state, owner, default_activity) = if observed_native_permission {
            (
                "awaiting_approval",
                Some("terminal"),
                "Codex 正在请求批准，请在原界面处理".to_owned(),
            )
        } else if preserves_native_request {
            (
                current_state.as_str(),
                current_owner.as_deref(),
                current_activity
                    .clone()
                    .unwrap_or_else(|| "Codex 正在请求批准，请在原界面处理".to_owned()),
            )
        } else if request.provider_handles_approval && parsed.kind == EventKind::PermissionRequested
        {
            (
                "thinking",
                Some("provider"),
                "Codex 正在自动审批".to_owned(),
            )
        } else {
            let (state, owner, activity) = project_event(parsed.kind, &request.raw, &current_state);
            if native_permission_resolved
                && matches!(parsed.kind, EventKind::ToolFinished | EventKind::ToolFailed)
            {
                (state, owner, "权限请求已在 Codex 原界面处理".to_owned())
            } else {
                (state, owner, activity)
            }
        };
        let activity = if preserves_native_request {
            default_activity
        } else {
            plan_progress
                .map(|(done, total)| format!("计划进度 {done}/{total}"))
                .or_else(|| {
                    active_subagents.map(|active| {
                        if active == 0 {
                            "子 Agent 已结束".to_owned()
                        } else {
                            format!("派了 {active} 个子 Agent")
                        }
                    })
                })
                .unwrap_or(default_activity)
        };
        transaction
            .execute(
                "UPDATE sessions SET
                   exec_state = ?2, approval_owner = ?3, activity = ?4,
                   activity_since = ?5, last_event_at = ?5,
                   plan_done = COALESCE(?7, plan_done),
                   plan_total = COALESCE(?8, plan_total),
                   title = COALESCE(?9, title),
                   provider_title = COALESCE(?10, provider_title),
                   provider_title_source = COALESCE(?11, provider_title_source),
                   token_total = COALESCE(?12, token_total),
                   context_window_tokens = COALESCE(?13, context_window_tokens),
                   term_app = COALESCE(?14, term_app),
                   term_session_id = COALESCE(?15, term_session_id),
                   term_tty = COALESCE(?16, term_tty),
                   term_title = COALESCE(?17, term_title),
                   term_bundle_id = COALESCE(?18, term_bundle_id),
                   term_surface = COALESCE(?19, term_surface),
                   provider_pid = COALESCE(?20, provider_pid),
                   ended_at = CASE WHEN ?6 = 1 THEN ?5 ELSE ended_at END
                 WHERE id = ?1",
                params![
                    session_id,
                    next_state,
                    owner,
                    activity,
                    occurred_at,
                    i64::from(parsed.kind == EventKind::SessionEnded),
                    plan_progress.map(|(done, _)| i64::from(done)),
                    plan_progress.map(|(_, total)| i64::from(total)),
                    task_title.as_deref(),
                    provider_title.as_ref().map(|value| value.title.as_str()),
                    provider_title.as_ref().map(|value| value.source),
                    token_total.map(to_i64),
                    context_window_tokens.map(to_i64),
                    request.term.as_ref().and_then(|value| value.app.as_deref()),
                    request
                        .term
                        .as_ref()
                        .and_then(|value| value.session_id.as_deref()),
                    request.term.as_ref().and_then(|value| value.tty.as_deref()),
                    request
                        .term
                        .as_ref()
                        .and_then(|value| value.title.as_deref()),
                    request
                        .term
                        .as_ref()
                        .and_then(|value| value.bundle_id.as_deref()),
                    request
                        .term
                        .as_ref()
                        .and_then(|value| value.surface.as_deref()),
                    request
                        .term
                        .as_ref()
                        .and_then(|value| value.provider_pid)
                        .map(i64::from),
                ],
            )
            .map_err(storage_error)?;
    }

    if matches!(
        parsed.kind,
        EventKind::Stopped | EventKind::Failed | EventKind::SessionEnded
    ) {
        if let Some(turn_id) = turn_id.as_deref() {
            transaction
                .execute(
                    "UPDATE turns SET state = ?2, ended_at = ?3 WHERE id = ?1",
                    params![
                        turn_id,
                        match parsed.kind {
                            EventKind::Failed => "failed",
                            EventKind::SessionEnded => "idle",
                            _ => "response_finished",
                        },
                        occurred_at
                    ],
                )
                .map_err(storage_error)?;
        }
    }

    increment_ingest_metrics(
        &transaction,
        request.received_at,
        new_session,
        parsed.kind == EventKind::PermissionRequested && request.needs_reply,
    )?;

    transaction.commit().map_err(storage_error)?;
    Ok(IngestResult {
        inserted: true,
        session_id,
        attention_id,
        kind: parsed.kind,
        resolved_request_ids,
    })
}

fn claim_transaction(
    connection: &mut Connection,
    command_id: Uuid,
    request_id: Uuid,
    action: ApprovalAction,
    now: u64,
) -> Result<ClaimResult, StoreError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    if let Some(existing) = command_claim(&transaction, command_id)? {
        return Ok(existing);
    }
    let request_id_string = request_id.to_string();
    let attention = transaction
        .query_row(
            "SELECT id, state, expires_at, created_at
             FROM attention_items WHERE request_id = ?1",
            [&request_id_string],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?
        .ok_or(StoreError::StaleApproval)?;
    if attention.1 != "open"
        || attention
            .2
            .is_some_and(|expires_at| to_i64(now) >= expires_at)
    {
        return Err(StoreError::StaleApproval);
    }
    let (attention_state, command_state) = match action {
        ApprovalAction::Approve | ApprovalAction::Deny => ("committing", "pending_commit"),
        ApprovalAction::PassThrough => ("passed_through", "passed_through"),
    };
    let updated = transaction
        .execute(
            "UPDATE attention_items SET state = ?2,
               resolved_at = CASE WHEN ?2 = 'passed_through' THEN ?3 ELSE NULL END,
               resolution = CASE WHEN ?2 = 'passed_through' THEN 'pass_through' ELSE NULL END
             WHERE id = ?1 AND state = 'open'",
            params![attention.0, attention_state, to_i64(now)],
        )
        .map_err(storage_error)?;
    if updated != 1 {
        return Err(StoreError::StaleApproval);
    }
    transaction
        .execute(
            "INSERT INTO commands (
               id, attention_id, request_id, action, state, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                command_id.to_string(),
                attention.0,
                request_id_string,
                action.as_str(),
                command_state,
                to_i64(now),
            ],
        )
        .map_err(storage_error)?;
    if action == ApprovalAction::PassThrough {
        increment_decision_metrics(
            &transaction,
            action,
            now.saturating_sub(from_i64(attention.3)),
            now,
        )?;
    }
    transaction.commit().map_err(storage_error)?;
    Ok(ClaimResult {
        created: true,
        command_id,
        attention_id: attention.0,
        request_id,
        action,
        state: CommandState::parse(command_state)?,
        commit_due_at: action
            .decision()
            .map(|_| now.saturating_add(PERMISSION_COMMIT_DELAY_MS)),
    })
}

fn undo_transaction(
    connection: &mut Connection,
    command_id: Uuid,
    now: u64,
) -> Result<CommandState, StoreError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    let command = transaction
        .query_row(
            "SELECT attention_id, state, created_at FROM commands WHERE id = ?1",
            [command_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?
        .ok_or(StoreError::CommandNotFound)?;
    if command.1 == "undone" {
        return Ok(CommandState::Undone);
    }
    if command.1 != "pending_commit"
        || to_i64(now) >= command.2.saturating_add(to_i64(PERMISSION_COMMIT_DELAY_MS))
    {
        return Err(StoreError::NotUndoable);
    }
    transaction
        .execute(
            "UPDATE commands SET state = 'undone' WHERE id = ?1 AND state = 'pending_commit'",
            [command_id.to_string()],
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "UPDATE attention_items SET state = 'open', resolved_at = NULL, resolution = NULL
             WHERE id = ?1 AND state = 'committing'",
            [&command.0],
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)?;
    Ok(CommandState::Undone)
}

fn commit_transaction(
    connection: &mut Connection,
    command_id: Uuid,
    now: u64,
    waiter_active: bool,
) -> Result<CommitResult, StoreError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    let command = transaction
        .query_row(
            "SELECT commands.attention_id, commands.request_id, commands.action,
                    commands.state, commands.created_at, attention_items.created_at
             FROM commands JOIN attention_items
               ON attention_items.id = commands.attention_id
             WHERE commands.id = ?1",
            [command_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?
        .ok_or(StoreError::CommandNotFound)?;
    let request_id =
        Uuid::parse_str(&command.1).map_err(|error| StoreError::Storage(error.to_string()))?;
    let action = ApprovalAction::parse(&command.2)?;
    if command.3 == "decision_sent" {
        return Ok(CommitResult {
            command_id,
            request_id,
            action,
            state: CommandState::DecisionSent,
        });
    }
    if command.3 != "pending_commit" {
        return Err(StoreError::StaleApproval);
    }
    if to_i64(now) < command.4.saturating_add(to_i64(PERMISSION_COMMIT_DELAY_MS)) {
        return Err(StoreError::CommitTooEarly);
    }
    if !waiter_active {
        transaction
            .execute(
                "UPDATE commands SET state = 'failed', error_code = 'STALE_WAITER'
                 WHERE id = ?1",
                [command_id.to_string()],
            )
            .map_err(storage_error)?;
        transaction
            .execute(
                "UPDATE attention_items SET state = 'expired', resolved_at = ?2,
                   resolution = 'stale_waiter' WHERE id = ?1",
                params![command.0, to_i64(now)],
            )
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
        return Err(StoreError::StaleApproval);
    }
    transaction
        .execute(
            "UPDATE commands SET state = 'decision_sent', sent_at = ?2 WHERE id = ?1",
            params![command_id.to_string(), to_i64(now)],
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "UPDATE attention_items SET state = 'decision_sent', resolution = ?2
             WHERE id = ?1 AND state = 'committing'",
            params![command.0, action.as_str()],
        )
        .map_err(storage_error)?;
    increment_decision_metrics(
        &transaction,
        action,
        from_i64(command.4).saturating_sub(from_i64(command.5)),
        from_i64(command.4),
    )?;
    transaction.commit().map_err(storage_error)?;
    Ok(CommitResult {
        command_id,
        request_id,
        action,
        state: CommandState::DecisionSent,
    })
}

fn act_attention_transaction(
    connection: &mut Connection,
    command_id: Uuid,
    attention_id: &str,
    action: AttentionAction,
    now: u64,
) -> Result<CommandState, StoreError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    if let Some(state) = transaction
        .query_row(
            "SELECT state FROM commands WHERE id = ?1",
            [command_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(storage_error)?
    {
        return CommandState::parse(&state);
    }
    let (kind, state) = transaction
        .query_row(
            "SELECT kind, state FROM attention_items WHERE id = ?1",
            [attention_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(storage_error)?
        .ok_or(StoreError::StaleApproval)?;
    if kind == "approval" || !matches!(state.as_str(), "open" | "snoozed") {
        return Err(StoreError::StaleApproval);
    }
    match action {
        AttentionAction::Ack => {
            transaction
                .execute(
                    "UPDATE attention_items SET state = 'resolved', resolved_at = ?2,
                       resolution = 'ack', expires_at = NULL WHERE id = ?1",
                    params![attention_id, to_i64(now)],
                )
                .map_err(storage_error)?;
        }
        AttentionAction::Snooze => {
            transaction
                .execute(
                    "UPDATE attention_items SET state = 'snoozed', resolved_at = NULL,
                       resolution = NULL, expires_at = ?2 WHERE id = ?1",
                    params![attention_id, to_i64(now.saturating_add(10 * 60 * 1_000))],
                )
                .map_err(storage_error)?;
        }
        AttentionAction::Dismiss => {
            transaction
                .execute(
                    "UPDATE attention_items SET state = 'dismissed', resolved_at = ?2,
                       resolution = 'user_dismissed', expires_at = NULL WHERE id = ?1",
                    params![attention_id, to_i64(now)],
                )
                .map_err(storage_error)?;
        }
    }
    transaction
        .execute(
            "INSERT INTO commands (
               id, attention_id, request_id, action, state, created_at, confirmed_at
             ) VALUES (?1, ?2, NULL, ?3, 'confirmed', ?4, ?4)",
            params![
                command_id.to_string(),
                attention_id,
                action.as_str(),
                to_i64(now)
            ],
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)?;
    Ok(CommandState::Confirmed)
}

fn reopen_due_snoozed(connection: &mut Connection, now: u64) -> Result<(), StoreError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    transaction
        .execute(
            "UPDATE attention_items SET state = 'open', expires_at = NULL
             WHERE state = 'snoozed' AND expires_at <= ?1",
            [to_i64(now)],
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)?;
    Ok(())
}

fn reconcile_transaction(
    connection: &mut Connection,
    active_request_ids: Vec<Uuid>,
    now: u64,
) -> Result<usize, StoreError> {
    let active: HashSet<String> = active_request_ids
        .into_iter()
        .map(|value| value.to_string())
        .collect();
    let transaction = connection.transaction().map_err(storage_error)?;
    let candidates = {
        let mut statement = transaction
            .prepare(
                "SELECT id, request_id FROM attention_items
                 WHERE kind IN ('approval', 'question') AND request_id IS NOT NULL
                 AND state IN ('open', 'committing', 'decision_sent')",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        rows
    };
    let mut expired = 0;
    for (attention_id, request_id) in candidates {
        if active.contains(&request_id) {
            continue;
        }
        transaction
            .execute(
                "UPDATE attention_items SET state = 'expired', resolved_at = ?2,
                   resolution = 'runtime_restart' WHERE id = ?1",
                params![attention_id, to_i64(now)],
            )
            .map_err(storage_error)?;
        transaction
            .execute(
                "UPDATE commands SET state = 'failed', error_code = 'RUNTIME_RESTART'
                 WHERE attention_id = ?1 AND state IN ('pending_commit', 'decision_sent')",
                [attention_id],
            )
            .map_err(storage_error)?;
        expired += 1;
    }
    transaction.commit().map_err(storage_error)?;
    Ok(expired)
}

fn expire_approval_transaction(
    connection: &mut Connection,
    request_id: Uuid,
    reason: &str,
    now: u64,
) -> Result<bool, StoreError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    let attention_id = transaction
        .query_row(
            "SELECT id, created_at, kind FROM attention_items WHERE request_id = ?1
             AND state IN ('open', 'committing', 'decision_sent')",
            [request_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?;
    let Some((attention_id, _created_at, kind)) = attention_id else {
        return Ok(false);
    };
    transaction
        .execute(
            "UPDATE attention_items SET state = 'expired', resolved_at = ?2,
               resolution = ?3 WHERE id = ?1",
            params![attention_id, to_i64(now), reason],
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "UPDATE commands SET state = 'failed', error_code = ?2
             WHERE attention_id = ?1 AND state IN ('pending_commit', 'decision_sent')",
            params![attention_id, reason],
        )
        .map_err(storage_error)?;
    if reason == "deadline" && kind == "approval" {
        ensure_metric_day(&transaction, now)?;
        transaction
            .execute(
                "UPDATE metrics_daily
                 SET pass_through_timeout = pass_through_timeout + 1
                 WHERE day = ?1",
                [metric_day(now)],
            )
            .map_err(storage_error)?;
    }
    transaction.commit().map_err(storage_error)?;
    Ok(true)
}

fn reconcile_sessions_transaction(
    connection: &mut Connection,
    active_sessions: Vec<(Provider, String)>,
    now: u64,
    idle_after_ms: u64,
) -> Result<usize, StoreError> {
    let active: HashSet<(String, String)> = active_sessions
        .into_iter()
        .map(|(provider, session)| (provider.to_string(), session))
        .collect();
    let transaction = connection.transaction().map_err(storage_error)?;
    let candidates = {
        let mut statement = transaction
            .prepare(
                "SELECT id, provider, provider_session_id, last_event_at
                 FROM sessions WHERE exec_state != 'idle'",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        rows
    };
    let mut idled = 0;
    for (session_id, provider, provider_session_id, last_event_at) in candidates {
        if active.contains(&(provider, provider_session_id))
            || to_i64(now).saturating_sub(last_event_at) < to_i64(idle_after_ms)
        {
            continue;
        }
        let pending: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM attention_items WHERE session_id = ?1
                 AND kind = 'approval'
                 AND state IN ('open', 'committing', 'decision_sent')",
                [&session_id],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if pending != 0 {
            continue;
        }
        transaction
            .execute(
                "UPDATE sessions SET exec_state = 'idle', approval_owner = NULL,
                   activity = 'Agent 进程未活动', ended_at = ?2 WHERE id = ?1",
                params![session_id, to_i64(now)],
            )
            .map_err(storage_error)?;
        idled += 1;
    }
    transaction.commit().map_err(storage_error)?;
    Ok(idled)
}

fn sync_native_approval_transaction(
    connection: &mut Connection,
    provider: Provider,
    provider_session_id: &str,
    waiting: bool,
    active: bool,
    now: u64,
) -> Result<NativeApprovalSyncResult, StoreError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    let provider_name = provider.to_string();
    let session = transaction
        .query_row(
            "SELECT id, project, approval_owner FROM sessions
             WHERE provider = ?1 AND provider_session_id = ?2
             ORDER BY last_event_at DESC LIMIT 1",
            params![provider_name, provider_session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?;
    let Some((session_id, project, approval_owner)) = session else {
        return Ok(NativeApprovalSyncResult {
            session_found: false,
            resolved_request_ids: Vec::new(),
        });
    };

    let mut resolved_request_ids = Vec::new();

    if waiting {
        let hook_owned: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM attention_items
                 WHERE session_id = ?1 AND kind = 'approval'
                   AND state IN ('open', 'committing', 'decision_sent')",
                [&session_id],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if hook_owned != 0 || approval_owner.as_deref() == Some("provider") {
            transaction.commit().map_err(storage_error)?;
            return Ok(NativeApprovalSyncResult {
                session_found: true,
                resolved_request_ids,
            });
        }

        let native_open: Option<String> = transaction
            .query_row(
                "SELECT id FROM attention_items
                 WHERE session_id = ?1 AND kind = 'native_approval'
                   AND state IN ('open', 'snoozed')
                 ORDER BY created_at DESC LIMIT 1",
                [&session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        if native_open.is_none() {
            let attention_id = Uuid::now_v7().to_string();
            let title = match provider {
                Provider::Codex => "Codex 需要你在原界面批准",
                Provider::Claude => "Claude 需要你在原界面批准",
                Provider::Gemini => "Agent 需要你在原界面批准",
            };
            let detail = match provider {
                Provider::Codex => {
                    "Codex 客户端或终端正在等待原生权限决定；请在对应对话中批准或拒绝。"
                }
                Provider::Claude => {
                    "Claude 客户端或终端正在等待原生权限决定；请在对应对话中批准或拒绝。"
                }
                Provider::Gemini => "Agent 正在等待原生权限决定；请在对应对话中处理。",
            };
            transaction
                .execute(
                    "INSERT INTO attention_items (
                       id, session_id, provider, project, turn_id, request_id,
                       kind, title, detail, command_preview, risk, risk_notes,
                       dedupe_key, state, created_at
                     ) VALUES (?1, ?2, ?3, ?4, NULL, NULL, 'native_approval',
                               ?5, ?6, NULL, 'unknown', '[]', ?7, 'open', ?8)",
                    params![
                        attention_id,
                        session_id,
                        provider_name,
                        project,
                        title,
                        detail,
                        format!("native-approval:{provider_session_id}:{now}"),
                        to_i64(now),
                    ],
                )
                .map_err(storage_error)?;
        }
        transaction
            .execute(
                "UPDATE sessions SET exec_state = 'awaiting_approval',
                   approval_owner = 'terminal', activity = ?2,
                   activity_since = ?3, last_event_at = MAX(last_event_at, ?3)
                 WHERE id = ?1",
                params![
                    session_id,
                    format!("等待你在 {} 中处理权限", provider_display_name(provider)),
                    to_i64(now),
                ],
            )
            .map_err(storage_error)?;
    } else {
        let hook_approvals = {
            let mut statement = transaction
                .prepare(
                    "SELECT id, request_id, state, resolution FROM attention_items
                     WHERE session_id = ?1 AND kind = 'approval'
                       AND state IN ('open', 'committing', 'decision_sent')",
                )
                .map_err(storage_error)?;
            let rows = statement
                .query_map([&session_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                })
                .map_err(storage_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(storage_error)?;
            rows
        };
        for (attention_id, request_id, state, resolution) in &hook_approvals {
            transaction
                .execute(
                    "UPDATE attention_items SET state = 'resolved', resolved_at = ?2,
                       resolution = COALESCE(resolution, 'provider_handled'), expires_at = NULL
                     WHERE id = ?1",
                    params![attention_id, to_i64(now)],
                )
                .map_err(storage_error)?;
            let confirmed_action = (state == "decision_sent")
                .then_some(resolution.as_deref())
                .flatten()
                .filter(|action| matches!(*action, "approve" | "deny"));
            if let Some(action) = confirmed_action {
                transaction
                    .execute(
                        "UPDATE commands SET state = 'confirmed', confirmed_at = ?2
                         WHERE attention_id = ?1 AND action = ?3
                           AND state = 'decision_sent'",
                        params![attention_id, to_i64(now), action],
                    )
                    .map_err(storage_error)?;
            } else {
                transaction
                    .execute(
                        "UPDATE commands SET state = 'failed', confirmed_at = ?2,
                           error_code = 'PROVIDER_HANDLED'
                         WHERE attention_id = ?1
                           AND state IN ('pending_commit', 'decision_sent')",
                        params![attention_id, to_i64(now)],
                    )
                    .map_err(storage_error)?;
            }
            if let Some(request_id) = request_id
                .as_deref()
                .and_then(|value| Uuid::parse_str(value).ok())
            {
                resolved_request_ids.push(request_id);
            }
        }
        transaction
            .execute(
                "UPDATE attention_items SET state = 'resolved', resolved_at = ?2,
                   resolution = 'provider_handled', expires_at = NULL
                 WHERE session_id = ?1 AND kind = 'native_approval'
                   AND state IN ('open', 'snoozed')",
                params![session_id, to_i64(now)],
            )
            .map_err(storage_error)?;
        let coordinated = !hook_approvals.is_empty()
            || matches!(
                approval_owner.as_deref(),
                Some("terminal" | "provider" | "widget")
            );
        if coordinated {
            transaction
                .execute(
                    "UPDATE sessions SET exec_state = ?2, approval_owner = NULL,
                       activity = ?3, activity_since = ?4,
                       last_event_at = MAX(last_event_at, ?4)
                     WHERE id = ?1",
                    params![
                        session_id,
                        if active {
                            "thinking"
                        } else {
                            "response_finished"
                        },
                        if active {
                            format!("{} 原界面权限请求已处理", provider_display_name(provider))
                        } else {
                            format!(
                                "{} 原界面权限请求已处理，本轮结束",
                                provider_display_name(provider)
                            )
                        },
                        to_i64(now),
                    ],
                )
                .map_err(storage_error)?;
        }
    }
    transaction.commit().map_err(storage_error)?;
    Ok(NativeApprovalSyncResult {
        session_found: true,
        resolved_request_ids,
    })
}

fn provider_display_name(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => "Claude",
        Provider::Codex => "Codex",
        Provider::Gemini => "Agent",
    }
}

fn refresh_provider_titles(connection: &mut Connection, now: u64) -> Result<(), StoreError> {
    let cutoff = to_i64(now.saturating_sub(PROVIDER_TITLE_ACTIVE_WINDOW_MS));
    let candidates = {
        let mut statement = connection
            .prepare(
                "SELECT id, provider, provider_session_id, cwd,
                        provider_title, provider_title_source
                 FROM sessions
                 WHERE provider IN ('claude', 'codex')
                   AND (exec_state != 'idle' OR last_event_at >= ?1)",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([cutoff], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        rows
    };

    let codex_ids = candidates
        .iter()
        .filter(|(_, provider, _, _, _, _)| provider == "codex")
        .map(|(_, _, provider_session_id, _, _, _)| provider_session_id.clone())
        .collect::<HashSet<_>>();
    let codex_titles = resolve_codex_session_titles(&codex_ids);
    let updates = candidates
        .into_iter()
        .filter_map(
            |(id, provider, provider_session_id, cwd, current_title, current_source)| {
                let resolved = if provider == "codex" {
                    codex_titles.get(&provider_session_id).cloned()
                } else {
                    resolve_session_title(&provider, &provider_session_id, cwd.as_deref())
                }?;
                if !should_refresh_provider_title(
                    current_title.as_deref(),
                    current_source.as_deref(),
                    &resolved,
                ) {
                    return None;
                }
                Some((id, resolved))
            },
        )
        .collect::<Vec<(String, ProviderTitle)>>();
    if updates.is_empty() {
        return Ok(());
    }

    let transaction = connection.transaction().map_err(storage_error)?;
    for (id, resolved) in updates {
        transaction
            .execute(
                "UPDATE sessions
                 SET provider_title = ?2, provider_title_source = ?3
                 WHERE id = ?1",
                params![id, resolved.title, resolved.source],
            )
            .map_err(storage_error)?;
    }
    transaction.commit().map_err(storage_error)?;
    Ok(())
}

fn should_refresh_provider_title(
    current_title: Option<&str>,
    current_source: Option<&str>,
    resolved: &ProviderTitle,
) -> bool {
    if current_title == Some(resolved.title.as_str()) && current_source == Some(resolved.source) {
        return false;
    }

    // SessionStart carries Claude's official current title. A transcript may still contain an
    // older AI-generated title, so background refreshes must not downgrade that authoritative
    // value. A later custom-title remains an intentional user rename and may replace either.
    !matches!(
        (current_source, resolved.source),
        (
            Some("claude_session_title" | "claude_custom_title"),
            "claude_ai_title"
        )
    )
}

fn read_snapshot(connection: &Connection) -> Result<StoreSnapshot, StoreError> {
    let sessions = {
        let mut statement = connection
            .prepare(
                "SELECT sessions.id, sessions.provider, sessions.provider_session_id,
                        sessions.project, sessions.title,
                        sessions.provider_title, sessions.provider_title_source, sessions.model,
                        sessions.exec_state, sessions.approval_owner,
                        sessions.activity, sessions.activity_since,
                        sessions.plan_done, sessions.plan_total,
                        (SELECT started_at FROM turns
                         WHERE turns.session_id = sessions.id
                         ORDER BY ordinal DESC LIMIT 1),
                        (SELECT ended_at FROM turns
                         WHERE turns.session_id = sessions.id
                         ORDER BY ordinal DESC LIMIT 1),
                        COALESCE(usage.token_total, sessions.token_total),
                        COALESCE(usage.context_window_tokens, sessions.context_window_tokens),
                        sessions.term_app, sessions.term_session_id, sessions.term_tty,
                        sessions.term_bundle_id, sessions.term_surface,
                        sessions.provider_pid, sessions.last_event_at,
                        sessions.permission_mode,
                        (SELECT tool_name FROM events
                         WHERE events.session_id = sessions.id AND tool_name IS NOT NULL
                         ORDER BY ingest_seq DESC LIMIT 1),
                        (SELECT COUNT(*) FROM session_subagents
                         WHERE session_subagents.session_id = sessions.id AND active = 1),
                        (SELECT provider_turn_id FROM turns
                         WHERE turns.session_id = sessions.id
                         ORDER BY ordinal DESC LIMIT 1),
                        usage.input_tokens, usage.output_tokens,
                        usage.cache_read_tokens, usage.cache_creation_tokens,
                        usage.reasoning_tokens, usage.last_turn_tokens,
                        usage.context_used_tokens, usage.context_used_percent,
                        usage.estimated_cost_usd_micros, usage.cost_kind,
                        usage.pricing_source, usage.usage_source,
                        usage.usage_quality, usage.captured_at
                 FROM sessions
                 LEFT JOIN session_usage AS usage
                   ON usage.provider = sessions.provider
                  AND usage.provider_session_id = sessions.provider_session_id
                 ORDER BY sessions.last_event_at DESC",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([], |row| {
                let provider = row.get::<_, String>(1)?;
                let provider_session_id = row.get::<_, String>(2)?;
                let term_app = row.get::<_, Option<String>>(18)?;
                let term_session_id = row.get::<_, Option<String>>(19)?;
                let term_tty = row.get::<_, Option<String>>(20)?;
                let term_bundle_id = row.get::<_, Option<String>>(21)?;
                let term_surface = row.get::<_, Option<String>>(22)?;
                let provider_pid = row
                    .get::<_, Option<i64>>(23)?
                    .and_then(|value| u32::try_from(value).ok());
                let exec_state = row.get::<_, String>(8)?;
                let current_tool = if exec_state == "tool_running" {
                    row.get(26)?
                } else {
                    None
                };
                let (jump_capability, jump_label) = jump_descriptor(
                    &provider,
                    &provider_session_id,
                    term_app.as_deref(),
                    term_session_id.as_deref(),
                    term_tty.as_deref(),
                    term_bundle_id.as_deref(),
                    term_surface.as_deref(),
                );
                let environment = environment_label(term_surface.as_deref(), term_app.as_deref());
                Ok(SessionRecord {
                    id: row.get(0)?,
                    provider,
                    provider_session_id,
                    project: row.get(3)?,
                    title: row.get(4)?,
                    provider_title: row.get(5)?,
                    provider_title_source: row.get(6)?,
                    model: row.get(7)?,
                    exec_state,
                    approval_owner: row.get(9)?,
                    activity: row.get(10)?,
                    activity_since: row.get::<_, Option<i64>>(11)?.map(from_i64),
                    plan_done: row
                        .get::<_, Option<i64>>(12)?
                        .and_then(|value| u32::try_from(value).ok()),
                    plan_total: row
                        .get::<_, Option<i64>>(13)?
                        .and_then(|value| u32::try_from(value).ok()),
                    turn_started_at: row.get::<_, Option<i64>>(14)?.map(from_i64),
                    turn_ended_at: row.get::<_, Option<i64>>(15)?.map(from_i64),
                    token_total: row.get::<_, Option<i64>>(16)?.map(from_i64),
                    context_window_tokens: row.get::<_, Option<i64>>(17)?.map(from_i64),
                    input_tokens: row.get::<_, Option<i64>>(29)?.map(from_i64),
                    output_tokens: row.get::<_, Option<i64>>(30)?.map(from_i64),
                    cache_read_tokens: row.get::<_, Option<i64>>(31)?.map(from_i64),
                    cache_creation_tokens: row.get::<_, Option<i64>>(32)?.map(from_i64),
                    reasoning_tokens: row.get::<_, Option<i64>>(33)?.map(from_i64),
                    last_turn_tokens: row.get::<_, Option<i64>>(34)?.map(from_i64),
                    context_used_tokens: row.get::<_, Option<i64>>(35)?.map(from_i64),
                    context_used_percent: row
                        .get::<_, Option<i64>>(36)?
                        .and_then(|value| u32::try_from(value).ok()),
                    estimated_cost_usd_micros: row.get::<_, Option<i64>>(37)?.map(from_i64),
                    cost_kind: row.get(38)?,
                    pricing_source: row.get(39)?,
                    usage_source: row.get(40)?,
                    usage_quality: row.get(41)?,
                    usage_captured_at: row.get::<_, Option<i64>>(42)?.map(from_i64),
                    permission_mode: row.get(25)?,
                    current_tool,
                    active_subagents: row
                        .get::<_, i64>(27)
                        .ok()
                        .and_then(|value| u32::try_from(value).ok())
                        .unwrap_or_default(),
                    provider_turn_id: row.get(28)?,
                    environment,
                    jump_capability,
                    jump_label,
                    term_app,
                    term_session_id,
                    term_tty,
                    term_bundle_id,
                    term_surface,
                    provider_pid,
                    last_event_at: from_i64(row.get(24)?),
                })
            })
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        rows
    };
    let attention = {
        let mut statement = connection
            .prepare(
                "SELECT id, session_id, provider, project, request_id, kind,
                        title, detail, state, risk, risk_notes, command_preview,
                        expires_at, created_at, resolution
                 FROM attention_items ORDER BY created_at DESC",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([], |row| {
                let request: Option<String> = row.get(4)?;
                Ok(AttentionRecord {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    provider: row.get(2)?,
                    project: row.get(3)?,
                    request_id: request.and_then(|value| Uuid::parse_str(&value).ok()),
                    kind: row.get(5)?,
                    title: row.get(6)?,
                    detail: row.get(7)?,
                    state: row.get(8)?,
                    risk: row.get(9)?,
                    risk_notes: serde_json::from_str(&row.get::<_, String>(10)?)
                        .unwrap_or_default(),
                    command_preview: row.get(11)?,
                    expires_at: row.get::<_, Option<i64>>(12)?.map(from_i64),
                    created_at: from_i64(row.get(13)?),
                    resolution: row.get(14)?,
                })
            })
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        rows
    };
    let commands = {
        let mut statement = connection
            .prepare(
                "SELECT id, attention_id, request_id, action, state, created_at
                 FROM commands ORDER BY created_at",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let request: Option<String> = row.get(2)?;
                Ok(CommandRecord {
                    id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()),
                    attention_id: row.get(1)?,
                    request_id: request.and_then(|value| Uuid::parse_str(&value).ok()),
                    action: row.get(3)?,
                    state: row.get(4)?,
                    created_at: from_i64(row.get(5)?),
                })
            })
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        rows
    };
    let event_count = connection
        .query_row("SELECT COUNT(*) FROM events", [], |row| {
            row.get::<_, i64>(0)
        })
        .map(from_i64)
        .map_err(storage_error)?;
    let metrics = read_metrics(connection, now_millis())?;
    Ok(StoreSnapshot {
        sessions,
        attention,
        commands,
        event_count,
        metrics,
    })
}

fn environment_label(surface: Option<&str>, app: Option<&str>) -> Option<String> {
    match surface {
        Some("codex_app") => Some("Codex 客户端".to_owned()),
        Some("claude_app") => Some("Claude 客户端".to_owned()),
        Some("terminal") => Some(
            app.filter(|value| !value.trim().is_empty())
                .unwrap_or("终端")
                .chars()
                .take(64)
                .collect(),
        ),
        _ => None,
    }
}

fn read_metrics(connection: &Connection, now: u64) -> Result<MetricsSummary, StoreError> {
    connection
        .query_row(
            "SELECT
               COUNT(*),
               COALESCE(SUM(approval_requests), 0),
               COALESCE(SUM(widget_approvals), 0),
               COALESCE(SUM(widget_denials), 0),
               COALESCE(SUM(pass_through_manual), 0),
               COALESCE(SUM(pass_through_timeout), 0),
               COALESCE(SUM(decision_response_ms_total), 0),
               COALESCE(SUM(decision_response_count), 0),
               COALESCE(SUM(banners_shown), 0),
               COALESCE(SUM(sessions_observed), 0),
               COALESCE(SUM(app_opened), 0),
               COALESCE(SUM(CASE WHEN day = ?1
                 THEN widget_approvals + widget_denials ELSE 0 END), 0)
             FROM metrics_daily",
            [metric_day(now)],
            |row| {
                Ok(MetricsSummary {
                    active_days: from_i64(row.get(0)?),
                    approval_requests: from_i64(row.get(1)?),
                    widget_approvals: from_i64(row.get(2)?),
                    widget_denials: from_i64(row.get(3)?),
                    pass_through_manual: from_i64(row.get(4)?),
                    pass_through_timeout: from_i64(row.get(5)?),
                    decision_response_ms_total: from_i64(row.get(6)?),
                    decision_response_count: from_i64(row.get(7)?),
                    banners_shown: from_i64(row.get(8)?),
                    sessions_observed: from_i64(row.get(9)?),
                    app_opened: from_i64(row.get(10)?),
                    today_widget_decisions: from_i64(row.get(11)?),
                })
            },
        )
        .map_err(storage_error)
}

fn select_or_create_turn(
    transaction: &Transaction<'_>,
    session_id: &str,
    parsed: &actrealm_providers::ParsedHookEvent,
    occurred_at: i64,
    kind: EventKind,
    terminal_session: bool,
) -> Result<Option<String>, StoreError> {
    if matches!(kind, EventKind::SessionStarted | EventKind::SessionEnded) {
        return Ok(None);
    }
    if kind != EventKind::PromptSubmitted {
        let current = transaction
            .query_row(
                "SELECT id FROM turns WHERE session_id = ?1 AND ended_at IS NULL
                 ORDER BY ordinal DESC LIMIT 1",
                [session_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(storage_error)?;
        if current.is_some() {
            return Ok(current);
        }
        if terminal_session {
            return transaction
                .query_row(
                    "SELECT id FROM turns WHERE session_id = ?1
                     ORDER BY ordinal DESC LIMIT 1",
                    [session_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(storage_error);
        }
    } else {
        transaction
            .execute(
                "UPDATE turns SET state = 'response_finished', ended_at = ?2
                 WHERE session_id = ?1 AND ended_at IS NULL",
                params![session_id, occurred_at],
            )
            .map_err(storage_error)?;
    }
    let ordinal: i64 = transaction
        .query_row(
            "SELECT COALESCE(MAX(ordinal), 0) + 1 FROM turns WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let turn_id = Uuid::now_v7().to_string();
    transaction
        .execute(
            "INSERT INTO turns (
               id, session_id, provider_turn_id, prompt_id, ordinal, state, started_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'running', ?6)",
            params![
                turn_id,
                session_id,
                parsed.provider_turn_id,
                parsed.prompt_id,
                ordinal,
                occurred_at,
            ],
        )
        .map_err(storage_error)?;
    Ok(Some(turn_id))
}

fn insert_approval_attention(
    transaction: &Transaction<'_>,
    session_id: &str,
    turn_id: Option<&str>,
    request: &BridgeRequest,
    tool_name: Option<&str>,
) -> Result<String, StoreError> {
    let request_id = request
        .request_id
        .ok_or_else(|| StoreError::Provider("PermissionRequest is missing requestId".to_owned()))?;
    if let Some(existing) = attention_id_for_request(transaction, request_id)? {
        return Ok(existing);
    }
    let id = Uuid::now_v7().to_string();
    let command = request
        .raw
        .pointer("/tool_input/command")
        .and_then(Value::as_str);
    let (risk, notes) = classify_risk(tool_name, command);
    let project = request
        .raw
        .get("cwd")
        .and_then(Value::as_str)
        .and_then(project_name);
    transaction
        .execute(
            "INSERT INTO attention_items (
               id, session_id, provider, project, turn_id, request_id, kind,
               title, detail, command_preview, risk, risk_notes, dedupe_key, state,
               expires_at, created_at
             ) VALUES (
               ?1, ?2, ?3, ?4, ?5, ?6, 'approval', ?7, ?8, ?9, ?10, ?11, ?6,
               'open', ?12, ?13
             )",
            params![
                id,
                session_id,
                request.provider.to_string(),
                project,
                turn_id,
                request_id.to_string(),
                format!(
                    "允许 {}？",
                    tool_name
                        .map(sanitized_tool_name)
                        .as_deref()
                        .unwrap_or("此操作")
                ),
                approval_detail(&request.raw, tool_name),
                command.map(redacted_preview),
                risk,
                serde_json::to_string(&notes)
                    .map_err(|error| StoreError::Storage(error.to_string()))?,
                request.deadline_at.map(to_i64),
                to_i64(request.received_at),
            ],
        )
        .map_err(storage_error)?;
    Ok(id)
}

fn insert_native_permission_attention(
    transaction: &Transaction<'_>,
    session_id: &str,
    turn_id: Option<&str>,
    request: &BridgeRequest,
) -> Result<String, StoreError> {
    let tool_use_id = request
        .raw
        .get("tool_use_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 256);
    let identity = tool_use_id
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| request.id.to_string());
    let dedupe_key = format!("native-request:{session_id}:{identity}");
    if let Some(existing) = transaction
        .query_row(
            "SELECT id FROM attention_items WHERE dedupe_key = ?1",
            [&dedupe_key],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(storage_error)?
    {
        return Ok(existing);
    }

    let id = Uuid::now_v7().to_string();
    let project = request
        .raw
        .get("cwd")
        .and_then(Value::as_str)
        .and_then(project_name);
    let detail = request
        .raw
        .pointer("/tool_input/reason")
        .or_else(|| request.raw.get("reason"))
        .and_then(Value::as_str)
        .and_then(sanitized_attention_text)
        .unwrap_or_else(|| {
            "Codex 客户端或终端正在显示原生权限请求；请回到对应对话查看并处理。".to_owned()
        });
    transaction
        .execute(
            "INSERT INTO attention_items (
               id, session_id, provider, project, turn_id, request_id, kind,
               title, detail, command_preview, risk, risk_notes, dedupe_key,
               state, expires_at, created_at
             ) VALUES (
               ?1, ?2, ?3, ?4, ?5, NULL, 'native_approval',
               'Codex 正在请求批准', ?6, NULL, 'unknown', '[]', ?7,
               'open', NULL, ?8
             )",
            params![
                id,
                session_id,
                request.provider.to_string(),
                project,
                turn_id,
                detail,
                dedupe_key,
                to_i64(request.received_at),
            ],
        )
        .map_err(storage_error)?;
    Ok(id)
}

struct NonApprovalSpec<'a> {
    kind: &'a str,
    title: &'a str,
    detail: Option<&'a str>,
    dedupe_key: String,
}

fn insert_nonapproval_attention(
    transaction: &Transaction<'_>,
    session_id: &str,
    turn_id: Option<&str>,
    request: &BridgeRequest,
    spec: NonApprovalSpec<'_>,
) -> Result<String, StoreError> {
    if let Some(existing) = transaction
        .query_row(
            "SELECT id FROM attention_items WHERE dedupe_key = ?1",
            [&spec.dedupe_key],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(storage_error)?
    {
        return Ok(existing);
    }
    let id = Uuid::now_v7().to_string();
    let project = request
        .raw
        .get("cwd")
        .and_then(Value::as_str)
        .and_then(project_name);
    transaction
        .execute(
            "INSERT INTO attention_items (
               id, session_id, provider, project, turn_id, request_id, kind,
               title, detail, command_preview, risk, risk_notes, dedupe_key,
               state, expires_at, created_at
             ) VALUES (
               ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, 'unknown',
               '[]', ?10, 'open', ?11, ?12
             )",
            params![
                id,
                session_id,
                request.provider.to_string(),
                project,
                turn_id,
                request.request_id.map(|value| value.to_string()),
                spec.kind,
                spec.title,
                spec.detail
                    .filter(|detail| !detail.trim().is_empty())
                    .and_then(sanitized_attention_text),
                spec.dedupe_key,
                request.deadline_at.map(to_i64),
                to_i64(request.received_at),
            ],
        )
        .map_err(storage_error)?;
    Ok(id)
}

fn is_structured_question(raw: &Value) -> bool {
    ["notification_type", "type", "kind"]
        .iter()
        .filter_map(|field| raw.get(field).and_then(Value::as_str))
        .any(|value| value.eq_ignore_ascii_case("question"))
}

fn turn_has_write_tool(transaction: &Transaction<'_>, turn_id: &str) -> Result<bool, StoreError> {
    transaction
        .query_row(
            "SELECT EXISTS(
               SELECT 1 FROM events WHERE turn_id = ?1
               AND type IN ('tool.started', 'tool.finished')
               AND tool_name IN ('Edit', 'Write', 'apply_patch', 'MultiEdit')
             )",
            [turn_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(storage_error)
}

fn attention_id_for_request(
    transaction: &Transaction<'_>,
    request_id: Uuid,
) -> Result<Option<String>, StoreError> {
    transaction
        .query_row(
            "SELECT id FROM attention_items WHERE request_id = ?1",
            [request_id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)
}

fn command_claim(
    transaction: &Transaction<'_>,
    command_id: Uuid,
) -> Result<Option<ClaimResult>, StoreError> {
    let row = transaction
        .query_row(
            "SELECT attention_id, request_id, action, state, created_at
             FROM commands WHERE id = ?1",
            [command_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?;
    row.map(|(attention_id, request_id, action, state, created_at)| {
        let action = ApprovalAction::parse(&action)?;
        Ok(ClaimResult {
            created: false,
            command_id,
            attention_id,
            request_id: Uuid::parse_str(&request_id)
                .map_err(|error| StoreError::Storage(error.to_string()))?,
            action,
            state: CommandState::parse(&state)?,
            commit_due_at: action
                .decision()
                .map(|_| from_i64(created_at).saturating_add(PERMISSION_COMMIT_DELAY_MS)),
        })
    })
    .transpose()
}

fn is_observed_codex_permission_start(request: &BridgeRequest) -> bool {
    request.provider == Provider::Codex
        && request.event_name() == Some("PreToolUse")
        && request.raw.get("tool_name").and_then(Value::as_str) == Some("request_permissions")
        && !request.needs_reply
}

fn reconcile_observed_native_permission(
    transaction: &Transaction<'_>,
    session_id: &str,
    turn_id: Option<&str>,
    request: &BridgeRequest,
    kind: EventKind,
    occurred_at: i64,
) -> Result<bool, StoreError> {
    if request.provider != Provider::Codex {
        return Ok(false);
    }
    let exact_tool_end = matches!(
        request.event_name(),
        Some("PostToolUse" | "PostToolUseFailure")
    ) && request.raw.get("tool_name").and_then(Value::as_str)
        == Some("request_permissions");
    let closes_actionable_state = exact_tool_end
        || matches!(
            kind,
            EventKind::PromptSubmitted
                | EventKind::Stopped
                | EventKind::Failed
                | EventKind::SessionEnded
        );
    if !closes_actionable_state {
        return Ok(false);
    }

    let tool_use_id = request
        .raw
        .get("tool_use_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 256);
    transaction
        .execute(
            "UPDATE attention_items SET state = 'resolved', resolved_at = ?2,
               resolution = 'provider_handled', expires_at = NULL
             WHERE session_id = ?1 AND kind = 'native_approval'
               AND state IN ('open', 'snoozed')
               AND (?3 = 0 OR ?4 IS NULL OR turn_id = ?4)
               AND (?5 IS NULL OR dedupe_key = 'native-request:' || ?1 || ':' || ?5)",
            params![
                session_id,
                occurred_at,
                i64::from(exact_tool_end),
                turn_id,
                tool_use_id,
            ],
        )
        .map_err(storage_error)?;
    Ok(true)
}

fn reconcile_provider_handled_approval(
    transaction: &Transaction<'_>,
    session_id: &str,
    turn_id: Option<&str>,
    kind: EventKind,
    tool_name: Option<&str>,
    occurred_at: i64,
) -> Result<Vec<Uuid>, StoreError> {
    let outcome = match kind {
        EventKind::ToolStarted | EventKind::ToolFinished => "provider_approved",
        EventKind::ToolFailed | EventKind::PermissionDenied => "provider_denied",
        EventKind::Stopped
        | EventKind::Failed
        | EventKind::SessionEnded
        | EventKind::PromptSubmitted => "provider_closed",
        _ => return Ok(Vec::new()),
    };
    let terminal_event = matches!(
        kind,
        EventKind::Stopped
            | EventKind::Failed
            | EventKind::SessionEnded
            | EventKind::PromptSubmitted
            | EventKind::PermissionDenied
    );
    let closes_whole_session = matches!(
        kind,
        EventKind::Stopped
            | EventKind::Failed
            | EventKind::SessionEnded
            | EventKind::PromptSubmitted
    );
    let scoped_turn = if closes_whole_session { None } else { turn_id };
    let candidates = {
        let mut statement = transaction
            .prepare(
                "SELECT id, request_id, title, resolution FROM attention_items
                 WHERE session_id = ?1 AND kind = 'approval'
                   AND state IN ('open', 'committing', 'decision_sent')
                   AND (?2 IS NULL OR turn_id = ?2)
                 ORDER BY created_at DESC",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map(params![session_id, scoped_turn], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        rows
    };
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let mut resolved_request_ids = Vec::new();
    for (attention_id, request_id, title, resolution) in candidates {
        if !terminal_event && resolution.is_none() {
            let Some(tool_name) = tool_name else { continue };
            let safe_tool = sanitized_tool_name(tool_name);
            if safe_tool == "Unknown" || !title.contains(&safe_tool) {
                continue;
            }
        }
        let confirmed_action = match (outcome, resolution.as_deref()) {
            ("provider_approved", Some("approve")) => Some("approve"),
            ("provider_denied", Some("deny")) => Some("deny"),
            _ => None,
        };
        transaction
            .execute(
                "UPDATE attention_items SET state = 'resolved', resolved_at = ?2,
                   expires_at = NULL, resolution = COALESCE(resolution, ?3)
                 WHERE id = ?1",
                params![attention_id, occurred_at, outcome],
            )
            .map_err(storage_error)?;
        if let Some(action) = confirmed_action {
            transaction
                .execute(
                    "UPDATE commands SET state = 'confirmed', confirmed_at = ?2
                     WHERE attention_id = ?1 AND action = ?3
                       AND state IN ('pending_commit', 'decision_sent')",
                    params![attention_id, occurred_at, action],
                )
                .map_err(storage_error)?;
        }
        if outcome != "provider_closed" {
            transaction
                .execute(
                    "UPDATE commands SET state = 'failed', confirmed_at = ?2,
                       error_code = 'PROVIDER_HANDLED'
                     WHERE attention_id = ?1 AND state IN ('pending_commit', 'decision_sent')",
                    params![attention_id, occurred_at],
                )
                .map_err(storage_error)?;
        }
        if let Some(request_id) = request_id.and_then(|value| Uuid::parse_str(&value).ok()) {
            resolved_request_ids.push(request_id);
        }
        if !terminal_event {
            break;
        }
    }
    Ok(resolved_request_ids)
}

fn approval_detail(raw: &Value, tool_name: Option<&str>) -> Option<String> {
    let supplied = ["reason", "description", "message"]
        .iter()
        .find_map(|key| raw.get(*key).and_then(Value::as_str))
        .and_then(sanitized_attention_text);
    if supplied.is_some() {
        return supplied;
    }
    let tool = tool_name.map(sanitized_tool_name);
    let input = raw.get("tool_input");
    match tool.as_deref() {
        Some("Edit" | "Write" | "Read" | "MultiEdit" | "apply_patch") => input
            .and_then(|value| {
                ["file_path", "path"]
                    .iter()
                    .find_map(|key| value.get(*key).and_then(Value::as_str))
            })
            .and_then(|path| Path::new(path).file_name().and_then(|name| name.to_str()))
            .map(|name| format!("Agent 请求访问文件 {name}，完整路径请在原对话中核对。")),
        Some("Bash" | "Shell") => Some(
            "Agent 请求执行终端命令；这里只展示脱敏摘要，完整内容请在原对话中核对。".to_owned(),
        ),
        Some(name) => Some(format!("Agent 请求运行 {name}，请核对操作目的和影响。")),
        None => None,
    }
}

fn sanitized_attention_text(value: &str) -> Option<String> {
    let normalized = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        return None;
    }
    let lower = normalized.to_ascii_lowercase();
    if [
        "authorization:",
        "api_key",
        "api-key",
        "password=",
        "token=",
        "secret=",
        "sk-",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return Some("内容可能包含凭据，已隐藏；请回到原对话查看。".to_owned());
    }
    let mut bounded = normalized.chars().take(180).collect::<String>();
    if normalized.chars().count() > 180 {
        bounded.push('…');
    }
    Some(bounded)
}

fn update_task_progress(
    transaction: &Transaction<'_>,
    session_id: &str,
    kind: EventKind,
    raw: &Value,
    occurred_at: i64,
) -> Result<Option<(u32, u32)>, StoreError> {
    if !matches!(kind, EventKind::TaskCreated | EventKind::TaskCompleted) {
        return Ok(None);
    }
    let Some(task_id) = raw
        .get("task_id")
        .and_then(Value::as_str)
        .filter(|task_id| !task_id.is_empty() && task_id.len() <= 256)
    else {
        return Ok(None);
    };
    if kind == EventKind::TaskCreated {
        transaction
            .execute(
                "INSERT OR IGNORE INTO session_tasks (
                   session_id, task_id, completed, created_at
                 ) VALUES (?1, ?2, 0, ?3)",
                params![session_id, task_id, occurred_at],
            )
            .map_err(storage_error)?;
    } else {
        transaction
            .execute(
                "INSERT INTO session_tasks (
                   session_id, task_id, completed, created_at, completed_at
                 ) VALUES (?1, ?2, 1, ?3, ?3)
                 ON CONFLICT(session_id, task_id) DO UPDATE SET
                   completed = 1,
                   completed_at = COALESCE(session_tasks.completed_at, excluded.completed_at)",
                params![session_id, task_id, occurred_at],
            )
            .map_err(storage_error)?;
    }
    let (done, total) = transaction
        .query_row(
            "SELECT COALESCE(SUM(completed), 0), COUNT(*)
             FROM session_tasks WHERE session_id = ?1",
            [session_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .map_err(storage_error)?;
    Ok(Some((
        u32::try_from(done).unwrap_or(u32::MAX),
        u32::try_from(total).unwrap_or(u32::MAX),
    )))
}

fn update_subagent_activity(
    transaction: &Transaction<'_>,
    session_id: &str,
    kind: EventKind,
    raw: &Value,
    occurred_at: i64,
) -> Result<Option<u32>, StoreError> {
    if !matches!(
        kind,
        EventKind::SubagentStarted | EventKind::SubagentStopped
    ) {
        return Ok(None);
    }
    let Some(agent_id) = raw
        .get("agent_id")
        .and_then(Value::as_str)
        .filter(|agent_id| !agent_id.is_empty() && agent_id.len() <= 256)
    else {
        return Ok(None);
    };
    if kind == EventKind::SubagentStarted {
        transaction
            .execute(
                "INSERT INTO session_subagents (
                   session_id, agent_id, active, started_at
                 ) VALUES (?1, ?2, 1, ?3)
                 ON CONFLICT(session_id, agent_id) DO UPDATE SET active = 1",
                params![session_id, agent_id, occurred_at],
            )
            .map_err(storage_error)?;
    } else {
        transaction
            .execute(
                "INSERT INTO session_subagents (
                   session_id, agent_id, active, started_at, stopped_at
                 ) VALUES (?1, ?2, 0, ?3, ?3)
                 ON CONFLICT(session_id, agent_id) DO UPDATE SET
                   active = 0,
                   stopped_at = COALESCE(session_subagents.stopped_at, excluded.stopped_at)",
                params![session_id, agent_id, occurred_at],
            )
            .map_err(storage_error)?;
    }
    let active = transaction
        .query_row(
            "SELECT COUNT(*) FROM session_subagents
             WHERE session_id = ?1 AND active = 1",
            [session_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(storage_error)?;
    Ok(Some(u32::try_from(active).unwrap_or(u32::MAX)))
}

fn has_background_work(raw: &Value) -> bool {
    ["background_tasks", "session_crons"].iter().any(|field| {
        raw.get(*field)
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
    })
}

fn normalized_token_usage(raw: &Value) -> (Option<u64>, Option<u64>) {
    let total = first_u64(
        raw,
        &[
            "/token_usage/total_tokens",
            "/tokenUsage/totalTokens",
            "/usage/total_tokens",
            "/usage/totalTokens",
            "/info/total_token_usage/total_tokens",
            "/params/tokenUsage/total/totalTokens",
            "/params/tokenUsage/last/totalTokens",
        ],
    )
    .or_else(|| {
        let input = first_u64(
            raw,
            &[
                "/usage/input_tokens",
                "/usage/inputTokens",
                "/info/total_token_usage/input_tokens",
            ],
        )?;
        let output = first_u64(
            raw,
            &[
                "/usage/output_tokens",
                "/usage/outputTokens",
                "/info/total_token_usage/output_tokens",
            ],
        )
        .unwrap_or_default();
        input.checked_add(output)
    });
    let context = first_u64(
        raw,
        &[
            "/model_context_window",
            "/context_window/max_input_tokens",
            "/contextWindow",
            "/params/tokenUsage/modelContextWindow",
        ],
    );
    (
        total.filter(|value| *value > 0),
        context.filter(|value| *value > 0),
    )
}

fn first_u64(raw: &Value, pointers: &[&str]) -> Option<u64> {
    pointers
        .iter()
        .find_map(|pointer| raw.pointer(pointer).and_then(Value::as_u64))
        .filter(|value| *value <= i64::MAX as u64)
}

fn task_title(raw: &Value, kind: EventKind) -> Option<String> {
    if kind != EventKind::PromptSubmitted {
        return None;
    }
    let prompt = raw
        .get("prompt")
        .or_else(|| raw.get("user_prompt"))
        .and_then(Value::as_str)?;
    let normalized = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return None;
    }
    let sentence = normalized
        .split_inclusive(['。', '！', '？', '.', '!', '?'])
        .next()
        .unwrap_or(&normalized)
        .trim();
    let mut title = sentence
        .chars()
        .take(MAX_TASK_TITLE_CHARS)
        .collect::<String>();
    if sentence.chars().count() > MAX_TASK_TITLE_CHARS {
        title.push('…');
    }
    (!title.is_empty()).then_some(title)
}

fn project_event<'a>(
    kind: EventKind,
    raw: &Value,
    current: &'a str,
) -> (&'a str, Option<&'static str>, String) {
    match kind {
        EventKind::SessionStarted | EventKind::SessionEnded => {
            ("idle", None, "等待新任务".to_owned())
        }
        EventKind::PromptSubmitted => ("thinking", None, "正在思考".to_owned()),
        EventKind::ToolStarted => (
            "tool_running",
            None,
            format!(
                "正在运行 {}",
                raw.get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("工具")
            ),
        ),
        EventKind::ToolFinished | EventKind::ToolFailed => {
            ("thinking", None, "继续思考".to_owned())
        }
        EventKind::PermissionRequested => {
            ("awaiting_approval", Some("widget"), "等待你批准".to_owned())
        }
        EventKind::QuestionRequested | EventKind::ElicitationRequested => {
            ("awaiting_approval", Some("widget"), "等待你回答".to_owned())
        }
        EventKind::PermissionDenied => ("thinking", None, "操作已在 Agent 中拒绝".to_owned()),
        EventKind::Compacting => ("compacting", None, "正在压缩记忆".to_owned()),
        EventKind::Stopped if has_background_work(raw) => {
            let count = ["background_tasks", "session_crons"]
                .iter()
                .filter_map(|field| raw.get(*field).and_then(Value::as_array))
                .map(Vec::len)
                .sum::<usize>();
            ("tool_running", None, format!("后台任务仍在运行 · {count}"))
        }
        EventKind::Stopped => ("response_finished", None, "本轮已完成".to_owned()),
        EventKind::Failed => ("failed", None, "运行失败".to_owned()),
        EventKind::Unknown => (current, None, "⚠ 事件不识别（可能版本不兼容）".to_owned()),
        EventKind::Notification
        | EventKind::SubagentStarted
        | EventKind::SubagentStopped
        | EventKind::TaskCreated
        | EventKind::TaskCompleted => (current, None, "活动已更新".to_owned()),
    }
}

fn classify_risk(
    tool_name: Option<&str>,
    command: Option<&str>,
) -> (&'static str, Vec<&'static str>) {
    let command = command.unwrap_or_default().trim().to_ascii_lowercase();
    let high = [
        "rm -rf",
        "git push",
        "sudo ",
        "chmod 777",
        "drop table",
        "docker system prune",
        "kill -9 1",
    ];
    if high.iter().any(|needle| command.contains(needle)) {
        return (
            "high",
            vec!["⚠ 已识别到高影响操作", "提交后动作本身不可撤销"],
        );
    }
    let has_shell_composition = ["|", ">", "<", "$(", "`", "&&", ";"]
        .iter()
        .any(|needle| command.contains(needle));
    if has_shell_composition {
        return ("unknown", vec!["命令包含组合语法", "建议查看原窗口"]);
    }
    let low = ["git status", "git diff", "git log", "ls", "rg"];
    if low
        .iter()
        .any(|prefix| command == *prefix || command.starts_with(&format!("{prefix} ")))
    {
        return (
            "low",
            vec!["只读意图（规则提示，非安全保证）", "↩ 3 秒内可撤回批准决定"],
        );
    }
    let medium = [
        "cargo test",
        "cargo build",
        "cargo clippy",
        "cargo fmt",
        "npm install",
        "pnpm install",
        "git commit",
        "mkdir ",
        "cp ",
        "mv ",
    ];
    if medium.iter().any(|prefix| command.starts_with(prefix))
        || matches!(tool_name, Some("Edit" | "Write" | "apply_patch"))
    {
        return (
            "med",
            vec!["可能执行项目代码或产生副作用", "建议核对原命令"],
        );
    }
    ("unknown", vec!["我不认识这个操作的影响", "建议查看原窗口"])
}

fn redacted_preview(command: &str) -> String {
    let one_line: String = command
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    let mut words = one_line.split_whitespace();
    let executable = loop {
        let Some(word) = words.next() else {
            return "<redacted>".to_owned();
        };
        let before_equals = word.split_once('=').map(|(name, _)| name);
        if before_equals.is_some_and(|name| {
            !name.is_empty()
                && name
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
        }) {
            continue;
        }
        break word;
    };
    let executable = Path::new(executable.trim_matches(['\'', '"']))
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let subcommand = words
        .next()
        .map(|word| word.trim_matches(['\'', '"']).to_ascii_lowercase());
    let safe = match (executable.as_str(), subcommand.as_deref()) {
        ("git", Some(value)) if matches!(value, "status" | "diff" | "log") => {
            Some(format!("git {value}"))
        }
        ("cargo", Some(value))
            if matches!(value, "test" | "build" | "check" | "clippy" | "fmt") =>
        {
            Some(format!("cargo {value}"))
        }
        ("npm" | "pnpm" | "yarn", Some("test")) => Some(format!("{executable} test")),
        ("rg" | "ls", _) => Some(executable.clone()),
        _ => None,
    };
    if let Some(safe) = safe {
        return safe;
    }
    let label: String = executable
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '+' | '-')
        })
        .take(24)
        .collect();
    if label.is_empty() {
        "<redacted>".to_owned()
    } else {
        format!("{label} <redacted>")
    }
}

fn sanitized_tool_name(name: &str) -> String {
    match name {
        "Bash"
        | "Shell"
        | "Edit"
        | "Write"
        | "Read"
        | "Glob"
        | "Grep"
        | "Task"
        | "WebFetch"
        | "WebSearch"
        | "apply_patch"
        | "MultiEdit"
        | "request_permissions" => name.to_owned(),
        _ => "Unknown".to_owned(),
    }
}

fn event_summary(raw: &Value, kind: EventKind) -> Option<String> {
    match kind {
        EventKind::PermissionRequested => {
            raw.get("tool_name").and_then(Value::as_str).map(|tool| {
                if tool == "request_permissions" {
                    "Codex 原界面请求批准".to_owned()
                } else {
                    format!("请求运行 {}", sanitized_tool_name(tool))
                }
            })
        }
        EventKind::QuestionRequested => Some("Claude AskUserQuestion".to_owned()),
        EventKind::ElicitationRequested => Some("Claude Elicitation".to_owned()),
        EventKind::Unknown => Some("未知 Provider 事件".to_owned()),
        _ => None,
    }
}

fn event_type(kind: EventKind) -> &'static str {
    match kind {
        EventKind::SessionStarted => "session.started",
        EventKind::SessionEnded => "session.ended",
        EventKind::PromptSubmitted => "prompt.submitted",
        EventKind::ToolStarted => "tool.started",
        EventKind::ToolFinished => "tool.finished",
        EventKind::ToolFailed => "tool.failed",
        EventKind::PermissionRequested => "approval.requested",
        EventKind::PermissionDenied => "approval.denied",
        EventKind::QuestionRequested => "question.requested",
        EventKind::ElicitationRequested => "elicitation.requested",
        EventKind::Notification => "notification",
        EventKind::SubagentStarted => "subagent.started",
        EventKind::SubagentStopped => "subagent.stopped",
        EventKind::TaskCreated => "task.created",
        EventKind::TaskCompleted => "task.completed",
        EventKind::Compacting => "session.compacting",
        EventKind::Stopped => "turn.stopped",
        EventKind::Failed => "turn.failed",
        EventKind::Unknown => "unknown",
    }
}

fn jump_descriptor(
    provider: &str,
    provider_session_id: &str,
    term_app: Option<&str>,
    term_session_id: Option<&str>,
    term_tty: Option<&str>,
    term_bundle_id: Option<&str>,
    term_surface: Option<&str>,
) -> (String, String) {
    let app = term_app.unwrap_or_default().to_ascii_lowercase();
    let bundle = term_bundle_id.unwrap_or_default().to_ascii_lowercase();
    let codex_app = term_surface == Some("codex_app") || bundle == "com.openai.codex";
    if provider == "codex" && codex_app && Uuid::parse_str(provider_session_id).is_ok() {
        return ("exact_conversation".to_owned(), "精确打开对话".to_owned());
    }
    let iterm = app.contains("iterm") || bundle == "com.googlecode.iterm2";
    let terminal = app == "apple_terminal" || bundle == "com.apple.terminal";
    if (iterm && term_session_id.is_some()) || (terminal && term_tty.is_some()) {
        return ("terminal".to_owned(), "打开对应终端".to_owned());
    }
    let known_app = codex_app
        || term_surface == Some("claude_app")
        || bundle == "com.anthropic.claudefordesktop"
        || iterm
        || terminal
        || app == "vscode"
        || bundle == "com.microsoft.vscode"
        || app.contains("warp")
        || bundle.starts_with("dev.warp.");
    if known_app {
        return ("app_only".to_owned(), "只能打开应用".to_owned());
    }
    ("unsupported".to_owned(), "当前环境不支持跳转".to_owned())
}

fn project_name(path: &str) -> Option<&str> {
    Path::new(path).file_name().and_then(|value| value.to_str())
}

fn storage_error(error: rusqlite::Error) -> StoreError {
    StoreError::Storage(error.to_string())
}

fn to_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

fn from_i64(value: i64) -> u64 {
    value.max(0) as u64
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

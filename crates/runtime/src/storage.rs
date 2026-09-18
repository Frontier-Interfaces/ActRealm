use crate::fsutil::ensure_private_directory;
use crate::title::{
    resolve_codex_session_titles, resolve_event_title, resolve_session_title, ProviderTitle,
};
use actrealm_core::{
    is_codex_native_attention_tool, AttentionRiskCode, BridgeRequest, Decision, EventKind,
    OperationCategory, Provider, RemoteActionCapability, PERMISSION_COMMIT_DELAY_MS,
};
use actrealm_providers::{parse_hook, ParsedHookEvent};
use rusqlite::trace::{TraceEvent, TraceEventCodes};
use rusqlite::types::ValueRef;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, ToSql, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Number, Value};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
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

const SCHEMA_VERSION: i64 = 38;
const SNAPSHOT_CACHE_MAX_AGE_MS: u64 = 2_000;
const STORAGE_DIAGNOSTICS_CACHE_MAX_AGE_MS: u64 = 60_000;
const SQLITE_MAX_VARIABLE_NUMBER: usize = 32_766;
const MAX_UI_SNAPSHOT_SESSION_IDS: usize = SQLITE_MAX_VARIABLE_NUMBER - 1;
const MAX_TASK_TITLE_CHARS: usize = 64;
const MAX_PLAN_STEPS: usize = 64;
const MAX_ACTIVE_SUBAGENTS: usize = 64;
const MAX_PLAN_STEP_CHARS: usize = 500;
const MAX_PLAN_DETAIL_CHARS: usize = 1_500;
const MAX_TOOL_TARGET_CHARS: usize = 96;
const PROVIDER_TITLE_REFRESH_INTERVAL_MS: u64 = 2_000;
const PROVIDER_TITLE_ACTIVE_WINDOW_MS: u64 = 30 * 60 * 1_000;
const UI_SETTINGS_KEY: &str = "ui_settings";
const CODEX_INTERNAL_PROMPT_PREFIXES: [&str; 4] = [
    "# Overview Generate 0 to 3 hyperpersonalized suggestions",
    "You are an expert at upholding safety and compliance standards",
    "# Memory Writing Agent:",
    "## Memory Writing Agent:",
];

thread_local! {
    static UI_SNAPSHOT_QUERY_COUNT: Cell<Option<usize>> = const { Cell::new(None) };
}

fn count_ui_snapshot_query(event: TraceEvent<'_>) {
    if matches!(event, TraceEvent::Stmt(_, _)) {
        UI_SNAPSHOT_QUERY_COUNT.with(|count| {
            if let Some(value) = count.get() {
                count.set(Some(value.saturating_add(1)));
            }
        });
    }
}

struct UiSnapshotQueryTrace<'connection> {
    connection: &'connection Connection,
}

impl<'connection> UiSnapshotQueryTrace<'connection> {
    fn install(connection: &'connection Connection) -> Self {
        UI_SNAPSHOT_QUERY_COUNT.with(|count| count.set(Some(0)));
        connection.trace_v2(
            TraceEventCodes::SQLITE_TRACE_STMT,
            Some(count_ui_snapshot_query),
        );
        Self { connection }
    }

    fn query_count(&self) -> usize {
        UI_SNAPSHOT_QUERY_COUNT.with(|count| count.get().unwrap_or_default())
    }
}

impl Drop for UiSnapshotQueryTrace<'_> {
    fn drop(&mut self) {
        self.connection
            .trace_v2(TraceEventCodes::SQLITE_TRACE_STMT, None);
        UI_SNAPSHOT_QUERY_COUNT.with(|count| count.set(None));
    }
}

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
    pub suppressed: bool,
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
    #[serde(default)]
    pub plan_steps: Vec<PlanStepRecord>,
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
    pub current_tool_category: Option<String>,
    pub current_target: Option<String>,
    pub active_subagents: u32,
    #[serde(default)]
    pub subagents: Vec<SubagentRecord>,
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
    #[serde(skip)]
    pub last_meaningful_activity_at: Option<u64>,
}

/// A bounded, local-only summary used by the History center. It intentionally
/// excludes prompts, commands, tool input/output, transcript text, file paths,
/// and Diff content. Review and Checkpoint details remain on-demand reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskHistoryRecord {
    pub id: String,
    pub provider: String,
    pub project: Option<String>,
    pub title: Option<String>,
    pub model: Option<String>,
    pub status: String,
    pub started_at: u64,
    pub last_event_at: u64,
    pub completed_at: Option<u64>,
    pub archived_at: Option<u64>,
    pub archive_reason: Option<String>,
    pub branch: Option<String>,
    pub validation_state: Option<String>,
    pub checkpoint_count: u32,
    pub security_event_count: u32,
    pub jump_capability: String,
    pub jump_label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskHistoryMutation {
    Applied,
    NotFound,
    Active,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalReviewContext {
    pub session_id: String,
    pub provider: String,
    pub provider_session_id: String,
    pub jump_capability: String,
    pub project: Option<String>,
    pub working_directory: Option<PathBuf>,
    pub exec_state: String,
    pub turn_id: Option<String>,
    pub turn_started_at: Option<u64>,
    pub turn_ended_at: Option<u64>,
    pub last_event_at: u64,
    pub concurrent_active_sessions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewBaselineCandidate {
    pub session_id: String,
    pub turn_id: String,
    pub working_directory: Option<PathBuf>,
    pub turn_started_at: u64,
    pub first_tool_at: Option<u64>,
    pub concurrent_active_sessions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewBaselineInput {
    pub session_id: String,
    pub turn_id: String,
    pub repository_root: PathBuf,
    pub repository_identity: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub worktree_kind: String,
    pub dirty: bool,
    pub changed_files: u64,
    pub staged_files: u64,
    pub unstaged_files: u64,
    pub untracked_files: u64,
    pub insertions: Option<u64>,
    pub deletions: Option<u64>,
    pub binary_files: Option<u64>,
    pub turn_started_at: u64,
    pub first_tool_at: Option<u64>,
    pub captured_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewBaselineRecord {
    pub session_id: String,
    pub turn_id: String,
    pub repository_root: PathBuf,
    pub repository_identity: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub worktree_kind: String,
    pub dirty: bool,
    pub changed_files: u64,
    pub staged_files: u64,
    pub unstaged_files: u64,
    pub untracked_files: u64,
    pub insertions: Option<u64>,
    pub deletions: Option<u64>,
    pub binary_files: Option<u64>,
    pub turn_started_at: u64,
    pub first_tool_at: Option<u64>,
    pub captured_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCheckpointInput {
    pub id: String,
    pub session_id: String,
    pub turn_id: String,
    pub label: Option<String>,
    pub kind: String,
    pub provider: String,
    pub provider_session_id: String,
    pub provider_resume_capability: String,
    pub repository_root: Option<PathBuf>,
    pub repository_identity: Option<String>,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub worktree_kind: Option<String>,
    pub dirty: Option<bool>,
    pub changed_files: Option<u64>,
    pub staged_files: Option<u64>,
    pub unstaged_files: Option<u64>,
    pub untracked_files: Option<u64>,
    pub git_object_id: Option<String>,
    pub git_ref: Option<String>,
    pub patch_digest: Option<String>,
    pub validation_json: String,
    pub review_baseline_captured_at: Option<u64>,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCheckpointRecord {
    pub id: String,
    pub session_id: String,
    pub turn_id: String,
    pub label: Option<String>,
    pub kind: String,
    pub provider: String,
    pub provider_session_id: String,
    pub provider_resume_capability: String,
    pub repository_root: Option<PathBuf>,
    pub repository_identity: Option<String>,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub worktree_kind: Option<String>,
    pub dirty: Option<bool>,
    pub changed_files: Option<u64>,
    pub staged_files: Option<u64>,
    pub unstaged_files: Option<u64>,
    pub untracked_files: Option<u64>,
    pub git_object_id: Option<String>,
    pub git_ref: Option<String>,
    pub patch_digest: Option<String>,
    pub validation_json: String,
    pub review_baseline_captured_at: Option<u64>,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStepRecord {
    pub id: String,
    pub text: String,
    pub detail: Option<String>,
    pub status: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentRecord {
    pub id: String,
    pub agent_type: Option<String>,
    pub status: String,
    pub source: Option<String>,
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
    #[serde(default)]
    pub primary_category: Option<OperationCategory>,
    #[serde(default)]
    pub risk_codes: Vec<AttentionRiskCode>,
    pub command_preview: Option<String>,
    pub expires_at: Option<u64>,
    #[serde(default)]
    pub auto_hide_at: Option<u64>,
    #[serde(default)]
    pub reminder_acknowledged_at: Option<u64>,
    #[serde(default)]
    pub reminder_resolution: Option<String>,
    #[serde(default)]
    pub retain_after_ack: bool,
    pub created_at: u64,
    pub resolution: Option<String>,
    #[serde(default)]
    pub remote_actionable: bool,
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
pub struct TokenUsageTotals {
    pub today: u64,
    pub month: u64,
    pub total: u64,
    pub active_days: u64,
    pub current_streak: u64,
    pub today_active_time_seconds: u64,
    pub month_active_time_seconds: u64,
    pub active_time_seconds: u64,
    pub today_execution_time_seconds: u64,
    pub month_execution_time_seconds: u64,
    pub execution_time_seconds: u64,
    pub turn_count: u64,
    pub message_count: u64,
    pub priced_tokens: u64,
    pub unpriced_tokens: u64,
    pub estimated_cost_usd_micros: Option<u64>,
    pub pricing_sources: Vec<TokenUsagePricingSource>,
    pub anomalies: Vec<TokenUsageAnomaly>,
    pub anomaly_count: u64,
    pub suspect_count: u64,
    pub peak_day: Option<String>,
    pub peak_day_total: u64,
    pub recorded_from: Option<u64>,
    pub captured_at: Option<u64>,
    pub by_provider: Vec<TokenUsageProviderTotal>,
    pub by_model: Vec<TokenUsageModelTotal>,
    pub recent_days: Vec<TokenUsageDayTotal>,
    pub detail_recorded_from: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsagePricingSource {
    pub cost_kind: String,
    pub source: String,
    pub token_total: u64,
    pub estimated_cost_usd_micros: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsageAnomaly {
    pub code: String,
    pub severity: String,
    pub scope: String,
    pub day: Option<String>,
    pub observed: Option<u64>,
    pub expected: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsageProviderTotal {
    pub provider: String,
    pub today: u64,
    pub month: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsageModelTotal {
    pub provider: String,
    pub model: String,
    pub total: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub estimated_cost_usd_micros: Option<u64>,
    pub priced_tokens: u64,
    pub unpriced_tokens: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsageDayProviderTotal {
    pub provider: String,
    pub total: u64,
    pub estimated_cost_usd_micros: Option<u64>,
    pub priced_tokens: u64,
    pub unpriced_tokens: u64,
    pub message_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsageDayModelTotal {
    pub provider: String,
    pub model: String,
    pub total: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub estimated_cost_usd_micros: Option<u64>,
    pub priced_tokens: u64,
    pub unpriced_tokens: u64,
    pub message_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsageDayTotal {
    pub day: String,
    pub total: u64,
    pub estimated_cost_usd_micros: Option<u64>,
    pub priced_tokens: u64,
    pub unpriced_tokens: u64,
    pub message_count: u64,
    pub by_provider: Vec<TokenUsageDayProviderTotal>,
    pub by_model: Vec<TokenUsageDayModelTotal>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsageProjectTotal {
    pub project: String,
    pub total: u64,
    pub task_count: u64,
    pub session_count: u64,
    pub captured_at: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsageTaskTotal {
    pub session_id: String,
    pub provider: String,
    pub project: Option<String>,
    pub title: Option<String>,
    pub model: Option<String>,
    pub total: u64,
    pub captured_at: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsageBurnRate {
    pub session_id: String,
    pub turn_id: String,
    pub provider: String,
    pub project: Option<String>,
    pub title: Option<String>,
    pub window_seconds: u64,
    pub token_delta: u64,
    pub tokens_per_minute: u64,
    pub sample_count: u64,
    pub baseline_tokens_per_minute: Option<u64>,
    pub ratio_basis_points: Option<u64>,
    pub state: String,
    pub captured_at: u64,
    pub threshold_exceeded: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsageDecisionSummary {
    pub schema_version: u16,
    pub source: String,
    pub generated_at: u64,
    pub freshness: String,
    pub captured_at: Option<u64>,
    pub total_tokens: u64,
    pub attributed_tokens: u64,
    pub unattributed_tokens: u64,
    pub attribution_coverage_basis_points: u64,
    pub project_attributed_tokens: u64,
    pub project_unattributed_tokens: u64,
    pub project_attribution_coverage_basis_points: u64,
    pub task_attributed_tokens: u64,
    pub task_unattributed_tokens: u64,
    pub task_attribution_coverage_basis_points: u64,
    pub project_totals: Vec<TokenUsageProjectTotal>,
    pub task_totals: Vec<TokenUsageTaskTotal>,
    pub burn_rates: Vec<TokenUsageBurnRate>,
    pub threshold_tokens_per_minute: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreSnapshot {
    pub sessions: Vec<SessionRecord>,
    pub attention: Vec<AttentionRecord>,
    pub commands: Vec<CommandRecord>,
    pub event_count: u64,
    pub metrics: MetricsSummary,
    pub token_usage: TokenUsageTotals,
}

/// Bounded local storage health returned only through the authenticated
/// Runtime diagnostics API. It never includes the database path or contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageDiagnostics {
    pub schema_version: i64,
    pub expected_schema_version: i64,
    pub integrity: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimelineEventKind {
    #[serde(rename = "session.started")]
    SessionStarted,
    #[serde(rename = "session.ended")]
    SessionEnded,
    #[serde(rename = "turn.started")]
    TurnStarted,
    #[serde(rename = "turn.completed")]
    TurnCompleted,
    #[serde(rename = "turn.interrupted")]
    TurnInterrupted,
    #[serde(rename = "turn.failed")]
    TurnFailed,
    #[serde(rename = "tool.started")]
    ToolStarted,
    #[serde(rename = "tool.completed")]
    ToolCompleted,
    #[serde(rename = "tool.failed")]
    ToolFailed,
    #[serde(rename = "approval.requested")]
    ApprovalRequested,
    #[serde(rename = "approval.resolved")]
    ApprovalResolved,
    #[serde(rename = "question.requested")]
    QuestionRequested,
    #[serde(rename = "elicitation.requested")]
    ElicitationRequested,
    #[serde(rename = "subagent.started")]
    SubagentStarted,
    #[serde(rename = "subagent.completed")]
    SubagentCompleted,
    #[serde(rename = "task.created")]
    TaskCreated,
    #[serde(rename = "task.completed")]
    TaskCompleted,
    #[serde(rename = "plan.updated")]
    PlanUpdated,
    #[serde(rename = "session.compacting")]
    SessionCompacting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineRiskLevel {
    Low,
    Medium,
    High,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineContextAvailability {
    Available,
    AnchorMissing,
    SourceRotated,
    ProviderUnsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineEventPhase {
    Session,
    Turn,
    Tool,
    Attention,
    Plan,
    Subagent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineEventStatus {
    Started,
    Running,
    Completed,
    Failed,
    Interrupted,
    Requested,
    Resolved,
    Updated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineEventConfidence {
    ProviderFact,
    RuntimeDerived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEventRecord {
    pub schema_version: u16,
    pub event_id: String,
    pub provider: String,
    pub kind: TimelineEventKind,
    pub tool_name: Option<String>,
    pub tool_category: Option<String>,
    pub tool_target: Option<String>,
    pub tool_call_id: Option<String>,
    pub source_version: Option<String>,
    pub validation_status: Option<String>,
    pub phase: TimelineEventPhase,
    pub status: TimelineEventStatus,
    pub confidence: TimelineEventConfidence,
    pub risk_level: Option<TimelineRiskLevel>,
    pub plan_step_count: Option<u32>,
    pub turn_id: Option<String>,
    pub outbox_id: Option<String>,
    pub occurred_at: u64,
    pub ingest_sequence: u64,
    pub context_availability: TimelineContextAvailability,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelinePage {
    pub events: Vec<TimelineEventRecord>,
    pub next_after_ingest_sequence: Option<u64>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TimelineReadOptions {
    after_ingest_sequence: Option<u64>,
    before_ingest_sequence: Option<u64>,
    limit: usize,
    latest: bool,
    current_turn_only: bool,
    include_local_tool_target: bool,
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
    pub limit_id: Option<String>,
    pub used_pct: f64,
    pub resets_at: u64,
    pub source: String,
    pub captured_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionUsageRecord {
    pub provider: String,
    pub provider_session_id: String,
    pub project_id: Option<String>,
    pub project_label: Option<String>,
    pub parent_provider_session_id: Option<String>,
    pub model: Option<String>,
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
    pub daily_usage: Vec<SessionUsageDailyRecord>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionUsageDailyRecord {
    pub day: String,
    pub model: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub reasoning_tokens: u64,
    pub token_total: u64,
    pub estimated_cost_usd_micros: Option<u64>,
    pub cost_kind: Option<String>,
    pub pricing_source: Option<String>,
    pub message_count: u64,
}

#[derive(Clone)]
pub struct RuntimeStore {
    inner: Arc<StoreInner>,
}

struct StoreInner {
    outcomes: Mutex<crate::outcome::OutcomeRegistry>,
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
        commit_delay_ms: u64,
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
    ObserveCodexTurnEnd {
        thread_id: String,
        turn_id: String,
        event: String,
        at: u64,
        reply: mpsc::SyncSender<Result<bool, StoreError>>,
    },
    MarkExecutionUnconfirmed {
        session_id: String,
        expected_last_event_at: u64,
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
    SyncProviderExecution {
        provider: Provider,
        provider_session_id: String,
        active: bool,
        now: u64,
        reply: mpsc::SyncSender<Result<bool, StoreError>>,
    },
    ResolveManagedRequest {
        request_id: Uuid,
        now: u64,
        reply: mpsc::SyncSender<Result<bool, StoreError>>,
    },
    Snapshot {
        reply: mpsc::SyncSender<Result<StoreSnapshot, StoreError>>,
    },
    StorageDiagnostics {
        reply: mpsc::SyncSender<Result<StorageDiagnostics, StoreError>>,
    },
    TaskHistory {
        cutoff: u64,
        limit: usize,
        reply: mpsc::SyncSender<Result<Vec<TaskHistoryRecord>, StoreError>>,
    },
    ArchiveTask {
        session_id: String,
        now: u64,
        reply: mpsc::SyncSender<Result<TaskHistoryMutation, StoreError>>,
    },
    DeleteTaskHistory {
        session_id: String,
        now: u64,
        reply: mpsc::SyncSender<Result<TaskHistoryMutation, StoreError>>,
    },
    UiSnapshot {
        cutoff: u64,
        reply: mpsc::SyncSender<Result<StoreSnapshot, StoreError>>,
    },
    UiSnapshotWithQueryCount {
        cutoff: u64,
        reply: mpsc::SyncSender<Result<(StoreSnapshot, usize), StoreError>>,
    },
    Timeline {
        session_id: String,
        after_ingest_sequence: Option<u64>,
        before_ingest_sequence: Option<u64>,
        limit: usize,
        latest: bool,
        current_turn_only: bool,
        include_local_tool_target: bool,
        reply: mpsc::SyncSender<Result<Option<TimelinePage>, StoreError>>,
    },
    LocalReviewContext {
        session_id: String,
        reply: mpsc::SyncSender<Result<Option<LocalReviewContext>, StoreError>>,
    },
    PendingReviewBaselines {
        limit: usize,
        reply: mpsc::SyncSender<Result<Vec<ReviewBaselineCandidate>, StoreError>>,
    },
    WriteReviewBaseline {
        baseline: Box<ReviewBaselineInput>,
        reply: mpsc::SyncSender<Result<bool, StoreError>>,
    },
    ReviewBaseline {
        turn_id: String,
        reply: mpsc::SyncSender<Result<Option<ReviewBaselineRecord>, StoreError>>,
    },
    TokenUsageDecision {
        now: u64,
        threshold_tokens_per_minute: Option<u64>,
        reply: mpsc::SyncSender<Result<TokenUsageDecisionSummary, StoreError>>,
    },
    CreateTaskCheckpoint {
        input: Box<TaskCheckpointInput>,
        reply: mpsc::SyncSender<Result<TaskCheckpointRecord, StoreError>>,
    },
    ListTaskCheckpoints {
        session_id: String,
        reply: mpsc::SyncSender<Result<Vec<TaskCheckpointRecord>, StoreError>>,
    },
    TaskCheckpoint {
        checkpoint_id: String,
        reply: mpsc::SyncSender<Result<Option<TaskCheckpointRecord>, StoreError>>,
    },
    DeleteTaskCheckpoint {
        checkpoint_id: String,
        reply: mpsc::SyncSender<Result<bool, StoreError>>,
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
    WriteUiSettings {
        value: String,
        now: u64,
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
    BeginUsageCollectionGeneration {
        reply: mpsc::SyncSender<Result<u64, StoreError>>,
    },
    ReplaceSessionUsage {
        records: Vec<SessionUsageRecord>,
        collected_at: u64,
        generation: u64,
        reply: mpsc::SyncSender<Result<bool, StoreError>>,
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
    ExportTokenUsageJson {
        now: u64,
        reply: mpsc::SyncSender<Result<Value, StoreError>>,
    },
    ExportTokenUsageCsv {
        now: u64,
        reply: mpsc::SyncSender<Result<String, StoreError>>,
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
        let mut connection = Connection::open(&path).map_err(storage_error)?;
        initialize(&mut connection)?;
        let (sender, receiver) = mpsc::channel();
        let writer = thread::Builder::new()
            .name("actrealm-sqlite-writer".to_owned())
            .spawn(move || writer_loop(connection, path, receiver))
            .map_err(|error| StoreError::Storage(error.to_string()))?;
        Ok(Self {
            inner: Arc::new(StoreInner {
                outcomes: Mutex::new(crate::outcome::OutcomeRegistry::default()),
                sender,
                writer: Mutex::new(Some(writer)),
            }),
        })
    }

    pub fn ingest(&self, request: BridgeRequest) -> Result<IngestResult, StoreError> {
        let event = request
            .raw
            .get("hook_event_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let at = request.received_at;
        let message = request
            .raw
            .get("last_assistant_message")
            .and_then(Value::as_str)
            .filter(|_| matches!(event.as_str(), "Stop" | "StopFailure"))
            .map(str::to_owned);
        let cwd = request
            .raw
            .get("cwd")
            .and_then(Value::as_str)
            .map(PathBuf::from);
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::Ingest {
            request: Box::new(request),
            reply,
        })?;
        let result: IngestResult = receive(receiver)?;
        if result.inserted && !result.suppressed {
            if event == "UserPromptSubmit" {
                if let Ok(mut registry) = self.inner.outcomes.lock() {
                    registry.clear(&result.session_id, at);
                }
            }
            if let Some(message) = message {
                self.observe_result(
                    &result.session_id,
                    &message,
                    cwd.as_deref(),
                    at,
                    &format!("hook:{event}"),
                );
            }
        }
        Ok(result)
    }

    pub fn clear_result(&self, session: &str, at: u64) {
        if let Ok(mut registry) = self.inner.outcomes.lock() {
            registry.clear(session, at);
        }
    }

    pub fn observe_result(
        &self,
        session: &str,
        text: &str,
        cwd: Option<&Path>,
        at: u64,
        source: &str,
    ) {
        if let Ok(mut registry) = self.inner.outcomes.lock() {
            registry.observe(session, text, cwd, at, source);
        }
    }

    pub fn session_result(
        &self,
        session: &str,
        start: u64,
        now: u64,
    ) -> Option<crate::SessionResult> {
        self.inner.outcomes.lock().ok()?.get(session, start, now)
    }

    pub fn result_artifact_path(
        &self,
        session: &str,
        artifact: &str,
        start: u64,
        now: u64,
    ) -> Option<PathBuf> {
        self.inner
            .outcomes
            .lock()
            .ok()?
            .reveal_path(session, artifact, start, now)
    }

    pub fn claim_approval(
        &self,
        command_id: Uuid,
        request_id: Uuid,
        action: ApprovalAction,
        now: u64,
    ) -> Result<ClaimResult, StoreError> {
        self.claim_approval_with_delay(
            command_id,
            request_id,
            action,
            now,
            PERMISSION_COMMIT_DELAY_MS,
        )
    }

    pub fn claim_approval_with_delay(
        &self,
        command_id: Uuid,
        request_id: Uuid,
        action: ApprovalAction,
        now: u64,
        commit_delay_ms: u64,
    ) -> Result<ClaimResult, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::Claim {
            command_id,
            request_id,
            action,
            now,
            commit_delay_ms,
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

    pub fn observe_codex_turn_end(
        &self,
        thread_id: &str,
        turn_id: &str,
        event: &str,
        at: u64,
    ) -> Result<bool, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::ObserveCodexTurnEnd {
            thread_id: thread_id.into(),
            turn_id: turn_id.into(),
            event: event.into(),
            at,
            reply,
        })?;
        receive(receiver)
    }

    pub fn mark_execution_unconfirmed(
        &self,
        session_id: &str,
        expected_last_event_at: u64,
    ) -> Result<bool, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::MarkExecutionUnconfirmed {
            session_id: session_id.into(),
            expected_last_event_at,
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

    /// Reconciles an explicit Provider connector lifecycle signal with the
    /// Runtime session projection. Process existence alone is never accepted
    /// as evidence that a turn is active.
    pub fn sync_provider_execution(
        &self,
        provider: Provider,
        provider_session_id: impl Into<String>,
        active: bool,
        now: u64,
    ) -> Result<bool, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::SyncProviderExecution {
            provider,
            provider_session_id: provider_session_id.into(),
            active,
            now,
            reply,
        })?;
        receive(receiver)
    }

    pub fn resolve_managed_request(&self, request_id: Uuid, now: u64) -> Result<bool, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::ResolveManagedRequest {
            request_id,
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

    pub fn storage_diagnostics(&self) -> Result<StorageDiagnostics, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::StorageDiagnostics { reply })?;
        receive(receiver)
    }

    /// Returns only tasks outside the active-board projection. The result is
    /// bounded so opening History cannot materialize the full event archive.
    pub fn task_history(
        &self,
        cutoff: u64,
        limit: usize,
    ) -> Result<Vec<TaskHistoryRecord>, StoreError> {
        if limit == 0 || limit > 500 {
            return Err(StoreError::Storage(
                "task history limit must be between 1 and 500".to_owned(),
            ));
        }
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::TaskHistory {
            cutoff,
            limit,
            reply,
        })?;
        receive(receiver)
    }

    pub fn archive_task(
        &self,
        session_id: impl Into<String>,
        now: u64,
    ) -> Result<TaskHistoryMutation, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::ArchiveTask {
            session_id: session_id.into(),
            now,
            reply,
        })?;
        receive(receiver)
    }

    pub fn delete_task_history(
        &self,
        session_id: impl Into<String>,
        now: u64,
    ) -> Result<TaskHistoryMutation, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::DeleteTaskHistory {
            session_id: session_id.into(),
            now,
            reply,
        })?;
        receive(receiver)
    }

    /// Returns a local-only working-directory context for the authenticated
    /// Review endpoint. The path is never serialized into Runtime snapshots or
    /// Companion projections.
    pub fn local_review_context(
        &self,
        session_id: impl Into<String>,
    ) -> Result<Option<LocalReviewContext>, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::LocalReviewContext {
            session_id: session_id.into(),
            reply,
        })?;
        receive(receiver)
    }

    pub fn pending_review_baselines(
        &self,
        limit: usize,
    ) -> Result<Vec<ReviewBaselineCandidate>, StoreError> {
        if limit == 0 || limit > 16 {
            return Err(StoreError::Storage(
                "review baseline limit must be between 1 and 16".to_owned(),
            ));
        }
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::PendingReviewBaselines { limit, reply })?;
        receive(receiver)
    }

    pub fn write_review_baseline(&self, baseline: ReviewBaselineInput) -> Result<bool, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::WriteReviewBaseline {
            baseline: Box::new(baseline),
            reply,
        })?;
        receive(receiver)
    }

    pub fn review_baseline(
        &self,
        turn_id: impl Into<String>,
    ) -> Result<Option<ReviewBaselineRecord>, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::ReviewBaseline {
            turn_id: turn_id.into(),
            reply,
        })?;
        receive(receiver)
    }

    /// Reads local-only task/project attribution and live sliding-window burn
    /// rates. It is intentionally separate from Companion/Cloud projection.
    pub fn token_usage_decision(
        &self,
        now: u64,
        threshold_tokens_per_minute: Option<u64>,
    ) -> Result<TokenUsageDecisionSummary, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::TokenUsageDecision {
            now,
            threshold_tokens_per_minute,
            reply,
        })?;
        receive(receiver)
    }

    pub fn create_task_checkpoint(
        &self,
        input: TaskCheckpointInput,
    ) -> Result<TaskCheckpointRecord, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::CreateTaskCheckpoint {
            input: Box::new(input),
            reply,
        })?;
        receive(receiver)
    }

    pub fn task_checkpoints(
        &self,
        session_id: impl Into<String>,
    ) -> Result<Vec<TaskCheckpointRecord>, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::ListTaskCheckpoints {
            session_id: session_id.into(),
            reply,
        })?;
        receive(receiver)
    }

    pub fn task_checkpoint(
        &self,
        checkpoint_id: impl Into<String>,
    ) -> Result<Option<TaskCheckpointRecord>, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::TaskCheckpoint {
            checkpoint_id: checkpoint_id.into(),
            reply,
        })?;
        receive(receiver)
    }

    pub fn delete_task_checkpoint(
        &self,
        checkpoint_id: impl Into<String>,
    ) -> Result<bool, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::DeleteTaskCheckpoint {
            checkpoint_id: checkpoint_id.into(),
            reply,
        })?;
        receive(receiver)
    }

    pub fn timeline(
        &self,
        session_id: impl Into<String>,
        after_ingest_sequence: Option<u64>,
        limit: usize,
    ) -> Result<Option<TimelinePage>, StoreError> {
        if limit == 0 || limit > 100 {
            return Err(StoreError::Storage(
                "timeline limit must be between 1 and 100".to_owned(),
            ));
        }
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::Timeline {
            session_id: session_id.into(),
            after_ingest_sequence,
            before_ingest_sequence: None,
            limit,
            latest: false,
            current_turn_only: false,
            include_local_tool_target: false,
            reply,
        })?;
        receive(receiver)
    }

    /// Returns the newest bounded events in chronological order. This is used
    /// for the initial companion activity load so a long-running task does not
    /// replay its entire history before the live tail becomes visible.
    pub fn latest_timeline(
        &self,
        session_id: impl Into<String>,
        limit: usize,
    ) -> Result<Option<TimelinePage>, StoreError> {
        if limit == 0 || limit > 100 {
            return Err(StoreError::Storage(
                "timeline limit must be between 1 and 100".to_owned(),
            ));
        }
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::Timeline {
            session_id: session_id.into(),
            after_ingest_sequence: None,
            before_ingest_sequence: None,
            limit,
            latest: true,
            current_turn_only: false,
            include_local_tool_target: false,
            reply,
        })?;
        receive(receiver)
    }

    /// Returns only the newest Provider turn. Active-task companions should
    /// not replay workflow rows from older turns in the same chat session.
    pub fn latest_current_timeline(
        &self,
        session_id: impl Into<String>,
        limit: usize,
    ) -> Result<Option<TimelinePage>, StoreError> {
        self.timeline_page(
            session_id.into(),
            TimelineReadOptions {
                after_ingest_sequence: None,
                before_ingest_sequence: None,
                limit,
                latest: true,
                current_turn_only: true,
                include_local_tool_target: false,
            },
        )
    }

    /// Local authenticated native-client projection. It may include a bounded
    /// basename captured from an allowlisted path field; Companion and Team
    /// timelines deliberately use `timeline` / `latest_timeline` instead.
    pub fn local_timeline(
        &self,
        session_id: impl Into<String>,
        after_ingest_sequence: Option<u64>,
        limit: usize,
    ) -> Result<Option<TimelinePage>, StoreError> {
        self.local_timeline_page(session_id.into(), after_ingest_sequence, limit, false)
    }

    pub fn latest_local_timeline(
        &self,
        session_id: impl Into<String>,
        limit: usize,
    ) -> Result<Option<TimelinePage>, StoreError> {
        self.local_timeline_page(session_id.into(), None, limit, true)
    }

    pub fn latest_current_local_timeline(
        &self,
        session_id: impl Into<String>,
        limit: usize,
    ) -> Result<Option<TimelinePage>, StoreError> {
        self.timeline_page(
            session_id.into(),
            TimelineReadOptions {
                after_ingest_sequence: None,
                before_ingest_sequence: None,
                limit,
                latest: true,
                current_turn_only: true,
                include_local_tool_target: true,
            },
        )
    }

    /// Loads the preceding page for the current Provider turn. The returned
    /// events remain chronological so native clients can prepend them without
    /// reordering the live tail.
    pub fn latest_current_local_timeline_before(
        &self,
        session_id: impl Into<String>,
        before_ingest_sequence: u64,
        limit: usize,
    ) -> Result<Option<TimelinePage>, StoreError> {
        self.timeline_page(
            session_id.into(),
            TimelineReadOptions {
                after_ingest_sequence: None,
                before_ingest_sequence: Some(before_ingest_sequence),
                limit,
                latest: true,
                current_turn_only: true,
                include_local_tool_target: true,
            },
        )
    }

    fn local_timeline_page(
        &self,
        session_id: String,
        after_ingest_sequence: Option<u64>,
        limit: usize,
        latest: bool,
    ) -> Result<Option<TimelinePage>, StoreError> {
        self.timeline_page(
            session_id,
            TimelineReadOptions {
                after_ingest_sequence,
                before_ingest_sequence: None,
                limit,
                latest,
                current_turn_only: false,
                include_local_tool_target: true,
            },
        )
    }

    fn timeline_page(
        &self,
        session_id: String,
        options: TimelineReadOptions,
    ) -> Result<Option<TimelinePage>, StoreError> {
        if options.limit == 0 || options.limit > 100 {
            return Err(StoreError::Storage(
                "timeline limit must be between 1 and 100".to_owned(),
            ));
        }
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::Timeline {
            session_id,
            after_ingest_sequence: options.after_ingest_sequence,
            before_ingest_sequence: options.before_ingest_sequence,
            limit: options.limit,
            latest: options.latest,
            current_turn_only: options.current_turn_only,
            include_local_tool_target: options.include_local_tool_target,
            reply,
        })?;
        receive(receiver)
    }

    /// Reads the bounded Runtime projection used by the local UI.
    pub fn ui_snapshot(&self, cutoff: u64) -> Result<StoreSnapshot, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::UiSnapshot { cutoff, reply })?;
        receive(receiver)
    }

    /// Returns a bounded UI snapshot with its SQL statement count for regression tests.
    #[doc(hidden)]
    pub fn ui_snapshot_with_query_count(
        &self,
        cutoff: u64,
    ) -> Result<(StoreSnapshot, usize), StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::UiSnapshotWithQueryCount { cutoff, reply })?;
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

    /// Persists the versioned UI settings and applies the completion-retention
    /// policy to already-open completion items in the same SQLite transaction.
    pub fn write_ui_settings(&self, value: impl Into<String>, now: u64) -> Result<(), StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::WriteUiSettings {
            value: value.into(),
            now,
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

    /// Begins a monotonic generation before a local usage collection starts.
    /// A result can replace derived usage only while its generation is current.
    pub fn begin_usage_collection_generation(&self) -> Result<u64, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::BeginUsageCollectionGeneration { reply })?;
        receive(receiver)
    }

    /// Atomically replaces the local derived-usage view if `generation` is
    /// still current. `captured_at` remains provider metadata, not ordering.
    pub fn replace_session_usages_for_generation(
        &self,
        records: Vec<SessionUsageRecord>,
        collected_at: u64,
        generation: u64,
    ) -> Result<bool, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::ReplaceSessionUsage {
            records,
            collected_at,
            generation,
            reply,
        })?;
        receive(receiver)
    }

    /// Atomically replaces the local derived-usage view with a newly started
    /// generation. Prefer the generation methods when collection happens
    /// outside the caller's immediate control flow.
    pub fn replace_session_usages(
        &self,
        records: Vec<SessionUsageRecord>,
        collected_at: u64,
    ) -> Result<(), StoreError> {
        let generation = self.begin_usage_collection_generation()?;
        self.replace_session_usages_for_generation(records, collected_at, generation)
            .map(|_| ())
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

    pub fn export_token_usage_json(&self, now: u64) -> Result<Value, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::ExportTokenUsageJson { now, reply })?;
        receive(receiver)
    }

    pub fn export_token_usage_csv(&self, now: u64) -> Result<String, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::ExportTokenUsageCsv { now, reply })?;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionTaskHidePolicy {
    AfterConfirmation,
    AfterDelay(u64),
    Manual,
}

impl CompletionTaskHidePolicy {
    const fn delay_ms(self) -> Option<u64> {
        match self {
            Self::AfterDelay(delay) => Some(delay),
            Self::AfterConfirmation | Self::Manual => None,
        }
    }

    const fn retain_after_ack(self) -> bool {
        matches!(self, Self::AfterDelay(_) | Self::Manual)
    }
}

fn completion_task_hide_policy(encoded: &str) -> Result<CompletionTaskHidePolicy, StoreError> {
    let value = serde_json::from_str::<Value>(encoded)
        .map_err(|error| StoreError::Storage(format!("settings JSON is invalid: {error}")))?;
    let mode = value
        .get("completionTaskHideMode")
        .and_then(Value::as_str)
        .unwrap_or("afterConfirmation");
    match mode {
        "afterConfirmation" => Ok(CompletionTaskHidePolicy::AfterConfirmation),
        "afterDelay" => {
            let minutes = value
                .get("completionAutoHideMinutes")
                .and_then(Value::as_u64)
                .unwrap_or(30);
            if !matches!(minutes, 5 | 15 | 30 | 60) {
                return Err(StoreError::Storage(
                    "completionAutoHideMinutes must be 5, 15, 30, or 60".to_owned(),
                ));
            }
            Ok(CompletionTaskHidePolicy::AfterDelay(
                minutes.saturating_mul(60_000),
            ))
        }
        "manual" => Ok(CompletionTaskHidePolicy::Manual),
        _ => Err(StoreError::Storage(
            "completionTaskHideMode must be afterConfirmation, afterDelay, or manual".to_owned(),
        )),
    }
}

fn load_completion_task_hide_policy(
    connection: &Connection,
) -> Result<CompletionTaskHidePolicy, StoreError> {
    connection
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [UI_SETTINGS_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(storage_error)?
        .map(|encoded| completion_task_hide_policy(&encoded))
        .transpose()
        .map(|value| value.unwrap_or(CompletionTaskHidePolicy::AfterConfirmation))
}

fn write_ui_settings_transaction(
    connection: &mut Connection,
    value: &str,
    _now: u64,
) -> Result<CompletionTaskHidePolicy, StoreError> {
    let hide_policy = completion_task_hide_policy(value)?;
    let transaction = connection.transaction().map_err(storage_error)?;
    transaction
        .execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![UI_SETTINGS_KEY, value],
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)?;
    Ok(hide_policy)
}

fn refresh_time_driven_attention(
    connection: &mut Connection,
    now: u64,
) -> Result<usize, StoreError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    let reopened = transaction
        .execute(
            "UPDATE attention_items SET state = 'open', expires_at = NULL
             WHERE state = 'snoozed' AND expires_at <= ?1",
            [to_i64(now)],
        )
        .map_err(storage_error)?;
    let _sessions_hidden = transaction
        .execute(
            "UPDATE sessions
             SET task_hidden_at = ?1, task_hidden_reason = 'auto_hidden'
             WHERE id IN (
               SELECT session_id FROM attention_items
               WHERE kind = 'completion' AND state IN ('open', 'snoozed')
                 AND auto_hide_at IS NOT NULL AND auto_hide_at <= ?1
             )",
            [to_i64(now)],
        )
        .map_err(storage_error)?;
    let auto_hidden_attention = transaction
        .execute(
            "UPDATE attention_items
             SET state = 'resolved', resolved_at = ?1,
                 resolution = 'auto_hidden', expires_at = NULL, auto_hide_at = NULL
             WHERE kind = 'completion' AND state IN ('open', 'snoozed')
               AND auto_hide_at IS NOT NULL AND auto_hide_at <= ?1",
            [to_i64(now)],
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)?;
    Ok(reopened.saturating_add(auto_hidden_attention))
}

fn writer_loop(
    mut connection: Connection,
    database_path: PathBuf,
    receiver: mpsc::Receiver<StoreMessage>,
) {
    let mut last_provider_title_refresh_at = 0;
    let mut latest_usage_collection_generation = 0_u64;
    let mut latest_usage_records: Option<Vec<SessionUsageRecord>> = None;
    let mut snapshot_cache: Option<(u64, StoreSnapshot)> = None;
    let mut storage_diagnostics_cache: Option<(u64, StorageDiagnostics)> = None;
    let mut ui_snapshot_cache: Option<(u64, u64, StoreSnapshot)> = None;
    let mut token_decision_cache: Option<(u64, Option<u64>, TokenUsageDecisionSummary)> = None;
    let mut completion_hide_policy = load_completion_task_hide_policy(&connection)
        .unwrap_or(CompletionTaskHidePolicy::AfterConfirmation);
    while let Ok(message) = receiver.recv() {
        if !matches!(
            &message,
            StoreMessage::Snapshot { .. }
                | StoreMessage::StorageDiagnostics { .. }
                | StoreMessage::UiSnapshot { .. }
                | StoreMessage::UiSnapshotWithQueryCount { .. }
                | StoreMessage::TokenUsageDecision { .. }
                | StoreMessage::ListTaskCheckpoints { .. }
                | StoreMessage::TaskCheckpoint { .. }
                | StoreMessage::ReadSetting { .. }
                | StoreMessage::Export { .. }
                | StoreMessage::ExportMetrics { .. }
                | StoreMessage::ExportTokenUsageJson { .. }
                | StoreMessage::ExportTokenUsageCsv { .. }
                | StoreMessage::BeginUsageCollectionGeneration { .. }
                | StoreMessage::ReplaceSessionUsage { .. }
                | StoreMessage::Shutdown
        ) {
            snapshot_cache = None;
            ui_snapshot_cache = None;
        }
        match message {
            StoreMessage::Ingest { request, reply } => {
                let result = ingest_transaction(&mut connection, *request, completion_hide_policy);
                if result.is_ok() {
                    // A newly observed Provider session can make an existing
                    // usage record eligible for the local session projection.
                    latest_usage_records = None;
                    token_decision_cache = None;
                }
                let _ = reply.send(result);
            }
            StoreMessage::Claim {
                command_id,
                request_id,
                action,
                now,
                commit_delay_ms,
                reply,
            } => {
                let _ = reply.send(claim_transaction(
                    &mut connection,
                    command_id,
                    request_id,
                    action,
                    now,
                    commit_delay_ms,
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
            StoreMessage::ObserveCodexTurnEnd {
                thread_id,
                turn_id,
                event,
                at,
                reply,
            } => {
                let _ = reply.send(observe_codex_turn_end_transaction(
                    &mut connection,
                    &thread_id,
                    &turn_id,
                    &event,
                    at,
                    completion_hide_policy,
                ));
            }
            StoreMessage::MarkExecutionUnconfirmed {
                session_id,
                expected_last_event_at,
                reply,
            } => {
                let _ = reply.send(mark_execution_unconfirmed_transaction(
                    &mut connection,
                    &session_id,
                    expected_last_event_at,
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
                    completion_hide_policy,
                ));
            }
            StoreMessage::SyncProviderExecution {
                provider,
                provider_session_id,
                active,
                now,
                reply,
            } => {
                let _ = reply.send(sync_provider_execution_transaction(
                    &mut connection,
                    provider,
                    &provider_session_id,
                    active,
                    now,
                ));
            }
            StoreMessage::ResolveManagedRequest {
                request_id,
                now,
                reply,
            } => {
                let _ = reply.send(resolve_managed_request_transaction(
                    &mut connection,
                    request_id,
                    now,
                ));
            }
            StoreMessage::Snapshot { reply } => {
                let now = now_millis();
                let cache_is_fresh = snapshot_cache.as_ref().is_some_and(|(built_at, _)| {
                    now.saturating_sub(*built_at) < SNAPSHOT_CACHE_MAX_AGE_MS
                });
                let result = if cache_is_fresh {
                    Ok(snapshot_cache
                        .as_ref()
                        .map(|(_, snapshot)| snapshot.clone())
                        .unwrap_or_default())
                } else {
                    refresh_time_driven_attention(&mut connection, now).and_then(|changed| {
                        if changed > 0 {
                            snapshot_cache = None;
                            ui_snapshot_cache = None;
                        }
                        if now.saturating_sub(last_provider_title_refresh_at)
                            >= PROVIDER_TITLE_REFRESH_INTERVAL_MS
                        {
                            refresh_provider_titles(&mut connection, now)?;
                            last_provider_title_refresh_at = now;
                        }
                        let snapshot = read_snapshot(&connection, None)?;
                        snapshot_cache = Some((now, snapshot.clone()));
                        Ok(snapshot)
                    })
                };
                let _ = reply.send(result);
            }
            StoreMessage::StorageDiagnostics { reply } => {
                let now = now_millis();
                let result = if storage_diagnostics_cache
                    .as_ref()
                    .is_some_and(|(checked_at, _)| {
                        now.saturating_sub(*checked_at) < STORAGE_DIAGNOSTICS_CACHE_MAX_AGE_MS
                    }) {
                    Ok(storage_diagnostics_cache
                        .as_ref()
                        .map(|(_, diagnostics)| diagnostics.clone())
                        .unwrap_or(StorageDiagnostics {
                            schema_version: 0,
                            expected_schema_version: SCHEMA_VERSION,
                            integrity: "unavailable".to_owned(),
                        }))
                } else {
                    read_storage_diagnostics(&connection).inspect(|diagnostics| {
                        storage_diagnostics_cache = Some((now, diagnostics.clone()));
                    })
                };
                let _ = reply.send(result);
            }
            StoreMessage::TaskHistory {
                cutoff,
                limit,
                reply,
            } => {
                let _ = reply.send(read_task_history(&connection, cutoff, limit));
            }
            StoreMessage::ArchiveTask {
                session_id,
                now,
                reply,
            } => {
                let result = archive_task_transaction(&mut connection, &session_id, now);
                if result == Ok(TaskHistoryMutation::Applied) {
                    snapshot_cache = None;
                    ui_snapshot_cache = None;
                }
                let _ = reply.send(result);
            }
            StoreMessage::DeleteTaskHistory {
                session_id,
                now,
                reply,
            } => {
                let result = delete_task_history_transaction(&mut connection, &session_id, now);
                if result == Ok(TaskHistoryMutation::Applied) {
                    snapshot_cache = None;
                    ui_snapshot_cache = None;
                    token_decision_cache = None;
                }
                let _ = reply.send(result);
            }
            StoreMessage::Timeline {
                session_id,
                after_ingest_sequence,
                before_ingest_sequence,
                limit,
                latest,
                current_turn_only,
                include_local_tool_target,
                reply,
            } => {
                let _ = reply.send(read_timeline(
                    &connection,
                    &session_id,
                    TimelineReadOptions {
                        after_ingest_sequence,
                        before_ingest_sequence,
                        limit,
                        latest,
                        current_turn_only,
                        include_local_tool_target,
                    },
                ));
            }
            StoreMessage::LocalReviewContext { session_id, reply } => {
                let _ = reply.send(read_local_review_context(&connection, &session_id));
            }
            StoreMessage::PendingReviewBaselines { limit, reply } => {
                let _ = reply.send(read_pending_review_baselines(&connection, limit));
            }
            StoreMessage::WriteReviewBaseline { baseline, reply } => {
                let _ = reply.send(write_review_baseline(&connection, &baseline));
            }
            StoreMessage::ReviewBaseline { turn_id, reply } => {
                let _ = reply.send(read_review_baseline(&connection, &turn_id));
            }
            StoreMessage::TokenUsageDecision {
                now,
                threshold_tokens_per_minute,
                reply,
            } => {
                let cache_is_fresh =
                    token_decision_cache
                        .as_ref()
                        .is_some_and(|(built_at, cached_threshold, _)| {
                            *cached_threshold == threshold_tokens_per_minute
                                && now.saturating_sub(*built_at) < SNAPSHOT_CACHE_MAX_AGE_MS
                        });
                let result = if cache_is_fresh {
                    Ok(token_decision_cache
                        .as_ref()
                        .map(|(_, _, decision)| decision.clone())
                        .unwrap_or_default())
                } else {
                    read_token_usage_decision(&connection, now, threshold_tokens_per_minute)
                        .inspect(|decision| {
                            token_decision_cache =
                                Some((now, threshold_tokens_per_minute, decision.clone()));
                        })
                };
                let _ = reply.send(result);
            }
            StoreMessage::CreateTaskCheckpoint { input, reply } => {
                let _ = reply.send(create_task_checkpoint(&connection, &input));
            }
            StoreMessage::ListTaskCheckpoints { session_id, reply } => {
                let _ = reply.send(read_task_checkpoints(&connection, &session_id));
            }
            StoreMessage::TaskCheckpoint {
                checkpoint_id,
                reply,
            } => {
                let _ = reply.send(read_task_checkpoint(&connection, &checkpoint_id));
            }
            StoreMessage::DeleteTaskCheckpoint {
                checkpoint_id,
                reply,
            } => {
                let _ = reply.send(delete_task_checkpoint(&connection, &checkpoint_id));
            }
            StoreMessage::UiSnapshot { cutoff, reply } => {
                let now = now_millis();
                let cache_is_fresh =
                    ui_snapshot_cache
                        .as_ref()
                        .is_some_and(|(built_at, cached_cutoff, _)| {
                            cutoff >= *cached_cutoff
                                && now.saturating_sub(*built_at) < SNAPSHOT_CACHE_MAX_AGE_MS
                        });
                let result = if cache_is_fresh {
                    let mut snapshot = ui_snapshot_cache
                        .as_ref()
                        .map(|(_, _, snapshot)| snapshot.clone())
                        .unwrap_or_default();
                    filter_ui_snapshot_for_cutoff(&mut snapshot, cutoff);
                    Ok(snapshot)
                } else {
                    refresh_time_driven_attention(&mut connection, now).and_then(|changed| {
                        if changed > 0 {
                            snapshot_cache = None;
                            ui_snapshot_cache = None;
                        }
                        if now.saturating_sub(last_provider_title_refresh_at)
                            >= PROVIDER_TITLE_REFRESH_INTERVAL_MS
                        {
                            refresh_provider_titles(&mut connection, now)?;
                            last_provider_title_refresh_at = now;
                        }
                        let snapshot = read_ui_snapshot(&connection, cutoff)?;
                        ui_snapshot_cache = Some((now, cutoff, snapshot.clone()));
                        Ok(snapshot)
                    })
                };
                let _ = reply.send(result);
            }
            StoreMessage::UiSnapshotWithQueryCount { cutoff, reply } => {
                let now = now_millis();
                let cache_is_fresh =
                    ui_snapshot_cache
                        .as_ref()
                        .is_some_and(|(built_at, cached_cutoff, _)| {
                            cutoff >= *cached_cutoff
                                && now.saturating_sub(*built_at) < SNAPSHOT_CACHE_MAX_AGE_MS
                        });
                let result = if cache_is_fresh {
                    let mut snapshot = ui_snapshot_cache
                        .as_ref()
                        .map(|(_, _, snapshot)| snapshot.clone())
                        .unwrap_or_default();
                    filter_ui_snapshot_for_cutoff(&mut snapshot, cutoff);
                    Ok((snapshot, 0))
                } else {
                    refresh_time_driven_attention(&mut connection, now).and_then(|changed| {
                        if changed > 0 {
                            snapshot_cache = None;
                            ui_snapshot_cache = None;
                        }
                        if now.saturating_sub(last_provider_title_refresh_at)
                            >= PROVIDER_TITLE_REFRESH_INTERVAL_MS
                        {
                            refresh_provider_titles(&mut connection, now)?;
                            last_provider_title_refresh_at = now;
                        }
                        let query_trace = UiSnapshotQueryTrace::install(&connection);
                        let snapshot = read_ui_snapshot(&connection, cutoff);
                        let query_count = query_trace.query_count();
                        drop(query_trace);
                        snapshot.map(|snapshot| {
                            ui_snapshot_cache = Some((now, cutoff, snapshot.clone()));
                            (snapshot, query_count)
                        })
                    })
                };
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
            StoreMessage::WriteUiSettings { value, now, reply } => {
                let result = write_ui_settings_transaction(&mut connection, &value, now).map(
                    |next_hide_policy| {
                        completion_hide_policy = next_hide_policy;
                    },
                );
                let _ = reply.send(result);
            }
            StoreMessage::ReplaceQuota { entries, reply } => {
                let _ = reply.send(replace_quota_transaction(&mut connection, entries));
            }
            StoreMessage::UpsertSessionUsage { records, reply } => {
                let result = upsert_session_usages(&mut connection, records);
                if result.is_ok() {
                    latest_usage_records = None;
                    token_decision_cache = None;
                }
                let _ = reply.send(result);
            }
            StoreMessage::BeginUsageCollectionGeneration { reply } => {
                let result = latest_usage_collection_generation
                    .checked_add(1)
                    .ok_or_else(|| StoreError::Storage("usage generation overflow".to_owned()))
                    .inspect(|generation| latest_usage_collection_generation = *generation);
                let _ = reply.send(result);
            }
            StoreMessage::ReplaceSessionUsage {
                mut records,
                collected_at,
                generation,
                reply,
            } => {
                records.sort_by(|left, right| {
                    left.provider
                        .cmp(&right.provider)
                        .then_with(|| left.provider_session_id.cmp(&right.provider_session_id))
                        .then_with(|| left.model.cmp(&right.model))
                });
                let unchanged = latest_usage_records
                    .as_ref()
                    .is_some_and(|previous| previous == &records);
                let result = if generation != latest_usage_collection_generation {
                    Ok(false)
                } else if unchanged {
                    // Collection still polls at the existing one-second cadence,
                    // but an unchanged scan records at most one live rate sample
                    // per 30 seconds and never invalidates the main snapshot.
                    refresh_unchanged_usage_rate_samples(&mut connection, &records, collected_at)
                        .map(|changed| {
                            if changed {
                                token_decision_cache = None;
                            }
                            true
                        })
                } else {
                    let retained = records.clone();
                    replace_session_usages(&mut connection, records, collected_at).map(|_| {
                        latest_usage_records = Some(retained);
                        snapshot_cache = None;
                        ui_snapshot_cache = None;
                        token_decision_cache = None;
                        true
                    })
                };
                let _ = reply.send(result);
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
            StoreMessage::ExportTokenUsageJson { now, reply } => {
                let _ = reply.send(export_token_usage_json_database(&connection, now));
            }
            StoreMessage::ExportTokenUsageCsv { now, reply } => {
                let _ = reply.send(export_token_usage_csv_database(&connection, now));
            }
            StoreMessage::ClearData { reply } => {
                let result = reset_database(&mut connection, &database_path);
                if result.is_ok() {
                    latest_usage_records = None;
                    token_decision_cache = None;
                }
                let _ = reply.send(result);
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
    let mut canonical_updated = false;
    for record in records {
        if is_ignored_provider_session(&transaction, &record.provider, &record.provider_session_id)?
        {
            continue;
        }
        canonical_updated |= !record.daily_usage.is_empty();
        let _ = record_usage_rate_sample(&transaction, &record, record.captured_at)?;
        record_token_usage_delta(&transaction, &record)?;
        archive_session_daily_usage(&transaction, &record)?;
        legacy_upsert_session_usage_row(&transaction, record)?;
    }
    if canonical_updated {
        reconcile_legacy_token_aggregates(&transaction)?;
        verify_token_ledger_projection_invariants(&transaction)?;
    }
    prune_usage_rate_samples(&transaction, now_millis())?;
    transaction.commit().map_err(storage_error)
}

fn replace_session_usages(
    connection: &mut Connection,
    records: Vec<SessionUsageRecord>,
    collected_at: u64,
) -> Result<(), StoreError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    // A caught-up collector generation is a complete replacement of the
    // locally observed ledger, even when one source is explicitly partial.
    // Retaining MAX() rows from an older partial generation permanently
    // inflated daily totals and made the daily ledger disagree with the
    // current per-session snapshot.
    if records.iter().any(|record| !record.daily_usage.is_empty()) {
        transaction
            .execute_batch(
                "DELETE FROM token_usage_session_days;
                 DELETE FROM token_usage_cursors;
                 DELETE FROM token_usage_daily;
                 DELETE FROM token_usage_daily_models;",
            )
            .map_err(storage_error)?;
    }
    let mut observed = HashSet::new();
    let mut canonical_updated = false;
    for record in records {
        if is_ignored_provider_session(&transaction, &record.provider, &record.provider_session_id)?
        {
            continue;
        }
        canonical_updated |= !record.daily_usage.is_empty();
        let identity = (record.provider.clone(), record.provider_session_id.clone());
        let _ = record_usage_rate_sample(&transaction, &record, collected_at)?;
        record_token_usage_delta(&transaction, &record)?;
        archive_session_daily_usage(&transaction, &record)?;
        if replace_session_usage_row(&transaction, record)? > 0 {
            observed.insert(identity);
        }
    }
    let mut stale = transaction
        .prepare(
            "SELECT provider, provider_session_id
             FROM session_usage",
        )
        .map_err(storage_error)?;
    let stale_rows = stale
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    drop(stale);
    for (provider, provider_session_id) in stale_rows {
        if observed.contains(&(provider.clone(), provider_session_id.clone())) {
            continue;
        }
        transaction
            .execute(
                "DELETE FROM session_usage
                 WHERE provider = ?1 AND provider_session_id = ?2",
                params![provider, provider_session_id],
            )
            .map_err(storage_error)?;
    }
    if canonical_updated {
        reconcile_legacy_token_aggregates(&transaction)?;
        verify_token_ledger_projection_invariants(&transaction)?;
    }
    prune_usage_rate_samples(&transaction, collected_at)?;
    transaction.commit().map_err(storage_error)
}

fn record_usage_rate_sample(
    connection: &Connection,
    record: &SessionUsageRecord,
    sampled_at: u64,
) -> Result<bool, StoreError> {
    let Some(token_total) = record.token_total else {
        return Ok(false);
    };
    let context = connection
        .query_row(
            "SELECT sessions.id, turns.id, turns.started_at
             FROM sessions
             JOIN turns ON turns.session_id = sessions.id
             WHERE sessions.provider = ?1
               AND sessions.provider_session_id = ?2
               AND sessions.exec_state NOT IN ('idle', 'response_finished', 'failed')
               AND turns.state = 'running'
             ORDER BY turns.ordinal DESC LIMIT 1",
            params![record.provider, record.provider_session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    from_i64(row.get(2)?),
                ))
            },
        )
        .optional()
        .map_err(storage_error)?;
    let Some((session_id, turn_id, turn_started_at)) = context else {
        return Ok(false);
    };
    // Historical collection may discover a large cumulative total while a
    // session exists. It becomes only the baseline sample unless the Provider
    // usage fact itself belongs to the current Turn.
    if record.captured_at < turn_started_at || sampled_at < turn_started_at {
        return Ok(false);
    }
    let previous = connection
        .query_row(
            "SELECT sampled_at, token_total
             FROM token_usage_rate_samples
             WHERE provider = ?1 AND provider_session_id = ?2
             ORDER BY sampled_at DESC LIMIT 1",
            params![record.provider, record.provider_session_id],
            |row| Ok((from_i64(row.get(0)?), from_i64(row.get(1)?))),
        )
        .optional()
        .map_err(storage_error)?;
    if previous.is_some_and(|(previous_at, previous_total)| {
        sampled_at.saturating_sub(previous_at) < 30_000 && previous_total == token_total
    }) {
        return Ok(false);
    }
    connection
        .execute(
            "INSERT OR IGNORE INTO token_usage_rate_samples(
               provider, provider_session_id, session_id, turn_id,
               sampled_at, token_total, usage_source, usage_quality
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                record.provider,
                record.provider_session_id,
                session_id,
                turn_id,
                to_i64(sampled_at),
                to_i64(token_total),
                record.usage_source,
                record.usage_quality,
            ],
        )
        .map(|changed| changed == 1)
        .map_err(storage_error)
}

fn refresh_unchanged_usage_rate_samples(
    connection: &mut Connection,
    records: &[SessionUsageRecord],
    collected_at: u64,
) -> Result<bool, StoreError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    let mut changed = false;
    for record in records {
        changed |= record_usage_rate_sample(&transaction, record, collected_at)?;
    }
    prune_usage_rate_samples(&transaction, collected_at)?;
    transaction.commit().map_err(storage_error)?;
    Ok(changed)
}

fn prune_usage_rate_samples(connection: &Connection, now: u64) -> Result<(), StoreError> {
    const RETENTION_MS: u64 = 6 * 60 * 60 * 1_000;
    connection
        .execute(
            "DELETE FROM token_usage_rate_samples WHERE sampled_at < ?1",
            [to_i64(now.saturating_sub(RETENTION_MS))],
        )
        .map(|_| ())
        .map_err(storage_error)
}

/// `token_usage_session_days` is the canonical, rebuildable ledger. The two
/// older aggregate tables remain for compatibility, but are projections of
/// that ledger so day/provider/model totals cannot drift into three truths.
fn reconcile_legacy_token_aggregates(connection: &Connection) -> Result<(), StoreError> {
    connection
        .execute_batch(
            "DELETE FROM token_usage_daily;
             INSERT INTO token_usage_daily(day, provider, token_total, captured_at)
             SELECT day, provider, SUM(token_total), MAX(captured_at)
             FROM token_usage_session_days
             GROUP BY day, provider;

             DELETE FROM token_usage_daily_models;
             INSERT INTO token_usage_daily_models(
               day, provider, model, token_total, input_tokens, output_tokens,
               cache_read_tokens, cache_creation_tokens, reasoning_tokens,
               estimated_cost_usd_micros, captured_at
             )
             SELECT day, provider, model,
                    SUM(token_total), SUM(input_tokens), SUM(output_tokens),
                    SUM(cache_read_tokens), SUM(cache_creation_tokens),
                    SUM(reasoning_tokens),
                    CASE WHEN COUNT(estimated_cost_usd_micros) = COUNT(*)
                         THEN SUM(estimated_cost_usd_micros) ELSE NULL END,
                    MAX(captured_at)
             FROM token_usage_session_days
             GROUP BY day, provider, model;",
        )
        .map_err(storage_error)
}

/// Verifies the staged canonical ledger against both compatibility projections
/// before the surrounding SQLite transaction commits. The collector's
/// generation-scoped in-memory result is the shadow state; readers keep seeing
/// the previous committed generation until this check and the transaction both
/// succeed.
fn verify_token_ledger_projection_invariants(connection: &Connection) -> Result<(), StoreError> {
    let daily_mismatch: bool = connection
        .query_row(
            "WITH canonical AS (
               SELECT day, provider, SUM(token_total) AS token_total,
                      MAX(captured_at) AS captured_at
               FROM token_usage_session_days
               GROUP BY day, provider
             ), mismatch AS (
               SELECT 1
               FROM canonical
               LEFT JOIN token_usage_daily AS projection
                 ON projection.day = canonical.day
                AND projection.provider = canonical.provider
               WHERE projection.day IS NULL
                  OR projection.token_total <> canonical.token_total
                  OR projection.captured_at <> canonical.captured_at
               UNION ALL
               SELECT 1
               FROM token_usage_daily AS projection
               LEFT JOIN canonical
                 ON canonical.day = projection.day
                AND canonical.provider = projection.provider
               WHERE canonical.day IS NULL
             )
             SELECT EXISTS(SELECT 1 FROM mismatch)",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let model_mismatch: bool = connection
        .query_row(
            "WITH canonical AS (
               SELECT day, provider, model,
                      SUM(token_total) AS token_total,
                      SUM(input_tokens) AS input_tokens,
                      SUM(output_tokens) AS output_tokens,
                      SUM(cache_read_tokens) AS cache_read_tokens,
                      SUM(cache_creation_tokens) AS cache_creation_tokens,
                      SUM(reasoning_tokens) AS reasoning_tokens,
                      CASE WHEN COUNT(estimated_cost_usd_micros) = COUNT(*)
                           THEN SUM(estimated_cost_usd_micros) ELSE NULL END AS cost,
                      MAX(captured_at) AS captured_at
               FROM token_usage_session_days
               GROUP BY day, provider, model
             ), mismatch AS (
               SELECT 1
               FROM canonical
               LEFT JOIN token_usage_daily_models AS projection
                 ON projection.day = canonical.day
                AND projection.provider = canonical.provider
                AND projection.model = canonical.model
               WHERE projection.day IS NULL
                  OR projection.token_total <> canonical.token_total
                  OR projection.input_tokens <> canonical.input_tokens
                  OR projection.output_tokens <> canonical.output_tokens
                  OR projection.cache_read_tokens <> canonical.cache_read_tokens
                  OR projection.cache_creation_tokens <> canonical.cache_creation_tokens
                  OR projection.reasoning_tokens <> canonical.reasoning_tokens
                  OR projection.estimated_cost_usd_micros IS NOT canonical.cost
                  OR projection.captured_at <> canonical.captured_at
               UNION ALL
               SELECT 1
               FROM token_usage_daily_models AS projection
               LEFT JOIN canonical
                 ON canonical.day = projection.day
                AND canonical.provider = projection.provider
                AND canonical.model = projection.model
               WHERE canonical.day IS NULL
             )
             SELECT EXISTS(SELECT 1 FROM mismatch)",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if daily_mismatch || model_mismatch {
        return Err(StoreError::Storage(
            "token ledger projection invariant failed".to_owned(),
        ));
    }
    Ok(())
}

fn archive_session_daily_usage(
    connection: &Connection,
    record: &SessionUsageRecord,
) -> Result<(), StoreError> {
    let complete_history = matches!(
        record.usage_quality.as_str(),
        "official_local" | "derived" | "verified" | "complete"
    );
    if !record.daily_usage.is_empty() {
        let desired = record.daily_usage.iter().fold(
            HashMap::<&str, HashSet<&str>>::new(),
            |mut days, usage| {
                days.entry(usage.day.as_str())
                    .or_default()
                    .insert(usage.model.as_deref().unwrap_or("Unknown"));
                days
            },
        );
        let stale_models = connection
            .prepare(
                "SELECT day, model FROM token_usage_session_days
                 WHERE provider = ?1 AND provider_session_id = ?2",
            )
            .map_err(storage_error)?
            .query_map(
                params![record.provider, record.provider_session_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        for (day, model) in stale_models {
            let still_present = desired
                .get(day.as_str())
                .is_some_and(|models| models.contains(model.as_str()));
            if still_present || (!complete_history && !desired.contains_key(day.as_str())) {
                continue;
            }
            connection
                .execute(
                    "DELETE FROM token_usage_session_days
                     WHERE provider = ?1 AND provider_session_id = ?2
                       AND day = ?3 AND model = ?4",
                    params![record.provider, record.provider_session_id, day, model],
                )
                .map_err(storage_error)?;
        }
    }
    for day in &record.daily_usage {
        if !valid_metric_day(&day.day) {
            return Err(StoreError::Storage(
                "usage history day is invalid".to_owned(),
            ));
        }
        let model = day.model.as_deref().unwrap_or("Unknown");
        if model.trim().is_empty() || model.len() > 128 || model.chars().any(char::is_control) {
            return Err(StoreError::Storage(
                "usage history model is invalid".to_owned(),
            ));
        }
        for value in [day.cost_kind.as_deref(), day.pricing_source.as_deref()]
            .into_iter()
            .flatten()
        {
            if value.trim().is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
                return Err(StoreError::Storage(
                    "usage history pricing metadata is invalid".to_owned(),
                ));
            }
        }
        let existing = connection
            .query_row(
                "SELECT input_tokens, output_tokens, cache_read_tokens,
                        cache_creation_tokens, reasoning_tokens, token_total,
                        estimated_cost_usd_micros, cost_kind, pricing_source
                 FROM token_usage_session_days
                 WHERE provider = ?1 AND provider_session_id = ?2
                   AND day = ?3 AND model = ?4",
                params![record.provider, record.provider_session_id, day.day, model],
                |row| {
                    Ok(ArchivedDailyUsage {
                        input_tokens: from_i64(row.get(0)?),
                        output_tokens: from_i64(row.get(1)?),
                        cache_read_tokens: from_i64(row.get(2)?),
                        cache_creation_tokens: from_i64(row.get(3)?),
                        reasoning_tokens: from_i64(row.get(4)?),
                        token_total: from_i64(row.get(5)?),
                        estimated_cost_usd_micros: row.get::<_, Option<i64>>(6)?.map(from_i64),
                        cost_kind: row.get(7)?,
                        pricing_source: row.get(8)?,
                    })
                },
            )
            .optional()
            .map_err(storage_error)?;
        let same_facts = existing
            .as_ref()
            .is_some_and(|existing| existing.same_token_facts(day));
        let incoming_is_provider_cost = day
            .cost_kind
            .as_deref()
            .is_some_and(is_provider_reported_cost);
        let preserve_existing_cost = existing
            .as_ref()
            .is_some_and(|existing| existing.estimated_cost_usd_micros.is_some())
            && (!complete_history || (same_facts && !incoming_is_provider_cost));
        let (estimated_cost_usd_micros, cost_kind, pricing_source) = if preserve_existing_cost {
            let existing = existing.as_ref().expect("checked above");
            (
                existing.estimated_cost_usd_micros,
                existing.cost_kind.clone(),
                existing.pricing_source.clone(),
            )
        } else {
            (
                day.estimated_cost_usd_micros,
                day.estimated_cost_usd_micros.map(|_| {
                    day.cost_kind
                        .clone()
                        .unwrap_or_else(|| "unclassified".to_owned())
                }),
                day.estimated_cost_usd_micros.map(|_| {
                    day.pricing_source
                        .clone()
                        .unwrap_or_else(|| "unspecified".to_owned())
                }),
            )
        };
        connection
            .execute(
                "INSERT INTO token_usage_session_days(
                   provider, provider_session_id, day, model,
                   input_tokens, output_tokens, cache_read_tokens,
                   cache_creation_tokens, reasoning_tokens, token_total,
                   estimated_cost_usd_micros, cost_kind, pricing_source,
                   message_count, captured_at
                 ) VALUES (
                   ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                   ?14, ?15
                 )
                 ON CONFLICT(provider, provider_session_id, day, model) DO UPDATE SET
                   input_tokens = CASE WHEN ?16 THEN excluded.input_tokens
                     ELSE MAX(input_tokens, excluded.input_tokens) END,
                   output_tokens = CASE WHEN ?16 THEN excluded.output_tokens
                     ELSE MAX(output_tokens, excluded.output_tokens) END,
                   cache_read_tokens = CASE WHEN ?16 THEN excluded.cache_read_tokens
                     ELSE MAX(cache_read_tokens, excluded.cache_read_tokens) END,
                   cache_creation_tokens = CASE WHEN ?16
                     THEN excluded.cache_creation_tokens ELSE MAX(
                       cache_creation_tokens, excluded.cache_creation_tokens) END,
                   reasoning_tokens = CASE WHEN ?16 THEN excluded.reasoning_tokens
                     ELSE MAX(reasoning_tokens, excluded.reasoning_tokens) END,
                   token_total = CASE WHEN ?16 THEN excluded.token_total
                     ELSE MAX(token_total, excluded.token_total) END,
                   estimated_cost_usd_micros = excluded.estimated_cost_usd_micros,
                   cost_kind = excluded.cost_kind,
                   pricing_source = excluded.pricing_source,
                   message_count = CASE WHEN ?16 THEN excluded.message_count
                     ELSE MAX(message_count, excluded.message_count) END,
                   captured_at = MAX(captured_at, excluded.captured_at)",
                params![
                    record.provider,
                    record.provider_session_id,
                    day.day,
                    model,
                    to_i64(day.input_tokens),
                    to_i64(day.output_tokens),
                    to_i64(day.cache_read_tokens),
                    to_i64(day.cache_creation_tokens),
                    to_i64(day.reasoning_tokens),
                    to_i64(day.token_total),
                    estimated_cost_usd_micros.map(to_i64),
                    cost_kind,
                    pricing_source,
                    to_i64(day.message_count),
                    to_i64(record.captured_at),
                    complete_history,
                ],
            )
            .map_err(storage_error)?;
    }
    Ok(())
}

#[derive(Debug)]
struct ArchivedDailyUsage {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
    reasoning_tokens: u64,
    token_total: u64,
    estimated_cost_usd_micros: Option<u64>,
    cost_kind: Option<String>,
    pricing_source: Option<String>,
}

impl ArchivedDailyUsage {
    fn same_token_facts(&self, incoming: &SessionUsageDailyRecord) -> bool {
        self.input_tokens == incoming.input_tokens
            && self.output_tokens == incoming.output_tokens
            && self.cache_read_tokens == incoming.cache_read_tokens
            && self.cache_creation_tokens == incoming.cache_creation_tokens
            && self.reasoning_tokens == incoming.reasoning_tokens
            && self.token_total == incoming.token_total
    }
}

fn is_provider_reported_cost(kind: &str) -> bool {
    matches!(kind, "provider_estimate" | "provider_reported" | "official")
}

fn valid_metric_day(day: &str) -> bool {
    day.len() == 10
        && day.as_bytes().get(4) == Some(&b'-')
        && day.as_bytes().get(7) == Some(&b'-')
        && day
            .bytes()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

/// Archives monotonic per-session token deltas before the replaceable live
/// usage projection is updated. The cursor survives source pruning and Runtime
/// restarts, so repeatedly observing the same cumulative session snapshot does
/// not inflate day/month/total usage. A lower partial snapshot is ignored until
/// it catches up instead of being mistaken for a new token counter generation.
#[derive(Default)]
struct TokenUsageCursorSnapshot {
    token_total: u64,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    cache_creation_tokens: Option<u64>,
    reasoning_tokens: Option<u64>,
    estimated_cost_usd_micros: Option<u64>,
}

fn validate_usage_attribution(record: &SessionUsageRecord) -> Result<(), StoreError> {
    match (&record.project_id, &record.project_label) {
        (Some(project_id), Some(project_label))
            if project_id.len() == 71
                && project_id.starts_with("sha256:")
                && project_id[7..]
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                && !project_label.trim().is_empty()
                && project_label.len() <= 128
                && !project_label.chars().any(char::is_control)
                && !project_label.contains('/')
                && !project_label.contains('\\') => {}
        (None, None) => {}
        _ => {
            return Err(StoreError::Storage(
                "usage project attribution metadata is invalid".to_owned(),
            ));
        }
    }
    if record
        .parent_provider_session_id
        .as_ref()
        .is_some_and(|value| {
            value.trim().is_empty()
                || value.len() > 128
                || !value.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
                })
        })
    {
        return Err(StoreError::Storage(
            "usage parent session identity is invalid".to_owned(),
        ));
    }
    Ok(())
}

fn record_token_usage_delta(
    connection: &Connection,
    record: &SessionUsageRecord,
) -> Result<(), StoreError> {
    validate_usage_attribution(record)?;
    let Some(current_total) = record.token_total else {
        return Ok(());
    };
    let previous = connection
        .query_row(
            "SELECT token_total, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, reasoning_tokens,
                    estimated_cost_usd_micros
             FROM token_usage_cursors
             WHERE provider = ?1 AND provider_session_id = ?2",
            params![record.provider, record.provider_session_id],
            |row| {
                Ok(TokenUsageCursorSnapshot {
                    token_total: from_i64(row.get(0)?),
                    input_tokens: row.get::<_, Option<i64>>(1)?.map(from_i64),
                    output_tokens: row.get::<_, Option<i64>>(2)?.map(from_i64),
                    cache_read_tokens: row.get::<_, Option<i64>>(3)?.map(from_i64),
                    cache_creation_tokens: row.get::<_, Option<i64>>(4)?.map(from_i64),
                    reasoning_tokens: row.get::<_, Option<i64>>(5)?.map(from_i64),
                    estimated_cost_usd_micros: row.get::<_, Option<i64>>(6)?.map(from_i64),
                })
            },
        )
        .optional()
        .map_err(storage_error)?;
    let existing_cursor = previous.is_some();
    let previous = previous.unwrap_or_default();
    let previous_total = previous.token_total;
    let high_watermark = previous_total.max(current_total);
    let delta = high_watermark.saturating_sub(previous_total);
    let input_delta =
        usage_component_delta(record.input_tokens, previous.input_tokens, existing_cursor);
    let output_delta = usage_component_delta(
        record.output_tokens,
        previous.output_tokens,
        existing_cursor,
    );
    let cache_read_delta = usage_component_delta(
        record.cache_read_tokens,
        previous.cache_read_tokens,
        existing_cursor,
    );
    let cache_creation_delta = usage_component_delta(
        record.cache_creation_tokens,
        previous.cache_creation_tokens,
        existing_cursor,
    );
    let reasoning_delta = usage_component_delta(
        record.reasoning_tokens,
        previous.reasoning_tokens,
        existing_cursor,
    );
    let cost_delta = usage_component_delta(
        record.estimated_cost_usd_micros,
        previous.estimated_cost_usd_micros,
        existing_cursor,
    );
    connection
        .execute(
            "INSERT INTO token_usage_cursors(
               provider, provider_session_id,
               project_id, project_label, parent_provider_session_id,
               token_total, model,
               input_tokens, output_tokens, cache_read_tokens,
               cache_creation_tokens, reasoning_tokens,
               estimated_cost_usd_micros, captured_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(provider, provider_session_id) DO UPDATE SET
               project_id = COALESCE(excluded.project_id, project_id),
               project_label = COALESCE(excluded.project_label, project_label),
               parent_provider_session_id = COALESCE(
                 excluded.parent_provider_session_id, parent_provider_session_id),
               token_total = MAX(token_total, excluded.token_total),
               model = COALESCE(excluded.model, model),
               input_tokens = COALESCE(MAX(input_tokens, excluded.input_tokens),
                                       input_tokens, excluded.input_tokens),
               output_tokens = COALESCE(MAX(output_tokens, excluded.output_tokens),
                                        output_tokens, excluded.output_tokens),
               cache_read_tokens = COALESCE(
                 MAX(cache_read_tokens, excluded.cache_read_tokens),
                 cache_read_tokens, excluded.cache_read_tokens),
               cache_creation_tokens = COALESCE(
                 MAX(cache_creation_tokens, excluded.cache_creation_tokens),
                 cache_creation_tokens, excluded.cache_creation_tokens),
               reasoning_tokens = COALESCE(
                 MAX(reasoning_tokens, excluded.reasoning_tokens),
                 reasoning_tokens, excluded.reasoning_tokens),
               estimated_cost_usd_micros = COALESCE(
                 MAX(estimated_cost_usd_micros, excluded.estimated_cost_usd_micros),
                 estimated_cost_usd_micros, excluded.estimated_cost_usd_micros),
               captured_at = MAX(captured_at, excluded.captured_at)",
            params![
                record.provider,
                record.provider_session_id,
                record.project_id,
                record.project_label,
                record.parent_provider_session_id,
                to_i64(high_watermark),
                record.model,
                record.input_tokens.map(to_i64),
                record.output_tokens.map(to_i64),
                record.cache_read_tokens.map(to_i64),
                record.cache_creation_tokens.map(to_i64),
                record.reasoning_tokens.map(to_i64),
                record.estimated_cost_usd_micros.map(to_i64),
                to_i64(record.captured_at),
            ],
        )
        .map_err(storage_error)?;
    if delta == 0 {
        return Ok(());
    }
    connection
        .execute(
            "INSERT INTO token_usage_daily(day, provider, token_total, captured_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(day, provider) DO UPDATE SET
               token_total = MIN(9223372036854775807,
                                 token_usage_daily.token_total + excluded.token_total),
               captured_at = MAX(token_usage_daily.captured_at, excluded.captured_at)",
            params![
                metric_day(record.captured_at),
                record.provider,
                to_i64(delta),
                to_i64(record.captured_at),
            ],
        )
        .map(|_| ())
        .map_err(storage_error)?;
    let model = record.model.as_deref().unwrap_or("Unknown");
    connection
        .execute(
            "INSERT INTO token_usage_daily_models(
               day, provider, model, token_total, input_tokens, output_tokens,
               cache_read_tokens, cache_creation_tokens, reasoning_tokens,
               estimated_cost_usd_micros, captured_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(day, provider, model) DO UPDATE SET
               token_total = MIN(9223372036854775807,
                                 token_usage_daily_models.token_total + excluded.token_total),
               input_tokens = COALESCE(
                 MIN(9223372036854775807,
                     token_usage_daily_models.input_tokens + excluded.input_tokens),
                 token_usage_daily_models.input_tokens, excluded.input_tokens),
               output_tokens = COALESCE(
                 MIN(9223372036854775807,
                     token_usage_daily_models.output_tokens + excluded.output_tokens),
                 token_usage_daily_models.output_tokens, excluded.output_tokens),
               cache_read_tokens = COALESCE(
                 MIN(9223372036854775807,
                     token_usage_daily_models.cache_read_tokens + excluded.cache_read_tokens),
                 token_usage_daily_models.cache_read_tokens, excluded.cache_read_tokens),
               cache_creation_tokens = COALESCE(
                 MIN(9223372036854775807,
                     token_usage_daily_models.cache_creation_tokens
                       + excluded.cache_creation_tokens),
                 token_usage_daily_models.cache_creation_tokens,
                 excluded.cache_creation_tokens),
               reasoning_tokens = COALESCE(
                 MIN(9223372036854775807,
                     token_usage_daily_models.reasoning_tokens + excluded.reasoning_tokens),
                 token_usage_daily_models.reasoning_tokens, excluded.reasoning_tokens),
               estimated_cost_usd_micros = COALESCE(
                 MIN(9223372036854775807,
                     token_usage_daily_models.estimated_cost_usd_micros
                       + excluded.estimated_cost_usd_micros),
                 token_usage_daily_models.estimated_cost_usd_micros,
                 excluded.estimated_cost_usd_micros),
               captured_at = MAX(token_usage_daily_models.captured_at,
                                 excluded.captured_at)",
            params![
                metric_day(record.captured_at),
                record.provider,
                model,
                to_i64(delta),
                input_delta.map(to_i64),
                output_delta.map(to_i64),
                cache_read_delta.map(to_i64),
                cache_creation_delta.map(to_i64),
                reasoning_delta.map(to_i64),
                cost_delta.map(to_i64),
                to_i64(record.captured_at),
            ],
        )
        .map(|_| ())
        .map_err(storage_error)
}

fn usage_component_delta(
    current: Option<u64>,
    previous: Option<u64>,
    existing_cursor: bool,
) -> Option<u64> {
    let current = current?;
    if !existing_cursor {
        return Some(current);
    }
    previous.map(|previous| current.max(previous).saturating_sub(previous))
}

fn legacy_upsert_session_usage_row(
    connection: &Connection,
    record: SessionUsageRecord,
) -> Result<(), StoreError> {
    if record.provider_session_id.trim().is_empty() || record.provider_session_id.len() > 128 {
        return Err(StoreError::Storage(
            "usage session identifier is invalid".to_owned(),
        ));
    }
    if record.model.as_ref().is_some_and(|model| {
        model.trim().is_empty() || model.len() > 128 || model.chars().any(char::is_control)
    }) {
        return Err(StoreError::Storage(
            "usage model identifier is invalid".to_owned(),
        ));
    }
    connection
        .execute(
            "INSERT INTO session_usage(
               provider, provider_session_id, model,
               input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
               reasoning_tokens, token_total, last_turn_tokens, context_used_tokens,
               context_window_tokens, context_used_percent, estimated_cost_usd_micros,
               cost_kind, pricing_source, usage_source, usage_quality, captured_at
             ) VALUES (
               ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
               ?14, ?15, ?16, ?17, ?18, ?19
             )
             ON CONFLICT(provider, provider_session_id) DO UPDATE SET
               model = COALESCE(excluded.model, model),
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
                record.model,
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

fn replace_session_usage_row(
    connection: &Connection,
    record: SessionUsageRecord,
) -> Result<usize, StoreError> {
    if record.provider_session_id.trim().is_empty() || record.provider_session_id.len() > 128 {
        return Err(StoreError::Storage(
            "usage session identifier is invalid".to_owned(),
        ));
    }
    if record.model.as_ref().is_some_and(|model| {
        model.trim().is_empty() || model.len() > 128 || model.chars().any(char::is_control)
    }) {
        return Err(StoreError::Storage(
            "usage model identifier is invalid".to_owned(),
        ));
    }
    connection
        .execute(
            "INSERT INTO session_usage(
               provider, provider_session_id, model,
               input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
               reasoning_tokens, token_total, last_turn_tokens, context_used_tokens,
               context_window_tokens, context_used_percent, estimated_cost_usd_micros,
               cost_kind, pricing_source, usage_source, usage_quality, captured_at
             ) SELECT
               ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
               ?14, ?15, ?16, ?17, ?18, ?19
             WHERE EXISTS (
                SELECT 1 FROM sessions
                WHERE provider = ?1 AND provider_session_id = ?2
             )
             ON CONFLICT(provider, provider_session_id) DO UPDATE SET
               model = excluded.model,
               input_tokens = excluded.input_tokens,
               output_tokens = excluded.output_tokens,
               cache_read_tokens = excluded.cache_read_tokens,
               cache_creation_tokens = excluded.cache_creation_tokens,
               reasoning_tokens = excluded.reasoning_tokens,
               token_total = excluded.token_total,
               last_turn_tokens = excluded.last_turn_tokens,
               context_used_tokens = excluded.context_used_tokens,
               context_window_tokens = excluded.context_window_tokens,
               context_used_percent = excluded.context_used_percent,
               estimated_cost_usd_micros = excluded.estimated_cost_usd_micros,
               cost_kind = excluded.cost_kind,
               pricing_source = excluded.pricing_source,
               usage_source = excluded.usage_source,
               usage_quality = excluded.usage_quality,
               captured_at = excluded.captured_at",
            params![
                record.provider,
                record.provider_session_id,
                record.model,
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
                   provider, window, limit_id, used_pct, resets_at, source, captured_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    entry.provider,
                    entry.window,
                    entry.limit_id.unwrap_or_default(),
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

fn local_period_start(now: u64, first_day_of_month: bool) -> u64 {
    let seconds: libc::time_t = (now / 1_000).try_into().unwrap_or(libc::time_t::MAX);
    let mut local = std::mem::MaybeUninit::<libc::tm>::zeroed();
    // SAFETY: local points to writable tm storage and seconds remains alive for the call.
    let result = unsafe { libc::localtime_r(&seconds, local.as_mut_ptr()) };
    if result.is_null() {
        return 0;
    }
    // SAFETY: localtime_r returned non-null and initialized the tm value.
    let mut local = unsafe { local.assume_init() };
    local.tm_hour = 0;
    local.tm_min = 0;
    local.tm_sec = 0;
    local.tm_isdst = -1;
    if first_day_of_month {
        local.tm_mday = 1;
    }
    // SAFETY: local is an initialized tm value and mktime accepts a mutable pointer to it.
    let start = unsafe { libc::mktime(&mut local) };
    if start < 0 {
        return 0;
    }
    u64::try_from(start)
        .unwrap_or_default()
        .saturating_mul(1_000)
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
    let closed_expired_sessions = "
        SELECT sessions.id, sessions.provider, sessions.provider_session_id
        FROM sessions
        WHERE sessions.last_event_at < ?1
          AND sessions.exec_state IN ('idle', 'response_finished', 'failed')
          AND NOT EXISTS (
              SELECT 1 FROM attention_items
              WHERE attention_items.session_id = sessions.id
                AND attention_items.state IN ('open', 'committing', 'decision_sent', 'snoozed')
          )";
    let transaction = connection.transaction().map_err(storage_error)?;
    let pruned_sessions = transaction
        .query_row(
            &format!("SELECT COUNT(*) FROM ({closed_expired_sessions})"),
            [to_i64(cutoff)],
            |row| row.get::<_, i64>(0),
        )
        .map_err(storage_error)?;
    for statement in [
        format!(
            "DELETE FROM commands
             WHERE attention_id IN (
                 SELECT id FROM attention_items
                 WHERE session_id IN (SELECT id FROM ({closed_expired_sessions}))
             )"
        ),
        format!(
            "DELETE FROM attention_items
             WHERE session_id IN (SELECT id FROM ({closed_expired_sessions}))"
        ),
        format!(
            "DELETE FROM session_usage
             WHERE EXISTS (
                 SELECT 1 FROM ({closed_expired_sessions}) AS expired
                 WHERE expired.provider = session_usage.provider
                   AND expired.provider_session_id = session_usage.provider_session_id
             )"
        ),
        format!("DELETE FROM events WHERE session_id IN (SELECT id FROM ({closed_expired_sessions}))"),
        format!(
            "DELETE FROM session_tasks WHERE session_id IN (SELECT id FROM ({closed_expired_sessions}))"
        ),
        format!(
            "DELETE FROM session_plan_steps
             WHERE session_id IN (SELECT id FROM ({closed_expired_sessions}))"
        ),
        format!(
            "DELETE FROM session_subagents
             WHERE session_id IN (SELECT id FROM ({closed_expired_sessions}))"
        ),
        format!(
            "DELETE FROM agent_execution_intervals
             WHERE session_id IN (SELECT id FROM ({closed_expired_sessions}))"
        ),
        format!("DELETE FROM turns WHERE session_id IN (SELECT id FROM ({closed_expired_sessions}))"),
        format!("DELETE FROM sessions WHERE id IN (SELECT id FROM ({closed_expired_sessions}))"),
    ] {
        transaction
            .execute(&statement, [to_i64(cutoff)])
            .map_err(storage_error)?;
    }
    transaction.commit().map_err(storage_error)?;
    if pruned_sessions > 0 {
        // Retention is triggered only at Runtime startup or an authenticated settings update;
        // never from the snapshot/WebSocket loop. SQLite requires this boundary after commit.
        connection.execute_batch("VACUUM").map_err(storage_error)?;
    }
    Ok(usize::try_from(pruned_sessions).unwrap_or(usize::MAX))
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
        let exported_table = table.clone();
        let rows = statement
            .query_map([], move |row| {
                let mut object = Map::new();
                for (index, column) in columns.iter().enumerate() {
                    let private_locator = exported_table == "task_checkpoints"
                        && matches!(column.as_str(), "repository_root" | "repository_identity");
                    let value = if private_locator {
                        Value::String("<redacted>".to_owned())
                    } else {
                        sqlite_value(row.get_ref(index)?)
                    };
                    object.insert(column.clone(), value);
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

#[derive(Debug, Clone)]
struct TokenUsageNumericRow {
    day: String,
    provider: String,
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
    reasoning_tokens: u64,
    unclassified_tokens: u64,
    token_total: u64,
    priced_tokens: u64,
    unpriced_tokens: u64,
    estimated_cost_usd_micros: Option<u64>,
    cost_kind: Option<String>,
    pricing_source: Option<String>,
    message_count: u64,
}

#[derive(Debug, Default)]
struct TokenUsageNumericTotals {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
    reasoning_tokens: u64,
    unclassified_tokens: u64,
    token_total: u64,
    priced_tokens: u64,
    unpriced_tokens: u64,
    estimated_cost_usd_micros: u64,
    message_count: u64,
}

impl TokenUsageNumericTotals {
    fn add(&mut self, row: &TokenUsageNumericRow) {
        self.input_tokens = self.input_tokens.saturating_add(row.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(row.output_tokens);
        self.cache_read_tokens = self.cache_read_tokens.saturating_add(row.cache_read_tokens);
        self.cache_creation_tokens = self
            .cache_creation_tokens
            .saturating_add(row.cache_creation_tokens);
        self.reasoning_tokens = self.reasoning_tokens.saturating_add(row.reasoning_tokens);
        self.unclassified_tokens = self
            .unclassified_tokens
            .saturating_add(row.unclassified_tokens);
        self.token_total = self.token_total.saturating_add(row.token_total);
        self.priced_tokens = self.priced_tokens.saturating_add(row.priced_tokens);
        self.unpriced_tokens = self.unpriced_tokens.saturating_add(row.unpriced_tokens);
        self.estimated_cost_usd_micros = self
            .estimated_cost_usd_micros
            .saturating_add(row.estimated_cost_usd_micros.unwrap_or_default());
        self.message_count = self.message_count.saturating_add(row.message_count);
    }
}

fn token_usage_numeric_rows(
    connection: &Connection,
) -> Result<Vec<TokenUsageNumericRow>, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT day, provider, model,
                    SUM(input_tokens), SUM(output_tokens),
                    SUM(cache_read_tokens), SUM(cache_creation_tokens),
                    SUM(reasoning_tokens), SUM(token_total), SUM(message_count),
                    SUM(estimated_cost_usd_micros),
                    SUM(CASE WHEN estimated_cost_usd_micros IS NOT NULL
                        THEN token_total ELSE 0 END),
                    SUM(CASE WHEN estimated_cost_usd_micros IS NULL
                        THEN token_total ELSE 0 END),
                    cost_kind, pricing_source
             FROM token_usage_session_days
             GROUP BY day, provider, model, cost_kind, pricing_source
             ORDER BY day, provider, model, cost_kind, pricing_source",
        )
        .map_err(storage_error)?;
    let rows = statement
        .query_map([], |row| {
            let input_tokens = from_i64(row.get(3)?);
            let output_tokens = from_i64(row.get(4)?);
            let cache_read_tokens = from_i64(row.get(5)?);
            let cache_creation_tokens = from_i64(row.get(6)?);
            let token_total = from_i64(row.get(8)?);
            let classified_tokens = input_tokens
                .saturating_add(output_tokens)
                .saturating_add(cache_read_tokens)
                .saturating_add(cache_creation_tokens);
            Ok(TokenUsageNumericRow {
                day: row.get(0)?,
                provider: row.get(1)?,
                model: row.get(2)?,
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_creation_tokens,
                reasoning_tokens: from_i64(row.get(7)?),
                unclassified_tokens: token_total.saturating_sub(classified_tokens),
                token_total,
                message_count: from_i64(row.get(9)?),
                estimated_cost_usd_micros: row.get::<_, Option<i64>>(10)?.map(from_i64),
                priced_tokens: from_i64(row.get(11)?),
                unpriced_tokens: from_i64(row.get(12)?),
                cost_kind: row.get(13)?,
                pricing_source: row.get(14)?,
            })
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    Ok(rows)
}

fn token_cost_status(priced_tokens: u64, unpriced_tokens: u64) -> &'static str {
    if unpriced_tokens == 0 {
        "priced"
    } else if priced_tokens == 0 {
        "unavailable"
    } else {
        "partial"
    }
}

fn token_usage_pricing_sources(
    connection: &Connection,
) -> Result<Vec<TokenUsagePricingSource>, StoreError> {
    connection
        .prepare(
            "SELECT COALESCE(cost_kind, 'unclassified'),
                    COALESCE(pricing_source, 'unspecified'),
                    SUM(token_total), SUM(estimated_cost_usd_micros)
             FROM token_usage_session_days
             WHERE estimated_cost_usd_micros IS NOT NULL
               AND token_total > 0
             GROUP BY COALESCE(cost_kind, 'unclassified'),
                      COALESCE(pricing_source, 'unspecified')
             ORDER BY SUM(token_total) DESC, pricing_source ASC",
        )
        .map_err(storage_error)?
        .query_map([], |row| {
            Ok(TokenUsagePricingSource {
                cost_kind: row.get(0)?,
                source: row.get(1)?,
                token_total: from_i64(row.get(2)?),
                estimated_cost_usd_micros: from_i64(row.get(3)?),
            })
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)
}

fn export_token_usage_json_database(
    connection: &Connection,
    now: u64,
) -> Result<Value, StoreError> {
    let rows = token_usage_numeric_rows(connection)?;
    let audit = read_metrics_and_token_usage(connection, now)?.1;
    let mut totals = TokenUsageNumericTotals::default();
    let daily = rows
        .iter()
        .map(|row| {
            totals.add(row);
            json!({
                "day": row.day,
                "provider": row.provider,
                "model": row.model,
                "inputTokens": row.input_tokens,
                "outputTokens": row.output_tokens,
                "cacheReadTokens": row.cache_read_tokens,
                "cacheCreationTokens": row.cache_creation_tokens,
                "reasoningTokens": row.reasoning_tokens,
                "unclassifiedTokens": row.unclassified_tokens,
                "tokenTotal": row.token_total,
                "pricedTokens": row.priced_tokens,
                "unpricedTokens": row.unpriced_tokens,
                "estimatedCostUsdMicros": row.estimated_cost_usd_micros,
                "costKind": row.cost_kind,
                "pricingSource": row.pricing_source,
                "costStatus": token_cost_status(row.priced_tokens, row.unpriced_tokens),
                "messageCount": row.message_count,
            })
        })
        .collect::<Vec<_>>();
    let total_cost = (totals.priced_tokens > 0).then_some(totals.estimated_cost_usd_micros);
    let pricing_sources = token_usage_pricing_sources(connection)?;
    Ok(json!({
        "schemaVersion": 1,
        "appVersion": env!("CARGO_PKG_VERSION"),
        "exportedAt": now,
        "scope": "token_usage_numeric",
        "dataQuality": "not_evaluated",
        "privacy": "Contains numeric day/provider/model aggregates only; no session IDs, prompts, paths, commands, tool content, or responses.",
        "semantics": {
            "currency": "USD",
            "estimatedCost": "API-equivalent estimate; a lower bound when unpricedTokens is greater than zero.",
            "reasoningTokens": "A diagnostic subset of outputTokens and never added to tokenTotal twice.",
            "unclassifiedTokens": "tokenTotal minus the non-overlapping input/output/cache components.",
            "pricingFreeze": "Computed historical cost remains attached to its stored pricingSource while token facts are unchanged; provider-reported cost may replace a computed estimate.",
            "observedTaskTime": "Turn start through the last factual event, including user waiting.",
            "agentExecutionTime": "Runtime intervals in thinking, tool_running, or compacting states only; user waiting is excluded and concurrent Agents are summed."
        },
        "pricingSources": pricing_sources,
        "anomalyCount": audit.anomaly_count,
        "suspectCount": audit.suspect_count,
        "anomalies": audit.anomalies,
        "totals": {
            "inputTokens": totals.input_tokens,
            "outputTokens": totals.output_tokens,
            "cacheReadTokens": totals.cache_read_tokens,
            "cacheCreationTokens": totals.cache_creation_tokens,
            "reasoningTokens": totals.reasoning_tokens,
            "unclassifiedTokens": totals.unclassified_tokens,
            "tokenTotal": totals.token_total,
            "pricedTokens": totals.priced_tokens,
            "unpricedTokens": totals.unpriced_tokens,
            "estimatedCostUsdMicros": total_cost,
            "costStatus": token_cost_status(totals.priced_tokens, totals.unpriced_tokens),
            "messageCount": totals.message_count,
            "todayObservedTaskTimeSeconds": audit.today_active_time_seconds,
            "monthObservedTaskTimeSeconds": audit.month_active_time_seconds,
            "observedTaskTimeSeconds": audit.active_time_seconds,
            "todayAgentExecutionTimeSeconds": audit.today_execution_time_seconds,
            "monthAgentExecutionTimeSeconds": audit.month_execution_time_seconds,
            "agentExecutionTimeSeconds": audit.execution_time_seconds
        },
        "daily": daily
    }))
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

fn export_token_usage_csv_database(
    connection: &Connection,
    _now: u64,
) -> Result<String, StoreError> {
    let rows = token_usage_numeric_rows(connection)?;
    let mut output = String::from(
        "day,provider,model,input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,reasoning_tokens_subset_of_output,unclassified_tokens,total_tokens,priced_tokens,unpriced_tokens,estimated_cost_usd_micros_lower_bound,cost_kind,pricing_source,message_count,cost_status\n",
    );
    for row in rows {
        let cost = row
            .estimated_cost_usd_micros
            .map(|value| value.to_string())
            .unwrap_or_default();
        output.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            csv_field(&row.day),
            csv_field(&row.provider),
            csv_field(&row.model),
            row.input_tokens,
            row.output_tokens,
            row.cache_read_tokens,
            row.cache_creation_tokens,
            row.reasoning_tokens,
            row.unclassified_tokens,
            row.token_total,
            row.priced_tokens,
            row.unpriced_tokens,
            cost,
            csv_field(row.cost_kind.as_deref().unwrap_or("")),
            csv_field(row.pricing_source.as_deref().unwrap_or("")),
            row.message_count,
            token_cost_status(row.priced_tokens, row.unpriced_tokens),
        ));
    }
    Ok(output)
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
                let mut reopened = Connection::open(path).map_err(storage_error)?;
                initialize(&mut reopened)?;
                *connection = reopened;
                return Err(StoreError::Storage(format!(
                    "failed to remove {}: {error}",
                    candidate.display()
                )));
            }
        }
    }
    prepare_database_file(path)?;
    let mut fresh = Connection::open(path).map_err(storage_error)?;
    initialize(&mut fresh)?;
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

fn initialize(connection: &mut Connection) -> Result<(), StoreError> {
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
              model TEXT, permission_mode TEXT, current_target TEXT, review_workdir TEXT,
              term_app TEXT, term_session_id TEXT, term_tty TEXT, term_title TEXT,
              term_bundle_id TEXT, term_surface TEXT, provider_pid INTEGER,
              exec_state TEXT NOT NULL DEFAULT 'idle',
              approval_owner TEXT, activity TEXT, activity_since INTEGER,
              plan_done INTEGER, plan_total INTEGER,
              token_total INTEGER, context_window_tokens INTEGER,
              started_at INTEGER NOT NULL, last_event_at INTEGER NOT NULL,
              last_meaningful_activity_at INTEGER,
              task_hidden_at INTEGER, task_hidden_reason TEXT,
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
            CREATE TABLE IF NOT EXISTS task_review_baselines (
              turn_id TEXT PRIMARY KEY,
              session_id TEXT NOT NULL,
              repository_root TEXT NOT NULL,
              repository_identity TEXT NOT NULL,
              branch TEXT, head TEXT, worktree_kind TEXT NOT NULL,
              dirty INTEGER NOT NULL,
              changed_files INTEGER NOT NULL,
              staged_files INTEGER NOT NULL,
              unstaged_files INTEGER NOT NULL,
              untracked_files INTEGER NOT NULL,
              insertions INTEGER, deletions INTEGER, binary_files INTEGER,
              turn_started_at INTEGER NOT NULL,
              first_tool_at INTEGER,
              captured_at INTEGER NOT NULL,
              FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE,
              FOREIGN KEY(turn_id) REFERENCES turns(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS events (
              id TEXT PRIMARY KEY, request_id TEXT,
              session_id TEXT NOT NULL, turn_id TEXT, provider TEXT NOT NULL,
              type TEXT NOT NULL, tool_name TEXT, tool_category TEXT,
              tool_target TEXT, tool_call_id TEXT, source_version TEXT,
              validation_status TEXT,
              summary TEXT,
              occurred_at INTEGER NOT NULL, ingest_seq INTEGER NOT NULL,
              timeline_risk TEXT,
              plan_step_count INTEGER, attention_id TEXT,
              derived INTEGER NOT NULL DEFAULT 0, provider_event_key TEXT,
              FOREIGN KEY(session_id) REFERENCES sessions(id),
              FOREIGN KEY(turn_id) REFERENCES turns(id)
            );
            CREATE TABLE IF NOT EXISTS session_tasks (
              session_id TEXT NOT NULL,
              task_id TEXT NOT NULL,
              subject TEXT,
              description TEXT,
              completed INTEGER NOT NULL DEFAULT 0,
              created_at INTEGER NOT NULL,
              completed_at INTEGER,
              PRIMARY KEY(session_id, task_id),
              FOREIGN KEY(session_id) REFERENCES sessions(id)
            );
            CREATE TABLE IF NOT EXISTS session_plan_steps (
              session_id TEXT NOT NULL,
              provider_turn_id TEXT NOT NULL,
              step_index INTEGER NOT NULL,
              step TEXT NOT NULL,
              detail TEXT,
              status TEXT NOT NULL,
              source TEXT NOT NULL,
              updated_at INTEGER NOT NULL,
              PRIMARY KEY(session_id, provider_turn_id, step_index),
              FOREIGN KEY(session_id) REFERENCES sessions(id)
            );
            CREATE TABLE IF NOT EXISTS session_subagents (
              session_id TEXT NOT NULL,
              agent_id TEXT NOT NULL,
              agent_type TEXT,
              status TEXT NOT NULL DEFAULT 'running',
              source TEXT,
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
              primary_category TEXT, risk_codes TEXT NOT NULL DEFAULT '[]',
              dedupe_key TEXT UNIQUE NOT NULL, state TEXT NOT NULL DEFAULT 'open',
              expires_at INTEGER, auto_hide_at INTEGER,
              reminder_acknowledged_at INTEGER, reminder_resolution TEXT,
              retain_after_ack INTEGER NOT NULL DEFAULT 0,
              created_at INTEGER NOT NULL,
              resolved_at INTEGER, resolution TEXT,
              remote_actionable INTEGER NOT NULL DEFAULT 0,
              FOREIGN KEY(session_id) REFERENCES sessions(id),
              FOREIGN KEY(turn_id) REFERENCES turns(id)
            );
            CREATE TABLE IF NOT EXISTS commands (
              id TEXT PRIMARY KEY,
              attention_id TEXT NOT NULL, request_id TEXT,
              action TEXT NOT NULL, state TEXT NOT NULL,
              commit_delay_ms INTEGER NOT NULL DEFAULT 3000,
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
              limit_id TEXT NOT NULL DEFAULT '',
              used_pct REAL, resets_at INTEGER, source TEXT,
              captured_at INTEGER NOT NULL, PRIMARY KEY(provider, window, limit_id)
            );
            CREATE TABLE IF NOT EXISTS session_usage (
              provider TEXT NOT NULL,
              provider_session_id TEXT NOT NULL,
              model TEXT,
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
            CREATE TABLE IF NOT EXISTS token_usage_cursors (
              provider TEXT NOT NULL,
              provider_session_id TEXT NOT NULL,
              project_id TEXT,
              project_label TEXT,
              parent_provider_session_id TEXT,
              token_total INTEGER NOT NULL,
              model TEXT,
              input_tokens INTEGER,
              output_tokens INTEGER,
              cache_read_tokens INTEGER,
              cache_creation_tokens INTEGER,
              reasoning_tokens INTEGER,
              estimated_cost_usd_micros INTEGER,
              captured_at INTEGER NOT NULL,
              PRIMARY KEY(provider, provider_session_id)
            );
            CREATE TABLE IF NOT EXISTS token_usage_daily (
              day TEXT NOT NULL,
              provider TEXT NOT NULL,
              token_total INTEGER NOT NULL DEFAULT 0,
              captured_at INTEGER NOT NULL,
              PRIMARY KEY(day, provider)
            );
            CREATE TABLE IF NOT EXISTS token_usage_daily_models (
              day TEXT NOT NULL,
              provider TEXT NOT NULL,
              model TEXT NOT NULL,
              token_total INTEGER NOT NULL DEFAULT 0,
              input_tokens INTEGER,
              output_tokens INTEGER,
              cache_read_tokens INTEGER,
              cache_creation_tokens INTEGER,
              reasoning_tokens INTEGER,
              estimated_cost_usd_micros INTEGER,
              captured_at INTEGER NOT NULL,
              PRIMARY KEY(day, provider, model)
            );
            CREATE TABLE IF NOT EXISTS token_usage_session_days (
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
              cost_kind TEXT,
              pricing_source TEXT,
              message_count INTEGER NOT NULL DEFAULT 0,
              captured_at INTEGER NOT NULL,
              PRIMARY KEY(provider, provider_session_id, day, model)
            );
            CREATE TABLE IF NOT EXISTS token_usage_rate_samples (
              provider TEXT NOT NULL,
              provider_session_id TEXT NOT NULL,
              session_id TEXT NOT NULL,
              turn_id TEXT NOT NULL,
              sampled_at INTEGER NOT NULL,
              token_total INTEGER NOT NULL,
              usage_source TEXT NOT NULL,
              usage_quality TEXT NOT NULL,
              PRIMARY KEY(provider, provider_session_id, sampled_at),
              FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE,
              FOREIGN KEY(turn_id) REFERENCES turns(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS task_checkpoints (
              id TEXT PRIMARY KEY,
              session_id TEXT NOT NULL,
              turn_id TEXT NOT NULL,
              label TEXT,
              kind TEXT NOT NULL,
              provider TEXT NOT NULL,
              provider_session_id TEXT NOT NULL,
              provider_resume_capability TEXT NOT NULL,
              repository_root TEXT,
              repository_identity TEXT,
              branch TEXT,
              head TEXT,
              worktree_kind TEXT,
              dirty INTEGER,
              changed_files INTEGER,
              staged_files INTEGER,
              unstaged_files INTEGER,
              untracked_files INTEGER,
              git_object_id TEXT,
              git_ref TEXT,
              patch_digest TEXT,
              validation_json TEXT NOT NULL,
              review_baseline_captured_at INTEGER,
              created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS ignored_provider_sessions (
              provider TEXT NOT NULL,
              provider_session_id TEXT NOT NULL,
              ignored_at INTEGER NOT NULL,
              reason TEXT NOT NULL,
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
            CREATE INDEX IF NOT EXISTS events_session_ingest_sequence
              ON events(session_id, ingest_seq ASC);
            CREATE INDEX IF NOT EXISTS events_ingest_sequence
              ON events(ingest_seq DESC);
            CREATE INDEX IF NOT EXISTS sessions_last_event_at
              ON sessions(last_event_at);
            CREATE INDEX IF NOT EXISTS events_occurred_at
              ON events(occurred_at);
            CREATE INDEX IF NOT EXISTS events_turn_occurred_at
              ON events(turn_id, occurred_at);
            CREATE INDEX IF NOT EXISTS attention_state
              ON attention_items(state, created_at);
            CREATE INDEX IF NOT EXISTS attention_session_blocker
              ON attention_items(session_id, state, kind, created_at);
            CREATE INDEX IF NOT EXISTS turns_session_ordinal
              ON turns(session_id, ordinal DESC);
            CREATE INDEX IF NOT EXISTS session_plan_steps_session_order
              ON session_plan_steps(session_id, step_index);
            CREATE INDEX IF NOT EXISTS session_tasks_session_order
              ON session_tasks(session_id, created_at);
            CREATE INDEX IF NOT EXISTS session_subagents_active_session_order
              ON session_subagents(session_id, active, started_at);
            "#,
        )
        .map_err(storage_error)?;
    ensure_session_title_column(connection)?;
    ensure_session_provider_title_columns(connection)?;
    ensure_session_current_target_column(connection)?;
    ensure_session_review_workdir_column(connection)?;
    ensure_review_baseline_schema(connection)?;
    ensure_session_usage_columns(connection)?;
    ensure_session_usage_model_column(connection)?;
    ensure_token_usage_detail_schema(connection)?;
    ensure_token_usage_rate_sample_schema(connection)?;
    ensure_task_checkpoint_schema(connection)?;
    ensure_agent_execution_interval_schema(connection)?;
    ensure_session_locator_columns(connection)?;
    ensure_session_activity_columns(connection)?;
    ensure_session_task_content_columns(connection)?;
    ensure_session_subagent_columns(connection)?;
    remove_retired_collaboration_storage(connection)?;
    ensure_event_timeline_columns(connection)?;
    ensure_event_tool_category_column(connection)?;
    ensure_event_validation_status_column(connection)?;
    ensure_quota_limit_identity(connection)?;
    ensure_approval_resolution_timeline_trigger(connection)?;
    ensure_attention_remote_actionable_column(connection)?;
    ensure_attention_classification_columns(connection)?;
    ensure_attention_auto_hide_column(connection)?;
    ensure_attention_reminder_columns(connection)?;
    ensure_attention_retention_column(connection)?;
    ensure_command_commit_delay_column(connection)?;
    ensure_session_task_visibility_columns(connection)?;
    normalize_legacy_subagent_rows(connection)?;
    normalize_orphaned_local_approval_sessions(connection, now_millis())?;
    suppress_existing_codex_internal_sessions(connection)?;
    remove_internal_context_task_titles(connection)?;
    normalize_existing_task_title_whitespace(connection)?;
    connection
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(storage_error)?;
    Ok(())
}

fn ensure_quota_limit_identity(connection: &mut Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(quota_snapshots)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    drop(statement);
    if columns.iter().any(|column| column == "limit_id") {
        return Ok(());
    }

    let transaction = connection.transaction().map_err(storage_error)?;
    transaction
        .execute_batch(
            "ALTER TABLE quota_snapshots RENAME TO quota_snapshots_legacy;
             CREATE TABLE quota_snapshots (
               provider TEXT NOT NULL, window TEXT NOT NULL,
               limit_id TEXT NOT NULL DEFAULT '',
               used_pct REAL, resets_at INTEGER, source TEXT,
               captured_at INTEGER NOT NULL,
               PRIMARY KEY(provider, window, limit_id)
             );
             INSERT INTO quota_snapshots(
               provider, window, limit_id, used_pct, resets_at, source, captured_at
             )
             SELECT provider, window, '', used_pct, resets_at, source, captured_at
             FROM quota_snapshots_legacy;
             DROP TABLE quota_snapshots_legacy;",
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)
}

/// Retire local collaboration metadata while preserving tasks and usage history.
fn remove_retired_collaboration_storage(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    transaction
        .execute_batch(
            "DROP TABLE IF EXISTS team_timeline_event_bindings;
         DROP TABLE IF EXISTS team_task_context_policies;
         DROP TABLE IF EXISTS transcript_sources;
         DROP TABLE IF EXISTS task_deletions;
         DROP INDEX IF EXISTS events_transcript_source;",
        )
        .map_err(storage_error)?;
    let columns = {
        let mut statement = transaction
            .prepare("PRAGMA table_info(events)")
            .map_err(storage_error)?;
        let result = statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        result
    };
    for column in [
        "transcript_source_id",
        "transcript_offset",
        "transcript_generation",
        "transcript_window_digest",
    ] {
        if columns.iter().any(|name| name == column) {
            transaction
                .execute(&format!("ALTER TABLE events DROP COLUMN {column}"), [])
                .map_err(storage_error)?;
        }
    }
    transaction.commit().map_err(storage_error)
}

fn ensure_event_timeline_columns(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(events)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    for (name, definition) in [
        ("timeline_risk", "TEXT"),
        ("plan_step_count", "INTEGER"),
        ("attention_id", "TEXT"),
        ("derived", "INTEGER NOT NULL DEFAULT 0"),
        ("provider_event_key", "TEXT"),
        ("tool_target", "TEXT"),
        ("tool_call_id", "TEXT"),
        ("source_version", "TEXT"),
    ] {
        if !columns.iter().any(|column| column == name) {
            connection
                .execute(
                    &format!("ALTER TABLE events ADD COLUMN {name} {definition}"),
                    [],
                )
                .map_err(storage_error)?;
        }
    }
    connection
        .execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS events_stable_provider_identity
             ON events(session_id, type, provider_event_key)
             WHERE provider_event_key IS NOT NULL",
            [],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn ensure_approval_resolution_timeline_trigger(connection: &Connection) -> Result<(), StoreError> {
    connection
        .execute_batch(
            "DROP TRIGGER IF EXISTS approval_resolution_timeline;
             CREATE TRIGGER approval_resolution_timeline
               AFTER UPDATE OF state ON attention_items
               WHEN OLD.state <> 'resolved'
                 AND NEW.state = 'resolved'
                 AND NEW.kind IN ('approval', 'native_approval')
               BEGIN
                 INSERT OR IGNORE INTO events (
                   id, request_id, session_id, turn_id, provider, type,
                   occurred_at, ingest_seq, attention_id, derived
                 )
                 VALUES (
                   'approval-resolution:' || NEW.id,
                   NEW.request_id,
                   NEW.session_id,
                   NEW.turn_id,
                   NEW.provider,
                   'approval.resolved',
                   COALESCE(NEW.resolved_at, NEW.created_at),
                   (SELECT COALESCE(MAX(ingest_seq), 0) + 1 FROM events),
                   NEW.id,
                   1
                 );
               END;",
        )
        .map_err(storage_error)
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

fn ensure_session_current_target_column(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(sessions)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    if !columns.iter().any(|column| column == "current_target") {
        connection
            .execute("ALTER TABLE sessions ADD COLUMN current_target TEXT", [])
            .map_err(storage_error)?;
    }
    Ok(())
}

fn ensure_session_review_workdir_column(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(sessions)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    if !columns.iter().any(|column| column == "review_workdir") {
        connection
            .execute("ALTER TABLE sessions ADD COLUMN review_workdir TEXT", [])
            .map_err(storage_error)?;
    }
    Ok(())
}

fn ensure_review_baseline_schema(connection: &Connection) -> Result<(), StoreError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS task_review_baselines (
               turn_id TEXT PRIMARY KEY,
               session_id TEXT NOT NULL,
               repository_root TEXT NOT NULL,
               repository_identity TEXT NOT NULL,
               branch TEXT, head TEXT, worktree_kind TEXT NOT NULL,
               dirty INTEGER NOT NULL,
               changed_files INTEGER NOT NULL,
               staged_files INTEGER NOT NULL,
               unstaged_files INTEGER NOT NULL,
               untracked_files INTEGER NOT NULL,
               insertions INTEGER, deletions INTEGER, binary_files INTEGER,
               turn_started_at INTEGER NOT NULL,
               first_tool_at INTEGER,
               captured_at INTEGER NOT NULL,
               FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE,
               FOREIGN KEY(turn_id) REFERENCES turns(id) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS review_baselines_session_capture
               ON task_review_baselines(session_id, captured_at DESC);",
        )
        .map_err(storage_error)
}

fn ensure_token_usage_rate_sample_schema(connection: &Connection) -> Result<(), StoreError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS token_usage_rate_samples (
               provider TEXT NOT NULL,
               provider_session_id TEXT NOT NULL,
               session_id TEXT NOT NULL,
               turn_id TEXT NOT NULL,
               sampled_at INTEGER NOT NULL,
               token_total INTEGER NOT NULL,
               usage_source TEXT NOT NULL,
               usage_quality TEXT NOT NULL,
               PRIMARY KEY(provider, provider_session_id, sampled_at),
               FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE,
               FOREIGN KEY(turn_id) REFERENCES turns(id) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS token_usage_rate_samples_turn_time
               ON token_usage_rate_samples(turn_id, sampled_at);
             CREATE INDEX IF NOT EXISTS token_usage_rate_samples_time
               ON token_usage_rate_samples(sampled_at);",
        )
        .map_err(storage_error)
}

fn ensure_task_checkpoint_schema(connection: &Connection) -> Result<(), StoreError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS task_checkpoints (
               id TEXT PRIMARY KEY,
               session_id TEXT NOT NULL,
               turn_id TEXT NOT NULL,
               label TEXT,
               kind TEXT NOT NULL,
               provider TEXT NOT NULL,
               provider_session_id TEXT NOT NULL,
               provider_resume_capability TEXT NOT NULL,
               repository_root TEXT,
               repository_identity TEXT,
               branch TEXT,
               head TEXT,
               worktree_kind TEXT,
               dirty INTEGER,
               changed_files INTEGER,
               staged_files INTEGER,
               unstaged_files INTEGER,
               untracked_files INTEGER,
               git_object_id TEXT,
               git_ref TEXT,
               patch_digest TEXT,
               validation_json TEXT NOT NULL,
               review_baseline_captured_at INTEGER,
               created_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS task_checkpoints_session_created
               ON task_checkpoints(session_id, created_at DESC);",
        )
        .map_err(storage_error)
}

fn ensure_event_tool_category_column(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(events)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    if !columns.iter().any(|column| column == "tool_category") {
        connection
            .execute("ALTER TABLE events ADD COLUMN tool_category TEXT", [])
            .map_err(storage_error)?;
    }
    Ok(())
}

fn ensure_event_validation_status_column(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(events)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    if !columns.iter().any(|column| column == "validation_status") {
        connection
            .execute("ALTER TABLE events ADD COLUMN validation_status TEXT", [])
            .map_err(storage_error)?;
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

fn ensure_session_usage_model_column(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(session_usage)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    if !columns.iter().any(|column| column == "model") {
        connection
            .execute("ALTER TABLE session_usage ADD COLUMN model TEXT", [])
            .map_err(storage_error)?;
    }
    Ok(())
}

fn ensure_token_usage_detail_schema(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(token_usage_cursors)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    drop(statement);
    for (column, definition) in [
        ("model", "TEXT"),
        ("project_id", "TEXT"),
        ("project_label", "TEXT"),
        ("parent_provider_session_id", "TEXT"),
        ("input_tokens", "INTEGER"),
        ("output_tokens", "INTEGER"),
        ("cache_read_tokens", "INTEGER"),
        ("cache_creation_tokens", "INTEGER"),
        ("reasoning_tokens", "INTEGER"),
        ("estimated_cost_usd_micros", "INTEGER"),
    ] {
        if !columns.iter().any(|existing| existing == column) {
            connection
                .execute(
                    &format!("ALTER TABLE token_usage_cursors ADD COLUMN {column} {definition}"),
                    [],
                )
                .map_err(storage_error)?;
        }
    }
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS token_usage_daily_models (
               day TEXT NOT NULL,
               provider TEXT NOT NULL,
               model TEXT NOT NULL,
               token_total INTEGER NOT NULL DEFAULT 0,
               input_tokens INTEGER,
               output_tokens INTEGER,
               cache_read_tokens INTEGER,
               cache_creation_tokens INTEGER,
               reasoning_tokens INTEGER,
               estimated_cost_usd_micros INTEGER,
               captured_at INTEGER NOT NULL,
               PRIMARY KEY(day, provider, model)
             );
             CREATE TABLE IF NOT EXISTS token_usage_session_days (
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
               cost_kind TEXT,
               pricing_source TEXT,
               message_count INTEGER NOT NULL DEFAULT 0,
               captured_at INTEGER NOT NULL,
               PRIMARY KEY(provider, provider_session_id, day, model)
             );",
        )
        .map_err(storage_error)?;
    let mut statement = connection
        .prepare("PRAGMA table_info(token_usage_session_days)")
        .map_err(storage_error)?;
    let daily_columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    drop(statement);
    for (column, definition) in [("cost_kind", "TEXT"), ("pricing_source", "TEXT")] {
        if !daily_columns.iter().any(|existing| existing == column) {
            connection
                .execute(
                    &format!(
                        "ALTER TABLE token_usage_session_days ADD COLUMN {column} {definition}"
                    ),
                    [],
                )
                .map_err(storage_error)?;
        }
    }
    connection
        .execute(
            "UPDATE token_usage_session_days
             SET cost_kind = COALESCE(cost_kind, 'legacy_unclassified'),
                 pricing_source = COALESCE(pricing_source, 'legacy_pre_v29')
             WHERE estimated_cost_usd_micros IS NOT NULL
               AND (cost_kind IS NULL OR pricing_source IS NULL)",
            [],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn ensure_agent_execution_interval_schema(connection: &Connection) -> Result<(), StoreError> {
    let existed: bool = connection
        .query_row(
            "SELECT EXISTS(
               SELECT 1 FROM sqlite_master
               WHERE type = 'table' AND name = 'agent_execution_intervals'
             )",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS agent_execution_intervals (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               session_id TEXT NOT NULL,
               turn_id TEXT,
               started_at INTEGER NOT NULL,
               ended_at INTEGER,
               start_reason TEXT NOT NULL,
               end_reason TEXT,
               FOREIGN KEY(session_id) REFERENCES sessions(id),
               FOREIGN KEY(turn_id) REFERENCES turns(id)
             );
             CREATE INDEX IF NOT EXISTS agent_execution_intervals_time
               ON agent_execution_intervals(started_at, ended_at);
             CREATE INDEX IF NOT EXISTS agent_execution_intervals_session_time
               ON agent_execution_intervals(session_id, started_at);
             CREATE UNIQUE INDEX IF NOT EXISTS agent_execution_intervals_one_open
               ON agent_execution_intervals(session_id)
               WHERE ended_at IS NULL;",
        )
        .map_err(storage_error)?;
    if !existed {
        // Historical intervals are reconstructed conservatively from Runtime's
        // normalized event states. Human-waiting states end execution; a later
        // Provider event must explicitly resume it. Every migrated interval is
        // closed at a factual event/turn/session boundary so an app restart can
        // never turn offline wall-clock time into Agent execution time.
        connection
            .execute_batch(
                "WITH classified AS (
                   SELECT session_id, turn_id, occurred_at, ingest_seq, type,
                          CASE
                            WHEN type IN (
                              'prompt.submitted', 'tool.started', 'tool.finished',
                              'tool.failed', 'approval.denied',
                              'approval.auto_review.started',
                              'approval.auto_review.completed', 'session.compacting'
                            ) THEN 1
                            WHEN type IN (
                              'session.started', 'session.ended',
                              'approval.requested', 'question.requested',
                              'elicitation.requested', 'turn.stopped',
                              'turn.interrupted', 'turn.failed'
                            ) THEN -1
                            ELSE 0
                          END AS marker
                   FROM events
                 ), meaningful AS (
                   SELECT * FROM classified WHERE marker <> 0
                 ), sequenced AS (
                   SELECT *, LAG(marker) OVER (
                     PARTITION BY session_id ORDER BY occurred_at, ingest_seq
                   ) AS previous_marker
                   FROM meaningful
                 ), starts AS (
                   SELECT * FROM sequenced
                   WHERE marker = 1
                     AND (type = 'prompt.submitted' OR COALESCE(previous_marker, -1) <> 1)
                 )
                 INSERT INTO agent_execution_intervals(
                   session_id, turn_id, started_at, ended_at,
                   start_reason, end_reason
                 )
                 SELECT starts.session_id, starts.turn_id, starts.occurred_at,
                        MAX(starts.occurred_at, MIN(
                          COALESCE((SELECT boundary.occurred_at
                             FROM sequenced AS boundary
                            WHERE boundary.session_id = starts.session_id
                              AND boundary.marker = -1
                              AND (
                                boundary.occurred_at > starts.occurred_at OR
                                (boundary.occurred_at = starts.occurred_at AND
                                 boundary.ingest_seq > starts.ingest_seq)
                              )
                            ORDER BY boundary.occurred_at, boundary.ingest_seq
                            LIMIT 1), 9223372036854775807),
                          COALESCE((SELECT MAX(turn_event.occurred_at)
                                      FROM events AS turn_event
                                     WHERE turn_event.turn_id = starts.turn_id),
                                   9223372036854775807),
                          COALESCE((SELECT last_event_at FROM sessions
                                     WHERE id = starts.session_id),
                                   9223372036854775807)
                        )),
                        'historical_event_rebuild', 'historical_boundary'
                 FROM starts;",
            )
            .map_err(storage_error)?;
    }
    // An interval left open by an earlier Runtime process is closed at that
    // session's last factual event. A live Provider signal can open a new
    // interval after startup; downtime is never counted as execution.
    connection
        .execute(
            "UPDATE agent_execution_intervals
             SET ended_at = MAX(started_at, COALESCE(
                   (SELECT last_event_at FROM sessions
                    WHERE sessions.id = agent_execution_intervals.session_id),
                   started_at
                 )),
                 end_reason = COALESCE(end_reason, 'runtime_restart')
             WHERE ended_at IS NULL",
            [],
        )
        .map_err(storage_error)?;
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

fn ensure_session_activity_columns(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(sessions)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    if !columns
        .iter()
        .any(|column| column == "last_meaningful_activity_at")
    {
        connection
            .execute(
                "ALTER TABLE sessions ADD COLUMN last_meaningful_activity_at INTEGER",
                [],
            )
            .map_err(storage_error)?;
    }
    // Lifecycle-only events are deliberately excluded: merely opening the
    // Claude app may replay SessionStart/SessionEnd for historical sessions.
    connection
        .execute(
            "UPDATE sessions
             SET last_meaningful_activity_at = (
               SELECT MAX(events.occurred_at) FROM events
               WHERE events.session_id = sessions.id
                 AND events.type IN (
                   'prompt.submitted', 'tool.started', 'tool.finished',
                   'tool.failed', 'approval.requested', 'approval.denied',
                   'question.requested', 'elicitation.requested',
                   'subagent.started', 'subagent.stopped',
                   'task.created', 'task.completed', 'plan.updated',
                   'approval.auto_review.started',
                   'approval.auto_review.completed', 'session.compacting',
                   'turn.interrupted', 'turn.failed'
                 )
             )
             WHERE last_meaningful_activity_at IS NULL",
            [],
        )
        .map_err(storage_error)?;
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS sessions_last_meaningful_activity_at
             ON sessions(last_meaningful_activity_at)",
            [],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn ensure_session_task_content_columns(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(session_tasks)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    for column in ["subject", "description"] {
        if !columns.iter().any(|existing| existing == column) {
            connection
                .execute(
                    &format!("ALTER TABLE session_tasks ADD COLUMN {column} TEXT"),
                    [],
                )
                .map_err(storage_error)?;
        }
    }
    Ok(())
}

fn ensure_session_subagent_columns(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(session_subagents)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    for (column, definition) in [
        ("agent_type", "TEXT"),
        ("status", "TEXT NOT NULL DEFAULT 'running'"),
        ("source", "TEXT"),
    ] {
        if !columns.iter().any(|existing| existing == column) {
            connection
                .execute(
                    &format!("ALTER TABLE session_subagents ADD COLUMN {column} {definition}"),
                    [],
                )
                .map_err(storage_error)?;
        }
    }
    Ok(())
}

fn ensure_attention_remote_actionable_column(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(attention_items)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    if !columns.iter().any(|column| column == "remote_actionable") {
        connection
            .execute(
                "ALTER TABLE attention_items
                 ADD COLUMN remote_actionable INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .map_err(storage_error)?;
    }
    Ok(())
}

fn ensure_command_commit_delay_column(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(commands)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    if !columns.iter().any(|column| column == "commit_delay_ms") {
        connection
            .execute(
                "ALTER TABLE commands
                 ADD COLUMN commit_delay_ms INTEGER NOT NULL DEFAULT 3000",
                [],
            )
            .map_err(storage_error)?;
    }
    Ok(())
}

fn ensure_attention_classification_columns(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(attention_items)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    if !columns.iter().any(|column| column == "primary_category") {
        connection
            .execute(
                "ALTER TABLE attention_items ADD COLUMN primary_category TEXT",
                [],
            )
            .map_err(storage_error)?;
    }
    if !columns.iter().any(|column| column == "risk_codes") {
        connection
            .execute(
                "ALTER TABLE attention_items
                 ADD COLUMN risk_codes TEXT NOT NULL DEFAULT '[]'",
                [],
            )
            .map_err(storage_error)?;
    }
    Ok(())
}

fn ensure_attention_auto_hide_column(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(attention_items)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    drop(statement);
    if !columns.iter().any(|column| column == "auto_hide_at") {
        connection
            .execute(
                "ALTER TABLE attention_items ADD COLUMN auto_hide_at INTEGER",
                [],
            )
            .map_err(storage_error)?;
    }
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS attention_completion_auto_hide
             ON attention_items(kind, state, auto_hide_at)",
            [],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn ensure_attention_reminder_columns(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(attention_items)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    drop(statement);
    for (column, definition) in [
        ("reminder_acknowledged_at", "INTEGER"),
        ("reminder_resolution", "TEXT"),
    ] {
        if !columns.iter().any(|existing| existing == column) {
            connection
                .execute(
                    &format!("ALTER TABLE attention_items ADD COLUMN {column} {definition}"),
                    [],
                )
                .map_err(storage_error)?;
        }
    }
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS attention_open_reminder
             ON attention_items(state, reminder_acknowledged_at, created_at)",
            [],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn ensure_attention_retention_column(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(attention_items)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    drop(statement);
    if !columns.iter().any(|column| column == "retain_after_ack") {
        connection
            .execute(
                "ALTER TABLE attention_items ADD COLUMN retain_after_ack INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .map_err(storage_error)?;
    }
    Ok(())
}

fn ensure_session_task_visibility_columns(connection: &Connection) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(sessions)")
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    drop(statement);
    for (column, definition) in [
        ("task_hidden_at", "INTEGER"),
        ("task_hidden_reason", "TEXT"),
    ] {
        if !columns.iter().any(|existing| existing == column) {
            connection
                .execute(
                    &format!("ALTER TABLE sessions ADD COLUMN {column} {definition}"),
                    [],
                )
                .map_err(storage_error)?;
        }
    }
    Ok(())
}

fn normalize_legacy_subagent_rows(connection: &Connection) -> Result<(), StoreError> {
    connection
        .execute(
            "UPDATE session_subagents
             SET status = 'completed',
                 stopped_at = COALESCE(stopped_at, started_at)
             WHERE active = 0 AND status = 'running'",
            [],
        )
        .map_err(storage_error)?;
    connection
        .execute(
            "UPDATE session_subagents
             SET active = 0,
                 status = CASE
                   WHEN sessions.exec_state = 'failed' THEN 'interrupted'
                   ELSE 'completed'
                 END,
                 stopped_at = COALESCE(stopped_at, sessions.last_event_at)
             FROM sessions
             WHERE session_subagents.session_id = sessions.id
               AND session_subagents.active = 1
               AND sessions.exec_state IN ('idle', 'response_finished', 'failed')
               AND sessions.ended_at IS NOT NULL",
            [],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn normalize_orphaned_local_approval_sessions(
    connection: &mut Connection,
    now: u64,
) -> Result<(), StoreError> {
    // Waiter continuations deliberately live only in memory. At startup there
    // cannot be a live local reply channel, so a widget-owned waiting state
    // without a blocking attention item is stale recovery metadata rather
    // than an active approval. Native provider-owned observations remain
    // untouched because their owner is `terminal` and their attention stays
    // blocking until an explicit Provider transition resolves it.
    let transaction = connection.transaction().map_err(storage_error)?;
    transaction
        .execute(
            "UPDATE sessions
             SET exec_state = 'waiting_for_event',
                 approval_owner = NULL,
                 activity = 'Runtime restarted and the old reply channel expired; waiting for a new Agent event',
                 activity_since = ?1
             WHERE exec_state = 'awaiting_approval'
               AND approval_owner = 'widget'
               AND NOT EXISTS (
                 SELECT 1 FROM attention_items
                 WHERE attention_items.session_id = sessions.id
                   AND attention_items.kind IN ('approval', 'native_approval', 'question')
                   AND attention_items.state IN ('open', 'committing', 'decision_sent', 'snoozed')
               )",
            [to_i64(now)],
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)?;
    Ok(())
}

fn ingest_transaction(
    connection: &mut Connection,
    request: BridgeRequest,
    completion_hide_policy: CompletionTaskHidePolicy,
) -> Result<IngestResult, StoreError> {
    let parsed = parse_hook(request.provider, request.raw.clone())
        .map_err(|error| StoreError::Provider(error.to_string()))?;
    let transaction = connection.transaction().map_err(storage_error)?;
    let provider = request.provider.to_string();
    if is_ignored_provider_session(&transaction, &provider, &parsed.provider_session_id)? {
        return Ok(IngestResult {
            inserted: false,
            suppressed: true,
            session_id: parsed.provider_session_id,
            attention_id: None,
            kind: parsed.kind,
            resolved_request_ids: Vec::new(),
        });
    }
    if is_codex_internal_background_prompt(&request, &parsed) {
        suppress_provider_session(
            &transaction,
            &provider,
            &parsed.provider_session_id,
            request.received_at,
        )?;
        transaction.commit().map_err(storage_error)?;
        return Ok(IngestResult {
            inserted: false,
            suppressed: true,
            session_id: parsed.provider_session_id,
            attention_id: None,
            kind: parsed.kind,
            resolved_request_ids: Vec::new(),
        });
    }
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
            suppressed: false,
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
    let current_target = if parsed.kind == EventKind::ToolStarted {
        tool_target_label(&request.raw)
    } else {
        None
    };
    let review_workdir = review_working_directory(&request.raw, parsed.cwd.as_deref());
    let (token_total, context_window_tokens) = normalized_token_usage(&request.raw);
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
                   term_bundle_id, term_surface, provider_pid, review_workdir,
                   exec_state, started_at, last_event_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, 'idle', ?19, ?19)",
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
                    review_workdir.as_deref(),
                    occurred_at,
                ],
            )
            .map_err(storage_error)?;
        (session_id, "idle".to_owned(), None, None, occurred_at)
    };

    let provider_event_key = stable_provider_event_key(&parsed, &request.raw);
    if let Some(provider_event_key) = provider_event_key.as_deref() {
        let duplicate_attention_id = transaction
            .query_row(
                "SELECT attention_id FROM events
                 WHERE session_id = ?1 AND type = ?2 AND provider_event_key = ?3",
                params![session_id, event_type(parsed.kind), provider_event_key],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(storage_error)?;
        if let Some(attention_id) = duplicate_attention_id {
            return Ok(IngestResult {
                inserted: false,
                suppressed: false,
                session_id,
                attention_id,
                kind: parsed.kind,
                resolved_request_ids: Vec::new(),
            });
        }
    }

    let terminal = matches!(current_state.as_str(), "response_finished" | "failed");
    let turn_id = select_or_create_turn(
        &transaction,
        &session_id,
        &parsed,
        occurred_at,
        parsed.kind,
        terminal,
    )?;
    if parsed.kind == EventKind::PromptSubmitted {
        reset_turn_scoped_task_state(&transaction, &session_id)?;
    }
    let timeline_risk = (parsed.kind == EventKind::PermissionRequested).then(|| {
        timeline_risk_level(classify_operation(
            parsed.tool_name.as_deref(),
            &request.raw,
        ))
    });
    let tool_category = matches!(
        parsed.kind,
        EventKind::ToolStarted | EventKind::ToolFinished | EventKind::ToolFailed
    )
    .then(|| workflow_tool_category(parsed.tool_name.as_deref(), &request.raw));
    let validation_status =
        structured_validation_status(parsed.kind, tool_category.as_deref(), &request.raw);
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
               tool_category, tool_target, tool_call_id, source_version,
               validation_status, summary, occurred_at, ingest_seq, timeline_risk, provider_event_key
             ) VALUES (
               ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17
             )",
            params![
                request.id.to_string(),
                request.request_id.map(|value| value.to_string()),
                session_id,
                turn_id,
                provider,
                event_type(parsed.kind),
                parsed.tool_name.as_deref().map(sanitized_tool_name),
                tool_category,
                current_target.as_deref(),
                parsed.tool_call_id.as_deref(),
                parsed.source_version.as_deref(),
                validation_status,
                event_summary(&request.raw, parsed.kind),
                occurred_at,
                sequence,
                timeline_risk.map(timeline_risk_value),
                provider_event_key,
            ],
        )
        .map_err(storage_error)?;

    let meaningful_activity = is_meaningful_activity(parsed.kind, &request.raw);
    if meaningful_activity {
        transaction
            .execute(
                "UPDATE sessions
                 SET last_meaningful_activity_at =
                   MAX(COALESCE(last_meaningful_activity_at, 0), ?2)
                 WHERE id = ?1",
                params![session_id, occurred_at],
            )
            .map_err(storage_error)?;
    }
    // A delayed tool/status event from an archived Turn must not resurrect the
    // task. Only a real new prompt (or an explicit live connector transition)
    // returns it to the active board.
    if parsed.kind == EventKind::PromptSubmitted {
        transaction
            .execute(
                "UPDATE sessions
                 SET task_hidden_at = NULL, task_hidden_reason = NULL
                 WHERE id = ?1",
                [session_id.as_str()],
            )
            .map_err(storage_error)?;
    }
    let turn_has_meaningful_activity = turn_id
        .as_deref()
        .map(|turn_id| turn_contains_meaningful_activity(&transaction, turn_id))
        .transpose()?
        .unwrap_or(false);
    let completed_without_background =
        parsed.kind == EventKind::Stopped && !has_background_work(&request.raw);
    let plan_progress = update_plan_progress(
        &transaction,
        &session_id,
        parsed.provider_turn_id.as_deref(),
        parsed.kind,
        &request.raw,
        occurred_at,
    )?;
    if parsed.kind == EventKind::PlanUpdated {
        transaction
            .execute(
                "UPDATE events SET plan_step_count = ?2 WHERE id = ?1",
                params![
                    request.id.to_string(),
                    i64::from(plan_progress.map(|(_, total)| total).unwrap_or(0)),
                ],
            )
            .map_err(storage_error)?;
    }
    let active_subagents = update_subagent_activity(
        &transaction,
        &session_id,
        parsed.kind,
        &request.raw,
        occurred_at,
    )?;
    reconcile_superseded_nonblocking_attention(
        &transaction,
        &session_id,
        parsed.kind,
        occurred_at,
    )?;
    let observed_native_permission = is_observed_codex_permission_start(&request);
    let native_permission_open = transaction
        .query_row(
            "SELECT EXISTS(
               SELECT 1 FROM attention_items
               WHERE session_id = ?1 AND kind = 'native_approval'
                 AND state IN ('open', 'snoozed')
             )",
            [&session_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(storage_error)?;
    if matches!(request.provider, Provider::Kimi | Provider::Grok)
        && matches!(parsed.kind, EventKind::ToolFinished | EventKind::ToolFailed)
    {
        resolve_observed_agent_question(&transaction, &session_id, &request, occurred_at)?;
    }
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
            if matches!(request.provider, Provider::Kimi | Provider::Grok) {
                resolve_observed_agent_question(&transaction, &session_id, &request, occurred_at)?;
            }
            Some(insert_nonapproval_attention(
                &transaction,
                &session_id,
                turn_id.as_deref(),
                &request,
                NonApprovalSpec {
                    kind: "question",
                    title: if matches!(request.provider, Provider::Kimi | Provider::Grok) {
                        "Agent is asking"
                    } else if request.provider == Provider::Codex {
                        "Codex is asking"
                    } else if parsed.kind == EventKind::ElicitationRequested {
                        "Claude needs more information"
                    } else {
                        "Claude is asking"
                    },
                    detail: Some(
                        "Answer directly in ActRealm. Answers are not written to local history.",
                    ),
                    dedupe_key: request
                        .request_id
                        .map(|id| format!("interactive:{id}"))
                        .unwrap_or_else(|| format!("interactive:{}", request.id)),
                },
                CompletionTaskHidePolicy::AfterConfirmation,
            )?)
        }
        EventKind::Failed => Some(insert_nonapproval_attention(
            &transaction,
            &session_id,
            turn_id.as_deref(),
            &request,
            NonApprovalSpec {
                kind: "error",
                title: "Agent run failed",
                detail: request.raw.get("error").and_then(Value::as_str),
                dedupe_key: format!(
                    "{}:{}:error:{}",
                    session_id,
                    turn_id.as_deref().unwrap_or("none"),
                    request.id
                ),
            },
            CompletionTaskHidePolicy::AfterConfirmation,
        )?),
        EventKind::Interrupted => Some(insert_nonapproval_attention(
            &transaction,
            &session_id,
            turn_id.as_deref(),
            &request,
            NonApprovalSpec {
                kind: "error",
                title: "Agent turn interrupted",
                detail: request
                    .raw
                    .get("error")
                    .or_else(|| request.raw.get("reason"))
                    .and_then(Value::as_str),
                dedupe_key: format!(
                    "{}:{}:interrupted",
                    session_id,
                    turn_id.as_deref().unwrap_or("none")
                ),
            },
            CompletionTaskHidePolicy::AfterConfirmation,
        )?),
        EventKind::Notification if is_structured_question(&request.raw) => {
            Some(insert_nonapproval_attention(
                &transaction,
                &session_id,
                turn_id.as_deref(),
                &request,
                NonApprovalSpec {
                    kind: "question",
                    title: "Agent needs attention",
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
                CompletionTaskHidePolicy::AfterConfirmation,
            )?)
        }
        EventKind::Stopped
            if completed_without_background
                && turn_has_meaningful_activity
                && !native_permission_open =>
        {
            Some(insert_nonapproval_attention(
                &transaction,
                &session_id,
                turn_id.as_deref(),
                &request,
                NonApprovalSpec {
                    kind: "completion",
                    title: "Task completed; waiting for confirmation",
                    detail: None,
                    dedupe_key: format!(
                        "{}:{}:completion",
                        session_id,
                        turn_id.as_deref().unwrap_or("none")
                    ),
                },
                completion_hide_policy,
            )?)
        }
        _ => None,
    };
    if let Some(attention_id) = attention_id.as_deref() {
        transaction
            .execute(
                "UPDATE events SET attention_id = ?2 WHERE id = ?1",
                params![request.id.to_string(), attention_id],
            )
            .map_err(storage_error)?;
    }

    reconcile_auto_review_attention(
        &transaction,
        &session_id,
        parsed.kind,
        &request.raw,
        occurred_at,
    )?;

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

    let preserves_native_request = current_state == "awaiting_approval"
        && current_owner.as_deref() == Some("terminal")
        && !native_permission_resolved
        && !matches!(
            parsed.kind,
            EventKind::PromptSubmitted
                | EventKind::PermissionDenied
                | EventKind::Interrupted
                | EventKind::Failed
                | EventKind::SessionEnded
        );
    // Codex emits SessionStart(source=compact) when it replaces the context
    // window inside an already-running turn. That lifecycle refresh must not
    // downgrade thinking/tool/approval state to idle; only an explicit Stop,
    // SessionEnd, failure, or authoritative Connector transition ends a turn.
    let preserves_compacting_turn = parsed.kind == EventKind::SessionStarted
        && request.raw.get("source").and_then(Value::as_str) == Some("compact")
        && !matches!(
            current_state.as_str(),
            "idle" | "response_finished" | "failed"
        );
    let may_update = occurred_at >= last_event_at
        && (!terminal
            || matches!(
                parsed.kind,
                EventKind::PromptSubmitted
                    | EventKind::SessionStarted
                    | EventKind::SessionEnded
                    | EventKind::ToolStarted
            ));
    // A structured plan update can legitimately arrive after the Provider has
    // already emitted a terminal lifecycle signal for the previous activity.
    // Keep the terminal execution state, but do not leave the session summary
    // behind the plan rows that were just persisted above.
    if occurred_at >= last_event_at {
        if let Some((done, total)) = plan_progress {
            transaction
                .execute(
                    "UPDATE sessions
                     SET plan_done = ?2, plan_total = ?3,
                         last_event_at = MAX(last_event_at, ?4)
                     WHERE id = ?1",
                    params![session_id, i64::from(done), i64::from(total), occurred_at],
                )
                .map_err(storage_error)?;
        }
    }
    if may_update {
        let (next_state, owner, default_activity) = if observed_native_permission {
            (
                "awaiting_approval",
                Some("terminal"),
                native_attention_activity(&request),
            )
        } else if native_permission_resolved && request.event_name() == Some("PermissionResult") {
            (
                "thinking",
                None,
                "Provider is continuing the active turn".to_owned(),
            )
        } else if preserves_native_request {
            (
                current_state.as_str(),
                current_owner.as_deref(),
                current_activity
                    .clone()
                    .unwrap_or_else(|| native_attention_activity(&request)),
            )
        } else if preserves_compacting_turn {
            (
                current_state.as_str(),
                current_owner.as_deref(),
                current_activity
                    .clone()
                    .unwrap_or_else(|| "Provider is continuing the active turn".to_owned()),
            )
        } else if request.provider_handles_approval && parsed.kind == EventKind::PermissionRequested
        {
            provider_handled_permission_state(parsed.permission_mode.as_deref(), &request.raw)
        } else {
            let (state, owner, activity) = project_event(parsed.kind, &request.raw, &current_state);
            if native_permission_resolved
                && matches!(parsed.kind, EventKind::ToolFinished | EventKind::ToolFailed)
            {
                (
                    state,
                    owner,
                    "The request was handled in Codex; continuing".to_owned(),
                )
            } else {
                (state, owner, activity)
            }
        };
        let activity = if preserves_native_request || preserves_compacting_turn {
            default_activity
        } else {
            plan_progress
                .map(|(done, total)| format!("Plan progress {done}/{total}"))
                .or_else(|| {
                    active_subagents.and_then(|active| {
                        if active == 0 && parsed.kind == EventKind::SubagentStopped {
                            Some("Subagent finished".to_owned())
                        } else if active > 0 {
                            Some(format!("{active} subagents running"))
                        } else {
                            None
                        }
                    })
                })
                .unwrap_or(default_activity)
        };
        transaction
            .execute(
                "UPDATE sessions SET
                   exec_state = ?2, approval_owner = ?3, activity = ?4,
                   activity_since = CASE
                     WHEN exec_state IN ('idle', 'response_finished', 'failed')
                      AND ?2 IN ('idle', 'response_finished', 'failed')
                     THEN COALESCE(activity_since, ?5)
                     ELSE ?5
                   END,
                   last_event_at = ?5,
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
                   permission_mode = COALESCE(?21, permission_mode),
                   model = COALESCE(?26, model),
                   current_target = CASE
                     WHEN ?2 != 'tool_running' THEN NULL
                     WHEN ?23 = 1 THEN ?22
                     ELSE current_target
                   END,
                   review_workdir = CASE
                     WHEN ?25 = 1 AND review_workdir IS NOT NULL
                     THEN review_workdir
                     ELSE COALESCE(?24, review_workdir)
                   END,
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
                    parsed.permission_mode.as_deref(),
                    current_target.as_deref(),
                    i64::from(parsed.kind == EventKind::ToolStarted),
                    review_workdir.as_deref(),
                    i64::from(parsed.kind == EventKind::PromptSubmitted),
                    parsed.model.as_deref(),
                ],
            )
            .map_err(storage_error)?;
        reconcile_agent_execution_interval(
            &transaction,
            &session_id,
            turn_id.as_deref(),
            next_state,
            occurred_at,
            event_type(parsed.kind),
        )?;
    }

    if matches!(
        parsed.kind,
        EventKind::Stopped | EventKind::Interrupted | EventKind::Failed | EventKind::SessionEnded
    ) && !(parsed.kind == EventKind::Stopped && preserves_native_request)
    {
        if let Some(turn_id) = turn_id.as_deref() {
            transaction
                .execute(
                    "UPDATE turns SET state = ?2, ended_at = ?3 WHERE id = ?1",
                    params![
                        turn_id,
                        match parsed.kind {
                            EventKind::Interrupted | EventKind::Failed => "failed",
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
        suppressed: false,
        session_id,
        attention_id,
        kind: parsed.kind,
        resolved_request_ids,
    })
}

fn is_codex_internal_background_prompt(
    request: &BridgeRequest,
    parsed: &actrealm_providers::ParsedHookEvent,
) -> bool {
    if request.provider != Provider::Codex || parsed.kind != EventKind::PromptSubmitted {
        return false;
    }
    let Some(prompt) = request
        .raw
        .get("prompt")
        .or_else(|| request.raw.get("user_prompt"))
        .and_then(Value::as_str)
    else {
        return false;
    };
    let normalized = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    matches_codex_internal_prompt(&normalized)
}

fn is_ignored_provider_session(
    connection: &Connection,
    provider: &str,
    provider_session_id: &str,
) -> Result<bool, StoreError> {
    connection
        .query_row(
            "SELECT EXISTS(
               SELECT 1 FROM ignored_provider_sessions
               WHERE provider = ?1 AND provider_session_id = ?2
             )",
            params![provider, provider_session_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(storage_error)
}

fn matches_codex_executable_probe(prompt: &str) -> bool {
    let candidate = Path::new(prompt);
    candidate.is_absolute()
        && candidate.file_name().and_then(|value| value.to_str()) == Some("codex")
        && !prompt.chars().any(char::is_whitespace)
}

fn matches_codex_hooks_setup(prompt: &str) -> bool {
    prompt == "/hooks"
        || prompt
            .strip_prefix('`')
            .and_then(|value| value.strip_suffix('`'))
            == Some("/hooks")
}

fn matches_codex_internal_prompt(prompt: &str) -> bool {
    CODEX_INTERNAL_PROMPT_PREFIXES
        .iter()
        .any(|prefix| prompt.starts_with(prefix))
        || matches_codex_hooks_setup(prompt)
        || matches_codex_executable_probe(prompt)
}

fn suppress_existing_codex_internal_sessions(
    connection: &mut Connection,
) -> Result<(), StoreError> {
    let candidates = {
        let mut statement = connection
            .prepare(
                "SELECT provider_session_id, title, last_event_at FROM sessions
                 WHERE provider = 'codex' AND title IS NOT NULL",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(storage_error)?
            .filter_map(|row| match row {
                Ok((provider_session_id, title, last_event_at))
                    if matches_codex_internal_prompt(&title) =>
                {
                    Some(Ok((provider_session_id, last_event_at)))
                }
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        rows
    };
    if candidates.is_empty() {
        return Ok(());
    }
    let transaction = connection.transaction().map_err(storage_error)?;
    for (provider_session_id, last_event_at) in candidates {
        suppress_provider_session(
            &transaction,
            "codex",
            &provider_session_id,
            from_i64(last_event_at),
        )?;
    }
    transaction.commit().map_err(storage_error)
}

fn suppress_provider_session(
    transaction: &Transaction<'_>,
    provider: &str,
    provider_session_id: &str,
    ignored_at: u64,
) -> Result<(), StoreError> {
    let existing = transaction
        .query_row(
            "SELECT id, started_at FROM sessions
             WHERE provider = ?1 AND provider_session_id = ?2",
            params![provider, provider_session_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(storage_error)?;
    transaction
        .execute(
            "INSERT INTO ignored_provider_sessions(
               provider, provider_session_id, ignored_at, reason
             ) VALUES (?1, ?2, ?3, 'codex_internal_background_prompt')
             ON CONFLICT(provider, provider_session_id) DO NOTHING",
            params![provider, provider_session_id, to_i64(ignored_at)],
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "DELETE FROM session_usage
             WHERE provider = ?1 AND provider_session_id = ?2",
            params![provider, provider_session_id],
        )
        .map_err(storage_error)?;

    let Some((session_id, started_at)) = existing else {
        return Ok(());
    };
    transaction
        .execute(
            "DELETE FROM commands WHERE attention_id IN (
               SELECT id FROM attention_items WHERE session_id = ?1
             )",
            [&session_id],
        )
        .map_err(storage_error)?;
    for table in [
        "attention_items",
        "events",
        "session_tasks",
        "session_plan_steps",
        "session_subagents",
        "agent_execution_intervals",
        "turns",
    ] {
        transaction
            .execute(
                &format!("DELETE FROM {table} WHERE session_id = ?1"),
                [&session_id],
            )
            .map_err(storage_error)?;
    }
    transaction
        .execute("DELETE FROM sessions WHERE id = ?1", [&session_id])
        .map_err(storage_error)?;
    transaction
        .execute(
            "UPDATE metrics_daily
             SET sessions_observed = MAX(sessions_observed - 1, 0)
             WHERE day = ?1",
            [metric_day(from_i64(started_at))],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn claim_transaction(
    connection: &mut Connection,
    command_id: Uuid,
    request_id: Uuid,
    action: ApprovalAction,
    now: u64,
    commit_delay_ms: u64,
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
    let commit_delay_ms = if action.decision().is_some() {
        commit_delay_ms
    } else {
        0
    };
    transaction
        .execute(
            "INSERT INTO commands (
               id, attention_id, request_id, action, state,
               commit_delay_ms, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                command_id.to_string(),
                attention.0,
                request_id_string,
                action.as_str(),
                command_state,
                to_i64(commit_delay_ms),
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
            .map(|_| now.saturating_add(commit_delay_ms)),
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
            "SELECT attention_id, state, created_at, commit_delay_ms
             FROM commands WHERE id = ?1",
            [command_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
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
        || command.3 <= 0
        || to_i64(now) >= command.2.saturating_add(command.3)
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
                    commands.state, commands.created_at, commands.commit_delay_ms,
                    attention_items.created_at, attention_items.session_id
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
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
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
    if to_i64(now) < command.4.saturating_add(command.5) {
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
        release_session_if_unblocked(
            &transaction,
            &command.7,
            "Reply channel expired; waiting for a new Agent event",
            now,
        )?;
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
        from_i64(command.4).saturating_sub(from_i64(command.6)),
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
    let (kind, state, session_id, auto_hide_at, reminder_acknowledged_at, retain_after_ack) =
        transaction
            .query_row(
                "SELECT kind, state, session_id, auto_hide_at, reminder_acknowledged_at,
                    retain_after_ack
             FROM attention_items WHERE id = ?1",
                [attention_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                        row.get::<_, bool>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(storage_error)?
            .ok_or(StoreError::StaleApproval)?;
    if kind == "approval" || !matches!(state.as_str(), "open" | "snoozed") {
        return Err(StoreError::StaleApproval);
    }
    let reminder_only_ack = action == AttentionAction::Ack
        && kind == "completion"
        && (auto_hide_at.is_some() || retain_after_ack);
    if reminder_only_ack && reminder_acknowledged_at.is_some() {
        return Err(StoreError::StaleApproval);
    }
    if action == AttentionAction::Snooze && reminder_acknowledged_at.is_some() {
        return Err(StoreError::StaleApproval);
    }
    match action {
        AttentionAction::Ack if reminder_only_ack => {
            transaction
                .execute(
                    "UPDATE attention_items
                     SET reminder_acknowledged_at = ?2,
                         reminder_resolution = 'reminder_acknowledged',
                         expires_at = NULL
                     WHERE id = ?1 AND reminder_acknowledged_at IS NULL",
                    params![attention_id, to_i64(now)],
                )
                .map_err(storage_error)?;
        }
        AttentionAction::Ack => {
            let resolution = if kind == "completion" {
                "ack_hidden"
            } else {
                "ack"
            };
            transaction
                .execute(
                    "UPDATE attention_items SET state = 'resolved', resolved_at = ?2,
                       resolution = ?3, expires_at = NULL, auto_hide_at = NULL
                     WHERE id = ?1",
                    params![attention_id, to_i64(now), resolution],
                )
                .map_err(storage_error)?;
            if kind == "completion" {
                transaction
                    .execute(
                        "UPDATE sessions
                         SET task_hidden_at = ?2, task_hidden_reason = 'ack_hidden'
                         WHERE id = ?1",
                        params![&session_id, to_i64(now)],
                    )
                    .map_err(storage_error)?;
            }
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
                       resolution = 'user_dismissed', expires_at = NULL,
                       auto_hide_at = NULL WHERE id = ?1",
                    params![attention_id, to_i64(now)],
                )
                .map_err(storage_error)?;
            if kind == "completion" {
                transaction
                    .execute(
                        "UPDATE sessions
                         SET task_hidden_at = ?2, task_hidden_reason = 'user_dismissed'
                         WHERE id = ?1",
                        params![&session_id, to_i64(now)],
                    )
                    .map_err(storage_error)?;
            }
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
    if matches!(action, AttentionAction::Ack | AttentionAction::Dismiss) && !reminder_only_ack {
        release_session_if_unblocked(
            &transaction,
            &session_id,
            "Attention item resolved; waiting for the Agent's next event",
            now,
        )?;
    }
    transaction.commit().map_err(storage_error)?;
    Ok(CommandState::Confirmed)
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
                "SELECT id, request_id, session_id FROM attention_items
                 WHERE kind IN ('approval', 'question') AND request_id IS NOT NULL
                 AND state IN ('open', 'committing', 'decision_sent')",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        rows
    };
    let mut expired = 0;
    let mut affected_sessions = HashSet::new();
    for (attention_id, request_id, session_id) in candidates {
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
        affected_sessions.insert(session_id);
        expired += 1;
    }
    for session_id in affected_sessions {
        release_session_if_unblocked(
            &transaction,
            &session_id,
            "Runtime restarted and the old reply channel expired; waiting for a new Agent event",
            now,
        )?;
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
            "SELECT id, created_at, kind, session_id FROM attention_items WHERE request_id = ?1
             AND state IN ('open', 'committing', 'decision_sent')",
            [request_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?;
    let Some((attention_id, _created_at, kind, session_id)) = attention_id else {
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
    release_session_if_unblocked(
        &transaction,
        &session_id,
        "Reply channel expired; waiting for a new Agent event",
        now,
    )?;
    transaction.commit().map_err(storage_error)?;
    Ok(true)
}

fn resolve_managed_request_transaction(
    connection: &mut Connection,
    request_id: Uuid,
    now: u64,
) -> Result<bool, StoreError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    let attention = transaction
        .query_row(
            "SELECT id, session_id FROM attention_items
             WHERE request_id = ?1
               AND state IN ('open', 'committing', 'decision_sent', 'snoozed')",
            [request_id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let Some((attention_id, session_id)) = attention else {
        return Ok(false);
    };
    transaction
        .execute(
            "UPDATE commands
             SET state = CASE
                   WHEN state = 'decision_sent' THEN 'confirmed'
                   ELSE 'failed'
                 END,
                 confirmed_at = ?2,
                 error_code = CASE
                   WHEN state = 'decision_sent' THEN NULL
                   ELSE 'provider_resolved_before_decision'
                 END
             WHERE attention_id = ?1
               AND state IN ('pending_commit', 'decision_sent')",
            params![attention_id, to_i64(now)],
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "UPDATE attention_items
             SET state = 'resolved', resolved_at = ?2,
                 resolution = 'provider_resolved', expires_at = NULL
             WHERE id = ?1",
            params![attention_id, to_i64(now)],
        )
        .map_err(storage_error)?;
    release_session_if_unblocked(
        &transaction,
        &session_id,
        "Codex received the approval decision; continuing",
        now,
    )?;
    transaction.commit().map_err(storage_error)?;
    Ok(true)
}

fn release_session_if_unblocked(
    transaction: &Transaction<'_>,
    session_id: &str,
    activity: &str,
    now: u64,
) -> Result<(), StoreError> {
    let blockers: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM attention_items
             WHERE session_id = ?1
               AND kind IN ('approval', 'native_approval', 'question')
               AND state IN ('open', 'committing', 'decision_sent', 'snoozed')",
            [session_id],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if blockers != 0 {
        return Ok(());
    }
    let updated = transaction
        .execute(
            "UPDATE sessions SET exec_state = 'waiting_for_event',
               approval_owner = NULL, activity = ?2, activity_since = ?3
             WHERE id = ?1
               AND (exec_state = 'awaiting_approval' OR approval_owner = 'widget')",
            params![session_id, activity, to_i64(now)],
        )
        .map_err(storage_error)?;
    if updated > 0 {
        let turn_id = current_open_turn_id(transaction, session_id)?;
        reconcile_agent_execution_interval(
            transaction,
            session_id,
            turn_id.as_deref(),
            "waiting_for_event",
            to_i64(now),
            "attention.resolved",
        )?;
    }
    Ok(())
}

fn observe_codex_turn_end_transaction(
    connection: &mut Connection,
    thread_id: &str,
    turn_id: &str,
    event: &str,
    at: u64,
    hide_policy: CompletionTaskHidePolicy,
) -> Result<bool, StoreError> {
    if !matches!(event, "Stop" | "StopFailure" | "TurnInterrupted") {
        return Ok(false);
    }
    let current = connection.query_row(
        "SELECT sessions.last_event_at, turns.provider_turn_id, turns.started_at, sessions.exec_state
         FROM sessions LEFT JOIN turns ON turns.id = (
           SELECT id FROM turns WHERE session_id = sessions.id ORDER BY ordinal DESC LIMIT 1
         ) WHERE sessions.provider = 'codex' AND sessions.provider_session_id = ?1",
        [thread_id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, Option<i64>>(2)?, row.get::<_, String>(3)?))
    ).optional().map_err(storage_error)?;
    let Some((last_event_at, current_turn, started_at, state)) = current else {
        return Ok(false);
    };
    if to_i64(at) < last_event_at
        || started_at.is_some_and(|start| to_i64(at) < start)
        || current_turn.as_deref().is_some_and(|id| id != turn_id)
        || matches!(state.as_str(), "response_finished" | "failed" | "idle")
    {
        return Ok(false);
    }
    // This executes on the sole writer, without an intervening Hook between
    // checking turn ownership and applying the ordinary lifecycle reducer.
    let mut request = BridgeRequest::from_hook_at(
        Provider::Codex,
        json!({
            "hook_event_name": event, "session_id": thread_id, "turn_id": turn_id
        }),
        at,
    );
    request.term = None;
    ingest_transaction(connection, request, hide_policy).map(|result| result.inserted)
}

fn mark_execution_unconfirmed_transaction(
    connection: &mut Connection,
    session_id: &str,
    expected_last_event_at: u64,
) -> Result<bool, StoreError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    let changed = transaction.execute(
        "UPDATE sessions SET exec_state = 'waiting_for_event', activity = NULL,
         current_target = NULL, approval_owner = NULL
         WHERE id = ?1 AND last_event_at = ?2 AND exec_state IN ('thinking', 'tool_running', 'compacting', 'awaiting_approval')
         AND NOT EXISTS (SELECT 1 FROM attention_items WHERE session_id = ?1
           AND kind IN ('approval', 'native_approval', 'question')
           AND state IN ('open', 'committing', 'decision_sent', 'snoozed'))",
        params![session_id, to_i64(expected_last_event_at)]
    ).map_err(storage_error)?;
    if changed > 0 {
        let turn_id = current_open_turn_id(&transaction, session_id)?;
        reconcile_agent_execution_interval(
            &transaction,
            session_id,
            turn_id.as_deref(),
            "waiting_for_event",
            to_i64(expected_last_event_at),
            "process.unconfirmed",
        )?;
    }
    transaction.commit().map_err(storage_error)?;
    Ok(changed > 0)
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
               activity = 'Agent process is no longer active', ended_at = ?2 WHERE id = ?1",
                params![session_id, to_i64(now)],
            )
            .map_err(storage_error)?;
        let turn_id = current_open_turn_id(&transaction, &session_id)?;
        reconcile_agent_execution_interval(
            &transaction,
            &session_id,
            turn_id.as_deref(),
            "idle",
            to_i64(now),
            "process.inactive",
        )?;
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
    completion_hide_policy: CompletionTaskHidePolicy,
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
                Provider::Codex => "Approve in Codex",
                Provider::Claude => "Approve in Claude",
                Provider::Gemini | Provider::Kimi | Provider::Grok => "Approve in the Agent",
            };
            let detail = match provider {
                Provider::Codex => {
                    "Codex is waiting for a native permission decision. Approve or deny it in the corresponding conversation."
                }
                Provider::Claude => {
                    "Claude is waiting for a native permission decision. Approve or deny it in the corresponding conversation."
                }
                Provider::Gemini | Provider::Kimi | Provider::Grok => {
                    "The Agent is waiting for a native permission decision. Handle it in the corresponding conversation."
                }
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
                 WHERE id = ?1 AND (
                   ?4 = 1 OR exec_state != 'awaiting_approval'
                   OR approval_owner IS NOT 'terminal'
                 )",
                params![
                    session_id,
                    format!(
                        "Waiting for you to handle permission in {}",
                        provider_display_name(provider)
                    ),
                    to_i64(now),
                    // Re-observing the same native request is polling, not a
                    // new Provider event that can restore a deleted Display card.
                    i64::from(native_open.is_none()),
                ],
            )
            .map_err(storage_error)?;
        let turn_id = current_open_turn_id(&transaction, &session_id)?;
        reconcile_agent_execution_interval(
            &transaction,
            &session_id,
            turn_id.as_deref(),
            "awaiting_approval",
            to_i64(now),
            "native_approval.waiting",
        )?;
    } else {
        let native_turn_id = transaction
            .query_row(
                "SELECT turn_id FROM attention_items
                 WHERE session_id = ?1 AND kind = 'native_approval'
                   AND state IN ('open', 'snoozed')
                 ORDER BY created_at DESC LIMIT 1",
                [&session_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(storage_error)?
            .flatten();
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
            let next_state = if active {
                "thinking"
            } else {
                "response_finished"
            };
            transaction
                .execute(
                    "UPDATE sessions SET exec_state = ?2, approval_owner = NULL,
                       activity = ?3, activity_since = ?4,
                       last_event_at = MAX(last_event_at, ?4)
                     WHERE id = ?1",
                    params![
                        session_id,
                        next_state,
                        if active {
                            format!(
                                "The permission request was handled in {}",
                                provider_display_name(provider)
                            )
                        } else {
                            format!(
                                "The permission request was handled in {}; the turn ended",
                                provider_display_name(provider)
                            )
                        },
                        to_i64(now),
                    ],
                )
                .map_err(storage_error)?;
            reconcile_agent_execution_interval(
                &transaction,
                &session_id,
                native_turn_id.as_deref(),
                next_state,
                to_i64(now),
                "native_approval.resolved",
            )?;
            if !active {
                let meaningful: bool = transaction
                    .query_row(
                        "SELECT last_meaningful_activity_at IS NOT NULL
                         FROM sessions WHERE id = ?1",
                        [&session_id],
                        |row| row.get(0),
                    )
                    .map_err(storage_error)?;
                if meaningful {
                    let completion_id = Uuid::now_v7().to_string();
                    let completion_key = format!(
                        "{}:{}:completion",
                        session_id,
                        native_turn_id.as_deref().unwrap_or("none")
                    );
                    transaction
                        .execute(
                            "INSERT INTO attention_items (
                               id, session_id, provider, project, turn_id, request_id,
                               kind, title, detail, command_preview, risk, risk_notes,
                               dedupe_key, state, auto_hide_at, retain_after_ack, created_at
                             ) VALUES (
                               ?1, ?2, ?3, ?4, ?5, NULL, 'completion',
                               'Task completed; waiting for confirmation', NULL, NULL, 'unknown', '[]',
                               ?6, 'open', ?7, ?8, ?9
                             )
                             ON CONFLICT(dedupe_key) DO NOTHING",
                            params![
                                completion_id,
                                session_id,
                                provider_name,
                                project.as_deref(),
                                native_turn_id,
                                completion_key,
                                completion_hide_policy
                                    .delay_ms()
                                    .map(|delay| to_i64(now.saturating_add(delay))),
                                completion_hide_policy.retain_after_ack(),
                                to_i64(now),
                            ],
                        )
                        .map_err(storage_error)?;
                }
            }
        }
    }
    transaction.commit().map_err(storage_error)?;
    Ok(NativeApprovalSyncResult {
        session_found: true,
        resolved_request_ids,
    })
}

fn sync_provider_execution_transaction(
    connection: &mut Connection,
    provider: Provider,
    provider_session_id: &str,
    active: bool,
    now: u64,
) -> Result<bool, StoreError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    let session = transaction
        .query_row(
            "SELECT id, exec_state FROM sessions
             WHERE provider = ?1 AND provider_session_id = ?2
             ORDER BY last_event_at DESC LIMIT 1",
            params![provider.to_string(), provider_session_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let Some((session_id, exec_state)) = session else {
        return Ok(false);
    };
    let blockers: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM attention_items
             WHERE session_id = ?1
               AND kind IN ('approval', 'native_approval', 'question')
               AND state IN ('open', 'committing', 'decision_sent', 'snoozed')",
            [&session_id],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let mut projected_state = exec_state.clone();
    if active {
        if blockers == 0
            && matches!(
                exec_state.as_str(),
                "idle" | "response_finished" | "failed" | "waiting_for_event"
            )
        {
            transaction
                .execute(
                    "UPDATE sessions SET exec_state = 'thinking', approval_owner = NULL,
                       activity = 'Provider reports an active turn', activity_since = ?2,
                       last_event_at = MAX(last_event_at, ?2),
                       last_meaningful_activity_at =
                         MAX(COALESCE(last_meaningful_activity_at, 0), ?2),
                       task_hidden_at = NULL, task_hidden_reason = NULL
                     WHERE id = ?1",
                    params![session_id, to_i64(now)],
                )
                .map_err(storage_error)?;
            projected_state = "thinking".to_owned();
        }
    } else if blockers == 0
        && !matches!(exec_state.as_str(), "idle" | "response_finished" | "failed")
    {
        transaction
            .execute(
                "UPDATE sessions SET exec_state = 'response_finished', approval_owner = NULL,
                   activity = 'Provider reports that the turn ended', activity_since = ?2,
                   last_event_at = MAX(last_event_at, ?2)
                 WHERE id = ?1",
                params![session_id, to_i64(now)],
            )
            .map_err(storage_error)?;
        projected_state = "response_finished".to_owned();
    }
    if blockers == 0 {
        let turn_id = current_open_turn_id(&transaction, &session_id)?;
        reconcile_agent_execution_interval(
            &transaction,
            &session_id,
            turn_id.as_deref(),
            &projected_state,
            to_i64(now),
            if active {
                "provider.execution.active"
            } else {
                "provider.execution.inactive"
            },
        )?;
    }
    transaction.commit().map_err(storage_error)?;
    Ok(true)
}

fn provider_display_name(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => "Claude",
        Provider::Codex => "Codex",
        Provider::Kimi => "Kimi Code",
        Provider::Grok => "Grok Build",
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

fn read_local_review_context(
    connection: &Connection,
    session_id: &str,
) -> Result<Option<LocalReviewContext>, StoreError> {
    let record = connection
        .query_row(
            "SELECT sessions.provider, sessions.provider_session_id,
                    sessions.project,
                    CASE
                      WHEN sessions.review_workdir IS NULL
                        OR sessions.review_workdir = sessions.cwd
                      THEN COALESCE(
                        (SELECT previous.repository_root
                         FROM task_review_baselines AS previous
                         WHERE previous.session_id = sessions.id
                         ORDER BY previous.captured_at DESC LIMIT 1),
                        sessions.review_workdir,
                        sessions.cwd
                      )
                      ELSE sessions.review_workdir
                    END,
                    sessions.exec_state, turns.id, turns.started_at, turns.ended_at,
                    sessions.last_event_at,
                    sessions.term_app, sessions.term_session_id, sessions.term_tty,
                    sessions.term_bundle_id, sessions.term_surface
             FROM sessions
             LEFT JOIN turns ON turns.id = (
               SELECT current_turn.id FROM turns AS current_turn
               WHERE current_turn.session_id = sessions.id
               ORDER BY current_turn.ordinal DESC LIMIT 1
             )
             WHERE sessions.id = ?1",
            [session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<i64>>(6)?.map(from_i64),
                    row.get::<_, Option<i64>>(7)?.map(from_i64),
                    from_i64(row.get(8)?),
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, Option<String>>(13)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?;
    let Some((
        provider,
        provider_session_id,
        project,
        working_directory,
        exec_state,
        turn_id,
        turn_started_at,
        turn_ended_at,
        last_event_at,
        term_app,
        term_session_id,
        term_tty,
        term_bundle_id,
        term_surface,
    )) = record
    else {
        return Ok(None);
    };
    let working_directory = working_directory
        .filter(|value| !value.trim().is_empty() && value != "/")
        .map(PathBuf::from);
    let (jump_capability, _) = jump_descriptor(
        &provider,
        &provider_session_id,
        term_app.as_deref(),
        term_session_id.as_deref(),
        term_tty.as_deref(),
        term_bundle_id.as_deref(),
        term_surface.as_deref(),
    );
    let concurrent_active_sessions = match (working_directory.as_ref(), turn_started_at) {
        (Some(working_directory), Some(turn_started_at)) => {
            let turn_ended_at = turn_ended_at.unwrap_or(last_event_at);
            connection
                .query_row(
                    "SELECT COUNT(DISTINCT peer.id)
                     FROM sessions AS peer
                     JOIN turns AS peer_turn ON peer_turn.session_id = peer.id
                     WHERE COALESCE(peer.review_workdir, peer.cwd) = ?1
                       AND peer.id != ?2
                       AND peer_turn.started_at <= ?4
                       AND COALESCE(peer_turn.ended_at, peer.last_event_at) >= ?3",
                    params![
                        working_directory.to_string_lossy(),
                        session_id,
                        to_i64(turn_started_at),
                        to_i64(turn_ended_at)
                    ],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(storage_error)
                .and_then(review_session_count)?
        }
        (Some(working_directory), None) => connection
            .query_row(
                "SELECT COUNT(*) FROM sessions
                 WHERE COALESCE(review_workdir, cwd) = ?1 AND id != ?2
                   AND exec_state NOT IN ('idle', 'response_finished', 'failed')",
                params![working_directory.to_string_lossy(), session_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(storage_error)
            .and_then(review_session_count)?,
        (None, _) => 0,
    };
    Ok(Some(LocalReviewContext {
        session_id: session_id.to_owned(),
        provider,
        provider_session_id,
        jump_capability,
        project,
        working_directory,
        exec_state,
        turn_id,
        turn_started_at,
        turn_ended_at,
        last_event_at,
        concurrent_active_sessions,
    }))
}

fn review_session_count(value: i64) -> Result<u32, StoreError> {
    u32::try_from(value)
        .map_err(|_| StoreError::Storage("review session count overflow".to_owned()))
}

fn read_pending_review_baselines(
    connection: &Connection,
    limit: usize,
) -> Result<Vec<ReviewBaselineCandidate>, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT sessions.id, turns.id,
                    CASE
                      WHEN sessions.review_workdir IS NULL
                        OR sessions.review_workdir = sessions.cwd
                      THEN COALESCE(
                        (SELECT previous.repository_root
                         FROM task_review_baselines AS previous
                         WHERE previous.session_id = sessions.id
                         ORDER BY previous.captured_at DESC LIMIT 1),
                        sessions.review_workdir,
                        sessions.cwd
                      )
                      ELSE sessions.review_workdir
                    END,
                    turns.started_at,
                    (SELECT MIN(events.occurred_at) FROM events
                     WHERE events.turn_id = turns.id AND events.type = 'tool.started'),
                    CASE WHEN COALESCE(sessions.review_workdir, sessions.cwd) IS NULL THEN 0
                         ELSE (SELECT COUNT(*) FROM sessions AS peer
                               WHERE peer.id != sessions.id
                                 AND COALESCE(peer.review_workdir, peer.cwd) =
                                     COALESCE(sessions.review_workdir, sessions.cwd)
                                 AND peer.exec_state NOT IN
                                     ('idle', 'response_finished', 'failed'))
                    END
             FROM turns
             JOIN sessions ON sessions.id = turns.session_id
             WHERE turns.state = 'running'
               AND sessions.exec_state NOT IN ('idle', 'response_finished', 'failed')
               AND turns.id = (
                 SELECT current_turn.id FROM turns AS current_turn
                 WHERE current_turn.session_id = sessions.id
                 ORDER BY current_turn.ordinal DESC LIMIT 1
               )
               AND NOT EXISTS (
                 SELECT 1 FROM task_review_baselines
                 WHERE task_review_baselines.turn_id = turns.id
               )
             ORDER BY turns.started_at DESC
             LIMIT ?1",
        )
        .map_err(storage_error)?;
    let candidates = statement
        .query_map([i64::try_from(limit).unwrap_or(16)], |row| {
            Ok(ReviewBaselineCandidate {
                session_id: row.get(0)?,
                turn_id: row.get(1)?,
                working_directory: row.get::<_, Option<String>>(2)?.map(PathBuf::from),
                turn_started_at: from_i64(row.get(3)?),
                first_tool_at: row.get::<_, Option<i64>>(4)?.map(from_i64),
                concurrent_active_sessions: u32::try_from(row.get::<_, i64>(5)?)
                    .unwrap_or(u32::MAX),
            })
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    Ok(candidates)
}

fn write_review_baseline(
    connection: &Connection,
    baseline: &ReviewBaselineInput,
) -> Result<bool, StoreError> {
    if baseline.session_id.is_empty()
        || baseline.session_id.len() > 256
        || baseline.turn_id.is_empty()
        || baseline.turn_id.len() > 256
        || !baseline.repository_root.is_absolute()
        || baseline.repository_identity.len() != 64
        || !baseline
            .repository_identity
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || !matches!(
            baseline.worktree_kind.as_str(),
            "primary" | "linked" | "unknown"
        )
    {
        return Err(StoreError::Storage(
            "invalid local Review baseline".to_owned(),
        ));
    }
    let inserted = connection
        .execute(
            "INSERT OR IGNORE INTO task_review_baselines (
               turn_id, session_id, repository_root, repository_identity,
               branch, head, worktree_kind, dirty, changed_files,
               staged_files, unstaged_files, untracked_files,
               insertions, deletions, binary_files, turn_started_at,
               first_tool_at, captured_at
             ) VALUES (
               ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
               ?10, ?11, ?12, ?13, ?14, ?15, ?16,
               COALESCE((SELECT MIN(events.occurred_at) FROM events
                         WHERE events.turn_id = ?1 AND events.type = 'tool.started'), ?17),
               ?18
             )",
            params![
                baseline.turn_id,
                baseline.session_id,
                baseline.repository_root.to_string_lossy(),
                baseline.repository_identity,
                baseline.branch,
                baseline.head,
                baseline.worktree_kind,
                i64::from(baseline.dirty),
                to_i64(baseline.changed_files),
                to_i64(baseline.staged_files),
                to_i64(baseline.unstaged_files),
                to_i64(baseline.untracked_files),
                baseline.insertions.map(to_i64),
                baseline.deletions.map(to_i64),
                baseline.binary_files.map(to_i64),
                to_i64(baseline.turn_started_at),
                baseline.first_tool_at.map(to_i64),
                to_i64(baseline.captured_at),
            ],
        )
        .map_err(storage_error)?
        == 1;
    if inserted {
        connection
            .execute(
                "UPDATE sessions
                 SET review_workdir = ?2
                 WHERE id = ?1
                   AND (review_workdir IS NULL OR review_workdir = cwd)",
                params![
                    baseline.session_id,
                    baseline.repository_root.to_string_lossy()
                ],
            )
            .map_err(storage_error)?;
    }
    Ok(inserted)
}

fn read_review_baseline(
    connection: &Connection,
    turn_id: &str,
) -> Result<Option<ReviewBaselineRecord>, StoreError> {
    connection
        .query_row(
            "SELECT session_id, repository_root, repository_identity,
                    branch, head, worktree_kind, dirty, changed_files,
                    staged_files, unstaged_files, untracked_files,
                    insertions, deletions, binary_files,
                    turn_started_at,
                    COALESCE(
                      first_tool_at,
                      (SELECT MIN(events.occurred_at) FROM events
                       WHERE events.turn_id = task_review_baselines.turn_id
                         AND events.type = 'tool.started')
                    ),
                    captured_at
             FROM task_review_baselines WHERE turn_id = ?1",
            [turn_id],
            |row| {
                Ok(ReviewBaselineRecord {
                    session_id: row.get(0)?,
                    turn_id: turn_id.to_owned(),
                    repository_root: PathBuf::from(row.get::<_, String>(1)?),
                    repository_identity: row.get(2)?,
                    branch: row.get(3)?,
                    head: row.get(4)?,
                    worktree_kind: row.get(5)?,
                    dirty: row.get::<_, i64>(6)? != 0,
                    changed_files: from_i64(row.get(7)?),
                    staged_files: from_i64(row.get(8)?),
                    unstaged_files: from_i64(row.get(9)?),
                    untracked_files: from_i64(row.get(10)?),
                    insertions: row.get::<_, Option<i64>>(11)?.map(from_i64),
                    deletions: row.get::<_, Option<i64>>(12)?.map(from_i64),
                    binary_files: row.get::<_, Option<i64>>(13)?.map(from_i64),
                    turn_started_at: from_i64(row.get(14)?),
                    first_tool_at: row.get::<_, Option<i64>>(15)?.map(from_i64),
                    captured_at: from_i64(row.get(16)?),
                })
            },
        )
        .optional()
        .map_err(storage_error)
}

fn create_task_checkpoint(
    connection: &Connection,
    input: &TaskCheckpointInput,
) -> Result<TaskCheckpointRecord, StoreError> {
    let valid_digest = |value: &str, length: usize| {
        value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    };
    let label_valid = input.label.as_ref().is_none_or(|label| {
        !label.trim().is_empty()
            && label.chars().count() <= 80
            && !label.chars().any(char::is_control)
    });
    let validation = serde_json::from_str::<Value>(&input.validation_json)
        .map_err(|_| StoreError::Storage("checkpoint validation JSON is invalid".to_owned()))?;
    if Uuid::parse_str(&input.id).is_err()
        || input.session_id.is_empty()
        || input.session_id.len() > 256
        || input.turn_id.is_empty()
        || input.turn_id.len() > 256
        || !label_valid
        || !matches!(input.kind.as_str(), "metadata" | "git_snapshot")
        || !matches!(input.provider.as_str(), "claude" | "codex")
        || input.provider_session_id.is_empty()
        || input.provider_session_id.len() > 256
        || !matches!(
            input.provider_resume_capability.as_str(),
            "exact_conversation" | "terminal" | "app_only" | "unsupported"
        )
        || input
            .repository_root
            .as_ref()
            .is_some_and(|root| !root.is_absolute())
        || input
            .repository_identity
            .as_ref()
            .is_some_and(|value| !valid_digest(value, 64))
        || input
            .head
            .as_ref()
            .is_some_and(|value| !valid_digest(value, 40))
        || input
            .git_object_id
            .as_ref()
            .is_some_and(|value| !valid_digest(value, 40))
        || input
            .patch_digest
            .as_ref()
            .is_some_and(|value| !valid_digest(value, 64))
        || input.git_ref.as_ref().is_some_and(|value| {
            !value.starts_with("refs/actrealm/checkpoints/")
                || value.len() > 256
                || value.chars().any(char::is_control)
        })
        || input.branch.as_ref().is_some_and(|value| {
            value.is_empty() || value.len() > 160 || value.chars().any(char::is_control)
        })
        || input.validation_json.len() > 16 * 1_024
        || !validation.is_array()
    {
        return Err(StoreError::Storage(
            "invalid local task checkpoint".to_owned(),
        ));
    }
    let belongs_to_session: bool = connection
        .query_row(
            "SELECT EXISTS(
               SELECT 1 FROM turns
               WHERE id = ?1 AND session_id = ?2
             )",
            params![input.turn_id, input.session_id],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let checkpoint_count: u64 = connection
        .query_row(
            "SELECT COUNT(*) FROM task_checkpoints WHERE session_id = ?1",
            [&input.session_id],
            |row| Ok(from_i64(row.get(0)?)),
        )
        .map_err(storage_error)?;
    if !belongs_to_session || checkpoint_count >= 100 {
        return Err(StoreError::Storage(
            "checkpoint session is unavailable or full".to_owned(),
        ));
    }
    connection
        .execute(
            "INSERT INTO task_checkpoints(
               id, session_id, turn_id, label, kind, provider,
               provider_session_id, provider_resume_capability,
               repository_root, repository_identity, branch, head,
               worktree_kind, dirty, changed_files, staged_files,
               unstaged_files, untracked_files, git_object_id, git_ref,
               patch_digest, validation_json, review_baseline_captured_at,
               created_at
             ) VALUES (
               ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
               ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24
             )",
            params![
                input.id,
                input.session_id,
                input.turn_id,
                input.label,
                input.kind,
                input.provider,
                input.provider_session_id,
                input.provider_resume_capability,
                input
                    .repository_root
                    .as_ref()
                    .map(|path| path.to_string_lossy()),
                input.repository_identity,
                input.branch,
                input.head,
                input.worktree_kind,
                input.dirty.map(i64::from),
                input.changed_files.map(to_i64),
                input.staged_files.map(to_i64),
                input.unstaged_files.map(to_i64),
                input.untracked_files.map(to_i64),
                input.git_object_id,
                input.git_ref,
                input.patch_digest,
                input.validation_json,
                input.review_baseline_captured_at.map(to_i64),
                to_i64(input.created_at),
            ],
        )
        .map_err(storage_error)?;
    read_task_checkpoint(connection, &input.id)?
        .ok_or_else(|| StoreError::Storage("checkpoint was not readable after insert".to_owned()))
}

fn task_checkpoint_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskCheckpointRecord> {
    Ok(TaskCheckpointRecord {
        id: row.get(0)?,
        session_id: row.get(1)?,
        turn_id: row.get(2)?,
        label: row.get(3)?,
        kind: row.get(4)?,
        provider: row.get(5)?,
        provider_session_id: row.get(6)?,
        provider_resume_capability: row.get(7)?,
        repository_root: row.get::<_, Option<String>>(8)?.map(PathBuf::from),
        repository_identity: row.get(9)?,
        branch: row.get(10)?,
        head: row.get(11)?,
        worktree_kind: row.get(12)?,
        dirty: row.get::<_, Option<i64>>(13)?.map(|value| value != 0),
        changed_files: row.get::<_, Option<i64>>(14)?.map(from_i64),
        staged_files: row.get::<_, Option<i64>>(15)?.map(from_i64),
        unstaged_files: row.get::<_, Option<i64>>(16)?.map(from_i64),
        untracked_files: row.get::<_, Option<i64>>(17)?.map(from_i64),
        git_object_id: row.get(18)?,
        git_ref: row.get(19)?,
        patch_digest: row.get(20)?,
        validation_json: row.get(21)?,
        review_baseline_captured_at: row.get::<_, Option<i64>>(22)?.map(from_i64),
        created_at: from_i64(row.get(23)?),
    })
}

const TASK_CHECKPOINT_COLUMNS: &str = "id, session_id, turn_id, label, kind, provider,
     provider_session_id, provider_resume_capability,
     repository_root, repository_identity, branch, head,
     worktree_kind, dirty, changed_files, staged_files,
     unstaged_files, untracked_files, git_object_id, git_ref,
     patch_digest, validation_json, review_baseline_captured_at, created_at";

fn read_task_checkpoints(
    connection: &Connection,
    session_id: &str,
) -> Result<Vec<TaskCheckpointRecord>, StoreError> {
    let query = format!(
        "SELECT {TASK_CHECKPOINT_COLUMNS}
         FROM task_checkpoints WHERE session_id = ?1
         ORDER BY created_at DESC LIMIT 100"
    );
    let mut statement = connection.prepare(&query).map_err(storage_error)?;
    let checkpoints = statement
        .query_map([session_id], task_checkpoint_from_row)
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    Ok(checkpoints)
}

fn read_task_checkpoint(
    connection: &Connection,
    checkpoint_id: &str,
) -> Result<Option<TaskCheckpointRecord>, StoreError> {
    let query = format!(
        "SELECT {TASK_CHECKPOINT_COLUMNS}
         FROM task_checkpoints WHERE id = ?1"
    );
    connection
        .query_row(&query, [checkpoint_id], task_checkpoint_from_row)
        .optional()
        .map_err(storage_error)
}

fn delete_task_checkpoint(
    connection: &Connection,
    checkpoint_id: &str,
) -> Result<bool, StoreError> {
    connection
        .execute(
            "DELETE FROM task_checkpoints WHERE id = ?1",
            [checkpoint_id],
        )
        .map(|changed| changed == 1)
        .map_err(storage_error)
}

type UsageAttributionKey = (String, String);

#[derive(Debug)]
struct UsageAttributionLedgerRow {
    provider: String,
    provider_session_id: String,
    model: Option<String>,
    token_total: u64,
    captured_at: Option<u64>,
}

#[derive(Debug, Default)]
struct UsageAttributionMetadata {
    project_id: Option<String>,
    project_label: Option<String>,
    parent_provider_session_id: Option<String>,
}

#[derive(Debug)]
struct UsageAttributionTask {
    session_id: String,
    provider: String,
    project: Option<String>,
    title: Option<String>,
    model: Option<String>,
}

#[derive(Default)]
struct UsageProjectAggregate {
    label: String,
    total: u64,
    task_ids: HashSet<String>,
    session_count: u64,
    captured_at: Option<u64>,
}

#[derive(Default)]
struct TokenAttributionResult {
    project_attributed_tokens: u64,
    task_attributed_tokens: u64,
    project_totals: Vec<TokenUsageProjectTotal>,
    task_totals: Vec<TokenUsageTaskTotal>,
}

fn usage_attribution_lineage(
    provider: &str,
    provider_session_id: &str,
    metadata: &HashMap<UsageAttributionKey, UsageAttributionMetadata>,
) -> Vec<UsageAttributionKey> {
    const MAX_DEPTH: usize = 8;
    let mut output = Vec::new();
    let mut visited = HashSet::new();
    let mut current = provider_session_id.to_owned();
    for _ in 0..=MAX_DEPTH {
        let key = (provider.to_owned(), current.clone());
        if !visited.insert(key.clone()) {
            break;
        }
        output.push(key.clone());
        let Some(parent) = metadata
            .get(&key)
            .and_then(|value| value.parent_provider_session_id.as_ref())
        else {
            break;
        };
        current.clone_from(parent);
    }
    output
}

fn attribution_coverage_basis_points(attributed: u64, total: u64) -> u64 {
    if total == 0 {
        return 0;
    }
    attributed
        .min(total)
        .saturating_mul(10_000)
        .checked_div(total)
        .unwrap_or_default()
        .min(10_000)
}

fn read_token_attribution(
    connection: &Connection,
    total_tokens: u64,
) -> Result<TokenAttributionResult, StoreError> {
    let mut ledger_statement = connection
        .prepare(
            "WITH session_ledger AS (
               SELECT provider, provider_session_id, MAX(model) AS model,
                      SUM(token_total) AS token_total, MAX(captured_at) AS captured_at
               FROM token_usage_session_days
               GROUP BY provider, provider_session_id
               UNION ALL
               SELECT live.provider, live.provider_session_id, live.model,
                      live.token_total, live.captured_at
               FROM session_usage AS live
               WHERE live.token_total IS NOT NULL
                 AND NOT EXISTS (
                   SELECT 1 FROM token_usage_session_days AS detail
                   WHERE detail.provider = live.provider
                     AND detail.provider_session_id = live.provider_session_id
                 )
             )
             SELECT provider, provider_session_id, model, token_total, captured_at
             FROM session_ledger",
        )
        .map_err(storage_error)?;
    let ledger = ledger_statement
        .query_map([], |row| {
            Ok(UsageAttributionLedgerRow {
                provider: row.get(0)?,
                provider_session_id: row.get(1)?,
                model: row.get(2)?,
                token_total: from_i64(row.get(3)?),
                captured_at: row.get::<_, Option<i64>>(4)?.map(from_i64),
            })
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;

    let mut metadata_statement = connection
        .prepare(
            "SELECT provider, provider_session_id, project_id, project_label,
                    parent_provider_session_id
             FROM token_usage_cursors",
        )
        .map_err(storage_error)?;
    let metadata = metadata_statement
        .query_map([], |row| {
            Ok((
                (row.get::<_, String>(0)?, row.get::<_, String>(1)?),
                UsageAttributionMetadata {
                    project_id: row.get(2)?,
                    project_label: row.get(3)?,
                    parent_provider_session_id: row.get(4)?,
                },
            ))
        })
        .map_err(storage_error)?
        .collect::<Result<HashMap<_, _>, _>>()
        .map_err(storage_error)?;

    let mut task_statement = connection
        .prepare(
            "SELECT sessions.id, sessions.provider, sessions.provider_session_id,
                    sessions.project, sessions.title,
                    COALESCE(
                      CASE WHEN usage.captured_at >= COALESCE(
                        (SELECT started_at FROM turns WHERE turns.session_id = sessions.id
                         ORDER BY ordinal DESC LIMIT 1), sessions.started_at)
                      THEN usage.model END,
                      sessions.model, usage.model)
             FROM sessions
             LEFT JOIN session_usage AS usage ON usage.provider = sessions.provider
               AND usage.provider_session_id = sessions.provider_session_id",
        )
        .map_err(storage_error)?;
    let tasks = task_statement
        .query_map([], |row| {
            let provider = row.get::<_, String>(1)?;
            let provider_session_id = row.get::<_, String>(2)?;
            Ok((
                (provider.clone(), provider_session_id),
                UsageAttributionTask {
                    session_id: row.get(0)?,
                    provider,
                    project: row
                        .get::<_, Option<String>>(3)?
                        .filter(|value| valid_usage_project_label(value)),
                    title: row.get(4)?,
                    model: row.get(5)?,
                },
            ))
        })
        .map_err(storage_error)?
        .collect::<Result<HashMap<_, _>, _>>()
        .map_err(storage_error)?;

    let mut project_aggregates = HashMap::<String, UsageProjectAggregate>::new();
    let mut task_aggregates = HashMap::<String, TokenUsageTaskTotal>::new();
    let mut unique_project_ids_by_label = HashMap::<String, Option<String>>::new();
    for value in metadata.values() {
        let (Some(project_id), Some(project_label)) =
            (value.project_id.as_ref(), value.project_label.as_ref())
        else {
            continue;
        };
        unique_project_ids_by_label
            .entry(project_label.to_lowercase())
            .and_modify(|existing| {
                if existing.as_ref() != Some(project_id) {
                    *existing = None;
                }
            })
            .or_insert_with(|| Some(project_id.clone()));
    }
    let mut project_attributed_tokens = 0_u64;
    let mut task_attributed_tokens = 0_u64;

    for row in ledger {
        let lineage = usage_attribution_lineage(&row.provider, &row.provider_session_id, &metadata);
        let owner_entry = lineage
            .iter()
            .find_map(|key| tasks.get(key).map(|task| (key, task)));
        let owner = owner_entry.map(|(_, task)| task);
        if let Some(owner) = owner {
            task_attributed_tokens = task_attributed_tokens.saturating_add(row.token_total);
            let task = task_aggregates
                .entry(owner.session_id.clone())
                .or_insert_with(|| TokenUsageTaskTotal {
                    session_id: owner.session_id.clone(),
                    provider: owner.provider.clone(),
                    project: owner.project.clone(),
                    title: owner.title.clone(),
                    model: owner.model.clone().or_else(|| row.model.clone()),
                    total: 0,
                    captured_at: None,
                });
            task.total = task.total.saturating_add(row.token_total);
            task.captured_at = task.captured_at.max(row.captured_at);
            if task.model.is_none() {
                task.model.clone_from(&row.model);
            }
        }

        let owner_project = owner_entry.and_then(|(owner_key, owner)| {
            let label = owner.project.as_ref()?;
            let project_id = metadata
                .get(owner_key)
                .and_then(|value| value.project_id.clone())
                .or_else(|| {
                    unique_project_ids_by_label
                        .get(&label.to_lowercase())
                        .cloned()
                        .flatten()
                })
                .unwrap_or_else(|| format!("task-label:{}", label.to_lowercase()));
            Some((project_id, label.clone()))
        });
        let metadata_project = lineage.iter().find_map(|key| {
            let value = metadata.get(key)?;
            Some((value.project_id.clone()?, value.project_label.clone()?))
        });
        if let Some((project_id, project_label)) = owner_project.or(metadata_project) {
            if valid_usage_project_label(&project_label) {
                project_attributed_tokens =
                    project_attributed_tokens.saturating_add(row.token_total);
                let project = project_aggregates.entry(project_id).or_default();
                if project.label.is_empty() || project_label < project.label {
                    project.label = project_label;
                }
                project.total = project.total.saturating_add(row.token_total);
                project.session_count = project.session_count.saturating_add(1);
                project.captured_at = project.captured_at.max(row.captured_at);
                if let Some(owner) = owner {
                    project.task_ids.insert(owner.session_id.clone());
                }
            }
        }
    }

    let mut project_totals = project_aggregates
        .into_values()
        .map(|project| TokenUsageProjectTotal {
            project: project.label,
            total: project.total,
            task_count: u64::try_from(project.task_ids.len()).unwrap_or(u64::MAX),
            session_count: project.session_count,
            captured_at: project.captured_at,
        })
        .collect::<Vec<_>>();
    project_totals.sort_by(|left, right| {
        right
            .total
            .cmp(&left.total)
            .then_with(|| left.project.cmp(&right.project))
    });
    project_totals.truncate(20);

    let mut task_totals = task_aggregates.into_values().collect::<Vec<_>>();
    task_totals.sort_by(|left, right| {
        right
            .total
            .cmp(&left.total)
            .then_with(|| left.session_id.cmp(&right.session_id))
    });
    task_totals.truncate(20);

    Ok(TokenAttributionResult {
        project_attributed_tokens: project_attributed_tokens.min(total_tokens),
        task_attributed_tokens: task_attributed_tokens.min(total_tokens),
        project_totals,
        task_totals,
    })
}

fn read_token_usage_decision(
    connection: &Connection,
    now: u64,
    threshold_tokens_per_minute: Option<u64>,
) -> Result<TokenUsageDecisionSummary, StoreError> {
    const CURRENT_WINDOW_MS: u64 = 5 * 60 * 1_000;
    const BASELINE_WINDOW_MS: u64 = 30 * 60 * 1_000;

    let (total_tokens, captured_at) = connection
        .query_row(
            "SELECT COALESCE(SUM(token_total), 0), MAX(captured_at)
             FROM token_usage_daily",
            [],
            |row| {
                Ok((
                    from_i64(row.get(0)?),
                    row.get::<_, Option<i64>>(1)?.map(from_i64),
                ))
            },
        )
        .map_err(storage_error)?;
    let attribution = read_token_attribution(connection, total_tokens)?;
    let project_attributed_tokens = attribution.project_attributed_tokens;
    let project_unattributed_tokens = total_tokens.saturating_sub(project_attributed_tokens);
    let project_attribution_coverage_basis_points =
        attribution_coverage_basis_points(project_attributed_tokens, total_tokens);
    let task_attributed_tokens = attribution.task_attributed_tokens;
    let task_unattributed_tokens = total_tokens.saturating_sub(task_attributed_tokens);
    let task_attribution_coverage_basis_points =
        attribution_coverage_basis_points(task_attributed_tokens, total_tokens);
    // Schema-v1 aliases remain task-scoped for older clients. New clients use
    // the explicit project/task fields and never conflate the two layers.
    let attributed_tokens = task_attributed_tokens;
    let unattributed_tokens = task_unattributed_tokens;
    let attribution_coverage_basis_points = task_attribution_coverage_basis_points;
    let project_totals = attribution.project_totals;
    let task_totals = attribution.task_totals;

    #[derive(Default)]
    struct BurnSamples {
        turn_id: String,
        provider: String,
        project: Option<String>,
        title: Option<String>,
        samples: Vec<(u64, u64)>,
    }
    let sample_floor = now.saturating_sub(CURRENT_WINDOW_MS + BASELINE_WINDOW_MS);
    let mut sample_statement = connection
        .prepare(
            "SELECT samples.session_id, samples.turn_id, sessions.provider,
                    sessions.project, sessions.title,
                    samples.sampled_at, samples.token_total
             FROM token_usage_rate_samples AS samples
             JOIN sessions ON sessions.id = samples.session_id
             JOIN turns ON turns.id = samples.turn_id
             WHERE samples.sampled_at >= ?1
               AND turns.state = 'running'
               AND sessions.exec_state NOT IN ('idle', 'response_finished', 'failed')
             ORDER BY samples.session_id ASC, samples.sampled_at ASC",
        )
        .map_err(storage_error)?;
    let sample_rows = sample_statement
        .query_map([to_i64(sample_floor)], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                from_i64(row.get(5)?),
                from_i64(row.get(6)?),
            ))
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    let mut grouped = HashMap::<String, BurnSamples>::new();
    for (session_id, turn_id, provider, project, title, sampled_at, token_total) in sample_rows {
        let samples = grouped.entry(session_id).or_insert_with(|| BurnSamples {
            turn_id,
            provider,
            project: project.filter(|project| valid_usage_project_label(project)),
            title,
            samples: Vec::new(),
        });
        samples.samples.push((sampled_at, token_total));
    }
    let current_start = now.saturating_sub(CURRENT_WINDOW_MS);
    let mut burn_rates = grouped
        .into_iter()
        .map(|(session_id, grouped)| {
            let current = grouped
                .samples
                .iter()
                .copied()
                .filter(|(sampled_at, _)| *sampled_at >= current_start)
                .collect::<Vec<_>>();
            let baseline = grouped
                .samples
                .iter()
                .copied()
                .filter(|(sampled_at, _)| *sampled_at < current_start)
                .collect::<Vec<_>>();
            let current_rate = usage_sample_rate(&current);
            let baseline_rate = usage_sample_rate(&baseline).map(|(_, _, rate)| rate);
            let (window_seconds, token_delta, tokens_per_minute) =
                current_rate.unwrap_or((0, 0, 0));
            let ratio_basis_points =
                baseline_rate
                    .filter(|baseline| *baseline > 0)
                    .map(|baseline| {
                        tokens_per_minute
                            .saturating_mul(10_000)
                            .checked_div(baseline)
                            .unwrap_or(u64::MAX)
                    });
            let elevated =
                token_delta >= 10_000 && ratio_basis_points.is_some_and(|ratio| ratio >= 30_000);
            let threshold_exceeded = threshold_tokens_per_minute
                .is_some_and(|threshold| current_rate.is_some() && tokens_per_minute >= threshold);
            TokenUsageBurnRate {
                session_id,
                turn_id: grouped.turn_id,
                provider: grouped.provider,
                project: grouped.project,
                title: grouped.title,
                window_seconds,
                token_delta,
                tokens_per_minute,
                sample_count: u64::try_from(current.len()).unwrap_or(u64::MAX),
                baseline_tokens_per_minute: baseline_rate,
                ratio_basis_points,
                state: if current_rate.is_none() {
                    "collecting".to_owned()
                } else if elevated {
                    "elevated".to_owned()
                } else {
                    "normal".to_owned()
                },
                captured_at: current
                    .last()
                    .map(|(sampled_at, _)| *sampled_at)
                    .unwrap_or(now),
                threshold_exceeded,
            }
        })
        .collect::<Vec<_>>();
    burn_rates.sort_by(|left, right| {
        right
            .tokens_per_minute
            .cmp(&left.tokens_per_minute)
            .then_with(|| left.session_id.cmp(&right.session_id))
    });
    burn_rates.truncate(16);

    let freshness = match captured_at.map(|captured_at| now.saturating_sub(captured_at)) {
        None => "unavailable",
        Some(age) if age <= 2 * 60 * 1_000 => "live",
        Some(age) if age <= 15 * 60 * 1_000 => "delayed",
        Some(_) => "stale",
    };
    Ok(TokenUsageDecisionSummary {
        schema_version: 2,
        source: "runtime:canonical_session_ledger".to_owned(),
        generated_at: now,
        freshness: freshness.to_owned(),
        captured_at,
        total_tokens,
        attributed_tokens,
        unattributed_tokens,
        attribution_coverage_basis_points,
        project_attributed_tokens,
        project_unattributed_tokens,
        project_attribution_coverage_basis_points,
        task_attributed_tokens,
        task_unattributed_tokens,
        task_attribution_coverage_basis_points,
        project_totals,
        task_totals,
        burn_rates,
        threshold_tokens_per_minute,
    })
}

fn usage_sample_rate(samples: &[(u64, u64)]) -> Option<(u64, u64, u64)> {
    let (first_at, first_total) = samples.first().copied()?;
    let (last_at, last_total) = samples.last().copied()?;
    let elapsed_ms = last_at.checked_sub(first_at)?;
    if samples.len() < 2 || elapsed_ms < 5_000 || last_total < first_total {
        return None;
    }
    let token_delta = last_total.saturating_sub(first_total);
    let tokens_per_minute = token_delta
        .saturating_mul(60_000)
        .checked_div(elapsed_ms)
        .unwrap_or(u64::MAX);
    Some((elapsed_ms / 1_000, token_delta, tokens_per_minute))
}

fn valid_usage_project_label(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 96
        && !value.chars().any(char::is_control)
        && !matches!(value.to_ascii_lowercase().as_str(), "unknown" | "untitled")
        && Uuid::parse_str(value).is_err()
}

fn read_timeline(
    connection: &Connection,
    session_id: &str,
    options: TimelineReadOptions,
) -> Result<Option<TimelinePage>, StoreError> {
    let session_exists = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)",
            [session_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(storage_error)?;
    if !session_exists {
        return Ok(None);
    }
    let row_limit = options.limit.saturating_add(1);
    let order = if options.latest { "DESC" } else { "ASC" };
    let current_turn_filter = if options.current_turn_only {
        "AND events.turn_id = (
           SELECT turns.id FROM turns
           WHERE turns.session_id = events.session_id
           ORDER BY turns.ordinal DESC LIMIT 1
         )"
    } else {
        ""
    };
    let query = format!(
        "SELECT events.id, events.provider, events.type, events.tool_name,
                    events.tool_category, events.tool_target,
                    events.occurred_at, events.ingest_seq,
                    'provider_unsupported',
                    events.timeline_risk, events.plan_step_count, events.turn_id,
                    events.attention_id, events.tool_call_id,
                    events.source_version, events.validation_status, events.derived
             FROM events
             WHERE events.session_id = ?1
               AND events.ingest_seq > ?2
               AND events.ingest_seq < ?3
               {current_turn_filter}
               AND events.provider IN ('claude', 'codex')
               AND events.type IN (
                 'session.started', 'session.ended', 'session.compacting',
                 'prompt.submitted',
                 'tool.started', 'tool.finished', 'tool.failed',
                 'approval.requested', 'approval.resolved',
                 'question.requested', 'elicitation.requested',
                 'subagent.started', 'subagent.stopped',
                 'task.created', 'task.completed', 'plan.updated',
                 'turn.stopped', 'turn.interrupted', 'turn.failed'
               )
             ORDER BY events.ingest_seq {order}
             LIMIT ?4"
    );
    let mut statement = connection.prepare(&query).map_err(storage_error)?;
    let rows = statement
        .query_map(
            params![
                session_id,
                to_i64(options.after_ingest_sequence.unwrap_or_default()),
                options
                    .before_ingest_sequence
                    .map(to_i64)
                    .unwrap_or(i64::MAX),
                i64::try_from(row_limit).unwrap_or(i64::MAX),
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, Option<String>>(14)?,
                    row.get::<_, Option<String>>(15)?,
                    row.get::<_, i64>(16)?,
                ))
            },
        )
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;

    let has_more = rows.len() > options.limit;
    let mut events = rows
        .into_iter()
        .take(options.limit)
        .filter_map(
            |(
                event_id,
                provider,
                event_type,
                tool_name,
                tool_category,
                tool_target,
                occurred_at,
                ingest_sequence,
                context_availability,
                timeline_risk,
                plan_step_count,
                turn_id,
                outbox_id,
                tool_call_id,
                source_version,
                validation_status,
                derived,
            )| {
                let kind = timeline_event_fact(&event_type)?;
                let (phase, status) = timeline_phase_and_status(kind);
                let tool_name = if matches!(
                    kind,
                    TimelineEventKind::ToolStarted
                        | TimelineEventKind::ToolCompleted
                        | TimelineEventKind::ToolFailed
                ) {
                    tool_name
                        .as_deref()
                        .map(sanitized_tool_name)
                        .filter(|name| name != "Unknown")
                } else {
                    None
                };
                let tool_category =
                    tool_category.filter(|category| workflow_category_is_allowlisted(category));
                let risk_level = (kind == TimelineEventKind::ApprovalRequested)
                    .then(|| parse_timeline_risk(timeline_risk.as_deref().unwrap_or("unknown")));
                let plan_step_count = (kind == TimelineEventKind::PlanUpdated).then(|| {
                    u32::try_from(plan_step_count.unwrap_or_default().clamp(0, 64))
                        .unwrap_or_default()
                });
                Some(TimelineEventRecord {
                    schema_version: 1,
                    event_id,
                    provider,
                    kind,
                    tool_name,
                    tool_category,
                    tool_target: options
                        .include_local_tool_target
                        .then_some(tool_target)
                        .flatten(),
                    tool_call_id: options
                        .include_local_tool_target
                        .then_some(tool_call_id)
                        .flatten(),
                    source_version,
                    validation_status,
                    phase,
                    status,
                    confidence: if derived == 0 {
                        TimelineEventConfidence::ProviderFact
                    } else {
                        TimelineEventConfidence::RuntimeDerived
                    },
                    risk_level,
                    plan_step_count,
                    turn_id,
                    outbox_id,
                    occurred_at: from_i64(occurred_at),
                    ingest_sequence: from_i64(ingest_sequence),
                    context_availability: parse_context_availability(&context_availability),
                })
            },
        )
        .collect::<Vec<_>>();
    if options.latest {
        events.reverse();
    }
    let next_after_ingest_sequence = events.last().map(|event| event.ingest_sequence);
    Ok(Some(TimelinePage {
        events,
        next_after_ingest_sequence,
        has_more,
    }))
}

fn timeline_event_fact(event_type: &str) -> Option<TimelineEventKind> {
    match event_type {
        "session.started" => Some(TimelineEventKind::SessionStarted),
        "session.ended" => Some(TimelineEventKind::SessionEnded),
        "session.compacting" => Some(TimelineEventKind::SessionCompacting),
        "prompt.submitted" => Some(TimelineEventKind::TurnStarted),
        "turn.stopped" => Some(TimelineEventKind::TurnCompleted),
        "turn.interrupted" => Some(TimelineEventKind::TurnInterrupted),
        "turn.failed" => Some(TimelineEventKind::TurnFailed),
        "tool.started" => Some(TimelineEventKind::ToolStarted),
        "tool.finished" => Some(TimelineEventKind::ToolCompleted),
        "tool.failed" => Some(TimelineEventKind::ToolFailed),
        "approval.requested" => Some(TimelineEventKind::ApprovalRequested),
        "approval.resolved" => Some(TimelineEventKind::ApprovalResolved),
        "question.requested" => Some(TimelineEventKind::QuestionRequested),
        "elicitation.requested" => Some(TimelineEventKind::ElicitationRequested),
        "subagent.started" => Some(TimelineEventKind::SubagentStarted),
        "subagent.stopped" => Some(TimelineEventKind::SubagentCompleted),
        "task.created" => Some(TimelineEventKind::TaskCreated),
        "task.completed" => Some(TimelineEventKind::TaskCompleted),
        "plan.updated" => Some(TimelineEventKind::PlanUpdated),
        _ => None,
    }
}

fn timeline_phase_and_status(kind: TimelineEventKind) -> (TimelineEventPhase, TimelineEventStatus) {
    use TimelineEventKind as Kind;
    use TimelineEventPhase as Phase;
    use TimelineEventStatus as Status;
    match kind {
        Kind::SessionStarted => (Phase::Session, Status::Started),
        Kind::SessionEnded => (Phase::Session, Status::Completed),
        Kind::SessionCompacting => (Phase::Session, Status::Running),
        Kind::TurnStarted => (Phase::Turn, Status::Started),
        Kind::TurnCompleted => (Phase::Turn, Status::Completed),
        Kind::TurnInterrupted => (Phase::Turn, Status::Interrupted),
        Kind::TurnFailed => (Phase::Turn, Status::Failed),
        Kind::ToolStarted => (Phase::Tool, Status::Started),
        Kind::ToolCompleted => (Phase::Tool, Status::Completed),
        Kind::ToolFailed => (Phase::Tool, Status::Failed),
        Kind::ApprovalRequested | Kind::QuestionRequested | Kind::ElicitationRequested => {
            (Phase::Attention, Status::Requested)
        }
        Kind::ApprovalResolved => (Phase::Attention, Status::Resolved),
        Kind::SubagentStarted => (Phase::Subagent, Status::Started),
        Kind::SubagentCompleted => (Phase::Subagent, Status::Completed),
        Kind::TaskCreated => (Phase::Plan, Status::Started),
        Kind::TaskCompleted => (Phase::Plan, Status::Completed),
        Kind::PlanUpdated => (Phase::Plan, Status::Updated),
    }
}

fn timeline_risk_level(classification: OperationClassification) -> TimelineRiskLevel {
    parse_timeline_risk(classification.risk)
}

fn timeline_risk_value(risk: TimelineRiskLevel) -> &'static str {
    match risk {
        TimelineRiskLevel::Low => "low",
        TimelineRiskLevel::Medium => "medium",
        TimelineRiskLevel::High => "high",
        TimelineRiskLevel::Unknown => "unknown",
    }
}

fn parse_timeline_risk(value: &str) -> TimelineRiskLevel {
    match value {
        "low" => TimelineRiskLevel::Low,
        "med" | "medium" => TimelineRiskLevel::Medium,
        "high" => TimelineRiskLevel::High,
        _ => TimelineRiskLevel::Unknown,
    }
}

fn parse_context_availability(value: &str) -> TimelineContextAvailability {
    match value {
        "available" => TimelineContextAvailability::Available,
        "source_rotated" => TimelineContextAvailability::SourceRotated,
        "provider_unsupported" => TimelineContextAvailability::ProviderUnsupported,
        _ => TimelineContextAvailability::AnchorMissing,
    }
}

fn read_storage_diagnostics(connection: &Connection) -> Result<StorageDiagnostics, StoreError> {
    let schema_version = connection
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .map_err(storage_error)?;
    let integrity = connection
        .query_row("PRAGMA quick_check(1)", [], |row| row.get::<_, String>(0))
        .map_err(storage_error)?;
    Ok(StorageDiagnostics {
        schema_version,
        expected_schema_version: SCHEMA_VERSION,
        integrity,
    })
}

fn read_snapshot(
    connection: &Connection,
    cutoff: Option<u64>,
) -> Result<StoreSnapshot, StoreError> {
    let mut sessions = {
        let mut statement = connection
            .prepare(
                "SELECT sessions.id, sessions.provider, sessions.provider_session_id,
                        sessions.project, sessions.title,
                        sessions.provider_title, sessions.provider_title_source,
                        COALESCE(
                          CASE WHEN usage.captured_at >= COALESCE(
                            (SELECT started_at FROM turns WHERE turns.session_id = sessions.id
                             ORDER BY ordinal DESC LIMIT 1), sessions.started_at)
                          THEN usage.model END,
                          sessions.model, usage.model),
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
                        (SELECT started.tool_name FROM events AS started
                         WHERE started.session_id = sessions.id
                           AND started.type = 'tool.started'
                           AND started.tool_name IS NOT NULL
                           AND NOT EXISTS (
                             SELECT 1 FROM events AS terminal
                             WHERE terminal.session_id = started.session_id
                               AND terminal.ingest_seq > started.ingest_seq
                               AND terminal.type IN ('tool.finished', 'tool.failed')
                               AND (
                                 (started.tool_call_id IS NOT NULL
                                  AND terminal.tool_call_id = started.tool_call_id)
                                 OR
                                 (started.tool_call_id IS NULL
                                  AND terminal.tool_call_id IS NULL
                                  AND terminal.tool_name = started.tool_name)
                               )
                           )
                         ORDER BY started.ingest_seq DESC LIMIT 1),
                        (SELECT started.tool_category FROM events AS started
                         WHERE started.session_id = sessions.id
                           AND started.type = 'tool.started'
                           AND started.tool_name IS NOT NULL
                           AND NOT EXISTS (
                             SELECT 1 FROM events AS terminal
                             WHERE terminal.session_id = started.session_id
                               AND terminal.ingest_seq > started.ingest_seq
                               AND terminal.type IN ('tool.finished', 'tool.failed')
                               AND (
                                 (started.tool_call_id IS NOT NULL
                                  AND terminal.tool_call_id = started.tool_call_id)
                                 OR
                                 (started.tool_call_id IS NULL
                                  AND terminal.tool_call_id IS NULL
                                  AND terminal.tool_name = started.tool_name)
                               )
                           )
                         ORDER BY started.ingest_seq DESC LIMIT 1),
                        COALESCE(
                          (SELECT started.tool_target FROM events AS started
                           WHERE started.session_id = sessions.id
                             AND started.type = 'tool.started'
                             AND started.tool_name IS NOT NULL
                             AND NOT EXISTS (
                               SELECT 1 FROM events AS terminal
                               WHERE terminal.session_id = started.session_id
                                 AND terminal.ingest_seq > started.ingest_seq
                                 AND terminal.type IN ('tool.finished', 'tool.failed')
                                 AND (
                                   (started.tool_call_id IS NOT NULL
                                    AND terminal.tool_call_id = started.tool_call_id)
                                   OR
                                   (started.tool_call_id IS NULL
                                    AND terminal.tool_call_id IS NULL
                                    AND terminal.tool_name = started.tool_name)
                                 )
                             )
                           ORDER BY started.ingest_seq DESC LIMIT 1),
                          sessions.current_target
                        ),
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
                        usage.usage_quality, usage.captured_at,
                        sessions.last_meaningful_activity_at
                 FROM sessions
                 LEFT JOIN session_usage AS usage
                   ON usage.provider = sessions.provider
                  AND usage.provider_session_id = sessions.provider_session_id
                 WHERE ?1 IS NULL
                    OR (
                      sessions.task_hidden_at IS NULL
                      AND (
                        sessions.exec_state NOT IN ('idle', 'response_finished', 'failed')
                        OR (
                            sessions.exec_state IN ('idle', 'response_finished', 'failed')
                            AND sessions.last_meaningful_activity_at IS NOT NULL
                            AND COALESCE(sessions.activity_since, sessions.last_event_at) >= ?1
                        )
                        OR EXISTS (
                            SELECT 1 FROM attention_items
                            WHERE attention_items.session_id = sessions.id
                              AND attention_items.state IN ('open', 'committing', 'decision_sent', 'snoozed')
                        )
                      )
                    )
                 ORDER BY sessions.last_event_at DESC",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([cutoff.map(to_i64)], |row| {
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
                let current_tool_category = if exec_state == "tool_running" {
                    row.get::<_, Option<String>>(27)?
                        .filter(|value| workflow_category_is_allowlisted(value))
                } else {
                    None
                };
                let current_target = if exec_state == "tool_running" {
                    row.get(28)?
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
                    plan_steps: Vec::new(),
                    turn_started_at: row.get::<_, Option<i64>>(14)?.map(from_i64),
                    turn_ended_at: row.get::<_, Option<i64>>(15)?.map(from_i64),
                    token_total: row.get::<_, Option<i64>>(16)?.map(from_i64),
                    context_window_tokens: row.get::<_, Option<i64>>(17)?.map(from_i64),
                    input_tokens: row.get::<_, Option<i64>>(31)?.map(from_i64),
                    output_tokens: row.get::<_, Option<i64>>(32)?.map(from_i64),
                    cache_read_tokens: row.get::<_, Option<i64>>(33)?.map(from_i64),
                    cache_creation_tokens: row.get::<_, Option<i64>>(34)?.map(from_i64),
                    reasoning_tokens: row.get::<_, Option<i64>>(35)?.map(from_i64),
                    last_turn_tokens: row.get::<_, Option<i64>>(36)?.map(from_i64),
                    context_used_tokens: row.get::<_, Option<i64>>(37)?.map(from_i64),
                    context_used_percent: row
                        .get::<_, Option<i64>>(38)?
                        .and_then(|value| u32::try_from(value).ok()),
                    estimated_cost_usd_micros: row.get::<_, Option<i64>>(39)?.map(from_i64),
                    cost_kind: row.get(40)?,
                    pricing_source: row.get(41)?,
                    usage_source: row.get(42)?,
                    usage_quality: row.get(43)?,
                    usage_captured_at: row.get::<_, Option<i64>>(44)?.map(from_i64),
                    permission_mode: row.get(25)?,
                    current_tool,
                    current_tool_category,
                    current_target,
                    active_subagents: row
                        .get::<_, i64>(29)
                        .ok()
                        .and_then(|value| u32::try_from(value).ok())
                        .unwrap_or_default(),
                    subagents: Vec::new(),
                    provider_turn_id: row.get(30)?,
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
                    last_meaningful_activity_at: row.get::<_, Option<i64>>(45)?.map(from_i64),
                })
            })
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        rows
    };
    read_plan_steps_batched(connection, &mut sessions)?;
    read_active_subagents_batched(connection, &mut sessions)?;
    let attention = {
        let mut statement = connection
            .prepare(
                "SELECT id, session_id, provider, project, request_id, kind,
                        title, detail, state, risk, risk_notes, primary_category,
                        risk_codes, command_preview, expires_at, auto_hide_at,
                        reminder_acknowledged_at, reminder_resolution,
                        retain_after_ack, created_at, resolution,
                        remote_actionable
                 FROM attention_items
                 WHERE ?1 IS NULL
                    OR state IN ('open', 'committing', 'decision_sent', 'snoozed')
                    OR session_id IN (
                        SELECT id FROM sessions
                        WHERE exec_state NOT IN ('idle', 'response_finished', 'failed')
                           OR (
                               exec_state IN ('idle', 'response_finished', 'failed')
                               AND last_meaningful_activity_at IS NOT NULL
                               AND COALESCE(activity_since, last_event_at) >= ?1
                           )
                           OR EXISTS (
                               SELECT 1 FROM attention_items AS blockers
                               WHERE blockers.session_id = sessions.id
                                 AND blockers.state IN ('open', 'committing', 'decision_sent', 'snoozed')
                           )
                    )
                 ORDER BY created_at DESC",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([cutoff.map(to_i64)], |row| {
                let request: Option<String> = row.get(4)?;
                let request_id = request.and_then(|value| Uuid::parse_str(&value).ok());
                let kind: String = row.get(5)?;
                let state: String = row.get(8)?;
                let expires_at = row.get::<_, Option<i64>>(14)?.map(from_i64);
                let auto_hide_at = row.get::<_, Option<i64>>(15)?.map(from_i64);
                let reminder_acknowledged_at = row.get::<_, Option<i64>>(16)?.map(from_i64);
                Ok(AttentionRecord {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    provider: row.get(2)?,
                    project: row.get(3)?,
                    request_id,
                    kind: kind.clone(),
                    title: row.get(6)?,
                    detail: row.get(7)?,
                    state: state.clone(),
                    risk: row.get(9)?,
                    risk_notes: serde_json::from_str(&row.get::<_, String>(10)?)
                        .unwrap_or_default(),
                    primary_category: row
                        .get::<_, Option<String>>(11)?
                        .as_deref()
                        .and_then(OperationCategory::from_code),
                    risk_codes: serde_json::from_str(&row.get::<_, String>(12)?)
                        .unwrap_or_default(),
                    command_preview: row.get(13)?,
                    expires_at,
                    auto_hide_at,
                    reminder_acknowledged_at,
                    reminder_resolution: row.get(17)?,
                    retain_after_ack: row.get(18)?,
                    created_at: from_i64(row.get(19)?),
                    resolution: row.get(20)?,
                    remote_actionable: row.get::<_, bool>(21)?
                        && kind == "approval"
                        && state == "open"
                        && request_id.is_some()
                        && expires_at.is_some(),
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
                 FROM commands
                 WHERE ?1 IS NULL
                    OR attention_id IN (
                        SELECT id FROM attention_items
                        WHERE state IN ('open', 'committing', 'decision_sent', 'snoozed')
                           OR session_id IN (
                               SELECT id FROM sessions
                               WHERE last_meaningful_activity_at >= ?1
                                  OR exec_state NOT IN ('idle', 'response_finished', 'failed')
                                  OR EXISTS (
                                      SELECT 1 FROM attention_items AS blockers
                                      WHERE blockers.session_id = sessions.id
                                        AND blockers.state IN ('open', 'committing', 'decision_sent', 'snoozed')
                                  )
                           )
                    )
                 ORDER BY created_at",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([cutoff.map(to_i64)], |row| {
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
        .query_row("SELECT COUNT(*) FROM events WHERE derived = 0", [], |row| {
            row.get::<_, i64>(0)
        })
        .map(from_i64)
        .map_err(storage_error)?;
    let (metrics, token_usage) = read_metrics_and_token_usage(connection, now_millis())?;
    Ok(StoreSnapshot {
        sessions,
        attention,
        commands,
        event_count,
        metrics,
        token_usage,
    })
}

fn read_task_history(
    connection: &Connection,
    cutoff: u64,
    limit: usize,
) -> Result<Vec<TaskHistoryRecord>, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT sessions.id, sessions.provider, sessions.provider_session_id,
                    sessions.project,
                    COALESCE(sessions.provider_title, sessions.title),
                    COALESCE(
                      CASE WHEN usage.captured_at >= COALESCE(
                        (SELECT started_at FROM turns WHERE turns.session_id = sessions.id
                         ORDER BY ordinal DESC LIMIT 1), sessions.started_at)
                      THEN usage.model END,
                      sessions.model, usage.model), sessions.exec_state,
                    sessions.started_at, sessions.last_event_at,
                    (SELECT ended_at FROM turns
                     WHERE turns.session_id = sessions.id
                     ORDER BY ordinal DESC LIMIT 1),
                    sessions.task_hidden_at, sessions.task_hidden_reason,
                    COALESCE(
                      (SELECT branch FROM task_review_baselines
                       WHERE task_review_baselines.session_id = sessions.id
                       ORDER BY captured_at DESC LIMIT 1),
                      (SELECT branch FROM task_checkpoints
                       WHERE task_checkpoints.session_id = sessions.id
                       ORDER BY created_at DESC LIMIT 1)
                    ),
                    (SELECT validation_status FROM events
                     WHERE events.session_id = sessions.id
                       AND validation_status IS NOT NULL
                     ORDER BY ingest_seq DESC LIMIT 1),
                    (SELECT COUNT(*) FROM task_checkpoints
                     WHERE task_checkpoints.session_id = sessions.id),
                    (SELECT COUNT(*) FROM events
                     WHERE events.session_id = sessions.id
                       AND events.type IN (
                         'approval.requested', 'approval.denied',
                         'tool.failed', 'turn.failed'
                       )),
                    sessions.term_app, sessions.term_session_id, sessions.term_tty,
                    sessions.term_bundle_id, sessions.term_surface
             FROM sessions
             LEFT JOIN session_usage AS usage
               ON usage.provider = sessions.provider
              AND usage.provider_session_id = sessions.provider_session_id
             WHERE sessions.last_meaningful_activity_at IS NOT NULL
               AND (
                 sessions.task_hidden_at IS NOT NULL
                 OR (
                   sessions.exec_state IN ('idle', 'response_finished', 'failed')
                   AND COALESCE(sessions.activity_since, sessions.last_event_at) < ?1
                   AND NOT EXISTS (
                     SELECT 1 FROM attention_items
                     WHERE attention_items.session_id = sessions.id
                       AND attention_items.state IN (
                         'open', 'committing', 'decision_sent', 'snoozed'
                       )
                   )
                 )
               )
             ORDER BY sessions.last_event_at DESC
             LIMIT ?2",
        )
        .map_err(storage_error)?;
    let rows = statement
        .query_map(
            params![to_i64(cutoff), i64::try_from(limit).unwrap_or(500)],
            |row| {
                let provider = row.get::<_, String>(1)?;
                let provider_session_id = row.get::<_, String>(2)?;
                let exec_state = row.get::<_, String>(6)?;
                let term_app = row.get::<_, Option<String>>(16)?;
                let term_session_id = row.get::<_, Option<String>>(17)?;
                let term_tty = row.get::<_, Option<String>>(18)?;
                let term_bundle_id = row.get::<_, Option<String>>(19)?;
                let term_surface = row.get::<_, Option<String>>(20)?;
                let (jump_capability, jump_label) = jump_descriptor(
                    &provider,
                    &provider_session_id,
                    term_app.as_deref(),
                    term_session_id.as_deref(),
                    term_tty.as_deref(),
                    term_bundle_id.as_deref(),
                    term_surface.as_deref(),
                );
                Ok(TaskHistoryRecord {
                    id: row.get(0)?,
                    provider,
                    project: row.get(3)?,
                    title: row.get(4)?,
                    model: row.get(5)?,
                    status: if exec_state == "failed" {
                        "failed".to_owned()
                    } else {
                        "completed".to_owned()
                    },
                    started_at: from_i64(row.get(7)?),
                    last_event_at: from_i64(row.get(8)?),
                    completed_at: row.get::<_, Option<i64>>(9)?.map(from_i64),
                    archived_at: row.get::<_, Option<i64>>(10)?.map(from_i64),
                    archive_reason: row.get(11)?,
                    branch: row.get(12)?,
                    validation_state: row.get(13)?,
                    checkpoint_count: row
                        .get::<_, i64>(14)
                        .ok()
                        .and_then(|value| u32::try_from(value).ok())
                        .unwrap_or_default(),
                    security_event_count: row
                        .get::<_, i64>(15)
                        .ok()
                        .and_then(|value| u32::try_from(value).ok())
                        .unwrap_or_default(),
                    jump_capability,
                    jump_label,
                })
            },
        )
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    Ok(rows)
}

fn task_history_mutation_state(
    transaction: &Transaction<'_>,
    session_id: &str,
) -> Result<TaskHistoryMutation, StoreError> {
    let state = transaction
        .query_row(
            "SELECT exec_state FROM sessions WHERE id = ?1",
            [session_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(storage_error)?;
    let Some(state) = state else {
        return Ok(TaskHistoryMutation::NotFound);
    };
    let blockers = transaction
        .query_row(
            "SELECT COUNT(*) FROM attention_items
             WHERE session_id = ?1
               AND kind IN ('approval', 'native_approval', 'question')
               AND state IN ('open', 'committing', 'decision_sent', 'snoozed')",
            [session_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(storage_error)?;
    if !matches!(state.as_str(), "idle" | "response_finished" | "failed") || blockers > 0 {
        return Ok(TaskHistoryMutation::Active);
    }
    Ok(TaskHistoryMutation::Applied)
}

fn archive_task_transaction(
    connection: &mut Connection,
    session_id: &str,
    now: u64,
) -> Result<TaskHistoryMutation, StoreError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    let state = task_history_mutation_state(&transaction, session_id)?;
    if state != TaskHistoryMutation::Applied {
        return Ok(state);
    }
    transaction
        .execute(
            "UPDATE attention_items
             SET state = 'resolved', resolved_at = ?2,
                 resolution = 'history_archived', expires_at = NULL,
                 auto_hide_at = NULL
             WHERE session_id = ?1 AND kind IN ('completion', 'error')
               AND state IN ('open', 'snoozed')",
            params![session_id, to_i64(now)],
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "UPDATE sessions
             SET task_hidden_at = ?2, task_hidden_reason = 'history_archived'
             WHERE id = ?1",
            params![session_id, to_i64(now)],
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)?;
    Ok(TaskHistoryMutation::Applied)
}

fn delete_task_history_transaction(
    connection: &mut Connection,
    session_id: &str,
    _now: u64,
) -> Result<TaskHistoryMutation, StoreError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    let state = task_history_mutation_state(&transaction, session_id)?;
    if state != TaskHistoryMutation::Applied {
        return Ok(state);
    }
    let (provider, provider_session_id) = transaction
        .query_row(
            "SELECT provider, provider_session_id FROM sessions WHERE id = ?1",
            [session_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "DELETE FROM commands WHERE attention_id IN (
               SELECT id FROM attention_items WHERE session_id = ?1
             )",
            [session_id],
        )
        .map_err(storage_error)?;
    for table in [
        "attention_items",
        "task_checkpoints",
        "task_review_baselines",
        "token_usage_rate_samples",
        "events",
        "session_tasks",
        "session_plan_steps",
        "session_subagents",
        "agent_execution_intervals",
        "turns",
    ] {
        transaction
            .execute(
                &format!("DELETE FROM {table} WHERE session_id = ?1"),
                [session_id],
            )
            .map_err(storage_error)?;
    }
    for table in [
        "session_usage",
        "token_usage_cursors",
        "token_usage_session_days",
    ] {
        transaction
            .execute(
                &format!(
                    "DELETE FROM {table}
                     WHERE provider = ?1 AND provider_session_id = ?2"
                ),
                params![&provider, &provider_session_id],
            )
            .map_err(storage_error)?;
    }
    transaction
        .execute("DELETE FROM sessions WHERE id = ?1", [session_id])
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)?;
    Ok(TaskHistoryMutation::Applied)
}

fn read_ui_snapshot(connection: &Connection, cutoff: u64) -> Result<StoreSnapshot, StoreError> {
    read_snapshot(connection, Some(cutoff))
}

fn filter_ui_snapshot_for_cutoff(snapshot: &mut StoreSnapshot, cutoff: u64) {
    let actionable_sessions = snapshot
        .attention
        .iter()
        .filter(|attention| {
            matches!(
                attention.state.as_str(),
                "open" | "committing" | "decision_sent" | "snoozed"
            )
        })
        .map(|attention| attention.session_id.as_str())
        .collect::<HashSet<_>>();
    snapshot.sessions.retain(|session| {
        !matches!(
            session.exec_state.as_str(),
            "idle" | "response_finished" | "failed"
        ) || (session.last_meaningful_activity_at.is_some()
            && session.activity_since.unwrap_or(session.last_event_at) >= cutoff)
            || actionable_sessions.contains(session.id.as_str())
    });

    let visible_sessions = snapshot
        .sessions
        .iter()
        .map(|session| session.id.as_str())
        .collect::<HashSet<_>>();
    snapshot.attention.retain(|attention| {
        matches!(
            attention.state.as_str(),
            "open" | "committing" | "decision_sent" | "snoozed"
        ) || visible_sessions.contains(attention.session_id.as_str())
    });

    let visible_attention = snapshot
        .attention
        .iter()
        .map(|attention| attention.id.as_str())
        .collect::<HashSet<_>>();
    snapshot
        .commands
        .retain(|command| visible_attention.contains(command.attention_id.as_str()));
}

fn read_plan_steps_batched(
    connection: &Connection,
    sessions: &mut [SessionRecord],
) -> Result<(), StoreError> {
    if sessions.is_empty() {
        return Ok(());
    }
    let session_positions = session_positions(sessions);
    let session_ids = sessions
        .iter()
        .map(|session| session.id.clone())
        .collect::<Vec<_>>();
    let max_steps = i64::try_from(MAX_PLAN_STEPS).unwrap_or(i64::MAX);

    for range in ui_snapshot_batch_ranges(session_ids.len()) {
        let batch_session_ids = &session_ids[range];
        let placeholders = placeholders(batch_session_ids);
        let query = format!(
            "SELECT session_id, provider_turn_id, provider_turn_id || ':' || step_index,
                    step, detail, status, source
             FROM (
               SELECT session_id, provider_turn_id, step_index, step, detail, status, source,
                      ROW_NUMBER() OVER (PARTITION BY session_id ORDER BY step_index ASC) AS position
               FROM session_plan_steps
               WHERE session_id IN ({placeholders})
             )
             WHERE position <= ?
             ORDER BY session_id ASC, step_index ASC"
        );
        let mut statement = connection.prepare(&query).map_err(storage_error)?;
        let rows = statement
            .query_map(
                params_from_iter(session_query_params(batch_session_ids, &max_steps)),
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        PlanStepRecord {
                            id: row.get(2)?,
                            text: row.get(3)?,
                            detail: row.get(4)?,
                            status: row.get(5)?,
                            source: row.get(6)?,
                        },
                    ))
                },
            )
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        for (session_id, provider_turn_id, step) in rows {
            if let Some(&position) = session_positions.get(&session_id) {
                if sessions[position].provider_turn_id.as_deref() == Some(provider_turn_id.as_str())
                {
                    sessions[position].plan_steps.push(step);
                }
            }
        }
    }

    let sessions_with_connector_plan = sessions
        .iter()
        .filter(|session| !session.plan_steps.is_empty())
        .map(|session| session.id.clone())
        .collect::<HashSet<_>>();
    for range in ui_snapshot_batch_ranges(session_ids.len()) {
        let batch_session_ids = &session_ids[range];
        let placeholders = placeholders(batch_session_ids);
        let query = format!(
            "SELECT session_id, task_id, COALESCE(subject, 'Untitled task'), description,
                    CASE completed WHEN 1 THEN 'completed' ELSE 'pending' END, created_at
             FROM (
               SELECT session_id, task_id, subject, description, completed, created_at,
                      ROW_NUMBER() OVER (PARTITION BY session_id ORDER BY created_at ASC) AS position
               FROM session_tasks
               WHERE session_id IN ({placeholders})
             )
             WHERE position <= ?
             ORDER BY session_id ASC, position ASC"
        );
        let mut statement = connection.prepare(&query).map_err(storage_error)?;
        let rows = statement
            .query_map(
                params_from_iter(session_query_params(batch_session_ids, &max_steps)),
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        PlanStepRecord {
                            id: row.get(1)?,
                            text: row.get(2)?,
                            detail: row.get(3)?,
                            status: row.get(4)?,
                            source: "claude_task".to_owned(),
                        },
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        for (session_id, step, created_at) in rows {
            if let Some(&position) = session_positions.get(&session_id) {
                let belongs_to_current_turn = sessions[position]
                    .turn_started_at
                    .is_none_or(|started_at| from_i64(created_at) >= started_at);
                if belongs_to_current_turn && !sessions_with_connector_plan.contains(&session_id) {
                    sessions[position].plan_steps.push(step);
                }
            }
        }
    }
    for session in sessions {
        if session.plan_steps.is_empty() {
            session.plan_done = None;
            session.plan_total = None;
            continue;
        }
        session.plan_done = Some(
            u32::try_from(
                session
                    .plan_steps
                    .iter()
                    .filter(|step| step.status == "completed")
                    .count(),
            )
            .unwrap_or(u32::MAX),
        );
        session.plan_total = Some(u32::try_from(session.plan_steps.len()).unwrap_or(u32::MAX));
    }
    Ok(())
}

fn read_active_subagents_batched(
    connection: &Connection,
    sessions: &mut [SessionRecord],
) -> Result<(), StoreError> {
    if sessions.is_empty() {
        return Ok(());
    }
    let session_positions = session_positions(sessions);
    let session_ids = sessions
        .iter()
        .map(|session| session.id.clone())
        .collect::<Vec<_>>();
    let max_subagents = i64::try_from(MAX_ACTIVE_SUBAGENTS).unwrap_or(i64::MAX);
    for range in ui_snapshot_batch_ranges(session_ids.len()) {
        let batch_session_ids = &session_ids[range];
        let placeholders = placeholders(batch_session_ids);
        let query = format!(
            "SELECT session_id, agent_id, agent_type, status, source
             FROM (
               SELECT session_id, agent_id, agent_type, status, source,
                      ROW_NUMBER() OVER (PARTITION BY session_id ORDER BY started_at ASC) AS position
               FROM session_subagents
               WHERE active = 1 AND session_id IN ({placeholders})
             )
             WHERE position <= ?
             ORDER BY session_id ASC, position ASC"
        );
        let mut statement = connection.prepare(&query).map_err(storage_error)?;
        let rows = statement
            .query_map(
                params_from_iter(session_query_params(batch_session_ids, &max_subagents)),
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        SubagentRecord {
                            id: row.get(1)?,
                            agent_type: row.get(2)?,
                            status: row.get(3)?,
                            source: row.get(4)?,
                        },
                    ))
                },
            )
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        for (session_id, subagent) in rows {
            if let Some(&position) = session_positions.get(&session_id) {
                sessions[position].subagents.push(subagent);
            }
        }
    }
    Ok(())
}

fn session_positions(sessions: &[SessionRecord]) -> HashMap<String, usize> {
    sessions
        .iter()
        .enumerate()
        .map(|(position, session)| (session.id.clone(), position))
        .collect()
}

fn placeholders(session_ids: &[String]) -> String {
    vec!["?"; session_ids.len()].join(", ")
}

fn ui_snapshot_batch_ranges(session_count: usize) -> impl Iterator<Item = std::ops::Range<usize>> {
    (0..session_count)
        .step_by(MAX_UI_SNAPSHOT_SESSION_IDS)
        .map(move |start| {
            let end = (start + MAX_UI_SNAPSHOT_SESSION_IDS).min(session_count);
            start..end
        })
}

fn session_query_params<'a>(
    session_ids: &'a [String],
    limit: &'a i64,
) -> impl Iterator<Item = &'a dyn ToSql> {
    session_ids
        .iter()
        .map(|session_id| session_id as &dyn ToSql)
        .chain(std::iter::once(limit as &dyn ToSql))
}

fn environment_label(surface: Option<&str>, app: Option<&str>) -> Option<String> {
    match surface {
        Some("codex_app") => Some("Codex app".to_owned()),
        Some("claude_app") => Some("Claude app".to_owned()),
        Some("terminal") => Some(
            app.filter(|value| !value.trim().is_empty())
                .unwrap_or("Terminal")
                .chars()
                .take(64)
                .collect(),
        ),
        _ => None,
    }
}

fn read_metrics_and_token_usage(
    connection: &Connection,
    now: u64,
) -> Result<(MetricsSummary, TokenUsageTotals), StoreError> {
    let today = metric_day(now);
    let month = today.get(..7).unwrap_or("1970-01");
    let today_start = local_period_start(now, false);
    let month_start = local_period_start(now, true);
    let (metrics, mut token_usage, projection_totals, negative_value_count) = connection
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
                 THEN widget_approvals + widget_denials ELSE 0 END), 0),
               (SELECT COALESCE(SUM(CASE WHEN day = ?1 THEN token_total ELSE 0 END), 0)
                  FROM token_usage_daily),
               (SELECT COALESCE(SUM(CASE WHEN substr(day, 1, 7) = ?2
                                        THEN token_total ELSE 0 END), 0)
                  FROM token_usage_daily),
               (SELECT COALESCE(SUM(token_total), 0) FROM token_usage_daily),
               (SELECT MIN(captured_at) FROM token_usage_daily),
               (SELECT MAX(captured_at) FROM token_usage_daily),
               (SELECT COUNT(*) FROM (
                  SELECT day FROM token_usage_daily
                  GROUP BY day HAVING SUM(token_total) > 0
                )),
               (SELECT COUNT(*) FROM turns),
               (SELECT COALESCE(SUM(
                  MAX(0, (
                    COALESCE(
                      (SELECT MAX(events.occurred_at)
                         FROM events WHERE events.turn_id = turns.id),
                      ended_at,
                      started_at
                    ) - started_at
                  ) / 1000)
                ), 0) FROM turns),
               (SELECT day FROM token_usage_daily
                 GROUP BY day
                 ORDER BY SUM(token_total) DESC, day DESC
                 LIMIT 1),
               (SELECT COALESCE(SUM(token_total), 0)
                  FROM token_usage_daily
                 WHERE day = (
                   SELECT day FROM token_usage_daily
                   GROUP BY day
                   ORDER BY SUM(token_total) DESC, day DESC
                   LIMIT 1
                 )),
               COALESCE(
                 (SELECT MIN(captured_at) FROM token_usage_session_days),
                 (SELECT MIN(captured_at) FROM token_usage_daily_models)
               ),
               (SELECT COALESCE(SUM(MAX(0, MIN(
                    COALESCE(
                      (SELECT MAX(events.occurred_at)
                         FROM events WHERE events.turn_id = turns.id),
                      ended_at,
                      started_at
                    ), ?3
                  ) - MAX(started_at, ?4)) / 1000), 0)
                  FROM turns),
               (SELECT COALESCE(SUM(MAX(0, MIN(
                    COALESCE(
                      (SELECT MAX(events.occurred_at)
                         FROM events WHERE events.turn_id = turns.id),
                      ended_at,
                      started_at
                    ), ?3
                  ) - MAX(started_at, ?5)) / 1000), 0)
                  FROM turns),
               (SELECT COALESCE(SUM(token_total), 0)
                  FROM token_usage_session_days),
               (SELECT COALESCE(SUM(token_total), 0)
                  FROM token_usage_daily),
               (SELECT COALESCE(SUM(token_total), 0)
                  FROM token_usage_daily_models),
               (SELECT COUNT(*) FROM token_usage_session_days
                 WHERE input_tokens < 0 OR output_tokens < 0
                    OR cache_read_tokens < 0 OR cache_creation_tokens < 0
                    OR reasoning_tokens < 0 OR token_total < 0
                    OR estimated_cost_usd_micros < 0 OR message_count < 0),
               (SELECT COALESCE(SUM(MAX(0,
                    MIN(COALESCE(ended_at, ?3), ?3) - MAX(started_at, ?4)
                  )), 0) / 1000
                  FROM agent_execution_intervals
                 WHERE started_at < ?3
                   AND COALESCE(ended_at, ?3) > ?4),
               (SELECT COALESCE(SUM(MAX(0,
                    MIN(COALESCE(ended_at, ?3), ?3) - MAX(started_at, ?5)
                  )), 0) / 1000
                  FROM agent_execution_intervals
                 WHERE started_at < ?3
                   AND COALESCE(ended_at, ?3) > ?5),
               (SELECT COALESCE(SUM(MAX(0,
                    MIN(COALESCE(ended_at, ?3), ?3) - started_at
                  )), 0) / 1000
                  FROM agent_execution_intervals
                 WHERE started_at < ?3)
             FROM metrics_daily",
            params![
                today,
                month,
                to_i64(now),
                to_i64(today_start),
                to_i64(month_start)
            ],
            |row| {
                Ok((
                    MetricsSummary {
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
                    },
                    TokenUsageTotals {
                        today: from_i64(row.get(12)?),
                        month: from_i64(row.get(13)?),
                        total: from_i64(row.get(14)?),
                        recorded_from: row.get::<_, Option<i64>>(15)?.map(from_i64),
                        captured_at: row.get::<_, Option<i64>>(16)?.map(from_i64),
                        active_days: from_i64(row.get(17)?),
                        current_streak: 0,
                        turn_count: from_i64(row.get(18)?),
                        message_count: 0,
                        priced_tokens: 0,
                        unpriced_tokens: 0,
                        estimated_cost_usd_micros: None,
                        pricing_sources: Vec::new(),
                        anomalies: Vec::new(),
                        anomaly_count: 0,
                        suspect_count: 0,
                        today_active_time_seconds: from_i64(row.get(23)?),
                        month_active_time_seconds: from_i64(row.get(24)?),
                        active_time_seconds: from_i64(row.get(19)?),
                        today_execution_time_seconds: from_i64(row.get(29)?),
                        month_execution_time_seconds: from_i64(row.get(30)?),
                        execution_time_seconds: from_i64(row.get(31)?),
                        peak_day: row.get(20)?,
                        peak_day_total: from_i64(row.get(21)?),
                        by_provider: Vec::new(),
                        by_model: Vec::new(),
                        recent_days: Vec::new(),
                        detail_recorded_from: row.get::<_, Option<i64>>(22)?.map(from_i64),
                    },
                    (
                        from_i64(row.get(25)?),
                        from_i64(row.get(26)?),
                        from_i64(row.get(27)?),
                    ),
                    from_i64(row.get(28)?),
                ))
            },
        )
        .map_err(storage_error)?;

    let rows = connection
        .prepare(
            "WITH history_models AS (
               SELECT day, provider, model,
                      SUM(input_tokens) AS input_tokens,
                      SUM(output_tokens) AS output_tokens,
                      SUM(cache_read_tokens) AS cache_read_tokens,
                      SUM(cache_creation_tokens) AS cache_creation_tokens,
                      SUM(reasoning_tokens) AS reasoning_tokens,
                      SUM(token_total) AS token_total,
                      SUM(estimated_cost_usd_micros) AS cost,
                      SUM(CASE WHEN estimated_cost_usd_micros IS NOT NULL
                               THEN token_total ELSE 0 END) AS priced_tokens,
                      SUM(CASE WHEN estimated_cost_usd_micros IS NULL
                               THEN token_total ELSE 0 END) AS unpriced_tokens,
                      SUM(message_count) AS message_count
               FROM token_usage_session_days
               GROUP BY day, provider, model
             ), history_providers AS (
               SELECT DISTINCT provider FROM history_models
             ), chart_models AS (
               SELECT day, provider, model, input_tokens, output_tokens,
                      cache_read_tokens, cache_creation_tokens, reasoning_tokens,
                      token_total, cost, priced_tokens, unpriced_tokens, message_count
               FROM history_models
               UNION ALL
               SELECT day, provider, model, input_tokens, output_tokens,
                      cache_read_tokens, cache_creation_tokens, reasoning_tokens,
                      token_total, estimated_cost_usd_micros,
                      CASE WHEN estimated_cost_usd_micros IS NOT NULL
                           THEN token_total ELSE 0 END,
                      CASE WHEN estimated_cost_usd_micros IS NULL
                           THEN token_total ELSE 0 END,
                      0
               FROM token_usage_daily_models AS legacy
               WHERE NOT EXISTS (
                 SELECT 1 FROM history_providers
                 WHERE history_providers.provider = legacy.provider
               )
             ), recent_days AS (
               SELECT day FROM chart_models
               GROUP BY day
             ), observed_providers AS (
               SELECT provider FROM session_usage
               UNION
               SELECT provider FROM token_usage_daily
               UNION
               SELECT provider FROM chart_models
             ), pricing_sources AS (
               SELECT COALESCE(cost_kind, 'unclassified') AS cost_kind,
                      COALESCE(pricing_source, 'unspecified') AS pricing_source,
                      SUM(token_total) AS token_total,
                      SUM(estimated_cost_usd_micros) AS cost
               FROM token_usage_session_days
               WHERE estimated_cost_usd_micros IS NOT NULL
                 AND token_total > 0
               GROUP BY COALESCE(cost_kind, 'unclassified'),
                        COALESCE(pricing_source, 'unspecified')
             )
             SELECT 0 AS kind, '' AS day, observed_providers.provider,
                    COALESCE(SUM(CASE WHEN chart_models.day = ?1
                                      THEN chart_models.token_total ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN substr(chart_models.day, 1, 7) = ?2
                                      THEN chart_models.token_total ELSE 0 END), 0),
                    COALESCE(SUM(chart_models.token_total), 0),
                    '' AS model, NULL AS cost,
                    NULL, NULL, NULL, NULL, NULL, 0, 0, 0
             FROM observed_providers
             LEFT JOIN chart_models
               ON chart_models.provider = observed_providers.provider
             GROUP BY observed_providers.provider
             UNION ALL
             SELECT 1, usage.day, usage.provider, 0, 0,
                    SUM(usage.token_total), '',
                    SUM(usage.cost),
                    NULL, NULL, NULL, NULL, NULL,
                    SUM(usage.priced_tokens), SUM(usage.unpriced_tokens),
                    SUM(usage.message_count)
             FROM chart_models AS usage
             INNER JOIN recent_days ON recent_days.day = usage.day
             GROUP BY usage.day, usage.provider
             UNION ALL
             SELECT 2, detail.day, detail.provider, 0, 0, detail.token_total,
                    detail.model, detail.cost,
                    detail.input_tokens, detail.output_tokens,
                    detail.cache_read_tokens, detail.cache_creation_tokens,
                    detail.reasoning_tokens, detail.priced_tokens,
                    detail.unpriced_tokens, detail.message_count
             FROM chart_models AS detail
             INNER JOIN recent_days ON recent_days.day = detail.day
             UNION ALL
             SELECT 3, '', pricing.cost_kind, 0, 0, pricing.token_total,
                    pricing.pricing_source, pricing.cost,
                    NULL, NULL, NULL, NULL, NULL,
                    pricing.token_total, 0, 0
             FROM pricing_sources AS pricing
             ORDER BY kind ASC, day ASC, provider ASC, model ASC",
        )
        .map_err(storage_error)?
        .query_map(params![today, month], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                from_i64(row.get(3)?),
                from_i64(row.get(4)?),
                from_i64(row.get(5)?),
                row.get::<_, String>(6)?,
                row.get::<_, Option<i64>>(7)?.map(from_i64),
                row.get::<_, Option<i64>>(8)?.map(from_i64),
                row.get::<_, Option<i64>>(9)?.map(from_i64),
                row.get::<_, Option<i64>>(10)?.map(from_i64),
                row.get::<_, Option<i64>>(11)?.map(from_i64),
                row.get::<_, Option<i64>>(12)?.map(from_i64),
                from_i64(row.get(13)?),
                from_i64(row.get(14)?),
                from_i64(row.get(15)?),
            ))
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    let mut days = HashMap::<String, TokenUsageDayTotal>::new();
    for (
        kind,
        day,
        provider,
        today_total,
        month_total,
        total,
        model,
        cost,
        input,
        output,
        cache_read,
        cache_creation,
        reasoning,
        priced_tokens,
        unpriced_tokens,
        message_count,
    ) in rows
    {
        match kind {
            0 => token_usage.by_provider.push(TokenUsageProviderTotal {
                provider,
                today: today_total,
                month: month_total,
                total,
            }),
            1 => {
                let entry = days
                    .entry(day.clone())
                    .or_insert_with(|| TokenUsageDayTotal {
                        day,
                        total: 0,
                        estimated_cost_usd_micros: None,
                        priced_tokens: 0,
                        unpriced_tokens: 0,
                        message_count: 0,
                        by_provider: Vec::new(),
                        by_model: Vec::new(),
                    });
                entry.total = entry.total.saturating_add(total);
                entry.estimated_cost_usd_micros = match (entry.estimated_cost_usd_micros, cost) {
                    (Some(left), Some(right)) => Some(left.saturating_add(right)),
                    (None, Some(right)) => Some(right),
                    (Some(left), None) => Some(left),
                    (None, None) => None,
                };
                entry.priced_tokens = entry.priced_tokens.saturating_add(priced_tokens);
                entry.unpriced_tokens = entry.unpriced_tokens.saturating_add(unpriced_tokens);
                entry.message_count = entry.message_count.saturating_add(message_count);
                entry.by_provider.push(TokenUsageDayProviderTotal {
                    provider,
                    total,
                    estimated_cost_usd_micros: cost,
                    priced_tokens,
                    unpriced_tokens,
                    message_count,
                });
            }
            2 => {
                let entry = days
                    .entry(day.clone())
                    .or_insert_with(|| TokenUsageDayTotal {
                        day,
                        total: 0,
                        estimated_cost_usd_micros: None,
                        priced_tokens: 0,
                        unpriced_tokens: 0,
                        message_count: 0,
                        by_provider: Vec::new(),
                        by_model: Vec::new(),
                    });
                entry.by_model.push(TokenUsageDayModelTotal {
                    provider,
                    model,
                    total,
                    input_tokens: input,
                    output_tokens: output,
                    cache_read_tokens: cache_read,
                    cache_creation_tokens: cache_creation,
                    reasoning_tokens: reasoning,
                    estimated_cost_usd_micros: cost,
                    priced_tokens,
                    unpriced_tokens,
                    message_count,
                });
            }
            3 => token_usage.pricing_sources.push(TokenUsagePricingSource {
                cost_kind: provider,
                source: model,
                token_total: total,
                estimated_cost_usd_micros: cost.unwrap_or_default(),
            }),
            _ => {}
        }
    }
    token_usage.by_provider.sort_by(|left, right| {
        right
            .total
            .cmp(&left.total)
            .then_with(|| left.provider.cmp(&right.provider))
    });
    token_usage.pricing_sources.sort_by(|left, right| {
        right
            .token_total
            .cmp(&left.token_total)
            .then_with(|| left.source.cmp(&right.source))
    });
    let mut recent_days = days.into_values().collect::<Vec<_>>();
    recent_days.sort_by(|left, right| left.day.cmp(&right.day));
    if !recent_days.is_empty() {
        token_usage.today = recent_days
            .iter()
            .filter(|day| day.day == today)
            .fold(0_u64, |total, day| total.saturating_add(day.total));
        token_usage.month = recent_days
            .iter()
            .filter(|day| day.day.starts_with(month))
            .fold(0_u64, |total, day| total.saturating_add(day.total));
        token_usage.total = recent_days
            .iter()
            .fold(0_u64, |total, day| total.saturating_add(day.total));
    }
    token_usage.active_days = recent_days.iter().filter(|day| day.total > 0).count() as u64;
    token_usage.message_count = recent_days
        .iter()
        .fold(0_u64, |total, day| total.saturating_add(day.message_count));
    token_usage.priced_tokens = recent_days
        .iter()
        .fold(0_u64, |total, day| total.saturating_add(day.priced_tokens));
    token_usage.unpriced_tokens = recent_days.iter().fold(0_u64, |total, day| {
        total.saturating_add(day.unpriced_tokens)
    });
    token_usage.estimated_cost_usd_micros = recent_days
        .iter()
        .filter_map(|day| day.estimated_cost_usd_micros)
        .reduce(u64::saturating_add);
    let detected_anomalies = token_usage_anomalies(
        &today,
        &recent_days,
        projection_totals,
        negative_value_count,
        token_usage.unpriced_tokens,
    );
    token_usage.anomaly_count = detected_anomalies.len() as u64;
    token_usage.suspect_count = detected_anomalies
        .iter()
        .filter(|anomaly| anomaly.severity == "suspect")
        .count() as u64;
    token_usage.anomalies = detected_anomalies.into_iter().take(32).collect();
    let suspect_days = token_usage
        .anomalies
        .iter()
        .filter(|anomaly| anomaly.severity == "suspect")
        .filter_map(|anomaly| anomaly.day.as_deref())
        .collect::<HashSet<_>>();
    token_usage.peak_day = None;
    token_usage.peak_day_total = 0;
    if let Some(peak) = recent_days
        .iter()
        .filter(|day| !suspect_days.contains(day.day.as_str()))
        .max_by(|left, right| {
            left.total
                .cmp(&right.total)
                .then_with(|| left.day.cmp(&right.day))
        })
    {
        token_usage.peak_day = Some(peak.day.clone());
        token_usage.peak_day_total = peak.total;
    }
    token_usage.current_streak = usage_current_streak(&recent_days, &today);
    token_usage.recent_days = recent_days;

    token_usage.by_model = connection
        .prepare(
            "WITH history_providers AS (
               SELECT DISTINCT provider FROM token_usage_session_days
             ), models AS (
               SELECT provider, model,
                      SUM(token_total) AS token_total,
                      SUM(input_tokens) AS input_tokens,
                      SUM(output_tokens) AS output_tokens,
                      SUM(cache_read_tokens) AS cache_read_tokens,
                      SUM(cache_creation_tokens) AS cache_creation_tokens,
                      SUM(reasoning_tokens) AS reasoning_tokens,
                      SUM(estimated_cost_usd_micros) AS cost,
                      SUM(CASE WHEN estimated_cost_usd_micros IS NOT NULL
                               THEN token_total ELSE 0 END) AS priced_tokens,
                      SUM(CASE WHEN estimated_cost_usd_micros IS NULL
                               THEN token_total ELSE 0 END) AS unpriced_tokens
               FROM token_usage_session_days
               GROUP BY provider, model
               UNION ALL
               SELECT provider, COALESCE(model, 'Unknown'),
                      SUM(token_total), SUM(input_tokens), SUM(output_tokens),
                      SUM(cache_read_tokens), SUM(cache_creation_tokens),
                      SUM(reasoning_tokens), SUM(estimated_cost_usd_micros),
                      SUM(CASE WHEN estimated_cost_usd_micros IS NOT NULL
                               THEN token_total ELSE 0 END),
                      SUM(CASE WHEN estimated_cost_usd_micros IS NULL
                               THEN token_total ELSE 0 END)
               FROM session_usage
               WHERE token_total IS NOT NULL
                 AND NOT EXISTS (
                   SELECT 1 FROM history_providers
                   WHERE history_providers.provider = session_usage.provider
                 )
               GROUP BY provider, COALESCE(model, 'Unknown')
             )
             SELECT provider, model, token_total, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, reasoning_tokens, cost,
                    priced_tokens, unpriced_tokens
             FROM models
             ORDER BY token_total DESC, provider ASC, model ASC
             LIMIT 64",
        )
        .map_err(storage_error)?
        .query_map([], |row| {
            Ok(TokenUsageModelTotal {
                provider: row.get(0)?,
                model: row.get(1)?,
                total: from_i64(row.get(2)?),
                input_tokens: row.get::<_, Option<i64>>(3)?.map(from_i64),
                output_tokens: row.get::<_, Option<i64>>(4)?.map(from_i64),
                cache_read_tokens: row.get::<_, Option<i64>>(5)?.map(from_i64),
                cache_creation_tokens: row.get::<_, Option<i64>>(6)?.map(from_i64),
                reasoning_tokens: row.get::<_, Option<i64>>(7)?.map(from_i64),
                estimated_cost_usd_micros: row.get::<_, Option<i64>>(8)?.map(from_i64),
                priced_tokens: from_i64(row.get(9)?),
                unpriced_tokens: from_i64(row.get(10)?),
            })
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    Ok((metrics, token_usage))
}

fn token_usage_anomalies(
    today: &str,
    days: &[TokenUsageDayTotal],
    projection_totals: (u64, u64, u64),
    negative_value_count: u64,
    unpriced_tokens: u64,
) -> Vec<TokenUsageAnomaly> {
    let (canonical_total, daily_total, model_total) = projection_totals;
    let mut anomalies = Vec::new();
    if daily_total != canonical_total {
        anomalies.push(TokenUsageAnomaly {
            code: "daily_projection_mismatch".to_owned(),
            severity: "suspect".to_owned(),
            scope: "ledger".to_owned(),
            day: None,
            observed: Some(daily_total),
            expected: Some(canonical_total),
        });
    }
    if model_total != canonical_total {
        anomalies.push(TokenUsageAnomaly {
            code: "model_projection_mismatch".to_owned(),
            severity: "suspect".to_owned(),
            scope: "ledger".to_owned(),
            day: None,
            observed: Some(model_total),
            expected: Some(canonical_total),
        });
    }
    if negative_value_count > 0 {
        anomalies.push(TokenUsageAnomaly {
            code: "negative_usage_value".to_owned(),
            severity: "suspect".to_owned(),
            scope: "ledger".to_owned(),
            day: None,
            observed: Some(negative_value_count),
            expected: Some(0),
        });
    }
    for day in days
        .iter()
        .filter(|day| day.total > 0 && day.day.as_str() > today)
    {
        anomalies.push(TokenUsageAnomaly {
            code: "future_usage_day".to_owned(),
            severity: "suspect".to_owned(),
            scope: "day".to_owned(),
            day: Some(day.day.clone()),
            observed: Some(day.total),
            expected: None,
        });
    }
    let mut active_totals = days
        .iter()
        .filter(|day| day.total > 0 && day.day.as_str() <= today)
        .map(|day| day.total)
        .collect::<Vec<_>>();
    if active_totals.len() >= 7 {
        active_totals.sort_unstable();
        let median = active_totals[active_totals.len() / 2];
        let threshold = 1_000_000_000_u64.max(median.saturating_mul(20));
        for day in days
            .iter()
            .filter(|day| day.day.as_str() <= today && day.total > threshold)
        {
            anomalies.push(TokenUsageAnomaly {
                code: "extreme_daily_jump".to_owned(),
                severity: "suspect".to_owned(),
                scope: "day".to_owned(),
                day: Some(day.day.clone()),
                observed: Some(day.total),
                expected: Some(threshold),
            });
        }
    }
    if unpriced_tokens > 0 {
        anomalies.push(TokenUsageAnomaly {
            code: "unpriced_tokens".to_owned(),
            severity: "warning".to_owned(),
            scope: "pricing".to_owned(),
            day: None,
            observed: Some(unpriced_tokens),
            expected: Some(0),
        });
    }
    anomalies.sort_by(|left, right| {
        let left_rank = usize::from(left.severity != "suspect");
        let right_rank = usize::from(right.severity != "suspect");
        left_rank
            .cmp(&right_rank)
            .then_with(|| left.day.cmp(&right.day))
            .then_with(|| left.code.cmp(&right.code))
    });
    anomalies
}

fn usage_current_streak(days: &[TokenUsageDayTotal], today: &str) -> u64 {
    let active = days
        .iter()
        .filter(|day| day.total > 0)
        .filter_map(|day| civil_day_number(&day.day))
        .collect::<HashSet<_>>();
    let Some(mut cursor) = civil_day_number(today) else {
        return 0;
    };
    if !active.contains(&cursor) && active.contains(&(cursor - 1)) {
        cursor -= 1;
    }
    let mut streak = 0_u64;
    while active.contains(&cursor) {
        streak = streak.saturating_add(1);
        cursor -= 1;
    }
    streak
}

fn civil_day_number(value: &str) -> Option<i64> {
    let mut parts = value.split('-');
    let year = parts.next()?.parse::<i64>().ok()?;
    let month = parts.next()?.parse::<i64>().ok()?;
    let day = parts.next()?.parse::<i64>().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let adjusted_year = year - i64::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era)
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
            let latest = transaction
                .query_row(
                    "SELECT id FROM turns WHERE session_id = ?1
                     ORDER BY ordinal DESC LIMIT 1",
                    [session_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(storage_error)?;
            // Codex can automatically continue the same provider turn after
            // first emitting Stop (for example after context compaction). A
            // concrete tool start is authoritative evidence that the turn is
            // active again; reopen that turn instead of leaving Runtime in a
            // contradictory completed state while workflow events advance.
            if kind == EventKind::ToolStarted {
                if let Some(turn_id) = latest.as_deref() {
                    transaction
                        .execute(
                            "UPDATE turns SET state = 'running', ended_at = NULL WHERE id = ?1",
                            [turn_id],
                        )
                        .map_err(storage_error)?;
                }
            }
            return Ok(latest);
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

/// Plans and Provider task lists describe one Agent turn, not the lifetime of
/// a chat session. Keep historical events, but reset the live projection as
/// soon as a new user prompt starts a new turn.
fn reset_turn_scoped_task_state(
    transaction: &Transaction<'_>,
    session_id: &str,
) -> Result<(), StoreError> {
    transaction
        .execute(
            "DELETE FROM session_plan_steps WHERE session_id = ?1",
            [session_id],
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "DELETE FROM session_tasks WHERE session_id = ?1",
            [session_id],
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "UPDATE sessions SET plan_done = NULL, plan_total = NULL WHERE id = ?1",
            [session_id],
        )
        .map_err(storage_error)?;
    Ok(())
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
    // A managed app-server request is the authoritative reply channel. If a
    // preceding thread status notification created an observation-only row,
    // retire it before exposing the directly actionable approval.
    transaction
        .execute(
            "UPDATE attention_items SET state = 'resolved', resolved_at = ?2,
               resolution = 'direct_channel_available', expires_at = NULL
             WHERE session_id = ?1 AND kind = 'native_approval'
               AND state IN ('open', 'snoozed')",
            params![session_id, to_i64(request.received_at)],
        )
        .map_err(storage_error)?;
    let id = Uuid::now_v7().to_string();
    let command = request
        .raw
        .pointer("/tool_input/command")
        .and_then(Value::as_str);
    let classification = classify_operation(tool_name, &request.raw);
    let notes = risk_notes(&classification.risk_codes);
    let project = request
        .raw
        .get("cwd")
        .and_then(Value::as_str)
        .and_then(project_name);
    transaction
        .execute(
            "INSERT INTO attention_items (
               id, session_id, provider, project, turn_id, request_id, kind,
               title, detail, command_preview, risk, risk_notes, primary_category,
               risk_codes, dedupe_key, state, expires_at, created_at, remote_actionable
             ) VALUES (
               ?1, ?2, ?3, ?4, ?5, ?6, 'approval', ?7, ?8, ?9, ?10, ?11, ?12,
               ?13, ?6, 'open', ?14, ?15, ?16
             )",
            params![
                id,
                session_id,
                request.provider.to_string(),
                project,
                turn_id,
                request_id.to_string(),
                format!(
                    "Allow {}?",
                    tool_name
                        .map(sanitized_tool_name)
                        .as_deref()
                        .unwrap_or("this operation")
                ),
                approval_detail(&request.raw, tool_name),
                command.map(redacted_preview),
                classification.risk,
                serde_json::to_string(&notes)
                    .map_err(|error| StoreError::Storage(error.to_string()))?,
                classification.primary_category.as_str(),
                serde_json::to_string(&classification.risk_codes)
                    .map_err(|error| StoreError::Storage(error.to_string()))?,
                request.deadline_at.map(to_i64),
                to_i64(request.received_at),
                request.remote_action_capability == Some(RemoteActionCapability::Approval),
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
    let (title, detail) = native_attention_copy(request);
    transaction
        .execute(
            "INSERT INTO attention_items (
               id, session_id, provider, project, turn_id, request_id, kind,
               title, detail, command_preview, risk, risk_notes, dedupe_key,
               state, expires_at, created_at
             ) VALUES (
               ?1, ?2, ?3, ?4, ?5, NULL, 'native_approval',
               ?6, ?7, NULL, 'unknown', '[]', ?8,
               'open', NULL, ?9
             )",
            params![
                id,
                session_id,
                request.provider.to_string(),
                project,
                turn_id,
                title,
                detail,
                dedupe_key,
                to_i64(request.received_at),
            ],
        )
        .map_err(storage_error)?;
    Ok(id)
}

fn native_attention_copy(request: &BridgeRequest) -> (String, String) {
    if matches!(request.provider, Provider::Kimi | Provider::Grok) {
        return ("Approve in the Agent".to_owned(), "The Agent is waiting for a native permission decision. Handle it in the corresponding conversation.".to_owned());
    }
    let tool_name = request.raw.get("tool_name").and_then(Value::as_str);
    if tool_name == Some("request_plugin_install") {
        let plugin = request
            .raw
            .pointer("/tool_input/plugin_name")
            .or_else(|| request.raw.pointer("/tool_input/name"))
            .or_else(|| request.raw.pointer("/tool_input/plugin_id"))
            .and_then(Value::as_str)
            .and_then(sanitized_plugin_label);
        let subject = plugin.as_deref().unwrap_or("plugin");
        let title = format!("Codex requests installation or connection of {subject}");
        let reason = request
            .raw
            .pointer("/tool_input/suggest_reason")
            .or_else(|| request.raw.pointer("/tool_input/reason"))
            .and_then(Value::as_str)
            .and_then(sanitized_attention_text);
        let detail = reason.map_or_else(
            || {
                format!(
                    "Codex is showing a native confirmation window for {subject}. Return to the corresponding conversation to confirm or cancel."
                )
            },
            |reason| {
                format!(
                    "{reason} Return to the Codex interface to confirm or cancel."
                )
            },
        );
        return (title, detail);
    }

    let detail = request
        .raw
        .pointer("/tool_input/reason")
        .or_else(|| request.raw.get("reason"))
        .and_then(Value::as_str)
        .and_then(sanitized_attention_text)
        .unwrap_or_else(|| {
            "Codex is showing a native permission request. Return to the corresponding conversation to review and handle it.".to_owned()
        });
    ("Codex is requesting approval".to_owned(), detail)
}

fn native_attention_activity(request: &BridgeRequest) -> String {
    if matches!(request.provider, Provider::Kimi | Provider::Grok) {
        return "Handle the permission request in the original Agent interface".to_owned();
    }
    if request.raw.get("tool_name").and_then(Value::as_str) == Some("request_plugin_install") {
        let plugin = request
            .raw
            .pointer("/tool_input/plugin_name")
            .or_else(|| request.raw.pointer("/tool_input/name"))
            .or_else(|| request.raw.pointer("/tool_input/plugin_id"))
            .and_then(Value::as_str)
            .and_then(sanitized_plugin_label)
            .unwrap_or_else(|| "plugin".to_owned());
        format!("Waiting for you to confirm installation or connection of {plugin} in Codex")
    } else {
        "Codex is requesting approval; handle it in the original interface".to_owned()
    }
}

fn sanitized_plugin_label(value: &str) -> Option<String> {
    let identifier = value.split('@').next().unwrap_or(value);
    match identifier.to_ascii_lowercase().as_str() {
        "github" => return Some("GitHub".to_owned()),
        "gmail" => return Some("Gmail".to_owned()),
        "google-drive" | "google_drive" => return Some("Google Drive".to_owned()),
        _ => {}
    }
    let words = identifier
        .split(['-', '_'])
        .filter(|word| !word.is_empty())
        .take(5)
        .map(|word| {
            let mut characters = word.chars().filter(|character| character.is_alphanumeric());
            let first = characters.next()?;
            let mut normalized = first.to_uppercase().collect::<String>();
            normalized.extend(characters);
            Some(normalized)
        })
        .collect::<Option<Vec<_>>>()?;
    if words.is_empty() {
        None
    } else {
        Some(words.join(" "))
    }
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
    completion_hide_policy: CompletionTaskHidePolicy,
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
    let auto_hide_at = (spec.kind == "completion")
        .then(|| completion_hide_policy.delay_ms())
        .flatten()
        .map(|delay| request.received_at.saturating_add(delay));
    let retain_after_ack = spec.kind == "completion" && completion_hide_policy.retain_after_ack();
    transaction
        .execute(
            "INSERT INTO attention_items (
               id, session_id, provider, project, turn_id, request_id, kind,
               title, detail, command_preview, risk, risk_notes, dedupe_key,
               state, expires_at, auto_hide_at, retain_after_ack, created_at
             ) VALUES (
               ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, 'unknown',
               '[]', ?10, 'open', ?11, ?12, ?13, ?14
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
                auto_hide_at.map(to_i64),
                retain_after_ack,
                to_i64(request.received_at)
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

fn resolve_observed_agent_question(
    transaction: &Transaction<'_>,
    session_id: &str,
    request: &BridgeRequest,
    at: i64,
) -> Result<(), StoreError> {
    let Some(tool) = request
        .raw
        .get("tool_call_id")
        .or_else(|| request.raw.get("tool_use_id"))
        .and_then(Value::as_str)
    else {
        return Ok(());
    };
    transaction
        .execute(
            "UPDATE attention_items SET state='resolved', resolved_at=?2,
        resolution='provider_handled', expires_at=NULL WHERE dedupe_key=?1 AND kind='question'
        AND request_id IS NULL AND state IN ('open','snoozed')",
            params![format!("{session_id}:question:{tool}"), at],
        )
        .map_err(storage_error)?;
    Ok(())
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
            "SELECT attention_id, request_id, action, state, created_at,
                    commit_delay_ms
             FROM commands WHERE id = ?1",
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
        .map_err(storage_error)?;
    row.map(
        |(attention_id, request_id, action, state, created_at, commit_delay_ms)| {
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
                    .map(|_| from_i64(created_at).saturating_add(from_i64(commit_delay_ms))),
            })
        },
    )
    .transpose()
}

fn is_observed_codex_permission_start(request: &BridgeRequest) -> bool {
    if matches!(request.provider, Provider::Kimi | Provider::Grok) {
        return request.event_name() == Some("PermissionRequest") && !request.needs_reply;
    }
    request.provider == Provider::Codex
        && request.event_name() == Some("PreToolUse")
        && is_codex_native_attention_tool(request.raw.get("tool_name").and_then(Value::as_str))
        && !request.needs_reply
        && !request.provider_handles_approval
}

fn reconcile_superseded_nonblocking_attention(
    transaction: &Transaction<'_>,
    session_id: &str,
    kind: EventKind,
    occurred_at: i64,
) -> Result<(), StoreError> {
    if !matches!(
        kind,
        EventKind::SessionStarted
            | EventKind::PromptSubmitted
            | EventKind::ToolStarted
            | EventKind::PermissionRequested
            | EventKind::QuestionRequested
            | EventKind::ElicitationRequested
            | EventKind::Compacting
            | EventKind::SubagentStarted
            | EventKind::TaskCreated
    ) {
        return Ok(());
    }
    transaction
        .execute(
            "UPDATE attention_items SET state = 'resolved', resolved_at = ?2,
               resolution = 'superseded_by_activity', expires_at = NULL,
               auto_hide_at = NULL, reminder_acknowledged_at = NULL,
               reminder_resolution = NULL
             WHERE session_id = ?1 AND kind IN ('completion', 'error')
               AND state IN ('open', 'snoozed') AND created_at < ?2",
            params![session_id, occurred_at],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn reconcile_auto_review_attention(
    transaction: &Transaction<'_>,
    session_id: &str,
    kind: EventKind,
    raw: &Value,
    occurred_at: i64,
) -> Result<(), StoreError> {
    if !matches!(
        kind,
        EventKind::AutoReviewStarted | EventKind::AutoReviewCompleted
    ) {
        return Ok(());
    }
    let status = raw
        .pointer("/review/status")
        .or_else(|| raw.get("status"))
        .and_then(Value::as_str)
        .unwrap_or(if kind == EventKind::AutoReviewStarted {
            "inProgress"
        } else {
            "completed"
        });
    transaction
        .execute(
            "UPDATE attention_items SET state = 'resolved', resolved_at = ?2,
               resolution = ?3, expires_at = NULL
             WHERE session_id = ?1 AND kind = 'native_approval'
               AND state IN ('open', 'snoozed')",
            params![session_id, occurred_at, format!("auto_review:{status}")],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn reconcile_observed_native_permission(
    transaction: &Transaction<'_>,
    session_id: &str,
    turn_id: Option<&str>,
    request: &BridgeRequest,
    kind: EventKind,
    occurred_at: i64,
) -> Result<bool, StoreError> {
    if matches!(request.provider, Provider::Kimi | Provider::Grok) {
        if !matches!(
            request.event_name(),
            Some(
                "PermissionResult"
                    | "PermissionDenied"
                    | "TurnInterrupted"
                    | "StopFailure"
                    | "SessionEnd"
            )
        ) {
            return Ok(false);
        }
        let count = transaction.execute("UPDATE attention_items SET state = 'resolved', resolved_at = ?2, resolution = 'provider_handled', expires_at = NULL WHERE session_id = ?1 AND kind = 'native_approval' AND state IN ('open', 'snoozed')", params![session_id, occurred_at]).map_err(storage_error)?;
        return Ok(count != 0);
    }
    if request.provider != Provider::Codex {
        return Ok(false);
    }
    // Codex Desktop's request_permissions helper can emit PostToolUse and Stop
    // while its sheet is still visible, so that exact helper end is not a user
    // decision. request_plugin_install differs: its function output is emitted
    // only after the native install/connect dialog resolves, so PostToolUse is
    // authoritative evidence that the request left the screen.
    let request_permissions_tool =
        request.raw.get("tool_name").and_then(Value::as_str) == Some("request_permissions");
    let provider_advanced = matches!(
        kind,
        EventKind::ToolStarted | EventKind::ToolFinished | EventKind::ToolFailed
    ) && !request_permissions_tool;
    let closes_actionable_state = provider_advanced
        || matches!(
            kind,
            EventKind::PromptSubmitted
                | EventKind::PermissionDenied
                | EventKind::Interrupted
                | EventKind::Failed
                | EventKind::SessionEnded
        );
    if !closes_actionable_state {
        return Ok(false);
    }

    let scope_to_turn = provider_advanced || kind == EventKind::PermissionDenied;
    let resolution = if kind == EventKind::PermissionDenied {
        "provider_denied"
    } else if provider_advanced {
        "provider_advanced"
    } else {
        "provider_closed"
    };
    let updated = transaction
        .execute(
            "UPDATE attention_items SET state = 'resolved', resolved_at = ?2,
               resolution = ?3, expires_at = NULL
             WHERE session_id = ?1 AND kind = 'native_approval'
               AND state IN ('open', 'snoozed')
               AND (?4 = 0 OR ?5 IS NULL OR turn_id = ?5)",
            params![
                session_id,
                occurred_at,
                resolution,
                i64::from(scope_to_turn),
                turn_id,
            ],
        )
        .map_err(storage_error)?;
    Ok(updated != 0)
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
            .map(|name| {
                format!(
                    "The Agent requests access to {name}. Verify the full path in the original conversation."
                )
            }),
        Some("Bash" | "Shell") => Some(
            "The Agent requests a terminal command. Only a redacted summary is shown here; verify the full command in the original conversation.".to_owned(),
        ),
        Some(name) => Some(format!(
            "The Agent requests to run {name}. Verify the purpose and impact."
        )),
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
        return Some(
            "Content may contain credentials and was hidden. Review it in the original conversation."
                .to_owned(),
        );
    }
    let mut bounded = normalized.chars().take(180).collect::<String>();
    if normalized.chars().count() > 180 {
        bounded.push('…');
    }
    Some(bounded)
}

fn update_plan_progress(
    transaction: &Transaction<'_>,
    session_id: &str,
    provider_turn_id: Option<&str>,
    kind: EventKind,
    raw: &Value,
    occurred_at: i64,
) -> Result<Option<(u32, u32)>, StoreError> {
    if kind == EventKind::PlanUpdated {
        return update_codex_plan_progress(
            transaction,
            session_id,
            provider_turn_id,
            raw,
            occurred_at,
        );
    }
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
                "INSERT INTO session_tasks (
                   session_id, task_id, subject, description, completed, created_at
                 ) VALUES (?1, ?2, ?3, ?4, 0, ?5)
                 ON CONFLICT(session_id, task_id) DO UPDATE SET
                   subject = COALESCE(excluded.subject, session_tasks.subject),
                   description = COALESCE(excluded.description, session_tasks.description)",
                params![
                    session_id,
                    task_id,
                    bounded_plan_text(raw.get("task_subject").and_then(Value::as_str)),
                    bounded_plan_detail(raw.get("task_description").and_then(Value::as_str)),
                    occurred_at
                ],
            )
            .map_err(storage_error)?;
    } else {
        transaction
            .execute(
                "INSERT INTO session_tasks (
                   session_id, task_id, subject, description, completed, created_at, completed_at
                 ) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5)
                 ON CONFLICT(session_id, task_id) DO UPDATE SET
                   subject = COALESCE(excluded.subject, session_tasks.subject),
                   description = COALESCE(excluded.description, session_tasks.description),
                   completed = 1,
                   completed_at = COALESCE(session_tasks.completed_at, excluded.completed_at)",
                params![
                    session_id,
                    task_id,
                    bounded_plan_text(raw.get("task_subject").and_then(Value::as_str)),
                    bounded_plan_detail(raw.get("task_description").and_then(Value::as_str)),
                    occurred_at
                ],
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

fn update_codex_plan_progress(
    transaction: &Transaction<'_>,
    session_id: &str,
    provider_turn_id: Option<&str>,
    raw: &Value,
    occurred_at: i64,
) -> Result<Option<(u32, u32)>, StoreError> {
    let provided_turn_id = provider_turn_id.filter(|value| !value.is_empty());
    let latest_provider_turn_id = transaction
        .query_row(
            "SELECT provider_turn_id FROM turns
             WHERE session_id = ?1 ORDER BY ordinal DESC LIMIT 1",
            [session_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(storage_error)?
        .flatten();
    if provided_turn_id.is_some_and(|provided| {
        latest_provider_turn_id
            .as_deref()
            .is_some_and(|latest| latest != provided)
    }) {
        // An out-of-order update for a completed turn remains in the event
        // history but must never replace the active task's plan projection.
        return Ok(None);
    }
    let synthetic_turn_id;
    let provider_turn_id = if let Some(turn_id) = provided_turn_id {
        turn_id
    } else if let Some(turn_id) = latest_provider_turn_id.as_deref() {
        turn_id
    } else {
        synthetic_turn_id = format!("synthetic-plan-{occurred_at}");
        synthetic_turn_id.as_str()
    };
    let plan_container = raw.get("tool_input").unwrap_or(raw);
    let Some(plan) = ["plan", "steps", "items"]
        .into_iter()
        .find_map(|key| plan_container.get(key).and_then(Value::as_array))
    else {
        return Ok(None);
    };
    let steps = plan
        .iter()
        .take(MAX_PLAN_STEPS)
        .filter_map(|entry| {
            let text = bounded_plan_text(
                ["step", "text", "content"]
                    .into_iter()
                    .find_map(|key| entry.get(key).and_then(Value::as_str)),
            )?;
            let status = normalize_plan_status(
                ["status", "state"]
                    .into_iter()
                    .find_map(|key| entry.get(key).and_then(Value::as_str)),
            );
            Some((text, status))
        })
        .collect::<Vec<_>>();
    transaction
        .execute(
            "DELETE FROM session_plan_steps WHERE session_id = ?1",
            [session_id],
        )
        .map_err(storage_error)?;
    let detail = bounded_plan_detail(
        ["explanation", "detail"]
            .into_iter()
            .find_map(|key| plan_container.get(key).and_then(Value::as_str)),
    );
    for (index, (step, status)) in steps.iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO session_plan_steps (
                   session_id, provider_turn_id, step_index, step, detail,
                   status, source, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'codex_turn_plan', ?7)",
                params![
                    session_id,
                    provider_turn_id,
                    i64::try_from(index).unwrap_or(i64::MAX),
                    step,
                    (index == 0).then_some(detail.as_deref()).flatten(),
                    status,
                    occurred_at
                ],
            )
            .map_err(storage_error)?;
    }
    let total = u32::try_from(steps.len()).unwrap_or(u32::MAX);
    let done = u32::try_from(
        steps
            .iter()
            .filter(|(_, status)| *status == "completed")
            .count(),
    )
    .unwrap_or(u32::MAX);
    Ok((total > 0).then_some((done, total)))
}

fn bounded_plan_text(value: Option<&str>) -> Option<String> {
    bounded_clean_text(value, MAX_PLAN_STEP_CHARS)
}

fn bounded_plan_detail(value: Option<&str>) -> Option<String> {
    bounded_clean_text(value, MAX_PLAN_DETAIL_CHARS)
}

fn bounded_clean_text(value: Option<&str>, max_chars: usize) -> Option<String> {
    let normalized = value?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_owned();
    if normalized.is_empty() {
        return None;
    }
    let mut bounded = normalized.chars().take(max_chars).collect::<String>();
    if normalized.chars().count() > max_chars {
        bounded.push('…');
    }
    Some(bounded)
}

fn normalize_plan_status(value: Option<&str>) -> &'static str {
    match value {
        Some("completed") => "completed",
        Some("inProgress" | "in_progress") => "in_progress",
        _ => "pending",
    }
}

fn update_subagent_activity(
    transaction: &Transaction<'_>,
    session_id: &str,
    kind: EventKind,
    raw: &Value,
    occurred_at: i64,
) -> Result<Option<u32>, StoreError> {
    if matches!(
        kind,
        EventKind::Stopped | EventKind::Interrupted | EventKind::Failed | EventKind::SessionEnded
    ) && !has_background_work(raw)
    {
        transaction
            .execute(
                "UPDATE session_subagents SET active = 0,
                   status = ?3,
                   stopped_at = COALESCE(stopped_at, ?2)
                 WHERE session_id = ?1 AND active = 1",
                params![
                    session_id,
                    occurred_at,
                    if matches!(kind, EventKind::Interrupted | EventKind::Failed) {
                        "interrupted"
                    } else {
                        "completed"
                    }
                ],
            )
            .map_err(storage_error)?;
        return Ok(Some(0));
    }
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
    let agent_type = bounded_clean_text(raw.get("agent_type").and_then(Value::as_str), 128);
    let agent_status = bounded_clean_text(raw.get("agent_status").and_then(Value::as_str), 64)
        .unwrap_or_else(|| {
            if kind == EventKind::SubagentStarted {
                "running".to_owned()
            } else {
                "completed".to_owned()
            }
        });
    let source = bounded_clean_text(raw.get("source").and_then(Value::as_str), 64);
    if kind == EventKind::SubagentStarted {
        transaction
            .execute(
                "INSERT INTO session_subagents (
                   session_id, agent_id, agent_type, status, source, active, started_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)
                 ON CONFLICT(session_id, agent_id) DO UPDATE SET
                   agent_type = COALESCE(excluded.agent_type, session_subagents.agent_type),
                   status = excluded.status,
                   source = COALESCE(excluded.source, session_subagents.source),
                   active = 1",
                params![
                    session_id,
                    agent_id,
                    agent_type,
                    agent_status,
                    source,
                    occurred_at
                ],
            )
            .map_err(storage_error)?;
    } else {
        transaction
            .execute(
                "INSERT INTO session_subagents (
                   session_id, agent_id, agent_type, status, source,
                   active, started_at, stopped_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?6)
                 ON CONFLICT(session_id, agent_id) DO UPDATE SET
                   agent_type = COALESCE(excluded.agent_type, session_subagents.agent_type),
                   status = excluded.status,
                   source = COALESCE(excluded.source, session_subagents.source),
                   active = 0,
                   stopped_at = COALESCE(session_subagents.stopped_at, excluded.stopped_at)",
                params![
                    session_id,
                    agent_id,
                    agent_type,
                    agent_status,
                    source,
                    occurred_at
                ],
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

fn is_meaningful_activity(kind: EventKind, raw: &Value) -> bool {
    match kind {
        EventKind::SessionStarted | EventKind::SessionEnded | EventKind::Stopped => false,
        EventKind::Notification => is_structured_question(raw),
        EventKind::Unknown => false,
        _ => true,
    }
}

fn turn_contains_meaningful_activity(
    transaction: &Transaction<'_>,
    turn_id: &str,
) -> Result<bool, StoreError> {
    transaction
        .query_row(
            "SELECT EXISTS(
               SELECT 1 FROM events
               WHERE turn_id = ?1
                 AND type IN (
                   'prompt.submitted', 'tool.started', 'tool.finished',
                   'tool.failed', 'approval.requested', 'approval.denied',
                   'question.requested', 'elicitation.requested',
                   'subagent.started', 'subagent.stopped',
                   'task.created', 'task.completed', 'plan.updated',
                   'approval.auto_review.started',
                   'approval.auto_review.completed', 'session.compacting',
                   'turn.interrupted', 'turn.failed'
                 )
             )",
            [turn_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(storage_error)
}

fn provider_handled_permission_state(
    permission_mode: Option<&str>,
    raw: &Value,
) -> (&'static str, Option<&'static str>, String) {
    let full_access = matches!(
        permission_mode,
        Some(
            "bypassPermissions"
                | "danger-full-access"
                | "full-access"
                | "fullAccess"
                | "full_access"
                | "never"
        )
    ) || raw
        .get("approval_policy")
        .or_else(|| raw.get("approvalPolicy"))
        .and_then(Value::as_str)
        .is_some_and(|policy| policy == "never");
    if full_access {
        (
            "thinking",
            None,
            "Codex full-access mode does not require user approval".to_owned(),
        )
    } else if permission_mode == Some("dontAsk") {
        (
            "thinking",
            None,
            "Codex non-interactive mode will not request user approval".to_owned(),
        )
    } else {
        (
            "thinking",
            Some("provider"),
            "Codex is reviewing permissions automatically".to_owned(),
        )
    }
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
    let visible_prompt = normalize_task_title_whitespace_entities(&user_visible_prompt(prompt));
    let normalized = visible_prompt
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        return None;
    }
    let sentence = first_task_title_sentence(&normalized).trim();
    let mut title = sentence
        .chars()
        .take(MAX_TASK_TITLE_CHARS)
        .collect::<String>();
    if sentence.chars().count() > MAX_TASK_TITLE_CHARS {
        title.push('…');
    }
    (!title.is_empty()).then_some(title)
}

fn normalize_task_title_whitespace_entities(value: &str) -> String {
    [
        "&#x20;", "&#X20;", "&#32;", "&nbsp;", "&NBSP;", "&#xa0;", "&#xA0;", "&#160;",
    ]
    .into_iter()
    .fold(value.to_owned(), |normalized, entity| {
        normalized.replace(entity, " ")
    })
}

fn first_task_title_sentence(normalized: &str) -> &str {
    let mut characters = normalized.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        let next = characters.peek().map(|(_, value)| *value);
        let boundary = match character {
            '。' | '！' | '？' | '!' | '?' => true,
            // Dots inside versions, filenames, hostnames, URLs and identifiers
            // are content, not sentence boundaries. A dot followed by space or
            // end-of-input retains the original first-sentence behavior.
            '.' => next.is_none_or(char::is_whitespace),
            _ => false,
        };
        if boundary {
            return &normalized[..index + character.len_utf8()];
        }
    }
    normalized
}

/// Removes product-owned context envelopes before deriving the bounded task
/// summary. These envelopes are transport metadata rather than user-authored
/// prompt content and must never become the visible task title.
fn user_visible_prompt(prompt: &str) -> String {
    const INTERNAL_ENVELOPES: [(&str, &str); 5] = [
        ("<in-app-browser-context", "</in-app-browser-context>"),
        ("<environment_context", "</environment_context>"),
        ("<app-context", "</app-context>"),
        ("<source_thread_id", "</source_thread_id>"),
        ("<source_turn_id", "</source_turn_id>"),
    ];
    const REQUEST_MARKER: &str = "## My request:";

    let mut visible = prompt.to_owned();
    unwrap_codex_delegation_input(&mut visible);
    for (open, close) in INTERNAL_ENVELOPES {
        while let Some(start) = visible.find(open) {
            let tagged = &visible[start..];
            let Some(close_offset) = tagged.find(close) else {
                visible.truncate(start);
                break;
            };
            let end = start + close_offset + close.len();
            visible.replace_range(start..end, " ");
        }
    }

    let visible = visible
        .rfind(REQUEST_MARKER)
        .map_or(visible.as_str(), |index| {
            &visible[index + REQUEST_MARKER.len()..]
        });
    visible.trim().to_owned()
}

/// Codex-created tasks wrap the actual user prompt in a delegation envelope.
/// Keep only the generated `<input>` value and discard source thread metadata.
fn unwrap_codex_delegation_input(visible: &mut String) {
    const OPEN: &str = "<codex_delegation";
    const CLOSE: &str = "</codex_delegation>";
    const INPUT_OPEN: &str = "<input";
    const INPUT_CLOSE: &str = "</input>";

    while let Some(start) = visible.find(OPEN) {
        let tagged = &visible[start..];
        let Some(close_offset) = tagged.find(CLOSE) else {
            visible.truncate(start);
            break;
        };
        let end = start + close_offset + CLOSE.len();
        let body = &visible[start..start + close_offset];
        let input = body.find(INPUT_OPEN).and_then(|input_start| {
            let open_tail = &body[input_start + INPUT_OPEN.len()..];
            let open_end = open_tail.find('>')?;
            let value_start = input_start + INPUT_OPEN.len() + open_end + 1;
            let value_tail = &body[value_start..];
            let value_end = value_tail.find(INPUT_CLOSE)?;
            Some(value_tail[..value_end].to_owned())
        });
        visible.replace_range(start..end, input.as_deref().unwrap_or(" "));
    }
}

fn remove_internal_context_task_titles(connection: &Connection) -> Result<(), StoreError> {
    connection
        .execute(
            "UPDATE sessions SET title = NULL
             WHERE title LIKE '<in-app-browser-context%'
                OR title LIKE '<codex_delegation%'
                OR title LIKE '<source_thread_id%'
                OR title LIKE '<environment_context%'
                OR title LIKE '<app-context%'",
            [],
        )
        .map(|_| ())
        .map_err(storage_error)
}

fn normalize_existing_task_title_whitespace(connection: &Connection) -> Result<(), StoreError> {
    connection
        .execute(
            "UPDATE sessions
             SET title = TRIM(
               REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(
                 title,
                 '&#x20;', ' '), '&#X20;', ' '), '&#32;', ' '),
                 '&nbsp;', ' '), '&NBSP;', ' '), '&#xa0;', ' '),
                 '&#xA0;', ' '), '&#160;', ' ')
             )
             WHERE title LIKE '%&#x20;%'
                OR title LIKE '%&#X20;%'
                OR title LIKE '%&#32;%'
                OR title LIKE '%&nbsp;%'
                OR title LIKE '%&NBSP;%'
                OR title LIKE '%&#xa0;%'
                OR title LIKE '%&#xA0;%'
                OR title LIKE '%&#160;%'",
            [],
        )
        .map(|_| ())
        .map_err(storage_error)
}

fn project_event<'a>(
    kind: EventKind,
    raw: &Value,
    current: &'a str,
) -> (&'a str, Option<&'static str>, String) {
    match kind {
        EventKind::SessionStarted => ("idle", None, "Waiting for a new task".to_owned()),
        EventKind::SessionEnded => ("idle", None, "Session ended".to_owned()),
        EventKind::PromptSubmitted => ("thinking", None, "Thinking".to_owned()),
        EventKind::ToolStarted => (
            "tool_running",
            None,
            format!(
                "Running {}",
                raw.get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
            ),
        ),
        EventKind::ToolFinished | EventKind::ToolFailed => {
            ("thinking", None, "Continuing to think".to_owned())
        }
        EventKind::PermissionRequested => (
            "awaiting_approval",
            Some("widget"),
            "Waiting for your approval".to_owned(),
        ),
        EventKind::QuestionRequested | EventKind::ElicitationRequested => (
            "awaiting_approval",
            Some("widget"),
            "Waiting for your answer".to_owned(),
        ),
        EventKind::PermissionDenied => (
            "thinking",
            None,
            "The operation was denied in the Agent".to_owned(),
        ),
        EventKind::AutoReviewStarted => (
            "thinking",
            Some("provider"),
            "Codex is reviewing permissions automatically".to_owned(),
        ),
        EventKind::AutoReviewCompleted => {
            let status = raw
                .pointer("/review/status")
                .or_else(|| raw.get("status"))
                .and_then(Value::as_str)
                .unwrap_or("completed");
            let label = match status {
                "approved" => "Codex automatic review approved the request",
                "denied" => "Codex automatic review denied the request",
                "timedOut" => "Codex automatic review timed out; waiting for a later status",
                "aborted" => "Codex automatic review was cancelled; waiting for a later status",
                _ => "Codex automatic review ended",
            };
            ("thinking", None, label.to_owned())
        }
        EventKind::Compacting => ("compacting", None, "Compacting context".to_owned()),
        EventKind::Stopped if has_background_work(raw) => {
            let count = ["background_tasks", "session_crons"]
                .iter()
                .filter_map(|field| raw.get(*field).and_then(Value::as_array))
                .map(Vec::len)
                .sum::<usize>();
            (
                "tool_running",
                None,
                format!("{count} background tasks still running"),
            )
        }
        EventKind::Stopped => ("response_finished", None, "Turn completed".to_owned()),
        EventKind::Interrupted => ("failed", None, "Turn interrupted".to_owned()),
        EventKind::Failed => ("failed", None, "Run failed".to_owned()),
        EventKind::Unknown => (
            current,
            None,
            "Unrecognized event; the Provider version may be incompatible".to_owned(),
        ),
        EventKind::SubagentStarted => ("thinking", None, "A subagent is running".to_owned()),
        EventKind::Notification if is_structured_question(raw) => (
            "awaiting_approval",
            Some("provider"),
            "Waiting for your answer".to_owned(),
        ),
        EventKind::Notification
        | EventKind::SubagentStopped
        | EventKind::TaskCreated
        | EventKind::TaskCompleted
        | EventKind::PlanUpdated => (current, None, "Activity updated".to_owned()),
    }
}

fn execution_state_is_active(state: &str) -> bool {
    matches!(state, "thinking" | "tool_running" | "compacting")
}

fn reconcile_agent_execution_interval(
    transaction: &Transaction<'_>,
    session_id: &str,
    turn_id: Option<&str>,
    next_state: &str,
    occurred_at: i64,
    reason: &str,
) -> Result<(), StoreError> {
    let open = transaction
        .query_row(
            "SELECT id, turn_id, started_at
             FROM agent_execution_intervals
             WHERE session_id = ?1 AND ended_at IS NULL",
            [session_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?;
    let active = execution_state_is_active(next_state);
    let turn_changed = open
        .as_ref()
        .is_some_and(|(_, open_turn, _)| turn_id.is_some() && open_turn.as_deref() != turn_id);
    if !active || turn_changed {
        if let Some((interval_id, _, started_at)) = open.as_ref() {
            transaction
                .execute(
                    "UPDATE agent_execution_intervals
                     SET ended_at = MAX(started_at, ?2), end_reason = ?3
                     WHERE id = ?1 AND ended_at IS NULL",
                    params![interval_id, occurred_at.max(*started_at), reason],
                )
                .map_err(storage_error)?;
        }
    }
    if active && (open.is_none() || turn_changed) {
        transaction
            .execute(
                "INSERT INTO agent_execution_intervals(
                   session_id, turn_id, started_at, start_reason
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![session_id, turn_id, occurred_at, reason],
            )
            .map_err(storage_error)?;
    }
    Ok(())
}

fn current_open_turn_id(
    transaction: &Transaction<'_>,
    session_id: &str,
) -> Result<Option<String>, StoreError> {
    transaction
        .query_row(
            "SELECT id FROM turns
             WHERE session_id = ?1 AND ended_at IS NULL
             ORDER BY ordinal DESC LIMIT 1",
            [session_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OperationClassification {
    primary_category: OperationCategory,
    risk: &'static str,
    risk_codes: Vec<AttentionRiskCode>,
}

impl OperationClassification {
    fn new(primary_category: OperationCategory) -> Self {
        use AttentionRiskCode::{
            HighImpact, Irreversible, ReadOnlyIntent, ReviewOriginal, SideEffects, UndoWindow,
            UnknownImpact,
        };
        use OperationCategory::{
            CredentialsAccess, FileCreate, FileDelete, FileEdit, GitPush, GitReadOnly,
            NetworkFetch, NetworkFetchAndExecute, PackageInstall, ProcessSpawn, ShellReadOnly,
            ShellRemove, ShellUnknown, VersionControlInternal,
        };

        let (risk, risk_codes) = match primary_category {
            CredentialsAccess
            | VersionControlInternal
            | NetworkFetchAndExecute
            | ShellRemove
            | GitPush
            | FileDelete => ("high", vec![HighImpact, Irreversible]),
            PackageInstall | FileEdit | FileCreate | ProcessSpawn | NetworkFetch => {
                ("med", vec![SideEffects, ReviewOriginal])
            }
            GitReadOnly | ShellReadOnly => ("low", vec![ReadOnlyIntent, UndoWindow]),
            ShellUnknown => ("unknown", vec![UnknownImpact, ReviewOriginal]),
        };
        Self {
            primary_category,
            risk,
            risk_codes,
        }
    }
}

/// Derives a bounded, non-sensitive workflow label while the raw Provider
/// payload is still in memory. Commands and tool inputs are never persisted;
/// only this allowlisted category reaches timelines and companion displays.
fn workflow_tool_category(tool_name: Option<&str>, raw: &Value) -> String {
    let tool = tool_name.unwrap_or_default().trim().to_ascii_lowercase();
    if tool.starts_with("mcp__node_repl__")
        || tool.contains("code_interpreter")
        || tool.contains("functions__exec")
        || tool == "exec" && raw.pointer("/tool_input/code").is_some()
    {
        return "code_execution".to_owned();
    }
    if tool.contains("computer_use")
        || tool.contains("cua_repl")
        || tool.contains("browser")
        || tool.contains("chrome")
    {
        return "interaction".to_owned();
    }
    match tool.as_str() {
        "read" | "view_image" => return "file_read".to_owned(),
        "grep" | "glob" | "find" | "search" => return "file_search".to_owned(),
        "edit" | "write" | "multiedit" | "apply_patch" => return "file_edit".to_owned(),
        "websearch" | "webfetch" | "web_search" | "web_fetch" => return "network".to_owned(),
        "write_stdin" | "wait" => return "process".to_owned(),
        _ => {}
    }

    let command = raw
        .pointer("/tool_input/command")
        .or_else(|| raw.pointer("/tool_input/cmd"))
        .or_else(|| raw.get("command"))
        .or_else(|| raw.get("cmd"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    // A script body may mention tools or URLs without running them. Recognize
    // the interpreter before searching the body for command categories.
    let head = command
        .lines()
        .find(|line| {
            let line = line.trim();
            !line.is_empty() && !line.starts_with('#') && !line.starts_with("set ")
        })
        .unwrap_or_default();
    if command_starts_with_any(
        head,
        &["python", "python3", "node", "ruby", "perl", "osascript"],
    ) && !command_runs_python_module(head, "unittest")
        && !command_runs_python_module(head, "pytest")
        && !command_has_subcommand(head, "node", "--test")
        && !command_has_subcommand(head, "node", "--check")
    {
        return "code_execution".to_owned();
    }

    if command_is_test(&command) {
        "test".to_owned()
    } else if command_has_subcommand(&command, "cargo", "check")
        || command_has_subcommand(&command, "cargo", "clippy")
        || command_has_subcommand(&command, "cargo", "fmt")
        || command_has_subcommand(&command, "node", "--check")
        || command_starts_with_any(&command, &["swiftlint", "eslint", "ruff"])
    {
        "code_check".to_owned()
    } else if command_is_build(&command) {
        "build".to_owned()
    } else if command_starts_with_any(&command, &["git", "gh"]) {
        "version_control".to_owned()
    } else if is_package_install(&command) {
        "package".to_owned()
    } else if command_starts_with_any(&command, &["curl", "wget"])
        || command.contains("http://")
        || command.contains("https://")
    {
        "network".to_owned()
    } else if is_file_delete(&command) || is_file_edit(&command) || is_file_create(&command) {
        "file_edit".to_owned()
    } else if command_starts_with_any(&command, &["rg", "grep", "find", "fd", "ls"]) {
        "file_search".to_owned()
    } else if command_starts_with_any(
        &command,
        &["cat", "head", "tail", "sed", "stat", "wc", "pwd", "which"],
    ) {
        "file_read".to_owned()
    } else if is_process_spawn(&command) {
        "process".to_owned()
    } else if matches!(tool.as_str(), "bash" | "shell" | "exec_command") {
        "shell".to_owned()
    } else {
        "tool".to_owned()
    }
}

fn structured_validation_status(
    kind: EventKind,
    tool_category: Option<&str>,
    raw: &Value,
) -> Option<&'static str> {
    if !matches!(tool_category, Some("test" | "build" | "code_check")) {
        return None;
    }
    match kind {
        EventKind::ToolStarted => Some("running"),
        EventKind::ToolFailed => Some("failed"),
        EventKind::ToolFinished => match explicit_validation_success(raw) {
            Some(true) => Some("passed"),
            Some(false) => Some("failed"),
            None => Some("unverifiable"),
        },
        _ => None,
    }
}

fn explicit_validation_success(raw: &Value) -> Option<bool> {
    for pointer in [
        "/tool_response/exit_code",
        "/tool_response/exitCode",
        "/tool_response/metadata/exit_code",
        "/tool_result/exit_code",
        "/tool_result/exitCode",
        "/exit_code",
        "/exitCode",
    ] {
        if let Some(exit_code) = raw.pointer(pointer).and_then(Value::as_i64) {
            return Some(exit_code == 0);
        }
    }
    for pointer in ["/tool_response/success", "/tool_result/success", "/success"] {
        if let Some(success) = raw.pointer(pointer).and_then(Value::as_bool) {
            return Some(success);
        }
    }
    for pointer in ["/tool_response/status", "/tool_result/status", "/status"] {
        match raw.pointer(pointer).and_then(Value::as_str) {
            Some("success" | "succeeded" | "ok" | "passed") => return Some(true),
            Some("failure" | "failed" | "error") => return Some(false),
            _ => {}
        }
    }
    None
}

fn workflow_category_is_allowlisted(category: &str) -> bool {
    matches!(
        category,
        "test"
            | "build"
            | "code_check"
            | "version_control"
            | "package"
            | "network"
            | "file_edit"
            | "file_read"
            | "file_search"
            | "process"
            | "code_execution"
            | "interaction"
            | "shell"
            | "tool"
    )
}

fn command_is_test(command: &str) -> bool {
    command_has_subcommand(command, "cargo", "test")
        || command_has_subcommand(command, "swift", "test")
        || command_has_subcommand(command, "node", "--test")
        || command_has_subcommand(command, "go", "test")
        || command_has_subcommand(command, "npm", "test")
        || command_has_subcommand(command, "npm", "run") && command.contains(" test")
        || command_has_subcommand(command, "pnpm", "test")
        || command_has_subcommand(command, "pnpm", "run") && command.contains(" test")
        || command_has_subcommand(command, "yarn", "test")
        || command_starts_with_any(command, &["pytest", "vitest", "jest"])
        || command_runs_python_module(command, "unittest")
        || command_runs_python_module(command, "pytest")
        || (command.contains("xcodebuild") && command.contains(" test"))
        || (command.contains("gradle") && command.contains("test"))
}

fn command_runs_python_module(command: &str, module: &str) -> bool {
    let tokens = command.split_whitespace().collect::<Vec<_>>();
    tokens.windows(3).any(|window| {
        let executable = Path::new(window[0])
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        (executable == "python" || executable.starts_with("python3"))
            && window[1] == "-m"
            && window[2].trim_matches(['\'', '"']) == module
    })
}

fn command_is_build(command: &str) -> bool {
    command_has_subcommand(command, "cargo", "build")
        || command_has_subcommand(command, "swift", "build")
        || command_has_subcommand(command, "go", "build")
        || command_has_subcommand(command, "npm", "run") && command.contains(" build")
        || command_has_subcommand(command, "pnpm", "build")
        || command_has_subcommand(command, "pnpm", "run") && command.contains(" build")
        || command_has_subcommand(command, "yarn", "build")
        || command.contains("xcodebuild")
        || (command.contains("gradle") && command.contains("build"))
}

fn classify_operation(tool_name: Option<&str>, raw: &Value) -> OperationClassification {
    let tool = tool_name.unwrap_or_default();
    let input = raw.get("tool_input");
    let command = input
        .and_then(|value| value.get("command"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let target_paths = ["file_path", "path", "grantRoot"]
        .iter()
        .filter_map(|key| {
            input
                .and_then(|value| value.get(*key))
                .and_then(Value::as_str)
        })
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    let path_or_command_mentions_credentials =
        target_paths.iter().any(|path| is_credential_path(path))
            || credential_markers()
                .iter()
                .any(|marker| command.contains(marker));
    if path_or_command_mentions_credentials {
        return classified_category(OperationCategory::CredentialsAccess, &command);
    }

    let structured_write = matches!(tool, "Edit" | "Write" | "MultiEdit" | "apply_patch");
    if (structured_write || shell_has_write_intent(&command))
        && (target_paths.iter().any(|path| is_git_internal_path(path))
            || command_mentions_git_internal_path(&command))
    {
        return classified_category(OperationCategory::VersionControlInternal, &command);
    }
    if is_network_fetch_and_execute(&command) {
        return classified_category(OperationCategory::NetworkFetchAndExecute, &command);
    }
    if is_recursive_or_forced_remove(&command) {
        return classified_category(OperationCategory::ShellRemove, &command);
    }
    if command_is_git_subcommand(&command, "push") {
        return classified_category(OperationCategory::GitPush, &command);
    }
    if is_file_delete(&command) {
        return classified_category(OperationCategory::FileDelete, &command);
    }
    // A compound shell expression must never inherit the safety label of one
    // recognizable segment. For example, `git status ; rm file` previously
    // returned GitReadOnly before this check ran. Keep known high-impact
    // matches above, then fail closed for every remaining composition.
    if has_shell_composition(&command) {
        if is_legacy_high_impact(&command) {
            return high_impact_classification(OperationCategory::ShellUnknown);
        }
        return OperationClassification {
            primary_category: OperationCategory::ShellUnknown,
            risk: "unknown",
            risk_codes: vec![
                AttentionRiskCode::CompoundSyntax,
                AttentionRiskCode::ReviewOriginal,
            ],
        };
    }
    if is_package_install(&command) {
        return classified_category(OperationCategory::PackageInstall, &command);
    }
    if is_legacy_high_impact(&command) {
        return high_impact_classification(OperationCategory::ShellUnknown);
    }
    if matches!(tool, "Edit" | "MultiEdit" | "apply_patch") || is_file_edit(&command) {
        return classified_category(OperationCategory::FileEdit, &command);
    }
    if tool == "Write" || is_file_create(&command) {
        return classified_category(OperationCategory::FileCreate, &command);
    }
    if is_process_spawn(&command) {
        return classified_category(OperationCategory::ProcessSpawn, &command);
    }
    if matches!(tool, "WebFetch" | "WebSearch")
        || command_starts_with_any(&command, &["curl", "wget"])
        || request_enables_network(input)
    {
        return classified_category(OperationCategory::NetworkFetch, &command);
    }
    if ["status", "diff", "log"]
        .iter()
        .any(|subcommand| command_is_git_subcommand(&command, subcommand))
    {
        return classified_category(OperationCategory::GitReadOnly, &command);
    }
    if matches!(tool, "Read" | "Glob" | "Grep") || is_read_only_shell(&command) {
        return classified_category(OperationCategory::ShellReadOnly, &command);
    }
    OperationClassification::new(OperationCategory::ShellUnknown)
}

fn classified_category(category: OperationCategory, command: &str) -> OperationClassification {
    if is_legacy_high_impact(command) {
        high_impact_classification(category)
    } else {
        OperationClassification::new(category)
    }
}

fn high_impact_classification(category: OperationCategory) -> OperationClassification {
    OperationClassification {
        primary_category: category,
        risk: "high",
        risk_codes: vec![
            AttentionRiskCode::HighImpact,
            AttentionRiskCode::Irreversible,
        ],
    }
}

fn is_legacy_high_impact(command: &str) -> bool {
    [
        "sudo ",
        "chmod 777",
        "drop table",
        "docker system prune",
        "kill -9 1",
    ]
    .iter()
    .any(|marker| command.contains(marker))
}

fn credential_markers() -> &'static [&'static str] {
    &[
        "/.aws/credentials",
        "/.aws/config",
        "/.ssh",
        "/.gnupg/",
        "/.config/gcloud/",
        "/.kube/config",
        "/.docker/config.json",
        "/keychains/",
        "id_rsa",
        "id_ed25519",
        "service-account.json",
        "service_account.json",
        ".env",
    ]
}

fn is_credential_path(path: &str) -> bool {
    credential_markers()
        .iter()
        .any(|marker| path.contains(marker))
}

fn is_git_internal_path(path: &str) -> bool {
    path == ".git"
        || path.starts_with(".git/")
        || path.ends_with("/.git")
        || path.contains("/.git/")
}

fn command_mentions_git_internal_path(command: &str) -> bool {
    command
        .split_whitespace()
        .map(|word| word.trim_matches(['\'', '"', ';', '|', '&', '(', ')']))
        .any(is_git_internal_path)
}

fn shell_tokens(command: &str) -> Vec<&str> {
    command
        .split_whitespace()
        .map(|word| word.trim_matches(['\'', '"', ';', '|', '&', '(', ')']))
        .filter(|word| !word.is_empty())
        .collect()
}

fn command_starts_with_any(command: &str, executables: &[&str]) -> bool {
    let tokens = shell_tokens(command);
    let executable = tokens
        .iter()
        .copied()
        .find(|token| !token.contains('=') && !matches!(*token, "env" | "sudo" | "command"));
    executable.is_some_and(|token| {
        Path::new(token)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| executables.contains(&name))
    })
}

fn command_is_git_subcommand(command: &str, expected: &str) -> bool {
    let tokens = shell_tokens(command);
    tokens.windows(2).any(|pair| {
        Path::new(pair[0])
            .file_name()
            .and_then(|name| name.to_str())
            == Some("git")
            && pair[1] == expected
    })
}

fn is_network_fetch_and_execute(command: &str) -> bool {
    let fetches = command.contains("curl ") || command.contains("wget ");
    let executes = [
        "| sh", "|sh", "| bash", "|bash", "| zsh", "|zsh", "$(curl", "$(wget",
    ]
    .iter()
    .any(|marker| command.contains(marker));
    fetches && executes
}

fn is_recursive_or_forced_remove(command: &str) -> bool {
    let tokens = shell_tokens(command);
    let Some(rm_index) = tokens.iter().position(|token| {
        Path::new(token).file_name().and_then(|name| name.to_str()) == Some("rm")
    }) else {
        return false;
    };
    tokens[rm_index + 1..].iter().any(|token| {
        *token == "--recursive"
            || *token == "--force"
            || (token.starts_with('-')
                && !token.starts_with("--")
                && token
                    .chars()
                    .any(|character| matches!(character, 'r' | 'R' | 'f')))
    })
}

fn is_package_install(command: &str) -> bool {
    const INSTALL_COMMANDS: &[(&str, &[&str])] = &[
        ("npm", &["install", "i"]),
        ("pnpm", &["install", "add"]),
        ("yarn", &["install", "add"]),
        ("brew", &["install"]),
        ("apt", &["install"]),
        ("apt-get", &["install"]),
        ("pip", &["install"]),
        ("pip3", &["install"]),
        ("cargo", &["install"]),
        ("gem", &["install"]),
    ];
    INSTALL_COMMANDS.iter().any(|(executable, subcommands)| {
        subcommands
            .iter()
            .any(|subcommand| command_has_subcommand(command, executable, subcommand))
    })
}

fn command_has_subcommand(command: &str, executable: &str, subcommand: &str) -> bool {
    let tokens = shell_tokens(command);
    tokens.windows(2).any(|pair| {
        Path::new(pair[0])
            .file_name()
            .and_then(|name| name.to_str())
            == Some(executable)
            && pair[1] == subcommand
    })
}

fn is_file_delete(command: &str) -> bool {
    command_starts_with_any(command, &["rm", "unlink", "rmdir"])
}

fn is_file_edit(command: &str) -> bool {
    command.contains("sed -i")
        || command.contains("perl -pi")
        || command_starts_with_any(command, &["mv", "cp", "tee"])
        || (command.contains('>') && !command.contains("2>"))
}

fn is_file_create(command: &str) -> bool {
    command_starts_with_any(command, &["touch", "mkdir"])
}

fn is_process_spawn(command: &str) -> bool {
    command_starts_with_any(command, &["nohup"])
        || command.contains(" disown")
        || (command.trim_end().ends_with('&') && !command.trim_end().ends_with("&&"))
        || [" serve", " server", " watch", " daemon"]
            .iter()
            .any(|marker| command.contains(marker))
}

fn request_enables_network(input: Option<&Value>) -> bool {
    input
        .and_then(|value| value.pointer("/permissions/network/enabled"))
        .and_then(Value::as_bool)
        == Some(true)
}

fn is_read_only_shell(command: &str) -> bool {
    if command.is_empty() || has_shell_composition(command) {
        return false;
    }
    command_starts_with_any(
        command,
        &[
            "ls", "rg", "grep", "cat", "head", "tail", "pwd", "wc", "which",
        ],
    )
}

fn has_shell_composition(command: &str) -> bool {
    ["|", ">", "<", "$(", "`", "&&", ";"]
        .iter()
        .any(|marker| command.contains(marker))
}

fn shell_has_write_intent(command: &str) -> bool {
    is_recursive_or_forced_remove(command)
        || is_file_delete(command)
        || is_file_edit(command)
        || is_file_create(command)
        || command_is_git_subcommand(command, "push")
        || command_is_git_subcommand(command, "commit")
}

fn risk_notes(codes: &[AttentionRiskCode]) -> Vec<&'static str> {
    codes
        .iter()
        .map(|code| match code {
            AttentionRiskCode::HighImpact => "High-impact operation detected",
            AttentionRiskCode::Irreversible => {
                "The operation cannot be undone after it is submitted"
            }
            AttentionRiskCode::CompoundSyntax => "The command contains compound syntax",
            AttentionRiskCode::ReadOnlyIntent => {
                "Read-only intent; this rule is not a security guarantee"
            }
            AttentionRiskCode::UndoWindow => {
                "The decision has not been sent and can be withdrawn during the 3-second window"
            }
            AttentionRiskCode::SideEffects => "May run project code or produce side effects",
            AttentionRiskCode::UnknownImpact => "The impact of this operation is unknown",
            AttentionRiskCode::ReviewOriginal => "Review the original window",
        })
        .collect()
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
    let name = name
        .strip_prefix("functions.")
        .or_else(|| name.strip_prefix("tools."))
        .unwrap_or(name);
    match name {
        "Agent"
        | "Bash"
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
        | "request_permissions"
        | "request_plugin_install"
        | "exec"
        | "wait"
        | "request_user_input_async"
        | "exec_command"
        | "write_stdin"
        | "view_image"
        | "read_mcp_resource"
        | "list_mcp_resources"
        | "list_mcp_resource_templates"
        | "update_plan" => name.to_owned(),
        _ => sanitized_mcp_tool_name(name).unwrap_or_else(|| "Unknown".to_owned()),
    }
}

/// MCP names use a stable `mcp__server__tool` identity. Expose only bounded
/// identifier segments; arguments, resource names and command content remain
/// outside the projection.
fn sanitized_mcp_tool_name(name: &str) -> Option<String> {
    if let Some(remainder) = name.strip_prefix("mcp__") {
        let mut segments = remainder.split("__");
        let server = safe_tool_identifier(segments.next()?, 32)?;
        let tool = safe_tool_identifier(segments.next()?, 40)?;
        if segments.next().is_some() {
            return None;
        }
        return Some(format!("MCP {server}.{tool}"));
    }
    // Timeline reads sanitize the already-bounded value a second time. Keep
    // this projection idempotent so a safe MCP identity does not degrade to
    // an anonymous "Tool" after pagination.
    let remainder = name.strip_prefix("MCP ")?;
    let (server, tool) = remainder.split_once('.')?;
    let server = safe_tool_identifier(server, 32)?;
    let tool = safe_tool_identifier(tool, 40)?;
    Some(format!("MCP {server}.{tool}"))
}

fn safe_tool_identifier(value: &str, limit: usize) -> Option<String> {
    if value.is_empty()
        || value.chars().count() > limit
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        return None;
    }
    Some(value.to_owned())
}

/// Extracts only a bounded basename for the live dashboard. Full paths, tool
/// input, commands and file contents never enter the session projection.
fn tool_target_label(raw: &Value) -> Option<String> {
    let input = raw
        .get("tool_input")
        .or_else(|| raw.get("toolInput"))
        .or_else(|| raw.get("input"))
        .unwrap_or(raw);
    let value = [
        "file_path",
        "filePath",
        "notebook_path",
        "notebookPath",
        "path",
    ]
    .into_iter()
    .find_map(|key| input.get(key).and_then(Value::as_str))?;
    let normalized = value.trim().trim_end_matches(['/', '\\']);
    let basename = normalized
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim();
    if basename.is_empty() || matches!(basename, "." | "..") {
        return None;
    }
    let safe = basename
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_TOOL_TARGET_CHARS)
        .collect::<String>();
    (!safe.is_empty()).then_some(safe)
}

/// Captures an explicit tool working directory for local Review only. The
/// value stays in SQLite and is never part of SessionRecord, snapshots, or
/// Companion projections. Filesystem resolution is deferred to the on-demand
/// Review endpoint so Hook ingestion remains nonblocking.
fn review_working_directory(raw: &Value, session_cwd: Option<&str>) -> Option<String> {
    let input = raw
        .get("tool_input")
        .or_else(|| raw.get("toolInput"))
        .or_else(|| raw.get("input"))
        .unwrap_or(raw);
    let value = ["workdir", "work_dir", "working_directory", "cwd"]
        .into_iter()
        .find_map(|key| input.get(key).and_then(Value::as_str))?
        .trim();
    if value.is_empty() || value.len() > 4_096 || value.chars().any(char::is_control) {
        return None;
    }
    let candidate = PathBuf::from(value);
    let candidate = if candidate.is_absolute() {
        candidate
    } else {
        let parent = Path::new(session_cwd?);
        if !parent.is_absolute() {
            return None;
        }
        parent.join(candidate)
    };
    let encoded = candidate.to_string_lossy();
    (!encoded.is_empty()).then(|| encoded.into_owned())
}

fn event_summary(raw: &Value, kind: EventKind) -> Option<String> {
    match kind {
        EventKind::PermissionRequested => {
            raw.get("tool_name").and_then(Value::as_str).map(|tool| {
                if tool == "request_permissions" {
                    "Codex requests approval in its original interface".to_owned()
                } else if tool == "request_plugin_install" {
                    "Codex requests plugin installation or connection in its original interface"
                        .to_owned()
                } else {
                    format!("Request to run {}", sanitized_tool_name(tool))
                }
            })
        }
        EventKind::QuestionRequested => Some("Claude AskUserQuestion".to_owned()),
        EventKind::ElicitationRequested => Some("Claude Elicitation".to_owned()),
        EventKind::PlanUpdated => Some("Provider plan updated".to_owned()),
        EventKind::AutoReviewStarted => Some("Codex auto approval review started".to_owned()),
        EventKind::AutoReviewCompleted => Some("Codex auto approval review completed".to_owned()),
        EventKind::Interrupted => Some("Provider turn interrupted".to_owned()),
        EventKind::Unknown => Some("Unknown Provider event".to_owned()),
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
        EventKind::PlanUpdated => "plan.updated",
        EventKind::AutoReviewStarted => "approval.auto_review.started",
        EventKind::AutoReviewCompleted => "approval.auto_review.completed",
        EventKind::Compacting => "session.compacting",
        EventKind::Stopped => "turn.stopped",
        EventKind::Interrupted => "turn.interrupted",
        EventKind::Failed => "turn.failed",
        EventKind::Unknown => "unknown",
    }
}

fn stable_provider_event_key(parsed: &ParsedHookEvent, raw: &Value) -> Option<String> {
    let candidates: &[&str] = match parsed.kind {
        EventKind::ToolStarted | EventKind::ToolFinished | EventKind::ToolFailed => {
            &["tool_use_id", "toolUseId", "_codex_item_id", "itemId"]
        }
        EventKind::PermissionRequested | EventKind::QuestionRequested => {
            &["tool_use_id", "toolUseId", "_codex_item_id", "itemId"]
        }
        EventKind::TaskCreated | EventKind::TaskCompleted => &["task_id", "taskId"],
        EventKind::SubagentStarted | EventKind::SubagentStopped => {
            &["agent_id", "agentId", "subagent_id", "subagentId"]
        }
        EventKind::PromptSubmitted => &["prompt_id", "promptId", "turn_id", "turnId"],
        EventKind::Stopped | EventKind::Interrupted | EventKind::Failed => {
            &["turn_id", "turnId", "prompt_id", "promptId"]
        }
        EventKind::PlanUpdated => &[
            "plan_revision",
            "planRevision",
            "plan_version",
            "planVersion",
        ],
        _ => &[],
    };
    for key in candidates {
        let Some(raw_value) = raw.get(*key) else {
            continue;
        };
        let Some(value) = raw_value
            .as_str()
            .map(ToOwned::to_owned)
            .or_else(|| raw_value.as_u64().map(|value| value.to_string()))
        else {
            continue;
        };
        if !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control) {
            return Some(format!("{key}:{value}"));
        }
    }
    None
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
        return (
            "exact_conversation".to_owned(),
            "Open exact conversation".to_owned(),
        );
    }
    let iterm = app.contains("iterm") || bundle == "com.googlecode.iterm2";
    let terminal = app == "apple_terminal" || bundle == "com.apple.terminal";
    if (iterm && term_session_id.is_some()) || (terminal && term_tty.is_some()) {
        return ("terminal".to_owned(), "Open terminal".to_owned());
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
        return ("app_only".to_owned(), "Open application".to_owned());
    }
    if provider == "codex" && Uuid::parse_str(provider_session_id).is_ok() {
        return (
            "exact_conversation".to_owned(),
            "Open exact conversation".to_owned(),
        );
    }
    ("unsupported".to_owned(), "Jump is not supported".to_owned())
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

#[cfg(test)]
mod tests {
    use super::{
        act_attention_transaction, archive_task_transaction, classify_operation,
        delete_task_history_transaction, initialize, local_period_start, placeholders,
        read_metrics_and_token_usage, read_task_history, read_timeline, read_ui_snapshot,
        reconcile_legacy_token_aggregates, refresh_time_driven_attention, session_query_params,
        to_i64, ui_snapshot_batch_ranges, verify_token_ledger_projection_invariants,
        write_ui_settings_transaction, AttentionAction, CompletionTaskHidePolicy, StoreError,
        TaskHistoryMutation, TimelineReadOptions, SCHEMA_VERSION, SQLITE_MAX_VARIABLE_NUMBER,
    };
    use actrealm_core::{AttentionRiskCode, OperationCategory};
    use rusqlite::{params, params_from_iter, Connection};
    use serde_json::json;
    use std::collections::HashMap;
    use uuid::Uuid;

    #[test]
    fn local_only_upgrade_removes_collaboration_metadata_and_keeps_tasks_and_usage() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        connection.execute_batch("CREATE TABLE team_task_context_policies (team_id TEXT);
            CREATE TABLE team_timeline_event_bindings (team_id TEXT);
            CREATE TABLE transcript_sources (canonical_path TEXT);
            INSERT INTO transcript_sources VALUES ('/private/context.jsonl');
            ALTER TABLE events ADD COLUMN transcript_source_id TEXT;
            INSERT INTO sessions(id, provider, provider_session_id, exec_state, started_at, last_event_at)
                VALUES ('kept', 'codex', 'kept', 'thinking', 1, 2);
            INSERT INTO session_usage(provider, provider_session_id, token_total, usage_source, usage_quality, captured_at)
                VALUES ('codex', 'kept', 12345, 'fixture', 'official', 2);
            PRAGMA user_version = 37;").unwrap();
        initialize(&mut connection).unwrap();
        initialize(&mut connection).unwrap();
        let retired: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name IN
            ('team_task_context_policies', 'team_timeline_event_bindings', 'transcript_sources')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(retired, 0);
        assert_eq!(
            connection
                .query_row(
                    "SELECT token_total FROM session_usage WHERE provider_session_id = 'kept'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            12345
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT exec_state FROM sessions WHERE id = 'kept'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "thinking"
        );
    }
    #[test]
    fn usage_attribution_lineage_is_bounded_and_cycle_safe() {
        let mut metadata = HashMap::new();
        metadata.insert(
            ("codex".to_owned(), "child-a".to_owned()),
            super::UsageAttributionMetadata {
                parent_provider_session_id: Some("child-b".to_owned()),
                ..super::UsageAttributionMetadata::default()
            },
        );
        metadata.insert(
            ("codex".to_owned(), "child-b".to_owned()),
            super::UsageAttributionMetadata {
                parent_provider_session_id: Some("child-a".to_owned()),
                ..super::UsageAttributionMetadata::default()
            },
        );
        let lineage = super::usage_attribution_lineage("codex", "child-a", &metadata);
        assert_eq!(lineage.len(), 2);
        assert_eq!(lineage[0].1, "child-a");
        assert_eq!(lineage[1].1, "child-b");
    }

    #[test]
    fn execution_intervals_are_clipped_to_local_day_and_month_boundaries() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        let now = 1_786_989_600_000_u64;
        let day_start = local_period_start(now, false);
        connection
            .execute(
                "INSERT INTO sessions(
                   id, provider, provider_session_id, exec_state,
                   started_at, last_event_at
                 ) VALUES ('execution-period', 'codex', 'execution-period',
                           'response_finished', ?1, ?2)",
                params![to_i64(day_start.saturating_sub(3_600_000)), to_i64(now)],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO agent_execution_intervals(
                   session_id, started_at, ended_at, start_reason, end_reason
                 ) VALUES
                   ('execution-period', ?1, ?2, 'test', 'test'),
                   ('execution-period', ?3, ?4, 'test', 'test')",
                params![
                    to_i64(day_start.saturating_sub(3_600_000)),
                    to_i64(day_start.saturating_add(3_600_000)),
                    to_i64(now.saturating_sub(3_600_000)),
                    to_i64(now),
                ],
            )
            .unwrap();

        let totals = read_metrics_and_token_usage(&connection, now).unwrap().1;
        assert_eq!(totals.today_execution_time_seconds, 7_200);
        assert_eq!(totals.month_execution_time_seconds, 10_800);
        assert_eq!(totals.execution_time_seconds, 10_800);
    }

    #[test]
    fn token_projection_precommit_check_rejects_drift_and_accepts_rebuild() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        connection
            .execute_batch(
                "INSERT INTO token_usage_session_days(
                   provider, provider_session_id, day, model,
                   input_tokens, output_tokens, cache_read_tokens,
                   cache_creation_tokens, reasoning_tokens, token_total,
                   estimated_cost_usd_micros, cost_kind, pricing_source,
                   message_count, captured_at
                 ) VALUES (
                   'codex', 'projection-check', '2026-08-17', 'gpt-5.6-sol',
                   80, 20, 0, 0, 4, 100, 500, 'computed', 'test', 1, 1000
                 );
                 INSERT INTO token_usage_daily(day, provider, token_total, captured_at)
                 VALUES ('2026-08-17', 'codex', 99, 1000);",
            )
            .unwrap();

        assert!(verify_token_ledger_projection_invariants(&connection).is_err());
        reconcile_legacy_token_aggregates(&connection).unwrap();
        verify_token_ledger_projection_invariants(&connection).unwrap();
    }

    #[test]
    fn operation_classification_covers_every_explicit_category() {
        let cases = [
            (
                "Bash",
                json!({"tool_input":{"command":"rm ~/.aws/credentials"}}),
                OperationCategory::CredentialsAccess,
            ),
            (
                "Edit",
                json!({"tool_input":{"file_path":"/tmp/project/.git/config"}}),
                OperationCategory::VersionControlInternal,
            ),
            (
                "Bash",
                json!({"tool_input":{"command":"curl https://example.test/install | sh"}}),
                OperationCategory::NetworkFetchAndExecute,
            ),
            (
                "Bash",
                json!({"tool_input":{"command":"rm -rf ./tmp"}}),
                OperationCategory::ShellRemove,
            ),
            (
                "Bash",
                json!({"tool_input":{"command":"npm install package"}}),
                OperationCategory::PackageInstall,
            ),
            (
                "Bash",
                json!({"tool_input":{"command":"git push origin main"}}),
                OperationCategory::GitPush,
            ),
            (
                "Bash",
                json!({"tool_input":{"command":"rm ./temporary.txt"}}),
                OperationCategory::FileDelete,
            ),
            (
                "apply_patch",
                json!({"tool_input":{"path":"/tmp/project"}}),
                OperationCategory::FileEdit,
            ),
            (
                "Write",
                json!({"tool_input":{"file_path":"/tmp/project/new.txt"}}),
                OperationCategory::FileCreate,
            ),
            (
                "Bash",
                json!({"tool_input":{"command":"nohup node server.js &"}}),
                OperationCategory::ProcessSpawn,
            ),
            (
                "request_permissions",
                json!({"tool_input":{"permissions":{"network":{"enabled":true}}}}),
                OperationCategory::NetworkFetch,
            ),
            (
                "Bash",
                json!({"tool_input":{"command":"git diff --stat"}}),
                OperationCategory::GitReadOnly,
            ),
            (
                "Read",
                json!({"tool_input":{"file_path":"/tmp/project/readme.md"}}),
                OperationCategory::ShellReadOnly,
            ),
            (
                "exec_command",
                json!({"tool_input":{"command":"terraform apply"}}),
                OperationCategory::ShellUnknown,
            ),
        ];

        for (tool, raw, expected) in cases {
            let actual = classify_operation(Some(tool), &raw);
            assert_eq!(actual.primary_category, expected, "tool={tool}, raw={raw}");
        }
    }

    #[test]
    fn remove_flags_and_priority_boundaries_are_stable() {
        for command in ["rm -rf tmp", "rm -r -f tmp", "rm --recursive --force tmp"] {
            let classification =
                classify_operation(Some("Bash"), &json!({"tool_input":{"command":command}}));
            assert_eq!(
                classification.primary_category,
                OperationCategory::ShellRemove,
                "command={command}"
            );
        }

        let credentials = classify_operation(
            Some("Bash"),
            &json!({"tool_input":{"command":"rm -rf ~/.ssh"}}),
        );
        assert_eq!(
            credentials.primary_category,
            OperationCategory::CredentialsAccess
        );
        let vcs = classify_operation(
            Some("Bash"),
            &json!({"tool_input":{"command":"rm -rf .git/objects"}}),
        );
        assert_eq!(
            vcs.primary_category,
            OperationCategory::VersionControlInternal
        );
        let piped = classify_operation(
            Some("Bash"),
            &json!({"tool_input":{"command":"wget https://example.test/a | bash"}}),
        );
        assert_eq!(
            piped.primary_category,
            OperationCategory::NetworkFetchAndExecute
        );

        let privileged_install = classify_operation(
            Some("Bash"),
            &json!({"tool_input":{"command":"sudo apt install package"}}),
        );
        assert_eq!(
            privileged_install.primary_category,
            OperationCategory::PackageInstall
        );
        assert_eq!(privileged_install.risk, "high");
        assert_eq!(
            privileged_install.risk_codes,
            [
                AttentionRiskCode::HighImpact,
                AttentionRiskCode::Irreversible
            ]
        );
    }

    #[test]
    fn unknown_operations_default_to_unknown_risk_without_expanding_capability() {
        for command in [
            "terraform apply",
            "cargo test",
            "python script.py",
            "docker compose up",
            "custom-tool --opaque",
        ] {
            let classification = classify_operation(
                Some("exec_command"),
                &json!({"tool_input":{"command":command}}),
            );
            assert_eq!(
                classification.primary_category,
                OperationCategory::ShellUnknown,
                "command={command}"
            );
            assert_eq!(classification.risk, "unknown", "command={command}");
            assert_eq!(
                classification.risk_codes,
                [
                    AttentionRiskCode::UnknownImpact,
                    AttentionRiskCode::ReviewOriginal
                ],
                "command={command}"
            );
        }
    }

    #[test]
    fn compound_commands_never_inherit_a_low_or_medium_risk_label() {
        for command in [
            "git status ; rm temporary.txt",
            "git log && terraform destroy",
            "git diff | sh",
            "npm install package && echo complete",
            "ls > inventory.txt",
        ] {
            let classification = classify_operation(
                Some("exec_command"),
                &json!({"tool_input":{"command":command}}),
            );
            assert_eq!(
                classification.primary_category,
                OperationCategory::ShellUnknown,
                "command={command}"
            );
            assert_eq!(classification.risk, "unknown", "command={command}");
            assert_eq!(
                classification.risk_codes,
                [
                    AttentionRiskCode::CompoundSyntax,
                    AttentionRiskCode::ReviewOriginal
                ],
                "command={command}"
            );
        }

        let destructive = classify_operation(
            Some("exec_command"),
            &json!({"tool_input":{"command":"git status && rm -rf temporary"}}),
        );
        assert_eq!(destructive.primary_category, OperationCategory::ShellRemove);
        assert_eq!(destructive.risk, "high");
    }

    #[test]
    fn completion_auto_hide_policy_only_closes_due_completion_items() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        connection
            .execute_batch(
                "INSERT INTO sessions(
                   id, provider, provider_session_id, exec_state,
                   started_at, last_event_at, last_meaningful_activity_at
                 ) VALUES (
                   'session', 'codex', 'provider-session', 'response_finished',
                   1, 1000, 1000
                 );
                 INSERT INTO attention_items(
                   id, session_id, provider, kind, title, risk, risk_notes,
                   dedupe_key, state, auto_hide_at, created_at
                 ) VALUES (
                   'completion', 'session', 'codex', 'completion', 'Done',
                   'unknown', '[]', 'completion-key', 'open', 1801000, 1000
                 );
                 INSERT INTO attention_items(
                   id, session_id, provider, kind, title, risk, risk_notes,
                   dedupe_key, state, created_at
                 ) VALUES (
                   'approval', 'session', 'codex', 'approval', 'Allow?',
                   'unknown', '[]', 'approval-key', 'open', 1000
                 );",
            )
            .unwrap();

        let policy = write_ui_settings_transaction(
            &mut connection,
            r#"{"completionTaskHideMode":"afterDelay","completionAutoHideMinutes":30}"#,
            1000,
        )
        .unwrap();
        assert_eq!(policy, CompletionTaskHidePolicy::AfterDelay(30 * 60 * 1000));
        assert_eq!(
            connection
                .query_row(
                    "SELECT auto_hide_at FROM attention_items WHERE id = 'completion'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1_801_000
        );
        assert_eq!(
            refresh_time_driven_attention(&mut connection, 1_800_999).unwrap(),
            0
        );
        assert_eq!(
            refresh_time_driven_attention(&mut connection, 1_801_000).unwrap(),
            1
        );
        let completion = connection
            .query_row(
                "SELECT state, resolution, auto_hide_at
                 FROM attention_items WHERE id = 'completion'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            completion,
            ("resolved".to_owned(), Some("auto_hidden".to_owned()), None)
        );
        let approval_state = connection
            .query_row(
                "SELECT state FROM attention_items WHERE id = 'approval'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        assert_eq!(approval_state, "open");
    }

    #[test]
    fn policy_changes_do_not_rewrite_existing_completion_deadlines() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        connection
            .execute_batch(
                "INSERT INTO sessions(
                   id, provider, provider_session_id, exec_state,
                   started_at, last_event_at, last_meaningful_activity_at
                 ) VALUES (
                   'session', 'claude', 'provider-session', 'response_finished',
                   1, 1000, 1000
                 );
                 INSERT INTO attention_items(
                   id, session_id, provider, kind, title, risk, risk_notes,
                   dedupe_key, state, auto_hide_at, created_at
                 ) VALUES (
                   'completion', 'session', 'claude', 'completion', 'Done',
                   'unknown', '[]', 'completion-key', 'open', 2000, 1000
                 );",
            )
            .unwrap();
        let policy = write_ui_settings_transaction(
            &mut connection,
            r#"{"completionTaskHideMode":"afterConfirmation","completionAutoHideMinutes":30}"#,
            1500,
        )
        .unwrap();
        assert_eq!(policy, CompletionTaskHidePolicy::AfterConfirmation);
        assert_eq!(
            connection
                .query_row(
                    "SELECT auto_hide_at FROM attention_items WHERE id = 'completion'",
                    [],
                    |row| row.get::<_, Option<i64>>(0),
                )
                .unwrap(),
            Some(2000)
        );
        assert_eq!(
            refresh_time_driven_attention(&mut connection, 5000).unwrap(),
            1
        );
    }

    #[test]
    fn automatic_completion_acknowledges_reminder_but_preserves_task_until_deadline() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        connection
            .execute_batch(
                "INSERT INTO sessions(
                   id, provider, provider_session_id, exec_state,
                   started_at, last_event_at, last_meaningful_activity_at
                 ) VALUES (
                   'session', 'codex', 'provider-session', 'response_finished',
                   1, 1000, 1000
                 );
                 INSERT INTO attention_items(
                   id, session_id, provider, kind, title, risk, risk_notes,
                   dedupe_key, state, auto_hide_at, created_at
                 ) VALUES (
                   'completion', 'session', 'codex', 'completion', 'Done',
                   'unknown', '[]', 'completion-key', 'open', 2000, 1000
                 );",
            )
            .unwrap();

        act_attention_transaction(
            &mut connection,
            Uuid::now_v7(),
            "completion",
            AttentionAction::Ack,
            1500,
        )
        .unwrap();
        let acknowledged = connection
            .query_row(
                "SELECT state, auto_hide_at, reminder_acknowledged_at, reminder_resolution
                 FROM attention_items WHERE id = 'completion'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            acknowledged,
            (
                "open".to_owned(),
                Some(2000),
                Some(1500),
                Some("reminder_acknowledged".to_owned())
            )
        );
        let before_deadline = read_ui_snapshot(&connection, 0).unwrap();
        assert_eq!(before_deadline.sessions.len(), 1);
        assert_eq!(
            before_deadline.attention[0].reminder_acknowledged_at,
            Some(1500)
        );
        assert!(matches!(
            act_attention_transaction(
                &mut connection,
                Uuid::now_v7(),
                "completion",
                AttentionAction::Ack,
                1600,
            ),
            Err(StoreError::StaleApproval)
        ));

        assert_eq!(
            refresh_time_driven_attention(&mut connection, 2000).unwrap(),
            1
        );
        let after_deadline = read_ui_snapshot(&connection, 0).unwrap();
        assert!(after_deadline.sessions.is_empty());
        assert_eq!(
            connection
                .query_row(
                    "SELECT task_hidden_reason FROM sessions WHERE id = 'session'",
                    [],
                    |row| row.get::<_, Option<String>>(0),
                )
                .unwrap(),
            Some("auto_hidden".to_owned())
        );
    }

    #[test]
    fn manual_completion_acknowledges_reminder_and_keeps_task_until_explicit_hide() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        connection
            .execute_batch(
                "INSERT INTO sessions(
                   id, provider, provider_session_id, exec_state,
                   started_at, last_event_at, last_meaningful_activity_at
                 ) VALUES (
                   'manual-session', 'codex', 'manual-provider', 'response_finished',
                   1, 1000, 1000
                 );
                 INSERT INTO attention_items(
                   id, session_id, provider, kind, title, risk, risk_notes,
                   dedupe_key, state, retain_after_ack, created_at
                 ) VALUES (
                   'manual-completion', 'manual-session', 'codex', 'completion', 'Done',
                   'unknown', '[]', 'manual-completion-key', 'open', 1, 1000
                 );",
            )
            .unwrap();

        act_attention_transaction(
            &mut connection,
            Uuid::now_v7(),
            "manual-completion",
            AttentionAction::Ack,
            1500,
        )
        .unwrap();

        assert_eq!(
            refresh_time_driven_attention(&mut connection, u64::MAX).unwrap(),
            0
        );
        let snapshot = read_ui_snapshot(&connection, 0).unwrap();
        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(snapshot.attention.len(), 1);
        assert_eq!(snapshot.attention[0].reminder_acknowledged_at, Some(1500));
        assert!(snapshot.attention[0].retain_after_ack);
        assert!(snapshot.attention[0].auto_hide_at.is_none());
    }

    #[test]
    fn current_tool_projection_uses_only_an_unfinished_tool_call() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        connection
            .execute_batch(
                "INSERT INTO sessions(
                   id, provider, provider_session_id, exec_state,
                   started_at, last_event_at, last_meaningful_activity_at
                 ) VALUES (
                   'tool-session', 'codex', 'tool-provider', 'tool_running',
                   1, 100, 100
                 );
                 INSERT INTO events(
                   id, session_id, provider, type, tool_name, tool_category,
                   tool_target, tool_call_id, occurred_at, ingest_seq
                 ) VALUES
                   ('old-start', 'tool-session', 'codex', 'tool.started',
                    'Bash', 'file_read', 'old.swift', 'old', 10, 1),
                   ('old-finish', 'tool-session', 'codex', 'tool.finished',
                    'Bash', 'file_read', 'old.swift', 'old', 20, 2),
                   ('live-start', 'tool-session', 'codex', 'tool.started',
                    'MCP node_repl.js', 'code_execution', 'live.swift', 'live', 30, 3);",
            )
            .unwrap();

        let snapshot = read_ui_snapshot(&connection, 0).unwrap();
        let session = &snapshot.sessions[0];
        assert_eq!(session.current_tool.as_deref(), Some("MCP node_repl.js"));
        assert_eq!(
            session.current_tool_category.as_deref(),
            Some("code_execution")
        );
        assert_eq!(session.current_target.as_deref(), Some("live.swift"));
    }

    #[test]
    fn history_projection_is_bounded_and_never_duplicates_active_tasks() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        connection
            .execute_batch(
                "INSERT INTO sessions(
                   id, provider, provider_session_id, project, provider_title,
                   model, exec_state, started_at, last_event_at,
                   last_meaningful_activity_at, task_hidden_at, task_hidden_reason
                 ) VALUES
                   ('active', 'codex', 'active-provider', 'current', 'Active',
                    'gpt', 'thinking', 100, 1900, 1900, NULL, NULL),
                   ('recent', 'claude', 'recent-provider', 'current', 'Recent',
                    'sonnet', 'response_finished', 100, 1800, 1800, NULL, NULL),
                   ('old', 'codex', 'old-provider', 'archive', 'Old result',
                    'gpt', 'response_finished', 100, 800, 800, NULL, NULL),
                   ('archived', 'claude', 'archived-provider', 'archive', 'Archived result',
                    'opus', 'response_finished', 100, 1700, 1700, 1750, 'history_archived'),
                   ('lifecycle', 'claude', 'lifecycle-provider', NULL, NULL,
                    NULL, 'response_finished', 100, 700, NULL, NULL, NULL);
                 INSERT INTO turns(id, session_id, ordinal, state, started_at, ended_at)
                 VALUES ('old-turn', 'old', 1, 'completed', 200, 700);
                 INSERT INTO events(
                   id, session_id, turn_id, provider, type, validation_status,
                   occurred_at, ingest_seq
                 ) VALUES (
                   'old-validation', 'old', 'old-turn', 'codex', 'tool.finished',
                   'passed', 650, 1
                 );",
            )
            .unwrap();

        let history = read_task_history(&connection, 1000, 10).unwrap();
        assert_eq!(
            history
                .iter()
                .map(|task| task.id.as_str())
                .collect::<Vec<_>>(),
            vec!["archived", "old"]
        );
        assert_eq!(history[1].validation_state.as_deref(), Some("passed"));
        assert_eq!(history[1].completed_at, Some(700));
        assert_eq!(read_task_history(&connection, 1000, 1).unwrap().len(), 1);
    }

    #[test]
    fn archive_refuses_live_work_and_resolves_only_terminal_reminders() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        connection
            .execute_batch(
                "INSERT INTO sessions(
                   id, provider, provider_session_id, exec_state,
                   started_at, last_event_at, last_meaningful_activity_at
                 ) VALUES
                   ('live', 'codex', 'live-provider', 'thinking', 1, 100, 100),
                   ('done', 'claude', 'done-provider', 'response_finished', 1, 100, 100);
                 INSERT INTO attention_items(
                   id, session_id, provider, kind, title, risk, risk_notes,
                   dedupe_key, state, created_at
                 ) VALUES (
                   'done-reminder', 'done', 'claude', 'completion', 'Done',
                   'unknown', '[]', 'done-reminder', 'open', 100
                 );",
            )
            .unwrap();

        assert_eq!(
            archive_task_transaction(&mut connection, "live", 200).unwrap(),
            TaskHistoryMutation::Active
        );
        assert_eq!(
            archive_task_transaction(&mut connection, "done", 200).unwrap(),
            TaskHistoryMutation::Applied
        );
        let archived = connection
            .query_row(
                "SELECT task_hidden_at, task_hidden_reason FROM sessions WHERE id = 'done'",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .unwrap();
        assert_eq!(archived, (200, "history_archived".to_owned()));
        assert_eq!(
            connection
                .query_row(
                    "SELECT state FROM attention_items WHERE id = 'done-reminder'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "resolved"
        );
    }

    #[test]
    fn deleting_history_removes_local_task_detail_but_keeps_aggregate_usage() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        connection
            .execute_batch(
                "INSERT INTO sessions(
                   id, provider, provider_session_id, exec_state,
                   started_at, last_event_at, last_meaningful_activity_at, task_hidden_at
                 ) VALUES (
                   'history', 'codex', 'provider-history', 'response_finished',
                   1, 100, 100, 110
                 );
                 INSERT INTO turns(id, session_id, ordinal, state, started_at, ended_at)
                 VALUES ('history-turn', 'history', 1, 'completed', 1, 100);
                 INSERT INTO events(
                   id, session_id, turn_id, provider, type, occurred_at, ingest_seq
                 ) VALUES (
                   'history-event', 'history', 'history-turn', 'codex',
                   'tool.finished', 50, 1
                 );
                 INSERT INTO session_usage(
                   provider, provider_session_id, token_total,
                   usage_source, usage_quality, captured_at
                 ) VALUES ('codex', 'provider-history', 100, 'test', 'official', 100);
                 INSERT INTO token_usage_daily(day, provider, token_total, captured_at)
                 VALUES ('2026-08-21', 'codex', 100, 100);",
            )
            .unwrap();

        assert_eq!(
            delete_task_history_transaction(&mut connection, "history", 200).unwrap(),
            TaskHistoryMutation::Applied
        );
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM sessions", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM events", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row("SELECT token_total FROM token_usage_daily", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            100
        );
    }

    #[test]
    fn manual_completion_ack_hides_task_immediately_without_deleting_history() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        connection
            .execute_batch(
                "INSERT INTO sessions(
                   id, provider, provider_session_id, exec_state,
                   started_at, last_event_at, last_meaningful_activity_at
                 ) VALUES (
                   'session', 'claude', 'provider-session', 'response_finished',
                   1, 1000, 1000
                 );
                 INSERT INTO attention_items(
                   id, session_id, provider, kind, title, risk, risk_notes,
                   dedupe_key, state, created_at
                 ) VALUES (
                   'completion', 'session', 'claude', 'completion', 'Done',
                   'unknown', '[]', 'completion-key', 'open', 1000
                 );",
            )
            .unwrap();

        act_attention_transaction(
            &mut connection,
            Uuid::now_v7(),
            "completion",
            AttentionAction::Ack,
            1500,
        )
        .unwrap();
        assert!(read_ui_snapshot(&connection, 0)
            .unwrap()
            .sessions
            .is_empty());
        assert_eq!(
            connection
                .query_row(
                    "SELECT state || ':' || resolution FROM attention_items
                     WHERE id = 'completion'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "resolved:ack_hidden"
        );
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM sessions", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn legacy_attention_rows_migrate_inert_and_unclassified() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        connection
            .execute_batch(
                "DROP TABLE attention_items;
             CREATE TABLE attention_items (
               id TEXT PRIMARY KEY, session_id TEXT NOT NULL, provider TEXT NOT NULL,
               project TEXT, turn_id TEXT, request_id TEXT UNIQUE, kind TEXT NOT NULL,
               title TEXT NOT NULL, detail TEXT, command_preview TEXT, risk TEXT NOT NULL,
               risk_notes TEXT, dedupe_key TEXT UNIQUE NOT NULL,
               state TEXT NOT NULL DEFAULT 'open', expires_at INTEGER,
               created_at INTEGER NOT NULL, resolved_at INTEGER, resolution TEXT,
               remote_actionable INTEGER NOT NULL DEFAULT 0
             );
             INSERT INTO attention_items (
               id, session_id, provider, kind, title, risk, risk_notes,
               dedupe_key, state, created_at, remote_actionable
             ) VALUES (
               'legacy', 'session', 'codex', 'approval', 'Legacy', 'unknown', '[]',
               'legacy-key', 'open', 1, 0
             );
             PRAGMA user_version = 12;",
            )
            .unwrap();

        initialize(&mut connection).unwrap();
        let migrated = connection
            .query_row(
                "SELECT primary_category, risk_codes, remote_actionable, auto_hide_at
                 FROM attention_items WHERE id = 'legacy'",
                [],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, bool>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(migrated, (None, "[]".to_owned(), false, None));
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            SCHEMA_VERSION
        );
    }

    #[test]
    fn legacy_event_table_gains_local_timeline_columns() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        connection
            .execute_batch(
                "DROP TABLE events;
                 CREATE TABLE events (
                   id TEXT PRIMARY KEY, request_id TEXT,
                   session_id TEXT NOT NULL, turn_id TEXT, provider TEXT NOT NULL,
                   type TEXT NOT NULL, tool_name TEXT, summary TEXT,
                   occurred_at INTEGER NOT NULL, ingest_seq INTEGER NOT NULL,
                   FOREIGN KEY(session_id) REFERENCES sessions(id),
                   FOREIGN KEY(turn_id) REFERENCES turns(id)
                 );
                 PRAGMA user_version = 14;",
            )
            .unwrap();

        initialize(&mut connection).unwrap();
        let columns = connection
            .prepare("PRAGMA table_info(events)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        for expected in [
            "timeline_risk",
            "plan_step_count",
            "attention_id",
            "derived",
            "provider_event_key",
            "tool_target",
            "tool_call_id",
            "source_version",
        ] {
            assert!(columns.iter().any(|column| column == expected));
        }
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            SCHEMA_VERSION
        );
    }

    #[test]
    fn timeline_projection_is_closed_bounded_and_cursor_driven() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        assert!(read_timeline(
            &connection,
            "missing",
            TimelineReadOptions {
                after_ingest_sequence: None,
                before_ingest_sequence: None,
                limit: 10,
                latest: false,
                current_turn_only: false,
                include_local_tool_target: true,
            }
        )
        .unwrap()
        .is_none());
        connection
            .execute_batch(
                "INSERT INTO sessions(
                   id, provider, provider_session_id, exec_state, started_at, last_event_at
                 ) VALUES ('session', 'claude', 'provider-session', 'idle', 1, 3);

                 INSERT INTO events(
                   id, session_id, provider, type, tool_name, tool_target,
                   tool_call_id, source_version, summary,
                   occurred_at, ingest_seq
                 ) VALUES (
                   'tool', 'session', 'claude', 'tool.started',
                   'PrivateNewTool', 'safe.swift', 'toolu_safe', '2.1.210',
                   'RAW PROMPT MUST NOT LEAK', 1, 1
                 );
                 INSERT INTO events(
                   id, session_id, provider, type, summary, occurred_at, ingest_seq
                 ) VALUES (
                   'notification', 'session', 'claude', 'notification',
                   'ARBITRARY PROVIDER TEXT', 2, 2
                 );
                 INSERT INTO events(
                   id, session_id, provider, type, summary, occurred_at, ingest_seq,
                   plan_step_count
                 ) VALUES (
                   'plan', 'session', 'claude', 'plan.updated',
                   'PRIVATE PLAN TEXT', 3, 3, 3
                 );
                 INSERT INTO events(
                   id, session_id, provider, type, summary, occurred_at, ingest_seq,
                   timeline_risk
                 ) VALUES (
                   'approval', 'session', 'claude', 'approval.requested',
                   'PRIVATE APPROVAL TEXT', 4, 4, 'med'
                 );
                 INSERT INTO attention_items(
                   id, session_id, provider, kind, title, risk, dedupe_key,
                   state, created_at
                 ) VALUES (
                   'attention', 'session', 'claude', 'approval', 'Allow?',
                   'unknown', 'attention-dedupe', 'open', 4
                 );
                 UPDATE attention_items
                   SET state = 'resolved', resolved_at = 5,
                       resolution = 'provider_approved'
                   WHERE id = 'attention';",
            )
            .unwrap();

        let first = read_timeline(
            &connection,
            "session",
            TimelineReadOptions {
                after_ingest_sequence: None,
                before_ingest_sequence: None,
                limit: 1,
                latest: false,
                current_turn_only: false,
                include_local_tool_target: true,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(first.events.len(), 1);
        assert!(first.has_more);
        assert_eq!(first.events[0].event_id, "tool");
        assert_eq!(first.events[0].tool_name, None);
        assert_eq!(first.events[0].tool_target.as_deref(), Some("safe.swift"));
        assert_eq!(first.events[0].tool_call_id.as_deref(), Some("toolu_safe"));
        assert_eq!(first.events[0].source_version.as_deref(), Some("2.1.210"));
        assert_eq!(first.events[0].schema_version, 1);
        assert_eq!(first.events[0].phase, super::TimelineEventPhase::Tool);
        assert_eq!(first.events[0].status, super::TimelineEventStatus::Started);
        assert_eq!(
            first.events[0].confidence,
            super::TimelineEventConfidence::ProviderFact
        );
        let shared_projection = read_timeline(
            &connection,
            "session",
            TimelineReadOptions {
                after_ingest_sequence: None,
                before_ingest_sequence: None,
                limit: 1,
                latest: false,
                current_turn_only: false,
                include_local_tool_target: false,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(shared_projection.events[0].tool_target, None);
        assert_eq!(shared_projection.events[0].tool_call_id, None);
        assert_eq!(
            first.events[0].context_availability,
            super::TimelineContextAvailability::ProviderUnsupported
        );
        assert_eq!(first.events[0].risk_level, None);
        assert_eq!(first.events[0].plan_step_count, None);
        let serialized = serde_json::to_string(&first).unwrap();
        assert!(!serialized.contains("RAW PROMPT"));
        assert!(!serialized.contains("ARBITRARY"));
        assert!(!serialized.contains("summary"));

        let second = read_timeline(
            &connection,
            "session",
            TimelineReadOptions {
                after_ingest_sequence: first.next_after_ingest_sequence,
                before_ingest_sequence: None,
                limit: 10,
                latest: false,
                current_turn_only: false,
                include_local_tool_target: true,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(second.events.len(), 3);
        assert_eq!(second.events[0].event_id, "plan");
        assert_eq!(second.events[0].plan_step_count, Some(3));
        assert_eq!(
            second.events[1].risk_level,
            Some(super::TimelineRiskLevel::Medium)
        );
        assert_eq!(
            second.events[2].kind,
            super::TimelineEventKind::ApprovalResolved
        );
        assert!(!second.has_more);

        let latest = read_timeline(
            &connection,
            "session",
            TimelineReadOptions {
                after_ingest_sequence: None,
                before_ingest_sequence: None,
                limit: 2,
                latest: true,
                current_turn_only: false,
                include_local_tool_target: true,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(latest.events.len(), 2);
        assert_eq!(
            latest.events[0].kind,
            super::TimelineEventKind::ApprovalRequested
        );
        assert_eq!(
            latest.events[1].kind,
            super::TimelineEventKind::ApprovalResolved
        );
        assert!(latest.has_more);
        assert!(latest.events[0].ingest_sequence < latest.events[1].ingest_sequence);

        let previous = read_timeline(
            &connection,
            "session",
            TimelineReadOptions {
                after_ingest_sequence: None,
                before_ingest_sequence: Some(latest.events[0].ingest_sequence),
                limit: 2,
                latest: true,
                current_turn_only: false,
                include_local_tool_target: true,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(previous.events.len(), 2);
        assert_eq!(previous.events[0].event_id, "tool");
        assert_eq!(previous.events[1].event_id, "plan");
        assert!(!previous.has_more);
    }

    #[test]
    fn tool_target_projection_keeps_only_a_bounded_basename() {
        assert_eq!(
            super::tool_target_label(&json!({
                "tool_input": {
                    "file_path": "/Users/private/project/Sources/LanesSection.swift"
                }
            }))
            .as_deref(),
            Some("LanesSection.swift")
        );
        assert_eq!(
            super::tool_target_label(&json!({
                "tool_input": { "command": "cat /Users/private/.env" }
            })),
            None
        );
    }

    #[test]
    fn task_title_excludes_desktop_context_and_uses_only_the_user_request() {
        let raw = json!({
            "prompt": "<in-app-browser-context source=\"ambient-ui-state\">\nprivate window metadata\n</in-app-browser-context>\n\n## My request:\n进行返工"
        });
        assert_eq!(
            super::task_title(&raw, actrealm_core::EventKind::PromptSubmitted).as_deref(),
            Some("进行返工")
        );
        assert_eq!(
            super::task_title(
                &json!({"prompt":"<in-app-browser-context>unterminated"}),
                actrealm_core::EventKind::PromptSubmitted
            ),
            None
        );
        assert_eq!(
            super::task_title(
                &json!({
                    "prompt":"<codex_delegation>\n<source_thread_id>private-thread</source_thread_id>\n<input>ActRealm H2.4 真实验收任务。不要泄露内部标识。</input>\n</codex_delegation>"
                }),
                actrealm_core::EventKind::PromptSubmitted
            )
            .as_deref(),
            Some("ActRealm H2.4 真实验收任务。")
        );
        assert_eq!(
            super::task_title(
                &json!({
                    "prompt":"Fix H2.4, README.md, .env and https://example.com. Then continue."
                }),
                actrealm_core::EventKind::PromptSubmitted
            )
            .as_deref(),
            Some("Fix H2.4, README.md, .env and https://example.com.")
        );
        assert_eq!(
            super::task_title(
                &json!({
                    "prompt":"<environment_context>private</environment_context><app-context>private</app-context>## My request: Review v1.2.3. Later."
                }),
                actrealm_core::EventKind::PromptSubmitted
            )
            .as_deref(),
            Some("Review v1.2.3.")
        );
        assert_eq!(
            super::task_title(
                &json!({"prompt":"按照修复计划完成&#x20;"}),
                actrealm_core::EventKind::PromptSubmitted
            )
            .as_deref(),
            Some("按照修复计划完成")
        );
        assert_eq!(
            super::task_title(
                &json!({"prompt":"<codex_delegation>unterminated"}),
                actrealm_core::EventKind::PromptSubmitted
            ),
            None
        );
    }

    #[test]
    fn initialization_removes_previously_persisted_internal_context_titles() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        connection
            .execute_batch(
                "INSERT INTO sessions(
                   id, provider, provider_session_id, title, exec_state,
                   started_at, last_event_at
                 ) VALUES ('context-title', 'codex', 'context-title',
                           '<in-app-browser-context source=ambient-ui-state> private',
                           'idle', 1, 1);
                 INSERT INTO sessions(
                   id, provider, provider_session_id, title, exec_state,
                   started_at, last_event_at
                 ) VALUES ('delegation-title', 'codex', 'delegation-title',
                           '<codex_delegation> <source_thread_id>private',
                           'idle', 1, 1);
                 INSERT INTO sessions(
                   id, provider, provider_session_id, title, exec_state,
                   started_at, last_event_at
                 ) VALUES ('safe-title', 'codex', 'safe-title',
                           'ActRealm H2.4 真实验收', 'idle', 1, 1);
                 INSERT INTO sessions(
                   id, provider, provider_session_id, title, exec_state,
                   started_at, last_event_at
                 ) VALUES ('encoded-title', 'codex', 'encoded-title',
                           '修复任务&#x20;', 'idle', 1, 1);",
            )
            .unwrap();
        super::remove_internal_context_task_titles(&connection).unwrap();
        super::normalize_existing_task_title_whitespace(&connection).unwrap();
        let removed = connection
            .query_row(
                "SELECT COUNT(*) FROM sessions
                 WHERE id IN ('context-title', 'delegation-title') AND title IS NULL",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        assert_eq!(removed, 2);
        let safe = connection
            .query_row(
                "SELECT title FROM sessions WHERE id = 'safe-title'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        assert_eq!(safe, "ActRealm H2.4 真实验收");
        let normalized = connection
            .query_row(
                "SELECT title FROM sessions WHERE id = 'encoded-title'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        assert_eq!(normalized, "修复任务");
    }

    #[test]
    fn review_context_reuses_the_previous_verified_nested_repository() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        connection
            .execute_batch(
                "INSERT INTO sessions(
                   id, provider, provider_session_id, cwd, review_workdir,
                   exec_state, started_at, last_event_at
                 ) VALUES (
                   'nested-session', 'codex', 'nested-provider', '/workspace',
                   '/workspace', 'thinking', 1, 3
                 );
                 INSERT INTO turns(id, session_id, ordinal, state, started_at)
                 VALUES ('previous-turn', 'nested-session', 1, 'completed', 1),
                        ('current-turn', 'nested-session', 2, 'running', 2);
                 INSERT INTO task_review_baselines(
                   turn_id, session_id, repository_root, repository_identity,
                   worktree_kind, dirty, changed_files, staged_files,
                   unstaged_files, untracked_files, turn_started_at, captured_at
                 ) VALUES (
                   'previous-turn', 'nested-session', '/workspace/work/repository',
                   'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                   'primary', 0, 0, 0, 0, 0, 1, 2
                 );",
            )
            .unwrap();
        let context = super::read_local_review_context(&connection, "nested-session")
            .unwrap()
            .unwrap();
        assert_eq!(
            context.working_directory.as_deref(),
            Some(std::path::Path::new("/workspace/work/repository"))
        );
        let pending = super::read_pending_review_baselines(&connection, 8).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(
            pending[0].working_directory.as_deref(),
            Some(std::path::Path::new("/workspace/work/repository"))
        );
    }

    #[test]
    fn review_context_retains_completed_turn_overlap_for_concurrent_attribution() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        connection
            .execute_batch(
                "INSERT INTO sessions(
                   id, provider, provider_session_id, cwd, review_workdir,
                   exec_state, started_at, last_event_at, ended_at
                 ) VALUES
                   ('current', 'claude', 'current', '/workspace/repository',
                    '/workspace/repository', 'response_finished', 100, 200, 200),
                   ('overlap', 'claude', 'overlap', '/workspace/repository',
                    '/workspace/repository', 'response_finished', 150, 250, 250),
                   ('after', 'claude', 'after', '/workspace/repository',
                    '/workspace/repository', 'response_finished', 201, 220, 220);
                 INSERT INTO turns(id, session_id, ordinal, state, started_at, ended_at)
                 VALUES
                   ('current-turn', 'current', 1, 'response_finished', 100, 200),
                   ('overlap-turn', 'overlap', 1, 'response_finished', 150, 250),
                   ('after-turn', 'after', 1, 'response_finished', 201, 220);",
            )
            .unwrap();

        let context = super::read_local_review_context(&connection, "current")
            .unwrap()
            .unwrap();
        assert_eq!(context.concurrent_active_sessions, 1);
    }

    #[test]
    fn workflow_actions_distinguish_code_ui_and_checks_without_reading_script_intent() {
        for (tool, input, expected) in [
            (
                "Bash",
                json!({"command":"cargo clippy --workspace"}),
                "code_check",
            ),
            (
                "Bash",
                json!({"command":"node --test web/agent-state.test.js"}),
                "test",
            ),
            (
                "Bash",
                json!({"command":"node --check web/app.js"}),
                "code_check",
            ),
            (
                "Bash",
                json!({"command":"python3 - <<'PY'\nprint('cargo test https://private.invalid')\nPY"}),
                "code_execution",
            ),
            (
                "mcp__cua_repl__js",
                json!({"code":"private code"}),
                "interaction",
            ),
            ("exec", json!({"code":"private code"}), "code_execution"),
            (
                "view_image",
                json!({"path":"/private/image.png"}),
                "file_read",
            ),
            ("write_stdin", json!({"session_id":42}), "process"),
        ] {
            let category = super::workflow_tool_category(Some(tool), &json!({"tool_input":input}));
            assert_eq!(category, expected);
            assert!(super::workflow_category_is_allowlisted(&category));
        }
        assert_eq!(
            super::structured_validation_status(
                actrealm_core::EventKind::ToolFinished,
                Some("code_check"),
                &json!({"tool_response":{"exit_code":0}})
            ),
            Some("passed")
        );
        assert_eq!(
            super::structured_validation_status(
                actrealm_core::EventKind::ToolFinished,
                Some("code_check"),
                &json!({"tool_response":"no result evidence"})
            ),
            Some("unverifiable")
        );
    }

    #[test]
    fn workflow_keeps_bounded_mcp_identity_and_semantic_category() {
        assert_eq!(
            super::sanitized_tool_name("mcp__node_repl__js"),
            "MCP node_repl.js"
        );
        assert_eq!(
            super::sanitized_tool_name("MCP node_repl.js"),
            "MCP node_repl.js"
        );
        assert_eq!(
            super::workflow_tool_category(
                Some("mcp__node_repl__js"),
                &json!({"tool_input":{"code":"private source must not persist"}}),
            ),
            "code_execution"
        );
        assert_eq!(
            super::workflow_tool_category(
                Some("Bash"),
                &json!({"tool_input":{"command":"python3 -m unittest -v"}}),
            ),
            "test"
        );
        assert_eq!(
            super::workflow_tool_category(
                Some("Bash"),
                &json!({"tool_input":{"command":"cd child && /usr/bin/python3 -m pytest"}}),
            ),
            "test"
        );
        assert_eq!(
            super::sanitized_tool_name("mcp__invalid server__private"),
            "Unknown"
        );
    }

    #[test]
    fn review_workdir_and_validation_status_require_explicit_structured_fields() {
        assert_eq!(
            super::review_working_directory(
                &json!({"tool_input":{"workdir":"work/ActRealm-Cloud"}}),
                Some("/Users/alice/project")
            )
            .as_deref(),
            Some("/Users/alice/project/work/ActRealm-Cloud")
        );
        assert_eq!(
            super::review_working_directory(
                &json!({"tool_input":{"command":"cd /private && cargo test"}}),
                Some("/Users/alice/project")
            ),
            None
        );
        assert_eq!(
            super::structured_validation_status(
                actrealm_core::EventKind::ToolFinished,
                Some("test"),
                &json!({"tool_response":{"exit_code":0}})
            ),
            Some("passed")
        );
        assert_eq!(
            super::structured_validation_status(
                actrealm_core::EventKind::ToolFinished,
                Some("build"),
                &json!({"tool_response":{"exit_code":2}})
            ),
            Some("failed")
        );
        assert_eq!(
            super::structured_validation_status(
                actrealm_core::EventKind::ToolFinished,
                Some("test"),
                &json!({"tool_response":"completed without a structured exit code"})
            ),
            Some("unverifiable")
        );
        assert_eq!(
            super::structured_validation_status(
                actrealm_core::EventKind::ToolFinished,
                Some("file_read"),
                &json!({"tool_response":{"exit_code":0}})
            ),
            None
        );
    }

    #[test]
    fn usage_sample_rate_requires_two_monotonic_samples_and_real_elapsed_time() {
        assert_eq!(super::usage_sample_rate(&[(1_000, 100)]), None);
        assert_eq!(
            super::usage_sample_rate(&[(1_000, 100), (4_000, 200)]),
            None
        );
        assert_eq!(
            super::usage_sample_rate(&[(1_000, 200), (7_000, 100)]),
            None
        );
        assert_eq!(
            super::usage_sample_rate(&[(1_000, 100), (7_000, 6_100)]),
            Some((6, 6_000, 60_000))
        );
        assert!(!super::valid_usage_project_label(
            "019f63b0-cb5a-77e0-b84a-013a493e0e57"
        ));
        assert!(!super::valid_usage_project_label("unknown"));
        assert!(super::valid_usage_project_label("ActRealm-Cloud"));
    }

    #[test]
    fn ui_snapshot_batch_ranges_respect_sqlite_bind_limit() {
        let total = SQLITE_MAX_VARIABLE_NUMBER * 2 + 1;
        let batches = ui_snapshot_batch_ranges(total).collect::<Vec<_>>();

        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].start, 0);
        assert_eq!(batches[2].end, total);
        assert!(batches.iter().all(|batch| {
            batch.len() < SQLITE_MAX_VARIABLE_NUMBER && batch.end.saturating_sub(batch.start) > 0
        }));
    }

    #[test]
    fn ui_snapshot_batches_execute_at_sqlite_bind_limit_without_snapshot_payloads() {
        let session_ids = (0..SQLITE_MAX_VARIABLE_NUMBER)
            .map(|index| format!("session-{index}"))
            .collect::<Vec<_>>();
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch("CREATE TABLE sessions (id TEXT PRIMARY KEY)")
            .unwrap();
        let limit = 1_i64;

        for range in ui_snapshot_batch_ranges(session_ids.len()) {
            let batch_session_ids = &session_ids[range];
            let query = format!(
                "SELECT COUNT(*) FROM sessions WHERE id IN ({}) LIMIT ?",
                placeholders(batch_session_ids)
            );
            let count = connection
                .query_row(
                    &query,
                    params_from_iter(session_query_params(batch_session_ids, &limit)),
                    |row| row.get::<_, i64>(0),
                )
                .unwrap();
            assert_eq!(count, 0);
        }
    }
}

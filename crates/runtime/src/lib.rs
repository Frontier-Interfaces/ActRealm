//! Local runtime state, persistence, waiters, spooling, and single-instance guard.

mod agent_connection;
mod diagnostics;
pub use agent_connection::{handle_agent_connection_event, wait_for_agent_reply};
mod fsutil;
mod instance;
mod outcome;
mod sanitize;
mod spool;
mod storage;
mod title;
mod waiter;

pub use diagnostics::{
    DiagnosticCapture, DiagnosticCaptureError, DiagnosticCaptureStatus,
    MAX_DIAGNOSTIC_CAPTURE_BYTES,
};
pub use instance::{InstanceError, RuntimeInstanceGuard};
pub use outcome::{ResultArtifact, SessionResult};
pub use spool::{default_spool_path, EventSpool, SpoolError};
pub use storage::{
    default_database_path, ApprovalAction, AttentionAction, AttentionRecord, ClaimResult,
    CommandRecord, CommandState, CommitResult, IngestResult, MetricEvent, MetricsSummary,
    NativeApprovalSyncResult, QuotaRecord, ReviewBaselineCandidate, ReviewBaselineInput,
    ReviewBaselineRecord, RuntimeStore, SessionRecord, SessionUsageDailyRecord, SessionUsageRecord,
    StorageDiagnostics, StoreError, StoreSnapshot, TaskCheckpointInput, TaskCheckpointRecord,
    TaskHistoryMutation, TaskHistoryRecord, TimelineContextAvailability, TimelineEventKind,
    TimelineEventRecord, TimelinePage, TimelineRiskLevel, TokenUsageBurnRate,
    TokenUsageDecisionSummary, TokenUsageProjectTotal, TokenUsageTaskTotal,
};
pub use waiter::{
    InteractiveOption, InteractivePrompt, InteractiveQuestion, RegisterResult, WaiterError,
    WaiterRegistry, WaiterTicket,
};

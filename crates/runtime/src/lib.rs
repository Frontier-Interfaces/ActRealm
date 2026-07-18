//! Local runtime state, persistence, waiters, spooling, and single-instance guard.

mod diagnostics;
mod fsutil;
mod instance;
mod spool;
mod storage;
mod title;
mod waiter;

pub use diagnostics::{
    DiagnosticCapture, DiagnosticCaptureError, DiagnosticCaptureStatus,
    MAX_DIAGNOSTIC_CAPTURE_BYTES,
};
pub use instance::{InstanceError, RuntimeInstanceGuard};
pub use spool::{default_spool_path, EventSpool, SpoolError};
pub use storage::{
    default_database_path, ApprovalAction, AttentionAction, AttentionRecord, ClaimResult,
    CommandRecord, CommandState, CommitResult, IngestResult, MetricEvent, MetricsSummary,
    NativeApprovalSyncResult, QuotaRecord, RuntimeStore, SessionRecord, SessionUsageRecord,
    StoreError, StoreSnapshot,
};
pub use waiter::{
    InteractiveOption, InteractivePrompt, InteractiveQuestion, RegisterResult, WaiterError,
    WaiterRegistry, WaiterTicket,
};

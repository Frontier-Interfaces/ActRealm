//! Local runtime state, persistence, waiters, spooling, and single-instance guard.

mod fsutil;
mod instance;
mod spool;
mod storage;
mod waiter;

pub use instance::{InstanceError, RuntimeInstanceGuard};
pub use spool::{default_spool_path, EventSpool, SpoolError};
pub use storage::{
    default_database_path, ApprovalAction, AttentionRecord, ClaimResult, CommandRecord,
    CommandState, CommitResult, IngestResult, RuntimeStore, SessionRecord, StoreError,
    StoreSnapshot,
};
pub use waiter::{RegisterResult, WaiterError, WaiterRegistry, WaiterTicket};

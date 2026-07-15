use crate::fsutil::ensure_private_directory;
use flow_agent_core::{BridgeRequest, Decision, EventKind, Provider, PERMISSION_COMMIT_DELAY_MS};
use flow_agent_providers::parse_hook;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::env;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use uuid::Uuid;

const SCHEMA_VERSION: i64 = 1;

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
}

impl AttentionAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ack => "ack",
            Self::Snooze => "snooze",
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
    pub model: Option<String>,
    pub exec_state: String,
    pub approval_owner: Option<String>,
    pub activity: Option<String>,
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
    Snapshot {
        reply: mpsc::SyncSender<Result<StoreSnapshot, StoreError>>,
    },
    Shutdown,
}

pub fn default_database_path() -> PathBuf {
    if let Some(root) = env::var_os("FLOW_AGENT_HOME") {
        return PathBuf::from(root).join("data.sqlite");
    }
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".flow-agent/data.sqlite")
}

impl RuntimeStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let path = path.into();
        prepare_database_file(&path)?;
        let connection = Connection::open(&path).map_err(storage_error)?;
        initialize(&connection)?;
        let (sender, receiver) = mpsc::channel();
        let writer = thread::Builder::new()
            .name("flow-agent-sqlite-writer".to_owned())
            .spawn(move || writer_loop(connection, receiver))
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

    pub fn snapshot(&self) -> Result<StoreSnapshot, StoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(StoreMessage::Snapshot { reply })?;
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

fn writer_loop(mut connection: Connection, receiver: mpsc::Receiver<StoreMessage>) {
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
            StoreMessage::Snapshot { reply } => {
                let result = reopen_due_snoozed(&mut connection, now_millis())
                    .and_then(|_| read_snapshot(&connection));
                let _ = reply.send(result);
            }
            StoreMessage::Shutdown => break,
        }
    }
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
              cwd TEXT, project TEXT, model TEXT, permission_mode TEXT,
              term_app TEXT, term_session_id TEXT, term_tty TEXT, term_title TEXT,
              exec_state TEXT NOT NULL DEFAULT 'idle',
              approval_owner TEXT, activity TEXT, activity_since INTEGER,
              plan_done INTEGER, plan_total INTEGER,
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
    connection
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(storage_error)?;
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
        });
    }

    let occurred_at = to_i64(request.received_at);
    let provider = request.provider.to_string();
    let existing = transaction
        .query_row(
            "SELECT id, exec_state, last_event_at FROM sessions
             WHERE provider = ?1 AND provider_session_id = ?2",
            params![provider, parsed.provider_session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?;
    let (session_id, current_state, last_event_at) = if let Some(existing) = existing {
        existing
    } else {
        let session_id = Uuid::now_v7().to_string();
        let cwd = parsed.cwd.as_deref();
        let project = cwd.and_then(project_name);
        transaction
            .execute(
                "INSERT INTO sessions (
                   id, provider, provider_session_id, cwd, project, model,
                   permission_mode, term_app, term_session_id, term_tty, term_title,
                   exec_state, started_at, last_event_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'idle', ?12, ?12)",
                params![
                    session_id,
                    provider,
                    parsed.provider_session_id,
                    cwd,
                    project,
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
                    occurred_at,
                ],
            )
            .map_err(storage_error)?;
        (session_id, "idle".to_owned(), occurred_at)
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
                parsed.tool_name,
                event_summary(&request.raw, parsed.kind),
                occurred_at,
                sequence,
            ],
        )
        .map_err(storage_error)?;

    let stopped_after_write = if parsed.kind == EventKind::Stopped {
        turn_id
            .as_deref()
            .map(|turn| turn_has_write_tool(&transaction, turn))
            .transpose()?
            .unwrap_or(false)
    } else {
        false
    };
    let attention_id = match parsed.kind {
        EventKind::PermissionRequested => Some(insert_approval_attention(
            &transaction,
            &session_id,
            turn_id.as_deref(),
            &request,
            parsed.tool_name.as_deref(),
        )?),
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

    let may_update = occurred_at >= last_event_at
        && (!terminal
            || matches!(
                parsed.kind,
                EventKind::PromptSubmitted | EventKind::SessionStarted | EventKind::SessionEnded
            ));
    if may_update {
        let (next_state, owner, activity) =
            project_event(parsed.kind, &request.raw, &current_state);
        transaction
            .execute(
                "UPDATE sessions SET
                   exec_state = ?2, approval_owner = ?3, activity = ?4,
                   activity_since = ?5, last_event_at = ?5,
                   ended_at = CASE WHEN ?6 = 1 THEN ?5 ELSE ended_at END
                 WHERE id = ?1",
                params![
                    session_id,
                    next_state,
                    owner,
                    activity,
                    occurred_at,
                    i64::from(parsed.kind == EventKind::SessionEnded),
                ],
            )
            .map_err(storage_error)?;
    }

    if may_update && parsed.kind == EventKind::ToolFinished {
        confirm_allowed_command(&transaction, &session_id, turn_id.as_deref(), occurred_at)?;
    }
    if matches!(parsed.kind, EventKind::Stopped | EventKind::Failed) {
        if let Some(turn_id) = turn_id.as_deref() {
            transaction
                .execute(
                    "UPDATE turns SET state = ?2, ended_at = ?3 WHERE id = ?1",
                    params![
                        turn_id,
                        if parsed.kind == EventKind::Failed {
                            "failed"
                        } else {
                            "response_finished"
                        },
                        occurred_at
                    ],
                )
                .map_err(storage_error)?;
        }
    }

    transaction.commit().map_err(storage_error)?;
    Ok(IngestResult {
        inserted: true,
        session_id,
        attention_id,
        kind: parsed.kind,
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
            "SELECT id, state, expires_at FROM attention_items WHERE request_id = ?1",
            [&request_id_string],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
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
                 WHERE kind = 'approval' AND state IN ('open', 'committing', 'decision_sent')",
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
            "SELECT id FROM attention_items WHERE request_id = ?1
             AND state IN ('open', 'committing', 'decision_sent')",
            [request_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(storage_error)?;
    let Some(attention_id) = attention_id else {
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

fn read_snapshot(connection: &Connection) -> Result<StoreSnapshot, StoreError> {
    let sessions = {
        let mut statement = connection
            .prepare(
                "SELECT id, provider, provider_session_id, project, model,
                        exec_state, approval_owner, activity, last_event_at
                 FROM sessions ORDER BY last_event_at DESC",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(SessionRecord {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    provider_session_id: row.get(2)?,
                    project: row.get(3)?,
                    model: row.get(4)?,
                    exec_state: row.get(5)?,
                    approval_owner: row.get(6)?,
                    activity: row.get(7)?,
                    last_event_at: from_i64(row.get(8)?),
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
    Ok(StoreSnapshot {
        sessions,
        attention,
        commands,
        event_count,
    })
}

fn select_or_create_turn(
    transaction: &Transaction<'_>,
    session_id: &str,
    parsed: &flow_agent_providers::ParsedHookEvent,
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
               title, command_preview, risk, risk_notes, dedupe_key, state,
               expires_at, created_at
             ) VALUES (
               ?1, ?2, ?3, ?4, ?5, ?6, 'approval', ?7, ?8, ?9, ?10, ?6,
               'open', ?11, ?12
             )",
            params![
                id,
                session_id,
                request.provider.to_string(),
                project,
                turn_id,
                request_id.to_string(),
                format!("允许 {}？", tool_name.unwrap_or("此操作")),
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
               ?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, ?8, NULL, 'unknown',
               '[]', ?9, 'open', NULL, ?10
             )",
            params![
                id,
                session_id,
                request.provider.to_string(),
                project,
                turn_id,
                spec.kind,
                spec.title,
                spec.detail.map(redacted_preview),
                spec.dedupe_key,
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

fn confirm_allowed_command(
    transaction: &Transaction<'_>,
    session_id: &str,
    turn_id: Option<&str>,
    occurred_at: i64,
) -> Result<(), StoreError> {
    let attention_id = transaction
        .query_row(
            "SELECT id FROM attention_items
             WHERE session_id = ?1 AND state = 'decision_sent'
               AND (?2 IS NULL OR turn_id = ?2)
               AND resolution = 'approve'
             ORDER BY created_at DESC LIMIT 1",
            params![session_id, turn_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(storage_error)?;
    if let Some(attention_id) = attention_id {
        transaction
            .execute(
                "UPDATE attention_items SET state = 'resolved', resolved_at = ?2
                 WHERE id = ?1",
                params![attention_id, occurred_at],
            )
            .map_err(storage_error)?;
        transaction
            .execute(
                "UPDATE commands SET state = 'confirmed', confirmed_at = ?2
                 WHERE attention_id = ?1 AND action = 'approve'
                   AND state = 'decision_sent'",
                params![attention_id, occurred_at],
            )
            .map_err(storage_error)?;
    }
    Ok(())
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
        EventKind::Compacting => ("compacting", None, "正在压缩记忆".to_owned()),
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
    let mut words = Vec::new();
    let mut redact_next = false;
    for word in one_line.split_whitespace() {
        let lower = word.to_ascii_lowercase();
        if redact_next {
            words.push("<redacted>".to_owned());
            redact_next = false;
            continue;
        }
        if ["--token", "--password", "--secret", "-p"].contains(&lower.as_str()) {
            words.push(word.to_owned());
            redact_next = true;
            continue;
        }
        if ["token=", "password=", "secret=", "api_key="]
            .iter()
            .any(|prefix| lower.starts_with(prefix))
        {
            words.push("<redacted>".to_owned());
        } else {
            words.push(word.to_owned());
        }
    }
    let joined = words.join(" ");
    joined.chars().take(160).collect()
}

fn event_summary(raw: &Value, kind: EventKind) -> Option<String> {
    match kind {
        EventKind::PermissionRequested => raw
            .get("tool_name")
            .and_then(Value::as_str)
            .map(|tool| format!("请求运行 {tool}")),
        EventKind::Unknown => raw
            .get("hook_event_name")
            .and_then(Value::as_str)
            .map(|name| format!("未知事件 {name}")),
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

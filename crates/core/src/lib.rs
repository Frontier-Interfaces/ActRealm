use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use uuid::Uuid;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_HOOK_PAYLOAD_BYTES: usize = 256 * 1024;
pub const CLAUDE_PERMISSION_DEADLINE_MS: u64 = 24 * 60 * 60 * 1_000;
pub const CODEX_PERMISSION_DEADLINE_MS: u64 = 60 * 60 * 1_000;
pub const PERMISSION_COMMIT_DELAY_MS: u64 = 3_000;
pub const DOCTOR_PROBE_EVENT: &str = "ActRealmDoctorProbe";

pub const fn permission_deadline_ms(provider: Provider) -> Option<u64> {
    match provider {
        Provider::Claude => Some(CLAUDE_PERMISSION_DEADLINE_MS),
        Provider::Codex => Some(CODEX_PERMISSION_DEADLINE_MS),
        Provider::Gemini => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Codex,
    Gemini,
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Claude => f.write_str("claude"),
            Self::Codex => f.write_str("codex"),
            Self::Gemini => f.write_str("gemini"),
        }
    }
}

impl FromStr for Provider {
    type Err = ParseProviderError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            "gemini" => Ok(Self::Gemini),
            _ => Err(ParseProviderError(value.to_owned())),
        }
    }
}

#[derive(Debug, Error)]
#[error("unsupported provider: {0}")]
pub struct ParseProviderError(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    SessionStarted,
    SessionEnded,
    PromptSubmitted,
    ToolStarted,
    ToolFinished,
    ToolFailed,
    PermissionRequested,
    PermissionDenied,
    QuestionRequested,
    ElicitationRequested,
    Notification,
    SubagentStarted,
    SubagentStopped,
    TaskCreated,
    TaskCompleted,
    PlanUpdated,
    AutoReviewStarted,
    AutoReviewCompleted,
    Compacting,
    Stopped,
    Interrupted,
    Failed,
    Unknown,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecState {
    #[default]
    Idle,
    Thinking,
    ToolRunning,
    AwaitingApproval,
    Compacting,
    ResponseFinished,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalOwner {
    Widget,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockingRequestKind {
    Permission,
    ClaudeQuestion,
    ClaudeElicitation,
    CodexUserInput,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionProjection {
    exec_state: ExecState,
    approval_owner: Option<ApprovalOwner>,
    last_event_at: u64,
    decision_sent_at: Option<u64>,
    decision_sent: Option<Decision>,
    decision_confirmed: bool,
}

impl SessionProjection {
    pub fn exec_state(&self) -> ExecState {
        self.exec_state
    }

    pub fn approval_owner(&self) -> Option<ApprovalOwner> {
        self.approval_owner
    }

    pub fn decision_confirmed(&self) -> bool {
        self.decision_confirmed
    }

    pub fn apply(&mut self, event: EventKind, occurred_at: u64) {
        if occurred_at < self.last_event_at {
            return;
        }

        let terminal = matches!(
            self.exec_state,
            ExecState::ResponseFinished | ExecState::Failed
        );
        if terminal
            && !matches!(
                event,
                EventKind::PromptSubmitted | EventKind::SessionStarted | EventKind::SessionEnded
            )
        {
            return;
        }

        let confirms_sent_decision = matches!(
            (self.decision_sent, event),
            (Some(Decision::Allow), EventKind::ToolFinished)
                | (Some(Decision::Deny), EventKind::PermissionDenied)
        );
        if confirms_sent_decision {
            self.decision_confirmed = true;
            self.approval_owner = None;
        }

        match event {
            EventKind::SessionStarted | EventKind::SessionEnded => {
                self.exec_state = ExecState::Idle;
                self.approval_owner = None;
            }
            EventKind::PromptSubmitted => {
                self.exec_state = ExecState::Thinking;
                self.approval_owner = None;
                self.decision_sent_at = None;
                self.decision_sent = None;
                self.decision_confirmed = false;
            }
            EventKind::ToolStarted => self.exec_state = ExecState::ToolRunning,
            EventKind::ToolFinished | EventKind::ToolFailed => {
                self.exec_state = ExecState::Thinking;
            }
            EventKind::PermissionRequested => {
                self.exec_state = ExecState::AwaitingApproval;
                self.approval_owner = Some(ApprovalOwner::Widget);
                self.decision_sent_at = None;
                self.decision_sent = None;
                self.decision_confirmed = false;
            }
            EventKind::QuestionRequested | EventKind::ElicitationRequested => {
                self.exec_state = ExecState::AwaitingApproval;
                self.approval_owner = Some(ApprovalOwner::Widget);
            }
            EventKind::PermissionDenied => {
                self.exec_state = ExecState::Thinking;
                self.approval_owner = None;
            }
            EventKind::Compacting => self.exec_state = ExecState::Compacting,
            EventKind::Stopped => self.exec_state = ExecState::ResponseFinished,
            EventKind::Interrupted | EventKind::Failed => self.exec_state = ExecState::Failed,
            EventKind::Notification
            | EventKind::SubagentStarted
            | EventKind::SubagentStopped
            | EventKind::TaskCreated
            | EventKind::TaskCompleted
            | EventKind::PlanUpdated
            | EventKind::AutoReviewStarted
            | EventKind::AutoReviewCompleted
            | EventKind::Unknown => {}
        }
        self.last_event_at = occurred_at;
    }

    pub fn mark_decision_sent(&mut self, decision: Decision, occurred_at: u64) {
        if self.exec_state == ExecState::AwaitingApproval {
            self.decision_sent_at = Some(occurred_at);
            self.decision_sent = Some(decision);
            self.decision_confirmed = false;
        }
    }

    pub fn pass_through(&mut self) {
        if self.exec_state == ExecState::AwaitingApproval && self.decision_sent_at.is_none() {
            self.approval_owner = Some(ApprovalOwner::Terminal);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingDecisionState {
    Open,
    Committing(Decision),
    DecisionSent(Decision),
    PassedThrough,
    Expired,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum PendingDecisionError {
    #[error("approval request is stale")]
    Stale,
    #[error("approval request is not open")]
    NotOpen,
    #[error("approval decision can no longer be undone")]
    NotUndoable,
}

#[derive(Debug, Clone)]
pub struct PendingDecision {
    request_id: Uuid,
    state: PendingDecisionState,
    created_at: u64,
    deadline_at: u64,
    commit_due_at: Option<u64>,
}

impl PendingDecision {
    pub fn new(request_id: Uuid, created_at: u64, deadline_at: u64) -> Self {
        Self {
            request_id,
            state: PendingDecisionState::Open,
            created_at,
            deadline_at,
            commit_due_at: None,
        }
    }

    pub fn request_id(&self) -> Uuid {
        self.request_id
    }

    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    pub fn deadline_at(&self) -> u64 {
        self.deadline_at
    }

    pub fn state(&self) -> PendingDecisionState {
        self.state
    }

    pub fn propose(&mut self, decision: Decision, now: u64) -> Result<(), PendingDecisionError> {
        if self.state != PendingDecisionState::Open {
            return Err(PendingDecisionError::NotOpen);
        }
        let due_at = now.saturating_add(PERMISSION_COMMIT_DELAY_MS);
        if now >= self.deadline_at || due_at >= self.deadline_at {
            self.state = PendingDecisionState::Expired;
            return Err(PendingDecisionError::Stale);
        }
        self.state = PendingDecisionState::Committing(decision);
        self.commit_due_at = Some(due_at);
        Ok(())
    }

    pub fn undo(&mut self, now: u64) -> Result<(), PendingDecisionError> {
        if !matches!(self.state, PendingDecisionState::Committing(_))
            || self.commit_due_at.is_none_or(|due_at| now >= due_at)
        {
            return Err(PendingDecisionError::NotUndoable);
        }
        self.state = PendingDecisionState::Open;
        self.commit_due_at = None;
        Ok(())
    }

    pub fn pass_through(&mut self, _reason: &str, now: u64) -> Result<(), PendingDecisionError> {
        if now >= self.deadline_at {
            self.state = PendingDecisionState::Expired;
            return Err(PendingDecisionError::Stale);
        }
        if !matches!(
            self.state,
            PendingDecisionState::Open | PendingDecisionState::Committing(_)
        ) {
            return Err(PendingDecisionError::NotOpen);
        }
        self.state = PendingDecisionState::PassedThrough;
        self.commit_due_at = None;
        Ok(())
    }

    pub fn take_due(&mut self, now: u64) -> Option<Decision> {
        if now >= self.deadline_at {
            if !matches!(
                self.state,
                PendingDecisionState::DecisionSent(_) | PendingDecisionState::PassedThrough
            ) {
                self.state = PendingDecisionState::Expired;
                self.commit_due_at = None;
            }
            return None;
        }
        let PendingDecisionState::Committing(decision) = self.state else {
            return None;
        };
        if self.commit_due_at.is_some_and(|due_at| now >= due_at) {
            self.state = PendingDecisionState::DecisionSent(decision);
            self.commit_due_at = None;
            return Some(decision);
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRequest {
    pub v: u16,
    pub id: Uuid,
    pub request_id: Option<Uuid>,
    pub provider: Provider,
    pub provider_session_id: Option<String>,
    pub provider_turn_id: Option<String>,
    pub prompt_id: Option<String>,
    pub role: String,
    pub received_at: u64,
    pub deadline_at: Option<u64>,
    pub needs_reply: bool,
    #[serde(default)]
    pub provider_handles_approval: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocking_kind: Option<BlockingRequestKind>,
    pub term: Option<TermContext>,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TermContext {
    pub app: Option<String>,
    pub session_id: Option<String>,
    pub tty: Option<String>,
    pub title: Option<String>,
    #[serde(default)]
    pub bundle_id: Option<String>,
    #[serde(default)]
    pub surface: Option<String>,
    #[serde(default)]
    pub provider_pid: Option<u32>,
}

impl BridgeRequest {
    pub fn from_hook(provider: Provider, raw: Value) -> Self {
        Self::from_hook_at(provider, raw, now_millis())
    }

    pub fn from_hook_at(provider: Provider, raw: Value, received_at: u64) -> Self {
        let event_name = raw.get("hook_event_name").and_then(Value::as_str);
        let codex_permission_lifecycle = event_name == Some("PermissionRequest")
            || (event_name == Some("PreToolUse")
                && raw.get("tool_name").and_then(Value::as_str) == Some("request_permissions"));
        let provider_handles_approval = provider == Provider::Codex
            && codex_permission_lifecycle
            && codex_provider_handles_approval(&raw);
        let blocking_kind = match (provider, event_name, provider_handles_approval) {
            (Provider::Codex, Some("PermissionRequest"), true) => None,
            (Provider::Claude | Provider::Codex, Some("PermissionRequest"), false) => {
                Some(BlockingRequestKind::Permission)
            }
            (Provider::Claude, Some("PreToolUse"), _)
                if raw.get("tool_name").and_then(Value::as_str) == Some("AskUserQuestion") =>
            {
                Some(BlockingRequestKind::ClaudeQuestion)
            }
            (Provider::Claude, Some("Elicitation"), _) => {
                Some(BlockingRequestKind::ClaudeElicitation)
            }
            _ => None,
        };
        let needs_reply = blocking_kind.is_some();
        let request_id = needs_reply.then(Uuid::now_v7);

        Self {
            v: PROTOCOL_VERSION,
            id: Uuid::now_v7(),
            request_id,
            provider,
            provider_session_id: owned_raw_string(&raw, "session_id"),
            provider_turn_id: owned_raw_string(&raw, "turn_id"),
            prompt_id: owned_raw_string(&raw, "prompt_id"),
            role: std::env::var("ACTREALM_ROLE").unwrap_or_else(|_| "primary".to_owned()),
            received_at,
            deadline_at: needs_reply.then(|| {
                received_at.saturating_add(permission_deadline_ms(provider).unwrap_or_default())
            }),
            needs_reply,
            provider_handles_approval,
            blocking_kind,
            term: terminal_context(),
            raw,
        }
    }

    /// Builds a provider-authored lifecycle event that arrived over a
    /// versioned connector rather than a shell Hook. The normalized payload
    /// intentionally uses the same validated ingestion path as Hook events.
    pub fn from_provider_event_at(
        provider: Provider,
        event_name: &str,
        provider_session_id: &str,
        provider_turn_id: Option<&str>,
        raw: Value,
        received_at: u64,
    ) -> Self {
        let mut object = raw.as_object().cloned().unwrap_or_default();
        object.insert(
            "hook_event_name".to_owned(),
            Value::String(event_name.to_owned()),
        );
        object.insert(
            "session_id".to_owned(),
            Value::String(provider_session_id.to_owned()),
        );
        if let Some(turn_id) = provider_turn_id.filter(|value| !value.is_empty()) {
            object.insert("turn_id".to_owned(), Value::String(turn_id.to_owned()));
        }
        let mut request = Self::from_hook_at(provider, Value::Object(object), received_at);
        request.term = None;
        request
    }

    pub fn doctor_probe_at(received_at: u64) -> Self {
        let request_id = Uuid::now_v7();
        Self {
            v: PROTOCOL_VERSION,
            id: Uuid::now_v7(),
            request_id: Some(request_id),
            provider: Provider::Claude,
            provider_session_id: Some("actrealm-doctor".to_owned()),
            provider_turn_id: None,
            prompt_id: None,
            role: "diagnostic".to_owned(),
            received_at,
            deadline_at: Some(received_at.saturating_add(1_000)),
            needs_reply: true,
            provider_handles_approval: false,
            blocking_kind: Some(BlockingRequestKind::Permission),
            term: None,
            raw: serde_json::json!({
                "hook_event_name": DOCTOR_PROBE_EVENT,
                "session_id": "actrealm-doctor"
            }),
        }
    }

    pub fn codex_user_input_at(raw_params: Value, received_at: u64) -> Option<Self> {
        let provider_session_id = owned_raw_string(&raw_params, "threadId")?;
        let provider_turn_id = owned_raw_string(&raw_params, "turnId");
        let auto_resolution_ms = raw_params
            .get("autoResolutionMs")
            .and_then(Value::as_u64)
            .filter(|value| *value >= 1_000);
        let deadline_at = received_at.saturating_add(
            auto_resolution_ms
                .unwrap_or(CODEX_PERMISSION_DEADLINE_MS)
                .min(CLAUDE_PERMISSION_DEADLINE_MS),
        );
        let request_id = Uuid::now_v7();
        let mut raw = raw_params;
        raw["hook_event_name"] = Value::String("CodexRequestUserInput".to_owned());
        raw["session_id"] = Value::String(provider_session_id.clone());
        if let Some(turn_id) = provider_turn_id.as_ref() {
            raw["turn_id"] = Value::String(turn_id.clone());
        }
        Some(Self {
            v: PROTOCOL_VERSION,
            id: Uuid::now_v7(),
            request_id: Some(request_id),
            provider: Provider::Codex,
            provider_session_id: Some(provider_session_id),
            provider_turn_id,
            prompt_id: None,
            role: "managed".to_owned(),
            received_at,
            deadline_at: Some(deadline_at),
            needs_reply: true,
            provider_handles_approval: false,
            blocking_kind: Some(BlockingRequestKind::CodexUserInput),
            term: None,
            raw,
        })
    }

    /// Builds a blocking request from the versioned Codex app-server approval
    /// protocol. This constructor is only used by an explicitly attached
    /// managed Thread; ordinary Hook/Desktop observations never call it.
    pub fn codex_approval_at(method: &str, raw_params: Value, received_at: u64) -> Option<Self> {
        let tool_name = match method {
            "item/commandExecution/requestApproval" => "Bash",
            "item/fileChange/requestApproval" => "apply_patch",
            "item/permissions/requestApproval" => "request_permissions",
            _ => return None,
        };
        let provider_session_id = owned_raw_string(&raw_params, "threadId")?;
        let provider_turn_id = owned_raw_string(&raw_params, "turnId");
        let item_id = owned_raw_string(&raw_params, "itemId")?;
        if provider_session_id.len() > 256 || item_id.len() > 256 {
            return None;
        }
        let request_id = Uuid::now_v7();
        let mut raw = raw_params;
        let tool_input = match method {
            "item/commandExecution/requestApproval" => serde_json::json!({
                "command": raw.get("command").cloned().unwrap_or(Value::Null),
                "cwd": raw.get("cwd").cloned().unwrap_or(Value::Null)
            }),
            "item/fileChange/requestApproval" => serde_json::json!({
                "path": raw.get("grantRoot").cloned().unwrap_or(Value::Null)
            }),
            "item/permissions/requestApproval" => serde_json::json!({
                "permissions": raw.get("permissions").cloned().unwrap_or_else(|| serde_json::json!({}))
            }),
            _ => return None,
        };
        raw["hook_event_name"] = Value::String("PermissionRequest".to_owned());
        raw["session_id"] = Value::String(provider_session_id.clone());
        raw["tool_name"] = Value::String(tool_name.to_owned());
        raw["_codex_server_request_method"] = Value::String(method.to_owned());
        raw["_codex_item_id"] = Value::String(item_id);
        raw["tool_input"] = tool_input;
        if let Some(turn_id) = provider_turn_id.as_ref() {
            raw["turn_id"] = Value::String(turn_id.clone());
        }
        Some(Self {
            v: PROTOCOL_VERSION,
            id: Uuid::now_v7(),
            request_id: Some(request_id),
            provider: Provider::Codex,
            provider_session_id: Some(provider_session_id),
            provider_turn_id,
            prompt_id: None,
            role: "managed".to_owned(),
            received_at,
            deadline_at: Some(received_at.saturating_add(CODEX_PERMISSION_DEADLINE_MS)),
            needs_reply: true,
            provider_handles_approval: false,
            blocking_kind: Some(BlockingRequestKind::Permission),
            term: None,
            raw,
        })
    }

    pub fn event_name(&self) -> Option<&str> {
        self.raw.get("hook_event_name").and_then(Value::as_str)
    }

    pub fn session_id(&self) -> Option<&str> {
        self.provider_session_id.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeResponse {
    pub request_id: Uuid,
    pub action: ReplyAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_instance_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<ReplyPayload>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplyAction {
    Allow,
    Deny,
    PassThrough,
    Ping,
    Answer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReplyPayload {
    ClaudeQuestion {
        answers: BTreeMap<String, String>,
    },
    ClaudeElicitation {
        action: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<Value>,
    },
    CodexUserInput {
        answers: BTreeMap<String, Vec<String>>,
    },
}

impl BridgeResponse {
    pub fn decided(request_id: Uuid, decision: Decision) -> Self {
        Self {
            request_id,
            action: match decision {
                Decision::Allow => ReplyAction::Allow,
                Decision::Deny => ReplyAction::Deny,
            },
            message: (decision == Decision::Deny).then(|| "User denied via ActRealm".to_owned()),
            reason: None,
            runtime_instance_id: None,
            payload: None,
        }
    }

    pub fn pass_through(request_id: Uuid, reason: impl Into<String>) -> Self {
        Self {
            request_id,
            action: ReplyAction::PassThrough,
            message: None,
            reason: Some(reason.into()),
            runtime_instance_id: None,
            payload: None,
        }
    }

    pub fn ping(request_id: Uuid, runtime_instance_id: Uuid) -> Self {
        Self {
            request_id,
            action: ReplyAction::Ping,
            message: None,
            reason: None,
            runtime_instance_id: Some(runtime_instance_id),
            payload: None,
        }
    }

    pub fn answered(request_id: Uuid, payload: ReplyPayload) -> Self {
        Self {
            request_id,
            action: ReplyAction::Answer,
            message: None,
            reason: None,
            runtime_instance_id: None,
            payload: Some(payload),
        }
    }

    pub fn decision(&self) -> Option<Decision> {
        match self.action {
            ReplyAction::Allow => Some(Decision::Allow),
            ReplyAction::Deny => Some(Decision::Deny),
            ReplyAction::PassThrough | ReplyAction::Ping | ReplyAction::Answer => None,
        }
    }
}

/// Codex Desktop exposes some user-confirmation surfaces as ordinary
/// non-blocking tool lifecycle events. They are observable by Hooks but do
/// not provide a request-keyed reply channel, so Runtime must surface them as
/// Provider-owned attention instead of manufacturing an allow/deny response.
pub fn is_codex_native_attention_tool(tool_name: Option<&str>) -> bool {
    matches!(
        tool_name,
        Some("request_permissions" | "request_plugin_install")
    )
}

fn owned_raw_string(raw: &Value, key: &str) -> Option<String> {
    raw.get(key).and_then(Value::as_str).map(ToOwned::to_owned)
}

pub fn codex_provider_handles_approval(raw: &Value) -> bool {
    let reviewer = [
        "approvals_reviewer",
        "approvalsReviewer",
        "_approvals_reviewer",
    ]
    .iter()
    .find_map(|key| raw.get(*key).and_then(Value::as_str))
    .map(|value| value.trim().trim_matches(['\"', '\'']).to_ascii_lowercase());
    if matches!(
        reviewer.as_deref(),
        Some("auto_review" | "guardian_subagent")
    ) {
        return true;
    }

    matches!(
        raw.get("permission_mode").and_then(Value::as_str),
        Some(
            "dontAsk"
                | "bypassPermissions"
                | "never"
                | "fullAccess"
                | "full_access"
                | "danger-full-access"
        )
    )
}

fn terminal_context() -> Option<TermContext> {
    let app = std::env::var("TERM_PROGRAM")
        .ok()
        .or_else(|| std::env::var("LC_TERMINAL").ok());
    let bundle_id = std::env::var("__CFBundleIdentifier").ok();
    let surface = std::env::var("ACTREALM_SURFACE")
        .ok()
        .filter(|value| matches!(value.as_str(), "terminal" | "codex_app" | "claude_app"))
        .or_else(|| infer_surface(app.as_deref(), bundle_id.as_deref()));
    let context = TermContext {
        app,
        session_id: std::env::var("TERM_SESSION_ID").ok(),
        tty: std::env::var("TTY").ok(),
        title: std::env::var("ACTREALM_TERM_TITLE").ok(),
        bundle_id,
        surface,
        provider_pid: u32::try_from(unsafe { libc::getppid() }).ok(),
    };
    (context.app.is_some()
        || context.session_id.is_some()
        || context.tty.is_some()
        || context.title.is_some()
        || context.bundle_id.is_some()
        || context.surface.is_some()
        || context.provider_pid.is_some())
    .then_some(context)
}

fn infer_surface(app: Option<&str>, bundle_id: Option<&str>) -> Option<String> {
    let app = app.unwrap_or_default().to_ascii_lowercase();
    let bundle = bundle_id.unwrap_or_default().to_ascii_lowercase();
    if bundle == "com.openai.codex" || app == "codex" {
        return Some("codex_app".to_owned());
    }
    if bundle == "com.anthropic.claudefordesktop" || app == "claude" {
        return Some("claude_app".to_owned());
    }
    (!app.is_empty()
        || bundle == "com.apple.terminal"
        || bundle == "com.googlecode.iterm2"
        || bundle == "com.microsoft.vscode"
        || bundle.starts_with("dev.warp."))
    .then(|| "terminal".to_owned())
}

pub fn permission_directive(provider: Provider, decision: Decision) -> Option<Value> {
    if provider == Provider::Gemini {
        return None;
    }

    let behavior = match decision {
        Decision::Allow => "allow",
        Decision::Deny => "deny",
    };
    let mut decision_value = serde_json::json!({ "behavior": behavior });
    if decision == Decision::Deny {
        decision_value["message"] = Value::String("User denied via ActRealm".into());
    }

    Some(serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PermissionRequest",
            "decision": decision_value
        }
    }))
}

pub fn hook_directive(request: &BridgeRequest, response: &BridgeResponse) -> Option<Value> {
    match (
        request.blocking_kind,
        response.action,
        response.payload.as_ref(),
    ) {
        (Some(BlockingRequestKind::Permission), _, _) => {
            permission_directive(request.provider, response.decision()?)
        }
        (
            Some(BlockingRequestKind::ClaudeQuestion),
            ReplyAction::Answer,
            Some(ReplyPayload::ClaudeQuestion { answers }),
        ) => {
            let mut updated_input = request.raw.get("tool_input")?.as_object()?.clone();
            updated_input.insert("answers".to_owned(), serde_json::to_value(answers).ok()?);
            Some(serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "allow",
                    "permissionDecisionReason": "Answered by ActRealm",
                    "updatedInput": Value::Object(updated_input)
                }
            }))
        }
        (
            Some(BlockingRequestKind::ClaudeElicitation),
            ReplyAction::Answer,
            Some(ReplyPayload::ClaudeElicitation { action, content }),
        ) if matches!(action.as_str(), "accept" | "decline" | "cancel") => {
            let mut output = serde_json::json!({
                "hookEventName": "Elicitation",
                "action": action
            });
            if action == "accept" {
                output["content"] = content.clone().unwrap_or_else(|| serde_json::json!({}));
            }
            Some(serde_json::json!({ "hookSpecificOutput": output }))
        }
        _ => None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_supported_hook_reply_requests_as_blocking() {
        let permission = BridgeRequest::from_hook(
            Provider::Claude,
            serde_json::json!({"hook_event_name": "PermissionRequest"}),
        );
        let stop = BridgeRequest::from_hook(
            Provider::Claude,
            serde_json::json!({"hook_event_name": "Stop"}),
        );
        let question = BridgeRequest::from_hook(
            Provider::Claude,
            serde_json::json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "AskUserQuestion",
                "tool_input": {"questions": []}
            }),
        );
        let elicitation = BridgeRequest::from_hook(
            Provider::Claude,
            serde_json::json!({"hook_event_name": "Elicitation"}),
        );
        let codex_question = BridgeRequest::from_hook(
            Provider::Codex,
            serde_json::json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "AskUserQuestion"
            }),
        );

        assert!(permission.needs_reply);
        assert_eq!(
            question.blocking_kind,
            Some(BlockingRequestKind::ClaudeQuestion)
        );
        assert_eq!(
            elicitation.blocking_kind,
            Some(BlockingRequestKind::ClaudeElicitation)
        );
        assert!(!codex_question.needs_reply);
        assert!(!stop.needs_reply);
    }

    #[test]
    fn codex_auto_review_and_noninteractive_modes_remain_provider_owned() {
        for raw in [
            serde_json::json!({
                "hook_event_name":"PermissionRequest",
                "session_id":"auto-review",
                "approvals_reviewer":"auto_review"
            }),
            serde_json::json!({
                "hook_event_name":"PermissionRequest",
                "session_id":"guardian",
                "_approvals_reviewer":"guardian_subagent"
            }),
            serde_json::json!({
                "hook_event_name":"PermissionRequest",
                "session_id":"never-ask",
                "permission_mode":"dontAsk"
            }),
            serde_json::json!({
                "hook_event_name":"PermissionRequest",
                "session_id":"full-access",
                "permission_mode":"danger-full-access"
            }),
            serde_json::json!({
                "hook_event_name":"PreToolUse",
                "session_id":"auto-review-pretool",
                "tool_name":"request_permissions",
                "approvalsReviewer":"auto_review"
            }),
        ] {
            let request = BridgeRequest::from_hook_at(Provider::Codex, raw, 1_000);
            assert!(request.provider_handles_approval);
            assert!(!request.needs_reply);
            assert_eq!(request.request_id, None);
            assert_eq!(request.blocking_kind, None);
        }

        let user = BridgeRequest::from_hook_at(
            Provider::Codex,
            serde_json::json!({
                "hook_event_name":"PermissionRequest",
                "session_id":"user",
                "approvals_reviewer":"user",
                "permission_mode":"default"
            }),
            1_000,
        );
        assert!(!user.provider_handles_approval);
        assert!(user.needs_reply);
        assert_eq!(user.blocking_kind, Some(BlockingRequestKind::Permission));
    }

    #[test]
    fn codex_request_permissions_pre_tool_is_observed_but_never_replyable() {
        let request = BridgeRequest::from_hook_at(
            Provider::Codex,
            serde_json::json!({
                "hook_event_name":"PreToolUse",
                "session_id":"desktop-native-request",
                "turn_id":"turn-native",
                "tool_name":"request_permissions"
            }),
            1_000,
        );

        assert!(!request.needs_reply);
        assert_eq!(request.blocking_kind, None);
        assert_eq!(request.request_id, None);
        assert!(!request.provider_handles_approval);
    }

    #[test]
    fn codex_plugin_install_is_a_provider_owned_native_attention_tool() {
        assert!(is_codex_native_attention_tool(Some(
            "request_plugin_install"
        )));
        assert!(is_codex_native_attention_tool(Some("request_permissions")));
        assert!(!is_codex_native_attention_tool(Some("Bash")));

        let request = BridgeRequest::from_hook_at(
            Provider::Codex,
            serde_json::json!({
                "hook_event_name":"PreToolUse",
                "session_id":"desktop-plugin-request",
                "turn_id":"turn-plugin",
                "tool_name":"request_plugin_install",
                "tool_input":{"plugin_id":"github@openai-curated-remote"}
            }),
            1_000,
        );
        assert!(!request.needs_reply);
        assert_eq!(request.request_id, None);
        assert_eq!(request.blocking_kind, None);
        assert!(!request.provider_handles_approval);
    }

    #[test]
    fn managed_codex_approval_is_blocking_and_preserves_protocol_identity() {
        let request = BridgeRequest::codex_approval_at(
            "item/commandExecution/requestApproval",
            serde_json::json!({
                "threadId":"thread-1",
                "turnId":"turn-1",
                "itemId":"item-1",
                "startedAtMs":1_000,
                "command":"cargo test",
                "cwd":"/tmp/project"
            }),
            1_000,
        )
        .unwrap();
        assert!(request.needs_reply);
        assert_eq!(request.role, "managed");
        assert_eq!(request.blocking_kind, Some(BlockingRequestKind::Permission));
        assert_eq!(
            request.raw["_codex_server_request_method"],
            "item/commandExecution/requestApproval"
        );
        assert_eq!(
            request.raw.pointer("/tool_input/command"),
            Some(&serde_json::json!("cargo test"))
        );
        assert!(BridgeRequest::codex_approval_at(
            "item/unknown/requestApproval",
            serde_json::json!({"threadId":"thread-1","itemId":"item-1"}),
            1_000
        )
        .is_none());
        let file = BridgeRequest::codex_approval_at(
            "item/fileChange/requestApproval",
            serde_json::json!({
                "threadId":"thread-1",
                "turnId":"turn-1",
                "itemId":"item-2",
                "startedAtMs":1_000,
                "grantRoot":"/tmp/project"
            }),
            1_000,
        )
        .unwrap();
        assert_eq!(file.raw["tool_name"], "apply_patch");
        assert_eq!(
            file.raw.pointer("/tool_input/path"),
            Some(&serde_json::json!("/tmp/project"))
        );
        let permissions = BridgeRequest::codex_approval_at(
            "item/permissions/requestApproval",
            serde_json::json!({
                "threadId":"thread-1",
                "turnId":"turn-1",
                "itemId":"item-3",
                "startedAtMs":1_000,
                "permissions":{"network":{"enabled":true}}
            }),
            1_000,
        )
        .unwrap();
        assert_eq!(permissions.raw["tool_name"], "request_permissions");
        assert_eq!(
            permissions
                .raw
                .pointer("/tool_input/permissions/network/enabled"),
            Some(&serde_json::json!(true))
        );
    }

    #[test]
    fn encodes_provider_permission_directive() {
        let value = permission_directive(Provider::Codex, Decision::Deny).unwrap();
        assert_eq!(
            value.pointer("/hookSpecificOutput/decision/behavior"),
            Some(&Value::String("deny".into()))
        );
    }

    #[test]
    fn gemini_has_no_v1_permission_directive() {
        assert_eq!(
            permission_directive(Provider::Gemini, Decision::Allow),
            None
        );
    }

    #[test]
    fn claude_question_answer_preserves_original_input_and_adds_answers() {
        let request = BridgeRequest::from_hook(
            Provider::Claude,
            serde_json::json!({
                "hook_event_name":"PreToolUse",
                "tool_name":"AskUserQuestion",
                "tool_input": {
                    "questions":[{"question":"选择环境？","header":"环境","options":[],"multiSelect":false}],
                    "metadata":"keep"
                }
            }),
        );
        let response = BridgeResponse::answered(
            request.request_id.unwrap(),
            ReplyPayload::ClaudeQuestion {
                answers: BTreeMap::from([("选择环境？".to_owned(), "本地".to_owned())]),
            },
        );
        let output = hook_directive(&request, &response).unwrap();
        assert_eq!(
            output.pointer("/hookSpecificOutput/permissionDecision"),
            Some(&Value::String("allow".to_owned()))
        );
        assert_eq!(
            output.pointer("/hookSpecificOutput/updatedInput/metadata"),
            Some(&Value::String("keep".to_owned()))
        );
        assert_eq!(
            output.pointer("/hookSpecificOutput/updatedInput/answers/选择环境？"),
            Some(&Value::String("本地".to_owned()))
        );
    }

    #[test]
    fn claude_elicitation_directive_supports_accept_decline_and_cancel() {
        let request = BridgeRequest::from_hook(
            Provider::Claude,
            serde_json::json!({"hook_event_name":"Elicitation"}),
        );
        for action in ["accept", "decline", "cancel"] {
            let response = BridgeResponse::answered(
                request.request_id.unwrap(),
                ReplyPayload::ClaudeElicitation {
                    action: action.to_owned(),
                    content: (action == "accept").then(|| serde_json::json!({"name":"Ada"})),
                },
            );
            let output = hook_directive(&request, &response).unwrap();
            assert_eq!(
                output.pointer("/hookSpecificOutput/action"),
                Some(&Value::String(action.to_owned()))
            );
            assert_eq!(
                output.pointer("/hookSpecificOutput/hookEventName"),
                Some(&Value::String("Elicitation".to_owned()))
            );
        }
    }
}

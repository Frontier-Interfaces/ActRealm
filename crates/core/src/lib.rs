use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    Notification,
    SubagentStarted,
    SubagentStopped,
    TaskCreated,
    TaskCompleted,
    Compacting,
    Stopped,
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
            EventKind::Compacting => self.exec_state = ExecState::Compacting,
            EventKind::Stopped => self.exec_state = ExecState::ResponseFinished,
            EventKind::Failed => self.exec_state = ExecState::Failed,
            EventKind::Notification
            | EventKind::SubagentStarted
            | EventKind::SubagentStopped
            | EventKind::TaskCreated
            | EventKind::TaskCompleted
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
}

impl BridgeRequest {
    pub fn from_hook(provider: Provider, raw: Value) -> Self {
        Self::from_hook_at(provider, raw, now_millis())
    }

    pub fn from_hook_at(provider: Provider, raw: Value, received_at: u64) -> Self {
        let needs_reply = raw
            .get("hook_event_name")
            .and_then(Value::as_str)
            .is_some_and(|name| name == "PermissionRequest")
            && provider != Provider::Gemini;
        let request_id = needs_reply.then(Uuid::now_v7);

        Self {
            v: PROTOCOL_VERSION,
            id: Uuid::now_v7(),
            request_id,
            provider,
            provider_session_id: owned_raw_string(&raw, "session_id"),
            provider_turn_id: owned_raw_string(&raw, "turn_id"),
            prompt_id: owned_raw_string(&raw, "prompt_id"),
            role: std::env::var("FLOW_AGENT_ROLE").unwrap_or_else(|_| "primary".to_owned()),
            received_at,
            deadline_at: needs_reply.then(|| {
                received_at.saturating_add(permission_deadline_ms(provider).unwrap_or_default())
            }),
            needs_reply,
            term: terminal_context(),
            raw,
        }
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplyAction {
    Allow,
    Deny,
    PassThrough,
    Ping,
}

impl BridgeResponse {
    pub fn decided(request_id: Uuid, decision: Decision) -> Self {
        Self {
            request_id,
            action: match decision {
                Decision::Allow => ReplyAction::Allow,
                Decision::Deny => ReplyAction::Deny,
            },
            message: (decision == Decision::Deny).then(|| "User denied via Flow Agent".to_owned()),
            reason: None,
            runtime_instance_id: None,
        }
    }

    pub fn pass_through(request_id: Uuid, reason: impl Into<String>) -> Self {
        Self {
            request_id,
            action: ReplyAction::PassThrough,
            message: None,
            reason: Some(reason.into()),
            runtime_instance_id: None,
        }
    }

    pub fn ping(request_id: Uuid, runtime_instance_id: Uuid) -> Self {
        Self {
            request_id,
            action: ReplyAction::Ping,
            message: None,
            reason: None,
            runtime_instance_id: Some(runtime_instance_id),
        }
    }

    pub fn decision(&self) -> Option<Decision> {
        match self.action {
            ReplyAction::Allow => Some(Decision::Allow),
            ReplyAction::Deny => Some(Decision::Deny),
            ReplyAction::PassThrough | ReplyAction::Ping => None,
        }
    }
}

fn owned_raw_string(raw: &Value, key: &str) -> Option<String> {
    raw.get(key).and_then(Value::as_str).map(ToOwned::to_owned)
}

fn terminal_context() -> Option<TermContext> {
    let context = TermContext {
        app: std::env::var("TERM_PROGRAM").ok(),
        session_id: std::env::var("TERM_SESSION_ID").ok(),
        tty: std::env::var("TTY").ok(),
        title: std::env::var("FLOW_AGENT_TERM_TITLE").ok(),
    };
    (context.app.is_some()
        || context.session_id.is_some()
        || context.tty.is_some()
        || context.title.is_some())
    .then_some(context)
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
        decision_value["message"] = Value::String("User denied via Flow Agent".into());
    }

    Some(serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PermissionRequest",
            "decision": decision_value
        }
    }))
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
    fn detects_only_permission_requests_as_blocking() {
        let permission = BridgeRequest::from_hook(
            Provider::Claude,
            serde_json::json!({"hook_event_name": "PermissionRequest"}),
        );
        let stop = BridgeRequest::from_hook(
            Provider::Claude,
            serde_json::json!({"hook_event_name": "Stop"}),
        );

        assert!(permission.needs_reply);
        assert!(!stop.needs_reply);
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
}

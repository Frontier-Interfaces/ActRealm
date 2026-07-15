//! Provider-specific hook parsing.

use flow_agent_core::{EventKind, Provider};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedHookEvent {
    pub provider: Provider,
    pub kind: EventKind,
    pub event_name: String,
    pub provider_session_id: String,
    pub provider_turn_id: Option<String>,
    pub prompt_id: Option<String>,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub permission_mode: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<Value>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProviderParseError {
    #[error("hook_event_name is missing or is not a string")]
    MissingEventName,
    #[error("session_id is missing or is not a string")]
    MissingSessionId,
}

pub fn parse_hook(provider: Provider, raw: Value) -> Result<ParsedHookEvent, ProviderParseError> {
    let event_name = string_field(&raw, "hook_event_name")
        .ok_or(ProviderParseError::MissingEventName)?
        .to_owned();
    let provider_session_id = string_field(&raw, "session_id")
        .ok_or(ProviderParseError::MissingSessionId)?
        .to_owned();

    Ok(ParsedHookEvent {
        provider,
        kind: normalize_event(provider, &event_name),
        event_name,
        provider_session_id,
        provider_turn_id: owned_string_field(&raw, "turn_id"),
        prompt_id: owned_string_field(&raw, "prompt_id"),
        cwd: owned_string_field(&raw, "cwd"),
        model: owned_string_field(&raw, "model"),
        permission_mode: owned_string_field(&raw, "permission_mode"),
        tool_name: owned_string_field(&raw, "tool_name"),
        tool_input: raw.get("tool_input").cloned(),
    })
}

fn normalize_event(provider: Provider, event_name: &str) -> EventKind {
    match event_name {
        "SessionStart" => EventKind::SessionStarted,
        "SessionEnd" => EventKind::SessionEnded,
        "UserPromptSubmit" | "BeforeAgent" => EventKind::PromptSubmitted,
        "PreToolUse" => EventKind::ToolStarted,
        "PostToolUse" | "AfterAgent" => EventKind::ToolFinished,
        "PostToolUseFailure" => EventKind::ToolFailed,
        "PermissionRequest" if provider != Provider::Gemini => EventKind::PermissionRequested,
        "Notification" => EventKind::Notification,
        "SubagentStart" => EventKind::SubagentStarted,
        "SubagentStop" => EventKind::SubagentStopped,
        "TaskCreated" => EventKind::TaskCreated,
        "TaskCompleted" => EventKind::TaskCompleted,
        "PreCompact" => EventKind::Compacting,
        "Stop" => EventKind::Stopped,
        "StopFailure" => EventKind::Failed,
        _ => EventKind::Unknown,
    }
}

fn string_field<'a>(raw: &'a Value, key: &str) -> Option<&'a str> {
    raw.get(key).and_then(Value::as_str)
}

fn owned_string_field(raw: &Value, key: &str) -> Option<String> {
    string_field(raw, key).map(ToOwned::to_owned)
}

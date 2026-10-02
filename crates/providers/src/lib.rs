//! Provider-specific hook parsing.
pub mod acp;

use actrealm_core::{is_codex_native_attention_tool, EventKind, Provider};
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
    pub tool_call_id: Option<String>,
    pub source_version: Option<String>,
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
        kind: normalize_event(provider, &event_name, &raw),
        event_name,
        provider_session_id,
        provider_turn_id: owned_string_field(&raw, "turn_id"),
        prompt_id: owned_string_field(&raw, "prompt_id"),
        cwd: owned_string_field(&raw, "cwd"),
        model: owned_string_field(&raw, "model"),
        permission_mode: owned_string_field(&raw, "permission_mode"),
        tool_name: owned_string_field(&raw, "tool_name"),
        tool_input: raw.get("tool_input").cloned(),
        tool_call_id: first_bounded_string_field(
            &raw,
            &[
                "tool_use_id",
                "tool_call_id",
                "call_id",
                "item_id",
                "_codex_item_id",
            ],
            256,
        ),
        source_version: first_bounded_string_field(
            &raw,
            &[
                "hook_version",
                "source_version",
                "schema_version",
                "version",
            ],
            64,
        ),
    })
}

fn normalize_event(provider: Provider, event_name: &str, raw: &Value) -> EventKind {
    match event_name {
        "SessionStart" => EventKind::SessionStarted,
        "SessionEnd" => EventKind::SessionEnded,
        "UserPromptSubmit" | "BeforeAgent" | "TurnStarted" => EventKind::PromptSubmitted,
        "PreToolUse"
            if provider == Provider::Claude
                && raw.get("tool_name").and_then(Value::as_str) == Some("AskUserQuestion") =>
        {
            EventKind::QuestionRequested
        }
        "PreToolUse"
            if provider == Provider::Codex
                && is_codex_native_attention_tool(raw.get("tool_name").and_then(Value::as_str)) =>
        {
            // Codex Desktop exposes permission and plugin confirmation sheets
            // as non-blocking tool lifecycles, not as replyable
            // PermissionRequest hooks. Treat them as observed Provider UI;
            // BridgeRequest keeps `needs_reply = false`.
            EventKind::PermissionRequested
        }
        "PreToolUse"
            if provider == Provider::Codex
                && raw.get("tool_name").and_then(Value::as_str) == Some("update_plan") =>
        {
            // Codex Hooks expose update_plan as a local function call. Treat
            // its allowlisted plan payload as the same fact as the managed
            // app-server turn/plan/updated notification so observe-only
            // desktop sessions can report honest progress too.
            EventKind::PlanUpdated
        }
        "PreToolUse" => EventKind::ToolStarted,
        "PostToolUse" | "AfterAgent" => EventKind::ToolFinished,
        "PostToolUseFailure" => EventKind::ToolFailed,
        "PermissionRequest" if provider != Provider::Gemini => EventKind::PermissionRequested,
        "PermissionDenied" if provider != Provider::Gemini => EventKind::PermissionDenied,
        "Elicitation" if provider == Provider::Claude => EventKind::ElicitationRequested,
        "CodexRequestUserInput" if provider == Provider::Codex => EventKind::QuestionRequested,
        "AgentQuestion" if matches!(provider, Provider::Kimi | Provider::Grok) => {
            EventKind::QuestionRequested
        }
        "AgentElicitation" if matches!(provider, Provider::Kimi | Provider::Grok) => {
            EventKind::ElicitationRequested
        }
        "Notification" => EventKind::Notification,
        "SubagentStart" => EventKind::SubagentStarted,
        "SubagentStop" => EventKind::SubagentStopped,
        "TaskCreated" => EventKind::TaskCreated,
        "TaskCompleted" => EventKind::TaskCompleted,
        "PlanUpdated" => EventKind::PlanUpdated,
        "AutoApprovalReviewStarted" => EventKind::AutoReviewStarted,
        "AutoApprovalReviewCompleted" => EventKind::AutoReviewCompleted,
        "PreCompact" => EventKind::Compacting,
        "Stop" => EventKind::Stopped,
        "TurnInterrupted" => EventKind::Interrupted,
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

fn first_bounded_string_field(raw: &Value, keys: &[&str], maximum_bytes: usize) -> Option<String> {
    keys.iter().find_map(|key| {
        let value = string_field(raw, key)?.trim();
        (!value.is_empty() && value.len() <= maximum_bytes && !value.chars().any(char::is_control))
            .then(|| value.to_owned())
    })
}

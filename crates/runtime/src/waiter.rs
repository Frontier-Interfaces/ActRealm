use flow_agent_core::{BridgeRequest, BridgeResponse, Decision, Provider, ReplyPayload};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{DefaultHasher, Hasher};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum WaiterError {
    #[error("request is not a blocking permission request")]
    NotBlocking,
    #[error("waiter is no longer active")]
    NotActive,
    #[error("waiter registry lock is unavailable")]
    Poisoned,
    #[error("request is not an interactive question")]
    NotInteractive,
    #[error("interactive answer is invalid")]
    InvalidAnswer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractivePrompt {
    pub request_id: Uuid,
    pub kind: String,
    pub provider: String,
    pub title: String,
    pub message: Option<String>,
    pub expires_at: u64,
    pub supports_native: bool,
    pub questions: Vec<InteractiveQuestion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractiveQuestion {
    pub id: String,
    pub label: String,
    pub prompt: String,
    pub input_type: String,
    pub multi_select: bool,
    pub is_secret: bool,
    pub required: bool,
    pub allows_other: bool,
    pub options: Vec<InteractiveOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractiveOption {
    pub label: String,
    pub description: Option<String>,
}

pub struct WaiterTicket {
    request_id: Uuid,
    receiver: mpsc::Receiver<BridgeResponse>,
}

impl WaiterTicket {
    pub fn request_id(&self) -> Uuid {
        self.request_id
    }

    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<BridgeResponse, mpsc::RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }
}

pub struct RegisterResult {
    pub ticket: WaiterTicket,
    pub replaced_request_id: Option<Uuid>,
}

#[derive(Clone, Default)]
pub struct WaiterRegistry {
    inner: Arc<RegistryInner>,
}

#[derive(Default)]
struct RegistryInner {
    state: Mutex<RegistryState>,
}

#[derive(Default)]
struct RegistryState {
    by_request: HashMap<Uuid, WaiterEntry>,
    by_correlation: HashMap<String, Uuid>,
}

struct WaiterEntry {
    correlation: String,
    deadline_at: u64,
    raw: Value,
    sender: mpsc::Sender<BridgeResponse>,
}

impl WaiterRegistry {
    pub fn register_at(
        &self,
        request: &BridgeRequest,
        now: u64,
    ) -> Result<RegisterResult, WaiterError> {
        let (Some(request_id), Some(deadline_at)) = (request.request_id, request.deadline_at)
        else {
            return Err(WaiterError::NotBlocking);
        };
        if !request.needs_reply {
            return Err(WaiterError::NotBlocking);
        }

        let correlation = correlation_key(request);
        let (sender, receiver) = mpsc::channel();
        let mut state = self.inner.state.lock().map_err(|_| WaiterError::Poisoned)?;
        let _ = expire_locked(&mut state, now);

        let replaced_request_id = state
            .by_correlation
            .get(&correlation)
            .copied()
            .filter(|existing| *existing != request_id);
        if let Some(replaced) = replaced_request_id {
            if let Some(old) = remove_locked(&mut state, replaced) {
                let _ = old
                    .sender
                    .send(BridgeResponse::pass_through(replaced, "duplicate_replaced"));
            }
        }

        if let Some(old) = remove_locked(&mut state, request_id) {
            let _ = old
                .sender
                .send(BridgeResponse::pass_through(request_id, "request_replaced"));
        }
        state.by_correlation.insert(correlation.clone(), request_id);
        state.by_request.insert(
            request_id,
            WaiterEntry {
                correlation,
                deadline_at,
                raw: request.raw.clone(),
                sender,
            },
        );

        Ok(RegisterResult {
            ticket: WaiterTicket {
                request_id,
                receiver,
            },
            replaced_request_id,
        })
    }

    pub fn decide(&self, request_id: Uuid, decision: Decision) -> Result<(), WaiterError> {
        self.resolve(request_id, BridgeResponse::decided(request_id, decision))
    }

    pub fn pass_through(
        &self,
        request_id: Uuid,
        reason: impl Into<String>,
    ) -> Result<(), WaiterError> {
        self.resolve(request_id, BridgeResponse::pass_through(request_id, reason))
    }

    pub fn interactive_prompt(
        &self,
        request_id: Uuid,
    ) -> Result<Option<InteractivePrompt>, WaiterError> {
        let state = self.inner.state.lock().map_err(|_| WaiterError::Poisoned)?;
        state
            .by_request
            .get(&request_id)
            .map(|entry| parse_interactive_prompt(request_id, entry.deadline_at, &entry.raw))
            .transpose()
    }

    pub fn answer(&self, request_id: Uuid, submission: &Value) -> Result<(), WaiterError> {
        let mut state = self.inner.state.lock().map_err(|_| WaiterError::Poisoned)?;
        let Some(entry) = state.by_request.get(&request_id) else {
            return Err(WaiterError::NotActive);
        };
        let payload = answer_payload(request_id, entry.deadline_at, &entry.raw, submission)?;
        let Some(entry) = remove_locked(&mut state, request_id) else {
            return Err(WaiterError::NotActive);
        };
        let _ = entry
            .sender
            .send(BridgeResponse::answered(request_id, payload));
        Ok(())
    }

    pub fn expire_at(&self, now: u64) -> Result<usize, WaiterError> {
        Ok(self.expire_request_ids_at(now)?.len())
    }

    pub fn expire_request_ids_at(&self, now: u64) -> Result<Vec<Uuid>, WaiterError> {
        let mut state = self.inner.state.lock().map_err(|_| WaiterError::Poisoned)?;
        Ok(expire_locked(&mut state, now))
    }

    pub fn active_request_ids(&self) -> Result<Vec<Uuid>, WaiterError> {
        let state = self.inner.state.lock().map_err(|_| WaiterError::Poisoned)?;
        Ok(state.by_request.keys().copied().collect())
    }

    pub fn is_active(&self, request_id: Uuid) -> Result<bool, WaiterError> {
        let state = self.inner.state.lock().map_err(|_| WaiterError::Poisoned)?;
        Ok(state.by_request.contains_key(&request_id))
    }

    pub fn raw(&self, request_id: Uuid) -> Result<Option<Value>, WaiterError> {
        let state = self.inner.state.lock().map_err(|_| WaiterError::Poisoned)?;
        Ok(state
            .by_request
            .get(&request_id)
            .map(|entry| entry.raw.clone()))
    }

    fn resolve(&self, request_id: Uuid, response: BridgeResponse) -> Result<(), WaiterError> {
        let mut state = self.inner.state.lock().map_err(|_| WaiterError::Poisoned)?;
        let Some(entry) = remove_locked(&mut state, request_id) else {
            return Err(WaiterError::NotActive);
        };
        let _ = entry.sender.send(response);
        Ok(())
    }
}

impl Drop for RegistryInner {
    fn drop(&mut self) {
        let Ok(state) = self.state.get_mut() else {
            return;
        };
        for (request_id, entry) in state.by_request.drain() {
            let _ = entry
                .sender
                .send(BridgeResponse::pass_through(request_id, "runtime_shutdown"));
        }
        state.by_correlation.clear();
    }
}

fn expire_locked(state: &mut RegistryState, now: u64) -> Vec<Uuid> {
    let expired: Vec<_> = state
        .by_request
        .iter()
        .filter_map(|(request_id, entry)| (now >= entry.deadline_at).then_some(*request_id))
        .collect();
    for request_id in &expired {
        if let Some(entry) = remove_locked(state, *request_id) {
            let _ = entry
                .sender
                .send(BridgeResponse::pass_through(*request_id, "deadline"));
        }
    }
    expired
}

fn remove_locked(state: &mut RegistryState, request_id: Uuid) -> Option<WaiterEntry> {
    let entry = state.by_request.remove(&request_id)?;
    if state.by_correlation.get(&entry.correlation) == Some(&request_id) {
        state.by_correlation.remove(&entry.correlation);
    }
    Some(entry)
}

fn correlation_key(request: &BridgeRequest) -> String {
    let provider_key = match request.provider {
        Provider::Claude => request.prompt_id.as_deref(),
        Provider::Codex => request.provider_turn_id.as_deref(),
        Provider::Gemini => None,
    }
    .map(ToOwned::to_owned)
    .or_else(|| request.request_id.map(|id| id.to_string()))
    .unwrap_or_else(|| "unknown".to_owned());
    let tool = request
        .raw
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let mut input_hasher = DefaultHasher::new();
    input_hasher.write(request.event_name().unwrap_or("unknown").as_bytes());
    for input in [
        request.raw.get("tool_input"),
        request.raw.get("requested_schema"),
        request.raw.get("requestedSchema"),
    ]
    .into_iter()
    .flatten()
    {
        input_hasher.write(input.to_string().as_bytes());
    }
    format!(
        "{}:{}:{}:{}:{:016x}",
        request.provider,
        request.provider_session_id.as_deref().unwrap_or("unknown"),
        provider_key,
        tool,
        input_hasher.finish(),
    )
}

fn parse_interactive_prompt(
    request_id: Uuid,
    expires_at: u64,
    raw: &Value,
) -> Result<InteractivePrompt, WaiterError> {
    if raw.get("hook_event_name").and_then(Value::as_str) == Some("PreToolUse")
        && raw.get("tool_name").and_then(Value::as_str) == Some("AskUserQuestion")
    {
        return parse_claude_question(request_id, expires_at, raw);
    }
    if raw.get("hook_event_name").and_then(Value::as_str) == Some("Elicitation") {
        return parse_claude_elicitation(request_id, expires_at, raw);
    }
    if raw.get("hook_event_name").and_then(Value::as_str) == Some("CodexRequestUserInput") {
        return parse_codex_user_input(request_id, expires_at, raw);
    }
    Err(WaiterError::NotInteractive)
}

fn parse_claude_question(
    request_id: Uuid,
    expires_at: u64,
    raw: &Value,
) -> Result<InteractivePrompt, WaiterError> {
    let source = raw
        .pointer("/tool_input/questions")
        .and_then(Value::as_array)
        .filter(|questions| !questions.is_empty() && questions.len() <= 4)
        .ok_or(WaiterError::NotInteractive)?;
    let mut questions = Vec::with_capacity(source.len());
    for (index, question) in source.iter().enumerate() {
        let prompt =
            bounded_text(question.get("question"), 2_000).ok_or(WaiterError::NotInteractive)?;
        let label = bounded_text(question.get("header"), 64).unwrap_or_else(|| "问题".to_owned());
        let options = question
            .get("options")
            .and_then(Value::as_array)
            .map(|options| {
                options
                    .iter()
                    .take(20)
                    .filter_map(|option| {
                        Some(InteractiveOption {
                            label: bounded_text(option.get("label"), 200)?,
                            description: bounded_text(option.get("description"), 500),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        questions.push(InteractiveQuestion {
            id: format!("q{index}"),
            label,
            prompt,
            input_type: "choice".to_owned(),
            multi_select: question
                .get("multiSelect")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            is_secret: false,
            required: true,
            allows_other: true,
            options,
        });
    }
    Ok(InteractivePrompt {
        request_id,
        kind: "claude_question".to_owned(),
        provider: "claude".to_owned(),
        title: "Claude 正在询问".to_owned(),
        message: None,
        expires_at,
        supports_native: true,
        questions,
    })
}

fn parse_claude_elicitation(
    request_id: Uuid,
    expires_at: u64,
    raw: &Value,
) -> Result<InteractivePrompt, WaiterError> {
    let schema = raw
        .get("requested_schema")
        .or_else(|| raw.get("requestedSchema"))
        .and_then(Value::as_object)
        .ok_or(WaiterError::NotInteractive)?;
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return Err(WaiterError::NotInteractive);
    }
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .filter(|properties| !properties.is_empty() && properties.len() <= 20)
        .ok_or(WaiterError::NotInteractive)?;
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    let mut questions = Vec::with_capacity(properties.len());
    for (id, field) in properties {
        if id.is_empty() || id.len() > 128 {
            return Err(WaiterError::NotInteractive);
        }
        let field = field.as_object().ok_or(WaiterError::NotInteractive)?;
        let schema_type = field
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("string");
        let input_type = match schema_type {
            "boolean" => "boolean",
            "integer" | "number" => "number",
            "string" => {
                if field.get("enum").is_some() {
                    "choice"
                } else {
                    "text"
                }
            }
            _ => return Err(WaiterError::NotInteractive),
        };
        let options = field
            .get("enum")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .take(50)
                    .filter_map(|value| bounded_text(Some(value), 500))
                    .map(|label| InteractiveOption {
                        label,
                        description: None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        questions.push(InteractiveQuestion {
            id: id.clone(),
            label: bounded_text(field.get("title"), 200).unwrap_or_else(|| id.clone()),
            prompt: bounded_text(field.get("description"), 2_000).unwrap_or_default(),
            input_type: input_type.to_owned(),
            multi_select: false,
            is_secret: field.get("format").and_then(Value::as_str) == Some("password")
                || field.get("writeOnly").and_then(Value::as_bool) == Some(true)
                || field.get("isSecret").and_then(Value::as_bool) == Some(true),
            required: required.contains(id.as_str()),
            allows_other: false,
            options,
        });
    }
    Ok(InteractivePrompt {
        request_id,
        kind: "claude_elicitation".to_owned(),
        provider: "claude".to_owned(),
        title: "Claude 需要补充信息".to_owned(),
        message: bounded_text(raw.get("message"), 2_000),
        expires_at,
        supports_native: true,
        questions,
    })
}

fn parse_codex_user_input(
    request_id: Uuid,
    expires_at: u64,
    raw: &Value,
) -> Result<InteractivePrompt, WaiterError> {
    let source = raw
        .get("questions")
        .and_then(Value::as_array)
        .filter(|questions| !questions.is_empty() && questions.len() <= 20)
        .ok_or(WaiterError::NotInteractive)?;
    let mut ids = HashSet::new();
    let mut questions = Vec::with_capacity(source.len());
    for question in source {
        let id = bounded_text(question.get("id"), 128).ok_or(WaiterError::NotInteractive)?;
        if !ids.insert(id.clone()) {
            return Err(WaiterError::NotInteractive);
        }
        let is_secret = question
            .get("isSecret")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let options = if is_secret {
            Vec::new()
        } else {
            question
                .get("options")
                .and_then(Value::as_array)
                .map(|options| {
                    options
                        .iter()
                        .take(50)
                        .filter_map(|option| {
                            Some(InteractiveOption {
                                label: bounded_text(option.get("label"), 500)?,
                                description: bounded_text(option.get("description"), 1_000),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        questions.push(InteractiveQuestion {
            id,
            label: bounded_text(question.get("header"), 200).unwrap_or_else(|| "问题".to_owned()),
            prompt: bounded_text(question.get("question"), 2_000)
                .ok_or(WaiterError::NotInteractive)?,
            input_type: if options.is_empty() { "text" } else { "choice" }.to_owned(),
            multi_select: false,
            is_secret,
            required: true,
            allows_other: question
                .get("isOther")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            options,
        });
    }
    Ok(InteractivePrompt {
        request_id,
        kind: "codex_user_input".to_owned(),
        provider: "codex".to_owned(),
        title: "Codex 正在询问".to_owned(),
        message: None,
        expires_at,
        supports_native: false,
        questions,
    })
}

fn answer_payload(
    request_id: Uuid,
    expires_at: u64,
    raw: &Value,
    submission: &Value,
) -> Result<ReplyPayload, WaiterError> {
    let prompt = parse_interactive_prompt(request_id, expires_at, raw)?;
    let action = submission
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("accept");
    if prompt.kind == "claude_question" {
        if action != "accept" {
            return Err(WaiterError::InvalidAnswer);
        }
        let submitted = submission
            .get("answers")
            .and_then(Value::as_object)
            .ok_or(WaiterError::InvalidAnswer)?;
        if submitted.len() != prompt.questions.len() {
            return Err(WaiterError::InvalidAnswer);
        }
        let source = raw
            .pointer("/tool_input/questions")
            .and_then(Value::as_array)
            .ok_or(WaiterError::InvalidAnswer)?;
        let mut answers = BTreeMap::new();
        for (index, question) in prompt.questions.iter().enumerate() {
            let values = answer_strings(submitted.get(&question.id), question.multi_select)?;
            let provider_key = source[index]
                .get("question")
                .and_then(Value::as_str)
                .ok_or(WaiterError::InvalidAnswer)?;
            answers.insert(provider_key.to_owned(), values.join(", "));
        }
        return Ok(ReplyPayload::ClaudeQuestion { answers });
    }

    if prompt.kind == "codex_user_input" {
        if action != "accept" {
            return Err(WaiterError::InvalidAnswer);
        }
        let submitted = submission
            .get("answers")
            .and_then(Value::as_object)
            .ok_or(WaiterError::InvalidAnswer)?;
        if submitted.len() != prompt.questions.len() {
            return Err(WaiterError::InvalidAnswer);
        }
        let mut answers = BTreeMap::new();
        for question in &prompt.questions {
            answers.insert(
                question.id.clone(),
                answer_strings(submitted.get(&question.id), question.multi_select)?,
            );
        }
        return Ok(ReplyPayload::CodexUserInput { answers });
    }

    if !matches!(action, "accept" | "decline" | "cancel") {
        return Err(WaiterError::InvalidAnswer);
    }
    if action != "accept" {
        return Ok(ReplyPayload::ClaudeElicitation {
            action: action.to_owned(),
            content: None,
        });
    }
    let submitted = submission
        .get("answers")
        .and_then(Value::as_object)
        .ok_or(WaiterError::InvalidAnswer)?;
    if submitted
        .keys()
        .any(|key| !prompt.questions.iter().any(|question| question.id == *key))
    {
        return Err(WaiterError::InvalidAnswer);
    }
    let mut content = Map::new();
    for question in &prompt.questions {
        let value = submitted.get(&question.id);
        if value.is_none() && question.required {
            return Err(WaiterError::InvalidAnswer);
        }
        let Some(value) = value else { continue };
        let normalized = normalize_elicitation_value(question, value)?;
        content.insert(question.id.clone(), normalized);
    }
    Ok(ReplyPayload::ClaudeElicitation {
        action: "accept".to_owned(),
        content: Some(Value::Object(content)),
    })
}

fn answer_strings(value: Option<&Value>, multi_select: bool) -> Result<Vec<String>, WaiterError> {
    let values = value
        .and_then(Value::as_array)
        .filter(|values| !values.is_empty() && values.len() <= if multi_select { 20 } else { 1 })
        .ok_or(WaiterError::InvalidAnswer)?;
    values
        .iter()
        .map(|value| bounded_text(Some(value), 2_000).ok_or(WaiterError::InvalidAnswer))
        .collect()
}

fn normalize_elicitation_value(
    question: &InteractiveQuestion,
    value: &Value,
) -> Result<Value, WaiterError> {
    match question.input_type.as_str() {
        "boolean" => value
            .as_bool()
            .map(Value::Bool)
            .ok_or(WaiterError::InvalidAnswer),
        "number" => value
            .as_f64()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .ok_or(WaiterError::InvalidAnswer),
        "choice" | "text" => {
            let text = bounded_text(Some(value), 8_192).ok_or(WaiterError::InvalidAnswer)?;
            if !question.options.is_empty()
                && !question.options.iter().any(|option| option.label == text)
            {
                return Err(WaiterError::InvalidAnswer);
            }
            Ok(Value::String(text))
        }
        _ => Err(WaiterError::InvalidAnswer),
    }
}

fn bounded_text(value: Option<&Value>, max_chars: usize) -> Option<String> {
    let text = value?.as_str()?.trim();
    (!text.is_empty() && text.chars().count() <= max_chars).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flow_agent_core::{BridgeRequest, Provider, ReplyAction};
    use serde_json::json;

    #[test]
    fn claude_question_is_sanitized_validated_and_released_after_answer() {
        let registry = WaiterRegistry::default();
        let request = BridgeRequest::from_hook_at(
            Provider::Claude,
            json!({
                "hook_event_name":"PreToolUse",
                "session_id":"question-session",
                "tool_name":"AskUserQuestion",
                "tool_input":{"questions":[{
                    "question":"选择部署环境？",
                    "header":"环境",
                    "options":[{"label":"测试","description":"先验证"},{"label":"生产","description":"直接上线"}],
                    "multiSelect":false
                }]}
            }),
            1_000,
        );
        let registration = registry.register_at(&request, 1_000).unwrap();
        let request_id = request.request_id.unwrap();
        let prompt = registry.interactive_prompt(request_id).unwrap().unwrap();
        assert_eq!(prompt.kind, "claude_question");
        assert_eq!(prompt.questions[0].prompt, "选择部署环境？");
        let encoded = serde_json::to_string(&prompt).unwrap();
        assert!(!encoded.contains("tool_input"));
        assert!(!encoded.contains("raw"));

        registry
            .answer(
                request_id,
                &json!({"action":"accept","answers":{"q0":["测试"]}}),
            )
            .unwrap();
        let response = registration
            .ticket
            .recv_timeout(Duration::from_millis(20))
            .unwrap();
        assert_eq!(response.action, ReplyAction::Answer);
        assert_eq!(
            response.payload,
            Some(ReplyPayload::ClaudeQuestion {
                answers: BTreeMap::from([("选择部署环境？".to_owned(), "测试".to_owned())])
            })
        );
        assert!(!registry.is_active(request_id).unwrap());
        assert!(registry.raw(request_id).unwrap().is_none());
    }

    #[test]
    fn elicitation_secret_is_password_typed_and_never_retained_after_submit() {
        let registry = WaiterRegistry::default();
        let request = BridgeRequest::from_hook_at(
            Provider::Claude,
            json!({
                "hook_event_name":"Elicitation",
                "session_id":"secret-session",
                "message":"需要认证",
                "requested_schema":{
                    "type":"object",
                    "required":["api_key"],
                    "properties":{
                        "api_key":{"type":"string","title":"API Key","format":"password","writeOnly":true}
                    }
                }
            }),
            2_000,
        );
        let registration = registry.register_at(&request, 2_000).unwrap();
        let request_id = request.request_id.unwrap();
        let prompt = registry.interactive_prompt(request_id).unwrap().unwrap();
        assert!(prompt.questions[0].is_secret);

        registry
            .answer(
                request_id,
                &json!({"action":"accept","answers":{"api_key":"top-secret-value"}}),
            )
            .unwrap();
        let response = registration
            .ticket
            .recv_timeout(Duration::from_millis(20))
            .unwrap();
        assert!(serde_json::to_string(&response)
            .unwrap()
            .contains("top-secret-value"));
        assert!(registry.raw(request_id).unwrap().is_none());
        assert!(registry.interactive_prompt(request_id).unwrap().is_none());
    }

    #[test]
    fn invalid_or_expired_interactive_submissions_are_rejected() {
        let registry = WaiterRegistry::default();
        let request = BridgeRequest::from_hook_at(
            Provider::Claude,
            json!({
                "hook_event_name":"PreToolUse",
                "session_id":"invalid-session",
                "tool_name":"AskUserQuestion",
                "tool_input":{"questions":[{"question":"继续？","header":"确认","options":[],"multiSelect":false}]}
            }),
            3_000,
        );
        registry.register_at(&request, 3_000).unwrap();
        let request_id = request.request_id.unwrap();
        assert_eq!(
            registry.answer(request_id, &json!({"action":"accept","answers":{}})),
            Err(WaiterError::InvalidAnswer)
        );
        registry.expire_at(u64::MAX).unwrap();
        assert_eq!(
            registry.answer(
                request_id,
                &json!({"action":"accept","answers":{"q0":["yes"]}})
            ),
            Err(WaiterError::NotActive)
        );
    }

    #[test]
    fn codex_request_user_input_uses_question_ids_and_secret_capability() {
        let registry = WaiterRegistry::default();
        let request = BridgeRequest::codex_user_input_at(
            json!({
                "threadId":"thread-1",
                "turnId":"turn-1",
                "itemId":"item-1",
                "questions":[{
                    "id":"token",
                    "header":"Token",
                    "question":"Enter token",
                    "isOther":false,
                    "isSecret":true,
                    "options":null
                }]
            }),
            4_000,
        )
        .unwrap();
        let registration = registry.register_at(&request, 4_000).unwrap();
        let request_id = request.request_id.unwrap();
        let prompt = registry.interactive_prompt(request_id).unwrap().unwrap();
        assert_eq!(prompt.kind, "codex_user_input");
        assert!(!prompt.supports_native);
        assert!(prompt.questions[0].is_secret);
        registry
            .answer(
                request_id,
                &json!({"action":"accept","answers":{"token":["secret"]}}),
            )
            .unwrap();
        assert_eq!(
            registration
                .ticket
                .recv_timeout(Duration::from_millis(20))
                .unwrap()
                .payload,
            Some(ReplyPayload::CodexUserInput {
                answers: BTreeMap::from([("token".to_owned(), vec!["secret".to_owned()])])
            })
        );
        assert!(registry.raw(request_id).unwrap().is_none());
    }
}

//! Bounded ACP v1 projections. No provider prose grants a control capability.
use actrealm_core::{
    BlockingRequestKind, BridgeRequest, BridgeResponse, Provider, ReplyAction, ReplyPayload,
};
use serde_json::{json, Value};
use std::collections::HashMap;

pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// The Grok leader forwards both direct ACP extension calls and gateway-wrapped
/// calls. Normalize the envelope only; no content grants a reply capability.
pub fn normalize_frame(frame: &Value) -> Value {
    let mut value = frame.clone();
    let Some(method) = frame["method"].as_str() else {
        return value;
    };
    if method.trim_start_matches('_').starts_with("x.ai/") {
        let inner = frame["params"]["method"]
            .as_str()
            .filter(|m| m.trim_start_matches('_') == method.trim_start_matches('_'));
        if inner.is_some() && frame["params"]["params"].is_object() {
            value["params"] = frame["params"]["params"].clone();
        }
        value["method"] = json!(format!("_{}", method.trim_start_matches('_')));
    }
    if let Some(params) = value.get_mut("params").and_then(Value::as_object_mut) {
        for (from, to) in [("session_id", "sessionId"), ("tool_call_id", "toolCallId")] {
            if !params.contains_key(to) {
                if let Some(v) = params.get(from).cloned() {
                    params.insert(to.into(), v);
                }
            }
        }
    }
    value
}

pub struct Observer {
    pub provider: Provider,
    pub session: String,
    pub cwd: String,
    pub model: Option<String>,
    pub turn: Option<String>,
    tools: HashMap<String, Value>,
}

impl Observer {
    pub fn new(provider: Provider, session: String, cwd: String) -> Self {
        Self {
            provider,
            session,
            cwd,
            model: None,
            turn: None,
            tools: HashMap::new(),
        }
    }

    pub fn begin_turn(&mut self, id: String) {
        self.turn = Some(id);
        self.tools.clear();
    }

    pub fn event(&self, event: &str, mut raw: Value, at: u64) -> BridgeRequest {
        raw["cwd"] = json!(self.cwd);
        raw["source_version"] = json!("acp/1");
        if let Some(model) = &self.model {
            raw["model"] = json!(model);
        }
        BridgeRequest::from_provider_event_at(
            self.provider,
            event,
            &self.session,
            self.turn.as_deref(),
            raw,
            at,
        )
    }

    pub fn notification(&mut self, frame: &Value, at: u64) -> Vec<BridgeRequest> {
        if frame["method"] != "session/update" || frame["params"]["sessionId"] != self.session {
            return Vec::new();
        }
        let update = &frame["params"]["update"];
        match update["sessionUpdate"].as_str() {
            Some("tool_call" | "tool_call_update") => {
                let Some(id) = bounded(&update["toolCallId"], 256) else {
                    return Vec::new();
                };
                if self.tools.len() >= 4096 && !self.tools.contains_key(id) {
                    return Vec::new();
                }
                let tool = self.tools.entry(id.to_owned()).or_insert_with(|| json!({}));
                let old_status = tool["status"].as_str().map(ToOwned::to_owned);
                if let (Some(target), Some(fields)) = (tool.as_object_mut(), update.as_object()) {
                    for (key, value) in fields {
                        target.insert(key.clone(), value.clone());
                    }
                }
                let state = tool["status"].as_str().unwrap_or("pending");
                if old_status.as_deref() == Some(state) {
                    return Vec::new();
                }
                let event = match state {
                    "in_progress" => "PreToolUse",
                    "completed" => "PostToolUse",
                    "failed" => "PostToolUseFailure",
                    _ => return Vec::new(),
                };
                let raw = tool_payload(tool);
                vec![self.event(event, raw, at)]
            }
            Some("plan") => {
                let Some(entries) = update["entries"].as_array().filter(|v| v.len() <= 100) else {
                    return Vec::new();
                };
                let plan = entries.iter().filter_map(|e| Some(json!({
                    "step":bounded(&e["content"],2000)?,
                    "status":match e["status"].as_str() { Some("completed")=>"completed",Some("in_progress")=>"in_progress",_=>"pending" }
                }))).collect::<Vec<_>>();
                vec![self.event("PlanUpdated", json!({"plan":plan}), at)]
            }
            Some("session_info_update") => bounded(&update["title"], 500)
                .map(|title| vec![self.event("Notification", json!({"session_title":title}), at)])
                .unwrap_or_default(),
            Some("config_option_update") => {
                if let Some(model) = selected_model(update) {
                    self.model = Some(model);
                    return vec![self.event("Notification", json!({}), at)];
                }
                Vec::new()
            }
            Some("current_mode_update" | "available_commands_update") => Vec::new(),
            _ => Vec::new(),
        }
    }

    pub fn interaction(&self, frame: &Value, at: u64) -> Option<Interaction> {
        let normalized = normalize_frame(frame);
        let frame = &normalized;
        let id = frame.get("id")?.clone();
        if !(id.is_i64() || id.is_u64() || bounded(&id, 256).is_some()) {
            return None;
        }
        let params = frame.get("params")?;
        if params["sessionId"] != self.session {
            return None;
        }
        let (kind, raw, route) = match frame["method"].as_str()? {
            "_x.ai/exit_plan_mode" if self.provider == Provider::Grok => {
                let plan = bounded(&params["planContent"], 65536)?;
                (
                    BlockingRequestKind::Permission,
                    json!({"tool_name":"ExitPlanMode","tool_input":{},"plan_content":plan}),
                    ReplyRoute::GrokPlan,
                )
            }
            "_x.ai/mcp/elicit" if self.provider == Provider::Grok => {
                if params["mode"] != "form" {
                    return None;
                }
                (
                    BlockingRequestKind::AgentElicitation,
                    json!({"requestedSchema":params.get("requestedSchema")?,"message":params["message"],"tool_name":"AskUserQuestion"}),
                    ReplyRoute::GrokElicitation,
                )
            }
            "_x.ai/ask_user_question" if self.provider == Provider::Grok => {
                let source = params["questions"]
                    .as_array()
                    .filter(|q| !q.is_empty() && q.len() <= 20)?;
                let mut mapping = Vec::new();
                let mut questions = Vec::new();
                for (index, question) in source.iter().enumerate() {
                    let text = bounded(&question["question"], 2000)?;
                    if mapping
                        .iter()
                        .any(|(_, old): &(String, String)| old == text)
                    {
                        return None;
                    }
                    let id = format!("q{index}");
                    mapping.push((id.clone(), text.to_owned()));
                    let options=question["options"].as_array().map(|items|items.iter().take(50).filter_map(|option|Some(json!({"label":bounded(&option["label"],500)?,"description":option.get("description")}))).collect::<Vec<_>>()).unwrap_or_default();
                    questions.push(json!({"id":id,"header":question.get("header").and_then(Value::as_str).unwrap_or("Grok"),"question":text,"options":options,"multiSelect":question["multi_select"].as_bool().or_else(||question["multiSelect"].as_bool()).unwrap_or(false),"isOther":true}));
                }
                (
                    BlockingRequestKind::AgentQuestion,
                    json!({"tool_name":"AskUserQuestion","questions":questions}),
                    ReplyRoute::GrokQuestion(mapping),
                )
            }
            "session/request_permission" => {
                let options = params["options"]
                    .as_array()?
                    .iter()
                    .map(|o| {
                        Some((
                            bounded(&o["kind"], 64)?.to_owned(),
                            bounded(&o["optionId"], 256)?.to_owned(),
                            bounded(&o["name"], 500)?.to_owned(),
                        ))
                    })
                    .collect::<Option<Vec<_>>>()?;
                if options.is_empty() || options.len() > 50 {
                    return None;
                }
                let allows = options
                    .iter()
                    .filter(|(kind, _, _)| kind == "allow_once")
                    .collect::<Vec<_>>();
                let deny = options
                    .iter()
                    .find(|(kind, _, _)| kind == "reject_once")
                    .map(|(_, id, _)| id.clone());
                if allows.len() > 1 {
                    let choices = options
                        .iter()
                        .filter(|(kind, _, _)| {
                            matches!(kind.as_str(), "allow_once" | "reject_once")
                        })
                        .map(|(_, id, label)| (label.clone(), id.clone()))
                        .collect::<Vec<_>>();
                    let title =
                        bounded(&params["toolCall"]["title"], 2000).unwrap_or("Choose an option");
                    let raw = json!({"tool_name":"AskUserQuestion","questions":[{"id":"choice","header":"Agent","question":title,"options":choices.iter().map(|(label,_)|json!({"label":label})).collect::<Vec<_>>()}]});
                    (
                        BlockingRequestKind::AgentQuestion,
                        raw,
                        ReplyRoute::Choice(choices),
                    )
                } else {
                    (
                        BlockingRequestKind::Permission,
                        tool_payload(&params["toolCall"]),
                        ReplyRoute::Permission {
                            allow: allows.first().map(|(_, id, _)| id.clone()),
                            deny,
                        },
                    )
                }
            }
            "elicitation/create" => {
                if params["mode"].as_str().unwrap_or("form") != "form" {
                    return None;
                }
                let schema = params.get("requestedSchema")?.clone();
                (
                    BlockingRequestKind::AgentElicitation,
                    json!({"requestedSchema":schema,"message":params["message"],"tool_name":"AskUserQuestion"}),
                    ReplyRoute::Elicitation,
                )
            }
            _ => return None,
        };
        let mut request = BridgeRequest::from_agent_request_at(
            self.provider,
            &self.session,
            self.turn.as_deref(),
            kind,
            raw,
            at,
            at.saturating_add(60 * 60 * 1000),
        )?;
        request.raw["cwd"] = json!(self.cwd);
        request.raw["source_version"] = json!("acp/1");
        if let Some(id) = bounded(&params["toolCallId"], 256) {
            request.raw["tool_call_id"] = json!(id);
        }
        if matches!(&route, ReplyRoute::Permission { allow: None, .. }) {
            request.remote_action_capability = None;
        }
        Some(Interaction {
            rpc_id: id,
            request,
            route,
        })
    }
}

pub struct Interaction {
    pub rpc_id: Value,
    pub request: BridgeRequest,
    pub route: ReplyRoute,
}
pub enum ReplyRoute {
    Permission {
        allow: Option<String>,
        deny: Option<String>,
    },
    Choice(Vec<(String, String)>),
    GrokQuestion(Vec<(String, String)>),
    GrokPlan,
    GrokElicitation,
    Elicitation,
}

impl ReplyRoute {
    pub fn response(&self, response: Option<&BridgeResponse>) -> Value {
        match self {
            Self::GrokPlan => {
                json!({"outcome":if response.is_some_and(|r|r.action==ReplyAction::Allow){"approved"}else{"cancelled"}})
            }
            Self::GrokElicitation => response
                .and_then(|r| match &r.payload {
                    Some(ReplyPayload::AgentElicitation { action, content })
                        if matches!(action.as_str(), "accept" | "decline" | "cancel") =>
                    {
                        let mut result = json!({"outcome":action});
                        if action == "accept" {
                            result["content"] = content.clone().unwrap_or_else(|| json!({}));
                        }
                        Some(result)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| json!({"outcome":"cancel"})),
            Self::GrokQuestion(questions) => {
                if let Some(BridgeResponse {
                    payload: Some(ReplyPayload::AgentQuestion { answers }),
                    ..
                }) = response
                {
                    if answers.len() == questions.len()
                        && questions.iter().all(|(id, _)| answers.contains_key(id))
                    {
                        let mapped = questions
                            .iter()
                            .map(|(id, text)| (text.clone(), json!(answers[id])))
                            .collect::<serde_json::Map<_, _>>();
                        return json!({"outcome":"accepted","answers":mapped});
                    }
                }
                json!({"outcome":"cancelled"})
            }
            Self::Permission { allow, deny } => {
                let option = response.and_then(|r| match r.action {
                    ReplyAction::Allow => allow.as_ref(),
                    ReplyAction::Deny => deny.as_ref(),
                    _ => None,
                });
                option
                    .map(|id| json!({"outcome":{"outcome":"selected","optionId":id}}))
                    .unwrap_or_else(|| json!({"outcome":{"outcome":"cancelled"}}))
            }
            Self::Choice(options) => {
                let choice = response.and_then(|r| match &r.payload {
                    Some(ReplyPayload::AgentQuestion { answers }) => answers
                        .get("choice")
                        .filter(|v| v.len() == 1)
                        .and_then(|v| options.iter().find(|(label, _)| label == &v[0])),
                    _ => None,
                });
                choice
                    .map(|(_, id)| json!({"outcome":{"outcome":"selected","optionId":id}}))
                    .unwrap_or_else(|| json!({"outcome":{"outcome":"cancelled"}}))
            }
            Self::Elicitation => response
                .and_then(|r| match &r.payload {
                    Some(ReplyPayload::AgentElicitation { action, content }) => {
                        Some(json!({"action":action,"content":content}))
                    }
                    _ => None,
                })
                .unwrap_or_else(|| json!({"action":"cancel"})),
        }
    }
}

fn tool_payload(tool: &Value) -> Value {
    let name = match tool["kind"].as_str() {
        Some("read") => "Read",
        Some("edit") => "Edit",
        Some("delete") => "Delete",
        Some("execute") => "Bash",
        Some("search") => "Search",
        Some("fetch") => "WebFetch",
        _ => "Tool",
    };
    let mut input = tool
        .get("rawInput")
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    if input.get("file_path").is_none() {
        if let Some(path) = tool
            .pointer("/locations/0/path")
            .and_then(Value::as_str)
            .filter(|s| s.len() <= 4096)
        {
            input["file_path"] = json!(path);
        }
    }
    json!({"tool_name":name,"tool_input":input,"tool_call_id":tool["toolCallId"]})
}
fn bounded(value: &Value, limit: usize) -> Option<&str> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= limit && !s.contains('\0'))
}

pub fn selected_model(value: &Value) -> Option<String> {
    value
        .pointer("/models/currentModelId")
        .and_then(|v| bounded(v, 128))
        .map(ToOwned::to_owned)
        .or_else(|| {
            value["configOptions"]
                .as_array()?
                .iter()
                .find(|option| option["category"] == "model" || option["id"] == "model")
                .and_then(|option| bounded(&option["currentValue"], 128))
                .map(ToOwned::to_owned)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use actrealm_core::Decision;

    #[test]
    fn grok_plan_and_mcp_form_keep_distinct_provider_reply_shapes() {
        let observer = Observer::new(Provider::Grok, "s".into(), "/tmp".into());
        let plan = json!({"id":"plan1","method":"_x.ai/exit_plan_mode","params":{"sessionId":"s","toolCallId":"t","planContent":"1. Read the file.\n2. Propose an edit."}});
        let interaction = observer.interaction(&plan, 1000).unwrap();
        assert!(interaction.request.needs_reply);
        assert_eq!(
            interaction.request.raw["plan_content"],
            plan["params"]["planContent"]
        );
        let reply = BridgeResponse::decided(interaction.request.id, Decision::Allow);
        assert_eq!(
            interaction.route.response(Some(&reply)),
            json!({"outcome":"approved"})
        );
        let reply = BridgeResponse::decided(interaction.request.id, Decision::Deny);
        assert_eq!(
            interaction.route.response(Some(&reply)),
            json!({"outcome":"cancelled"})
        );
        let mut no_context = plan;
        no_context["params"]["planContent"] = Value::Null;
        assert!(observer.interaction(&no_context, 1000).is_none());
        let form = json!({"id":"form1","method":"_x.ai/mcp/elicit","params":{"sessionId":"s","toolCallId":"mcp1","mode":"form","message":"Choose project","requestedSchema":{"type":"object","properties":{"project":{"type":"string"}}}}});
        let interaction = observer.interaction(&form, 1000).unwrap();
        let reply = BridgeResponse::answered(
            interaction.request.id,
            ReplyPayload::AgentElicitation {
                action: "accept".into(),
                content: Some(json!({"project":"scratch"})),
            },
        );
        assert_eq!(
            interaction.route.response(Some(&reply)),
            json!({"outcome":"accept","content":{"project":"scratch"}})
        );
        let mut url = form;
        url["params"]["mode"] = json!("url");
        assert!(observer.interaction(&url, 1000).is_none());
    }

    #[test]
    fn shell_permission_hooks_never_acquire_connector_authority() {
        for provider in [Provider::Kimi, Provider::Grok] {
            let request = BridgeRequest::from_hook_at(
                provider,
                json!({"hookEventName":"PermissionRequest","sessionId":"s","toolName":"Bash","toolInput":{"command":"ls"}}),
                1000,
            );
            assert!(!request.needs_reply);
            assert!(request.request_id.is_none());
            assert!(request.remote_action_capability.is_none());
            assert!(request.provider_handles_approval);
            assert_eq!(request.session_id(), Some("s"));
        }
    }

    #[test]
    fn permission_reply_uses_only_the_advertised_once_options() {
        let observer = Observer::new(Provider::Grok, "s".into(), "/tmp".into());
        let frame = json!({"id":7,"method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"t","title":"Run ls","kind":"execute","rawInput":{"command":"ls"}},"options":[{"optionId":"yes-once","name":"Allow once","kind":"allow_once"},{"optionId":"never","name":"Always allow","kind":"allow_always"},{"optionId":"no","name":"Deny","kind":"reject_once"}]}});
        let interaction = observer.interaction(&frame, 1000).unwrap();
        assert!(interaction.request.needs_reply);
        let reply =
            BridgeResponse::decided(interaction.request.request_id.unwrap(), Decision::Allow);
        assert_eq!(
            interaction.route.response(Some(&reply)),
            json!({"outcome":{"outcome":"selected","optionId":"yes-once"}})
        );
        assert_eq!(
            interaction.route.response(None),
            json!({"outcome":{"outcome":"cancelled"}})
        );
        let mut changed = frame.clone();
        changed["params"]["sessionId"] = json!("other");
        assert!(observer.interaction(&changed, 1000).is_none());
    }

    #[test]
    fn always_allow_only_does_not_become_one_time_approval() {
        let observer = Observer::new(Provider::Kimi, "s".into(), "/tmp".into());
        let frame = json!({"id":1,"method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"t"},"options":[{"optionId":"persist","name":"Always","kind":"allow_always"}]}});
        let interaction = observer.interaction(&frame, 1).unwrap();
        assert!(interaction.request.remote_action_capability.is_none());
        assert_eq!(
            interaction.route.response(Some(&BridgeResponse::decided(
                interaction.request.request_id.unwrap(),
                Decision::Allow
            ))),
            json!({"outcome":{"outcome":"cancelled"}})
        );
    }

    #[test]
    fn pending_tools_are_not_reported_as_executing_and_duplicates_do_not_replay() {
        let mut observer = Observer::new(Provider::Kimi, "s".into(), "/tmp".into());
        let mut frame = json!({"method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"tool_call","toolCallId":"t","kind":"read","status":"pending","locations":[{"path":"/tmp/file.rs"}]}}});
        assert!(observer.notification(&frame, 1).is_empty());
        frame["params"]["update"]["status"] = json!("in_progress");
        let events = observer.notification(&frame, 2);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_name(), Some("PreToolUse"));
        assert_eq!(events[0].raw["tool_input"]["file_path"], "/tmp/file.rs");
        assert!(observer.notification(&frame, 3).is_empty());
        frame["params"]["update"]["status"] = json!("failed");
        assert_eq!(
            observer.notification(&frame, 4)[0].event_name(),
            Some("PostToolUseFailure")
        );
    }
}

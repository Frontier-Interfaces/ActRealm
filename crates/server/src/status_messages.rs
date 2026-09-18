use actrealm_quota::QuotaEntry;
use actrealm_runtime::{AttentionRecord, SessionRecord};
use serde_json::{json, Map, Value};

fn message(code: &'static str, args: Map<String, Value>) -> Value {
    json!({
        "code": code,
        "args": args,
    })
}

fn empty_message(code: &'static str) -> Value {
    message(code, Map::new())
}

fn string_arg(key: &str, value: impl Into<String>) -> Map<String, Value> {
    let mut args = Map::new();
    args.insert(key.to_owned(), Value::String(value.into()));
    args
}

pub(crate) fn session_activity(
    session: &SessionRecord,
    blocking_attention_kind: Option<&str>,
) -> Value {
    // Unicode-escaped branches recognize rows written by pre-contract builds.
    // New Runtime state remains English plus structured message codes.
    if blocking_attention_kind == Some("question") {
        return empty_message("session.activity.awaiting_answer");
    }
    if matches!(
        blocking_attention_kind,
        Some("approval" | "native_approval")
    ) {
        return empty_message("session.activity.awaiting_approval");
    }
    if let (Some(done), Some(total)) = (session.plan_done, session.plan_total) {
        if session.activity.as_deref().is_some_and(|text| {
            text.starts_with("Plan progress ")
                || text.starts_with("\u{8ba1}\u{5212}\u{8fdb}\u{5ea6} ")
        }) {
            let mut args = string_arg("done", done.to_string());
            args.insert("total".to_owned(), Value::String(total.to_string()));
            return message("session.activity.plan_progress", args);
        }
    }
    if matches!(
        session.activity.as_deref(),
        Some(
            "The operation was denied in the Agent"
                | "\u{64cd}\u{4f5c}\u{5df2}\u{5728} Agent \u{4e2d}\u{62d2}\u{7edd}"
        )
    ) {
        return empty_message("session.activity.permission_denied");
    }
    if session.activity.as_deref().is_some_and(|text| {
        (text.contains("waiting for") && text.contains("Agent event"))
            || text.contains("\u{7b49}\u{5f85} Agent")
            || text.contains("\u{65e7}\u{56de}\u{590d}\u{901a}\u{9053}\u{5931}\u{6548}")
    }) {
        return empty_message("session.activity.waiting_for_provider_event");
    }
    if session.activity.as_deref().is_some_and(|text| {
        text.starts_with("Unrecognized event")
            || text.starts_with("\u{4e8b}\u{4ef6}\u{4e0d}\u{8bc6}\u{522b}")
    }) {
        return empty_message("session.activity.unknown_event");
    }
    if let Some(count) = session
        .activity
        .as_deref()
        .and_then(|text| {
            text.strip_suffix(" background tasks still running")
                .or_else(|| {
                    text.strip_suffix(
                        " \u{4e2a}\u{540e}\u{53f0}\u{4efb}\u{52a1}\u{4ecd}\u{5728}\u{8fd0}\u{884c}",
                    )
                })
        })
        .and_then(|value| value.parse::<u32>().ok())
    {
        return message(
            "session.activity.background_tasks_running",
            string_arg("count", count.to_string()),
        );
    }
    if session.exec_state == "tool_running" {
        let tool = session.current_tool.as_deref().unwrap_or("tool");
        return message("session.activity.tool_running", string_arg("tool", tool));
    }
    if session.exec_state == "thinking" && session.active_subagents > 0 {
        return message(
            "session.activity.subagents_running",
            string_arg("count", session.active_subagents.to_string()),
        );
    }
    let code = match session.exec_state.as_str() {
        "thinking" => "session.activity.thinking",
        "waiting_for_event" => "session.activity.waiting_for_provider_event",
        "awaiting_approval" => "session.activity.awaiting_approval",
        "compacting" => "session.activity.compacting",
        "response_finished" => "session.activity.completed",
        "failed"
            if matches!(
                session.activity.as_deref(),
                Some("Turn interrupted" | "\u{672c}\u{8f6e}\u{5df2}\u{4e2d}\u{65ad}")
            ) =>
        {
            "session.activity.interrupted"
        }
        "failed" => "session.activity.failed",
        "idle"
            if matches!(
                session.activity.as_deref(),
                Some("Session ended" | "\u{4f1a}\u{8bdd}\u{5df2}\u{7ed3}\u{675f}")
            ) =>
        {
            "session.activity.ended"
        }
        _ => "session.activity.idle",
    };
    empty_message(code)
}

pub(crate) fn attention_title(attention: &AttentionRecord) -> Value {
    match attention.kind.as_str() {
        "native_approval" => message(
            "attention.native_approval.title",
            string_arg("provider", provider_name(&attention.provider)),
        ),
        "question" => message(
            "attention.question.title",
            string_arg("provider", provider_name(&attention.provider)),
        ),
        "error" => empty_message("attention.error.title"),
        "interrupted" => empty_message("attention.interrupted.title"),
        "completion" => empty_message("attention.completion.title"),
        _ => empty_message("attention.approval.title"),
    }
}

pub(crate) fn attention_detail(attention: &AttentionRecord) -> Option<Value> {
    match attention.kind.as_str() {
        "native_approval" => Some(message(
            "attention.native_approval.detail",
            string_arg("provider", provider_name(&attention.provider)),
        )),
        "question" => Some(empty_message("attention.question.detail")),
        "approval" => Some(empty_message("attention.approval.detail")),
        _ => None,
    }
}

pub(crate) fn attention_risks(attention: &AttentionRecord) -> Vec<Value> {
    if !attention.risk_codes.is_empty() {
        return attention
            .risk_codes
            .iter()
            .map(|code| empty_message(code.as_str()))
            .collect();
    }
    attention
        .risk_notes
        .iter()
        .filter_map(|note| {
            let code = match note.as_str() {
                "High-impact operation detected"
                | "\u{26a0} \u{5df2}\u{8bc6}\u{522b}\u{5230}\u{9ad8}\u{5f71}\u{54cd}\u{64cd}\u{4f5c}"
                | "\u{5df2}\u{8bc6}\u{522b}\u{5230}\u{9ad8}\u{5f71}\u{54cd}\u{64cd}\u{4f5c}" => {
                    "attention.risk.high_impact"
                }
                "The operation cannot be undone after it is submitted"
                | "\u{63d0}\u{4ea4}\u{540e}\u{52a8}\u{4f5c}\u{672c}\u{8eab}\u{4e0d}\u{53ef}\u{64a4}\u{9500}" => {
                    "attention.risk.irreversible"
                }
                "The command contains compound syntax"
                | "\u{547d}\u{4ee4}\u{5305}\u{542b}\u{7ec4}\u{5408}\u{8bed}\u{6cd5}" => {
                    "attention.risk.compound_syntax"
                }
                "Read-only intent; this rule is not a security guarantee"
                | "\u{53ea}\u{8bfb}\u{610f}\u{56fe}\u{ff08}\u{89c4}\u{5219}\u{63d0}\u{793a}\u{ff0c}\u{975e}\u{5b89}\u{5168}\u{4fdd}\u{8bc1}\u{ff09}"
                | "\u{53ea}\u{8bfb}\u{610f}\u{56fe}\u{ff1b}\u{89c4}\u{5219}\u{63d0}\u{793a}\u{4e0d}\u{6784}\u{6210}\u{5b89}\u{5168}\u{4fdd}\u{8bc1}" => {
                    "attention.risk.read_only_intent"
                }
                "The approval decision can be undone for 3 seconds"
                | "\u{21a9} 3 \u{79d2}\u{5185}\u{53ef}\u{64a4}\u{56de}\u{6279}\u{51c6}\u{51b3}\u{5b9a}"
                | "\u{6279}\u{51c6}\u{51b3}\u{5b9a}\u{53ef}\u{5728} 3 \u{79d2}\u{5185}\u{64a4}\u{56de}" => {
                    "attention.risk.undo_window"
                }
                "May run project code or produce side effects"
                | "\u{53ef}\u{80fd}\u{6267}\u{884c}\u{9879}\u{76ee}\u{4ee3}\u{7801}\u{6216}\u{4ea7}\u{751f}\u{526f}\u{4f5c}\u{7528}" => {
                    "attention.risk.side_effects"
                }
                "The impact of this operation is unknown"
                | "\u{6211}\u{4e0d}\u{8ba4}\u{8bc6}\u{8fd9}\u{4e2a}\u{64cd}\u{4f5c}\u{7684}\u{5f71}\u{54cd}"
                | "\u{6b64}\u{64cd}\u{4f5c}\u{7684}\u{5f71}\u{54cd}\u{672a}\u{77e5}" => {
                    "attention.risk.unknown_impact"
                }
                "Review the original window" | "Verify the original command"
                | "\u{5efa}\u{8bae}\u{67e5}\u{770b}\u{539f}\u{7a97}\u{53e3}"
                | "\u{5efa}\u{8bae}\u{6838}\u{5bf9}\u{539f}\u{547d}\u{4ee4}" => {
                    "attention.risk.review_original"
                }
                _ => return None,
            };
            Some(empty_message(code))
        })
        .collect()
}

pub(crate) fn jump(capability: &str) -> Value {
    let code = match capability {
        "exact_conversation" => "jump.exact_conversation",
        "terminal" => "jump.terminal",
        "app_only" => "jump.app_only",
        _ => "jump.unsupported",
    };
    empty_message(code)
}

pub(crate) fn quota_window(entry: &QuotaEntry) -> Option<Value> {
    if entry.window == "extra_usage" {
        return Some(empty_message("quota.window.extra_usage"));
    }
    if entry.provider == "claude" && entry.window == "7d" {
        return Some(empty_message("quota.window.claude_weekly"));
    }
    if entry.provider == "claude"
        && (entry.window.starts_with("scoped_") || entry.window.starts_with("7d_"))
    {
        if let Some(name) = entry.limit_name.as_deref() {
            let name = name.split('·').next().unwrap_or(name).trim();
            let mut args = string_arg("name", name);
            if let Some(minutes) = entry
                .window_minutes
                .filter(|minutes| *minutes > 0 && minutes.is_multiple_of(10_080))
            {
                args.insert(
                    "count".into(),
                    Value::String((minutes / 10_080).to_string()),
                );
                return Some(message("quota.window.scoped_weeks", args));
            }
            return Some(message("quota.window.scoped", args));
        }
    }
    if entry.window == "week" {
        return Some(empty_message("quota.window.current_week"));
    }
    let minutes = entry.window_minutes?;
    let (code, count) = if (40_320..=44_640).contains(&minutes) {
        ("quota.window.months", 1)
    } else if minutes.is_multiple_of(43_200) {
        ("quota.window.months", minutes / 43_200)
    } else if minutes.is_multiple_of(10_080) {
        ("quota.window.weeks", minutes / 10_080)
    } else if minutes.is_multiple_of(1_440) {
        ("quota.window.days", minutes / 1_440)
    } else if minutes.is_multiple_of(60) {
        ("quota.window.hours", minutes / 60)
    } else {
        ("quota.window.minutes", minutes)
    };
    Some(message(code, string_arg("count", count.to_string())))
}

pub(crate) fn quota_reason(entry: &QuotaEntry) -> Option<Value> {
    let code = entry.reason_code.as_deref()?;
    let mut args = Map::new();
    for (key, value) in &entry.reason_args {
        args.insert(key.clone(), Value::String(value.clone()));
    }
    Some(json!({
        "code": code,
        "args": args,
    }))
}

fn provider_name(provider: &str) -> String {
    match provider {
        "claude" => "Claude".to_owned(),
        "codex" => "Codex".to_owned(),
        "gemini" => "Gemini".to_owned(),
        "kimi" => "Kimi Code".to_owned(),
        "grok" => "Grok Build".to_owned(),
        other => other.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actrealm_core::{AttentionRiskCode, OperationCategory};

    #[test]
    fn claude_global_and_scoped_weekly_windows_have_distinct_labels() {
        let global: QuotaEntry = serde_json::from_value(json!({"provider":"claude","window":"7d","windowMinutes":10080,"status":"available","source":"oauth_usage"})).unwrap();
        let scoped: QuotaEntry = serde_json::from_value(json!({"provider":"claude","window":"scoped_fable","windowMinutes":10080,"limitName":"Fable","status":"available","source":"oauth_usage"})).unwrap();
        assert_eq!(
            quota_window(&global).unwrap()["code"],
            "quota.window.claude_weekly"
        );
        let label = quota_window(&scoped).unwrap();
        assert_eq!(label["code"], "quota.window.scoped_weeks");
        assert_eq!(label["args"]["name"], "Fable");
        assert_eq!(label["args"]["count"], "1");
    }

    #[test]
    fn legacy_risk_copy_projects_to_stable_codes() {
        let attention = AttentionRecord {
            id: "legacy".to_owned(),
            session_id: "session".to_owned(),
            provider: "codex".to_owned(),
            project: None,
            request_id: None,
            kind: "approval".to_owned(),
            title: "Allow Bash?".to_owned(),
            detail: None,
            state: "open".to_owned(),
            risk: "unknown".to_owned(),
            risk_notes: vec![
                "我不认识这个操作的影响".to_owned(),
                "建议查看原窗口".to_owned(),
            ],
            primary_category: None,
            risk_codes: Vec::new(),
            command_preview: None,
            expires_at: None,
            auto_hide_at: None,
            reminder_acknowledged_at: None,
            reminder_resolution: None,
            retain_after_ack: false,
            created_at: 1,
            resolution: None,
            remote_actionable: false,
        };

        let codes = attention_risks(&attention)
            .into_iter()
            .filter_map(|message| message["code"].as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        assert_eq!(
            codes,
            [
                "attention.risk.unknown_impact",
                "attention.risk.review_original"
            ]
        );
    }

    #[test]
    fn structured_risk_codes_are_used_without_legacy_text_inference() {
        let attention = AttentionRecord {
            id: "structured".to_owned(),
            session_id: "session".to_owned(),
            provider: "codex".to_owned(),
            project: None,
            request_id: None,
            kind: "approval".to_owned(),
            title: "Allow Bash?".to_owned(),
            detail: None,
            state: "open".to_owned(),
            risk: "high".to_owned(),
            risk_notes: vec!["unrecognized legacy copy".to_owned()],
            primary_category: Some(OperationCategory::ShellRemove),
            risk_codes: vec![
                AttentionRiskCode::HighImpact,
                AttentionRiskCode::Irreversible,
            ],
            command_preview: None,
            expires_at: None,
            auto_hide_at: None,
            reminder_acknowledged_at: None,
            reminder_resolution: None,
            retain_after_ack: false,
            created_at: 1,
            resolution: None,
            remote_actionable: false,
        };

        let codes = attention_risks(&attention)
            .into_iter()
            .filter_map(|message| message["code"].as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        assert_eq!(
            codes,
            ["attention.risk.high_impact", "attention.risk.irreversible"]
        );
    }
}

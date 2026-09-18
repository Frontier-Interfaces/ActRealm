//! Grok Build's official numeric session summary. It already carries resume-
//! stable totals and per-turn deltas, so it supersedes process-local ACP caches.
use super::*;
use std::collections::BTreeMap;

pub(super) fn records(root: &Path) -> Vec<UsageRecord> {
    let mut result = Vec::new();
    let start = Instant::now();
    let mut budget = 16 * 1024 * 1024u64;
    let Ok(workspaces) = fs::read_dir(root) else {
        return result;
    };
    for workspace in workspaces.take(512).flatten() {
        if !workspace.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Ok(sessions) = fs::read_dir(workspace.path()) else {
            continue;
        };
        for session in sessions.take(512).flatten() {
            if result.len() >= 512 || start.elapsed() > Duration::from_millis(250) || budget == 0 {
                return result;
            }
            if !session.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let path = session.path().join("usage.json");
            let Ok(file) = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&path)
            else {
                continue;
            };
            let Ok(meta) = file.metadata() else { continue };
            if !meta.is_file()
                || meta.uid() != unsafe { libc::getuid() }
                || meta.len() > 8 * 1024 * 1024
                || meta.len() > budget
            {
                continue;
            }
            let mut bytes = Vec::new();
            if file
                .take(8 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .is_err()
            {
                continue;
            }
            budget = budget.saturating_sub(bytes.len() as u64);
            let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                continue;
            };
            let Some(mut record) = parse(&value) else {
                continue;
            };
            if session.file_name().to_str() != Some(record.provider_session_id.as_str()) {
                continue;
            }
            if let Some(project) = workspace
                .file_name()
                .to_str()
                .and_then(decode_workspace)
                .and_then(|path| project_identity_from_workspace(Path::new(&path)))
            {
                record.project_id = Some(project.id);
                record.project_label = Some(project.label);
            }
            result.push(record);
        }
    }
    result
}

pub fn parse(value: &Value) -> Option<UsageRecord> {
    let id = value["sessionId"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128)?;
    let at = value["updatedAt"].as_str().and_then(timestamp_millis)?;
    let session = &value["session"];
    let (mut total, _) = row(
        session,
        session["primaryModelId"].as_str().map(ToOwned::to_owned),
    )?;
    let turns = value["turns"].as_array().filter(|v| v.len() <= 10000)?;
    let mut days = BTreeMap::<(String, Option<String>), UsageDailyRecord>::new();
    let mut last_turn = None;
    let mut last_at = 0;
    for turn in turns {
        let turn_at = turn["endedAt"].as_str().and_then(timestamp_millis)?;
        let day = local_day(turn_at)?;
        if turn_at >= last_at {
            last_at = turn_at;
            last_turn = turn["totalTokens"].as_u64();
        }
        let rows = if let Some(models) = turn["modelUsage"].as_object().filter(|m| !m.is_empty()) {
            models
                .iter()
                .map(|(model, data)| row(data, Some(model.clone())))
                .collect::<Option<Vec<_>>>()?
        } else {
            vec![row(
                turn,
                turn["primaryModelId"].as_str().map(ToOwned::to_owned),
            )?]
        };
        for (usage, calls) in rows {
            let key = (day.clone(), usage.model.clone());
            let record = days.entry(key).or_insert_with(|| UsageDailyRecord {
                day: day.clone(),
                model: usage.model.clone(),
                estimated_cost_usd_micros: Some(0),
                ..Default::default()
            });
            record.input_tokens = record
                .input_tokens
                .saturating_add(usage.input_tokens.unwrap_or(0));
            record.output_tokens = record
                .output_tokens
                .saturating_add(usage.output_tokens.unwrap_or(0));
            record.cache_read_tokens = record
                .cache_read_tokens
                .saturating_add(usage.cache_read_tokens.unwrap_or(0));
            record.cache_creation_tokens = record
                .cache_creation_tokens
                .saturating_add(usage.cache_creation_tokens.unwrap_or(0));
            record.reasoning_tokens = record
                .reasoning_tokens
                .saturating_add(usage.reasoning_tokens.unwrap_or(0));
            record.token_total = record
                .token_total
                .saturating_add(usage.token_total.unwrap_or(0));
            record.estimated_cost_usd_micros = record
                .estimated_cost_usd_micros
                .zip(usage.estimated_cost_usd_micros)
                .map(|(a, b)| a.saturating_add(b));
            if record.estimated_cost_usd_micros.is_some() {
                record.cost_kind = Some("provider_reported".to_owned());
                record.pricing_source = Some("provider".to_owned());
            }
            record.message_count = record.message_count.saturating_add(calls);
        }
    }
    total.provider = "grok".to_owned();
    total.provider_session_id = id.to_owned();
    total.captured_at = at;
    total.last_turn_tokens = last_turn;
    total.usage_source = "grok_session_usage".to_owned();
    total.daily_usage = days.into_values().collect();
    let day_total = total
        .daily_usage
        .iter()
        .fold(0u64, |a, r| a.saturating_add(r.token_total));
    total.usage_quality = if session["usageIsIncomplete"].as_bool() != Some(true)
        && Some(day_total) == total.token_total
    {
        "official_local"
    } else {
        "partial"
    }
    .to_owned();
    Some(total)
}

fn row(value: &Value, model: Option<String>) -> Option<(UsageRecord, u64)> {
    let full = value["inputTokens"].as_u64()?;
    let output = value["outputTokens"].as_u64()?;
    let cached = value["cachedReadTokens"].as_u64().unwrap_or(0);
    let creation = value["cacheCreationTokens"].as_u64().unwrap_or(0);
    let input = full.checked_sub(cached)?.checked_sub(creation)?;
    let total = value["totalTokens"].as_u64()?;
    if full.checked_add(output)? != total
        || model
            .as_ref()
            .is_some_and(|m| m.len() > 128 || m.chars().any(char::is_control))
    {
        return None;
    }
    let cost = if value["costIsPartial"].as_bool() == Some(true)
        || value["usageIsIncomplete"].as_bool() == Some(true)
    {
        None
    } else {
        value["costUsdTicks"].as_u64().map(|n| n / 10000)
    };
    Some((
        UsageRecord {
            model,
            input_tokens: Some(input),
            output_tokens: Some(output),
            cache_read_tokens: Some(cached),
            cache_creation_tokens: Some(creation),
            reasoning_tokens: value["reasoningTokens"].as_u64(),
            token_total: Some(total),
            estimated_cost_usd_micros: cost,
            cost_kind: cost.map(|_| "provider_reported".to_owned()),
            pricing_source: cost.map(|_| "provider".to_owned()),
            ..Default::default()
        },
        value["modelCalls"].as_u64().unwrap_or(0),
    ))
}

fn decode_workspace(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = std::str::from_utf8(bytes.get(index + 1..index + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    let text = String::from_utf8(out).ok()?;
    (text.starts_with('/') && text.len() <= 4096 && !text.chars().any(char::is_control))
        .then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_resume_total_is_not_added_to_its_turn_deltas() {
        let usage = serde_json::json!({"inputTokens":80,"outputTokens":20,"cachedReadTokens":30,"cacheCreationTokens":0,"totalTokens":100,"modelCalls":1,"primaryModelId":"grok-test"});
        let mut total = usage.clone();
        total["inputTokens"] = serde_json::json!(160);
        total["outputTokens"] = serde_json::json!(40);
        total["cachedReadTokens"] = serde_json::json!(60);
        total["totalTokens"] = serde_json::json!(200);
        let mut one = usage.clone();
        one["endedAt"] = serde_json::json!("2026-09-16T12:00:00Z");
        let mut two = usage;
        two["endedAt"] = serde_json::json!("2026-09-17T12:00:00Z");
        let record=parse(&serde_json::json!({"sessionId":"session-1","updatedAt":"2026-09-17T12:00:00Z","session":total,"turns":[one,two]})).unwrap();
        assert_eq!(record.token_total, Some(200));
        assert_eq!(record.input_tokens, Some(100));
        assert_eq!(record.last_turn_tokens, Some(100));
        assert_eq!(record.daily_usage.len(), 2);
        assert_eq!(record.estimated_cost_usd_micros, None);
        assert_eq!(
            record
                .daily_usage
                .iter()
                .map(|r| r.token_total)
                .sum::<u64>(),
            200
        );
    }
}

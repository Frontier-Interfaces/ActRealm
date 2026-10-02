//! Official Kimi account quota. Credentials never enter the cache or errors.
use super::*;
use std::os::unix::fs::MetadataExt;

pub fn refresh_kimi(user_home: &Path, runtime_home: &Path, now: u64) -> Result<(), QuotaError> {
    let result = fetch_kimi(user_home, now);
    let entries = match result {
        Ok(entries) => entries,
        Err(error) => {
            let old = read_cache(runtime_home, "kimi", now);
            if old.is_empty() {
                let _ = error;
                return atomic_write(
                    &runtime_home.join("cache/kimi-quota.json"),
                    &serde_json::to_vec(&vec![unavailable("kimi")])?,
                    0o600,
                );
            }
            old.into_iter()
                .map(|entry| {
                    entry.mark_stale(
                        "quota.reason.agent_refresh_failed",
                        "Agent quota refresh failed; showing the last captured value.",
                    )
                })
                .collect()
        }
    };
    atomic_write(
        &runtime_home.join("cache/kimi-quota.json"),
        &serde_json::to_vec(&entries)?,
        0o600,
    )
}

pub fn read_agent_cache(runtime_home: &Path, now: u64) -> Vec<QuotaEntry> {
    ["kimi", "grok"]
        .into_iter()
        .flat_map(|provider| read_cache(runtime_home, provider, now))
        .collect()
}
fn read_cache(runtime_home: &Path, provider: &str, now: u64) -> Vec<QuotaEntry> {
    let path = runtime_home.join(format!("cache/{provider}-quota.json"));
    let Ok(bytes) = read_private(&path, 64 * 1024) else {
        return Vec::new();
    };
    let Ok(entries) = serde_json::from_slice::<Vec<QuotaEntry>>(&bytes) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .take(4)
        .filter(|q| {
            q.provider == provider
                && (q.status == "unavailable"
                    || q.used_pct
                        .is_some_and(|n| n.is_finite() && (0.0..=100.0).contains(&n)))
        })
        .map(|q| {
            if q.captured_at.is_none_or(|at| {
                now.saturating_sub(at) > 5 * 60 * 1000 || at > now.saturating_add(60_000)
            }) {
                q.mark_stale(
                    "quota.reason.agent_refresh_failed",
                    "Agent quota refresh failed; showing the last captured value.",
                )
            } else {
                q
            }
        })
        .collect()
}

fn unavailable(provider: &str) -> QuotaEntry {
    QuotaEntry::unavailable(
        provider,
        "account",
        "agent_account",
        "quota.reason.agent_unavailable",
        "The Agent did not return account quota information.",
    )
}

pub fn capture_grok_billing(home: &Path, value: &Value, now: u64) -> Result<(), QuotaError> {
    let config = &value["config"];
    let number = |v: &Value| {
        v.as_f64()
            .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
            .filter(|n| n.is_finite())
    };
    let percentage = number(&config["creditUsagePercent"]).or_else(|| {
        let limit = number(&config["monthlyLimit"]["val"])?;
        let used = number(&config["used"]["val"])?;
        (limit > 0.0 && used >= 0.0).then_some(used / limit * 100.0)
    });
    let entry = if let Some(percent) = percentage.filter(|n| *n >= 0.0) {
        let period = &config["currentPeriod"];
        let (window, minutes) = match period["type"].as_str() {
            Some("USAGE_PERIOD_TYPE_WEEKLY") => ("week", Some(10080)),
            Some("USAGE_PERIOD_TYPE_MONTHLY") => ("month", None),
            _ => ("credits", None),
        };
        let reset = period["end"]
            .as_str()
            .or_else(|| config["billingPeriodEnd"].as_str())
            .and_then(parse_rfc3339_epoch);
        QuotaEntry::available_optional("grok", window, percent, reset, "grok_acp_billing", now)
            .with_metadata(
                minutes,
                Some("grok_build_credits".to_owned()),
                Some("Grok Build credits".to_owned()),
                None,
            )
    } else {
        unavailable("grok")
    };
    atomic_write(
        &home.join("cache/grok-quota.json"),
        &serde_json::to_vec(&vec![entry])?,
        0o600,
    )
}

fn fetch_kimi(user_home: &Path, now: u64) -> Result<Vec<QuotaEntry>, QuotaError> {
    let root = user_home.join(".kimi-code");
    let config = read_private(&root.join("config.toml"), 1024 * 1024)?;
    let config = std::str::from_utf8(&config)
        .ok()
        .and_then(|text| text.parse::<toml_edit::DocumentMut>().ok())
        .ok_or(QuotaError::AgentRequest)?;
    let configured = config
        .get("providers")
        .and_then(|p| p.get("managed:kimi-code"))
        .and_then(|p| p.get("base_url"))
        .and_then(|p| p.as_str())
        .ok_or(QuotaError::AgentRequest)?;
    let url = match configured.trim_end_matches('/') {
        "https://api.kimi.com/coding/v1" => "https://api.kimi.com/coding/v1/usages",
        "https://api.kimi.ai/coding/v1" => "https://api.kimi.ai/coding/v1/usages",
        _ => return Err(QuotaError::AgentRequest),
    };
    let credential: Value = serde_json::from_slice(&read_private(
        &root.join("credentials/kimi-code.json"),
        32 * 1024,
    )?)?;
    let token = credential["access_token"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 16 * 1024 && !v.chars().any(char::is_control))
        .ok_or(QuotaError::AgentRequest)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| QuotaError::AgentRequest)?;
    let response = client
        .get(url)
        .bearer_auth(token)
        .header("Accept", "application/json")
        .send()
        .map_err(|_| QuotaError::AgentRequest)?;
    if !response.status().is_success() {
        return Err(QuotaError::AgentRequest);
    }
    let mut bytes = Vec::new();
    response
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| QuotaError::AgentRequest)?;
    if bytes.len() > 64 * 1024 {
        return Err(QuotaError::TooLarge(64 * 1024));
    }
    parse_kimi(&serde_json::from_slice(&bytes)?, now)
}

pub fn parse_kimi(value: &Value, now: u64) -> Result<Vec<QuotaEntry>, QuotaError> {
    let mut entries = Vec::new();
    for (key, window, minutes, name) in [
        ("limit_5h", "5h", Some(300), "Kimi 5h"),
        ("limit_7d", "week", Some(10080), "Kimi 7d"),
        ("limit_month_total", "month", None, "Kimi monthly total"),
        (
            "limit_month_code",
            "month_code",
            None,
            "Kimi monthly coding",
        ),
    ] {
        let data = &value["usages"][key];
        let Some(ratio) = data["used_ratio"]
            .as_f64()
            .filter(|n| n.is_finite() && (0.0..=1.0).contains(n))
        else {
            continue;
        };
        let reset = data["reset_time"].as_str().and_then(parse_rfc3339_epoch);
        entries.push(
            QuotaEntry::available_optional(
                "kimi",
                window,
                ratio * 100.0,
                reset,
                "kimi_oauth_usage",
                now,
            )
            .with_metadata(minutes, Some(key.to_owned()), Some(name.to_owned()), None),
        );
    }
    if entries.is_empty() {
        return Err(QuotaError::AgentRequest);
    }
    Ok(entries)
}

fn read_private(path: &Path, limit: u64) -> Result<Vec<u8>, QuotaError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| QuotaError::AgentRequest)?;
    let metadata = file.metadata().map_err(|_| QuotaError::AgentRequest)?;
    if !metadata.is_file() || metadata.uid() != unsafe { libc::getuid() } || metadata.len() > limit
    {
        return Err(QuotaError::AgentRequest);
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| QuotaError::AgentRequest)?;
    if bytes.len() as u64 > limit {
        return Err(QuotaError::TooLarge(limit));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_quota_windows_keep_missing_limits_missing_and_monthly_exhaustion_visible() {
        let entries=parse_kimi(&serde_json::json!({"usages":{"limit_5h":{"used_ratio":0.2},"limit_month_code":{"used_ratio":1.0,"reset_time":"2026-10-01T00:00:00Z"},"limit_7d":{"used_ratio":9.0}}}),1000).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].remaining_pct, Some(80.0));
        assert_eq!(entries[1].remaining_pct, Some(0.0));
        assert_eq!(entries[1].window, "month_code");
        assert_eq!(entries[0].resets_at, None);
    }
}

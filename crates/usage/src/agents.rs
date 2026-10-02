//! Numeric-only Agent usage snapshots. A process instance is an independent
//! cumulative counter; replay replaces that counter rather than adding it again.
use super::*;
use std::collections::BTreeMap;
use std::os::fd::AsRawFd;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsageSample {
    pub model: Option<String>,
    pub parent_provider_session_id: Option<String>,
    pub last_turn_tokens: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub token_total: Option<u64>,
    pub model_calls: Option<u64>,
    pub cost_usd_micros: Option<u64>,
    pub context_tokens: Option<u64>,
    pub context_limit: Option<u64>,
    pub incomplete: bool,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Ledger {
    schema_version: u32,
    provider: String,
    session_id: String,
    parent_provider_session_id: Option<String>,
    project_id: Option<String>,
    project_label: Option<String>,
    sources: BTreeMap<String, AgentUsageSample>,
    days: BTreeMap<String, UsageDailyRecord>,
    latest: AgentUsageSample,
    captured_at: u64,
}

#[derive(Default)]
pub(super) struct KimiScanner {
    files: BTreeMap<PathBuf, KimiFile>,
    discovered_at: Option<Instant>,
    next: usize,
}
#[derive(Default)]
struct KimiFile {
    offset: u64,
    discarding: bool,
    identity: Option<(u64, u64)>,
    session: String,
    parent: Option<String>,
    current_turn: Option<String>,
    turn_tokens: u64,
    cwd: Option<PathBuf>,
    totals: BTreeMap<(String, String), (AgentUsageSample, u64)>,
}

impl KimiScanner {
    pub(super) fn poll(&mut self, root: &Path, home: &Path) {
        if self
            .discovered_at
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(5))
        {
            let mut found = 0;
            for workspace in child_directories(root, 512) {
                for session in child_directories(&workspace, 512) {
                    let Some(id) = session
                        .file_name()
                        .and_then(|v| v.to_str())
                        .filter(|v| v.len() <= 256)
                    else {
                        continue;
                    };
                    let cwd = read_kimi_cwd(&session.join("state.json"));
                    for agent in child_directories(&session.join("agents"), 128) {
                        let path = agent.join("wire.jsonl");
                        if path.is_file() {
                            self.files.entry(path).or_insert_with(|| KimiFile {
                                session: if agent.file_name().and_then(|v| v.to_str())
                                    == Some("main")
                                {
                                    id.to_owned()
                                } else {
                                    format!(
                                        "kimi-child-{}",
                                        hash(&format!(
                                            "{id}:{}",
                                            agent.file_name().unwrap_or_default().to_string_lossy()
                                        ))
                                    )
                                },
                                parent: (agent.file_name().and_then(|v| v.to_str())
                                    != Some("main"))
                                .then(|| id.to_owned()),
                                cwd: cwd.clone(),
                                ..Default::default()
                            });
                            found += 1;
                        }
                        if found >= 1024 {
                            break;
                        }
                    }
                    if found >= 1024 {
                        break;
                    }
                }
                if found >= 1024 {
                    break;
                }
            }
            self.discovered_at = Some(Instant::now());
        }
        let keys = self.files.keys().cloned().collect::<Vec<_>>();
        if keys.is_empty() {
            return;
        }
        let mut budget: usize = 4 * 1024 * 1024;
        for index in 0..keys.len() {
            if budget == 0 {
                break;
            }
            let path = &keys[(self.next + index) % keys.len()];
            let state = self.files.get_mut(path).unwrap();
            let Ok(file) = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(path)
            else {
                continue;
            };
            let Ok(metadata) = file.metadata() else {
                continue;
            };
            if !metadata.is_file() || metadata.uid() != unsafe { libc::getuid() } {
                continue;
            }
            let identity = (metadata.dev(), metadata.ino());
            if state.identity != Some(identity) || metadata.len() < state.offset {
                state.offset = 0;
                state.discarding = false;
                state.totals.clear();
                state.identity = Some(identity);
            }
            if metadata.len() == state.offset {
                continue;
            }
            let mut reader = BufReader::new(file);
            if reader.seek(SeekFrom::Start(state.offset)).is_err() {
                continue;
            }
            let mut changed = HashSet::new();
            while budget > 0 {
                let mut bytes = Vec::new();
                let maximum = (MAX_JSONL_LINE_BYTES + 1).min(budget as u64);
                let Ok(n) = (&mut reader).take(maximum).read_until(b'\n', &mut bytes) else {
                    break;
                };
                if n == 0 {
                    break;
                }
                budget = budget.saturating_sub(n);
                if state.discarding {
                    state.offset = state.offset.saturating_add(n as u64);
                    state.discarding = bytes.last() != Some(&b'\n');
                    continue;
                }
                // Retry partial appends; skip oversized records using the same bounded budget.
                if bytes.last() != Some(&b'\n') {
                    if n as u64 > MAX_JSONL_LINE_BYTES {
                        state.offset = state.offset.saturating_add(n as u64);
                        state.discarding = true;
                    }
                    break;
                }
                state.offset = state.offset.saturating_add(n as u64);
                let Ok(event) = serde_json::from_slice::<Value>(&bytes) else {
                    continue;
                };
                if event["type"] == "turn.prompt" {
                    let turn = event["turnId"]
                        .as_u64()
                        .map(|v| v.to_string())
                        .or_else(|| event["turnId"].as_str().map(ToOwned::to_owned));
                    if turn != state.current_turn {
                        state.current_turn = turn;
                        state.turn_tokens = 0;
                    }
                }
                let Some((mut sample, at)) = kimi_usage_event(&event) else {
                    continue;
                };
                sample.parent_provider_session_id = state.parent.clone();
                if event["usageScope"] == "turn" && state.current_turn.is_some() {
                    state.turn_tokens = state
                        .turn_tokens
                        .saturating_add(sample.token_total.unwrap_or(0));
                    sample.last_turn_tokens = Some(state.turn_tokens);
                }
                let Some(day) = local_day(at) else { continue };
                let key = (sample.model.clone().unwrap_or_default(), day);
                if state.totals.len() >= 256 && !state.totals.contains_key(&key) {
                    continue;
                }
                let (total, last) = state.totals.entry(key.clone()).or_insert_with(|| {
                    (
                        AgentUsageSample {
                            model: sample.model.clone(),
                            incomplete: true,
                            ..Default::default()
                        },
                        at,
                    )
                });
                macro_rules! add {
                    ($field:ident) => {
                        if let Some(value) = sample.$field {
                            total.$field = Some(total.$field.unwrap_or(0).saturating_add(value));
                        }
                    };
                }
                add!(input_tokens);
                add!(output_tokens);
                add!(cache_read_tokens);
                add!(cache_creation_tokens);
                add!(token_total);
                total.model_calls = Some(total.model_calls.unwrap_or(0).saturating_add(1));
                total.parent_provider_session_id = sample.parent_provider_session_id.clone();
                total.last_turn_tokens = sample.last_turn_tokens;
                *last = (*last).max(at);
                changed.insert(key);
            }
            for key in changed {
                let (sample, at) = &state.totals[&key];
                // Stable file/day identity makes a rescan/restart idempotent.
                let source = format!(
                    "kimi-wire:{}:{}",
                    path.parent()
                        .and_then(|p| p.file_name())
                        .and_then(|p| p.to_str())
                        .unwrap_or("main"),
                    key.1
                );
                let _ = capture_agent_usage(
                    home,
                    "kimi",
                    &state.session,
                    &source,
                    state.cwd.as_deref(),
                    sample.clone(),
                    *at,
                );
            }
        }
        self.next = (self.next + 1) % keys.len();
    }
}

fn child_directories(root: &Path, limit: usize) -> Vec<PathBuf> {
    fs::read_dir(root)
        .into_iter()
        .flatten()
        .take(limit)
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect()
}
fn read_kimi_cwd(path: &Path) -> Option<PathBuf> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .ok()?;
    if file.metadata().ok()?.len() > 1024 * 1024 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1).read_to_end(&mut bytes).ok()?;
    let state: Value = serde_json::from_slice(&bytes).ok()?;
    state["cwd"]
        .as_str()
        .filter(|s| s.len() <= 4096)
        .map(PathBuf::from)
}
pub fn kimi_usage_event(event: &Value) -> Option<(AgentUsageSample, u64)> {
    if event["type"] != "usage.record" {
        return None;
    }
    let usage = &event["usage"];
    let input = usage["inputOther"].as_u64()?;
    let output = usage["output"].as_u64()?;
    let read = usage["inputCacheRead"].as_u64()?;
    let create = usage["inputCacheCreation"].as_u64()?;
    let model = event["model"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 256)?
        .to_owned();
    let at = event["time"]
        .as_u64()
        .or_else(|| event["time"].as_str().and_then(timestamp_millis))?;
    Some((
        AgentUsageSample {
            model: Some(model),
            input_tokens: Some(input),
            output_tokens: Some(output),
            cache_read_tokens: Some(read),
            cache_creation_tokens: Some(create),
            token_total: Some(
                input
                    .saturating_add(output)
                    .saturating_add(read)
                    .saturating_add(create),
            ),
            incomplete: true,
            ..Default::default()
        },
        at,
    ))
}

pub fn capture_agent_usage(
    home: &Path,
    provider: &str,
    session: &str,
    source: &str,
    cwd: Option<&Path>,
    sample: AgentUsageSample,
    at: u64,
) -> Result<(), UsageError> {
    if !matches!(provider, "kimi" | "grok")
        || session.is_empty()
        || session.len() > 256
        || source.len() > 512
        || sample
            .model
            .as_ref()
            .is_some_and(|v| v.len() > 256 || v.chars().any(char::is_control))
    {
        return Err(UsageError::Pricing(
            "Invalid Agent usage identity".to_owned(),
        ));
    }
    let directory = home.join("cache/agent-usage");
    for path in directory.ancestors() {
        if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(UsageError::Pricing(
                "Unsafe Agent usage directory".to_owned(),
            ));
        }
    }
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&directory)
        .map_err(|source| UsageError::Io {
            path: directory.clone(),
            source,
        })?;
    let key = hash(&format!("{provider}\0{session}"));
    let path = directory.join(format!("{key}.json"));
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(directory.join(format!("{key}.lock")))
        .map_err(|source| UsageError::Io {
            path: path.clone(),
            source,
        })?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(UsageError::Io {
            path,
            source: io::Error::last_os_error(),
        });
    }
    let mut ledger = if path.exists() {
        read(&path)?
    } else {
        Ledger {
            schema_version: 1,
            provider: provider.to_owned(),
            session_id: session.to_owned(),
            ..Default::default()
        }
    };
    if ledger.schema_version != 1 || ledger.provider != provider || ledger.session_id != session {
        return Err(UsageError::Pricing(
            "Incompatible Agent usage cache".to_owned(),
        ));
    }
    if let Some(parent) = &sample.parent_provider_session_id {
        if parent.len() > 128
            || parent.is_empty()
            || !parent
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        {
            return Err(UsageError::Pricing(
                "Invalid parent usage identity".to_owned(),
            ));
        }
        ledger.parent_provider_session_id = Some(parent.clone());
    }
    if let Some(project) = cwd.and_then(project_identity_from_workspace) {
        ledger.project_id = Some(project.id);
        ledger.project_label = Some(project.label);
    }
    let source = hash(&format!(
        "{source}\0{}",
        sample.model.as_deref().unwrap_or("unknown")
    ));
    let previous = ledger.sources.get(&source).cloned().unwrap_or_default();
    if let (Some(old), Some(new)) = (previous.token_total, sample.token_total) {
        if new < old {
            return Err(UsageError::Pricing(
                "Agent cumulative usage regressed; start a new counter identity".to_owned(),
            ));
        }
    }
    if let Some(day) = local_day(at) {
        if sample.token_total.is_some() {
            let model = sample.model.clone();
            let key = format!("{day}\0{}", model.as_deref().unwrap_or("unknown"));
            let row = ledger.days.entry(key).or_insert_with(|| UsageDailyRecord {
                day,
                model,
                ..Default::default()
            });
            macro_rules! delta {
                ($field:ident) => {
                    row.$field = row.$field.saturating_add(
                        sample
                            .$field
                            .unwrap_or(0)
                            .saturating_sub(previous.$field.unwrap_or(0)),
                    );
                };
            }
            delta!(input_tokens);
            delta!(output_tokens);
            delta!(cache_read_tokens);
            delta!(cache_creation_tokens);
            delta!(reasoning_tokens);
            delta!(token_total);
            if let Some(cost) = sample.cost_usd_micros {
                row.estimated_cost_usd_micros =
                    Some(row.estimated_cost_usd_micros.unwrap_or(0).saturating_add(
                        cost.saturating_sub(previous.cost_usd_micros.unwrap_or(0)),
                    ));
                row.cost_kind = Some("provider_reported".to_owned());
                row.pricing_source = Some("provider".to_owned());
            }
            if let Some(calls) = sample.model_calls {
                row.message_count = row
                    .message_count
                    .saturating_add(calls.saturating_sub(previous.model_calls.unwrap_or(0)));
            }
        }
    }
    if ledger.sources.len() >= 4096 && !ledger.sources.contains_key(&source) {
        return Err(UsageError::TooLarge(4096));
    }
    ledger.sources.insert(source, sample.clone());
    if at >= ledger.captured_at {
        let mut latest = sample;
        if latest.model.is_none() {
            latest.model = ledger.latest.model.clone();
        }
        if latest.model == ledger.latest.model {
            latest.context_tokens = latest.context_tokens.or(ledger.latest.context_tokens);
            latest.context_limit = latest.context_limit.or(ledger.latest.context_limit);
            latest.last_turn_tokens = latest.last_turn_tokens.or(ledger.latest.last_turn_tokens);
        }
        ledger.latest = latest;
        ledger.captured_at = at;
    } else if ledger.latest.model.is_none() {
        ledger.latest.model = sample.model;
    }
    atomic_write(&path, &serde_json::to_vec(&ledger)?, 0o600)
}

pub(super) fn records(home: &Path) -> Vec<UsageRecord> {
    let Ok(entries) = fs::read_dir(home.join("cache/agent-usage")) else {
        return Vec::new();
    };
    entries
        .take(8192)
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| read(&e.path()).ok())
        .filter(|l| l.schema_version == 1 && matches!(l.provider.as_str(), "kimi" | "grok"))
        .map(|ledger| {
            let mut record = UsageRecord {
                provider: ledger.provider,
                provider_session_id: ledger.session_id,
                parent_provider_session_id: ledger.parent_provider_session_id,
                last_turn_tokens: ledger.latest.last_turn_tokens,
                project_id: ledger.project_id,
                project_label: ledger.project_label,
                model: ledger.latest.model.clone(),
                context_used_tokens: ledger.latest.context_tokens,
                context_window_tokens: ledger.latest.context_limit,
                context_used_percent: ledger
                    .latest
                    .context_tokens
                    .zip(ledger.latest.context_limit)
                    .filter(|(_, limit)| *limit > 0)
                    .map(|(used, limit)| ((used.saturating_mul(100) / limit).min(100)) as u32),
                usage_source: "agent_connector".to_owned(),
                usage_quality: "partial".to_owned(),
                captured_at: ledger.captured_at,
                daily_usage: ledger.days.into_values().collect(),
                ..Default::default()
            };
            for sample in ledger.sources.values() {
                macro_rules! add {
                    ($field:ident) => {
                        if let Some(value) = sample.$field {
                            record.$field = Some(record.$field.unwrap_or(0).saturating_add(value));
                        }
                    };
                }
                add!(input_tokens);
                add!(output_tokens);
                add!(cache_read_tokens);
                add!(cache_creation_tokens);
                add!(reasoning_tokens);
                add!(token_total);
            }
            let costs = ledger
                .sources
                .values()
                .filter(|s| s.token_total.is_some())
                .map(|s| s.cost_usd_micros)
                .collect::<Option<Vec<_>>>();
            if let Some(costs) = costs.filter(|c| !c.is_empty()) {
                record.estimated_cost_usd_micros =
                    Some(costs.into_iter().fold(0u64, u64::saturating_add));
                record.cost_kind = Some("provider_reported".to_owned());
                record.pricing_source = Some("provider".to_owned());
            }
            record
        })
        .collect()
}

fn hash(value: &str) -> String {
    digest(&SHA256, value.as_bytes())
        .as_ref()
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect()
}
fn read(path: &Path) -> Result<Ledger, UsageError> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|source| UsageError::Io {
            path: path.to_owned(),
            source,
        })?;
    let meta = file.metadata().map_err(|source| UsageError::Io {
        path: path.to_owned(),
        source,
    })?;
    if !meta.is_file() || meta.len() > 4 * 1024 * 1024 || meta.uid() != unsafe { libc::getuid() } {
        return Err(UsageError::TooLarge(4 * 1024 * 1024));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| UsageError::Io {
            path: path.to_owned(),
            source,
        })?;
    Ok(serde_json::from_slice(&bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replay_restart_and_model_switch_preserve_numeric_history() {
        let home = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "actrealm-agent-usage-{}-{}",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let sample = AgentUsageSample {
            model: Some("grok-test".into()),
            input_tokens: Some(80),
            output_tokens: Some(20),
            token_total: Some(100),
            ..Default::default()
        };
        capture_agent_usage(
            &home,
            "grok",
            "s1",
            "process-one",
            None,
            sample.clone(),
            1_800_000_000_000,
        )
        .unwrap();
        capture_agent_usage(
            &home,
            "grok",
            "s1",
            "process-one",
            None,
            sample.clone(),
            1_800_000_000_000,
        )
        .unwrap();
        assert_eq!(records(&home)[0].token_total, Some(100));
        capture_agent_usage(
            &home,
            "grok",
            "s1",
            "process-two",
            None,
            sample.clone(),
            1_800_000_000_001,
        )
        .unwrap();
        assert_eq!(records(&home)[0].token_total, Some(200));
        let switched = AgentUsageSample {
            model: Some("grok-next".into()),
            ..sample
        };
        capture_agent_usage(
            &home,
            "grok",
            "s1",
            "process-two",
            None,
            switched,
            1_800_000_000_002,
        )
        .unwrap();
        let result = records(&home).remove(0);
        assert_eq!(result.token_total, Some(300));
        assert_eq!(result.model.as_deref(), Some("grok-next"));
        assert_eq!(result.daily_usage.len(), 2);
        assert_eq!(
            result
                .daily_usage
                .iter()
                .map(|r| r.token_total)
                .sum::<u64>(),
            300
        );
        assert_eq!(result.estimated_cost_usd_micros, None);
        fs::remove_dir_all(home).unwrap();
    }
}

#[cfg(test)]
mod kimi_scanner_tests {
    use super::*;
    #[test]
    fn kimi_wire_restart_and_partial_append_do_not_duplicate_usage() {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "ar-kimi-scan-{}-{}",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let sessions = root.join("sessions");
        let session = sessions.join("workspace/session_1");
        let agent = session.join("agents/main");
        fs::create_dir_all(&agent).unwrap();
        fs::write(
            session.join("state.json"),
            serde_json::to_vec(&serde_json::json!({"cwd":root})).unwrap(),
        )
        .unwrap();
        let line=serde_json::json!({"type":"usage.record","agentId":"main","model":"kimi-test","usage":{"inputOther":50,"output":20,"inputCacheRead":30,"inputCacheCreation":0},"time":1800000000000u64}).to_string()+"\n";
        let path = agent.join("wire.jsonl");
        fs::write(&path, &line).unwrap();
        let home = root.join("runtime");
        let mut scanner = KimiScanner::default();
        scanner.poll(&sessions, &home);
        assert_eq!(records(&home)[0].token_total, Some(100));
        KimiScanner::default().poll(&sessions, &home);
        assert_eq!(records(&home)[0].token_total, Some(100));
        let half = line.len() / 2;
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&line.as_bytes()[..half]).unwrap();
        scanner.poll(&sessions, &home);
        assert_eq!(records(&home)[0].token_total, Some(100));
        file.write_all(&line.as_bytes()[half..]).unwrap();
        file.flush().unwrap();
        scanner.poll(&sessions, &home);
        let record = records(&home).remove(0);
        assert_eq!(record.token_total, Some(200));
        assert_eq!(record.daily_usage[0].message_count, 2);
        assert!(record.estimated_cost_usd_micros.is_none());
        fs::remove_dir_all(root).unwrap();
    }
}

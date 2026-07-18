//! Local, privacy-bounded session usage collection for Claude Code and Codex.
//!
//! The collector stores only numeric usage metadata. It never persists prompts,
//! tool input/output, transcript text, or provider credentials.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

const STATUS_CACHE_SCHEMA: u32 = 1;
const MAX_STATUSLINE_BYTES: u64 = 256 * 1_024;
const MAX_JSONL_LINE_BYTES: u64 = 10 * 1_024 * 1_024;
const MAX_DISCOVERED_FILES: usize = 512;
const RECENT_FILE_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const DISCOVERY_INTERVAL: Duration = Duration::from_secs(5);
const PRICING_SOURCE: &str = "embedded_official_snapshot_2026-07-18";
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Error)]
pub enum UsageError {
    #[error("usage input exceeds {0} bytes")]
    TooLarge(u64),
    #[error("usage JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("usage I/O failed for {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
}

/// A partial session usage snapshot. Optional fields allow an official live
/// source (for example Claude StatusLine) to complement transcript totals
/// without changing the meaning of those totals.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRecord {
    pub provider: String,
    pub provider_session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_turn_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_used_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_used_percent: Option<u32>,
    /// USD represented as millionths to keep persisted values deterministic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_cost_usd_micros: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pricing_source: Option<String>,
    pub usage_source: String,
    pub usage_quality: String,
    pub captured_at: u64,
}

impl UsageRecord {
    fn merge_from(&mut self, newer: UsageRecord) {
        let replace_existing = newer.captured_at >= self.captured_at;
        macro_rules! replace_some {
            ($field:ident) => {
                if newer.$field.is_some() && (replace_existing || self.$field.is_none()) {
                    self.$field = newer.$field;
                }
            };
        }
        replace_some!(input_tokens);
        replace_some!(output_tokens);
        replace_some!(cache_read_tokens);
        replace_some!(cache_creation_tokens);
        replace_some!(reasoning_tokens);
        replace_some!(token_total);
        replace_some!(last_turn_tokens);
        replace_some!(context_used_tokens);
        replace_some!(context_window_tokens);
        replace_some!(context_used_percent);
        replace_some!(estimated_cost_usd_micros);
        replace_some!(cost_kind);
        replace_some!(pricing_source);
        if newer.captured_at >= self.captured_at {
            self.captured_at = newer.captured_at;
            self.usage_source = newer.usage_source;
            self.usage_quality = newer.usage_quality;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsagePaths {
    pub actrealm_home: PathBuf,
    pub claude_projects: Vec<PathBuf>,
    pub codex_sessions: Vec<PathBuf>,
}

impl UsagePaths {
    pub fn discover() -> Self {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let actrealm_home = env::var_os("ACTREALM_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".actrealm"));
        let mut claude_roots = Vec::new();
        if let Some(value) = env::var_os("CLAUDE_CONFIG_DIR") {
            for value in value.to_string_lossy().split(',') {
                let path = PathBuf::from(value.trim()).join("projects");
                push_unique(&mut claude_roots, path);
            }
        }
        push_unique(&mut claude_roots, home.join(".claude/projects"));
        push_unique(&mut claude_roots, home.join(".config/claude/projects"));

        let codex_home = env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));
        Self {
            actrealm_home,
            claude_projects: claude_roots,
            codex_sessions: vec![
                codex_home.join("sessions"),
                codex_home.join("archived_sessions"),
            ],
        }
    }

    pub fn claude_status_cache_dir(&self) -> PathBuf {
        self.actrealm_home.join("cache/claude-session-usage")
    }
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.contains(&path) {
        paths.push(path);
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatusCacheDocument {
    schema_version: u32,
    record: UsageRecord,
}

/// Captures Claude's official StatusLine session metrics. The returned text is
/// intentionally not used; ActRealm's existing quota status text remains the
/// visible StatusLine output.
pub fn capture_claude_statusline_usage(
    input: &[u8],
    cache_dir: &Path,
    now_ms: u64,
) -> Result<Option<UsageRecord>, UsageError> {
    if input.len() as u64 > MAX_STATUSLINE_BYTES {
        return Err(UsageError::TooLarge(MAX_STATUSLINE_BYTES));
    }
    let payload: Value = serde_json::from_slice(input)?;
    let Some(session_id) = payload
        .get("session_id")
        .and_then(Value::as_str)
        .and_then(safe_session_id)
    else {
        return Ok(None);
    };

    let context = payload.get("context_window");
    let context_window_tokens = context
        .and_then(|value| value.get("context_window_size"))
        .and_then(value_u64);
    let context_used_percent = context
        .and_then(|value| value.get("used_percentage"))
        .and_then(value_f64)
        .map(|value| value.clamp(0.0, 100.0).round() as u32);
    let current = context.and_then(|value| value.get("current_usage"));
    let context_used_tokens = current.and_then(current_usage_total).or_else(|| {
        let window = context_window_tokens?;
        let percent = u64::from(context_used_percent?);
        window.checked_mul(percent)?.checked_div(100)
    });
    let last_turn_tokens = current.and_then(current_usage_total);
    let estimated_cost_usd_micros = payload
        .pointer("/cost/total_cost_usd")
        .and_then(value_f64)
        .and_then(dollars_to_micros);

    let record = UsageRecord {
        provider: "claude".to_owned(),
        provider_session_id: session_id.clone(),
        last_turn_tokens,
        context_used_tokens,
        context_window_tokens,
        context_used_percent,
        estimated_cost_usd_micros,
        cost_kind: estimated_cost_usd_micros.map(|_| "provider_estimate".to_owned()),
        pricing_source: estimated_cost_usd_micros.map(|_| "claude_statusline".to_owned()),
        usage_source: "statusline".to_owned(),
        usage_quality: "official".to_owned(),
        captured_at: now_ms,
        ..UsageRecord::default()
    };
    let document = StatusCacheDocument {
        schema_version: STATUS_CACHE_SCHEMA,
        record: record.clone(),
    };
    let mut encoded = serde_json::to_vec(&document)?;
    encoded.push(b'\n');
    atomic_write(
        &cache_dir.join(format!("{session_id}.json")),
        &encoded,
        0o600,
    )?;
    Ok(Some(record))
}

fn current_usage_total(value: &Value) -> Option<u64> {
    if let Some(total) = value_u64(value) {
        return Some(total);
    }
    let object = value.as_object()?;
    [
        "input_tokens",
        "output_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
    ]
    .into_iter()
    .try_fold(0_u64, |total, key| {
        total.checked_add(object.get(key).and_then(value_u64).unwrap_or_default())
    })
}

fn safe_session_id(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 128
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return None;
    }
    Some(value.to_owned())
}

fn value_u64(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        value
            .as_f64()
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(|value| value.round() as u64)
    })
}

fn value_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .filter(|value| value.is_finite() && *value >= 0.0)
}

fn dollars_to_micros(value: f64) -> Option<u64> {
    let micros = value * 1_000_000.0;
    (micros.is_finite() && micros >= 0.0 && micros <= u64::MAX as f64)
        .then(|| micros.round() as u64)
}

#[derive(Debug, Clone, Default)]
struct TokenEntry {
    model: Option<String>,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_creation: u64,
    official_cost_usd_micros: Option<u64>,
    is_sidechain: bool,
    timestamp: String,
}

impl TokenEntry {
    fn claude_total(&self) -> u64 {
        self.input
            .saturating_add(self.output)
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_creation)
    }
}

#[derive(Debug)]
struct ClaudeFileState {
    offset: u64,
    size: u64,
    modified: SystemTime,
    session_id: Option<String>,
    entries: HashMap<String, TokenEntry>,
    latest_key: Option<String>,
}

impl Default for ClaudeFileState {
    fn default() -> Self {
        Self {
            offset: 0,
            size: 0,
            modified: UNIX_EPOCH,
            session_id: None,
            entries: HashMap::new(),
            latest_key: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct CodexUsage {
    input: u64,
    cached: u64,
    output: u64,
    reasoning: u64,
    total: u64,
}

#[derive(Debug)]
struct CodexFileState {
    offset: u64,
    size: u64,
    modified: SystemTime,
    session_id: Option<String>,
    model: Option<String>,
    context_window: Option<u64>,
    cumulative: Option<CodexUsage>,
    last: Option<CodexUsage>,
}

impl Default for CodexFileState {
    fn default() -> Self {
        Self {
            offset: 0,
            size: 0,
            modified: UNIX_EPOCH,
            session_id: None,
            model: None,
            context_window: None,
            cumulative: None,
            last: None,
        }
    }
}

/// Incremental local collector. Discovery is throttled, while known hot files
/// are tailed on every call.
pub struct UsageCollector {
    paths: UsagePaths,
    claude_files: HashMap<PathBuf, ClaudeFileState>,
    codex_files: HashMap<PathBuf, CodexFileState>,
    known_claude: Vec<PathBuf>,
    known_codex: Vec<PathBuf>,
    last_discovery: Option<Instant>,
}

impl UsageCollector {
    pub fn new(paths: UsagePaths) -> Self {
        Self {
            paths,
            claude_files: HashMap::new(),
            codex_files: HashMap::new(),
            known_claude: Vec::new(),
            known_codex: Vec::new(),
            last_discovery: None,
        }
    }

    pub fn discover() -> Self {
        Self::new(UsagePaths::discover())
    }

    pub fn paths(&self) -> &UsagePaths {
        &self.paths
    }

    pub fn collect(&mut self, now_ms: u64) -> Vec<UsageRecord> {
        if self
            .last_discovery
            .is_none_or(|last| last.elapsed() >= DISCOVERY_INTERVAL)
        {
            self.known_claude = discover_recent_files(&self.paths.claude_projects);
            self.known_codex = discover_recent_files(&self.paths.codex_sessions);
            self.last_discovery = Some(Instant::now());
            let claude_set = self.known_claude.iter().cloned().collect::<HashSet<_>>();
            let codex_set = self.known_codex.iter().cloned().collect::<HashSet<_>>();
            self.claude_files
                .retain(|path, _| claude_set.contains(path));
            self.codex_files.retain(|path, _| codex_set.contains(path));
        }

        let known_claude = self.known_claude.clone();
        for path in known_claude {
            self.refresh_claude_file(&path);
        }
        let known_codex = self.known_codex.clone();
        for path in known_codex {
            self.refresh_codex_file(&path);
        }

        let mut records = HashMap::<(String, String), UsageRecord>::new();
        for record in claude_records(self.claude_files.values(), now_ms) {
            merge_record(&mut records, record);
        }
        for state in self.codex_files.values() {
            if let Some(record) = codex_record(state, now_ms) {
                merge_record(&mut records, record);
            }
        }
        for record in read_status_caches(&self.paths.claude_status_cache_dir()) {
            merge_record(&mut records, record);
        }
        records.into_values().collect()
    }

    fn refresh_claude_file(&mut self, path: &Path) {
        let Ok(metadata) = regular_file_metadata(path) else {
            return;
        };
        let size = metadata.len();
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        let state = self.claude_files.entry(path.to_path_buf()).or_default();
        if size == state.size && modified == state.modified {
            return;
        }
        if size < state.offset {
            *state = ClaudeFileState::default();
        }
        parse_claude_tail(path, state, size);
        state.size = size;
        state.modified = modified;
    }

    fn refresh_codex_file(&mut self, path: &Path) {
        let Ok(metadata) = regular_file_metadata(path) else {
            return;
        };
        let size = metadata.len();
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        let state = self.codex_files.entry(path.to_path_buf()).or_default();
        if size == state.size && modified == state.modified {
            return;
        }
        if size < state.offset {
            *state = CodexFileState::default();
        }
        parse_codex_tail(path, state, size);
        state.size = size;
        state.modified = modified;
    }
}

fn claude_records<'a>(
    states: impl Iterator<Item = &'a ClaudeFileState>,
    now_ms: u64,
) -> Vec<UsageRecord> {
    let mut grouped = HashMap::<String, ClaudeFileState>::new();
    for state in states {
        let Some(session_id) = state.session_id.as_ref() else {
            continue;
        };
        let group = grouped.entry(session_id.clone()).or_default();
        group.session_id = Some(session_id.clone());
        group.modified = group.modified.max(state.modified);
        for (key, entry) in &state.entries {
            let replace = group.entries.get(key).is_none_or(|previous| {
                (previous.is_sidechain && !entry.is_sidechain)
                    || previous.claude_total() <= entry.claude_total()
            });
            if replace {
                group.entries.insert(key.clone(), entry.clone());
            }
            let latest = group.latest_key.as_ref().is_none_or(|previous_key| {
                let previous = group.entries.get(previous_key);
                let current = group.entries.get(key);
                match (previous, current) {
                    (Some(previous), Some(current)) => current.timestamp >= previous.timestamp,
                    _ => true,
                }
            });
            if latest {
                group.latest_key = Some(key.clone());
            }
        }
    }
    grouped
        .values()
        .filter_map(|state| claude_record(state, now_ms))
        .collect()
}

fn merge_record(records: &mut HashMap<(String, String), UsageRecord>, record: UsageRecord) {
    let key = (record.provider.clone(), record.provider_session_id.clone());
    records
        .entry(key)
        .and_modify(|current| current.merge_from(record.clone()))
        .or_insert(record);
}

fn discover_recent_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::<(PathBuf, SystemTime)>::new();
    for root in roots {
        collect_jsonl(root, 0, &mut files);
    }
    files.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    files.truncate(MAX_DISCOVERED_FILES);
    files.into_iter().map(|(path, _)| path).collect()
}

fn collect_jsonl(path: &Path, depth: usize, output: &mut Vec<(PathBuf, SystemTime)>) {
    if depth > 8 || output.len() >= MAX_DISCOVERED_FILES.saturating_mul(4) {
        return;
    }
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_symlink() {
        return;
    }
    if metadata.is_file() {
        if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
            return;
        }
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        if SystemTime::now()
            .duration_since(modified)
            .unwrap_or_default()
            <= RECENT_FILE_AGE
        {
            output.push((path.to_path_buf(), modified));
        }
        return;
    }
    if !metadata.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        collect_jsonl(&entry.path(), depth + 1, output);
    }
}

fn regular_file_metadata(path: &Path) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage source is not a regular file",
        ));
    }
    Ok(metadata)
}

fn parse_claude_tail(path: &Path, state: &mut ClaudeFileState, size: u64) {
    let Ok(mut file) = File::open(path) else {
        return;
    };
    if file.seek(SeekFrom::Start(state.offset)).is_err() {
        return;
    }
    let mut reader = BufReader::new(file);
    loop {
        let line_offset = state.offset;
        let Ok((line, consumed, complete, too_large)) =
            read_bounded_line(&mut reader, MAX_JSONL_LINE_BYTES as usize)
        else {
            return;
        };
        if consumed == 0 {
            break;
        }
        if too_large {
            if complete {
                state.offset = state.offset.saturating_add(consumed);
                continue;
            }
            break;
        }
        let Ok(root) = serde_json::from_slice::<Value>(&line) else {
            if complete || state.offset.saturating_add(consumed) < size {
                state.offset = state.offset.saturating_add(consumed);
                continue;
            }
            break;
        };
        state.offset = state.offset.saturating_add(consumed);

        if state.session_id.is_none() {
            state.session_id = root
                .get("sessionId")
                .or_else(|| root.get("session_id"))
                .and_then(Value::as_str)
                .and_then(safe_session_id)
                .or_else(|| {
                    path.file_stem()
                        .and_then(|value| value.to_str())
                        .and_then(safe_session_id)
                });
        }

        let payload = if root.pointer("/message/usage").is_some() {
            &root
        } else if root.pointer("/data/message/message/usage").is_some() {
            root.pointer("/data/message").unwrap_or(&root)
        } else {
            continue;
        };
        let Some(usage) = payload.pointer("/message/usage") else {
            continue;
        };
        let model = payload
            .pointer("/message/model")
            .and_then(Value::as_str)
            .filter(|value| !value.starts_with('<'))
            .map(ToOwned::to_owned);
        let message_id = payload
            .pointer("/message/id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        let request_id = payload
            .get("requestId")
            .or_else(|| payload.get("request_id"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        let key = match (message_id, request_id) {
            (Some(message), Some(request)) => format!("{message}:{request}"),
            (Some(message), None) => message.to_owned(),
            _ => format!("offset:{line_offset}"),
        };
        let cache_creation = usage
            .pointer("/cache_creation/ephemeral_5m_input_tokens")
            .and_then(value_u64)
            .unwrap_or_default()
            .saturating_add(
                usage
                    .pointer("/cache_creation/ephemeral_1h_input_tokens")
                    .and_then(value_u64)
                    .unwrap_or_default(),
            );
        let entry = TokenEntry {
            model,
            input: usage
                .get("input_tokens")
                .and_then(value_u64)
                .unwrap_or_default(),
            output: usage
                .get("output_tokens")
                .and_then(value_u64)
                .unwrap_or_default(),
            cache_read: usage
                .get("cache_read_input_tokens")
                .and_then(value_u64)
                .unwrap_or_default(),
            cache_creation: if cache_creation > 0 {
                cache_creation
            } else {
                usage
                    .get("cache_creation_input_tokens")
                    .and_then(value_u64)
                    .unwrap_or_default()
            },
            official_cost_usd_micros: payload
                .get("costUSD")
                .or_else(|| root.get("costUSD"))
                .and_then(value_f64)
                .and_then(dollars_to_micros),
            is_sidechain: payload
                .get("isSidechain")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            timestamp: payload
                .get("timestamp")
                .or_else(|| root.get("timestamp"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        };
        let should_replace = state.entries.get(&key).is_none_or(|previous| {
            (previous.is_sidechain && !entry.is_sidechain)
                || previous.claude_total() <= entry.claude_total()
        });
        if should_replace {
            state.entries.insert(key.clone(), entry);
        }
        let is_latest = state.latest_key.as_ref().is_none_or(|previous_key| {
            let previous = state.entries.get(previous_key);
            let current = state.entries.get(&key);
            match (previous, current) {
                (Some(previous), Some(current)) => {
                    current.timestamp >= previous.timestamp || key == *previous_key
                }
                _ => true,
            }
        });
        if is_latest {
            state.latest_key = Some(key);
        }
    }
}

fn parse_codex_tail(path: &Path, state: &mut CodexFileState, size: u64) {
    let Ok(mut file) = File::open(path) else {
        return;
    };
    if file.seek(SeekFrom::Start(state.offset)).is_err() {
        return;
    }
    let mut reader = BufReader::new(file);
    loop {
        let Ok((line, consumed, complete, too_large)) =
            read_bounded_line(&mut reader, MAX_JSONL_LINE_BYTES as usize)
        else {
            return;
        };
        if consumed == 0 {
            break;
        }
        if too_large {
            if complete {
                state.offset = state.offset.saturating_add(consumed);
                continue;
            }
            break;
        }
        let Ok(root) = serde_json::from_slice::<Value>(&line) else {
            if complete || state.offset.saturating_add(consumed) < size {
                state.offset = state.offset.saturating_add(consumed);
                continue;
            }
            break;
        };
        state.offset = state.offset.saturating_add(consumed);
        let event_type = root.get("type").and_then(Value::as_str).unwrap_or_default();
        let payload = root.get("payload").unwrap_or(&Value::Null);
        match event_type {
            "session_meta" => {
                state.session_id = payload
                    .get("id")
                    .or_else(|| payload.get("session_id"))
                    .and_then(Value::as_str)
                    .and_then(safe_session_id)
                    .or_else(|| state.session_id.clone());
            }
            "turn_context" => {
                state.model = payload
                    .get("model")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .or_else(|| state.model.clone());
                state.context_window = payload
                    .get("context_window")
                    .or_else(|| payload.get("context_window_tokens"))
                    .and_then(value_u64)
                    .or(state.context_window);
            }
            "event_msg" if payload.get("type").and_then(Value::as_str) == Some("token_count") => {
                let Some(info) = payload.get("info") else {
                    continue;
                };
                let previous = state.cumulative;
                state.cumulative = info
                    .get("total_token_usage")
                    .and_then(parse_codex_usage)
                    .or(state.cumulative);
                state.last = info
                    .get("last_token_usage")
                    .and_then(parse_codex_usage)
                    .or_else(|| codex_usage_delta(state.cumulative, previous))
                    .or(state.last);
                state.context_window = info
                    .get("model_context_window")
                    .or_else(|| info.get("context_window"))
                    .and_then(value_u64)
                    .or(state.context_window);
            }
            _ => {}
        }
    }
    if state.session_id.is_none() {
        state.session_id = path
            .file_stem()
            .and_then(|value| value.to_str())
            .and_then(safe_session_id);
    }
}

fn parse_codex_usage(value: &Value) -> Option<CodexUsage> {
    let input = value
        .get("input_tokens")
        .and_then(value_u64)
        .unwrap_or_default();
    let cached = value
        .get("cached_input_tokens")
        .and_then(value_u64)
        .unwrap_or_default();
    let output = value
        .get("output_tokens")
        .and_then(value_u64)
        .unwrap_or_default();
    let reasoning = value
        .get("reasoning_output_tokens")
        .or_else(|| value.get("reasoning_tokens"))
        .and_then(value_u64)
        .unwrap_or_default();
    let total = value
        .get("total_tokens")
        .and_then(value_u64)
        .unwrap_or_else(|| input.saturating_add(output));
    (input > 0 || output > 0 || cached > 0 || reasoning > 0 || total > 0).then_some(CodexUsage {
        input,
        cached,
        output,
        reasoning,
        total,
    })
}

fn codex_usage_delta(
    current: Option<CodexUsage>,
    previous: Option<CodexUsage>,
) -> Option<CodexUsage> {
    let current = current?;
    let previous = previous.unwrap_or_default();
    Some(CodexUsage {
        input: current.input.saturating_sub(previous.input),
        cached: current.cached.saturating_sub(previous.cached),
        output: current.output.saturating_sub(previous.output),
        reasoning: current.reasoning.saturating_sub(previous.reasoning),
        total: current.total.saturating_sub(previous.total),
    })
}

fn claude_record(state: &ClaudeFileState, now_ms: u64) -> Option<UsageRecord> {
    let session_id = state.session_id.clone()?;
    if state.entries.is_empty() {
        return None;
    }
    let mut aggregate = TokenEntry::default();
    for entry in state.entries.values() {
        aggregate.input = aggregate.input.saturating_add(entry.input);
        aggregate.output = aggregate.output.saturating_add(entry.output);
        aggregate.cache_read = aggregate.cache_read.saturating_add(entry.cache_read);
        aggregate.cache_creation = aggregate
            .cache_creation
            .saturating_add(entry.cache_creation);
    }
    let latest = state
        .latest_key
        .as_ref()
        .and_then(|key| state.entries.get(key));
    let model = latest.and_then(|entry| entry.model.as_deref()).or_else(|| {
        state
            .entries
            .values()
            .find_map(|entry| entry.model.as_deref())
    });
    let context_used = latest.map(|entry| {
        entry.input.saturating_add(if entry.cache_read > 0 {
            entry.cache_read
        } else {
            entry.cache_creation
        })
    });
    let context_window = model.map(claude_context_window);
    let context_percent = percent(context_used, context_window);
    let official_cost = state
        .entries
        .values()
        .map(|entry| entry.official_cost_usd_micros)
        .try_fold(0_u64, |total, cost| total.checked_add(cost?));
    let cost =
        official_cost.or_else(|| model.and_then(|model| claude_cost_micros(model, &aggregate)));
    let cost_kind = cost.map(|_| {
        if official_cost.is_some() {
            "provider_estimate".to_owned()
        } else {
            "estimated_api_price".to_owned()
        }
    });
    let pricing_source = cost.map(|_| {
        if official_cost.is_some() {
            "claude_transcript_cost".to_owned()
        } else {
            PRICING_SOURCE.to_owned()
        }
    });
    Some(UsageRecord {
        provider: "claude".to_owned(),
        provider_session_id: session_id,
        input_tokens: Some(aggregate.input),
        output_tokens: Some(aggregate.output),
        cache_read_tokens: Some(aggregate.cache_read),
        cache_creation_tokens: Some(aggregate.cache_creation),
        reasoning_tokens: None,
        token_total: Some(aggregate.claude_total()),
        last_turn_tokens: latest.map(TokenEntry::claude_total),
        context_used_tokens: context_used,
        context_window_tokens: context_window,
        context_used_percent: context_percent,
        estimated_cost_usd_micros: cost,
        cost_kind,
        pricing_source,
        usage_source: "claude_transcript".to_owned(),
        usage_quality: "derived".to_owned(),
        captured_at: system_time_millis(state.modified).unwrap_or(now_ms),
    })
}

fn codex_record(state: &CodexFileState, now_ms: u64) -> Option<UsageRecord> {
    let session_id = state.session_id.clone()?;
    let total = state.cumulative?;
    let last = state.last.unwrap_or_default();
    let context_used = (last.input > 0).then_some(last.input);
    let context_percent = percent(context_used, state.context_window);
    let cost = state
        .model
        .as_deref()
        .and_then(|model| codex_cost_micros(model, total));
    Some(UsageRecord {
        provider: "codex".to_owned(),
        provider_session_id: session_id,
        input_tokens: Some(total.input),
        output_tokens: Some(total.output),
        cache_read_tokens: Some(total.cached),
        cache_creation_tokens: None,
        reasoning_tokens: Some(total.reasoning),
        token_total: Some(if total.total > 0 {
            total.total
        } else {
            total.input.saturating_add(total.output)
        }),
        last_turn_tokens: Some(if last.total > 0 {
            last.total
        } else {
            last.input.saturating_add(last.output)
        }),
        context_used_tokens: context_used,
        context_window_tokens: state.context_window,
        context_used_percent: context_percent,
        estimated_cost_usd_micros: cost,
        cost_kind: cost.map(|_| "estimated_api_price".to_owned()),
        pricing_source: cost.map(|_| PRICING_SOURCE.to_owned()),
        usage_source: "codex_rollout".to_owned(),
        usage_quality: "official_local".to_owned(),
        captured_at: system_time_millis(state.modified).unwrap_or(now_ms),
    })
}

fn percent(used: Option<u64>, window: Option<u64>) -> Option<u32> {
    let (used, window) = (used?, window?);
    if window == 0 {
        return None;
    }
    Some(
        ((used as f64 / window as f64) * 100.0)
            .clamp(0.0, 100.0)
            .round() as u32,
    )
}

fn claude_context_window(model: &str) -> u64 {
    let model = model.to_ascii_lowercase();
    if model.contains("[1m]")
        || model.contains("fable")
        || model.contains("mythos")
        || model.contains("opus-4-6")
        || model.contains("opus-4.6")
        || model.contains("opus-4-7")
        || model.contains("opus-4.7")
        || model.contains("opus-4-8")
        || model.contains("opus-4.8")
        || model.contains("sonnet-4-6")
        || model.contains("sonnet-4.6")
    {
        1_000_000
    } else {
        200_000
    }
}

#[derive(Clone, Copy)]
struct Price {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_create: f64,
}

fn claude_price(model: &str) -> Option<Price> {
    let model = model.to_ascii_lowercase();
    if model.contains("fast") {
        None
    } else if model.contains("fable") || model.contains("mythos") {
        Some(Price {
            input: 10.0,
            output: 50.0,
            cache_read: 1.0,
            cache_create: 12.5,
        })
    } else if model.contains("opus-4-6") || model.contains("opus-4.6") {
        Some(Price {
            input: 5.0,
            output: 25.0,
            cache_read: 0.5,
            cache_create: 6.25,
        })
    } else if model.contains("opus") {
        Some(Price {
            input: 15.0,
            output: 75.0,
            cache_read: 1.5,
            cache_create: 18.75,
        })
    } else if model.contains("sonnet") {
        Some(Price {
            input: 3.0,
            output: 15.0,
            cache_read: 0.3,
            cache_create: 3.75,
        })
    } else if model.contains("haiku") {
        Some(Price {
            input: 1.0,
            output: 5.0,
            cache_read: 0.1,
            cache_create: 1.25,
        })
    } else {
        None
    }
}

fn codex_price(model: &str) -> Option<Price> {
    let model = model.to_ascii_lowercase();
    let (input, output, cache_read) = if model.contains("gpt-5.6-sol") {
        (5.0, 30.0, 0.5)
    } else if model.contains("gpt-5.6-terra") || model.contains("gpt-5.4") {
        (2.5, 15.0, 0.25)
    } else if model.contains("gpt-5.6-luna") {
        (1.0, 6.0, 0.1)
    } else if model.contains("gpt-5.3") || model.contains("gpt-5.2-codex") {
        (1.75, 14.0, 0.175)
    } else if model.contains("gpt-5.2") || model.contains("gpt-5") {
        (1.25, 10.0, 0.125)
    } else {
        return None;
    };
    Some(Price {
        input,
        output,
        cache_read,
        cache_create: 0.0,
    })
}

fn claude_cost_micros(model: &str, usage: &TokenEntry) -> Option<u64> {
    let price = claude_price(model)?;
    priced_cost_micros(
        usage.input,
        usage.output,
        usage.cache_read,
        usage.cache_creation,
        price,
    )
}

fn codex_cost_micros(model: &str, usage: CodexUsage) -> Option<u64> {
    let price = codex_price(model)?;
    priced_cost_micros(
        usage.input.saturating_sub(usage.cached),
        usage.output,
        usage.cached,
        0,
        price,
    )
}

fn priced_cost_micros(
    input: u64,
    output: u64,
    cache_read: u64,
    cache_create: u64,
    price: Price,
) -> Option<u64> {
    let dollars = input as f64 * price.input / 1_000_000.0
        + output as f64 * price.output / 1_000_000.0
        + cache_read as f64 * price.cache_read / 1_000_000.0
        + cache_create as f64 * price.cache_create / 1_000_000.0;
    dollars_to_micros(dollars)
}

fn read_status_caches(cache_dir: &Path) -> Vec<UsageRecord> {
    let Ok(entries) = fs::read_dir(cache_dir) else {
        return Vec::new();
    };
    let mut records = Vec::new();
    for entry in entries.flatten().take(MAX_DISCOVERED_FILES) {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(metadata) = regular_file_metadata(&path) else {
            continue;
        };
        if metadata.len() > MAX_STATUSLINE_BYTES {
            continue;
        }
        let Ok(mut file) = File::open(&path) else {
            continue;
        };
        let mut encoded = Vec::with_capacity(metadata.len() as usize);
        if Read::by_ref(&mut file)
            .take(MAX_STATUSLINE_BYTES.saturating_add(1))
            .read_to_end(&mut encoded)
            .is_err()
            || encoded.len() as u64 > MAX_STATUSLINE_BYTES
        {
            continue;
        }
        let Ok(document) = serde_json::from_slice::<StatusCacheDocument>(&encoded) else {
            continue;
        };
        if document.schema_version == STATUS_CACHE_SCHEMA
            && document.record.provider == "claude"
            && safe_session_id(&document.record.provider_session_id).is_some()
        {
            records.push(document.record);
        }
    }
    records
}

fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    limit: usize,
) -> io::Result<(Vec<u8>, u64, bool, bool)> {
    let mut output = Vec::new();
    let mut consumed = 0_u64;
    let mut complete = false;
    let mut too_large = false;
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            break;
        }
        let take = buffer
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(buffer.len(), |position| position + 1);
        if output.len().saturating_add(take) <= limit {
            output.extend_from_slice(&buffer[..take]);
        } else {
            too_large = true;
        }
        consumed = consumed.saturating_add(take as u64);
        complete = buffer[..take].ends_with(b"\n");
        reader.consume(take);
        if complete {
            break;
        }
    }
    Ok((output, consumed, complete, too_large))
}

fn system_time_millis(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<(), UsageError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut builder = DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder.create(parent).map_err(|source| UsageError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|source| {
        UsageError::Io {
            path: parent.to_path_buf(),
            source,
        }
    })?;
    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("usage"),
        std::process::id(),
        id
    ));
    let write_result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(mode)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
        Ok(())
    })();
    if let Err(source) = write_result {
        let _ = fs::remove_file(&temp);
        return Err(UsageError::Io {
            path: path.to_path_buf(),
            source,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "actrealm-usage-{label}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp directory");
        path
    }

    #[test]
    fn captures_official_statusline_context_without_prompt_data() {
        let root = temp_dir("statusline");
        let payload = br#"{
          "session_id":"session-123",
          "context_window":{"context_window_size":200000,"used_percentage":25,
            "current_usage":{"input_tokens":40000,"cache_read_input_tokens":10000}},
          "cost":{"total_cost_usd":1.25},
          "transcript_path":"/secret/path","cwd":"/secret/project"
        }"#;
        let record = capture_claude_statusline_usage(payload, &root, 42)
            .expect("capture succeeds")
            .expect("record exists");
        assert_eq!(record.context_used_tokens, Some(50_000));
        assert_eq!(record.context_used_percent, Some(25));
        assert_eq!(record.estimated_cost_usd_micros, Some(1_250_000));
        let saved = fs::read_to_string(root.join("session-123.json")).expect("cache exists");
        assert!(!saved.contains("secret"));
        let mode = fs::metadata(root.join("session-123.json"))
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn incrementally_deduplicates_claude_streamed_messages() {
        let root = temp_dir("claude");
        let path = root.join("session-a.jsonl");
        let first = concat!(
            "{\"sessionId\":\"session-a\",\"timestamp\":\"2026-07-18T01:00:00Z\",",
            "\"message\":{\"id\":\"msg-1\",\"model\":\"claude-sonnet-4\",",
            "\"usage\":{\"input_tokens\":100,\"output_tokens\":5,",
            "\"cache_read_input_tokens\":20,\"cache_creation_input_tokens\":10}},",
            "\"requestId\":\"req-1\"}\n",
            "{\"sessionId\":\"session-a\",\"timestamp\":\"2026-07-18T01:00:01Z\",",
            "\"message\":{\"id\":\"msg-1\",\"model\":\"claude-sonnet-4\",",
            "\"usage\":{\"input_tokens\":100,\"output_tokens\":50,",
            "\"cache_read_input_tokens\":20,\"cache_creation_input_tokens\":10}},",
            "\"requestId\":\"req-1\"}\n"
        );
        fs::write(&path, first).expect("write fixture");
        let mut state = ClaudeFileState::default();
        parse_claude_tail(&path, &mut state, first.len() as u64);
        let record = claude_record(&state, 100).expect("record");
        assert_eq!(record.token_total, Some(180));
        assert_eq!(record.context_used_tokens, Some(120));

        let second = concat!(
            "{\"sessionId\":\"session-a\",\"timestamp\":\"2026-07-18T01:00:02Z\",",
            "\"message\":{\"id\":\"msg-2\",\"model\":\"claude-sonnet-4\",",
            "\"usage\":{\"input_tokens\":30,\"output_tokens\":10,",
            "\"cache_read_input_tokens\":150,\"cache_creation_input_tokens\":0}},",
            "\"requestId\":\"req-2\"}\n"
        );
        let mut file = OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open append");
        file.write_all(second.as_bytes()).expect("append fixture");
        parse_claude_tail(
            &path,
            &mut state,
            first.len().saturating_add(second.len()) as u64,
        );
        let record = claude_record(&state, 200).expect("record");
        assert_eq!(record.token_total, Some(370));
        assert_eq!(record.context_used_tokens, Some(180));
        assert_eq!(record.context_used_percent, Some(0));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parses_codex_cumulative_and_last_turn_without_double_counting_cache() {
        let root = temp_dir("codex");
        let path = root.join("rollout-session-b.jsonl");
        let fixture = concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"session-b\"}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-5.6-sol\",",
            "\"context_window\":1000}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{",
            "\"total_token_usage\":{\"input_tokens\":700,\"cached_input_tokens\":400,",
            "\"output_tokens\":100,\"reasoning_output_tokens\":20,\"total_tokens\":800},",
            "\"last_token_usage\":{\"input_tokens\":250,\"cached_input_tokens\":200,",
            "\"output_tokens\":50,\"reasoning_output_tokens\":10,\"total_tokens\":300},",
            "\"model_context_window\":1000}}}\n"
        );
        fs::write(&path, fixture).expect("write fixture");
        let mut state = CodexFileState::default();
        parse_codex_tail(&path, &mut state, fixture.len() as u64);
        let record = codex_record(&state, 100).expect("record");
        assert_eq!(record.token_total, Some(800));
        assert_eq!(record.cache_read_tokens, Some(400));
        assert_eq!(record.reasoning_tokens, Some(20));
        assert_eq!(record.last_turn_tokens, Some(300));
        assert_eq!(record.context_used_tokens, Some(250));
        assert_eq!(record.context_used_percent, Some(25));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unknown_models_do_not_claim_zero_cost() {
        let usage = TokenEntry {
            input: 1_000,
            output: 500,
            ..TokenEntry::default()
        };
        assert_eq!(claude_cost_micros("future-model", &usage), None);
        assert_eq!(
            codex_cost_micros(
                "future-model",
                CodexUsage {
                    input: 1_000,
                    output: 500,
                    ..CodexUsage::default()
                }
            ),
            None
        );
    }
}

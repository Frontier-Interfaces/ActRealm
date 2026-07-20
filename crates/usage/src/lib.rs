//! Local, privacy-bounded session usage collection for Claude Code and Codex.
//!
//! The collector stores only numeric usage metadata. It never persists prompts,
//! tool input/output, transcript text, or provider credentials.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

const STATUS_CACHE_SCHEMA: u32 = 1;
const MAX_STATUSLINE_BYTES: u64 = 256 * 1_024;
const MAX_JSONL_LINE_BYTES: u64 = 10 * 1_024 * 1_024;
const MAX_DISCOVERED_FILES: usize = 512;
const RECENT_FILE_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const DISCOVERY_INTERVAL: Duration = Duration::from_secs(5);
const MAX_RECENT_CLAUDE_ENTRIES_PER_FILE: usize = 256;
const PRICING_SNAPSHOT_JSON: &str = include_str!("pricing_snapshot.json");
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
    /// Structured Provider model identifier used for this usage sample. It is
    /// metadata only; prompts and transcript paths are never retained here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
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
        replace_some!(model);
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

#[derive(Debug, Clone, Default)]
struct TokenAccumulator {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_creation: u64,
    entry_count: u64,
    official_cost_count: u64,
    official_cost_usd_micros: u64,
    computed_cost_count: u64,
    computed_cost_usd_micros: f64,
}

impl TokenAccumulator {
    fn add_entry(&mut self, entry: &TokenEntry) {
        self.input = self.input.saturating_add(entry.input);
        self.output = self.output.saturating_add(entry.output);
        self.cache_read = self.cache_read.saturating_add(entry.cache_read);
        self.cache_creation = self.cache_creation.saturating_add(entry.cache_creation);
        self.entry_count = self.entry_count.saturating_add(1);
        let has_no_billable_tokens = entry.claude_total() == 0;
        if let Some(cost) = entry.official_cost_usd_micros {
            self.official_cost_count = self.official_cost_count.saturating_add(1);
            self.official_cost_usd_micros = self.official_cost_usd_micros.saturating_add(cost);
        } else if has_no_billable_tokens {
            self.official_cost_count = self.official_cost_count.saturating_add(1);
        }
        if let Some(cost) = entry
            .model
            .as_deref()
            .and_then(|model| claude_cost_micros_value(model, entry))
        {
            self.computed_cost_count = self.computed_cost_count.saturating_add(1);
            self.computed_cost_usd_micros += cost;
        } else if has_no_billable_tokens {
            self.computed_cost_count = self.computed_cost_count.saturating_add(1);
        }
    }

    fn remove_entry(&mut self, entry: &TokenEntry) {
        self.input = self.input.saturating_sub(entry.input);
        self.output = self.output.saturating_sub(entry.output);
        self.cache_read = self.cache_read.saturating_sub(entry.cache_read);
        self.cache_creation = self.cache_creation.saturating_sub(entry.cache_creation);
        self.entry_count = self.entry_count.saturating_sub(1);
        let has_no_billable_tokens = entry.claude_total() == 0;
        if let Some(cost) = entry.official_cost_usd_micros {
            self.official_cost_count = self.official_cost_count.saturating_sub(1);
            self.official_cost_usd_micros = self.official_cost_usd_micros.saturating_sub(cost);
        } else if has_no_billable_tokens {
            self.official_cost_count = self.official_cost_count.saturating_sub(1);
        }
        if let Some(cost) = entry
            .model
            .as_deref()
            .and_then(|model| claude_cost_micros_value(model, entry))
        {
            self.computed_cost_count = self.computed_cost_count.saturating_sub(1);
            self.computed_cost_usd_micros = (self.computed_cost_usd_micros - cost).max(0.0);
        } else if has_no_billable_tokens {
            self.computed_cost_count = self.computed_cost_count.saturating_sub(1);
        }
    }

    fn add_accumulator(&mut self, other: &Self) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.cache_creation = self.cache_creation.saturating_add(other.cache_creation);
        self.entry_count = self.entry_count.saturating_add(other.entry_count);
        self.official_cost_count = self
            .official_cost_count
            .saturating_add(other.official_cost_count);
        self.official_cost_usd_micros = self
            .official_cost_usd_micros
            .saturating_add(other.official_cost_usd_micros);
        self.computed_cost_count = self
            .computed_cost_count
            .saturating_add(other.computed_cost_count);
        self.computed_cost_usd_micros += other.computed_cost_usd_micros;
    }

    fn official_cost(&self) -> Option<u64> {
        (self.entry_count > 0 && self.official_cost_count == self.entry_count)
            .then_some(self.official_cost_usd_micros)
    }

    fn computed_cost(&self) -> Option<u64> {
        (self.entry_count > 0
            && self.computed_cost_count == self.entry_count
            && self.computed_cost_usd_micros.is_finite()
            && self.computed_cost_usd_micros >= 0.0
            && self.computed_cost_usd_micros <= u64::MAX as f64)
            .then(|| self.computed_cost_usd_micros.round() as u64)
    }

    fn token_total(&self) -> u64 {
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
    compacted: TokenAccumulator,
    recent: TokenAccumulator,
    entries: HashMap<String, TokenEntry>,
    entry_order: VecDeque<String>,
    latest_key: Option<String>,
}

impl Default for ClaudeFileState {
    fn default() -> Self {
        Self {
            offset: 0,
            size: 0,
            modified: UNIX_EPOCH,
            session_id: None,
            compacted: TokenAccumulator::default(),
            recent: TokenAccumulator::default(),
            entries: HashMap::new(),
            entry_order: VecDeque::new(),
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
    computed_cost_usd_micros: f64,
    computed_cost_complete: bool,
    pricing_source: Option<String>,
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
            computed_cost_usd_micros: 0.0,
            computed_cost_complete: true,
            pricing_source: None,
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
    let mut states = states.collect::<Vec<_>>();
    states.sort_by_key(|state| state.modified);
    let mut grouped = HashMap::<String, ClaudeFileState>::new();
    for state in states {
        let Some(session_id) = state.session_id.as_ref() else {
            continue;
        };
        let group = grouped.entry(session_id.clone()).or_default();
        group.session_id = Some(session_id.clone());
        group.modified = group.modified.max(state.modified);
        group.compacted.add_accumulator(&state.compacted);
        for key in &state.entry_order {
            if let Some(entry) = state.entries.get(key) {
                upsert_claude_entry(group, key.clone(), entry.clone());
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
        upsert_claude_entry(state, key, entry);
    }
}

fn upsert_claude_entry(state: &mut ClaudeFileState, key: String, entry: TokenEntry) {
    let should_replace = state.entries.get(&key).is_none_or(|previous| {
        (previous.is_sidechain && !entry.is_sidechain)
            || previous.claude_total() <= entry.claude_total()
    });
    if !should_replace {
        return;
    }
    let is_new = !state.entries.contains_key(&key);
    if let Some(previous) = state.entries.insert(key.clone(), entry.clone()) {
        state.recent.remove_entry(&previous);
    }
    state.recent.add_entry(&entry);
    if is_new {
        state.entry_order.push_back(key.clone());
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
    compact_claude_entries(state);
}

fn compact_claude_entries(state: &mut ClaudeFileState) {
    let mut latest_removed = false;
    while state.entry_order.len() > MAX_RECENT_CLAUDE_ENTRIES_PER_FILE {
        let Some(key) = state.entry_order.pop_front() else {
            break;
        };
        if let Some(entry) = state.entries.remove(&key) {
            state.recent.remove_entry(&entry);
            state.compacted.add_entry(&entry);
        }
        latest_removed |= state.latest_key.as_deref() == Some(key.as_str());
    }
    if latest_removed {
        state.latest_key = state
            .entry_order
            .iter()
            .filter_map(|key| state.entries.get(key).map(|entry| (key, entry)))
            .max_by(|(_, left), (_, right)| left.timestamp.cmp(&right.timestamp))
            .map(|(key, _)| key.clone());
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
                let current = info
                    .get("total_token_usage")
                    .and_then(parse_codex_usage)
                    .or(state.cumulative);
                if let Some(current) = current {
                    let reset =
                        previous.is_some_and(|previous| codex_usage_decreased(current, previous));
                    if reset {
                        state.computed_cost_usd_micros = 0.0;
                        state.computed_cost_complete = true;
                        state.pricing_source = None;
                    }
                    let cost_previous = if reset { None } else { previous };
                    if let Some(delta) = codex_usage_delta(Some(current), cost_previous) {
                        record_codex_cost(state, delta);
                    }
                    state.cumulative = Some(current);
                }
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

fn codex_usage_decreased(current: CodexUsage, previous: CodexUsage) -> bool {
    current.input < previous.input
        || current.cached < previous.cached
        || current.output < previous.output
        || current.reasoning < previous.reasoning
        || current.total < previous.total
}

fn record_codex_cost(state: &mut CodexFileState, delta: CodexUsage) {
    if delta.input == 0 && delta.cached == 0 && delta.output == 0 {
        return;
    }
    let Some(model) = state.model.as_deref() else {
        state.computed_cost_complete = false;
        return;
    };
    let Some(price) = model_pricing("codex", model) else {
        state.computed_cost_complete = false;
        return;
    };
    let Some(cost) = codex_cost_micros_value(model, delta) else {
        state.computed_cost_complete = false;
        return;
    };
    state.computed_cost_usd_micros += cost;
    match state.pricing_source.as_deref() {
        None => state.pricing_source = Some(price.source.clone()),
        Some(source) if source == price.source => {}
        Some("mixed_pricing_sources_2026-07-20") => {}
        Some(_) => state.pricing_source = Some("mixed_pricing_sources_2026-07-20".to_owned()),
    }
}

fn claude_record(state: &ClaudeFileState, now_ms: u64) -> Option<UsageRecord> {
    let session_id = state.session_id.clone()?;
    let mut aggregate = state.compacted.clone();
    aggregate.add_accumulator(&state.recent);
    if aggregate.entry_count == 0 {
        return None;
    }
    let latest = state
        .entries
        .values()
        .filter(|entry| !is_synthetic_model(entry.model.as_deref()))
        .max_by(|left, right| left.timestamp.cmp(&right.timestamp));
    let model = latest.and_then(|entry| entry.model.as_deref());
    let context_used = latest.map(|entry| {
        entry.input.saturating_add(if entry.cache_read > 0 {
            entry.cache_read
        } else {
            entry.cache_creation
        })
    });
    let context_window = model.map(claude_context_window);
    let context_percent = percent(context_used, context_window);
    let official_cost = aggregate.official_cost();
    let cost = official_cost.or_else(|| aggregate.computed_cost());
    let cost_kind = cost.map(|_| {
        if official_cost.is_some() {
            "provider_estimate".to_owned()
        } else {
            "computed".to_owned()
        }
    });
    let pricing_source = cost.map(|_| {
        if official_cost.is_some() {
            "claude_transcript_cost".to_owned()
        } else {
            model
                .and_then(|model| model_pricing("claude", model))
                .map(|price| price.source.clone())
                .unwrap_or_else(|| pricing_snapshot().source.clone())
        }
    });
    Some(UsageRecord {
        provider: "claude".to_owned(),
        provider_session_id: session_id,
        model: model.map(ToOwned::to_owned),
        input_tokens: Some(aggregate.input),
        output_tokens: Some(aggregate.output),
        cache_read_tokens: Some(aggregate.cache_read),
        cache_creation_tokens: Some(aggregate.cache_creation),
        reasoning_tokens: None,
        token_total: Some(aggregate.token_total()),
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

fn is_synthetic_model(model: Option<&str>) -> bool {
    model.is_some_and(|model| model.trim().eq_ignore_ascii_case("<synthetic>"))
}

fn codex_record(state: &CodexFileState, now_ms: u64) -> Option<UsageRecord> {
    let session_id = state.session_id.clone()?;
    let total = state.cumulative?;
    let last = state.last.unwrap_or_default();
    let context_used = (last.input > 0).then_some(last.input);
    let context_percent = percent(context_used, state.context_window);
    let cost = (state.computed_cost_complete
        && state.pricing_source.is_some()
        && state.computed_cost_usd_micros.is_finite()
        && state.computed_cost_usd_micros >= 0.0
        && state.computed_cost_usd_micros <= u64::MAX as f64)
        .then(|| state.computed_cost_usd_micros.round() as u64);
    Some(UsageRecord {
        provider: "codex".to_owned(),
        provider_session_id: session_id,
        model: state.model.clone(),
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
        cost_kind: cost.map(|_| "computed".to_owned()),
        pricing_source: cost.and_then(|_| state.pricing_source.clone()),
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
        || model.contains("sonnet-5")
    {
        1_000_000
    } else {
        200_000
    }
}

#[derive(Debug, Deserialize)]
struct PricingSnapshot {
    source: String,
    models: Vec<ModelPrice>,
}

#[derive(Debug, Deserialize)]
struct ModelPrice {
    provider: String,
    source: String,
    id: String,
    #[serde(default)]
    aliases: Vec<String>,
    input: f64,
    output: f64,
    cache_read: f64,
    cache_create: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Price {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_create: f64,
}

fn pricing_snapshot() -> &'static PricingSnapshot {
    static SNAPSHOT: OnceLock<PricingSnapshot> = OnceLock::new();
    SNAPSHOT.get_or_init(|| {
        serde_json::from_str(PRICING_SNAPSHOT_JSON)
            .expect("embedded usage pricing snapshot must be valid")
    })
}

fn claude_price(model: &str) -> Option<Price> {
    model_price("claude", model)
}

fn codex_price(model: &str) -> Option<Price> {
    model_price("codex", model)
}

fn model_price(provider: &str, model: &str) -> Option<Price> {
    model_pricing(provider, model).map(|price| Price {
        input: price.input,
        output: price.output,
        cache_read: price.cache_read,
        cache_create: price.cache_create,
    })
}

fn model_pricing(provider: &str, model: &str) -> Option<&'static ModelPrice> {
    let candidates = normalized_model_candidates(model);
    pricing_snapshot()
        .models
        .iter()
        .filter(|price| price.provider == provider)
        .find(|price| {
            candidates.iter().any(|candidate| {
                candidate == &price.id || price.aliases.iter().any(|alias| candidate == alias)
            })
        })
}

fn normalized_model_candidates(model: &str) -> Vec<String> {
    let normalized = model.trim().to_ascii_lowercase();
    if normalized.is_empty() || normalized.contains("fast") {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    push_model_candidate(&mut candidates, normalized.clone());
    if let Some(last) = normalized.rsplit('/').next() {
        push_model_candidate(&mut candidates, last.to_owned());
    }
    for prefix in ["anthropic--", "anthropic.", "openai--", "openai."] {
        if let Some(value) = normalized.strip_prefix(prefix) {
            push_model_candidate(&mut candidates, value.to_owned());
        }
    }
    let snapshot = candidates.clone();
    for candidate in snapshot {
        if let Some((base, _)) = candidate.split_once(":thinking") {
            push_model_candidate(&mut candidates, base.to_owned());
        }
        if let Some(base) = candidate.strip_suffix("-thinking") {
            push_model_candidate(&mut candidates, base.to_owned());
        }
        if let Some(base) = candidate.strip_suffix("@default") {
            push_model_candidate(&mut candidates, base.to_owned());
        }
    }
    candidates
}

fn push_model_candidate(candidates: &mut Vec<String>, candidate: String) {
    if !candidate.is_empty() && !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

#[cfg(test)]
fn claude_cost_micros(model: &str, usage: &TokenEntry) -> Option<u64> {
    let micros = claude_cost_micros_value(model, usage)?;
    (micros.is_finite() && micros >= 0.0 && micros <= u64::MAX as f64)
        .then(|| micros.round() as u64)
}

fn claude_cost_micros_value(model: &str, usage: &TokenEntry) -> Option<f64> {
    let price = claude_price(model)?;
    priced_cost_micros_value(
        usage.input,
        usage.output,
        usage.cache_read,
        usage.cache_creation,
        price,
    )
}

#[cfg(test)]
fn codex_cost_micros(model: &str, usage: CodexUsage) -> Option<u64> {
    let micros = codex_cost_micros_value(model, usage)?;
    (micros.is_finite() && micros >= 0.0 && micros <= u64::MAX as f64)
        .then(|| micros.round() as u64)
}

fn codex_cost_micros_value(model: &str, usage: CodexUsage) -> Option<f64> {
    let price = codex_price(model)?;
    priced_cost_micros_value(
        usage.input.saturating_sub(usage.cached),
        usage.output,
        usage.cached,
        0,
        price,
    )
}

fn priced_cost_micros_value(
    input: u64,
    output: u64,
    cache_read: u64,
    cache_create: u64,
    price: Price,
) -> Option<f64> {
    let micros = input as f64 * price.input
        + output as f64 * price.output
        + cache_read as f64 * price.cache_read
        + cache_create as f64 * price.cache_create;
    (micros.is_finite() && micros >= 0.0).then_some(micros)
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
        assert_eq!(record.model.as_deref(), Some("claude-sonnet-4"));
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
        assert_eq!(record.model.as_deref(), Some("gpt-5.6-sol"));
        assert_eq!(record.token_total, Some(800));
        assert_eq!(record.cache_read_tokens, Some(400));
        assert_eq!(record.reasoning_tokens, Some(20));
        assert_eq!(record.last_turn_tokens, Some(300));
        assert_eq!(record.context_used_tokens, Some(250));
        assert_eq!(record.context_used_percent, Some(25));
        assert_eq!(record.estimated_cost_usd_micros, Some(4_700));
        assert_eq!(
            record.pricing_source.as_deref(),
            Some("openai_standard_2026-07-20")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn codex_price_accumulates_each_model_delta_at_its_own_rate() {
        let root = temp_dir("codex-model-change");
        let path = root.join("rollout-session-model-change.jsonl");
        let fixture = concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"session-model-change\"}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-5.6-luna\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{",
            "\"total_token_usage\":{\"input_tokens\":1000,\"output_tokens\":0,",
            "\"total_tokens\":1000}}}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-5.6-sol\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{",
            "\"total_token_usage\":{\"input_tokens\":2000,\"output_tokens\":0,",
            "\"total_tokens\":2000}}}}\n"
        );
        fs::write(&path, fixture).expect("write fixture");
        let mut state = CodexFileState::default();
        parse_codex_tail(&path, &mut state, fixture.len() as u64);
        let record = codex_record(&state, 100).expect("record");
        assert_eq!(record.estimated_cost_usd_micros, Some(6_000));
        assert_eq!(
            record.pricing_source.as_deref(),
            Some("openai_standard_2026-07-20")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn claude_usage_compacts_old_entries_without_changing_totals() {
        let mut state = ClaudeFileState {
            session_id: Some("long-session".to_owned()),
            ..ClaudeFileState::default()
        };
        for index in 0..10_000 {
            upsert_claude_entry(
                &mut state,
                format!("message-{index}"),
                TokenEntry {
                    model: Some("claude-sonnet-4-6".to_owned()),
                    input: 1,
                    output: 2,
                    cache_read: 3,
                    cache_creation: 4,
                    timestamp: format!("{index:05}"),
                    ..TokenEntry::default()
                },
            );
        }
        assert_eq!(state.entries.len(), MAX_RECENT_CLAUDE_ENTRIES_PER_FILE);
        assert_eq!(state.entry_order.len(), MAX_RECENT_CLAUDE_ENTRIES_PER_FILE);
        assert_eq!(state.compacted.entry_count, 9_744);
        assert_eq!(state.recent.entry_count, 256);
        let record = claude_record(&state, 100).expect("record");
        assert_eq!(record.input_tokens, Some(10_000));
        assert_eq!(record.output_tokens, Some(20_000));
        assert_eq!(record.cache_read_tokens, Some(30_000));
        assert_eq!(record.cache_creation_tokens, Some(40_000));
        assert_eq!(record.token_total, Some(100_000));
        assert_eq!(record.last_turn_tokens, Some(10));
        assert_eq!(record.estimated_cost_usd_micros, Some(489_000));
    }

    #[test]
    fn recent_claude_replacement_updates_the_incremental_accumulator() {
        let mut state = ClaudeFileState {
            session_id: Some("replacement-session".to_owned()),
            ..ClaudeFileState::default()
        };
        upsert_claude_entry(
            &mut state,
            "message-1".to_owned(),
            TokenEntry {
                model: Some("claude-sonnet-4-6".to_owned()),
                input: 100,
                output: 5,
                timestamp: "1".to_owned(),
                ..TokenEntry::default()
            },
        );
        upsert_claude_entry(
            &mut state,
            "message-1".to_owned(),
            TokenEntry {
                model: Some("claude-sonnet-4-6".to_owned()),
                input: 100,
                output: 50,
                timestamp: "2".to_owned(),
                ..TokenEntry::default()
            },
        );
        let record = claude_record(&state, 100).expect("record");
        assert_eq!(state.entries.len(), 1);
        assert_eq!(state.recent.entry_count, 1);
        assert_eq!(record.token_total, Some(150));
        assert_eq!(record.estimated_cost_usd_micros, Some(1_050));
    }

    #[test]
    fn pricing_snapshot_matches_aliases_but_not_unknown_future_models() {
        assert_eq!(
            claude_price("anthropic/claude-opus-4.6"),
            Some(Price {
                input: 5.0,
                output: 25.0,
                cache_read: 0.5,
                cache_create: 6.25,
            })
        );
        assert_eq!(
            claude_price("claude-sonnet-4-6:thinking"),
            claude_price("claude-sonnet-4-6")
        );
        assert_eq!(claude_price("claude-opus-4-9"), None);
        assert!(claude_price("claude-3-opus-20240229").is_some());
        assert!(claude_price("claude-mythos-5").is_some());
        assert_eq!(claude_price("claude-mythos-preview"), None);
        assert_eq!(
            codex_price("openai/gpt-5.6-sol"),
            codex_price("gpt-5.6-sol")
        );
        assert_eq!(
            codex_price("gpt-5.2"),
            Some(Price {
                input: 1.75,
                output: 14.0,
                cache_read: 0.175,
                cache_create: 0.0,
            })
        );
        assert_eq!(codex_price("gpt-5.3"), None);
        assert!(codex_price("gpt-5.3-codex").is_some());
        assert_eq!(codex_price("gpt-5.6"), codex_price("gpt-5.6-sol"));
        assert!(codex_price("gpt-5.5").is_some());
        assert!(codex_price("gpt-5.4-mini").is_some());
        assert_eq!(codex_price("gpt-5.7"), None);
    }

    #[test]
    fn claude_sonnet_5_uses_dated_official_introductory_price() {
        let price = model_pricing("claude", "claude-sonnet-5").expect("known model");
        assert_eq!(price.source, "anthropic_intro_2026-07-20");
        assert_eq!(
            claude_price("claude-sonnet-5"),
            Some(Price {
                input: 2.0,
                output: 10.0,
                cache_read: 0.2,
                cache_create: 2.5,
            })
        );

        let usage = TokenEntry {
            input: 1_000_000,
            output: 1_000_000,
            cache_read: 1_000_000,
            cache_creation: 1_000_000,
            ..TokenEntry::default()
        };
        assert_eq!(
            claude_cost_micros("claude-sonnet-5", &usage),
            Some(14_700_000)
        );
    }

    #[test]
    fn zero_token_synthetic_entry_does_not_hide_known_model_cost() {
        let mut state = ClaudeFileState {
            session_id: Some("synthetic-session".to_owned()),
            ..ClaudeFileState::default()
        };
        upsert_claude_entry(
            &mut state,
            "real-message".to_owned(),
            TokenEntry {
                model: Some("claude-sonnet-5".to_owned()),
                input: 1_000_000,
                timestamp: "1".to_owned(),
                ..TokenEntry::default()
            },
        );
        upsert_claude_entry(
            &mut state,
            "synthetic-message".to_owned(),
            TokenEntry {
                model: Some("<synthetic>".to_owned()),
                timestamp: "2".to_owned(),
                ..TokenEntry::default()
            },
        );

        let record = claude_record(&state, 100).expect("record");
        assert_eq!(record.model.as_deref(), Some("claude-sonnet-5"));
        assert_eq!(record.estimated_cost_usd_micros, Some(2_000_000));
        assert_eq!(record.cost_kind.as_deref(), Some("computed"));
        assert_eq!(
            record.pricing_source.as_deref(),
            Some("anthropic_intro_2026-07-20")
        );
    }

    #[test]
    fn mixed_missing_official_cost_falls_back_to_complete_computed_cost() {
        let mut state = ClaudeFileState {
            session_id: Some("cost-session".to_owned()),
            ..ClaudeFileState::default()
        };
        for (index, official_cost) in [Some(100), None].into_iter().enumerate() {
            upsert_claude_entry(
                &mut state,
                format!("message-{index}"),
                TokenEntry {
                    model: Some("claude-haiku-4-5".to_owned()),
                    input: 100,
                    official_cost_usd_micros: official_cost,
                    timestamp: index.to_string(),
                    ..TokenEntry::default()
                },
            );
        }
        let record = claude_record(&state, 100).expect("record");
        assert_eq!(record.estimated_cost_usd_micros, Some(200));
        assert_eq!(record.cost_kind.as_deref(), Some("computed"));
        assert_eq!(
            record.pricing_source.as_deref(),
            Some("anthropic_standard_2026-07-20")
        );
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

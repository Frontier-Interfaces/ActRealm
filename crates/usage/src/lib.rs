//! Local, privacy-bounded session usage collection for Claude Code and Codex.
//!
//! The collector stores only numeric usage metadata. It never persists prompts,
//! tool input/output, transcript text, or provider credentials.

use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

const STATUS_CACHE_SCHEMA: u32 = 1;
const MAX_STATUSLINE_BYTES: u64 = 256 * 1_024;
// Keep one adversarial transcript record from inflating the long-lived
// Runtime allocator. The collector still advances through 10 MiB per refresh;
// records above the line cap are skipped and make history explicitly partial.
const MAX_JSONL_LINE_BYTES: u64 = 2 * 1_024 * 1_024;
const MAX_JSONL_BYTES_PER_REFRESH: u64 = 10 * 1_024 * 1_024;
const MAX_DISCOVERED_FILES: usize = 512;
const MAX_DISCOVERY_VISITED_ENTRIES: usize = 512;
const MAX_DISCOVERY_DURATION: Duration = Duration::from_millis(100);
const RECENT_FILE_AGE: Duration = Duration::from_secs(370 * 24 * 60 * 60);
const DISCOVERY_INTERVAL: Duration = Duration::from_secs(5);
const MAX_RECENT_CLAUDE_ENTRIES_PER_FILE: usize = 256;
const PRICING_SNAPSHOT_JSON: &str = include_str!("pricing_snapshot.json");
const MODELS_DEV_API_URL: &str = "https://models.dev/api.json";
const MODELS_DEV_CACHE_MAX_BYTES: u64 = 16 * 1_024 * 1_024;
// Schema 6 records response-keyed Codex usage and distinct resume fragments.
// Re-read older checkpoints rather than perpetuating their order-dependent daily rows.
const USAGE_SCAN_CHECKPOINT_SCHEMA: u32 = 6;
const MAX_CODEX_RESPONSE_ENTRIES: usize = 131_072;
const MAX_CODEX_RESPONSE_ENTRIES_PER_FILE: usize = 65_536;
const USAGE_SCAN_CHECKPOINT_MAX_BYTES: u64 = 64 * 1_024 * 1_024;
// A first historical scan can take minutes on a large Codex ledger, while the
// Runtime may legitimately restart when the official CLI rotates credentials.
// Persist bounded numeric parser state during the scan so that a restart loses
// at most one short slice instead of replaying the whole ledger. Atomic writes
// keep the previous checkpoint valid if the process exits between slices.
const USAGE_SCAN_CHECKPOINT_INTERVAL: Duration = Duration::from_secs(15);
const MODELS_DEV_REFRESH_INTERVAL: Duration = Duration::from_secs(60 * 60);
const MODELS_DEV_RETRY_INTERVAL: Duration = Duration::from_secs(5 * 60);
const MODELS_DEV_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PricingStatus {
    pub source: String,
    pub updated_at: Option<u64>,
    pub model_count: usize,
    pub updating: bool,
    pub refresh_failed: bool,
    pub automatic_interval_minutes: u32,
}

struct PricingUpdate {
    encoded: Vec<u8>,
    snapshot: PricingSnapshot,
}
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Error)]
pub enum UsageError {
    #[error("usage input exceeds {0} bytes")]
    TooLarge(u64),
    #[error("usage JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("usage I/O failed for {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("pricing catalog is invalid: {0}")]
    Pricing(String),
}

/// A partial session usage snapshot. Optional fields allow an official live
/// source (for example Claude StatusLine) to complement transcript totals
/// without changing the meaning of those totals.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRecord {
    pub provider: String,
    pub provider_session_id: String,
    /// SHA-256 identity of the normalized Provider working directory. The raw
    /// path is never retained in a record, checkpoint, database or export.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// Bounded final path component suitable for local project grouping.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_label: Option<String>,
    /// Opaque Provider session identity used to attribute child/subagent usage
    /// to a verified parent task without retaining transcript content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_provider_session_id: Option<String>,
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
    /// Numeric day/model aggregates parsed from the same local source. These
    /// contain no prompts, responses, commands, paths, or transcript content.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub daily_usage: Vec<UsageDailyRecord>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDailyRecord {
    pub day: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub reasoning_tokens: u64,
    pub token_total: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_cost_usd_micros: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pricing_source: Option<String>,
    pub message_count: u64,
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
        replace_some!(project_id);
        replace_some!(project_label);
        replace_some!(parent_provider_session_id);
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
        if !newer.daily_usage.is_empty() && (replace_existing || self.daily_usage.is_empty()) {
            self.daily_usage = newer.daily_usage;
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

    fn models_dev_pricing_cache(&self) -> PathBuf {
        self.actrealm_home.join("cache/models-dev-pricing.json")
    }

    fn usage_scan_checkpoint(&self) -> PathBuf {
        self.actrealm_home.join("cache/usage-scan-checkpoint.json")
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectIdentity {
    id: String,
    label: String,
}

/// Mirrors the useful part of Token Monitor's project decoration without
/// persisting the private working directory: hash the normalized full path and
/// retain only its bounded final component for presentation.
fn project_identity(value: &str) -> Option<ProjectIdentity> {
    let mut normalized = value.trim().replace('\\', "/");
    if normalized.is_empty() || normalized.len() > 4_096 || normalized.chars().any(char::is_control)
    {
        return None;
    }
    while normalized.len() > 1 && normalized.ends_with('/') {
        normalized.pop();
    }
    if normalized == "/" {
        return None;
    }
    #[cfg(target_os = "windows")]
    normalized.make_ascii_lowercase();
    let label = normalized.rsplit('/').find(|part| !part.is_empty())?;
    if label.len() > 128 || label.chars().any(char::is_control) {
        return None;
    }
    if let Some(repository) = project_identity_from_workspace(Path::new(&normalized)) {
        return Some(repository);
    }
    hashed_project_identity(&format!("path:{normalized}"), label)
}

fn hashed_project_identity(canonical: &str, label: &str) -> Option<ProjectIdentity> {
    if canonical.is_empty()
        || canonical.len() > 4_096
        || label.trim().is_empty()
        || label.len() > 128
        || canonical.chars().any(char::is_control)
        || label.chars().any(char::is_control)
    {
        return None;
    }
    let mut input = b"actrealm-project-v1\0".to_vec();
    input.extend_from_slice(canonical.as_bytes());
    let encoded = digest(&SHA256, &input)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Some(ProjectIdentity {
        id: format!("sha256:{encoded}"),
        label: label.to_owned(),
    })
}

fn project_identity_from_workspace(workspace: &Path) -> Option<ProjectIdentity> {
    const MAX_ENTRIES: usize = 32;
    const MAX_CONFIG_BYTES: u64 = 64 * 1_024;
    let work = workspace.join("work");
    let mut repositories = Vec::new();
    let entries = fs::read_dir(work).ok()?;
    for entry in entries.take(MAX_ENTRIES).flatten() {
        let path = entry.path();
        if !entry.file_type().ok().is_some_and(|kind| kind.is_dir()) {
            continue;
        }
        let config = path.join(".git/config");
        let Ok(metadata) = fs::symlink_metadata(&config) else {
            continue;
        };
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > MAX_CONFIG_BYTES
        {
            continue;
        }
        let Ok(mut file) = File::open(config) else {
            continue;
        };
        let mut encoded = Vec::new();
        if Read::by_ref(&mut file)
            .take(MAX_CONFIG_BYTES + 1)
            .read_to_end(&mut encoded)
            .is_err()
            || encoded.len() as u64 > MAX_CONFIG_BYTES
        {
            continue;
        }
        let Ok(text) = String::from_utf8(encoded) else {
            continue;
        };
        if let Some(repository) = origin_project_identity(&text) {
            repositories.push(repository);
        }
    }
    let mut unique = HashMap::<String, ProjectIdentity>::new();
    for repository in repositories {
        unique.entry(repository.id.clone()).or_insert(repository);
    }
    if unique.len() == 1 {
        return unique.into_values().next();
    }
    let workspace_tokens = workspace
        .file_name()
        .and_then(|value| value.to_str())?
        .to_ascii_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| {
            !token.is_empty() && !matches!(*token, "http" | "https" | "www" | "github" | "com")
        })
        .map(ToOwned::to_owned)
        .collect::<HashSet<_>>();
    let mut ranked = unique
        .into_values()
        .map(|repository| {
            let score = repository
                .label
                .to_ascii_lowercase()
                .split(|character: char| !character.is_ascii_alphanumeric())
                .filter(|token| workspace_tokens.contains(*token))
                .count();
            (score, repository)
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|value| std::cmp::Reverse(value.0));
    let best = ranked.first()?;
    let runner_up = ranked.get(1).map_or(0, |value| value.0);
    (best.0 > 0 && best.0 > runner_up).then(|| best.1.clone())
}

fn origin_project_identity(config: &str) -> Option<ProjectIdentity> {
    let mut in_origin = false;
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_origin = line.eq_ignore_ascii_case("[remote \"origin\"]");
            continue;
        }
        if !in_origin {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !key.trim().eq_ignore_ascii_case("url") {
            continue;
        }
        return git_remote_project_identity(value.trim());
    }
    None
}

fn git_remote_project_identity(remote: &str) -> Option<ProjectIdentity> {
    let mut value = remote.trim().replace('\\', "/");
    if value.is_empty() || value.len() > 2_048 || value.chars().any(char::is_control) {
        return None;
    }
    if let Some((_, tail)) = value.split_once("://") {
        value = tail.to_owned();
        if let Some((authority, path)) = value.split_once('/') {
            let host = authority.rsplit('@').next().unwrap_or(authority);
            value = format!("{host}/{path}");
        }
    } else if let Some((authority, path)) = value.split_once(':') {
        if authority.contains('@') && !path.starts_with('/') {
            let host = authority.rsplit('@').next().unwrap_or(authority);
            value = format!("{host}/{path}");
        }
    }
    value = value
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_owned();
    let label = value.rsplit('/').next()?.trim();
    if label.is_empty() {
        return None;
    }
    let display = label.to_owned();
    hashed_project_identity(&format!("git:{}", value.to_ascii_lowercase()), &display)
}

fn project_identity_from_value(value: &Value) -> Option<ProjectIdentity> {
    [
        "/cwd",
        "/project_path",
        "/projectPath",
        "/workingDirectory",
        "/working_directory",
        "/payload/cwd",
        "/payload/project_path",
        "/payload/projectPath",
        "/payload/workingDirectory",
        "/payload/working_directory",
    ]
    .into_iter()
    .find_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
    .and_then(project_identity)
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TokenAccumulator {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_creation: u64,
    entry_count: u64,
    official_cost_count: u64,
    official_cost_usd_micros: u64,
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
    }

    fn official_cost(&self) -> Option<u64> {
        (self.entry_count > 0 && self.official_cost_count == self.entry_count)
            .then_some(self.official_cost_usd_micros)
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
    identity: Option<(u64, u64)>,
    discarding_oversized_line: bool,
    history_complete: bool,
    session_id: Option<String>,
    project_id: Option<String>,
    project_label: Option<String>,
    compacted: TokenAccumulator,
    compacted_by_model: HashMap<String, TokenAccumulator>,
    recent: TokenAccumulator,
    entries: HashMap<String, TokenEntry>,
    entry_order: VecDeque<String>,
    latest_key: Option<String>,
    daily: HashMap<(String, String), TokenAccumulator>,
}

impl Default for ClaudeFileState {
    fn default() -> Self {
        Self {
            offset: 0,
            size: 0,
            modified: UNIX_EPOCH,
            identity: None,
            discarding_oversized_line: false,
            history_complete: true,
            session_id: None,
            project_id: None,
            project_label: None,
            compacted: TokenAccumulator::default(),
            compacted_by_model: HashMap::new(),
            recent: TokenAccumulator::default(),
            entries: HashMap::new(),
            entry_order: VecDeque::new(),
            latest_key: None,
            daily: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
struct CodexUsage {
    input: u64,
    cached: u64,
    output: u64,
    reasoning: u64,
    total: u64,
}

impl CodexUsage {
    fn is_empty(self) -> bool {
        self.input == 0
            && self.cached == 0
            && self.output == 0
            && self.reasoning == 0
            && self.total == 0
    }

    fn add_assign(&mut self, other: Self) {
        self.input = self.input.saturating_add(other.input);
        self.cached = self.cached.saturating_add(other.cached);
        self.output = self.output.saturating_add(other.output);
        self.reasoning = self.reasoning.saturating_add(other.reasoning);
        self.total = self.total.saturating_add(other.total);
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CodexDailyAccumulator {
    usage: CodexUsage,
    message_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CodexResponseUsage {
    at: u64,
    day: String,
    model: String,
    usage: CodexUsage,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CodexUsagePrefix {
    usage: CodexUsage,
    by_model: HashMap<String, CodexUsage>,
    daily: Vec<(String, String, CodexDailyAccumulator)>,
}

#[derive(Debug)]
struct CodexFileState {
    offset: u64,
    size: u64,
    modified: SystemTime,
    identity: Option<(u64, u64)>,
    discarding_oversized_line: bool,
    history_complete: bool,
    session_id: Option<String>,
    project_id: Option<String>,
    project_label: Option<String>,
    parent_provider_session_id: Option<String>,
    model: Option<String>,
    context_window: Option<u64>,
    /// Codex subagents and forked threads may begin with a replay of their
    /// parent's cumulative token history. That replay is a baseline for
    /// computing later deltas, not usage produced by this session.
    inherited_history: bool,
    inherited_history_cutoff: Option<u64>,
    cumulative: Option<CodexUsage>,
    accounted: CodexUsage,
    incremental: CodexUsage,
    last: Option<CodexUsage>,
    by_model: HashMap<String, CodexUsage>,
    daily: HashMap<(String, String), CodexDailyAccumulator>,
    fragment_started_at: Option<u64>,
    last_usage_at: Option<u64>,
    pending_compaction: bool,
    response_prefix: Option<CodexUsagePrefix>,
    responses: HashMap<String, CodexResponseUsage>,
}

#[derive(Debug, Deserialize)]
struct CodexLineKind {
    #[serde(rename = "type")]
    event_type: Option<String>,
    payload: Option<CodexPayloadKind>,
}

#[derive(Debug, Deserialize)]
struct CodexPayloadKind {
    #[serde(rename = "type")]
    payload_type: Option<String>,
}

impl CodexLineKind {
    fn carries_usage_metadata(&self) -> bool {
        match self.event_type.as_deref() {
            Some("session_meta" | "turn_context" | "token_usage_record" | "compacted") => true,
            Some("event_msg") => {
                self.payload
                    .as_ref()
                    .and_then(|payload| payload.payload_type.as_deref())
                    == Some("token_count")
            }
            _ => false,
        }
    }
}

/// Codex rollout records are serialized with bounded top-level type metadata
/// before their potentially large payloads. For oversized records we only
/// preserve complete-history status when that prefix proves the record belongs
/// to a version-gated, non-usage shape. Unknown, malformed and usage-bearing
/// prefixes continue to fail closed as partial.
fn codex_oversized_prefix_is_known_non_usage(prefix: &[u8]) -> bool {
    let Some(top_level) = json_prefix_string_field(prefix, 0, b"type") else {
        return false;
    };
    match top_level {
        b"response_item" | b"compacted" | b"world_state" => true,
        b"event_msg" => {
            json_prefix_object_field_start(prefix, 0, b"payload").is_some_and(|payload| {
                match json_prefix_string_field(prefix, payload, b"type") {
                    Some(b"image_generation_end" | b"user_message" | b"agent_message") => true,
                    Some(b"item_completed") => {
                        json_prefix_object_field_start(prefix, payload, b"item")
                            .and_then(|item| json_prefix_string_field(prefix, item, b"type"))
                            .is_some_and(|kind| {
                                matches!(
                                    kind,
                                    b"CommandExecution"
                                        | b"McpToolCall"
                                        | b"AgentMessage"
                                        | b"UserMessage"
                                )
                            })
                    }
                    _ => false,
                }
            })
        }
        _ => false,
    }
}

fn json_prefix_string_field<'a>(
    input: &'a [u8],
    object_start: usize,
    field: &[u8],
) -> Option<&'a [u8]> {
    let mut value_start = json_prefix_field_start(input, object_start, field)?;
    parse_plain_json_string(input, &mut value_start)
}

fn json_prefix_object_field_start(
    input: &[u8],
    object_start: usize,
    field: &[u8],
) -> Option<usize> {
    let value_start = json_prefix_field_start(input, object_start, field)?;
    (input.get(value_start) == Some(&b'{')).then_some(value_start)
}

fn json_prefix_field_start(input: &[u8], object_start: usize, field: &[u8]) -> Option<usize> {
    let mut cursor = object_start;
    skip_json_whitespace(input, &mut cursor);
    if input.get(cursor) != Some(&b'{') {
        return None;
    }
    cursor += 1;
    loop {
        skip_json_whitespace(input, &mut cursor);
        if input.get(cursor) == Some(&b'}') {
            return None;
        }
        let key = parse_plain_json_string(input, &mut cursor)?;
        skip_json_whitespace(input, &mut cursor);
        if input.get(cursor) != Some(&b':') {
            return None;
        }
        cursor += 1;
        skip_json_whitespace(input, &mut cursor);
        if key == field {
            return Some(cursor);
        }
        skip_json_value_prefix(input, &mut cursor, 0)?;
        skip_json_whitespace(input, &mut cursor);
        match input.get(cursor) {
            Some(b',') => cursor += 1,
            Some(b'}') | None => return None,
            _ => return None,
        }
    }
}

fn parse_plain_json_string<'a>(input: &'a [u8], cursor: &mut usize) -> Option<&'a [u8]> {
    if input.get(*cursor) != Some(&b'"') {
        return None;
    }
    *cursor += 1;
    let start = *cursor;
    while let Some(byte) = input.get(*cursor) {
        match byte {
            b'"' => {
                let value = &input[start..*cursor];
                *cursor += 1;
                return Some(value);
            }
            b'\\' | 0x00..=0x1f => return None,
            _ => *cursor += 1,
        }
    }
    None
}

fn skip_json_string_prefix(input: &[u8], cursor: &mut usize) -> Option<()> {
    if input.get(*cursor) != Some(&b'"') {
        return None;
    }
    *cursor += 1;
    while let Some(byte) = input.get(*cursor) {
        match byte {
            b'"' => {
                *cursor += 1;
                return Some(());
            }
            b'\\' => {
                *cursor += 1;
                let escaped = *input.get(*cursor)?;
                *cursor += 1;
                if escaped == b'u' {
                    for _ in 0..4 {
                        if !input.get(*cursor)?.is_ascii_hexdigit() {
                            return None;
                        }
                        *cursor += 1;
                    }
                }
            }
            0x00..=0x1f => return None,
            _ => *cursor += 1,
        }
    }
    None
}

fn skip_json_value_prefix(input: &[u8], cursor: &mut usize, depth: usize) -> Option<()> {
    if depth > 16 {
        return None;
    }
    skip_json_whitespace(input, cursor);
    match *input.get(*cursor)? {
        b'"' => skip_json_string_prefix(input, cursor),
        b'{' => {
            *cursor += 1;
            skip_json_whitespace(input, cursor);
            if input.get(*cursor) == Some(&b'}') {
                *cursor += 1;
                return Some(());
            }
            loop {
                parse_plain_json_string(input, cursor)?;
                skip_json_whitespace(input, cursor);
                if input.get(*cursor) != Some(&b':') {
                    return None;
                }
                *cursor += 1;
                skip_json_value_prefix(input, cursor, depth + 1)?;
                skip_json_whitespace(input, cursor);
                match input.get(*cursor) {
                    Some(b',') => *cursor += 1,
                    Some(b'}') => {
                        *cursor += 1;
                        return Some(());
                    }
                    _ => return None,
                }
            }
        }
        b'[' => {
            *cursor += 1;
            skip_json_whitespace(input, cursor);
            if input.get(*cursor) == Some(&b']') {
                *cursor += 1;
                return Some(());
            }
            loop {
                skip_json_value_prefix(input, cursor, depth + 1)?;
                skip_json_whitespace(input, cursor);
                match input.get(*cursor) {
                    Some(b',') => *cursor += 1,
                    Some(b']') => {
                        *cursor += 1;
                        return Some(());
                    }
                    _ => return None,
                }
            }
        }
        b't' if input.get(*cursor..(*cursor + 4)) == Some(b"true") => {
            *cursor += 4;
            Some(())
        }
        b'f' if input.get(*cursor..(*cursor + 5)) == Some(b"false") => {
            *cursor += 5;
            Some(())
        }
        b'n' if input.get(*cursor..(*cursor + 4)) == Some(b"null") => {
            *cursor += 4;
            Some(())
        }
        b'-' | b'0'..=b'9' => {
            *cursor += 1;
            while input.get(*cursor).is_some_and(|byte| {
                byte.is_ascii_digit() || matches!(byte, b'.' | b'e' | b'E' | b'+' | b'-')
            }) {
                *cursor += 1;
            }
            Some(())
        }
        _ => None,
    }
}

fn skip_json_whitespace(input: &[u8], cursor: &mut usize) {
    while input
        .get(*cursor)
        .is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t'))
    {
        *cursor += 1;
    }
}

impl Default for CodexFileState {
    fn default() -> Self {
        Self {
            offset: 0,
            size: 0,
            modified: UNIX_EPOCH,
            identity: None,
            discarding_oversized_line: false,
            history_complete: true,
            session_id: None,
            project_id: None,
            project_label: None,
            parent_provider_session_id: None,
            model: None,
            context_window: None,
            inherited_history: false,
            inherited_history_cutoff: None,
            cumulative: None,
            accounted: CodexUsage::default(),
            incremental: CodexUsage::default(),
            last: None,
            by_model: HashMap::new(),
            daily: HashMap::new(),
            fragment_started_at: None,
            last_usage_at: None,
            pending_compaction: false,
            response_prefix: None,
            responses: HashMap::new(),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageCollectorCheckpoint {
    schema_version: u32,
    #[serde(default)]
    claude: Vec<ClaudeFileCheckpoint>,
    #[serde(default)]
    codex: Vec<CodexFileCheckpoint>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeDailyCheckpoint {
    day: String,
    model: String,
    value: TokenAccumulator,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeFileCheckpoint {
    source_key: String,
    offset: u64,
    size: u64,
    modified_ms: u64,
    identity: Option<(u64, u64)>,
    discarding_oversized_line: bool,
    history_complete: bool,
    session_id: Option<String>,
    project_id: Option<String>,
    project_label: Option<String>,
    compacted: TokenAccumulator,
    compacted_by_model: HashMap<String, TokenAccumulator>,
    recent: TokenAccumulator,
    entries: HashMap<String, TokenEntry>,
    entry_order: VecDeque<String>,
    latest_key: Option<String>,
    daily: Vec<ClaudeDailyCheckpoint>,
}

impl ClaudeFileCheckpoint {
    fn from_state(source_key: String, state: &ClaudeFileState) -> Self {
        let mut daily = state
            .daily
            .iter()
            .map(|((day, model), value)| ClaudeDailyCheckpoint {
                day: day.clone(),
                model: model.clone(),
                value: value.clone(),
            })
            .collect::<Vec<_>>();
        daily.sort_by(|left, right| {
            left.day
                .cmp(&right.day)
                .then_with(|| left.model.cmp(&right.model))
        });
        Self {
            source_key,
            offset: state.offset,
            size: state.size,
            modified_ms: system_time_millis(state.modified).unwrap_or_default(),
            identity: state.identity,
            discarding_oversized_line: state.discarding_oversized_line,
            history_complete: state.history_complete,
            session_id: state.session_id.clone(),
            project_id: state.project_id.clone(),
            project_label: state.project_label.clone(),
            compacted: state.compacted.clone(),
            compacted_by_model: state.compacted_by_model.clone(),
            recent: state.recent.clone(),
            entries: state.entries.clone(),
            entry_order: state.entry_order.clone(),
            latest_key: state.latest_key.clone(),
            daily,
        }
    }

    fn into_state(self) -> ClaudeFileState {
        ClaudeFileState {
            offset: self.offset,
            size: self.size,
            modified: UNIX_EPOCH + Duration::from_millis(self.modified_ms),
            identity: self.identity,
            discarding_oversized_line: self.discarding_oversized_line,
            history_complete: self.history_complete,
            session_id: self.session_id,
            project_id: self.project_id,
            project_label: self.project_label,
            compacted: self.compacted,
            compacted_by_model: self.compacted_by_model,
            recent: self.recent,
            entries: self.entries,
            entry_order: self.entry_order,
            latest_key: self.latest_key,
            daily: self
                .daily
                .into_iter()
                .map(|entry| ((entry.day, entry.model), entry.value))
                .collect(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexDailyCheckpoint {
    day: String,
    model: String,
    value: CodexDailyAccumulator,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexFileCheckpoint {
    source_key: String,
    offset: u64,
    size: u64,
    modified_ms: u64,
    identity: Option<(u64, u64)>,
    discarding_oversized_line: bool,
    history_complete: bool,
    session_id: Option<String>,
    project_id: Option<String>,
    project_label: Option<String>,
    parent_provider_session_id: Option<String>,
    model: Option<String>,
    context_window: Option<u64>,
    inherited_history: bool,
    inherited_history_cutoff: Option<u64>,
    cumulative: Option<CodexUsage>,
    accounted: CodexUsage,
    incremental: CodexUsage,
    last: Option<CodexUsage>,
    by_model: HashMap<String, CodexUsage>,
    daily: Vec<CodexDailyCheckpoint>,
    fragment_started_at: Option<u64>,
    last_usage_at: Option<u64>,
    pending_compaction: bool,
    response_prefix: Option<CodexUsagePrefix>,
    responses: HashMap<String, CodexResponseUsage>,
}

impl CodexFileCheckpoint {
    fn from_state(source_key: String, state: &CodexFileState) -> Self {
        let mut daily = state
            .daily
            .iter()
            .map(|((day, model), value)| CodexDailyCheckpoint {
                day: day.clone(),
                model: model.clone(),
                value: value.clone(),
            })
            .collect::<Vec<_>>();
        daily.sort_by(|left, right| {
            left.day
                .cmp(&right.day)
                .then_with(|| left.model.cmp(&right.model))
        });
        Self {
            source_key,
            offset: state.offset,
            size: state.size,
            modified_ms: system_time_millis(state.modified).unwrap_or_default(),
            identity: state.identity,
            discarding_oversized_line: state.discarding_oversized_line,
            history_complete: state.history_complete,
            session_id: state.session_id.clone(),
            project_id: state.project_id.clone(),
            project_label: state.project_label.clone(),
            parent_provider_session_id: state.parent_provider_session_id.clone(),
            model: state.model.clone(),
            context_window: state.context_window,
            inherited_history: state.inherited_history,
            inherited_history_cutoff: state.inherited_history_cutoff,
            cumulative: state.cumulative,
            accounted: state.accounted,
            incremental: state.incremental,
            last: state.last,
            by_model: state.by_model.clone(),
            daily,
            fragment_started_at: state.fragment_started_at,
            last_usage_at: state.last_usage_at,
            pending_compaction: state.pending_compaction,
            response_prefix: state.response_prefix.clone(),
            responses: state.responses.clone(),
        }
    }

    fn into_state(self) -> CodexFileState {
        CodexFileState {
            fragment_started_at: self.fragment_started_at,
            last_usage_at: self.last_usage_at,
            pending_compaction: self.pending_compaction,
            response_prefix: self.response_prefix,
            responses: self.responses,
            offset: self.offset,
            size: self.size,
            modified: UNIX_EPOCH + Duration::from_millis(self.modified_ms),
            identity: self.identity,
            discarding_oversized_line: self.discarding_oversized_line,
            history_complete: self.history_complete,
            session_id: self.session_id,
            project_id: self.project_id,
            project_label: self.project_label,
            parent_provider_session_id: self.parent_provider_session_id,
            model: self.model,
            context_window: self.context_window,
            inherited_history: self.inherited_history,
            inherited_history_cutoff: self.inherited_history_cutoff,
            cumulative: self.cumulative,
            accounted: self.accounted,
            incremental: self.incremental,
            last: self.last,
            by_model: self.by_model,
            daily: self
                .daily
                .into_iter()
                .map(|entry| ((entry.day, entry.model), entry.value))
                .collect(),
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
    next_usage_file_index: usize,
    last_discovery: Option<Instant>,
    line_buffer: Vec<u8>,
    pricing: PricingSnapshot,
    live_pricing_enabled: bool,
    pricing_last_attempt: Option<Instant>,
    pricing_pending: Option<mpsc::Receiver<Result<PricingUpdate, UsageError>>>,
    pricing_refresh_failed: bool,
    pricing_force_refresh: bool,
    checkpoint_claude: HashMap<String, ClaudeFileCheckpoint>,
    checkpoint_codex: HashMap<String, CodexFileCheckpoint>,
    last_checkpoint_signature: Option<u64>,
    last_checkpoint_persisted_at: Option<Instant>,
}

impl UsageCollector {
    pub fn new(paths: UsagePaths) -> Self {
        let cached_pricing = load_models_dev_cache(&paths);
        let checkpoint = load_usage_scan_checkpoint(&paths).unwrap_or_default();
        Self {
            paths,
            claude_files: HashMap::new(),
            codex_files: HashMap::new(),
            known_claude: Vec::new(),
            known_codex: Vec::new(),
            next_usage_file_index: 0,
            last_discovery: None,
            line_buffer: Vec::new(),
            pricing: cached_pricing.unwrap_or_else(embedded_pricing_snapshot),
            live_pricing_enabled: false,
            pricing_last_attempt: None,
            pricing_pending: None,
            pricing_refresh_failed: false,
            pricing_force_refresh: false,
            checkpoint_claude: checkpoint
                .claude
                .into_iter()
                .map(|source| (source.source_key.clone(), source))
                .collect(),
            checkpoint_codex: checkpoint
                .codex
                .into_iter()
                .map(|source| (source.source_key.clone(), source))
                .collect(),
            last_checkpoint_signature: None,
            last_checkpoint_persisted_at: None,
        }
    }

    /// Enables the fixed-endpoint models.dev catalog refresh. The Runtime is
    /// deliberately the only production caller; tests and standalone parsers
    /// remain deterministic and offline unless they opt in explicitly.
    pub fn enable_live_pricing(&mut self) {
        self.live_pricing_enabled = true;
    }

    pub fn live_pricing_enabled(&self) -> bool {
        self.live_pricing_enabled
    }

    pub fn request_pricing_refresh(&mut self) {
        self.pricing_force_refresh = true;
        self.live_pricing_enabled = true;
    }

    pub fn pricing_status(&self) -> PricingStatus {
        let updated_at = fs::metadata(self.paths.models_dev_pricing_cache())
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .and_then(|d| u64::try_from(d.as_millis()).ok());
        PricingStatus {
            source: self.pricing.source.clone(),
            updated_at,
            model_count: self.pricing.models.len(),
            updating: self.pricing_pending.is_some() || self.pricing_force_refresh,
            refresh_failed: self.pricing_refresh_failed,
            automatic_interval_minutes: 60,
        }
    }

    pub fn discover() -> Self {
        Self::new(UsagePaths::discover())
    }

    pub fn paths(&self) -> &UsagePaths {
        &self.paths
    }

    /// Returns true only when every currently discovered source has been read
    /// through its present end-of-file. A successful bounded collection pass
    /// is not necessarily a complete historical rebuild.
    pub fn is_caught_up(&self) -> bool {
        self.last_discovery.is_some()
            && self.known_claude.iter().all(|path| {
                self.claude_files.get(path).is_some_and(|state| {
                    source_is_caught_up(
                        path,
                        state.offset,
                        state.identity,
                        state.discarding_oversized_line,
                    )
                })
            })
            && self.known_codex.iter().all(|path| {
                self.codex_files.get(path).is_some_and(|state| {
                    source_is_caught_up(
                        path,
                        state.offset,
                        state.identity,
                        state.discarding_oversized_line,
                    )
                })
            })
    }

    /// Returns true only when every discovered source was read from a trusted
    /// beginning without skipping an oversized file or line. Reaching EOF and
    /// having complete history are separate facts: the former stops the
    /// rebuild spinner, while the latter is required before data is verified.
    pub fn is_history_complete(&self) -> bool {
        self.last_discovery.is_some()
            && self.known_claude.iter().all(|path| {
                self.claude_files
                    .get(path)
                    .is_some_and(|state| state.history_complete)
            })
            && self.known_codex.iter().all(|path| {
                self.codex_files
                    .get(path)
                    .is_some_and(|state| state.history_complete)
            })
    }

    /// Sessions whose entire known local source has been parsed. A ready active
    /// session need not wait for unrelated historical files to reach EOF.
    pub fn ready_codex_session_ids(&self) -> HashSet<String> {
        let mut readiness = HashMap::<String, bool>::new();
        for (path, state) in &self.codex_files {
            let Some(id) = state.session_id.as_ref() else {
                continue;
            };
            let ready = state.history_complete
                && source_is_caught_up(
                    path,
                    state.offset,
                    state.identity,
                    state.discarding_oversized_line,
                );
            readiness
                .entry(id.clone())
                .and_modify(|value| *value &= ready)
                .or_insert(ready);
        }
        readiness
            .into_iter()
            .filter_map(|(id, ready)| ready.then_some(id))
            .collect()
    }

    /// Private local source inventory for the Runtime's transient question observer.
    pub fn codex_source_files(&self) -> Vec<PathBuf> {
        self.known_codex.clone()
    }

    pub fn collect(&mut self, now_ms: u64) -> Vec<UsageRecord> {
        self.collect_with_shutdown(now_ms, None)
    }

    /// Collects locally derived usage until `shutdown` is requested. The
    /// collector checks between bounded JSONL reads, so a shutdown cannot be
    /// held by an unbounded source line.
    pub fn collect_until_shutdown(
        &mut self,
        now_ms: u64,
        shutdown: &AtomicBool,
    ) -> Vec<UsageRecord> {
        self.collect_with_shutdown(now_ms, Some(shutdown))
    }

    fn collect_with_shutdown(
        &mut self,
        now_ms: u64,
        shutdown: Option<&AtomicBool>,
    ) -> Vec<UsageRecord> {
        if collection_cancelled(shutdown) {
            return Vec::new();
        }
        // Network work is independent of indexing and never blocks usage or
        // question observation. Continue using the last validated rates.
        if self.live_pricing_enabled {
            self.refresh_models_dev_pricing_if_due(shutdown);
        }
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
            self.restore_checkpoint_states();
        }

        let known_files = self
            .known_claude
            .iter()
            .cloned()
            .map(|path| (true, path))
            .chain(self.known_codex.iter().cloned().map(|path| (false, path)))
            .collect::<Vec<_>>();
        if known_files.is_empty() {
            self.next_usage_file_index = 0;
        } else {
            let start = self.next_usage_file_index % known_files.len();
            let mut remaining_bytes = MAX_JSONL_BYTES_PER_REFRESH;
            // Reuse one bounded allocation across files and refreshes. Creating
            // a fresh multi-megabyte Vec for every JSONL source causes the
            // macOS allocator to retain several freed size classes.
            let mut line_buffer = std::mem::take(&mut self.line_buffer);
            let occupied = self
                .codex_files
                .values()
                .map(|state| state.responses.len())
                .sum::<usize>();
            let mut response_slots = MAX_CODEX_RESPONSE_ENTRIES.saturating_sub(occupied);
            let mut scan_cancelled = false;
            for turn in 0..known_files.len() {
                if remaining_bytes == 0 {
                    break;
                }
                if collection_cancelled(shutdown) {
                    scan_cancelled = true;
                    break;
                }
                let (is_claude, path) = &known_files[(start + turn) % known_files.len()];
                if *is_claude {
                    self.refresh_claude_file(
                        path,
                        shutdown,
                        &mut remaining_bytes,
                        &mut line_buffer,
                    );
                } else {
                    self.refresh_codex_file(
                        path,
                        shutdown,
                        &mut remaining_bytes,
                        &mut line_buffer,
                        &mut response_slots,
                    );
                }
            }
            self.line_buffer = line_buffer;
            if scan_cancelled {
                return Vec::new();
            }
            // Rotate one source each collection so a file that consumes the
            // shared budget cannot permanently starve later known sources.
            self.next_usage_file_index = (start + 1) % known_files.len();
        }
        if collection_cancelled(shutdown) {
            return Vec::new();
        }

        let mut records = HashMap::<(String, String), UsageRecord>::new();
        for record in claude_records(self.claude_files.values(), now_ms, &self.pricing) {
            merge_record(&mut records, record);
        }
        for record in codex_records(self.codex_files.values(), now_ms, &self.pricing) {
            merge_record(&mut records, record);
        }
        for record in read_status_caches(&self.paths.claude_status_cache_dir()) {
            merge_record(&mut records, record);
        }
        let output = records.into_values().collect::<Vec<_>>();
        // Persist bounded backfill progress as well as a completed scan. A
        // large local ledger can take longer than a Provider credential
        // rotation or an intentional Runtime restart; waiting until EOF made
        // those restarts discard the entire first scan. The private
        // checkpoint contains numeric parser state and source hashes only.
        self.persist_scan_checkpoint_if_needed();
        output
    }

    fn restore_checkpoint_states(&mut self) {
        for path in self.known_claude.clone() {
            if self.claude_files.contains_key(&path) {
                continue;
            }
            let key = usage_source_key("claude", &path);
            let Some(checkpoint) = self.checkpoint_claude.remove(&key) else {
                continue;
            };
            if usage_checkpoint_source_matches(
                &path,
                checkpoint.offset,
                checkpoint.modified_ms,
                checkpoint.identity,
            ) {
                self.claude_files.insert(path, checkpoint.into_state());
            }
        }
        for path in self.known_codex.clone() {
            if self.codex_files.contains_key(&path) {
                continue;
            }
            let key = usage_source_key("codex", &path);
            let Some(checkpoint) = self.checkpoint_codex.remove(&key) else {
                continue;
            };
            if usage_checkpoint_source_matches(
                &path,
                checkpoint.offset,
                checkpoint.modified_ms,
                checkpoint.identity,
            ) {
                self.codex_files.insert(path, checkpoint.into_state());
            }
        }
    }

    /// Flush only private numeric parser state at a controlled shutdown/audit boundary.
    pub fn flush_scan_checkpoint(&mut self) {
        self.last_checkpoint_persisted_at = None;
        self.persist_scan_checkpoint_if_needed();
    }

    fn persist_scan_checkpoint_if_needed(&mut self) {
        let signature = usage_checkpoint_signature(&self.claude_files, &self.codex_files);
        if self.last_checkpoint_signature == Some(signature) {
            return;
        }
        if self
            .last_checkpoint_persisted_at
            .is_some_and(|last| last.elapsed() < USAGE_SCAN_CHECKPOINT_INTERVAL)
        {
            return;
        }
        let mut checkpoint = UsageCollectorCheckpoint {
            schema_version: USAGE_SCAN_CHECKPOINT_SCHEMA,
            claude: self
                .known_claude
                .iter()
                .filter_map(|path| {
                    self.claude_files.get(path).map(|state| {
                        ClaudeFileCheckpoint::from_state(usage_source_key("claude", path), state)
                    })
                })
                .collect(),
            codex: self
                .known_codex
                .iter()
                .filter_map(|path| {
                    self.codex_files.get(path).map(|state| {
                        CodexFileCheckpoint::from_state(usage_source_key("codex", path), state)
                    })
                })
                .collect(),
        };
        checkpoint
            .claude
            .sort_by(|left, right| left.source_key.cmp(&right.source_key));
        checkpoint
            .codex
            .sort_by(|left, right| left.source_key.cmp(&right.source_key));
        let Ok(encoded) = serde_json::to_vec(&checkpoint) else {
            return;
        };
        if encoded.len() as u64 > USAGE_SCAN_CHECKPOINT_MAX_BYTES {
            return;
        }
        if atomic_write(&self.paths.usage_scan_checkpoint(), &encoded, 0o600).is_ok() {
            self.last_checkpoint_signature = Some(signature);
            self.last_checkpoint_persisted_at = Some(Instant::now());
        }
    }

    fn refresh_models_dev_pricing_if_due(&mut self, shutdown: Option<&AtomicBool>) {
        if collection_cancelled(shutdown) {
            return;
        }
        if let Some(pending) = self.pricing_pending.as_ref() {
            match pending.try_recv() {
                Ok(Ok(update)) => {
                    self.pricing_pending = None;
                    if atomic_write(
                        &self.paths.models_dev_pricing_cache(),
                        &update.encoded,
                        0o600,
                    )
                    .is_ok()
                    {
                        self.pricing = update.snapshot;
                        self.pricing_refresh_failed = false;
                    } else {
                        self.pricing_refresh_failed = true;
                    }
                }
                Ok(Err(_)) | Err(mpsc::TryRecvError::Disconnected) => {
                    self.pricing_pending = None;
                    self.pricing_refresh_failed = true;
                }
                Err(mpsc::TryRecvError::Empty) => return,
            }
        }
        let missing_model = self
            .codex_files
            .values()
            .filter_map(|s| s.model.as_deref())
            .any(|m| model_price_in(&self.pricing, "codex", m).is_none())
            || self
                .claude_files
                .values()
                .flat_map(|s| {
                    s.compacted_by_model
                        .keys()
                        .chain(s.entries.values().filter_map(|e| e.model.as_ref()))
                })
                .filter(|m| !m.is_empty() && m.as_str() != "<synthetic>")
                .any(|m| model_price_in(&self.pricing, "claude", m).is_none());
        if !pricing_refresh_due(
            cache_is_fresh(&self.paths.models_dev_pricing_cache()),
            missing_model,
            self.pricing_last_attempt.map(|t| t.elapsed()),
            self.pricing_force_refresh,
        ) {
            return;
        }
        self.pricing_force_refresh = false;
        self.pricing_last_attempt = Some(Instant::now());
        let (sender, receiver) = mpsc::sync_channel(1);
        match std::thread::Builder::new()
            .name("actrealm-pricing".into())
            .spawn(move || {
                let result = fetch_models_dev_catalog().and_then(|encoded| {
                    let snapshot = models_dev_pricing_snapshot(&encoded)?;
                    Ok(PricingUpdate { encoded, snapshot })
                });
                let _ = sender.send(result);
            }) {
            Ok(_) => self.pricing_pending = Some(receiver),
            Err(_) => self.pricing_refresh_failed = true,
        }
    }

    fn refresh_claude_file(
        &mut self,
        path: &Path,
        shutdown: Option<&AtomicBool>,
        remaining_bytes: &mut u64,
        line_buffer: &mut Vec<u8>,
    ) {
        if collection_cancelled(shutdown) || *remaining_bytes == 0 {
            return;
        }
        let Ok(metadata) = regular_file_metadata(path) else {
            return;
        };
        let size = metadata.len();
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        let identity = (metadata.dev(), metadata.ino());
        let state = self.claude_files.entry(path.to_path_buf()).or_default();
        if size == state.size
            && modified == state.modified
            && state.identity == Some(identity)
            && state.offset >= size
        {
            return;
        }
        // Device/inode identity catches atomic rename replacement even when
        // size and mtime collide. An in-place truncate-and-regrow that reaches
        // the previous offset between polls has the same identity and remains
        // intentionally ambiguous; rewinding there could double-count usage.
        if size < state.offset || state.identity.is_some_and(|previous| previous != identity) {
            *state = ClaudeFileState::default();
        }
        parse_claude_tail_with_shutdown(path, state, size, shutdown, remaining_bytes, line_buffer);
        state.size = size;
        state.modified = modified;
        state.identity = Some(identity);
    }

    fn refresh_codex_file(
        &mut self,
        path: &Path,
        shutdown: Option<&AtomicBool>,
        remaining_bytes: &mut u64,
        line_buffer: &mut Vec<u8>,
        response_slots: &mut usize,
    ) {
        if collection_cancelled(shutdown) || *remaining_bytes == 0 {
            return;
        }
        let Ok(metadata) = regular_file_metadata(path) else {
            return;
        };
        let size = metadata.len();
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        let identity = (metadata.dev(), metadata.ino());
        let state = self.codex_files.entry(path.to_path_buf()).or_default();
        if size == state.size
            && modified == state.modified
            && state.identity == Some(identity)
            && state.offset >= size
        {
            return;
        }
        // See the Claude path above for the unavoidable in-place
        // truncate-and-regrow limitation between refreshes.
        if size < state.offset || state.identity.is_some_and(|previous| previous != identity) {
            *response_slots = response_slots
                .saturating_add(state.responses.len())
                .min(MAX_CODEX_RESPONSE_ENTRIES);
            *state = CodexFileState::default();
        }
        parse_codex_tail_with_shutdown(
            path,
            state,
            size,
            shutdown,
            remaining_bytes,
            line_buffer,
            response_slots,
        );
        state.size = size;
        state.modified = modified;
        state.identity = Some(identity);
    }
}

fn source_is_caught_up(
    path: &Path,
    offset: u64,
    identity: Option<(u64, u64)>,
    discarding_oversized_line: bool,
) -> bool {
    let Ok(metadata) = regular_file_metadata(path) else {
        return false;
    };
    identity == Some((metadata.dev(), metadata.ino()))
        && offset >= metadata.len()
        && !discarding_oversized_line
}

fn collection_cancelled(shutdown: Option<&AtomicBool>) -> bool {
    shutdown.is_some_and(|flag| flag.load(Ordering::Acquire))
}

fn claude_records<'a>(
    states: impl Iterator<Item = &'a ClaudeFileState>,
    now_ms: u64,
    pricing: &PricingSnapshot,
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
        group.history_complete &= state.history_complete;
        if state.project_id.is_some() {
            group.project_id.clone_from(&state.project_id);
            group.project_label.clone_from(&state.project_label);
        }
        group.compacted.add_accumulator(&state.compacted);
        for (model, value) in &state.compacted_by_model {
            group
                .compacted_by_model
                .entry(model.clone())
                .or_default()
                .add_accumulator(value);
        }
        for (key, value) in &state.daily {
            group
                .daily
                .entry(key.clone())
                .or_default()
                .add_accumulator(value);
        }
        for key in &state.entry_order {
            if let Some(entry) = state.entries.get(key) {
                upsert_claude_entry_with_daily(group, key.clone(), entry.clone(), false);
            }
        }
    }
    grouped
        .values()
        .filter_map(|state| claude_record_with_pricing(state, now_ms, pricing))
        .collect()
}

fn mark_codex_history_partial(state: &mut CodexFileState) {
    if state.history_complete && state.response_prefix.is_none() {
        state.incremental.add_assign(state.accounted);
        state.accounted = CodexUsage::default();
    }
    state.history_complete = false;
}

fn observe_codex_response(
    state: &mut CodexFileState,
    payload: &Value,
    at: Option<u64>,
    day: Option<&str>,
    slots: &mut usize,
) {
    // Replayed parent/fork records belong to their original thread.
    let Some(thread_id) = state.session_id.as_deref() else {
        return;
    };
    if payload.get("thread_id").and_then(Value::as_str) != Some(thread_id) {
        return;
    }
    let Some(id) = payload
        .get("response_id")
        .and_then(Value::as_str)
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 160
                && id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
        })
    else {
        mark_codex_history_partial(state);
        return;
    };
    let (Some(at), Some(day), Some(usage)) =
        (at, day, payload.get("usage").and_then(parse_codex_usage))
    else {
        mark_codex_history_partial(state);
        return;
    };
    if usage.input.checked_add(usage.output) != Some(usage.total)
        || usage.cached > usage.input
        || usage.reasoning > usage.output
    {
        mark_codex_history_partial(state);
        return;
    }
    if state
        .inherited_history_cutoff
        .is_some_and(|cutoff| at < cutoff)
    {
        return;
    }
    if state.response_prefix.is_none() {
        state.response_prefix = Some(CodexUsagePrefix {
            usage: if state.history_complete {
                state.accounted
            } else {
                state.incremental
            },
            by_model: state.by_model.clone(),
            daily: state
                .daily
                .iter()
                .map(|((day, model), value)| (day.clone(), model.clone(), value.clone()))
                .collect(),
        });
    }
    if !state.responses.contains_key(id) {
        if *slots == 0 || state.responses.len() >= MAX_CODEX_RESPONSE_ENTRIES_PER_FILE {
            mark_codex_history_partial(state);
            return;
        }
        *slots -= 1;
    }
    let value = CodexResponseUsage {
        at,
        day: day.to_owned(),
        model: if state.fragment_started_at.is_some_and(|start| at < start) {
            "Unknown".into()
        } else {
            state.model.clone().unwrap_or_else(|| "Unknown".to_owned())
        },
        usage,
    };
    match state.responses.entry(id.to_owned()) {
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            if response_rank(&value) > response_rank(entry.get()) {
                entry.insert(value);
            }
        }
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(value);
        }
    }
    state.fragment_started_at = state.fragment_started_at.or(Some(at));
    state.last_usage_at = state.last_usage_at.max(Some(at));
    state.last = Some(usage);
}

fn response_rank(value: &CodexResponseUsage) -> (u64, u64, &str) {
    (value.at, value.usage.total, value.model.as_str())
}

/// One stable ledger per Provider session. Resume files are fragments, not
/// competing last-writer snapshots. Response identities deduplicate replays.
fn codex_records<'a>(
    states: impl Iterator<Item = &'a CodexFileState>,
    now: u64,
    pricing: &PricingSnapshot,
) -> Vec<UsageRecord> {
    let mut sessions = HashMap::<String, Vec<&CodexFileState>>::new();
    for state in states {
        if let Some(id) = &state.session_id {
            sessions.entry(id.clone()).or_default().push(state);
        }
    }
    let mut result = Vec::new();
    for (id, mut states) in sessions {
        states.sort_by_key(|state| {
            (
                state.last_usage_at.or(state.fragment_started_at),
                state.modified,
                state.offset,
            )
        });
        let latest = states.last().expect("nonempty source group");
        let mut group = CodexFileState {
            session_id: Some(id),
            modified: latest
                .last_usage_at
                .map(|at| UNIX_EPOCH + Duration::from_millis(at))
                .unwrap_or(latest.modified),
            project_id: latest.project_id.clone(),
            project_label: latest.project_label.clone(),
            parent_provider_session_id: latest.parent_provider_session_id.clone(),
            model: latest.model.clone(),
            context_window: latest.context_window,
            last: latest.last,
            history_complete: states.iter().all(|state| state.history_complete),
            inherited_history: states.iter().any(|state| state.inherited_history),
            ..CodexFileState::default()
        };
        let mut fragments = HashMap::<Option<u64>, &CodexFileState>::new();
        let mut responses = HashMap::<String, &CodexResponseUsage>::new();
        for state in states {
            // Prefer a response-aware copy, then the widest observed prefix.
            let rank = |s: &CodexFileState| {
                (
                    s.response_prefix.is_some(),
                    s.response_prefix
                        .as_ref()
                        .map(|p| p.usage.total)
                        .unwrap_or(s.accounted.total.max(s.incremental.total)),
                    s.offset,
                    s.modified,
                )
            };
            match fragments.entry(state.fragment_started_at) {
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    if rank(state) > rank(entry.get()) {
                        entry.insert(state);
                    }
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(state);
                }
            }
            for (id, value) in &state.responses {
                match responses.entry(id.clone()) {
                    std::collections::hash_map::Entry::Occupied(mut entry) => {
                        if response_rank(value) > response_rank(entry.get()) {
                            entry.insert(value);
                        }
                    }
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(value);
                    }
                }
            }
        }
        for source in fragments.values() {
            let prefix = source
                .response_prefix
                .clone()
                .unwrap_or_else(|| CodexUsagePrefix {
                    usage: if source.history_complete {
                        source.accounted
                    } else {
                        source.incremental
                    },
                    by_model: source.by_model.clone(),
                    daily: source
                        .daily
                        .iter()
                        .map(|((d, m), v)| (d.clone(), m.clone(), v.clone()))
                        .collect(),
                });
            group.accounted.add_assign(prefix.usage);
            for (model, usage) in prefix.by_model {
                group.by_model.entry(model).or_default().add_assign(usage);
            }
            for (day, model, value) in prefix.daily {
                let target = group.daily.entry((day, model)).or_default();
                target.usage.add_assign(value.usage);
                target.message_count = target.message_count.saturating_add(value.message_count);
            }
        }
        for value in responses.values() {
            group.accounted.add_assign(value.usage);
            group
                .by_model
                .entry(value.model.clone())
                .or_default()
                .add_assign(value.usage);
            let day = group
                .daily
                .entry((value.day.clone(), value.model.clone()))
                .or_default();
            day.usage.add_assign(value.usage);
            day.message_count = day.message_count.saturating_add(1);
        }
        group.incremental = group.accounted;
        if let Some(mut record) = codex_record_with_pricing(&group, now, pricing) {
            if !responses.is_empty() {
                record.usage_source = "codex_response_records".into();
            }
            result.push(record);
        }
    }
    result.sort_by(|a, b| a.provider_session_id.cmp(&b.provider_session_id));
    result
}

fn merge_record(records: &mut HashMap<(String, String), UsageRecord>, record: UsageRecord) {
    let key = (record.provider.clone(), record.provider_session_id.clone());
    records
        .entry(key)
        .and_modify(|current| current.merge_from(record.clone()))
        .or_insert(record);
}

struct DiscoveryResult {
    files: Vec<PathBuf>,
    #[cfg(test)]
    visited_entries: usize,
    #[cfg(test)]
    read_dir_nexts: usize,
    #[cfg(test)]
    metadata_checks: usize,
}

struct DiscoveryBudget {
    elapsed: Box<dyn Fn() -> Duration>,
    visited_entries: usize,
    #[cfg(test)]
    read_dir_nexts: usize,
    #[cfg(test)]
    metadata_checks: usize,
}

impl DiscoveryBudget {
    fn new() -> Self {
        let started_at = Instant::now();
        Self::with_elapsed(move || started_at.elapsed())
    }

    fn with_elapsed(elapsed: impl Fn() -> Duration + 'static) -> Self {
        Self {
            elapsed: Box::new(elapsed),
            visited_entries: 0,
            #[cfg(test)]
            read_dir_nexts: 0,
            #[cfg(test)]
            metadata_checks: 0,
        }
    }

    #[cfg(test)]
    fn with_elapsed_for_test(elapsed: impl Fn() -> Duration + 'static) -> Self {
        Self::with_elapsed(elapsed)
    }

    fn reserve_ticket(&mut self) -> bool {
        if self.exhausted() {
            return false;
        }
        self.visited_entries = self.visited_entries.saturating_add(1);
        true
    }

    fn reserve_before_read_dir_next(&mut self) -> bool {
        if !self.reserve_ticket() {
            return false;
        }
        #[cfg(test)]
        {
            self.read_dir_nexts = self.read_dir_nexts.saturating_add(1);
        }
        true
    }

    fn metadata_allowed(&self) -> bool {
        !self.time_exhausted()
    }

    fn record_metadata_check(&mut self) {
        #[cfg(test)]
        {
            self.metadata_checks = self.metadata_checks.saturating_add(1);
        }
    }

    fn time_exhausted(&self) -> bool {
        (self.elapsed)() >= MAX_DISCOVERY_DURATION
    }

    fn exhausted(&self) -> bool {
        self.visited_entries >= MAX_DISCOVERY_VISITED_ENTRIES || self.time_exhausted()
    }
}

fn discover_recent_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    discover_recent_files_with_budget(roots).files
}

fn discover_recent_files_with_budget(roots: &[PathBuf]) -> DiscoveryResult {
    let mut budget = DiscoveryBudget::new();
    discover_recent_files_with_budget_and_budget(roots, &mut budget)
}

fn discover_recent_files_with_budget_and_budget(
    roots: &[PathBuf],
    budget: &mut DiscoveryBudget,
) -> DiscoveryResult {
    let mut files = Vec::<(PathBuf, SystemTime)>::new();
    for root in roots {
        collect_jsonl(root, 0, &mut files, budget, false);
        if budget.exhausted() {
            break;
        }
    }
    files.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    files.truncate(MAX_DISCOVERED_FILES);
    DiscoveryResult {
        files: files.into_iter().map(|(path, _)| path).collect(),
        #[cfg(test)]
        visited_entries: budget.visited_entries,
        #[cfg(test)]
        read_dir_nexts: budget.read_dir_nexts,
        #[cfg(test)]
        metadata_checks: budget.metadata_checks,
    }
}

fn collect_jsonl(
    path: &Path,
    depth: usize,
    output: &mut Vec<(PathBuf, SystemTime)>,
    budget: &mut DiscoveryBudget,
    already_visited: bool,
) {
    if depth > 8
        || output.len() >= MAX_DISCOVERED_FILES.saturating_mul(4)
        || (!already_visited && !budget.reserve_ticket())
        || !budget.metadata_allowed()
    {
        return;
    }
    budget.record_metadata_check();
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
    let Ok(mut read_dir) = fs::read_dir(path) else {
        return;
    };
    let mut entries = Vec::new();
    loop {
        if !budget.reserve_before_read_dir_next() {
            break;
        }
        let Some(entry) = read_dir.next() else {
            break;
        };
        if let Ok(entry) = entry {
            entries.push(entry);
        }
    }
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if !budget.metadata_allowed() {
            break;
        }
        collect_jsonl(&entry.path(), depth + 1, output, budget, true);
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

#[cfg(test)]
fn parse_claude_tail(path: &Path, state: &mut ClaudeFileState, size: u64) {
    let mut remaining_bytes = MAX_JSONL_BYTES_PER_REFRESH;
    let mut line_buffer = Vec::new();
    parse_claude_tail_with_shutdown(
        path,
        state,
        size,
        None,
        &mut remaining_bytes,
        &mut line_buffer,
    );
}

fn parse_claude_tail_with_shutdown(
    path: &Path,
    state: &mut ClaudeFileState,
    size: u64,
    shutdown: Option<&AtomicBool>,
    remaining_bytes: &mut u64,
    line_buffer: &mut Vec<u8>,
) {
    let Ok(mut file) = File::open(path) else {
        return;
    };
    if file.seek(SeekFrom::Start(state.offset)).is_err() {
        return;
    }
    let mut reader = BufReader::new(file);
    loop {
        if collection_cancelled(shutdown) {
            break;
        }
        if *remaining_bytes == 0 {
            break;
        }
        // Never confuse the end of the shared refresh budget with an
        // oversized source line. Starting a new line requires a full per-line
        // allowance; otherwise leave the offset at the previous newline and
        // resume that ordinary line in the next bounded refresh.
        if !state.discarding_oversized_line && *remaining_bytes < MAX_JSONL_LINE_BYTES {
            break;
        }
        let remaining = if state.discarding_oversized_line {
            (*remaining_bytes).min(MAX_JSONL_LINE_BYTES)
        } else {
            MAX_JSONL_LINE_BYTES
        };
        let line_offset = state.offset;
        let Ok((consumed, complete, too_large)) =
            read_bounded_line_into(&mut reader, remaining as usize, line_buffer)
        else {
            return;
        };
        if collection_cancelled(shutdown) {
            break;
        }
        if consumed == 0 {
            break;
        }
        *remaining_bytes = remaining_bytes.saturating_sub(consumed);
        if state.discarding_oversized_line {
            state.offset = state.offset.saturating_add(consumed);
            if complete {
                state.discarding_oversized_line = false;
            }
            continue;
        }
        if too_large {
            // Keep the line-discard state across refreshes. We must never
            // deserialize a later fragment as if it were a JSONL record.
            state.history_complete = false;
            state.offset = state.offset.saturating_add(consumed);
            state.discarding_oversized_line = !complete;
            continue;
        }
        let Ok(root) = serde_json::from_slice::<Value>(line_buffer) else {
            if complete || state.offset.saturating_add(consumed) < size {
                state.offset = state.offset.saturating_add(consumed);
                continue;
            }
            break;
        };
        state.offset = state.offset.saturating_add(consumed);

        if state.project_id.is_none() {
            if let Some(project) = project_identity_from_value(&root) {
                state.project_id = Some(project.id);
                state.project_label = Some(project.label);
            }
        }

        if state.session_id.is_none() {
            state.session_id = root
                .get("sessionId")
                .or_else(|| root.get("session_id"))
                .and_then(Value::as_str)
                .and_then(safe_session_id)
                .or_else(|| {
                    state.history_complete.then(|| {
                        path.file_stem()
                            .and_then(|value| value.to_str())
                            .and_then(safe_session_id)
                    })?
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
    upsert_claude_entry_with_daily(state, key, entry, true);
}

fn upsert_claude_entry_with_daily(
    state: &mut ClaudeFileState,
    key: String,
    entry: TokenEntry,
    update_daily: bool,
) {
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
        if update_daily {
            update_claude_daily(state, &previous, false);
        }
    }
    state.recent.add_entry(&entry);
    if update_daily {
        update_claude_daily(state, &entry, true);
    }
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

fn update_claude_daily(state: &mut ClaudeFileState, entry: &TokenEntry, adding: bool) {
    let Some(day) = local_day_from_timestamp(&entry.timestamp) else {
        return;
    };
    let model = entry
        .model
        .as_deref()
        .filter(|value| !is_synthetic_model(Some(value)))
        .unwrap_or("Unknown")
        .to_owned();
    let key = (day, model);
    if adding {
        state.daily.entry(key).or_default().add_entry(entry);
        return;
    }
    let should_remove = if let Some(total) = state.daily.get_mut(&key) {
        total.remove_entry(entry);
        total.entry_count == 0
    } else {
        false
    };
    if should_remove {
        state.daily.remove(&key);
    }
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
            let model = entry
                .model
                .as_deref()
                .filter(|value| !is_synthetic_model(Some(value)))
                .unwrap_or("Unknown")
                .to_owned();
            state
                .compacted_by_model
                .entry(model)
                .or_default()
                .add_entry(&entry);
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

#[cfg(test)]
fn parse_codex_tail(path: &Path, state: &mut CodexFileState, size: u64) {
    let mut remaining_bytes = MAX_JSONL_BYTES_PER_REFRESH;
    let mut line_buffer = Vec::new();
    let mut response_slots = MAX_CODEX_RESPONSE_ENTRIES;
    parse_codex_tail_with_shutdown(
        path,
        state,
        size,
        None,
        &mut remaining_bytes,
        &mut line_buffer,
        &mut response_slots,
    );
}

fn parse_codex_tail_with_shutdown(
    path: &Path,
    state: &mut CodexFileState,
    size: u64,
    shutdown: Option<&AtomicBool>,
    remaining_bytes: &mut u64,
    line_buffer: &mut Vec<u8>,
    response_slots: &mut usize,
) {
    let Ok(mut file) = File::open(path) else {
        return;
    };
    if file.seek(SeekFrom::Start(state.offset)).is_err() {
        return;
    }
    let mut reader = BufReader::new(file);
    loop {
        if collection_cancelled(shutdown) {
            break;
        }
        if *remaining_bytes == 0 {
            break;
        }
        // See the Claude path above: a refresh-budget boundary is not evidence
        // that the Provider emitted an oversized JSONL record.
        if !state.discarding_oversized_line && *remaining_bytes < MAX_JSONL_LINE_BYTES {
            break;
        }
        let remaining = if state.discarding_oversized_line {
            (*remaining_bytes).min(MAX_JSONL_LINE_BYTES)
        } else {
            MAX_JSONL_LINE_BYTES
        };
        let Ok((consumed, complete, too_large)) =
            read_bounded_line_into(&mut reader, remaining as usize, line_buffer)
        else {
            return;
        };
        if collection_cancelled(shutdown) {
            break;
        }
        if consumed == 0 {
            break;
        }
        *remaining_bytes = remaining_bytes.saturating_sub(consumed);
        if state.discarding_oversized_line {
            state.offset = state.offset.saturating_add(consumed);
            if complete {
                state.discarding_oversized_line = false;
            }
            continue;
        }
        if too_large {
            if json_prefix_string_field(line_buffer, 0, b"type") == Some(b"compacted") {
                state.pending_compaction = true;
            }
            // Keep the line-discard state across refreshes. We must never
            // deserialize a later fragment as if it were a JSONL record.
            if !codex_oversized_prefix_is_known_non_usage(line_buffer) {
                if state.history_complete {
                    state.incremental.add_assign(state.accounted);
                    state.accounted = CodexUsage::default();
                }
                state.history_complete = false;
            }
            state.offset = state.offset.saturating_add(consumed);
            state.discarding_oversized_line = !complete;
            continue;
        }
        let Ok(kind) = serde_json::from_slice::<CodexLineKind>(line_buffer) else {
            if complete || state.offset.saturating_add(consumed) < size {
                state.offset = state.offset.saturating_add(consumed);
                continue;
            }
            break;
        };
        if !kind.carries_usage_metadata() {
            state.offset = state.offset.saturating_add(consumed);
            continue;
        }
        let Ok(root) = serde_json::from_slice::<Value>(line_buffer) else {
            state.offset = state.offset.saturating_add(consumed);
            continue;
        };
        state.offset = state.offset.saturating_add(consumed);
        let event_day = root
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(local_day_from_timestamp);
        let event_at = root
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(timestamp_millis);
        let event_type = root.get("type").and_then(Value::as_str).unwrap_or_default();
        let payload = root.get("payload").unwrap_or(&Value::Null);
        match event_type {
            "session_meta" => {
                if codex_metadata_inherits_history(payload) {
                    state.inherited_history = true;
                    state.inherited_history_cutoff = event_at.or_else(|| {
                        payload
                            .get("timestamp")
                            .and_then(Value::as_str)
                            .and_then(timestamp_millis)
                    });
                }
                if state.project_id.is_none() {
                    if let Some(project) = project_identity_from_value(payload) {
                        state.project_id = Some(project.id);
                        state.project_label = Some(project.label);
                    }
                }
                if state.parent_provider_session_id.is_none() {
                    state.parent_provider_session_id = codex_parent_session_id(payload);
                }
                // A fork/compaction can replay a parent `session_meta` later
                // in the child file. The first valid file identity is
                // canonical; allowing a later replay to overwrite it merges
                // independent child ledgers into the parent and pairs totals
                // with unrelated daily rows.
                if state.session_id.is_none() {
                    state.fragment_started_at = event_at.or_else(|| {
                        payload
                            .get("timestamp")
                            .and_then(Value::as_str)
                            .and_then(timestamp_millis)
                    });
                    state.session_id = payload
                        .get("id")
                        .or_else(|| payload.get("session_id"))
                        .and_then(Value::as_str)
                        .and_then(safe_session_id);
                }
            }
            "turn_context" => {
                if let Some(model) = payload
                    .get("model")
                    .and_then(Value::as_str)
                    .filter(|model| !model.trim().is_empty())
                {
                    adopt_codex_model(state, model);
                }
                state.context_window = payload
                    .get("context_window")
                    .or_else(|| payload.get("context_window_tokens"))
                    .and_then(value_u64)
                    .or(state.context_window);
            }
            "compacted" => {
                state.pending_compaction = true;
            }
            "token_usage_record" => {
                observe_codex_response(
                    state,
                    payload,
                    event_at,
                    event_day.as_deref(),
                    response_slots,
                );
            }
            "event_msg" if payload.get("type").and_then(Value::as_str) == Some("token_count") => {
                let Some(info) = payload.get("info") else {
                    continue;
                };
                state.fragment_started_at = state.fragment_started_at.or(event_at);
                state.last_usage_at = state.last_usage_at.max(event_at);
                if state.response_prefix.is_some() {
                    // The same call is mirrored in token_count. Keep context
                    // measurements, but never add the response twice.
                    state.last = info
                        .get("last_token_usage")
                        .and_then(parse_codex_usage)
                        .or(state.last);
                    state.context_window = info
                        .get("model_context_window")
                        .or_else(|| info.get("context_window"))
                        .and_then(value_u64)
                        .or(state.context_window);
                    continue;
                }
                let mut previous = state.cumulative;
                let current = info
                    .get("total_token_usage")
                    .and_then(parse_codex_usage)
                    .or(state.cumulative);
                let reset_epoch = state.pending_compaction
                    && matches!((previous, current), (Some(old), Some(new)) if codex_usage_decreased(new, old));
                if reset_epoch {
                    previous = None;
                }
                if info
                    .get("total_token_usage")
                    .and_then(parse_codex_usage)
                    .is_some()
                {
                    state.pending_compaction = false;
                }
                let reported_last = info
                    .get("last_token_usage")
                    .and_then(parse_codex_usage)
                    .or_else(|| codex_usage_delta(current, previous))
                    .or(state.last);
                let inherited_baseline = state.inherited_history
                    && match (event_at, state.inherited_history_cutoff) {
                        (Some(event_at), Some(cutoff)) => event_at <= cutoff,
                        // Without a trustworthy fork boundary, counting the
                        // replay would be worse than returning partial data.
                        _ => true,
                    };
                if inherited_baseline {
                    state.cumulative = current;
                    state.last = reported_last;
                    state.context_window = info
                        .get("model_context_window")
                        .or_else(|| info.get("context_window"))
                        .and_then(value_u64)
                        .or(state.context_window);
                    continue;
                }
                if state.history_complete {
                    if let Some(current) = current {
                        let stale = previous
                            .is_some_and(|previous| codex_usage_decreased(current, previous));
                        if !stale {
                            if let Some(delta) = codex_usage_delta(Some(current), previous) {
                                add_codex_daily(state, event_day.as_deref(), delta);
                                state.accounted.add_assign(delta);
                            }
                            state.cumulative = Some(current);
                        }
                    }
                } else {
                    let stale = matches!((previous, current), (Some(previous), Some(current)) if codex_usage_decreased(current, previous));
                    let observed = match (previous, current, stale) {
                        (Some(previous), Some(current), false) => {
                            codex_usage_delta(Some(current), Some(previous))
                        }
                        (None, Some(value), false) => {
                            if reset_epoch {
                                Some(value)
                            } else {
                                reported_last
                            }
                        }
                        _ => None,
                    };
                    if state.session_id.is_some() {
                        if let Some(observed) = observed.filter(|usage| !usage.is_empty()) {
                            state.incremental.add_assign(observed);
                            add_codex_daily(state, event_day.as_deref(), observed);
                        }
                    }
                    if !stale {
                        state.cumulative = current.or(previous);
                    }
                }
                state.last = reported_last;
                state.context_window = info
                    .get("model_context_window")
                    .or_else(|| info.get("context_window"))
                    .and_then(value_u64)
                    .or(state.context_window);
            }
            _ => {}
        }
    }
    if state.history_complete
        && state.session_id.is_none()
        && state.offset >= size
        && !state.discarding_oversized_line
    {
        state.session_id = path
            .file_stem()
            .and_then(|value| value.to_str())
            .and_then(safe_session_id);
    }
}

fn codex_metadata_inherits_history(payload: &Value) -> bool {
    payload.get("parent_thread_id").is_some()
        || payload.get("forked_from_id").is_some()
        || payload.pointer("/source/subagent").is_some()
        || payload.get("thread_source").and_then(Value::as_str) == Some("subagent")
}

fn codex_parent_session_id(payload: &Value) -> Option<String> {
    [
        "/parent_thread_id",
        "/forked_from_id",
        "/source/subagent/parent_thread_id",
        "/source/subagent/parent_id",
        "/source/parent_thread_id",
    ]
    .into_iter()
    .find_map(|pointer| payload.pointer(pointer).and_then(Value::as_str))
    .and_then(safe_session_id)
}

fn adopt_codex_model(state: &mut CodexFileState, model: &str) {
    if state.model.is_none() {
        if let Some(unknown) = state.by_model.remove("Unknown") {
            state
                .by_model
                .entry(model.to_owned())
                .or_default()
                .add_assign(unknown);
        }
        let unknown_keys = state
            .daily
            .keys()
            .filter(|(_, existing_model)| existing_model == "Unknown")
            .cloned()
            .collect::<Vec<_>>();
        for (day, existing_model) in unknown_keys {
            let Some(unknown) = state.daily.remove(&(day.clone(), existing_model)) else {
                continue;
            };
            let target = state.daily.entry((day, model.to_owned())).or_default();
            target.usage.add_assign(unknown.usage);
            target.message_count = target.message_count.saturating_add(unknown.message_count);
        }
    }
    state.model = Some(model.to_owned());
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

fn add_codex_daily(state: &mut CodexFileState, day: Option<&str>, usage: CodexUsage) {
    if usage.is_empty() {
        return;
    }
    let model = state.model.as_deref().unwrap_or("Unknown").to_owned();
    state
        .by_model
        .entry(model.clone())
        .or_default()
        .add_assign(usage);
    let Some(day) = day else { return };
    let entry = state.daily.entry((day.to_owned(), model)).or_default();
    entry.usage.add_assign(usage);
    entry.message_count = entry.message_count.saturating_add(1);
}

#[cfg(test)]
fn claude_record(state: &ClaudeFileState, now_ms: u64) -> Option<UsageRecord> {
    claude_record_with_pricing(state, now_ms, pricing_snapshot())
}

fn claude_record_with_pricing(
    state: &ClaudeFileState,
    now_ms: u64,
    pricing: &PricingSnapshot,
) -> Option<UsageRecord> {
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
    // A new model identifier is enough to show the model, not enough to
    // invent its context capacity. Official StatusLine can supply it later.
    let context_window = model.and_then(|model| {
        (model.contains("[1m]") || model_pricing_in(pricing_snapshot(), "claude", model).is_some())
            .then(|| claude_context_window(model))
    });
    let context_percent = percent(context_used, context_window);
    let daily_usage = claude_daily_records(state, pricing);
    let (cost, cost_kind, pricing_source) = claude_model_cost_summary(state, pricing);
    Some(UsageRecord {
        provider: "claude".to_owned(),
        provider_session_id: session_id,
        project_id: state.project_id.clone(),
        project_label: state.project_label.clone(),
        parent_provider_session_id: None,
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
        usage_source: if state.history_complete {
            "claude_transcript".to_owned()
        } else {
            "claude_transcript_incremental".to_owned()
        },
        usage_quality: if state.history_complete {
            "derived".to_owned()
        } else {
            "partial".to_owned()
        },
        captured_at: system_time_millis(state.modified).unwrap_or(now_ms),
        daily_usage,
    })
}

fn claude_daily_records(
    state: &ClaudeFileState,
    pricing: &PricingSnapshot,
) -> Vec<UsageDailyRecord> {
    let mut output = state
        .daily
        .iter()
        .filter(|(_, value)| value.entry_count > 0)
        .map(|((day, model), value)| {
            let official_cost = value.official_cost();
            let computed_cost = claude_accumulator_cost(pricing, model, value);
            let estimated_cost_usd_micros = official_cost.or(computed_cost);
            let (cost_kind, pricing_source) = if official_cost.is_some() {
                (
                    Some("provider_estimate".to_owned()),
                    Some("claude_transcript_cost".to_owned()),
                )
            } else if computed_cost.is_some() {
                (
                    Some("computed".to_owned()),
                    model_pricing_in(pricing, "claude", model).map(|price| price.source),
                )
            } else {
                (None, None)
            };
            UsageDailyRecord {
                day: day.clone(),
                model: (model != "Unknown").then(|| model.clone()),
                input_tokens: value.input,
                output_tokens: value.output,
                cache_read_tokens: value.cache_read,
                cache_creation_tokens: value.cache_creation,
                reasoning_tokens: 0,
                token_total: value.token_total(),
                estimated_cost_usd_micros,
                cost_kind,
                pricing_source,
                message_count: value.entry_count,
            }
        })
        .collect::<Vec<_>>();
    output.sort_by(|left, right| {
        left.day
            .cmp(&right.day)
            .then_with(|| left.model.cmp(&right.model))
    });
    output
}

fn claude_accumulator_cost(
    pricing: &PricingSnapshot,
    model: &str,
    value: &TokenAccumulator,
) -> Option<u64> {
    let price = model_price_in(pricing, "claude", model)?;
    let micros = priced_cost_micros_value(
        value.input,
        value.output,
        value.cache_read,
        value.cache_creation,
        price,
    )?;
    (micros <= u64::MAX as f64).then(|| micros.round() as u64)
}

fn claude_model_cost_summary(
    state: &ClaudeFileState,
    pricing: &PricingSnapshot,
) -> (Option<u64>, Option<String>, Option<String>) {
    let mut by_model = state.compacted_by_model.clone();
    for entry in state.entries.values() {
        let model = entry
            .model
            .as_deref()
            .filter(|value| !is_synthetic_model(Some(value)))
            .unwrap_or("Unknown")
            .to_owned();
        by_model.entry(model).or_default().add_entry(entry);
    }
    let records = by_model
        .iter()
        .filter(|(_, value)| value.entry_count > 0)
        .map(|(model, value)| {
            let official_cost = value.official_cost();
            let computed_cost = claude_accumulator_cost(pricing, model, value);
            UsageDailyRecord {
                token_total: value.token_total(),
                estimated_cost_usd_micros: official_cost.or(computed_cost),
                cost_kind: if official_cost.is_some() {
                    Some("provider_estimate".to_owned())
                } else {
                    computed_cost.map(|_| "computed".to_owned())
                },
                pricing_source: if official_cost.is_some() {
                    Some("claude_transcript_cost".to_owned())
                } else {
                    computed_cost.and_then(|_| {
                        model_pricing_in(pricing, "claude", model).map(|price| price.source)
                    })
                },
                ..UsageDailyRecord::default()
            }
        })
        .collect::<Vec<_>>();
    aggregate_daily_cost(&records)
}

fn is_synthetic_model(model: Option<&str>) -> bool {
    model.is_some_and(|model| model.trim().eq_ignore_ascii_case("<synthetic>"))
}

#[cfg(test)]
fn codex_record(state: &CodexFileState, now_ms: u64) -> Option<UsageRecord> {
    codex_record_with_pricing(state, now_ms, pricing_snapshot())
}

fn codex_record_with_pricing(
    state: &CodexFileState,
    now_ms: u64,
    pricing: &PricingSnapshot,
) -> Option<UsageRecord> {
    let session_id = state.session_id.clone()?;
    let total = if state.history_complete {
        (!state.accounted.is_empty()).then_some(state.accounted)?
    } else {
        (!state.incremental.is_empty()).then_some(state.incremental)?
    };
    let last = state.last.unwrap_or_default();
    let context_used = (last.input > 0).then_some(last.input);
    let context_percent = percent(context_used, state.context_window);
    let daily_usage = codex_daily_records(state, pricing);
    let (cost, cost_kind, pricing_source) = codex_model_cost_summary(state, pricing);
    Some(UsageRecord {
        provider: "codex".to_owned(),
        provider_session_id: session_id,
        project_id: state.project_id.clone(),
        project_label: state.project_label.clone(),
        parent_provider_session_id: state.parent_provider_session_id.clone(),
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
        cost_kind,
        pricing_source,
        usage_source: if state.history_complete && state.inherited_history {
            "codex_rollout_session_local".to_owned()
        } else if state.history_complete {
            "codex_rollout".to_owned()
        } else {
            "codex_rollout_incremental".to_owned()
        },
        usage_quality: if state.history_complete && state.inherited_history {
            "derived".to_owned()
        } else if state.history_complete {
            "official_local".to_owned()
        } else {
            "partial".to_owned()
        },
        captured_at: system_time_millis(state.modified).unwrap_or(now_ms),
        daily_usage,
    })
}

fn codex_daily_records(state: &CodexFileState, pricing: &PricingSnapshot) -> Vec<UsageDailyRecord> {
    let mut output = state
        .daily
        .iter()
        .map(|((day, model), value)| {
            let estimated_cost_usd_micros = codex_cost_micros_value_in(pricing, model, value.usage)
                .filter(|cost| *cost <= u64::MAX as f64)
                .map(|cost| cost.round() as u64);
            UsageDailyRecord {
                day: day.clone(),
                model: (model != "Unknown").then(|| model.clone()),
                input_tokens: value.usage.input,
                output_tokens: value.usage.output,
                cache_read_tokens: value.usage.cached,
                cache_creation_tokens: 0,
                reasoning_tokens: value.usage.reasoning,
                token_total: if value.usage.total > 0 {
                    value.usage.total
                } else {
                    value.usage.input.saturating_add(value.usage.output)
                },
                estimated_cost_usd_micros,
                cost_kind: estimated_cost_usd_micros.map(|_| "computed".to_owned()),
                pricing_source: estimated_cost_usd_micros.and_then(|_| {
                    model_pricing_in(pricing, "codex", model).map(|price| price.source)
                }),
                message_count: value.message_count,
            }
        })
        .collect::<Vec<_>>();
    output.sort_by(|left, right| {
        left.day
            .cmp(&right.day)
            .then_with(|| left.model.cmp(&right.model))
    });
    output
}

fn codex_model_cost_summary(
    state: &CodexFileState,
    pricing: &PricingSnapshot,
) -> (Option<u64>, Option<String>, Option<String>) {
    let records = state
        .by_model
        .iter()
        .map(|(model, usage)| {
            let estimated_cost_usd_micros = codex_cost_micros_value_in(pricing, model, *usage)
                .filter(|cost| *cost <= u64::MAX as f64)
                .map(|cost| cost.round() as u64);
            UsageDailyRecord {
                token_total: if usage.total > 0 {
                    usage.total
                } else {
                    usage.input.saturating_add(usage.output)
                },
                estimated_cost_usd_micros,
                cost_kind: estimated_cost_usd_micros.map(|_| "computed".to_owned()),
                pricing_source: estimated_cost_usd_micros.and_then(|_| {
                    model_pricing_in(pricing, "codex", model).map(|price| price.source)
                }),
                ..UsageDailyRecord::default()
            }
        })
        .collect::<Vec<_>>();
    aggregate_daily_cost(&records)
}

fn aggregate_daily_cost(
    daily: &[UsageDailyRecord],
) -> (Option<u64>, Option<String>, Option<String>) {
    let billable = daily
        .iter()
        .filter(|record| record.token_total > 0)
        .collect::<Vec<_>>();
    if billable.is_empty()
        || billable
            .iter()
            .any(|record| record.estimated_cost_usd_micros.is_none())
    {
        return (None, None, None);
    }
    let cost = billable.iter().fold(0_u64, |total, record| {
        total.saturating_add(record.estimated_cost_usd_micros.unwrap_or_default())
    });
    let mut kinds = billable
        .iter()
        .filter_map(|record| record.cost_kind.clone())
        .collect::<Vec<_>>();
    kinds.sort();
    kinds.dedup();
    let mut sources = billable
        .iter()
        .filter_map(|record| record.pricing_source.clone())
        .collect::<Vec<_>>();
    sources.sort();
    sources.dedup();
    (
        Some(cost),
        Some(if kinds.len() == 1 {
            kinds.remove(0)
        } else {
            "mixed".to_owned()
        }),
        Some(if sources.len() == 1 {
            sources.remove(0)
        } else {
            "mixed_pricing_sources".to_owned()
        }),
    )
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

#[derive(Debug, Clone, Deserialize)]
struct PricingSnapshot {
    source: String,
    models: Vec<ModelPrice>,
}

#[derive(Debug, Clone, Deserialize)]
struct ModelPrice {
    provider: String,
    source: String,
    id: String,
    #[serde(default)]
    aliases: Vec<String>,
    input: f64,
    output: f64,
    #[serde(default)]
    cache_read: Option<f64>,
    #[serde(default)]
    cache_create: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Price {
    input: f64,
    output: f64,
    cache_read: Option<f64>,
    cache_create: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct ModelsDevProvider {
    #[serde(default)]
    models: HashMap<String, ModelsDevModel>,
}

#[derive(Debug, Deserialize)]
struct ModelsDevModel {
    id: String,
    cost: Option<ModelsDevCost>,
}

#[derive(Debug, Deserialize)]
struct ModelsDevCost {
    input: Option<f64>,
    output: Option<f64>,
    cache_read: Option<f64>,
    cache_write: Option<f64>,
}

fn pricing_snapshot() -> &'static PricingSnapshot {
    static SNAPSHOT: OnceLock<PricingSnapshot> = OnceLock::new();
    SNAPSHOT.get_or_init(|| {
        serde_json::from_str(PRICING_SNAPSHOT_JSON)
            .expect("embedded usage pricing snapshot must be valid")
    })
}

fn embedded_pricing_snapshot() -> PricingSnapshot {
    pricing_snapshot().clone()
}

#[cfg(test)]
fn claude_price(model: &str) -> Option<Price> {
    model_price("claude", model)
}

#[cfg(test)]
fn codex_price(model: &str) -> Option<Price> {
    model_price("codex", model)
}

#[cfg(test)]
fn model_price(provider: &str, model: &str) -> Option<Price> {
    model_price_in(pricing_snapshot(), provider, model)
}

fn model_price_in(snapshot: &PricingSnapshot, provider: &str, model: &str) -> Option<Price> {
    model_pricing_in(snapshot, provider, model).map(|price| Price {
        input: price.input,
        output: price.output,
        cache_read: price.cache_read,
        cache_create: price.cache_create,
    })
}

#[cfg(test)]
fn model_pricing(provider: &str, model: &str) -> Option<ModelPrice> {
    model_pricing_in(pricing_snapshot(), provider, model)
}

fn model_pricing_in(snapshot: &PricingSnapshot, provider: &str, model: &str) -> Option<ModelPrice> {
    let candidates = normalized_model_candidates(model);
    for candidate in &candidates {
        if let Some(exact) = snapshot
            .models
            .iter()
            .find(|price| price.provider == provider && &price.id == candidate)
        {
            return Some(exact.clone());
        }
    }
    snapshot
        .models
        .iter()
        .filter(|price| price.provider == provider)
        .find(|price| {
            candidates.iter().any(|candidate| {
                candidate == &price.id || price.aliases.iter().any(|alias| candidate == alias)
            })
        })
        .cloned()
}

fn models_dev_pricing_snapshot(encoded: &[u8]) -> Result<PricingSnapshot, UsageError> {
    if encoded.len() as u64 > MODELS_DEV_CACHE_MAX_BYTES {
        return Err(UsageError::TooLarge(MODELS_DEV_CACHE_MAX_BYTES));
    }
    let providers = serde_json::from_slice::<HashMap<String, ModelsDevProvider>>(encoded)?;
    let source = format!("models_dev_api_{:016x}", catalog_fingerprint(encoded));
    let mut snapshot = embedded_pricing_snapshot();
    for (remote_provider, local_provider) in [("anthropic", "claude"), ("openai", "codex")] {
        let Some(provider) = providers.get(remote_provider) else {
            continue;
        };
        for model in provider.models.values() {
            let Some(cost) = model.cost.as_ref() else {
                continue;
            };
            let Some(input) = valid_catalog_rate(cost.input) else {
                continue;
            };
            let Some(output) = valid_catalog_rate(cost.output) else {
                continue;
            };
            let id = model.id.trim().to_ascii_lowercase();
            if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
                continue;
            }
            let cache_read = optional_catalog_rate(cost.cache_read)?;
            let cache_create = optional_catalog_rate(cost.cache_write)?;
            let existing = snapshot
                .models
                .iter()
                .find(|price| price.provider == local_provider && price.id == id)
                .cloned();
            snapshot
                .models
                .retain(|price| !(price.provider == local_provider && price.id == id));
            snapshot.models.push(ModelPrice {
                provider: local_provider.to_owned(),
                source: source.clone(),
                id,
                aliases: existing.map(|price| price.aliases).unwrap_or_default(),
                input,
                output,
                cache_read,
                cache_create,
            });
        }
    }
    if !snapshot
        .models
        .iter()
        .any(|price| price.source == source && price.provider == "claude")
        || !snapshot
            .models
            .iter()
            .any(|price| price.source == source && price.provider == "codex")
    {
        return Err(UsageError::Pricing(
            "models.dev did not contain both Anthropic and OpenAI prices".to_owned(),
        ));
    }
    snapshot.source = source;
    Ok(snapshot)
}

fn valid_catalog_rate(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite() && *value >= 0.0 && *value <= 1_000_000.0)
}

fn optional_catalog_rate(value: Option<f64>) -> Result<Option<f64>, UsageError> {
    match value {
        Some(value) => valid_catalog_rate(Some(value)).map(Some).ok_or_else(|| {
            UsageError::Pricing("models.dev contained an invalid Token rate".to_owned())
        }),
        None => Ok(None),
    }
}

fn catalog_fingerprint(encoded: &[u8]) -> u64 {
    encoded
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

fn usage_source_key(provider: &str, path: &Path) -> String {
    let encoded_path = path.to_string_lossy();
    let mut bytes = Vec::with_capacity(provider.len() + encoded_path.len() + 1);
    bytes.extend_from_slice(provider.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(encoded_path.as_bytes());
    format!("{:016x}", catalog_fingerprint(&bytes))
}

fn usage_checkpoint_source_matches(
    path: &Path,
    offset: u64,
    modified_ms: u64,
    identity: Option<(u64, u64)>,
) -> bool {
    let Ok(metadata) = regular_file_metadata(path) else {
        return false;
    };
    identity == Some((metadata.dev(), metadata.ino()))
        && offset <= metadata.len()
        && system_time_millis(metadata.modified().unwrap_or(UNIX_EPOCH))
            .is_some_and(|modified| modified >= modified_ms)
}

fn usage_checkpoint_signature(
    claude: &HashMap<PathBuf, ClaudeFileState>,
    codex: &HashMap<PathBuf, CodexFileState>,
) -> u64 {
    let mut values = claude
        .iter()
        .map(|(path, state)| {
            (
                usage_source_key("claude", path),
                state.offset,
                state.size,
                state.identity,
                state.history_complete,
            )
        })
        .chain(codex.iter().map(|(path, state)| {
            (
                usage_source_key("codex", path),
                state.offset,
                state.size,
                state.identity,
                state.history_complete,
            )
        }))
        .collect::<Vec<_>>();
    values.sort_by(|left, right| left.0.cmp(&right.0));
    let mut bytes = Vec::with_capacity(values.len() * 64);
    for (key, offset, size, identity, complete) in values {
        bytes.extend_from_slice(key.as_bytes());
        bytes.extend_from_slice(&offset.to_le_bytes());
        bytes.extend_from_slice(&size.to_le_bytes());
        if let Some((device, inode)) = identity {
            bytes.extend_from_slice(&device.to_le_bytes());
            bytes.extend_from_slice(&inode.to_le_bytes());
        }
        bytes.push(u8::from(complete));
    }
    catalog_fingerprint(&bytes)
}

fn load_usage_scan_checkpoint(paths: &UsagePaths) -> Option<UsageCollectorCheckpoint> {
    let path = paths.usage_scan_checkpoint();
    let metadata = regular_file_metadata(&path).ok()?;
    if metadata.len() > USAGE_SCAN_CHECKPOINT_MAX_BYTES {
        return None;
    }
    let checkpoint =
        serde_json::from_slice::<UsageCollectorCheckpoint>(&fs::read(path).ok()?).ok()?;
    (checkpoint.schema_version == USAGE_SCAN_CHECKPOINT_SCHEMA).then_some(checkpoint)
}

fn load_models_dev_cache(paths: &UsagePaths) -> Option<PricingSnapshot> {
    let path = paths.models_dev_pricing_cache();
    let metadata = fs::metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > MODELS_DEV_CACHE_MAX_BYTES {
        return None;
    }
    let encoded = fs::read(path).ok()?;
    models_dev_pricing_snapshot(&encoded).ok()
}

fn cache_is_fresh(path: &Path) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age < MODELS_DEV_REFRESH_INTERVAL)
}

fn pricing_refresh_due(
    fresh: bool,
    missing_model: bool,
    since_attempt: Option<Duration>,
    force: bool,
) -> bool {
    force
        || ((!fresh || missing_model)
            && since_attempt.is_none_or(|elapsed| elapsed >= MODELS_DEV_RETRY_INTERVAL))
}

fn fetch_models_dev_catalog() -> Result<Vec<u8>, UsageError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(MODELS_DEV_REQUEST_TIMEOUT)
        .connect_timeout(Duration::from_secs(5))
        .gzip(true)
        .user_agent("ActRealm/0.1 models.dev pricing")
        .build()
        .map_err(|error| UsageError::Pricing(error.to_string()))?;
    let response = client
        .get(MODELS_DEV_API_URL)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| UsageError::Pricing(error.to_string()))?;
    let mut encoded = Vec::new();
    response
        .take(MODELS_DEV_CACHE_MAX_BYTES + 1)
        .read_to_end(&mut encoded)
        .map_err(|source| UsageError::Io {
            path: PathBuf::from("models.dev/api.json"),
            source,
        })?;
    if encoded.len() as u64 > MODELS_DEV_CACHE_MAX_BYTES {
        return Err(UsageError::TooLarge(MODELS_DEV_CACHE_MAX_BYTES));
    }
    Ok(encoded)
}

fn normalized_model_candidates(model: &str) -> Vec<String> {
    let normalized = model.trim().to_ascii_lowercase();
    if normalized.is_empty() {
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

#[cfg(test)]
fn claude_cost_micros_value(model: &str, usage: &TokenEntry) -> Option<f64> {
    claude_cost_micros_value_in(pricing_snapshot(), model, usage)
}

#[cfg(test)]
fn claude_cost_micros_value_in(
    snapshot: &PricingSnapshot,
    model: &str,
    usage: &TokenEntry,
) -> Option<f64> {
    let price = model_price_in(snapshot, "claude", model)?;
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

#[cfg(test)]
fn codex_cost_micros_value(model: &str, usage: CodexUsage) -> Option<f64> {
    codex_cost_micros_value_in(pricing_snapshot(), model, usage)
}

fn codex_cost_micros_value_in(
    snapshot: &PricingSnapshot,
    model: &str,
    usage: CodexUsage,
) -> Option<f64> {
    let price = model_price_in(snapshot, "codex", model)?;
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
    let cache_read_cost = priced_optional_component(cache_read, price.cache_read)?;
    let cache_create_cost = priced_optional_component(cache_create, price.cache_create)?;
    let micros = input as f64 * price.input
        + output as f64 * price.output
        + cache_read_cost
        + cache_create_cost;
    (micros.is_finite() && micros >= 0.0).then_some(micros)
}

fn priced_optional_component(tokens: u64, rate: Option<f64>) -> Option<f64> {
    if tokens == 0 {
        Some(0.0)
    } else {
        rate.map(|rate| tokens as f64 * rate)
    }
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

fn read_bounded_line_into<R: BufRead>(
    reader: &mut R,
    limit: usize,
    output: &mut Vec<u8>,
) -> io::Result<(u64, bool, bool)> {
    output.clear();
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
        let remaining = limit.saturating_sub(output.len());
        if remaining == 0 {
            too_large = true;
            break;
        }
        if take > remaining {
            output.extend_from_slice(&buffer[..remaining]);
            consumed = consumed.saturating_add(remaining as u64);
            reader.consume(remaining);
            too_large = true;
            break;
        }
        output.extend_from_slice(&buffer[..take]);
        consumed = consumed.saturating_add(take as u64);
        complete = buffer[..take].ends_with(b"\n");
        reader.consume(take);
        if complete {
            break;
        }
    }
    Ok((consumed, complete, too_large))
}

#[cfg(test)]
fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    limit: usize,
) -> io::Result<(Vec<u8>, u64, bool, bool)> {
    let mut output = Vec::new();
    let (consumed, complete, too_large) = read_bounded_line_into(reader, limit, &mut output)?;
    Ok((output, consumed, complete, too_large))
}

fn system_time_millis(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

fn local_day_from_timestamp(value: &str) -> Option<String> {
    local_day(timestamp_millis(value)?)
}

fn local_day(milliseconds: u64) -> Option<String> {
    let seconds: libc::time_t = (milliseconds / 1_000).try_into().ok()?;
    let mut output = std::mem::MaybeUninit::<libc::tm>::zeroed();
    // SAFETY: output points to writable tm storage and seconds remains alive.
    let result = unsafe { libc::localtime_r(&seconds, output.as_mut_ptr()) };
    if result.is_null() {
        return None;
    }
    // SAFETY: localtime_r returned non-null and initialized output.
    let output = unsafe { output.assume_init() };
    Some(format!(
        "{:04}-{:02}-{:02}",
        output.tm_year + 1900,
        output.tm_mon + 1,
        output.tm_mday
    ))
}

pub fn timestamp_millis(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || !matches!(bytes.get(10), Some(b'T' | b' '))
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return None;
    }
    let number = |range: std::ops::Range<usize>| {
        std::str::from_utf8(bytes.get(range)?)
            .ok()?
            .parse::<i64>()
            .ok()
    };
    let year = number(0..4)?;
    let month = number(5..7)?;
    let day = number(8..10)?;
    let hour = number(11..13)?;
    let minute = number(14..16)?;
    let second = number(17..19)?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
    {
        return None;
    }
    let mut index = 19;
    let mut fraction_millis = 0_i64;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        let digits = bytes.get(start..index)?;
        for position in 0..3 {
            fraction_millis *= 10;
            if let Some(digit) = digits.get(position) {
                fraction_millis += i64::from(digit.saturating_sub(b'0'));
            }
        }
    }
    let offset_seconds = match bytes.get(index).copied()? {
        b'Z' | b'z' => 0,
        sign @ (b'+' | b'-') => {
            let offset_hour = number(index + 1..index + 3)?;
            let separator = bytes.get(index + 3).copied();
            let minute_start = if separator == Some(b':') {
                index + 4
            } else {
                index + 3
            };
            let offset_minute = number(minute_start..minute_start + 2)?;
            if offset_hour > 23 || offset_minute > 59 {
                return None;
            }
            let value = offset_hour * 3_600 + offset_minute * 60;
            if sign == b'+' {
                value
            } else {
                -value
            }
        }
        _ => return None,
    };
    let seconds = days_from_civil(year, month, day)
        .checked_mul(86_400)?
        .checked_add(hour * 3_600 + minute * 60 + second)?
        .checked_sub(offset_seconds)?;
    u64::try_from(seconds.checked_mul(1_000)?.checked_add(fraction_millis)?).ok()
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
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

    fn response_fixture(thread: &str, id: &str, at: &str, input: u64, output: u64) -> Value {
        serde_json::json!({"timestamp":at,"type":"token_usage_record","payload":{
            "thread_id":thread,"response_id":id,"usage":{"input_tokens":input,
            "cached_input_tokens":input/2,"output_tokens":output,"reasoning_output_tokens":output/2,
            "total_tokens":input+output}
        }})
    }

    fn write_numeric_rollout(path: &Path, thread: &str, at: &str, model: &str, events: &[Value]) {
        let mut rows = vec![
            serde_json::json!({"timestamp":at,"type":"session_meta","payload":{"id":thread}}),
            serde_json::json!({"timestamp":at,"type":"turn_context","payload":{"model":model}}),
        ];
        rows.extend_from_slice(events);
        fs::write(
            path,
            rows.iter()
                .map(|row| format!("{row}\n"))
                .collect::<String>(),
        )
        .unwrap();
    }

    #[test]
    fn explicit_compaction_resets_legacy_counters_without_recounting_continuous_ones() {
        let root = temp_dir("compaction-epochs");
        let path = root.join("source.jsonl");
        let at = "2026-09-07T12:00:00Z";
        let usage = |value| {
            serde_json::json!({"timestamp":at,"type":"event_msg","payload":{"type":"token_count","info":{
                "total_token_usage":{"input_tokens":value,"total_tokens":value}
            }}})
        };
        for (next, expected) in [(40, 140), (140, 140)] {
            write_numeric_rollout(
                &path,
                "thread",
                at,
                "gpt-6-astra",
                &[
                    usage(100),
                    serde_json::json!({"timestamp":at,"type":"compacted","payload":{}}),
                    serde_json::json!({"timestamp":at,"type":"event_msg","payload":{"type":"token_count","info":null}}),
                    usage(next),
                ],
            );
            let mut state = CodexFileState::default();
            parse_codex_tail(&path, &mut state, fs::metadata(&path).unwrap().len());
            assert_eq!(
                codex_record(&state, 1_000).unwrap().token_total,
                Some(expected)
            );
        }
        assert!(codex_oversized_prefix_is_known_non_usage(br#"{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"CommandExecution","stdout":""#));
        assert!(!codex_oversized_prefix_is_known_non_usage(br#"{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"FutureUsage","data":""#));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn response_usage_deduplicates_copies_and_survives_counter_resets_and_restart() {
        let root = temp_dir("response-accounting");
        let sessions = root.join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        let day_a = "2026-09-03T12:00:00Z";
        let day_b = "2026-09-07T12:00:00Z";
        let r1 = response_fixture("thread", "resp_one", day_a, 100, 10);
        let r2 = response_fixture("thread", "resp_two", day_a, 200, 20);
        let reset_echo = serde_json::json!({"timestamp":day_a,"type":"event_msg","payload":{"type":"token_count","info":{
            "total_token_usage":{"input_tokens":50,"output_tokens":5,"total_tokens":55},
            "last_token_usage":{"input_tokens":200,"output_tokens":20,"total_tokens":220}
        }}});
        write_numeric_rollout(
            &sessions.join("a.jsonl"),
            "thread",
            day_a,
            "gpt-5.6-sol",
            &[r1.clone(), r2.clone(), reset_echo],
        );
        write_numeric_rollout(
            &sessions.join("b.jsonl"),
            "thread",
            day_b,
            "gpt-6-astra",
            &[
                r1,
                r2,
                response_fixture("thread", "resp_three", day_b, 300, 30),
                response_fixture("parent", "resp_parent", day_b, 9_000, 900),
            ],
        );
        fs::copy(sessions.join("a.jsonl"), sessions.join("copy.jsonl")).unwrap();
        let paths = UsagePaths {
            actrealm_home: root.join("audit"),
            claude_projects: vec![],
            codex_sessions: vec![sessions],
        };
        let mut first = UsageCollector::new(paths.clone());
        let records = first.collect(1_000);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].token_total, Some(660));
        assert_eq!(records[0].input_tokens, Some(600));
        assert_eq!(records[0].cache_read_tokens, Some(300));
        assert_eq!(records[0].model.as_deref(), Some("gpt-6-astra"));
        assert_eq!(
            records[0]
                .daily_usage
                .iter()
                .map(|row| row.token_total)
                .sum::<u64>(),
            660
        );
        let a = local_day_from_timestamp(day_a).unwrap();
        let b = local_day_from_timestamp(day_b).unwrap();
        assert_eq!(
            records[0]
                .daily_usage
                .iter()
                .filter(|row| row.day == a)
                .map(|row| row.token_total)
                .sum::<u64>(),
            330
        );
        assert_eq!(
            records[0]
                .daily_usage
                .iter()
                .filter(|row| row.day == b)
                .map(|row| row.token_total)
                .sum::<u64>(),
            330
        );
        let numeric = |record: &UsageRecord| {
            record
                .daily_usage
                .iter()
                .map(|row| {
                    (
                        row.day.clone(),
                        row.model.clone(),
                        row.token_total,
                        row.input_tokens,
                        row.output_tokens,
                    )
                })
                .collect::<Vec<_>>()
        };
        for _ in 0..5 {
            let mut restarted = UsageCollector::new(paths.clone());
            let reloaded = restarted.collect(2_000);
            assert_eq!(reloaded[0].token_total, Some(660));
            assert_eq!(numeric(&reloaded[0]), numeric(&records[0]));
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_prefix_is_counted_once_before_response_records_take_over() {
        let root = temp_dir("mixed-response-accounting");
        let sessions = root.join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        let at = "2026-09-07T12:00:00Z";
        let legacy = serde_json::json!({"timestamp":at,"type":"event_msg","payload":{"type":"token_count","info":{
            "total_token_usage":{"input_tokens":100,"output_tokens":10,"total_tokens":110}
        }}});
        write_numeric_rollout(
            &sessions.join("mixed.jsonl"),
            "thread",
            at,
            "gpt-6-astra",
            &[
                legacy,
                response_fixture("thread", "resp_new", at, 200, 20),
                response_fixture("thread", "resp_new", at, 200, 20),
            ],
        );
        let mut collector = UsageCollector::new(UsagePaths {
            actrealm_home: root.join("audit"),
            claude_projects: vec![],
            codex_sessions: vec![sessions],
        });
        let records = collector.collect(1000);
        assert_eq!(records[0].token_total, Some(330));
        assert_eq!(
            records[0]
                .daily_usage
                .iter()
                .map(|row| row.token_total)
                .sum::<u64>(),
            330
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn older_usage_records_never_replace_newer_daily_facts() {
        let mut state = CodexFileState {
            session_id: Some("thread".into()),
            accounted: CodexUsage {
                input: 100,
                total: 100,
                ..Default::default()
            },
            ..Default::default()
        };
        state.daily.insert(
            ("2026-09-07".into(), "gpt-6-astra".into()),
            CodexDailyAccumulator {
                usage: state.accounted,
                message_count: 1,
            },
        );
        let mut newer = codex_record(&state, 200).unwrap();
        newer.captured_at = 200;
        state.daily.clear();
        state.daily.insert(
            ("2026-09-03".into(), "gpt-6-astra".into()),
            CodexDailyAccumulator {
                usage: state.accounted,
                message_count: 1,
            },
        );
        let mut older = codex_record(&state, 100).unwrap();
        older.captured_at = 100;
        newer.merge_from(older);
        assert_eq!(newer.daily_usage[0].day, "2026-09-07");
    }

    #[test]
    fn codex_line_prefilter_only_routes_usage_bearing_records() {
        for (line, expected) in [
            (r#"{"type":"session_meta","payload":{"id":"s"}}"#, true),
            (
                r#"{"type":"turn_context","payload":{"model":"gpt-5.6-sol"}}"#,
                true,
            ),
            (
                r#"{"type":"event_msg","payload":{"type":"token_count","info":{}}}"#,
                true,
            ),
            (
                r#"{"type":"event_msg","payload":{"type":"agent_message","message":"ignored"}}"#,
                false,
            ),
            (
                r#"{"type":"response_item","payload":{"type":"function_call","arguments":"ignored"}}"#,
                false,
            ),
        ] {
            let kind: CodexLineKind = serde_json::from_str(line).expect("valid fixture");
            assert_eq!(kind.carries_usage_metadata(), expected, "{line}");
        }
    }

    #[test]
    fn project_identity_is_stable_bounded_and_path_free() {
        let first = project_identity("/Users/private/work/ActRealm-Cloud/").expect("project");
        let second = project_identity("/Users/private/work/ActRealm-Cloud").expect("project");
        assert_eq!(first, second);
        assert_eq!(first.label, "ActRealm-Cloud");
        assert!(first.id.starts_with("sha256:"));
        assert_eq!(first.id.len(), 71);
        assert!(!first.id.contains("Users"));
        assert!(project_identity("/").is_none());

        let root = temp_dir("project-workspace-origin");
        let workspace = root.join("019f63b0-cb5a-77e0-b84a-013a493e0e57");
        for repository in ["ActRealm-Cloud", "ActRealm-Cloud-firebase"] {
            let git = workspace.join("work").join(repository).join(".git");
            fs::create_dir_all(&git).expect("create repository metadata");
            fs::write(
                git.join("config"),
                "[remote \"origin\"]\n  url = https://github.com/Frontier-Interfaces/ActRealm-Cloud.git\n",
            )
            .expect("write origin metadata");
        }
        let repository =
            project_identity(workspace.to_string_lossy().as_ref()).expect("repository identity");
        assert_eq!(repository.label, "ActRealm-Cloud");
        assert_ne!(repository.id, first.id);

        let slug_workspace = root.join("https-github-com-frontier-interfaces-actrealm");
        for (repository, remote) in [
            ("ActRealm-Cloud", "Frontier-Interfaces/ActRealm-Cloud"),
            ("Display", "Frontier-Interfaces/Display"),
        ] {
            let git = slug_workspace.join("work").join(repository).join(".git");
            fs::create_dir_all(&git).expect("create mixed repository metadata");
            fs::write(
                git.join("config"),
                format!("[remote \"origin\"]\n  url = https://github.com/{remote}.git\n"),
            )
            .expect("write mixed origin metadata");
        }
        assert_eq!(
            project_identity(slug_workspace.to_string_lossy().as_ref())
                .expect("matched repository")
                .label,
            "ActRealm-Cloud"
        );

        let linked_workspace = root.join("019f63b0-cb5a-77e0-b84a-013a493e0e58");
        let linked_git = linked_workspace.join("work/repository/.git");
        fs::create_dir_all(&linked_git).expect("create linked metadata root");
        let outside = root.join("outside-config");
        fs::write(
            &outside,
            "[remote \"origin\"]\n  url = https://private.example/Hidden.git\n",
        )
        .expect("write outside config");
        std::os::unix::fs::symlink(&outside, linked_git.join("config"))
            .expect("link outside config");
        assert_eq!(
            project_identity(linked_workspace.to_string_lossy().as_ref())
                .expect("fallback workspace identity")
                .label,
            "019f63b0-cb5a-77e0-b84a-013a493e0e58"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn oversized_codex_prefix_only_skips_versioned_non_usage_shapes() {
        for (line, expected) in [
            (
                r#"{"timestamp":"2026-08-26T00:00:00Z","type":"response_item","payload":{"type":"message"}}"#,
                true,
            ),
            (
                r#"{"timestamp":"2026-08-26T00:00:00Z","type":"compacted","payload":{"replacement_history":[]}}"#,
                true,
            ),
            (
                r#"{"timestamp":"2026-08-26T00:00:00Z","type":"world_state","payload":{}}"#,
                true,
            ),
            (
                r#"{"timestamp":"2026-08-26T00:00:00Z","type":"event_msg","payload":{"type":"image_generation_end","result":"large"}}"#,
                true,
            ),
            (
                r#"{"timestamp":"2026-08-26T00:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{}}}"#,
                false,
            ),
            (
                r#"{"timestamp":"2026-08-26T00:00:00Z","type":"event_msg","payload":{"type":"agent_message"}}"#,
                true,
            ),
            (
                r#"{"timestamp":"2026-08-26T00:00:00Z","type":"session_meta","payload":{"id":"s"}}"#,
                false,
            ),
            (
                r#"{"timestamp":{"type":"response_item"},"payload":{"type":"image_generation_end"}}"#,
                false,
            ),
            (r#"{"type":"future_provider_shape","payload":{}}"#, false),
        ] {
            assert_eq!(
                codex_oversized_prefix_is_known_non_usage(line.as_bytes()),
                expected,
                "{line}"
            );
        }
    }

    #[test]
    fn refresh_budget_boundary_never_turns_an_ordinary_line_into_partial_history() {
        let root = temp_dir("refresh-budget-line-boundary");
        let sessions = root.join("sessions");
        fs::create_dir_all(&sessions).expect("create sessions");
        let path = sessions.join("rollout-budget.jsonl");
        let mut fixture =
            b"{\"type\":\"session_meta\",\"payload\":{\"id\":\"budget-session\"}}\n".to_vec();
        let padding = "x".repeat(1_700_000);
        for _ in 0..5 {
            fixture.extend_from_slice(
                format!(
                    "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"text\":\"{padding}\"}}}}\n"
                )
                .as_bytes(),
            );
        }
        fixture.extend_from_slice(
            format!(
                "{{\"type\":\"turn_context\",\"payload\":{{\"model\":\"gpt-5.6-sol\",\"padding\":\"{}\"}}}}\n",
                "y".repeat(1_800_000)
            )
            .as_bytes(),
        );
        fixture.extend_from_slice(
            b"{\"timestamp\":\"2026-08-26T00:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":42,\"total_tokens\":42}}}}\n",
        );
        fs::write(&path, fixture).expect("write boundary fixture");

        let mut collector = UsageCollector::new(UsagePaths {
            actrealm_home: root.join("actrealm-home"),
            claude_projects: vec![],
            codex_sessions: vec![sessions],
        });
        let _ = collector.collect(100);
        assert!(!collector.is_caught_up());
        assert!(collector.is_history_complete());
        assert!(
            !collector
                .codex_files
                .get(&path)
                .expect("boundary state")
                .discarding_oversized_line
        );

        let records = collector.collect(200);
        assert!(collector.is_caught_up());
        assert!(collector.is_history_complete());
        assert_eq!(
            records
                .iter()
                .find(|record| record.provider_session_id == "budget-session")
                .and_then(|record| record.token_total),
            Some(42)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn oversized_known_non_usage_line_preserves_verified_codex_history() {
        let root = temp_dir("oversized-known-non-usage");
        let sessions = root.join("sessions");
        fs::create_dir_all(&sessions).expect("create sessions");
        let path = sessions.join("rollout-large-response.jsonl");
        let mut fixture =
            b"{\"type\":\"session_meta\",\"payload\":{\"id\":\"large-response-session\"}}\n"
                .to_vec();
        fixture.extend_from_slice(
            format!(
                "{{\"timestamp\":\"2026-08-26T00:00:00Z\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"text\":\"{}\"}}}}\n",
                "z".repeat(MAX_JSONL_LINE_BYTES as usize + 1)
            )
            .as_bytes(),
        );
        fixture.extend_from_slice(
            b"{\"timestamp\":\"2026-08-26T00:00:01Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":90,\"output_tokens\":10,\"total_tokens\":100}}}}\n",
        );
        fs::write(&path, fixture).expect("write oversized response fixture");

        let mut collector = UsageCollector::new(UsagePaths {
            actrealm_home: root.join("actrealm-home"),
            claude_projects: vec![],
            codex_sessions: vec![sessions],
        });
        let mut records = Vec::new();
        while !collector.is_caught_up() {
            records = collector.collect(100);
        }
        assert!(collector.is_history_complete());
        let record = records
            .iter()
            .find(|record| record.provider_session_id == "large-response-session")
            .expect("verified resumed record");
        assert_eq!(record.token_total, Some(100));
        assert_eq!(record.usage_quality, "official_local");
        let _ = fs::remove_dir_all(root);
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
            "{\"sessionId\":\"session-a\",\"cwd\":\"/Users/private/work/Claude-App\",",
            "\"timestamp\":\"2026-07-18T01:00:00Z\",",
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
        assert_eq!(record.project_label.as_deref(), Some("Claude-App"));
        assert!(!serde_json::to_string(&record).unwrap().contains("/Users/"));
        assert_eq!(record.token_total, Some(180));
        assert_eq!(record.context_used_tokens, Some(120));
        assert_eq!(record.daily_usage.len(), 1);
        assert_eq!(record.daily_usage[0].token_total, 180);
        assert_eq!(record.daily_usage[0].message_count, 1);
        assert_eq!(record.daily_usage[0].cost_kind.as_deref(), Some("computed"));
        assert_eq!(
            record.daily_usage[0].pricing_source.as_deref(),
            Some("anthropic_standard_2026-07-20")
        );

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
        assert_eq!(record.daily_usage[0].token_total, 370);
        assert_eq!(record.daily_usage[0].message_count, 2);
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
            "{\"timestamp\":\"2026-07-18T01:00:00Z\",\"type\":\"event_msg\",",
            "\"payload\":{\"type\":\"token_count\",\"info\":{",
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
        // 300 uncached input × $4 + 400 cached × $0.4 + 100 output × $20.
        assert_eq!(record.estimated_cost_usd_micros, Some(3_360));
        assert_eq!(record.daily_usage.len(), 1);
        assert_eq!(record.daily_usage[0].token_total, 800);
        assert_eq!(record.daily_usage[0].message_count, 1);
        assert_eq!(
            record.pricing_source.as_deref(),
            Some("openai_standard_2026-09-08")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn interleaved_codex_cumulative_snapshots_use_a_monotonic_envelope() {
        let root = temp_dir("codex-interleaved-cumulative");
        let path = root.join("rollout-interleaved.jsonl");
        let fixture = concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"interleaved-session\"}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-5.6-sol\"}}\n",
            "{\"timestamp\":\"2026-08-26T00:00:00Z\",\"type\":\"event_msg\",",
            "\"payload\":{\"type\":\"token_count\",\"info\":{",
            "\"total_token_usage\":{\"input_tokens\":90,\"output_tokens\":10,\"total_tokens\":100},",
            "\"last_token_usage\":{\"input_tokens\":9,\"output_tokens\":1,\"total_tokens\":10}}}}\n",
            "{\"timestamp\":\"2026-08-26T00:00:01Z\",\"type\":\"event_msg\",",
            "\"payload\":{\"type\":\"token_count\",\"info\":{",
            "\"total_token_usage\":{\"input_tokens\":72,\"output_tokens\":8,\"total_tokens\":80},",
            "\"last_token_usage\":{\"input_tokens\":4,\"output_tokens\":1,\"total_tokens\":5}}}}\n",
            "{\"timestamp\":\"2026-08-26T00:00:02Z\",\"type\":\"event_msg\",",
            "\"payload\":{\"type\":\"token_count\",\"info\":{",
            "\"total_token_usage\":{\"input_tokens\":99,\"output_tokens\":11,\"total_tokens\":110},",
            "\"last_token_usage\":{\"input_tokens\":9,\"output_tokens\":1,\"total_tokens\":10}}}}\n"
        );
        fs::write(&path, fixture).expect("write interleaved fixture");
        let mut state = CodexFileState::default();
        parse_codex_tail(&path, &mut state, fixture.len() as u64);
        let record = codex_record(&state, 100).expect("record");

        assert_eq!(record.token_total, Some(110));
        assert_eq!(record.input_tokens, Some(99));
        assert_eq!(record.output_tokens, Some(11));
        assert_eq!(record.daily_usage.len(), 1);
        assert_eq!(record.daily_usage[0].token_total, 110);
        assert_eq!(record.daily_usage[0].message_count, 2);
        assert_eq!(record.last_turn_tokens, Some(10));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn forked_codex_sessions_exclude_the_shared_parent_replay() {
        let root = temp_dir("codex-inherited-baseline");
        let fixture = |session: &str, child_input: u64, child_total: u64| {
            format!(
                concat!(
                    "{{\"timestamp\":\"2026-07-27T04:28:11Z\",\"type\":\"session_meta\",",
                    "\"payload\":{{\"id\":\"{}\",\"parent_thread_id\":\"parent\",",
                    "\"source\":{{\"subagent\":{{\"thread_spawn\":{{",
                    "\"parent_thread_id\":\"parent\"}}}}}}}}}}\n",
                    "{{\"type\":\"turn_context\",\"payload\":{{\"model\":\"gpt-5.6-sol\"}}}}\n",
                    "{{\"timestamp\":\"2026-07-27T04:28:11Z\",\"type\":\"event_msg\",",
                    "\"payload\":{{\"type\":\"token_count\",\"info\":{{",
                    "\"total_token_usage\":{{\"input_tokens\":900,\"output_tokens\":100,",
                    "\"total_tokens\":1000}},\"last_token_usage\":{{\"input_tokens\":90,",
                    "\"output_tokens\":10,\"total_tokens\":100}}}}}}}}\n",
                    "{{\"timestamp\":\"2026-07-27T04:28:12Z\",\"type\":\"event_msg\",",
                    "\"payload\":{{\"type\":\"token_count\",\"info\":{{",
                    "\"total_token_usage\":{{\"input_tokens\":{},\"output_tokens\":110,",
                    "\"total_tokens\":{}}},\"last_token_usage\":{{\"input_tokens\":{},",
                    "\"output_tokens\":10,\"total_tokens\":{}}}}}}}}}\n"
                ),
                session,
                900 + child_input,
                1_000 + child_total,
                child_input,
                child_total,
            )
        };
        let first = root.join("first.jsonl");
        let second = root.join("second.jsonl");
        let first_fixture = fixture("child-a", 40, 50);
        let second_fixture = fixture("child-b", 60, 70);
        fs::write(&first, &first_fixture).expect("write first child fixture");
        fs::write(&second, &second_fixture).expect("write second child fixture");

        let mut first_state = CodexFileState::default();
        let mut second_state = CodexFileState::default();
        parse_codex_tail(&first, &mut first_state, first_fixture.len() as u64);
        parse_codex_tail(&second, &mut second_state, second_fixture.len() as u64);
        let first_record = codex_record(&first_state, 100).expect("first child record");
        let second_record = codex_record(&second_state, 100).expect("second child record");

        assert_eq!(first_record.token_total, Some(50));
        assert_eq!(second_record.token_total, Some(70));
        assert_eq!(first_record.input_tokens, Some(40));
        assert_eq!(second_record.input_tokens, Some(60));
        assert_eq!(first_record.usage_source, "codex_rollout_session_local");
        assert_eq!(first_record.usage_quality, "derived");
        assert_eq!(first_record.daily_usage[0].token_total, 50);
        assert_eq!(second_record.daily_usage[0].token_total, 70);
        assert_eq!(
            first_record.token_total.unwrap() + second_record.token_total.unwrap(),
            120,
            "the shared 1,000-token parent baseline must not be counted twice"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn replayed_parent_session_meta_never_relabels_the_child_file() {
        let root = temp_dir("codex-replayed-parent-meta");
        let path = root.join("rollout-child.jsonl");
        let fixture = concat!(
            "{\"timestamp\":\"2026-08-26T00:00:00Z\",\"type\":\"session_meta\",",
            "\"payload\":{\"id\":\"child-session\",\"parent_thread_id\":\"parent-session\",",
            "\"cwd\":\"/Users/private/work/ActRealm-Cloud\"}}\n",
            "{\"timestamp\":\"2026-08-26T00:00:01Z\",\"type\":\"session_meta\",",
            "\"payload\":{\"id\":\"parent-session\"}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-5.6-sol\"}}\n",
            "{\"timestamp\":\"2026-08-26T00:00:02Z\",\"type\":\"event_msg\",",
            "\"payload\":{\"type\":\"token_count\",\"info\":{",
            "\"total_token_usage\":{\"input_tokens\":120,\"total_tokens\":120},",
            "\"last_token_usage\":{\"input_tokens\":20,\"total_tokens\":20}}}}\n"
        );
        fs::write(&path, fixture).expect("write replay fixture");
        let mut state = CodexFileState::default();
        parse_codex_tail(&path, &mut state, fixture.len() as u64);

        assert_eq!(state.session_id.as_deref(), Some("child-session"));
        assert!(state.inherited_history);
        let record = codex_record(&state, 100).expect("child record");
        assert_eq!(record.provider_session_id, "child-session");
        assert_eq!(
            record.parent_provider_session_id.as_deref(),
            Some("parent-session")
        );
        assert_eq!(record.project_label.as_deref(), Some("ActRealm-Cloud"));
        assert!(record
            .project_id
            .as_deref()
            .is_some_and(|value| value.starts_with("sha256:")));
        assert!(!serde_json::to_string(&record).unwrap().contains("/Users/"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn delayed_codex_model_relabels_imported_history_and_restores_cost() {
        let root = temp_dir("codex-delayed-model");
        let path = root.join("rollout-session-delayed-model.jsonl");
        let fixture = concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"session-delayed-model\"}}\n",
            "{\"timestamp\":\"2026-07-18T01:00:00Z\",\"type\":\"event_msg\",",
            "\"payload\":{\"type\":\"token_count\",\"info\":{",
            "\"total_token_usage\":{\"input_tokens\":700,\"cached_input_tokens\":400,",
            "\"output_tokens\":100,\"reasoning_output_tokens\":20,\"total_tokens\":800},",
            "\"last_token_usage\":{\"input_tokens\":250,\"cached_input_tokens\":200,",
            "\"output_tokens\":50,\"reasoning_output_tokens\":10,\"total_tokens\":300}}}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-5.6-sol\"}}\n"
        );
        fs::write(&path, fixture).expect("write delayed model fixture");
        let mut state = CodexFileState::default();
        parse_codex_tail(&path, &mut state, fixture.len() as u64);
        let record = codex_record(&state, 100).expect("record");

        assert_eq!(record.model.as_deref(), Some("gpt-5.6-sol"));
        assert_eq!(record.estimated_cost_usd_micros, Some(3_360));
        assert_eq!(record.daily_usage.len(), 1);
        assert_eq!(record.daily_usage[0].model.as_deref(), Some("gpt-5.6-sol"));
        assert_eq!(record.daily_usage[0].estimated_cost_usd_micros, Some(3_360));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parses_rfc3339_timestamps_without_host_time_zone_inference() {
        assert_eq!(timestamp_millis("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(timestamp_millis("1970-01-01T08:00:00+08:00"), Some(0));
        assert_eq!(timestamp_millis("1970-01-01T00:00:00.123Z"), Some(123));
        assert_eq!(timestamp_millis("not-a-timestamp"), None);
    }

    #[test]
    fn discovery_stops_after_entry_budget_when_all_files_are_old() {
        let root = temp_dir("discovery-entry-budget");
        let link_target_root = temp_dir("discovery-link-target");
        let link_target = link_target_root.join("outside-recent.jsonl");
        fs::write(&link_target, "{}\n").expect("write link target");
        std::os::unix::fs::symlink(&link_target, root.join("a-link.jsonl"))
            .expect("create symlink");

        let old = SystemTime::now()
            .checked_sub(RECENT_FILE_AGE.saturating_add(Duration::from_secs(1)))
            .expect("old timestamp");
        let old_directory = root.join("b-old");
        fs::create_dir_all(&old_directory).expect("create old directory");
        for index in 0..1_000 {
            let path = old_directory.join(format!("{index:04}.jsonl"));
            fs::write(&path, "{}\n").expect("write old fixture");
            File::open(&path)
                .expect("open old fixture")
                .set_modified(old)
                .expect("age old fixture");
        }
        let recent = root.join("z-recent.jsonl");
        fs::write(&recent, "{}\n").expect("write recent fixture");
        let older_recent = root.join("y-recent.jsonl");
        fs::write(&older_recent, "{}\n").expect("write older recent fixture");
        File::open(&older_recent)
            .expect("open older recent fixture")
            .set_modified(
                SystemTime::now()
                    .checked_sub(Duration::from_secs(1))
                    .expect("older recent timestamp"),
            )
            .expect("age older recent fixture");

        let discovery = discover_recent_files_with_budget(std::slice::from_ref(&root));
        let discovered = discovery.files;

        assert!(
            discovery.visited_entries <= MAX_DISCOVERY_VISITED_ENTRIES,
            "visited {} entries with a hard budget of {MAX_DISCOVERY_VISITED_ENTRIES}",
            discovery.visited_entries
        );
        assert_eq!(
            discovered,
            vec![recent.clone(), older_recent.clone()],
            "charged candidates must be processed newest-first even after later descent exhausts the budget"
        );
        assert!(
            !discovered
                .iter()
                .any(|path| path == &root.join("a-link.jsonl")),
            "symlinked JSONL sources must remain rejected"
        );
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(link_target_root);
    }

    #[test]
    fn discovery_charges_before_next_and_caps_metadata_work() {
        let root = temp_dir("discovery-ticket-bound");
        for index in 0..MAX_DISCOVERY_VISITED_ENTRIES {
            fs::write(root.join(format!("{index:04}.jsonl")), "{}\n")
                .expect("write recent fixture");
        }

        let mut budget = DiscoveryBudget::new();
        let discovery =
            discover_recent_files_with_budget_and_budget(std::slice::from_ref(&root), &mut budget);

        assert_eq!(discovery.visited_entries, MAX_DISCOVERY_VISITED_ENTRIES);
        assert_eq!(discovery.read_dir_nexts, MAX_DISCOVERY_VISITED_ENTRIES - 1);
        assert_eq!(discovery.metadata_checks, MAX_DISCOVERY_VISITED_ENTRIES);
        assert_eq!(discovery.files.len(), MAX_DISCOVERY_VISITED_ENTRIES - 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn expired_discovery_clock_does_no_next_or_metadata_work() {
        let root = temp_dir("discovery-expired-clock");
        fs::write(root.join("recent.jsonl"), "{}\n").expect("write fixture");
        let mut budget = DiscoveryBudget::with_elapsed_for_test(|| MAX_DISCOVERY_DURATION);

        let discovery =
            discover_recent_files_with_budget_and_budget(std::slice::from_ref(&root), &mut budget);

        assert!(discovery.files.is_empty());
        assert_eq!(discovery.visited_entries, 0);
        assert_eq!(discovery.read_dir_nexts, 0);
        assert_eq!(discovery.metadata_checks, 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn first_oversized_log_is_scanned_incrementally_instead_of_skipped() {
        let root = temp_dir("oversized-rollout");
        let sessions = root.join("sessions");
        fs::create_dir_all(&sessions).expect("create sessions directory");
        let path = sessions.join("untrusted-filename.jsonl");
        let initial = concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"authoritative-session\"}}\n",
            "{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-5.6-sol\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{",
            "\"total_token_usage\":{\"input_tokens\":1000,\"output_tokens\":100,\"total_tokens\":1100},",
            "\"last_token_usage\":{\"input_tokens\":100,\"output_tokens\":100,\"total_tokens\":200}}}}\n"
        );
        fs::write(&path, initial).expect("write oversized fixture prefix");
        // This is sparse: it exercises the 1 GiB size boundary without allocating a 1 GiB fixture.
        File::options()
            .write(true)
            .open(&path)
            .expect("open oversized fixture")
            .set_len(1_073_741_825)
            .expect("make sparse oversized fixture");

        let paths = UsagePaths {
            actrealm_home: root.join("actrealm-home"),
            claude_projects: vec![],
            codex_sessions: vec![sessions],
        };
        let mut collector = UsageCollector::new(paths);
        let first = collector.collect(100);
        let record = first
            .iter()
            .find(|record| record.provider_session_id == "authoritative-session")
            .expect("the trusted prefix is parsed before the sparse tail");
        assert_eq!(record.usage_quality, "partial");
        assert!(!collector.is_caught_up());
        assert!(!collector.is_history_complete());
        let state = collector.codex_files.get(&path).expect("source state");
        assert!(state.offset > initial.len() as u64);
        assert!(
            state.offset < state.size,
            "the old 1 GiB skip must not return"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn oversized_claude_history_is_scanned_incrementally() {
        let root = temp_dir("oversized-claude");
        let projects = root.join("projects");
        fs::create_dir_all(&projects).expect("create projects directory");
        let path = projects.join("untrusted-claude-filename.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"authoritative-claude\",\"timestamp\":\"1\",",
                "\"message\":{\"id\":\"old\",\"model\":\"claude-sonnet-5\",",
                "\"usage\":{\"input_tokens\":1000}},\"requestId\":\"old\"}\n"
            ),
        )
        .expect("write oversized Claude prefix");
        File::options()
            .write(true)
            .open(&path)
            .expect("open oversized Claude fixture")
            .set_len(1_073_741_825)
            .expect("make sparse oversized Claude fixture");

        let paths = UsagePaths {
            actrealm_home: root.join("actrealm-home"),
            claude_projects: vec![projects],
            codex_sessions: vec![],
        };
        let mut collector = UsageCollector::new(paths);
        let record = collector
            .collect(100)
            .into_iter()
            .find(|record| record.provider_session_id == "authoritative-claude")
            .expect("trusted Claude prefix is retained");
        assert_eq!(record.usage_quality, "partial");
        assert!(!collector.is_caught_up());
        assert!(!collector.is_history_complete());
        let state = collector.claude_files.get(&path).expect("source state");
        assert!(state.offset > 0);
        assert!(
            state.offset < state.size,
            "the old 1 GiB skip must not return"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn atomic_rollout_replacement_resets_usage_even_when_size_and_mtime_match() {
        let root = temp_dir("rollout-replacement");
        let sessions = root.join("sessions");
        fs::create_dir_all(&sessions).expect("create sessions directory");
        let path = sessions.join("rollout.jsonl");
        let fixture = |session: &str, total: u64| {
            let meta = serde_json::json!({
                "type": "session_meta",
                "payload": { "id": session }
            });
            let token_count = serde_json::json!({
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": {
                        "total_token_usage": { "input_tokens": total, "total_tokens": total },
                        "last_token_usage": { "input_tokens": total, "total_tokens": total }
                    }
                }
            });
            format!("{meta}\n{token_count}\n")
        };
        let first = fixture("session-a", 100);
        let replacement = fixture("session-b", 200);
        assert_eq!(first.len(), replacement.len());
        fs::write(&path, first).expect("write first rollout");
        let original_modified = fs::metadata(&path)
            .expect("first rollout metadata")
            .modified()
            .expect("first rollout modified time");

        let paths = UsagePaths {
            actrealm_home: root.join("actrealm-home"),
            claude_projects: vec![],
            codex_sessions: vec![sessions.clone()],
        };
        let mut collector = UsageCollector::new(paths);
        assert_eq!(collector.collect(100)[0].provider_session_id, "session-a");

        let staged = root.join("rollout-staged.jsonl");
        fs::write(&staged, replacement).expect("write replacement rollout");
        File::open(&staged)
            .expect("open replacement rollout")
            .set_modified(original_modified)
            .expect("preserve replacement mtime");
        fs::rename(&staged, &path).expect("atomically replace rollout");
        let after_same_size = collector.collect(200);
        assert_eq!(after_same_size.len(), 1);
        assert_eq!(after_same_size[0].provider_session_id, "session-b");
        assert_eq!(after_same_size[0].token_total, Some(200));

        let larger = fixture("session-c", 3_000);
        let staged = root.join("rollout-staged-larger.jsonl");
        fs::write(&staged, larger).expect("write larger replacement rollout");
        fs::rename(&staged, &path).expect("atomically replace larger rollout");
        let after_larger = collector.collect(300);
        assert_eq!(after_larger.len(), 1);
        assert_eq!(after_larger[0].provider_session_id, "session-c");
        assert_eq!(after_larger[0].token_total, Some(3_000));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn oversized_jsonl_line_never_reads_past_the_per_refresh_cap() {
        let mut bytes = vec![b'x'; MAX_JSONL_LINE_BYTES as usize + 1];
        bytes.push(b'\n');
        let mut reader = BufReader::new(std::io::Cursor::new(bytes));

        let (line, consumed, complete, too_large) =
            read_bounded_line(&mut reader, MAX_JSONL_LINE_BYTES as usize)
                .expect("read bounded prefix");
        assert_eq!(line.len(), MAX_JSONL_LINE_BYTES as usize);
        assert_eq!(consumed, MAX_JSONL_LINE_BYTES);
        assert!(!complete);
        assert!(too_large);

        let (_, remaining, complete, too_large) =
            read_bounded_line(&mut reader, MAX_JSONL_LINE_BYTES as usize)
                .expect("read bounded suffix");
        assert_eq!(remaining, 2);
        assert!(complete);
        assert!(!too_large);
    }

    #[test]
    fn collector_resumes_an_unchanged_oversized_line_across_refreshes() {
        let root = temp_dir("collector-oversized-resume");
        let sessions = root.join("sessions");
        fs::create_dir_all(&sessions).expect("create sessions directory");
        let path = sessions.join("rollout.jsonl");
        let meta = "{\"type\":\"session_meta\",\"payload\":{\"id\":\"resume-session\"}}\n";
        let token = concat!(
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{",
            "\"total_token_usage\":{\"input_tokens\":42,\"total_tokens\":42},",
            "\"last_token_usage\":{\"input_tokens\":42,\"total_tokens\":42}}}}\n"
        );
        let mut fixture = meta.as_bytes().to_vec();
        fixture.extend(std::iter::repeat_n(
            b'x',
            MAX_JSONL_BYTES_PER_REFRESH as usize * 2,
        ));
        fixture.push(b'\n');
        fixture.extend_from_slice(token.as_bytes());
        fs::write(&path, fixture).expect("write oversized fixture");
        let metadata = fs::metadata(&path).expect("fixture metadata");
        let modified = metadata.modified().expect("fixture mtime");

        let mut collector = UsageCollector::new(UsagePaths {
            actrealm_home: root.join("actrealm-home"),
            claude_projects: vec![],
            codex_sessions: vec![sessions],
        });
        assert!(collector.collect(100).is_empty());
        let after_first = collector
            .codex_files
            .get(&path)
            .expect("known Codex state after first refresh");
        assert!(after_first.discarding_oversized_line);
        let first_offset = after_first.offset;
        assert!(first_offset <= MAX_JSONL_BYTES_PER_REFRESH);
        assert!(after_first.cumulative.is_none());

        assert_eq!(
            fs::metadata(&path)
                .expect("unchanged fixture metadata")
                .modified()
                .expect("unchanged fixture mtime"),
            modified
        );
        assert!(collector.collect(200).is_empty());
        let after_second = collector
            .codex_files
            .get(&path)
            .expect("known Codex state after second refresh");
        assert!(after_second.discarding_oversized_line);
        assert!(after_second.offset.saturating_sub(first_offset) <= MAX_JSONL_BYTES_PER_REFRESH);
        assert!(after_second.cumulative.is_none());

        let records = collector.collect(300);
        let record = records
            .iter()
            .find(|record| record.provider_session_id == "resume-session")
            .expect("valid full line eventually parses without append");
        assert_eq!(record.token_total, Some(42));
        assert!(
            !collector
                .codex_files
                .get(&path)
                .expect("final Codex state")
                .discarding_oversized_line
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn collector_shares_the_byte_budget_fairly_across_known_files() {
        let root = temp_dir("collector-shared-byte-budget");
        let first = root.join("a-first.jsonl");
        let second = root.join("b-second.jsonl");
        for path in [&first, &second] {
            fs::write(path, b"x").expect("write oversized prefix");
            File::options()
                .write(true)
                .open(path)
                .expect("open oversized fixture")
                .set_len(MAX_JSONL_BYTES_PER_REFRESH * 2 + 1)
                .expect("make sparse oversized fixture");
        }
        let mut collector = UsageCollector::new(UsagePaths {
            actrealm_home: root.join("actrealm-home"),
            claude_projects: vec![],
            codex_sessions: vec![],
        });
        collector.known_codex = vec![first.clone(), second.clone()];
        collector.last_discovery = Some(Instant::now());

        assert!(collector.collect(100).is_empty());
        let first_round = collector
            .codex_files
            .get(&first)
            .expect("first file receives the initial turn")
            .offset;
        assert!(first_round <= MAX_JSONL_BYTES_PER_REFRESH);
        assert!(!collector.codex_files.contains_key(&second));

        assert!(collector.collect(200).is_empty());
        let second_round = collector
            .codex_files
            .get(&second)
            .expect("second file receives the next turn")
            .offset;
        let first_after_second = collector
            .codex_files
            .get(&first)
            .expect("first state remains known")
            .offset;
        assert_eq!(first_after_second, first_round);
        assert!(second_round <= MAX_JSONL_BYTES_PER_REFRESH);
        assert!(
            first_after_second
                .saturating_add(second_round)
                .saturating_sub(first_round)
                <= MAX_JSONL_BYTES_PER_REFRESH,
            "all files share one byte budget per collection"
        );

        assert!(collector.collect(300).is_empty());
        let first_after_third = collector
            .codex_files
            .get(&first)
            .expect("first file receives a later turn")
            .offset;
        assert!(
            first_after_third.saturating_sub(first_after_second) <= MAX_JSONL_BYTES_PER_REFRESH
        );
        assert_eq!(
            collector
                .codex_files
                .get(&second)
                .expect("second state remains known")
                .offset,
            second_round
        );
        assert!(
            first_after_third.saturating_sub(first_after_second) <= MAX_JSONL_BYTES_PER_REFRESH,
            "the next deterministic turn also stays within the shared budget"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn collector_reports_caught_up_only_at_every_known_end_of_file() {
        let root = temp_dir("collector-caught-up");
        let path = root.join("rollout-caught-up.jsonl");
        fs::write(&path, b"{\"type\":\"noise\"}\n").expect("write source fixture");
        let mut collector = UsageCollector::new(UsagePaths {
            actrealm_home: root.join("actrealm-home"),
            claude_projects: vec![],
            codex_sessions: vec![],
        });
        collector.known_codex = vec![path.clone()];
        collector.last_discovery = Some(Instant::now());

        assert!(!collector.is_caught_up());
        assert!(collector.collect(100).is_empty());
        assert!(collector.is_caught_up());
        assert!(collector.is_history_complete());

        let mut source = OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open source fixture");
        source
            .write_all(b"{\"type\":\"noise\"}\n")
            .expect("append source fixture");
        source.sync_all().expect("sync source fixture");
        assert!(!collector.is_caught_up());

        assert!(collector.collect(200).is_empty());
        assert!(collector.is_caught_up());
        assert!(collector.is_history_complete());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn collector_restores_private_scan_checkpoint_and_reads_only_the_appended_tail() {
        let root = temp_dir("collector-scan-checkpoint");
        let sessions = root.join("sessions");
        fs::create_dir_all(&sessions).expect("create sessions");
        let path = sessions.join("rollout-checkpoint.jsonl");
        let mut source = File::create(&path).expect("create rollout");
        let noise = format!("{}\n", "x".repeat(1_800_000));
        for _ in 0..6 {
            source
                .write_all(noise.as_bytes())
                .expect("write bounded noise");
        }
        source
            .write_all(
                concat!(
                    "{\"type\":\"session_meta\",\"payload\":{\"id\":\"checkpoint-session\"}}\n",
                    "{\"timestamp\":\"2026-08-26T00:00:00Z\",\"type\":\"event_msg\",",
                    "\"payload\":{\"type\":\"token_count\",\"info\":{",
                    "\"total_token_usage\":{\"input_tokens\":1000,\"output_tokens\":100,",
                    "\"total_tokens\":1100}}}}\n"
                )
                .as_bytes(),
            )
            .expect("write usage");
        source.sync_all().expect("sync rollout");
        let paths = UsagePaths {
            actrealm_home: root.join("actrealm-home"),
            claude_projects: vec![],
            codex_sessions: vec![sessions],
        };
        let mut first = UsageCollector::new(paths.clone());
        let _ = first.collect(100);
        assert!(
            !first.is_caught_up(),
            "the fixture must exercise an interrupted first scan"
        );
        let checkpoint_path = paths.usage_scan_checkpoint();
        let checkpoint = fs::read_to_string(&checkpoint_path).expect("checkpoint written");
        assert!(!checkpoint.contains(path.to_string_lossy().as_ref()));

        let mut completed = UsageCollector::new(paths.clone());
        let mut completed_records = Vec::new();
        while !completed.is_caught_up() {
            completed_records = completed.collect(150);
        }
        assert_eq!(
            completed_records
                .iter()
                .find(|record| record.provider_session_id == "checkpoint-session")
                .and_then(|record| record.token_total),
            Some(1_100),
            "the resumed partial checkpoint must preserve the historical prefix"
        );

        let mut source = OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("append rollout");
        source
            .write_all(
                concat!(
                    "{\"timestamp\":\"2026-08-26T00:00:01Z\",\"type\":\"event_msg\",",
                    "\"payload\":{\"type\":\"token_count\",\"info\":{",
                    "\"total_token_usage\":{\"input_tokens\":1200,\"output_tokens\":100,",
                    "\"total_tokens\":1300}}}}\n"
                )
                .as_bytes(),
            )
            .expect("append usage");
        source.sync_all().expect("sync appended usage");

        let mut resumed = UsageCollector::new(paths);
        let records = resumed.collect(200);
        assert!(
            resumed.is_caught_up(),
            "checkpoint should avoid a full replay"
        );
        let record = records
            .iter()
            .find(|record| record.provider_session_id == "checkpoint-session")
            .expect("resumed record");
        assert_eq!(record.token_total, Some(1_300));
        assert_eq!(
            record
                .daily_usage
                .iter()
                .map(|day| day.token_total)
                .sum::<u64>(),
            1_300
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
        // 1,000 Luna input × $0.2 + 1,000 Sol input × $4.
        assert_eq!(record.estimated_cost_usd_micros, Some(4_200));
        assert_eq!(
            record.pricing_source.as_deref(),
            Some("openai_standard_2026-09-08")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sol_to_astra_keeps_separate_input_output_cache_and_cost() {
        let root = temp_dir("sol-astra-switch");
        let path = root.join("rollout-switch.jsonl");
        let rows = [
            serde_json::json!({"type":"session_meta","payload":{"id":"switch"}}),
            serde_json::json!({"type":"turn_context","payload":{"model":"gpt-5.6-sol","context_window":1000}}),
            serde_json::json!({"timestamp":"2026-09-08T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{
                "total_token_usage":{"input_tokens":700,"cached_input_tokens":400,"output_tokens":100,"total_tokens":800}
            }}}),
            serde_json::json!({"type":"turn_context","payload":{"model":"gpt-6-astra","context_window":2000}}),
            serde_json::json!({"timestamp":"2026-09-08T10:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{
                "total_token_usage":{"input_tokens":1200,"cached_input_tokens":700,"output_tokens":150,"total_tokens":1350},
                "last_token_usage":{"input_tokens":500,"cached_input_tokens":300,"output_tokens":50,"total_tokens":550}
            }}}),
        ];
        let fixture = rows.iter().map(|v| format!("{v}\n")).collect::<String>();
        fs::write(&path, &fixture).unwrap();
        let mut state = CodexFileState::default();
        parse_codex_tail(&path, &mut state, fixture.len() as u64);
        let record = codex_record(&state, 1).unwrap();
        assert_eq!(record.model.as_deref(), Some("gpt-6-astra"));
        assert_eq!(record.token_total, Some(1350));
        assert_eq!(record.last_turn_tokens, Some(550));
        assert_eq!(record.context_window_tokens, Some(2000));
        let sol = record
            .daily_usage
            .iter()
            .find(|d| d.model.as_deref() == Some("gpt-5.6-sol"))
            .unwrap();
        let astra = record
            .daily_usage
            .iter()
            .find(|d| d.model.as_deref() == Some("gpt-6-astra"))
            .unwrap();
        assert_eq!(
            (sol.input_tokens, sol.output_tokens, sol.cache_read_tokens),
            (700, 100, 400)
        );
        assert_eq!(
            (
                astra.input_tokens,
                astra.output_tokens,
                astra.cache_read_tokens
            ),
            (500, 50, 300)
        );
        assert_eq!(sol.estimated_cost_usd_micros, Some(3360));
        assert_eq!(astra.estimated_cost_usd_micros, Some(4800));
        assert_eq!(record.estimated_cost_usd_micros, Some(8160));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn claude_model_switch_preserves_each_models_billable_components() {
        let root = temp_dir("claude-model-switch");
        let path = root.join("session-switch.jsonl");
        let rows = [
            serde_json::json!({"sessionId":"switch","timestamp":"2026-09-08T10:00:00Z","requestId":"r1","message":{"id":"m1","model":"claude-sonnet-4","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":20,"cache_creation_input_tokens":10}}}),
            serde_json::json!({"sessionId":"switch","timestamp":"2026-09-08T10:01:00Z","requestId":"r2","message":{"id":"m2","model":"claude-opus-4-6","usage":{"input_tokens":200,"output_tokens":60,"cache_read_input_tokens":30,"cache_creation_input_tokens":10}}}),
        ];
        let fixture = rows.iter().map(|v| format!("{v}\n")).collect::<String>();
        fs::write(&path, &fixture).unwrap();
        let mut state = ClaudeFileState::default();
        parse_claude_tail(&path, &mut state, fixture.len() as u64);
        let record = claude_record(&state, 1).unwrap();
        assert_eq!(record.model.as_deref(), Some("claude-opus-4-6"));
        assert_eq!(record.token_total, Some(480));
        let sonnet = record
            .daily_usage
            .iter()
            .find(|d| d.model.as_deref() == Some("claude-sonnet-4"))
            .unwrap();
        let opus = record
            .daily_usage
            .iter()
            .find(|d| d.model.as_deref() == Some("claude-opus-4-6"))
            .unwrap();
        assert_eq!(
            (
                sonnet.input_tokens,
                sonnet.output_tokens,
                sonnet.cache_read_tokens,
                sonnet.cache_creation_tokens
            ),
            (100, 50, 20, 10)
        );
        assert_eq!(
            (
                opus.input_tokens,
                opus.output_tokens,
                opus.cache_read_tokens,
                opus.cache_creation_tokens
            ),
            (200, 60, 30, 10)
        );
        // Each model is priced independently, including Claude's additive cache writes.
        assert_eq!(sonnet.estimated_cost_usd_micros, Some(1094));
        assert_eq!(opus.estimated_cost_usd_micros, Some(2578));
        assert_eq!(record.estimated_cost_usd_micros, Some(3672));
        fs::remove_dir_all(root).unwrap();
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
                cache_read: Some(0.5),
                cache_create: Some(6.25),
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
                cache_read: Some(0.175),
                cache_create: Some(0.0),
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
    fn models_dev_catalog_updates_exact_models_and_preserves_embedded_aliases() {
        let encoded = serde_json::to_vec(&serde_json::json!({
            "anthropic": {
                "models": {
                    "claude-opus-5": {
                        "id": "claude-opus-5",
                        "cost": {
                            "input": 5.0,
                            "output": 25.0,
                            "cache_read": 0.5,
                            "cache_write": 6.25
                        }
                    }
                }
            },
            "openai": {
                "models": {
                    "gpt-5.6-sol": {
                        "id": "gpt-5.6-sol",
                        "cost": {
                            "input": 6.0,
                            "output": 31.0,
                            "cache_read": 0.6,
                            "cache_write": 7.0
                        }
                    }
                }
            }
        }))
        .expect("encode models.dev fixture");

        let snapshot = models_dev_pricing_snapshot(&encoded).expect("valid models.dev catalog");
        let opus =
            model_pricing_in(&snapshot, "claude", "claude-opus-5").expect("new Claude model");
        assert_eq!(opus.input, 5.0);
        assert!(opus.source.starts_with("models_dev_api_"));
        let sol = model_pricing_in(&snapshot, "codex", "openai/gpt-5.6")
            .expect("embedded alias remains valid after refresh");
        assert_eq!(sol.input, 6.0);
        assert_eq!(sol.output, 31.0);
        assert_eq!(sol.cache_read, Some(0.6));
        assert_eq!(sol.cache_create, Some(7.0));
        assert!(sol.source.starts_with("models_dev_api_"));
    }

    #[test]
    fn astra_offline_rates_match_the_verified_standard_price() {
        let price = codex_price("gpt-6-astra").unwrap();
        assert_eq!(price.input, 10.0);
        assert_eq!(price.output, 50.0);
        assert_eq!(price.cache_read, Some(1.0));
        assert_eq!(price.cache_create, Some(12.5));
        assert_eq!(codex_price("gpt-5.6-sol").unwrap().input, 4.0);
        assert_eq!(codex_price("gpt-5.6-luna").unwrap().output, 1.2);
    }

    #[test]
    fn newly_published_models_are_added_without_a_code_allowlist() {
        let encoded = serde_json::to_vec(&serde_json::json!({
            "openai": {"models": {"gpt-future-fast": {"id":"gpt-future-fast", "cost":{"input":2.0,"output":8.0,"cache_read":0.2}}}},
            "anthropic": {"models": {"claude-future": {"id":"claude-future", "cost":{"input":3.0,"output":9.0,"cache_read":0.3,"cache_write":3.75}}}}
        })).unwrap();
        let catalog = models_dev_pricing_snapshot(&encoded).unwrap();
        assert_eq!(
            model_price_in(&catalog, "codex", "gpt-future-fast")
                .unwrap()
                .output,
            8.0
        );
        assert_eq!(
            model_price_in(&catalog, "claude", "claude-future")
                .unwrap()
                .input,
            3.0
        );
        assert!(model_price_in(&catalog, "codex", "gpt-unknown").is_none());
        assert!(model_price_in(&catalog, "codex", "gpt-6-astra-fast").is_none());
    }

    #[test]
    fn published_exact_model_price_takes_precedence_over_an_old_alias() {
        let mut catalog = embedded_pricing_snapshot();
        let mut exact = model_pricing_in(&catalog, "codex", "gpt-5.6-sol").unwrap();
        exact.id = "gpt-5.6".to_owned();
        exact.aliases.clear();
        exact.input = 9.0;
        catalog.models.push(exact);
        assert_eq!(
            model_price_in(&catalog, "codex", "openai/gpt-5.6")
                .unwrap()
                .input,
            9.0
        );
    }

    #[test]
    fn missing_models_trigger_early_refresh_with_bounded_retries() {
        assert!(!pricing_refresh_due(true, false, None, false));
        assert!(pricing_refresh_due(false, false, None, false));
        assert!(pricing_refresh_due(true, true, None, false));
        assert!(!pricing_refresh_due(
            true,
            true,
            Some(Duration::from_secs(30)),
            false
        ));
        assert!(pricing_refresh_due(
            true,
            true,
            Some(Duration::from_secs(300)),
            false
        ));
        assert!(pricing_refresh_due(
            true,
            false,
            Some(Duration::from_secs(1)),
            true
        ));
    }

    #[test]
    fn background_price_refresh_never_waits_or_discards_valid_rates_on_error() {
        let root = temp_dir("pricing-worker");
        let mut collector = UsageCollector::new(UsagePaths {
            actrealm_home: root.clone(),
            claude_projects: vec![],
            codex_sessions: vec![],
        });
        let original = collector.pricing.source.clone();
        let (sender, receiver) = mpsc::channel();
        collector.pricing_pending = Some(receiver);
        collector.pricing_last_attempt = Some(Instant::now());
        collector.refresh_models_dev_pricing_if_due(None);
        assert!(collector.pricing_status().updating);
        sender
            .send(Err(UsageError::Pricing("offline test".into())))
            .unwrap();
        collector.refresh_models_dev_pricing_if_due(None);
        assert_eq!(collector.pricing.source, original);
        assert!(collector.pricing_status().refresh_failed);
        assert!(!collector.pricing_status().updating);
        assert!(model_price_in(&collector.pricing, "codex", "gpt-6-astra").is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn models_dev_catalog_requires_both_supported_providers() {
        let encoded = br#"{
            "anthropic": {
                "models": {
                    "claude-opus-5": {
                        "id": "claude-opus-5",
                        "cost": {"input": 5.0, "output": 25.0}
                    }
                }
            }
        }"#;
        assert!(matches!(
            models_dev_pricing_snapshot(encoded),
            Err(UsageError::Pricing(_))
        ));
    }

    #[test]
    fn partial_codex_history_prices_observed_model_components() {
        let usage = CodexUsage {
            input: 100,
            cached: 20,
            output: 10,
            total: 110,
            ..CodexUsage::default()
        };
        let mut state = CodexFileState {
            history_complete: false,
            session_id: Some("partial-codex".to_owned()),
            model: Some("gpt-5.6-sol".to_owned()),
            incremental: usage,
            last: Some(usage),
            ..CodexFileState::default()
        };
        add_codex_daily(&mut state, Some("2026-08-18"), usage);

        let record = codex_record(&state, 100).expect("partial Codex record");
        assert_eq!(record.usage_quality, "partial");
        assert_eq!(record.estimated_cost_usd_micros, Some(528));
        assert_eq!(record.cost_kind.as_deref(), Some("computed"));
        assert_eq!(
            record.pricing_source.as_deref(),
            Some("openai_standard_2026-09-08")
        );
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
                cache_read: Some(0.2),
                cache_create: Some(2.5),
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
        let mut state = ClaudeFileState {
            session_id: Some("future".into()),
            ..ClaudeFileState::default()
        };
        upsert_claude_entry(
            &mut state,
            "future-message".into(),
            TokenEntry {
                model: Some("future-model".into()),
                input: 1000,
                output: 500,
                timestamp: "2026-09-08T00:00:00Z".into(),
                ..TokenEntry::default()
            },
        );
        let record = claude_record(&state, 1).unwrap();
        assert_eq!(record.model.as_deref(), Some("future-model"));
        assert_eq!(record.context_window_tokens, None);
        assert_eq!(record.context_used_percent, None);
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

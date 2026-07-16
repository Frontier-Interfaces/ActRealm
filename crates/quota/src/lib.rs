//! Privacy-bounded quota adapters with explicit schema and freshness gates.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;
use thiserror::Error;

const CACHE_SCHEMA_VERSION: u32 = 1;
const CLAUDE_SOURCE: &str = "statusline";
const CODEX_SOURCE: &str = "rollout_experimental";
const SUPPORTED_CODEX_VERSIONS: &[&str] = &["0.144.2", "0.144.4", "0.144.5"];
const FRESH_FOR_MS: u64 = 30 * 60 * 1_000;
const MAX_CLOCK_SKEW_MS: u64 = 5 * 60 * 1_000;
const MAX_STATUSLINE_BYTES: u64 = 256 * 1_024;
const MAX_ROLLOUT_TAIL_BYTES: u64 = 2 * 1_024 * 1_024;
const MAX_SESSION_META_BYTES: u64 = 128 * 1_024;
const MAX_ROLLOUT_FILES: usize = 256;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Error)]
pub enum QuotaError {
    #[error("quota input exceeds {0} bytes")]
    TooLarge(u64),
    #[error("unsafe symbolic link refused: {0}")]
    SymlinkRefused(PathBuf),
    #[error("quota JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("quota I/O failed for {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaEntry {
    pub provider: String,
    pub window: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<u64>,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl QuotaEntry {
    fn available(
        provider: &str,
        window: impl Into<String>,
        used_pct: f64,
        resets_at: u64,
        source: &str,
        captured_at: u64,
    ) -> Self {
        let used_pct = used_pct.clamp(0.0, 100.0);
        Self {
            provider: provider.to_owned(),
            window: window.into(),
            status: "available".to_owned(),
            used_pct: Some(used_pct),
            remaining_pct: Some(100.0 - used_pct),
            resets_at: Some(resets_at),
            source: source.to_owned(),
            captured_at: Some(captured_at),
            reason: None,
        }
    }

    fn unavailable(provider: &str, window: &str, source: &str, reason: impl Into<String>) -> Self {
        Self {
            provider: provider.to_owned(),
            window: window.to_owned(),
            status: "unavailable".to_owned(),
            used_pct: None,
            remaining_pct: None,
            resets_at: None,
            source: source.to_owned(),
            captured_at: None,
            reason: Some(reason.into()),
        }
    }

    fn stale(provider: &str, window: &str, source: &str, captured_at: u64, now_ms: u64) -> Self {
        let minutes = now_ms.saturating_sub(captured_at) / 60_000;
        Self {
            provider: provider.to_owned(),
            window: window.to_owned(),
            status: "stale".to_owned(),
            used_pct: None,
            remaining_pct: None,
            resets_at: None,
            source: source.to_owned(),
            captured_at: Some(captured_at),
            reason: Some(format!("数据已过期（{minutes} 分钟前）")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaPaths {
    pub flow_home: PathBuf,
    pub codex_sessions: PathBuf,
}

impl QuotaPaths {
    pub fn discover() -> Self {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let flow_home = env::var_os("FLOW_AGENT_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".flow-agent"));
        let codex_home = env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));
        Self {
            flow_home,
            codex_sessions: codex_home.join("sessions"),
        }
    }

    pub fn claude_cache(&self) -> PathBuf {
        self.flow_home.join("cache/claude-rl.json")
    }
}

#[derive(Debug, Clone)]
pub struct QuotaCollector {
    paths: QuotaPaths,
}

impl QuotaCollector {
    pub fn new(paths: QuotaPaths) -> Self {
        Self { paths }
    }

    pub fn paths(&self) -> &QuotaPaths {
        &self.paths
    }

    pub fn collect(&self, now_ms: u64) -> Vec<QuotaEntry> {
        let mut entries = self.collect_claude(now_ms);
        entries.extend(self.collect_codex(now_ms));
        entries
    }

    pub fn collect_claude(&self, now_ms: u64) -> Vec<QuotaEntry> {
        let path = self.paths.claude_cache();
        let bytes = match read_bounded(&path, MAX_STATUSLINE_BYTES) {
            Ok(bytes) => bytes,
            Err(QuotaError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                return unavailable_windows(
                    "claude",
                    CLAUDE_SOURCE,
                    &["5h", "7d"],
                    "额度缓存不存在；请开启 Claude 额度桥并完成一次对话",
                )
            }
            Err(error) => {
                return unavailable_windows(
                    "claude",
                    CLAUDE_SOURCE,
                    &["5h", "7d"],
                    format!("额度缓存不可读：{error}"),
                )
            }
        };
        let cache = match serde_json::from_slice::<CacheDocument>(&bytes) {
            Ok(cache)
                if cache.schema_version == CACHE_SCHEMA_VERSION
                    && cache.provider == "claude"
                    && cache.source == CLAUDE_SOURCE =>
            {
                cache
            }
            Ok(_) => {
                return unavailable_windows(
                    "claude",
                    CLAUDE_SOURCE,
                    &["5h", "7d"],
                    "额度缓存版本不兼容",
                )
            }
            Err(_) => {
                return unavailable_windows(
                    "claude",
                    CLAUDE_SOURCE,
                    &["5h", "7d"],
                    "额度缓存解析失败",
                )
            }
        };
        if cache.captured_at > now_ms.saturating_add(MAX_CLOCK_SKEW_MS) {
            return unavailable_windows(
                "claude",
                CLAUDE_SOURCE,
                &["5h", "7d"],
                "额度缓存时间晚于本机时间",
            );
        }
        if now_ms.saturating_sub(cache.captured_at) > FRESH_FOR_MS {
            return ["5h", "7d"]
                .into_iter()
                .map(|window| {
                    QuotaEntry::stale("claude", window, CLAUDE_SOURCE, cache.captured_at, now_ms)
                })
                .collect();
        }
        ["5h", "7d"]
            .into_iter()
            .map(|expected| {
                cache
                    .windows
                    .iter()
                    .find(|window| {
                        window.window == expected
                            && window.used_pct.is_finite()
                            && (0.0..=100.0).contains(&window.used_pct)
                            && window.resets_at > 0
                    })
                    .map(|window| {
                        QuotaEntry::available(
                            "claude",
                            expected,
                            window.used_pct,
                            window.resets_at,
                            CLAUDE_SOURCE,
                            cache.captured_at,
                        )
                    })
                    .unwrap_or_else(|| {
                        QuotaEntry::unavailable(
                            "claude",
                            expected,
                            CLAUDE_SOURCE,
                            "额度缓存没有这个窗口",
                        )
                    })
            })
            .collect()
    }

    pub fn collect_codex(&self, now_ms: u64) -> Vec<QuotaEntry> {
        let mut files = Vec::new();
        collect_rollouts(&self.paths.codex_sessions, 0, &mut files);
        files.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
        let Some((path, modified_at)) = files.into_iter().next() else {
            return vec![QuotaEntry::unavailable(
                "codex",
                "week",
                CODEX_SOURCE,
                "未找到 Codex rollout 文件",
            )];
        };
        let version = match read_codex_version(&path) {
            Ok(Some(version)) => version,
            Ok(None) => {
                return vec![QuotaEntry::unavailable(
                    "codex",
                    "week",
                    CODEX_SOURCE,
                    "rollout 缺少版本信息",
                )]
            }
            Err(_) => {
                return vec![QuotaEntry::unavailable(
                    "codex",
                    "week",
                    CODEX_SOURCE,
                    "rollout 无法读取",
                )]
            }
        };
        if !SUPPORTED_CODEX_VERSIONS.contains(&version.as_str()) {
            return vec![QuotaEntry::unavailable(
                "codex",
                "week",
                CODEX_SOURCE,
                format!("Codex {version} 的额度结构尚未验证"),
            )];
        }
        if modified_at > now_ms.saturating_add(MAX_CLOCK_SKEW_MS) {
            return vec![QuotaEntry::unavailable(
                "codex",
                "week",
                CODEX_SOURCE,
                "rollout 时间晚于本机时间",
            )];
        }
        if now_ms.saturating_sub(modified_at) > FRESH_FOR_MS {
            return vec![QuotaEntry::stale(
                "codex",
                "week",
                CODEX_SOURCE,
                modified_at,
                now_ms,
            )];
        }
        match read_codex_limits(&path, modified_at) {
            Ok(entries) if !entries.is_empty() => entries,
            Ok(_) => vec![QuotaEntry::unavailable(
                "codex",
                "week",
                CODEX_SOURCE,
                "最新 rollout 没有可用的 rate_limits",
            )],
            Err(_) => vec![QuotaEntry::unavailable(
                "codex",
                "week",
                CODEX_SOURCE,
                "rollout 额度结构解析失败",
            )],
        }
    }
}

fn unavailable_windows(
    provider: &str,
    source: &str,
    windows: &[&str],
    reason: impl Into<String>,
) -> Vec<QuotaEntry> {
    let reason = reason.into();
    windows
        .iter()
        .map(|window| QuotaEntry::unavailable(provider, window, source, reason.clone()))
        .collect()
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CacheDocument {
    schema_version: u32,
    provider: String,
    source: String,
    captured_at: u64,
    windows: Vec<CacheWindow>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CacheWindow {
    window: String,
    used_pct: f64,
    resets_at: u64,
}

pub fn capture_claude_statusline(
    input: &[u8],
    cache_path: &Path,
    now_ms: u64,
) -> Result<Vec<QuotaEntry>, QuotaError> {
    if input.len() as u64 > MAX_STATUSLINE_BYTES {
        return Err(QuotaError::TooLarge(MAX_STATUSLINE_BYTES));
    }
    let payload: Value = serde_json::from_slice(input)?;
    let mut windows = Vec::new();
    for (name, pointer) in [
        ("5h", "/rate_limits/five_hour"),
        ("7d", "/rate_limits/seven_day"),
    ] {
        let Some(window) = payload.pointer(pointer) else {
            continue;
        };
        let Some(used_pct) = window.get("used_percentage").and_then(Value::as_f64) else {
            continue;
        };
        let Some(resets_at) = window.get("resets_at").and_then(Value::as_u64) else {
            continue;
        };
        if !used_pct.is_finite() || !(0.0..=100.0).contains(&used_pct) || resets_at == 0 {
            continue;
        }
        windows.push(CacheWindow {
            window: name.to_owned(),
            used_pct,
            resets_at,
        });
    }
    if windows.is_empty() {
        return Ok(Vec::new());
    }
    let cache = CacheDocument {
        schema_version: CACHE_SCHEMA_VERSION,
        provider: "claude".to_owned(),
        source: CLAUDE_SOURCE.to_owned(),
        captured_at: now_ms,
        windows,
    };
    let mut bytes = serde_json::to_vec_pretty(&cache)?;
    bytes.push(b'\n');
    atomic_write(cache_path, &bytes, 0o600)?;
    Ok(cache
        .windows
        .into_iter()
        .map(|window| {
            QuotaEntry::available(
                "claude",
                window.window,
                window.used_pct,
                window.resets_at,
                CLAUDE_SOURCE,
                now_ms,
            )
        })
        .collect())
}

pub fn statusline_text(entries: &[QuotaEntry]) -> String {
    let parts = entries
        .iter()
        .filter_map(|entry| {
            entry
                .remaining_pct
                .map(|remaining| format!("{} 剩余 {:.0}%", entry.window, remaining))
        })
        .collect::<Vec<_>>();
    if parts.is_empty() {
        "Flow Agent · 额度等待首次响应".to_owned()
    } else {
        format!("Flow Agent · {}", parts.join(" · "))
    }
}

#[derive(Debug, Deserialize)]
struct SessionMetaEnvelope {
    #[serde(rename = "type")]
    kind: String,
    payload: SessionMetaPayload,
}

#[derive(Debug, Deserialize)]
struct SessionMetaPayload {
    cli_version: Option<String>,
}

fn read_codex_version(path: &Path) -> Result<Option<String>, QuotaError> {
    refuse_symlink(path)?;
    let file = File::open(path).map_err(|source| QuotaError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut reader = BufReader::new(file.take(MAX_SESSION_META_BYTES));
    let mut line = String::new();
    while reader
        .read_line(&mut line)
        .map_err(|source| QuotaError::Io {
            path: path.to_path_buf(),
            source,
        })?
        > 0
    {
        if line.contains("\"session_meta\"") {
            if let Ok(envelope) = serde_json::from_str::<SessionMetaEnvelope>(&line) {
                if envelope.kind == "session_meta" {
                    return Ok(envelope.payload.cli_version);
                }
            }
        }
        line.clear();
    }
    Ok(None)
}

#[derive(Debug, Deserialize)]
struct EventEnvelope {
    #[serde(rename = "type")]
    kind: String,
    payload: EventPayload,
}

#[derive(Debug, Deserialize)]
struct EventPayload {
    #[serde(rename = "type")]
    kind: String,
    rate_limits: Option<CodexLimits>,
}

#[derive(Debug, Deserialize)]
struct CodexLimits {
    limit_id: Option<String>,
    primary: Option<CodexWindow>,
    secondary: Option<CodexWindow>,
}

#[derive(Debug, Deserialize)]
struct CodexWindow {
    used_percent: f64,
    window_minutes: u64,
    resets_at: u64,
}

fn read_codex_limits(path: &Path, captured_at: u64) -> Result<Vec<QuotaEntry>, QuotaError> {
    refuse_symlink(path)?;
    let mut file = File::open(path).map_err(|source| QuotaError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let length = file
        .metadata()
        .map_err(|source| QuotaError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .len();
    let start = length.saturating_sub(MAX_ROLLOUT_TAIL_BYTES);
    file.seek(SeekFrom::Start(start))
        .map_err(|source| QuotaError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let mut bytes = Vec::with_capacity((length - start) as usize);
    file.read_to_end(&mut bytes)
        .map_err(|source| QuotaError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let mut lines = bytes.split(|byte| *byte == b'\n').collect::<Vec<_>>();
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    for line in lines.into_iter().rev() {
        if !contains_bytes(line, b"\"token_count\"") || !contains_bytes(line, b"\"rate_limits\"") {
            continue;
        }
        let Ok(envelope) = serde_json::from_slice::<EventEnvelope>(line) else {
            continue;
        };
        if envelope.kind != "event_msg" || envelope.payload.kind != "token_count" {
            continue;
        }
        let Some(limits) = envelope.payload.rate_limits else {
            continue;
        };
        if limits.limit_id.as_deref().is_some_and(|id| id != "codex") {
            continue;
        }
        let mut entries = Vec::new();
        for window in [limits.primary, limits.secondary].into_iter().flatten() {
            if !window.used_percent.is_finite()
                || !(0.0..=100.0).contains(&window.used_percent)
                || window.window_minutes == 0
                || window.resets_at == 0
            {
                continue;
            }
            if window.window_minutes != 10_080 {
                continue;
            }
            entries.push(QuotaEntry::available(
                "codex",
                "week",
                window.used_percent,
                window.resets_at,
                CODEX_SOURCE,
                captured_at,
            ));
        }
        return Ok(entries);
    }
    Ok(Vec::new())
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn collect_rollouts(path: &Path, depth: usize, output: &mut Vec<(PathBuf, u64)>) {
    if depth > 5 || output.len() >= MAX_ROLLOUT_FILES {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.file_name()));
    for entry in entries {
        if output.len() >= MAX_ROLLOUT_FILES {
            break;
        }
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            collect_rollouts(&path, depth + 1, output);
        } else if metadata.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
            && path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.starts_with("rollout-"))
        {
            let modified = metadata
                .modified()
                .ok()
                .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
                .and_then(|value| value.as_millis().try_into().ok())
                .unwrap_or(0);
            output.push((path, modified));
        }
    }
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, QuotaError> {
    refuse_symlink(path)?;
    let metadata = fs::metadata(path).map_err(|source| QuotaError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > limit {
        return Err(QuotaError::TooLarge(limit));
    }
    fs::read(path).map_err(|source| QuotaError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn refuse_symlink(path: &Path) -> Result<(), QuotaError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(QuotaError::SymlinkRefused(path.to_path_buf()))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(QuotaError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<(), QuotaError> {
    refuse_symlink(path)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if !parent.exists() {
        let mut builder = DirBuilder::new();
        builder
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .map_err(|source| QuotaError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
    }
    refuse_symlink(parent)?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("quota");
    let temporary = parent.join(format!(
        ".{name}.flow-agent.{}.{}.tmp",
        std::process::id(),
        TEMP_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&temporary)
            .map_err(|source| QuotaError::Io {
                path: temporary.clone(),
                source,
            })?;
        file.write_all(bytes).map_err(|source| QuotaError::Io {
            path: temporary.clone(),
            source,
        })?;
        file.sync_all().map_err(|source| QuotaError::Io {
            path: temporary.clone(),
            source,
        })?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(mode)).map_err(|source| {
            QuotaError::Io {
                path: temporary.clone(),
                source,
            }
        })?;
        fs::rename(&temporary, path).map_err(|source| QuotaError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn root(name: &str) -> PathBuf {
        let path = PathBuf::from("/tmp").join(format!(
            "flow-agent-quota-{name}-{}-{}",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn claude_capture_persists_only_valid_rate_limit_fields() {
        let root = root("claude");
        let cache = root.join("cache/claude-rl.json");
        let payload = br#"{
          "session_id":"secret-session",
          "cwd":"/private/customer-project",
          "transcript_path":"/private/transcript.jsonl",
          "rate_limits":{
            "five_hour":{"used_percentage":23.5,"resets_at":1784140000},
            "seven_day":{"used_percentage":41.2,"resets_at":1784740000}
          }
        }"#;
        let entries = capture_claude_statusline(payload, &cache, 1_784_130_000_000).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].remaining_pct, Some(76.5));
        let saved = fs::read_to_string(&cache).unwrap();
        assert!(!saved.contains("secret-session"));
        assert!(!saved.contains("customer-project"));
        assert!(!saved.contains("transcript"));
        assert_eq!(
            fs::metadata(&cache).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let collected = QuotaCollector::new(QuotaPaths {
            flow_home: root.clone(),
            codex_sessions: root.join("none"),
        })
        .collect_claude(1_784_130_100_000);
        assert_eq!(collected[1].status, "available");
    }

    #[test]
    fn stale_or_incompatible_claude_cache_never_exposes_a_percentage() {
        let stale_root = root("stale");
        let paths = QuotaPaths {
            flow_home: stale_root.clone(),
            codex_sessions: stale_root.join("none"),
        };
        capture_claude_statusline(
            br#"{"rate_limits":{"five_hour":{"used_percentage":50,"resets_at":1784140000}}}"#,
            &paths.claude_cache(),
            1_000,
        )
        .unwrap();
        let stale = QuotaCollector::new(paths.clone()).collect_claude(FRESH_FOR_MS + 2_000);
        assert_eq!(stale[0].status, "stale");
        assert_eq!(stale[0].used_pct, None);
        fs::write(
            paths.claude_cache(),
            br#"{"schemaVersion":99,"provider":"claude","source":"statusline","capturedAt":1,"windows":[]}"#,
        )
        .unwrap();
        let incompatible = QuotaCollector::new(paths).collect_claude(2);
        assert_eq!(incompatible[0].status, "unavailable");
        assert_eq!(incompatible[0].remaining_pct, None);

        let future_root = root("future-claude");
        let future_paths = QuotaPaths {
            flow_home: future_root.clone(),
            codex_sessions: future_root.join("none"),
        };
        capture_claude_statusline(
            br#"{"rate_limits":{"five_hour":{"used_percentage":50,"resets_at":1784140000}}}"#,
            &future_paths.claude_cache(),
            MAX_CLOCK_SKEW_MS + 10,
        )
        .unwrap();
        let future = QuotaCollector::new(future_paths).collect_claude(1);
        assert_eq!(future[0].status, "unavailable");
        assert_eq!(future[0].remaining_pct, None);
    }

    fn write_rollout(root: &Path, version: &str, rate_limits: &str) -> PathBuf {
        let directory = root.join("2026/07/15");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("rollout-fixture.jsonl");
        let limits: Value = serde_json::from_str(rate_limits).unwrap();
        let meta = serde_json::json!({
            "type": "session_meta",
            "payload": {
                "cli_version": version,
                "base_instructions": "must never be surfaced"
            }
        });
        let private_record = serde_json::json!({
            "type": "response_item",
            "payload": { "content": "private prompt" }
        });
        let limit_record = serde_json::json!({
            "type": "event_msg",
            "payload": { "type": "token_count", "rate_limits": limits }
        });
        let text = format!("{meta}\n{private_record}\n{limit_record}\n");
        fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn codex_rollout_is_exact_version_gated_and_reads_only_limit_records() {
        let root = root("codex");
        write_rollout(
            &root,
            "0.144.4",
            r#"{"limit_id":"codex","primary":{"used_percent":12.0,"window_minutes":300,"resets_at":1784140000},"secondary":{"used_percent":44.0,"window_minutes":10080,"resets_at":1784740000}}"#,
        );
        let now = fs::metadata(root.join("2026/07/15/rollout-fixture.jsonl"))
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let collector = QuotaCollector::new(QuotaPaths {
            flow_home: root.join("flow"),
            codex_sessions: root.clone(),
        });
        let entries = collector.collect_codex(now);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].window, "week");
        assert_eq!(entries[0].remaining_pct, Some(56.0));

        write_rollout(&root, "0.145.0", "null");
        let incompatible = collector.collect_codex(now + 1);
        assert_eq!(incompatible[0].status, "unavailable");
        assert!(incompatible[0]
            .reason
            .as_deref()
            .unwrap()
            .contains("尚未验证"));
        assert_eq!(incompatible[0].used_pct, None);
    }

    #[test]
    fn current_codex_rollout_fixture_matches_the_gated_adapter() {
        let root = root("codex-fixture");
        let directory = root.join("2026/07/15");
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("rollout-fixture.jsonl"),
            include_bytes!("../../../fixtures/codex/0.144.4/rate-limits-rollout.jsonl"),
        )
        .unwrap();
        let captured_at = fs::metadata(directory.join("rollout-fixture.jsonl"))
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let entries = QuotaCollector::new(QuotaPaths {
            flow_home: root.join("flow"),
            codex_sessions: root.clone(),
        })
        .collect_codex(captured_at);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].used_pct, Some(44.0));
        assert_eq!(entries[0].window, "week");

        let future = QuotaCollector::new(QuotaPaths {
            flow_home: root.join("future-flow"),
            codex_sessions: directory
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .to_path_buf(),
        })
        .collect_codex(captured_at.saturating_sub(MAX_CLOCK_SKEW_MS + 1));
        assert_eq!(future[0].status, "unavailable");
        assert_eq!(future[0].remaining_pct, None);
    }

    #[test]
    fn codex_0_144_5_weekly_fixture_matches_the_local_rollout_schema() {
        let root = root("codex-0-144-5");
        let directory = root.join("2026/07/16");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("rollout-fixture.jsonl");
        fs::write(
            &path,
            include_bytes!("../../../fixtures/codex/0.144.5/rate-limits-rollout.jsonl"),
        )
        .unwrap();
        let captured_at = fs::metadata(&path)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let entries = QuotaCollector::new(QuotaPaths {
            flow_home: root.join("flow"),
            codex_sessions: root.clone(),
        })
        .collect_codex(captured_at);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].window, "week");
        assert_eq!(entries[0].used_pct, Some(8.0));
        assert_eq!(entries[0].remaining_pct, Some(92.0));
    }

    #[test]
    fn codex_0_144_2_desktop_fixture_matches_the_local_rollout_schema() {
        let root = root("codex-0-144-2-desktop");
        let directory = root.join("2026/07/16");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("rollout-fixture.jsonl");
        fs::write(
            &path,
            include_bytes!("../../../fixtures/codex/0.144.2/rate-limits-rollout.jsonl"),
        )
        .unwrap();
        let captured_at = fs::metadata(&path)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let entries = QuotaCollector::new(QuotaPaths {
            flow_home: root.join("flow"),
            codex_sessions: root.clone(),
        })
        .collect_codex(captured_at);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].window, "week");
        assert_eq!(entries[0].used_pct, Some(49.0));
        assert_eq!(entries[0].remaining_pct, Some(51.0));
    }

    #[test]
    fn rollout_scan_cap_prefers_newest_lexical_session_paths() {
        let root = root("rollout-cap");
        for index in 0..=MAX_ROLLOUT_FILES {
            let directory = root.join(format!("session-{index:03}"));
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join(format!("rollout-{index:03}.jsonl")), b"{}\n").unwrap();
        }

        let mut files = Vec::new();
        collect_rollouts(&root, 0, &mut files);
        assert_eq!(files.len(), MAX_ROLLOUT_FILES);
        assert!(files
            .iter()
            .any(|(path, _)| path.to_string_lossy().contains("session-256")));
        assert!(!files
            .iter()
            .any(|(path, _)| path.to_string_lossy().contains("session-000")));
    }
}

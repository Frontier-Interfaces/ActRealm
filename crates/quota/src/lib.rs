//! Privacy-bounded quota adapters with explicit schema and freshness gates.

use actrealm_installer::{provider_cli_candidates, HookProvider};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(target_os = "macos")]
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, UNIX_EPOCH};
use thiserror::Error;

const CACHE_SCHEMA_VERSION: u32 = 1;
const CLAUDE_SOURCE: &str = "statusline";
const CLAUDE_OAUTH_SOURCE: &str = "oauth_usage";
const CODEX_SOURCE: &str = "rollout_experimental";
const MAX_CLOCK_SKEW_MS: u64 = 5 * 60 * 1_000;
const MAX_STATUSLINE_BYTES: u64 = 256 * 1_024;
const MAX_ROLLOUT_TAIL_BYTES: u64 = 2 * 1_024 * 1_024;
const MAX_SESSION_META_BYTES: u64 = 128 * 1_024;
const MAX_ROLLOUT_FILES: usize = 256;
const MAX_CREDENTIAL_BYTES: u64 = 256 * 1_024;
const MAX_OAUTH_RESPONSE_BYTES: usize = 256 * 1_024;
const OAUTH_RETRY_AFTER_MS: u64 = 60 * 1_000;
const OAUTH_REFRESH_SKEW_MS: u64 = 4 * 60 * 1_000;
const OAUTH_REFRESH_COOLDOWN_MS: u64 = 60 * 1_000;
const OAUTH_REFRESH_TIMEOUT: Duration = Duration::from_secs(12);
#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
#[cfg(target_os = "macos")]
const MAX_KEYCHAIN_DUMP_BYTES: usize = 4 * 1024 * 1024;
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
    #[error("Claude OAuth credentials are unavailable")]
    OAuthUnavailable,
    #[error("Claude OAuth usage request failed: {0}")]
    OAuthRequest(String),
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
    pub window_minutes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_type: Option<String>,
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
        Self::available_optional(
            provider,
            window,
            used_pct,
            Some(resets_at),
            source,
            captured_at,
        )
    }

    fn available_optional(
        provider: &str,
        window: impl Into<String>,
        used_pct: f64,
        resets_at: Option<u64>,
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
            resets_at,
            source: source.to_owned(),
            window_minutes: None,
            limit_id: None,
            limit_name: None,
            plan_type: None,
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
            window_minutes: None,
            limit_id: None,
            limit_name: None,
            plan_type: None,
            captured_at: None,
            reason: Some(reason.into()),
        }
    }

    fn with_metadata(
        mut self,
        window_minutes: Option<u64>,
        limit_id: Option<String>,
        limit_name: Option<String>,
        plan_type: Option<String>,
    ) -> Self {
        self.window_minutes = window_minutes;
        self.limit_id = limit_id;
        self.limit_name = limit_name;
        self.plan_type = plan_type;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaPaths {
    pub actrealm_home: PathBuf,
    pub codex_sessions: PathBuf,
}

impl QuotaPaths {
    pub fn discover() -> Self {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let actrealm_home = env::var_os("ACTREALM_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".actrealm"));
        let codex_home = env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));
        Self {
            actrealm_home,
            codex_sessions: codex_home.join("sessions"),
        }
    }

    pub fn claude_cache(&self) -> PathBuf {
        self.actrealm_home.join("cache/claude-rl.json")
    }
}

#[derive(Debug)]
pub struct QuotaCollector {
    paths: QuotaPaths,
    oauth_credential: Option<OAuthCredential>,
    oauth_retry_after: u64,
    oauth_refresh_after: u64,
}

impl QuotaCollector {
    pub fn new(paths: QuotaPaths) -> Self {
        Self {
            paths,
            oauth_credential: None,
            oauth_retry_after: 0,
            oauth_refresh_after: 0,
        }
    }

    pub fn paths(&self) -> &QuotaPaths {
        &self.paths
    }

    pub fn collect(&self, now_ms: u64) -> Vec<QuotaEntry> {
        let mut entries = self.collect_claude(now_ms);
        entries.extend(self.collect_codex(now_ms));
        entries
    }

    /// Fetches Anthropic's first-party OAuth usage endpoint when Claude Code
    /// credentials are already present. The credential remains memory-only;
    /// only validated percentages and reset timestamps enter the local cache.
    pub fn refresh_claude_oauth(&mut self, now_ms: u64) -> Result<Vec<QuotaEntry>, QuotaError> {
        if now_ms < self.oauth_retry_after {
            return Err(QuotaError::OAuthRequest(
                "temporarily rate limited".to_owned(),
            ));
        }
        if self.oauth_credential.is_none() {
            self.oauth_credential = read_oauth_credential();
        }
        // Claude Desktop/Code may be signed in while no readable credential
        // has been materialized yet (for example after wake or a provider
        // update). Ask the official CLI to reconcile its auth state once,
        // then retry the same bounded credential discovery used for expiry
        // and 401 recovery.
        if self.oauth_credential.is_none() {
            self.refresh_oauth_credential_from_provider(now_ms);
        }
        if self
            .oauth_credential
            .as_ref()
            .is_some_and(|credential| credential.should_refresh(now_ms))
        {
            self.refresh_oauth_credential_from_provider(now_ms);
        }
        let access_token = self
            .oauth_credential
            .as_ref()
            .ok_or(QuotaError::OAuthUnavailable)?
            .access_token
            .clone();
        let response = match fetch_oauth_usage(&access_token) {
            Ok(response) => response,
            Err(OAuthFetchError::Unauthorized) => {
                self.refresh_oauth_credential_from_provider(now_ms);
                let updated_token = self
                    .oauth_credential
                    .as_ref()
                    .map(|credential| credential.access_token.clone())
                    .ok_or(QuotaError::OAuthUnavailable)?;
                if updated_token == access_token {
                    return Err(self.map_oauth_error(OAuthFetchError::Unauthorized, now_ms));
                }
                fetch_oauth_usage(&updated_token)
                    .map_err(|error| self.map_oauth_error(error, now_ms))?
            }
            Err(error) => return Err(self.map_oauth_error(error, now_ms)),
        };
        let entries = oauth_entries(response, now_ms);
        if entries.is_empty() {
            return Err(QuotaError::OAuthRequest(
                "response contained no supported usage windows".to_owned(),
            ));
        }
        write_claude_cache(
            &self.paths.claude_cache(),
            CLAUDE_OAUTH_SOURCE,
            &entries,
            now_ms,
        )?;
        Ok(entries)
    }

    fn refresh_oauth_credential_from_provider(&mut self, now_ms: u64) {
        if now_ms < self.oauth_refresh_after {
            self.oauth_credential = read_oauth_credential();
            return;
        }
        self.oauth_refresh_after = now_ms.saturating_add(OAUTH_REFRESH_COOLDOWN_MS);
        let _ = refresh_oauth_via_claude_cli();
        self.oauth_credential = read_oauth_credential();
    }

    fn map_oauth_error(&mut self, error: OAuthFetchError, now_ms: u64) -> QuotaError {
        match error {
            OAuthFetchError::Unauthorized => {
                self.oauth_credential = None;
                QuotaError::OAuthRequest("credential was rejected".to_owned())
            }
            OAuthFetchError::RateLimited => {
                self.oauth_retry_after = now_ms.saturating_add(OAUTH_RETRY_AFTER_MS);
                QuotaError::OAuthRequest("temporarily rate limited".to_owned())
            }
            OAuthFetchError::Other(message) => QuotaError::OAuthRequest(message),
        }
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
                    && matches!(cache.source.as_str(), CLAUDE_SOURCE | CLAUDE_OAUTH_SOURCE) =>
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
        let entries = cache
            .windows
            .into_iter()
            .filter(|window| {
                !window.window.is_empty()
                    && window.used_pct.is_finite()
                    && (0.0..=100.0).contains(&window.used_pct)
            })
            .map(|window| {
                QuotaEntry::available_optional(
                    "claude",
                    window.window,
                    window.used_pct,
                    window.resets_at,
                    &cache.source,
                    cache.captured_at,
                )
                .with_metadata(window.window_minutes, None, window.label, None)
            })
            .collect::<Vec<_>>();
        if entries.is_empty() {
            unavailable_windows(
                "claude",
                CLAUDE_SOURCE,
                &["5h", "7d"],
                "额度缓存没有可验证窗口",
            )
        } else {
            entries
        }
    }

    pub fn collect_codex(&self, now_ms: u64) -> Vec<QuotaEntry> {
        let mut files = Vec::new();
        collect_rollouts(&self.paths.codex_sessions, 0, &mut files);
        files.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
        if files.is_empty() {
            return vec![QuotaEntry::unavailable(
                "codex",
                "unknown",
                CODEX_SOURCE,
                "未找到 Codex rollout 文件",
            )];
        }
        for (path, modified_at) in files {
            if modified_at > now_ms.saturating_add(MAX_CLOCK_SKEW_MS)
                || read_codex_version(&path).ok().flatten().is_none()
            {
                continue;
            }
            let Ok(entries) = read_codex_limits(&path, modified_at) else {
                continue;
            };
            if entries.is_empty() {
                continue;
            }
            return entries;
        }
        vec![QuotaEntry::unavailable(
            "codex",
            "unknown",
            CODEX_SOURCE,
            "rollout 中没有可验证的额度窗口",
        )]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    window_minutes: Option<u64>,
    used_pct: f64,
    resets_at: Option<u64>,
}

#[derive(Debug, Clone)]
struct OAuthCredential {
    access_token: String,
    expires_at_ms: Option<u64>,
}

impl OAuthCredential {
    fn should_refresh(&self, now_ms: u64) -> bool {
        self.expires_at_ms
            .is_some_and(|expires_at| expires_at <= now_ms.saturating_add(OAUTH_REFRESH_SKEW_MS))
    }
}

#[derive(Debug)]
enum OAuthFetchError {
    Unauthorized,
    RateLimited,
    Other(String),
}

#[derive(Debug, Deserialize)]
struct OAuthUsageResponse {
    five_hour: Option<OAuthUsageWindow>,
    seven_day: Option<OAuthUsageWindow>,
    seven_day_sonnet: Option<OAuthUsageWindow>,
    seven_day_opus: Option<OAuthUsageWindow>,
    #[serde(default)]
    limits: Vec<OAuthLimit>,
    extra_usage: Option<OAuthExtraUsage>,
}

#[derive(Debug, Deserialize)]
struct OAuthUsageWindow {
    utilization: f64,
    resets_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthLimit {
    kind: Option<String>,
    group: Option<String>,
    percent: Option<f64>,
    resets_at: Option<String>,
    #[serde(default)]
    is_active: bool,
    scope: Option<OAuthScope>,
}

#[derive(Debug, Deserialize)]
struct OAuthScope {
    model: Option<OAuthModel>,
}

#[derive(Debug, Deserialize)]
struct OAuthModel {
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthExtraUsage {
    is_enabled: bool,
    utilization: Option<f64>,
}

fn read_oauth_credential() -> Option<OAuthCredential> {
    #[cfg(target_os = "macos")]
    if let Some(credential) = read_known_keychain_oauth_credential() {
        return Some(credential);
    }
    if let Some(credential) = read_file_oauth_credential() {
        return Some(credential);
    }
    #[cfg(target_os = "macos")]
    if let Some(credential) = read_discovered_keychain_oauth_credential() {
        return Some(credential);
    }
    None
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct KeychainLocator {
    service: String,
    account: Option<String>,
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct KeychainLookupCache {
    locator: Option<KeychainLocator>,
    services: Vec<String>,
    services_discovered_at: Option<Instant>,
}

#[cfg(target_os = "macos")]
fn keychain_lookup_cache() -> &'static Mutex<KeychainLookupCache> {
    static CACHE: OnceLock<Mutex<KeychainLookupCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(KeychainLookupCache::default()))
}

#[cfg(target_os = "macos")]
fn read_known_keychain_oauth_credential() -> Option<OAuthCredential> {
    let cached = keychain_lookup_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .locator
        .clone();
    if let Some(locator) = cached {
        if let Some(credential) = read_keychain_locator(&locator) {
            return Some(credential);
        }
        keychain_lookup_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .locator = None;
    }
    try_keychain_service("Claude Code-credentials")
}

#[cfg(target_os = "macos")]
fn read_discovered_keychain_oauth_credential() -> Option<OAuthCredential> {
    for service in discovered_keychain_services() {
        if service != "Claude Code-credentials" {
            if let Some(credential) = try_keychain_service(&service) {
                return Some(credential);
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn try_keychain_service(service: &str) -> Option<OAuthCredential> {
    let username = env::var("USER").ok();
    let mut accounts = vec![Some("unknown".to_owned())];
    if let Some(username) = username {
        if username != "unknown" {
            accounts.push(Some(username));
        }
    }
    accounts.push(None);
    for account in accounts {
        let locator = KeychainLocator {
            service: service.to_owned(),
            account,
        };
        if let Some(credential) = read_keychain_locator(&locator) {
            keychain_lookup_cache()
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .locator = Some(locator);
            return Some(credential);
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn read_keychain_locator(locator: &KeychainLocator) -> Option<OAuthCredential> {
    let mut command = Command::new("/usr/bin/security");
    command.args(["find-generic-password", "-s", &locator.service]);
    if let Some(account) = locator.account.as_deref() {
        command.args(["-a", account]);
    }
    let output = command.arg("-w").output().ok()?;
    output
        .status
        .success()
        .then(|| parse_oauth_credential(&output.stdout))
        .flatten()
}

#[cfg(target_os = "macos")]
fn discovered_keychain_services() -> Vec<String> {
    {
        let cache = keychain_lookup_cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if cache
            .services_discovered_at
            .is_some_and(|discovered| discovered.elapsed() < KEYCHAIN_SERVICE_CACHE_TTL)
        {
            return cache.services.clone();
        }
    }
    let services = Command::new("/usr/bin/security")
        .args(["dump-keychain"])
        .output()
        .ok()
        .filter(|output| output.status.success() && output.stdout.len() <= MAX_KEYCHAIN_DUMP_BYTES)
        .map(|output| parse_keychain_service_names(&output.stdout))
        .unwrap_or_default();
    let mut cache = keychain_lookup_cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    cache.services = services.clone();
    cache.services_discovered_at = Some(Instant::now());
    services
}

#[cfg(target_os = "macos")]
fn parse_keychain_service_names(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut services = Vec::new();
    for line in text.lines() {
        let mut remainder = line;
        while let Some(start) = remainder.find("\"Claude Code-credentials") {
            let candidate = &remainder[start + 1..];
            let Some(end) = candidate.find('"') else {
                break;
            };
            let name = &candidate[..end];
            if name.len() <= 128 && !services.iter().any(|service| service == name) {
                services.push(name.to_owned());
            }
            remainder = &candidate[end + 1..];
        }
    }
    services
}

fn read_file_oauth_credential() -> Option<OAuthCredential> {
    let home = env::var_os("HOME").map(PathBuf::from)?;
    let config_root = env::var_os("CLAUDE_CONFIG_DIR")
        .and_then(|value| {
            value
                .to_string_lossy()
                .split(',')
                .map(str::trim)
                .find(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| home.join(".claude"));
    let path = config_root.join(".credentials.json");
    let metadata = fs::symlink_metadata(&path).ok()?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_CREDENTIAL_BYTES
    {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    parse_oauth_credential(&bytes)
}

fn parse_oauth_credential(bytes: &[u8]) -> Option<OAuthCredential> {
    if bytes.len() as u64 > MAX_CREDENTIAL_BYTES {
        return None;
    }
    let start = bytes.iter().position(|byte| *byte == b'{')?;
    let value = serde_json::from_slice::<Value>(&bytes[start..]).ok()?;
    let token = value
        .pointer("/claudeAiOauth/accessToken")
        .or_else(|| value.pointer("/claudeAiOauth/access_token"))
        .and_then(Value::as_str)?;
    let expires_at_ms = value
        .pointer("/claudeAiOauth/expiresAt")
        .or_else(|| value.pointer("/claudeAiOauth/expires_at"))
        .and_then(value_epoch)
        .map(normalize_epoch_millis);
    if token.is_empty()
        || token.len() > 16 * 1_024
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'"' | b'\\'))
    {
        return None;
    }
    Some(OAuthCredential {
        access_token: token.to_owned(),
        expires_at_ms,
    })
}

fn value_epoch(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|value| value.try_into().ok()))
        .or_else(|| {
            value
                .as_f64()
                .filter(|value| value.is_finite() && *value >= 0.0)
                .map(|value| value as u64)
        })
        .or_else(|| Value::as_str(value).and_then(|value| value.parse().ok()))
}

fn normalize_epoch_millis(value: u64) -> u64 {
    if value > 10_000_000_000 {
        value
    } else {
        value.saturating_mul(1_000)
    }
}

fn refresh_oauth_via_claude_cli() -> bool {
    provider_cli_candidates(HookProvider::Claude)
        .into_iter()
        .any(|candidate| run_claude_auth_status(&candidate, OAUTH_REFRESH_TIMEOUT))
}

fn run_claude_auth_status(executable: &Path, timeout: Duration) -> bool {
    let mut child = match Command::new(executable)
        .args(["auth", "status", "--json"])
        .env("BROWSER", "true")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if started.elapsed() < timeout => {
                thread::sleep(Duration::from_millis(50));
            }
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

fn fetch_oauth_usage(token: &str) -> Result<OAuthUsageResponse, OAuthFetchError> {
    let mut child = Command::new("/usr/bin/curl")
        .args([
            "--silent",
            "--show-error",
            "--max-time",
            "8",
            "--max-filesize",
            &MAX_OAUTH_RESPONSE_BYTES.to_string(),
            "--output",
            "-",
            "--write-out",
            "\n%{http_code}",
            "--config",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| OAuthFetchError::Other(error.to_string()))?;
    let config = format!(
        "url = \"https://api.anthropic.com/api/oauth/usage\"\n\
         header = \"Authorization: Bearer {token}\"\n\
         header = \"anthropic-beta: oauth-2025-04-20\"\n\
         header = \"Content-Type: application/json\"\n\
         user-agent = \"claude-code/actrealm\"\n"
    );
    child
        .stdin
        .take()
        .ok_or_else(|| OAuthFetchError::Other("curl stdin unavailable".to_owned()))?
        .write_all(config.as_bytes())
        .map_err(|error| OAuthFetchError::Other(error.to_string()))?;
    let output = child
        .wait_with_output()
        .map_err(|error| OAuthFetchError::Other(error.to_string()))?;
    if output.stdout.len() > MAX_OAUTH_RESPONSE_BYTES.saturating_add(8) {
        return Err(OAuthFetchError::Other(
            "response exceeded size limit".to_owned(),
        ));
    }
    let split = output
        .stdout
        .iter()
        .rposition(|byte| *byte == b'\n')
        .ok_or_else(|| OAuthFetchError::Other("response omitted HTTP status".to_owned()))?;
    let status = std::str::from_utf8(&output.stdout[split + 1..])
        .ok()
        .and_then(|value| value.trim().parse::<u16>().ok())
        .ok_or_else(|| OAuthFetchError::Other("response had invalid HTTP status".to_owned()))?;
    match status {
        200..=299 => serde_json::from_slice(&output.stdout[..split])
            .map_err(|error| OAuthFetchError::Other(error.to_string())),
        401 => Err(OAuthFetchError::Unauthorized),
        429 => Err(OAuthFetchError::RateLimited),
        value => Err(OAuthFetchError::Other(format!("HTTP {value}"))),
    }
}

fn oauth_entries(response: OAuthUsageResponse, now_ms: u64) -> Vec<QuotaEntry> {
    let mut entries = Vec::new();
    push_oauth_window(
        &mut entries,
        "5h",
        "5 小时",
        300,
        response.five_hour,
        now_ms,
    );
    push_oauth_window(
        &mut entries,
        "7d",
        "7 天",
        10_080,
        response.seven_day,
        now_ms,
    );
    push_oauth_window(
        &mut entries,
        "7d_sonnet",
        "Sonnet · 7 天",
        10_080,
        response.seven_day_sonnet,
        now_ms,
    );
    push_oauth_window(
        &mut entries,
        "7d_opus",
        "Opus · 7 天",
        10_080,
        response.seven_day_opus,
        now_ms,
    );
    let mut seen_models = Vec::<String>::new();
    for limit in response.limits {
        if !limit.is_active {
            continue;
        }
        let Some(percent) = limit.percent.filter(|value| value.is_finite()) else {
            continue;
        };
        if !(0.0..=100.0).contains(&percent) {
            continue;
        }
        let Some(model) = limit
            .scope
            .and_then(|scope| scope.model)
            .and_then(|model| model.display_name)
            .and_then(|name| bounded_label(&name))
        else {
            continue;
        };
        if seen_models.iter().any(|seen| seen == &model) {
            continue;
        }
        seen_models.push(model.clone());
        let window_minutes = if limit.group.as_deref() == Some("weekly")
            || limit
                .kind
                .as_deref()
                .is_some_and(|kind| kind.contains("weekly"))
        {
            Some(10_080)
        } else {
            None
        };
        let id = format!("scoped_{}", safe_window_component(&model));
        entries.push(
            QuotaEntry::available_optional(
                "claude",
                id,
                percent,
                limit.resets_at.as_deref().and_then(parse_rfc3339_epoch),
                CLAUDE_OAUTH_SOURCE,
                now_ms,
            )
            .with_metadata(window_minutes, None, Some(model), None),
        );
    }
    if let Some(extra) = response.extra_usage {
        if extra.is_enabled {
            if let Some(utilization) = extra
                .utilization
                .filter(|value| value.is_finite() && (0.0..=100.0).contains(value))
            {
                entries.push(
                    QuotaEntry::available_optional(
                        "claude",
                        "extra_usage",
                        utilization,
                        None,
                        CLAUDE_OAUTH_SOURCE,
                        now_ms,
                    )
                    .with_metadata(
                        None,
                        None,
                        Some("额外用量".to_owned()),
                        None,
                    ),
                );
            }
        }
    }
    entries
}

fn push_oauth_window(
    entries: &mut Vec<QuotaEntry>,
    id: &str,
    label: &str,
    minutes: u64,
    window: Option<OAuthUsageWindow>,
    now_ms: u64,
) {
    let Some(window) = window else { return };
    if !window.utilization.is_finite() || !(0.0..=100.0).contains(&window.utilization) {
        return;
    }
    entries.push(
        QuotaEntry::available_optional(
            "claude",
            id,
            window.utilization,
            window.resets_at.as_deref().and_then(parse_rfc3339_epoch),
            CLAUDE_OAUTH_SOURCE,
            now_ms,
        )
        .with_metadata(Some(minutes), None, Some(label.to_owned()), None),
    );
}

fn safe_window_component(value: &str) -> String {
    let value = value
        .chars()
        .filter_map(|character| {
            if character.is_ascii_alphanumeric() {
                Some(character.to_ascii_lowercase())
            } else if character.is_whitespace() || matches!(character, '-' | '_') {
                Some('_')
            } else {
                None
            }
        })
        .take(40)
        .collect::<String>();
    if value.is_empty() {
        "model".to_owned()
    } else {
        value
    }
}

fn parse_rfc3339_epoch(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
        || !value.ends_with('Z')
    {
        return None;
    }
    let number = |start: usize, end: usize| value.get(start..end)?.parse::<i64>().ok();
    let year = number(0, 4)?;
    let month = number(5, 7)?;
    let day = number(8, 10)?;
    let hour = number(11, 13)?;
    let minute = number(14, 16)?;
    let second = number(17, 19)?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
    {
        return None;
    }
    let adjusted_year = year - i64::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    let seconds = days
        .checked_mul(86_400)?
        .checked_add(hour * 3_600 + minute * 60 + second)?;
    u64::try_from(seconds).ok()
}

fn write_claude_cache(
    cache_path: &Path,
    source: &str,
    entries: &[QuotaEntry],
    now_ms: u64,
) -> Result<(), QuotaError> {
    let windows = entries
        .iter()
        .filter_map(|entry| {
            Some(CacheWindow {
                window: entry.window.clone(),
                label: entry.limit_name.clone(),
                window_minutes: entry.window_minutes,
                used_pct: entry.used_pct?,
                resets_at: entry.resets_at,
            })
        })
        .collect::<Vec<_>>();
    let document = CacheDocument {
        schema_version: CACHE_SCHEMA_VERSION,
        provider: "claude".to_owned(),
        source: source.to_owned(),
        captured_at: now_ms,
        windows,
    };
    let mut bytes = serde_json::to_vec_pretty(&document)?;
    bytes.push(b'\n');
    atomic_write(cache_path, &bytes, 0o600)
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
    let Some(rate_limits) = payload.get("rate_limits").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };
    for (raw_name, window) in rate_limits {
        let Some(used_pct) = window.get("used_percentage").and_then(Value::as_f64) else {
            continue;
        };
        let Some(resets_at) = window.get("resets_at").and_then(Value::as_u64) else {
            continue;
        };
        if !used_pct.is_finite() || !(0.0..=100.0).contains(&used_pct) || resets_at == 0 {
            continue;
        }
        let Some(name) = claude_window_id(raw_name) else {
            continue;
        };
        windows.push(CacheWindow {
            window: name.clone(),
            label: window
                .get("limit_name")
                .or_else(|| window.get("name"))
                .and_then(Value::as_str)
                .and_then(bounded_label),
            window_minutes: window
                .get("window_minutes")
                .and_then(Value::as_u64)
                .or_else(|| canonical_window_minutes(&name)),
            used_pct,
            resets_at: Some(resets_at),
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
            let CacheWindow {
                window,
                label,
                window_minutes,
                used_pct,
                resets_at,
            } = window;
            QuotaEntry::available_optional(
                "claude",
                window,
                used_pct,
                resets_at,
                CLAUDE_SOURCE,
                now_ms,
            )
            .with_metadata(window_minutes, None, label, None)
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
        "ActRealm · 额度等待首次响应".to_owned()
    } else {
        format!("ActRealm · {}", parts.join(" · "))
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
    limit_name: Option<String>,
    plan_type: Option<String>,
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
        let limit_id = limits.limit_id.and_then(|value| bounded_label(&value));
        let limit_name = limits.limit_name.and_then(|value| bounded_label(&value));
        let plan_type = limits.plan_type.and_then(|value| bounded_label(&value));
        let mut entries = Vec::new();
        for window in [limits.primary, limits.secondary].into_iter().flatten() {
            if !window.used_percent.is_finite()
                || !(0.0..=100.0).contains(&window.used_percent)
                || window.window_minutes == 0
                || window.resets_at == 0
            {
                continue;
            }
            entries.push(
                QuotaEntry::available(
                    "codex",
                    format!("{}m", window.window_minutes),
                    window.used_percent,
                    window.resets_at,
                    CODEX_SOURCE,
                    captured_at,
                )
                .with_metadata(
                    Some(window.window_minutes),
                    limit_id.clone(),
                    limit_name.clone(),
                    plan_type.clone(),
                ),
            );
        }
        return Ok(entries);
    }
    Ok(Vec::new())
}

fn claude_window_id(value: &str) -> Option<String> {
    let canonical = match value {
        "five_hour" => "5h".to_owned(),
        "seven_day" => "7d".to_owned(),
        other => other
            .chars()
            .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
            .take(48)
            .collect(),
    };
    (!canonical.is_empty()).then_some(canonical)
}

fn canonical_window_minutes(window: &str) -> Option<u64> {
    match window {
        "5h" => Some(300),
        "7d" => Some(10_080),
        _ => window
            .strip_suffix('m')
            .and_then(|minutes| minutes.parse::<u64>().ok())
            .filter(|minutes| *minutes > 0),
    }
}

fn bounded_label(value: &str) -> Option<String> {
    let normalized = value
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(64)
        .collect::<String>();
    (!normalized.is_empty()).then_some(normalized)
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
        ".{name}.actrealm.{}.{}.tmp",
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
            "actrealm-quota-{name}-{}-{}",
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
            "seven_day":{"used_percentage":41.2,"resets_at":1784740000},
            "fable":{"used_percentage":9.0,"resets_at":1784800000,"name":"Fable","window_minutes":1440}
          }
        }"#;
        let entries = capture_claude_statusline(payload, &cache, 1_784_130_000_000).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.window == "5h")
                .and_then(|entry| entry.remaining_pct),
            Some(76.5)
        );
        let fable = entries
            .iter()
            .find(|entry| entry.window == "fable")
            .unwrap();
        assert_eq!(fable.limit_name.as_deref(), Some("Fable"));
        assert_eq!(fable.window_minutes, Some(1_440));
        let saved = fs::read_to_string(&cache).unwrap();
        assert!(!saved.contains("secret-session"));
        assert!(!saved.contains("customer-project"));
        assert!(!saved.contains("transcript"));
        assert_eq!(
            fs::metadata(&cache).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let collected = QuotaCollector::new(QuotaPaths {
            actrealm_home: root.clone(),
            codex_sessions: root.join("none"),
        })
        .collect_claude(1_784_130_100_000);
        assert_eq!(collected[1].status, "available");
    }

    #[test]
    fn old_claude_cache_preserves_last_value_but_incompatible_data_stays_unavailable() {
        let stale_root = root("stale");
        let paths = QuotaPaths {
            actrealm_home: stale_root.clone(),
            codex_sessions: stale_root.join("none"),
        };
        capture_claude_statusline(
            br#"{"rate_limits":{"five_hour":{"used_percentage":50,"resets_at":1784140000}}}"#,
            &paths.claude_cache(),
            1_000,
        )
        .unwrap();
        let last_known = QuotaCollector::new(paths.clone()).collect_claude(86_400_000);
        assert_eq!(last_known[0].status, "available");
        assert_eq!(last_known[0].used_pct, Some(50.0));
        assert_eq!(last_known[0].remaining_pct, Some(50.0));
        assert_eq!(last_known[0].resets_at, Some(1_784_140_000));
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
            actrealm_home: future_root.clone(),
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
    fn codex_rollout_is_shape_validated_and_returns_every_limit_window() {
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
            actrealm_home: root.join("flow"),
            codex_sessions: root.clone(),
        });
        let entries = collector.collect_codex(now);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].window, "300m");
        assert_eq!(entries[0].remaining_pct, Some(88.0));
        assert_eq!(entries[1].window, "10080m");
        assert_eq!(entries[1].remaining_pct, Some(56.0));

        write_rollout(&root, "0.145.0", "null");
        let incompatible = collector.collect_codex(now + 1);
        assert_eq!(incompatible[0].status, "unavailable");
        assert!(incompatible[0]
            .reason
            .as_deref()
            .unwrap()
            .contains("没有可验证"));
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
            actrealm_home: root.join("flow"),
            codex_sessions: root.clone(),
        })
        .collect_codex(captured_at);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].used_pct, Some(12.0));
        assert_eq!(entries[0].window, "300m");
        assert_eq!(entries[1].used_pct, Some(44.0));
        assert_eq!(entries[1].window, "10080m");

        let future = QuotaCollector::new(QuotaPaths {
            actrealm_home: root.join("future-flow"),
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
            actrealm_home: root.join("flow"),
            codex_sessions: root.clone(),
        })
        .collect_codex(captured_at);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].window, "10080m");
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
            actrealm_home: root.join("flow"),
            codex_sessions: root.clone(),
        })
        .collect_codex(captured_at);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].window, "10080m");
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

    #[test]
    fn oauth_usage_parses_dynamic_scoped_limits_and_null_resets() {
        let response: OAuthUsageResponse = serde_json::from_str(
            r#"{
              "five_hour":{"utilization":63,"resets_at":"2026-07-18T10:00:00Z"},
              "seven_day":{"utilization":8,"resets_at":null},
              "limits":[
                {"kind":"weekly_scoped","group":"weekly","percent":97,
                 "resets_at":"2026-07-20T00:00:00Z","is_active":true,
                 "scope":{"model":{"display_name":"Fable"}}},
                {"kind":"weekly_scoped","group":"weekly","percent":1,
                 "resets_at":null,"is_active":false,
                 "scope":{"model":{"display_name":"Old model"}}}
              ],
              "extra_usage":{"is_enabled":true,"utilization":12.5}
            }"#,
        )
        .unwrap();
        let entries = oauth_entries(response, 123);
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].window, "5h");
        assert!(entries[0].resets_at.is_some());
        assert_eq!(entries[1].window, "7d");
        assert_eq!(entries[1].resets_at, None);
        let fable = entries
            .iter()
            .find(|entry| entry.limit_name.as_deref() == Some("Fable"))
            .unwrap();
        assert_eq!(fable.used_pct, Some(97.0));
        assert_eq!(fable.window_minutes, Some(10_080));
        assert!(entries
            .iter()
            .all(|entry| entry.limit_name.as_deref() != Some("Old model")));
    }

    #[test]
    fn oauth_cache_contains_only_validated_usage_not_credentials() {
        let credential = parse_oauth_credential(
            b"\x07{\"claudeAiOauth\":{\"accessToken\":\"secret-token-value\"}}",
        )
        .unwrap();
        assert_eq!(credential.access_token, "secret-token-value");
        assert_eq!(credential.expires_at_ms, None);
        let root = root("oauth-cache");
        let cache = root.join("cache/claude-rl.json");
        let entries = vec![QuotaEntry::available_optional(
            "claude",
            "7d",
            20.0,
            None,
            CLAUDE_OAUTH_SOURCE,
            100,
        )];
        write_claude_cache(&cache, CLAUDE_OAUTH_SOURCE, &entries, 100).unwrap();
        let saved = fs::read_to_string(cache).unwrap();
        assert!(!saved.contains("secret-token-value"));
        assert!(saved.contains("oauth_usage"));
        assert_eq!(parse_rfc3339_epoch("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339_epoch("2026-07-18T10:00:00Z"),
            Some(1_784_368_800)
        );
    }

    #[test]
    fn oauth_credentials_parse_expiry_and_refresh_before_deadline() {
        let millis = parse_oauth_credential(
            br#"{"claudeAiOauth":{"accessToken":"token","expiresAt":1900000000000}}"#,
        )
        .unwrap();
        assert_eq!(millis.expires_at_ms, Some(1_900_000_000_000));
        assert!(millis.should_refresh(1_900_000_000_000 - OAUTH_REFRESH_SKEW_MS));
        assert!(!millis.should_refresh(1_900_000_000_000 - OAUTH_REFRESH_SKEW_MS - 1));

        let seconds = parse_oauth_credential(
            br#"{"claudeAiOauth":{"access_token":"token","expires_at":"1900000000"}}"#,
        )
        .unwrap();
        assert_eq!(seconds.expires_at_ms, Some(1_900_000_000_000));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn keychain_service_discovery_is_deduplicated_and_bounded_to_claude() {
        let services = parse_keychain_service_names(
            br#"
              "svce"<blob>="unrelated"
              "svce"<blob>="Claude Code-credentials"
              "svce"<blob>="Claude Code-credentials-profile-a"
              "svce"<blob>="Claude Code-credentials-profile-a"
            "#,
        );
        assert_eq!(
            services,
            vec![
                "Claude Code-credentials".to_owned(),
                "Claude Code-credentials-profile-a".to_owned()
            ]
        );
    }

    #[test]
    fn claude_auth_status_runner_requires_a_successful_process() {
        assert!(run_claude_auth_status(
            Path::new("/usr/bin/true"),
            Duration::from_secs(1)
        ));
        assert!(!run_claude_auth_status(
            Path::new("/usr/bin/false"),
            Duration::from_secs(1)
        ));
    }
}

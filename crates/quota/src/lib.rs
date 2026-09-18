//! Privacy-bounded quota adapters with explicit schema and freshness gates.

use actrealm_installer::{provider_cli_candidates, HookProvider};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, UNIX_EPOCH};

pub mod agents;
mod claude_auth;
use thiserror::Error;

const CACHE_SCHEMA_VERSION: u32 = 1;
const CLAUDE_SOURCE: &str = "statusline";
const CLAUDE_OAUTH_SOURCE: &str = "oauth_usage";
const CODEX_SOURCE: &str = "rollout_experimental";
pub const CODEX_APP_SERVER_SOURCE: &str = "codex_app_server";
const CLAUDE_CACHE_FRESHNESS_MS: u64 = 15 * 60 * 1_000;
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
const OAUTH_REFRESH_TIMEOUT: Duration = Duration::from_secs(20);
#[cfg(target_os = "macos")]
const MAX_KEYCHAIN_DUMP_BYTES: usize = 4 * 1024 * 1024;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Error)]
pub enum QuotaError {
    #[error("Agent account quota is unavailable; check the official CLI account status")]
    AgentRequest,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_captured_at: Option<u64>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub reason_args: BTreeMap<String, String>,
}

impl QuotaEntry {
    /// Preserves the last validated values while making it explicit that the
    /// provider refresh failed. Callers must never present an old percentage
    /// as current after an authentication or transport failure.
    pub fn mark_stale(mut self, reason_code: &str, reason: impl Into<String>) -> Self {
        if self.used_pct.is_none() && self.remaining_pct.is_none() {
            return self;
        }
        self.status = "stale".to_owned();
        self.reason_code = Some(reason_code.to_owned());
        self.reason = Some(reason.into());
        self.reason_args.clear();
        self
    }

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
            reset_source: resets_at.map(|_| source.to_owned()),
            reset_captured_at: resets_at.map(|_| captured_at),
            source: source.to_owned(),
            window_minutes: None,
            limit_id: None,
            limit_name: None,
            plan_type: None,
            captured_at: Some(captured_at),
            reason: None,
            reason_code: None,
            reason_args: BTreeMap::new(),
        }
    }

    fn unavailable(
        provider: &str,
        window: &str,
        source: &str,
        reason_code: &str,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.to_owned(),
            window: window.to_owned(),
            status: "unavailable".to_owned(),
            used_pct: None,
            remaining_pct: None,
            resets_at: None,
            reset_source: None,
            reset_captured_at: None,
            source: source.to_owned(),
            window_minutes: None,
            limit_id: None,
            limit_name: None,
            plan_type: None,
            captured_at: None,
            reason: Some(reason.into()),
            reason_code: Some(reason_code.to_owned()),
            reason_args: BTreeMap::new(),
        }
    }

    fn with_reason_arg(mut self, key: &str, value: impl Into<String>) -> Self {
        self.reason_args.insert(key.to_owned(), value.into());
        self
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

    fn with_reset_metadata(
        mut self,
        reset_source: Option<String>,
        reset_captured_at: Option<u64>,
    ) -> Self {
        self.reset_source = self.resets_at.and(reset_source);
        self.reset_captured_at = self.resets_at.and(reset_captured_at);
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
        if should_reload_oauth_credential(self.oauth_credential.as_ref()) {
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
                self.oauth_credential = None;
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
        // Return the merged cache projection as well. A fresh OAuth response
        // can omit reset timestamps even while an unexpired official
        // StatusLine reset remains valid for the same window.
        Ok(self.collect_claude(now_ms))
    }

    fn refresh_oauth_credential_from_provider(&mut self, now_ms: u64) {
        if now_ms < self.oauth_refresh_after {
            self.oauth_credential = read_oauth_credential();
            return;
        }
        self.oauth_refresh_after = now_ms.saturating_add(OAUTH_REFRESH_COOLDOWN_MS);
        let previous = self.oauth_credential.clone();
        let _ = refresh_oauth_via_claude_cli(&self.paths.actrealm_home, previous.as_ref(), now_ms);
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
                    "quota.reason.cache_missing",
                    "Quota cache is missing. Enable the Claude quota bridge and complete one conversation.",
                )
            }
            Err(error) => {
                let detail = error.to_string();
                return unavailable_windows(
                    "claude",
                    CLAUDE_SOURCE,
                    &["5h", "7d"],
                    "quota.reason.cache_unreadable",
                    format!("Quota cache could not be read: {detail}"),
                )
                .into_iter()
                .map(|entry| entry.with_reason_arg("error", detail.clone()))
                .collect();
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
                    "quota.reason.cache_incompatible",
                    "Quota cache schema is incompatible.",
                )
            }
            Err(_) => {
                return unavailable_windows(
                    "claude",
                    CLAUDE_SOURCE,
                    &["5h", "7d"],
                    "quota.reason.cache_invalid",
                    "Quota cache could not be parsed.",
                )
            }
        };
        if cache.captured_at > now_ms.saturating_add(MAX_CLOCK_SKEW_MS) {
            return unavailable_windows(
                "claude",
                CLAUDE_SOURCE,
                &["5h", "7d"],
                "quota.reason.cache_from_future",
                "Quota cache timestamp is later than the local clock.",
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
                let source = window
                    .used_pct_source
                    .as_deref()
                    .unwrap_or(cache.source.as_str());
                let captured_at = window.used_pct_captured_at.unwrap_or(cache.captured_at);
                let identity = window.window.clone();
                let reset_passed = window
                    .resets_at
                    .is_some_and(|reset| reset <= now_ms / 1_000);
                let entry = QuotaEntry::available_optional(
                    "claude",
                    window.window,
                    window.used_pct,
                    window.resets_at,
                    source,
                    captured_at,
                )
                .with_metadata(window.window_minutes, Some(identity), window.label, None)
                .with_reset_metadata(window.reset_source, window.reset_captured_at);
                if now_ms.saturating_sub(captured_at) > CLAUDE_CACHE_FRESHNESS_MS || reset_passed {
                    entry.mark_stale(
                        "quota.reason.cache_stale",
                        "This is a historical quota value; waiting for a fresh Provider update.",
                    )
                } else {
                    entry
                }
            })
            .collect::<Vec<_>>();
        if entries.is_empty() {
            unavailable_windows(
                "claude",
                CLAUDE_SOURCE,
                &["5h", "7d"],
                "quota.reason.no_valid_window",
                "No verifiable quota window was found.",
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
                "quota.reason.codex_rollout_missing",
                "No Codex rollout file was found.",
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
            "quota.reason.codex_window_missing",
            "No verifiable quota window was found in the Codex rollout.",
        )]
    }
}

fn unavailable_windows(
    provider: &str,
    source: &str,
    windows: &[&str],
    reason_code: &str,
    reason: impl Into<String>,
) -> Vec<QuotaEntry> {
    let reason = reason.into();
    windows
        .iter()
        .map(|window| {
            QuotaEntry::unavailable(provider, window, source, reason_code, reason.clone())
        })
        .collect()
}

fn should_reload_oauth_credential(_credential: Option<&OAuthCredential>) -> bool {
    true
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    used_pct_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    used_pct_captured_at: Option<u64>,
    resets_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reset_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reset_captured_at: Option<u64>,
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
fn read_known_keychain_oauth_credential() -> Option<OAuthCredential> {
    // The non-profile service represents the currently selected account.
    // Never reuse a profile locator across polls: an older profile credential
    // can remain valid after the user switches accounts.
    try_keychain_service("Claude Code-credentials")
}

#[cfg(target_os = "macos")]
fn read_discovered_keychain_oauth_credential() -> Option<OAuthCredential> {
    let services = discovered_keychain_services();
    selected_profile_keychain_service(&services).and_then(try_keychain_service)
}

#[cfg(target_os = "macos")]
fn selected_profile_keychain_service(services: &[String]) -> Option<&str> {
    let mut profiles = services
        .iter()
        .filter(|service| service.as_str() != "Claude Code-credentials");
    let selected = profiles.next()?;
    profiles.next().is_none().then_some(selected.as_str())
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
    let output = bounded_command_output(
        command.arg("-w"),
        MAX_CREDENTIAL_BYTES as usize,
        Duration::from_secs(3),
    )?;
    parse_oauth_credential(&output)
}

#[cfg(target_os = "macos")]
fn discovered_keychain_services() -> Vec<String> {
    // Account switching can leave the previous profile credential valid, so
    // the bounded service-name scan is deliberately fresh on every quota poll.
    bounded_command_output(
        Command::new("/usr/bin/security").args(["dump-keychain"]),
        MAX_KEYCHAIN_DUMP_BYTES,
        Duration::from_secs(3),
    )
    .map(|output| parse_keychain_service_names(&output))
    .unwrap_or_default()
}

fn bounded_command_output(
    command: &mut Command,
    limit: usize,
    timeout: Duration,
) -> Option<Vec<u8>> {
    let mut child = command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(limit as u64 + 1).read_to_end(&mut bytes).ok()?;
        (bytes.len() <= limit).then_some(bytes)
    });
    let start = Instant::now();
    let success = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
                let _ = child.wait();
                break false;
            }
        }
    };
    // An exited command may have left a helper holding its stdout pipe.
    unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
    let bytes = reader.join().ok().flatten();
    success.then_some(bytes).flatten()
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

fn refresh_oauth_via_claude_cli(
    home: &Path,
    previous: Option<&OAuthCredential>,
    now_ms: u64,
) -> bool {
    let Some(executable) = provider_cli_candidates(HookProvider::Claude)
        .into_iter()
        .find(|candidate| candidate.is_file())
    else {
        return false;
    };
    // `auth status` is useful only as a logged-out gate, never as renewal.
    // Do not launch an interactive probe every minute for a signed-out user.
    if previous.is_none() && !claude_auth::signed_in(&executable) {
        return false;
    }
    claude_auth::refresh(
        &executable,
        &home.join("run/claude-quota-probe"),
        OAUTH_REFRESH_TIMEOUT,
        || {
            read_oauth_credential()
                .is_some_and(|current| credential_renewed(previous, &current, now_ms))
        },
    )
    .unwrap_or(false)
}

fn credential_renewed(
    previous: Option<&OAuthCredential>,
    current: &OAuthCredential,
    now_ms: u64,
) -> bool {
    !current.should_refresh(now_ms)
        && previous.is_none_or(|old| old.access_token != current.access_token)
}

fn fetch_oauth_usage(token: &str) -> Result<OAuthUsageResponse, OAuthFetchError> {
    let mut child = Command::new("/usr/bin/curl")
        .args([
            "-q",
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
    if !output.status.success() {
        return Err(OAuthFetchError::Other(
            "network request failed or timed out".to_owned(),
        ));
    }
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
        "5 hours",
        300,
        response.five_hour,
        now_ms,
    );
    push_oauth_window(
        &mut entries,
        "7d",
        "7 days",
        10_080,
        response.seven_day,
        now_ms,
    );
    push_oauth_window(
        &mut entries,
        "7d_sonnet",
        "Sonnet · 7 days",
        10_080,
        response.seven_day_sonnet,
        now_ms,
    );
    push_oauth_window(
        &mut entries,
        "7d_opus",
        "Opus · 7 days",
        10_080,
        response.seven_day_opus,
        now_ms,
    );
    let mut seen_models = Vec::<String>::new();
    for limit in response.limits {
        // is_active is not a validity flag. A nonbinding weekly scope still
        // carries current usage (including zero) and must replace old cache.
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
                        Some("Extra usage".to_owned()),
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
    let incoming = entries
        .iter()
        .filter_map(|entry| {
            Some(CacheWindow {
                window: entry.window.clone(),
                label: entry.limit_name.clone(),
                window_minutes: entry.window_minutes,
                used_pct: entry.used_pct?,
                used_pct_source: Some(entry.source.clone()),
                used_pct_captured_at: entry.captured_at.or(Some(now_ms)),
                resets_at: entry.resets_at,
                reset_source: entry.reset_source.clone(),
                reset_captured_at: entry.reset_captured_at,
            })
        })
        .collect::<Vec<_>>();
    let windows = merge_claude_windows(cache_path, incoming, now_ms);
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

fn merge_claude_windows(
    cache_path: &Path,
    mut incoming: Vec<CacheWindow>,
    now_ms: u64,
) -> Vec<CacheWindow> {
    let existing = read_bounded(cache_path, MAX_STATUSLINE_BYTES)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<CacheDocument>(&bytes).ok())
        .filter(|cache| {
            cache.schema_version == CACHE_SCHEMA_VERSION
                && cache.provider == "claude"
                && matches!(cache.source.as_str(), CLAUDE_SOURCE | CLAUDE_OAUTH_SOURCE)
                && cache.captured_at <= now_ms.saturating_add(MAX_CLOCK_SKEW_MS)
        });
    let Some(existing) = existing else {
        return incoming;
    };
    for mut previous in existing.windows {
        normalize_legacy_cache_window(&mut previous, &existing.source, existing.captured_at);
        if let Some(current) = incoming
            .iter_mut()
            .find(|current| current.window == previous.window)
        {
            merge_reset_field(current, &previous, now_ms);
        } else {
            // A StatusLine payload usually contains only core windows, while
            // OAuth can also expose scoped-model and Extra Usage buckets.
            // Preserve those independently captured percentages instead of
            // making one source erase another source's unrelated windows.
            incoming.push(previous);
        }
    }
    incoming
}

fn normalize_legacy_cache_window(window: &mut CacheWindow, source: &str, captured_at: u64) {
    if window.used_pct_source.is_none() {
        window.used_pct_source = Some(source.to_owned());
    }
    if window.used_pct_captured_at.is_none() {
        window.used_pct_captured_at = Some(captured_at);
    }
    if window.resets_at.is_some() && window.reset_source.is_none() {
        window.reset_source = Some(source.to_owned());
    }
    if window.resets_at.is_some() && window.reset_captured_at.is_none() {
        window.reset_captured_at = Some(captured_at);
    }
}

fn merge_reset_field(current: &mut CacheWindow, previous: &CacheWindow, now_ms: u64) {
    let now_seconds = now_ms / 1_000;
    let previous_valid = previous.resets_at.is_some_and(|value| value > now_seconds);
    let current_valid = current.resets_at.is_some_and(|value| value > now_seconds);
    if !previous_valid {
        if !current_valid {
            current.resets_at = None;
            current.reset_source = None;
            current.reset_captured_at = None;
        }
        return;
    }
    let previous_source = previous.reset_source.as_deref().unwrap_or("");
    let current_source = current.reset_source.as_deref().unwrap_or("");
    let previous_priority = reset_source_priority(previous_source);
    let current_priority = reset_source_priority(current_source);
    let previous_is_preferred = !current_valid
        || previous_priority > current_priority
        || (previous_priority == current_priority
            && previous.reset_captured_at.unwrap_or(0) > current.reset_captured_at.unwrap_or(0));
    if previous_is_preferred {
        current.resets_at = previous.resets_at;
        current.reset_source = previous.reset_source.clone();
        current.reset_captured_at = previous.reset_captured_at;
    }
}

fn reset_source_priority(source: &str) -> u8 {
    match source {
        CLAUDE_SOURCE => 3,
        CLAUDE_OAUTH_SOURCE => 2,
        "local_estimate" => 1,
        _ => 0,
    }
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
            used_pct_source: Some(CLAUDE_SOURCE.to_owned()),
            used_pct_captured_at: Some(now_ms),
            resets_at: Some(resets_at),
            reset_source: Some(CLAUDE_SOURCE.to_owned()),
            reset_captured_at: Some(now_ms),
        });
    }
    if windows.is_empty() {
        return Ok(Vec::new());
    }
    let entries = windows
        .into_iter()
        .map(|window| {
            let CacheWindow {
                window,
                label,
                window_minutes,
                used_pct,
                resets_at,
                ..
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
        .collect::<Vec<_>>();
    write_claude_cache(cache_path, CLAUDE_SOURCE, &entries, now_ms)?;
    Ok(entries)
}

pub fn statusline_text(entries: &[QuotaEntry]) -> String {
    let parts = entries
        .iter()
        .filter_map(|entry| {
            entry
                .remaining_pct
                .map(|remaining| format!("{} {:.0}% remaining", entry.window, remaining))
        })
        .collect::<Vec<_>>();
    if parts.is_empty() {
        "ActRealm · quota waiting for first response".to_owned()
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexAppServerWindow {
    used_percent: f64,
    window_duration_mins: Option<u64>,
    resets_at: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexAppServerSnapshot {
    limit_id: Option<String>,
    limit_name: Option<String>,
    plan_type: Option<String>,
    primary: Option<CodexAppServerWindow>,
    secondary: Option<CodexAppServerWindow>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexAppServerResponse {
    rate_limits: CodexAppServerSnapshot,
    #[serde(default)]
    rate_limits_by_limit_id: Option<BTreeMap<String, CodexAppServerSnapshot>>,
}

/// Converts the official Codex app-server account snapshot into ActRealm's
/// privacy-bounded quota contract. The response contains account-level
/// percentages and reset timestamps only; no task needs to be started.
pub fn codex_app_server_entries(value: &Value, captured_at: u64) -> Vec<QuotaEntry> {
    let Ok(response) = serde_json::from_value::<CodexAppServerResponse>(value.clone()) else {
        return Vec::new();
    };
    let snapshots = response
        .rate_limits_by_limit_id
        .filter(|snapshots| !snapshots.is_empty())
        .map(|snapshots| snapshots.into_iter().collect::<Vec<_>>())
        .unwrap_or_else(|| {
            vec![(
                response
                    .rate_limits
                    .limit_id
                    .clone()
                    .unwrap_or_else(|| "codex".to_owned()),
                response.rate_limits,
            )]
        });
    snapshots
        .into_iter()
        .flat_map(|(fallback_limit_id, snapshot)| {
            let limit_id = snapshot
                .limit_id
                .as_deref()
                .and_then(bounded_label)
                .or_else(|| bounded_label(&fallback_limit_id));
            let limit_name = snapshot.limit_name.as_deref().and_then(bounded_label);
            let plan_type = snapshot.plan_type.as_deref().and_then(bounded_label);
            [snapshot.primary, snapshot.secondary]
                .into_iter()
                .flatten()
                .filter_map(move |window| {
                    let minutes = window.window_duration_mins?;
                    if !window.used_percent.is_finite()
                        || !(0.0..=100.0).contains(&window.used_percent)
                        || minutes == 0
                    {
                        return None;
                    }
                    Some(
                        QuotaEntry::available_optional(
                            "codex",
                            format!("{minutes}m"),
                            window.used_percent,
                            window.resets_at.filter(|value| *value > 0),
                            CODEX_APP_SERVER_SOURCE,
                            captured_at,
                        )
                        .with_metadata(
                            Some(minutes),
                            limit_id.clone(),
                            limit_name.clone(),
                            plan_type.clone(),
                        ),
                    )
                })
        })
        .collect()
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

    pub(super) fn root(name: &str) -> PathBuf {
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
    fn claude_scopes_survive_cache_reload_and_expire_at_their_own_timestamp() {
        let root = root("scoped-freshness");
        let paths = QuotaPaths {
            actrealm_home: root.clone(),
            codex_sessions: root.join("none"),
        };
        let response: OAuthUsageResponse = serde_json::from_value(serde_json::json!({
            "seven_day":{"utilization":20,"resets_at":null},
            "limits":[{"kind":"weekly_scoped","group":"weekly","percent":30,"is_active":true,"scope":{"model":{"display_name":"Fable"}}}]
        })).unwrap();
        let entries = oauth_entries(response, 1_000);
        write_claude_cache(&paths.claude_cache(), CLAUDE_OAUTH_SOURCE, &entries, 1_000).unwrap();
        let collector = QuotaCollector::new(paths);
        let current = collector.collect_claude(2_000);
        let scoped = current.iter().find(|e| e.window == "scoped_fable").unwrap();
        assert_eq!(scoped.limit_name.as_deref(), Some("Fable"));
        assert_eq!(scoped.limit_id.as_deref(), Some("scoped_fable"));
        assert_eq!(scoped.status, "available");
        let old = collector.collect_claude(1_001 + CLAUDE_CACHE_FRESHNESS_MS);
        assert!(old.iter().all(|e| e.status == "stale"));
        assert!(old.iter().all(|e| e.captured_at == Some(1_000)));
        assert_eq!(
            old.iter()
                .find(|e| e.window == "scoped_fable")
                .unwrap()
                .remaining_pct,
            Some(70.0)
        );
        fs::remove_dir_all(root).unwrap();
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
    fn stale_quota_preserves_last_validated_values_and_labels_the_failure() {
        let entry = QuotaEntry::available("claude", "5h", 25.0, 200, "oauth_usage", 100)
            .mark_stale("quota.reason.claude_refresh_failed", "refresh failed");
        assert_eq!(entry.status, "stale");
        assert_eq!(entry.used_pct, Some(25.0));
        assert_eq!(entry.remaining_pct, Some(75.0));
        assert_eq!(entry.captured_at, Some(100));
        assert_eq!(
            entry.reason_code.as_deref(),
            Some("quota.reason.claude_refresh_failed")
        );
    }

    #[test]
    fn oauth_null_reset_preserves_unexpired_official_statusline_reset() {
        let root = root("merge-reset");
        let cache = root.join("cache/claude-rl.json");
        capture_claude_statusline(
            br#"{"rate_limits":{"five_hour":{"used_percentage":25,"resets_at":500}}}"#,
            &cache,
            1_000,
        )
        .unwrap();
        let oauth = vec![QuotaEntry::available_optional(
            "claude",
            "5h",
            40.0,
            None,
            CLAUDE_OAUTH_SOURCE,
            2_000,
        )];
        write_claude_cache(&cache, CLAUDE_OAUTH_SOURCE, &oauth, 2_000).unwrap();

        let collected = QuotaCollector::new(QuotaPaths {
            actrealm_home: root.clone(),
            codex_sessions: root.join("none"),
        })
        .collect_claude(2_000);
        assert_eq!(collected[0].used_pct, Some(40.0));
        assert_eq!(collected[0].source, CLAUDE_OAUTH_SOURCE);
        assert_eq!(collected[0].resets_at, Some(500));
        assert_eq!(collected[0].reset_source.as_deref(), Some(CLAUDE_SOURCE));
        assert_eq!(collected[0].reset_captured_at, Some(1_000));
    }

    #[test]
    fn expired_reset_is_not_carried_into_a_new_provider_snapshot() {
        let root = root("expired-reset");
        let cache = root.join("cache/claude-rl.json");
        capture_claude_statusline(
            br#"{"rate_limits":{"five_hour":{"used_percentage":25,"resets_at":2}}}"#,
            &cache,
            1_000,
        )
        .unwrap();
        let oauth = vec![QuotaEntry::available_optional(
            "claude",
            "5h",
            40.0,
            None,
            CLAUDE_OAUTH_SOURCE,
            3_000,
        )];
        write_claude_cache(&cache, CLAUDE_OAUTH_SOURCE, &oauth, 3_000).unwrap();

        let collected = QuotaCollector::new(QuotaPaths {
            actrealm_home: root.clone(),
            codex_sessions: root.join("none"),
        })
        .collect_claude(3_000);
        assert_eq!(collected[0].resets_at, None);
        assert_eq!(collected[0].reset_source, None);
    }

    #[test]
    fn stale_marker_does_not_claim_last_values_when_none_exist() {
        let entry = QuotaEntry::unavailable(
            "claude",
            "5h",
            "oauth_usage",
            "quota.reason.cache_missing",
            "cache missing",
        )
        .mark_stale("quota.reason.claude_refresh_failed", "refresh failed");
        assert_eq!(entry.status, "unavailable");
        assert_eq!(
            entry.reason_code.as_deref(),
            Some("quota.reason.cache_missing")
        );
        assert_eq!(entry.reason.as_deref(), Some("cache missing"));
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
        assert_eq!(last_known[0].status, "stale");
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
        assert_eq!(
            incompatible[0].reason_code.as_deref(),
            Some("quota.reason.codex_window_missing")
        );
        assert_eq!(incompatible[0].used_pct, None);
    }

    #[test]
    fn codex_app_server_snapshot_refreshes_without_a_rollout() {
        let entries = codex_app_server_entries(
            &serde_json::json!({
                "rateLimits": {
                    "limitId": "codex",
                    "limitName": "Codex",
                    "planType": "plus",
                    "primary": {
                        "usedPercent": 12,
                        "windowDurationMins": 300,
                        "resetsAt": 1_784_140_000
                    },
                    "secondary": {
                        "usedPercent": 44,
                        "windowDurationMins": 43_800,
                        "resetsAt": 1_788_219_474
                    }
                },
                "rateLimitsByLimitId": null
            }),
            1_785_744_057_000,
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].source, CODEX_APP_SERVER_SOURCE);
        assert_eq!(entries[0].remaining_pct, Some(88.0));
        assert_eq!(entries[1].window_minutes, Some(43_800));
        assert_eq!(entries[1].resets_at, Some(1_788_219_474));
    }

    #[test]
    fn codex_app_server_prefers_named_multi_bucket_limits() {
        let entries = codex_app_server_entries(
            &serde_json::json!({
                "rateLimits": {},
                "rateLimitsByLimitId": {
                    "codex": {
                        "limitId": "codex",
                        "primary": {
                            "usedPercent": 25,
                            "windowDurationMins": 300,
                            "resetsAt": null
                        }
                    },
                    "codex_bengalfox": {
                        "limitId": "codex_bengalfox",
                        "secondary": {
                            "usedPercent": 0,
                            "windowDurationMins": 10080,
                            "resetsAt": 1_787_213_254
                        }
                    }
                }
            }),
            42,
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].limit_id.as_deref(), Some("codex"));
        assert_eq!(entries[0].resets_at, None);
        assert_eq!(entries[0].captured_at, Some(42));
        assert_eq!(entries[1].window, "10080m");
        assert_eq!(entries[1].limit_id.as_deref(), Some("codex_bengalfox"));
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
                 "scope":{"model":{"display_name":"Other model"}}}
              ],
              "extra_usage":{"is_enabled":true,"utilization":12.5}
            }"#,
        )
        .unwrap();
        let entries = oauth_entries(response, 123);
        assert_eq!(entries.len(), 5);
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
        let other = entries
            .iter()
            .find(|entry| entry.limit_name.as_deref() == Some("Other model"))
            .unwrap();
        assert_eq!(other.used_pct, Some(1.0));
        assert_eq!(other.resets_at, None);
    }

    #[test]
    fn nonbinding_fable_zero_replaces_stale_cache_and_survives_statusline_merge() {
        let root = root("fable-nonbinding-refresh");
        let paths = QuotaPaths {
            actrealm_home: root.clone(),
            codex_sessions: root.join("none"),
        };
        let cache = paths.claude_cache();
        let now = 1_789_563_200_000;
        let old = QuotaEntry::available_optional(
            "claude",
            "scoped_fable",
            29.0,
            None,
            CLAUDE_OAUTH_SOURCE,
            now - 14 * 24 * 60 * 60 * 1_000,
        )
        .with_metadata(Some(10_080), None, Some("Fable".into()), None);
        write_claude_cache(
            &cache,
            CLAUDE_OAUTH_SOURCE,
            &[old],
            now - 14 * 24 * 60 * 60 * 1_000,
        )
        .unwrap();
        let response: OAuthUsageResponse = serde_json::from_value(serde_json::json!({
            "five_hour":{"utilization":0,"resets_at":null},
            "seven_day":{"utilization":0,"resets_at":null},
            "limits":[{"kind":"weekly_scoped","group":"weekly","percent":0,
                "is_active":false,"resets_at":"2026-09-23T04:00:00Z",
                "scope":{"model":{"id":null,"display_name":"Fable"}}}]
        }))
        .unwrap();
        let entries = oauth_entries(response, now);
        write_claude_cache(&cache, CLAUDE_OAUTH_SOURCE, &entries, now).unwrap();
        capture_claude_statusline(
            br#"{"rate_limits":{"five_hour":{"used_percentage":1,"resets_at":1790000000}}}"#,
            &cache,
            now + 1_000,
        )
        .unwrap();
        let collector = QuotaCollector::new(paths);
        let snapshot = collector.collect_claude(now + 2_000);
        let fable = snapshot
            .iter()
            .find(|entry| entry.window == "scoped_fable")
            .unwrap();
        assert_eq!(fable.status, "available");
        assert_eq!(fable.used_pct, Some(0.0));
        assert_eq!(fable.remaining_pct, Some(100.0));
        assert_eq!(fable.captured_at, Some(now));
        assert_eq!(fable.resets_at, Some(1_790_136_000));
        assert_eq!(fable.source, CLAUDE_OAUTH_SOURCE);
        fs::remove_dir_all(root).unwrap();
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

    #[test]
    fn every_poll_rediscovers_the_current_claude_credential() {
        let credential = OAuthCredential {
            access_token: "still-valid-old-account".to_owned(),
            expires_at_ms: Some(u64::MAX),
        };
        assert!(should_reload_oauth_credential(Some(&credential)));
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

    #[cfg(target_os = "macos")]
    #[test]
    fn multiple_valid_profile_services_fail_closed_instead_of_guessing() {
        let services = vec![
            "Claude Code-credentials-profile-a".to_owned(),
            "Claude Code-credentials-profile-b".to_owned(),
        ];
        assert_eq!(selected_profile_keychain_service(&services), None);
    }

    #[test]
    fn renewal_requires_a_changed_nonexpired_credential() {
        let old = OAuthCredential {
            access_token: "old".into(),
            expires_at_ms: Some(1_000),
        };
        let fresh = OAuthCredential {
            access_token: "new".into(),
            expires_at_ms: Some(1_000_000),
        };
        assert!(credential_renewed(Some(&old), &fresh, 2_000));
        assert!(!credential_renewed(Some(&fresh), &fresh, 2_000));
        assert!(!credential_renewed(None, &old, 2_000));
        assert!(credential_renewed(None, &fresh, 2_000));
        assert!(parse_oauth_credential(
            br#"{"claudeAiOauth":{"accessToken":"","refreshToken":"","expiresAt":0}}"#
        )
        .is_none());
    }
}

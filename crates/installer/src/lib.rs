use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use toml_edit::DocumentMut;

const CONFIG_LIMIT_BYTES: u64 = 4 * 1024 * 1024;
const STATE_SCHEMA_VERSION: u32 = 1;
const CLAUDE_ORIGINAL_STATUSLINE_KEY: &str = "_flowAgentOriginalStatusLine";
static UNIQUE_FILE_ID: AtomicU64 = AtomicU64::new(0);

const CLAUDE_EVENTS: &[&str] = &[
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "PermissionRequest",
    "PermissionDenied",
    "Elicitation",
    "Notification",
    "Stop",
    "StopFailure",
    "SubagentStart",
    "SubagentStop",
    "TaskCreated",
    "TaskCompleted",
    "PreCompact",
];
const CODEX_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PermissionRequest",
    "Stop",
];
const CODEX_ENHANCED_EVENTS: &[&str] = &["PreToolUse", "PostToolUse"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookProvider {
    Claude,
    Codex,
}

impl HookProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAvailability {
    pub cli_path: Option<PathBuf>,
    pub desktop_app_path: Option<PathBuf>,
    pub bundled_cli_path: Option<PathBuf>,
}

impl ProviderAvailability {
    pub fn is_available(&self) -> bool {
        self.cli_path.is_some() || self.desktop_app_path.is_some()
    }

    pub fn version_executable(&self) -> Option<&Path> {
        self.cli_path
            .as_deref()
            .or(self.bundled_cli_path.as_deref())
    }

    pub fn codex_review_command(&self) -> Option<String> {
        self.cli_path
            .as_ref()
            .or(self.bundled_cli_path.as_ref())
            .map(|path| shell_quote(path))
    }
}

pub fn discover_provider_availability(provider: HookProvider) -> ProviderAvailability {
    let cli_path = find_executable_in_path(provider.as_str());
    let mut desktop_app_path = None;
    let mut bundled_cli_path = None;

    #[cfg(target_os = "macos")]
    {
        for root in application_roots() {
            let candidates: &[(&str, &str)] = match provider {
                HookProvider::Claude => &[("Claude.app", "Contents/MacOS/Claude")],
                HookProvider::Codex => &[
                    ("ChatGPT.app", "Contents/Resources/codex"),
                    ("Codex.app", "Contents/Resources/codex"),
                ],
            };
            for (bundle, executable) in candidates {
                let app = root.join(bundle);
                let runtime = app.join(executable);
                if is_executable_file(&runtime) {
                    desktop_app_path = Some(app);
                    if provider == HookProvider::Codex {
                        bundled_cli_path = Some(runtime);
                    }
                    break;
                }
            }
            if desktop_app_path.is_some() {
                break;
            }
        }
    }

    ProviderAvailability {
        cli_path,
        desktop_app_path,
        bundled_cli_path,
    }
}

/// Returns whether the effective Codex profile delegates approvals to Codex's
/// automatic reviewer. Hook payload fields still take precedence; this is only
/// a compatibility fallback for Codex builds that omit the reviewer field.
pub fn codex_config_enables_auto_review(path: &Path) -> bool {
    if refuse_symlink(path).is_err() || !path.exists() {
        return false;
    }
    let Ok(bytes) = read_bounded(path, CONFIG_LIMIT_BYTES) else {
        return false;
    };
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return false;
    };
    let Ok(document) = text.parse::<DocumentMut>() else {
        return false;
    };
    let selected_profile = document.get("profile").and_then(|item| item.as_str());
    let reviewer = selected_profile
        .and_then(|profile| {
            document
                .get("profiles")
                .and_then(|profiles| profiles.get(profile))
                .and_then(|profile| profile.get("approvals_reviewer"))
                .and_then(|item| item.as_str())
        })
        .or_else(|| {
            document
                .get("approvals_reviewer")
                .and_then(|item| item.as_str())
        });
    reviewer.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "auto_review" | "guardian_subagent"
        )
    })
}

fn find_executable_in_path(executable: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths).find_map(|directory| {
            let candidate = directory.join(executable);
            is_executable_file(&candidate).then_some(candidate)
        })
    })
}

fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn application_roots() -> Vec<PathBuf> {
    if cfg!(debug_assertions) {
        if let Some(roots) = env::var_os("FLOW_AGENT_TEST_APPLICATIONS_PATH") {
            return env::split_paths(&roots).collect();
        }
    }
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(home) = env::var_os("HOME") {
        roots.push(PathBuf::from(home).join("Applications"));
    }
    roots
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum InstallIntent {
    #[default]
    Untouched,
    Installed,
    Uninstalled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InstallOptions {
    pub enhanced_codex_activity: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPaths {
    pub flow_home: PathBuf,
    pub claude_settings: PathBuf,
    pub codex_hooks: PathBuf,
    pub codex_config: PathBuf,
}

impl InstallPaths {
    pub fn discover() -> Result<Self, InstallerError> {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or(InstallerError::MissingHome)?;
        let flow_home = env::var_os("FLOW_AGENT_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".flow-agent"));
        let codex_home = env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));
        Ok(Self {
            flow_home,
            claude_settings: home.join(".claude/settings.json"),
            codex_hooks: codex_home.join("hooks.json"),
            codex_config: codex_home.join("config.toml"),
        })
    }

    pub fn provider_config(&self, provider: HookProvider) -> &Path {
        match provider {
            HookProvider::Claude => &self.claude_settings,
            HookProvider::Codex => &self.codex_hooks,
        }
    }

    pub fn stable_binary(&self) -> PathBuf {
        self.flow_home.join("bin/flow-agent")
    }

    pub fn statusline_helper(&self) -> PathBuf {
        self.flow_home.join("bin/statusline")
    }

    pub fn statusline_delegate(&self) -> PathBuf {
        self.flow_home.join("bin/statusline-delegate")
    }

    pub fn state_file(&self) -> PathBuf {
        self.flow_home.join("install-state.json")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallReport {
    pub provider: HookProvider,
    pub intent: InstallIntent,
    pub config_path: PathBuf,
    pub stable_binary: PathBuf,
    pub config_changed: bool,
    pub binary_changed: bool,
    pub backup_path: Option<PathBuf>,
    pub definition_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairReport {
    pub provider: HookProvider,
    pub previous_intent: InstallIntent,
    pub attempted: bool,
    pub skipped_reason: Option<String>,
    pub result: Option<InstallReport>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeStatuslineStatus {
    NotInstalled,
    Installed,
    HelperMissing,
    CustomConflict,
    ConfigMalformed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeStatuslineInspection {
    pub status: ClaudeStatuslineStatus,
    pub config_path: PathBuf,
    pub helper_path: PathBuf,
    pub config_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigHealth {
    Missing,
    Valid,
    Malformed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryHealth {
    Missing,
    Executable,
    NotExecutable,
    NotRegular,
    Symlink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexFeatureStatus {
    EnabledByDefault,
    EnabledCanonical,
    DisabledCanonical,
    EnabledLegacy,
    DisabledLegacy,
    ConflictingFlags,
    ConfigMalformed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexTrustStatus {
    NotInstalled,
    ReviewRequired,
    TrustedStatePresent,
    ConfigMalformed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInspection {
    pub provider: HookProvider,
    pub intent: InstallIntent,
    pub config_path: PathBuf,
    pub config_health: ConfigHealth,
    pub config_error: Option<String>,
    pub owned_handlers: usize,
    pub expected_handlers: usize,
    pub definition_matches_manifest: bool,
    pub binary_path: PathBuf,
    pub binary_health: BinaryHealth,
    pub codex_inline_events: Vec<String>,
    pub codex_config_error: Option<String>,
    pub codex_feature_status: Option<CodexFeatureStatus>,
    pub codex_trust_status: Option<CodexTrustStatus>,
    pub installed_definition_changed_at_ms: Option<u64>,
}

#[derive(Debug, Error)]
pub enum InstallerError {
    #[error("HOME is not set")]
    MissingHome,
    #[error("unsafe symbolic link refused: {0}")]
    SymlinkRefused(PathBuf),
    #[error("source binary is missing, not a regular file, or not executable: {0}")]
    InvalidSourceBinary(PathBuf),
    #[error("provider configuration exceeds the {limit} byte safety limit: {path}")]
    ConfigTooLarge { path: PathBuf, limit: u64 },
    #[error("provider configuration is malformed: {path}; backup: {backup:?}; {reason}")]
    MalformedProviderConfig {
        path: PathBuf,
        backup: Option<PathBuf>,
        reason: String,
    },
    #[error("Flow Agent state is malformed: {path}; backup: {backup:?}; {reason}")]
    MalformedState {
        path: PathBuf,
        backup: Option<PathBuf>,
        reason: String,
    },
    #[error("Codex inline hook definitions conflict with hooks.json: {events:?}")]
    CodexInlineHooksConflict { events: Vec<String> },
    #[error("Claude already has a custom statusLine; Flow Agent will not replace it")]
    ClaudeStatuslineConflict,
    #[error("Claude custom statusLine has no command that Flow Agent can preserve")]
    ClaudeStatuslineNotWrappable,
    #[error("installer lock failed: {0}")]
    Lock(io::Error),
    #[error("I/O failed for {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("JSON serialization failed: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallState {
    #[serde(default = "state_schema_version")]
    schema_version: u32,
    #[serde(default)]
    providers: BTreeMap<String, ProviderState>,
}

impl Default for InstallState {
    fn default() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            providers: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderState {
    intent: InstallIntent,
    #[serde(default)]
    definition_hash: Option<String>,
    #[serde(default)]
    config_path: Option<PathBuf>,
    #[serde(default)]
    enhanced_codex_activity: bool,
    updated_at_ms: u64,
}

fn state_schema_version() -> u32 {
    STATE_SCHEMA_VERSION
}

pub struct Installer {
    paths: InstallPaths,
    source_binary: PathBuf,
}

impl Installer {
    pub fn new(paths: InstallPaths, source_binary: impl Into<PathBuf>) -> Self {
        Self {
            paths,
            source_binary: source_binary.into(),
        }
    }

    pub fn paths(&self) -> &InstallPaths {
        &self.paths
    }

    pub fn intent(&self, provider: HookProvider) -> Result<InstallIntent, InstallerError> {
        let state = self.load_state()?;
        Ok(provider_intent(&state, provider))
    }

    pub fn inspect(&self, provider: HookProvider) -> Result<ProviderInspection, InstallerError> {
        let state = self.load_state_without_backup()?;
        let provider_state = state.providers.get(provider.as_str());
        let intent = provider_state
            .map(|provider_state| provider_state.intent)
            .unwrap_or_default();
        let options = InstallOptions {
            enhanced_codex_activity: provider_state
                .is_some_and(|provider_state| provider_state.enhanced_codex_activity),
        };
        let config_path = self.paths.provider_config(provider).to_path_buf();
        let (config_health, config_error, config) = inspect_json_config(&config_path)?;
        let command = self.hook_command(provider);
        let owned_handlers = config
            .as_ref()
            .map(|config| owned_hook_locations(config, &command).len())
            .unwrap_or(0);
        let expected_handlers = desired_events(provider, options).len();
        let definition_matches_manifest = config.as_ref().is_some_and(|config| {
            has_exact_owned_install(config, provider, &command, options).unwrap_or(false)
        });
        let binary_path = self.paths.stable_binary();
        let binary_health = inspect_binary(&binary_path)?;

        let mut codex_inline_events = Vec::new();
        let mut codex_config_error = None;
        let mut codex_feature_status = None;
        let mut codex_trust_status = None;
        if provider == HookProvider::Codex {
            match inspect_codex_toml(
                &self.paths.codex_config,
                &config_path,
                config.as_ref(),
                &command,
                provider_state.map(|state| state.updated_at_ms),
            )? {
                CodexTomlInspection::Valid {
                    inline_events,
                    feature_status,
                    trust_status,
                } => {
                    codex_inline_events = inline_events;
                    codex_feature_status = Some(feature_status);
                    codex_trust_status = Some(trust_status);
                }
                CodexTomlInspection::Malformed(error) => {
                    codex_config_error = Some(error);
                    codex_feature_status = Some(CodexFeatureStatus::ConfigMalformed);
                    codex_trust_status = Some(CodexTrustStatus::ConfigMalformed);
                }
            }
        }

        Ok(ProviderInspection {
            provider,
            intent,
            config_path,
            config_health,
            config_error,
            owned_handlers,
            expected_handlers,
            definition_matches_manifest,
            binary_path,
            binary_health,
            codex_inline_events,
            codex_config_error,
            codex_feature_status,
            codex_trust_status,
            installed_definition_changed_at_ms: provider_state.map(|state| state.updated_at_ms),
        })
    }

    pub fn inspect_claude_statusline(&self) -> Result<ClaudeStatuslineInspection, InstallerError> {
        let config_path = self.paths.claude_settings.clone();
        let helper_path = self.paths.statusline_helper();
        let (health, config_error, config) = inspect_json_config(&config_path)?;
        let status = match health {
            ConfigHealth::Malformed => ClaudeStatuslineStatus::ConfigMalformed,
            ConfigHealth::Missing => ClaudeStatuslineStatus::NotInstalled,
            ConfigHealth::Valid => {
                let configured = config.as_ref().and_then(|value| value.get("statusLine"));
                if configured.is_none() {
                    ClaudeStatuslineStatus::NotInstalled
                } else if configured == Some(&self.managed_statusline_value()) {
                    let wrapper_mode = config
                        .as_ref()
                        .is_some_and(|value| value.get(CLAUDE_ORIGINAL_STATUSLINE_KEY).is_some());
                    if inspect_binary(&helper_path)? == BinaryHealth::Executable
                        && inspect_binary(&self.paths.stable_binary())? == BinaryHealth::Executable
                        && (!wrapper_mode
                            || inspect_binary(&self.paths.statusline_delegate())?
                                == BinaryHealth::Executable)
                    {
                        ClaudeStatuslineStatus::Installed
                    } else {
                        ClaudeStatuslineStatus::HelperMissing
                    }
                } else {
                    ClaudeStatuslineStatus::CustomConflict
                }
            }
        };
        Ok(ClaudeStatuslineInspection {
            status,
            config_path,
            helper_path,
            config_error,
        })
    }

    pub fn install_claude_statusline(&self) -> Result<InstallReport, InstallerError> {
        let _lock = InstallLock::acquire(&self.paths.flow_home)?;
        self.validate_source_binary()?;
        let config_path = self.paths.claude_settings.clone();
        let (original, existed) = self.load_provider_json(&config_path)?;
        if original
            .get("statusLine")
            .is_some_and(|value| value != &self.managed_statusline_value())
        {
            return Err(InstallerError::ClaudeStatuslineConflict);
        }
        let mut updated = original.clone();
        updated
            .as_object_mut()
            .expect("validated provider configuration")
            .insert("statusLine".to_owned(), self.managed_statusline_value());
        let config_changed = updated != original;
        let backup_path = if config_changed && existed {
            Some(self.backup_file(&config_path)?)
        } else {
            None
        };
        let binary_changed = self.install_stable_binary()?;
        let wrapper = format!(
            "#!/bin/sh\nexec {} statusline\n",
            shell_quote(&self.paths.stable_binary())
        );
        atomic_write(&self.paths.statusline_helper(), wrapper.as_bytes(), 0o700)?;
        if config_changed {
            atomic_write_json(&config_path, &updated, 0o600)?;
        }
        Ok(InstallReport {
            provider: HookProvider::Claude,
            intent: InstallIntent::Installed,
            config_path,
            stable_binary: self.paths.statusline_helper(),
            config_changed,
            binary_changed,
            backup_path,
            definition_hash: None,
        })
    }

    pub fn install_claude_statusline_wrapper(&self) -> Result<InstallReport, InstallerError> {
        let _lock = InstallLock::acquire(&self.paths.flow_home)?;
        self.validate_source_binary()?;
        let config_path = self.paths.claude_settings.clone();
        let (original, existed) = self.load_provider_json(&config_path)?;
        let original_statusline = original
            .get("statusLine")
            .filter(|value| *value != &self.managed_statusline_value())
            .cloned()
            .ok_or(InstallerError::ClaudeStatuslineNotWrappable)?;
        let original_command = original_statusline
            .get("command")
            .and_then(Value::as_str)
            .filter(|command| !command.trim().is_empty())
            .ok_or(InstallerError::ClaudeStatuslineNotWrappable)?
            .to_owned();

        let mut updated = original.clone();
        let object = updated
            .as_object_mut()
            .expect("validated provider configuration");
        object.insert(
            CLAUDE_ORIGINAL_STATUSLINE_KEY.to_owned(),
            original_statusline,
        );
        object.insert("statusLine".to_owned(), self.managed_statusline_value());
        let config_changed = updated != original;
        let backup_path = if config_changed && existed {
            Some(self.backup_file(&config_path)?)
        } else {
            None
        };
        let binary_changed = self.install_stable_binary()?;
        let delegate = format!(
            "#!/bin/sh\n# Original Claude statusLine preserved by Flow Agent.\n{original_command}\n"
        );
        atomic_write(
            &self.paths.statusline_delegate(),
            delegate.as_bytes(),
            0o700,
        )?;
        let wrapper = format!(
            "#!/bin/sh\ninput=$(cat)\nprintf '%s' \"$input\" | {} statusline >/dev/null 2>&1\nprintf '%s' \"$input\" | {}\n",
            shell_quote(&self.paths.stable_binary()),
            shell_quote(&self.paths.statusline_delegate()),
        );
        atomic_write(&self.paths.statusline_helper(), wrapper.as_bytes(), 0o700)?;
        if config_changed {
            atomic_write_json(&config_path, &updated, 0o600)?;
        }
        Ok(InstallReport {
            provider: HookProvider::Claude,
            intent: InstallIntent::Installed,
            config_path,
            stable_binary: self.paths.statusline_helper(),
            config_changed,
            binary_changed,
            backup_path,
            definition_hash: None,
        })
    }

    pub fn uninstall_claude_statusline(&self) -> Result<InstallReport, InstallerError> {
        let _lock = InstallLock::acquire(&self.paths.flow_home)?;
        let config_path = self.paths.claude_settings.clone();
        let (original, existed) = self.load_provider_json(&config_path)?;
        let mut updated = original.clone();
        let owns_entry = original.get("statusLine") == Some(&self.managed_statusline_value());
        if owns_entry {
            let object = updated
                .as_object_mut()
                .expect("validated provider configuration");
            if let Some(saved) = object.remove(CLAUDE_ORIGINAL_STATUSLINE_KEY) {
                object.insert("statusLine".to_owned(), saved);
            } else {
                object.remove("statusLine");
            }
        }
        let config_changed = existed && updated != original;
        let backup_path = if config_changed {
            Some(self.backup_file(&config_path)?)
        } else {
            None
        };
        if config_changed {
            atomic_write_json(&config_path, &updated, 0o600)?;
        }
        let mut binary_changed = false;
        if owns_entry {
            binary_changed |= remove_regular_file(&self.paths.statusline_helper())?;
            binary_changed |= remove_regular_file(&self.paths.statusline_delegate())?;
            let state = self.load_state()?;
            let hooks_installed = state
                .providers
                .values()
                .any(|provider| provider.intent == InstallIntent::Installed);
            if !hooks_installed {
                binary_changed |= self.remove_stable_binary()?;
            }
        }
        Ok(InstallReport {
            provider: HookProvider::Claude,
            intent: InstallIntent::Uninstalled,
            config_path,
            stable_binary: self.paths.statusline_helper(),
            config_changed,
            binary_changed,
            backup_path,
            definition_hash: None,
        })
    }

    pub fn install(
        &self,
        provider: HookProvider,
        options: InstallOptions,
    ) -> Result<InstallReport, InstallerError> {
        let _lock = InstallLock::acquire(&self.paths.flow_home)?;
        self.install_locked(provider, options)
    }

    fn install_locked(
        &self,
        provider: HookProvider,
        options: InstallOptions,
    ) -> Result<InstallReport, InstallerError> {
        self.validate_source_binary()?;
        let mut state = self.load_state()?;
        if provider == HookProvider::Codex {
            let inline_events = self.codex_inline_hook_events()?;
            if !inline_events.is_empty() {
                return Err(InstallerError::CodexInlineHooksConflict {
                    events: inline_events,
                });
            }
        }

        let config_path = self.paths.provider_config(provider).to_path_buf();
        let (original, existed) = self.load_provider_json(&config_path)?;
        let mut updated = original.clone();
        merge_provider_hooks(
            &mut updated,
            provider,
            &self.hook_command(provider),
            options,
        )
        .map_err(|reason| self.malformed_config_error(&config_path, reason))?;
        let config_changed = updated != original;
        let definition_hash = definition_hash(provider, &self.hook_command(provider), options);

        let backup_path = if config_changed && existed {
            Some(self.backup_file(&config_path)?)
        } else {
            None
        };
        let binary_changed = self.install_stable_binary()?;
        if config_changed {
            atomic_write_json(&config_path, &updated, 0o600)?;
        }

        let previous_state = state.providers.get(provider.as_str());
        let definition_changed = config_changed
            || previous_state.is_none_or(|previous| previous.intent != InstallIntent::Installed)
            || previous_state.and_then(|previous| previous.definition_hash.as_deref())
                != Some(definition_hash.as_str());
        let updated_at_ms = if definition_changed {
            now_millis()
        } else {
            previous_state
                .map(|previous| previous.updated_at_ms)
                .unwrap_or_else(now_millis)
        };
        state.providers.insert(
            provider.as_str().to_owned(),
            ProviderState {
                intent: InstallIntent::Installed,
                definition_hash: Some(definition_hash.clone()),
                config_path: Some(config_path.clone()),
                enhanced_codex_activity: options.enhanced_codex_activity,
                updated_at_ms,
            },
        );
        self.save_state(&state)?;
        Ok(InstallReport {
            provider,
            intent: InstallIntent::Installed,
            config_path,
            stable_binary: self.paths.stable_binary(),
            config_changed,
            binary_changed,
            backup_path,
            definition_hash: Some(definition_hash),
        })
    }

    pub fn uninstall(&self, provider: HookProvider) -> Result<InstallReport, InstallerError> {
        let _lock = InstallLock::acquire(&self.paths.flow_home)?;
        let mut state = self.load_state()?;
        let config_path = self.paths.provider_config(provider).to_path_buf();
        let (original, existed) = self.load_provider_json(&config_path)?;
        let mut updated = original.clone();
        strip_owned_hooks(&mut updated, &self.hook_command(provider))
            .map_err(|reason| self.malformed_config_error(&config_path, reason))?;
        let config_changed = existed && updated != original;
        let backup_path = if config_changed {
            Some(self.backup_file(&config_path)?)
        } else {
            None
        };
        if config_changed {
            atomic_write_json(&config_path, &updated, 0o600)?;
        }

        state.providers.insert(
            provider.as_str().to_owned(),
            ProviderState {
                intent: InstallIntent::Uninstalled,
                definition_hash: None,
                config_path: Some(config_path.clone()),
                enhanced_codex_activity: false,
                updated_at_ms: now_millis(),
            },
        );
        let any_installed = state
            .providers
            .values()
            .any(|provider_state| provider_state.intent == InstallIntent::Installed);
        let statusline_installed = self
            .inspect_claude_statusline()
            .map(|inspection| inspection.status == ClaudeStatuslineStatus::Installed)
            .unwrap_or(false);
        let binary_changed = if any_installed || statusline_installed {
            false
        } else {
            self.remove_stable_binary()?
        };
        self.save_state(&state)?;
        Ok(InstallReport {
            provider,
            intent: InstallIntent::Uninstalled,
            config_path,
            stable_binary: self.paths.stable_binary(),
            config_changed,
            binary_changed,
            backup_path,
            definition_hash: None,
        })
    }

    pub fn repair(
        &self,
        provider: HookProvider,
        options: InstallOptions,
    ) -> Result<RepairReport, InstallerError> {
        let _lock = InstallLock::acquire(&self.paths.flow_home)?;
        let previous_intent = provider_intent(&self.load_state()?, provider);
        if previous_intent != InstallIntent::Installed {
            return Ok(RepairReport {
                provider,
                previous_intent,
                attempted: false,
                skipped_reason: Some("installation intent is not installed".to_owned()),
                result: None,
            });
        }
        let config_path = self.paths.provider_config(provider);
        let (config, existed) = self.load_provider_json(config_path)?;
        let complete = existed
            && has_complete_owned_install(&config, provider, &self.hook_command(provider), options)
                .map_err(|reason| self.malformed_config_error(config_path, reason))?;
        if !complete {
            return Ok(RepairReport {
                provider,
                previous_intent,
                attempted: false,
                skipped_reason: Some(
                    "managed hook entries were removed or changed; explicit install-hooks is required"
                        .to_owned(),
                ),
                result: None,
            });
        }
        let result = self.install_locked(provider, options)?;
        Ok(RepairReport {
            provider,
            previous_intent,
            attempted: true,
            skipped_reason: None,
            result: Some(result),
        })
    }

    fn hook_command(&self, provider: HookProvider) -> String {
        format!(
            "{} hook --provider {}",
            shell_quote(&self.paths.stable_binary()),
            provider.as_str()
        )
    }

    fn managed_statusline_value(&self) -> Value {
        let mut value = Map::new();
        value.insert("type".to_owned(), Value::String("command".to_owned()));
        value.insert(
            "command".to_owned(),
            Value::String(shell_quote(&self.paths.statusline_helper())),
        );
        Value::Object(value)
    }

    fn validate_source_binary(&self) -> Result<(), InstallerError> {
        refuse_symlink(&self.source_binary)?;
        let metadata = fs::metadata(&self.source_binary)
            .map_err(|_| InstallerError::InvalidSourceBinary(self.source_binary.clone()))?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
            return Err(InstallerError::InvalidSourceBinary(
                self.source_binary.clone(),
            ));
        }
        Ok(())
    }

    fn install_stable_binary(&self) -> Result<bool, InstallerError> {
        let destination = self.paths.stable_binary();
        refuse_symlink(&destination)?;
        if regular_files_equal(&self.source_binary, &destination)?
            && fs::metadata(&destination)
                .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        {
            return Ok(false);
        }
        let bytes = read_bounded(&self.source_binary, u64::MAX)?;
        atomic_write(&destination, &bytes, 0o700)?;
        Ok(true)
    }

    fn remove_stable_binary(&self) -> Result<bool, InstallerError> {
        let destination = self.paths.stable_binary();
        refuse_symlink(&destination)?;
        if !destination.exists() {
            return Ok(false);
        }
        fs::remove_file(&destination).map_err(|source| InstallerError::Io {
            path: destination,
            source,
        })?;
        Ok(true)
    }

    fn load_provider_json(&self, path: &Path) -> Result<(Value, bool), InstallerError> {
        refuse_symlink(path)?;
        if !path.exists() {
            return Ok((Value::Object(Map::new()), false));
        }
        let bytes = read_bounded(path, CONFIG_LIMIT_BYTES).map_err(|error| match error {
            InstallerError::ConfigTooLarge { .. } => error,
            other => other,
        })?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|error| self.malformed_config_error(path, format!("invalid JSON: {error}")))?;
        if !value.is_object() {
            return Err(self.malformed_config_error(
                path,
                "top-level configuration must be an object".to_owned(),
            ));
        }
        Ok((value, true))
    }

    fn codex_inline_hook_events(&self) -> Result<Vec<String>, InstallerError> {
        let path = &self.paths.codex_config;
        refuse_symlink(path)?;
        if !path.exists() {
            return Ok(Vec::new());
        }
        let bytes = read_bounded(path, CONFIG_LIMIT_BYTES)?;
        let text = std::str::from_utf8(&bytes).map_err(|error| {
            self.malformed_config_error(path, format!("config.toml is not UTF-8: {error}"))
        })?;
        let document = text
            .parse::<DocumentMut>()
            .map_err(|error| self.malformed_config_error(path, format!("invalid TOML: {error}")))?;
        let Some(hooks) = document.get("hooks").and_then(|item| item.as_table()) else {
            return Ok(Vec::new());
        };
        let mut events = hooks
            .iter()
            .filter(|(name, item)| {
                *name != "state" && (item.is_array_of_tables() || item.is_table())
            })
            .map(|(name, _)| name.to_owned())
            .collect::<Vec<_>>();
        events.sort();
        Ok(events)
    }

    fn malformed_config_error(&self, path: &Path, reason: String) -> InstallerError {
        let backup = self.backup_file(path).ok();
        InstallerError::MalformedProviderConfig {
            path: path.to_path_buf(),
            backup,
            reason,
        }
    }

    fn load_state(&self) -> Result<InstallState, InstallerError> {
        self.load_state_with_backup(true)
    }

    fn load_state_without_backup(&self) -> Result<InstallState, InstallerError> {
        self.load_state_with_backup(false)
    }

    fn load_state_with_backup(
        &self,
        backup_on_error: bool,
    ) -> Result<InstallState, InstallerError> {
        let path = self.paths.state_file();
        refuse_symlink(&path)?;
        if !path.exists() {
            return Ok(InstallState::default());
        }
        let bytes = read_bounded(&path, CONFIG_LIMIT_BYTES)?;
        serde_json::from_slice(&bytes).map_err(|error| {
            let backup = backup_on_error
                .then(|| self.backup_file(&path).ok())
                .flatten();
            InstallerError::MalformedState {
                path,
                backup,
                reason: error.to_string(),
            }
        })
    }

    fn save_state(&self, state: &InstallState) -> Result<(), InstallerError> {
        atomic_write_json(&self.paths.state_file(), state, 0o600)
    }

    fn backup_file(&self, source_path: &Path) -> Result<PathBuf, InstallerError> {
        refuse_symlink(source_path)?;
        if !source_path.exists() {
            return Err(InstallerError::Io {
                path: source_path.to_path_buf(),
                source: io::Error::new(io::ErrorKind::NotFound, "backup source is missing"),
            });
        }
        let backups = self.paths.flow_home.join("backups");
        ensure_private_directory(&backups)?;
        let file_name = source_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("config");
        let mut destination = backups.join(format!("{file_name}.{}", now_millis()));
        let mut collision = 0_u64;
        while destination.exists() {
            collision += 1;
            destination = backups.join(format!("{file_name}.{}.{}", now_millis(), collision));
        }
        let bytes = read_bounded(source_path, CONFIG_LIMIT_BYTES)?;
        atomic_write(&destination, &bytes, 0o600)?;
        Ok(destination)
    }
}

fn provider_intent(state: &InstallState, provider: HookProvider) -> InstallIntent {
    state
        .providers
        .get(provider.as_str())
        .map(|provider_state| provider_state.intent)
        .unwrap_or_default()
}

fn desired_events(provider: HookProvider, options: InstallOptions) -> Vec<&'static str> {
    match provider {
        HookProvider::Claude => CLAUDE_EVENTS.to_vec(),
        HookProvider::Codex => {
            let mut events = CODEX_EVENTS.to_vec();
            if options.enhanced_codex_activity {
                events.extend_from_slice(CODEX_ENHANCED_EVENTS);
            }
            events
        }
    }
}

fn merge_provider_hooks(
    config: &mut Value,
    provider: HookProvider,
    command: &str,
    options: InstallOptions,
) -> Result<(), String> {
    strip_owned_hooks(config, command)?;
    let object = config
        .as_object_mut()
        .ok_or_else(|| "top-level configuration must be an object".to_owned())?;
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| "the hooks field must be an object".to_owned())?;
    for event in desired_events(provider, options) {
        let groups = hooks
            .entry(event)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| format!("hooks.{event} must be an array"))?;
        groups.push(hook_group(provider, event, command));
    }
    Ok(())
}

fn hook_group(provider: HookProvider, event: &str, command: &str) -> Value {
    let timeout = match (provider, event) {
        (HookProvider::Claude, "PermissionRequest" | "PreToolUse" | "Elicitation") => 86_400,
        (HookProvider::Codex, "PermissionRequest") => 3_600,
        _ => 5,
    };
    let mut handler = Map::new();
    handler.insert("type".to_owned(), Value::String("command".to_owned()));
    handler.insert("command".to_owned(), Value::String(command.to_owned()));
    handler.insert("timeout".to_owned(), Value::Number(timeout.into()));
    if provider == HookProvider::Codex {
        handler.insert(
            "statusMessage".to_owned(),
            Value::String(if event == "PermissionRequest" {
                "Waiting for Flow Agent approval".to_owned()
            } else {
                "Updating Flow Agent".to_owned()
            }),
        );
    }
    let mut group = Map::new();
    if provider == HookProvider::Codex {
        group.insert("matcher".to_owned(), Value::String("*".to_owned()));
    }
    group.insert(
        "hooks".to_owned(),
        Value::Array(vec![Value::Object(handler)]),
    );
    Value::Object(group)
}

fn strip_owned_hooks(config: &mut Value, command: &str) -> Result<usize, String> {
    let object = config
        .as_object_mut()
        .ok_or_else(|| "top-level configuration must be an object".to_owned())?;
    let Some(hooks_value) = object.get_mut("hooks") else {
        return Ok(0);
    };
    let hooks = hooks_value
        .as_object_mut()
        .ok_or_else(|| "the hooks field must be an object".to_owned())?;
    let event_names = hooks.keys().cloned().collect::<Vec<_>>();
    let mut removed = 0_usize;
    for event_name in event_names {
        let Some(groups) = hooks.get_mut(&event_name).and_then(Value::as_array_mut) else {
            continue;
        };
        groups.retain_mut(|group| {
            let Some(group_object) = group.as_object_mut() else {
                return true;
            };
            let Some(handlers) = group_object.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            let before = handlers.len();
            handlers.retain(|handler| !is_owned_handler(handler, command));
            removed += before.saturating_sub(handlers.len());
            if handlers.is_empty()
                && group_object
                    .keys()
                    .all(|key| key == "matcher" || key == "hooks")
            {
                return false;
            }
            true
        });
        if groups.is_empty() {
            hooks.remove(&event_name);
        }
    }
    if hooks.is_empty() {
        object.remove("hooks");
    }
    Ok(removed)
}

fn is_owned_handler(handler: &Value, command: &str) -> bool {
    handler.get("type").and_then(Value::as_str) == Some("command")
        && handler.get("command").and_then(Value::as_str) == Some(command)
}

fn has_complete_owned_install(
    config: &Value,
    provider: HookProvider,
    command: &str,
    options: InstallOptions,
) -> Result<bool, String> {
    let object = config
        .as_object()
        .ok_or_else(|| "top-level configuration must be an object".to_owned())?;
    let Some(hooks) = object.get("hooks").and_then(Value::as_object) else {
        return Ok(false);
    };
    for event in desired_events(provider, options) {
        let Some(groups) = hooks.get(event).and_then(Value::as_array) else {
            return Ok(false);
        };
        let found = groups.iter().any(|group| {
            group
                .get("hooks")
                .and_then(Value::as_array)
                .is_some_and(|handlers| {
                    handlers
                        .iter()
                        .any(|handler| is_owned_handler(handler, command))
                })
        });
        if !found {
            return Ok(false);
        }
    }
    Ok(true)
}

fn has_exact_owned_install(
    config: &Value,
    provider: HookProvider,
    command: &str,
    options: InstallOptions,
) -> Result<bool, String> {
    if !has_complete_owned_install(config, provider, command, options)? {
        return Ok(false);
    }
    let expected_events = desired_events(provider, options);
    let locations = owned_hook_locations(config, command);
    if locations.len() != expected_events.len() {
        return Ok(false);
    }
    let hooks = config
        .get("hooks")
        .and_then(Value::as_object)
        .ok_or_else(|| "the hooks field must be an object".to_owned())?;
    for event in expected_events {
        let expected = hook_group(provider, event, command);
        let exact = hooks
            .get(event)
            .and_then(Value::as_array)
            .is_some_and(|groups| groups.iter().any(|group| group == &expected));
        if !exact {
            return Ok(false);
        }
    }
    Ok(true)
}

#[derive(Debug, Clone)]
struct OwnedHookLocation {
    event: String,
    group_index: usize,
    handler_index: usize,
}

fn owned_hook_locations(config: &Value, command: &str) -> Vec<OwnedHookLocation> {
    let mut locations = Vec::new();
    let Some(hooks) = config.get("hooks").and_then(Value::as_object) else {
        return locations;
    };
    for (event, groups) in hooks {
        let Some(groups) = groups.as_array() else {
            continue;
        };
        for (group_index, group) in groups.iter().enumerate() {
            let Some(handlers) = group.get("hooks").and_then(Value::as_array) else {
                continue;
            };
            for (handler_index, handler) in handlers.iter().enumerate() {
                if is_owned_handler(handler, command) {
                    locations.push(OwnedHookLocation {
                        event: event.clone(),
                        group_index,
                        handler_index,
                    });
                }
            }
        }
    }
    locations
}

fn inspect_json_config(
    path: &Path,
) -> Result<(ConfigHealth, Option<String>, Option<Value>), InstallerError> {
    refuse_symlink(path)?;
    if !path.exists() {
        return Ok((ConfigHealth::Missing, None, None));
    }
    let bytes = read_bounded(path, CONFIG_LIMIT_BYTES)?;
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(value) if value.is_object() => Ok((ConfigHealth::Valid, None, Some(value))),
        Ok(_) => Ok((
            ConfigHealth::Malformed,
            Some("top-level configuration must be an object".to_owned()),
            None,
        )),
        Err(error) => Ok((ConfigHealth::Malformed, Some(error.to_string()), None)),
    }
}

fn inspect_binary(path: &Path) -> Result<BinaryHealth, InstallerError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Ok(BinaryHealth::Symlink),
        Ok(metadata) if !metadata.is_file() => Ok(BinaryHealth::NotRegular),
        Ok(metadata) if metadata.permissions().mode() & 0o111 == 0 => {
            Ok(BinaryHealth::NotExecutable)
        }
        Ok(_) => Ok(BinaryHealth::Executable),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(BinaryHealth::Missing),
        Err(source) => Err(InstallerError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

enum CodexTomlInspection {
    Valid {
        inline_events: Vec<String>,
        feature_status: CodexFeatureStatus,
        trust_status: CodexTrustStatus,
    },
    Malformed(String),
}

fn inspect_codex_toml(
    path: &Path,
    hooks_path: &Path,
    hooks_config: Option<&Value>,
    command: &str,
    definition_changed_at_ms: Option<u64>,
) -> Result<CodexTomlInspection, InstallerError> {
    refuse_symlink(path)?;
    if !path.exists() {
        return Ok(CodexTomlInspection::Valid {
            inline_events: Vec::new(),
            feature_status: CodexFeatureStatus::EnabledByDefault,
            trust_status: if hooks_config
                .is_some_and(|config| !owned_hook_locations(config, command).is_empty())
            {
                CodexTrustStatus::ReviewRequired
            } else {
                CodexTrustStatus::NotInstalled
            },
        });
    }
    let bytes = read_bounded(path, CONFIG_LIMIT_BYTES)?;
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(error) => return Ok(CodexTomlInspection::Malformed(error.to_string())),
    };
    let document = match text.parse::<DocumentMut>() {
        Ok(document) => document,
        Err(error) => return Ok(CodexTomlInspection::Malformed(error.to_string())),
    };
    let hooks_table = document.get("hooks").and_then(|item| item.as_table());
    let mut inline_events = hooks_table
        .into_iter()
        .flat_map(|table| table.iter())
        .filter(|(name, item)| *name != "state" && (item.is_array_of_tables() || item.is_table()))
        .map(|(name, _)| name.to_owned())
        .collect::<Vec<_>>();
    inline_events.sort();

    let features = document.get("features").and_then(|item| item.as_table());
    let canonical = features
        .and_then(|table| table.get("hooks"))
        .and_then(|item| item.as_bool());
    let legacy = features
        .and_then(|table| table.get("codex_hooks"))
        .and_then(|item| item.as_bool());
    let feature_status = match (canonical, legacy) {
        (Some(_), Some(_)) => CodexFeatureStatus::ConflictingFlags,
        (Some(true), None) => CodexFeatureStatus::EnabledCanonical,
        (Some(false), None) => CodexFeatureStatus::DisabledCanonical,
        (None, Some(true)) => CodexFeatureStatus::EnabledLegacy,
        (None, Some(false)) => CodexFeatureStatus::DisabledLegacy,
        (None, None) => CodexFeatureStatus::EnabledByDefault,
    };

    let locations = hooks_config
        .map(|config| owned_hook_locations(config, command))
        .unwrap_or_default();
    let trust_status = if locations.is_empty() {
        CodexTrustStatus::NotInstalled
    } else {
        let state_table = hooks_table
            .and_then(|table| table.get("state"))
            .and_then(|item| item.as_table());
        let raw_source = hooks_path.to_string_lossy().into_owned();
        let canonical_source = fs::canonicalize(hooks_path)
            .unwrap_or_else(|_| hooks_path.to_path_buf())
            .to_string_lossy()
            .into_owned();
        let all_state_present = state_table.is_some_and(|state| {
            locations.iter().all(|location| {
                let event = event_to_snake_case(&location.event);
                let suffix = format!(
                    ":{event}:{}:{}",
                    location.group_index, location.handler_index
                );
                [&raw_source, &canonical_source].iter().any(|source| {
                    let key = format!("{source}{suffix}");
                    state
                        .get(&key)
                        .and_then(|item| item.as_table())
                        .is_some_and(|hook_state| {
                            let enabled = hook_state
                                .get("enabled")
                                .and_then(|item| item.as_bool())
                                .unwrap_or(true);
                            let trusted_hash = hook_state
                                .get("trusted_hash")
                                .or_else(|| hook_state.get("hash"))
                                .and_then(|item| item.as_str())
                                .is_some_and(|hash| !hash.is_empty());
                            enabled && trusted_hash
                        })
                })
            })
        });
        let trust_file_is_new_enough = definition_changed_at_ms.is_some_and(|changed_at| {
            modified_millis(path).is_some_and(|modified| modified >= changed_at)
        });
        if all_state_present && trust_file_is_new_enough {
            CodexTrustStatus::TrustedStatePresent
        } else {
            CodexTrustStatus::ReviewRequired
        }
    };
    Ok(CodexTomlInspection::Valid {
        inline_events,
        feature_status,
        trust_status,
    })
}

fn event_to_snake_case(event: &str) -> String {
    let mut output = String::with_capacity(event.len() + 4);
    for (index, character) in event.chars().enumerate() {
        if character.is_ascii_uppercase() && index > 0 {
            output.push('_');
        }
        output.push(character.to_ascii_lowercase());
    }
    output
}

fn modified_millis(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis()
        .try_into()
        .ok()
}

fn definition_hash(provider: HookProvider, command: &str, options: InstallOptions) -> String {
    let definitions = desired_events(provider, options)
        .into_iter()
        .map(|event| (event, hook_group(provider, event, command)))
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&(provider.as_str(), definitions)).unwrap_or_default();
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fnv1a64:{hash:016x}")
}

fn shell_quote(path: &Path) -> String {
    let value = path.to_string_lossy();
    if value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"/_+.,:@%=-".contains(&byte))
    {
        return value.into_owned();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn refuse_symlink(path: &Path) -> Result<(), InstallerError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(InstallerError::SymlinkRefused(path.to_path_buf()))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(InstallerError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, InstallerError> {
    let metadata = fs::metadata(path).map_err(|source| InstallerError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > limit {
        return Err(InstallerError::ConfigTooLarge {
            path: path.to_path_buf(),
            limit,
        });
    }
    let mut file = File::open(path).map_err(|source| InstallerError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::with_capacity(metadata.len().try_into().unwrap_or(0));
    file.read_to_end(&mut bytes)
        .map_err(|source| InstallerError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(bytes)
}

fn regular_files_equal(left: &Path, right: &Path) -> Result<bool, InstallerError> {
    let Ok(right_metadata) = fs::metadata(right) else {
        return Ok(false);
    };
    if !right_metadata.is_file() {
        return Ok(false);
    }
    let left_metadata = fs::metadata(left).map_err(|source| InstallerError::Io {
        path: left.to_path_buf(),
        source,
    })?;
    if left_metadata.len() != right_metadata.len() {
        return Ok(false);
    }
    Ok(read_bounded(left, u64::MAX)? == read_bounded(right, u64::MAX)?)
}

fn remove_regular_file(path: &Path) -> Result<bool, InstallerError> {
    refuse_symlink(path)?;
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => {
            fs::remove_file(path).map_err(|source| InstallerError::Io {
                path: path.to_path_buf(),
                source,
            })?;
            Ok(true)
        }
        Ok(_) => Err(InstallerError::Io {
            path: path.to_path_buf(),
            source: io::Error::new(io::ErrorKind::InvalidData, "path is not a regular file"),
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(InstallerError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn atomic_write_json(path: &Path, value: &impl Serialize, mode: u32) -> Result<(), InstallerError> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    atomic_write(path, &bytes, mode)
}

fn atomic_write(path: &Path, bytes: &[u8], default_mode: u32) -> Result<(), InstallerError> {
    refuse_symlink(path)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    ensure_private_directory(parent)?;
    let mode = fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o777)
        .unwrap_or(default_mode);
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("flow-agent");
    let unique = UNIQUE_FILE_ID.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".{name}.flow-agent.{}.{}.tmp",
        std::process::id(),
        unique
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&temporary)
            .map_err(|source| InstallerError::Io {
                path: temporary.clone(),
                source,
            })?;
        file.write_all(bytes).map_err(|source| InstallerError::Io {
            path: temporary.clone(),
            source,
        })?;
        file.sync_all().map_err(|source| InstallerError::Io {
            path: temporary.clone(),
            source,
        })?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(mode)).map_err(|source| {
            InstallerError::Io {
                path: temporary.clone(),
                source,
            }
        })?;
        fs::rename(&temporary, path).map_err(|source| InstallerError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn ensure_private_directory(path: &Path) -> Result<(), InstallerError> {
    if path.exists() {
        refuse_symlink(path)?;
        return Ok(());
    }
    let mut builder = DirBuilder::new();
    builder
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(|source| InstallerError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(())
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

struct InstallLock {
    file: File,
}

impl InstallLock {
    fn acquire(flow_home: &Path) -> Result<Self, InstallerError> {
        let run = flow_home.join("run");
        ensure_private_directory(&run)?;
        let path = run.join("hooks-install.lock");
        refuse_symlink(&path)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .map_err(InstallerError::Lock)?;
        // SAFETY: flock only borrows the live descriptor for this call. The file
        // is retained by InstallLock until the operation finishes.
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if result != 0 {
            return Err(InstallerError::Lock(io::Error::last_os_error()));
        }
        Ok(Self { file })
    }
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        // SAFETY: the descriptor remains live for the duration of this call.
        let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

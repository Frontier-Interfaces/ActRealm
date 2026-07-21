use actrealm_bridge::{default_socket_path, validate_socket_path, BridgeClient, BridgeListener};
use actrealm_core::{
    hook_directive, permission_deadline_ms, BlockingRequestKind, BridgeRequest, Decision, Provider,
    DOCTOR_PROBE_EVENT, MAX_HOOK_PAYLOAD_BYTES, PERMISSION_COMMIT_DELAY_MS,
};
use actrealm_installer::{
    codex_config_enables_auto_review, discover_provider_availability, BinaryHealth,
    CodexFeatureStatus, CodexTrustStatus, ConfigHealth, HookProvider, InstallIntent,
    InstallOptions, InstallPaths, Installer,
};
use actrealm_quota::{capture_claude_statusline, statusline_text, QuotaPaths};
use actrealm_runtime::{
    default_database_path, ApprovalAction, DiagnosticCapture, EventSpool, RuntimeInstanceGuard,
    RuntimeStore, WaiterRegistry,
};
use actrealm_server::{ApiServer, ApiServerConfig, RuntimeRestartHandle, RuntimeRestartRequest};
use actrealm_usage::{capture_claude_statusline_usage, UsagePaths};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const PROVIDER_VERSION_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Parser)]
#[command(
    name = "actrealm",
    version,
    about = "Local-first agent attention runtime"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the local runtime and control panel.
    Serve {
        #[arg(long, value_enum, default_value_t = ApprovalMode::Widget)]
        approval: ApprovalMode,
        #[arg(long)]
        socket: Option<PathBuf>,
        /// Open the one-time authenticated control panel in the default browser.
        #[arg(long)]
        open: bool,
        /// Internal one-time state used to re-exec the Runtime on the same port.
        #[arg(long, hide = true)]
        restart_state: Option<PathBuf>,
    },
    /// Receive one provider hook payload from stdin and forward it to the runtime.
    Hook {
        #[arg(long)]
        provider: String,
        #[arg(long)]
        socket: Option<PathBuf>,
    },
    /// Safely install Claude and/or Codex hooks into user configuration.
    InstallHooks {
        #[arg(value_enum, default_value_t = HookTarget::All)]
        provider: HookTarget,
        /// Also observe Codex tool start/finish events (off by default).
        #[arg(long)]
        enhanced_codex_activity: bool,
        /// Repair only an intact installation; never recreate manually removed hooks.
        #[arg(long)]
        repair: bool,
    },
    /// Remove only ActRealm hook entries and preserve all user configuration.
    UninstallHooks {
        #[arg(value_enum, default_value_t = HookTarget::All)]
        provider: HookTarget,
    },
    /// Diagnose provider CLIs, hook configuration, trust, runtime, and fail-open behavior.
    Doctor {
        /// Emit a stable machine-readable report.
        #[arg(long)]
        json: bool,
    },
    /// Export all locally persisted, sanitized ActRealm data as JSON.
    Export,
    /// Export only aggregate daily metrics, with no session or event records.
    ExportMetrics,
    /// Manage explicit, sanitized, time-limited local diagnostic capture.
    Diagnostics {
        #[command(subcommand)]
        action: DiagnosticsCommand,
    },
    /// Claude Code status line bridge. Installed and invoked by ActRealm.
    #[command(hide = true)]
    Statusline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ApprovalMode {
    Widget,
    Prompt,
    Allow,
    Deny,
    PassThrough,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum HookTarget {
    Claude,
    Codex,
    All,
}

#[derive(Debug, Subcommand)]
enum DiagnosticsCommand {
    /// Enable sanitized capture for 1-60 minutes.
    Enable {
        #[arg(long, default_value_t = 15, value_parser = clap::value_parser!(u64).range(1..=60))]
        minutes: u64,
    },
    /// Show whether sanitized capture is active.
    Status,
    /// Disable capture and delete captured diagnostic metadata.
    Clear,
}

impl HookTarget {
    fn providers(self) -> &'static [HookProvider] {
        match self {
            Self::Claude => &[HookProvider::Claude],
            Self::Codex => &[HookProvider::Codex],
            Self::All => &[HookProvider::Claude, HookProvider::Codex],
        }
    }
}

enum RuntimeOutcome {
    Decision {
        decision: Decision,
        proposed_at: u64,
    },
    PassThrough(&'static str),
}

enum PromptInput {
    Line(String),
    Closed,
}

#[derive(Debug, Clone)]
struct ServeLaunch {
    socket_path: PathBuf,
    approval: ApprovalMode,
    api_enabled: bool,
    open: bool,
    api_bind: SocketAddr,
    bootstrap_token: Option<String>,
    restart_count: u32,
    restart_state_path: Option<PathBuf>,
}

enum ServeOutcome {
    Stopped,
    Restart(PathBuf),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RestartState {
    schema_version: u32,
    created_at: u64,
    socket_path: PathBuf,
    approval: String,
    api_enabled: bool,
    api_bind: SocketAddr,
    bootstrap_token: String,
    restart_count: u32,
}

const RESTART_STATE_MAX_BYTES: u64 = 16 * 1024;
const RESTART_STATE_TTL_MS: u64 = 30_000;

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Serve {
            approval,
            socket,
            open,
            restart_state,
        } => {
            let launch = load_serve_launch(socket, approval, open, restart_state)?;
            match serve(launch)? {
                ServeOutcome::Stopped => Ok(()),
                ServeOutcome::Restart(state_path) => replace_runtime_process(&state_path),
            }
        }
        Command::Hook { provider, socket } => {
            // Hook failures must be silent and fail open. Parsing CLI arguments still
            // reports errors because malformed installation is an operator error.
            let provider = Provider::from_str(&provider)?;
            let _ = run_hook(provider, socket.unwrap_or_else(default_socket_path));
            Ok(())
        }
        Command::InstallHooks {
            provider,
            enhanced_codex_activity,
            repair,
        } => install_hooks(provider, enhanced_codex_activity, repair),
        Command::UninstallHooks { provider } => uninstall_hooks(provider),
        Command::Doctor { json } => doctor(json),
        Command::Export => export_local_data(),
        Command::ExportMetrics => export_metrics(),
        Command::Diagnostics { action } => manage_diagnostics(action),
        Command::Statusline => run_statusline(),
    }
}

fn manage_diagnostics(action: DiagnosticsCommand) -> Result<()> {
    let database = default_database_path();
    let root = database
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("diagnostics");
    let capture = DiagnosticCapture::new(root);
    match action {
        DiagnosticsCommand::Enable { minutes } => {
            let status = capture.enable(minutes, now_millis())?;
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
        DiagnosticsCommand::Status => {
            let status = capture.status(now_millis())?;
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
        DiagnosticsCommand::Clear => {
            capture.clear()?;
            println!("diagnostic capture cleared");
        }
    }
    Ok(())
}

fn export_local_data() -> Result<()> {
    let store = RuntimeStore::open(default_database_path()).context("failed to open local data")?;
    let export = store
        .export_json(now_millis())
        .context("failed to export local data")?;
    println!("{}", serde_json::to_string_pretty(&export)?);
    Ok(())
}

fn export_metrics() -> Result<()> {
    let store = RuntimeStore::open(default_database_path()).context("failed to open local data")?;
    let export = store
        .export_metrics_json(now_millis())
        .context("failed to export local metrics")?;
    println!("{}", serde_json::to_string_pretty(&export)?);
    Ok(())
}

fn run_statusline() -> Result<()> {
    let mut input = Vec::new();
    let _ = io::stdin()
        .take((MAX_HOOK_PAYLOAD_BYTES + 1) as u64)
        .read_to_end(&mut input);
    if input.len() > MAX_HOOK_PAYLOAD_BYTES {
        println!("ActRealm · 额度输入过大");
        return Ok(());
    }
    let paths = QuotaPaths::discover();
    let now = now_millis();
    let usage_paths = UsagePaths::discover();
    // Numeric session metrics are captured independently; a malformed usage
    // extension must never break Claude's existing StatusLine or quota bridge.
    let _ = capture_claude_statusline_usage(&input, &usage_paths.claude_status_cache_dir(), now);
    match capture_claude_statusline(&input, &paths.claude_cache(), now) {
        Ok(entries) => println!("{}", statusline_text(&entries)),
        Err(_) => println!("ActRealm · 额度暂不可用"),
    }
    Ok(())
}

fn install_hooks(target: HookTarget, enhanced_codex_activity: bool, repair: bool) -> Result<()> {
    validate_socket_path(&default_socket_path()).context("invalid ActRealm socket path")?;
    for provider in target.providers() {
        ensure_provider_available(*provider)?;
    }
    let paths = InstallPaths::discover()?;
    let source_binary = std::env::current_exe().context("failed to locate actrealm binary")?;
    let installer = Installer::new(paths, source_binary);
    let options = InstallOptions {
        enhanced_codex_activity,
    };
    for provider in target.providers() {
        if repair {
            let report = installer.repair(*provider, options)?;
            if report.attempted {
                println!("repaired {} hooks", provider.as_str());
            } else {
                println!(
                    "left {} hooks unchanged: {}",
                    provider.as_str(),
                    report
                        .skipped_reason
                        .as_deref()
                        .unwrap_or("repair not needed")
                );
            }
            continue;
        }
        let report = installer.install(*provider, options)?;
        println!(
            "installed {} hooks in {}{}",
            provider.as_str(),
            report.config_path.display(),
            report
                .backup_path
                .as_ref()
                .map(|path| format!(" (backup: {})", path.display()))
                .unwrap_or_default()
        );
        if *provider == HookProvider::Codex {
            let availability = discover_provider_availability(*provider);
            let command = availability
                .codex_review_command()
                .unwrap_or_else(|| "codex".to_owned());
            println!("Codex requires one manual trust step: run {command}, then run /hooks, review and trust each ActRealm command hook.");
        }
    }
    Ok(())
}

fn uninstall_hooks(target: HookTarget) -> Result<()> {
    let paths = InstallPaths::discover()?;
    let source_binary = std::env::current_exe().context("failed to locate actrealm binary")?;
    let installer = Installer::new(paths, source_binary);
    for provider in target.providers() {
        let report = installer.uninstall(*provider)?;
        println!(
            "uninstalled {} hooks from {}{}",
            provider.as_str(),
            report.config_path.display(),
            report
                .backup_path
                .as_ref()
                .map(|path| format!(" (backup: {})", path.display()))
                .unwrap_or_default()
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DiagnosticStatus {
    Pass,
    Warning,
    Fail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Repairability {
    NotApplicable,
    Automatic,
    Manual,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticCheck {
    id: String,
    status: DiagnosticStatus,
    summary: String,
    detail: String,
    repairability: Repairability,
    #[serde(skip_serializing_if = "Option::is_none")]
    action: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DoctorReport {
    schema_version: u16,
    generated_at_ms: u64,
    overall: DiagnosticStatus,
    checks: Vec<DiagnosticCheck>,
}

#[derive(Debug, Clone, Copy)]
struct ProviderVerification {
    provider: HookProvider,
    intent: InstallIntent,
    definition_changed_at_ms: Option<u64>,
}

fn doctor(json: bool) -> Result<()> {
    let paths = InstallPaths::discover()?;
    let source_binary = std::env::current_exe().context("failed to locate actrealm binary")?;
    let installer = Installer::new(paths.clone(), source_binary);
    let socket_path = default_socket_path();
    let mut checks = Vec::new();

    let socket_valid = match validate_socket_path(&socket_path) {
        Ok(()) => {
            checks.push(diagnostic(
                "socket.path",
                DiagnosticStatus::Pass,
                "Unix socket path fits the operating-system limit",
                socket_path.display().to_string(),
                Repairability::NotApplicable,
                None,
            ));
            true
        }
        Err(error) => {
            checks.push(diagnostic(
                "socket.path",
                DiagnosticStatus::Fail,
                "Unix socket path is too long",
                error.to_string(),
                Repairability::Manual,
                Some("Set ACTREALM_HOME to a shorter absolute path"),
            ));
            false
        }
    };

    for provider in [HookProvider::Claude, HookProvider::Codex] {
        add_cli_check(&mut checks, provider);
    }

    let mut installed_any = false;
    let mut provider_verifications = Vec::new();
    for provider in [HookProvider::Claude, HookProvider::Codex] {
        match installer.inspect(provider) {
            Ok(inspection) => {
                installed_any |= inspection.intent == InstallIntent::Installed;
                provider_verifications.push(ProviderVerification {
                    provider,
                    intent: inspection.intent,
                    definition_changed_at_ms: inspection.installed_definition_changed_at_ms,
                });
                add_provider_checks(&mut checks, &inspection);
            }
            Err(error) => checks.push(diagnostic(
                &format!("{}.inspection", provider.as_str()),
                DiagnosticStatus::Fail,
                &format!("Could not inspect {} integration", provider.as_str()),
                error.to_string(),
                Repairability::Manual,
                Some("Restore the reported state/configuration file from backup"),
            )),
        }
    }

    add_runtime_checks(
        &mut checks,
        &socket_path,
        socket_valid,
        installed_any,
        &provider_verifications,
    );
    add_pass_through_check(&mut checks, &paths, socket_valid);

    let overall = if checks
        .iter()
        .any(|check| check.status == DiagnosticStatus::Fail)
    {
        DiagnosticStatus::Fail
    } else if checks
        .iter()
        .any(|check| check.status == DiagnosticStatus::Warning)
    {
        DiagnosticStatus::Warning
    } else {
        DiagnosticStatus::Pass
    };
    let report = DoctorReport {
        schema_version: 1,
        generated_at_ms: now_millis(),
        overall,
        checks,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_doctor_report(&report);
    }
    Ok(())
}

fn diagnostic(
    id: &str,
    status: DiagnosticStatus,
    summary: &str,
    detail: String,
    repairability: Repairability,
    action: Option<&str>,
) -> DiagnosticCheck {
    DiagnosticCheck {
        id: id.to_owned(),
        status,
        summary: summary.to_owned(),
        detail,
        repairability,
        action: action.map(ToOwned::to_owned),
    }
}

fn add_cli_check(checks: &mut Vec<DiagnosticCheck>, provider: HookProvider) {
    let id = format!("{}.cli", provider.as_str());
    let availability = discover_provider_availability(provider);
    if !availability.is_available() {
        checks.push(diagnostic(
            &id,
            DiagnosticStatus::Fail,
            &format!("{} client is not installed", provider.as_str()),
            "No CLI in PATH or supported macOS desktop app was found".to_owned(),
            Repairability::Manual,
            Some("Install the provider desktop app or CLI, then run actrealm doctor again"),
        ));
        return;
    }
    let Some(executable) = availability.version_executable() else {
        let app = availability
            .desktop_app_path
            .as_deref()
            .expect("available desktop-only provider must have an app path");
        checks.push(diagnostic(
            &id,
            DiagnosticStatus::Pass,
            &format!("{} desktop app is available", provider.as_str()),
            app.display().to_string(),
            Repairability::NotApplicable,
            None,
        ));
        return;
    };
    let source = if availability.cli_path.is_some() {
        "CLI"
    } else {
        "desktop runtime"
    };
    let version_child = std::process::Command::new(executable)
        .arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();
    match version_child.and_then(|child| wait_child_with_timeout(child, PROVIDER_VERSION_TIMEOUT)) {
        Ok((_, true)) => checks.push(diagnostic(
            &id,
            DiagnosticStatus::Warning,
            &format!("{} {source} version check timed out", provider.as_str()),
            format!(
                "{} exceeded 5 seconds and was stopped",
                executable.display()
            ),
            Repairability::Manual,
            Some("Run the provider CLI --version command directly"),
        )),
        Ok((output, false)) if output.status.success() => {
            let version = first_bounded_line(&output.stdout)
                .or_else(|| first_bounded_line(&output.stderr))
                .unwrap_or_else(|| "version command succeeded without text".to_owned());
            checks.push(diagnostic(
                &id,
                DiagnosticStatus::Pass,
                &format!("{} {source} is available", provider.as_str()),
                format!("{} · {version}", executable.display()),
                Repairability::NotApplicable,
                None,
            ));
        }
        Ok((output, false)) => checks.push(diagnostic(
            &id,
            DiagnosticStatus::Warning,
            &format!("{} {source} version check failed", provider.as_str()),
            format!("{} exited with {}", executable.display(), output.status),
            Repairability::Manual,
            Some("Run the provider CLI --version command directly"),
        )),
        Err(error) => checks.push(diagnostic(
            &id,
            DiagnosticStatus::Warning,
            &format!("{} {source} could not be started", provider.as_str()),
            error.to_string(),
            Repairability::Manual,
            Some("Check executable permissions and PATH"),
        )),
    }
}

fn add_provider_checks(
    checks: &mut Vec<DiagnosticCheck>,
    inspection: &actrealm_installer::ProviderInspection,
) {
    let provider = inspection.provider.as_str();
    match inspection.config_health {
        ConfigHealth::Malformed => checks.push(diagnostic(
            &format!("{provider}.config"),
            DiagnosticStatus::Fail,
            &format!("{provider} configuration is malformed"),
            format!(
                "{} · {}",
                inspection.config_path.display(),
                inspection.config_error.as_deref().unwrap_or("parse failed")
            ),
            Repairability::Manual,
            Some("Fix or restore the provider configuration; doctor will not rewrite it"),
        )),
        ConfigHealth::Missing if inspection.intent == InstallIntent::Installed => {
            checks.push(diagnostic(
                &format!("{provider}.config"),
                DiagnosticStatus::Fail,
                &format!("{provider} hooks were removed after installation"),
                inspection.config_path.display().to_string(),
                Repairability::Manual,
                Some("Run install-hooks explicitly if you want to reconnect; repair will not recreate manually removed hooks"),
            ));
        }
        ConfigHealth::Missing => checks.push(diagnostic(
            &format!("{provider}.config"),
            DiagnosticStatus::Warning,
            &format!("{provider} is not connected"),
            format!("{} is missing", inspection.config_path.display()),
            Repairability::Manual,
            Some(&format!("Run actrealm install-hooks {provider}")),
        )),
        ConfigHealth::Valid if inspection.definition_matches_manifest => checks.push(diagnostic(
            &format!("{provider}.config"),
            DiagnosticStatus::Pass,
            &format!("{provider} ActRealm hooks match the installation manifest"),
            format!(
                "{} managed handlers in {}",
                inspection.owned_handlers,
                inspection.config_path.display()
            ),
            Repairability::NotApplicable,
            None,
        )),
        ConfigHealth::Valid if inspection.owned_handlers > 0 => checks.push(diagnostic(
            &format!("{provider}.config"),
            DiagnosticStatus::Fail,
            &format!("{provider} ActRealm hooks are incomplete or changed"),
            format!(
                "found {} managed handlers; expected {}",
                inspection.owned_handlers, inspection.expected_handlers
            ),
            Repairability::Manual,
            Some(&format!(
                "Review the configuration, then explicitly run actrealm install-hooks {provider}"
            )),
        )),
        ConfigHealth::Valid => checks.push(diagnostic(
            &format!("{provider}.config"),
            DiagnosticStatus::Warning,
            &format!("{provider} has no ActRealm hooks"),
            inspection.config_path.display().to_string(),
            Repairability::Manual,
            Some(&format!("Run actrealm install-hooks {provider}")),
        )),
    }

    match inspection.binary_health {
        BinaryHealth::Executable => checks.push(diagnostic(
            &format!("{provider}.binary"),
            DiagnosticStatus::Pass,
            "Stable hook binary is executable",
            inspection.binary_path.display().to_string(),
            Repairability::NotApplicable,
            None,
        )),
        BinaryHealth::Missing if inspection.intent != InstallIntent::Installed => {
            checks.push(diagnostic(
                &format!("{provider}.binary"),
                DiagnosticStatus::Warning,
                "Stable hook binary is not installed",
                inspection.binary_path.display().to_string(),
                Repairability::Manual,
                Some(&format!("Run actrealm install-hooks {provider}")),
            ));
        }
        health => checks.push(diagnostic(
            &format!("{provider}.binary"),
            DiagnosticStatus::Fail,
            "Stable hook binary is unavailable or unsafe",
            format!("{} · {health:?}", inspection.binary_path.display()),
            if inspection.definition_matches_manifest {
                Repairability::Automatic
            } else {
                Repairability::Manual
            },
            Some(if inspection.definition_matches_manifest {
                "Run install-hooks --repair for this provider"
            } else {
                "Review the hook configuration before explicitly reinstalling"
            }),
        )),
    }

    if inspection.provider == HookProvider::Codex {
        add_codex_checks(checks, inspection);
    }
}

fn add_codex_checks(
    checks: &mut Vec<DiagnosticCheck>,
    inspection: &actrealm_installer::ProviderInspection,
) {
    if let Some(error) = inspection.codex_config_error.as_deref() {
        checks.push(diagnostic(
            "codex.config_toml",
            DiagnosticStatus::Fail,
            "Codex config.toml is malformed",
            error.to_owned(),
            Repairability::Manual,
            Some("Fix or restore config.toml; ActRealm will not rewrite malformed TOML"),
        ));
        return;
    }
    if !inspection.codex_inline_events.is_empty() {
        checks.push(diagnostic(
            "codex.inline_hooks",
            DiagnosticStatus::Fail,
            "Codex has same-layer inline hook definitions",
            inspection.codex_inline_events.join(", "),
            Repairability::Manual,
            Some("Keep either inline [hooks] or hooks.json at this layer to avoid duplicate execution"),
        ));
    }
    match inspection.codex_feature_status {
        Some(CodexFeatureStatus::EnabledByDefault) | Some(CodexFeatureStatus::EnabledCanonical) => {
            checks.push(diagnostic(
                "codex.feature",
                DiagnosticStatus::Pass,
                "Codex Hooks are enabled",
                format!("{:?}", inspection.codex_feature_status.unwrap()),
                Repairability::NotApplicable,
                None,
            ))
        }
        Some(CodexFeatureStatus::EnabledLegacy) => checks.push(diagnostic(
            "codex.feature",
            DiagnosticStatus::Warning,
            "Codex Hooks use the legacy codex_hooks feature alias",
            "The alias is recognized but new configuration uses hooks".to_owned(),
            Repairability::Manual,
            Some("Migrate [features].codex_hooks to [features].hooks when convenient"),
        )),
        Some(CodexFeatureStatus::DisabledCanonical) | Some(CodexFeatureStatus::DisabledLegacy) => {
            checks.push(diagnostic(
                "codex.feature",
                DiagnosticStatus::Fail,
                "Codex Hooks are disabled",
                format!("{:?}", inspection.codex_feature_status.unwrap()),
                Repairability::Manual,
                Some("Enable [features].hooks in Codex config.toml"),
            ))
        }
        Some(CodexFeatureStatus::ConflictingFlags) => checks.push(diagnostic(
            "codex.feature",
            DiagnosticStatus::Fail,
            "Codex has conflicting canonical and legacy Hook flags",
            "Both hooks and codex_hooks are present".to_owned(),
            Repairability::Manual,
            Some("Keep the canonical [features].hooks value and remove the legacy alias"),
        )),
        Some(CodexFeatureStatus::ConfigMalformed) | None => {}
    }
    match inspection.codex_trust_status {
        Some(CodexTrustStatus::TrustedStatePresent) => checks.push(diagnostic(
            "codex.trust",
            DiagnosticStatus::Pass,
            "Codex trust state covers every ActRealm hook",
            "Exact hook locations are enabled with trusted hashes newer than the installed definition".to_owned(),
            Repairability::NotApplicable,
            None,
        )),
        Some(CodexTrustStatus::ReviewRequired) => checks.push(diagnostic(
            "codex.trust",
            DiagnosticStatus::Warning,
            "Codex Hook review is still required",
            "ActRealm never writes Codex trust state or bypasses its security review".to_owned(),
            Repairability::Manual,
            Some("Open Codex, run /hooks, review each ActRealm command, then trust and trigger a new session"),
        )),
        Some(CodexTrustStatus::NotInstalled) => checks.push(diagnostic(
            "codex.trust",
            DiagnosticStatus::Warning,
            "Codex trust is not applicable until hooks are installed",
            "No ActRealm Codex hook was found".to_owned(),
            Repairability::Manual,
            Some("Run actrealm install-hooks codex"),
        )),
        Some(CodexTrustStatus::ConfigMalformed) | None => {}
    }
}

fn add_runtime_checks(
    checks: &mut Vec<DiagnosticCheck>,
    socket_path: &std::path::Path,
    socket_valid: bool,
    installed_any: bool,
    provider_verifications: &[ProviderVerification],
) {
    if !socket_valid {
        checks.push(diagnostic(
            "runtime.control_loop",
            DiagnosticStatus::Fail,
            "Runtime probe skipped because the socket path is invalid",
            socket_path.display().to_string(),
            Repairability::Manual,
            Some("Shorten ACTREALM_HOME first"),
        ));
        add_real_event_checks(checks, provider_verifications, None);
        return;
    }
    match std::fs::symlink_metadata(socket_path) {
        Ok(metadata) if metadata.file_type().is_socket() => {
            let mode = metadata.permissions().mode() & 0o777;
            if mode != 0o600 {
                checks.push(diagnostic(
                    "runtime.socket_permissions",
                    DiagnosticStatus::Fail,
                    "Runtime socket permissions are unsafe",
                    format!("{} has mode {mode:o}", socket_path.display()),
                    Repairability::Automatic,
                    Some("Stop and restart ActRealm Runtime"),
                ));
            } else {
                checks.push(diagnostic(
                    "runtime.socket_permissions",
                    DiagnosticStatus::Pass,
                    "Runtime socket is private to the current user",
                    socket_path.display().to_string(),
                    Repairability::NotApplicable,
                    None,
                ));
            }
        }
        Ok(_) => checks.push(diagnostic(
            "runtime.socket_permissions",
            DiagnosticStatus::Fail,
            "Runtime path exists but is not a Unix socket",
            socket_path.display().to_string(),
            Repairability::Manual,
            Some("Inspect the path before removing it, then restart ActRealm Runtime"),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => checks.push(diagnostic(
            "runtime.socket_permissions",
            if installed_any {
                DiagnosticStatus::Fail
            } else {
                DiagnosticStatus::Warning
            },
            "ActRealm Runtime is not running",
            socket_path.display().to_string(),
            Repairability::Automatic,
            Some("Start actrealm serve --approval widget"),
        )),
        Err(error) => checks.push(diagnostic(
            "runtime.socket_permissions",
            DiagnosticStatus::Fail,
            "Runtime socket could not be inspected",
            error.to_string(),
            Repairability::Manual,
            Some("Check the ActRealm home directory permissions"),
        )),
    }

    let probe = BridgeRequest::doctor_probe_at(now_millis());
    let mut probe_summary = None;
    match BridgeClient::new(socket_path.to_path_buf()).send(&probe, Duration::from_secs(1)) {
        Ok(Some(response)) if response.reason.as_deref() == Some("doctor_probe_ok") => {
            probe_summary = response
                .message
                .as_deref()
                .and_then(|message| serde_json::from_str::<Value>(message).ok());
            checks.push(diagnostic(
                "runtime.control_loop",
                DiagnosticStatus::Pass,
                "Runtime control round trip succeeded",
                "The diagnostic frame was acknowledged without creating an Agent session"
                    .to_owned(),
                Repairability::NotApplicable,
                None,
            ));
        }
        Ok(_) => checks.push(diagnostic(
            "runtime.control_loop",
            DiagnosticStatus::Fail,
            "Runtime returned an unexpected diagnostic response",
            "The bridge connected but did not acknowledge the probe".to_owned(),
            Repairability::Automatic,
            Some("Restart ActRealm Runtime and rerun doctor"),
        )),
        Err(error) => checks.push(diagnostic(
            "runtime.control_loop",
            if installed_any {
                DiagnosticStatus::Fail
            } else {
                DiagnosticStatus::Warning
            },
            "Runtime control round trip is unavailable",
            error.to_string(),
            Repairability::Automatic,
            Some("Start or restart actrealm serve --approval widget"),
        )),
    }
    add_real_event_checks(checks, provider_verifications, probe_summary.as_ref());
}

fn add_real_event_checks(
    checks: &mut Vec<DiagnosticCheck>,
    providers: &[ProviderVerification],
    probe_summary: Option<&Value>,
) {
    for verification in providers
        .iter()
        .filter(|verification| verification.intent == InstallIntent::Installed)
    {
        let provider = verification.provider.as_str();
        let latest = probe_summary
            .and_then(|summary| summary.pointer(&format!("/latestProviderEventAt/{provider}")))
            .and_then(Value::as_u64);
        let verified = verification
            .definition_changed_at_ms
            .zip(latest)
            .is_some_and(|(installed_at, event_at)| event_at >= installed_at);
        if verified {
            checks.push(diagnostic(
                &format!("{provider}.real_event"),
                DiagnosticStatus::Pass,
                &format!("{provider} emitted a real event after installation"),
                format!("latest event at {}", latest.unwrap_or_default()),
                Repairability::NotApplicable,
                None,
            ));
        } else {
            checks.push(diagnostic(
                &format!("{provider}.real_event"),
                DiagnosticStatus::Warning,
                &format!("{provider} installation is not yet verified by a real event"),
                "A diagnostic bridge probe is not counted as provider evidence".to_owned(),
                Repairability::Manual,
                Some(&format!(
                    "Start a new {provider} session, then run actrealm doctor again"
                )),
            ));
        }
    }
}

fn add_pass_through_check(
    checks: &mut Vec<DiagnosticCheck>,
    paths: &InstallPaths,
    socket_valid: bool,
) {
    let stable_binary = paths.stable_binary();
    if !socket_valid || !stable_binary.exists() {
        checks.push(diagnostic(
            "hook.pass_through",
            DiagnosticStatus::Warning,
            "Fail-open pass-through probe was skipped",
            if !socket_valid {
                "socket path is invalid".to_owned()
            } else {
                format!("{} is missing", stable_binary.display())
            },
            Repairability::Manual,
            Some("Install hooks after fixing earlier diagnostics"),
        ));
        return;
    }
    let missing_socket = paths
        .actrealm_home
        .join(format!("run/doctor-missing-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&missing_socket);
    let mut child = match std::process::Command::new(&stable_binary)
        .args(["hook", "--provider", "claude", "--socket"])
        .arg(&missing_socket)
        .env("ACTREALM_HOME", &paths.actrealm_home)
        .env("ACTREALM_STDIN_TIMEOUT_MS", "500")
        .env("ACTREALM_HOOK_REPLY_TIMEOUT_MS", "100")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            checks.push(diagnostic(
                "hook.pass_through",
                DiagnosticStatus::Fail,
                "Fail-open pass-through probe could not start",
                error.to_string(),
                Repairability::Automatic,
                Some("Repair the stable hook binary"),
            ));
            return;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(
            br#"{"hook_event_name":"PermissionRequest","session_id":"actrealm-doctor","tool_name":"Bash","tool_input":{"command":"true"}}"#,
        );
    }
    match wait_child_with_timeout(child, Duration::from_secs(2)) {
        Ok((_, true)) => checks.push(diagnostic(
            "hook.pass_through",
            DiagnosticStatus::Fail,
            "Runtime-offline pass-through exceeded its safety deadline",
            "The probe was stopped after 2 seconds".to_owned(),
            Repairability::Automatic,
            Some("Repair or reinstall the stable hook binary"),
        )),
        Ok((output, false)) if output.status.success() && output.stdout.is_empty() => {
            checks.push(diagnostic(
                "hook.pass_through",
                DiagnosticStatus::Pass,
                "Runtime-offline pass-through is silent and successful",
                "No approval directive was written to stdout".to_owned(),
                Repairability::NotApplicable,
                None,
            ))
        }
        Ok((output, false)) => checks.push(diagnostic(
            "hook.pass_through",
            DiagnosticStatus::Fail,
            "Runtime-offline pass-through violated the fail-open contract",
            format!(
                "exit={} stdout_bytes={} stderr_bytes={}",
                output.status,
                output.stdout.len(),
                output.stderr.len()
            ),
            Repairability::Automatic,
            Some("Repair or reinstall the stable hook binary"),
        )),
        Err(error) => checks.push(diagnostic(
            "hook.pass_through",
            DiagnosticStatus::Fail,
            "Fail-open pass-through probe did not complete",
            error.to_string(),
            Repairability::Automatic,
            Some("Repair or reinstall the stable hook binary"),
        )),
    }
}

fn print_doctor_report(report: &DoctorReport) {
    println!("ActRealm doctor: {:?}", report.overall);
    for check in &report.checks {
        let marker = match check.status {
            DiagnosticStatus::Pass => "✓",
            DiagnosticStatus::Warning => "!",
            DiagnosticStatus::Fail => "×",
        };
        println!("[{marker}] {} — {}", check.id, check.summary);
        println!("    {}", check.detail);
        if let Some(action) = check.action.as_deref() {
            println!("    next: {action}");
        }
    }
}

fn first_bounded_line(bytes: &[u8]) -> Option<String> {
    let value = String::from_utf8_lossy(bytes);
    let line = value.lines().find(|line| !line.trim().is_empty())?.trim();
    Some(line.chars().take(256).collect())
}

fn wait_child_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> io::Result<(std::process::Output, bool)> {
    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output().map(|output| (output, false));
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            return child.wait_with_output().map(|output| (output, true));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn ensure_provider_available(provider: HookProvider) -> Result<()> {
    if !discover_provider_availability(provider).is_available() {
        anyhow::bail!(
            "{} client is not installed; no CLI in PATH or supported macOS desktop app was found; refusing to create provider configuration",
            provider.as_str()
        );
    }
    Ok(())
}

fn approval_mode_name(mode: ApprovalMode) -> &'static str {
    match mode {
        ApprovalMode::Widget => "widget",
        ApprovalMode::Prompt => "prompt",
        ApprovalMode::Allow => "allow",
        ApprovalMode::Deny => "deny",
        ApprovalMode::PassThrough => "pass-through",
    }
}

fn parse_approval_mode(value: &str) -> Result<ApprovalMode> {
    match value {
        "widget" => Ok(ApprovalMode::Widget),
        "prompt" => Ok(ApprovalMode::Prompt),
        "allow" => Ok(ApprovalMode::Allow),
        "deny" => Ok(ApprovalMode::Deny),
        "pass-through" => Ok(ApprovalMode::PassThrough),
        _ => anyhow::bail!("restart state contains an invalid approval mode"),
    }
}

fn load_serve_launch(
    socket: Option<PathBuf>,
    approval: ApprovalMode,
    open: bool,
    restart_state_path: Option<PathBuf>,
) -> Result<ServeLaunch> {
    let Some(state_path) = restart_state_path else {
        return Ok(ServeLaunch {
            socket_path: socket.unwrap_or_else(default_socket_path),
            approval,
            api_enabled: approval == ApprovalMode::Widget || open,
            open,
            api_bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            bootstrap_token: None,
            restart_count: 0,
            restart_state_path: None,
        });
    };
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&state_path)
        .with_context(|| format!("failed to open restart state {}", state_path.display()))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("failed to inspect restart state {}", state_path.display()))?;
    if !metadata.is_file()
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.len() > RESTART_STATE_MAX_BYTES
    {
        anyhow::bail!("restart state is not a private bounded regular file");
    }
    let mut payload = Vec::new();
    file.take(RESTART_STATE_MAX_BYTES + 1)
        .read_to_end(&mut payload)
        .context("failed to read restart state")?;
    if payload.len() as u64 > RESTART_STATE_MAX_BYTES {
        anyhow::bail!("restart state exceeds the size limit");
    }
    let state: RestartState =
        serde_json::from_slice(&payload).context("failed to parse restart state")?;
    let now = now_millis();
    if state.schema_version != 1
        || !state.api_enabled
        || now.saturating_sub(state.created_at) > RESTART_STATE_TTL_MS
        || state.created_at > now.saturating_add(5_000)
        || !state.api_bind.ip().is_loopback()
        || state.api_bind.port() == 0
        || Uuid::parse_str(&state.bootstrap_token).is_err()
    {
        anyhow::bail!("restart state is invalid or expired");
    }
    Ok(ServeLaunch {
        socket_path: state.socket_path,
        approval: parse_approval_mode(&state.approval)?,
        api_enabled: state.api_enabled,
        open: false,
        api_bind: state.api_bind,
        bootstrap_token: Some(state.bootstrap_token),
        restart_count: state.restart_count,
        restart_state_path: Some(state_path),
    })
}

fn write_restart_state(
    paths: &RuntimePaths,
    launch: &ServeLaunch,
    bootstrap_token: &str,
    api_bind: SocketAddr,
) -> Result<PathBuf> {
    let directory = paths
        .lock
        .parent()
        .ok_or_else(|| anyhow::anyhow!("runtime lock has no parent directory"))?;
    let state_path = directory.join(format!("restart-{}.json", Uuid::now_v7()));
    let state = RestartState {
        schema_version: 1,
        created_at: now_millis(),
        socket_path: launch.socket_path.clone(),
        approval: approval_mode_name(launch.approval).to_owned(),
        api_enabled: launch.api_enabled,
        api_bind,
        bootstrap_token: bootstrap_token.to_owned(),
        restart_count: launch.restart_count.saturating_add(1),
    };
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&state_path)
        .with_context(|| format!("failed to create {}", state_path.display()))?;
    serde_json::to_writer(&mut file, &state).context("failed to encode restart state")?;
    file.sync_all().context("failed to sync restart state")?;
    Ok(state_path)
}

fn replace_runtime_process(state_path: &Path) -> Result<()> {
    let executable = std::env::current_exe().context("failed to locate Runtime executable")?;
    let error = std::process::Command::new(executable)
        .arg("serve")
        .arg("--restart-state")
        .arg(state_path)
        .exec();
    let _ = fs::remove_file(state_path);
    Err(error).context("failed to replace Runtime process")
}

fn serve(launch: ServeLaunch) -> Result<ServeOutcome> {
    let socket_path = launch.socket_path.clone();
    let approval = launch.approval;
    let open = launch.open;
    validate_socket_path(&socket_path).context("invalid runtime socket path")?;
    let paths = runtime_paths(&socket_path);
    let _instance = RuntimeInstanceGuard::acquire(&paths.lock)
        .with_context(|| format!("failed to acquire {}", paths.lock.display()))?;
    let store = RuntimeStore::open(&paths.database)
        .with_context(|| format!("failed to open {}", paths.database.display()))?;
    let retention_days = store
        .read_setting("ui_settings")
        .ok()
        .flatten()
        .and_then(|value| serde_json::from_str::<Value>(&value).ok())
        .and_then(|value| value.get("retentionDays").and_then(Value::as_u64))
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| matches!(value, 0 | 30 | 90 | 180 | 365))
        .unwrap_or(90);
    store
        .prune_events(retention_days, now_millis())
        .context("failed to apply event retention")?;
    store
        .reconcile_orphaned_approvals(Vec::new(), now_millis())
        .context("failed to reconcile stale approvals")?;
    let diagnostics = DiagnosticCapture::new(paths.diagnostics.clone());
    let _ = diagnostics.status(now_millis());
    let spool = EventSpool::new(paths.spool.clone());
    let _ = spool.drain(|request| store.ingest(request).is_ok());
    let listener = BridgeListener::bind(&socket_path)
        .with_context(|| format!("failed to bind {}", socket_path.display()))?;
    let waiters = WaiterRegistry::default();
    let (restart_sender, restart_receiver) = mpsc::channel::<RuntimeRestartRequest>();
    let runtime_started_at = now_millis();
    let api = if launch.api_enabled {
        Some(
            ApiServer::start(
                store.clone(),
                waiters.clone(),
                ApiServerConfig {
                    bind: launch.api_bind,
                    bootstrap_token: launch.bootstrap_token.clone(),
                    runtime_started_at,
                    restart_count: launch.restart_count,
                    commit_delay: commit_delay(),
                    enable_codex_connector: true,
                    enable_claude_oauth_quota: true,
                    runtime_restart: Some(RuntimeRestartHandle::new(
                        restart_sender,
                        socket_path.clone(),
                    )),
                    ..ApiServerConfig::default()
                },
            )
            .context("failed to start local control API")?,
        )
    } else {
        None
    };
    if let Some(state_path) = launch.restart_state_path.as_ref() {
        fs::remove_file(state_path)
            .with_context(|| format!("failed to consume {}", state_path.display()))?;
    }
    let mut runtime_output = io::stdout().lock();
    if let Some(api) = api.as_ref() {
        let _ = writeln!(
            runtime_output,
            "ActRealm control panel: {}",
            api.bootstrap_url()
        );
        if open {
            let _ = std::process::Command::new("open")
                .arg(api.bootstrap_url())
                .spawn();
        }
    }
    let _ = writeln!(
        runtime_output,
        "actrealm runtime listening on {}",
        socket_path.display()
    );
    let _ = runtime_output.flush();
    drop(runtime_output);
    let prompt_lock = Arc::new(Mutex::new(()));
    let prompt_input = (approval == ApprovalMode::Prompt).then(prompt_input_channel);
    let expiry_waiters = waiters.clone();
    let expiry_store = store.clone();
    let expiry_diagnostics = diagnostics.clone();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(2));
        let now = now_millis();
        let _ = expiry_diagnostics.status(now);
        if let Ok(expired) = expiry_waiters.expire_request_ids_at(now) {
            for request_id in expired {
                let _ = expiry_store.expire_approval(request_id, "deadline", now_millis());
            }
        }
    });

    loop {
        let Some(stream) = listener.incoming().next() else {
            break;
        };
        if let Ok(request) = restart_receiver.try_recv() {
            drop(stream);
            let state_path = match write_restart_state(
                &paths,
                &launch,
                &request.bootstrap_token,
                request.api_bind,
            ) {
                Ok(path) => path,
                Err(error) => {
                    request.respond(Err(error.to_string()));
                    continue;
                }
            };
            for request_id in waiters.active_request_ids().unwrap_or_default() {
                let _ = waiters.pass_through(request_id, "runtime_restart");
            }
            let _ = store.reconcile_orphaned_approvals(Vec::new(), now_millis());
            request.respond(Ok(()));
            thread::sleep(Duration::from_millis(250));
            return Ok(ServeOutcome::Restart(state_path));
        }
        let Ok(mut stream) = stream else { continue };
        let prompt_lock = Arc::clone(&prompt_lock);
        let prompt_input = prompt_input.clone();
        let store = store.clone();
        let waiters = waiters.clone();
        let diagnostics = diagnostics.clone();
        thread::spawn(move || {
            let Ok(request) = BridgeListener::read_request(&mut stream) else {
                return;
            };
            if request.event_name() == Some(DOCTOR_PROBE_EVENT) {
                if let Some(request_id) = request.request_id {
                    let mut response =
                        actrealm_core::BridgeResponse::pass_through(request_id, "doctor_probe_ok");
                    if let Ok(snapshot) = store.snapshot() {
                        let latest_claude = snapshot
                            .sessions
                            .iter()
                            .filter(|session| session.provider == "claude")
                            .map(|session| session.last_event_at)
                            .max();
                        let latest_codex = snapshot
                            .sessions
                            .iter()
                            .filter(|session| session.provider == "codex")
                            .map(|session| session.last_event_at)
                            .max();
                        response.message = Some(
                            json!({
                                "eventCount": snapshot.event_count,
                                "latestProviderEventAt": {
                                    "claude": latest_claude,
                                    "codex": latest_codex
                                }
                            })
                            .to_string(),
                        );
                    }
                    let _ = BridgeListener::write_response(&mut stream, &response);
                }
                return;
            }
            let _ = diagnostics.capture(&request, now_millis());
            let registration = if request.needs_reply {
                let Ok(registration) = waiters.register_at(&request, now_millis()) else {
                    return;
                };
                if let Some(replaced) = registration.replaced_request_id {
                    let _ = store.expire_approval(replaced, "duplicate_replaced", now_millis());
                }
                Some(registration)
            } else {
                None
            };
            let ingest_result = match store.ingest(request.clone()) {
                Ok(result) => result,
                Err(_) => {
                    if let Some(registration) = registration {
                        let request_id = request.request_id.unwrap_or(request.id);
                        let _ = waiters.pass_through(request_id, "runtime_error");
                        if let Ok(response) =
                            registration.ticket.recv_timeout(Duration::from_secs(1))
                        {
                            let _ = BridgeListener::write_response(&mut stream, &response);
                        }
                    }
                    return;
                }
            };
            if ingest_result.suppressed {
                if let Some(registration) = registration {
                    let request_id = request.request_id.unwrap_or(request.id);
                    let _ = waiters.pass_through(request_id, "provider_internal");
                    if let Ok(response) = registration.ticket.recv_timeout(Duration::from_secs(1)) {
                        let _ = BridgeListener::write_response(&mut stream, &response);
                    }
                }
                return;
            }
            for resolved_request_id in ingest_result.resolved_request_ids {
                let _ = waiters.pass_through(resolved_request_id, "provider_handled");
            }

            if let Some(registration) = registration {
                if approval == ApprovalMode::Widget {
                    let request_id = request.request_id.unwrap_or(request.id);
                    let wait_for = request
                        .deadline_at
                        .map(|deadline| {
                            Duration::from_millis(deadline.saturating_sub(now_millis()))
                        })
                        .unwrap_or(Duration::from_millis(200));
                    if let Ok(response) = registration.ticket.recv_timeout(wait_for) {
                        let _ = BridgeListener::write_response(&mut stream, &response);
                    } else {
                        let _ = waiters.pass_through(request_id, "deadline");
                        let _ = store.expire_approval(request_id, "deadline", now_millis());
                    }
                    return;
                }
                if request.blocking_kind != Some(BlockingRequestKind::Permission) {
                    let request_id = request.request_id.unwrap_or(request.id);
                    let _ = waiters.pass_through(request_id, "native_provider_ui");
                    let _ = store.expire_approval(request_id, "native_provider_ui", now_millis());
                    if let Ok(response) = registration.ticket.recv_timeout(Duration::from_secs(1)) {
                        let _ = BridgeListener::write_response(&mut stream, &response);
                    }
                    return;
                }
                let _prompt_guard = prompt_lock.lock().ok();
                let outcome = choose_outcome(approval, prompt_input.as_deref());
                let request_id = request.request_id.unwrap_or(request.id);
                let command_id = Uuid::now_v7();
                let resolved = match outcome {
                    RuntimeOutcome::Decision {
                        decision,
                        proposed_at,
                    } => {
                        let action = if decision == Decision::Allow {
                            ApprovalAction::Approve
                        } else {
                            ApprovalAction::Deny
                        };
                        store
                            .claim_approval(command_id, request_id, action, proposed_at)
                            .and_then(|_| {
                                store.commit(
                                    command_id,
                                    proposed_at.saturating_add(PERMISSION_COMMIT_DELAY_MS),
                                    true,
                                )
                            })
                            .is_ok()
                            && waiters.decide(request_id, decision).is_ok()
                    }
                    RuntimeOutcome::PassThrough(reason) => {
                        store
                            .claim_approval(
                                command_id,
                                request_id,
                                ApprovalAction::PassThrough,
                                now_millis(),
                            )
                            .is_ok()
                            && waiters.pass_through(request_id, reason).is_ok()
                    }
                };
                if !resolved {
                    let _ = waiters.pass_through(request_id, "runtime_error");
                }
                if let Ok(response) = registration.ticket.recv_timeout(Duration::from_secs(1)) {
                    let _ = BridgeListener::write_response(&mut stream, &response);
                }
            }
        });
    }
    Ok(ServeOutcome::Stopped)
}

fn prompt_input_channel() -> Arc<Mutex<mpsc::Receiver<PromptInput>>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(line) => {
                    if sender.send(PromptInput::Line(line)).is_err() {
                        return;
                    }
                }
                Err(_) => {
                    let _ = sender.send(PromptInput::Closed);
                    return;
                }
            }
        }
        let _ = sender.send(PromptInput::Closed);
    });
    Arc::new(Mutex::new(receiver))
}

fn choose_outcome(
    mode: ApprovalMode,
    prompt_input: Option<&Mutex<mpsc::Receiver<PromptInput>>>,
) -> RuntimeOutcome {
    match mode {
        ApprovalMode::Widget => RuntimeOutcome::PassThrough("invalid_widget_dispatch"),
        ApprovalMode::Allow => delayed_decision(Decision::Allow),
        ApprovalMode::Deny => delayed_decision(Decision::Deny),
        ApprovalMode::PassThrough => RuntimeOutcome::PassThrough("user"),
        ApprovalMode::Prompt => {
            let Some(receiver) = prompt_input.and_then(|input| input.lock().ok()) else {
                return RuntimeOutcome::PassThrough("stdin_error");
            };
            loop {
                eprint!("Approve this request? [y/N/t=terminal] ");
                let _ = io::stderr().flush();
                let answer = match receiver.recv() {
                    Ok(PromptInput::Line(answer)) => answer,
                    Ok(PromptInput::Closed) | Err(_) => {
                        return RuntimeOutcome::PassThrough("stdin_closed")
                    }
                };
                let decision = match answer.trim().to_ascii_lowercase().as_str() {
                    "y" | "yes" => Some(Decision::Allow),
                    "" | "n" | "no" => Some(Decision::Deny),
                    "t" | "terminal" | "p" | "pass" => return RuntimeOutcome::PassThrough("user"),
                    _ => None,
                };
                let Some(decision) = decision else { continue };
                let proposed_at = now_millis();
                eprintln!("Decision pending for 3 seconds; type u then Enter to undo.");
                if undo_requested(&receiver, commit_delay()) {
                    eprintln!("Decision undone.");
                    continue;
                }
                return RuntimeOutcome::Decision {
                    decision,
                    proposed_at,
                };
            }
        }
    }
}

fn delayed_decision(decision: Decision) -> RuntimeOutcome {
    let proposed_at = now_millis();
    thread::sleep(commit_delay());
    RuntimeOutcome::Decision {
        decision,
        proposed_at,
    }
}

fn commit_delay() -> Duration {
    std::env::var("ACTREALM_COMMIT_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_secs(3))
}

fn undo_requested(receiver: &mpsc::Receiver<PromptInput>, timeout: Duration) -> bool {
    if timeout.is_zero() {
        return false;
    }
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        match receiver.recv_timeout(remaining) {
            Ok(PromptInput::Line(answer)) if answer.trim().eq_ignore_ascii_case("u") => {
                return true
            }
            Ok(PromptInput::Line(_)) => {}
            Ok(PromptInput::Closed) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                thread::sleep(deadline.saturating_duration_since(Instant::now()));
                return false;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => return false,
        }
    }
}

fn run_hook(provider: Provider, socket_path: PathBuf) -> Result<()> {
    if std::env::var("ACTREALM_SKIP_HOOKS").as_deref() == Ok("1") {
        return Ok(());
    }
    let input = read_hook_input()?;
    let mut raw: Value = serde_json::from_slice(&input)?;
    apply_codex_auto_review_fallback(provider, &mut raw);
    let request = BridgeRequest::from_hook(provider, raw);
    let timeout = if request.needs_reply {
        reply_timeout(provider)
    } else {
        Duration::from_millis(200)
    };

    let response = match BridgeClient::new(socket_path).send(&request, timeout) {
        Ok(response) => response,
        Err(_) => {
            if !request.needs_reply {
                let _ = EventSpool::default().append(&request);
            }
            return Ok(());
        }
    };
    let Some(response) = response else {
        return Ok(());
    };
    if let Some(directive) = hook_directive(&request, &response) {
        serde_json::to_writer(io::stdout(), &directive)?;
        println!();
    }
    Ok(())
}

fn apply_codex_auto_review_fallback(provider: Provider, raw: &mut Value) {
    let event_name = raw.get("hook_event_name").and_then(Value::as_str);
    let permission_lifecycle = event_name == Some("PermissionRequest")
        || (event_name == Some("PreToolUse")
            && raw.get("tool_name").and_then(Value::as_str) == Some("request_permissions"));
    if provider != Provider::Codex
        || !permission_lifecycle
        || [
            "approvals_reviewer",
            "approvalsReviewer",
            "_approvals_reviewer",
        ]
        .iter()
        .any(|key| raw.get(*key).and_then(Value::as_str).is_some())
    {
        return;
    }
    let enabled = InstallPaths::discover()
        .ok()
        .is_some_and(|paths| codex_config_enables_auto_review(&paths.codex_config));
    if enabled {
        if let Some(object) = raw.as_object_mut() {
            object.insert(
                "_approvals_reviewer".to_owned(),
                Value::String("auto_review".to_owned()),
            );
        }
    }
}

fn read_hook_input() -> Result<Vec<u8>> {
    let Some(deadline) = Instant::now().checked_add(stdin_timeout()) else {
        anyhow::bail!("invalid hook stdin deadline");
    };
    let stdin = io::stdin();
    let mut handle = stdin.lock();
    let mut input = Vec::new();
    let mut chunk = [0_u8; 8 * 1024];

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            anyhow::bail!("hook stdin deadline exceeded");
        }
        let timeout_ms = remaining.as_millis().max(1).min(i32::MAX as u128) as i32;
        let mut descriptor = libc::pollfd {
            fd: handle.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll receives one live stdin descriptor and does not retain
        // the pointer after returning.
        let ready = unsafe { libc::poll(&mut descriptor, 1, timeout_ms) };
        if ready == 0 {
            anyhow::bail!("hook stdin deadline exceeded");
        }
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        if descriptor.revents & libc::POLLNVAL != 0 {
            anyhow::bail!("hook stdin is unavailable");
        }

        match handle.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                input.extend_from_slice(&chunk[..count]);
                if input.len() > MAX_HOOK_PAYLOAD_BYTES {
                    anyhow::bail!("hook payload exceeds {} bytes", MAX_HOOK_PAYLOAD_BYTES);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(input)
}

fn stdin_timeout() -> Duration {
    std::env::var("ACTREALM_STDIN_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_secs(5))
}

fn reply_timeout(provider: Provider) -> Duration {
    if let Ok(value) = std::env::var("ACTREALM_HOOK_REPLY_TIMEOUT_MS") {
        if let Ok(milliseconds) = value.parse::<u64>() {
            return Duration::from_millis(milliseconds);
        }
    }
    default_reply_timeout(provider)
}

fn default_reply_timeout(provider: Provider) -> Duration {
    permission_deadline_ms(provider)
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_millis(200))
}

struct RuntimePaths {
    database: PathBuf,
    spool: PathBuf,
    lock: PathBuf,
    diagnostics: PathBuf,
}

fn runtime_paths(socket_path: &std::path::Path) -> RuntimePaths {
    if socket_path == default_socket_path() {
        let database = default_database_path();
        let root = database
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf();
        return RuntimePaths {
            database,
            spool: root.join("spool"),
            lock: root.join("run/runtime.lock"),
            diagnostics: root.join("diagnostics"),
        };
    }
    RuntimePaths {
        database: socket_path.with_extension("sqlite"),
        spool: socket_path.with_extension("spool"),
        lock: socket_path.with_extension("lock"),
        diagnostics: socket_path.with_extension("diagnostics"),
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn restart_state_file(name: &str, created_at: u64, mode: u32) -> PathBuf {
        let root = PathBuf::from("/tmp").join(format!(
            "actrealm-restart-state-{name}-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("state.json");
        let state = RestartState {
            schema_version: 1,
            created_at,
            socket_path: root.join("bridge.sock"),
            approval: "widget".to_owned(),
            api_enabled: true,
            api_bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 43121),
            bootstrap_token: Uuid::now_v7().to_string(),
            restart_count: 3,
        };
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(mode)
            .open(&path)
            .unwrap();
        serde_json::to_writer(&mut file, &state).unwrap();
        file.sync_all().unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    #[test]
    fn p0_permission_deadlines_allow_real_human_response_time() {
        assert_eq!(
            default_reply_timeout(Provider::Claude),
            Duration::from_secs(24 * 60 * 60)
        );
        assert_eq!(
            default_reply_timeout(Provider::Codex),
            Duration::from_secs(60 * 60)
        );
        assert_eq!(
            default_reply_timeout(Provider::Gemini),
            Duration::from_millis(200)
        );
    }

    #[test]
    fn private_restart_state_restores_exact_runtime_contract() {
        let opened = load_serve_launch(None, ApprovalMode::PassThrough, true, None).unwrap();
        assert!(opened.api_enabled);
        let path = restart_state_file("valid", now_millis(), 0o600);
        let launch = load_serve_launch(None, ApprovalMode::Deny, true, Some(path.clone())).unwrap();
        assert_eq!(launch.approval, ApprovalMode::Widget);
        assert!(launch.api_enabled);
        assert!(!launch.open);
        assert_eq!(launch.api_bind.port(), 43121);
        assert_eq!(launch.restart_count, 3);
        assert!(launch.bootstrap_token.is_some());
        assert_eq!(launch.restart_state_path.as_deref(), Some(path.as_path()));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn stale_or_public_restart_state_is_rejected() {
        let public = restart_state_file("public", now_millis(), 0o644);
        assert!(
            load_serve_launch(None, ApprovalMode::Widget, false, Some(public.clone())).is_err()
        );
        fs::remove_dir_all(public.parent().unwrap()).unwrap();

        let stale = restart_state_file(
            "stale",
            now_millis().saturating_sub(RESTART_STATE_TTL_MS + 1),
            0o600,
        );
        assert!(load_serve_launch(None, ApprovalMode::Widget, false, Some(stale.clone())).is_err());
        fs::remove_dir_all(stale.parent().unwrap()).unwrap();
    }

    #[test]
    fn restart_state_never_changes_a_custom_socket_parent_mode() {
        let root = PathBuf::from("/tmp").join(format!(
            "actrealm-restart-parent-{}-{}",
            std::process::id(),
            Uuid::now_v7()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        let socket = root.join("bridge.sock");
        let paths = runtime_paths(&socket);
        let launch = ServeLaunch {
            socket_path: socket,
            approval: ApprovalMode::Widget,
            api_enabled: true,
            open: false,
            api_bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 43121),
            bootstrap_token: None,
            restart_count: 0,
            restart_state_path: None,
        };
        let state_path = write_restart_state(
            &paths,
            &launch,
            &Uuid::now_v7().to_string(),
            launch.api_bind,
        )
        .unwrap();
        let parent_mode = fs::metadata(&root).unwrap().permissions().mode() & 0o777;
        let state_mode = fs::metadata(&state_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(parent_mode, 0o755);
        assert_eq!(state_mode, 0o600);
        fs::remove_dir_all(root).unwrap();
    }
}

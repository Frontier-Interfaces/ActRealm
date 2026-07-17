use flow_agent_installer::{
    BinaryHealth, CodexFeatureStatus, CodexTrustStatus, ConfigHealth, HookProvider, InstallIntent,
    InstallOptions, InstallPaths, Installer, InstallerError,
};
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

static TEST_ID: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "flow-agent-installer-{name}-{}-{id}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Fixture {
    _root: TestDir,
    paths: InstallPaths,
    source_binary: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = TestDir::new(name);
        let home = root.0.join("home");
        let paths = InstallPaths {
            flow_home: root.0.join("flow-home"),
            claude_settings: home.join(".claude/settings.json"),
            codex_hooks: root.0.join("custom-codex/hooks.json"),
            codex_config: root.0.join("custom-codex/config.toml"),
        };
        let source_binary = root.0.join("release/flow-agent");
        write_file(&source_binary, b"test-flow-agent-binary", 0o700);
        Self {
            _root: root,
            paths,
            source_binary,
        }
    }

    fn installer(&self) -> Installer {
        Installer::new(self.paths.clone(), self.source_binary.clone())
    }
}

fn write_file(path: &Path, bytes: &[u8], mode: u32) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

fn write_json(path: &Path, value: &Value) {
    write_file(path, &serde_json::to_vec_pretty(value).unwrap(), 0o640);
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn user_claude_config() -> Value {
    json!({
        "$schema": "https://json.schemastore.org/claude-code-settings.json",
        "unknownFutureField": {"preserve": [1, 2, 3]},
        "hooks": {
            "PreToolUse": [{
                "matcher": "Bash",
                "hooks": [{
                    "type": "command",
                    "command": "/usr/local/bin/user-security-hook",
                    "timeout": 9,
                    "unknownHandlerField": true
                }],
                "unknownGroupField": "keep"
            }],
            "FutureEvent": {"unknownShape": true}
        }
    })
}

fn flow_handlers<'a>(config: &'a Value, command_suffix: &str) -> Vec<&'a Value> {
    let mut result = Vec::new();
    if let Some(hooks) = config.get("hooks").and_then(Value::as_object) {
        for groups in hooks.values().filter_map(Value::as_array) {
            for handlers in groups.iter().filter_map(|group| group.get("hooks")) {
                if let Some(handlers) = handlers.as_array() {
                    result.extend(handlers.iter().filter(|handler| {
                        handler
                            .get("command")
                            .and_then(Value::as_str)
                            .is_some_and(|command| command.ends_with(command_suffix))
                    }));
                }
            }
        }
    }
    result
}

#[test]
fn claude_install_backs_up_and_preserves_user_semantics() {
    let fixture = Fixture::new("claude-preserve");
    let original = user_claude_config();
    write_json(&fixture.paths.claude_settings, &original);

    let report = fixture
        .installer()
        .install(HookProvider::Claude, InstallOptions::default())
        .unwrap();

    assert!(report.config_changed);
    assert!(report.binary_changed);
    assert_eq!(report.intent, InstallIntent::Installed);
    let backup = report
        .backup_path
        .expect("existing config must be backed up");
    assert_eq!(read_json(&backup), original);

    let installed = read_json(&fixture.paths.claude_settings);
    assert_eq!(
        installed["unknownFutureField"],
        original["unknownFutureField"]
    );
    assert_eq!(
        installed["hooks"]["FutureEvent"],
        original["hooks"]["FutureEvent"]
    );
    assert_eq!(
        installed["hooks"]["PreToolUse"][0],
        original["hooks"]["PreToolUse"][0]
    );
    let handlers = flow_handlers(&installed, "hook --provider claude");
    assert_eq!(handlers.len(), 17);
    assert_eq!(
        installed["hooks"]["PermissionRequest"][0]["hooks"][0]["timeout"],
        86_400
    );
    assert_eq!(
        installed["hooks"]["Elicitation"][0]["hooks"][0]["timeout"],
        86_400
    );
    assert_eq!(
        installed["hooks"]["PostToolUse"][0]["hooks"][0]["timeout"],
        5
    );
    assert_eq!(
        fs::metadata(fixture.paths.stable_binary())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&fixture.paths.claude_settings)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640,
        "atomic replacement preserves an existing config mode"
    );
}

#[test]
fn install_then_uninstall_restores_original_semantics_and_removes_only_ours() {
    let fixture = Fixture::new("round-trip");
    let original = user_claude_config();
    write_json(&fixture.paths.claude_settings, &original);
    let installer = fixture.installer();
    installer
        .install(HookProvider::Claude, InstallOptions::default())
        .unwrap();

    let report = installer.uninstall(HookProvider::Claude).unwrap();

    assert!(report.config_changed);
    assert!(report.backup_path.unwrap().exists());
    assert_eq!(read_json(&fixture.paths.claude_settings), original);
    assert_eq!(
        installer.intent(HookProvider::Claude).unwrap(),
        InstallIntent::Uninstalled
    );
    assert!(!fixture.paths.stable_binary().exists());
}

#[test]
fn codex_uses_custom_home_keeps_state_table_and_defaults_to_low_noise_events() {
    let fixture = Fixture::new("codex-home");
    let original = json!({
        "future": "keep",
        "hooks": {
            "PermissionRequest": [{
                "matcher": "Bash",
                "hooks": [{"type": "command", "command": "user-codex-hook", "timeout": 7}]
            }]
        }
    });
    write_json(&fixture.paths.codex_hooks, &original);
    write_file(
        &fixture.paths.codex_config,
        br#"[features]
hooks = true

[hooks.state]

[hooks.state."existing:permission_request:0:0"]
enabled = true
hash = "keep"
"#,
        0o600,
    );

    fixture
        .installer()
        .install(HookProvider::Codex, InstallOptions::default())
        .unwrap();

    let installed = read_json(&fixture.paths.codex_hooks);
    assert_eq!(installed["future"], "keep");
    assert_eq!(
        installed["hooks"]["PermissionRequest"][0],
        original["hooks"]["PermissionRequest"][0]
    );
    assert_eq!(flow_handlers(&installed, "hook --provider codex").len(), 4);
    assert!(installed["hooks"].get("PreToolUse").is_none());
    assert!(installed["hooks"].get("PostToolUse").is_none());
    assert_eq!(
        installed["hooks"]["PermissionRequest"][1]["hooks"][0]["timeout"],
        3_600
    );
    assert_eq!(
        installed["hooks"]["PermissionRequest"][1]["hooks"][0]["statusMessage"],
        "Waiting for Flow Agent approval"
    );
    let toml = fs::read_to_string(&fixture.paths.codex_config).unwrap();
    assert!(toml.contains("existing:permission_request:0:0"));
    assert!(toml.contains("hash = \"keep\""));
}

#[test]
fn enhanced_codex_activity_is_explicit_and_idempotent() {
    let fixture = Fixture::new("codex-enhanced");
    let options = InstallOptions {
        enhanced_codex_activity: true,
    };
    let installer = fixture.installer();
    let first = installer.install(HookProvider::Codex, options).unwrap();
    let first_config = fs::read(&fixture.paths.codex_hooks).unwrap();
    let second = installer.install(HookProvider::Codex, options).unwrap();
    let second_config = fs::read(&fixture.paths.codex_hooks).unwrap();

    assert!(first.config_changed);
    assert!(!second.config_changed);
    assert!(!second.binary_changed);
    assert_eq!(first_config, second_config);
    let installed = read_json(&fixture.paths.codex_hooks);
    assert_eq!(flow_handlers(&installed, "hook --provider codex").len(), 6);
}

#[test]
fn codex_inline_definitions_are_detected_without_creating_duplicate_hooks() {
    let fixture = Fixture::new("codex-inline");
    write_file(
        &fixture.paths.codex_config,
        br#"[[hooks.PermissionRequest]]
matcher = "*"

[[hooks.PermissionRequest.hooks]]
type = "command"
command = "user-inline-hook"
"#,
        0o600,
    );

    let error = fixture
        .installer()
        .install(HookProvider::Codex, InstallOptions::default())
        .unwrap_err();

    assert!(matches!(
        error,
        InstallerError::CodexInlineHooksConflict { ref events }
            if events == &vec!["PermissionRequest".to_owned()]
    ));
    assert!(!fixture.paths.codex_hooks.exists());
    assert!(!fixture.paths.stable_binary().exists());
}

#[test]
fn malformed_json_is_backed_up_and_never_rewritten() {
    let fixture = Fixture::new("malformed-json");
    let malformed = b"{\"hooks\": [ this is not valid json";
    write_file(&fixture.paths.claude_settings, malformed, 0o600);

    let error = fixture
        .installer()
        .install(HookProvider::Claude, InstallOptions::default())
        .unwrap_err();

    let InstallerError::MalformedProviderConfig { backup, .. } = error else {
        panic!("expected malformed provider config");
    };
    let backup = backup.expect("malformed provider config must be backed up");
    assert_eq!(fs::read(&backup).unwrap(), malformed);
    assert_eq!(fs::read(&fixture.paths.claude_settings).unwrap(), malformed);
    assert!(!fixture.paths.stable_binary().exists());
}

#[test]
fn malformed_codex_toml_is_backed_up_and_blocks_hooks_json_mutation() {
    let fixture = Fixture::new("malformed-toml");
    let malformed = b"[hooks\nthis = is not toml";
    write_file(&fixture.paths.codex_config, malformed, 0o600);
    let original_hooks = json!({"future": true});
    write_json(&fixture.paths.codex_hooks, &original_hooks);

    let error = fixture
        .installer()
        .install(HookProvider::Codex, InstallOptions::default())
        .unwrap_err();

    let InstallerError::MalformedProviderConfig { backup, path, .. } = error else {
        panic!("expected malformed provider config");
    };
    assert_eq!(path, fixture.paths.codex_config);
    assert_eq!(fs::read(backup.unwrap()).unwrap(), malformed);
    assert_eq!(read_json(&fixture.paths.codex_hooks), original_hooks);
}

#[test]
fn repair_respects_untouched_uninstalled_and_manual_removal() {
    let fixture = Fixture::new("repair-intent");
    let installer = fixture.installer();
    let untouched = installer
        .repair(HookProvider::Claude, InstallOptions::default())
        .unwrap();
    assert!(!untouched.attempted);
    assert_eq!(untouched.previous_intent, InstallIntent::Untouched);
    assert!(!fixture.paths.claude_settings.exists());

    let original = user_claude_config();
    write_json(&fixture.paths.claude_settings, &original);
    installer
        .install(HookProvider::Claude, InstallOptions::default())
        .unwrap();
    write_json(&fixture.paths.claude_settings, &original);
    let manually_removed = installer
        .repair(HookProvider::Claude, InstallOptions::default())
        .unwrap();
    assert!(!manually_removed.attempted);
    assert!(manually_removed
        .skipped_reason
        .as_deref()
        .unwrap()
        .contains("explicit install-hooks"));
    assert_eq!(read_json(&fixture.paths.claude_settings), original);

    installer.uninstall(HookProvider::Claude).unwrap();
    let uninstalled = installer
        .repair(HookProvider::Claude, InstallOptions::default())
        .unwrap();
    assert!(!uninstalled.attempted);
    assert_eq!(uninstalled.previous_intent, InstallIntent::Uninstalled);
    assert!(!fixture.paths.stable_binary().exists());
}

#[test]
fn repair_can_restore_only_the_binary_when_managed_hooks_are_complete() {
    let fixture = Fixture::new("repair-binary");
    let installer = fixture.installer();
    installer
        .install(HookProvider::Claude, InstallOptions::default())
        .unwrap();
    fs::remove_file(fixture.paths.stable_binary()).unwrap();

    let report = installer
        .repair(HookProvider::Claude, InstallOptions::default())
        .unwrap();

    assert!(report.attempted);
    assert!(report.result.unwrap().binary_changed);
    assert!(fixture.paths.stable_binary().exists());
}

#[test]
fn symbolic_link_configuration_is_refused_without_touching_target() {
    let fixture = Fixture::new("symlink");
    let victim = fixture._root.0.join("victim.json");
    let original = json!({"doNotTouch": true});
    write_json(&victim, &original);
    fs::create_dir_all(fixture.paths.claude_settings.parent().unwrap()).unwrap();
    symlink(&victim, &fixture.paths.claude_settings).unwrap();

    let error = fixture
        .installer()
        .install(HookProvider::Claude, InstallOptions::default())
        .unwrap_err();

    assert!(
        matches!(error, InstallerError::SymlinkRefused(path) if path == fixture.paths.claude_settings)
    );
    assert_eq!(read_json(&victim), original);
}

#[test]
fn concurrent_installs_are_serialized_and_leave_one_valid_definition_set() {
    let fixture = Fixture::new("concurrent");
    let paths = fixture.paths.clone();
    let source = fixture.source_binary.clone();
    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let barrier = Arc::clone(&barrier);
        let paths = paths.clone();
        let source = source.clone();
        workers.push(thread::spawn(move || {
            barrier.wait();
            Installer::new(paths, source)
                .install(HookProvider::Claude, InstallOptions::default())
                .unwrap();
        }));
    }
    barrier.wait();
    for worker in workers {
        worker.join().unwrap();
    }

    let installed = read_json(&fixture.paths.claude_settings);
    assert_eq!(
        flow_handlers(&installed, "hook --provider claude").len(),
        17
    );
    assert_eq!(
        fixture.installer().intent(HookProvider::Claude).unwrap(),
        InstallIntent::Installed
    );
    let leftovers = fs::read_dir(fixture.paths.claude_settings.parent().unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp"))
        .count();
    assert_eq!(leftovers, 0);
}

#[test]
fn inspection_is_read_only_for_malformed_configuration() {
    let fixture = Fixture::new("inspect-malformed");
    let malformed = b"not-json";
    write_file(&fixture.paths.claude_settings, malformed, 0o600);

    let inspection = fixture.installer().inspect(HookProvider::Claude).unwrap();

    assert_eq!(inspection.config_health, ConfigHealth::Malformed);
    assert!(inspection.config_error.is_some());
    assert_eq!(inspection.binary_health, BinaryHealth::Missing);
    assert_eq!(fs::read(&fixture.paths.claude_settings).unwrap(), malformed);
    assert!(!fixture.paths.flow_home.join("backups").exists());
}

#[test]
fn codex_inspection_distinguishes_review_from_trusted_state_and_feature_modes() {
    let fixture = Fixture::new("inspect-codex-trust");
    let installer = fixture.installer();
    installer
        .install(HookProvider::Codex, InstallOptions::default())
        .unwrap();

    let review = installer.inspect(HookProvider::Codex).unwrap();
    assert_eq!(review.config_health, ConfigHealth::Valid);
    assert_eq!(review.binary_health, BinaryHealth::Executable);
    assert_eq!(review.owned_handlers, 4);
    assert_eq!(review.expected_handlers, 4);
    assert!(review.definition_matches_manifest);
    assert_eq!(
        review.codex_feature_status,
        Some(CodexFeatureStatus::EnabledByDefault)
    );
    assert_eq!(
        review.codex_trust_status,
        Some(CodexTrustStatus::ReviewRequired)
    );

    let source = fixture.paths.codex_hooks.to_string_lossy();
    let mut trust = String::from("[features]\nhooks = true\n");
    for event in [
        "session_start",
        "user_prompt_submit",
        "permission_request",
        "stop",
    ] {
        trust.push_str(&format!(
            "\n[hooks.state.\"{source}:{event}:0:0\"]\nenabled = true\ntrusted_hash = \"sha256:test-{event}\"\n"
        ));
    }
    write_file(&fixture.paths.codex_config, trust.as_bytes(), 0o600);

    let trusted = installer.inspect(HookProvider::Codex).unwrap();
    assert_eq!(
        trusted.codex_feature_status,
        Some(CodexFeatureStatus::EnabledCanonical)
    );
    assert_eq!(
        trusted.codex_trust_status,
        Some(CodexTrustStatus::TrustedStatePresent)
    );

    installer
        .install(HookProvider::Codex, InstallOptions::default())
        .unwrap();
    assert_eq!(
        installer
            .inspect(HookProvider::Codex)
            .unwrap()
            .codex_trust_status,
        Some(CodexTrustStatus::TrustedStatePresent),
        "an idempotent install must not falsely require trust again"
    );
}

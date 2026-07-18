use actrealm_installer::{
    ClaudeStatuslineStatus, HookProvider, InstallOptions, InstallPaths, Installer,
};
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static ID: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    paths: InstallPaths,
    source: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = PathBuf::from("/tmp").join(format!(
            "actrealm-statusline-{name}-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source-actrealm");
        fs::write(&source, b"binary").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            paths: InstallPaths {
                actrealm_home: root.join("flow"),
                claude_settings: root.join("home/.claude/settings.json"),
                codex_hooks: root.join("home/.codex/hooks.json"),
                codex_config: root.join("home/.codex/config.toml"),
            },
            root,
            source,
        }
    }

    fn installer(&self) -> Installer {
        Installer::new(self.paths.clone(), self.source.clone())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn write_json(path: &Path, value: &Value) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn existing_custom_statusline_is_never_replaced_or_backed_up_as_if_managed() {
    let fixture = Fixture::new("custom");
    let original = json!({
        "statusLine": {"type":"command","command":"~/.claude/my-line.sh","padding":2},
        "keep": true
    });
    write_json(&fixture.paths.claude_settings, &original);
    let installer = fixture.installer();

    let inspection = installer.inspect_claude_statusline().unwrap();
    assert_eq!(inspection.status, ClaudeStatuslineStatus::CustomConflict);
    let error = installer.install_claude_statusline().unwrap_err();
    assert!(error.to_string().contains("will not replace"));
    assert_eq!(read_json(&fixture.paths.claude_settings), original);
    assert!(!fixture.paths.statusline_helper().exists());
    assert!(!fixture.paths.stable_binary().exists());
    assert!(!fixture.paths.actrealm_home.join("backups").exists());
}

#[test]
fn explicit_wrapper_mode_preserves_runs_and_restores_the_custom_statusline() {
    let fixture = Fixture::new("custom-wrapper");
    let original_statusline = json!({
        "type":"command",
        "command":"node /Users/example/custom-statusline.js --compact",
        "padding":4
    });
    let original = json!({
        "statusLine": original_statusline,
        "keep": {"nested": true}
    });
    write_json(&fixture.paths.claude_settings, &original);
    let installer = fixture.installer();

    let report = installer.install_claude_statusline_wrapper().unwrap();
    assert!(report.config_changed);
    assert!(report.backup_path.unwrap().is_file());
    let installed = read_json(&fixture.paths.claude_settings);
    assert_eq!(installed["keep"], original["keep"]);
    assert_eq!(
        installed["_actRealmOriginalStatusLine"],
        original["statusLine"]
    );
    assert!(installed["statusLine"]["command"]
        .as_str()
        .unwrap()
        .contains("bin/statusline"));
    let helper = fs::read_to_string(fixture.paths.statusline_helper()).unwrap();
    let delegate = fs::read_to_string(fixture.paths.statusline_delegate()).unwrap();
    assert!(helper.contains(" statusline >/dev/null"));
    assert!(helper.contains("statusline-delegate"));
    assert!(delegate.contains("node /Users/example/custom-statusline.js --compact"));
    assert_eq!(
        installer.inspect_claude_statusline().unwrap().status,
        ClaudeStatuslineStatus::Installed
    );

    installer.uninstall_claude_statusline().unwrap();
    assert_eq!(read_json(&fixture.paths.claude_settings), original);
    assert!(!fixture.paths.statusline_helper().exists());
    assert!(!fixture.paths.statusline_delegate().exists());
}

#[test]
fn managed_statusline_round_trip_preserves_user_json_and_file_modes() {
    let fixture = Fixture::new("roundtrip");
    let original = json!({"keep": {"nested": [1,2,3]}});
    write_json(&fixture.paths.claude_settings, &original);
    let installer = fixture.installer();

    let report = installer.install_claude_statusline().unwrap();
    assert!(report.config_changed);
    let installed = read_json(&fixture.paths.claude_settings);
    assert_eq!(installed["keep"], original["keep"]);
    assert_eq!(installed["statusLine"]["type"], "command");
    assert!(installed["statusLine"]["command"]
        .as_str()
        .unwrap()
        .contains("bin/statusline"));
    assert_eq!(
        fs::metadata(fixture.paths.statusline_helper())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        installer.inspect_claude_statusline().unwrap().status,
        ClaudeStatuslineStatus::Installed
    );

    installer.uninstall_claude_statusline().unwrap();
    assert_eq!(read_json(&fixture.paths.claude_settings), original);
    assert!(!fixture.paths.statusline_helper().exists());
    assert!(!fixture.paths.stable_binary().exists());
}

#[test]
fn hook_uninstall_keeps_the_binary_needed_by_an_enabled_statusline() {
    let fixture = Fixture::new("shared-binary");
    let installer = fixture.installer();
    installer
        .install(HookProvider::Claude, InstallOptions::default())
        .unwrap();
    installer.install_claude_statusline().unwrap();
    installer.uninstall(HookProvider::Claude).unwrap();

    assert!(fixture.paths.stable_binary().exists());
    assert_eq!(
        installer.inspect_claude_statusline().unwrap().status,
        ClaudeStatuslineStatus::Installed
    );
}

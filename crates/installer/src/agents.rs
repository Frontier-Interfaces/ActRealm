//! Kimi/Grok installation. Hooks observe; live Provider channels own replies.
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
static TEMP: AtomicU64 = AtomicU64::new(0);
use toml_edit::{value, ArrayOfTables, DocumentMut, Table};

pub const AGENT_PROVIDERS: &[&str] = &["kimi", "grok"];
const COMMON_EVENTS: &[&str] = &[
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "Stop",
    "StopFailure",
    "Notification",
    "SubagentStart",
    "SubagentStop",
    "PreCompact",
    "PostCompact",
];

pub fn agent_executable(provider: &str) -> Option<PathBuf> {
    if !AGENT_PROVIDERS.contains(&provider) {
        return None;
    }
    let mut paths = Vec::new();
    if let Some(path) = std::env::var_os(format!("ACTREALM_{}_CLI", provider.to_uppercase())) {
        paths.push(PathBuf::from(path));
    }
    if let Some(path) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&path).map(|p| p.join(provider)));
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for dir in [".local/bin", ".grok/bin", ".kimi-code/bin"] {
            paths.push(home.join(dir).join(provider));
        }
    }
    paths.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(|p| Path::new(p).join(provider)));
    paths
        .into_iter()
        .find(|p| fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
}

pub struct AgentHooks {
    pub home: PathBuf,
    pub runtime_home: PathBuf,
}

impl AgentHooks {
    pub fn path(&self, provider: &str) -> io::Result<PathBuf> {
        match provider {
            "kimi" => Ok(self.home.join(".kimi-code/config.toml")),
            "grok" => Ok(self.home.join(".grok/hooks/actrealm.json")),
            _ => Err(io::Error::other("Unsupported Agent provider")),
        }
    }

    pub fn command(&self, provider: &str) -> String {
        format!(
            "{} hook --provider {provider}",
            quote(&self.runtime_home.join("bin/actrealm"))
        )
    }

    pub fn launch_command(&self, provider: &str) -> String {
        format!(
            "{} agent {provider} --cwd \"$PWD\"",
            quote(&self.runtime_home.join("bin/actrealm"))
        )
    }

    fn events(&self, provider: &str) -> Vec<&'static str> {
        let mut events = COMMON_EVENTS.to_vec();
        if provider == "kimi" {
            events.extend([
                "PermissionRequest",
                "PermissionResult",
                "SessionHeartbeat",
                "Interrupt",
            ]);
        }
        if provider == "grok" {
            events.push("PermissionDenied");
        }
        events
    }

    fn grok_definition(&self) -> Value {
        let hooks = self
            .events("grok")
            .into_iter()
            .map(|event| {
                (
                    event.to_owned(),
                    json!([
                        {"hooks":[{"type":"command", "command":self.command("grok"), "timeout":5}]}
                    ]),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        json!({"hooks": hooks})
    }

    fn installed(&self, provider: &str, text: &str) -> io::Result<bool> {
        if provider == "grok" {
            let definition: Value = serde_json::from_str(text).map_err(io::Error::other)?;
            return Ok(definition == self.grok_definition());
        }
        let doc: DocumentMut = text.parse().map_err(io::Error::other)?;
        let Some(hooks) = doc.get("hooks").and_then(|v| v.as_array_of_tables()) else {
            return Ok(false);
        };
        let command = self.command(provider);
        Ok(self.events(provider).into_iter().all(|event| {
            hooks.iter().any(|h| {
                h.get("event").and_then(|v| v.as_str()) == Some(event)
                    && h.get("command").and_then(|v| v.as_str()) == Some(command.as_str())
                    && h.get("timeout").and_then(|v| v.as_integer()) == Some(5)
            })
        }))
    }

    pub fn inspect(&self, provider: &str, last_event_at: Option<u64>) -> io::Result<Value> {
        let path = self.path(provider)?;
        let executable = agent_executable(provider);
        #[cfg(target_os = "macos")]
        let desktop_installed = provider == "kimi"
            && super::application_roots()
                .iter()
                .any(|p| p.join("Kimi Code.app/Contents/MacOS/Kimi Code").is_file());
        #[cfg(not(target_os = "macos"))]
        let desktop_installed = false;
        let text = read_optional(&path)?;
        let installed = text
            .as_deref()
            .map(|s| self.installed(provider, s))
            .transpose()?
            == Some(true);
        let state_path = self
            .runtime_home
            .join(format!("providers/{provider}-hooks.json"));
        let state: Value = read_optional(&state_path)?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        let verified = installed
            && state["installedAt"]
                .as_u64()
                .is_some_and(|at| last_event_at.is_some_and(|last| last >= at));
        let shared = provider != "grok" || self.grok_shared_enabled()?;
        Ok(json!({
            "provider":provider,
            "status":if executable.is_none() && !desktop_installed {"provider_missing"} else if installed && !shared {"needs_reinstall"} else if installed && verified {"connected"} else if installed {"installed_unverified"} else {"not_installed"},
            "cliInstalled":executable.is_some(), "desktopInstalled":desktop_installed,
            "cliPath":executable, "configPath":path, "realEventVerified":verified,
            "intent": if installed {"installed"} else {"untouched"},
            "ownedHandlers":if installed {self.events(provider).len()} else {0},
            "expectedHandlers":self.events(provider).len(), "canRepair":false,
            "launchCommand":self.launch_command(provider),
            "guideURL":if provider == "kimi" {"https://moonshotai.github.io/kimi-code/en/guides/getting-started.html"} else {"https://x.ai/build"},
            "connectionMode":if provider=="grok" {"grok_shared_session"} else {"kimi_local_sessions"}
        }))
    }

    fn grok_shared_enabled(&self) -> io::Result<bool> {
        let text = read_optional(&self.home.join(".grok/config.toml"))?.unwrap_or_default();
        let doc: DocumentMut = text.parse().map_err(io::Error::other)?;
        Ok(doc
            .get("cli")
            .and_then(|v| v.get("use_leader"))
            .and_then(|v| v.as_bool())
            == Some(true))
    }

    // Save the previous single preference, not credentials or the whole config
    // in installation state. Uninstall only restores a value we still own.
    fn configure_grok_shared(&self, install: bool, state: &mut Value) -> io::Result<()> {
        let path = self.home.join(".grok/config.toml");
        let old = read_optional(&path)?.unwrap_or_default();
        let mut doc: DocumentMut = old.parse().map_err(io::Error::other)?;
        if doc.get("cli").is_some_and(|v| !v.is_table()) {
            return Err(io::Error::other("Grok cli configuration is not a table"));
        }
        let previous = doc
            .get("cli")
            .and_then(|v| v.get("use_leader"))
            .and_then(|v| v.as_bool());
        if doc.get("cli").and_then(|v| v.get("use_leader")).is_some() && previous.is_none() {
            return Err(io::Error::other(
                "Grok use_leader configuration is not a boolean",
            ));
        }
        if install {
            if state.get("grokUseLeaderBefore").is_none() {
                state["grokUseLeaderBefore"] = json!(previous);
            }
            if doc.get("cli").is_none() {
                doc["cli"] = toml_edit::Item::Table(Table::new());
            }
            doc["cli"]["use_leader"] = value(true);
        } else if previous == Some(true) && state.get("grokUseLeaderBefore").is_some() {
            if let Some(before) = state["grokUseLeaderBefore"].as_bool() {
                doc["cli"]["use_leader"] = value(before);
            } else if let Some(table) = doc.get_mut("cli").and_then(|v| v.as_table_mut()) {
                table.remove("use_leader");
            }
            state.as_object_mut().unwrap().remove("grokUseLeaderBefore");
        }
        if doc.to_string() != old {
            let backup = self
                .runtime_home
                .join(format!("backups/providers/grok-config-{}.bak", unique()));
            self.prepare_backup_directory()?;
            atomic_write(&backup, old.as_bytes())?;
            private_directory(path.parent().unwrap())?;
            atomic_write(&path, doc.to_string().as_bytes())?;
        }
        Ok(())
    }

    fn prepare_backup_directory(&self) -> io::Result<()> {
        for suffix in ["backups", "backups/providers"] {
            let path = self.runtime_home.join(suffix);
            private_directory(&path)?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }

    pub fn change(&self, provider: &str, install: bool) -> io::Result<()> {
        let path = self.path(provider)?;
        private_directory(&self.runtime_home.join("providers"))?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(self.runtime_home.join("providers/hooks.lock"))?;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let old = read_optional(&path)?;
        let content = if provider == "grok" {
            if let Some(old) = old.as_deref() {
                if !self.installed(provider, old)? {
                    return Err(io::Error::other("The ActRealm Grok hook file was edited; preserve it and resolve the conflict first"));
                }
            }
            if install {
                Some(
                    serde_json::to_string_pretty(&self.grok_definition())
                        .map_err(io::Error::other)?
                        + "\n",
                )
            } else {
                None
            }
        } else {
            let mut doc: DocumentMut = old
                .as_deref()
                .unwrap_or("")
                .parse()
                .map_err(io::Error::other)?;
            if doc.contains_key("hooks") && doc["hooks"].as_array_of_tables().is_none() {
                return Err(io::Error::other("Kimi hooks must be an array of tables"));
            }
            let command = self.command(provider);
            let mut hooks = doc
                .get("hooks")
                .and_then(|v| v.as_array_of_tables())
                .cloned()
                .unwrap_or_default();
            let mut retained = ArrayOfTables::new();
            for item in hooks.iter() {
                if item.get("command").and_then(|v| v.as_str()) == Some(command.as_str())
                    && (item
                        .get("event")
                        .and_then(|v| v.as_str())
                        .is_none_or(|event| !self.events(provider).contains(&event))
                        || item.get("timeout").and_then(|v| v.as_integer()) != Some(5)
                        || item.contains_key("matcher"))
                {
                    return Err(io::Error::other("An ActRealm Kimi hook was edited; preserve it and resolve the conflict first"));
                }
                if item.get("command").and_then(|v| v.as_str()) != Some(command.as_str()) {
                    retained.push(item.clone());
                }
            }
            if install {
                for event in self.events(provider) {
                    let mut hook = Table::new();
                    hook["event"] = value(event);
                    hook["command"] = value(command.clone());
                    hook["timeout"] = value(5);
                    retained.push(hook);
                }
            }
            hooks = retained;
            if hooks.is_empty() {
                doc.remove("hooks");
            } else {
                doc["hooks"] = toml_edit::Item::ArrayOfTables(hooks);
            }
            Some(doc.to_string())
        };
        if let Some(old) = old {
            let backup =
                self.runtime_home
                    .join(format!("backups/providers/{}-{}.bak", provider, unique()));
            self.prepare_backup_directory()?;
            atomic_write(&backup, old.as_bytes())?;
        }
        if let Some(content) = content {
            private_directory(path.parent().unwrap())?;
            atomic_write(&path, content.as_bytes())?;
        } else if path.exists() {
            fs::remove_file(&path)?;
        }
        let state_path = self
            .runtime_home
            .join(format!("providers/{provider}-hooks.json"));
        let mut state = read_optional(&state_path)?
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        if provider == "grok" {
            self.configure_grok_shared(install, &mut state)?;
        }
        state["installedAt"] = json!(now());
        state["installed"] = json!(install);
        atomic_write(
            &state_path,
            serde_json::to_string(&state).unwrap().as_bytes(),
        )
    }
}

fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
fn unique() -> String {
    format!(
        "{}-{}-{}",
        now(),
        std::process::id(),
        TEMP.fetch_add(1, Ordering::Relaxed)
    )
}
fn read_optional(path: &Path) -> io::Result<Option<String>> {
    match fs::symlink_metadata(path) {
        Ok(m) if !m.is_file() || m.file_type().is_symlink() || m.len() > 4 * 1024 * 1024 => Err(
            io::Error::other("Unsafe or oversized provider configuration"),
        ),
        Ok(_) => fs::read_to_string(path).map(Some),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}
fn private_directory(path: &Path) -> io::Result<()> {
    for parent in path.ancestors() {
        if fs::symlink_metadata(parent).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(io::Error::other("Refusing a symlinked provider directory"));
        }
    }
    fs::create_dir_all(path)
}
fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension(format!("tmp-{}", unique()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> AgentHooks {
        let home = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("actrealm-agent-install-{}", unique()));
        fs::create_dir_all(&home).unwrap();
        AgentHooks {
            runtime_home: home.join(".actrealm"),
            home,
        }
    }

    #[test]
    fn provider_backups_are_counted_but_unknown_files_and_symlinks_block_deletion() {
        let hooks = setup();
        let kimi = hooks.path("kimi").unwrap();
        fs::create_dir_all(kimi.parent().unwrap()).unwrap();
        fs::write(&kimi, "model = 'original'\n").unwrap();
        hooks.change("kimi", true).unwrap();
        hooks.change("grok", true).unwrap();
        let installer = crate::Installer::new(
            crate::InstallPaths {
                actrealm_home: hooks.runtime_home.clone(),
                claude_settings: hooks.home.join(".claude/settings.json"),
                codex_hooks: hooks.home.join(".codex/hooks.json"),
                codex_config: hooks.home.join(".codex/config.toml"),
            },
            hooks.runtime_home.join("bin/actrealm"),
        );
        let summary = installer.backup_summary().unwrap();
        assert_eq!(summary.count, 2);
        assert!(summary.total_bytes > 0);
        let directory = hooks.runtime_home.join("backups/providers");
        let unknown = directory.join("personal-notes.txt");
        fs::write(&unknown, "keep").unwrap();
        fs::set_permissions(&unknown, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(installer.backup_summary().is_err());
        assert!(installer.clear_backups().is_err());
        assert_eq!(fs::read_to_string(&unknown).unwrap(), "keep");
        fs::remove_file(&unknown).unwrap();
        let link = directory.join("grok-1-2-3.bak");
        std::os::unix::fs::symlink(&kimi, &link).unwrap();
        assert!(installer.clear_backups().is_err());
        assert!(kimi.exists());
        fs::remove_file(&link).unwrap();
        assert_eq!(installer.clear_backups().unwrap().removed_count, 2);
        assert_eq!(installer.backup_summary().unwrap().count, 0);
        assert!(kimi.exists());
        fs::remove_dir_all(hooks.home).unwrap();
    }
    #[test]
    fn kimi_install_and_remove_preserve_user_hooks_and_config() {
        let hooks = setup();
        let path = hooks.path("kimi").unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original =
            "model = 'my-model'\n\n[[hooks]]\nevent = 'Stop'\ncommand = 'my-check'\ntimeout = 30\n";
        fs::write(&path, original).unwrap();
        hooks.change("kimi", true).unwrap();
        assert!(hooks
            .installed("kimi", &fs::read_to_string(&path).unwrap())
            .unwrap());
        hooks.change("kimi", true).unwrap();
        hooks.change("kimi", false).unwrap();
        let doc: DocumentMut = fs::read_to_string(&path).unwrap().parse().unwrap();
        assert_eq!(doc["model"].as_str(), Some("my-model"));
        let retained = doc["hooks"].as_array_of_tables().unwrap();
        assert_eq!(retained.len(), 1);
        assert_eq!(
            retained.get(0).unwrap()["command"].as_str(),
            Some("my-check")
        );
        fs::remove_dir_all(hooks.home).unwrap();
    }
    #[test]
    fn grok_edited_file_and_symlink_are_never_overwritten() {
        let hooks = setup();
        hooks.change("grok", true).unwrap();
        let path = hooks.path("grok").unwrap();
        fs::write(&path, "{\"custom\":true}").unwrap();
        assert!(hooks.change("grok", false).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"custom\":true}");
        fs::remove_file(&path).unwrap();
        let target = hooks.home.join("user.json");
        fs::write(&target, "private").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(hooks.change("grok", true).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "private");
        fs::remove_dir_all(hooks.home).unwrap();
    }

    #[test]
    fn grok_shared_preference_is_idempotent_and_restores_only_its_own_change() {
        for previous in [None, Some(false), Some(true)] {
            let hooks = setup();
            let path = hooks.home.join(".grok/config.toml");
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let mut original = "model = 'custom'\n[cli]\nauto_update = false\n".to_owned();
            if let Some(v) = previous {
                original += &format!("use_leader = {v}\n");
            }
            fs::write(&path, &original).unwrap();
            hooks.change("grok", true).unwrap();
            hooks.change("grok", true).unwrap();
            assert!(hooks.grok_shared_enabled().unwrap());
            hooks.change("grok", false).unwrap();
            let doc: DocumentMut = fs::read_to_string(&path).unwrap().parse().unwrap();
            assert_eq!(doc["model"].as_str(), Some("custom"));
            assert_eq!(doc["cli"]["auto_update"].as_bool(), Some(false));
            assert_eq!(
                doc["cli"].get("use_leader").and_then(|v| v.as_bool()),
                previous
            );
            fs::remove_dir_all(hooks.home).unwrap();
        }
        let hooks = setup();
        hooks.change("grok", true).unwrap();
        let path = hooks.home.join(".grok/config.toml");
        fs::write(&path, "[cli]\nuse_leader = false\n").unwrap();
        hooks.change("grok", false).unwrap();
        assert!(!hooks.grok_shared_enabled().unwrap());
        fs::remove_dir_all(hooks.home).unwrap();
    }
}

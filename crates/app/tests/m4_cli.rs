use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static ID: AtomicU64 = AtomicU64::new(0);

struct Root(PathBuf);

impl Root {
    fn new(name: &str) -> Self {
        let path = PathBuf::from("/tmp").join(format!(
            "actrealm-m4-cli-{name}-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_actrealm"));
        command
            .env("HOME", self.0.join("home"))
            .env("ACTREALM_HOME", self.0.join("actrealm-home"))
            .env("CODEX_HOME", self.0.join("codex-home"));
        command
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn statusline_command_caches_only_quota_and_prints_remaining_windows() {
    let root = Root::new("statusline");
    let mut child = root
        .command()
        .arg("statusline")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            br#"{
              "session_id":"must-not-persist",
              "cwd":"/private/workspace",
              "rate_limits":{
                "five_hour":{"used_percentage":20,"resets_at":1784140000},
                "seven_day":{"used_percentage":60,"resets_at":1784740000}
              }
            }"#,
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("5h 剩余 80%"));
    assert!(stdout.contains("7d 剩余 40%"));
    assert!(output.stderr.is_empty());

    let cache = fs::read_to_string(root.0.join("actrealm-home/cache/claude-rl.json")).unwrap();
    assert!(!cache.contains("must-not-persist"));
    assert!(!cache.contains("private/workspace"));
}

#[test]
fn malformed_statusline_input_is_nonfatal_and_never_creates_a_cache() {
    let root = Root::new("malformed");
    let mut child = root
        .command()
        .arg("statusline")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"not-json").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("额度暂不可用"));
    assert!(!root.0.join("actrealm-home/cache/claude-rl.json").exists());
}

#[test]
fn export_command_emits_every_local_table_as_json() {
    let root = Root::new("export");
    let output = root.command().arg("export").output().unwrap();
    assert!(output.status.success());
    let export: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(export["schemaVersion"], 1);
    for table in [
        "sessions",
        "events",
        "attention_items",
        "commands",
        "quota_snapshots",
        "settings",
    ] {
        assert!(export["tables"][table].is_array(), "missing table {table}");
    }
}

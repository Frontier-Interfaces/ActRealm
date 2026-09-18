//! One user-owned launchd service shared by both native apps. No token is
//! printed, stored in the plist, or passed through launchctl arguments.
use actrealm_server::native_transport::{signing, RUNTIME_IDENTIFIER};
use anyhow::{bail, Context, Result};
use clap::ValueEnum;
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const LABEL: &str = "com.frontierinterfaces.actrealm.runtime";

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum Action {
    Ensure,
    Restart,
    Status,
}

pub(crate) fn run(action: Action) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("Shared service management requires macOS");
    }
    let home = PathBuf::from(std::env::var_os("HOME").context("Missing user home")?);
    let root = std::env::var_os("ACTREALM_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".actrealm"));
    let status = || read_health(&root);
    let previous_instance =
        status().and_then(|health| health["instanceId"].as_str().map(str::to_owned));
    if matches!(action, Action::Status) {
        println!("{}", status().unwrap_or_else(|| json!({"ok": false})));
        return Ok(());
    }
    let team =
        signing::current_team().context("Runtime helper must have a trusted signing identity")?;
    private_directory(&root)?;
    let run = root.join("run");
    private_directory(&run)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(run.join("service-install.lock"))?;
    let lock_metadata = lock.metadata()?;
    if !lock_metadata.is_file()
        || lock_metadata.uid() != unsafe { libc::geteuid() }
        || lock_metadata.mode() & 0o077 != 0
    {
        bail!("Unsafe service installation lock");
    }
    let started = Instant::now();
    while unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        if started.elapsed() > Duration::from_secs(15) {
            bail!("Service installation is busy");
        }
        thread::sleep(Duration::from_millis(50));
    }
    // Do not terminate another app's Runtime to acquire its bootstrap token.
    // Existing signed protocol-v7 services are shared, regardless of launcher.
    if let Some(health) = status() {
        if health["protocolVersion"].as_u64().unwrap_or(0) < 7 {
            bail!("An older Runtime is running; quit its owning app before upgrading the shared service");
        }
        if matches!(action, Action::Ensure) {
            println!("{health}");
            return Ok(());
        }
    }
    let bin = root.join("bin");
    let logs = root.join("logs");
    private_directory(&bin)?;
    private_directory(&logs)?;
    let binary = bin.join("actrealm");
    let installed = match fs::symlink_metadata(&binary) {
        Ok(metadata) => {
            if !metadata.is_file()
                || metadata.uid() != unsafe { libc::geteuid() }
                || metadata.mode() & 0o022 != 0
            {
                bail!("Shared Runtime is not a private user-owned executable");
            }
            if signing::validate_binary(&binary, &team, RUNTIME_IDENTIFIER).is_ok() {
                true
            } else {
                signing::validate_binary(&binary, &team, "actrealm")
                    .context("Refusing to replace an untrusted shared Runtime")?;
                false
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    if !installed {
        let source = std::env::current_exe()?;
        signing::validate_binary(&source, &team, RUNTIME_IDENTIFIER)?;
        let temporary = bin.join(format!(".native-service-{}", uuid::Uuid::now_v7()));
        let result = (|| -> Result<()> {
            let mut input = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(source)?;
            let mut output = OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o700)
                .open(&temporary)?;
            std::io::copy(&mut input, &mut output)?;
            output.sync_all()?;
            signing::validate_binary(&temporary, &team, RUNTIME_IDENTIFIER)?;
            fs::rename(&temporary, &binary)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result?;
    }
    let library = home.join("Library");
    let agents = library.join("LaunchAgents");
    fs::create_dir_all(&agents)?;
    let metadata = fs::symlink_metadata(&agents)?;
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o022 != 0
    {
        bail!("Unsafe user LaunchAgents directory");
    }
    let log = logs.join("runtime-service.log");
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&log)?;
    if file.metadata()?.uid() != unsafe { libc::geteuid() } || !file.metadata()?.is_file() {
        bail!("Unsafe service log");
    }
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    let plist = agents.join(format!("{LABEL}.plist"));
    let contents = launch_agent_plist(&binary, &root, &log);
    let domain = format!("gui/{}", unsafe { libc::geteuid() });
    let target = format!("{domain}/{LABEL}");
    let loaded = launchctl(&["print", &target])?;
    let unchanged = fs::symlink_metadata(&plist).is_ok_and(|m| {
        m.is_file() && m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o022 == 0
    }) && fs::read_to_string(&plist).is_ok_and(|text| text == contents);
    if loaded && !unchanged {
        bail!(
            "Registered Runtime service configuration differs; repair it before replacing the job"
        );
    }
    if !unchanged {
        if let Ok(metadata) = fs::symlink_metadata(&plist) {
            if !metadata.is_file() || metadata.uid() != unsafe { libc::geteuid() } {
                bail!("Unsafe service plist");
            }
        }
        let temporary = agents.join(format!(".{LABEL}-{}.tmp", uuid::Uuid::now_v7()));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        fs::rename(temporary, &plist)?;
    }
    if !loaded
        && !launchctl(&[
            "bootstrap",
            &domain,
            plist.to_str().context("Invalid service path")?,
        ])?
    {
        bail!("macOS could not register the local Runtime service; check background-item settings");
    }
    let launched = if matches!(action, Action::Restart) {
        launchctl(&["kickstart", "-k", &target])?
    } else {
        launchctl(&["kickstart", &target])?
    };
    if !launched {
        bail!("macOS could not start the local Runtime service");
    }
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(15) {
        if let Some(health) = status() {
            if health["protocolVersion"].as_u64().unwrap_or(0) >= 7
                && (!matches!(action, Action::Restart)
                    || health["instanceId"].as_str() != previous_instance.as_deref())
            {
                println!("{health}");
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    bail!("Runtime service did not become ready; inspect the private service log")
}

fn private_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } => {
            bail!("Unsafe Runtime directory")
        }
        Ok(_) => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(path)?,
        Err(error) => return Err(error.into()),
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn launchctl(arguments: &[&str]) -> Result<bool> {
    let mut child = Command::new("/bin/launchctl")
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.success());
        }
        if start.elapsed() >= Duration::from_secs(8) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("launchctl timed out");
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn read_health(root: &Path) -> Option<Value> {
    let path = root.join("run/companion-endpoint.json");
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
        || metadata.len() > 4096
    {
        return None;
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .ok()?;
    let discovery: Value = serde_json::from_reader(file.take(4096)).ok()?;
    let endpoint = discovery["endpoint"].as_str()?.trim_end_matches('/');
    let address = endpoint
        .strip_prefix("http://")?
        .parse::<SocketAddr>()
        .ok()?;
    if !address.ip().is_loopback() || address.port() == 0 {
        return None;
    }
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(400)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .ok()?;
    stream
        .set_write_timeout(Some(Duration::from_millis(500)))
        .ok()?;
    write!(
        stream,
        "GET /api/v1/health HTTP/1.0\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut response = String::new();
    stream.take(16 * 1024).read_to_string(&mut response).ok()?;
    let (headers, body) = response.split_once("\r\n\r\n")?;
    if !headers.lines().next()?.contains(" 200 ") {
        return None;
    }
    let mut health: Value = serde_json::from_str(body).ok()?;
    if health["ok"] != true {
        return None;
    }
    health["endpoint"] = json!(endpoint);
    health["instanceId"] = discovery["instanceId"].clone();
    Some(health)
}

fn xml(value: &Path) -> String {
    value
        .to_string_lossy()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn launch_agent_plist(binary: &Path, root: &Path, log: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{LABEL}</string>
<key>ProgramArguments</key><array><string>{}</string><string>serve</string><string>--service</string></array>
<key>EnvironmentVariables</key><dict><key>ACTREALM_HOME</key><string>{}</string><key>PATH</key><string>/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string></dict>
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><true/>
<key>ThrottleInterval</key><integer>5</integer>
<key>ProcessType</key><string>Background</string>
<key>StandardOutPath</key><string>{}</string>
<key>StandardErrorPath</key><string>{}</string>
<key>Umask</key><integer>63</integer>
</dict></plist>
"#,
        xml(binary),
        xml(root),
        xml(log),
        xml(log)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_job_is_user_scoped_secret_free_and_independent_of_app_bundles() {
        let plist = launch_agent_plist(
            Path::new("/tmp/A & B/bin/actrealm"),
            Path::new("/tmp/A & B"),
            Path::new("/tmp/A & B/log"),
        );
        assert!(plist.contains("/tmp/A &amp; B/bin/actrealm"));
        assert!(plist.contains("<string>--service</string>"));
        assert!(plist.contains("<key>KeepAlive</key><true/>"));
        assert!(!plist.contains("bootstrap="));
        assert!(!plist.contains("/Applications/"));
        assert!(!plist.contains("<key>UserName</key>"));
    }
}

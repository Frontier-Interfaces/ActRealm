//! Let the credential owner renew its own OAuth chain. No inference prompt,
//! tools, hooks, MCP servers, transcript capture, or credential writes here.
use std::fs::{self, DirBuilder, File};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

struct ProbeChild(Child);

impl Drop for ProbeChild {
    fn drop(&mut self) {
        // The child owns a new session. Reap it and any subprocesses on every
        // exit path, including timeout, a closed PTY, and a successful renewal.
        unsafe { libc::kill(-(self.0.id() as i32), libc::SIGKILL) };
        let _ = self.0.wait();
    }
}

pub(super) fn signed_in(executable: &Path) -> bool {
    super::bounded_command_output(
        Command::new(executable).args(["auth", "status", "--json"]),
        64 * 1024,
        Duration::from_secs(3),
    )
    .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
    .is_some_and(|status| status["loggedIn"] == true)
}

pub(super) fn refresh(
    executable: &Path,
    directory: &Path,
    timeout: Duration,
    mut credential_changed: impl FnMut() -> bool,
) -> io::Result<bool> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)?;
    let metadata = fs::symlink_metadata(directory)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::other("unsafe Claude quota probe directory"));
    }
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    let (mut master, slave) = terminal()?;
    let mut command = Command::new(executable);
    command
        .args([
            "--tools", "", "--strict-mcp-config", "--mcp-config", "{\"mcpServers\":{}}",
            "--setting-sources", "", "--settings",
            "{\"disableAllHooks\":true,\"remoteControlAtStartup\":false,\"disableDeepLinkRegistration\":\"disable\"}",
            "--no-chrome",
        ])
        .current_dir(directory)
        .env_remove("CLAUDECODE")
        .env("TERM", "xterm-256color")
        .env("DISABLE_AUTOUPDATER", "1")
        .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
        .stdin(Stdio::from(slave.try_clone()?))
        .stdout(Stdio::from(slave.try_clone()?))
        .stderr(Stdio::from(slave));
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = ProbeChild(command.spawn()?);
    let started = Instant::now();
    let mut next_check = Duration::from_millis(500);
    let mut sent_status = false;
    let mut screen = Vec::new();
    let mut buffer = [0; 8192];
    while started.elapsed() < timeout {
        match master.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                screen.extend_from_slice(&buffer[..count]);
                if buffer[..count].windows(4).any(|bytes| bytes == b"\x1b[6n") {
                    let _ = master.write_all(b"\x1b[1;1R");
                }
                if screen.len() > 32 * 1024 {
                    screen.drain(..screen.len() - 16 * 1024);
                }
                // Never type into a login/trust/onboarding prompt. Startup
                // renewal may already have updated the credential before it.
                let text = String::from_utf8_lossy(&screen).to_ascii_lowercase();
                if [
                    "select login method",
                    "sign in to claude",
                    "not logged in",
                    "trust this folder",
                    "quick safety check",
                ]
                .iter()
                .any(|marker| text.contains(marker))
                {
                    return Ok(credential_changed());
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(_) => break,
        }
        if started.elapsed() >= next_check {
            if credential_changed() {
                return Ok(true);
            }
            next_check = started.elapsed() + Duration::from_secs(1);
        }
        // Only a built-in local command is sent; never an inference request.
        // If onboarding blocks it, the unchanged credential means failure.
        if !sent_status && started.elapsed() >= Duration::from_secs(2) {
            master.write_all(b"/status\r")?;
            sent_status = true;
        }
        if child.0.try_wait()?.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    Ok(credential_changed())
}

fn terminal() -> io::Result<(File, File)> {
    let (mut master, mut slave) = (-1, -1);
    let mut size = libc::winsize {
        ws_row: 50,
        ws_col: 160,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    if unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    } == -1
    {
        return Err(io::Error::last_os_error());
    }
    let master = unsafe { File::from_raw_fd(master) };
    let slave = unsafe { File::from_raw_fd(slave) };
    for file in [&master, &slave] {
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    if unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok((master, slave))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_process_exit_is_not_credential_renewal() {
        let root = super::super::tests::root("auth-probe-exit");
        assert!(!refresh(
            Path::new("/usr/bin/true"),
            &root,
            Duration::from_secs(1),
            || false
        )
        .unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn real_terminal_renewal_is_detected_and_process_is_reaped() {
        let root = super::super::tests::root("auth-probe-terminal");
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("fake-claude");
        fs::write(&executable, "#!/bin/sh\n[ -t 0 ] && [ -t 1 ] || exit 1\nprintf '%s' $$ > pid\ntouch renewed\nwhile :; do sleep 1; done\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(refresh(&executable, &root, Duration::from_secs(2), || root
            .join("renewed")
            .exists())
        .unwrap());
        let pid: i32 = fs::read_to_string(root.join("pid"))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn blocked_login_prompt_is_bounded_and_never_accepted() {
        let root = super::super::tests::root("auth-probe-timeout");
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("fake-claude");
        fs::write(&executable, "#!/bin/sh\nprintf 'Sign in to Claude Code\\n'\nread -r reply\nprintf '%s' \"$reply\" > accepted\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(!refresh(&executable, &root, Duration::from_millis(150), || false).unwrap());
        assert!(!root.join("accepted").exists());
        fs::remove_dir_all(root).unwrap();
    }
}

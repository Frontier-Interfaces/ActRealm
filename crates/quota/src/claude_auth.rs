//! Let the credential owner renew its own OAuth chain. No inference prompt,
//! tools, hooks, MCP servers, transcript capture, or credential writes here.
use std::fs::{self, DirBuilder, File};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// How long cleanup waits for a killed probe before leaving it to a reaper.
const PROBE_REAP_TIMEOUT: Duration = Duration::from_secs(2);

/// A probe that leads its own session, and the PTY master that is its
/// controlling terminal.
struct ProbeChild {
    child: Child,
    master: Option<File>,
}

impl Drop for ProbeChild {
    fn drop(&mut self) {
        // The child owns a new session. Reap it and any subprocesses on every
        // exit path, including timeout, a closed PTY, and a successful renewal.
        unsafe { libc::kill(-(self.child.id() as i32), libc::SIGKILL) };
        // macOS finishes a session leader's exit only after its terminal
        // output drains, and nothing reads the master from here on. Close it
        // first: waiting with it open held the quota job for hours.
        drop(self.master.take());
        let deadline = Instant::now() + PROBE_REAP_TIMEOUT;
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Ok(Some(_)) | Err(_) => return,
            }
        }
        // Never hold the quota job on a process that cannot finish exiting.
        let pid = self.child.id() as libc::pid_t;
        let _ = thread::Builder::new()
            .name("actrealm-claude-probe-reaper".to_owned())
            .spawn(move || {
                let mut status = 0;
                unsafe { libc::waitpid(pid, &mut status, 0) };
            });
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
    credential_changed: impl FnMut() -> bool,
) -> io::Result<bool> {
    refresh_command(
        Command::new(executable),
        directory,
        timeout,
        credential_changed,
    )
}

fn refresh_command(
    mut command: Command,
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
    let (master, slave) = terminal()?;
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
    let mut probe = ProbeChild {
        child: command.spawn()?,
        master: Some(master),
    };
    // Command retains its configured Stdio handles after spawn. Close the
    // parent's slave handles so only the child owns that side of the PTY.
    drop(command);
    let ProbeChild {
        child,
        master: Some(master),
    } = &mut probe
    else {
        return Ok(credential_changed());
    };
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
        if child.try_wait()?.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    Ok(credential_changed())
}

const TERMINAL_SIZE: libc::winsize = libc::winsize {
    ws_row: 50,
    ws_col: 160,
    ws_xpixel: 0,
    ws_ypixel: 0,
};

/// Opens the probe's PTY with both ends close-on-exec from the start. std
/// sets that flag in the same call that opens a file, while openpty() sets it
/// only afterwards: a child another thread spawned in between would inherit
/// the master, and closing ours would then no longer end the terminal.
#[cfg(target_os = "macos")]
fn terminal() -> io::Result<(File, File)> {
    use std::ffi::{CStr, OsStr};
    use std::fs::OpenOptions;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;

    let open = |path: &Path| {
        OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open(path)
    };
    let master = open(Path::new("/dev/ptmx"))?;
    let fd = master.as_raw_fd();
    if unsafe { libc::grantpt(fd) } == -1 || unsafe { libc::unlockpt(fd) } == -1 {
        return Err(io::Error::last_os_error());
    }
    let mut name = [0u8; 128];
    if unsafe { libc::ioctl(fd, libc::TIOCPTYGNAME as _, name.as_mut_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    let name = CStr::from_bytes_until_nul(&name)
        .map_err(|_| io::Error::other("unnamed Claude quota probe terminal"))?;
    let slave = open(Path::new(OsStr::from_bytes(name.to_bytes())))?;
    if unsafe { libc::ioctl(slave.as_raw_fd(), libc::TIOCSWINSZ, &TERMINAL_SIZE) } == -1 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok((master, slave))
}

#[cfg(not(target_os = "macos"))]
fn terminal() -> io::Result<(File, File)> {
    use std::os::fd::FromRawFd;

    let (mut master, mut slave) = (-1, -1);
    let mut size = TERMINAL_SIZE;
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

    fn fixture_command(script: &Path) -> Command {
        // Use the signed system shell directly so PTY timing tests do not
        // depend on first-launch inspection of freshly created executables.
        let mut command = Command::new("/bin/sh");
        command.arg(script);
        command
    }

    #[test]
    fn probe_terminal_is_a_sized_nonblocking_tty_closed_on_exec() {
        let (master, slave) = terminal().unwrap();
        for file in [&master, &slave] {
            let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
            assert_ne!(flags, -1);
            assert_ne!(flags & libc::FD_CLOEXEC, 0);
        }
        assert_eq!(unsafe { libc::isatty(slave.as_raw_fd()) }, 1);
        let mut size = libc::winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        assert_ne!(
            unsafe { libc::ioctl(slave.as_raw_fd(), libc::TIOCGWINSZ, &mut size) },
            -1
        );
        assert_eq!((size.ws_row, size.ws_col), (50, 160));
        let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
        assert_ne!(flags, -1);
        assert_ne!(flags & libc::O_NONBLOCK, 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn children_spawned_while_probe_terminals_open_never_inherit_them() {
        // openpty() marked both ends close-on-exec only after opening them,
        // so a child spawned by another thread in between kept the terminal.
        let baseline = device_fds(&spawn_sleeper());
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let spawning = stop.clone();
        let spawner = thread::spawn(move || {
            let mut children = Vec::new();
            while !spawning.load(std::sync::atomic::Ordering::Relaxed) && children.len() < 200 {
                children.push(spawn_sleeper());
            }
            children
        });
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(300) {
            drop(terminal().unwrap());
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let mut children = spawner.join().unwrap();
        // Let the last children finish starting before looking at them.
        thread::sleep(Duration::from_millis(50));
        let leaked = children
            .iter()
            .filter(|child| device_fds(child) > baseline)
            .count();
        for child in &mut children {
            let _ = child.kill();
            let _ = child.wait();
        }
        assert_eq!(leaked, 0, "of {} children", children.len());
    }

    #[cfg(target_os = "macos")]
    fn spawn_sleeper() -> Child {
        Command::new("/bin/sleep")
            .arg("5")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    /// Open files above stdio. A terminal is a file; pipes and sockets other
    /// tests may leak into children are not counted.
    #[cfg(target_os = "macos")]
    fn device_fds(child: &Child) -> usize {
        let empty = libc::proc_fdinfo {
            proc_fd: 0,
            proc_fdtype: 0,
        };
        let mut fds = vec![empty; 64];
        let size = std::mem::size_of_val(fds.as_slice()) as libc::c_int;
        let bytes = unsafe {
            libc::proc_pidinfo(
                child.id() as libc::c_int,
                libc::PROC_PIDLISTFDS,
                0,
                fds.as_mut_ptr().cast(),
                size,
            )
        };
        assert!(bytes > 0);
        fds.truncate(bytes as usize / std::mem::size_of::<libc::proc_fdinfo>());
        fds.iter()
            .filter(|fd| fd.proc_fd > 2 && fd.proc_fdtype as libc::c_int == libc::PROX_FDTYPE_VNODE)
            .count()
    }

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
        fs::write(&executable, "[ -t 0 ] && [ -t 1 ] || exit 1\nprintf '%s' $$ > pid\n: > renewed\nwhile :; do sleep 1; done\n").unwrap();
        assert!(refresh_command(
            fixture_command(&executable),
            &root,
            Duration::from_secs(2),
            || root.join("renewed").exists()
        )
        .unwrap());
        let pid: i32 = fs::read_to_string(root.join("pid"))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn renewal_returns_while_the_probe_is_still_writing_to_its_terminal() {
        // macOS finishes a session leader's exit only after its terminal
        // output drains. A probe still printing when renewal is detected
        // once held the quota job in wait() for hours.
        let root = super::super::tests::root("auth-probe-flood");
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("fake-claude");
        fs::write(
            &executable,
            "[ -t 1 ] || exit 1\nprintf '%s' $$ > pid\n: > renewed\nwhile :; do printf '%s\\n' 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'; done\n",
        )
        .unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let renewed_at = std::sync::Arc::new(std::sync::Mutex::new(None::<Instant>));
        let seen = renewed_at.clone();
        let probe_root = root.clone();
        thread::spawn(move || {
            let renewed = refresh_command(
                fixture_command(&probe_root.join("fake-claude")),
                &probe_root,
                Duration::from_secs(5),
                || {
                    let renewed = probe_root.join("renewed").exists();
                    if renewed {
                        seen.lock().unwrap().get_or_insert_with(Instant::now);
                    }
                    renewed
                },
            );
            let _ = sender.send((renewed.unwrap(), Instant::now()));
        });
        let (renewed, returned) = receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("probe cleanup must not wait on an undrained terminal");
        assert!(renewed);
        // Closing the terminal first lets the probe exit at once. Waiting on
        // it first would run out the whole reap timeout instead.
        let renewed_at = renewed_at.lock().unwrap().expect("renewal was seen");
        assert!(returned.duration_since(renewed_at) < PROBE_REAP_TIMEOUT / 2);
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
        fs::write(&executable, "printf '%s' $$ > pid\nprintf 'Sign in to Claude Code\\n'\nread -r reply\nprintf '%s' \"$reply\" > accepted\n").unwrap();
        let started = Instant::now();
        assert!(!refresh_command(
            fixture_command(&executable),
            &root,
            Duration::from_secs(1),
            || false
        )
        .unwrap());
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(!root.join("accepted").exists());
        let pid: i32 = fs::read_to_string(root.join("pid"))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn silent_terminal_is_bounded_and_process_is_reaped() {
        let root = super::super::tests::root("auth-probe-silent");
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("fake-claude");
        fs::write(
            &executable,
            "printf '%s' $$ > pid\nread -r reply\nprintf '%s' \"$reply\" > accepted\n",
        )
        .unwrap();
        let started = Instant::now();
        assert!(!refresh_command(
            fixture_command(&executable),
            &root,
            Duration::from_secs(1),
            || false
        )
        .unwrap());
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(!root.join("accepted").exists());
        let pid: i32 = fs::read_to_string(root.join("pid"))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        fs::remove_dir_all(root).unwrap();
    }
}

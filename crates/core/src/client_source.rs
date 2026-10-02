//! Client provenance is independent of the provider and of managed control.
//! Only bounded identity metadata is retained; no argv, environment dump or
//! executable path is included in the event sent to Runtime.
use crate::{Provider, TermContext};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Source {
    app: String,
    bundle: Option<String>,
    surface: String,
}

type KnownClient = (
    &'static str,
    &'static str,
    &'static str,
    &'static [&'static str],
);
const CLIENTS: &[KnownClient] = &[
    ("codex_app", "Codex app", "com.openai.codex", &["codex"]),
    ("codex_app", "ChatGPT app", "com.openai.chat", &["chatgpt"]),
    (
        "claude_app",
        "Claude app",
        "com.anthropic.claudefordesktop",
        &["claude"],
    ),
    (
        "kimi_app",
        "Kimi Code app",
        "com.kimi.code.desktop",
        &["kimi code"],
    ),
    ("grok_app", "Grok app", "", &["grok build", "grok"]),
    (
        "editor",
        "Visual Studio Code",
        "com.microsoft.VSCode",
        &["vscode", "code", "visual studio code"],
    ),
    (
        "editor",
        "VS Code Insiders",
        "com.microsoft.VSCodeInsiders",
        &[
            "code-insiders",
            "code - insiders",
            "visual studio code - insiders",
        ],
    ),
    ("editor", "Cursor", "", &["cursor"]),
    ("editor", "Windsurf", "", &["windsurf"]),
    ("editor", "Zed", "", &["zed"]),
    ("editor", "Xcode", "com.apple.dt.Xcode", &["xcode"]),
    (
        "terminal",
        "Terminal",
        "com.apple.Terminal",
        &["apple_terminal", "terminal"],
    ),
    (
        "terminal",
        "iTerm2",
        "com.googlecode.iterm2",
        &["iterm", "iterm2", "iterm.app"],
    ),
    (
        "terminal",
        "Warp",
        "dev.warp.Warp-Stable",
        &["warp", "warpterminal"],
    ),
    ("terminal", "Ghostty", "", &["ghostty"]),
    ("terminal", "WezTerm", "", &["wezterm", "wezterm-gui"]),
    ("terminal", "Alacritty", "", &["alacritty"]),
    ("terminal", "kitty", "", &["kitty"]),
    ("terminal", "Windows Terminal", "", &["windows terminal"]),
    ("terminal", "tmux", "", &["tmux"]),
    ("terminal", "GNU screen", "", &["screen", "gnu screen"]),
];

fn name(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()
        && value.chars().count() <= 80
        && !value.chars().any(char::is_control)
        && !value.contains(['/', '\\']))
    .then(|| value.to_owned())
}

pub fn valid_client_bundle_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.contains('.')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

fn identify(app: Option<&str>, bundle: Option<&str>) -> Option<Source> {
    let app = app.and_then(name);
    let bundle = bundle.filter(|s| valid_client_bundle_id(s));
    let normalized = app
        .as_deref()
        .unwrap_or_default()
        .trim_end_matches(".app")
        .to_ascii_lowercase();
    let known = bundle
        .and_then(|bundle| {
            CLIENTS
                .iter()
                .find(|(_, _, id, _)| !id.is_empty() && id.eq_ignore_ascii_case(bundle))
        })
        .or_else(|| {
            CLIENTS.iter().find(|(_, label, _, aliases)| {
                label.eq_ignore_ascii_case(&normalized) || aliases.iter().any(|a| *a == normalized)
            })
        });
    if let Some((surface, label, id, _)) = known {
        return Some(Source {
            app: (*label).to_owned(),
            bundle: bundle
                .map(str::to_owned)
                .or_else(|| (!id.is_empty()).then(|| (*id).to_owned())),
            surface: (*surface).to_owned(),
        });
    }
    if bundle.is_some_and(|b| b.starts_with("com.jetbrains."))
        || [
            "intellij idea",
            "pycharm",
            "webstorm",
            "goland",
            "rustrover",
            "android studio",
            "fleet",
        ]
        .iter()
        .any(|prefix| normalized.starts_with(prefix))
    {
        return Some(Source {
            app: app.unwrap_or_else(|| "JetBrains IDE".to_owned()),
            bundle: bundle.map(str::to_owned),
            surface: "editor".to_owned(),
        });
    }
    None
}

pub fn client_environment(
    surface: Option<&str>,
    app: Option<&str>,
    bundle: Option<&str>,
) -> Option<String> {
    if matches!(surface, Some("connector" | "remote")) {
        return app.and_then(name).or_else(|| {
            Some(
                if surface == Some("remote") {
                    "Remote session"
                } else {
                    "Agent connector"
                }
                .to_owned(),
            )
        });
    }
    if let Some(source) = identify(app, bundle) {
        return Some(source.app);
    }
    match surface {
        Some("codex_app") => Some("Codex app".to_owned()),
        Some("claude_app") => Some("Claude app".to_owned()),
        Some("kimi_app") => Some("Kimi Code app".to_owned()),
        Some("grok_app") => Some("Grok app".to_owned()),
        Some("terminal") => Some(app.and_then(name).unwrap_or_else(|| "Terminal".to_owned())),
        Some("editor" | "desktop_app") => app.and_then(name),
        _ => None,
    }
}

#[derive(Clone)]
struct Process {
    pid: u32,
    parent: u32,
    path: PathBuf,
}

fn provider_executable(provider: Provider, path: &Path) -> bool {
    let Some(file) = path.file_name().and_then(|p| p.to_str()) else {
        return false;
    };
    let file = file.trim_end_matches(".exe").to_ascii_lowercase();
    match provider {
        Provider::Codex => file == "codex",
        Provider::Claude => matches!(file.as_str(), "claude" | "claude-code"),
        Provider::Kimi => matches!(file.as_str(), "kimi" | "kimi-code"),
        Provider::Grok => matches!(file.as_str(), "grok" | "grok-build"),
        Provider::Gemini => file == "gemini",
    }
}

fn host_bundle(path: &Path) -> Option<PathBuf> {
    // A resource binary inside an app bundle can be launched from any terminal.
    // Require an actual app/helper executable, then use its outer host bundle.
    let text = path.to_str()?;
    if !text.contains(".app/Contents/MacOS/") {
        return None;
    }
    let end = text.find(".app/")? + 4;
    Some(PathBuf::from(&text[..end]))
}

fn process_source(
    processes: &[Process],
    inspect: impl Fn(&Path) -> Option<Source>,
) -> Option<Source> {
    processes
        .iter()
        .filter_map(|p| host_bundle(&p.path))
        .find_map(|bundle| inspect(&bundle))
}

fn provider_process(provider: Provider, processes: &[Process]) -> Option<u32> {
    processes
        .iter()
        .find(|p| provider_executable(provider, &p.path))
        .map(|p| p.pid)
        // Node/Python providers need not have a provider-named executable.
        // Preserve the verified direct Hook parent as the compatibility fallback.
        .or_else(|| processes.first().map(|p| p.pid))
}

pub fn capture_client_context(provider: Provider, start_pid: u32) -> TermContext {
    let mut processes = Vec::new();
    let mut next = start_pid;
    for _ in 0..24 {
        if next <= 1 || processes.iter().any(|p: &Process| p.pid == next) {
            break;
        }
        let Some(process) = process_info(next) else {
            break;
        };
        next = process.parent;
        processes.push(process);
    }
    let provider_pid = provider_process(provider, &processes);
    let app = std::env::var("TERM_PROGRAM")
        .ok()
        .or_else(|| std::env::var("LC_TERMINAL").ok());
    let bundle = std::env::var("__CFBundleIdentifier")
        .ok()
        .filter(|b| valid_client_bundle_id(b));
    let source = process_source(&processes, inspect_bundle).or_else(|| {
        if processes
            .iter()
            .any(|p| p.path.file_name().is_some_and(|n| n == "sshd"))
            || std::env::var_os("SSH_CONNECTION").is_some()
            || std::env::var_os("SSH_CLIENT").is_some()
        {
            return Some(Source {
                app: "SSH".to_owned(),
                bundle: None,
                surface: "remote".to_owned(),
            });
        }
        let mux = if std::env::var_os("TMUX").is_some() {
            Some("tmux")
        } else if std::env::var_os("STY").is_some() {
            Some("screen")
        } else {
            None
        };
        if let Some(mux) = mux {
            return identify(Some(mux), None);
        }
        if let Some(source) = identify(app.as_deref(), bundle.as_deref()) {
            return Some(source);
        }
        if let Some(surface) = std::env::var("ACTREALM_SURFACE").ok().filter(|s| {
            matches!(
                s.as_str(),
                "terminal"
                    | "editor"
                    | "codex_app"
                    | "claude_app"
                    | "kimi_app"
                    | "grok_app"
                    | "desktop_app"
                    | "connector"
                    | "remote"
            )
        }) {
            return Some(Source {
                app: app.as_deref().and_then(name).unwrap_or_default(),
                bundle: bundle.clone(),
                surface,
            });
        }
        app.as_deref().and_then(name).map(|app| Source {
            app,
            bundle,
            surface: "terminal".to_owned(),
        })
    });
    let mut context = TermContext {
        app: None,
        bundle_id: None,
        surface: None,
        provider_pid,
        session_id: std::env::var("TERM_SESSION_ID").ok(),
        tty: std::env::var("TTY").ok(),
        title: std::env::var("ACTREALM_TERM_TITLE").ok(),
    };
    if let Some(source) = source {
        context.app = name(&source.app);
        context.bundle_id = source.bundle;
        context.surface = Some(source.surface);
    }
    context
}

#[cfg(target_os = "macos")]
fn process_info(pid: u32) -> Option<Process> {
    let pid = i32::try_from(pid).ok()?;
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    let got = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size as i32,
        )
    };
    if got != size as i32 {
        return None;
    }
    let info = unsafe { info.assume_init() };
    if info.pbi_uid != unsafe { libc::geteuid() } {
        return None;
    }
    let mut bytes = [0_u8; 4096];
    let got = unsafe { libc::proc_pidpath(pid, bytes.as_mut_ptr().cast(), bytes.len() as u32) };
    if got <= 0 {
        return None;
    }
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    Some(Process {
        pid: pid as u32,
        parent: info.pbi_ppid,
        path: PathBuf::from(std::str::from_utf8(&bytes[..end]).ok()?),
    })
}

#[cfg(target_os = "linux")]
fn process_info(pid: u32) -> Option<Process> {
    use std::os::unix::fs::MetadataExt;
    let root = PathBuf::from(format!("/proc/{pid}"));
    if std::fs::metadata(&root).ok()?.uid() != unsafe { libc::geteuid() } {
        return None;
    }
    let stat = std::fs::read_to_string(root.join("stat")).ok()?;
    let parent = stat
        .rsplit_once(") ")?
        .1
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some(Process {
        pid,
        parent,
        path: std::fs::read_link(root.join("exe")).ok()?,
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn process_info(_pid: u32) -> Option<Process> {
    None
}

#[cfg(target_os = "macos")]
fn inspect_bundle(path: &Path) -> Option<Source> {
    use std::ffi::{c_char, c_void, CString};
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFURLCreateFromFileSystemRepresentation(
            allocator: *const c_void,
            bytes: *const u8,
            len: isize,
            directory: u8,
        ) -> *const c_void;
        fn CFBundleCopyInfoDictionaryForURL(url: *const c_void) -> *const c_void;
        fn CFStringCreateWithCString(
            allocator: *const c_void,
            value: *const c_char,
            encoding: u32,
        ) -> *const c_void;
        fn CFDictionaryGetValue(dictionary: *const c_void, key: *const c_void) -> *const c_void;
        fn CFGetTypeID(value: *const c_void) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFStringGetCString(
            value: *const c_void,
            bytes: *mut c_char,
            size: isize,
            encoding: u32,
        ) -> u8;
        fn CFRelease(value: *const c_void);
    }
    let bytes = path.as_os_str().as_encoded_bytes();
    let utf8 = 0x0800_0100;
    unsafe {
        let url = CFURLCreateFromFileSystemRepresentation(
            std::ptr::null(),
            bytes.as_ptr(),
            bytes.len() as isize,
            1,
        );
        if url.is_null() {
            return None;
        }
        let dictionary = CFBundleCopyInfoDictionaryForURL(url);
        CFRelease(url);
        if dictionary.is_null() {
            return None;
        }
        let read = |key: &str| -> Option<String> {
            let key = CString::new(key).ok()?;
            let key = CFStringCreateWithCString(std::ptr::null(), key.as_ptr(), utf8);
            if key.is_null() {
                return None;
            }
            let value = CFDictionaryGetValue(dictionary, key);
            CFRelease(key);
            if value.is_null() || CFGetTypeID(value) != CFStringGetTypeID() {
                return None;
            }
            let mut bytes = [0_u8; 512];
            if CFStringGetCString(value, bytes.as_mut_ptr().cast(), bytes.len() as isize, utf8) == 0
            {
                return None;
            }
            let end = bytes.iter().position(|b| *b == 0)?;
            std::str::from_utf8(&bytes[..end]).ok().map(str::to_owned)
        };
        let bundle = read("CFBundleIdentifier").filter(|b| valid_client_bundle_id(b));
        let app = read("CFBundleDisplayName")
            .or_else(|| read("CFBundleName"))
            .and_then(|s| name(&s))
            .or_else(|| path.file_stem().and_then(|s| s.to_str()).and_then(name));
        CFRelease(dictionary);
        identify(app.as_deref(), bundle.as_deref()).or_else(|| {
            app.map(|app| Source {
                app,
                bundle,
                surface: "desktop_app".to_owned(),
            })
        })
    }
}

#[cfg(not(target_os = "macos"))]
fn inspect_bundle(_path: &Path) -> Option<Source> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_desktop_editor_and_terminal_sources_keep_distinct_names() {
        for (surface, label, bundle, aliases) in CLIENTS {
            for alias in *aliases {
                let source = identify(Some(alias), None).unwrap();
                assert_eq!(source.app, *label);
                assert_eq!(source.surface, *surface);
            }
            if !bundle.is_empty() {
                assert_eq!(
                    client_environment(None, None, Some(bundle)).as_deref(),
                    Some(*label)
                );
            }
        }
        assert_eq!(
            identify(Some("PyCharm Professional"), Some("com.jetbrains.pycharm"))
                .unwrap()
                .surface,
            "editor"
        );
        assert_eq!(client_environment(None, None, None), None);
        assert_eq!(client_environment(None, Some("/private/path"), None), None);
        assert_eq!(
            client_environment(Some("connector"), Some("ActRealm ACP"), None).as_deref(),
            Some("ActRealm ACP")
        );
        assert_eq!(
            client_environment(Some("remote"), Some("SSH"), None).as_deref(),
            Some("SSH")
        );
    }

    #[test]
    fn bundled_cli_does_not_claim_the_desktop_app_that_ships_it() {
        let processes = vec![
            Process {
                pid: 3,
                parent: 2,
                path: "/Applications/ChatGPT.app/Contents/Resources/codex".into(),
            },
            Process {
                pid: 2,
                parent: 1,
                path: "/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal".into(),
            },
        ];
        let source = process_source(&processes, |bundle| {
            identify(bundle.file_stem()?.to_str(), None)
        })
        .unwrap();
        assert_eq!(source.app, "Terminal");
        assert!(provider_executable(Provider::Codex, &processes[0].path));
        let python = vec![Process {
            pid: 9,
            parent: 1,
            path: "/usr/bin/python3".into(),
        }];
        assert_eq!(provider_process(Provider::Kimi, &python), Some(9));
        assert_eq!(provider_process(Provider::Grok, &[]), None);
        assert!(process_source(&processes[..1], |_| panic!(
            "resource binary is not a host app"
        ))
        .is_none());
        assert_eq!(host_bundle(Path::new("/Applications/Visual Studio Code.app/Contents/Frameworks/Code Helper (Plugin).app/Contents/MacOS/Code Helper (Plugin)")), Some(PathBuf::from("/Applications/Visual Studio Code.app")));
    }

    #[test]
    fn bundle_identity_wins_over_packaged_app_name_and_internal_events_have_no_host() {
        let source = identify(Some("ChatGPT"), Some("com.openai.codex")).unwrap();
        assert_eq!(source.app, "Codex app");
        assert_eq!(source.surface, "codex_app");
        for provider in [
            Provider::Codex,
            Provider::Claude,
            Provider::Kimi,
            Provider::Grok,
            Provider::Gemini,
        ] {
            let request = crate::BridgeRequest::from_hook_at(
                provider,
                serde_json::json!({
                    "hook_event_name":"PreToolUse", "session_id":"source-fixture", "tool_name":"Bash"
                }),
                1000,
            );
            assert!(
                request.term.is_none(),
                "internal events must not adopt the Runtime's client"
            );
        }
        for value in [
            "",
            "../../Applications/Other.app",
            "com.example app",
            "com.example\napp",
        ] {
            assert!(!valid_client_bundle_id(value));
        }
    }
}

//! Private, mutually authenticated native-client enrollment. HTTP never trusts
//! a caller-supplied application name, PID, or bundle identifier.
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub const RUNTIME_IDENTIFIER: &str = "com.frontierinterfaces.actrealm.runtime";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeIdentity {
    ActRealm,
    Display,
}

impl NativeIdentity {
    pub(crate) fn identifier(self) -> &'static str {
        match self {
            Self::ActRealm => "com.frontierinterfaces.actrealm",
            Self::Display => "com.mmx.animation-display-demo",
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ConnectRequest {
    pub schema_version: u32,
    #[serde(default)]
    pub previous_token: Option<String>,
    #[serde(default)]
    pub enable_access: bool,
}

pub(crate) struct NativeListener {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    path: PathBuf,
    inode: u64,
}

impl NativeListener {
    pub(crate) fn start(
        path: PathBuf,
        handler: impl Fn(NativeIdentity, ConnectRequest) -> Value + Send + Sync + 'static,
    ) -> io::Result<Self> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("missing socket directory"))?;
        let metadata = fs::symlink_metadata(parent)?;
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(io::Error::other("native socket directory must be private"));
        }
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if !metadata.file_type().is_socket()
                || metadata.uid() != unsafe { libc::geteuid() }
                || UnixStream::connect(&path).is_ok()
            {
                return Err(io::Error::other(
                    "native socket is unsafe or already in use",
                ));
            }
            fs::remove_file(&path)?;
        }
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let inode = fs::symlink_metadata(&path)?.ino();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let active = Arc::new(AtomicUsize::new(0));
        let handler = Arc::new(handler);
        // Unsigned development servers still expose ordinary test HTTP routes,
        // but cannot enroll native clients. There is no environment bypass.
        let expected_team = signing::current_team().ok();
        let thread = thread::Builder::new()
            .name("native-enrollment".into())
            .spawn(move || {
                let mut workers: Vec<JoinHandle<()>> = Vec::new();
                while !thread_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            if active.load(Ordering::Acquire) >= 4 {
                                continue;
                            }
                            let active = active.clone();
                            let handler = handler.clone();
                            let team = expected_team.clone();
                            active.fetch_add(1, Ordering::AcqRel);
                            let worker = thread::spawn(move || {
                                if let Err(error) =
                                    serve_connection(stream, team.as_deref(), &*handler)
                                {
                                    eprintln!("native enrollment transport: {}", error.kind());
                                }
                                active.fetch_sub(1, Ordering::AcqRel);
                            });
                            workers.push(worker);
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(25));
                        }
                        Err(_) => break,
                    }
                    let mut index = 0;
                    while index < workers.len() {
                        if workers[index].is_finished() {
                            let _ = workers.swap_remove(index).join();
                        } else {
                            index += 1;
                        }
                    }
                }
                for worker in workers {
                    let _ = worker.join();
                }
            })?;
        Ok(Self {
            stop,
            thread: Some(thread),
            path,
            inode,
        })
    }
}

impl Drop for NativeListener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if fs::symlink_metadata(&self.path).is_ok_and(|m| m.ino() == self.inode) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn serve_connection(
    mut stream: UnixStream,
    team: Option<&str>,
    handler: &dyn Fn(NativeIdentity, ConnectRequest) -> Value,
) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    // Authenticate before reading client-controlled data or disclosing secrets.
    let identity = team.and_then(|team| signing::peer_identity(&stream, team).ok());
    let response = if let Some(identity) = identity {
        let mut reader = BufReader::new(&stream);
        let mut bytes = Vec::new();
        let mut limited = (&mut reader).take(2_049);
        use std::io::Read;
        limited.read_until(b'\n', &mut bytes)?;
        if bytes.len() > 2_048 || bytes.last() != Some(&b'\n') {
            json!({"error": "invalidRequest"})
        } else {
            match serde_json::from_slice::<ConnectRequest>(&bytes) {
                Ok(request)
                    if request.schema_version == 1
                        && request.previous_token.as_ref().is_none_or(|token| {
                            token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit())
                        }) =>
                {
                    handler(identity, request)
                }
                _ => json!({"error": "invalidRequest"}),
            }
        }
    } else {
        json!({"error": "untrustedClient"})
    };
    serde_json::to_writer(&mut stream, &response)?;
    stream.write_all(b"\n")
}

#[cfg(target_os = "macos")]
pub mod signing {
    use super::*;
    use std::ffi::{c_char, c_void, CString};
    use std::ptr;
    type Ref = *const c_void;
    struct Owned(Ref);
    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) };
            }
        }
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(value: Ref);
        fn CFStringCreateWithCString(allocator: Ref, value: *const c_char, encoding: u32) -> Ref;
        fn CFStringGetCString(value: Ref, buffer: *mut c_char, size: isize, encoding: u32) -> bool;
        fn CFURLCreateWithFileSystemPath(
            allocator: Ref,
            path: Ref,
            style: isize,
            directory: bool,
        ) -> Ref;
        fn CFDataCreate(allocator: Ref, bytes: *const u8, length: isize) -> Ref;
        fn CFDictionaryCreateMutable(
            allocator: Ref,
            capacity: isize,
            keys: Ref,
            values: Ref,
        ) -> Ref;
        fn CFDictionarySetValue(dictionary: Ref, key: Ref, value: Ref);
        fn CFDictionaryGetValue(dictionary: Ref, key: Ref) -> Ref;
        static kCFTypeDictionaryKeyCallBacks: u8;
        static kCFTypeDictionaryValueCallBacks: u8;
    }
    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        fn SecCodeCopySelf(flags: u32, code: *mut Ref) -> i32;
        fn SecCodeCopyStaticCode(code: Ref, flags: u32, static_code: *mut Ref) -> i32;
        fn SecCodeCopyGuestWithAttributes(
            host: Ref,
            attributes: Ref,
            flags: u32,
            code: *mut Ref,
        ) -> i32;
        fn SecCodeCopySigningInformation(code: Ref, flags: u32, info: *mut Ref) -> i32;
        fn SecCodeCheckValidity(code: Ref, flags: u32, requirement: Ref) -> i32;
        fn SecStaticCodeCreateWithPath(path: Ref, flags: u32, code: *mut Ref) -> i32;
        fn SecStaticCodeCheckValidity(code: Ref, flags: u32, requirement: Ref) -> i32;
        fn SecRequirementCreateWithString(text: Ref, flags: u32, requirement: *mut Ref) -> i32;
        static kSecGuestAttributeAudit: Ref;
        static kSecCodeInfoTeamIdentifier: Ref;
    }
    const UTF8: u32 = 0x08000100;
    fn denied() -> io::Error {
        io::Error::new(io::ErrorKind::PermissionDenied, "untrusted native process")
    }
    fn string(value: &str) -> io::Result<Owned> {
        let value = CString::new(value).map_err(|_| denied())?;
        let value = unsafe { CFStringCreateWithCString(ptr::null(), value.as_ptr(), UTF8) };
        if value.is_null() {
            Err(denied())
        } else {
            Ok(Owned(value))
        }
    }
    fn requirement(team: &str, identifier: &str) -> io::Result<Owned> {
        if team.is_empty() || team.len() > 32 || !team.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(denied());
        }
        let text = string(&format!("anchor apple generic and certificate leaf[subject.OU] = \"{team}\" and identifier \"{identifier}\""))?;
        let mut requirement = ptr::null();
        if unsafe { SecRequirementCreateWithString(text.0, 0, &mut requirement) } != 0 {
            return Err(denied());
        }
        Ok(Owned(requirement))
    }
    fn valid(code: Ref, team: &str, identifier: &str) -> io::Result<()> {
        let requirement = requirement(team, identifier)?;
        if unsafe { SecCodeCheckValidity(code, 1 << 4, requirement.0) } != 0 {
            return Err(denied());
        }
        Ok(())
    }
    pub fn validate_binary(path: &std::path::Path, team: &str, identifier: &str) -> io::Result<()> {
        let path = string(path.to_str().ok_or_else(denied)?)?;
        let url = Owned(unsafe { CFURLCreateWithFileSystemPath(ptr::null(), path.0, 0, false) });
        if url.0.is_null() {
            return Err(denied());
        }
        let mut code = ptr::null();
        if unsafe { SecStaticCodeCreateWithPath(url.0, 0, &mut code) } != 0 {
            return Err(denied());
        }
        let code = Owned(code);
        let requirement = requirement(team, identifier)?;
        if unsafe { SecStaticCodeCheckValidity(code.0, 1 << 4, requirement.0) } != 0 {
            return Err(denied());
        }
        Ok(())
    }
    pub fn current_team() -> io::Result<String> {
        let mut code = ptr::null();
        if unsafe { SecCodeCopySelf(0, &mut code) } != 0 {
            return Err(denied());
        }
        let code = Owned(code);
        let mut static_code = ptr::null();
        if unsafe { SecCodeCopyStaticCode(code.0, 0, &mut static_code) } != 0 {
            return Err(denied());
        }
        let static_code = Owned(static_code);
        let mut information = ptr::null();
        if unsafe { SecCodeCopySigningInformation(static_code.0, 1 << 1, &mut information) } != 0 {
            return Err(denied());
        }
        let information = Owned(information);
        let team = unsafe { CFDictionaryGetValue(information.0, kSecCodeInfoTeamIdentifier) };
        if team.is_null() {
            return Err(denied());
        }
        let mut buffer = [0_i8; 128];
        if !unsafe { CFStringGetCString(team, buffer.as_mut_ptr(), buffer.len() as isize, UTF8) } {
            return Err(denied());
        }
        let team = unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) }
            .to_str()
            .map_err(|_| denied())?
            .to_owned();
        valid(code.0, &team, RUNTIME_IDENTIFIER)?;
        Ok(team)
    }
    pub(super) fn peer_identity(stream: &UnixStream, team: &str) -> io::Result<NativeIdentity> {
        let (mut uid, mut gid) = (0, 0);
        if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0
            || uid != unsafe { libc::geteuid() }
        {
            return Err(denied());
        }
        let mut audit = [0_u32; 8];
        let mut length = std::mem::size_of_val(&audit) as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_LOCAL,
                libc::LOCAL_PEERTOKEN,
                audit.as_mut_ptr().cast(),
                &mut length,
            )
        } != 0
            || length as usize != std::mem::size_of_val(&audit)
        {
            return Err(denied());
        }
        let data =
            Owned(unsafe { CFDataCreate(ptr::null(), audit.as_ptr().cast(), length as isize) });
        if data.0.is_null() {
            return Err(denied());
        }
        let attributes = Owned(unsafe {
            CFDictionaryCreateMutable(
                ptr::null(),
                1,
                (&raw const kCFTypeDictionaryKeyCallBacks).cast(),
                (&raw const kCFTypeDictionaryValueCallBacks).cast(),
            )
        });
        if attributes.0.is_null() {
            return Err(denied());
        }
        unsafe { CFDictionarySetValue(attributes.0, kSecGuestAttributeAudit, data.0) };
        let mut code = ptr::null();
        if unsafe { SecCodeCopyGuestWithAttributes(ptr::null(), attributes.0, 0, &mut code) } != 0 {
            return Err(denied());
        }
        let code = Owned(code);
        [NativeIdentity::ActRealm, NativeIdentity::Display]
            .into_iter()
            .find(|identity| valid(code.0, team, identity.identifier()).is_ok())
            .ok_or_else(denied)
    }
}

#[cfg(not(target_os = "macos"))]
pub mod signing {
    use super::*;
    pub fn current_team() -> io::Result<String> {
        Err(io::Error::other(
            "native enrollment requires macOS signing identity",
        ))
    }
    pub fn validate_binary(_: &std::path::Path, _: &str, _: &str) -> io::Result<()> {
        Err(io::Error::other("native signing requires macOS"))
    }
    pub(super) fn peer_identity(_: &UnixStream, _: &str) -> io::Result<NativeIdentity> {
        Err(io::Error::other(
            "native enrollment requires macOS signing identity",
        ))
    }
}

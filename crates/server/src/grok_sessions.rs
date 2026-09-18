//! Subscribe to the official local Grok leader without creating a session or
//! replacing its terminal driver. Only resident sessions and live reverse RPCs
//! grant controls. History and dormant on-disk sessions never do.
use actrealm_core::{Provider, ReplyAction};
use actrealm_providers::acp::{normalize_frame, Observer, ReplyRoute};
use actrealm_runtime::{RuntimeStore, WaiterRegistry, WaiterTicket};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_FRAME: usize = 2 * 1024 * 1024;
const MAX_SESSIONS: usize = 64;
const MAX_PENDING: usize = 32;

pub fn start(
    home: PathBuf,
    runtime_home: PathBuf,
    store: RuntimeStore,
    waiters: WaiterRegistry,
    shutdown: Arc<AtomicBool>,
) -> io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("actrealm-grok-sessions".into())
        .spawn(move || {
            while !shutdown.load(Ordering::Acquire) {
                if enabled(&runtime_home) {
                    let _ = connect(
                        &home.join(".grok/leader.sock"),
                        &runtime_home,
                        store.clone(),
                        waiters.clone(),
                        &shutdown,
                    );
                }
                for _ in 0..10 {
                    if shutdown.load(Ordering::Acquire) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
        })
}

fn enabled(home: &Path) -> bool {
    fs::read(home.join("providers/grok-hooks.json"))
        .ok()
        .filter(|v| v.len() < 8192)
        .and_then(|v| serde_json::from_slice::<Value>(&v).ok())
        .is_some_and(|v| v["installed"] == true)
}

fn checked_socket(path: &Path) -> io::Result<UnixStream> {
    for parent in path.ancestors() {
        let m = fs::symlink_metadata(parent)?;
        if m.file_type().is_symlink() {
            return Err(io::Error::other("Symlinked Grok endpoint"));
        }
    }
    let m = fs::symlink_metadata(path)?;
    if !m.file_type().is_socket() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o022 != 0
    {
        return Err(io::Error::other("Untrusted Grok endpoint"));
    }
    let socket = UnixStream::connect(path)?;
    socket.set_read_timeout(Some(Duration::from_millis(50)))?;
    socket.set_write_timeout(Some(Duration::from_secs(1)))?;
    Ok(socket)
}

fn send(socket: &mut UnixStream, message: &Value) -> io::Result<()> {
    let bytes = serde_json::to_vec(message)?;
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::other("Grok message too large"));
    }
    socket.write_all(&(bytes.len() as u32).to_be_bytes())?;
    socket.write_all(&bytes)
}

fn rpc(socket: &mut UnixStream, frame: Value) -> io::Result<()> {
    send(socket, &json!({"type":"acp","payload":frame.to_string()}))
}

// Retain partial headers/payloads across read timeouts. Never parse or log a
// truncated frame; bound allocation before accepting an advertised length.
#[derive(Default)]
struct Frames {
    bytes: Vec<u8>,
}
impl Frames {
    fn next(&mut self) -> io::Result<Option<Value>> {
        if self.bytes.len() < 4 {
            return Ok(None);
        }
        let len = u32::from_be_bytes(self.bytes[..4].try_into().unwrap()) as usize;
        if len > MAX_FRAME {
            return Err(io::Error::other("Grok frame too large"));
        }
        if self.bytes.len() < len + 4 {
            return Ok(None);
        }
        let frame = serde_json::from_slice(&self.bytes[4..len + 4])?;
        self.bytes.drain(..len + 4);
        Ok(Some(frame))
    }
}

struct Pending {
    session: String,
    tool: Option<String>,
    rpc_id: Value,
    route: ReplyRoute,
    ticket: WaiterTicket,
    expires: u64,
}

struct Live {
    store: RuntimeStore,
    waiters: WaiterRegistry,
    observers: HashMap<String, Observer>,
    attaching: HashMap<String, String>,
    pending: HashMap<String, Pending>,
    // An answered or handed-back RPC must not reappear while it remains cached
    // by the leader. Cleared on a new transport generation, never persisted.
    seen: HashSet<String>,
    next_id: u64,
    initialized: bool,
    roster_request: Option<(String, Instant)>,
}

impl Drop for Live {
    fn drop(&mut self) {
        for (_, pending) in self.pending.drain() {
            let id = pending.ticket.request_id();
            let _ = self.waiters.pass_through(id, "connector_disconnected");
            let _ = self
                .store
                .expire_approval(id, "connector_disconnected", now());
        }
        // The original terminal still owns execution. Losing this subscriber
        // does not cancel, complete, or change the session's execution state.
    }
}

impl Live {
    fn new(store: RuntimeStore, waiters: WaiterRegistry) -> Self {
        Self {
            store,
            waiters,
            observers: HashMap::new(),
            attaching: HashMap::new(),
            pending: HashMap::new(),
            seen: HashSet::new(),
            next_id: 0,
            initialized: false,
            roster_request: None,
        }
    }

    fn request(
        &mut self,
        socket: &mut UnixStream,
        method: &str,
        params: Value,
    ) -> io::Result<String> {
        self.next_id += 1;
        let id = format!("actrealm-{}", self.next_id);
        rpc(
            socket,
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
        )?;
        Ok(id)
    }

    fn roster(&mut self, socket: &mut UnixStream, value: &Value) -> io::Result<()> {
        let Some(rows) = value
            .pointer("/result/result/sessions")
            .and_then(Value::as_array)
        else {
            return Ok(());
        };
        let mut resident = HashSet::new();
        for row in rows.iter().take(1024).filter(|r| r["resident"] == true) {
            let (Some(id), Some(cwd)) =
                (bounded(&row["sessionId"], 256), bounded(&row["cwd"], 4096))
            else {
                continue;
            };
            if !Path::new(cwd).is_absolute() {
                continue;
            }
            resident.insert(id.to_owned());
            if self.observers.contains_key(id) || self.observers.len() >= MAX_SESSIONS {
                continue;
            }
            let mut observer = Observer::new(Provider::Grok, id.into(), cwd.into());
            observer.model = bounded(&row["modelId"], 256).map(str::to_owned);
            self.observers.insert(id.into(), observer);
            // Load only a session confirmed resident in THIS leader. Loading a
            // dormant transcript could fork a second execution of a CLI session.
            let rpc_id = self.request(
                socket,
                "session/load",
                json!({"sessionId":id,"cwd":cwd,"mcpServers":[]}),
            )?;
            self.attaching.insert(rpc_id, id.into());
        }
        let removed = self
            .observers
            .keys()
            .filter(|id| !resident.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        for id in removed {
            self.observers.remove(&id);
            self.expire_matching(&id, None, "provider_handled");
        }
        Ok(())
    }

    fn expire_matching(&mut self, session: &str, tool: Option<&str>, reason: &str) {
        let keys = self
            .pending
            .iter()
            .filter(|(_, p)| {
                p.session == session && tool.is_none_or(|t| p.tool.as_deref() == Some(t))
            })
            .map(|(k, _)| k.clone())
            .collect::<Vec<_>>();
        for key in keys {
            if let Some(p) = self.pending.remove(&key) {
                let id = p.ticket.request_id();
                let _ = self.waiters.pass_through(id, reason);
                let _ = self.store.expire_approval(id, reason, now());
            }
        }
    }

    fn frame(&mut self, socket: &mut UnixStream, frame: Value) -> io::Result<()> {
        if let Some(id) = frame["id"].as_str() {
            if self
                .roster_request
                .as_ref()
                .is_some_and(|(expected, _)| expected == id)
            {
                self.roster_request = None;
                if frame.get("error").is_some() {
                    return Err(io::Error::other("Grok roster unavailable"));
                }
            }
            if let Some(session) = self.attaching.remove(id) {
                if frame.get("error").is_some() {
                    self.observers.remove(&session);
                }
                return Ok(());
            }
        }
        if frame.pointer("/result/protocolVersion").is_some() {
            if frame.pointer("/result/protocolVersion") != Some(&json!(1)) {
                return Err(io::Error::other("Unsupported Grok ACP version"));
            }
            self.initialized = true;
        }
        if frame.pointer("/result/result/sessions").is_some() {
            self.roster(socket, &frame)?;
        }
        let frame = normalize_frame(&frame);
        let Some(session) = bounded(&frame["params"]["sessionId"], 256) else {
            return Ok(());
        };
        if self.attaching.values().any(|s| s == session) {
            return Ok(());
        }
        if frame["method"] == "_x.ai/session_notification"
            && frame["params"]["update"]["sessionUpdate"] == "interaction_resolved"
        {
            if let Some(tool) = bounded(&frame["params"]["update"]["toolCallId"], 256)
                .or_else(|| bounded(&frame["params"]["update"]["tool_call_id"], 256))
            {
                self.expire_matching(session, Some(tool), "provider_handled");
            }
            return Ok(());
        }
        let Some(observer) = self.observers.get_mut(session) else {
            return Ok(());
        };
        if frame.get("id").is_none() {
            for event in observer.notification(&frame, now()) {
                if event.event_name() == Some("PlanUpdated") {
                    let _ = self.store.ingest(event);
                }
            }
            return Ok(());
        }
        let key = format!("{session}:{}", frame["id"]);
        if self.seen.len() >= 4096 {
            return Err(io::Error::other("Renew Grok interaction generation"));
        }
        if self.pending.len() >= MAX_PENDING || self.seen.contains(&key) {
            return Ok(());
        }
        let Some(interaction) = observer.interaction(&frame, now()) else {
            return Ok(());
        };
        let mut request = interaction.request;
        request.raw["source_version"] = json!("grok-leader/1");
        let registration = self
            .waiters
            .register_at(&request, now())
            .map_err(io::Error::other)?;
        if let Some(id) = registration.replaced_request_id {
            let _ = self.store.expire_approval(id, "duplicate_replaced", now());
        }
        match self.store.ingest(request.clone()) {
            Ok(result) if !result.suppressed => {}
            _ => {
                let _ = self
                    .waiters
                    .pass_through(registration.ticket.request_id(), "runtime_error");
                return Ok(());
            }
        }
        self.seen.insert(key.clone());
        self.pending.insert(
            key,
            Pending {
                session: session.into(),
                tool: bounded(&frame["params"]["toolCallId"], 256)
                    .or_else(|| bounded(&frame["params"]["toolCall"]["toolCallId"], 256))
                    .map(str::to_owned),
                rpc_id: interaction.rpc_id,
                route: interaction.route,
                ticket: registration.ticket,
                expires: request.deadline_at.unwrap_or(now()),
            },
        );
        Ok(())
    }

    fn replies(&mut self, socket: &mut UnixStream) -> io::Result<()> {
        let mut done = Vec::new();
        for (key, p) in &self.pending {
            if let Ok(response) = p.ticket.recv_timeout(Duration::ZERO) {
                // Pass-through / Runtime loss returns control to the already
                // visible terminal modal; it must never cancel that modal.
                if matches!(
                    response.action,
                    ReplyAction::Allow | ReplyAction::Deny | ReplyAction::Answer
                ) {
                    rpc(
                        socket,
                        json!({"jsonrpc":"2.0","id":p.rpc_id,"result":p.route.response(Some(&response))}),
                    )?;
                }
                done.push(key.clone());
            } else if now() >= p.expires {
                let id = p.ticket.request_id();
                let _ = self.waiters.pass_through(id, "deadline");
                let _ = self.store.expire_approval(id, "deadline", now());
                done.push(key.clone());
            }
        }
        for key in done {
            self.pending.remove(&key);
        }
        Ok(())
    }
}

fn connect(
    path: &Path,
    runtime_home: &Path,
    store: RuntimeStore,
    waiters: WaiterRegistry,
    shutdown: &AtomicBool,
) -> io::Result<()> {
    let mut socket = checked_socket(path)?;
    send(
        &mut socket,
        &json!({"type":"register","client_type":"actrealm","mode":"stdio","capabilities":{}}),
    )?;
    let mut live = Live::new(store, waiters);
    let mut frames = Frames::default();
    let mut initialized = false;
    let mut last_poll = Instant::now();
    let connected = Instant::now();
    while !shutdown.load(Ordering::Acquire) {
        let mut bytes = [0u8; 65536];
        match socket.read(&mut bytes) {
            Ok(0) => return Ok(()),
            Ok(n) => frames.bytes.extend_from_slice(&bytes[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(e) => return Err(e),
        }
        while let Some(message) = frames.next()? {
            match message["type"].as_str() {
                Some("registered") => {
                    if message["leader_protocol_version"] != 1 {
                        return Err(io::Error::other("Unsupported Grok leader version"));
                    }
                    if message["ready"].as_bool().unwrap_or(true) {
                        initialized = true;
                    }
                }
                Some("leader_ready") => initialized = true,
                Some("acp") => {
                    let frame: Value = serde_json::from_str(
                        message["payload"]
                            .as_str()
                            .ok_or_else(|| io::Error::other("Invalid Grok payload"))?,
                    )?;
                    live.frame(&mut socket, frame)?;
                }
                Some("shutdown" | "shutting_down" | "error") => return Ok(()),
                _ => {}
            }
            if initialized && live.next_id == 0 {
                live.request(&mut socket,"initialize",json!({"protocolVersion":1,"clientInfo":{"name":"ActRealm","version":"0.1.0"},"clientCapabilities":{}}))?;
            }
        }
        if !live.initialized && connected.elapsed() > Duration::from_secs(30) {
            return Err(io::Error::other("Grok initialization timed out"));
        }
        if last_poll.elapsed() >= Duration::from_secs(1) {
            if !enabled(runtime_home) {
                return Ok(());
            }
            if live
                .roster_request
                .as_ref()
                .is_some_and(|(_, started)| started.elapsed() > Duration::from_secs(15))
            {
                return Err(io::Error::other("Grok roster timed out"));
            }
            if live.initialized && live.roster_request.is_none() {
                let id = live.request(&mut socket, "_x.ai/sessions/list", json!({}))?;
                live.roster_request = Some((id, Instant::now()));
            }
            last_poll = Instant::now();
        }
        // Process provider resolutions from this read BEFORE forwarding a local
        // reply, so an already answered terminal modal cannot be replayed.
        live.replies(&mut socket)?;
    }
    Ok(())
}

fn bounded(value: &Value, max: usize) -> Option<&str> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control))
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use actrealm_core::{BridgeRequest, Decision};

    fn fixture() -> (Live, UnixStream, UnixStream, PathBuf) {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("ar-grok-shared-{}", uuid::Uuid::now_v7()));
        let store = RuntimeStore::open(root.join("test.sqlite")).unwrap();
        let mut live = Live::new(store, WaiterRegistry::default());
        live.observers.insert(
            "s".into(),
            Observer::new(Provider::Grok, "s".into(), "/tmp".into()),
        );
        let (a, b) = UnixStream::pair().unwrap();
        b.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        (live, a, b, root)
    }
    fn question(id: &str, tool: &str) -> Value {
        json!({"jsonrpc":"2.0","id":id,"method":"_x.ai/ask_user_question","params":{
            "method":"x.ai/ask_user_question","params":{"sessionId":"s","tool_call_id":tool,
            "questions":[{"question":"Color?","options":[{"label":"Blue"},{"label":"Green"}],"multi_select":false}]}}})
    }
    fn read_rpc(socket: &mut UnixStream) -> Value {
        let mut header = [0u8; 4];
        socket.read_exact(&mut header).unwrap();
        let mut bytes = vec![0; u32::from_be_bytes(header) as usize];
        socket.read_exact(&mut bytes).unwrap();
        let message: Value = serde_json::from_slice(&bytes).unwrap();
        serde_json::from_str(message["payload"].as_str().unwrap()).unwrap()
    }
    #[test]
    fn shared_question_answers_and_original_terminal_resolution_are_request_scoped() {
        let (mut live, mut socket, mut peer, root) = fixture();
        live.frame(&mut socket, question("rpc1", "tool1")).unwrap();
        live.frame(&mut socket, question("rpc1", "tool1")).unwrap();
        assert_eq!(live.pending.len(), 1);
        let id = live.pending.values().next().unwrap().ticket.request_id();
        assert_eq!(
            live.waiters
                .interactive_prompt(id)
                .unwrap()
                .unwrap()
                .questions[0]
                .prompt,
            "Color?"
        );
        live.waiters
            .answer(id, &json!({"answers":{"q0":["Blue"]}}))
            .unwrap();
        live.replies(&mut socket).unwrap();
        let sent = read_rpc(&mut peer);
        assert_eq!(sent["id"], "rpc1");
        assert_eq!(
            sent["result"],
            json!({"outcome":"accepted","answers":{"Color?":["Blue"]}})
        );
        live.frame(&mut socket, question("rpc2", "tool2")).unwrap();
        live.frame(&mut socket, question("rpc3", "tool3")).unwrap();
        live.frame(&mut socket,json!({"method":"_x.ai/session_notification","params":{"method":"x.ai/session_notification","params":{"sessionId":"s","update":{"sessionUpdate":"interaction_resolved","tool_call_id":"tool2"}}}})).unwrap();
        assert_eq!(live.pending.len(), 1);
        assert_eq!(
            live.pending.values().next().unwrap().tool.as_deref(),
            Some("tool3")
        );
        live.replies(&mut socket).unwrap();
        let mut byte = [0];
        assert!(
            peer.read(&mut byte).is_err(),
            "original answer must not trigger another reply"
        );
        let store = live.store.clone();
        let registry = live.waiters.clone();
        drop(live);
        assert!(registry.active_request_ids().unwrap().is_empty());
        assert!(!store
            .snapshot()
            .unwrap()
            .attention
            .iter()
            .any(|a| a.kind == "completion"));
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn dormant_sessions_and_history_never_acquire_a_reply_channel() {
        let (mut live, mut socket, mut peer, root) = fixture();
        live.observers.clear();
        live.roster(
            &mut socket,
            &json!({"result":{"result":{"sessions":[
            {"sessionId":"old","cwd":"/tmp","resident":false},
            {"sessionId":"s","cwd":"/tmp","resident":true}]}}}),
        )
        .unwrap();
        let sent = read_rpc(&mut peer);
        assert_eq!(sent["method"], "session/load");
        assert_eq!(sent["params"]["sessionId"], "s");
        live.frame(&mut socket, question("history", "t")).unwrap();
        assert!(live.pending.is_empty());
        live.frame(&mut socket, json!({"id":sent["id"],"result":{}}))
            .unwrap();
        live.frame(&mut socket, question("current", "t")).unwrap();
        assert_eq!(live.pending.len(), 1);
        drop(live);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn approval_deny_and_pass_through_keep_the_original_provider_options() {
        let (mut live, mut socket, mut peer, root) = fixture();
        let frame = json!({"id":44,"method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"edit1","kind":"edit","rawInput":{"path":"file.txt"}},"options":[{"optionId":"one","kind":"allow_once","name":"Once"},{"optionId":"no","kind":"reject_once","name":"No"}]}});
        live.frame(&mut socket, frame.clone()).unwrap();
        let id = live.pending.values().next().unwrap().ticket.request_id();
        live.waiters.decide(id, Decision::Deny).unwrap();
        live.replies(&mut socket).unwrap();
        assert_eq!(
            read_rpc(&mut peer)["result"],
            json!({"outcome":{"outcome":"selected","optionId":"no"}})
        );
        let mut frame = frame;
        frame["id"] = json!(45);
        live.frame(&mut socket, frame).unwrap();
        let id = live.pending.values().next().unwrap().ticket.request_id();
        live.waiters.pass_through(id, "native_provider_ui").unwrap();
        live.replies(&mut socket).unwrap();
        let mut byte = [0];
        assert!(peer.read(&mut byte).is_err());
        drop(live);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn fragmented_frames_are_retained_and_oversized_frames_rejected() {
        let payload = serde_json::to_vec(&json!({"type":"pong"})).unwrap();
        let mut all = (payload.len() as u32).to_be_bytes().to_vec();
        all.extend(payload);
        let mut parser = Frames::default();
        for b in &all[..all.len() - 1] {
            parser.bytes.push(*b);
            assert!(parser.next().unwrap().is_none());
        }
        parser.bytes.push(*all.last().unwrap());
        assert_eq!(parser.next().unwrap(), Some(json!({"type":"pong"})));
        parser.bytes.extend(((MAX_FRAME + 1) as u32).to_be_bytes());
        assert!(parser.next().is_err());
    }
    #[test]
    fn ordinary_hook_question_is_visible_and_replaced_by_the_live_request() {
        let (mut live, mut socket, _peer, root) = fixture();
        let hook = BridgeRequest::from_hook_at(
            Provider::Grok,
            json!({"hookEventName":"PreToolUse","sessionId":"s","toolName":"ask_user_question","toolCallId":"t","toolInput":{"questions":[{"question":"Color?"}]}}),
            now(),
        );
        assert!(!hook.needs_reply);
        live.store.ingest(hook).unwrap();
        assert!(live
            .store
            .snapshot()
            .unwrap()
            .attention
            .iter()
            .any(|a| a.kind == "question" && a.state == "open"));
        live.frame(&mut socket, question("rpc", "t")).unwrap();
        assert_eq!(
            live.store
                .snapshot()
                .unwrap()
                .attention
                .iter()
                .filter(|a| a.kind == "question" && a.state == "open")
                .count(),
            1
        );
        drop(live);
        fs::remove_dir_all(root).unwrap();
    }
}

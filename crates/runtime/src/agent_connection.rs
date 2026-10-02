//! Local connector loss is not a Provider completion or an approval decision.
use crate::{RuntimeStore, WaiterRegistry, WaiterTicket};
use actrealm_core::{BridgeRequest, BridgeResponse, Provider};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn handle_agent_connection_event(
    store: &RuntimeStore,
    waiters: &WaiterRegistry,
    event: &BridgeRequest,
) -> bool {
    if !matches!(event.provider, Provider::Kimi | Provider::Grok) {
        return false;
    }
    match event.event_name() {
        Some("AgentRequestClosed") => {
            if let Some(id) = event.raw["request_id"]
                .as_str()
                .and_then(|id| uuid::Uuid::parse_str(id).ok())
            {
                let matches = waiters.raw(id).ok().flatten().is_some_and(|raw| {
                    raw["_actrealm_provider"].as_str() == Some(event.provider.to_string().as_str())
                        && raw["session_id"].as_str() == event.provider_session_id.as_deref()
                });
                if matches {
                    let _ = waiters.pass_through(id, "connector_disconnected");
                    let _ = store.expire_approval(id, "connector_disconnected", event.received_at);
                    mark_lost(store, event);
                }
            }
            true
        }
        Some("AgentDisconnected") => {
            mark_lost(store, event);
            true
        }
        _ => false,
    }
}

fn mark_lost(store: &RuntimeStore, event: &BridgeRequest) {
    if let (Some(id), Some(pid), Ok(snapshot)) = (
        event.provider_session_id.as_deref(),
        event.term.as_ref().and_then(|t| t.provider_pid),
        store.snapshot(),
    ) {
        if let Some(session) = snapshot.sessions.iter().find(|s| {
            s.provider == event.provider.to_string()
                && s.provider_session_id == id
                && s.provider_pid == Some(pid)
        }) {
            let _ = store.mark_execution_unconfirmed(&session.id, session.last_event_at);
        }
    }
}

/// BridgeClient keeps both halves open while awaiting a reply. A dead peer
/// must not leave an actionable request in the native interface.
pub fn wait_for_agent_reply(
    store: &RuntimeStore,
    waiters: &WaiterRegistry,
    request: &BridgeRequest,
    ticket: &WaiterTicket,
    stream: &UnixStream,
) -> Option<BridgeResponse> {
    loop {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let mut byte = 0u8;
        let peek = unsafe {
            libc::recv(
                stream.as_raw_fd(),
                (&mut byte as *mut u8).cast(),
                1,
                libc::MSG_PEEK | libc::MSG_DONTWAIT,
            )
        };
        let disconnected = peek == 0
            || (peek < 0
                && !matches!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::EAGAIN) | Some(libc::EINTR)
                ));
        if disconnected || request.deadline_at.is_none_or(|deadline| now >= deadline) {
            let id = request.request_id.unwrap_or(request.id);
            let _ = waiters.pass_through(id, "connector_disconnected");
            let _ = store.expire_approval(id, "connector_disconnected", now);
            mark_lost(store, request);
            return None;
        }
        match ticket.recv_timeout(Duration::from_millis(100)) {
            Ok(reply) => return Some(reply),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actrealm_core::{BlockingRequestKind, TermContext};
    use serde_json::json;
    #[test]
    fn disconnect_expires_only_its_own_request_without_completing_work() {
        let root = std::env::temp_dir().join(format!("ar-connection-{}", uuid::Uuid::now_v7()));
        let store = RuntimeStore::open(root.join("test.sqlite")).unwrap();
        let waiters = WaiterRegistry::default();
        let mut request = BridgeRequest::from_agent_request_at(
            Provider::Grok,
            "owned",
            Some("turn"),
            BlockingRequestKind::Permission,
            json!({"tool_name":"Edit","tool_input":{"path":"file"}}),
            1000,
            10000,
        )
        .unwrap();
        request.term = Some(TermContext {
            provider_pid: Some(42),
            ..Default::default()
        });
        let id = request.request_id.unwrap();
        let _registration = waiters.register_at(&request, 1000).unwrap();
        store.ingest(request).unwrap();
        let wrong = BridgeRequest::from_provider_event_at(
            Provider::Grok,
            "AgentRequestClosed",
            "other",
            None,
            json!({"request_id":id}),
            2000,
        );
        handle_agent_connection_event(&store, &waiters, &wrong);
        assert!(waiters.is_active(id).unwrap());
        let close = BridgeRequest::from_provider_event_at(
            Provider::Grok,
            "AgentRequestClosed",
            "owned",
            None,
            json!({"request_id":id}),
            2000,
        );
        handle_agent_connection_event(&store, &waiters, &close);
        assert!(!waiters.is_active(id).unwrap());
        let mut disconnected = BridgeRequest::from_provider_event_at(
            Provider::Grok,
            "AgentDisconnected",
            "owned",
            None,
            json!({}),
            2001,
        );
        disconnected.term = Some(TermContext {
            provider_pid: Some(42),
            ..Default::default()
        });
        handle_agent_connection_event(&store, &waiters, &disconnected);
        let snapshot = store.snapshot().unwrap();
        assert_eq!(snapshot.sessions[0].exec_state, "waiting_for_event");
        assert!(!snapshot.attention.iter().any(|a| a.kind == "completion"));
        drop(store);
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn socket_eof_removes_live_controls_even_without_a_close_message() {
        let root = std::env::temp_dir().join(format!("ar-eof-{}", uuid::Uuid::now_v7()));
        let store = RuntimeStore::open(root.join("test.sqlite")).unwrap();
        let waiters = WaiterRegistry::default();
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let mut request = BridgeRequest::from_agent_request_at(
            Provider::Kimi,
            "session",
            Some("turn"),
            BlockingRequestKind::Permission,
            json!({"tool_name":"Edit","tool_input":{"path":"file"}}),
            at,
            at + 60000,
        )
        .unwrap();
        request.term = Some(TermContext {
            provider_pid: Some(42),
            ..Default::default()
        });
        let id = request.request_id.unwrap();
        let registration = waiters.register_at(&request, at).unwrap();
        store.ingest(request.clone()).unwrap();
        let (server, peer) = UnixStream::pair().unwrap();
        drop(peer);
        assert!(
            wait_for_agent_reply(&store, &waiters, &request, &registration.ticket, &server)
                .is_none()
        );
        assert!(!waiters.is_active(id).unwrap());
        assert_eq!(
            store.snapshot().unwrap().sessions[0].exec_state,
            "waiting_for_event"
        );
        drop(store);
        let _ = std::fs::remove_dir_all(root);
    }
}

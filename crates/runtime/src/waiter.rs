use flow_agent_core::{BridgeRequest, BridgeResponse, Decision, Provider};
use serde_json::Value;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hasher};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum WaiterError {
    #[error("request is not a blocking permission request")]
    NotBlocking,
    #[error("waiter is no longer active")]
    NotActive,
    #[error("waiter registry lock is unavailable")]
    Poisoned,
}

pub struct WaiterTicket {
    request_id: Uuid,
    receiver: mpsc::Receiver<BridgeResponse>,
}

impl WaiterTicket {
    pub fn request_id(&self) -> Uuid {
        self.request_id
    }

    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<BridgeResponse, mpsc::RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }
}

pub struct RegisterResult {
    pub ticket: WaiterTicket,
    pub replaced_request_id: Option<Uuid>,
}

#[derive(Clone, Default)]
pub struct WaiterRegistry {
    inner: Arc<RegistryInner>,
}

#[derive(Default)]
struct RegistryInner {
    state: Mutex<RegistryState>,
}

#[derive(Default)]
struct RegistryState {
    by_request: HashMap<Uuid, WaiterEntry>,
    by_correlation: HashMap<String, Uuid>,
}

struct WaiterEntry {
    correlation: String,
    deadline_at: u64,
    raw: Value,
    sender: mpsc::Sender<BridgeResponse>,
}

impl WaiterRegistry {
    pub fn register_at(
        &self,
        request: &BridgeRequest,
        now: u64,
    ) -> Result<RegisterResult, WaiterError> {
        let (Some(request_id), Some(deadline_at)) = (request.request_id, request.deadline_at)
        else {
            return Err(WaiterError::NotBlocking);
        };
        if !request.needs_reply {
            return Err(WaiterError::NotBlocking);
        }

        let correlation = correlation_key(request);
        let (sender, receiver) = mpsc::channel();
        let mut state = self.inner.state.lock().map_err(|_| WaiterError::Poisoned)?;
        let _ = expire_locked(&mut state, now);

        let replaced_request_id = state
            .by_correlation
            .get(&correlation)
            .copied()
            .filter(|existing| *existing != request_id);
        if let Some(replaced) = replaced_request_id {
            if let Some(old) = remove_locked(&mut state, replaced) {
                let _ = old
                    .sender
                    .send(BridgeResponse::pass_through(replaced, "duplicate_replaced"));
            }
        }

        if let Some(old) = remove_locked(&mut state, request_id) {
            let _ = old
                .sender
                .send(BridgeResponse::pass_through(request_id, "request_replaced"));
        }
        state.by_correlation.insert(correlation.clone(), request_id);
        state.by_request.insert(
            request_id,
            WaiterEntry {
                correlation,
                deadline_at,
                raw: request.raw.clone(),
                sender,
            },
        );

        Ok(RegisterResult {
            ticket: WaiterTicket {
                request_id,
                receiver,
            },
            replaced_request_id,
        })
    }

    pub fn decide(&self, request_id: Uuid, decision: Decision) -> Result<(), WaiterError> {
        self.resolve(request_id, BridgeResponse::decided(request_id, decision))
    }

    pub fn pass_through(
        &self,
        request_id: Uuid,
        reason: impl Into<String>,
    ) -> Result<(), WaiterError> {
        self.resolve(request_id, BridgeResponse::pass_through(request_id, reason))
    }

    pub fn expire_at(&self, now: u64) -> Result<usize, WaiterError> {
        Ok(self.expire_request_ids_at(now)?.len())
    }

    pub fn expire_request_ids_at(&self, now: u64) -> Result<Vec<Uuid>, WaiterError> {
        let mut state = self.inner.state.lock().map_err(|_| WaiterError::Poisoned)?;
        Ok(expire_locked(&mut state, now))
    }

    pub fn active_request_ids(&self) -> Result<Vec<Uuid>, WaiterError> {
        let state = self.inner.state.lock().map_err(|_| WaiterError::Poisoned)?;
        Ok(state.by_request.keys().copied().collect())
    }

    pub fn raw(&self, request_id: Uuid) -> Result<Option<Value>, WaiterError> {
        let state = self.inner.state.lock().map_err(|_| WaiterError::Poisoned)?;
        Ok(state
            .by_request
            .get(&request_id)
            .map(|entry| entry.raw.clone()))
    }

    fn resolve(&self, request_id: Uuid, response: BridgeResponse) -> Result<(), WaiterError> {
        let mut state = self.inner.state.lock().map_err(|_| WaiterError::Poisoned)?;
        let Some(entry) = remove_locked(&mut state, request_id) else {
            return Err(WaiterError::NotActive);
        };
        let _ = entry.sender.send(response);
        Ok(())
    }
}

impl Drop for RegistryInner {
    fn drop(&mut self) {
        let Ok(state) = self.state.get_mut() else {
            return;
        };
        for (request_id, entry) in state.by_request.drain() {
            let _ = entry
                .sender
                .send(BridgeResponse::pass_through(request_id, "runtime_shutdown"));
        }
        state.by_correlation.clear();
    }
}

fn expire_locked(state: &mut RegistryState, now: u64) -> Vec<Uuid> {
    let expired: Vec<_> = state
        .by_request
        .iter()
        .filter_map(|(request_id, entry)| (now >= entry.deadline_at).then_some(*request_id))
        .collect();
    for request_id in &expired {
        if let Some(entry) = remove_locked(state, *request_id) {
            let _ = entry
                .sender
                .send(BridgeResponse::pass_through(*request_id, "deadline"));
        }
    }
    expired
}

fn remove_locked(state: &mut RegistryState, request_id: Uuid) -> Option<WaiterEntry> {
    let entry = state.by_request.remove(&request_id)?;
    if state.by_correlation.get(&entry.correlation) == Some(&request_id) {
        state.by_correlation.remove(&entry.correlation);
    }
    Some(entry)
}

fn correlation_key(request: &BridgeRequest) -> String {
    let provider_key = match request.provider {
        Provider::Claude => request.prompt_id.as_deref(),
        Provider::Codex => request.provider_turn_id.as_deref(),
        Provider::Gemini => None,
    }
    .map(ToOwned::to_owned)
    .or_else(|| request.request_id.map(|id| id.to_string()))
    .unwrap_or_else(|| "unknown".to_owned());
    let tool = request
        .raw
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let mut input_hasher = DefaultHasher::new();
    if let Some(input) = request.raw.get("tool_input") {
        input_hasher.write(input.to_string().as_bytes());
    }
    format!(
        "{}:{}:{}:{}:{:016x}",
        request.provider,
        request.provider_session_id.as_deref().unwrap_or("unknown"),
        provider_key,
        tool,
        input_hasher.finish(),
    )
}

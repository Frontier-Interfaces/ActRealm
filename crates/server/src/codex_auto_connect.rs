//! Automatic connection of observed Codex sessions. A successful resume is
//! required before declaring attachment; connection never starts a model turn.
use super::*;
use actrealm_codex_connector::WeakCodexConnector;

const BATCH_SIZE: usize = 4;
const POLL_INTERVAL: Duration = Duration::from_secs(2);

pub(super) fn connection_state(state: &CodexManagerState, id: &str) -> &'static str {
    if state.status != "connected" || state.credential_change_handled {
        "unavailable"
    } else if state.connecting.contains(id) {
        "connecting"
    } else if state.owned_elsewhere.contains(id) {
        "owned_elsewhere"
    } else if state.resume_failed.contains(id) {
        "retrying"
    } else if state.managed.contains(id)
        && state
            .threads
            .get(id)
            .is_some_and(|t| t.status != "notLoaded")
    {
        "connected"
    } else {
        "pending"
    }
}

pub(super) fn attach(
    connector: &CodexConnector,
    state: &Arc<Mutex<CodexManagerState>>,
    store: &RuntimeStore,
    waiters: &WaiterRegistry,
    id: &str,
) -> Result<CodexThread, String> {
    {
        let mut current = state.lock().map_err(|_| "Connector state is unavailable")?;
        if current.credential_change_handled || !current.connecting.insert(id.to_owned()) {
            return Err("Connector attachment is already pending or reconnecting".to_owned());
        }
    }
    let resumed = connector
        .resume_thread(id)
        .map_err(|e| e.to_string())
        .and_then(|thread| {
            if thread.id == id && thread.status != "notLoaded" {
                Ok(thread)
            } else {
                Err("Codex did not confirm the requested thread attachment".to_owned())
            }
        });
    let mut current = state.lock().map_err(|_| "Connector state is unavailable")?;
    current.connecting.remove(id);
    if current.status != "connected" || current.credential_change_handled {
        current.resume_failed.insert(id.to_owned());
        return Err("Connector changed while attaching the thread".to_owned());
    }
    let thread = match resumed {
        Ok(thread) => thread,
        Err(error) => {
            if error.contains("already has an active writer") {
                current.owned_elsewhere.insert(id.to_owned());
            } else {
                current.owned_elsewhere.remove(id);
            }
            current.resume_failed.insert(id.to_owned());
            return Err(error);
        }
    };
    current.managed.insert(id.to_owned());
    current.resume_failed.remove(id);
    current.owned_elsewhere.remove(id);
    current.threads.insert(id.to_owned(), thread.clone());
    drop(current);
    // An idle independent connector must not end a turn owned by Desktop.
    if initial_codex_execution_is_authoritative(&thread) {
        let _ = store.sync_provider_execution(Provider::Codex, id, true, now_millis());
    }
    sync_initial_codex_native_attention(state, store, waiters, &thread);
    Ok(thread)
}

#[derive(Default)]
struct Retries(HashMap<String, (u32, u64)>);

impl Retries {
    fn due(&self, id: &str, now: u64) -> bool {
        self.0.get(id).is_none_or(|(_, retry_at)| now >= *retry_at)
    }

    fn record(&mut self, id: &str, succeeded: bool, now: u64) {
        if succeeded {
            self.0.remove(id);
        } else {
            let (count, _) = self.0.get(id).copied().unwrap_or_default();
            let delay = 5_000_u64.saturating_mul(1 << count.min(4)).min(60_000);
            self.0.insert(
                id.to_owned(),
                (count.saturating_add(1), now.saturating_add(delay)),
            );
        }
    }
}

fn candidates(store: &RuntimeStore, restored: &HashSet<String>, now: u64) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut ids = store
        .ui_snapshot(now.saturating_sub(SESSION_LIST_RETENTION_MS))
        .map(|s| {
            s.sessions
                .into_iter()
                .filter(|s| s.provider == "codex")
                .map(|s| s.provider_session_id)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut restored = restored.iter().cloned().collect::<Vec<_>>();
    restored.sort();
    ids.extend(restored);
    ids.retain(|id| !id.is_empty() && id.len() <= 256 && seen.insert(id.clone()));
    ids
}

pub(super) fn spawn(
    connector: WeakCodexConnector,
    state: Arc<Mutex<CodexManagerState>>,
    store: RuntimeStore,
    waiters: WaiterRegistry,
    mut restored: HashSet<String>,
) {
    let _ = thread::Builder::new()
        .name("actrealm-codex-auto-connect".to_owned())
        .spawn(move || {
            let mut retries = Retries::default();
            loop {
                let Some(connector) = connector.upgrade() else {
                    break;
                };
                if state
                    .lock()
                    .is_ok_and(|s| s.credential_change_handled || s.status != "connected")
                {
                    break;
                }
                let ids = candidates(&store, &restored, now_millis());
                retries.0.retain(|id, _| ids.contains(id));
                let mut attempted = 0;
                for id in ids {
                    let eligible = state.lock().is_ok_and(|s| {
                        matches!(
                            connection_state(&s, &id),
                            "pending" | "retrying" | "owned_elsewhere"
                        )
                    });
                    if !eligible || !retries.due(&id, now_millis()) {
                        continue;
                    }
                    let succeeded = attach(&connector, &state, &store, &waiters, &id).is_ok();
                    retries.record(&id, succeeded, now_millis());
                    if succeeded {
                        restored.remove(&id);
                    }
                    attempted += 1;
                    if attempted >= BATCH_SIZE {
                        break;
                    }
                }
                drop(connector);
                thread::sleep(POLL_INTERVAL);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("actrealm-auto-connect-{}", Uuid::now_v7()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn observed(store: &RuntimeStore, id: &str, now: u64) {
        let mut request = BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"UserPromptSubmit", "session_id":id,
                "turn_id":format!("turn-{id}"), "prompt":"fixture"
            }),
            now,
        );
        request.term = None;
        store.ingest(request).unwrap();
    }

    fn connector(root: &FilePath) -> (CodexConnector, ConnectorChannels) {
        let path = root.join("fake-codex");
        let log = root.join("rpc.log");
        fs::write(&path, format!(r#"#!/bin/sh
while IFS= read -r line; do
  printf '%s\n' "$line" >> '{}'
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{{"id":%s,"result":{{"userAgent":"codex_cli_rs/0.144.5"}}}}\n' "$id" ;;
    *'"method":"thread/resume"'*)
      task=$(printf '%s' "$line" | sed -n 's/.*"threadId":"\([^"]*\)".*/\1/p')
      if [ "$task" = fail ]; then
        printf '{{"id":%s,"error":{{"code":-32000,"message":"not ready"}}}}\n' "$id"
      elif [ "$task" = busy ]; then
        printf '{{"id":%s,"error":{{"code":-32600,"message":"thread already has an active writer"}}}}\n' "$id"
      else
        [ "$task" = mismatch ] && task=wrong
        printf '{{"id":%s,"result":{{"thread":{{"id":"%s","status":{{"type":"idle"}}}}}}}}\n' "$id" "$task"
      fi ;;
  esac
done
"#, log.display())).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        warm_codex_test_executable(&path);
        CodexConnector::connect(&path, &root.join("unused.sock")).unwrap()
    }

    fn connected() -> Arc<Mutex<CodexManagerState>> {
        Arc::new(Mutex::new(CodexManagerState {
            status: "connected".to_owned(),
            ..Default::default()
        }))
    }

    fn wait_for(state: &Arc<Mutex<CodexManagerState>>, id: &str) {
        let start = Instant::now();
        while connection_state(&state.lock().unwrap(), id) != "connected" {
            assert!(
                start.elapsed() < Duration::from_secs(8),
                "automatic attachment timed out for {id}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn existing_and_new_sessions_connect_without_a_manual_action_and_restore_after_restart() {
        let root = root();
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        observed(&store, "existing", now_millis());
        for pass in 0..2 {
            let (connector, _channels) = connector(&root);
            let state = connected();
            spawn(
                connector.downgrade(),
                state.clone(),
                store.clone(),
                WaiterRegistry::default(),
                HashSet::new(),
            );
            wait_for(&state, "existing");
            if pass == 0 {
                observed(&store, "new", now_millis());
            }
            wait_for(&state, "new");
            assert!(store
                .snapshot()
                .unwrap()
                .sessions
                .iter()
                .all(|s| s.exec_state == "thinking"));
            connector.shutdown();
        }
        let rpc = fs::read_to_string(root.join("rpc.log")).unwrap();
        assert_eq!(rpc.matches("\"method\":\"thread/resume\"").count(), 4);
        assert!(!rpc.contains("turn/start"));
        assert!(!rpc.contains("turn/steer"));
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_and_wrong_identity_resumes_never_grant_managed_control() {
        let root = root();
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        let (connector, _channels) = connector(&root);
        let state = connected();
        for (id, expected) in [
            ("fail", "retrying"),
            ("mismatch", "retrying"),
            ("busy", "owned_elsewhere"),
        ] {
            assert!(attach(&connector, &state, &store, &WaiterRegistry::default(), id).is_err());
            let current = state.lock().unwrap();
            assert!(!current.managed.contains(id));
            assert!(!current.connecting.contains(id));
            assert_eq!(connection_state(&current, id), expected);
        }
        assert!(attach(
            &connector,
            &state,
            &store,
            &WaiterRegistry::default(),
            "good"
        )
        .is_ok());
        assert_eq!(
            connection_state(&state.lock().unwrap(), "good"),
            "connected"
        );
        connector.shutdown();
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resume_notifications_do_not_finish_an_independently_running_turn() {
        let root = root();
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        observed(&store, "desktop", now_millis());
        let state = connected();
        let waiters = WaiterRegistry::default();
        for (method, params) in [
            (
                "thread/started",
                json!({"thread":{"id":"desktop","status":{"type":"idle"}}}),
            ),
            (
                "thread/status/changed",
                json!({"threadId":"desktop","status":{"type":"idle"}}),
            ),
            ("thread/closed", json!({"threadId":"desktop"})),
        ] {
            update_codex_notification(
                &state,
                &store,
                &waiters,
                ServerNotification {
                    method: method.to_owned(),
                    params,
                },
            );
            assert_eq!(
                store.snapshot().unwrap().sessions[0].exec_state,
                "thinking",
                "{method} ended the original turn"
            );
        }
        assert_eq!(state.lock().unwrap().threads["desktop"].status, "notLoaded");
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn every_observed_codex_session_is_eligible_beyond_the_old_manual_limit() {
        let root = root();
        let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
        for index in 0..40 {
            observed(&store, &format!("task-{index}"), 100_000);
        }
        let ids = candidates(&store, &HashSet::from(["task-0".to_owned()]), 100_001);
        assert_eq!(ids.len(), 40);
        let mut retries = Retries::default();
        retries.record("task-0", false, 1_000);
        assert!(!retries.due("task-0", 5_999));
        assert!(retries.due("task-0", 6_000));
        assert!(retries.due("task-1", 1_001));
        retries.record("task-0", false, 6_000);
        assert!(!retries.due("task-0", 15_999));
        retries.record("task-0", true, 16_000);
        assert!(retries.due("task-0", 16_000));
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}

//! Automatic connection of observed Codex sessions. A successful resume is
//! required before declaring attachment; connection never starts a model turn.
use super::*;
use actrealm_codex_connector::WeakCodexConnector;

const BATCH_SIZE: usize = 4;
const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// A thread that keeps failing (it may not exist in Codex at all) is left
/// alone for the rest of this Runtime run after this many attempts.
const MAX_ATTEMPTS: u32 = 6;

pub(super) fn connection_state(state: &CodexManagerState, id: &str) -> &'static str {
    let current = if state.status != "connected" || state.credential_change_handled {
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
    };
    // Waiting for an attachment that will never come (see `spawn`, and
    // `MAX_ATTEMPTS`) is no wait at all.
    let attaches = state.automatic_attachment && !state.attach_given_up.contains(id);
    if matches!(current, "pending" | "retrying") && !attaches {
        "unavailable"
    } else {
        current
    }
}

/// Only a connection that shares Codex's official app-server attaches
/// sessions on its own; see `spawn`.
pub(super) fn attaches_on_its_own(connector: &CodexConnector) -> bool {
    connector.is_shared_connection()
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
        self.0
            .get(id)
            .is_none_or(|(count, retry_at)| *count < MAX_ATTEMPTS && now >= *retry_at)
    }

    /// Threads that are no longer tried.
    fn exhausted(&self) -> HashSet<String> {
        self.0
            .iter()
            .filter(|(_, (count, _))| *count >= MAX_ATTEMPTS)
            .map(|(id, _)| id.clone())
            .collect()
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

/// None when the session list cannot be read: that round is skipped, so it
/// cannot reset what earlier rounds learned about failing threads.
fn candidates(store: &RuntimeStore, restored: &HashSet<String>, now: u64) -> Option<Vec<String>> {
    let mut seen = HashSet::new();
    let mut ids = store
        .ui_snapshot(now.saturating_sub(SESSION_LIST_RETENTION_MS))
        .ok()?
        .sessions
        .into_iter()
        .filter(|s| s.provider == "codex")
        .map(|s| s.provider_session_id)
        .collect::<Vec<_>>();
    let mut restored = restored.iter().cloned().collect::<Vec<_>>();
    restored.sort();
    ids.extend(restored);
    ids.retain(|id| !id.is_empty() && id.len() <= 256 && seen.insert(id.clone()));
    Some(ids)
}

/// A thread another app-server holds stays with it: taking it over as soon as
/// that app unloads it would lock the user out of their own conversation.
fn eligible(state: &CodexManagerState, id: &str) -> bool {
    matches!(connection_state(state, id), "pending" | "retrying")
}

/// Attaches observed Codex sessions on their own. Only for a connection that
/// shares the Codex app's own app-server: resuming a thread in a separate
/// app-server takes Codex's single-writer lock, and the Codex app then shows
/// the conversation as open in another app until that process exits.
pub(super) fn spawn(
    connector: WeakCodexConnector,
    state: Arc<Mutex<CodexManagerState>>,
    store: RuntimeStore,
    waiters: WaiterRegistry,
    mut restored: HashSet<String>,
) {
    if let Ok(mut current) = state.lock() {
        current.automatic_attachment = true;
    }
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
                let Some(ids) = candidates(&store, &restored, now_millis()) else {
                    drop(connector);
                    thread::sleep(POLL_INTERVAL);
                    continue;
                };
                // A thread that left the list and came back has new activity.
                retries.0.retain(|id, _| ids.contains(id));
                let mut attempted = 0;
                for id in ids {
                    let can_attach = state.lock().is_ok_and(|s| eligible(&s, &id));
                    if !can_attach || !retries.due(&id, now_millis()) {
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
                if let Ok(mut current) = state.lock() {
                    current.attach_given_up = retries.exhausted();
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
            automatic_attachment: true,
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
        let ids = candidates(&store, &HashSet::from(["task-0".to_owned()]), 100_001).unwrap();
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

    #[test]
    fn conversations_held_elsewhere_or_never_found_are_left_alone() {
        let state = connected();
        let mut current = state.lock().unwrap();
        current.owned_elsewhere.insert("desktop".to_owned());
        current.resume_failed.insert("desktop".to_owned());
        current.resume_failed.insert("flaky".to_owned());
        // The Codex app holds it: never taken over when the app lets it go.
        assert_eq!(connection_state(&current, "desktop"), "owned_elsewhere");
        assert!(!eligible(&current, "desktop"));
        assert!(eligible(&current, "flaky"));
        assert!(eligible(&current, "new"));

        // Without automatic attachment nothing is pending; a thread attached
        // on request still reads as connected.
        current.automatic_attachment = false;
        current.managed.insert("asked".to_owned());
        current.threads.insert(
            "asked".to_owned(),
            CodexThread {
                id: "asked".to_owned(),
                name: None,
                cwd: None,
                status: "idle".to_owned(),
                active_flags: Vec::new(),
                updated_at: None,
                approval_policy: None,
                approvals_reviewer: None,
                sandbox_mode: None,
            },
        );
        assert_eq!(connection_state(&current, "new"), "unavailable");
        assert!(!eligible(&current, "new"));
        assert_eq!(connection_state(&current, "asked"), "connected");
        drop(current);

        // A thread that keeps failing stops being tried.
        let mut retries = Retries::default();
        let mut now = 0;
        for _ in 0..MAX_ATTEMPTS {
            assert!(retries.due("gone", now));
            retries.record("gone", false, now);
            now += 120_000;
        }
        assert!(!retries.due("gone", now));
        assert!(!retries.due("gone", u64::MAX));
        retries.record("flaky", false, now);
        assert_eq!(retries.exhausted(), HashSet::from(["gone".to_owned()]));

        // Given up on, it no longer reads as retrying.
        let state = connected();
        let mut current = state.lock().unwrap();
        current.resume_failed.insert("gone".to_owned());
        current.resume_failed.insert("flaky".to_owned());
        current.attach_given_up = retries.exhausted();
        assert_eq!(connection_state(&current, "gone"), "unavailable");
        assert!(!eligible(&current, "gone"));
        assert_eq!(connection_state(&current, "flaky"), "retrying");
    }
}

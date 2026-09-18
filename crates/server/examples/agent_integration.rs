//! Isolated local acceptance harness. Does not load unrelated Provider accounts.
//! The private connection file is for the test operator; never publish it.
use actrealm_bridge::BridgeListener;
use actrealm_core::{BridgeResponse, DOCTOR_PROBE_EVENT};
use actrealm_installer::InstallPaths;
use actrealm_runtime::{RuntimeStore, WaiterRegistry};
use actrealm_server::{ApiServer, ApiServerConfig};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("Pass an isolated absolute test directory")?,
    );
    if !root.is_absolute() {
        return Err("Test directory must be absolute".into());
    }
    fs::create_dir_all(root.join("run"))?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    let store = RuntimeStore::open(root.join("test.sqlite"))?;
    let waiters = WaiterRegistry::default();
    let paths = InstallPaths {
        actrealm_home: root.clone(),
        claude_settings: root.join("test-user/.claude/settings.json"),
        codex_hooks: root.join("test-user/.codex/hooks.json"),
        codex_config: root.join("test-user/.codex/config.toml"),
    };
    let api = ApiServer::start(
        store.clone(),
        waiters.clone(),
        ApiServerConfig {
            install_paths: Some(paths),
            ..Default::default()
        },
    )?;
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(root.join("connection.json"))?;
    serde_json::to_writer(
        &mut file,
        &serde_json::json!({"url":api.origin(),"bootstrapToken":api.bootstrap_token()}),
    )?;
    file.flush()?;
    let listener = BridgeListener::bind(root.join("run/bridge.sock"))?;
    println!("Isolated Agent acceptance Runtime is ready");
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let store = store.clone();
        let waiters = waiters.clone();
        std::thread::spawn(move || {
            let Ok(request) = BridgeListener::read_request(&mut stream) else {
                return;
            };
            if request.event_name() == Some(DOCTOR_PROBE_EVENT) {
                let _ = BridgeListener::write_response(
                    &mut stream,
                    &BridgeResponse::pass_through(
                        request.request_id.unwrap_or(request.id),
                        "doctor_probe_ok",
                    ),
                );
                return;
            }
            if actrealm_runtime::handle_agent_connection_event(&store, &waiters, &request) {
                return;
            }
            let registration = if request.needs_reply {
                waiters.register_at(&request, now()).ok()
            } else {
                None
            };
            if let Ok(result) = store.ingest(request.clone()) {
                for id in result.resolved_request_ids {
                    let _ = waiters.pass_through(id, "provider_handled");
                }
            }
            if let Some(registration) = registration {
                if let Some(reply) = actrealm_runtime::wait_for_agent_reply(
                    &store,
                    &waiters,
                    &request,
                    &registration.ticket,
                    &stream,
                ) {
                    let _ = BridgeListener::write_response(&mut stream, &reply);
                }
            }
        });
    }
    drop(api);
    Ok(())
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

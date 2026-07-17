#![cfg(unix)]

use flow_agent_core::{BridgeRequest, Provider};
use flow_agent_installer::InstallPaths;
use flow_agent_runtime::{RuntimeStore, WaiterRegistry};
use flow_agent_server::{ApiServer, ApiServerConfig};
use serde_json::json;
use std::env;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn preview_paths(root: &std::path::Path) -> InstallPaths {
    InstallPaths {
        flow_home: root.join("flow-home"),
        claude_settings: root.join("home/.claude/settings.json"),
        codex_hooks: root.join("codex/hooks.json"),
        codex_config: root.join("codex/config.toml"),
    }
}

/// Starts an isolated, seeded control panel for manual browser QA.
///
/// Run with `FLOW_AGENT_PREVIEW_SECONDS=180 cargo test -p flow-agent-server
/// --test manual_preview -- --ignored --nocapture`.
#[test]
#[ignore = "manual local browser preview"]
fn seeded_m10_m12_control_panel_preview() {
    let root = env::temp_dir().join(format!(
        "flow-agent-manual-preview-{}-{}",
        std::process::id(),
        Uuid::now_v7()
    ));
    let store = RuntimeStore::open(root.join("data.sqlite")).unwrap();
    let waiters = WaiterRegistry::default();
    let now = now_millis();

    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"UserPromptSubmit",
                "session_id":"preview-codex-thread",
                "turn_id":"preview-turn",
                "cwd":"/tmp/flow-agent",
                "prompt":"实现 M10 可配置任务卡和安全字段目录",
                "model":"gpt-5.6-codex",
                "thread_name":"Flow Agent · M10 可配置展示"
            }),
            now.saturating_sub(42_000),
        ))
        .unwrap();

    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"UserPromptSubmit",
                "session_id":"preview-native-approval",
                "turn_id":"preview-native-turn",
                "cwd":"/Users/example/Desktop",
                "prompt":"在桌面建立一个空白文件夹",
                "model":"gpt-5.6-codex",
                "thread_name":"在桌面建立空白文件夹"
            }),
            now.saturating_sub(10_000),
        ))
        .unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"PreToolUse",
                "session_id":"preview-native-approval",
                "turn_id":"preview-native-turn",
                "cwd":"/Users/example/Desktop",
                "tool_name":"request_permissions",
                "tool_use_id":"preview-request-permissions",
                "tool_input":{
                    "reason":"允许 Codex 在桌面创建文件夹。"
                }
            }),
            now.saturating_sub(2_000),
        ))
        .unwrap();
    store
        .ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"PreToolUse",
                "session_id":"preview-codex-thread",
                "turn_id":"preview-turn",
                "cwd":"/tmp/flow-agent",
                "tool_name":"Bash",
                "tool_input":{"command":"cargo test --workspace"},
                "model":"gpt-5.6-codex"
            }),
            now.saturating_sub(8_000),
        ))
        .unwrap();

    let question = BridgeRequest::from_hook_at(
        Provider::Claude,
        json!({
            "hook_event_name":"PreToolUse",
            "session_id":"preview-claude-session",
            "cwd":"/tmp/customer-portal",
            "tool_name":"AskUserQuestion",
            "tool_input":{
                "questions":[{
                    "question":"发布前采用哪一档任务卡信息密度？",
                    "header":"展示密度",
                    "multiSelect":false,
                    "options":[
                        {"label":"详细","description":"显示工具、Token、子 Agent 和恢复状态"},
                        {"label":"简洁","description":"只显示任务内容和实时状态"}
                    ]
                }]
            }
        }),
        now.saturating_sub(3_000),
    );
    let _question_ticket = waiters.register_at(&question, now).unwrap();
    store.ingest(question).unwrap();

    let resolver = store.clone();
    let api = ApiServer::start(
        store,
        waiters,
        ApiServerConfig {
            install_paths: Some(preview_paths(&root)),
            enable_codex_connector: false,
            ..ApiServerConfig::default()
        },
    )
    .unwrap();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(90));
        let _ = resolver.ingest(BridgeRequest::from_hook_at(
            Provider::Codex,
            json!({
                "hook_event_name":"PostToolUse",
                "session_id":"preview-native-approval",
                "turn_id":"preview-native-turn",
                "cwd":"/Users/example/Desktop",
                "tool_name":"request_permissions",
                "tool_use_id":"preview-request-permissions",
                "tool_response":{"status":"handled"}
            }),
            now_millis(),
        ));
    });
    println!("FLOW_AGENT_PREVIEW_URL={}", api.bootstrap_url());
    let seconds = env::var("FLOW_AGENT_PREVIEW_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (10..=600).contains(value))
        .unwrap_or(120);
    thread::sleep(Duration::from_secs(seconds));
    drop(api);
}

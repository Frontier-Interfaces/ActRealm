#![cfg(unix)]

use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

struct Auth {
    address: SocketAddr,
    origin: String,
    cookie: String,
    csrf: String,
}

struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Value,
}

struct RuntimeGuard {
    child: Child,
}

impl Drop for RuntimeGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn temp_root() -> PathBuf {
    PathBuf::from("/tmp").join(format!(
        "flow-agent-m2-e2e-{}-{}",
        std::process::id(),
        Uuid::now_v7()
    ))
}

fn try_http(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&Value>,
) -> Option<HttpResponse> {
    let payload = body.map(Value::to_string).unwrap_or_default();
    let mut stream = TcpStream::connect(address).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: {}\r\n",
        payload.len()
    )
    .ok()?;
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n").ok()?;
    }
    if body.is_some() {
        write!(stream, "Content-Type: application/json\r\n").ok()?;
    }
    write!(stream, "\r\n{payload}").ok()?;
    stream.flush().ok()?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).ok()?;
    let marker = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")?;
    let head = String::from_utf8(response[..marker].to_vec()).ok()?;
    let mut lines = head.lines();
    let status = lines.next()?.split_whitespace().nth(1)?.parse().ok()?;
    let response_headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let bytes = &response[marker + 4..];
    Some(HttpResponse {
        status,
        headers: response_headers,
        body: if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(bytes).ok()?
        },
    })
}

fn http(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&Value>,
) -> HttpResponse {
    try_http(address, method, path, headers, body).unwrap()
}

fn bootstrap_auth(address: SocketAddr, origin: &str, token: &str) -> Auth {
    let bootstrap = http(
        address,
        "POST",
        "/api/v1/bootstrap",
        &[("Origin", origin)],
        Some(&json!({ "token": token })),
    );
    assert_eq!(bootstrap.status, 200);
    let cookie = bootstrap
        .headers
        .iter()
        .find(|(name, _)| name == "set-cookie")
        .unwrap()
        .1
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    Auth {
        address,
        origin: origin.to_owned(),
        cookie,
        csrf: bootstrap.body["csrfToken"].as_str().unwrap().to_owned(),
    }
}

fn public_health(address: SocketAddr) -> Value {
    let response = http(address, "GET", "/api/v1/health", &[], None);
    assert_eq!(response.status, 200);
    response.body
}

fn wait_for_restarted_health(address: SocketAddr, previous_instance: &str) -> Value {
    let started = Instant::now();
    loop {
        if let Some(response) = try_http(address, "GET", "/api/v1/health", &[], None) {
            if response.status == 200
                && response.body["instanceId"]
                    .as_str()
                    .is_some_and(|instance| !instance.is_empty() && instance != previous_instance)
            {
                return response.body;
            }
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "timed out waiting for the Runtime to re-exec"
        );
        thread::sleep(Duration::from_millis(25));
    }
}

fn start_runtime(root: &Path) -> (RuntimeGuard, PathBuf, Auth) {
    fs::create_dir_all(root).unwrap();
    let socket = root.join("bridge.sock");
    let mut runtime = Command::new(env!("CARGO_BIN_EXE_flow-agent"))
        .args(["serve", "--socket", socket.to_str().unwrap()])
        .env("FLOW_AGENT_COMMIT_DELAY_MS", "30")
        .env("FLOW_AGENT_HOME", root.join("flow-home"))
        .env("HOME", root.join("home"))
        .env("CODEX_HOME", root.join("codex-home"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut output = BufReader::new(runtime.stdout.take().unwrap());
    let mut control_line = String::new();
    output.read_line(&mut control_line).unwrap();
    assert!(
        control_line.starts_with("Flow Agent control panel: http://"),
        "unexpected Runtime bootstrap line: {control_line:?}"
    );
    let url = control_line
        .trim()
        .strip_prefix("Flow Agent control panel: ")
        .unwrap();
    let (origin, token) = url.split_once("/#bootstrap=").unwrap();
    let address: SocketAddr = origin.strip_prefix("http://").unwrap().parse().unwrap();
    // Keep the pipe open. The runtime writes one diagnostic line per provider
    // event before ingesting it, and a closed reader would cause that thread
    // to stop at the diagnostic write.
    runtime.stdout = Some(output.into_inner());

    let started = Instant::now();
    while !socket.exists() {
        assert!(started.elapsed() < Duration::from_secs(2));
        thread::sleep(Duration::from_millis(10));
    }
    let auth = bootstrap_auth(address, origin, token);
    (RuntimeGuard { child: runtime }, socket, auth)
}

fn spawn_hook(socket: &Path, provider: &str, fixture: &str) -> Child {
    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-agent"))
        .args([
            "hook",
            "--provider",
            provider,
            "--socket",
            socket.to_str().unwrap(),
        ])
        .env("FLOW_AGENT_HOOK_REPLY_TIMEOUT_MS", "2000")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(fixture.as_bytes())
        .unwrap();
    drop(child.stdin.take());
    child
}

fn headers(auth: &Auth) -> [(&str, &str); 3] {
    [
        ("Origin", &auth.origin),
        ("Cookie", &auth.cookie),
        ("X-Flow-Agent-CSRF", &auth.csrf),
    ]
}

fn wait_for_open_attention(auth: &Auth, provider: &str) -> Value {
    let started = Instant::now();
    loop {
        let response = http(
            auth.address,
            "GET",
            "/api/v1/snapshot",
            &[("Cookie", &auth.cookie)],
            None,
        );
        assert_eq!(response.status, 200);
        if let Some(item) = response.body["attention"].as_array().and_then(|items| {
            items.iter().find(|item| {
                item["provider"] == provider
                    && item["kind"] == "approval"
                    && item["state"] == "open"
            })
        }) {
            return item.clone();
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "timed out waiting for {provider} attention"
        );
        thread::sleep(Duration::from_millis(15));
    }
}

fn decide(auth: &Auth, item: &Value, action: &str) {
    let response = http(
        auth.address,
        "POST",
        "/api/v1/commands",
        &headers(auth),
        Some(&json!({
            "id": Uuid::now_v7(),
            "attentionId": item["id"],
            "requestId": item["requestId"],
            "action": action
        })),
    );
    assert_eq!(response.status, 202);
    assert_eq!(response.body["state"], "pending_commit");
}

#[test]
fn real_claude_and_codex_fixtures_are_controlled_through_the_widget_api() {
    let root = temp_root();
    let (runtime, socket, mut auth) = start_runtime(&root);
    let cases = [
        (
            "claude",
            include_str!("../../../fixtures/claude/2.1.210/permission-request.json"),
            "approve",
            "allow",
        ),
        (
            "codex",
            include_str!("../../../fixtures/codex/0.144.4/permission-request.json"),
            "deny",
            "deny",
        ),
    ];
    for _round in 1..=5 {
        for (provider, fixture, action, expected) in cases {
            let hook = spawn_hook(&socket, provider, fixture);
            let attention = wait_for_open_attention(&auth, provider);
            decide(&auth, &attention, action);
            let output = hook.wait_with_output().unwrap();
            assert!(output.status.success());
            assert!(output.stderr.is_empty());
            let directive: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(
                directive.pointer("/hookSpecificOutput/decision/behavior"),
                Some(&Value::String(expected.to_owned()))
            );
        }
    }

    let original_pid = runtime.child.id();
    for restart_count in 1..=5 {
        let interrupted_hook = (restart_count == 1).then(|| {
            let hook = spawn_hook(&socket, "claude", cases[0].1);
            let _ = wait_for_open_attention(&auth, "claude");
            hook
        });
        let previous = public_health(auth.address);
        let previous_instance = previous["instanceId"].as_str().unwrap().to_owned();
        let restart_token = Uuid::now_v7().to_string();
        let restarted = http(
            auth.address,
            "POST",
            "/api/v1/runtime/restart",
            &headers(&auth),
            Some(&json!({ "restartToken": restart_token })),
        );
        assert_eq!(restarted.status, 200);
        assert_eq!(restarted.body["restartToken"], restart_token);
        assert_eq!(restarted.body["previousInstanceId"], previous_instance);

        if let Some(interrupted_hook) = interrupted_hook {
            let interrupted_output = interrupted_hook.wait_with_output().unwrap();
            assert!(interrupted_output.status.success());
            assert!(interrupted_output.stdout.is_empty());
            assert!(interrupted_output.stderr.is_empty());
        }

        let current = wait_for_restarted_health(auth.address, &previous_instance);
        assert_ne!(current["instanceId"], previous["instanceId"]);
        auth = bootstrap_auth(auth.address, &auth.origin, &restart_token);
        let status = http(
            auth.address,
            "GET",
            "/api/v1/runtime/status",
            &[("Cookie", &auth.cookie)],
            None,
        );
        assert_eq!(status.status, 200);
        assert_eq!(status.body["pid"], original_pid);
        assert_eq!(status.body["restart"]["count"], restart_count);
        assert_eq!(status.body["restart"]["lastResult"], "recovered");
        assert_eq!(status.body["api"]["address"], auth.address.to_string());
        assert_eq!(status.body["hook"]["status"], "ready");
        assert!(status.body["hook"]["private"].as_bool().unwrap());
        assert!(socket.exists());
    }

    let hook = spawn_hook(&socket, "claude", cases[0].1);
    let attention = wait_for_open_attention(&auth, "claude");
    decide(&auth, &attention, "approve");
    let output = hook.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let directive: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        directive.pointer("/hookSpecificOutput/decision/behavior"),
        Some(&Value::String("allow".to_owned()))
    );

    drop(runtime);
    fs::remove_dir_all(root).unwrap();
}

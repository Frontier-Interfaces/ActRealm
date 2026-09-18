//! A local interactive ACP client. The provider executes tools; ActRealm's
//! existing request registry owns only each live approval/question decision.
use actrealm_bridge::{default_socket_path, BridgeClient};
use actrealm_core::{BridgeRequest, Provider, TermContext};
use actrealm_installer::agents::agent_executable;
use actrealm_providers::acp::{Observer, MAX_FRAME_BYTES};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
static INTERRUPTED: AtomicBool = AtomicBool::new(false);
extern "C" fn interrupted(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::Relaxed);
}
struct SignalGuard(libc::sighandler_t, libc::sighandler_t);
impl SignalGuard {
    fn install() -> Self {
        unsafe {
            Self(
                libc::signal(libc::SIGINT, interrupted as *const () as libc::sighandler_t),
                libc::signal(
                    libc::SIGTERM,
                    interrupted as *const () as libc::sighandler_t,
                ),
            )
        }
    }
}
impl Drop for SignalGuard {
    fn drop(&mut self) {
        unsafe {
            libc::signal(libc::SIGINT, self.0);
            libc::signal(libc::SIGTERM, self.1);
        }
    }
}
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

type Writer = Arc<Mutex<ChildStdin>>;
enum Message {
    Frame(Value),
    Closed,
    Input(String),
}

struct Client {
    child: Child,
    writer: Writer,
    receive: mpsc::Receiver<Message>,
    send: mpsc::SyncSender<Message>,
    observer: Option<Observer>,
    next_id: u64,
    answered: Arc<Mutex<HashSet<String>>>,
    counter_instance: String,
    pending: Arc<Mutex<HashSet<uuid::Uuid>>>,
    provider_failed: bool,
    replaying: bool,
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Some(observer) = &self.observer {
            let pending = self
                .pending
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .iter()
                .copied()
                .collect::<Vec<_>>();
            for id in &pending {
                publish(
                    &self.child,
                    observer.event("AgentRequestClosed", json!({"request_id":id}), now()),
                );
            }
            if observer.turn.is_some() || !pending.is_empty() {
                publish(
                    &self.child,
                    observer.event("AgentDisconnected", json!({}), now()),
                );
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn run(
    provider: Provider,
    cwd: PathBuf,
    prompt: Option<String>,
    resume: Option<String>,
    model: Option<String>,
) -> Result<()> {
    let _signals = SignalGuard::install();
    if !matches!(provider, Provider::Kimi | Provider::Grok) {
        bail!("Connected Agent sessions support kimi and grok");
    }
    let cwd = cwd
        .canonicalize()
        .context("Select an existing project directory")?;
    if !cwd.is_dir() {
        bail!("The project path must be a directory");
    }
    let probe = BridgeRequest::doctor_probe_at(now());
    if BridgeClient::new(default_socket_path())
        .send(&probe, Duration::from_secs(2))
        .ok()
        .flatten()
        .is_none()
    {
        bail!("Open ActRealm before starting a connected Agent session");
    }
    let executable = agent_executable(&provider.to_string())
        .context("Official CLI not found; install Kimi Code or Grok Build first")?;
    let mut command = Command::new(executable);
    if provider == Provider::Kimi {
        command.arg("acp");
    } else {
        command.args(["--no-auto-update", "agent", "--no-leader", "stdio"]);
    }
    // The wrapper projects structured events itself, so installed observation
    // Hooks must not duplicate the same session/turn/tool records.
    command
        .current_dir(&cwd)
        .env("ACTREALM_CONNECTED_PROVIDER", provider.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut child = command
        .spawn()
        .context("Unable to launch the Agent ACP process")?;
    let writer = Arc::new(Mutex::new(
        child.stdin.take().context("Missing Agent stdin")?,
    ));
    let stdout = child.stdout.take().context("Missing Agent stdout")?;
    let (send, receive) = mpsc::sync_channel(128);
    let output = send.clone();
    std::thread::spawn(move || {
        let mut reader = io::BufReader::new(stdout);
        loop {
            let mut bytes = Vec::new();
            match (&mut reader)
                .take((MAX_FRAME_BYTES + 1) as u64)
                .read_until(b'\n', &mut bytes)
            {
                Ok(n) if n > 0 && n <= MAX_FRAME_BYTES => {
                    let Ok(frame) = serde_json::from_slice::<Value>(&bytes) else {
                        break;
                    };
                    if output.send(Message::Frame(frame)).is_err() {
                        return;
                    }
                }
                _ => break,
            }
        }
        let _ = output.send(Message::Closed);
    });
    let mut client = Client {
        child,
        writer,
        receive,
        send,
        observer: None,
        next_id: 1,
        answered: Arc::new(Mutex::new(HashSet::new())),
        counter_instance: uuid::Uuid::now_v7().to_string(),
        pending: Arc::new(Mutex::new(HashSet::new())),
        provider_failed: false,
        replaying: false,
    };
    let init=client.call("initialize",json!({"protocolVersion":1,"clientInfo":{"name":"ActRealm","version":env!("CARGO_PKG_VERSION")},"clientCapabilities":{"elicitation":{"form":{}}}}),Duration::from_secs(30))?;
    if init["protocolVersion"] != 1 {
        bail!("Unsupported Agent ACP protocol; expected version 1");
    }
    let auth = if provider == Provider::Grok {
        init["authMethods"]
            .as_array()
            .and_then(|items| {
                items
                    .iter()
                    .find(|m| m["id"] == "cached_token")
                    .or_else(|| items.iter().find(|m| m["id"] == "xai.api_key"))
            })
            .and_then(|m| m["id"].as_str())
    } else {
        None
    };
    if let Some(method) = auth {
        client
            .call(
                "authenticate",
                json!({"methodId":method,"_meta":{"headless":true}}),
                Duration::from_secs(30),
            )
            .with_context(|| format!("Sign in first using `{provider} login`, then retry"))?;
    }
    let (method, params) = if let Some(id) = resume {
        if init
            .pointer("/agentCapabilities/loadSession")
            .and_then(Value::as_bool)
            != Some(true)
        {
            bail!("This Agent version does not support session/load");
        }
        (
            "session/load",
            json!({"sessionId":id,"cwd":cwd,"mcpServers":[]}),
        )
    } else {
        ("session/new", json!({"cwd":cwd,"mcpServers":[]}))
    };
    if method == "session/load" {
        let id = params["sessionId"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 256)
            .context("Invalid session ID")?;
        client.observer = Some(Observer::new(
            provider,
            id.to_owned(),
            cwd.to_string_lossy().into_owned(),
        ));
        client.replaying = true;
    }
    let session=client.call(method,params.clone(),Duration::from_secs(30))
        .with_context(||format!("Could not open the session. Complete `{provider} login` or configure the provider API credentials"))?;
    let id = session["sessionId"]
        .as_str()
        .or_else(|| params["sessionId"].as_str())
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .context("Agent returned no valid session ID")?
        .to_owned();
    client.replaying = false;
    let mut observer = client
        .observer
        .take()
        .unwrap_or_else(|| Observer::new(provider, id.clone(), cwd.to_string_lossy().into_owned()));
    observer.model = actrealm_providers::acp::selected_model(&session);
    if let Some(model) = model {
        client.call(
            "session/set_model",
            json!({"sessionId":id,"modelId":model}),
            Duration::from_secs(15),
        )?;
        observer.model = Some(model);
    }
    publish(&client.child,observer.event("SessionStart",json!({"session_title":format!("{} · {}",provider,cwd.file_name().unwrap_or_default().to_string_lossy())}),now()));
    client.observer = Some(observer);
    eprintln!("Connected {provider} session {id}. Approvals and questions appear in ActRealm.");
    if let Some(prompt) = prompt {
        client.prompt(prompt)?;
        return Ok(());
    }
    eprintln!("Enter a prompt. /cancel interrupts the current turn; /exit closes this client.");
    let input = client.send.clone();
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            match line {
                Ok(line) => {
                    if input.send(Message::Input(line)).is_err() {
                        return;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = input.send(Message::Input("/exit".to_owned()));
    });
    loop {
        if INTERRUPTED.load(Ordering::Relaxed) {
            break;
        }
        let incoming = match client.receive.recv_timeout(Duration::from_millis(100)) {
            Ok(message) => message,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => break,
        };
        match incoming {
            Message::Input(text) if text.trim() == "/exit" => break,
            Message::Input(text) if text.trim() == "/cancel" => eprintln!("No turn is running."),
            Message::Input(text) if !text.trim().is_empty() => client.prompt(text)?,
            Message::Frame(frame) => client.frame(&frame),
            Message::Closed => {
                bail!("The Agent disconnected; no pending decision will be replayed")
            }
            _ => {}
        }
    }
    Ok(())
}

impl Client {
    fn call(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        self.provider_failed = false;
        let id = self.next_id;
        self.next_id += 1;
        write_frame(
            &self.writer,
            &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
        )?;
        let start = Instant::now();
        while start.elapsed() < timeout {
            if INTERRUPTED.load(Ordering::Relaxed) {
                if let Some(observer) = &self.observer {
                    let _ = write_frame(
                        &self.writer,
                        &json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":observer.session}}),
                    );
                }
                bail!("Interrupted by user");
            }
            match self.receive.recv_timeout(Duration::from_millis(100)) {
                Ok(Message::Frame(frame)) if frame["id"] == id && frame.get("method").is_none() => {
                    if let Some(error) = frame.get("error") {
                        self.provider_failed = true;
                        let code = error["code"].as_i64().unwrap_or(-32603);
                        bail!(
                            "Agent rejected {method} (error {code}): {}",
                            rpc_failure_reason(error)
                        );
                    }
                    return frame
                        .get("result")
                        .cloned()
                        .context("Malformed ACP response");
                }
                Ok(Message::Frame(frame)) => self.frame(&frame),
                Ok(Message::Closed) => bail!("Agent ACP connection closed"),
                Ok(Message::Input(text)) if matches!(text.trim(), "/cancel" | "/exit") => {
                    if let Some(observer) = &self.observer {
                        write_frame(
                            &self.writer,
                            &json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":observer.session}}),
                        )?;
                    }
                    if text.trim() == "/exit" {
                        bail!("Session closed by user");
                    }
                }
                Ok(Message::Input(_)) => {
                    eprintln!("A turn is running. Use /cancel or wait for it to finish.")
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => bail!("Agent reader stopped"),
            }
        }
        bail!("Agent timed out during {method}")
    }

    fn prompt(&mut self, text: String) -> Result<()> {
        if text.len() > 256 * 1024 {
            bail!("Prompt exceeds 256 KiB");
        }
        if !self
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_empty()
        {
            eprintln!(
                "Respond to the pending interaction in ActRealm before starting another turn."
            );
            return Ok(());
        }
        let observer = self.observer.as_mut().context("No Agent session")?;
        observer.begin_turn(uuid::Uuid::now_v7().to_string());
        self.answered
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        publish(
            &self.child,
            observer.event("UserPromptSubmit", json!({"prompt":text}), now()),
        );
        let id = observer.session.clone();
        match self.call(
            "session/prompt",
            json!({"sessionId":id,"prompt":[{"type":"text","text":text}]}),
            Duration::from_secs(24 * 60 * 60),
        ) {
            Ok(result) => {
                if self
                    .observer
                    .as_ref()
                    .is_some_and(|o| o.provider == Provider::Grok)
                {
                    if let Ok(usage) = self.call(
                        "_x.ai/session/usage",
                        json!({"sessionId":id}),
                        Duration::from_secs(5),
                    ) {
                        self.grok_usage(&usage["usage"]);
                    }
                }
                if self
                    .observer
                    .as_ref()
                    .is_some_and(|o| o.provider == Provider::Grok)
                {
                    if let Ok(billing) =
                        self.call("_x.ai/billing", json!({}), Duration::from_secs(8))
                    {
                        let home = runtime_home();
                        let _ =
                            actrealm_quota::agents::capture_grok_billing(&home, &billing, now());
                    }
                }
                if let Some(observer) = &self.observer {
                    let event = if result["stopReason"] == "cancelled" {
                        "TurnInterrupted"
                    } else if result["stopReason"] == "end_turn" {
                        "Stop"
                    } else {
                        "StopFailure"
                    };
                    publish(&self.child, observer.event(event, json!({}), now()));
                }
                if let Some(observer) = self.observer.as_mut() {
                    observer.turn = None;
                }
                println!();
                Ok(())
            }
            Err(error) => {
                if self.provider_failed {
                    if let Some(observer) = self.observer.as_mut() {
                        publish(
                            &self.child,
                            observer.event(
                                "StopFailure",
                                json!({"error":error.to_string()}),
                                now(),
                            ),
                        );
                        observer.turn = None;
                    }
                }
                Err(error)
            }
        }
    }

    fn grok_usage(&self, usage: &Value) {
        let Some(observer) = &self.observer else {
            return;
        };
        let mut samples = Vec::new();
        if let Some(models) = usage["modelUsage"].as_object().filter(|m| !m.is_empty()) {
            for (model, value) in models {
                samples.push((Some(model.clone()), value));
            }
        } else {
            samples.push((observer.model.clone(), usage));
        }
        for (model, value) in samples {
            let input = value["inputTokens"].as_u64();
            let output = value["outputTokens"].as_u64();
            let cached = value["cachedReadTokens"].as_u64();
            let creation = value["cacheCreationTokens"].as_u64();
            let incomplete = usage["usageIsIncomplete"].as_bool().unwrap_or(false);
            let partial_cost = incomplete || value["costIsPartial"].as_bool().unwrap_or(false);
            let sample = actrealm_usage::agents::AgentUsageSample {
                model,
                input_tokens: input.map(|n| {
                    n.saturating_sub(cached.unwrap_or(0))
                        .saturating_sub(creation.unwrap_or(0))
                }),
                output_tokens: output,
                cache_read_tokens: cached,
                cache_creation_tokens: creation,
                reasoning_tokens: value["reasoningTokens"].as_u64(),
                token_total: value["totalTokens"]
                    .as_u64()
                    .or_else(|| input.zip(output).map(|(i, o)| i.saturating_add(o))),
                model_calls: value["modelCalls"].as_u64(),
                cost_usd_micros: if partial_cost {
                    None
                } else {
                    value["costUsdTicks"].as_u64().map(|n| n / 10_000)
                },
                incomplete,
                ..Default::default()
            };
            if sample.token_total.is_some() {
                cache_usage(observer, &self.counter_instance, sample);
            }
        }
    }

    fn frame(&mut self, frame: &Value) {
        let Some(observer) = &mut self.observer else {
            if frame.get("method").is_some() && frame.get("id").is_some() {
                let _ = write_frame(
                    &self.writer,
                    &json!({"jsonrpc":"2.0","id":frame["id"],"error":{"code":-32601,"message":"Client not ready"}}),
                );
            }
            return;
        };
        if frame.get("id").is_some() && frame.get("method").is_some() {
            if let Some(interaction) = observer.interaction(frame, now()) {
                let key = interaction.rpc_id.to_string();
                if !self
                    .answered
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(key)
                {
                    return;
                }
                if self.pending.lock().unwrap_or_else(|p| p.into_inner()).len() >= 16 {
                    let _ = write_frame(
                        &self.writer,
                        &json!({"jsonrpc":"2.0","id":interaction.rpc_id,"error":{"code":-32603,"message":"Too many pending interactions"}}),
                    );
                    return;
                }
                let pending = self.pending.clone();
                let writer = self.writer.clone();
                let mut request = interaction.request;
                let request_id = request.request_id.unwrap_or(request.id);
                pending
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(request_id);
                request.term = Some(provider_term(self.child.id()));
                eprintln!("Waiting for your response in ActRealm.");
                std::thread::spawn(move || {
                    let response = BridgeClient::new(default_socket_path())
                        .send(&request, Duration::from_secs(60 * 60))
                        .ok()
                        .flatten();
                    pending
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .remove(&request_id);
                    let result = interaction.route.response(response.as_ref());
                    let _ = write_frame(
                        &writer,
                        &json!({"jsonrpc":"2.0","id":interaction.rpc_id,"result":result}),
                    );
                });
            } else {
                let _ = write_frame(
                    &self.writer,
                    &json!({"jsonrpc":"2.0","id":frame["id"],"error":{"code":-32601,"message":"Unsupported client method or request shape"}}),
                );
            }
            return;
        }
        for event in observer.notification(frame, now()) {
            if !self.replaying {
                publish(&self.child, event);
            }
        }
        if frame["method"] == "session/update"
            && frame["params"]["sessionId"] == observer.session
            && frame
                .pointer("/params/update/sessionUpdate")
                .and_then(Value::as_str)
                == Some("usage_update")
        {
            let update = &frame["params"]["update"];
            let sample = actrealm_usage::agents::AgentUsageSample {
                model: observer.model.clone(),
                context_tokens: update["used"].as_u64(),
                context_limit: update["size"].as_u64(),
                incomplete: true,
                ..Default::default()
            };
            cache_usage(observer, "context", sample);
        }
        if frame["method"] == "session/update"
            && frame["params"]["sessionId"] == observer.session
            && frame
                .pointer("/params/update/sessionUpdate")
                .and_then(Value::as_str)
                == Some("agent_message_chunk")
        {
            if let Some(text) = frame
                .pointer("/params/update/content/text")
                .and_then(Value::as_str)
            {
                print!("{text}");
                let _ = io::stdout().flush();
            }
        }
    }
}

fn write_frame(writer: &Writer, frame: &Value) -> Result<()> {
    let mut writer = writer
        .lock()
        .map_err(|_| anyhow::anyhow!("Agent input unavailable"))?;
    serde_json::to_writer(&mut *writer, frame)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}
fn provider_term(pid: u32) -> TermContext {
    let mut term = BridgeRequest::from_hook(Provider::Kimi, json!({}))
        .term
        .unwrap_or_default();
    term.provider_pid = Some(pid);
    term.surface = Some("terminal".to_owned());
    term
}
fn publish(child: &Child, mut request: BridgeRequest) {
    request.term = Some(provider_term(child.id()));
    let _ = BridgeClient::new(default_socket_path()).send(&request, Duration::from_millis(200));
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn cache_usage(
    observer: &Observer,
    source: &str,
    sample: actrealm_usage::agents::AgentUsageSample,
) {
    let home = std::env::var_os("ACTREALM_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".actrealm")
        });
    if let Err(error) = actrealm_usage::agents::capture_agent_usage(
        &home,
        &observer.provider.to_string(),
        &observer.session,
        source,
        Some(std::path::Path::new(&observer.cwd)),
        sample,
        now(),
    ) {
        eprintln!("Agent usage could not be recorded: {error}");
    }
}

fn rpc_failure_reason(error: &Value) -> &'static str {
    let message = error["message"].as_str().unwrap_or("").to_ascii_lowercase();
    if message.contains("monthly usage limit")
        || message.contains("quota exceeded")
        || message.contains("insufficient balance")
    {
        "Provider usage limit reached; add usage or configure a funded API account"
    } else if message.contains("authentication required") || message.contains("unauthorized") {
        "Sign in to the official CLI or check its API credentials"
    } else {
        "Check the official CLI account and connection status"
    }
}

fn runtime_home() -> PathBuf {
    std::env::var_os("ACTREALM_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".actrealm")
        })
}

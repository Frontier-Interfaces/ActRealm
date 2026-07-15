use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use flow_agent_bridge::{default_socket_path, BridgeClient, BridgeListener};
use flow_agent_core::{
    permission_deadline_ms, permission_directive, BridgeRequest, Decision, Provider,
    MAX_HOOK_PAYLOAD_BYTES, PERMISSION_COMMIT_DELAY_MS,
};
use flow_agent_runtime::{
    default_database_path, ApprovalAction, EventSpool, RuntimeInstanceGuard, RuntimeStore,
    WaiterRegistry,
};
use flow_agent_server::{ApiServer, ApiServerConfig};
use std::io::{self, BufRead, Read, Write};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "flow-agent",
    version,
    about = "Local-first agent attention runtime"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the local runtime and control panel.
    Serve {
        #[arg(long, value_enum, default_value_t = ApprovalMode::Widget)]
        approval: ApprovalMode,
        #[arg(long)]
        socket: Option<PathBuf>,
        /// Open the one-time authenticated control panel in the default browser.
        #[arg(long)]
        open: bool,
    },
    /// Receive one provider hook payload from stdin and forward it to the runtime.
    Hook {
        #[arg(long)]
        provider: String,
        #[arg(long)]
        socket: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ApprovalMode {
    Widget,
    Prompt,
    Allow,
    Deny,
    PassThrough,
}

enum RuntimeOutcome {
    Decision {
        decision: Decision,
        proposed_at: u64,
    },
    PassThrough(&'static str),
}

enum PromptInput {
    Line(String),
    Closed,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Serve {
            approval,
            socket,
            open,
        } => serve(socket.unwrap_or_else(default_socket_path), approval, open),
        Command::Hook { provider, socket } => {
            // Hook failures must be silent and fail open. Parsing CLI arguments still
            // reports errors because malformed installation is an operator error.
            let provider = Provider::from_str(&provider)?;
            let _ = run_hook(provider, socket.unwrap_or_else(default_socket_path));
            Ok(())
        }
    }
}

fn serve(socket_path: PathBuf, approval: ApprovalMode, open: bool) -> Result<()> {
    let paths = runtime_paths(&socket_path);
    let _instance = RuntimeInstanceGuard::acquire(&paths.lock)
        .with_context(|| format!("failed to acquire {}", paths.lock.display()))?;
    let store = RuntimeStore::open(&paths.database)
        .with_context(|| format!("failed to open {}", paths.database.display()))?;
    store
        .reconcile_orphaned_approvals(Vec::new(), now_millis())
        .context("failed to reconcile stale approvals")?;
    let spool = EventSpool::new(paths.spool);
    let _ = spool.drain(|request| store.ingest(request).is_ok());
    let listener = BridgeListener::bind(&socket_path)
        .with_context(|| format!("failed to bind {}", socket_path.display()))?;
    let waiters = WaiterRegistry::default();
    let api = if approval == ApprovalMode::Widget || open {
        Some(
            ApiServer::start(
                store.clone(),
                waiters.clone(),
                ApiServerConfig {
                    commit_delay: commit_delay(),
                    ..ApiServerConfig::default()
                },
            )
            .context("failed to start local control API")?,
        )
    } else {
        None
    };
    let mut runtime_output = io::stdout().lock();
    if let Some(api) = api.as_ref() {
        let _ = writeln!(
            runtime_output,
            "Flow Agent control panel: {}",
            api.bootstrap_url()
        );
        if open {
            let _ = std::process::Command::new("open")
                .arg(api.bootstrap_url())
                .spawn();
        }
    }
    let _ = writeln!(
        runtime_output,
        "flow-agent runtime listening on {}",
        socket_path.display()
    );
    let _ = runtime_output.flush();
    drop(runtime_output);
    let prompt_lock = Arc::new(Mutex::new(()));
    let prompt_input = (approval == ApprovalMode::Prompt).then(prompt_input_channel);
    let expiry_waiters = waiters.clone();
    let expiry_store = store.clone();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(2));
        if let Ok(expired) = expiry_waiters.expire_request_ids_at(now_millis()) {
            for request_id in expired {
                let _ = expiry_store.expire_approval(request_id, "deadline", now_millis());
            }
        }
    });

    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let prompt_lock = Arc::clone(&prompt_lock);
        let prompt_input = prompt_input.clone();
        let store = store.clone();
        let waiters = waiters.clone();
        thread::spawn(move || {
            let Ok(request) = BridgeListener::read_request(&mut stream) else {
                return;
            };
            let _ = writeln!(
                io::stdout().lock(),
                "provider={} event={} session={}",
                request.provider,
                request.event_name().unwrap_or("unknown"),
                request.session_id().unwrap_or("unknown")
            );
            let registration = if request.needs_reply {
                let Ok(registration) = waiters.register_at(&request, now_millis()) else {
                    return;
                };
                if let Some(replaced) = registration.replaced_request_id {
                    let _ = store.expire_approval(replaced, "duplicate_replaced", now_millis());
                }
                Some(registration)
            } else {
                None
            };
            if store.ingest(request.clone()).is_err() {
                if let Some(registration) = registration {
                    let request_id = request.request_id.unwrap_or(request.id);
                    let _ = waiters.pass_through(request_id, "runtime_error");
                    if let Ok(response) = registration.ticket.recv_timeout(Duration::from_secs(1)) {
                        let _ = BridgeListener::write_response(&mut stream, &response);
                    }
                }
                return;
            }

            if let Some(registration) = registration {
                if approval == ApprovalMode::Widget {
                    let request_id = request.request_id.unwrap_or(request.id);
                    let wait_for = request
                        .deadline_at
                        .map(|deadline| {
                            Duration::from_millis(deadline.saturating_sub(now_millis()))
                        })
                        .unwrap_or(Duration::from_millis(200));
                    if let Ok(response) = registration.ticket.recv_timeout(wait_for) {
                        let _ = BridgeListener::write_response(&mut stream, &response);
                    } else {
                        let _ = waiters.pass_through(request_id, "deadline");
                        let _ = store.expire_approval(request_id, "deadline", now_millis());
                    }
                    return;
                }
                let _prompt_guard = prompt_lock.lock().ok();
                let outcome = choose_outcome(approval, prompt_input.as_deref());
                let request_id = request.request_id.unwrap_or(request.id);
                let command_id = Uuid::now_v7();
                let resolved = match outcome {
                    RuntimeOutcome::Decision {
                        decision,
                        proposed_at,
                    } => {
                        let action = if decision == Decision::Allow {
                            ApprovalAction::Approve
                        } else {
                            ApprovalAction::Deny
                        };
                        store
                            .claim_approval(command_id, request_id, action, proposed_at)
                            .and_then(|_| {
                                store.commit(
                                    command_id,
                                    proposed_at.saturating_add(PERMISSION_COMMIT_DELAY_MS),
                                    true,
                                )
                            })
                            .is_ok()
                            && waiters.decide(request_id, decision).is_ok()
                    }
                    RuntimeOutcome::PassThrough(reason) => {
                        store
                            .claim_approval(
                                command_id,
                                request_id,
                                ApprovalAction::PassThrough,
                                now_millis(),
                            )
                            .is_ok()
                            && waiters.pass_through(request_id, reason).is_ok()
                    }
                };
                if !resolved {
                    let _ = waiters.pass_through(request_id, "runtime_error");
                }
                if let Ok(response) = registration.ticket.recv_timeout(Duration::from_secs(1)) {
                    let _ = BridgeListener::write_response(&mut stream, &response);
                }
            }
        });
    }
    Ok(())
}

fn prompt_input_channel() -> Arc<Mutex<mpsc::Receiver<PromptInput>>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(line) => {
                    if sender.send(PromptInput::Line(line)).is_err() {
                        return;
                    }
                }
                Err(_) => {
                    let _ = sender.send(PromptInput::Closed);
                    return;
                }
            }
        }
        let _ = sender.send(PromptInput::Closed);
    });
    Arc::new(Mutex::new(receiver))
}

fn choose_outcome(
    mode: ApprovalMode,
    prompt_input: Option<&Mutex<mpsc::Receiver<PromptInput>>>,
) -> RuntimeOutcome {
    match mode {
        ApprovalMode::Widget => RuntimeOutcome::PassThrough("invalid_widget_dispatch"),
        ApprovalMode::Allow => delayed_decision(Decision::Allow),
        ApprovalMode::Deny => delayed_decision(Decision::Deny),
        ApprovalMode::PassThrough => RuntimeOutcome::PassThrough("user"),
        ApprovalMode::Prompt => {
            let Some(receiver) = prompt_input.and_then(|input| input.lock().ok()) else {
                return RuntimeOutcome::PassThrough("stdin_error");
            };
            loop {
                eprint!("Approve this request? [y/N/t=terminal] ");
                let _ = io::stderr().flush();
                let answer = match receiver.recv() {
                    Ok(PromptInput::Line(answer)) => answer,
                    Ok(PromptInput::Closed) | Err(_) => {
                        return RuntimeOutcome::PassThrough("stdin_closed")
                    }
                };
                let decision = match answer.trim().to_ascii_lowercase().as_str() {
                    "y" | "yes" => Some(Decision::Allow),
                    "" | "n" | "no" => Some(Decision::Deny),
                    "t" | "terminal" | "p" | "pass" => return RuntimeOutcome::PassThrough("user"),
                    _ => None,
                };
                let Some(decision) = decision else { continue };
                let proposed_at = now_millis();
                eprintln!("Decision pending for 3 seconds; type u then Enter to undo.");
                if undo_requested(&receiver, commit_delay()) {
                    eprintln!("Decision undone.");
                    continue;
                }
                return RuntimeOutcome::Decision {
                    decision,
                    proposed_at,
                };
            }
        }
    }
}

fn delayed_decision(decision: Decision) -> RuntimeOutcome {
    let proposed_at = now_millis();
    thread::sleep(commit_delay());
    RuntimeOutcome::Decision {
        decision,
        proposed_at,
    }
}

fn commit_delay() -> Duration {
    std::env::var("FLOW_AGENT_COMMIT_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_secs(3))
}

fn undo_requested(receiver: &mpsc::Receiver<PromptInput>, timeout: Duration) -> bool {
    if timeout.is_zero() {
        return false;
    }
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        match receiver.recv_timeout(remaining) {
            Ok(PromptInput::Line(answer)) if answer.trim().eq_ignore_ascii_case("u") => {
                return true
            }
            Ok(PromptInput::Line(_)) => {}
            Ok(PromptInput::Closed) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                thread::sleep(deadline.saturating_duration_since(Instant::now()));
                return false;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => return false,
        }
    }
}

fn run_hook(provider: Provider, socket_path: PathBuf) -> Result<()> {
    if std::env::var("FLOW_AGENT_SKIP_HOOKS").as_deref() == Ok("1") {
        return Ok(());
    }
    let input = read_hook_input()?;
    let raw = serde_json::from_slice(&input)?;
    let request = BridgeRequest::from_hook(provider, raw);
    let timeout = if request.needs_reply {
        reply_timeout(provider)
    } else {
        Duration::from_millis(200)
    };

    let response = match BridgeClient::new(socket_path).send(&request, timeout) {
        Ok(response) => response,
        Err(_) => {
            if !request.needs_reply {
                let _ = EventSpool::default().append(&request);
            }
            return Ok(());
        }
    };
    let Some(response) = response else {
        return Ok(());
    };
    let Some(decision) = response.decision() else {
        return Ok(());
    };
    if let Some(directive) = permission_directive(provider, decision) {
        serde_json::to_writer(io::stdout(), &directive)?;
        println!();
    }
    Ok(())
}

fn read_hook_input() -> Result<Vec<u8>> {
    let Some(deadline) = Instant::now().checked_add(stdin_timeout()) else {
        anyhow::bail!("invalid hook stdin deadline");
    };
    let stdin = io::stdin();
    let mut handle = stdin.lock();
    let mut input = Vec::new();
    let mut chunk = [0_u8; 8 * 1024];

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            anyhow::bail!("hook stdin deadline exceeded");
        }
        let timeout_ms = remaining.as_millis().max(1).min(i32::MAX as u128) as i32;
        let mut descriptor = libc::pollfd {
            fd: handle.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll receives one live stdin descriptor and does not retain
        // the pointer after returning.
        let ready = unsafe { libc::poll(&mut descriptor, 1, timeout_ms) };
        if ready == 0 {
            anyhow::bail!("hook stdin deadline exceeded");
        }
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        if descriptor.revents & libc::POLLNVAL != 0 {
            anyhow::bail!("hook stdin is unavailable");
        }

        match handle.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                input.extend_from_slice(&chunk[..count]);
                if input.len() > MAX_HOOK_PAYLOAD_BYTES {
                    anyhow::bail!("hook payload exceeds {} bytes", MAX_HOOK_PAYLOAD_BYTES);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(input)
}

fn stdin_timeout() -> Duration {
    std::env::var("FLOW_AGENT_STDIN_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_secs(5))
}

fn reply_timeout(provider: Provider) -> Duration {
    if let Ok(value) = std::env::var("FLOW_AGENT_HOOK_REPLY_TIMEOUT_MS") {
        if let Ok(milliseconds) = value.parse::<u64>() {
            return Duration::from_millis(milliseconds);
        }
    }
    default_reply_timeout(provider)
}

fn default_reply_timeout(provider: Provider) -> Duration {
    permission_deadline_ms(provider)
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_millis(200))
}

struct RuntimePaths {
    database: PathBuf,
    spool: PathBuf,
    lock: PathBuf,
}

fn runtime_paths(socket_path: &std::path::Path) -> RuntimePaths {
    if socket_path == default_socket_path() {
        let database = default_database_path();
        let root = database
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf();
        return RuntimePaths {
            database,
            spool: root.join("spool"),
            lock: root.join("run/runtime.lock"),
        };
    }
    RuntimePaths {
        database: socket_path.with_extension("sqlite"),
        spool: socket_path.with_extension("spool"),
        lock: socket_path.with_extension("lock"),
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p0_permission_deadlines_allow_real_human_response_time() {
        assert_eq!(
            default_reply_timeout(Provider::Claude),
            Duration::from_secs(24 * 60 * 60)
        );
        assert_eq!(
            default_reply_timeout(Provider::Codex),
            Duration::from_secs(60 * 60)
        );
        assert_eq!(
            default_reply_timeout(Provider::Gemini),
            Duration::from_millis(200)
        );
    }
}

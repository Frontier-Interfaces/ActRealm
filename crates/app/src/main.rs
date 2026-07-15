use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use flow_agent_bridge::{default_socket_path, BridgeClient, BridgeListener};
use flow_agent_core::{
    permission_deadline_ms, permission_directive, BridgeRequest, BridgeResponse, Decision,
    Provider, MAX_HOOK_PAYLOAD_BYTES,
};
use std::io::{self, BufRead, Read, Write};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

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
    /// Run the local M0 bridge runtime.
    Serve {
        #[arg(long, value_enum, default_value_t = ApprovalMode::Prompt)]
        approval: ApprovalMode,
        #[arg(long)]
        socket: Option<PathBuf>,
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
    Prompt,
    Allow,
    Deny,
    PassThrough,
}

enum RuntimeOutcome {
    Decision(Decision),
    PassThrough(&'static str),
}

enum PromptInput {
    Line(String),
    Closed,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Serve { approval, socket } => {
            serve(socket.unwrap_or_else(default_socket_path), approval)
        }
        Command::Hook { provider, socket } => {
            // Hook failures must be silent and fail open. Parsing CLI arguments still
            // reports errors because malformed installation is an operator error.
            let provider = Provider::from_str(&provider)?;
            let _ = run_hook(provider, socket.unwrap_or_else(default_socket_path));
            Ok(())
        }
    }
}

fn serve(socket_path: PathBuf, approval: ApprovalMode) -> Result<()> {
    let listener = BridgeListener::bind(&socket_path)
        .with_context(|| format!("failed to bind {}", socket_path.display()))?;
    println!(
        "flow-agent M0 bridge listening on {}",
        socket_path.display()
    );
    let prompt_lock = Arc::new(Mutex::new(()));
    let prompt_input = (approval == ApprovalMode::Prompt).then(prompt_input_channel);

    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let prompt_lock = Arc::clone(&prompt_lock);
        let prompt_input = prompt_input.clone();
        thread::spawn(move || {
            let Ok(request) = BridgeListener::read_request(&mut stream) else {
                return;
            };
            println!(
                "provider={} event={} session={}",
                request.provider,
                request.event_name().unwrap_or("unknown"),
                request.session_id().unwrap_or("unknown")
            );

            if request.needs_reply {
                let _prompt_guard = prompt_lock.lock().ok();
                let outcome = choose_outcome(approval, prompt_input.as_deref());
                let request_id = request.request_id.unwrap_or(request.id);
                let response = match outcome {
                    RuntimeOutcome::Decision(decision) => {
                        BridgeResponse::decided(request_id, decision)
                    }
                    RuntimeOutcome::PassThrough(reason) => {
                        BridgeResponse::pass_through(request_id, reason)
                    }
                };
                let _ = BridgeListener::write_response(&mut stream, &response);
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
                eprintln!("Decision pending for 3 seconds; type u then Enter to undo.");
                if undo_requested(&receiver, commit_delay()) {
                    eprintln!("Decision undone.");
                    continue;
                }
                return RuntimeOutcome::Decision(decision);
            }
        }
    }
}

fn delayed_decision(decision: Decision) -> RuntimeOutcome {
    thread::sleep(commit_delay());
    RuntimeOutcome::Decision(decision)
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

    let Some(response) = BridgeClient::new(socket_path).send(&request, timeout)? else {
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

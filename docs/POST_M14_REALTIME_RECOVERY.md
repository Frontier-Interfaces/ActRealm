# Post-M14 live-state and Runtime-recovery verification

Last reviewed: 2026-07-20

This post-M14 refinement fixes two observable failures without changing the
Provider control boundary:

1. an open panel could look connected while task state and elapsed time stopped
   changing; and
2. recovering the local Runtime and Hook socket required separate manual steps.

It is not M15. M15 remains reserved for version-gated managed Codex app-server
approval methods.

## Delivered behavior

### Live channel

- Runtime emits a lightweight WebSocket heartbeat every 10 seconds.
- A visible client treats 25 seconds without any WebSocket frame as stale and
  reconnects with capped exponential backoff.
- A visible client performs a bounded authenticated snapshot fallback after 15
  seconds without a rendered snapshot.
- Returning to the foreground immediately rechecks the channel; background tabs
  do not create continuous fallback traffic.

### Stable rendering

- Task, Attention, and quota sections use deterministic render signatures.
- Unchanged rows retain their DOM nodes, focus, selection, scroll position, and
  animation state.
- Live elapsed text is updated in place once per second.
- Real event changes still render immediately through the Runtime snapshot.

### Health monitor

The authenticated `/api/v1/runtime/status` response is rendered under
“通知与数据”. It contains only local operational metadata:

- Runtime PID, version, instance ID, and uptime;
- API and WebSocket state;
- Hook socket type/permissions and last event age;
- active-session and pending-Attention counts;
- SQLite event count; and
- controlled restart count/result.

It does not add telemetry, outbound networking, prompt content, commands, or
transcripts.

### Controlled restart

The single “重启 Runtime” action:

1. validates an authenticated, CSRF-protected, one-time restart token;
2. warns when a replyable request is waiting;
3. passes through active waiters and reconciles non-restorable approvals;
4. writes a mode-`0600` one-time restart state;
5. re-execs the current binary and rebinds the same loopback API port;
6. recreates the private `bridge.sock`;
7. restores durable sessions/events from SQLite;
8. rotates browser authentication using the one-time token; and
9. reconnects the existing page and refreshes setup/settings state.

If the process has already stopped, the browser cannot launch a local executable;
the truthful fallback remains `~/.actrealm/bin/actrealm serve --open`.

## Automated coverage

Focused coverage includes:

- authenticated health and strict restart-input validation;
- WebSocket heartbeat delivery;
- browser restart/reconnect contract and absence of a separate Hook-reconnect
  control;
- five consecutive same-port process replacements;
- Hook fail-open during restart;
- post-restart control-loop recovery, SQLite continuity, and socket mode `0600`;
- timer-only updates without full-card replacement; and
- two-minute idle resource sampling.

## Verification status

The ActRealm-renamed merged tree passed on 2026-07-20:

- `cargo fmt --all -- --check`;
- zero-warning workspace/all-target Clippy;
- 161 Rust tests passed, with the two explicit manual/release tests ignored;
- 30 macOS client tests passed using SwiftPM's no-nested-sandbox mode;
- JavaScript syntax, ActRealm language, and whitespace/diff checks;
- optimized workspace release build;
- real local API/WebSocket widget flow with five consecutive same-port Runtime
  restarts; and
- the explicit 120-second release resource gate: 118 samples, 0.000% average
  idle CPU, and 6,128 KiB maximum Runtime RSS against an 81,920 KiB budget.

The standalone shell replay could not directly create its temporary Unix Socket
inside the automation sandbox (`Operation not permitted`). Its Claude allow,
Codex deny, pass-through, missing-Runtime fail-open, Socket, and process-restart
contracts are covered by the passing Rust integration and end-to-end suites.

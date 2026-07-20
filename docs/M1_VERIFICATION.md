# M1 verification record

Status: **PASS** on 2026-07-15 (Asia/Shanghai).

Branch: `agent/v1-full`

Baseline: `d5b89081ff5b56066a1ea76e0cd048e5fccc484d`

Environment: macOS 26.5.2 (25F84), rustc 1.97.0, cargo 1.97.0.

## Delivered runtime core

- `actrealm-runtime` owns SQLite, the single writer thread, schema v1,
  transactional ingestion/commands, waiter registry, event spool, risk preview,
  liveness reconciliation, and the process lock.
- SQLite is created with mode 0600, WAL, foreign keys, and a single writer.
  Raw hook JSON, full prompts, and full commands are not stored.
- Event envelopes are idempotent by event ID. Session, turn, event, attention,
  and command changes occur in one transaction per business action.
- Approval commands implement open → committing → decision_sent, three-second
  undo, immediate pass-through, stale-waiter rejection, and later evidence
  confirmation. Concurrent approve/deny/pass-through has exactly one winner.
- Waiters are memory-only and keyed by ActRealm request ID plus a provider
  correlation key and in-memory tool-input fingerprint. Exact duplicates pass
  the older waiter through; distinct commands in one turn remain independent.
- Client write-side half-close remains a live response channel. Deadline,
  runtime shutdown, and true response failure fail open without auto-deny.
- Non-permission events use an atomic 0700/0600 spool limited to 500 files and
  5 MB. Permission requests are rejected by the spool API.
- Runtime restart expires persisted approvals without an active waiter. Stop is
  turn-end; the liveness reconciliation interface marks inactive sessions idle
  only after its grace period and never while an approval is pending.
- A non-blocking file lock prevents a second Runtime from replacing the live
  instance or deleting its socket.

Actual provider process discovery is intentionally owned by M3. M1 supplies
and tests the liveness reconciliation contract it will call.

## Automated gate

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | PASS, zero warnings |
| `cargo test --workspace --offline` | PASS, 47 tests, zero failures |
| `cargo build --workspace --release --offline` | PASS |
| `./scripts/m0-e2e.sh` | PASS after Runtime integration: Claude allow, Codex deny, pass-through, missing-runtime fail-open |

The 47-test total includes 14 M1 runtime contract tests, three M1 app-level
cross-process tests, and all M0/provider regressions. M1 cross-process coverage
proves offline spool replay exactly once, duplicate permission replacement,
single-instance socket preservation, and real Hook-to-Runtime persistence.

## Defects found before acceptance

1. A normal late PostToolUse after Stop could create a synthetic new turn and
   appear to confirm old work. Terminal sessions now attach late evidence to
   the prior turn without reviving or confirming it.
2. Session/turn/tool name alone could falsely deduplicate two different Bash
   commands. The in-memory correlation key now includes a tool-input
   fingerprint; no full input is persisted.
3. The first cross-process test path exceeded macOS Unix Socket `SUN_LEN`.
   Tests now use a short path, and overlong socket-path diagnosis is a binding
   M3 `doctor` requirement.
4. The writer message enum placed full event envelopes inline. Envelopes are
   boxed to keep queue messages small.
5. Recursive directory creation could leave an intermediate `~/.actrealm`
   directory governed by the user's umask. Newly created Runtime directories
   now use mode 0700 from their first filesystem operation; database, lock,
   spool files, and the socket remain 0600.

## Gate decision

M1 meets every repository acceptance criterion and may be committed and
pushed. This does not claim the M2 HTTP/WebSocket/UI surface, M3 provider
installation and discovery, M4 quota/settings, or M5 release readiness.

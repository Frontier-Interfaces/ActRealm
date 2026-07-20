# M10-M12 verification record

Status: implementation, milestone-specific tests, browser QA, full workspace
tests, release build, and the exact two-minute resource gate pass locally. The
exact candidate is installed, Doctor reports overall PASS, all four local
Provider surfaces emitted events, and the user accepted the candidate and
authorized commit/push on 2026-07-17.

## M10 - configurable safe display

- Three persisted profiles: concise, detailed, developer.
- Server-owned field allowlist and task-card field selector.
- Safe detail drawer; no raw Hook payload endpoint or renderer.
- Unknown, raw, payload, full-command, and transcript fields are rejected.
- Legacy settings migrate to the detailed default without data loss.

Targeted evidence:

```text
cargo test -p actrealm-server m10_display_settings_migrate_and_reject_fields_outside_the_safe_catalog --offline
PASS
node --check web/app.js
PASS
```

Browser evidence:

- detailed is the persisted default and renders the safe task fields;
- developer enables and persists all five developer-only fields across reload;
- concise selects only task and live activity;
- the browser catalog contains no `raw`, `payload`, `fullCommand`, or
  `transcript` option;
- the seeded Claude question form, task rows, recovery labels, and detail
  entry render without exposing the original Hook object.

## M11 - direct question answers

- Claude AskUserQuestion: choice, multi-select, free input and exact
  PreToolUse `updatedInput.answers` response.
- Claude Elicitation: typed accept/decline/cancel response.
- Secret fields: password UI, bounded validation, memory-only waiter, no
  persistence/export/diagnostic content.
- Expired waiter and invalid answer rejection.

Targeted evidence:

```text
cargo test -p actrealm-core -p actrealm-runtime -p actrealm-server -p actrealm --offline
PASS
```

## M12 - Codex Connector and recovery

- Official private Unix-Socket app-server/proxy JSONL initialization with
  `experimentalApi`, compatible with the desktop-bundled Codex executable and
  not dependent on the standalone-only `daemon start` command.
- `thread/list`, explicit `thread/resume`, managed Thread persistence.
- `item/tool/requestUserInput` and ToolRequestUserInputResponse routing.
- External parent-process liveness and honest five-state recovery UI.
- Old approvals/questions expire across Runtime restart; only a fresh Provider
  request creates a new waiter.

Targeted evidence:

```text
cargo test -p actrealm-codex-connector -p actrealm-runtime -p actrealm-server --offline
PASS
```

The Connector test uses a deterministic local protocol process and verifies
initialize, list, resume, a server-originated request, and the matching client
response without network access.

The current desktop-bundled Codex 0.144.5 executable also completed real
private app-server initialization and `thread/list`; ActRealm no longer calls
the standalone-install-only `app-server daemon start` command.

## Release evidence

- zero-warning workspace Clippy: PASS;
- workspace suite: PASS, 140 tests plus two explicit ignored/manual gates;
- release workspace build: PASS;
- JavaScript syntax check: PASS;
- five-round Claude/Codex Widget control replay: PASS 5/5;
- exact two-minute CPU/RSS gate: PASS, 117 samples, CPU 0.000%, RSS peak
  5,792 KiB against 0.5% and 81,920 KiB budgets;
- exact candidate installation: PASS; installed binary SHA-256
  `f7ecc89bc6913a0849a9260492538f5292d7af3656f4cd7180b3d7d4e87844d2`,
  Doctor overall PASS, control loop PASS, and both Claude/Codex post-install
  real-event checks PASS;
- post-install local source aggregation confirms Claude app, Claude Terminal,
  Codex app, and Codex Terminal sessions all reached the same Runtime;
- user acceptance: PASS on 2026-07-17;
- explicit commit/push authorization: GRANTED on 2026-07-17.

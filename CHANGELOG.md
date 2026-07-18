# Changelog

All notable Flow Agent changes are recorded here. The project has not yet
published a final v1 release; entries below describe development milestones on
`agent/v1-full`, not released packages.

## Unreleased - M14 accepted candidate

### M14 - Live usage, context, price, and OAuth quota

- Incrementally tails Claude transcript and Codex rollout usage with bounded
  parsing and cross-file/stream de-duplication.
- Separates session cumulative Token, latest-turn Token, cached/reasoning
  breakdowns, and current-context occupancy.
- Adds explicitly labelled estimated API price with unknown-model omission.
- Adds background Anthropic OAuth usage refresh with dynamic scoped limits,
  Fable/extra-usage support, one-minute cadence, and StatusLine fallback.
- Keeps OAuth credentials memory-only and keeps the last validated quota value
  visible with its factual capture age.

The 161-test workspace suite, zero-warning Clippy, release build, and explicit
two-minute resource gate pass. The exact release was installed locally with a
matching SHA-256, schema 7 and live OAuth/session records were verified, and
the user accepted the candidate and authorized this commit. A branch push still
requires separate authorization.

### M13 - Provider-owned approval-state coordination

- Detects Provider-owned Codex review and avoids creating a competing Flow
  Agent waiter.
- Tracks native `request_permissions` and managed `waitingOnApproval` states.
- Clears native waiting only on an explicit Provider resolution signal.
- Distinguishes observation-only native approval from Flow-controlled approval
  in Attention, task state, notifications, and available actions.
- Keeps approval outcome neutral when the Provider does not expose it.

Automated and local gates passed. Real-Provider manual acceptance remains
pending. Commit: `311306d`.

### M10-M12 - Safe display, questions, Connector, and recovery

- Adds concise, detailed, and developer task-card profiles using a server-owned
  safe field allowlist.
- Adds Claude AskUserQuestion and Elicitation forms with memory-only secret
  handling.
- Adds the explicit Codex app-server Connector for `requestUserInput`, managed
  Thread attach/resume, and truthful restart recovery states.
- Never restores an old approval/question waiter across Runtime restart.

Commit: `ba2f328`.

### M6-M9 - v1.1 functional corrections

- Keeps the live task list to active, attention-bearing, or recently active
  sessions and links Attention to its task card.
- Renders all valid quota windows, preserves the last valid sample, and shows
  factual total-turn/current-phase timing.
- Supports desktop-only Claude/Codex installations without requiring a global
  CLI, while retaining Codex's user-controlled trust step.
- Reconciles Provider-handled attention, adds safe ignore, and exposes honest
  jump/recovery capabilities.
- Uses Provider conversation titles, bounded current-question summaries,
  model-only third lines, and recognizable Provider icons.

Primary commits: `6b7c465`, `63c6fce`, and `120e89d`.

### M5 - Release hardening candidate

- Adds privacy-bounded diagnostics, aggregate metrics, export, security tests,
  performance checks, and pass-through coverage.
- Keeps raw Hook bodies, prompts, commands, paths, and tokens out of default
  logs and aggregate exports.

The two-minute resource gates pass; the continuous 48-hour Runtime RSS gate is
still pending, so this is not a final v1 release.

### M4 - Honest quota and local controls

- Adds bounded Claude/Codex quota adapters, unavailable/stale states,
  notification and retention settings, local export, and destructive clear.

Commit: `c739355`.

### M3 - Safe Provider onboarding

- Adds backup-preserving Hook installation/uninstallation, onboarding, Codex
  trust guidance, repair state, and Doctor diagnostics.

Commit: `bd15994`.

### M2 - Authenticated local control panel

- Adds authenticated localhost API/WebSocket transport and the fixed
  three-module Attention, Agent task, and Quota interface.

Commit: `bb68922`.

### M1 - Persistent Runtime core

- Adds SQLite/WAL persistence, session state, request-keyed waiters, bounded
  event spool, single-instance coordination, and restart-safe expiration.

Commit: `87868fc`.

### M0 - Provider control-path proof

- Verifies Claude and Codex Hook ingestion, socket wait/reply, allow, deny,
  pass-through, and fail-open behavior with versioned fixtures.

Commit: `d23c27b`.

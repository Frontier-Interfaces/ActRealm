# Changelog

All notable ActRealm changes are recorded here. The project has not yet
published a final v1 release; entries below describe development milestones on
`agent/v1-full`, not released packages.

## Unreleased - M14 accepted candidate

### Post-M14 - Codex internal-session filtering

- Discards Codex App overview-suggestion and safety-review background sessions
  at Runtime ingest instead of storing or displaying them as Agent tasks.
- Removes the provisional `SessionStart` row and metric when a later prompt
  identifies an internal session, then suppresses its remaining lifecycle
  across Runtime restarts without adding a session visibility state.

### Post-M14 - Usage, pricing, and OAuth hardening candidate

- Adds a versioned per-model price registry with distinct `provider_estimate`
  and `computed` kinds plus dated `models.dev`, OpenAI standard-price, and
  compatibility-fallback source labels.
- Expands computed price coverage to validated Claude and Codex models while
  preserving exact cached-input/cache-write semantics and unknown-model
  omission. Codex model changes price each cumulative delta at the model active
  for that event instead of repricing earlier usage.
- Separates current desktop-picker coverage from historical rollout support:
  Claude's visible Fable/Opus/Sonnet/Haiku choices and Codex GPT-5.6 plus
  GPT-5.5 use dated first-party rates; GPT-5.4 and older remain history-only.
- Carries the structured transcript/rollout model into SQLite schema 8 and
  uses it only when the Hook session model is absent, eliminating cards that
  showed an unknown model beside a model-derived price.
- Treats zero-token Claude `<synthetic>` rows as zero-cost metadata, so they
  neither replace the real session model nor suppress a complete estimate.
- Covers the locally observed `claude-sonnet-5` with Anthropic's dated
  introductory API rate and cache multipliers; the source label makes the
  promotion's August 31, 2026 boundary explicit rather than treating it as a
  timeless price.
- Parses Claude OAuth expiry metadata and delegates near-expiry/401 recovery to
  one bounded official `claude auth status --json` invocation; ActRealm never
  owns or persists refresh tokens.
- Tries the fixed Claude Keychain service and a cached successful locator before
  a size-bounded, five-minute-cached service enumeration fallback.
- Replaces full-history Claude re-aggregation with O(1) accumulators and a
  bounded 256-entry correction window. A 10,000-entry regression preserves
  exact Token totals and computed price.

Focused quota/usage tests, the 172-test workspace suite, zero-warning Clippy,
release build, language contract, and the two-minute resource gate pass. Exact
release installation, local user acceptance, commit, and push remain
separately gated.

### Post-M14 - Live state and controlled Runtime recovery

- Adds WebSocket heartbeats, stale-connection detection, bounded snapshot
  fallback, and visibility-aware reconnect behavior so a visually open panel
  does not silently stop receiving state.
- Keeps task and Attention cards structurally stable: elapsed time and activity
  text update in place, while unchanged rows are not recreated when a new
  event arrives. This removes visible card jitter during active work.
- Adds an authenticated local health monitor for the Runtime, API, WebSocket,
  Hook socket, latest Hook event, active tasks, pending Attention, SQLite event
  count, and restart history.
- Adds one controlled `重启 Runtime` action. It safely releases active waiters,
  re-execs the same binary on the same loopback port, recreates the Hook socket,
  restores durable session state, rotates browser authentication, and reconnects
  the page automatically.
- Keeps crash/offline behavior truthful: the browser cannot relaunch a process
  that is no longer running and falls back to the documented terminal command.

This work is separate from M15, which remains reserved for managed Codex
app-server approval methods.

### Post-M14 - Single-toolbar visual refinement

- Removes the decorative macOS menu bar and red/yellow/green traffic lights.
- Keeps one in-page ActRealm toolbar with the brand on the left and
  Notification & Data, local time, and truthful Runtime state on the right.
- Adds an embedded-UI regression contract so the removed chrome cannot return
  accidentally. The focused UI test, JavaScript syntax check, format/diff
  checks, and workspace release build pass; the user accepted the visual result
  and authorized the local commit.

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

- Detects Provider-owned Codex review and avoids creating a competing ActRealm
  waiter.
- Tracks native `request_permissions` and managed `waitingOnApproval` states.
- Clears native waiting only on an explicit Provider resolution signal.
- Distinguishes observation-only native approval from ActRealm-controlled approval
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

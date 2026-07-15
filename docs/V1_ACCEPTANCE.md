# Flow Agent v1 delivery contract

Source baseline: `WIDGET_V1_PLAN.md` v1.1 dated 2026-07-15, amended by the
M0 evidence in `REFERENCE_REVIEW.md` and `M0_PROVIDER_REPORT.md`. This
repository file keeps the milestone gates next to the implementation; it does
not weaken or replace the full plan.

## Release shape

- One Rust binary, `flow-agent`.
- macOS-first local runtime plus embedded 1600x600 native web UI.
- Three modules only: Attention, Agent sessions, and Quota.
- Claude Code and Codex CLI are P0. Gemini round-level observation is P1 and
  cannot block v1 release.
- External Hook Control handles each official permission request through
  approve, deny, or pass-through. Multiple sessions may wait concurrently, but
  every waiter is request-keyed and receives exactly one outcome. Managed
  sessions, reply, cancel, interrupt, steer, Coach, cloud accounts, and
  telemetry are outside v1.

## Milestone gates

### M0 - provider control path

- [x] Reference review records exact revisions, licenses, adopted decisions,
      rejected patterns, and milestone ownership.
- [x] Versioned, sanitized Claude and Codex fixtures from real probes.
- [x] Session, prompt, tool, permission, and stop events form basic state.
- [x] Real allow and deny produce subsequent provider evidence.
- [x] Undo before three seconds writes no provider decision.
- [x] Manual pass-through restores the native provider prompt.
- [x] Injected deadline pass-through works.
- [x] Claude uses a 24-hour human-approval budget and Codex uses 1 hour;
      automated tests inject short deadlines.
- [x] Hook stdin that never closes fails open within its 5-second budget.
- [x] Missing runtime completes within 200ms with empty stdout.
- [x] Killing the runtime while waiting immediately returns native control.
- [x] Another Codex hook denial cannot become a false confirmation.
- [x] Untrusted Codex hooks show not connected; a trusted probe connects.
- [x] Capability matrix and integration boundary report are current.

### M1 - runtime core

- [x] Core state machine and attention rules are fixture-tested.
- [x] SQLite WAL storage uses one writer and transactional business actions.
- [x] Envelope replay is idempotent.
- [x] Approve/deny/pass-through races have one winner.
- [x] Waiters are memory-only and stale approvals expire after restart.
- [x] Concurrent waiters are keyed by request/correlation ID, and duplicate
      requests resolve an older waiter without leaking its decision.
- [x] Socket half-close is not treated as user disconnect or auto-deny.
- [x] Stop remains turn-end; process liveness reconciles sessions that emit no
      terminal session event.
- [x] Runtime is single-instance; non-permission spool is bounded and replayed.

### M2 - API and minimum UI

- [ ] Authenticated localhost API and WebSocket snapshot are implemented.
- [ ] The fixed three-column 1600x600 UI has no fake data.
- [ ] Attention supports approve, deny, undo, pass-through, ack, and snooze.
- [ ] UI distinguishes pending, sent, confirmed, passed-through, and expired.
- [ ] Real Claude and Codex approval paths pass end to end.

### M3 - installation and onboarding

- [ ] Claude and Codex hook installation uses backup, semantic merge, lock,
      temporary file, and atomic rename.
- [ ] Uninstall removes only Flow Agent entries and preserves user semantics.
- [ ] Installation intent is tri-state and repair never recreates intentionally
      removed or uninstalled hooks.
- [ ] Stable hook binary installation, `CODEX_HOME`, canonical/legacy feature
      detection, and Codex trust guidance are implemented.
- [ ] `doctor` reports CLI/version, configuration, runtime, trust/probe state,
      control loop, and pass-through.
- [ ] `doctor` emits structured, repairability-aware issues and refuses to
      mutate malformed provider configuration.
- [ ] `doctor` reports an overlong Unix Socket path before attempting Hook
      installation or Runtime startup.
- [ ] Unknown fields and events are visible and never panic.

### M4 - quota, settings, and P1

- [ ] Claude quota bridge never replaces an existing custom status line.
- [ ] Codex rollout parsing is isolated, version-gated, and read-only.
- [ ] Missing, stale, or incompatible quota data renders an honest unavailable
      state without percentages.
- [ ] Notification, retention, export, and destructive-clear settings work.
- [ ] Gemini round-level observation may ship only if contract-tested.

### M5 - release evidence

- [ ] Local metrics and JSON export match the plan definitions.
- [ ] Oversize/deep JSON, host/origin/CSRF, socket permissions, and redaction
      tests pass.
- [ ] Default logs contain no raw hook payload; diagnostic capture is explicit,
      redacted, bounded, and expires.
- [ ] Hook non-blocking p95 is below 50ms; event-to-UI p95 below 300ms.
- [ ] Idle runtime CPU is below 0.5%; browser tab memory below 150MB.
- [ ] Runtime RSS remains below 80MB throughout a continuous 48-hour soak.
- [ ] Every pass-through path leaves the provider terminal usable.

## Publishing rule

Each milestone is implemented test-first. Only a fully passing milestone gets a
milestone commit and GitHub push. Failed or incomplete milestones remain local
and are never represented as complete in documentation, tags, or releases.

# ActRealm current status

Last reviewed: 2026-07-22

Source branch: `agent/v1-full`

Current functional baseline: M14 plus the post-M14 live-state and controlled
Runtime-recovery refinements on `agent/v1-full`. A subsequent usage/OAuth
hardening candidate has passed its automated/resource gates and awaits exact
local installation and user acceptance. The first-run/setup-center candidate
has passed visual acceptance and its original exact-helper installation. The
company-branch Codex internal-session filter is now merged locally, with the
schema 9 synchronized tree passing its Rust and resource gates.

This file is the short, current source of truth. The
[implementation plan](WIDGET_V1_PLAN.md),
[acceptance contract](V1_ACCEPTANCE.md), and milestone verification records
provide the detailed requirements and evidence.

## Status at a glance

- **Committed functional implementation:** delivered through M14 plus the
  ActRealm design alignment on `agent/v1-full`.
- **P0 release-remediation candidate (uncommitted):** the 2026-07-22 global
  release audit is being addressed on frozen baseline `50b891a`. The candidate
  binds OUTBOX selection to stable Attention IDs, renders Claude questions and
  Elicitation in the native primary card, disables stale controls while the
  Runtime is offline, adds supervised Runtime restart and safe abandoned-child
  takeover, releases sessions atomically when their reply channel ends, moves
  the Codex app-server Connector to directly owned stdio transport, and pauses
  background native projection/animation work while the workspace is hidden.
  Local automated and targeted resource gates are recorded in the P0
  remediation report; remote GitHub CI, Developer ID notarization, clean-Mac
  install, and the continuous 48-hour soak remain external release gates.
- **M14 acceptance:** automated/local gates passed; the exact release was
  installed with a matching SHA-256, live session/OAuth records were verified,
  and the user accepted the candidate and authorized the local commit on
  2026-07-18. Push remains separately gated.
- **Post-M14 UI refinement:** the redundant simulated macOS menu bar and
  traffic lights were removed; Notification & Data, local time, and Runtime
  state now share the single ActRealm toolbar. Focused checks and local visual
  acceptance passed on 2026-07-18; commit was authorized separately from push.
- **Post-M14 live-state refinement:** WebSocket heartbeats, stale-channel
  detection, snapshot fallback, stable in-place timer updates, and render
  signatures keep Agent state current without rebuilding unchanged cards.
- **Post-M14 Runtime recovery:** the settings page exposes authenticated local
  health details and one controlled Runtime restart action. The process
  re-execs on the same loopback port, recreates `bridge.sock`, restores durable
  sessions from SQLite, rotates browser authentication, and safely expires
  non-restorable reply waiters. See the
  [verification record](POST_M14_REALTIME_RECOVERY.md).
- **Post-M14 usage/OAuth hardening candidate:** adds source-labelled Claude and
  Codex computed prices, OAuth expiry preflight plus bounded official-CLI
  delegation, fixed-first cached Keychain lookup, and a bounded incremental
  transcript accumulator. The 172-test workspace suite, zero-warning Clippy,
  release build, language contract, and two-minute resource gate pass; exact
  local-candidate installation and user acceptance are still pending. See the
  [verification record](POST_M14_USAGE_OAUTH_HARDENING.md).
- **Post-M14 first-run/setup candidate:** adds one truthful empty first-run
  workspace and a unified Claude/Codex setup center backed by the existing
  authenticated setup API. Unsupported Provider placeholders are omitted,
  Codex trust remains user-controlled, and the GitHub guide is linked from the
  interface. The isolated 300-second preview and board 6/7 visual acceptance
  passed on 2026-07-20. The stable helper matched that first-run candidate at
  acceptance time. After the company sync, the installed helper remains
  `340bf2f5...d429` while the synchronized release is `d70d2ea8...935b`, so the
  latest binary is not installed. Current Doctor still passes Hook manifests,
  helper execution, the canonical Codex feature, private Socket permissions,
  and silent fail-open, but reports Codex trust review and fresh real Claude/
  Codex events as pending. Push also remains open. See the
  [verification record](POST_M14_FIRST_RUN_ONBOARDING.md).
- **Post-M14 Codex internal-session filtering:** synced from company branch
  commit `43a268d`. Known Codex App overview-suggestion and safety-review
  background sessions are discarded at Runtime ingest; an already-created
  provisional row, its local metric, related events/Attention/usage, and later
  lifecycle are removed without adding a fake visibility state. Ordinary user
  sessions remain visible and suppressed waiters fail open. Schema advances to
  9. The exact synchronized tree passes the focused regression, 177-test Rust
  suite, zero-warning Clippy, release build, language contract, and two-minute
  resource gate. Latest-binary installation and macOS Swift rerun remain open.
  See the
  [verification record](POST_M14_CODEX_INTERNAL_SESSION_FILTERING.md).
- **Post-M14 macOS/Web parity candidate:** the native macOS client now consumes
  the setup, settings, question, session-control, quota, export, clear-data,
  and metrics contracts already used by Web. It adds the unified Agent setup
  center, real interactive-question forms, dynamic session/usage/quota fields,
  and matching local controls while retaining macOS-only foreground
  scheduling. The isolated ActRealmKit suite passes 39 tests. Full SwiftUI
  compilation and visual acceptance remain open because the installed Command
  Line Tools do not include `SwiftUIMacros`; see the
  [verification record](POST_M14_MACOS_WEB_PARITY.md).
- **Post-M14 Provider lifecycle projection candidate:** successful Provider
  turn completion is no longer restricted to write tools; managed Codex plan,
  auto-review, interruption, and collab/sub-Agent events now enter the same
  Runtime projection used by Claude Hooks. The native task detail shows real
  plan steps and active sub-Agent records, and future adapter names receive a
  dynamic lane instead of being dropped. Full Rust, release, language, and
  macOS gates pass; fresh real-Provider visual acceptance remains pending. See the
  [verification record](POST_M14_PROVIDER_LIFECYCLE_PROJECTION.md).
- **Latest synchronized-tree gates:** 177 Rust tests passed, with three
  explicitly manual/resource tests ignored in the ordinary workspace run;
  zero-warning Clippy, format, JavaScript syntax, release build, language
  contract, focused schema-9 regression, and the explicit two-minute resource
  gate passed. The resource sample recorded 0.000% average idle CPU and 6,272
  KiB maximum Runtime RSS. The current macOS Swift rerun was blocked before any
  test assertion by SwiftPM `sandbox_apply: Operation not permitted`; the prior
  30-test result is not represented as a test of this synchronized tree.
- **M13 real-Provider acceptance:** still pending. The milestone was committed
  and pushed at the user's direction before this final manual confirmation.
- **Final v1 release:** not yet declared. The required continuous 48-hour
  Runtime RSS soak remains unchecked.
- **Default branch alignment:** the user separately approved the alignment on
  2026-07-17. `main` was fast-forwarded to the reviewed `agent/v1-full`
  history without restarting Runtime, reinstalling ActRealm, or touching the
  live data directory.
- **Version/tag:** Cargo remains `0.1.0`; no release tag has been created.

## Milestone matrix

| Milestone | Scope | Implementation | Evidence |
| --- | --- | --- | --- |
| M0 | Provider Hook control path | Complete | [M0](M0_VERIFICATION.md) |
| M1 | Persistent Runtime core | Complete | [M1](M1_VERIFICATION.md) |
| M2 | Authenticated API and minimum UI | Complete | [M2](M2_VERIFICATION.md) |
| M3 | Safe install, onboarding, and Doctor | Complete | [M3](M3_VERIFICATION.md) |
| M4 | Quota, settings, and local data controls | Complete | [M4](M4_VERIFICATION.md) |
| M5 | Release hardening and evidence | Partial | [M5](M5_VERIFICATION.md); 48-hour soak pending |
| M6 | Live sessions and Attention linkage | Complete | [M6](M6_VERIFICATION.md) |
| M7 | Dynamic quota and truthful timing | Complete | [M7](M7_VERIFICATION.md) |
| M8 | Desktop compatibility, ignore, jump, and recovery truth | Complete | [M8](M8_VERIFICATION.md) |
| M9 | Provider conversation-title consistency | Complete | [M9](M9_VERIFICATION.md) |
| M10 | Configurable safe display | Complete | [M10-M12](M10_M12_VERIFICATION.md) |
| M11 | Direct Claude questions and secret handling | Complete | [M10-M12](M10_M12_VERIFICATION.md) |
| M12 | Codex Connector and restart recovery | Complete within the recorded boundary | [M10-M12](M10_M12_VERIFICATION.md) |
| M13 | Provider-owned approval-state coordination | Code and automated gates complete; real-Provider acceptance pending | [M13](M13_PROVIDER_STATE_COORDINATION.md) |
| M14 | Live usage, context, price, and OAuth quota | Complete; exact release installed and accepted locally | [M14](M14_USAGE_CONTEXT_QUOTA.md) |
| Post-M14 | Stable live rendering and controlled Runtime recovery | Implemented; merged with the ActRealm identity baseline | [verification](POST_M14_REALTIME_RECOVERY.md) |
| Post-M14 | Usage, pricing, and OAuth hardening | Full automated/resource gates pass; exact local installation and user acceptance pending | [verification](POST_M14_USAGE_OAUTH_HARDENING.md) |
| Post-M14 | First-run workspace and Agent setup center | Visual acceptance passed; real-Provider install/function gates pending | [verification](POST_M14_FIRST_RUN_ONBOARDING.md) |
| Post-M14 | Codex internal-session filtering | Synchronized Rust/resource gates pass; latest installation and macOS Swift rerun pending | [verification](POST_M14_CODEX_INTERNAL_SESSION_FILTERING.md) |
| Post-M14 | macOS/Web feature parity | Native source and focused Kit tests complete; full SwiftUI build and visual/real-Provider acceptance pending | [verification](POST_M14_MACOS_WEB_PARITY.md) |
| Post-M14 | Provider lifecycle projection | Full automated/release/macOS gates pass; real Claude/Codex visual acceptance pending | [verification](POST_M14_PROVIDER_LIFECYCLE_PROJECTION.md) |

M5 is a release-qualification track, not the chronological end of feature
development. Later functional milestones may be implemented while M5's
long-running release gate remains open; that does not make the final v1 release
complete.

## Capability boundary after M14

ActRealm can directly respond only when it owns a live, official reply
channel:

- request-keyed Claude/Codex Hook `PermissionRequest` allow, deny, or
  pass-through;
- Claude `AskUserQuestion` and `Elicitation` Hook replies;
- Codex app-server `item/tool/requestUserInput` after explicit managed attach.

When Codex or Claude exposes an approval only in its own native interface,
ActRealm observes and synchronizes the waiting/resolved state. It must not show
fake allow/deny controls or infer whether the user approved, denied, or ran the
command.

## Later candidate, not implemented

M15 is reserved for version-gated managed Codex app-server approval methods:

- `item/commandExecution/requestApproval`;
- `item/fileChange/requestApproval`;
- `item/permissions/requestApproval`;
- official available-decision rendering and response;
- `serverRequest/resolved` and item completion reconciliation;
- explicit UI capability labels and compatibility tests.

M15 does not promise control of an arbitrary independently running Codex
Desktop conversation. It requires a supported, explicitly attached managed
Thread and must be separately planned, implemented, tested, and approved.

## Remaining release work

1. Complete M13 manual reproduction against the real Provider surfaces and
   record the user's result.
2. Run the new required GitHub core-gate workflow and protect the release
   branch after the P0 remediation candidate is reviewed and pushed.
3. Run Developer ID signing/notarization and clean-macOS-26 installation for
   the exact release artifact; the current support target is Apple Silicon.
4. Run and retain the continuous 48-hour Runtime RSS soak on the exact frozen
   release candidate.
5. Re-run the full release gate after any resulting change.
6. Obtain separate approval before bumping the version, tagging, publishing a
   release, or changing the default branch again.

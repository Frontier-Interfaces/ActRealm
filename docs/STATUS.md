# Flow Agent current status

Last reviewed: 2026-07-18

Source branch: `agent/v1-full`

Current M14 functional commit: `5a29d88`

This file is the short, current source of truth. The
[implementation plan](WIDGET_V1_PLAN.md),
[acceptance contract](V1_ACCEPTANCE.md), and milestone verification records
provide the detailed requirements and evidence.

## Status at a glance

- **Committed functional implementation:** delivered through M14 plus the
  ActRealm design alignment on `agent/v1-full`.
- **M14 acceptance:** automated/local gates passed; the exact release was
  installed with a matching SHA-256, live session/OAuth records were verified,
  and the user accepted the candidate and authorized the local commit on
  2026-07-18. Push remains separately gated.
- **Post-M14 UI refinement:** the redundant simulated macOS menu bar and
  traffic lights were removed; Notification & Data, local time, and Runtime
  state now share the single ActRealm toolbar. Focused checks and local visual
  acceptance passed on 2026-07-18; commit was authorized separately from push.
- **Latest local gates:** 161 workspace tests passed, two explicit/manual
  tests ignored; zero-warning Clippy, format, release build, five-round native
  request replay, and the two-minute resource gate passed. M14's resource
  sample recorded 0.000% average idle CPU and 6,016 KiB maximum Runtime RSS.
- **M13 real-Provider acceptance:** still pending. The milestone was committed
  and pushed at the user's direction before this final manual confirmation.
- **Final v1 release:** not yet declared. The required continuous 48-hour
  Runtime RSS soak remains unchecked.
- **Default branch alignment:** the user separately approved the alignment on
  2026-07-17. `main` was fast-forwarded to the reviewed `agent/v1-full`
  history without restarting Runtime, reinstalling Flow Agent, or touching the
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

M5 is a release-qualification track, not the chronological end of feature
development. Later functional milestones may be implemented while M5's
long-running release gate remains open; that does not make the final v1 release
complete.

## Capability boundary after M14

Flow Agent can directly respond only when it owns a live, official reply
channel:

- request-keyed Claude/Codex Hook `PermissionRequest` allow, deny, or
  pass-through;
- Claude `AskUserQuestion` and `Elicitation` Hook replies;
- Codex app-server `item/tool/requestUserInput` after explicit managed attach.

When Codex or Claude exposes an approval only in its own native interface, Flow
Agent observes and synchronizes the waiting/resolved state. It must not show
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
2. Run and retain the continuous 48-hour Runtime RSS soak on the exact frozen
   release candidate.
3. Re-run the full release gate after any resulting change.
4. Obtain separate approval before bumping the version, tagging, publishing a
   release, or changing the default branch again.

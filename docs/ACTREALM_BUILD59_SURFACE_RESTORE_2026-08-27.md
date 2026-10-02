# ActRealm Build 59 surface restoration

Date: 2026-08-27

Branch: `actrealm最新版`

Reference surface: ActRealm `0.1.0 (59)`, source
`f3e1e3689995432f0d4384f4206cd9fdd7a722e4`.

Current data/runtime baseline: Build 97 source
`b5fc9d998dc9fec6bf4c612d841d6289c9e870f0`.

## Decision

Restore the Build 59 macOS product surface and interaction model while keeping
the following newer subsystems:

1. the Build 97 canonical Token ledger, pricing, project/task attribution and
   verified-data presentation;
2. current Codex internal/fake task filtering;
3. current optimistic local archive lifecycle and rollback on failure;
4. current H7 layered diagnostics;
5. current fail-closed approval safety and Runtime/API compatibility.

This is a surface restoration, not a checkout of the old Runtime or database
schema.

## Restored visible behavior

- the fixed three-column `OUTBOX / AGENT TASKS / QUOTA` workspace;
- an always-present OUTBOX and the Build 59 minimum window width;
- the top-level Join action in place of the later History action;
- Team/Cloud identity, settings, task sharing, comments, plan review, Today and
  notifications;
- local Display Companion pairing, scopes, connection list and revocation;
- full task workflow with bounded paging for earlier events;
- exact tool names alongside semantic tool categories;
- the Build 59 flat expanded-task fact grid and newest-event-first lane order;
- Developer task-card preset and per-field customization;
- static image, GIF and video theme selection;
- local usage statistics and sanitized crash-report controls;
- Build 59 data-export button layout.

## Intentionally not restored

- Build 59 Token collection, partial ledger, pricing projection or attribution;
- the later History center as a top-level product destination;
- ActRealm Review and metadata-only Checkpoint presentation;
- H6 mobile/Watch remote approval and Claude Cowork;
- older permissive approval behavior or obsolete Runtime contracts.

The History implementation and later Review/Checkpoint code remain dormant so
their stored local data is not destructively removed. They have no production
navigation or active worker in this candidate.

## Verification

- `git diff --check`: passed;
- `plutil -lint apps/macos/Resources/Info.plist`: passed;
- focused `MainWindowLayoutTests`: 17/17 passed;
- complete macOS suite: 198 tests across 27 suites passed;
- complete Rust workspace tests and doc tests: passed;
- Clippy with warnings denied and release compilation: passed;
- Runtime language contract: 57 messages, 88 API errors and 56 emitted codes;
- `SnapshotTool`: 28 compiled Chinese snapshots generated;
- visual inspection confirmed Join, fixed three columns, full Workflow, Team,
  Developer/custom display controls and the restored settings navigation.

## Installed candidate

- source commit: `fad854873ff08ea993a8f6c4ba6230e2b51193a1`;
- pushed branch: `origin/actrealm最新版`;
- installed version: ActRealm `0.1.0 (98)`;
- signature: Apple Development, strict deep verification passed;
- architecture: arm64 app and bundled Runtime;
- Doctor: 14/14 passed;
- Computer Use: real installed app confirmed Join, fixed three columns,
  always-present OUTBOX, full paged Workflow, Team navigation, Developer preset
  and per-field customization without acting on any OUTBOX item;
- rollback: Build 97 is recoverable at
  `~/.Trash/ActRealm-build97-before-build98-20260827.app`.

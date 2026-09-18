# ActRealm local-only migration — 2026-09-16

The candidate removes the native cloud module, team/share UI and identity flows,
Firebase Functions/Firestore code and configuration, mobile and Watch clients,
remote context extraction, cloud revocation queues, mobile diagnostics, and
packaging/CI dependencies. Local Agent monitoring, approval, history, usage,
Agent Focus, notifications, and authenticated loopback Companion remain.

## Task state repair

Codex rollout `task_complete` and abort records now enter the ordinary lifecycle
reducer with their exact thread, turn and timestamp. Error text is not retained.
Older timestamps, different turns and duplicate terminal records cannot end a
new turn. Resumed rollout filenames are accepted only after validating their
session metadata. Confirmed process loss moves running work to
`waiting_for_event`, with an event-watermark comparison inside the storage writer.
It does not manufacture a completion or resolve a live human request. Execution
intervals stop at the last verified event; new Provider events restore activity.

Native task derivation also refuses to count an explicitly lost/unconfirmed
source as running. A live `observing` process is not stopped merely because its
current tool runs for a long time.

## Token display

Task cards and expanded details use their own usage record rather than the
completeness of unrelated global history. Partial values remain visible as
observed usage with coverage/source information. Global accounting quality
checks remain intact. Codex's `lastTurnTokens` is labeled latest-call usage,
matching the existing collector semantics.

## Local storage and packaging

Schema 38 drops retired collaboration/context metadata and cloud deletion
queues. Task history and numeric usage are preserved. Packaging has no Firebase
configuration prerequisite and emits no cloud module or Google configuration.
The local package identifies an uncommitted source build via
`ActRealmSourceModified`; its Git field names the base commit only.

Production Firebase resources and account data are outside the repository
migration and have not been deleted. Historical design reports do not describe
current supported capabilities.

## Verification

- Full Rust workspace: 389 tests passed before the final internal-task filter refinement.
- Final Runtime regression suite: 138 tests passed, including the added maintenance-task exclusion; Clippy with warnings denied and release build passed again afterward.
- Native Swift suite: 191 tests passed.
- Rust formatting, product/runtime language contracts, CI pin policy, plist and shell validation passed.
- ActRealm 0.1.0 build 114 installed at `/Applications/ActRealm.app`; strict deep signature verification passed. The package marks its source as modified and has no Firebase configuration, OAuth URL handlers or linked ActRealmCloud module.
- Installed helper, packaged helper and `~/.actrealm/bin/actrealm` have identical SHA-256 hashes.
- Live Runtime is healthy on Companion protocol 6; the existing database migrated to schema 38 with all retired tables absent.
- Real UI changed from seven apparent running tasks to the single current task. Former 166/189-hour entries stopped appearing as active. Codex internal memory maintenance is excluded from the user task list.
- Real task card and detail panel show cumulative and latest-call Token values while global historical collection remains independent.
- Settings contain only General, Agent, Notifications, Theme, Display and Data. Cloud account, Team, mobile approval and crash-upload controls are absent.
- Retired team policy/context and cloud deletion acknowledgement routes return HTTP 404.

Evidence and rollback backups are stored in
`~/Documents/ChatGPT/actrealm/outputs/actrealm-local-only-20260916`.
The original build 113 and a consistent pre-migration SQLite backup are retained.
No commit, push, release publication, Firebase project deletion or remote data deletion was performed.

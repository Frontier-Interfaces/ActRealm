# M8 verification record - desktop compatibility and truthful control

Status: complete on `agent/v1-full` within the capability boundary below.

This record was split retrospectively from the previously published combined
`V1_1_FUNCTIONAL_CORRECTIONS.md` record. It references the original committed
evidence and does not claim an independent historical candidate.

## Delivered contract

- Claude.app and ChatGPT/Codex.app can satisfy Provider discovery without a
  same-name global CLI.
- Codex Desktop exposes its bundled official executable for the user-owned
  `/hooks` trust review; ActRealm never edits or bypasses trust.
- Provider-side progress, denial, stop, failure, new prompt, or session end
  reconciles matching Attention and removes stale `等你` state when the Hook
  supplies a reliable resolution signal.
- Safe ignore hides non-authorization attention; ignoring a replyable
  authorization first returns it to the Provider instead of abandoning a live
  waiter.
- Jump UI exposes exactly one truthful level: exact conversation, matching
  Terminal/iTerm session, application only, or unsupported.
- Restart restores presentation and factual recovery state, not an old
  disconnected reply channel.
- Token/usage fields render only when the Provider supplies real structured
  values.

## Evidence mapping

- Desktop-only discovery and bundled Codex command:
  `crates/installer/src/lib.rs` and
  `crates/installer/tests/m3_installer.rs`.
- Attention/session reconciliation, ignore, jump privacy, liveness, recovery,
  and usage: `crates/runtime/src/storage.rs`,
  `crates/runtime/tests/m1_runtime.rs`, and `crates/server/src/server.rs`.
- Capability labels and actions: `web/app.js`.
- Install/Doctor and end-to-end coverage: `crates/app/tests/`.
- Combined exact-tree gate record: `V1_1_FUNCTIONAL_CORRECTIONS.md`.

Primary implementation commits: `63c6fce` and `120e89d`.

## Capability boundary

M8 does not claim that ActRealm owns an independently running Provider
session. Exact jump requires a verified locator. Restarted external Hook
sessions may be observable but not controllable, and old approval/question
waiters always expire.

## Acceptance result

The recorded desktop-only five-round suite, Doctor checks, Provider event
aggregation, workspace gates, release build, and two-minute resource gate
passed. Later M13 work strengthens native approval-state coordination and has
its own verification record.

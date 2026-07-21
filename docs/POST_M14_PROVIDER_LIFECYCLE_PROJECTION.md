# Post-M14 Provider lifecycle projection

Date: 2026-07-21

This candidate fixes five related projection gaps without expanding ActRealm's
approval authority.

## Implemented facts

- Completion follows a real foreground turn end: Hook `Stop` or managed Codex
  `turn/completed`. It does not depend on a write tool or approval setting.
- Codex `item/autoApprovalReview/*` remains Provider-owned. Approved/denied
  review never appears as a user approval; timed-out/aborted review can surface
  only if the Thread still says it is waiting for the user.
- Managed Codex `turn/plan/updated` and Claude Task Hooks populate bounded plan
  steps. No structured event means no invented plan.
- Claude Subagent Hooks plus Codex collab/sub-Agent items populate active child
  count, type, status, and source. A terminal turn reconciles stale children.
- The macOS projection accepts arbitrary future provider adapter identifiers and
  creates a dynamic lane. This is UI forward compatibility, not a claim that an
  unimplemented Provider adapter already works.
- Existing direct question paths remain: Claude AskUserQuestion/Elicitation and
  managed Codex requestUserInput. Secret answers stay ephemeral.

## Automated evidence

Passed during implementation:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test --workspace --offline
cargo build --workspace --release --offline
./scripts/check-actrealm-language.sh
apps/macos/Scripts/test.sh
plutil -lint apps/macos/Resources/Info.plist
```

Focused cases cover read-only completion, Codex plan persistence, provider-owned
permission suppression, full-access suppression, auto-review escalation,
Codex collab Agent lifecycle, model decoding, and a future provider lane. The
macOS suite passed 45 tests. The complete Rust workspace, release build,
language gate, and Info.plist validation also passed.

## Still required before release acceptance

- Install the exact candidate locally.
- In fresh real Claude Code and managed Codex sessions, visually verify plan,
  sub-Agent, auto-review/escalation, interactive question, and completion cards.
- Record the resulting commit SHA after local visual acceptance.

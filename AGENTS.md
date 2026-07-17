# Flow Agent repository guidance

`docs/V1_ACCEPTANCE.md` is the executable delivery contract for v1. Work through
M0-M5 in order. A milestone may be committed and pushed only after its listed
checks pass and its verification note is updated with the exact commands and
results.

Product invariants:

- Claude Code and Codex CLI are P0 providers.
- External Hook Control controls each request-keyed `PermissionRequest`; it
  does not own the provider session and must never imply interrupt or steer
  support. Claude `AskUserQuestion` and `Elicitation` may use their official
  blocking Hook reply channels, but answers remain memory-only.
- Codex direct question answers require an explicitly attached, version-gated
  app-server Connector. Hook-only Codex sessions remain observe/approval-only
  and must never be shown as managed or directly answerable.
- Allow, deny, and pass-through are the only v1 approval outcomes.
- Permission hooks use provider-aligned hard deadlines owned by the hook
  process: Claude 24 hours and Codex 1 hour. Tests inject short budgets.
- Runtime absence, socket EOF, protocol mismatch, or deadline expiry must leave
  stdout empty and return control to the provider.
- Approve and deny use a three-second delayed commit and remain undoable until
  the provider directive is written.
- A written directive is `decision_sent`, never `confirmed`; only a later
  provider event may confirm progress.
- Permission requests are never spooled or replayed.
- Interactive question waiters are never persisted or restored. Runtime
  restart expires the old request; a managed Provider may issue a fresh one
  after Thread/Turn reconnection.
- Raw prompts, full commands, tool input/output, transcripts, and file contents
  are not persisted by default.
- The runtime is local-only. Do not add telemetry, cloud SDKs, CDNs, or outbound
  update checks.
- The v1 web client uses native HTML/CSS/JS with no framework or build step.

Required local gate before every milestone commit:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test --workspace --offline
cargo build --workspace --release --offline
```

Run milestone-specific integration, security, and performance checks in
addition to this common gate.

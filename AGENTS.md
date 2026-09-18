# ActRealm repository guidance

`docs/V1_ACCEPTANCE.md` is the executable delivery contract for v1 and
`docs/STATUS.md` is the current short status. Functional work is recorded
through M14. M5 remains a parallel final-release qualification track because
its continuous 48-hour soak is still incomplete. A test-candidate commit/push
requires the listed automated/local gates and explicit user authorization;
manual acceptance, merge to `main`, version/tag, and release publication are
separate decisions.

Product invariants:

- Claude Code and Codex CLI are P0 providers.
- External Hook Control controls each request-keyed `PermissionRequest`; it
  does not own the provider session and must never imply interrupt or steer
  support. Claude `AskUserQuestion` and `Elicitation` may use their official
  blocking Hook reply channels, but answers remain memory-only.
- Codex direct question answers require an explicitly attached, version-gated
  app-server Connector. Hook-only Codex sessions remain observe/approval-only
  and must never be shown as managed or directly answerable.
- Provider-native approval is not the same as a request-keyed ActRealm reply
  channel. Native `request_permissions` / `waitingOnApproval` is observation
  only: no request ID, no allow/deny controls, neutral resolution, and no
  inference that the user approved, denied, or executed the action.
- A live native waiting state survives incidental running/tool updates and
  clears only on a matching explicit Provider lifecycle/status transition.
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
- ActRealm is a local-only product. Do not add team sharing, cloud account
  synchronization, telemetry uploads, mobile control or Firebase dependencies.
- Local Companion control stays on authenticated loopback routes and retains
  per-request permission, expiry and live reply-channel validation.
- The v1 web client uses native HTML/CSS/JS with no framework or build step.
- The Rust Runtime remains the single owner of Hooks, SQLite, approval state,
  sanitization, and Provider reply channels. Native clients consume the
  authenticated localhost API and WebSocket; they must not read or mutate the
  Runtime database directly.
- Keep macOS and Windows UI implementations separate under `apps/`. Share only
  stable contracts, fixtures, terminology, and documentation. Do not vendor a
  second copy of this Rust workspace inside a native client.
- Foreground scheduling is an OS-client responsibility. It may activate apps
  or windows using platform APIs, but it must not invent Provider capability or
  approval state beyond the Runtime snapshot.

Required local gate before every milestone commit:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test --workspace --offline
cargo build --workspace --release --offline
./scripts/check-actrealm-language.sh
TZ=UTC apps/macos/Scripts/test.sh
```

CI reproducibility rules:

- Treat the GitHub job log as the source of truth; identify the exact failing
  test and assertion before changing code or rerunning a workflow.
- Tests must not combine `Date()`, `Calendar.current`, the host time zone, or
  the host locale with fixed expected text. Inject a fixed clock, calendar,
  time zone, and locale whenever the output depends on them.
- Tests that distinguish file freshness or trust state must not rely on two
  real filesystem writes landing in different clock ticks. Inject the clock or
  set a deterministic timestamp gap before asserting the transition.
- Adding a Runtime message or API error code requires updating the shared
  contract and every implemented localization in the same change, followed by
  `./scripts/check-actrealm-language.sh` before any push.
- The macOS test script defaults to UTC, matching GitHub-hosted runners. Tests
  for user-local behavior must inject and assert each intended zone explicitly.
- Keep every action pinned to a full commit SHA and on a supported Node runtime;
  a deprecation warning is a maintenance failure even when it is not yet the
  failing step.

Run milestone-specific integration, security, and performance checks in
addition to this common gate. Documentation-only changes must still pass link,
format, stale-status, and `git diff --check` validation before commit.

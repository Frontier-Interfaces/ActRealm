# Native approval parity — 2026-09-16

Build 119 aligns the native ActRealm approval surfaces with Display. The retired
Web client is outside this change.

## Behavior

- AttentionRecord now decodes allowedActions. An explicit empty list overrides
  remoteActionable; only approve and deny are recognized.
- All risk levels use the same capability rule. The former git status / diff /
  log restriction is removed from the workspace, HUD and menu bar.
- Each surface requires a current open request, a request ID, unexpired reply
  window and live connection. Native-only Provider confirmations remain
  observation-only. The view model rechecks the newest item and request ID when
  a button is used, so an old view cannot reuse retired capabilities.
- Legacy snapshots without allowedActions use Display's fallback: an explicit
  remoteActionable permits allow/deny, otherwise only denial is offered.
- The server rejects approval of an unrecognized reply shape, matching its own
  deny-only declaration, before creating a command or writing to the Provider.
- Risk warnings stay visible, including an explicit risk line in the menu bar.
  HUD helper text describes only currently available controls. A request with
  no reply action offers the original Provider window.
- The existing three-second delayed commit and undo behavior remains. No
  approval auto-execution, permanent permission grant or Provider interruption
  capability was added.

## Verification

- 403 Rust tests passed; 3 explicitly ignored tests remain ignored.
- 202 native Swift tests passed, including six capability regression cases.
- Tests cover high/unknown risk with valid actions, explicit empty/deny-only
  actions, unknown future actions, legacy fallback, absent/expired/closed
  requests, lost connections, decoding empty versus missing declarations and a
  stale button referencing a replaced request.
- Server regression verifies that a deny-only request rejects a direct approve
  without creating a command or consuming the waiter, and that deny still
  reaches the same waiter. Existing high-risk approval, managed Codex, replay,
  deadline and undo tests pass.
- Clippy with warnings denied, formatting, language contracts and diff whitespace
  checks pass.
- SnapshotTool renders use synthetic requests with no Provider execution. The
  high-risk request is visibly actionable in the workspace and menu bar; HUD
  preview verification is recorded with the local artifacts.

No commit or push was made. Tests and previews do not approve or execute an
actual user shell command. App installation and Runtime signature/hash checks
are recorded in the local delivery artifacts.

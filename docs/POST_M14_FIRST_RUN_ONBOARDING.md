# Post-M14 first-run workspace and Agent setup center

Status: implemented candidate; board 6/7 visual acceptance passed, while exact
real-Provider installation and remaining local gates are not yet complete.

Date: 2026-07-20

Source branch: `agent/v1-full`

## Scope

This candidate implements the approved board 6/7 direction without copying
unsupported visual placeholders into the product:

- board 6 is the default three-column workspace when both supported Providers
  are truly uninstalled or unavailable;
- board 7 is one setup center for first-run and returning users;
- Claude and Codex are the only visible Provider choices because they are the
  only supported setup backends;
- every mutation calls the authenticated `/api/v1/setup` contract;
- Codex `/hooks` trust remains a user-controlled action in the official
  interface;
- the maintained GitHub Chinese guide is directly reachable from both views.

No Runtime setup/install implementation was duplicated in JavaScript. The UI
renders server-owned detection, paths, details, repairability, review command,
and `firstRun` state.

## Truthful state mapping

| Server state | Visible action |
| --- | --- |
| `provider_missing` | Explain that no local CLI/Desktop was detected; show the guide |
| `not_installed` | Safe install |
| `installed_unverified` | Refresh, repair, or uninstall |
| `needs_trust` | Copy the Runtime-supplied Codex command, complete `/hooks` trust, then refresh |
| `connected` | Refresh or uninstall |
| `needs_reinstall` | Reinstall or uninstall |
| `inline_conflict` / `error` | Show the actual detail and allow re-detection only |
| Runtime offline | Preserve visible facts but disable mutation controls |

The first-run workspace does not show stale cached quota as current Provider
state. Returning users keep their existing task, attention, quota, settings,
and recovery behavior.

## Automated checks

Run on the exact candidate:

```bash
node --check web/app.js
git diff --check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test --workspace --offline
cargo build --workspace --release --offline
./scripts/check-actrealm-language.sh
ACTREALM_RESOURCE_DURATION_SECONDS=120 \
  ./scripts/m5-resource-check.sh target/release/actrealm
```

The embedded-UI regression contract additionally verifies the visible setup
entry, `firstRun` rendering, real setup endpoint, Codex trust command copy,
guide link, and absence of Kimi/“coming soon” placeholders.

### Current candidate results

- JavaScript syntax and diff whitespace: passed.
- Embedded UI contract: passed.
- Rust format and workspace Clippy with `-D warnings`: passed.
- Full Rust workspace suite: passed once with the manual preview/resource tests
  ignored as designed.
- Workspace release build and ActRealm language contract: passed.
- macOS Swift suite: not executed in the managed Codex sandbox because
  SwiftPM's own `sandbox-exec` was rejected with `sandbox_apply: Operation not
  permitted`; no Swift test assertion ran.
- Two-minute Runtime resource gate and the repeated setup gate: not accepted in
  the managed sandbox. Reproduction reached Runtime startup and failed before
  the first product assertion because Unix Socket bind returned
  `Operation not permitted (os error 1)`.
- Isolated first-run preview harness: passed for 300 seconds in the user's
  ordinary terminal on 2026-07-20 (`1 passed; 0 failed`). This proves the
  temporary-path preview server starts and exits cleanly; visual and button
  acceptance remain separate.
- Board 6/7 visual acceptance: passed by the user on 2026-07-20 against the
  isolated preview candidate. The release binary prepared for the next real
  installation gate has SHA-256
  `340bf2f5d0fd36fd9cc085caf85de8d435d11437f04ad13fad6b7e2104c2d429`.
- Exact-candidate installation: the stable helper and release binary both have
  SHA-256 `340bf2f5d0fd36fd9cc085caf85de8d435d11437f04ad13fad6b7e2104c2d429`.
  Read-only Doctor evidence reports Claude's 17 managed handlers, Codex's 6
  managed handlers, executable helper, canonical Codex Hooks feature, complete
  trusted hashes, private Runtime Socket permissions, and silent offline
  pass-through as passing. Fresh Claude/Codex real events remain pending.
- The managed Codex environment could not complete Doctor's Socket round trip
  (`Operation not permitted`), matching its known Unix Socket restriction. The
  user's open page and fresh Provider events are the remaining authoritative
  local Runtime evidence.

The last two items must be rerun in the user's ordinary terminal. The candidate
must not be described as fully gated until they pass there.

## Isolated visual preview

The ignored preview test sets temporary Claude/Codex paths so it cannot install,
repair, or uninstall the user's real Hook configuration:

```bash
ACTREALM_PREVIEW_SECONDS=300 \
  cargo test -p actrealm-server --test manual_preview --offline \
  first_run_onboarding_preview -- --ignored --nocapture
```

Open the printed loopback URL and verify:

1. board 6 renders before any generic/stale empty state;
2. toolbar status reads “未连接 Agent”;
3. OUTBOX, Agent Tasks, and Quota agree on the disconnected state;
4. “连接 Agent” opens the board 7 setup center;
5. only Claude and Codex appear;
6. “查看接入指南” opens the GitHub guide;
7. the page remains usable at 1600×600 and in its responsive narrow layout.

Then install the exact release candidate in the normal local data directory and
verify real Claude/Codex detection, install, Codex trust, refresh, repair, and
uninstall behavior. Visual acceptance is complete. The user explicitly
authorized a local candidate commit on 2026-07-20 while those remaining gates
stay open; push remains separately gated.

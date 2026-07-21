# Post-M14 macOS/Web feature parity

Status: native source implementation and focused ActRealmKit verification are
complete. Full SwiftUI compilation, visual acceptance, exact installation, and
real-Provider acceptance remain open.

Date: 2026-07-20

Source branch: local working tree based on `agent/v1-full`

## Purpose

The Web control surface had accumulated Runtime-backed capabilities that the
native macOS client did not expose. The native client now consumes those same
authenticated localhost contracts and keeps only Agent Focus as an
additional macOS-specific capability.

The Rust Runtime remains the sole owner of setup/configuration writes, SQLite,
sanitization, approval state, Provider waiters, and Connector replies. The
native client does not inspect or mutate Runtime storage directly.

## Synchronized native surfaces

- Header Agent status, truthful first-run empty states, and a unified Claude/
  Codex setup center with install, repair, uninstall, refresh, manual Codex
  trust steps, and the maintained Chinese guide.
- Request-keyed approval, Provider-native approval observation, completion/
  error handling, and memory-only Claude/Codex interactive-question forms.
- Current session title/activity/turn, usage/context/cost, environment, plan,
  tool, subagent, recovery/control, jump, and Connector-management fields.
- Dynamic quota windows and Runtime-provided source, plan, reset, and capture
  metadata, without invented fixed-window placeholders.
- Runtime monitoring/restart, reminder rules, sound, retention, JSON and
  metrics export, destructive local-data clear, Claude quota bridge, Codex
  enhanced Hook mode, display profiles/field catalog, and Provider sound mute.
- Web-equivalent recent-session visibility and task-clear behavior. Clearing a
  task hands open questions back to the Provider, dismisses other presentation
  items, and hides only the current local row version until a new event.

Foreground scheduling remains a native-only page and continues to use AppKit/
`NSWorkspace` without changing Runtime task, approval, or Provider state.

## Capability and privacy boundaries

- Native Provider approval is observation only and exposes no allow/deny
  control.
- Direct Codex question answers appear only for an explicitly managed,
  version-gated app-server Connector request.
- Question drafts, including secret fields, live only in view-local memory and
  are not written to AppModel, UserDefaults, SQLite, logs, or exports.
- Provider mute suppresses only the optional arrival sound. It does not hide or
  discard Runtime records.
- Setup and settings mutations are disabled when the authenticated local
  Runtime is unavailable.

## Verification

Completed on this working tree:

```bash
swiftc -parse apps/macos/Sources/ActRealmKit/*.swift \
  apps/macos/Sources/ActRealmUI/Design/*.swift \
  apps/macos/Sources/ActRealmUI/Views/*.swift \
  apps/macos/Tests/ActRealmKitTests/*.swift

# Isolated package containing ActRealmKit and its tests, using the matching
# macOS 27 SDK and no SwiftUI target.
swift test --disable-sandbox
```

- Swift parser check: passed for ActRealmKit, ActRealmUI, and focused tests.
- Isolated ActRealmKit suite: 39 passed, 0 failed, across 8 suites.
- ActRealmKit target build with the macOS 27 SDK: passed.
- Rust workspace rerun in the managed sandbox was blocked by Unix-socket
  permissions. The same run outside that sandbox passed 9 of 10 M0 Hook tests;
  the remaining process reported `runtime socket did not become ready`. A
  focused retry produced no test output and was terminated after an extended
  wait, so this working tree does not claim a fresh full Rust gate.

The repository `apps/macos/Scripts/test.sh` currently stops before test
execution because SwiftPM's nested sandbox reports `sandbox_apply: Operation
not permitted` in the managed environment. Running the underlying command with
that sandbox disabled exposes two further toolchain layers: the preferred
macOS 26.0 SDK was built with Swift 6.2 while the installed compiler is Swift
6.4; selecting the matching macOS 27 SDK clears that mismatch, but the
installed Command Line Tools contain no `SwiftUIMacros` compiler plugin. Any
SwiftUI source using standard property-wrapper macros therefore fails before
ordinary project type-checking. No full SwiftUI or snapshot result is claimed
from this environment.

## Remaining acceptance

1. Install an Xcode/toolchain containing the matching SwiftUI macro plugin and
   run `apps/macos/Scripts/test.sh` unchanged or with an explicitly matching
   `SDKROOT`.
2. Render the native snapshots and inspect the main workspace, setup center,
   settings tabs, interactive forms, compact menu-bar handoff, and minimum
   supported window size.
3. Exercise setup, question, quota, jump, and Connector flows against fresh
   real Claude and Codex sessions.
4. Obtain separate user authorization before any commit or push.

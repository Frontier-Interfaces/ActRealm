# Companion v5 candidate verification — 2026-09-09

The candidate extends the existing local Companion API and remains on
`actrealm最新版`. Its primary consumer is the native Display Agent board.

## Final behavior

- Codex tool lifecycle observations and transient asynchronous questions are
  projected from authenticated Connector events or bounded local observation.
- Provider-native questions have no invented reply channel. Direct answers
  require a matching live attached turn/request and current respond capability.
- Initial independent Connector idle/notLoaded listings cannot terminate an
  externally running Desktop turn. Repeated metadata/usage refreshes do not
  revive a locally dismissed task through a false new-event timestamp.
- Result excerpts contain at most 600 characters and five validated local
  references, remain memory-only, and expire on a later turn or retention limit.
- File reveal rechecks the current result, ownership, path boundary and file
  identity. A native request can receive a one-time local target; legacy reveal
  checks the system command's exit status and enforces a three-second timeout.
- Model pricing refreshes hourly without blocking usage collection; failed
  refreshes retain the last validated catalog. Model switches use the current
  model's metrics and the embedded snapshot includes the September update.
- Offline spooling excludes prompt/reply/command/tool-content payloads.

## Required local gates

| Gate | Outcome |
| --- | --- |
| cargo fmt --all -- --check | Passed |
| cargo clippy --workspace --all-targets --offline -- -D warnings | Passed |
| cargo test --workspace --offline | 408 passed, 3 explicitly ignored, 0 failed |
| cargo build --workspace --release --offline | Passed |
| scripts/check-actrealm-language.sh | Passed |
| TZ=UTC apps/macos/Scripts/test.sh | 127 XCTest + 198 Swift Testing passed, 0 failed |
| macOS Resources/Info.plist validation | Passed |
| git diff --check | Passed |

Tests cover scopes, stale turns, missing/replaced files, privacy boundaries,
question clearing, answer routing, native-approval lifecycle, usage/model
switches, pricing refresh, and offline replay. Ignored live/manual fixtures are
not counted as passing tests. Public release qualification and long-duration
soak remain separate; this candidate does not claim them complete.

The user explicitly requested commit and push to `actrealm最新版` on
2026-09-09. Generated app bundles and test-candidate packaging directories are
excluded from the commit. No new version tag or public release is requested.

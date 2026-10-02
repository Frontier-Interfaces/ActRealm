# ActRealm Agent observability and Companion v2 candidate

Date: 2026-08-17

Branch: `agent/runtime-v2-agent-observability`

Baseline: `85149b2f1937b7c52c8b3b03c278358f7eb89ee1` (`main`)

Compatibility source: the focused Runtime protocol and packaging changes from
`30d9f8291f9f26694f66e680dfb52aed4aab9144`
(`agent/shared-runtime-phase-1`) were ported onto the baseline. The candidate
does not replace or rebase away the cloud-sharing merge in `85149b2`.

## Delivery scope

This candidate combines all local work made after the baseline. The changes
belong to four related product areas.

### Active Agent task truth

- Claude Code and Codex tasks are projected from Runtime state into one
  newest-event-first Agent task feed. Simultaneous tasks remain visible and
  deterministic ties use stable session identity.
- Current-turn plan progress is separated from older completed turns. A Codex
  plan notification without a Provider turn ID is bound by Runtime state rather
  than assigned a synthetic turn that could make stale progress appear live.
- Expanded tasks can show task flow and a bounded, scrollable workflow. Empty,
  loading, failed, and no-current-turn states are explicit.
- Workflow presentation coalesces tool lifecycle noise, keeps the exact tool
  name, labels semantic categories, and collapses routine shell activity while
  retaining long-running or failed commands.
- Task-card fields, workflow visibility, Token details, and quota density are
  controlled by validated Runtime settings. The client does not invent fields
  the Runtime did not allow.

### Local Token usage

- Runtime records numeric daily aggregates for Codex and Claude from their
  bounded local usage sources. It does not persist prompts, responses, full
  commands, tool input/output, file contents, or transcripts for this feature.
- Aggregates cover today, current month, lifetime observed total, Provider,
  model, input, output, cache read, cache creation, reasoning, estimated cost
  where pricing is known, message count, active days, peak day, and streaks.
- The macOS Token dashboard includes overview and trend views, daily/weekly/
  cumulative selection, Provider/model breakdowns, composition details, and a
  calendar heatmap whose hover target reports the exact date and usage.
- Collection is incremental and cursor-backed to avoid double counting.
  Collection readiness, refresh-in-progress state, latest success, and stale or
  partial data are represented explicitly.
- Settings support full, compact, or hidden Token presentation; component
  visibility; and automatic, western, or East Asian units.

### Quota and task-card presentation

- Codex quota labels use the plan identity returned by the Provider. Spark is
  shown separately from other windows and is hidden unless the account is
  identified as Codex Pro.
- Reset timestamps remain Provider data rather than locally guessed schedule
  text.
- Task cards expose sanitized project, model, activity, plan, Token/context,
  current tool, recovery/control, task flow, and workflow fields according to
  settings and Runtime capability.

### Companion protocol v2

- Public health now reports `protocolVersion: 2` only with the complete local
  Companion route set compiled into this Runtime.
- Display can reject protocol-v1 Runtime instances before pairing instead of
  failing later on a missing activity capability.
- `GET /api/v1/companion/sessions/{id}/activity` returns a bounded sanitized
  current-turn activity page and supports cursor-based incremental refresh.
- Companion snapshot schema remains v1. Protocol version and snapshot schema
  are deliberately independent compatibility gates.
- The standalone macOS Runtime packager writes only to an explicit absolute
  path (default `outputs/runtime-macos/actrealm`), verifies the release binary,
  sets executable permissions, and prints its SHA-256 digest.

## Privacy and authority boundaries

- Rust Runtime remains the sole owner of Hooks, SQLite, sanitization,
  Provider waiters, approval state, and reply channels.
- Companion authorization remains loopback-only, bearer-token scoped, and
  revocable. Snapshot and activity use `snapshot.read`; jump and response
  capabilities are separately scoped.
- Raw prompts, transcripts, file contents, Provider credentials, Web
  credentials, and private reply-channel locators are excluded from Companion
  responses and Token aggregates.
- A displayed plan or native waiting state is observational unless Runtime
  reports a matching live reply capability. The UI must not infer control.

## Compatibility notes

- Display requires Companion protocol version 2. An installed Runtime built
  before this candidate reports version 1 and must be rebuilt/reinstalled
  before Display can connect.
- Snapshot schema remains version 1, so protocol v2 does not widen the
  serialized privacy contract.
- `outputs/` is ignored because it contains local build artifacts, not source.
- No existing cloud-sharing change from the `85149b2` baseline is removed by
  this integration.

## Verification contract

Before push, the repository-required gates are:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test --workspace --offline
cargo build --workspace --release --offline
./scripts/check-actrealm-language.sh
TZ=UTC apps/macos/Scripts/test.sh
plutil -lint apps/macos/Resources/Info.plist
git diff --check
```

The final commit and remote branch identify the exact candidate. Merge to
`main`, signing/notarization, installation, and public release remain separate
actions.

## Verification result

Verified locally on 2026-08-17:

- Rust formatting check passed.
- Workspace Clippy passed offline with warnings denied.
- Full offline Rust workspace tests passed. The explicitly manual UI previews
  and release-candidate resource soak remained ignored by their test contract.
- Full offline release build passed.
- ActRealm and Runtime language contracts passed.
- UTC macOS test script passed, including the Agent task ordering, current-turn
  task-flow/workflow, Token dashboard, quota, localization, Runtime control,
  cloud-sharing, and native projection suites.
- `Info.plist`, zsh syntax, executable mode, and `git diff --check` passed.
- The standalone packager produced an executable Apple Silicon Mach-O Runtime
  and printed its SHA-256 digest under the ignored `outputs/` directory.

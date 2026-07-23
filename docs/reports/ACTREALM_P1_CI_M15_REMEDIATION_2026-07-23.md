# ActRealm CI, P1/P2, and M15 remediation

Date: 2026-07-23
Branch: `agent/v1-full`
Starting revision: `b8559af fix: close P0 release blockers`
Status: code complete; local automated gates passed; real sleep/wake, packaged
active-load sampling, and Codex Desktop acceptance remain manual release gates.

## Approved scope

This remediation implements CI, AR-010 through AR-015, AR-023, AR-024, and
version-gated Codex managed approvals. Per the product decision, AR-016 and
AR-017 were not changed.

## Results by issue

### CI / AR-019

- Root cause: both PR core-gate runs `29974445484` and `29974575389` failed in
  Clippy while compiling `libsqlite3-sys 0.38.1`. The workflow pinned Rust
  1.85, but the locked dependency uses the newer `cfg_select!` build macro.
- Fix: declare Rust 1.97 in `Cargo.toml` and install the same exact toolchain
  in `.github/workflows/ci.yml`.
- Impact: local and GitHub CI no longer use different implicit MSRV
  assumptions. The existing format, Clippy, tests, release, Swift, RustSec,
  package, codesign, and artifact gates remain intact.
- Verification: local zero-warning Clippy passes on Rust 1.97. A remote green
  run requires the reviewed commit to be pushed.

### AR-010 · Runtime snapshot CPU

- Root cause: every WebSocket connection rebuilt the full SQLite session,
  plan, sub-Agent, Attention, command, and metrics projection at a 100 ms
  cadence, even when no durable state changed.
- Fix: the single storage writer now owns a revision-invalidated snapshot
  cache. Every mutation invalidates it immediately; unchanged reads reuse the
  projection for up to two seconds. The WebSocket cadence stays at 100 ms, so
  event latency does not regress. A Runtime-owned quota scheduler also removes
  quota freshness from the WebSocket read path.
- Verification: the focused event-to-WebSocket test measured 108.976 ms p95
  against the 300 ms gate. Cache invalidation and schema-11 migration tests
  pass. The two-minute release resource gate sampled 118 times: idle Runtime
  CPU averaged 0.000% and maximum RSS was 6,640 KiB, within the 0.5% CPU and
  81,920 KiB RSS budgets.
- Remaining release proof: repeat the original 1/10/100-session and 1 GB
  active-rollout CPU sample on the packaged candidate.

### AR-011 · Acknowledgement feedback

- Root cause: `acknowledge` showed a generic Provider handoff toast before
  knowing whether Runtime accepted the command.
- Fix: the client now awaits the authenticated command and refreshed snapshot.
  Completion, error, question, and approval acknowledgements have distinct
  accepted-outcome text and include the task title.
- Verification: Swift acknowledgement semantic tests pass. Failure paths show
  the returned Runtime error instead of a false success.

### AR-012 · Full access and auto-review semantics

- Root cause: all `provider_handles_approval` events shared the “Codex 正在自动审批”
  projection.
- Fix: permission mode and approval owner are stored independently.
  `danger-full-access`, `bypassPermissions`, `fullAccess`, `full_access`, and
  `never` display full-access wording; `dontAsk` displays noninteractive
  wording; only guardian/auto-review ownership uses automatic-review wording.
- Verification: all supported noninteractive aliases and the auto-review
  control case have snapshot assertions.

### AR-013 · Root SessionEnd wording

- Root cause: a zero active-sub-Agent projection overrode the root lifecycle
  message with “子 Agent 已结束”.
- Fix: that message is emitted only for a real `SubagentStop`. Root
  `SessionEnd` now projects “会话已结束”.
- Verification: lifecycle-only and sub-Agent reducer tests pass.

### AR-014 · Stale sub-Agent rows

- Root cause: older stop paths could set `active=0` without moving
  `status=running`, and ended parent sessions did not normalize legacy rows.
- Fix: schema 11 migrates inactive/running rows to completed, closes active
  children of ended parents, and makes stop/session-end transitions update
  active, status, and stopped time together.
- Verification: a deliberately inconsistent schema-10 database migrates to a
  consistent completed row.

### AR-015 · Bootstrap token in diagnostics

- Root cause: Runtime stdout/stderr was appended to the visible tail before
  parsing the one-time bootstrap URL.
- Fix: bootstrap is consumed before the stdout tail is stored, and a shared
  diagnostic redactor replaces every `bootstrap=` value in stdout and stderr.
- Verification: Swift tests inject a UUID token and assert that only
  `<redacted>` remains.

### AR-023 · Claude quota after sleep/wake

- Root cause: macOS did not observe wake, the URLSession WebSocket could remain
  apparently live while suspended, and quota polling occurred only while a
  WebSocket snapshot loop was healthy.
- Fix:
  - observe `NSWorkspace.didWakeNotification`;
  - cancel the suspended socket and stream task;
  - call an authenticated `/api/v1/quota/refresh`;
  - pull a fresh snapshot and start a new WebSocket;
  - fall back to supervised Runtime restart if recovery fails; and
  - poll quota independently inside Runtime even with zero WebSocket clients.
- Follow-up after live regression: Settings now provides a synchronous
  authenticated “立即更新” action. It reports missing/rejected credentials,
  rate limiting, or Provider failure without exposing credential material, and
  claims success only after Claude's real capture time advances.
- Verification: Swift compilation/tests pass, the refresh endpoint requires
  cookie/origin/CSRF, and its cache invalidation test passes.
- Remaining manual proof: sleep for 1, 10, and 60 minutes, five rounds each;
  verify reconnect within 10 seconds and a fresh value or explicit failure
  within 60 seconds.

### AR-024 · History-only Claude cards

- Root cause: visibility used `last_event_at`, so Claude Desktop replaying only
  `SessionStart`/`SessionEnd` refreshed historical sessions into Agent Tasks.
- Fix: schema 11 adds internal `last_meaningful_activity_at`. Lifecycle-only
  events can restore internal state and official titles but cannot make a task
  visible or create completion Attention. Prompt, tool, approval, question,
  failure, plan, and sub-Agent work remain meaningful. Open Attention always
  keeps its session visible.
- Verification: lifecycle-only sessions stay hidden, the first real prompt
  reveals exactly one task, and migrated real-event histories retain their
  30-minute visibility.

### M15 · Codex client direct approval

- Root cause: the Connector handled `requestUserInput` and native
  `waitingOnApproval`, but did not answer Codex app-server approval requests.
- Fix:
  - capture `initialize.userAgent` and fail closed outside the generated
    0.144.5–0.144.x schema family;
  - accept only explicitly attached managed Threads;
  - support command execution, file change, and permissions requests;
  - route each request through the existing request-keyed waiter and
    three-second undo transaction;
  - return one-turn accept/decline, or the exact requested network/fileSystem
    permission subset;
  - reconcile `serverRequest/resolved` with command and Attention state; and
  - replace any preceding observation-only native card with the authoritative
    direct approval card.
- Safety boundary: arbitrary independent Codex Desktop conversations remain
  observation-only. Explicit attach does not transfer an already-running Turn
  from Codex Desktop's private app-server connection. Direct controls appear
  only for a request actually delivered to the ActRealm-owned connection.
  Unknown versions and malformed/expired requests never receive guessed
  protocol responses.
- Verification: generated-schema version gates, detached-thread rejection,
  three request constructors, command round-trip, least-privilege permission
  allow/deny, native-card dedupe, and Provider resolution tests pass.
- Remaining manual proof: attach a real Codex Desktop Thread and run command,
  file, and permission approvals through allow and deny.

## Automated gate record

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | PASS |
| focused Runtime/Server/Codex tests | PASS |
| event-to-WebSocket p95 | PASS, 108.976 ms |
| macOS Swift build and tests | PASS, 83 tests |
| full workspace tests | prior unrestricted run PASS; current sandbox blocks listener creation before 12 integration assertions |
| workspace release build | PASS |
| language and plist checks | PASS |
| two-minute release resource gate | PASS, 118 samples; 0.000% average CPU; 6,640 KiB maximum RSS |
| RustSec audit | PASS, 132 dependencies scanned with warnings denied |
| local QA package/codesign | PASS, arm64 app and embedded Runtime satisfy designated requirements |
| remote GitHub CI | pending push |
| real sleep/wake matrix | pending manual acceptance |
| real Codex Desktop M15 matrix | pending manual acceptance |

## Data and migration impact

- SQLite advances from schema 10 to 11.
- Existing event, session, Attention, quota, and usage data is retained.
- The migration adds one nullable session activity column and normalizes only
  contradictory sub-Agent rows.
- Old in-memory approval waiters are still never restored after Runtime
  restart.
- No user Hook configuration, Codex trust choice, or installed app is changed
  by the source-tree tests.

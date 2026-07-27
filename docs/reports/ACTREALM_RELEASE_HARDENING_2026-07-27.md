# ActRealm release-hardening report

- Date: 2026-07-27 (Asia/Shanghai)
- Source baseline: `1dff02d879443876a1ab59aca1654ca9d084e7ed`
- Candidate: release-hardening branch candidate
- Support target: Apple Silicon, macOS 26, Rust 1.97, Xcode 26.6
- Publication state: installed and accepted locally; commit/push authorized for
  `agent/runtime-client-localization-hardening`; not merged, tagged, publicly
  signed, notarized, or released

## Scope

This report covers the ten-task release-hardening plan in
`docs/superpowers/plans/2026-07-27-actrealm-release-hardening.md`.

Explicitly deferred:

- Intel;
- automatic updates or outbound update checks;
- 48-hour soak;
- accessibility qualification;
- Windows and Gemini release support.

Deferral means “not implemented or claimed,” not “passed.”

## Implemented changes

### 1. Session and recovery truth

- Execution state remains Provider truth.
- Recovery describes control/reconnection capability only.
- Startup normalizes orphaned local waiting ownership without changing
  Provider-native observation.
- Nonterminal execution cannot be presented as ended.

### 2. Bounded snapshots and retention

- UI filtering happens at the database boundary.
- Plan/sub-Agent rows are batch-loaded for visible sessions.
- Closed expired session graphs are pruned transactionally.
- Actionable Attention, live commands, quota state, and install state remain.
- Full export stays separate from bounded UI projection.

### 3. Usage discovery

- Directory traversal has entry/time budgets, including old files.
- Oversized initial logs cannot produce a false complete cumulative total.
- Incremental state is compacted and bounded.
- Blocking discovery is separated from the snapshot hot path.

### 4. Native projection

- Task render facts use stable session identity and per-card signatures.
- Quota, metrics, setup, and one-second clock changes do not republish
  unchanged task cards.
- Relative time uses a lightweight visible clock path.
- Native p95 samples are bounded and reject stale/future input.

### 5–6. macOS and Web localization

- ActRealm-owned presentation supports System, Simplified Chinese, and English.
- Client language choice is local and does not mutate Runtime settings.
- Provider/user content remains verbatim.
- Runtime messages and API errors use stable client-rendered contracts.

### 7. Native process and Stage Manager safety

- Stage Manager restoration authority is persisted only when ActRealm enabled
  it and is released at the configured boundary.
- User-owned Stage Manager state is never reset.
- Subprocess pipes are drained asynchronously.
- Runtime/desktop PID identity is checked against reuse and executable moves.

### 8. Local Web security

- Bootstrap/session/CSRF credentials use 32 bytes of OS randomness.
- Secret comparison is constant-time.
- WebSocket access uses a short-lived one-use ticket through
  `Sec-WebSocket-Protocol`; credentials are absent from URLs.
- Origin, Cookie, CSRF, and ticket checks are exact.
- CSP and security response headers are centralized.
- Raw internal failure details are not returned to clients.

### 9. Backup governance and reproducible CI

- Backups identify their source and remain `0700`/`0600`.
- Settings exposes backup count and total size.
- Deletion is separate, requires exact `DELETE BACKUPS`, and refuses the whole
  operation if any entry is unsafe or unmanaged.
- No automatic backup deletion exists.
- GitHub Actions use full immutable revisions.
- Rust is fixed to 1.97, Xcode to 26.6, and cargo-audit to 0.22.2 with
  `--locked`.
- Raw local test evidence was removed from the current tree and replaced by a
  sanitized conclusion index.

## Verification state

### Full common gate

Passed after the final review fix:

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace --all-targets --offline -- -D warnings`;
- `cargo test --workspace --offline`: 236 passed, 3 explicitly ignored
  manual/release-candidate tests;
- `cargo build --workspace --release --offline`;
- CI pin, ActRealm language, and Runtime language-contract checks;
- Web syntax checks and 5 Node tests;
- macOS: 23 suites and 130 tests;
- `plutil -lint apps/macos/Resources/Info.plist`;
- `git diff --check`.

Task 9 scoped results are included in the totals above:

- installer: 16 integration and 4 statusline tests;
- server: 27 unit, 7 API, and 3 performance tests;
- macOS: 23 suites and 130 tests;
- CI pin and language checks;
- `git diff --check`.

### Performance and resource gates

- Hook process p95: **3.146 ms** (budget: 50 ms).
- Runtime event to WebSocket render-entry p95: **110.409 ms**
  (budget: 300 ms).
- Bounded 5,000-session snapshot: **6.550 ms** (budget: 300 ms).
- Snapshot while usage collection is in flight: **3.293 ms**
  (budget: 300 ms).
- Native 500-session projection: passed its deterministic 300 ms bound.
- Two-minute release-candidate resource gate: 118 samples,
  **0.000% idle CPU average** (budget: 0.5%) and
  **6,960 KiB maximum Runtime RSS** (budget: 81,920 KiB).
- Initial and continuing oversized-log tests, including the sparse 1 GiB
  boundary, passed without reporting partial totals as complete.

### Security and privacy gates

- Exact loopback Origin/Cookie/CSRF authentication passed.
- Auth secrets are OS-random and error responses do not expose internals.
- WebSocket tickets are short-lived, single-use, and absent from URLs.
- CSP and all centralized security response headers passed.
- Export/redaction, diagnostic-directory permissions, Unix-socket permissions,
  and explicit data-clear tests passed.
- Backup inventory and exact-confirmation deletion passed, including refusal
  of symlinks/unmanaged files without partial deletion.
- Interactive secret answers remain memory-only and absent from export.

### Final review finding and fix

The final whole-branch review found one load-bearing regression in the batched
snapshot fallback: when a Claude session had multiple factual Task records and
no Connector plan, only the first Task was projected. A focused regression test
first reproduced `["task-1"]` instead of `["task-1", "task-2"]`. The fallback
now freezes whether each session had a Connector plan before appending all
bounded Claude Task rows. The focused test, the 500-session batch test, all
workspace tests, performance gates, and the two-minute resource gate passed
after the fix.

### Installed-candidate finding and fix

The first installed candidate exposed a native integration regression that
unit Server tests could not detect: Task 8 changed WebSocket authentication to
one-use tickets sent through `Sec-WebSocket-Protocol`, while the macOS client
still placed CSRF in the WebSocket URL. The Server correctly rejected every
native upgrade, leaving the client in a reconnect loop.

The macOS client now requests a fresh `/api/v1/ws-ticket` for every connection
attempt, sends `actrealm.<ticket>` as the subprotocol, and puts no credential in
the URL. A focused native request-contract test was added. The complete macOS
suite now passes 130 tests, the Server's 27 unit, 7 API, and 3 performance tests
pass, and the full common gate passes after the repair.

The rebuilt candidate passed deep ad-hoc signature validation, arm64 checks,
Info.plist validation, and source/package binary comparison. After signatures
were removed from temporary copies, the only byte differences were the
expected `__LINKEDIT.vmsize` adjustments created by re-signing; executable
content matched. Installation preserved the existing `~/.actrealm` inode,
`0700` permissions, and modification time. The native UI reports
`Runtime · 本机在线`, and Doctor passes the Runtime control loop, Claude/Codex
real-event evidence, Hook configuration/trust, and fail-open checks.

The user accepted the real Claude/Codex workflows on 2026-07-27.

## Release decision

Current decision: **ready for the authorized branch commit and push; not ready
for merge or public release**.

The automated engineering gates, final review, package verification, local
installation, Doctor, and user acceptance are green. Commit/push is authorized
only for `agent/runtime-client-localization-hardening`; merge, tag, signing,
notarization, and public release remain separately gated.

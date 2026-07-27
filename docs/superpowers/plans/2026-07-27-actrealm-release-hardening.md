# ActRealm Release Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix every confirmed release-hardening issue in the 2026-07-27 consolidated audit except the five explicitly deferred areas, while preserving ActRealm's truthful local-only provider control contract.

**Architecture:** Rust remains the sole owner of provider events, persistence, sanitization, recovery, authenticated localhost APIs, and WebSocket delivery. Swift and Web consume stable presentation fields and localized message codes; neither client reads SQLite directly nor invents provider capabilities. High-frequency UI paths use bounded, incremental projections, while retention/export remain separate storage concerns.

**Tech Stack:** Rust 1.97, SQLite/rusqlite, Tokio/Axum, Swift 6/SwiftUI, native HTML/CSS/JavaScript, Node built-in test runner.

## Global Constraints

- Web must support System, Simplified Chinese, and English presentation; the language preference is client-local and must not mutate Runtime settings.
- Do not add Intel support. The product remains Apple Silicon-only.
- Do not add automatic updates or outbound update checks.
- Do not run or claim the 48-hour soak in this plan.
- Do not implement accessibility or keyboard-navigation work in this plan.
- Do not add telemetry, cloud SDKs, CDNs, or non-loopback services.
- Preserve exact provider ownership: native Codex approvals remain observation-only unless a request-keyed managed waiter exists.
- Raw prompts, full commands, answers, transcript contents, and file contents must not be persisted.
- Provider-authored and user-authored text remains verbatim; only ActRealm-owned presentation text is localized.
- Existing backups are not automatically deleted. This plan may add source identity, statistics, and an explicit safe deletion path, but no background destructive rotation.
- Before any production change, add a focused test and observe it fail for the intended reason.
- Do not push. Do not create a final milestone commit until the user has tested the locally installed candidate and explicitly approves commit.

---

### Task 1: Unify Session Execution and Recovery Truth

**Files:**
- Modify: `crates/runtime/src/storage.rs`
- Modify: `crates/runtime/tests/m1_runtime.rs`
- Modify: `crates/server/src/server.rs`
- Modify: `crates/server/tests/m2_api.rs`
- Modify: `apps/macos/Sources/ActRealmKit/DerivedState.swift`
- Modify: `apps/macos/Sources/ActRealmUI/Views/LanesSection.swift`
- Modify: `apps/macos/Tests/ActRealmKitTests/DerivedStateTests.swift`

**Interfaces:**
- Produces one factual projection in which `execState` describes provider execution and `recovery` describes control/reconnection capability only.
- Produces startup normalization for orphaned `awaiting_approval` rows without changing native provider-owned approvals.

- [ ] **Step 1: Add failing Rust migration and snapshot tests**

```rust
#[test]
fn legacy_waiting_session_without_live_blocker_is_normalized_on_open() {
    // Insert awaiting_approval + approval_owner=widget with no live blocking attention.
    // Reopen RuntimeStore and assert owner is NULL and exec_state is waiting_for_event.
}

#[test]
fn running_execution_never_projects_recovery_as_ended() {
    // Insert tool_running external-hook session with a live provider PID fixture.
    // Assert snapshot recovery != "ended" while execState remains "tool_running".
}
```

- [ ] **Step 2: Run the focused Rust tests and confirm RED**

Run: `cargo test -p actrealm-runtime --test m1_runtime legacy_waiting_session_without_live_blocker_is_normalized_on_open --offline`
Run: `cargo test -p actrealm-server running_execution_never_projects_recovery_as_ended --offline`
Expected: failures showing the stale owner/state and contradictory recovery projection.

- [ ] **Step 3: Implement idempotent state normalization and projection precedence**

Implement one transaction that clears local approval ownership when no `open|committing|decision_sent|snoozed` blocking Attention and no live waiter exists. Keep `native_approval` observation rows intact. In `snapshot_value`, derive recovery only after checking terminal execution states and never return `ended` for a nonterminal `exec_state`.

- [ ] **Step 4: Add failing Swift presentation tests**

```swift
@Test func runningTaskAndRecoveryStatusNeverContradict() {
    let task = fixtureTask(execState: "tool_running", recovery: "ended")
    #expect(task.status == .running)
    #expect(task.recoveryPresentation != .ended)
}

@Test func missingToolNameIsOmittedInsteadOfRenderedAsUnknown() {
    let task = fixtureTask(execState: "tool_running", currentTool: nil)
    #expect(task.activityLabel == "正在运行")
    #expect(task.detailRows.contains(where: { $0.value == "Unknown" }) == false)
}
```

- [ ] **Step 5: Run Swift tests RED, implement the shared presentation helper, then run GREEN**

Run: `apps/macos/Scripts/test.sh --filter DerivedStateTests` if filtering is supported; otherwise `apps/macos/Scripts/test.sh`.
Expected before implementation: the new assertions fail.
Expected after implementation: all Swift tests pass and missing facts are omitted.

- [ ] **Step 6: Run task gates and write the task report; do not commit**

Run: `cargo test -p actrealm-runtime --test m1_runtime --offline && cargo test -p actrealm-server --offline && apps/macos/Scripts/test.sh && git diff --check`.

---

### Task 2: Bound UI Snapshots and Implement Complete Retention

**Files:**
- Modify: `crates/runtime/src/storage.rs`
- Modify: `crates/runtime/tests/m1_runtime.rs`
- Modify: `crates/runtime/tests/m4_data.rs`
- Modify: `crates/server/src/server.rs`
- Modify: `crates/server/tests/m5_performance.rs`

**Interfaces:**
- Produces `RuntimeStore::ui_snapshot(cutoff)` or an equivalent bounded request distinct from full export.
- Retention deletes a closed, expired session graph atomically but preserves actionable attention and export correctness.

- [ ] **Step 1: Add failing bounded-snapshot tests**

```rust
#[test]
fn ui_snapshot_reads_only_recent_or_actionable_sessions() {
    // Create 500 stale completed sessions, one recent session, and one stale session with open attention.
    // Assert UI snapshot contains exactly the recent and actionable sessions.
    // Assert export still contains all 502 before retention runs.
}

#[test]
fn snapshot_batches_plan_steps_and_subagents() {
    // Populate 500 visible sessions with plan/subagent rows.
    // Use the test query counter and assert query count is bounded independently of N.
}
```

- [ ] **Step 2: Run focused snapshot tests and confirm RED**

Run: `cargo test -p actrealm-runtime --test m1_runtime ui_snapshot_reads_only_recent_or_actionable_sessions --offline`
Expected: stale rows are read/projected or the query-count assertion fails.

- [ ] **Step 3: Add SQL indexes and replace N+1 reads with batched reads**

Add explicit indexes for `sessions(last_event_at)`, event retention time, attention blocker lookup, turns/session ordering, plan/session ordering, and active subagents. Filter at SQL level and batch Plan/Subagent records by visible session IDs. Full export continues to read every table.

- [ ] **Step 4: Add failing retention graph tests**

```rust
#[test]
fn retention_prunes_closed_expired_session_graph_but_preserves_actionable_rows() {
    // Insert expired closed and expired actionable graphs across all related tables.
    // Apply 30-day retention.
    // Assert the closed graph is gone and the actionable graph remains internally consistent.
}

#[test]
fn retention_reclaims_free_pages_at_a_controlled_boundary() {
    // Apply retention outside a WebSocket hot path and assert the database reopens cleanly.
    // Assert freelist/page reclamation improves without VACUUM inside a transaction.
}
```

- [ ] **Step 5: Run retention tests RED, implement transactional graph pruning and controlled compaction, then run GREEN**

Run: `cargo test -p actrealm-runtime --test m4_data --offline`.
Keep actionable attention, live commands, quota snapshots, and installation state. Never run VACUUM from the 100ms snapshot loop.

- [ ] **Step 6: Add and run a 5,000-session snapshot performance gate**

Extend `crates/server/tests/m5_performance.rs` with a deterministic fixture and assert the bounded snapshot remains below the existing 300ms server budget.

- [ ] **Step 7: Run task gates and write the task report; do not commit**

Run: `cargo test -p actrealm-runtime --offline && cargo test -p actrealm-server --offline && cargo clippy -p actrealm-runtime -p actrealm-server --all-targets --offline -- -D warnings && git diff --check`.

---

### Task 3: Bound Usage Discovery and Oversized Provider Logs

**Files:**
- Modify: `crates/usage/src/lib.rs`
- Modify: `crates/server/src/server.rs`

**Interfaces:**
- Produces a directory traversal budget based on visited entries, not only accepted recent files.
- Produces trustworthy behavior for first discovery of an oversized log: unavailable/partial-quality metadata rather than false cumulative totals.

- [ ] **Step 1: Add failing traversal and oversized-log tests**

```rust
#[test]
fn discovery_stops_after_entry_budget_when_all_files_are_old() {
    // Create more than the entry budget of old nested files before one recent JSONL.
    // Assert visited entries do not exceed the hard budget and symlinks remain rejected.
}

#[test]
fn first_oversized_log_is_not_reported_as_a_complete_total() {
    // Create an oversized valid-looking log and collect once.
    // Assert no computed complete cumulative record is emitted.
    // Append a complete line and assert later incremental data can be consumed honestly.
}
```

- [ ] **Step 2: Run tests and confirm RED**

Run: `cargo test -p actrealm-usage discovery_stops_after_entry_budget_when_all_files_are_old --offline`
Run: `cargo test -p actrealm-usage first_oversized_log_is_not_reported_as_a_complete_total --offline`.

- [ ] **Step 3: Implement visit/time budgets and durable parse checkpoints**

Track directories and entries visited separately from discovered recent files. Preserve newest-first selection where known. Store bounded cursors/aggregates and never label a tail-only initial parse as a full session total.

- [ ] **Step 4: Add a server responsiveness regression**

Create a large-directory fixture and assert usage refresh cannot hold the WebSocket snapshot path past the 300ms budget. Move blocking discovery/parse work off the current-thread async executor or behind an actor/cache boundary.

- [ ] **Step 5: Run task gates and write the task report; do not commit**

Run: `cargo test -p actrealm-usage --offline && cargo test -p actrealm-server --test m5_performance --offline && cargo clippy -p actrealm-usage -p actrealm-server --all-targets --offline -- -D warnings`.

---

### Task 4: Make Native Task Projection Incremental

**Files:**
- Modify: `apps/macos/Sources/ActRealmKit/AppModel.swift`
- Modify: `apps/macos/Sources/ActRealmKit/DerivedState.swift`
- Modify: `apps/macos/Sources/ActRealmUI/Views/LanesSection.swift`
- Modify: `apps/macos/Sources/ActRealmUI/Views/MainWindowView.swift`
- Create or modify: `apps/macos/Tests/ActRealmKitTests/BackgroundSnapshotProjectionTests.swift`

**Interfaces:**
- Produces a stable task render signature keyed by Session ID.
- Quota, metrics, setup, and one-second clock changes do not republish unchanged task-card facts.

- [ ] **Step 1: Add failing projection tests**

```swift
@Test func quotaOnlySnapshotDoesNotRepublishUnchangedTaskCards() {
    let before = projector.apply(snapshot(tasks: [.runningA], quota: 80))
    let after = projector.apply(snapshot(tasks: [.runningA], quota: 79))
    #expect(after.changedTaskIDs.isEmpty)
}

@Test func oneChangedSessionInvalidatesOnlyOneTaskCard() {
    let result = projector.diff(old: [.runningA, .idleB], new: [.waitingA, .idleB])
    #expect(result.changedTaskIDs == ["A"])
}
```

- [ ] **Step 2: Run Swift tests and confirm RED**

Run: `apps/macos/Scripts/test.sh`.
Expected: current full `DerivedState` replacement cannot satisfy the invalidation assertions.

- [ ] **Step 3: Implement stable per-session projections and isolate the clock**

Separate factual task projection from relative-time rendering. Use stable Session IDs and Equatable render facts so unrelated snapshot fields do not reconstruct task rows. Keep live elapsed time driven by a lightweight timeline/tick local to the visible label.

- [ ] **Step 4: Add bounded native p95 sample tests**

Assert a 100-sample rolling window, correct 95th-percentile index, and rejection of future or older-than-10-second samples. Keep Runtime transport p95 and native presentation p95 as separate metrics.

- [ ] **Step 5: Run Swift tests plus deterministic 500-session projection benchmark**

The benchmark must demonstrate native projection p95 below 300ms without depending on App focus.

- [ ] **Step 6: Write the task report; do not commit**

Run: `apps/macos/Scripts/test.sh && plutil -lint apps/macos/Resources/Info.plist && git diff --check`.

---

### Task 5: Complete macOS English Localization

**Files:**
- Modify: `apps/macos/Sources/ActRealmKit/Resources/en.lproj/Localizable.strings`
- Modify: `apps/macos/Sources/ActRealmKit/Localization.swift`
- Modify: `apps/macos/Sources/ActRealmKit/Formatting.swift`
- Modify: `apps/macos/Sources/ActRealmKit/AppModel.swift`
- Modify: `apps/macos/Sources/ActRealmKit/RuntimeSupervisor.swift`
- Modify: `apps/macos/Sources/ActRealmUI/Views/AgentSetupView.swift`
- Modify: `apps/macos/Sources/ActRealmUI/Views/SettingsView.swift`
- Modify: other ActRealm-owned Swift presentation call sites found by the localization test
- Modify: `apps/macos/Tests/ActRealmKitTests/LocalizationTests.swift`
- Modify: `apps/macos/Tests/ActRealmKitTests/ToastBehaviorTests.swift`

**Interfaces:**
- Produces explicit localized keys for Runtime failures and dynamic Toasts.
- Produces `Recovery status`, singular/plural English units, distinct `Set up`/`Connected`, and a singular `Agent` where appropriate.

- [ ] **Step 1: Add failing localization and Toast tests**

```swift
@Test func everyActRealmOwnedToastUsesTheSelectedLanguage() {
    #expect(localized("Codex 启动命令已复制；运行后输入 /hooks", locale: .english)
            == "Codex launch command copied. Run it, then enter /hooks.")
}

@Test func englishErrorPriorityIsCaseInsensitiveAndExplicit() {
    #expect(AppModel.inferredToastPriorityForTest("Could not save settings") == .error)
    #expect(AppModel.inferredToastPriorityForTest("Enter DELETE") == .error)
}

@Test func recoveryAndAgentTermsDoNotReuseConflictingKeys() {
    #expect(localized("恢复状态", locale: .english) == "Recovery status")
    #expect(localized("settings.tab.agents", locale: .english) == "Agents")
    #expect(localized("Agent", locale: .english) == "Agent")
}
```

- [ ] **Step 2: Run Swift tests and confirm RED**

Run: `apps/macos/Scripts/test.sh`.

- [ ] **Step 3: Replace raw client copy with stable localized calls**

Cover RuntimeSupervisor failure/backoff messages, AppModel Runtime-exit text, copied-command Toasts, export/save Toasts, Stage Manager owned copy, Demo risk messages, and any ActRealm-owned strings exposed by English snapshots. Do not translate Provider or user content.

- [ ] **Step 4: Make Toast priority explicit at error call sites**

Retain a lowercase compatibility fallback only for older call sites; new and changed error paths pass `.error` directly.

- [ ] **Step 5: Generate English then Chinese snapshots sequentially**

Run the existing SnapshotTool sequentially, not concurrently. Inspect main, expanded task, setup, Settings, menu, HUD, and Runtime monitor surfaces for raw Chinese ActRealm copy and clipping.

- [ ] **Step 6: Run language and Swift gates; write the task report; do not commit**

Run: `apps/macos/Scripts/test.sh && ./scripts/check-actrealm-language.sh && plutil -lint apps/macos/Resources/Info.plist && git diff --check`.

---

### Task 6: Add Complete Web English and Correct Bootstrap Ordering

**Files:**
- Create: `web/i18n.js`
- Create: `web/i18n.test.js`
- Modify: `web/index.html`
- Modify: `web/app.js`
- Modify: `web/app.css`
- Modify: `crates/server/src/server.rs`
- Modify: `crates/server/tests/m2_api.rs`

**Interfaces:**
- Produces `t(key, args, locale)` and locale-aware duration/plural helpers with System/Chinese/English preferences stored in `localStorage`.
- Bootstrap consumes a hash token before the first authenticated snapshot or socket request.

- [ ] **Step 1: Add failing Node localization tests**

```javascript
test('english maps every runtime and API code', () => {
  assert.deepEqual(missingCodes('en'), []);
});

test('english pluralizes one day and two days', () => {
  assert.equal(formatWindow('quota.window.days', 1, 'en'), '1 day');
  assert.equal(formatWindow('quota.window.days', 2, 'en'), '2 days');
});

test('provider-authored text remains verbatim', () => {
  assert.equal(providerText('修复数据库', 'en'), '修复数据库');
});
```

- [ ] **Step 2: Run Node tests and confirm RED**

Run: `node --test web/i18n.test.js`.
Expected: missing module/registry and plural behavior failures.

- [ ] **Step 3: Implement the locale module and migrate every fixed Web string**

Add a Settings language selector. Update `<html lang>` immediately. Keep native HTML/CSS/JS with no build system. Translate headings, empty states, buttons, details, settings, quota labels, Runtime/API errors, setup, Toasts, recovery labels, and accessibility-neutral document metadata. Never pass Provider/user text through `t()`.

- [ ] **Step 4: Add failing bootstrap-state tests**

Extract a pure bootstrap decision helper and assert: a hash token is consumed before any stored CSRF snapshot; bootstrap success clears stale auth errors; bootstrap failure preserves one actionable error and does not claim Live.

- [ ] **Step 5: Implement bootstrap-first ordering and missing-field truthfulness**

Do not call `setConnected(false)` as a transient default that produces a visible failure. Missing model/tool/recovery facts render localized unavailable text only where the field is required; optional fields are omitted.

- [ ] **Step 6: Update Rust embedded-asset tests and run Web gates**

Run: `node --test web/i18n.test.js && node --check web/i18n.js && node --check web/app.js && cargo test -p actrealm-server --test m2_api --offline && ./scripts/check-runtime-language.sh`.

- [ ] **Step 7: Run local 1600×600 and 1160×600 Chinese/English previews; write report; do not commit**

Verify no horizontal overflow, language persistence, OUTBOX answer flow, setup, quota, Runtime monitor, and bootstrap refresh.

---

### Task 7: Persist Stage Manager Responsibility and Remove Blocking Process I/O

**Files:**
- Modify: `apps/macos/Sources/ActRealmUI/ForegroundSchedulingController.swift`
- Modify: `apps/macos/Sources/ActRealmKit/RuntimeSupervisor.swift`
- Modify: `apps/macos/Tests/ActRealmKitTests/AgentFocusStageManagerTests.swift`
- Modify: `apps/macos/Tests/ActRealmKitTests/BootstrapParsingTests.swift`
- Modify: `crates/server/src/server.rs`
- Modify: `crates/server/tests/m2_api.rs`

**Interfaces:**
- Persists only ActRealm's responsibility for a successful Stage Manager false→true change and safely restores it after relaunch.
- Process execution reads pipes asynchronously and never waits synchronously on MainActor.
- External-hook PID observation requires identity evidence beyond `kill(pid, 0)` when such evidence is available.

- [ ] **Step 1: Add failing Stage Manager responsibility tests**

```swift
@Test func persistedLeaseRestoresAfterRelaunchOnlyWhenActRealmEnabledStageManager() { }
@Test func userEnabledStageManagerNeverCreatesPersistentRestoreAuthority() { }
@Test func failedRestoreKeepsLeaseForASafeRetry() { }
@Test func keepEnabledClearsActRealmRestoreAuthority() { }
```

- [ ] **Step 2: Run tests RED, implement minimal persistent lease, run GREEN**

Persist original false state, successful ActRealm enablement, and restore timing. Clear only after successful restore or explicit keep-enabled choice.

- [ ] **Step 3: Add failing asynchronous Process/Pipe tests**

Inject a process runner that emits more than one pipe buffer, exits, and records whether it was invoked on MainActor. Assert complete bounded capture, no deadlock, redacted diagnostics, and cleaned readability handlers.

- [ ] **Step 4: Replace synchronous `waitUntilExit` paths with async runners**

Cover Stage Manager defaults, Runtime codesign identity lookup, and launchctl cleanup. Preserve executable paths and never invoke a shell.

- [ ] **Step 5: Add PID reuse regression and identity validation**

Create a fixture in which the numeric PID exists but executable/start identity differs. Assert recovery is `lost_control` or `waiting_for_event`, never `observing` based solely on PID existence.

- [ ] **Step 6: Run task gates and write report; do not commit**

Run: `apps/macos/Scripts/test.sh && cargo test -p actrealm-server --offline && plutil -lint apps/macos/Resources/Info.plist && git diff --check`.

---

### Task 8: Harden Local Web Authentication and Error Boundaries

**Files:**
- Modify: `crates/server/src/server.rs`
- Modify: `crates/server/tests/m2_api.rs`
- Modify: `crates/server/tests/m5_performance.rs`
- Modify: `web/app.js`

**Interfaces:**
- Every response has local security headers.
- Session/bootstrap/CSRF material uses 32 random bytes and constant-time equality.
- WebSocket authentication avoids query-string bearer material while preserving exact Origin, Cookie, and CSRF binding.

- [ ] **Step 1: Add failing response-header and error-boundary tests**

```rust
#[test]
fn every_static_and_api_response_has_security_headers() {
    // Assert no-store, nosniff, DENY/frame-ancestors, no-referrer, restrictive CSP and Permissions-Policy.
}

#[test]
fn dynamic_errors_never_return_paths_tokens_or_provider_secrets() {
    // Inject an absolute path and credential-shaped error; assert stable code only.
}
```

- [ ] **Step 2: Add failing WebSocket auth tests**

Assert the URL contains no CSRF token and that mismatched/missing Origin, Cookie, ticket/subprotocol, or single-use state is rejected.

- [ ] **Step 3: Implement centralized security middleware and stable error mapping**

Use a strict CSP compatible with the existing local assets. Keep loopback/Host/Origin enforcement. Replace raw internal detail with stable codes and safe client-owned wording.

- [ ] **Step 4: Replace UUID auth tokens and ordinary equality**

Generate 32 random bytes using the OS CSPRNG, encode them without logging, and compare secret material in constant time. Do not change non-secret event/session UUID semantics.

- [ ] **Step 5: Implement short-lived WebSocket ticket/subprotocol authentication**

The authenticated client obtains a single-use, short-lived ticket via a CSRF-protected POST; the WebSocket URL contains no bearer token. Performance remains below the existing 300ms budget.

- [ ] **Step 6: Run security/performance gates and write report; do not commit**

Run: `cargo test -p actrealm-server --offline && cargo test -p actrealm-server --test m5_performance --offline && node --test web/i18n.test.js && cargo clippy -p actrealm-server --all-targets --offline -- -D warnings`.

---

### Task 9: Add Explicit Backup Governance and Reproducible CI

**Files:**
- Modify: `crates/installer/src/lib.rs`
- Modify: `crates/installer/tests/m3_installer.rs`
- Modify: `crates/installer/tests/m4_statusline.rs`
- Modify: `crates/server/src/server.rs`
- Modify: `apps/macos/Sources/ActRealmUI/Views/SettingsView.swift`
- Modify: `web/index.html`
- Modify: `web/app.js`
- Modify: `.github/workflows/ci.yml`
- Modify: `.github/workflows/release-macos.yml`
- Modify: other tracked workflow files using mutable action tags
- Remove or sanitize: `docs/reports/evidence/2026-07-22/*`
- Modify: `.gitignore`

**Interfaces:**
- Backups carry unambiguous source identity and remain 0700/0600.
- Users can explicitly inspect and delete ActRealm-owned backups; no automatic deletion occurs.
- CI actions/toolchains are pinned to immutable approved revisions without adding Intel or update work.

- [x] **Step 1: Add failing backup ownership tests**

```rust
#[test]
fn backup_identity_distinguishes_same_basename_provider_files() { }
#[test]
fn explicit_backup_clear_removes_only_actrealm_owned_private_regular_files() { }
#[test]
fn backup_clear_refuses_symlinks_and_unmanaged_files() { }
```

- [x] **Step 2: Run RED, implement source-aware backup metadata and explicit clear, run GREEN**

Preserve the newest recoverable backup until the user explicitly confirms backup deletion. Surface backup count/size and a separate destructive confirmation in macOS/Web Settings.

- [x] **Step 3: Add a failing workflow immutability check**

Create a repository script that rejects `uses: ...@vN`, `@stable`, `latest-stable`, and unpinned install behavior in signing-capable workflows, while accepting Rust `1.97` and the intentional macOS runner label.

- [x] **Step 4: Pin workflow actions and audit installation**

Pin every third-party action to a full commit SHA and pin the cargo-audit version/checksum strategy. Do not add automatic updates or modify target architecture.

- [x] **Step 5: Remove public raw evidence from the current tree**

Keep a sanitized text index describing test names and outcomes. Remove screenshots, process lists, local paths, crash logs, and machine fingerprints from the current branch, and add an ignore rule for future raw evidence. Do not rewrite remote Git history and do not push.

- [x] **Step 6: Run task gates and write report; do not commit**

Run: `cargo test -p actrealm-installer --offline && cargo test -p actrealm-server --offline && ./scripts/check-ci-pins.sh && ./scripts/check-actrealm-language.sh && git diff --check`.

---

### Task 10: Documentation, Full Verification, and Local Candidate Installation

**Files:**
- Modify: `README.md`
- Modify: `docs/STATUS.md`
- Modify: `docs/V1_ACCEPTANCE.md`
- Modify: `docs/USER_GUIDE_zh-CN.md`
- Create: `docs/USER_GUIDE_en.md`
- Create: `docs/reports/ACTREALM_RELEASE_HARDENING_2026-07-27.md`

**Interfaces:**
- Documents exact supported scope: Apple Silicon, macOS 26, no auto-update, no 48-hour-soak claim, Web/macOS bilingual, local-only.
- Produces a locally installed candidate tied to the exact source SHA/diff and a user-facing manual acceptance checklist.

- [x] **Step 1: Update docs from verified behavior only**

Document state/recovery meanings, backup deletion semantics, Web language selection, security model, retention tables, and known deferred areas. Do not claim tests not run.

- [x] **Step 2: Run the full common gate**

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test --workspace --offline
cargo build --workspace --release --offline
./scripts/check-actrealm-language.sh
./scripts/check-runtime-language.sh
node --test web/i18n.test.js
node --check web/i18n.js
node --check web/app.js
apps/macos/Scripts/test.sh
plutil -lint apps/macos/Resources/Info.plist
git diff --check
```

- [x] **Step 3: Run milestone performance and security checks**

Run Hook p95, Runtime→WebSocket p95, 5,000-session snapshot, 500-session native projection, 1GB usage fixture, export/privacy, local network, permissions, response-header, and backup tests. The two-minute resource gate is allowed; the 48-hour soak is explicitly excluded.

- [x] **Step 4: Request final whole-branch review and resolve load-bearing findings**

Review every changed file against this plan and the repository AGENTS invariants. One fix wave maximum after final review, followed by one scoped re-review.

- [ ] **Step 5: Package and install the local candidate without publishing**

Quit the existing ActRealm, package from this worktree, verify the embedded Helper SHA and code signature, install to `/Applications/ActRealm.app`, relaunch, and run Doctor. Preserve the user's existing `~/.actrealm` data.

- [ ] **Step 6: Provide the manual acceptance checklist and wait for user approval**

The checklist covers truthful task/recovery state, OUTBOX approval/question/completion, background timers, English macOS/Web, quota refresh wording, Runtime restart, retention/export/backup controls, and p95 display. Do not commit or push until the user explicitly approves.

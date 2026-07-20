# ActRealm v1 delivery contract

Source baseline: `WIDGET_V1_PLAN.md` v1.1 dated 2026-07-15, amended by the
M0-M14 verification records and summarized in `STATUS.md`. This
repository file keeps the milestone gates next to the implementation; it does
not weaken or replace the full plan.

## Release shape

- One Rust binary, `actrealm`.
- macOS-first local runtime plus embedded 1600x600 native web UI.
- Three modules only: Attention, Agent sessions, and Quota.
- Claude Code and Codex local sessions are P0, whether launched from their CLI
  or macOS desktop app. A global provider CLI is not a prerequisite when the
  supported desktop app is present. Gemini round-level observation is P1 and
  cannot block v1 release.
- External Hook Control handles each official permission request through
  approve, deny, or pass-through. Multiple sessions may wait concurrently, but
  every waiter is request-keyed and receives exactly one outcome. v1.1 adds
  Claude's official AskUserQuestion/Elicitation Hook replies and an explicit,
  version-gated Codex app-server Connector for requestUserInput and Thread
  recovery. Interrupt, steer, Coach, cloud accounts, and telemetry remain out
  of scope.

## Milestone gates

### M0 - provider control path

- [x] Reference review records exact revisions, licenses, adopted decisions,
      rejected patterns, and milestone ownership.
- [x] Versioned, sanitized Claude and Codex fixtures from real probes.
- [x] Session, prompt, tool, permission, and stop events form basic state.
- [x] Real allow and deny produce subsequent provider evidence.
- [x] Undo before three seconds writes no provider decision.
- [x] Manual pass-through restores the native provider prompt.
- [x] Injected deadline pass-through works.
- [x] Claude uses a 24-hour human-approval budget and Codex uses 1 hour;
      automated tests inject short deadlines.
- [x] Hook stdin that never closes fails open within its 5-second budget.
- [x] Missing runtime completes within 200ms with empty stdout.
- [x] Killing the runtime while waiting immediately returns native control.
- [x] Another Codex hook denial cannot become a false confirmation.
- [x] Untrusted Codex hooks show not connected; a trusted probe connects.
- [x] Capability matrix and integration boundary report are current.

### M1 - runtime core

- [x] Core state machine and attention rules are fixture-tested.
- [x] SQLite WAL storage uses one writer and transactional business actions.
- [x] Envelope replay is idempotent.
- [x] Approve/deny/pass-through races have one winner.
- [x] Waiters are memory-only and stale approvals expire after restart.
- [x] Concurrent waiters are keyed by request/correlation ID, and duplicate
      requests resolve an older waiter without leaking its decision.
- [x] Socket half-close is not treated as user disconnect or auto-deny.
- [x] Stop remains turn-end; process liveness reconciles sessions that emit no
      terminal session event.
- [x] Runtime is single-instance; non-permission spool is bounded and replayed.

### M2 - API and minimum UI

- [x] Authenticated localhost API and WebSocket snapshot are implemented.
- [x] The fixed three-column 1600x600 UI has no fake data.
- [x] Attention supports approve, deny, undo, pass-through, ack, and snooze.
- [x] UI distinguishes pending, sent, confirmed, passed-through, and expired.
- [x] Real Claude and Codex approval paths pass end to end.

### M3 - installation and onboarding

- [x] Claude and Codex hook installation uses backup, semantic merge, lock,
      temporary file, and atomic rename.
- [x] Uninstall removes only ActRealm entries and preserves user semantics.
- [x] Installation intent is tri-state and repair never recreates intentionally
      removed or uninstalled hooks.
- [x] Stable hook binary installation, `CODEX_HOME`, canonical/legacy feature
      detection, and Codex trust guidance are implemented.
- [x] `doctor` reports CLI/version, configuration, runtime, trust/probe state,
      control loop, and pass-through.
- [x] `doctor` emits structured, repairability-aware issues and refuses to
      mutate malformed provider configuration.
- [x] `doctor` reports an overlong Unix Socket path before attempting Hook
      installation or Runtime startup.
- [x] Unknown fields and events are visible and never panic.
- [x] With neither Provider installed, the three-column workspace shows one
      consistent first-run state and never substitutes cached/demo tasks or
      quota for a live connection.
- [x] A visible Agent setup center maps Claude/Codex detection and status to the
      authenticated setup API; install, repair, reinstall, uninstall, refresh,
      and Codex trust guidance are real actions rather than placeholders.
- [x] Unsupported Provider placeholders are absent, and Codex trust remains a
      manual action in the official interface.
- [ ] The exact candidate passes local board 6/7 visual QA and real
      Claude/Codex install, trust, refresh, repair, and uninstall acceptance.

### M4 - quota, settings, and P1

- [x] Claude quota bridge never silently replaces an existing custom status
      line; explicit wrapper mode preserves its visible output and restores
      the complete original object on uninstall.
- [x] Codex rollout parsing is isolated, structurally validated, bounded, and read-only.
- [x] Stale quota keeps its last real value and capture time; missing or
      incompatible data remains honestly unavailable.
- [x] Notification, retention, export, and destructive-clear settings work.
- [x] Gemini round-level observation is intentionally not shipped in v1; it
      remains optional P1 and did not block either P0 provider.

### M5 - release qualification track

M5 is a parallel release gate. Later functional milestones can be implemented
without making the final v1 release complete.

- [x] Local metrics and JSON export match the plan definitions.
- [x] Oversize/deep JSON, host/origin/CSRF, socket permissions, and redaction
      tests pass.
- [x] Default logs contain no raw hook payload; diagnostic capture is explicit,
      redacted, bounded, and expires.
- [x] Hook non-blocking p95 is below 50ms; event-to-UI p95 below 300ms.
- [x] Idle runtime CPU is below 0.5%; short Runtime/browser resource gates are
      within budget.
- [ ] Runtime RSS remains below 80MB throughout a continuous 48-hour soak on
      the exact frozen release candidate.
- [x] Every pass-through path leaves the Provider interface usable.

### M6 - live sessions and Attention linkage

- [x] The main list keeps active, attention-bearing, and last-30-minute
      sessions only.
- [x] Attention-bearing sessions remain visible regardless of age.
- [x] Selecting an attention item selects, pins, highlights, focuses, and
      reveals its corresponding Agent session.
- [x] Agent rows expose factual thinking/tool/waiting/completed/failed/idle
      activity without inventing unavailable tool detail.
- [x] Timer ticks update text without rebuilding the full task row.
- [x] Claude and Codex use locally served image marks throughout the UI.

### M7 - dynamic quota and truthful timing

- [x] Quota renders every valid Provider window without a fixed weekly label.
- [x] Codex fixture families remain regression evidence while future versions
      are accepted only when the same bounded numeric schema validates.
- [x] Last valid quota values remain visible with their real capture time;
      stale age never fabricates freshness or percentage.
- [x] A Claude cache created after an unavailable snapshot refreshes
      immediately instead of waiting for the normal poll.
- [x] Existing Claude status-line output is preserved by explicit wrapper mode
      and restored exactly on uninstall.
- [x] Agent rows show factual total-turn time plus current-phase time.

### M8 - desktop compatibility and truthful control

- [x] Claude.app and ChatGPT/Codex.app can satisfy Provider discovery without a
      global same-name CLI.
- [x] Codex trust remains a user-controlled `/hooks` review and is never
      written or bypassed by ActRealm.
- [x] Provider-handled progress/resolution removes matching stale Attention and
      task waiting state only when a reliable signal exists.
- [x] Attention supports safe ignore/dismiss and a visible exit transition.
- [x] Jump labels/actions distinguish exact conversation, matching
      Terminal/iTerm, application-only, and unsupported environments.
- [x] Running presentation and turn start survive Runtime restart; old reply
      channels do not, and private jump locators never enter browser snapshots.
- [x] Token/usage fields appear only when supplied by a real structured source.

### M9 - Provider title consistency

- [x] Claude official `session_title` is accepted directly from the Hook.
- [x] Claude custom/AI JSONL titles and Codex `thread_name` are local-only,
      bounded compatibility sources; unknown or absent metadata falls back.
- [x] Provider title, project, and bounded current task remain separate fields;
      the UI intentionally renders title, task content, and model only.
- [x] Recent title changes refresh without restarting the Runtime.
- [x] SQLite v4 upgrades in place to v5 with no session loss.
- [x] Title priority, privacy, API snapshot, and UI rendering are automated.
- [x] Format, zero-warning Clippy, 127-test workspace suite, release build,
      five-round Claude/Codex control replay, and 120-second resource gate pass.
- [x] Exact release installed locally; Runtime/control loop, real Provider
      events, live title rendering, and browser console checks pass.
- [x] User completes Codex's official `/hooks` re-trust and accepts the build.
- [x] Local commit authorized by the user on 2026-07-17.
- [x] GitHub push authorized separately by the user after final local acceptance.

### M10 - configurable safe display

- [x] Settings provide concise, detailed, and developer profiles.
- [x] Task-card fields are selected from a server-owned allowlist and persist
      across Runtime/UI restart.
- [x] The safe catalog covers project/task/model/activity/plan/usage, sanitized
      current tool, permission mode, child-Agent count, environment, recovery,
      control and developer IDs when those facts exist.
- [x] A detail drawer uses only the same safe field catalog.
- [x] Unknown fields and raw/payload/full-command/transcript requests are
      rejected; raw Hook payload is never directly rendered.

### M11 - Agent questions in Attention

- [x] Claude AskUserQuestion supports one-to-four questions, choice,
      multi-select, and free input using official updatedInput.answers output.
- [x] Claude Elicitation supports accept, decline, cancel, typed fields, and
      official hookSpecificOutput.
- [x] Secret fields use password inputs and answers exist only in the DOM,
      authenticated request body, in-memory waiter, and one Provider response.
- [x] Answers never enter SQLite, logs, diagnostics, snapshots, or exports.
- [x] Expired questions cannot be submitted; handing a Claude question back to
      native Provider UI emits no directive.
- [x] Hook-only Codex sessions never display a fake direct-answer input.

### M12 - managed Codex and restart recovery

- [x] The Connector initializes with experimental API capability through a
      private persistent official app-server Unix Socket and proxy transport;
      it does not require Codex's standalone-only daemon command.
- [x] Explicit attach uses thread/list and thread/resume; managed Thread IDs
      persist and are rejoined after ActRealm restarts.
- [x] item/tool/requestUserInput maps to the same memory-only question model
      and writes ToolRequestUserInputResponse keyed by official question IDs.
- [x] External Hook sessions use captured parent-process liveness and expose
      observing, waiting-for-event, lost-control, or ended truthfully.
- [x] Managed sessions expose controllable only after a real app-server resume;
      other Codex sessions remain external_hook.
- [x] Runtime restart restores session display but expires every old approval
      and question waiter; no disconnected stdout/RPC continuation is revived.
- [x] Full workspace/release gates and the two-minute resource gate pass on the
      exact candidate.
- [x] Exact candidate is installed locally and accepted by the user on
      2026-07-17 before any commit or GitHub push.

### M13 - Provider-owned approval-state coordination

- [x] Codex auto-review/guardian ownership avoids a competing ActRealm
      blocking waiter.
- [x] Native `PreToolUse(request_permissions)` creates an observation-only
      waiting item with no replyable request ID.
- [x] Matching Provider lifecycle or managed Thread status resolves native
      waiting neutrally without claiming approve, deny, or execution.
- [x] Incidental running/tool activity cannot overwrite an explicit native
      waiting state.
- [x] Managed `waitingOnApproval` and Hook waiting deduplicate by Session.
- [x] Provider resolution clears notifications, stale Attention, task `等你`,
      and any competing unsent waiter transactionally.
- [x] Native approval UI exposes only original-Agent handling, snooze, and
      ignore; ActRealm-controlled approval retains allow/deny/pass-through.
- [x] Five-round native lifecycle replay, 153-test workspace suite,
      zero-warning Clippy, format, release build, two-minute resource gate, and
      isolated browser QA pass.
- [x] Candidate committed and pushed as `311306d` at the user's explicit
      direction.
- [ ] Real Claude/Codex Provider manual acceptance is recorded after reproducing
      both native waiting and native resolution on the installed candidate.

### M14 - Live usage, context, price, and OAuth quota

- [x] Claude transcript streaming updates are de-duplicated by stable
      message/request identity, including main/sub-Agent overlap.
- [x] Codex cumulative input/output does not add cached input or reasoning a
      second time; last-turn usage remains separate.
- [x] Context uses Provider current-turn fields and never lifetime Token divided
      by the model window.
- [x] Price is labelled `估算 API 价`; unknown/fast or unsupported models omit
      price instead of returning zero.
- [x] OAuth responses render all validated windows, active scoped models such
      as Fable, null reset times, and optional extra usage.
- [x] OAuth credentials remain memory-only and are excluded from cache,
      SQLite, logs, diagnostics, exports, and process arguments.
- [x] OAuth refresh runs outside the snapshot path and keeps the last validated
      value on missing credentials, 401, 429, network failure, or old samples.
- [x] Full 161-test workspace gate, zero-warning Clippy, release build, and
      two-minute resource check pass on the exact local
      candidate.
- [x] The exact M14 release was installed with matching SHA-256; schema 7,
      session usage, and OAuth quota windows were verified; the user accepted
      the candidate and authorized the local commit on 2026-07-18. Push remains
      separately gated.

### Post-M14 - stable live state and controlled Runtime recovery

- [x] WebSocket heartbeats expose a silent/stale channel instead of leaving the
      page falsely online.
- [x] A visible page performs a bounded authenticated snapshot fallback and
      reconnects with capped backoff; hidden pages avoid unnecessary polling.
- [x] Elapsed task/phase/Attention time updates every second without rebuilding
      unchanged cards, preserving selection, scroll position, and animation
      stability when new events arrive.
- [x] The authenticated health monitor reports only local operational metadata:
      Runtime/API/WebSocket/Hook health, event age, active/pending counts,
      SQLite event count, and controlled restart history.
- [x] One restart action re-execs the current binary on the same loopback port,
      recreates the private Hook socket, restores durable SQLite state, rotates
      browser authentication, and reconnects the current page.
- [x] Active reply waiters fail open before restart and are never restored as
      controllable requests; a fully stopped process still requires an explicit
      terminal launch.
- [x] The renamed merged tree passes 161 Rust workspace tests, 30 macOS client
      tests, zero-warning Clippy, format, JavaScript syntax, release build, five
      consecutive controlled restarts, and the two-minute resource gate
      (0.000% average idle CPU; 6,128 KiB maximum Runtime RSS).

### Post-M14 - usage, pricing, and OAuth hardening

- [x] Claude and Codex sessions without Provider cost fields use an embedded,
      dated per-model snapshot with separate `computed` and source labels;
      unknown, ambiguous, future, and fast variants still omit price.
- [x] Current desktop-picker coverage is explicit: Claude Fable 5,
      Opus 4.8/4.7/4.6/3, Sonnet 5/4.6, and Haiku 4.5; Codex GPT-5.6
      Sol/Terra/Luna and GPT-5.5. Older rollout prices are historical only.
- [x] A structured transcript/rollout model can fill a missing session model
      through schema 8 without overwriting an event-supplied model.
- [x] Codex cached input is priced at the cache rate and removed from ordinary
      input before calculation; Claude cache read/create remain separate.
- [x] A Codex model change prices only the new cumulative delta at the newly
      active model rate and does not reprice earlier session usage.
- [x] Claude OAuth parsing recognizes expiry metadata and performs a bounded
      preflight or 401 recovery through the official Claude CLI without
      reading or retaining refresh tokens.
- [x] OAuth subprocess execution is shell-free, output-free, time-bounded, and
      cooldown-limited; a request retry requires an actually changed token.
- [x] macOS credential lookup tries the fixed Claude Keychain service and a
      cached successful locator before bounded service enumeration.
- [x] Claude transcript aggregation uses incremental accumulators and retains
      only a 256-entry correction window per file; a 10,000-entry regression
      verifies exact totals and computed price after compaction.
- [x] A zero-token Claude `<synthetic>` entry contributes zero cost and cannot
      replace the real displayed model or suppress a complete estimate.
- [x] Focused quota/usage tests and focused zero-warning Clippy pass.
- [x] Full workspace format, zero-warning Clippy, 172 tests, release build,
      language contract, and the 120-second resource gate pass on the exact
      candidate; the resource sample records 0.000% idle CPU and 6,176 KiB
      maximum Runtime RSS.
- [ ] The exact tested release is installed and accepted locally by the user.
- [ ] Commit and push are separately authorized by the user.

### Post-M14 - first-run workspace and Agent setup center

- [x] Initial setup state is loaded before the first workspace render, avoiding
      a generic empty-state flash before `firstRun` is known.
- [x] The toolbar exposes truthful Agent connection state and opens one setup
      center for both first-run and returning users.
- [x] Provider rows disclose detected CLI/Desktop source, configuration path,
      backend status, and the exact next action.
- [x] Runtime events and a return to the visible page trigger bounded setup
      re-detection; offline mutation controls are disabled.
- [x] “查看接入指南” opens the maintained Chinese guide on the
      `agent/v1-full` branch.
- [x] An isolated ignored preview harness uses temporary config paths and does
      not touch the user's real Provider Hook files.
- [x] The isolated first-run preview harness ran for 300 seconds and exited
      cleanly in the user's ordinary terminal on 2026-07-20.
- [x] JavaScript syntax, embedded UI contract, format, zero-warning Clippy,
      full Rust workspace suite, release build, and language contract pass.
- [ ] macOS Swift and two-minute Runtime resource gates pass in an ordinary
      local terminal; the managed Codex sandbox rejected SwiftPM sandboxing and
      Unix Socket bind before these gates could execute.
- [x] Board 6 empty state and board 7 setup-center visual direction are
      accepted locally by the user on 2026-07-20.
- [ ] Board 7 real Claude/Codex detection, install, trust, refresh, repair, and
      uninstall behavior is accepted on the exact installed candidate.
- [x] The installed stable helper exactly matches the release candidate
      SHA-256; Claude/Codex Hook manifests, helper execution, canonical Codex
      feature, Codex trust hashes, and private Socket permissions pass Doctor.
- [ ] Fresh Claude and Codex sessions each deliver a real post-install event
      and appear as connected in the setup center.
- [x] The user explicitly authorized the local candidate commit on 2026-07-20
      with the remaining real-event and local resource gates still recorded as
      open.
- [ ] Push is separately authorized by the user.

## Publishing rule

Each milestone is implemented test-first. A test-candidate branch push requires
its automated/local gates and explicit user authorization. A milestone is not
marked accepted until its required manual evidence also passes. Failed or
incomplete gates remain visible and are never represented as complete in
documentation, tags, or releases. Merging to `main`, changing the default
branch, versioning, tagging, and publishing a release require separate user
approval.

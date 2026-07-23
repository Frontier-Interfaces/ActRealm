# ActRealm OUTBOX and Claude quota regression remediation

Date: 2026-07-23
Branch: `agent/v1-full`
Regression revision: `b8559af fix: close P0 release blockers`
Known comparison revision: `d9f206e`
Status: implementation, unrestricted local gates and packaged UI acceptance
pass. The reviewed CI baseline is green; the four follow-up fixes below remain
local and uncommitted pending user acceptance.

## User-visible failures

### OUTBOX native Codex approval disappears

- Trigger: Codex Desktop opens a native macOS permission sheet for
  `request_permissions`.
- Actual result on `b8559af`: OUTBOX contains only older completion rows, the
  native approval is missing, and the task is already marked completed while
  Codex is still waiting.
- Impact: the highest-priority user action is invisible, completion
  notifications are false, and the user cannot trust Agent Tasks or OUTBOX.
- Evidence: the user's 2026-07-23 12:10 screenshot and the
  [sanitized matching live event chain](evidence/2026-07-23/01-live-outbox-event-chain.log).

The affected session contained:

1. `approval.requested` for `request_permissions`;
2. `tool.finished` for the same synthetic permission tool;
3. `turn.stopped`;
4. an open Codex native permission sheet still visible on screen.

The reducer incorrectly treated steps 2 and 3 as proof that the Provider-owned
approval was resolved. That proof does not exist: Codex can emit both while the
macOS sheet remains open.

### Claude quota remains stale without explanation

- Trigger: wake from sleep or retain an old OAuth/status-line sample while no
  readable Claude OAuth credential is available.
- Actual result: the last valid percentage remains visible indefinitely and
  background OAuth errors are discarded.
- Impact: users cannot distinguish a valid unchanged limit from a failed
  refresh and have no recovery action.
- [Local non-secret evidence](evidence/2026-07-23/01-live-outbox-event-chain.log):
  the persisted quota table contained a current
  Codex sample but no current Claude sample; the fixed
  `Claude Code-credentials` lookup returned “item not found”. No token value
  was printed or stored during diagnosis.

### GitHub core gate fails

- Trigger: run the `b8559af` workflow.
- Actual result: both push and pull-request core gates fail while the language
  guard passes.
- Root cause: CI pins Rust 1.85 while locked `libsqlite3-sys 0.38.1` uses the
  newer `cfg_select!` build macro.
- Fix in the current candidate: workspace MSRV and CI both use Rust 1.97.

## Remediation

### Native approval lifecycle

- Preserve an open native approval across `PostToolUse`/
  `PostToolUseFailure(request_permissions)` and `Stop`.
- Do not mark the Turn complete or create completion Attention while that
  native approval is still open.
- Resolve only on an explicit later Provider transition: a different real
  tool activity, prompt, denial, interruption, failure, session end, or an
  authoritative app-server waiting-state clear.
- On Runtime restart or manual Connector attach, an initial app-server listing
  without `waitingOnApproval` no longer clears a Hook-observed native request.
  A newly spawned app-server may not see an already-running Codex Desktop Turn;
  ActRealm now requires a positively observed waiting state followed by a live
  clear before treating that channel as authoritative.
- After an authoritative clear, create the deferred completion item once and
  update the task state.

### OUTBOX interaction

- A newly arrived higher-priority item replaces a completion currently shown
  in the primary card; lower-priority arrivals do not interrupt an approval.
- Stable Attention IDs remain the selection key, so list reordering cannot
  point controls at another request.
- The primary card is anchored and automatically scrolled into view when the
  selected request changes.
- Provider-owned native cards expose only truthful actions: open the Provider,
  mark handled, or snooze. They never show a fake direct allow/deny control.

### Manual Claude quota refresh

- Settings now contains `Provider 数据 → 主动更新额度 → 立即更新`.
- The button calls a separate authenticated Runtime endpoint and waits for the
  OAuth request to finish.
- If no readable credential exists after wake/login, Runtime first asks the
  official `claude auth status --json` command to reconcile Provider auth,
  then repeats bounded credential discovery.
- Success requires a newer real Claude `capturedAt`; otherwise no success is
  claimed.
- Safe failure messages distinguish missing credential, rejected credential,
  rate limiting and service failure.
- Progress and the final result remain visible inside Settings; they are not
  hidden behind the separate main-window toast.
- The OAuth access token remains memory-only and is never returned to Swift,
  SQLite, logs, diagnostics or export.
- Sleep/wake retains the nonblocking refresh path; the explicit button uses the
  diagnostic wait path.

### Codex direct-control boundary

ActRealm can answer only request-keyed approvals delivered to the
ActRealm-owned app-server connection. Attaching an independently running Codex
Desktop Thread does not transfer an already-running Turn. The UI no longer
claims that a connector alone can approve the native sheet.

## Follow-up regression fixes

### Provider-owned plugin install and connect requests

- The missing GitHub, Gmail and Google Drive cards are Codex
  `request_plugin_install` native requests, not ordinary shell permissions.
- `PreToolUse(request_plugin_install)` now creates an observation-only native
  OUTBOX item and moves the matching task to `awaiting_approval`.
- The card names the sanitized plugin and truthfully directs the user back to
  Codex; it never presents a false ActRealm allow/deny action.
- Incidental notifications cannot clear the item. Codex emits the function
  output only after its native dialog resolves, so the matching
  `PostToolUse`/failure is treated as authoritative completion and the card is
  resolved with neutral wording.
- `request_permissions` keeps its stricter existing rule because Codex can
  emit its synthetic `PostToolUse` and `Stop` while that permission sheet is
  still visible.

### Background live rendering and latency

- A visible ActRealm window no longer pauses merely because Codex or another
  app is frontmost. Live elapsed time, phase time, quota age and snapshot
  projection continue while the user works elsewhere.
- Rendering still pauses when the ActRealm window is minimized or genuinely
  occluded.
- The previous active-app pause accumulated old event timestamps and then
  counted the catch-up render as multi-second UI latency. Removing that pause
  restores current-event latency accounting.
- Packaged acceptance with another app frontmost showed the task and Runtime
  timestamps continuing to advance. The same candidate measured 129 ms UI p95
  instead of the reported 6,000+ ms regression.

### Actionable Claude refresh recovery

- Manual refresh failures now distinguish missing credentials from rejected
  credentials.
- Both cases explain the real dependency: start Claude Code CLI, finish login
  or begin a session so the official CLI can refresh its credential, then
  return to ActRealm and press “立即更新”.
- Settings shows this prerequisite before the user presses the button, and the
  timeout path repeats it instead of claiming a successful refresh.

### Evidence-based Runtime status

- “Runtime 已启动，但控制连接尚未恢复” is now a temporary state, not a
  persistent action log.
- A later live WebSocket transition reconciles it to
  “Runtime 已重新启动并恢复连接”.
- If the connection later drops, stale success wording is removed.
- Packaged acceptance exercised a real Runtime restart: the UI moved from
  “正在重启” to “运行正常 / Runtime 已重新启动并恢复连接” in about four
  seconds.

## Focused verification

| Gate | Result |
| --- | --- |
| native approval survives same-tool PostToolUse and Stop | PASS |
| authoritative Provider clear resolves native approval | PASS |
| no premature completion while native approval is open | PASS |
| new approval replaces visible completion selection | PASS |
| lower-priority item does not steal approval selection | PASS |
| manual quota capture comparison ignores Codex samples | PASS |
| manual quota error text is actionable and credential-free | PASS |
| provider-owned plugin request enters and leaves OUTBOX on authoritative events | PASS |
| visible non-key window continues live rendering | PASS |
| minimized or occluded window pauses rendering | PASS |
| Runtime status warning reconciles after a live connection | PASS |
| macOS Swift suite | PASS, 88 tests |
| focused Runtime and Server tests | PASS |
| zero-warning Server Clippy | PASS |
| workspace format and diff checks | PASS |
| workspace zero-warning Clippy | PASS |
| full unrestricted Rust workspace tests, including Hook/Bridge/API listeners | PASS |
| workspace release build | PASS |
| language and plist checks | PASS |
| RustSec audit | PASS, 1,167 advisories / 132 dependencies |
| local arm64 app package and ad-hoc codesign | PASS |
| packaged native approval enters OUTBOX | PASS |
| packaged native approval survives `PostToolUse(request_permissions)` and `Stop` | PASS |
| packaged native approval can be locally acknowledged without deciding Provider permission | PASS |
| restart listing without waiting flag preserves Hook native approval | PASS |
| Settings keeps quota progress and rate-limit failure visible | PASS |
| two-minute resource harness | PASS, idle CPU 0.003%, Runtime RSS max 6,672 KiB |
| packaged UI p95 after background-render fix | PASS, 129 ms |
| reviewed remote GitHub CI baseline | PASS |
| follow-up fixes commit/push | pending user acceptance |

The previously blocked listener tests were rerun after the local sandbox was
removed. The first unrestricted run exposed one obsolete end-to-end assertion
that still expected `PostToolUse(request_permissions)` to clear native
approval. The test contract now verifies preservation through both incidental
events and resolution only after a different real tool starts. The full rerun
passes. No commit or push is authorized by this report. Exact GUI and resource
evidence is retained in
[03-unrestricted-packaged-acceptance.log](evidence/2026-07-23/03-unrestricted-packaged-acceptance.log).

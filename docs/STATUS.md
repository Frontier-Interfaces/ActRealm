# ActRealm current status

Last reviewed: 2026-09-17

## 2026-09-17 English follow-up — build 123

Build 123 fills missing Data settings, pricing-source and quota tooltip
translations, fixes singular quota and task-count labels, and extends the native localization
guard to custom view labels. The installed candidate was inspected in English;
404 Rust and 211 Swift tests and the required local gates passed. Follow system
remains the default, while explicit language choices stay local.

The public candidate is under review in
[PR #10](https://github.com/Frontier-Interfaces/ActRealm/pull/10), targeting
`agent/v1-full`. CI status is tracked in the PR. Earlier uncommitted/local-only
notes below describe the state at those milestones; the candidate source is
now included in this PR. No merge or public release is implied.

## 2026-09-16 System-default language and English UI — build 121

The native UI keeps System as its default language and preserves explicit local
choices. English window/menu labels, usage metadata, picker options, dates,
chart axes and compact copy have been checked across native surfaces. A static
localization guard complements the unit and visual checks.

The public candidate is prepared on a branch from `agent/v1-full`; source-install
links refer to Frontier-Interfaces/ActRealm. See
[NATIVE_ENGLISH_LOCALIZATION_2026-09-16.md](reports/NATIVE_ENGLISH_LOCALIZATION_2026-09-16.md)
for scope, validation and remaining release qualification.

## 2026-09-16 Native status and activity parity — build 120

Native task cards now emphasize semantic actions and explicit targets instead
of the shell transport name. Workflow rows preserve start metadata, distinct
file targets and verifiable outcome boundaries; pending decisions and completion
confirmation are presented distinctly. The bounded Runtime classifier also
recognizes code checks and CUA/code-runner activity. Web source is unchanged.
See [NATIVE_ACTIVITY_PARITY_2026-09-16.md](reports/NATIVE_ACTIVITY_PARITY_2026-09-16.md).
The source remains uncommitted and local-only.

## 2026-09-16 Native approval parity — build 119

The native workspace, HUD and menu bar now consume Runtime `allowedActions`,
matching Display's approval scope. Risk labels remain visible and do not impose
an additional Git-only allowlist. Missing, expired, observation-only or closed
reply channels cannot gain approval controls. The shared command handler also
rejects allow for a request whose declared capability is deny-only.

The three-second undo default remains. The retired Web UI was not changed.
The candidate retains the Claude quota recovery and Fable parsing fixes from
builds 117–118. Verification is recorded in
[NATIVE_APPROVAL_PARITY_2026-09-16.md](reports/NATIVE_APPROVAL_PARITY_2026-09-16.md).
The source is uncommitted; no push or public release is implied.

## 2026-09-16 Token accounting and task cards — build 116

The installed local candidate restores the complete native Token dashboard,
background updates, partial-history presentation, exports and Display-style
card deletion. Claude aggregate and model-scoped weekly limits retain distinct
labels, and historical quotas remain explicitly stale.

The collector now reconciles resumed Codex files deterministically, deduplicates
response-keyed usage, preserves legacy prefixes, and handles explicit compaction
counter epochs. Numeric checkpoint schema 6 rebuilds earlier cached facts.
Verification includes real-log repeatability and a live Runtime restart; see
[TOKEN_USAGE_RESTORE_2026-09-16.md](reports/TOKEN_USAGE_RESTORE_2026-09-16.md).
The source remains uncommitted and retains the local-only product boundary.

## 2026-09-16 Local-only candidate

The current source removes cloud identity, sharing, teams, event-context
transfer, Firebase services, crash uploads, mobile/Watch clients, and their
backend/contracts/packaging dependencies. Local Runtime, native/Web clients,
Agent Focus, task history, usage, and loopback Companion control remain.

Schema 38 removes retired collaboration metadata while preserving local tasks
and usage. Explicit Codex terminal records repair matching current turns;
missing process evidence moves running work to an unconfirmed state without
inventing completion. Task Token visibility now uses per-task coverage rather
than global historical-index completeness.

Verification and local installation are recorded in
[local-only verification](reports/LOCAL_ONLY_2026-09-16.md). Historical sections
below describe earlier candidates and do not define current cloud capability.

## 2026-09-09 Companion v5 candidate

The current `actrealm最新版` candidate adds bounded local result excerpts and
referenced-file reveal, current-turn Codex tool observations, asynchronous
question projection/routing, and nonblocking hourly model-price refresh.
Provider-native questions remain observation-only unless an attached Connector
supplies a valid live reply route. Independent app-server startup listings do
not mark externally owned Desktop turns finished.

Result excerpts and question contents remain transient. Offline event replay
now keeps allowlisted lifecycle metadata and numeric exit status instead of
raw prompts, replies, commands or tool input/output. Repeated metadata and
usage refreshes do not advance a task's meaningful Provider-event timestamp.
The native reveal target is available only after current-turn, file-identity
and `session.jump` checks; it is not added to general snapshots or exports.

All required local gates passed on 2026-09-09: workspace formatting, Clippy with
warnings denied, offline Rust tests and release build, language contract checks,
and the UTC macOS suite. Exact counts and scope are recorded in
[Companion v5 verification](reports/COMPANION_V5_2026-09-09.md).
The user explicitly authorized committing and pushing this candidate to
`actrealm最新版`. This is not a version tag, public release or merge to `main`.

The Build 98 installation notes below are retained as historical context.

## 2026-09-09 First-run Codex metrics

The source candidate fixes a reproduced fresh-database gap: an already parsed
Codex task can expose Token and context metrics while unrelated history is
still being indexed. The numeric fallback is memory-only, fills missing UI
fields, rejects model mismatches and preserves the canonical historical ledger.
Companion protocol v6 lets Display reject an older collector during installation.

The isolated regression checks 12,800 Tokens and 6% of a 200,000-token context
window in both Runtime and Companion projections before the first ledger commit;
SQLite remains empty until historical scanning completes. Full Rust tests,
Clippy, release build, language/CI contracts and 198 native tests passed locally.
This is local automated evidence, not acceptance on the reporting user's Mac.
The current repository/branch is ActRealm-Cloud / `actrealm最新版`; the README
and both source-install guides have been corrected. The previous build-98
sections below are historical product qualification records.

## 2026-08-27 Build 59 surface restoration

The current working candidate restores the Build 59 macOS product surface from
`f3e1e3689995432f0d4384f4206cd9fdd7a722e4` while retaining the Build 97 Token
ledger and attribution, internal-task filtering, optimistic local archive,
layered diagnostics, fail-closed approval safety and current Runtime/API
contracts. History is no longer a top-level destination; Team/Cloud,
Companion, the full paged Workflow, Developer/custom task-card controls,
animated themes, local statistics and the Build 59 three-column workspace are
visible again. Review, metadata Checkpoint, mobile/Watch and Cowork remain
parked. Exact scope and verification are recorded in
`ACTREALM_BUILD59_SURFACE_RESTORE_2026-08-27.md`.

Current installed local candidate:
`fad854873ff08ea993a8f6c4ba6230e2b51193a1` on `actrealm最新版`, Apple
Development signed build 98. Build 97 is preserved in Trash at
`~/.Trash/ActRealm-build97-before-build98-20260827.app`.

Build 98 restores the Build 59 product surface without reverting the Build 97
Runtime or Token ledger. It keeps resumable and atomic usage collection,
project/task attribution, current fake-task filtering, optimistic local
archive, layered diagnostics and fail-closed approval safety. Team/Cloud and
local Companion are active again; Review, metadata Checkpoint, H6 mobile,
Watch and Cowork remain parked. Agent Focus is unchanged.
The complete audit issue inventory, status of every item, and R0–R5 execution
sequence are maintained in
`ACTREALM_PRODUCT_REFRAME_EXECUTION_PLAN_2026-08-25.md`.
Current R0 and follow-up evidence is recorded in
`reports/ACTREALM_R0_R4_PRODUCT_REFRAME_2026-08-25.md`.

Current working candidate: `actrealm最新版`. H6 mobile, Watch, Claude Cowork,
the later History destination, current Review and metadata Checkpoint remain
parked and are not part of this acceptance round; their existing data/code and
historical evidence are preserved for a separately authorized continuation.

The prioritized follow-up work, evidence gates, milestone acceptance criteria,
resource budgets, and Display vertical-slice plan are maintained in
`ACTREALM_HUMAN_CONTROL_PLANE_UPGRADE_PLAN_2026-08-18.md`.

Candidate state: build 98 is installed locally, Apple Development signed and
arm64. Schema 4 uses a monotonic cumulative envelope and an immutable first
Session ID. The installed ledger remains verified and continues to use the
Build 97 project/task attribution contract. Doctor passes 14/14, strict
codesign passes and Computer Use confirms the fixed three-column workspace,
top-level Join, full paged Workflow and restored display controls. The separate
seven-day soak remains unclaimed. Nothing has been tagged, notarized or
publicly released. The previous 20-task Review gate remains retired because
Review is parked rather than presented with untrusted attribution.
The exact convergence scope, data backup and acceptance boundary are recorded
in `ACTREALM_CONVERGENCE_REPAIR_2026-08-26.md`.

The Build 97 Token attribution correction separates project facts from
task recovery, adds path-free Provider metadata and parent-session lineage, and
is recorded in `reports/ACTREALM_TOKEN_ATTRIBUTION_REWORK_2026-08-26.md`.

This is the short current source of truth. Historical milestone detail remains
in `V1_ACCEPTANCE.md` and the milestone verification records.

## Supported candidate scope

- Apple Silicon only.
- macOS 26; native CI uses Xcode 26.6.
- Rust 1.97.
- Local Claude Code and Codex sessions through installed Provider Hooks.
- Direct actions only when ActRealm owns a live official reply channel.
- Native macOS and embedded Web clients.
- System/Simplified Chinese/English presentation. Provider-authored and
  user-authored text remains verbatim.
- Loopback HTTP/WebSocket and current-user Unix sockets only.
- Local SQLite persistence, bounded retention, explicit export, diagnostics,
  and source-aware backups.

The candidate does not claim Intel support, automatic updates, a completed
48-hour soak, accessibility qualification, Windows support, Gemini support, or
a publicly signed installer.

## Local Companion integration (2026-08-07)

ActRealm now has an explicit, local-only boundary for a display Companion.
Settings can generate a five-minute, one-use pairing code with read/jump and
optional respond scopes, list paired applications, and revoke them. The server
stores only hashed tokens in a private file and exposes a strict allowlist
snapshot; real actions continue through the existing Runtime command and
question paths with current waiter/capability/expiry validation.

Runtime restart recovery uses a `0600` discovery descriptor containing only
the current loopback endpoint and instance ID. It contains no token, Provider
locator, Cloud credential, or Agent content. This is a local development
integration, not a public remote-control API. See `COMPANION_PROTOCOL.md`.

The 2026-08-17 working candidate advertises Companion protocol v2. Protocol v2
adds a bounded current-turn activity route required by Display while retaining
the schema-v1 privacy allowlist. A protocol-v1 Runtime is intentionally treated
as incompatible by Display rather than being consumed optimistically.

## Release-hardening progress

The 2026-07-27 plan contains ten tasks.

| Task | Result | Verified outcome |
| --- | --- | --- |
| 1. Session truth and recovery | Complete | Execution state and recovery/control capability no longer contradict each other; stale local ownership is normalized |
| 2. Bounded snapshots and retention | Complete | UI snapshots filter at SQL level, related rows are batched, expired closed graphs are pruned transactionally, actionable work is preserved |
| 3. Bounded usage discovery | Complete | Directory traversal and oversized first reads are bounded and report partial/unavailable truth instead of false totals |
| 4. Incremental native projection | Complete | Stable per-session facts prevent quota/metric/clock changes from rebuilding unchanged task cards |
| 5. macOS English localization | Complete | ActRealm-owned native copy uses stable localized keys; Provider/user text remains unchanged |
| 6. Web English localization | Complete | Web supports System, Simplified Chinese, and English without mutating Runtime settings |
| 7. Stage Manager and process safety | Complete | Ownership survives relaunch, process execution is asynchronous, and PID reuse is identity-checked |
| 8. Local Web security | Complete | CSPRNG secrets, constant-time comparison, one-use WebSocket tickets, strict Origin/Cookie/CSRF checks, CSP/security headers |
| 9. Backup governance and reproducible CI | Complete | Private source-aware backups, explicit deletion, immutable Action SHAs, pinned tools, raw evidence removed from the current tree |
| 10. Documentation, full verification, installation | Complete | Documentation, full gates, final review, package/signature checks, local installation, Doctor, and user acceptance passed |

## Current verified gates

Task 9 completed with:

- installer: 16 integration tests and 4 statusline tests passed;
- server: 27 unit, 7 API, and 3 performance tests passed; 2 manual previews
  remained intentionally ignored;
- macOS: 23 suites and 130 tests passed;
- CI immutability, language contracts, and `git diff --check`: passed.

These are scoped Task 9 results, not the final Task 10 release result. The final
whole-workspace counts and performance/security checks will be recorded in
`reports/ACTREALM_RELEASE_HARDENING_2026-07-27.md`.

## Product truth

### Provider control

- External Hook approval is request-keyed and supports allow, deny, or
  pass-through.
- Claude `AskUserQuestion` and `Elicitation` can be answered only while their
  official blocking Hook waiter is alive. Answers remain memory-only.
- Codex direct question/approval actions require an explicitly attached,
  version-gated app-server connection and a matching live request.
- Provider-native `request_permissions` / `waitingOnApproval` is observation
  only. ActRealm opens the Provider interface; it does not invent allow/deny
  controls or infer the result.
- Restart never restores an old Hook stdout/RPC waiter. Durable history may be
  shown, while control returns only after a new verified Provider event or a
  managed Thread reconnection.

### Data and retention

- Raw prompts, complete commands, tool input/output, transcripts, file contents,
  tokens, and complete local paths are not persisted by default.
- UI snapshots include only recent or actionable sessions; full export remains
  a separate path.
- Client retention choices are 30, 90, 180 days, or forever. Closed expired
  session graphs are removed transactionally; actionable attention and live
  state are preserved.
- Provider configuration backups are source-aware, private (`0700` directory,
  `0600` files), and never deleted automatically.
- Settings shows backup count/size. Deletion is a separate operation requiring
  exact `DELETE BACKUPS`; unsafe or unknown entries cause refusal.

### Security

- The Web UI is embedded in the Runtime and served on a random loopback port.
- Session and CSRF credentials are 32-byte OS-random secrets and are never put
  in the WebSocket URL.
- WebSocket access uses a short-lived single-use ticket sent as a subprotocol.
- Web and native clients use authenticated Runtime APIs and never open SQLite
  directly.
- Runtime/Agent contents are not telemetry. The Cloud candidate can send an
  independently disableable, strict crash summary containing only version,
  OS, exception, and ActRealm symbol fields; raw `.ips`, paths, accounts,
  prompts, commands, and tokens remain local. No CDN or outbound update check
  is present.

## Remaining release work

1. Complete the product-reframe acceptance gates recorded in
   `ACTREALM_PRODUCT_REFRAME_EXECUTION_PLAN_2026-08-25.md`.
2. Request separate authorization for merge, tag, public signing/notarization,
   and release.

## Release decision

- Development testing: allowed.
- Historical 2026-07-27 local release-hardening candidate: accepted.
- Current build 88 product-reframe candidate: engineering gates passed; user
  acceptance and long-duration gates remain open.
- Commit/push to `actrealm最新版`: authorized and complete through the installed
  code baseline above.
- Public v1 release: not declared.

## Cloud team development candidate (2026-08-03)

The candidate on `agent/team-collaboration-v2`, based on
`0859b3a86fc1d41ff6f7ceb6b3354a6728c2d618`, extends Team v2.1 without changing
the public-release state above. It includes:

- Google-first identity, device-bound host credentials, team membership,
  status-only task projections, retained comments, and default-deny remote
  operations;
- complete team management for rename, invitation copy/revoke, member roles,
  removal, ownership transfer, leave, and retryable deletion;
- every locally generated active invitation retains its own copyable secret
  across refreshes, and a completed cloud mutation is not misreported as
  failed merely because the following local refresh is delayed;
- safe per-task visibility with persisted private defaults and deletion
  tombstones, so enabling the team default does not expose existing tasks;
- packaged localization lookup that stays inside the App bundle during window
  restoration, plus asynchronous Keychain I/O so a locked keychain cannot
  block the macOS main thread;
- transaction-time membership enforcement, bounded invite/projection quotas,
  latest-comment pagination, explicit Firestore deny rules, TTL/index
  definitions, and per-device presence limiting.
- truthful account-switch quota handling: Claude credentials and bounded
  profile service names are rediscovered on every poll, ambiguous profiles
  fail closed, failed refresh state remains stale across polling and Runtime
  restart, and Codex credential changes are detected without reading provider
  credential contents;
- coordinated Codex reconnect: automatic Runtime restart preserves the private
  authenticated browser session, cancelled or timed-out restart requests cannot
  execute later, the old app-server child is reaped, and failed reconnects
  release tracked Codex waiters and surface an explicit manual-restart state;
- UTC-pinned native CI assertions and Node 24-compatible immutable Action pins.

Current verified gates: Rust fmt, zero-warning clippy, full workspace tests,
release build, language contracts, immutable CI pins, and RustSec audit pass;
the UTC-pinned macOS suite passes, including 146 Swift Testing cases, and the
packaged Apple Silicon app passes strict deep signature verification. Functions
has 34 unit tests, Firestore Rules has 81 cases, and the Node 22 multi-identity
Firebase Emulator integration flow passes end to end. The Firebase source in
this candidate is unchanged from the already deployed `actrealm-share-dev`
rules, indexes, and Functions. PR merge, public App Check enforcement, public
signing/notarization, release, and two-Mac UI acceptance remain separate work.

## H6 paused mobile remote-approval candidate (2026-08-21)

H6-B adds server-private FCM endpoint registration/rotation/revocation, a frozen
opaque notification with generic lock-screen copy, iPhone APNs/FCM handling and
fresh-state fetch, and an iPhone-relay-only Watch boundary. A dedicated Firebase
iOS App is registered in `actrealm-share-dev`; no APNs private key was created
or uploaded. Commit `596d68816510b53ef2e169e718966b865af904de` Functions,
Rules and indexes are deployed to the development project with App Check still
in monitor mode; the H6 functions are Node.js 22, `ACTIVE`, min 0 and max 2.
Functions 41/41, Rules 85/85, the full Emulator flow, iOS source-check and 4/4
Simulator tests pass. Apple Developer reports that the current account cannot
create a Key and must contact a Team Admin. Real iPhone/Watch provisioning and
the 30-request safety matrix remain blocking.
See `reports/ACTREALM_H6B_OPAQUE_PUSH_2026-08-21.md`.

H6 is deliberately paused at the Apple Team Admin permission boundary. Resume
from `reports/ACTREALM_H6_PAUSE_HANDOFF_2026-08-21.md`; do not repeat H6-A,
H6-B, Firebase app registration, Cloud deployment, or the completed local
gates. The first resume action is APNs Key authorization/configuration, followed
by provisioning and the real-device matrix. Display work is not included in
this paused H6 scope.

## H7 layered diagnostics candidate (2026-08-25)

H7 implementation, automated gates, and exact installed-candidate UI checks are
complete; the user's final control-plane decision remains open. Exact commit
`5ddcfb0fbd24364c3e3a84163eaf0e195d004056` is installed as Apple Development
signed build 81, and Doctor passes overall, control-loop, Claude, and Codex real-
event checks. The
authenticated Runtime diagnostics schema v2 reports only bounded operational
facts for Runtime/Hook, Provider/Connector, Review/Git, Token collection,
Companion, and projection/UI layers. SQLite schema and cached `quick_check`,
snapshot freshness, Runtime commit/protocol, Provider versions, and neutral H6
and Cowork conditions are visible without exposing prompts, commands, paths,
credentials, tokens, transcripts, file contents, or reply channels. The native
panel provides layer-scoped recovery and keeps Runtime output collapsed under
technical details. See
`reports/ACTREALM_H7_LAYERED_DIAGNOSTICS_2026-08-25.md`.

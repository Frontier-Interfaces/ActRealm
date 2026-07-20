# M13 Provider state coordination verification

Status: implementation, five-round process replay, full workspace gates,
release build, exact two-minute resource gate, isolated browser QA, and
exact-tree local installation pass. The candidate was committed and pushed as
`311306d` at the user's explicit direction. Real Provider user acceptance
remains pending, so M13 is not yet marked manually accepted and does not make
v1 a final release.

## Problem reproduced

Two distinct Provider races produced the screenshots reported on 2026-07-17:

1. Codex automatic review could decide a permission while ActRealm's
   concurrent Hook still created a second blocking waiter. The panel therefore
   kept an obsolete approval and notification.
2. Managed Codex conversations expose native approval through app-server
   `ThreadStatus.activeFlags = ["waitingOnApproval"]`. ActRealm stored this
   flag but did not project it into attention or session state, and its
   `thread/started` handler ignored the embedded Thread object.
3. Real Codex Desktop 0.144.5 sessions also emitted an official Hook lifecycle
   around the native sheet: `PreToolUse(request_permissions)` while the sheet
   was visible, followed by `PostToolUse(request_permissions)` after the user
   handled it. ActRealm previously classified the first event as an ordinary
   running tool and therefore showed `在跑` with no attention item.

The implementation follows the reducer boundary used by open-vibe-island:
create an actionable waiting state from an explicit permission-request signal,
protect that state from incidental running updates, and clear it only on an
explicit resolution signal. Resolution is deliberately neutral; disappearance
of the native sheet never proves approve, deny, or command execution.

## Delivered behavior

- Codex Hook payload aliases `approvals_reviewer`, `approvalsReviewer`, and
  `_approvals_reviewer` detect `auto_review` and `guardian_subagent`.
- The effective Codex profile in `config.toml` is a bounded, read-only fallback
  for builds that omit the reviewer field. A payload reviewer always wins.
- Provider-owned approval is ingested as observation only: no ActRealm
  waiter, no approval card, and no allow/deny directive.
- Codex `PreToolUse(request_permissions)` is normalized as an observed native
  request. It creates a `native_approval` item with no `request_id`, so the UI
  cannot submit an approval or denial.
- Codex `PostToolUse(request_permissions)` resolves only the matching native
  request as `provider_handled`. It does not record approve, deny, or execution.
- Incidental Notification/tool activity cannot replace a live native waiting
  state. This mirrors open-vibe-island's actionable-state preservation rule.
- For managed conversations, `thread/started`, `thread/status/changed`, `turn/started`, `turn/completed`,
  and `thread/closed` update the Connector's Thread state.
- Native `waitingOnApproval` creates one `native_approval` attention item and
  projects the session to `awaiting_approval` / `terminal` ownership.
- A Hook approval already covering the same Session suppresses the synthetic
  native card, so the UI never shows duplicate approval items.
- When managed native waiting clears, ActRealm resolves both synthetic and competing
  Hook approvals transactionally, cancels an unsent decision, releases the
  live Hook with pass-through, clears session ownership, and projects either
  `thinking` or `response_finished` from the authoritative Thread status.
- Tool progress, denial, Stop, failure, new prompt, and session end still
  reconcile approvals for both Claude and Codex Hook paths and now release the
  matching in-memory waiter as well as updating SQLite.
- The active notification banner closes when its attention item is no longer
  open. An older result is not rendered beneath a different active item.
- Native approval UI exposes only honest actions: `去 Agent 处理`, `待会提醒`,
  and `忽略`; it never pretends that ActRealm can answer a native-only prompt.
- Wording is capability-specific: native items say `原界面请求批准 / 仅同步状态`;
  replyable Hook items say `可在 ActRealm 审批`.

## Provider coverage

| Surface | Waiting source | Resolution source |
| --- | --- | --- |
| Claude CLI | official Hook | PostToolUse, PermissionDenied, Stop/failure/session events |
| Claude Desktop | shared official Hook configuration | same Provider events as CLI |
| Codex CLI | official Hook; auto-review is Provider-owned | matching Hook lifecycle; managed app-server status when available |
| Codex Desktop | official `request_permissions` Hook lifecycle; managed app-server status is a second signal | matching PostToolUse/terminal Hook event; managed app-server status clear |

## Automated evidence

Targeted tests cover:

- raw and profile-based auto-review deferral;
- native approval open, neutral resolution, and ended projections;
- five consecutive process-level `request_permissions` Hook lifecycles;
- incidental activity preserving the actionable waiting state;
- native requests never producing Hook stdout or a replyable request ID;
- `thread/started` parsing and incremental status changes;
- native resolution releasing a competing Hook waiter;
- Claude and Codex Provider-side tool progress resolving Widget waiters;
- no duplicate native card when a Hook approval is already live;
- notification removal and JavaScript syntax.

Exact-tree gates:

| Gate | Result |
| --- | --- |
| targeted Provider coordination replay | PASS, 5/5 consecutive real-process rounds |
| dedicated runtime/provider regressions | PASS |
| `cargo test --workspace --no-fail-fast` | PASS, 153 passed; 2 explicit/manual gates ignored |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS, zero warnings |
| `cargo fmt --all -- --check` | PASS |
| `node --check web/app.js` | PASS |
| `git diff --check` | PASS |
| `cargo build --workspace --release` | PASS |
| exact two-minute resource gate | PASS, idle CPU 0.000%; Runtime RSS max 5,808 KiB |
| isolated browser native request/open/resolution replay | PASS |
| exact-tree local installation | PASS, source/install SHA-256 `9a41ca3938f6afb2086e3fb2cb9edc25974a5af1a62ec592c18f55597c09eb68` |
| real Codex user acceptance | PENDING |

The isolated browser replay verified that a native request renders as
`原界面请求 / ActRealm 仅同步状态`, exposes only `去 Agent 处理 / 待会提醒 /
忽略`, and disappears after the matching provider lifecycle closes. The task
then shows the neutral activity `权限请求已在 Codex 原界面处理`; it never says
approved, denied, or executed.

## Release boundary

- The test-candidate commit and branch push have already occurred by explicit
  user direction. Do not mark M13 manually accepted, merge it into a release,
  tag it, or use it to declare v1 complete until the installed candidate
  reproduces native waiting and native resolution against real Provider
  sessions and the user records the result.
- ActRealm does not infer the content or outcome of a native-only approval.
  It reports where the decision must be made and follows the Provider's
  authoritative waiting state.

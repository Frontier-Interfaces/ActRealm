# ActRealm convergence repair

Date: 2026-08-26

Branch: `actrealm最新版`

Installed rollback baseline: Build 88, code commit `d6c891f`.

## Product boundary

The current candidate is a local human control plane for Claude Code and Codex.
Its default surface answers only:

1. which Agent needs the user;
2. whether active work is healthy and what it is doing;
3. how to handle or return to the correct Provider;
4. how ended work is archived, found, opened or deleted.

Agent Focus is frozen by user decision. Its HUD, display/workspace binding,
Stage Manager behavior, pointer acceptance, timing and return rules are not
changed by this convergence pass.

## Kept in the production surface

- request-keyed Attention and OUTBOX;
- active Claude/Codex task status;
- safe approval/question/error/completion handling;
- truthful Provider handoff;
- official quota windows and reset times;
- Hook setup and Runtime recovery;
- local History search/open/delete;
- retention, export, backup and clear controls.

## Parked without deleting existing data

- current ActRealm Review and Review Baseline collection;
- metadata-only Checkpoint presentation;
- Team/Cloud polling, task projection, menus and notifications;
- mobile/Watch/Cowork work;
- formal Companion/Display management entry points;
- partial/suspect Token totals, API-equivalent cost and analytics;
- animated image/video themes and decorative local usage statistics.

## Implemented repairs

### Source and task truth

- known Codex internal Overview/safety prompts are suppressed independent of
  their cwd;
- absolute-path Codex executable probes are also suppressed and cleaned from
  existing active-task state;
- the exact Codex `/hooks` setup command, including its observed single-backtick
  presentation, is suppressed as configuration UI without hiding other slash
  commands that may perform real work;
- existing exact internal sessions are removed transactionally on Runtime open;
- known markup whitespace emitted around a Prompt is normalized in the bounded
  task summary instead of showing strings such as `&#x20;`;
- local archive disappears immediately, then rolls back visibly if the Runtime
  rejects it;
- parked Cloud cannot block local archive;
- task ordering uses Attention/error priority before recency.

### Workspace convergence

- Review/Checkpoint presentation and network reads are disabled;
- routine Bash/MCP/read/search rows do not enter important status;
- expanded tasks show at most three meaningful events;
- current tool names collapse into semantic states such as reading, editing,
  building or testing;
- History no longer fetches Review/Checkpoint while those capabilities are
  parked and exposes a smaller default filter surface.

### Token truth

- non-verified Token totals are absent from the workspace;
- the dashboard shows only the quality explanation until verified;
- the display settings do not offer analytics controls while unverified;
- scan checkpoints persist numeric parser state, inode/offset and a hashed
  source key in a private `0600` cache without paths, prompts or commands;
- incomplete backfill progress is written at most every 15 seconds, so a
  Runtime restart no longer replays a multi-gigabyte first scan from zero;
- Runtime restart resumes from matching source checkpoints;
- Codex credential refresh cannot interrupt the first canonical usage
  generation during a bounded 30-minute startup grace;
- shared refresh-budget exhaustion now stops at the previous JSONL newline
  instead of misclassifying an ordinary line as oversized and losing it;
- oversized Codex `response_item`, `compacted`, `world_state` and the observed
  image-generation completion shape are skipped only after their bounded prefix
  proves they cannot carry usage; unknown or usage-bearing shapes remain
  partial;
- checkpoint schema 3 rejects earlier parser state that could contain the old
  false-partial boundary, interleaved-counter inflation or replayed parent ID;
- interleaved cumulative Codex samples use a monotonic envelope: lower stale
  snapshots never become a new full reset and cannot multiply a long session;
- the first valid Session ID in a rollout file is immutable, so a replayed
  parent `session_meta` cannot merge independent child ledgers into its parent;
- files larger than 1 GiB are parsed incrementally instead of skipped;
- readiness is sticky after the first atomic generation so a growing hot file
  cannot restart the first-scan UI;
- each caught-up canonical generation replaces old session-day/cursor
  projections transactionally, including old inflated partial rows.

The final numeric-only isolated replay and installed schema-3 publication of
the real 5.7 GiB ledger produced `historyComplete=true`, zero partial records
and matching canonical day projections. The installed acceptance sample was
`12,012,670,827` Token across 35 active days; live totals continue increasing
while Agents run. This total includes Provider-reported cache input;
input/output/cache/reasoning stay separately projected.

### Parked worker lifecycle

- Firebase bootstrap and CloudShareModel polling do not start;
- the Review Baseline thread does not start;
- animated background players do not start;
- Companion and Team management are absent from normal settings;
- Team errors cannot pollute General/Data/diagnostics.

## Data safety

Before source changes, the live database was backed up to:

`~/.actrealm/token-backups/convergence-r0-2026-08-26/data-before-convergence.sqlite`

SHA-256:

`e130ab0d14406e62e12e8a8e8215ebf2e9a4efc1da880fc4a878a6e4898bf01f`

The candidate does not silently revoke or delete existing Cloud shares,
Companion registrations, Team data, task history or Provider configuration.

## Acceptance boundary

No replacement app is installed until:

- all workspace Rust tests and the macOS suite pass;
- Cloud contracts and language checks pass;
- release compilation and signature packaging pass;
- compiled snapshots show the converged navigation and settings;
- the real database completes/restarts its usage index without returning to a
  first-scan state;
- Computer Use confirms task filtering, immediate archive, History, official
  quotas, diagnostics and unchanged Agent Focus;
- idle resource sampling is recorded.

Build 96 is the accepted local Apple Development signed candidate. Display,
Team, Review v2, mobile, Watch, Cowork, notarization, tagging, public release and
the seven-day soak remain separate decisions.

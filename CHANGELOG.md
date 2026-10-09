# Changelog

All notable ActRealm changes are recorded here. The project has not yet
published a final v1 release; entries below describe development milestones on
`agent/v1-full`, not released packages.

## Unreleased - M14 accepted candidate

### Post-P0 - Companion task roles, history and turn context

- Companion result excerpts keep up to the first 60 non-empty lines and 4,000
  characters (was 5 lines and 600 characters), so a short answer can be read
  in full on the Display desk screen. Code blocks are still skipped and every
  line still passes the credential filter.
- Adds additive Companion fields for the Display Agent page without changing
  `protocolVersion` 7: snapshot sessions report `userTurnCount` and a
  `main`/`side` `taskRole`; `GET /api/v1/companion/history` lists recently
  active tasks with status and review state; the result envelope carries the
  current turn's sanitized, memory-only `prompt`; the Companion review lists up
  to 20 changed files with line counts and no content.
- A user interruption no longer counts as a failure: the session idles with
  the existing `session.activity.interrupted` message and no `error`
  Attention. `StopFailure` is unchanged. The local History center keeps
  reporting such a task as `failed`.
- Review fixes: the turn prompt also drops lines that name a credential in
  English or Chinese (full-width colons included) and the value line after a
  bare label; Companion history reports `seen` only after a user action or a
  new instruction, keeps `failed` after the session ends, and never falls back
  to a prompt-derived title; the Companion review runs git off the API thread,
  reuses the resolved repository, and briefly caches unchanged repositories.
- A new prompt now closes earlier completion/error reminders with resolution
  `superseded_by_prompt`; other activity keeps `superseded_by_activity`, which
  Companion history counts as unseen. Result excerpts use the same credential
  line filter as the turn prompt.
- Second review round, prompt and result excerpt filter: keys written
  straight against Chinese text or full-width punctuation are found (tokens
  are split at every non-ASCII character); `AIza`, `glpat-`, `sk_live_`,
  `rk_live_`, `sk_test_`, `hf_`, `npm_`, `xapp-`, `xoxa-`, `gho_`, `ghs_`,
  `ghu_`, `ghr_` and `LTAI` keys are recognized; Chinese labels followed by
  a predicate such as `改成`/`设置为`/`就是`/`如下` or by quotes, labels
  in Markdown emphasis, multi-line values (`password: |`, `"passwords": [`,
  deeper-indented continuation lines), `passphrase`, short `pass`/`pwd`/`pin`
  assignments, `mysql -p<password>` and `curl -u user:password` are removed.
- Second review round, history and review state: a delayed or manual
  completion the user acknowledged stays `seen` when later compaction,
  resume or subagent activity closes it (resolution `ack_hidden`, the
  acknowledgement time is kept); a session that ends while its turn is still
  working ends that turn as `interrupted` instead of reporting `completed`
  without an end time; the History center reads the latest turn's outcome
  too, so an interruption followed by `SessionEnd` or `SessionStart` stays
  `failed` there (its `completed`/`failed` vocabulary is unchanged).
- Third review round, history: a task's finished status and `completedAt`
  come from its outcome turn, the latest turn the user started with a prompt.
  Implicit turns that a non-prompt event opens later (an idle notification
  after `claude --continue`, a background subagent finishing, a compaction)
  no longer turn an interrupted task `completed` with no end time, and the
  exit time no longer replaces a completed turn's `Stop` time. On
  `SessionEnd` only a working turn that holds the user's prompt or a started
  tool ends as `interrupted`; an implicit turn opened while background work
  was running ends as `idle`, so a completed task is no longer reported
  `interrupted` (History center `failed`) after the user exits.
- Third review round, prompt and result excerpt filter: after a Chinese
  credential label the rest of the sentence (up to twelve Chinese characters
  or words) is searched for a password-like ASCII word, so any wording
  (`初始密码默认是 Admin@123`, `密码我改成了 …`, `用户名和密码分别是 admin 和 …`,
  arrows and emoji) is removed while paths, versions, file names, dimensions,
  colors, algorithm names and references after `commit`/`id`/`port` are
  kept; a bracketed note before the colon (`数据库密码（测试环境）：…`) counts
  as part of the label, and `私钥` is a label. Markdown tables whose header
  names a credential column lose their separator and data rows (a `Key`/`SK`
  column only rows with a key-like value; count columns such as `Token 数`
  are not credential columns). Keys glued to ASCII punctuation in compact
  JSON or code, Telegram and Discord bot tokens, and URLs with a password are
  found. More command-line password options are recognized (`sshpass -p`,
  `redis-cli -a`, `docker login -p`, `mongo -p`, `sqlcmd -P`, `ldapsearch -w`,
  `zip -P`, `7z -p`, `keytool -storepass`, `smbclient -U user%password`,
  `lftp -u user,password`); a tool's options end at a shell separator or the
  next command, so `curl … && docker run -u 1000:1000` is kept. A bare
  `key`/`SK`/`AK`/`appKey` label with a key-like value is removed. Long lines
  of repeated labels no longer take quadratic time.
- Fourth review round, history: the outcome turn is the latest turn that
  the user started with a prompt or that the Provider ended itself (`Stop`,
  `StopFailure`, `TurnInterrupted`). A turn a background task woke up that
  then fails (or a Codex turn without a prompt that the user interrupts) is
  no longer replaced by the earlier prompted turn after `SessionEnd` or
  `SessionStart(resume)`: the task stays `failed`/`interrupted` with that
  turn's end time, and a woken turn that completes later reports its own
  `Stop` time. A session that never had a prompt keeps a failure or an
  interruption at exit after a resume and an idle notification.
- Fourth review round, prompt and result excerpt filter: the sentence
  search after a Chinese label no longer removes ordinary questions and
  reports. Punctuation that ends a word (`MySQL？`, `Codex！`) is not a
  password symbol; words after `提交`, `错误码`, `上限`, `耗时`, `模板`,
  `参考` and similar Chinese references, or after `traceId`/`trace_id`, are
  kept; numbers, commit-like hex runs, identifiers with `_`/`-` and products
  with a version number (`iPhone15`, `Node20`, `RTX4090`) count only when
  the wording hands them over as a value (`是`, `改成`, `了`, a colon), and
  a number followed by a unit (`毫秒`, `次`, `QPS`) never does; an algorithm
  name right after a predicate is not a value (`密码改成 AES-256 加密`).
- Fourth review round, values after a label line: a line that ends with a
  label after other words (`数据库密码`, `MySQL root password`,
  `username,password`) also removes the next line when it looks like a
  value; a list item that only names a label (`2. 密码`) stays, so lists of
  form fields are kept; comma- and tab-separated rows under a header naming
  a password column are removed. A code block (backticks, tildes or
  `<pre>`) after a label is removed as its value instead of its fence line,
  and a lone quote or `---` line is skipped; result excerpts also skip `~~~`
  blocks. Chinese labels may contain spaces (`密　码：`), `账密` is a label,
  `pw`/`psw`/`pswd` are short keys, a key name before a Chinese predicate
  (`高德的 key 是 …`) counts, and `sshpass -p<password>`, `mysql -p
  <password>`, attached `sqlcmd -P` / `ldapsearch -w`, `unrar -p`,
  `jarsigner`, `mosquitto_pub -P` and `openssl -k`/`-pass` are recognized.
- The short key prefixes `sk-`, `ghp_` and `akia` now count only at the
  start of a run of key characters followed by at least 16 more: words such
  as `task-runner`, `flask-login`, `disk-usage`, `agent-desk-redesign` or
  `Slovakia` no longer drop the whole prompt line.
- Companion snapshot Attention items add `handBackAvailable`: true only when
  `pass_through` returns a live approval to the original Agent UI (Hook
  waiters, Kimi/Grok Connector requests), false for the managed Codex
  app-server channel, where it would answer Codex with an error.
  `POST /api/v1/companion/sessions/{id}/seen` (scope `attention.respond`)
  marks a task as seen from a history row without an Attention ID: it
  acknowledges the latest completion/error reminder like `ack`, also when it
  was already auto-hidden, and returns `reviewState` `seen` or `none`.

### Post-P0 - OUTBOX lifecycle and quota recovery

- Keeps Codex Desktop native permission requests in OUTBOX across the
  synthetic `request_permissions` tool end and `Stop`; completion is deferred
  until an authoritative Provider transition clears the wait.
- Moves a newly arrived higher-priority approval into the visible primary card
  without losing stable Attention-ID selection, and scrolls that card back
  into view.
- Adds truthful native actions: open Provider, mark handled, and snooze.
  Direct allow/deny appears only for a live request delivered to an
  ActRealm-owned reply channel.
- Adds Settings → Provider Data → manual Claude quota refresh. It waits for
  the OAuth request and reports missing/rejected credentials, rate limiting,
  or Provider failure without exposing secrets.
- Aligns local and GitHub CI on Rust 1.97 for the locked SQLite dependency.

Focused Runtime/Server tests, the 83-test Swift suite, workspace Clippy,
release build, language/plist checks, local RustSec audit, and arm64 package
codesign pass. The current managed sandbox blocks listener/resource tests at
Socket `bind`; packaged user acceptance and remote CI remain open.

### Post-M14 - First-run workspace and Agent setup center candidate

- Adds an honest first-run state to the main three-column workspace when no
  Claude or Codex integration is installed: OUTBOX, Agent Tasks, and Quota all
  explain the same disconnected state instead of showing generic empty data.
- Adds one visible Agent-status/setup entry in the ActRealm toolbar and a
  unified, on-theme setup center based on the approved board 6/7 direction.
- Connect, repair, uninstall, refresh, and Codex trust guidance call the
  existing authenticated `/api/v1/setup` API; no setup action is a visual
  placeholder and no unsupported Provider is advertised.
- Keeps Provider discovery truthful for CLI-only, desktop-only, and mixed
  installations. Codex trust remains an explicit user action in the official
  interface, with a copyable command supplied by the Runtime.
- Links “查看接入指南” directly to the maintained Chinese GitHub guide and
  refreshes setup state after Runtime events or returning to the page.
- Adds an isolated ignored preview harness for visual QA without changing the
  user's real Hook configuration, plus embedded-UI regression coverage for the
  first-run contract.

The JavaScript syntax, static UI contract, format/diff checks, Rust workspace
suite, release build, and board 6/7 visual acceptance pass. Exact real-Provider
events and remaining local gates stay open. The user authorized the local
candidate commit on 2026-07-20; push remains separately gated.

### Post-M14 - Codex internal-session filtering

- Discards Codex App overview-suggestion and safety-review background sessions
  at Runtime ingest instead of storing or displaying them as Agent tasks.
- Removes the provisional `SessionStart` row and metric when a later prompt
  identifies an internal session, then suppresses its remaining lifecycle
  across Runtime restarts without adding a session visibility state.

The synchronized tree passes JavaScript syntax, format, zero-warning Clippy,
177 Rust tests, release build, the ActRealm language contract, the focused
schema-9 regression, and the two-minute resource gate. Three explicitly manual
or resource tests remain ignored in the ordinary workspace run. The macOS
Swift suite could not start in the managed environment because SwiftPM's own
`sandbox-exec` was rejected; the latest release binary is not yet installed.

### Post-M14 - Usage, pricing, and OAuth hardening candidate

- Adds a versioned per-model price registry with distinct `provider_estimate`
  and `computed` kinds plus dated `models.dev`, OpenAI standard-price, and
  compatibility-fallback source labels.
- Expands computed price coverage to validated Claude and Codex models while
  preserving exact cached-input/cache-write semantics and unknown-model
  omission. Codex model changes price each cumulative delta at the model active
  for that event instead of repricing earlier usage.
- Separates current desktop-picker coverage from historical rollout support:
  Claude's visible Fable/Opus/Sonnet/Haiku choices and Codex GPT-5.6 plus
  GPT-5.5 use dated first-party rates; GPT-5.4 and older remain history-only.
- Carries the structured transcript/rollout model into SQLite schema 8 and
  uses it only when the Hook session model is absent, eliminating cards that
  showed an unknown model beside a model-derived price.
- Treats zero-token Claude `<synthetic>` rows as zero-cost metadata, so they
  neither replace the real session model nor suppress a complete estimate.
- Covers the locally observed `claude-sonnet-5` with Anthropic's dated
  introductory API rate and cache multipliers; the source label makes the
  promotion's August 31, 2026 boundary explicit rather than treating it as a
  timeless price.
- Parses Claude OAuth expiry metadata and delegates near-expiry/401 recovery to
  one bounded official `claude auth status --json` invocation; ActRealm never
  owns or persists refresh tokens.
- Tries the fixed Claude Keychain service and a cached successful locator before
  a size-bounded, five-minute-cached service enumeration fallback.
- Replaces full-history Claude re-aggregation with O(1) accumulators and a
  bounded 256-entry correction window. A 10,000-entry regression preserves
  exact Token totals and computed price.

Focused quota/usage tests, the 172-test workspace suite, zero-warning Clippy,
release build, language contract, and the two-minute resource gate pass. Exact
release installation, local user acceptance, commit, and push remain
separately gated.

### Post-M14 - Live state and controlled Runtime recovery

- Adds WebSocket heartbeats, stale-connection detection, bounded snapshot
  fallback, and visibility-aware reconnect behavior so a visually open panel
  does not silently stop receiving state.
- Keeps task and Attention cards structurally stable: elapsed time and activity
  text update in place, while unchanged rows are not recreated when a new
  event arrives. This removes visible card jitter during active work.
- Adds an authenticated local health monitor for the Runtime, API, WebSocket,
  Hook socket, latest Hook event, active tasks, pending Attention, SQLite event
  count, and restart history.
- Adds one controlled `重启 Runtime` action. It safely releases active waiters,
  re-execs the same binary on the same loopback port, recreates the Hook socket,
  restores durable session state, rotates browser authentication, and reconnects
  the page automatically.
- Keeps crash/offline behavior truthful: the browser cannot relaunch a process
  that is no longer running and falls back to the documented terminal command.

This work is separate from M15, which remains reserved for managed Codex
app-server approval methods.

### Post-M14 - Single-toolbar visual refinement

- Removes the decorative macOS menu bar and red/yellow/green traffic lights.
- Keeps one in-page ActRealm toolbar with the brand on the left and
  Notification & Data, local time, and truthful Runtime state on the right.
- Adds an embedded-UI regression contract so the removed chrome cannot return
  accidentally. The focused UI test, JavaScript syntax check, format/diff
  checks, and workspace release build pass; the user accepted the visual result
  and authorized the local commit.

### M14 - Live usage, context, price, and OAuth quota

- Incrementally tails Claude transcript and Codex rollout usage with bounded
  parsing and cross-file/stream de-duplication.
- Separates session cumulative Token, latest-turn Token, cached/reasoning
  breakdowns, and current-context occupancy.
- Adds explicitly labelled estimated API price with unknown-model omission.
- Adds background Anthropic OAuth usage refresh with dynamic scoped limits,
  Fable/extra-usage support, one-minute cadence, and StatusLine fallback.
- Keeps OAuth credentials memory-only and keeps the last validated quota value
  visible with its factual capture age.

The 161-test workspace suite, zero-warning Clippy, release build, and explicit
two-minute resource gate pass. The exact release was installed locally with a
matching SHA-256, schema 7 and live OAuth/session records were verified, and
the user accepted the candidate and authorized this commit. A branch push still
requires separate authorization.

### M13 - Provider-owned approval-state coordination

- Detects Provider-owned Codex review and avoids creating a competing ActRealm
  waiter.
- Tracks native `request_permissions` and managed `waitingOnApproval` states.
- Clears native waiting only on an explicit Provider resolution signal.
- Distinguishes observation-only native approval from ActRealm-controlled approval
  in Attention, task state, notifications, and available actions.
- Keeps approval outcome neutral when the Provider does not expose it.

Automated and local gates passed. Real-Provider manual acceptance remains
pending. Commit: `311306d`.

### M10-M12 - Safe display, questions, Connector, and recovery

- Adds concise, detailed, and developer task-card profiles using a server-owned
  safe field allowlist.
- Adds Claude AskUserQuestion and Elicitation forms with memory-only secret
  handling.
- Adds the explicit Codex app-server Connector for `requestUserInput`, managed
  Thread attach/resume, and truthful restart recovery states.
- Never restores an old approval/question waiter across Runtime restart.

Commit: `ba2f328`.

### M6-M9 - v1.1 functional corrections

- Keeps the live task list to active, attention-bearing, or recently active
  sessions and links Attention to its task card.
- Renders all valid quota windows, preserves the last valid sample, and shows
  factual total-turn/current-phase timing.
- Supports desktop-only Claude/Codex installations without requiring a global
  CLI, while retaining Codex's user-controlled trust step.
- Reconciles Provider-handled attention, adds safe ignore, and exposes honest
  jump/recovery capabilities.
- Uses Provider conversation titles, bounded current-question summaries,
  model-only third lines, and recognizable Provider icons.

Primary commits: `6b7c465`, `63c6fce`, and `120e89d`.

### M5 - Release hardening candidate

- Adds privacy-bounded diagnostics, aggregate metrics, export, security tests,
  performance checks, and pass-through coverage.
- Keeps raw Hook bodies, prompts, commands, paths, and tokens out of default
  logs and aggregate exports.

The two-minute resource gates pass; the continuous 48-hour Runtime RSS gate is
still pending, so this is not a final v1 release.

### M4 - Honest quota and local controls

- Adds bounded Claude/Codex quota adapters, unavailable/stale states,
  notification and retention settings, local export, and destructive clear.

Commit: `c739355`.

### M3 - Safe Provider onboarding

- Adds backup-preserving Hook installation/uninstallation, onboarding, Codex
  trust guidance, repair state, and Doctor diagnostics.

Commit: `bd15994`.

### M2 - Authenticated local control panel

- Adds authenticated localhost API/WebSocket transport and the fixed
  three-module Attention, Agent task, and Quota interface.

Commit: `bb68922`.

### M1 - Persistent Runtime core

- Adds SQLite/WAL persistence, session state, request-keyed waiters, bounded
  event spool, single-instance coordination, and restart-safe expiration.

Commit: `87868fc`.

### M0 - Provider control-path proof

- Verifies Claude and Codex Hook ingestion, socket wait/reply, allow, deny,
  pass-through, and fail-open behavior with versioned fixtures.

Commit: `d23c27b`.

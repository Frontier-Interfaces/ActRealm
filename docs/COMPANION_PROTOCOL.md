# ActRealm local Companion protocol

Status: local v1, schema version 1. The first consumer is the display Companion.

Runtime health reports `protocolVersion: 7`. Native applications use the
[verified local enrollment and pinned TLS service](NATIVE_SERVICE.md); they do
not require AR1 pairing or an ActRealm-owned bootstrap session. The AR1 sections
below describe legacy compatibility routes. Snapshot schema version remains 1.

The optional `pricing` summary contains `source`, `updatedAt`, `modelCount`,
`updating`, `refreshFailed` and `automaticIntervalMinutes`. Catalog reads run in
a separate bounded background worker, hourly or early after an unpriced model
is observed (five-minute retry limit), and do not wait for historical indexing.
`POST /api/v1/companion/pricing/refresh` requires `attention.respond` and schedules
a refresh; HTTP 202 is not a claim that a new catalog was already downloaded.
Failures keep the last validated prices. Requests use only the fixed public
models.dev URL and send no session, Prompt, Token-use or account data.

Model names in tasks are Provider facts, not a client-maintained whitelist.
Pricing accepts newly published OpenAI/Anthropic model IDs; exact IDs take
precedence over aliases. Unknown model prices remain absent, never substituted
with a different model. The reference estimate uses standard base Token rates;
long-context tiers, Fast mode, regional and tool charges are not included.

Protocol v4 adds the optional `asyncQuestions` snapshot array. Each entry contains
`id`, `sessionId`, `createdAt`, `canAnswer`, and `questions` (title and string
options). These transient local fields never enter SQLite, checkpoints or Cloud
exports. They do not change a session's execution state or imply a permission
request. Observed Codex Desktop JSONL questions have `canAnswer: false`; a
request-owned Connector or active, explicitly attached turn may expose direct
answers. Read-only companions always receive `canAnswer: false`.

`POST /api/v1/companion/async-questions/{id}/answer` accepts
`{"answers":["answer for question one"]}` and requires `attention.respond`.
The Runtime validates the live question, answer count, attached thread and reply
route. RPC responses retain their original request ID; asynchronous message
answers use `turn/steer` with `expectedTurnId`. Duplicate/in-flight submissions,
expired questions and uncertain transmissions cannot be replayed. Native-only
observation offers a session jump, never a manufactured reply channel.

The initial listing from an independent Codex app-server is not authoritative
for an external Desktop task's completion. `notLoaded`/`idle` must not overwrite
Hook-observed execution; only attached-thread status or explicit lifecycle events
can establish that terminal transition.

## Security model

The Rust Runtime remains authoritative for Hook and app-server connections,
SQLite, sanitization, session recovery, approval state, question waiters, and
Provider replies. A Companion is an untrusted local presentation client with
explicitly bounded capabilities. It never receives the official Web Cookie or
CSRF secret and never opens SQLite.

Threats addressed:

- pairing from another machine: every Companion endpoint is loopback-only;
- leaked reusable code: enrollment codes expire after five minutes and are
  removed on first use;
- token disclosure at rest: the server stores only SHA-256(token) in a `0600`
  file; the client owns its own OS credential storage;
- privilege expansion: scopes are fixed at enrollment and every action checks
  the corresponding scope again;
- replay/stale decisions: action paths revalidate the current request ID,
  declared capability, expiry, and live Runtime waiter;
- restart confusion: old waiters are never restored; a private discovery file
  publishes only the new loopback endpoint and instance ID;
- data overexposure: response models are explicit allowlists rather than a
  serialization of internal session or database records.

## Pairing and registration

Official authenticated clients create an enrollment code with:

`POST /api/v1/companions/pairing`

```json
{ "clientName": "Display Companion", "allowControl": true }
```

The response contains `AR1:<port>:<64-hex-secret>` and an expiry time. The
Companion submits it once to:

`POST /api/v1/companion/enroll`

The enrollment response contains a random bearer token, Companion ID, endpoint,
scopes, and discovery path. ActRealm keeps at most 16 registrations and replaces
an older registration with the same client name.

Official clients list or revoke registrations with:

- `GET /api/v1/companions`
- `DELETE /api/v1/companions/{id}`

Revocation takes effect on the next request. Tokens are not displayed again.

## Companion endpoints

All requests except enrollment use `Authorization: Bearer <token>`.

| Endpoint | Required scope | Meaning |
| --- | --- | --- |
| `GET /api/v1/companion/snapshot` | `snapshot.read` | Sanitized sessions, attention, quota, stats and effective capabilities |
| `GET /api/v1/companion/history` | `snapshot.read` | Recently active tasks (bounded metadata only); see "Task roles, history and turn context" |
| `GET /api/v1/companion/settings/completion` | `snapshot.read` | Read the completion-task retention mode and configured delay |
| `PUT /api/v1/companion/settings/completion` | `attention.respond` | Update only the completion-task retention mode and delay; all other settings remain unchanged |
| `GET /api/v1/companion/sessions/{id}/activity` | `snapshot.read` | Bounded, sanitized activity for the current turn; accepts `limit=1...100` and an optional `afterIngestSequence` cursor |
| `GET /api/v1/companion/sessions/{id}/review` | `snapshot.read` | On-demand branch/worktree, bounded Diff counts, validation states and outcome evidence; never returns patches, commands or file contents |
| `POST /api/v1/companion/sessions/{id}/jump` | `session.jump` | Ask ActRealm to perform its current safe jump behavior |
| `POST /api/v1/companion/sessions/{id}/seen` | `attention.respond` | Mark a task as seen: acknowledge the session's latest completion/error reminder, even when it is already hidden; see "Task roles, history and turn context" |
| `POST /api/v1/companion/commands` | `attention.respond` | Submit an action for a current Attention item; allow/deny may opt into the fixed three-second undo window or submit immediately |
| `POST /api/v1/companion/commands/{id}/undo` | `attention.respond` | Undo an allow/deny command that opted into and remains inside the three-second pending window |
| `POST /api/v1/companion/questions/{id}/answer` | `attention.respond` | Answer or hand a live question back to the Provider UI |

The command endpoint does not create new semantics. It accepts only actions
already supported by the existing Runtime command path, including approve,
deny, pass-through, acknowledge, ignore and snooze where applicable. For an
allow/deny action, a Companion may send `undoDelayMs: 3000` to retain the fixed
three-second undo window or `undoDelayMs: 0` to commit immediately. The field is
optional and defaults to 3000; any other value is rejected. An immediate
decision cannot be undone. Question answers use the existing typed answer
contract and remain memory-only.

The snapshot publishes `allowedActions` for each Attention item. A verified,
live Provider reply channel may expose both `approve` and `deny` at every risk
level; risk classification is an informational warning, not an extra permission
gate. An unreviewed future tool may expose only `deny`. Observation-only native
Provider waiting state, a missing request ID, a stale waiter, or a Companion
without `attention.respond` exposes no approval actions. Consumers must render
exactly the declared actions and must not infer control from Provider name or
risk level.

Each Attention item also carries `handBackAvailable`. It is true only for an
open approval with a live reply channel whose `pass_through` really returns the
request to the original Agent UI: a Hook waiter (Claude Code, Codex or Gemini
Hooks), or a Kimi/Grok Connector request, which stays pending in the Provider
until its own UI answers it. It is false for an approval held by the managed
Codex app-server channel (session `controlCapability` `managed`): there
`pass_through` answers Codex with an RPC error instead of showing the request
in a Codex UI, so a consumer should offer only the declared allow/deny actions.
It is also false without a live reply channel, for every non-approval item,
and for a Companion without `attention.respond`. Question hand-back is
described by `interaction.supportsNative`.

Jump selection is source-first. Codex App sessions use the
`codex://threads/{id}` deep link; iTerm and Terminal sessions return to their
safe session/TTY locator; VS Code and other known sources open the recorded
application. A UUID-backed Codex conversation remains a fallback when the
original source cannot be restored. The Runtime tries only allowlisted safe
targets and reports the target that actually opened.

Completion retention supports `afterConfirmation`, `afterDelay` (5, 15, 30 or
60 minutes), and `manual`. A delayed or manual completion acknowledgement only
closes the reminder. It does not change the immutable deadline or hide the task;
manual mode keeps the task until an explicit hide/archive action. The snapshot
exposes `retainAfterAck` and the optional `autoHideAt` so clients can describe
the policy without inventing a countdown.

The activity endpoint returns the current-turn page when no cursor is supplied,
so a display does not revive a completed earlier turn as live work. With an
`afterIngestSequence` cursor it returns later sanitized events for stable
incremental refresh. Long histories remain bounded and scrollable in the
consumer; the Runtime never sends raw tool input/output, full commands, file
contents, or transcripts through this route.

## Snapshot privacy contract

Schema v1 may include sanitized session identity, Provider, bounded title and
project labels, execution/activity state, plan progress, sanitized current tool,
session/current-turn Token counters, input/output/cache/reasoning counters,
context counters, estimated API-equivalent cost, child-Agent count, jump/control
capability, Attention metadata, typed interactive question schema, and Provider
quota summaries. Optional counters remain absent when the Provider does not
report them; Companion clients must not infer missing values.

Codex account quota prefers the desktop-bundled app-server over an older global
CLI. A transient Connector failure may return the last official, unexpired
snapshot with `status: stale`; consumers may keep the percentage visible but
must preserve its original `capturedAt`. A Spark-only rollout without an
official standard-plan entry never proves that the account is Pro. See
"Quota entries" for sources, freshness and Extra Usage amounts.

It excludes raw prompts, complete commands, tool input/output, file contents,
transcripts (beyond the sanitized excerpt of the latest Agent message, see
"Latest Agent message"), answers, secrets, Hook payloads, Provider reply
channels, private jump locators, Web credentials, Cloud credentials, and
Provider cookies.

## Project labels

The `project` of a session (and of its Attention items and history task) is a
label derived from the session's working directory so that the user can
recognise the repository; it is a label, never a path. A directory inside a
git worktree folder `<Repo>.worktrees/<name>`, or inside
`<Repo>/.worktrees/<name>`, at any depth, is labelled `<Repo>`
(`~/ActRealm/Display.worktrees/main` is `Display`). Otherwise the label is the
last folder name, except that a generic folder or branch name (`main`,
`master`, `trunk`, `develop`, `dev`, `src`, `app`, `repo`, `workspace`,
`worktree`, `code`, any case) is replaced by its parent folder's name, again
while that name is generic: `~/code/foo/main` is `foo`, `~/proj/app/src` is
`proj`, `~/work/ActRealm` stays `ActRealm`. The label never climbs to the home
directory or above it (`~/code` and `~/main` keep their names, and the home
directory keeps its own), and outside the home directory never to a top-level
folder (`/tmp/main` stays `main`). Runtime re-derives the label of every
recorded session when it opens its database, so sessions recorded under the
earlier last-folder rule, including a live one, show the new label after the
Runtime restarts.

## Runtime restart discovery

At startup the Runtime atomically writes:

`~/.actrealm/run/companion-endpoint.json`

```json
{ "schemaVersion": 1, "endpoint": "http://127.0.0.1:PORT", "instanceId": "..." }
```

The parent directory is `0700` and the file is `0600`. Consumers must also
verify that it is a small regular file owned by the current user and that the
endpoint is exactly loopback HTTP. Tokens are never written to discovery.

## Failure semantics

- `401`: registration missing, revoked, or token invalid; remove the client
  credential and require a new explicit pairing.
- `403`: required scope is absent; do not show the operation as successful.
- stale/expired request: refresh and display the current Runtime truth.
- Runtime unavailable: keep the last truthful snapshot visibly stale, disable
  mutations, and retry using the private discovery descriptor.
- incompatible schema: stop consuming the snapshot and require a compatible
  version; do not guess field meaning.

### Codex connection that needs a Runtime restart

These are additive fields in protocol 7. `capabilities.codexConnector`
reports `restartRequired: true` when only a new Runtime process can bring the
Codex app-server connection back; Runtime never reconnects it on its own.
`restartReason` says why: `connection_lost` (the app-server exited, or was
shut down after it stopped reading its input), `credentials_changed` (the
automatic restart for changed Codex credentials failed), or
`state_unavailable` (the connector state can no longer be read). It is `null`
while no restart is needed, including while Runtime restarts itself for
changed credentials. Clients treat an unknown reason as a plain restart, word
the message themselves, and use the English `error` for diagnostics only.
Older Runtime versions omit both fields, which means no restart is requested.

## Protocol v5: local result excerpts and referenced files

`GET /api/v1/companion/sessions/{id}/result` requires `snapshot.read` and returns
`{schemaVersion: 1, sessionId, result: null | {sessionId, observedAt, source,
summary, truncated, artifacts}}`. It is available only for the latest ended or
failed turn. A new turn invalidates old results. `source` identifies Stop Hook
text or a final Codex response; the text is Provider-reported, not independent
verification of the claims inside it. Each excerpt keeps at most the first 60
non-empty lines and 4,000 characters (code blocks are skipped), so a short
answer can be read in full on a scrolling desk screen. `truncated` is also true
when a code block was skipped or lines past the limit were left out.
The excerpt goes through the same credential line filter as the turn prompt
(see the `prompt` field below): credential lines and the value line after a
bare label are removed, the known secret formats are checked per line and over
the whole excerpt, and `truncated` is true whenever a line was removed.

Results are held only in bounded Runtime memory (up to 128 sessions, 24-hour
retention). They are excluded from generic snapshots, SQLite, exports, Cloud,
diagnostics and offline spool records. Codex can recover recent final responses
from the same already-discovered, identity-checked JSONL inventory used by its
question observer (24-hour result lookback, one-hour question lookback, bounded tails and read budgets). Claude
uses `Stop.last_assistant_message`; no reply-history backfill is claimed for it.

At most five existing local files referenced by Provider Markdown links are
projected as `{id, name, kind, canReveal}`. A referenced file is not proof of a
newly-created artifact. Absolute paths are omitted from result and activity projections. Missing, non-regular,
sensitive-directory or unsupported file references are omitted. File identity,
owner and path boundaries are revalidated before use.

`POST /api/v1/companion/sessions/{id}/artifacts/{artifact}/reveal` requires
`session.jump`, a current result and the matching still-existing file. On macOS
it requests Finder reveal; it never runs or opens Provider-linked files.
`ARTIFACT_UNAVAILABLE` reports a stale or unavailable reference.
`ARTIFACT_REVEAL_FAILED` reports launch failure, nonzero exit or a three-second
command timeout. `{ok: true}` is returned only after the system reveal command
exits successfully; it is not a claim that the user has seen the Finder window.
Native clients may explicitly request `?native=true` on this same authenticated
POST. After the same scope, turn and file-identity checks, the response is
`{ok: true, localPath}` and Runtime does not launch Finder. This transient target
is delivered only in the user-initiated response; it must not be persisted or
added to snapshots, exports or diagnostics. Display passes it directly to
`NSWorkspace.activateFileViewerSelecting`, then uses existing Accessibility
permission to place only the matching Finder directory on the primary control
screen and checks foreground activation and window geometry. Missing permission
or failed placement produces an explicit fallback message without prompting or
changing macOS permissions.
Clients must distinguish a prepared target from a displayed window, and keep
per-file feedback visible after the transient toast ends. An older Runtime
without `localPath` retains the legacy dispatch behavior. Read-only companions receive
`canReveal: false`.

Codex enhanced activity remains an explicit installer option. The current local
candidate enables PreToolUse/PostToolUse through the existing installer, while
Claude already has tool lifecycle hooks. These events remain descriptive;
no retry/interrupt/steer capability is inferred for Hook-only sessions.
Offline spool records retain bounded lifecycle metadata and optional numeric
exit status, not prompts, command text, tool input/output or final reply text.

References: https://learn.chatgpt.com/docs/hooks and
https://code.claude.com/docs/en/hooks#stop.

The v5 result envelope may also include `activities`: bounded, current-turn
Codex tool lifecycle metadata observed from the same verified local inventory.
These fallback events never change Provider execution/approval state, enter
SQLite/Cloud, or claim a test passed from unstructured tool output. Companion
`lastEventAt` may advance on a genuinely newer observed tool event, so task
removal/reappearance is consistent with real activity. Display deduplicates
Hook and observer events by tool-call ID and kind.

## Protocol v6: first-run Codex metrics

Display requires v6 to distinguish the cold-start fix from older compatible
v5 binaries. A newly installed client must not silently keep an older collector.
While the first atomic historical ledger generation is pending, a Codex session
whose known sources have been read completely can supply numeric UI fields in
memory. This fills missing session metrics only, preserves committed fields,
and rejects a model mismatch. The fallback is marked `partial` with source
`codex_rollout_during_indexing`; historical aggregates and exports continue to
use the previous committed generation. The cache is cleared after publication.
No raw conversation content, path or daily ledger is added to this projection.

Token/context availability still depends on local Provider usage events and
readable local sources. A Hook or a healthy Runtime alone is not proof that
`token_count` and context-window data have been collected. See the current
source-build and first-run guide instead of the historical public-repo setup.

### Connection observation

Session `managedConnectionState` carries the Runtime automatic-connection status
(`pending`, `connecting`, `connected`, `owned_elsewhere`, `retrying`, or
`unavailable`) to authenticated local clients. It is optional for older Runtime
versions and non-Codex providers. This observation never grants response scope
or substitutes for a live request channel. `tokenUsage` remains the Runtime
aggregation; partial history must not be presented as complete account usage.

Runtime attaches Codex sessions only through Codex's official shared
app-server, reached through its control socket
(`capabilities.codexConnector.automaticConnection` is then `true`). Codex lets
one app-server process write a thread at a time; attaching in Runtime's own
app-server would hold that lock until Runtime exits, and the Codex app would
show the conversation as open in another app. So without the shared app-server
nothing is attached, on its own or through `POST /api/v1/sessions/{id}/manage`
(which answers `409 MANAGED_CONNECTOR_UNSUPPORTED`), and sessions report
`unavailable`. A thread held elsewhere (`owned_elsewhere`) is never taken over,
and one that failed to attach six times reports `unavailable` for the rest of
the Runtime run. An attached thread that is loaded reports `connected`.

## Task roles, history and turn context

These are additive v7 fields; `protocolVersion` and every schema version stay
unchanged, and existing fields keep their meaning.

Each snapshot session carries `userTurnCount` and `taskRole`. `userTurnCount`
counts turns opened by a user prompt submission (`UserPromptSubmit`,
`BeforeAgent` or a Connector `turn/started`, stored as `prompt.submitted`);
turns that Runtime opened implicitly for a late tool or lifecycle event are not
counted. `taskRole` is `main` when the session has a non-empty Provider title,
at least one user turn, and is not an ignored internal session; otherwise it is
`side` (script-launched runs, untitled sessions, forks without user turns).

A user interruption (`TurnInterrupted`) ends the turn without counting as a
failure: the session becomes `idle` with `activityMessage`
`session.activity.interrupted`, no `error` Attention is raised, and the turn is
recorded as `interrupted`. `StopFailure` still produces `failed` and an `error`
Attention. A new prompt in the same session (`UserPromptSubmit`,
`BeforeAgent` or a Connector `turn/started`) resolves that session's earlier
open or snoozed `completion` and `error` Attention with resolution
`superseded_by_prompt`; other new activity (a tool start, compaction, a
subagent or task, a Codex turn continuing on its own, a session start) resolves
them with `superseded_by_activity`. Both close the Attention as `resolved`, so
clients that only look at the state are unaffected. The local History center (`GET /api/v1/history`)
keeps its `completed` / `failed` vocabulary and still reports a
user-interrupted task as `failed`, as before. It follows the Companion history
status, including the outcome turn described below: a task Companion history
reports as `interrupted` (also after a later `SessionEnd` or `SessionStart`)
or `failed` is `failed` there, and every other finished task is `completed`.

`GET /api/v1/companion/history?since=<epoch ms>&limit=<1..200, default
100>&includeSide=<true|false, default false>` returns `{schemaVersion: 1,
generatedAt, tasks}` ordered by `lastEventAt` descending, limited to sessions
whose `lastEventAt >= since`, including active ones. A missing or malformed
`since`, `limit` or `includeSide` is rejected with HTTP 400
`INVALID_HISTORY_LIMIT`. Without `includeSide=true` only `main` tasks are
returned. Each task contains `id`, `provider`, `project`, `title` (the
Provider title, or null when the Provider gave none; unlike snapshot sessions it
never falls back to the Runtime title derived from the prompt), `taskRole`,
`userTurnCount`, `status` (`running`, `waiting`, `completed`, `interrupted` or
`failed`), `startedAt`, `lastEventAt`, `completedAt` (end of the outcome
turn), `branch`, `validationState`, `reviewState`, `latestAttentionKind`
(`completion`, `error` or null), `jumpCapability`, `jumpLabel`, and, as on
snapshot sessions, `termApp`, `termBundleId`, `termSurface` (see "Client
application") and `lastMessage` (see "Latest Agent message"; null past its
window or when none was found), so a finished task that already left the
snapshot can still be read without opening its conversation. Once the
session is idle, `status` and `completedAt` come from its outcome turn: the
latest turn that the user started with a prompt or that the Provider ended
itself (`Stop`, `StopFailure`, `TurnInterrupted`). A turn without a prompt
that a background task woke up and that then completed, failed or was
interrupted is therefore the outcome turn, with its own status and end time.
Turns that Runtime opened implicitly for a later non-prompt event and that
only the session's end closed (an idle notification after `claude
--continue`, a background subagent finishing, a compaction) never replace that
outcome. A session that never had such a turn (Hooks installed in the middle
of a turn) uses its latest turn that `SessionEnd` ended as `interrupted`, and
otherwise its latest turn of any kind. A
session that ended (`SessionEnd`) or resumed after a failed or interrupted
turn keeps that turn's `failed` or `interrupted` status and its `completedAt`,
also after such implicit turns and a second `SessionEnd`; a completed turn
stays `completed` with the time of its `Stop`, not the exit time. A session
that ends while it is still working on a turn that holds the user's prompt or
a tool the Agent started (Claude Code reports no Hook when the user presses
Esc and then exits) ends that turn as `interrupted` at the `SessionEnd` time,
so it is reported as `interrupted` with that `completedAt`, never as
`completed` without an end time. Any other open turn (one opened implicitly
while the session was idle, or only waiting for background work after its
`Stop`) simply ends and never makes a completed task `interrupted`. A
`SessionEnd` delivered after newer activity changes nothing.
`reviewState` becomes `seen` only through the user or the user's next
instruction: the latest completion/error Attention was
acknowledged (including a reminder-only acknowledgement), dismissed, archived
from the History center, or superseded by the user's next prompt (resolutions
`ack`, `ack_hidden`, `user_dismissed`, `history_archived`,
`superseded_by_prompt`), or the user submitted a prompt in the session after
it was raised. It stays `unseen` while that Attention is open or snoozed, and
when it was closed without the user: `superseded_by_activity`, `auto_hidden` by
the completion hide timer, or expired. `superseded_by_activity` rows written
before `superseded_by_prompt` existed cannot tell a prompt apart and count as
`unseen` unless a later prompt event exists. A delayed or manual completion
whose reminder the user already acknowledged stays `seen` when later
activity closes it: it is resolved as `ack_hidden` and keeps its
`reminderAcknowledgedAt`. It is `none` when the session raised no
completion/error Attention. No prompt, reply, command or file path is
included.

`POST /api/v1/companion/sessions/{id}/seen` (scope `attention.respond`, no
body) marks a task as seen after the user opened it, for example after a
successful jump from a history row, which carries no Attention ID. It acts on
the session's latest completion/error Attention, the one `reviewState` reads:
an open or snoozed one is closed exactly like an `ack` command (an error or an
after-confirmation completion is resolved; a delayed or manual completion only
records the reminder acknowledgement); one that is already closed or hidden
(`auto_hidden`, `superseded_by_activity`) stays closed and hidden. In every
case `reminderAcknowledgedAt` keeps the first acknowledgement time, so
`reviewState` becomes `seen`. The response is `200 {"reviewState": "seen"}`, or
`{"reviewState": "none"}` when the session never raised a completion/error
Attention. It is idempotent. An unknown session is `404 SESSION_NOT_FOUND`;
an empty or longer than 256-byte ID is `400 INVALID_SESSION_ID`.

The result envelope adds `prompt: null | {text, truncated, observedAt}`: the
user's own prompt for the current turn, taken from the prompt Hook (`prompt`
or `user_prompt`). It is sanitized with the same line-level result sanitizer
(credential lines removed, paths/URLs/hosts/emails replaced by placeholders),
bounded to 2,000 characters, and `truncated` is true when text was cut or a
line was removed. In addition, a line that names a credential is removed as a
whole: `password`, `passwd`, `passphrase`, `pwd`, `secret`, `token`, `api key`,
`apikey`, `access key`, `private key`, `cookie`, `authorization`, `bearer`,
`密码`, `口令`, `密钥`, `秘钥`, `私钥`, `令牌`, `凭证`, `凭据`, `授权码`,
`验证码` or `账密`, plus traditional forms such as `密碼` (case-insensitive,
`_`/`-` matching a space, full-width forms folded, up to two half- or
full-width spaces between the characters of a Chinese label as in `密　码：`,
Markdown `**`/backticks, quotes and list markers such as `1.`, `2)`, `3、`,
`•` or `·` around the label ignored), followed by a half- or full-width colon or equals
sign, whitespace, or the end of the line, or by a short bracketed note and then
a colon and an ASCII value (`数据库密码（测试环境）：Abc123!`,
`password(prod):…`; a Chinese value such as `密码（可选）：留空则不修改` is
kept). A Chinese label written straight against its value, or followed by a
predicate (`是`, `为`, `就是`, `改成`, `改为`, `设为`, `设置为`, `换成`,
`更新为`, `重置为`, `如下` and similar, optionally with `已`/`已经` before and
`了` after it), is removed when the value looks like a credential: at least six
printable ASCII characters without spaces, including a digit or a symbol
(`我的密码是Abc123!`, `密码改成了 Qwer1234!`, `验证码884213`); a `?`, `.`,
`,` or `;` that ends it ends the sentence instead (`令牌还是 cookie？` is
kept), and an algorithm name is not a value (`密码改成 AES-256 加密`). A value in
quotes (`「」`, `『』`, `【】`, `“”`, `‘’`, `《》` or ASCII quotes) needs only
four characters (`口令「opensesame」`). With any other wording, the rest of
the sentence after a Chinese label is searched as well: within twelve Chinese
characters, symbols or ASCII words, and before a `。` or a `!`, `?` or `;`
(full-width ones included) that stands alone or ends a word, an ASCII word
that looks like a password, or a quoted ASCII word of at least six
characters, removes the line. Punctuation that ends a word is not part of it
(`MySQL？`, `Codex！`), except that `!` after letters and digits still counts
as a symbol (`Abc123!`). A password symbol such as `@` or `#` next to letters
or digits, and letters with digits (`hunter2`, `admin2024`), always count:
`初始密码默认是 Admin@123`, `用户名和密码分别是 admin 和 Abc123!`,
`密码我改成了 Abc123!`, `密码👉Abc123!`. Shapes that usually name something
else count only when the wording hands the word over as a value (a copula
or setter such as `是`, `为`, `成`, `用`, `填`, `叫`, `了`, `和`, `默认`,
`就`, `统一`, a colon, an arrow or an emoji right before it): only digits, and not
followed by a unit such as `毫秒`, `秒`, `次`, `个` or `QPS` (`WiFi
默认密码统一是 12345678` is removed, `令牌桶容量 100000` is kept), a hex run
whose letters and digits alternate like a commit hash (`a1b2c3d`), an
identifier with `_` or `-`, and a product or standard with a version number
(`iPhone15`, `Node20`, `RTX4090`, `RFC7519`, `iPhone16Pro`; for these `用`
and `和` do not count). Words that name something else are never values
there: paths and URLs, emails, `package@1.2.3`, versions and IP addresses
(also `Python3.11`), file and dotted names (`auth.rs`, `bcrypt.compare`),
dimensions (`320px`, `1.5em`, `120x40`), hex colors, algorithm and encoding
names (`AES-256`, `JWT-HS256`, `argon2id`, `HKDF-SHA256`, `base64`), a word
right after `commit`, `id`, `port`, `version`, `traceId`, `trace_id` and
similar, and a word right after (optionally with a colon, `是` or a setter
such as `改成` in between) `提交`, `版本`, `分支`, `端口`, `错误码`, `编号`,
`工单`, `型号`, `上限`, `耗时`, `次数`, `超时`, `模板`, `参考` and similar.
So sentences such as `验证码为空时报错`, `密码是abcdef`, `密码改成 bcrypt
加密`, `密码页面的 commit 是 3f2a9c1`, `验证码存 Redis 还是 MySQL？`,
`已修复令牌刷新逻辑，提交 3f2a9c1。`, `令牌过期处理改好了，在 iPhone15
上验证通过。`, `令牌桶限流已上线，QPS 上限 100000。` and
`密钥轮换脚本跑完了，耗时 1234567 毫秒。` are kept. The short keys `pass`,
`pwd`, `pw`, `psw`, `pswd` and `pin` count only in an assignment:
`db_pass=hunter2`, `PIN：884213`, `userPin=1234`, `PW: Abc123!` (a value
after a colon needs a digit or symbol, so `pin: string` and `pw: string` are
kept; `pinned`, `passing`, `bypass`, `pass_rate`, `upward` and `--- PASS:
TestName` are not keys). A bare key name (`key`, `SK`, `AK`, or `app`,
`secret`, `access`, `private`, `client`, `signing` and similar followed by
`key`: `appKey`, `secret_key`) before a colon, an equals sign or a Chinese
predicate (`高德的 key 是 …`, `appKey 改成了 …`) counts when the
value looks like a key: at least ten characters with letters and digits, and
mixed case, a password symbol or at least 20 characters, without `_`, `.`,
`:` or `/` (`key：Abc123!xyz`, `SK：Xyz12345678abcdefghij`); `primary key: id`,
`cache key: user:1234:profile`, `key: user_profile_2024` and `cacheKey: …` are
kept. Command-line credentials are removed too: `mysql`/`mysqladmin`/
`mysqldump` (and the other MySQL and MariaDB clients) with `-p<password>`,
`--password=`, or `-p` followed by a password-like word (`mysql -u root -p
Hunter2024`; `mysql -uroot -p app` is kept), `curl`/`wget` with
`-u`/`--user` and `user:password`, `sshpass -p` (also `-p<password>`),
`redis-cli -a`/`--pass`, `docker`/`podman`/`helm`/`nerdctl login
-p`/`--password`, `mongo`/`mongosh`/`mongodump`/`mongorestore -p`/
`--password`, `sqlcmd`/`bcp -P` (also attached), `ldapsearch` (and the
other OpenLDAP clients) `-w` (also attached), `zip`/`unzip -P`, `7z`/`unrar
-p<password>`, `keytool`/`jarsigner -storepass`/`-keypass`,
`mosquitto_pub`/`mosquitto_sub -P`, `openssl -k`/`-K`/`-pass`/`-passin`/
`-passout`, `smbclient -U user%password` and `lftp -u user,password`. A tool's
options end at a shell separator (`|`, `&&`, `||`, `;`, `&`) or the next
command, so `docker run -p 8080:80`, `ssh -p 22`, `redis-cli -h` and `curl … &&
docker run -u 1000:1000` are kept.

A Markdown table whose header row names a credential column (a header cell
that ends with one of the labels above: `密码`, `初始密码`, `Password`, `API
Key`, `AccessToken`, `Token (prod)`) loses its header, its separator row and
every data row, until a line without `|` or a blank line ends the table. A
header cell that counts (`Token 数`, `输入 Token`, `Input Token`, `Token
count`) is not a credential column. A column named only `Key`, `App Key`,
`SK`, `AK` or a similar key name drops just the rows whose cell in that column
looks like a key (as for a bare key name above), so a table of setting names
keeps its rows. A comma- or tab-separated header row (short cells; a
comma-separated cell has no spaces) that names a credential column, such as
`username,password` or a header copied from a spreadsheet, drops the
following rows with as many separators and an ASCII value in that column,
until another line or a blank line.

A line that ends with a label after other words (`数据库密码`, `Redis 密码`,
`MySQL root password`, `GitHub token`, `username,password`) is removed, and
so is the next non-empty line when it looks like a value: at most three
ASCII words, one of which looks like a password or a key (as above), or a
single word of at least four characters with a digit or a symbol
(`Abc123!`, `NewPass@2026`, `1234`, `admin / Abc123!`). A list item that only
names a label (`2. 密码`, `- Password`, `· 密码`) stays, and the next line
goes on the same condition, so a list of form fields (`1. 用户名`, `2. 密码`,
`3. 验证码`) and ordinary sentences, versions and file names after such a
line stay. After such a list item a single word of at least six letters or
digits (`opensesame`) also counts as a value unless it is the next list
item; after a line ending with a label it does not. Whenever a label's value is
expected on the next line, here or as described below, a code block opening
there (three backticks or tildes, optionally with a language, or `<pre>`) is
the value and is removed up to its closing fence, and a line of punctuation
only (a lone quote, `---`) is skipped so that the value is the line after
it.

When a label has no value on its own line (`密码：`, `token:`, `我的密码是`,
`密码如下：`, `数据库密码（测试环境）：` with at most six Chinese characters
between the label and the final colon, or the label alone), or it is followed
only by the opening of a multi-line value (`password: |`, `password: >-`,
`"passwords": [`, `token = (`, `secret: {`, `\`, a quote), the next non-empty
line is removed as its value too, and so is every following line indented
deeper than the label line, until the indentation returns (YAML block scalars,
bracketed lists).

The known secret formats (`sk-`, `ghp_`, `xoxb-`, private key blocks and
similar; `AIza`, `glpat-`, `sk_live_`, `rk_live_`, `sk_test_`, `hf_`, `npm_`,
`xapp-`, `gho_`, `ghs_`, `ghu_`, `ghr_` and Alibaba Cloud `LTAI` keys when
they start a run of key characters and are long enough) and long
high-entropy tokens are checked per line and once more over the whole bounded
text; a hit there drops the prompt. Tokens are also split at every non-ASCII
character (CJK text, CJK and full-width punctuation, Chinese quotes), so a
key written straight against Chinese text (`用这个AIza…调一下`, `…FBWY。`) is
found as well. A token that is not a URL or path is also split at quotes,
colons, commas, brackets, braces, `=`, `;`, `.` and other punctuation, and
each run of key characters that is long and high-entropy and contains a digit
removes the line, so keys in compact JSON or code (`{"appKey":"9f8e…"}`,
`appKey="…"`, `key:…`, `Client("…")`) are found; long identifiers without a
digit (`handleCompanionSnapshotRequest`) are kept. Telegram bot tokens
(`<bot id>:<secret>`), Discord bot tokens (three dot-separated parts) and URLs
or connection strings with a password (`user:password@` before the host, as
in a PostgreSQL connection URL) remove the line too. The prefixes `sk-`,
`ghp_` and `akia` (any case), which are shorter than five characters, count
only at the start of a run of key characters followed by at least 16 more,
so `task-runner`, `flask-login` or `Slovakia` are not keys. Result excerpts (`summary`) go through the same line
filter; they leave out code blocks fenced with three backticks or tildes as a
whole, and a value expected after a label before such a block is the block,
not the line after it.
Like result excerpts the prompt is held only in Runtime memory, is replaced by the
next turn, is never written to SQLite, spool, exports or snapshots, and is lost
when Runtime restarts. A prompt from an earlier turn is never returned for the
current one.

The Companion review adds `repository.files`: at most 20 changed files from
the same repository and base as the local review/diff route, ordered by changed
lines (`insertions + deletions`) descending. Each entry is `{path, insertions,
deletions, status}` with a repository-relative path and status `modified`,
`added`, `deleted`, `renamed` or `untracked`. Untracked file contents are never
read, so their line counts are zero. File contents and patches are never
returned; the local `/api/v1/sessions/{id}/review` response is unchanged. Git
runs on Runtime's blocking pool, never on the API thread, and the repository
root, HEAD and status resolved for the review summary are reused for the file
list. For 2.5 seconds a session's review is reused when its inputs (turn,
working directory, baseline, concurrent sessions) are the same and one
`git status --branch` probe still matches HEAD, branch and working tree status
exactly; otherwise it is recomputed.

## Turn end: background work and awaiting a reply

These are additive v7 fields; `protocolVersion` and every schema version stay
unchanged, and existing fields keep their meaning.

A Provider `Stop` ends the turn as `response_finished` and, when the turn held
real work, raises a `completion` Attention. Claude Code also reports the
session's background work with `Stop`: `background_tasks` (each item carries a
`status`) and `session_crons`. Only active items hold the completion back: a
`status` of `running`, `pending`, `queued`, `in_progress` or `started` (any
case; `-` or a space count as `_`), or an item without a string `status`
(cron jobs have none, and an unknown shape keeps the earlier behaviour).
Finished items (`completed`, `failed`, `killed`, `stopped`, `cancelled`,
`done` or any other status) do not count, and neither does ambient work that
Claude Code keeps running on its own for the rest of the session: the
live-update monitor attached to every published Artifact (`type` `monitor`
with a `description` starting `live updates for artifact`), memory
consolidation (`dream`), the auto-mode scan (`auto-mode scan`) and memory
import (`memory import`).

A Claude Code `Stop` always ends the turn, also while active items remain: the
Agent has handed control back and the next move is the user's. The session
becomes `response_finished` (never `thinking`, `tool_running` or
`compacting`), the turn is recorded as `response_finished` with its end time,
and `activityMessage` is `session.activity.background_tasks_running` with
`count` the number of active items. No completion is raised for such a turn
(each one would otherwise announce "task finished" while the work goes on),
and active subagents stay active. Clients that need to tell this waiting state
from a plain finished turn read `backgroundTasks`:

| Snapshot session field | Type | Meaning |
| --- | --- | --- |
| `backgroundTasks` | integer, never null | Active background items the session's last `Stop` reported; `0` when there were none and as soon as the session leaves `response_finished` (a new prompt, a tool start of the main Agent, a session start or end) |

`execState` `response_finished` with `backgroundTasks > 0` therefore means
"the Agent waits for the user while N background items run". Such a session
stays in the snapshot for a further 24 hours past the usual 30-minute
retention of a finished session, since no completion keeps it listed.

The next turn makes the session run again: a new prompt (Claude Code also
submits one, starting with `<task-notification>`, when a finished background
task wakes the Agent), a tool start of the main Agent (a `Stop` Hook that
blocks continues the same turn) or a Connector `turn/started`. A Hook from
inside a subagent (Claude Code adds `agent_id` only there) does not: a
background subagent keeps calling tools after the main turn ended, and those
events neither reopen the turn nor change `execState`.

Claude Code's `Notification` with `notification_type` `idle_prompt` ("Claude
is waiting for your input") is sent only by its terminal interface, 60 seconds
after a turn ended without input (later while a background subagent, workflow
or teammate still runs); the VS Code extension, the desktop app and other SDK
clients never send it. When it arrives while the session is still shown as
working (its `Stop` was lost) and no approval, question or elicitation is
pending, the session ends the turn the same way: `response_finished`, the turn
ended, activity "Turn completed", and no completion.

For diagnosis, the internal `events.summary` of a Claude Code `Stop` lists the
reported background items by type and status, for example
`background: subagent running×2, shell pending×1, monitor running ambient×1, cron×1`.
It never contains a description, command, prompt or identifier, and it is not
part of any client response.

Snapshot Attention items carry `awaitsReply` (boolean, `false` by default). It
is `true` on a `completion` whose turn ended with a final Agent message that
asks the user something: the Agent has stopped and waits for the user's
answer. Such a completion is raised even while active background work remains,
because the user is needed either way; the session then also reports the
running work (`backgroundTasks`, `background_tasks_running`). Without a question the
background rule above applies unchanged. Every other Attention kind, and a
completion raised without a final message (a Codex native approval that ends a
turn, a `Stop` replayed from the offline spool, which never keeps message
text), is `false`. Only the judgement is stored; the message text stays where
result excerpts keep it (Runtime memory) and is never written to SQLite.

The message comes from the Claude Code or Codex `Stop` Hook
(`last_assistant_message`), from the Codex Connector (the turn's last
`final_answer` agent message, judged when `item/completed` delivers it and
applied to that turn's `turn/completed`, whose `turn.items` is empty), or from
the Codex session file (`task_complete.last_agent_message`). Whichever of these
ends the turn first decides; a later report of the same turn is a duplicate.

The judgement is conservative and only looks at the end of the message:

1. Fenced code blocks (three or more backticks or tildes; an unclosed fence
   runs to the end), headings, horizontal rules, table rows and block quotes
   are dropped. In the remaining lines, list markers, task boxes, emphasis
   markers, inline code spans, bare URLs and link targets are removed (a link
   keeps its text), so a question inside backticks never counts.
2. The last non-empty line that remains is the end of the last prose
   paragraph. Its last sentence starts after the last `.`, `!`, `?` or `;`
   followed by whitespace, or after any `。`, `！`, `？`, `；` or `…`.
3. The sentence awaits a reply when it ends with `?` or `？` (closing quotes,
   brackets, emphasis or emoji after the mark are ignored); when it ends with
   the particle `吗` or `呢`; when it contains `要不要`, `选哪`, `说一声`,
   `可以吗`, `好吗` or `行吗`; `你选`/`您选`, `告诉我`, `你决定`/`您决定`
   unless `的`, `了` or `过` follows (`你选的方案`, `你告诉我的路径`); `是否`
   at the start of a clause or in a clause that opens with `你`, `您`, `请`,
   `麻烦` or `帮我` (`请确认是否合并`, not `我检查了是否有遗漏`); `确认一下`
   unless the clause has `我` without addressing the user (`我再确认一下`);
   `要我` at the start of a clause only (not `不需要我`); or, case-insensitive
   and between non-letters, `should I`, `shall I`, `do you want`,
   `would you like`, `would you prefer`, `do you prefer`, `let me know`,
   `please confirm`, `can you`, `could you`, `which one(s)`, `which option(s)`,
   `which approach`, `which do you` or `which would you` (a bare `which` is
   usually a relative pronoun and does not count). Phrases are looked for in
   the last 2,048 bytes of the sentence.

Questions earlier in the message do not count (`要不要提交？` followed by a
paragraph `已经提交并推送了。` is `false`), and neither does a conditional
offer without an explicit request: `你要是也觉得没用，我一并删掉。`,
`如果需要，我可以再加测试。` and `If you want, I can also add tests.` are
`false`, while `要提交的话说一声。`, `需要的话告诉我。` and
`Let me know if you want it pushed.` are `true`. `已完成，测试全部通过。` and a message that is only a code
block (`foo?` inside a fence) are `false`; `Should I push?` is `true`.

## Latest Agent message

These are additive v7 fields; `protocolVersion` and every schema version stay
unchanged, and existing fields keep their meaning.

Each snapshot session carries `lastMessage`, the latest text the Agent wrote
in the session's main conversation, at any point of a turn, or `null`:

| Field | Type | Meaning |
| --- | --- | --- |
| `text` | string | Sanitized excerpt: the end of the message, at most 400 characters |
| `at` | integer | When the Agent wrote it (milliseconds since the epoch) |
| `final` | boolean | The message is its turn's final answer: it came with the turn's end (`Stop`, Codex `final_answer`), or the turn ended right after it |
| `truncated` | boolean | Something of the message is not in `text` |

The excerpt is the end of the message, where an Agent puts its conclusion or
question: whole lines from the end; when room remains, the line before
contributes its last sentences, starting after a sentence end (`.`, `!`, `?`,
`;` before a space, or `。`, `！`, `？`, `；`, `…`), else after a space, else at
the character bound. A last line longer than the bound is cut the same way.
`truncated` is `true` when the start was cut off or anything was left out (a
code block, a line that looked like a credential); a client may show a
leading ellipsis. The message is not cleared when a new turn starts: compare
`at` with `turnStartedAt` to tell whether it belongs to the current turn.
`lastMessage` is `null` when no message is known, when the message is more
than 24 hours old, or when the session had no event in the last 24 hours.

`GET /api/v1/companion/sessions/{id}/result` adds `latest`, the same object
for the same message but with the start of the message within the result
excerpt bounds (the first 60 non-empty lines and 4,000 characters, code blocks
skipped). Unlike `result`, which only a finished turn has and whose meaning is
unchanged, `latest` is also returned while the turn is running.

Sources, newest first wins:

- Claude Code: the `Stop` Hook's `last_assistant_message` (`final: true`), and
  the session transcript. The transcript path comes from the Hooks'
  `transcript_path` (accepted only for `<session id>.jsonl` inside a Claude
  Code `projects` directory; stored in `sessions.transcript_path` so it
  survives a Runtime restart) or, for older rows, is derived as
  `~/.claude/projects/<encoded cwd>/<session id>.jsonl` (`CLAUDE_CONFIG_DIR`
  replaces `~/.claude`; every UTF-16 unit of the working directory that is not
  an ASCII letter or digit becomes `-`, and a name longer than 200 characters
  is cut to 200 and followed by `-` and Claude Code's base-36 hash). Claude
  desktop app sessions use the same location. The transcript is read from its
  end (64 KB, doubling up to 4 MB): the latest `type: "assistant"` record with
  non-empty `text` content, skipping subagent records (`isSidechain: true`),
  thinking and tool-use blocks, API errors (`isApiErrorMessage`), meta and
  synthetic (`model: "<synthetic>"`) records and a half-written last line; text
  blocks of one message are joined. A transcript message is `final` when the
  session's latest turn has ended and the message was written during it.
- Codex: the `Stop` Hook's `last_assistant_message` (`final: true`), the
  Connector's `agentMessage` items (`final` for phase `final_answer`), and the
  latest assistant message of the session's rollout file (`final` for phase
  `final_answer`).

Every excerpt passes the result-excerpt line filter and the whole-text secret
check. Message text and excerpts live only in Runtime memory; SQLite, the
spool and exports never receive them (only the transcript path is stored), and
after a Runtime restart a Claude Code message is read again from its
transcript and a Codex message from its rollout file. Transcripts are read
only for sessions in the snapshot with an event in the last 24 hours, and only
when the file changed since the last read (path, size and modification time);
when it only grew, just the appended lines are read.

## Client application

Each snapshot session also names the application it runs in, as the Hook
process recorded it (`null` when unknown). Only the Companion snapshot carries
these fields; `environment` keeps its meaning as the display label.

| Field | Type | Meaning |
| --- | --- | --- |
| `termApp` | string or null | Application name |
| `termBundleId` | string or null | macOS bundle identifier |
| `termSurface` | string or null | Kind of client, one of the values below |

| `termSurface` | Recorded for | `termApp` / `termBundleId` examples |
| --- | --- | --- |
| `terminal` | A CLI in a terminal or multiplexer: Terminal, iTerm2, Warp, Ghostty, WezTerm, Alacritty, kitty, Windows Terminal, tmux, GNU screen, or any other terminal that sets `TERM_PROGRAM` | `Terminal`/`com.apple.Terminal`, `iTerm2`/`com.googlecode.iterm2`, `Warp`/`dev.warp.Warp-Stable`, `Ghostty`/its bundle identifier, `tmux`/null |
| `editor` | An editor or IDE: its Agent extension, or a CLI started in its integrated terminal (both run below the editor's app bundle): Visual Studio Code, VS Code Insiders, Cursor, Windsurf, Zed, Xcode, JetBrains IDEs | `Visual Studio Code`/`com.microsoft.VSCode`, `Cursor`/its bundle identifier |
| `codex_app` | The Codex app or the ChatGPT app | `Codex app`/`com.openai.codex`, `ChatGPT app`/`com.openai.chat` |
| `claude_app` | The Claude app, including the Claude Code sessions it runs | `Claude app`/`com.anthropic.claudefordesktop` |
| `kimi_app` | The Kimi Code app | `Kimi Code app`/`com.kimi.code.desktop` |
| `grok_app` | The Grok app | `Grok app` |
| `desktop_app` | Any other app bundle found among the Hook's parent processes | the app's name and bundle identifier |
| `connector` | An ActRealm Connector session (Codex app-server, ACP) | the Connector name, e.g. `ActRealm ACP` |
| `remote` | A session reached over SSH | `SSH`/null |

The first matching rule wins: an app bundle among the Hook's parent processes,
then SSH, then tmux or GNU screen, then `TERM_PROGRAM` / `LC_TERMINAL` and the
bundle identifier of the environment, then an explicit `ACTREALM_SURFACE`, and
finally any other `TERM_PROGRAM` value as `terminal`.

An Agent's own app bundle that is not a known client is passed over in favor
of the app that launched it, and used only when no rule above matches. Claude
desktop runs Claude Code from such a bundle, a background-only `claude.app`
(`com.anthropic.claude-code`), so its sessions are recorded as `claude_app`.
Sessions recorded with that bundle before this rule, or another Agent run
inside Claude desktop, still jump to the Claude app.

An app inside a framework is never a client: `python3` runs as the
`Python.app` inside `Python.framework` (Homebrew, python.org) or
`Python3.framework` (Xcode, Command Line Tools), so an Agent started by a
Python script is recorded under the app that ran the script. Sessions recorded
under that `Python.app` before this rule never jump to it: a Codex
conversation opens directly, and other providers have no jump target.

## Quota entries

Each `quota[]` item of the snapshot describes one Provider quota window.

| Field | Type | Meaning |
| --- | --- | --- |
| `provider`, `window` | string | Provider and window identity (`10080m`, `5h`, `7d`, `extra_usage`, ...) |
| `status` | string | `available`, `stale` (values kept, not current) or `unavailable` (no values) |
| `usedPct`, `remainingPct` | number, optional | Percentages, 0 to 100 |
| `resetsAt` | integer, optional | Window reset, epoch seconds |
| `source` | string | `codex_app_server`, `rollout_experimental`, `oauth_usage`, `statusline`, ... |
| `windowMinutes`, `limitId`, `limitName`, `planType`, `quotaKind` | optional | Window metadata |
| `capturedAt` | integer, optional | When the source produced the value, epoch milliseconds rounded down to 30 s |
| `reasonMessage` | object, optional | Why the item is `stale` or `unavailable` |
| `spentMinor`, `limitMinor`, `currency`, `currencyExponent`, `disabledReason` | optional | Claude Extra Usage only, see below |

### Freshness

`capturedAt` is the time the value was produced, never the time the snapshot
was built:

- `codex_app_server`: when Runtime received Codex's `account/rateLimits/read`
  response or an `account/rateLimits/updated` notification. Runtime reads the
  account every 60 seconds through the Codex app-server (its own
  `codex app-server --stdio` child, or the shared daemon when one is running),
  so this is server-side usage including other devices.
- `rollout_experimental`: the `timestamp` of the Codex `token_count` event that
  carried the limits (the rollout file's modification time only for early
  Codex records without a timestamp). A rollout only changes when Codex runs on
  this Mac.
- `oauth_usage`: when Claude's usage response was received; `statusline`: when
  Claude Code's StatusLine reported the value.

An item is `stale` with one of these `reasonMessage` codes:

| Code | When |
| --- | --- |
| `quota.reason.window_reset` | `resetsAt` has passed; the value describes the previous window |
| `quota.reason.codex_refresh_failed` | The last official Codex read is more than 15 minutes old, or only an earlier official snapshot is left |
| `quota.reason.codex_rollout_stale` | A rollout value more than one hour old |
| `quota.reason.cache_stale`, `quota.reason.claude_refresh_failed` | A Claude value more than 15 minutes old, or the latest Claude refresh failed |

### Codex source selection

The official app-server snapshot is used while it is at most 15 minutes old.
After that the last official values stay, marked stale, unless a rollout has
recorded a newer reading, which then replaces them. Without any official
snapshot the newest rollout reading is used, chosen by event time across the
recent rollout files, not by file modification time. A plan with only a weekly
window (Codex Pro: `windowMinutes: 10080`, no five-hour window) is a complete
standard quota. Codex's `credits.balance` is not exposed.

### Claude Extra Usage

`window: "extra_usage"` is the overage spend for the current billing month, not
a window that stops the Agent, and appears only on Claude items.

| Field | Type | Meaning |
| --- | --- | --- |
| `spentMinor` | integer, optional | Spend so far this billing period, in the minor unit of `currency` (cents for USD) |
| `limitMinor` | integer, optional | Monthly cap in the minor unit of `currency`; absent when there is no cap |
| `currency` | string, optional | ISO 4217 code in upper case (`USD`); the two amounts never appear without it |
| `currencyExponent` | integer, optional | Decimal places reported by Claude; one major unit is `10^currencyExponent` minor units. When absent use the ISO 4217 minor unit (USD: 2) |
| `disabledReason` | string, optional | Why Extra Usage is off: Claude's own code (such as `org_level_disabled`), or `user_disabled` when the account holder turned it off |

While Extra Usage is on, the item is `available` with `usedPct` equal to spend
over cap (Claude's `utilization`, or `spentMinor / limitMinor` when Claude
omits it). While it is off, the item is `unavailable` with no percentages,
`reasonMessage.code` `quota.reason.extra_usage_disabled`, `capturedAt` of that
response and, when known, `disabledReason`. Every successful Claude usage
response replaces the previous Extra Usage state, and an Extra Usage value more
than 31 days old is never shown.

```json
{"provider": "claude", "window": "extra_usage", "status": "available",
 "usedPct": 90.0, "remainingPct": 10.0, "source": "oauth_usage",
 "limitName": "Extra usage", "capturedAt": 1791712530000,
 "spentMinor": 4500, "limitMinor": 5000, "currency": "USD", "currencyExponent": 2}
```

# ActRealm local Companion protocol

Status: local v1, schema version 1. The first consumer is the display Companion.

Runtime health reports `protocolVersion: 5` once all local Companion routes in
this document, including activity, review and completion settings, are available. Consumers
must not attempt Companion pairing with an older protocol. This compatibility
gate is independent from the snapshot schema version above: protocol v5 still
uses the allowlisted snapshot schema v1.

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
| `GET /api/v1/companion/settings/completion` | `snapshot.read` | Read the completion-task retention mode and configured delay |
| `PUT /api/v1/companion/settings/completion` | `attention.respond` | Update only the completion-task retention mode and delay; all other settings remain unchanged |
| `GET /api/v1/companion/sessions/{id}/activity` | `snapshot.read` | Bounded, sanitized activity for the current turn; accepts `limit=1...100` and an optional `afterIngestSequence` cursor |
| `GET /api/v1/companion/sessions/{id}/review` | `snapshot.read` | On-demand branch/worktree, bounded Diff counts, validation states and outcome evidence; never returns patches, commands or file contents |
| `POST /api/v1/companion/sessions/{id}/jump` | `session.jump` | Ask ActRealm to perform its current safe jump behavior |
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
official standard-plan entry never proves that the account is Pro.

It excludes raw prompts, complete commands, tool input/output, file contents,
transcripts, answers, secrets, Hook payloads, Provider reply channels, private
jump locators, Web credentials, Cloud credentials, and Provider cookies.

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

## Protocol v5: local result excerpts and referenced files

`GET /api/v1/companion/sessions/{id}/result` requires `snapshot.read` and returns
`{schemaVersion: 1, sessionId, result: null | {sessionId, observedAt, source,
summary, truncated, artifacts}}`. It is available only for the latest ended or
failed turn. A new turn invalidates old results. `source` identifies Stop Hook
text or a final Codex response; the text is Provider-reported, not independent
verification of the claims inside it. Each excerpt has at most 600 characters.

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

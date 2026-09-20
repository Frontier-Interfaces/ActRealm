# Automatic managed session connections — 2026-09-20

## Requested behavior

Remove the manual connection choice. Runtime attempts to connect observed
current Codex sessions by default, including existing task cards, newly observed
sessions and recovery
after Runtime restart. The old 32-entry manual restoration limit does not cap
the automatic discovery queue. Dormant archived transcripts do not become new
tasks merely because they exist on disk.

## Implementation

At startup, the connector prefers the official `codex app-server proxy --sock`
transport when a same-user, non-writable-by-others control socket exists. It
otherwise uses an owned stdio app-server. Proxy shutdown closes only the
ActRealm client, not the shared service.

A dedicated worker reads the Runtime task inventory and calls the existing
`thread/resume` RPC with the same thread ID and `excludeTurns: true`. Each pass
attempts at most four sessions; failures back off from five to sixty seconds.
A failed session does not prevent the rest from connecting. State locks are
released before RPC calls, keeping the HTTP/UI path independent.

Only a successful response for the requested thread establishes a connection.
An idle independent app-server cannot end a running turn in the original
Desktop window. Closing/unloading a thread returns it to the automatic queue.
Native task details show pending, connecting, retrying, owned elsewhere,
unavailable or connected states; the button and its Swift/web action implementations are removed. The
old authenticated route remains compatible but is no longer required by clients.

Grok leader sessions and Kimi desktop/web retain their existing automatic
integration. Claude retains automatic Hook interaction. Standalone Kimi CLI
still needs the ACP entry point because no equivalent attach protocol exists
in the integrated path; unsupported entry points are not labelled controllable.

## Verification

Protocol fixtures cover startup connection, newly observed sessions, Runtime
restart rediscovery, identity mismatch and failed-resume rejection, bounded
retry timing, more than 32 observed sessions, and no `turn/start` or `turn/steer`
request from automatic connection. Existing approval/question ownership,
version checks and live waiter tests remain mandatory. Shared-socket tests
reject symlinks and writable endpoints and verify official proxy invocation.
The generated local ThreadResumeParams schema and the live writer-lock error
are captured in the private local acceptance directory.

Official protocol reference: [Codex app-server](https://developers.openai.com/codex/app-server).
`thread/resume` loads and subscribes to an existing thread; `turn/start` is the
separate operation that begins generation. Attaching does not transfer pending
requests from another app-server connection.

## Local Codex limitation found during acceptance

The installed Codex 0.154.0-alpha.6.2 returns `already has an active writer` when
an independent app-server resumes a thread still loaded by the original app.
Its generated ThreadResumeParams schema provides no force-attach/read-only
bypass. The current desktop process exposes no official shared control socket;
`app-server daemon version` reports the socket missing. Default automatic
connection is implemented, but simultaneous managed attachment of these
original-window sessions is blocked by the provider. They display "In use by
the original window; waiting to connect automatically" and continue bounded
retries. No writer locks are deleted and no original Codex process is stopped.

Shared proxy transport is covered by local protocol/ownership tests. Current
live acceptance can establish automatic attempts, truthful ownership status,
button removal and continued original execution; it cannot establish successful
shared-daemon attachment when that daemon is absent.

## Installed result — build 133

The installed native app has no manual connection button. Authenticated Runtime
snapshots report `automaticConnection: true` and five successfully attached
threads. The two currently running Desktop sessions are both `owned_elsewhere`
with `canManage: false`; both retain their actual running state. This is not
complete managed control of every live Desktop session. The app and shared
helper pass strict signature and matching-hash checks; Display remains unchanged.

444 Rust tests (3 ignored) and 221 Swift tests pass, including automatic
connection fixtures, ownership-conflict handling and shared-transport tests.
Clippy, release build, localization, JavaScript syntax, Info.plist and diff
checks pass. Full workspace regression is recorded in the acceptance directory.

This report records acceptance before submission to PR #10. Display is outside
the change scope. No provider model prompts, merge or public release were
performed during this acceptance.

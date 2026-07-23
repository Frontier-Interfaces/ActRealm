# M15 Codex managed approvals

Status: implemented and covered by generated-schema contract tests. A final
real Codex Desktop acceptance pass is still required before release.

## Capability boundary

ActRealm answers a Codex approval request only when all of these are true:

1. Runtime started the official Codex `app-server --stdio` child;
2. the request itself arrived on that ActRealm-owned app-server connection;
3. the user explicitly attached the Thread to ActRealm;
4. initialize returned a supported server version;
5. the request is one of the verified methods below; and
6. the in-memory waiter is still live.

Attaching or resuming a Thread does not transfer an already running Turn from
Codex Desktop's private app-server connection. An approval sheet already shown
by Codex Desktop therefore remains Provider-owned: ActRealm can observe it,
keep it in OUTBOX, open Codex, snooze it, or let the user mark it handled, but
cannot fabricate a response on a connection that never received the request.

The verified schema family is Codex 0.144.5–0.144.x:

- `item/commandExecution/requestApproval`;
- `item/fileChange/requestApproval`;
- `item/permissions/requestApproval`.

Earlier, future, malformed, detached, expired, and native-only requests never
receive guessed responses. Their UI remains observation-only or returns the
user to Codex.

## Response safety

- Command and file requests support one-turn `accept` or `decline`.
- Permission allow returns only the requested `network` and `fileSystem`
  profile fields and uses turn scope.
- Permission deny returns an empty permission profile.
- Null and unknown future permission fields are not promoted into a grant.
- The existing three-second undo window applies before the response is sent.
- `serverRequest/resolved` confirms the command and resolves Attention. Item
  completion remains a second reconciliation signal.
- A preceding `waitingOnApproval` observation card is resolved as soon as the
  authoritative direct request arrives, so OUTBOX never shows duplicate
  native and actionable approvals for the same managed Thread.

## Truthful UI

Tasks display one of these truthful states:

- `托管请求已接入，可直接审批` only for a live request-keyed waiter;
- `app-server 已连接；原生审批仍需在 Codex 处理` when only the
  ActRealm connector is available;
- `原界面处理` for a Provider-owned native approval.

Hook-only and independently running Codex Desktop conversations are not
silently claimed. Merely enumerating, attaching, or resuming their Thread is
not sufficient to approve an in-flight native request.

## Automated coverage

Tests cover:

- initialize version parsing and future-version fail-closed behavior;
- all three request constructors and protocol identity;
- command approval response round-trip;
- permission least-privilege allow and empty deny;
- `serverRequest/resolved` command confirmation;
- observation-only native card replacement;
- detached Thread rejection and waiter expiry paths;
- capability decoding and direct-control labels.

The release remediation report records the exact full-gate results for the
candidate commit.

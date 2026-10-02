# ActRealm native client architecture

## Decision

ActRealm is one local Runtime with separate native macOS and Windows clients.
The repository is a monorepo so API, privacy, packaging, and acceptance changes
can be reviewed together, but platform UI code is not shared.

```text
ActRealm/
├── crates/                    Rust Runtime and actrealm CLI
├── web/                       Existing browser control surface
├── apps/
│   ├── macos/                 SwiftUI + AppKit client
│   └── windows/               WinUI client (planned)
├── shared/
│   └── contracts/             Stable cross-platform schemas
├── fixtures/                  Provider Hook fixtures
└── docs/                      Product and acceptance documentation
```

## Ownership boundary

| Concern | Rust Runtime | Native client |
| --- | --- | --- |
| Provider Hooks and app-server protocol | Owns | Never duplicates |
| SQLite, WAL, retention, export | Owns | Never opens directly |
| Approval/request capability | Owns and declares | Renders declared capability |
| Sanitization and privacy limits | Owns | Consumes sanitized fields |
| HTTP/WebSocket authentication | Serves and validates | Bootstraps and reconnects |
| Window, tray/menu-bar, notification UI | No | Owns per platform |
| Foreground scheduling policy | Supplies attention facts | Owns local preference |
| App/window activation | No | Owns per platform |

Local companion applications are a third, narrower client class. They never
bootstrap through the Web session and receive no SQLite, Hook, Provider, or
Cloud access. The Runtime issues each explicitly paired companion an
independent bearer token with `snapshot.read`, `session.jump`, and optionally
`attention.respond` scopes. The Runtime still owns sanitization, capability
declaration, expiry, waiter liveness, Provider replies, and the three-second
approval commit window.

This boundary prevents two processes from competing for SQLite or Provider
reply channels and keeps foreground behavior replaceable per operating system.

## Runtime flow

```text
Claude/Codex event
  -> actrealm Hook or managed Connector
  -> Runtime normalization and SQLite state
  -> authenticated snapshot/WebSocket
  -> native derived state
  -> OS-specific presentation and foreground scheduling
  -> user action through Runtime API
  -> Runtime waiter and Provider reply channel
```

The client may foreground an Agent application when a sanitized Attention item
arrives. It must not infer that the Provider supports a reply, or mark an item
approved, denied, or completed without a matching Runtime transition.

## Local companion flow

```text
ActRealm Settings creates one-time pairing code
  -> companion enrolls over 127.0.0.1
  -> Runtime stores only SHA-256(token) and explicit scopes
  -> companion stores the token in its own OS credential store
  -> companion polls the sanitized allowlist snapshot
  -> user chooses an available action
  -> Runtime revalidates scope + capability + ID + expiry + live waiter
  -> existing Runtime command/question path owns the Provider reply
```

Pairing codes expire after five minutes and are single-use. Registrations are
revocable from ActRealm Settings. Runtime port discovery uses
`~/.actrealm/run/companion-endpoint.json`, a current-user private file that
contains only schema version, loopback endpoint, and Runtime instance ID. It
does not contain a bearer token or Provider state. The complete contract and
threat model are in `COMPANION_PROTOCOL.md`.

## Foreground scheduling

Keep the scheduling state machine small and portable:

1. An open Attention item arrives with stable ID, Provider, kind, and safe title.
2. The client applies its local global policy and optional Provider override.
3. The policy enters `reminding`, `opening`, `awaitingWorkspace`, or
   `returnedToActRealmWorkspace`.
4. A platform executor activates the target application and observes whether
   the expected workspace became active.
5. If activation cannot be confirmed, the client returns to ActRealm or leaves
   the item in ActRealm Workspace; the Runtime attention state is unchanged.

macOS implements the executor with AppKit and `NSWorkspace`. Windows should
implement the same policy semantics with Windows APIs and an explicit failure
result when the OS refuses foreground activation. The Runtime must not contain
either platform executor.

## Windows Runtime prerequisite

The current Runtime is not yet a Windows deliverable. Before adding the WinUI
shell, isolate the Unix-specific parts behind platform modules:

- Unix-domain socket transport in `crates/bridge/src/unix.rs`;
- Unix file modes, ownership checks, and process inspection;
- provider CLI/application discovery and install paths;
- helper lifecycle and packaging.

The loopback Axum API, snapshot model, waiter semantics, and SQLite data model
should remain behaviorally identical across platforms.

## Branch and release plan

- base Runtime integration work on `agent/v1-full` until the repository's
  unrelated `main` history is reconciled;
- use focused branches such as `agent/macos-runtime-client`,
  `agent/runtime-windows-port`, and `agent/windows-shell`;
- keep native client changes in draft PRs until automated tests and manual
  Provider acceptance are recorded;
- version the Runtime and each packaged client independently while recording a
  tested compatibility matrix.

## Delivery phases

1. Import and clean the existing macOS client without vendoring Runtime source.
2. Freeze shared schemas and sanitized snapshot fixtures.
3. Add Rust platform abstractions and Windows CI compilation.
4. Build the Windows read-only shell, then actions and reconnection.
5. Implement OS-specific foreground scheduling and parity acceptance.
6. Add signing, installers, updates, and release channels only after the local
   trust and rollback model is documented.

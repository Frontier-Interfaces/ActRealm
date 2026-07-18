# ActRealm for Windows

Status: planned. This directory intentionally contains architecture guidance
but no generated Visual Studio solution yet.

## Recommended implementation

- C#/.NET with WinUI 3 for the native shell, windows, notifications, and tray;
- a typed Runtime client for the same authenticated loopback HTTP/WebSocket API
  used by macOS;
- a Windows-only foreground scheduling executor behind a small interface;
- the Rust `flow-agent` binary packaged as a signed helper once the Runtime's
  Unix-specific bridge, file permissions, and installer paths are abstracted.

Do not share SwiftUI/WinUI views or introduce a cross-platform UI framework.
Share only the contracts and fixtures under `shared/`.

## Planned layout

```text
apps/windows/
├── ActRealm.sln
├── src/
│   ├── ActRealm.App/             # WinUI shell and lifecycle
│   ├── ActRealm.Client/          # Runtime API/WebSocket client
│   ├── ActRealm.Presentation/    # view models and derived state
│   └── ActRealm.Platform/        # tray, notifications, foreground scheduling
└── tests/
    ├── ActRealm.Client.Tests/
    └── ActRealm.Presentation.Tests/
```

## Delivery order

1. Make the Rust Runtime compile and run on Windows without changing its API
   semantics.
2. Build a read-only Windows shell against recorded snapshot fixtures.
3. Add authenticated Runtime bootstrap and WebSocket reconnection.
4. Add Provider actions and the three-second undo flow.
5. Add Windows foreground scheduling with explicit fallback when activation is
   blocked by the OS.
6. Package, sign, and run parity acceptance against the macOS client.

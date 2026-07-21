# ActRealm for macOS

Native SwiftUI/AppKit client for the local `actrealm` Runtime.

## Responsibilities

- render Attention, tasks, quota, settings, menu-bar, and HUD surfaces;
- supervise the bundled Runtime helper and connect through its authenticated
  loopback API and WebSocket;
- execute macOS-only Agent Focus through `NSWorkspace` and AppKit, including
  pointer acceptance and bounded Stage Manager state restoration;
- package one tested `actrealm` binary inside `ActRealm.app`.

The client does not read SQLite or Provider configuration directly. Hooks,
sanitization, approval state, persistence, and Provider replies remain owned by
the Rust Runtime at the repository root.

## Development

The current UI uses macOS 26 APIs and Swift tools 6.2 or newer.

```bash
swift build --package-path apps/macos
apps/macos/Scripts/test.sh
```

The helper scripts prefer the installed macOS 26 SDK because the current UI
uses that SDK's SwiftUI surface. Set `SDKROOT` explicitly to test another SDK.

When run from source, `ActRealmApp` finds the Rust workspace at the monorepo
root and builds `target/release/actrealm` if needed. A packaged app always
prefers its embedded `Contents/Helpers/actrealm` helper.

## Packaging

From the repository root:

```bash
apps/macos/Scripts/package-app.sh
```

The result is written to `apps/macos/dist/ActRealm.app`. Build output, Xcode
user state, snapshots, and packaged apps are ignored by Git.

## Source layout

- `Sources/ActRealmKit/`: models, derived state, Runtime client/supervisor, and
  platform-neutral Agent Focus policy;
- `Sources/ActRealmUI/`: SwiftUI views and the AppKit Agent Focus executor;
- `Sources/ActRealmApp/`: app, window, menu-bar, and lifecycle entry point;
- `Sources/SnapshotTool/`: deterministic UI snapshot utility;
- `Tests/ActRealmKitTests/`: model, decoding, scheduling, and bootstrap tests.

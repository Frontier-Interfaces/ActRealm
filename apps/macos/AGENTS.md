# macOS client guidance

- Treat the Rust workspace at the repository root as the only Runtime source.
  Never copy or vendor it under `apps/macos/`.
- Keep Hook installation, SQLite access, sanitization, approval ownership, and
  Provider replies in Rust. Swift communicates through the authenticated
  localhost API and WebSocket.
- Keep scheduling policy/timers in `ActRealmKit` and macOS window/app activation
  in `ActRealmUI/ForegroundSchedulingController.swift`.
- Do not infer approval capabilities from visible Provider state. Render only
  capabilities declared by the Runtime snapshot.
- Do not add developer-specific SDK, Xcode, or home-directory paths to tracked
  files. Build and package output belongs in ignored directories.
- Before committing macOS changes, run `apps/macos/Scripts/test.sh` and
  validate `Resources/Info.plist` with `plutil -lint`.

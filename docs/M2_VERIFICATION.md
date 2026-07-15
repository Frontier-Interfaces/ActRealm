# M2 verification

Status: **complete**.

Verified on 2026-07-15 (Asia/Shanghai) against the final M2 tree.

## Current-tree checks that pass

```text
cargo fmt --all -- --check
  PASS

cargo clippy --workspace --all-targets --offline -- -D warnings
  PASS (zero warnings)

cargo test --workspace --offline
  PASS (53 tests; includes authenticated HTTP, CSRF, WebSocket snapshot,
        approve, undo, deny, pass-through, ack, snooze, socket failure modes,
        and real versioned Claude/Codex widget E2E)

cargo build --workspace --release --offline
  PASS

node --check web/app.js
  PASS

./scripts/m0-e2e.sh
  PASS (release binary: Claude allow, Codex deny, pass-through, and
        missing-Runtime fail-open)
```

## Open checks

- [x] `cargo test --workspace --offline` passes on the final tree.
- [x] `scripts/m0-e2e.sh` passes against the final release binary.
- [x] M2 API and Claude/Codex widget E2E tests pass on the final tree.
- [x] The real page is inspected at exactly 1600x600 for clipping, scrolling,
      connection state, touch targets, and the high-risk confirmation path.
- [x] `docs/V1_ACCEPTANCE.md` M2 boxes are checked only after all items above.

## Visual-check note

The bundled browser-control client failed during its own module
initialization with `TypeError: Cannot redefine property: process`, before it
could connect to the page. The exact final release page was therefore opened in
the user's default browser and received explicit manual visual sign-off. The
tooling failure was not counted as a product pass or failure.

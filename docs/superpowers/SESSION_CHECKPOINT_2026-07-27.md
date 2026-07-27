# ActRealm Release Hardening Session Checkpoint

Date: 2026-07-27

Worktree: local isolated worktree

Baseline: `1dff02d879443876a1ab59aca1654ca9d084e7ed`

Rules:

- Do not commit, push, reset, clean, or install the candidate until the user explicitly authorizes it.
- Web supports English.
- Apple Silicon only; Intel support is deferred.
- Automatic updates, 48-hour soak, and accessibility work are deferred.
- Superpowers and subagents were stopped after Task 4 at the user's request. Continue directly with evidence-backed tests.

Completed:

1. Tasks 1–4: Runtime, projection, lifecycle, and localization foundations.
2. Task 5: macOS English localization and language gates.
3. Task 6: Web English localization and bootstrap-before-first-snapshot flow.
4. Task 7: persistent Stage Manager ownership, asynchronous process runner, and PID identity verification.
5. Task 8: local Web security hardening:
   - 32-byte OS-random bootstrap/session/CSRF secrets;
   - constant-time secret comparisons;
   - short-lived, one-use WebSocket tickets via `Sec-WebSocket-Protocol`;
   - no CSRF/token in the WebSocket URL;
   - exact Origin/Cookie/ticket checks;
   - centralized CSP and security response headers;
   - raw internal error details removed from HTTP responses.

Latest successful Task 8 gates:

- `node --check web/app.js`
- `node --test web/i18n.test.js` — 5 passed
- `cargo test -p actrealm-server --offline` — 27 unit, 6 API, 3 performance passed; 2 manual previews ignored
- `cargo clippy -p actrealm-server --all-targets --offline -- -D warnings`
- `git diff --check`

Completed after the original checkpoint:

9. Task 9: explicit backup governance and reproducible CI:
   - source-aware private backup identity and explicit safe deletion;
   - backup count/size plus separate destructive confirmation in macOS and Web;
   - immutable Action SHAs, Rust 1.97, Xcode 26.6, and cargo-audit 0.22.2;
   - `scripts/check-ci-pins.sh`;
   - raw tracked evidence removed from the current tree and replaced by a sanitized text index.

Latest successful Task 9 gates:

- `cargo test -p actrealm-installer --offline` — 16 installer and 4 statusline tests passed.
- `cargo test -p actrealm-server --offline` — 27 unit, 7 API, and 3 performance tests passed; 2 manual previews ignored.
- `./scripts/check-ci-pins.sh` — passed.
- `./scripts/check-actrealm-language.sh` — passed.
- `apps/macos/Scripts/test.sh` — 23 suites and 130 tests passed after the
  native WebSocket repair.
- `git diff --check` — passed.

Current position:

- Task 10 documentation, full common gates, performance/security checks, final
  whole-branch review, and one focused fix wave are complete.
- Final review fixed the Claude Task fallback so every bounded factual Task is
  returned when no Connector plan exists.
- Post-fix verification: 236 Rust tests passed (3 explicit ignores), 5 Web
  tests passed, 23 macOS suites/130 tests passed, Hook p95 was 3.146 ms,
  Runtime-to-WebSocket p95 was 110.409 ms, and the two-minute resource gate
  measured 0.000% average idle CPU with 6,960 KiB maximum Runtime RSS.
- Package/Helper/signature verification and local candidate installation
  completed after explicit authorization without replacing `~/.actrealm`.
- Installed-candidate testing found and fixed a native WebSocket contract
  mismatch: macOS now requests a one-use `/api/v1/ws-ticket` and sends
  `actrealm.<ticket>` through `Sec-WebSocket-Protocol`, with no credential in
  the URL.
- The installed candidate reports `Runtime · 本机在线`; Doctor passes the
  control loop, Provider event, trust, and fail-open checks.
- The user accepted the real Claude/Codex workflows on 2026-07-27 and
  authorized commit/push to `agent/runtime-client-localization-hardening`.
- Merge, tag, signing/notarization, and public release remain separately gated.

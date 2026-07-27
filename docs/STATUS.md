# ActRealm current status

Last reviewed: 2026-07-27

Source baseline: `1dff02d879443876a1ab59aca1654ca9d084e7ed`

Candidate state: the exact ad-hoc-signed Apple Silicon candidate is installed
locally, Doctor passes, and the user accepted it on 2026-07-27. Delivery is on
`agent/runtime-client-localization-hardening`; it has not been merged, tagged,
signed for public distribution, or released.

This is the short current source of truth. Historical milestone detail remains
in `V1_ACCEPTANCE.md` and the milestone verification records.

## Supported candidate scope

- Apple Silicon only.
- macOS 26; native CI uses Xcode 26.6.
- Rust 1.97.
- Local Claude Code and Codex sessions through installed Provider Hooks.
- Direct actions only when ActRealm owns a live official reply channel.
- Native macOS and embedded Web clients.
- System/Simplified Chinese/English presentation. Provider-authored and
  user-authored text remains verbatim.
- Loopback HTTP/WebSocket and current-user Unix sockets only.
- Local SQLite persistence, bounded retention, explicit export, diagnostics,
  and source-aware backups.

The candidate does not claim Intel support, automatic updates, a completed
48-hour soak, accessibility qualification, Windows support, Gemini support, or
a publicly signed installer.

## Release-hardening progress

The 2026-07-27 plan contains ten tasks.

| Task | Result | Verified outcome |
| --- | --- | --- |
| 1. Session truth and recovery | Complete | Execution state and recovery/control capability no longer contradict each other; stale local ownership is normalized |
| 2. Bounded snapshots and retention | Complete | UI snapshots filter at SQL level, related rows are batched, expired closed graphs are pruned transactionally, actionable work is preserved |
| 3. Bounded usage discovery | Complete | Directory traversal and oversized first reads are bounded and report partial/unavailable truth instead of false totals |
| 4. Incremental native projection | Complete | Stable per-session facts prevent quota/metric/clock changes from rebuilding unchanged task cards |
| 5. macOS English localization | Complete | ActRealm-owned native copy uses stable localized keys; Provider/user text remains unchanged |
| 6. Web English localization | Complete | Web supports System, Simplified Chinese, and English without mutating Runtime settings |
| 7. Stage Manager and process safety | Complete | Ownership survives relaunch, process execution is asynchronous, and PID reuse is identity-checked |
| 8. Local Web security | Complete | CSPRNG secrets, constant-time comparison, one-use WebSocket tickets, strict Origin/Cookie/CSRF checks, CSP/security headers |
| 9. Backup governance and reproducible CI | Complete | Private source-aware backups, explicit deletion, immutable Action SHAs, pinned tools, raw evidence removed from the current tree |
| 10. Documentation, full verification, installation | Complete | Documentation, full gates, final review, package/signature checks, local installation, Doctor, and user acceptance passed |

## Current verified gates

Task 9 completed with:

- installer: 16 integration tests and 4 statusline tests passed;
- server: 27 unit, 7 API, and 3 performance tests passed; 2 manual previews
  remained intentionally ignored;
- macOS: 23 suites and 130 tests passed;
- CI immutability, language contracts, and `git diff --check`: passed.

These are scoped Task 9 results, not the final Task 10 release result. The final
whole-workspace counts and performance/security checks will be recorded in
`reports/ACTREALM_RELEASE_HARDENING_2026-07-27.md`.

## Product truth

### Provider control

- External Hook approval is request-keyed and supports allow, deny, or
  pass-through.
- Claude `AskUserQuestion` and `Elicitation` can be answered only while their
  official blocking Hook waiter is alive. Answers remain memory-only.
- Codex direct question/approval actions require an explicitly attached,
  version-gated app-server connection and a matching live request.
- Provider-native `request_permissions` / `waitingOnApproval` is observation
  only. ActRealm opens the Provider interface; it does not invent allow/deny
  controls or infer the result.
- Restart never restores an old Hook stdout/RPC waiter. Durable history may be
  shown, while control returns only after a new verified Provider event or a
  managed Thread reconnection.

### Data and retention

- Raw prompts, complete commands, tool input/output, transcripts, file contents,
  tokens, and complete local paths are not persisted by default.
- UI snapshots include only recent or actionable sessions; full export remains
  a separate path.
- Client retention choices are 30, 90, 180 days, or forever. Closed expired
  session graphs are removed transactionally; actionable attention and live
  state are preserved.
- Provider configuration backups are source-aware, private (`0700` directory,
  `0600` files), and never deleted automatically.
- Settings shows backup count/size. Deletion is a separate operation requiring
  exact `DELETE BACKUPS`; unsafe or unknown entries cause refusal.

### Security

- The Web UI is embedded in the Runtime and served on a random loopback port.
- Session and CSRF credentials are 32-byte OS-random secrets and are never put
  in the WebSocket URL.
- WebSocket access uses a short-lived single-use ticket sent as a subprotocol.
- Web and native clients use authenticated Runtime APIs and never open SQLite
  directly.
- No telemetry, cloud SDK, CDN, or outbound update check is present.

## Remaining release work

1. Request separate authorization for merge, tag, public signing/notarization,
   and release.

## Release decision

- Development testing: allowed.
- Exact local candidate acceptance: passed on 2026-07-27.
- Commit/push to `agent/runtime-client-localization-hardening`: authorized.
- Public v1 release: not declared.

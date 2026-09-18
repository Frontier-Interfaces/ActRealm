# Provider interaction follow-up — build 127

## Defect and scope

An ordinary `grok` terminal session invoked `ask_user_question`, but ActRealm
only received passive Hooks. Build 126's real question tests had used its
separate ACP launcher and did not establish parity for this ordinary entry point.
The user's report was valid. This update changes ActRealm and its Runtime only.

## Implemented behavior

- Grok setup enables `[cli] use_leader = true` through the official configuration
  file, backs up existing content, and remembers the previous preference.
  Uninstall restores only an unchanged ActRealm-owned preference. Unrelated
  model, update and Hook settings remain intact.
- Runtime discovers the current user's official local leader socket, negotiates
  protocol 1, and subscribes only to sessions that leader identifies as resident.
  It does not load dormant transcripts into another agent process.
- Ordinary Grok sessions retain their original UI. A live question or permission
  request can be answered through ActRealm; the original interface receives its
  resolution. An original-interface answer expires the matching ActRealm request.
- Historical replay is suppressed. Request deduplication, bounded frames,
  partial-frame reads, reconnects, timeouts and per-request resolution are covered.
  Losing the subscriber removes its controls without cancelling the original
  modal or marking the provider session complete.
- Grok plan review uses its own approved/cancelled result. Full plan text is
  exposed only while the native owner has a live request, and is excluded from
  exports and Companion projections. MCP form elicitation retains its native
  accept/decline/cancel response shape. URL-only requests stay in the provider UI.
- Question Hooks also create observed attention without inventing a reply
  channel. A matching live request replaces that observation; a matching tool
  result clears it.
- Kimi desktop/web sessions are discovered from owned live instance records.
  The existing bearer token is used only against loopback, with proxies and
  redirects disabled. Busy-session pending interaction endpoints are read;
  answering, dismissing or approving requires an explicit user action. One-time
  approval never becomes session-wide permission. Original-client resolution and
  transport loss remove stale controls.

## Actual validation

With official Grok Build 1.0.34, an original client and ActRealm subscribed to
the same real backend session:

1. Grok asked for Blue or Orange. ActRealm received and answered Blue. The
   original client received the resolution, and Grok wrote Blue to the test file.
2. A second test answered through the original client. ActRealm's corresponding
   question closed and the provider wrote the expected file.
3. After installing build 127 and updating Grok setup through native Agent Setup,
   the actual ActRealm window displayed the question
   `ActRealm 同会话验收：选择标记`. Selecting 通过 and sending the answer caused
   Grok to continue and create a file containing `通过\n`. The original client
   received the resolution. This was a real provider request, not injected UI.

Kimi's running desktop server reported version 0.43.1 and backend v2. Its live
OpenAPI schema was checked against the adapter. Authenticated metadata and
busy-session reads succeeded. A local HTTP integration test verifies the
question-to-waiter-to-answer route, including original option IDs, multi-select,
free text and no session-wide approval. No new Kimi model prompt was sent.

## Gates and artifacts

- Rust workspace: 433 passed, zero failed, three ignored.
- UTC macOS suite: 213 tests in 31 suites passed.
- Formatting, offline all-target Clippy with warnings denied, workspace release
  build, Info.plist, diff whitespace and language contracts passed.
- Language validation: 64 messages, 81 API errors, 61 emitted codes and 1,898
  English entries.
- Installed app and bundled/shared Runtime hashes matched the signed package;
  deep strict signatures verified.
- Build 127 app executable SHA-256:
  `11c6a1bfa9787bb0d449faeb9daad17d4b27911215e272746927e602331331a0`.
- Bundled and shared Runtime SHA-256:
  `1c26f8f52421b40a6a84327044798ebef08faf398c036eb6c9341b20b3b018f5`.
- Display 0.39.4 build 86 remained unchanged. No commits or pushes were made.

The local evidence folder `outputs/provider-parity-20260918` contains sanitized
interaction results, test logs, installation hashes and recovery journals.

## Remaining boundaries

Already-running non-shared Grok terminals cannot acquire a new transport in
place: exit and resume the existing session once. Future ordinary `grok`
launches use the configured shared connection. Explicit `--no-leader` opts out.
No existing user terminal was stopped or replaced by this update.

Standalone Kimi CLI sessions still need the explicit ACP launcher for direct
answers. Desktop/web integration does not establish equivalence for that entry
point. Real Kimi model acceptance remains deferred after the user's quota issue.
Thus this report does not claim that every entry point of every provider has
identical verified capability. Grok plan/form handling and Kimi reply formats
have protocol tests; only the real workflows listed above have live acceptance.

## Primary interface references

- [Grok Hooks](https://docs.x.ai/build/features/hooks)
- [Grok ACP](https://docs.x.ai/build/cli/headless-scripting)
- [Grok leader routing source](https://github.com/xai-org/grok-build/blob/482711333c7195dc16a272777f86086d615e2afb/crates/codegen/xai-grok-shell/src/leader/server.rs)
- [Kimi server API](https://moonshotai.github.io/kimi-code/en/reference/server-api.html)

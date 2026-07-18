# v1.1 functional-correction verification

Status: M6-M9 and M10-M12 were completed, accepted, committed, and pushed.
M13 implementation and automated/local gates also passed and were pushed as a
test candidate at the user's explicit direction, but its final real-Provider
manual acceptance remains pending. The 48-hour final release soak is separate
and still incomplete.

## User contract

The main product surface must obey these visible rules:

1. Quota renders every structurally valid Provider window and preserves the
   last valid sample instead of hiding it after 30 minutes.
2. Agent task cards use the verified Provider conversation title as the main
   title, the bounded current question as plain second-line content, and only
   the current model as the third line.
3. Agent rows show factual live thinking/tool/waiting time when events exist,
   and an honest lower-resolution state when they do not.
4. The main Agent list contains active sessions, sessions with attention, and
   sessions seen in the last 30 minutes only.
5. Selecting an attention item reveals and selects the corresponding Agent
   session on the main surface.
6. Existing Claude status-line behavior is preserved while quota capture is
   explicitly enabled.
7. Claude and Codex use recognizable image icons throughout the UI instead of
   letter monograms.
8. A user with only Claude.app and ChatGPT/Codex.app can install and verify
   hooks without installing global `claude` or `codex` commands.
9. Provider-side approval, denial, or turn completion resolves matching
   attention and removes stale `等你` state.
10. Jump labels describe only real capability: exact conversation, matching
    terminal, application only, or unsupported.
11. Provider conversation title and current task are separate: the verified
    local Claude/Codex conversation title is the visible main title, while the
    bounded current prompt remains visible without a synthetic label.
12. Users choose a concise, detailed, or developer task-card profile from a
    safe field allowlist; raw Hook payload is never directly rendered.
13. Claude AskUserQuestion/Elicitation and managed Codex requestUserInput can be
    answered in Attention; secrets remain memory-only.
14. Restart recovery distinguishes managed/controllable, external/observing,
    waiting-for-event, lost-control, and ended without restoring old waiters.
15. Provider-native approval and ActRealm-controlled approval are separate:
    native-only waiting has no allow/deny controls, follows explicit Provider
    resolution, and never fabricates the outcome.

## Reference decisions

The correction re-reviewed Open Vibe Island at revision
`6e5e7a6a5b5097ee627a7d4dea6226c128747a71` and CodeIsland at revision
`3e2aec7fa87c56b0f5129d7ba11d0dc3699dd500`. They are GPL-3.0 architecture
and failure-mode references; no source was copied.

Adopted lessons:

- Open Vibe Island's later wrapper mode demonstrated that a custom Claude
  status line can remain visible while a managed wrapper reads the same stdin
  for quota capture. ActRealm uses its own installer, backup, locking,
  delegate, stable-binary, and uninstall implementation.
- Open Vibe Island's bounded Codex rollout scanning and CodeIsland's local
  activity/session modeling reinforced the existing local-only, event-derived
  design. ActRealm keeps a strict schema allowlist and does not read response
  text to construct quota.
- CodeIsland's local title-store design and Open Vibe Island's Codex App Server
  thread metadata confirmed that client titles are separate from prompts.
  ActRealm uses the lower-risk local index/Hook path instead of starting a second
  Provider subprocess: official Claude `session_title`, bounded Claude
  custom/AI title records, and Codex's bounded `session_index.jsonl` tail.
- Both references treat missing or stale local data as unavailable instead of
  inventing a percentage or a running state. This remains a hard UI rule.
- CodeIsland's MIT Provider icon assets are used only after optimization and
  attribution. Open Vibe Island's GPL code and assets remain reference-only.

## Implementation boundary

### Runtime and session list

- SQLite schema version 6 adds bounded task usage, private jump locators,
  Provider process liveness, and
  separate Provider title/source fields.
  Existing databases migrate in place without losing sessions.
- `UserPromptSubmit` stores only a normalized, single-line, maximum-64-character
  task summary. The full prompt and text after the bounded summary are not
  persisted in the session snapshot or event payload.
- Snapshots expose current-turn start/end, real usage when supplied, and a
  four-level jump descriptor. Private window locators are excluded from JSON.
- Pending attention therefore cannot disappear just because its session is
  old. Ordinary completed history no longer occupies the live list forever.
- The title resolver stores only a normalized title and source. Claude
  transcript reads are limited to 64 KiB at each edge; Codex index reads are
  limited to a 2 MiB tail. Raw transcript text and its path are never exposed
  in the snapshot. Recent titles refresh at most once every two seconds, and an
  official Claude session title cannot be downgraded by an older AI title.

### M10-M12 capability boundary

- M10 persists only display profile and allowlisted field identifiers. The
  snapshot already contains sanitized structured facts; there is no raw Hook
  payload display or export path.
- Claude direct answers use official blocking Hook output. Secret content is
  validated and forwarded from memory, then released with the waiter.
- Codex direct answers exist only after explicit app-server attach. The
  Connector initializes an official persistent private Unix-Socket app-server
  plus proxy transport with experimental API capability, resumes saved Thread IDs, and handles
  `item/tool/requestUserInput`. Hook-only Codex stays observe/approval-only.
- Runtime restart restores SQLite session presentation and reconnects saved
  managed Threads, but every old permission/question waiter expires first.
  A fresh Provider request is required before the UI becomes actionable again.

### M13 Provider-state boundary

- Codex auto-review/guardian ownership cannot create a competing ActRealm
  reply waiter.
- Native `request_permissions` and managed `waitingOnApproval` produce an
  observation-only item without a replyable request ID.
- Only an explicit matching Provider lifecycle/status transition clears the
  native wait. Ordinary tool/running updates cannot overwrite it.
- Native UI offers original-Agent handling, snooze, and ignore only. It does
  not claim approve, deny, or execution.
- The M13 exact evidence and still-pending manual acceptance are recorded in
  `M13_PROVIDER_STATE_COORDINATION.md`.

### UI linkage and activity

- `session.providerTitle` is the verified local main title when present, while
  `session.title` is the plain bounded current-question line. The following
  line contains only `session.model`; project, Provider name, title provenance,
  and Token usage are not rendered in the card's title stack.
- A selected attention item selects the matching session, sorts it first,
  highlights it, and scrolls/focuses it into view.
- A one-second text-only tick uses current-turn total time as the primary timer
  and current-phase time as secondary context.
- First-run UI installation enables Codex tool-level activity. Command-line
  installation remains low-noise by default and has an explicit enhanced flag.

### Quota and custom status line

- Backend and frontend render all structurally valid windows and their actual
  durations/names. No account is forced into a “week” label.
- Codex scans bounded rollout tails and validates values rather than patch
  version; primary and secondary windows are both retained.
- Samples older than 30 minutes keep their real percentage/reset time and are
  marked as the last valid value.
- Claude normally retains the five-minute polling budget, but a cache file
  appearing or changing invalidates an earlier unavailable snapshot
  immediately. The UI therefore does not wait five minutes after the first
  post-bridge Claude response.
- The default Claude bridge install still rejects a custom `statusLine` without
  mutation. Explicit wrapper mode saves the complete original object, runs the
  managed capture silently, delegates the same stdin to the original command,
  leaves original stdout visible, and restores the original object on
  uninstall.

### Desktop-only Provider discovery

- Provider availability is no longer equivalent to a same-name executable in
  `PATH`. The installer recognizes Claude.app and ChatGPT/Codex.app in the
  standard system and per-user macOS application locations.
- Claude Desktop can install the shared user-level Hook configuration without
  launching its GUI executable for a version check.
- Codex Desktop exposes its bundled official `codex` executable as the manual
  `/hooks` review command. ActRealm still never writes or bypasses trust.
- Setup JSON and the first-run UI distinguish desktop-app detection from a
  global CLI and explain that the latter is not required.
- The Claude status-line quota bridge remains CLI-only because Claude Desktop
  does not render that terminal status line. Missing this optional adapter does
  not block Hook installation or Agent control.

## Exact-tree automated evidence

| Gate | Result |
| --- | --- |
| `cargo test -p actrealm-runtime --test m1_runtime --offline` | PASS, 25 tests including title provenance, migration, restart, usage, and jump privacy |
| `cargo test -p actrealm-quota --lib --offline` | PASS, 7 tests including dynamic windows and stale-value retention |
| `cargo test -p actrealm-installer --test m4_statusline --offline` | PASS, 4 tests including wrapper/restore |
| `cargo test -p actrealm-server --lib --offline` | PASS, 11 tests including safe display fields, interactive questions, recovery truthfulness, jump targets, and immediate Claude cache refresh |
| `cargo test -p actrealm-server --test m2_api --offline` | PASS, 3 authenticated API cases |
| `cargo test -p actrealm --test m2_widget_e2e --offline` | PASS, 5 Claude + 5 Codex control flows |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | PASS, zero warnings |
| `cargo test --workspace --offline` | PASS, 140 tests, two explicit resource/manual-preview tests ignored by default, and all doc-tests |
| `cargo build --workspace --release --offline` | PASS |
| `node --check web/app.js` | PASS |
| event-to-WebSocket performance | PASS, below 300 ms budget |
| five-round Claude + Codex widget control replay | PASS after isolating its Runtime/home from the user's live instance |
| explicit 120-second release resource gate | PASS, 117 samples, CPU 0.000%, RSS max 5,792 KiB |
| install exact release + Doctor on user home | PASS; M10-M12 candidate installed with matching SHA-256; Doctor overall PASS; Claude/Codex app and Terminal sources all emitted post-install events; user accepted the candidate |
| user release decision | Commit and GitHub push authorized on 2026-07-17 |

The resource report from this exact candidate tree is:

```json
{"schemaVersion":1,"durationSeconds":120,"sampleCount":117,"idleCpuAveragePct":0.000,"runtimeRssMaxKiB":5792,"cpuBudgetPct":0.5,"rssBudgetKiB":81920}
```

## Remaining long-run release evidence

- record M13's real-Provider waiting/resolution manual acceptance;
- keep the final 48-hour release soak unchecked until it actually passes.

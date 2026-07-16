# v1.1 functional-correction verification

Status: eligible for the requested user-test branch push. The final 115-test
exact-tree gate and user-amended 2-minute resource gate passed on 2026-07-16;
the verified binary was installed locally, all 14 doctor checks passed, three
real quota snapshots were persisted, and the user accepted the current
functionality and visuals. This record does not satisfy or replace the separate
48-hour final release soak.

## User contract

The main product surface must obey these visible rules:

1. Quota has exactly three independent slots: Claude 5h, Claude 7d, and Codex
   week.
2. Agent task titles describe the current task, not the user, Provider, or
   project.
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

## Reference decisions

The correction re-reviewed Open Vibe Island at revision
`6e5e7a6a5b5097ee627a7d4dea6226c128747a71` and CodeIsland at revision
`3e2aec7fa87c56b0f5129d7ba11d0dc3699dd500`. They are GPL-3.0 architecture
and failure-mode references; no source was copied.

Adopted lessons:

- Open Vibe Island's later wrapper mode demonstrated that a custom Claude
  status line can remain visible while a managed wrapper reads the same stdin
  for quota capture. Flow Agent uses its own installer, backup, locking,
  delegate, stable-binary, and uninstall implementation.
- Open Vibe Island's bounded Codex rollout scanning and CodeIsland's local
  activity/session modeling reinforced the existing local-only, event-derived
  design. Flow Agent keeps a strict schema allowlist and does not read response
  text to construct quota.
- Both references treat missing or stale local data as unavailable instead of
  inventing a percentage or a running state. This remains a hard UI rule.
- CodeIsland's MIT Provider icon assets are used only after optimization and
  attribution. Open Vibe Island's GPL code and assets remain reference-only.

## Implementation boundary

### Runtime and session list

- SQLite schema version 2 adds a nullable session title. Existing version-1
  databases migrate in place without losing sessions.
- `UserPromptSubmit` stores only a normalized, single-line, maximum-64-character
  task summary. The full prompt and text after the bounded summary are not
  persisted in the session snapshot or event payload.
- Snapshots expose `activitySince`. The server reconciles liveness every 30
  seconds and filters the main list to activity within 30 minutes, unless a
  session still owns an open, committing, sent, or snoozed attention item.
- Pending attention therefore cannot disappear just because its session is
  old. Ordinary completed history no longer occupies the live list forever.

### UI linkage and activity

- The task title uses `session.title`; Provider and project remain secondary
  metadata.
- A selected attention item selects the matching session, sorts it first,
  highlights it, and scrolls/focuses it into view.
- A one-second text-only tick updates elapsed activity without rebuilding the
  row DOM. The visible state distinguishes thinking, current tool, waiting,
  completed, failed, and idle facts.
- First-run UI installation enables Codex tool-level activity. Command-line
  installation remains low-noise by default and has an explicit enhanced flag.

### Quota and custom status line

- Backend and frontend both materialize Claude 5h, Claude 7d, and Codex week as
  separate slots. A missing source or window affects only that slot.
- Codex accepts only sanitized, fixture-tested rollout families 0.144.2
  (desktop), 0.144.4, and 0.144.5. Only the 10080-minute window maps to Codex
  week.
- Claude normally retains the five-minute polling budget, but a cache file
  appearing or changing invalidates an earlier unavailable snapshot
  immediately. The UI therefore does not wait five minutes after the first
  post-bridge Claude response.
- The default Claude bridge install still rejects a custom `statusLine` without
  mutation. Explicit wrapper mode saves the complete original object, runs the
  managed capture silently, delegates the same stdin to the original command,
  leaves original stdout visible, and restores the original object on
  uninstall.

## Exact-tree automated evidence

| Gate | Result |
| --- | --- |
| `cargo test -p flow-agent-runtime --test m1_runtime --offline` | PASS, 19 tests including migration/title privacy |
| `cargo test -p flow-agent-quota --lib --offline` | PASS, 7 tests including 0.144.2 desktop, 0.144.5, and fixed slots |
| `cargo test -p flow-agent-installer --test m4_statusline --offline` | PASS, 4 tests including wrapper/restore |
| `cargo test -p flow-agent-server --lib --offline` | PASS, 7 tests including immediate Claude cache refresh |
| `cargo test -p flow-agent-server --test m2_api --offline` | PASS, 3 authenticated API cases |
| `cargo test -p flow-agent --test m2_widget_e2e --offline` | PASS |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | PASS, zero warnings |
| `cargo test --workspace --offline -q` | PASS, 115 tests and all doc-tests |
| `cargo build --workspace --release --offline` | PASS |
| `node --check web/app.js` | PASS |
| `./scripts/m0-e2e.sh` | PASS, Claude allow, Codex deny, pass-through, missing-Runtime fail-open |
| event-to-WebSocket performance | PASS, p95 104.483 ms, budget 300 ms |
| final 120-second release resource gate | PASS, 118 samples, CPU 0.000%, RSS max 5,440 KiB |

The in-app browser integration failed before navigation with the external
error `Cannot redefine property: process`. Per its control contract, no
standalone browser automation was substituted. The user instead tested the
installed exact release page and accepted its current functionality and
visuals on 2026-07-16.

The resource report from the exact release binary was:

```json
{"schemaVersion":1,"durationSeconds":120,"sampleCount":118,"idleCpuAveragePct":0.000,"runtimeRssMaxKiB":5440,"cpuBudgetPct":0.5,"rssBudgetKiB":81920}
```

## Remaining release gate

- keep the final 48-hour release soak unchecked until it actually passes; this
  branch push is a user-test candidate, not a final v1 release.

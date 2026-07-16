# M5 verification record

Status: user-test release candidate verified. On 2026-07-16 the user
explicitly changed the immediate pre-local-test soak from 48 hours to 30
minutes and requested a commit and branch push after it passes. The 30-minute
gate passed with 60 samples. That push is a
user-test candidate only: the unchecked 48-hour gate still blocks declaring M5
complete, creating a final v1 tag, or publishing a final v1 release.

## Implemented release evidence

### Local metrics

`metrics_daily` stores aggregate counters only:

- approval requests are counted once when a new permission request is stored;
- sessions are counted once per unique session;
- widget allow/deny is counted only after the final decision is sent;
- undone decisions are not counted;
- manual and deadline pass-through are separate counters;
- decision-response totals and counts support an aggregate average;
- notification banners and application opens are counted only when the
  authenticated local UI reports the action.

`flow-agent export-metrics` and `/api/v1/metrics/export` produce a separate,
aggregate-only JSON file. It contains the schema version, export time,
definitions, and daily counters. It contains no session, event, attention,
command, project, or path data. The existing full local backup remains a
separate `flow-agent export` operation.

### Diagnostic capture and redaction

Diagnostic capture is disabled by default and can be enabled for 1 to 60
minutes. It records only a fixed event name, provider, capture time,
`needsReply`, and payload byte count. It never records session identifiers,
paths, prompts, commands, arguments, URLs, or tokens.

The diagnostic directory is private, diagnostic files are private, symbolic
links are refused, each file is capped at 1 MiB, and expired data is removed
even if no provider event arrives. Diagnostic failures never block an agent.
Normal Runtime logs contain no raw Hook payload and no raw session identifier.
Persisted command previews are category labels only.

The bridge independently limits complete frames to 320 KiB and uses bounded
reads. The HTTP API limits request bodies to 64 KiB. Existing host, origin,
CSRF, authentication, socket-permission, oversized/deep JSON, and redaction
tests remain part of the release gate.

### Provider compatibility and pass-through

Fixture and local compatibility versions:

- Claude Code fixture 2.1.210; local installation validation 2.1.211 with a
  real post-install event;
- Codex CLI fixture 0.144.4; final local installation validation 0.144.5 with
  a real post-install event and every trust check passing.

The read-only doctor check reports the local CLI checks as passing and Codex
canonical Hooks as enabled. It warns, correctly, when the user's real Hook
configuration and Runtime are not installed; verification did not alter the
user's real provider configuration or start a paid provider session.

The later user-authorized local installation validated real post-install events
from Claude Code 2.1.211 and Codex CLI 0.144.5. That installation exposed a
false doctor warning when Claude's healthy `--version` command took about three
seconds. Provider-version discovery now allows five seconds, while the
independent fail-open safety probe remains capped at two seconds. A regression
test covers a healthy three-second provider version command.

Automated pass-through coverage includes absent Runtime, explicit
pass-through, mismatched request ID, malformed response, end-of-file, and
deadline expiry. Every case exits successfully with empty stdout and stderr.

## Performance results

| Measurement | Result | Budget | Status |
| --- | ---: | ---: | --- |
| Non-blocking Hook p95, 40 measured samples | 3.164 ms | < 50 ms | PASS |
| Event to WebSocket/render entry, 20 samples | 104.811 ms | < 300 ms | PASS |
| Browser completed-render p95 | user-verified within budget | < 300 ms | PASS |
| Idle Runtime CPU average, release widget mode | 0.000% | < 0.5% | PASS |
| Runtime RSS maximum, 15-second release check | 5,408 KiB | < 80 MiB | PASS |
| Browser tab memory | user-verified within budget | < 150 MiB | PASS |
| Pre-user-test continuous soak | CPU 0.000%, RSS max 5,424 KiB, 60 samples | 30 minutes, CPU < 0.5%, RSS < 80 MiB | PASS |
| Final release continuous soak | deferred | 48 hours, RSS < 80 MiB | PENDING |

The UI exposes its measured completed-render p95 in the settings performance
row and in `document.body.dataset.eventUiP95Ms`.

The in-app browser controller could not reach the page because its environment
failed before navigation with `Cannot redefine property: process`. Per the
browser-control contract, no unapproved standalone browser automation was used.
The two browser measurements above were performed by the user against the
isolated release-candidate Chrome instance. Exact numeric readings were not
recorded, so this evidence states only the verified threshold result.

The short resource report was generated from the final release binary in
widget mode:

```json
{"schemaVersion":1,"durationSeconds":15,"sampleCount":15,"idleCpuAveragePct":0.000,"runtimeRssMaxKiB":5408,"cpuBudgetPct":0.5,"rssBudgetKiB":81920}
```

The user-directed continuous pre-local-test report used the same release
binary and widget mode:

```json
{"schemaVersion":1,"durationSeconds":1800,"sampleCount":60,"idleCpuAveragePct":0.000,"runtimeRssMaxKiB":5424,"cpuBudgetPct":0.5,"rssBudgetKiB":81920}
```

## Frozen-candidate gate results

The following commands passed on the candidate that is eligible to enter the
48-hour soak:

```text
cargo fmt --all -- --check
  PASS
cargo clippy --workspace --all-targets --offline -- -D warnings
  PASS, zero warnings
cargo test --workspace --offline
  PASS, 108 tests and all doc-tests; zero failures
cargo build --workspace --release --offline
  PASS
node --check web/app.js
  PASS
./scripts/m0-e2e.sh
  PASS: Claude allow, Codex deny, pass-through, missing-Runtime fail-open
cargo test -p flow-agent --test m5_performance --offline -- --nocapture
  PASS: nonblocking_hook_p95_ms=3.164
cargo test -p flow-agent-server --test m5_performance --offline -- --nocapture
  PASS: event_to_websocket_p95_ms=104.811
FLOW_AGENT_RESOURCE_DURATION_SECONDS=15 \
  ./scripts/m5-resource-check.sh target/release/flow-agent
  PASS: CPU average 0.000%, RSS maximum 5,408 KiB
git diff --check
  PASS
```

The sensitive-data scan found no real credential, private key, bootstrap token,
local browser-test path, maintainer email, or user home path in the candidate.
The security tests intentionally contain fixed fake secret canaries and verify
that they are absent from persisted data and normal logs.

The user-directed 30-minute soak passed without a Runtime exit or budget
violation. Runtime source and the release binary remained unchanged during the
soak; the Chinese user guide and its surface-support clarification were added
while it ran and received the final documentation/static checks. This candidate
is eligible for the requested user-test branch commit and push. The final
48-hour gate remains unchecked and must pass before M5 or v1 can be represented
as complete.

## Post-candidate functional correction

After the frozen candidate, the user required current-task titles, factual live
activity, a 30-minute main-list window, attention-to-session selection, exactly
three quota slots, custom Claude status-line preservation, and Codex 0.144.5
quota compatibility. These are bounded product corrections rather than a
claim that the final 48-hour release gate passed.

A subsequent local check found the active Codex desktop rollout reports
`0.144.2` even though the installed CLI reports `0.144.5`, and found that a
Claude cache created just after bridge setup could remain behind the normal
five-minute poll. A sanitized desktop fixture and an immediate cache-mtime
invalidation regression close both gaps without widening the schema parser.

The correction implementation passed 113 workspace tests before the final
documentation/static rerun. Its exact implementation and gate evidence is
recorded in `V1_1_FUNCTIONAL_CORRECTIONS.md`. Because Runtime, installer,
quota, server, and web code changed after the prior 30-minute candidate soak,
a new pre-local-test resource gate had to run on the rebuilt binary before this
correction could be committed and pushed. The user amended this correction
gate to 2 minutes on 2026-07-16; it passed with 118 samples, CPU average
0.000%, and Runtime RSS maximum 5,504 KiB. After the live quota compatibility
fix, the exact 115-test tree repeated that gate and passed with 118 samples,
CPU average 0.000%, and Runtime RSS maximum 5,440 KiB. Local replacement and
real Provider/quota checks then passed: all 14 doctor checks were green and the
three fixed quota slots were persisted from real local sources. The user
accepted the current functionality and visuals on 2026-07-16, making this
correction eligible for the requested user-test branch push. The 48-hour final
release soak remains pending afterward.

# M3 verification record

Status: accepted on 2026-07-15 after the automated gate and human visual
review of the exact first-run screen at the 1600x600 target size.

M3 delivers the Claude/Codex installer, first-run onboarding, Codex trust
guidance, structured doctor report, post-install real-event verification, and
fact-based Agent activity required by `WIDGET_V1_PLAN.md` v1.1. Tests use
isolated `HOME`, `CODEX_HOME`, and `ACTREALM_HOME` roots. They did not install
or remove entries in the developer's real provider configuration.

## Current provider contract review

- Claude Code `2.1.210` was checked against the current
  [Claude Hooks reference](https://code.claude.com/docs/en/hooks). User hooks
  belong in `~/.claude/settings.json`; `PermissionRequest` has no
  `tool_use_id`; `TaskCreated` and `TaskCompleted` carry stable `task_id`
  values; `Stop` may report `background_tasks` and `session_crons`.
- Codex CLI `0.144.4` was checked against the current OpenAI Codex Hooks manual
  and the official
  [Codex configuration schema](https://github.com/openai/codex/blob/main/codex-rs/core/config.schema.json).
  User `hooks.json` and same-layer inline hooks merge, unmanaged command hooks
  require exact-definition trust, Hooks default to enabled, `hooks` is the
  canonical feature key, and `codex_hooks` is a legacy alias.
- Open Vibe Island and CodeIsland remain architecture/failure-mode references
  only. Provider output and trust behavior follow current official contracts
  and the M0 live probes.

## Installer and onboarding evidence

- Existing JSON is parsed and semantically merged. Unknown top-level fields,
  unknown events, user matcher groups, handler fields, and file modes survive.
- Every mutation is protected by an advisory lock, same-directory temporary
  file, file sync, atomic rename, and pre-change backup. Provider config and the
  stable helper reject symbolic-link targets.
- Claude permission hooks use `86400s`; Codex permission hooks use `3600s`;
  non-permission hooks use `5s`.
- Codex defaults to `SessionStart`, `UserPromptSubmit`, `PermissionRequest`, and
  `Stop`. `PreToolUse` and `PostToolUse` require
  `--enhanced-codex-activity`.
- Install intent persists as `untouched`, `installed`, or `uninstalled`.
  Repair restores an executable only when all managed entries are intact; it
  does not recreate a manually removed or intentionally uninstalled Hook.
- The authenticated onboarding endpoint and CLI call the same installer.
  Install/uninstall mutations require the existing Origin, cookie, and CSRF
  checks. Codex trust remains a manual `/hooks` action.
- Onboarding and doctor require a provider event newer than the installed Hook
  definition before reporting that provider as connected. The doctor control
  probe is explicitly excluded and is not persisted as a provider session.

## Fact-based activity and degradation

- Claude task progress is derived only from deduplicated
  `TaskCreated`/`TaskCompleted` `task_id` values. Missing IDs leave the prior
  count unchanged; no inferred percentage is rendered.
- Subagent activity is deduplicated by `agent_id` and reports the active count.
- A Claude `Stop` carrying background work remains active and cannot create a
  false completion card.
- Codex's low-noise event set does not pretend tool-level visibility. Unknown
  events stay visible as a compatibility warning and never panic.

## Automated results

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | PASS, zero warnings |
| `cargo test --workspace --offline` | PASS, 75 tests |
| `cargo build --workspace --release --offline` | PASS |
| `node --check web/app.js` | PASS |
| `./scripts/m0-e2e.sh` against the release binary | PASS: Claude allow, Codex deny, explicit pass-through, missing-runtime fail-open |
| `m3_installer` | PASS, 13 destructive/config safety cases |
| `m3_install_cli` | PASS, 6 CLI/doctor/onboarding end-to-end cases |
| Post-install real event boundary | PASS: install → unverified → real `SessionStart` → connected → doctor evidence → uninstall semantic restore |
| Socket path preflight | PASS: overlong path blocks install and Runtime before provider config mutation |

The first full workspace run exposed one fixture-only mismatch: the official
task contract examples used a different sanitized session ID and correctly
created a third session. The fixture ID was aligned with the existing Claude
fixture set, the failed test passed in isolation, and the entire 75-test suite
then passed from the updated tree.

The final gate also exposed test-only timing noise: parallel M3 process tests
created temporary shell executables and repeatedly copied/started the large
debug binary, which could trigger macOS executable inspection or disk pressure
past the intentional two-second Doctor deadline. The test Provider is now a
native `/bin/echo` stand-in and the process-heavy M3 integration cases are
serialized. Product deadlines and the separate Runtime concurrency tests were
not relaxed. The updated M3 test binary and full workspace suite passed.

## Visual check

The browser automation plugin failed before page initialization with
`Cannot redefine property: process`, the same external tooling fault recorded
during M2. A temporary Runtime using only `/tmp` configuration opened the exact
M3 first-run page in the default browser. Four human-captured screenshots cover
the uninstalled state, Claude installed-unverified state, Codex needs-trust
state, and mixed Provider states. The two Provider cards, safety badges, Codex
trust steps, footer controls, and close control fit the 1600x600 target without
clipping, overlap, or hidden actions. Human visual review: PASS.

## Remaining scope

This record does not claim M4 quota/settings/Gemini work or M5 performance,
48-hour soak, export, packaging, signing, and release evidence.

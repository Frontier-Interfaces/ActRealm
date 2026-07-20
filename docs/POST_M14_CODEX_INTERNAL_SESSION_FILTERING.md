# Post-M14 Codex internal-session filtering

Status: company implementation merged locally; synchronized Rust/resource
verification passes and the local merge commit is created. Latest installation,
macOS Swift rerun, and user-performed push remain pending.

Date: 2026-07-20

Source branch: `agent/v1-full`

Upstream head: `43a268d39a9805eef7872e669b3a3fdcd97e9963`

## Purpose

Codex App creates internal background sessions for overview suggestions and
safety review. They are implementation work owned by the Provider, not user
Agent tasks. Displaying them inflated task/session metrics and created ghost
rows with internal prompt-derived titles.

The synchronized implementation filters only events that satisfy all of these
facts:

- Provider is Codex;
- event is `PromptSubmitted`;
- working directory is `/`;
- term surface is `codex_app` or bundle ID is `com.openai.codex`;
- normalized prompt begins with one of the two tested internal prefixes.

A normal Codex App prompt from `/` remains visible. The filter is deliberately
not a broad title, path, or “background-looking” heuristic.

## Runtime behavior

When an internal prompt arrives after a provisional `SessionStart`, Runtime
transactionally removes its session, events, Attention/commands, tasks,
subagents, turns, usage, and the corresponding session metric. It stores only
Provider, Provider session ID, ignored time, and the fixed reason
`codex_internal_background_prompt` in `ignored_provider_sessions`.

Later lifecycle and usage for that Provider session remain suppressed across
Runtime restart. A registered Hook waiter is passed through with
`provider_internal`; a managed Connector request receives a bounded internal
session error. ActRealm never claims approval control for the suppressed work.

Database schema advances from 8 to 9. Opening an existing database scans only
Codex App root sessions with stored titles, removes tested legacy internal
matches transactionally, and preserves ordinary sessions and metrics.

## Verification

The upstream regression covers:

- provisional SessionStart removal;
- overview and safety-review matching;
- ordinary root-directory user prompt preservation;
- lifecycle suppression after reopening SQLite;
- legacy schema migration and metric correction;
- absence of a generic `sessions.visibility` column.

Run on the synchronized exact tree:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test --workspace --offline
cargo build --workspace --release --offline
./scripts/check-actrealm-language.sh
cargo test -p actrealm --test m6_m8_resource --release --offline \
  two_minute_release_candidate_resource_gate -- --ignored --nocapture
```

### Exact synchronized-tree result

- JavaScript syntax, Rust format, and the ActRealm language contract: passed.
- Focused schema-9 filtering regression: `1 passed; 0 failed`.
- Workspace Clippy with `-D warnings`: passed.
- Full Rust workspace: `177 passed; 0 failed; 3 ignored`. The ignored tests are
  explicitly manual preview/resource entries and are not silent failures.
- Workspace release build: passed.
- Explicit two-minute resource gate: passed with 118 samples, 0.000% average
  idle CPU, and 6,272 KiB maximum Runtime RSS against budgets of 0.5% and
  81,920 KiB.
- macOS Swift suite: not executed in the managed environment. SwiftPM's own
  `sandbox-exec` failed with `sandbox_apply: Operation not permitted` before
  any test assertion ran.
- Built synchronized release SHA-256:
  `d70d2ea89d6d63115e0657194128ee8e9ca78df3e2cd095fc3028605ccce935b`.
  The installed helper remains the earlier first-run candidate, so this exact
  tree is not yet installed or accepted against real Provider sessions.

The final local merge commit is reported in the handoff; the user performs the
separately gated push of `agent/v1-full`.

# Post-M14 usage, pricing, and OAuth hardening

Status: implementation, focused verification, full workspace gates, and the
two-minute release resource gate complete. Exact-candidate local installation
and user acceptance are required before commit or push.

This refinement closes four long-running correctness and resilience gaps in the
M14 telemetry path. It does not add telemetry, a remote pricing updater, or a
second owner for Provider credentials.

## Delivered scope

### Broader, source-labelled price estimates

- Claude transcript entries without an official cost now prefer a dated
  [Anthropic price table](https://platform.claude.com/docs/en/about-claude/pricing)
  for the models currently exposed by the local desktop picker: Fable 5,
  Opus 4.8/4.7/4.6, Sonnet 5/4.6, and Haiku 4.5. The picker also exposes the
  retired Opus 3 model, so its exact `claude-3-opus-20240229` identifier is
  retained as a clearly labelled historical compatibility rate. A small
  `models.dev` subset remains only where no current first-party table exists.
- `claude-sonnet-5` uses Anthropic's dated
  [introductory price](https://www.anthropic.com/news/claude-sonnet-5) plus the
  documented prompt-cache multipliers, and now carries the documented 1M
  context window instead of the old 200K fallback.
- Codex rollout entries can now be priced from the dated
  [OpenAI standard API price](https://developers.openai.com/api/docs/pricing)
  snapshot. The current desktop choices—GPT-5.6 Sol, Terra, Luna, and GPT-5.5—
  are covered. GPT-5.4 and older entries remain only for historical rollout
  files and are not described as currently selectable. The legacy
  `gpt-5.2-codex` fallback is labelled separately as a `ccusage` compatibility
  input.
- Input, cached input, cache creation, and output rates remain separate. Cached
  input is subtracted from ordinary Codex input before pricing, so it is never
  charged twice.
- Codex cumulative counters are converted to per-event deltas and priced with
  the model active for that delta. Changing models inside one rollout no longer
  reprices earlier Token at the newest model's rate.
- Provider-reported totals retain `cost_kind=provider_estimate`; locally
  calculated totals use `cost_kind=computed`. `pricing_source` identifies the
  concrete dated source used for that model.
- Model aliases are exact and versioned. Unknown, ambiguous, future, and fast
  variants omit price rather than inheriting a nearby model's rate.
- The snapshot is compiled into the binary. ActRealm performs no price update
  request at Runtime and never turns a dated comparison estimate into a
  subscription-billing claim.
- The `claude-sonnet-5` introductory rate is explicitly time-sensitive: the
  source says it applies through August 31, 2026. The embedded snapshot and its
  source label must be updated before a later release; ActRealm does not silently
  substitute a post-promotion rate.
- Coverage means the standard API base rate for a recognized model, not every
  possible billing modifier. Provider totals still win when supplied. Local
  computation does not claim exact subscription-credit billing, Claude fast
  mode/data-residency/one-hour-cache adjustments, or GPT-5.5 long-context
  surcharges when the local source does not expose enough per-request detail.

### Usage model propagation

- Claude/Codex transcript and rollout collectors now return the structured
  model identifier together with numeric usage.
- SQLite schema 8 adds a bounded `session_usage.model` column. Snapshot reads
  prefer an event-supplied session model and otherwise use the matching usage
  model, so a card can no longer show “unknown model” while its price was
  computed from a known model.
- The migration is additive and preserves existing sessions and usage. It does
  not infer a model from a dollar amount or overwrite a Provider event model.

### OAuth expiry recovery without owning refresh tokens

- Claude credential parsing now accepts millisecond- or second-based
  `expiresAt` / `expires_at` values.
- A credential within four minutes of expiry is refreshed before the usage
  request. A 401 also triggers one bounded refresh attempt.
- Refresh delegates to the discovered official Claude executable with
  `claude auth status --json`; ActRealm never reads, stores, or submits the
  OAuth refresh token itself.
- The subprocess is invoked directly without a shell, has no inherited stdin,
  suppresses output, times out after 12 seconds, and is limited to one launch
  per minute.
- After delegation ActRealm re-reads the official credential store. A failed
  request is retried only if the access token actually changed. Otherwise the
  last validated quota sample remains visible with its real capture age.
- A desktop-only installation without a usable Claude CLI cannot be forced to
  refresh by ActRealm. In that case Claude Desktop/Claude Code may refresh its
  own credential, and ActRealm will pick up the changed value on a later pass.

### Keychain lookup with a bounded fallback

- macOS first tries the known `Claude Code-credentials` service directly,
  using bounded account candidates.
- A previously successful service/account locator is cached and tried first on
  later refreshes.
- Claude's local credentials file remains the next safe fallback.
- Full Keychain service enumeration is now last resort only, limited to 4 MiB,
  normalized to short Claude credential service names, de-duplicated, and
  cached for five minutes.
- Symbolic-link refusal, credential size limits, and memory-only token handling
  remain unchanged.

### Bounded incremental transcript aggregation

- Each Claude file maintains an O(1) token/cost accumulator plus a recent
  correction window instead of re-summing every retained message on each
  collection pass.
- Completed history is folded into a compact accumulator. Only the most recent
  256 message identities remain replaceable, which covers adjacent streamed
  revisions while placing a hard bound on per-file entry memory.
- A 10,000-entry regression fixture verifies exact input, output, cache, total,
  and computed-price preservation after compaction.
- Zero-token `<synthetic>` Claude records contribute exactly zero cost and are
  ignored when selecting the displayed model. They no longer hide an otherwise
  complete multi-model session estimate.
- File truncation/rotation still resets that file's cursor and accumulators;
  malformed or oversized records remain ignored without stopping Runtime.

The existing global limits still apply: bounded source files, bounded line
length, bounded file count, no transcript path or text in the browser snapshot,
and SQLite persistence of numeric aggregates only.

## Focused verification

- `cargo test -p actrealm-quota --offline`: PASS, 12 tests;
- `cargo test -p actrealm-usage --offline`: PASS, 11 tests, including the
  10,000-entry compaction and Codex model-change regressions;
- zero-warning focused Clippy for installer, quota, and usage: PASS.

## Full candidate verification

- `cargo fmt --all -- --check`: PASS;
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: PASS;
- `cargo test --workspace --offline`: PASS, 172 tests; two explicit manual or
  release-only tests ignored by the default suite;
- `cargo build --workspace --release --offline`: PASS;
- `./scripts/check-actrealm-language.sh`: PASS;
- explicit 120-second release resource gate: PASS, 117 samples, 0.000% average
  idle CPU, 6,176 KiB maximum Runtime RSS against an 81,920 KiB budget.

## Remaining acceptance gate

Before this candidate may be committed or pushed:

1. install the exact tested release locally and verify Runtime/Doctor plus live
   Claude/Codex usage and quota behavior;
2. obtain the user's explicit local acceptance;
3. obtain separate authorization for commit and for push.

The continuous 48-hour Runtime soak remains a separate final-v1 release gate.

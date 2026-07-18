# M14 live usage, context, price, and OAuth quota

Status: implementation, automated/local gates, exact-release installation, and
user acceptance complete. Local commit authorized on 2026-07-18; push remains
separately gated.

M14 adds a privacy-bounded numeric telemetry layer for local Agent sessions. It
does not persist prompts, assistant text, tool content, transcript paths, or
Provider credentials.

## Delivered scope

- Claude transcript JSONL is tailed incrementally. Streamed copies are
  de-duplicated by message/request identity, including main/sub-Agent overlap.
- Claude StatusLine contributes the official current-context size, percentage,
  and official session cost when present.
- Codex rollout JSONL contributes cumulative and last-turn token usage. Cached
  input remains a subset of input and reasoning remains a subset of output, so
  neither is double-counted.
- Session cards show cumulative Token, current-turn context occupancy, and an
  explicitly labelled estimated API price. Unknown models show no price rather
  than a false zero.
- Claude quota prefers the first-party OAuth usage endpoint when an existing
  Claude credential is available. It renders every returned window, including
  active model-scoped limits such as Fable and extra usage.
- OAuth runs on a background thread. The control panel stays responsive and
  immediately renders the last local value while refresh is in progress.
- OAuth credentials are read from the macOS Keychain or Claude's local
  credentials file, kept only in process memory, passed to `curl` over stdin,
  and never written to SQLite, cache, logs, diagnostics, export, or argv.
- Quota values no longer disappear or change to an artificial expired state
  after 30 minutes. The UI keeps the last validated percentage and shows its
  real capture age.

## Source and truth labels

| Display | Source | Meaning |
| --- | --- | --- |
| Session cumulative Token | Claude transcript / Codex rollout | All validated usage observed for that Provider session |
| Current-turn Token | Claude latest message / Codex `last_token_usage` | Latest Provider-reported turn usage |
| Context | Claude StatusLine/current message or Codex last turn | Current prompt/context consumption, never lifetime Token divided by window |
| Estimated API price | Provider cost field or embedded pricing snapshot | Comparison estimate, not a subscription invoice |
| Claude quota | Anthropic OAuth usage, then StatusLine cache fallback | Dynamic Provider-returned windows |
| Codex quota | Local rollout `rate_limits` | Dynamic local primary/secondary windows |

The embedded price snapshot is dated 2026-07-18. Unsupported or ambiguous
models omit price. The UI always says `估算 API 价` and explains that it is not
the user's subscription bill.

## Privacy and resilience gates

- usage cache files are mode `0600`; cache directories are mode `0700`;
- JSONL lines, StatusLine payloads, credential files, and OAuth responses are
  size-bounded;
- symbolic-link credential and usage sources are refused;
- malformed/unknown records are ignored without stopping Runtime;
- unknown price models return `None`;
- OAuth 401/429/network failure keeps the last local value and never blocks the
  dashboard snapshot path;
- data arrives in SQLite through the single writer and schema v7 migration.

## Reference review

- `ccusage/ccusage`: Claude/Codex token semantics, stream de-duplication, and
  offline pricing snapshot shape;
- `sirmalloc/ccstatusline`: official StatusLine context/cost fields and OAuth
  usage endpoint behavior;
- `soulduse/ai-token-monitor`: dynamic scoped quota/Fable response shapes and
  macOS credential-location compatibility. Its source was reviewed only; no
  source was copied because the checked-out repository did not include a
  license file;
- `graykode/abtop`: incremental local-file tailing and bounded refresh cadence.

Flow Agent's Rust collector, persistence model, security boundaries, and UI
rendering are implemented independently for this repository.

## Verification state

Passed before the final gate:

- Claude streamed-message de-duplication and incremental append;
- Codex cumulative/last-turn/cache/reasoning semantics;
- StatusLine context and cost capture with no prompt/path persistence;
- dynamic OAuth windows, null reset times, Fable, and extra usage parsing;
- OAuth cache credential-exclusion and file permissions;
- SQLite schema migration and session join;
- JavaScript syntax check and focused quota/usage/runtime/server suites.

Final gate evidence:

- `cargo fmt --all -- --check`: PASS;
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: PASS;
- `cargo test --workspace --offline`: PASS, 161 tests; two explicit/manual
  tests ignored by the default suite;
- `cargo build --workspace --release --offline`: PASS;
- explicit two-minute resource gate: PASS, 120 seconds, 118 samples, average
  idle CPU 0.000%, maximum Runtime RSS 6,016 KiB (budget 81,920 KiB).

Completed before commit:

1. installed binary SHA-256 matched the exact tested release:
   `ef85e991ace6e0e376ffb71ac9e6da7c98576cde2c08a4b5b36735d45beddbe5`;
2. schema 7, 18 real session-usage records, and first-party OAuth quota windows
   were verified on the installed Runtime;
3. the user accepted the installed candidate and explicitly authorized commit.

Still required before push: separate explicit user authorization.

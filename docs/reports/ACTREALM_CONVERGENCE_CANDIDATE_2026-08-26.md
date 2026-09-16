# ActRealm convergence candidate evidence

Date: 2026-08-26

Branch: `actrealm最新版`

Installed rollback baseline: Build 88 / `d6c891f`.

## Source result

- Agent Focus is frozen and its full macOS test suites remain unchanged/pass.
- Team/Cloud startup and five-second polling are parked.
- Review Baseline collection and Review/metadata Checkpoint presentation are parked.
- local archive is optimistic and no longer waits for Cloud or a full Snapshot refresh.
- Codex internal Overview prompts are filtered independent of cwd.
- Codex executable-path probes and the exact `/hooks` setup command are filtered; known whitespace entities are normalized in task summaries.
- task ordering prioritizes Attention/error before recency.
- routine Bash/MCP/read/search activity is absent from important status; running tool state is semantic.
- partial/suspect Token totals, cost, burn rate and analytics are absent from normal presentation.
- usage scan checkpoints are private, resumable and path-free; partial backfill is saved every 15 seconds and files above 1 GiB are incremental.
- Codex credential refresh is deferred until the first canonical usage generation, with a bounded 30-minute startup grace.
- JSONL refresh-budget boundaries no longer create false oversized lines; schema-3 checkpoints invalidate all affected earlier state.
- oversized Codex rows preserve completeness only for a closed, tested non-usage shape set; unknown and usage-bearing rows fail closed.
- schema 3 adds a monotonic cumulative envelope and immutable first Session ID, preventing stale-stream reset inflation and parent replay relabeling.
- caught-up generations atomically replace stale partial canonical rows.
- Team/mobile/Companion and advanced Token controls are absent from normal settings.
- History no longer loads Review/Checkpoint while parked.

## Automated gates

- `cargo fmt --all -- --check`: pass.
- workspace Clippy, all targets, offline, warnings denied: pass.
- full Rust workspace tests and doc tests: pass.
- Runtime: 56 unit plus 78 integration cases pass.
- Server: 59 unit, 8 API and 3 performance cases pass.
- Usage: 34/34 pass, including partial checkpoint resume, refresh-boundary truth, known non-usage oversized rows, interleaved cumulative streams, parent metadata replay and >1 GiB incremental coverage.
- release Rust workspace compilation: pass.
- Cloud Functions: 42/42 pass.
- language/runtime contracts: pass.
- final macOS package test script: pass, including 201 Swift Testing cases and all XCTest suites.
- `git diff --check`: pass.

## Compiled visual evidence

SnapshotTool output is stored outside the repository at:

`~/Documents/ChatGPT/actrealm/outputs/convergence-snapshots-final-20260826-v3`

Observed results:

- no partial Token card or per-task Token warning;
- no raw Bash/MCP name for ordinary running task status;
- official quota remains visible;
- wide, dark and narrow layouts render without clipping;
- Attention/OUTBOX hierarchy and Agent Focus navigation are unchanged.

## Exact-candidate acceptance

- Build 89 was withheld after live validation found two stale strings and a real first-scan restart loop; Build 90 was withheld before installation because it did not yet contain the lifecycle root-cause repair.
- Build 91 proved partial checkpoint progress and credential-boundary stability, then was withheld when Computer Use found encoded whitespace and a Codex executable probe in the active task surface.
- Build 92 proved checkpoint resume and both UI corrections, then was withheld when its real ledger exposed the schema-1 refresh-boundary false-partial defect.
- Build 93 proved schema-2 completeness, then was withheld when isolated real-ledger replay exposed stale cumulative reset inflation and replayed parent identity merging.
- Builds 94 and 95 validated schema 3 and exposed the final exact `/hooks` markdown presentation; Build 96 contains the bounded fix.
- Build 96 embeds `4015829f6c0e29756983dab34cc051e63e6370aa`, is arm64, Apple Development signed and passes strict codesign.
- Build 88 and every replaced validation build remain recoverable in Trash; live database and Provider configuration were preserved.
- Doctor passes 14/14 and packaged/stable Hook helper SHA-256 values match.
- schema-3 checkpoint is private `0600`, path-free, caught up for all discovered Claude/Codex sources and remains ready across restart.
- installed ledger has zero partial records, no future/negative facts and matching session-day/Provider-day/model-day projections; dashboard pricing coverage is explicitly 99.8% rather than claiming complete cost.
- acceptance sample: 12,012,670,827 Token, 35 active days, peak 2,795,031,882 on 2026-07-31; live totals continue increasing.
- Computer Use confirms two real active tasks only, working OUTBOX auto-hide copy, verified Token dashboard, official quota separation, reduced settings/history/diagnostics and unchanged Agent Focus.
- day/month/total Agent execution time changes to 2h46m / 73h30m / 105h2m in the acceptance sample.
- short two-Agent live sample: Native 2.69% average CPU / 153.3 MiB RSS; Runtime 4.32% / 24.8 MiB. This is not the separate seven-day soak.

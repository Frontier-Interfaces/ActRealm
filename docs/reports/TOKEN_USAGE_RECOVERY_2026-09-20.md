# Current token usage recovery — 2026-09-20

## Failure and evidence

The installed build 129 showed zero usage today and no usage on current Codex
tasks even though the local rollout files contained official numeric records.
During observation its cumulative total fell from about 83.17 million to
4,121,728 tokens. The authenticated snapshot still described collection as
partial with `collectionInProgress: false`. The live checkpoint contained
246 Codex sources, most with no read progress; the remaining source data was
approximately 4 GiB.

The old bounded directory walker restarted each discovery pass and treated the
returned prefix as the entire inventory. This could discard parser progress
and publish a smaller generation. Separately, a shared byte budget and atomic
whole-history publication let historical scans delay current usage.

## Change

- Resume directory enumeration across bounded slices. Retain known sources
  while discovery is incomplete; certify inventory only after a full walk.
- Read bounded first-line metadata to identify Codex sources before large
  backfills, using the same metadata parser as the ordinary scan. Account for
  these reads in the existing 10 MiB collection budget. Prioritize recent
  sources while reserving byte capacity for historical progress.
- Publish complete Codex sessions independently after all discovered source
  identities are accounted for. An incomplete resume fragment still prevents
  its session from overwriting the committed ledger.
- Keep existing totals during an unfinished generation and display an explicit
  local backfill state. Preserve response deduplication, inherited/fork history,
  daily attribution, private numeric checkpoints and full-generation replacement.
- Route discovery regression tests through the production walker. Add coverage
  for live publication with multiple large unrelated historical sources and
  the shared byte budget. Warm the fake terminal executable before its existing
  two-second renewal test; production deadlines remain unchanged.

## Verification

Final local regression: 438 Rust tests passed, 3 intentionally ignored; 221
Swift tests passed. All-target Clippy with warnings denied, release build,
language contract, formatting, Info.plist and diff checks passed. This includes
52 usage tests, live-session publication, discovery resumption, shared byte
budgets, and the existing snapshot/WebSocket performance gates. Earlier
parallel attempts hit short child-process deadlines; the final full workspace
run used `--test-threads=1` and preserved all production deadlines.

Build 131 was installed for this acceptance. Its strict signature passed, and
the bundled helper and shared Runtime hashes matched the running installation.
The Display executable hash and version were unchanged. Build 130 had already
restored task-level usage; its global backfill exposed the need for early source
identity discovery, included in build 131. The latest build 133 retains this
recovery; see [automatic connection verification](AUTO_MANAGED_2026-09-20.md).

The previous task's official local counter is 93,835,323 tokens, consisting of
93,596,693 input tokens and 238,630 output tokens. Its cache-read and reasoning
counts are subsets and are not added again. The installed snapshot must match
these component values; the installed snapshot matches exactly, both before
and after Runtime restart. The native UI displayed approximately 209 million tokens
for today during acceptance. Restart snapshots increased from 210,120,814 to
210,311,355 today, and from 9,506,250 to 9,696,791 on the continuing task.
The frozen prior-task counter remained 93,835,323.

At the build 131 acceptance snapshot, the full historical aggregate was still
backfilling and explicitly labelled. It was not presented as final. No numeric cache was cleared or seeded.
The isolated audit also repeated 257 unchanged source sessions with zero numeric
mismatches. Local evidence is under `outputs/usage-recovery-20260920/` in the
outer workspace (not tracked in this source repository).

This report records acceptance before submission to PR #10. No Grok/Kimi model
prompts, cache deletion, direct database repair, Display update, merge or public
release occurred during this repair.

# ActRealm R0 real gate and R1–R4 reframe progress

Date: 2026-08-25

Branch: `actrealm最新版`

Installed final candidate: build 88, commit
`d6c891f5548518063e2fac38b00f4dfb57922d6e`, Apple Development signed, arm64.

R1–R4 and the subsequent rendering/transport performance corrections are
committed, pushed, installed, and tested. Documentation may advance HEAD after
this exact installed code commit without changing the packaged binary.

## R0 installed-candidate evidence

- build 81 moved to
  `~/.Trash/ActRealm-build81-pre-product-reframe-20260825.app`;
- build 82 `CFBundleVersion=82`, exact embedded commit `03b8d21...`;
- strict deep codesign verification passes; TeamIdentifier `5P4AM5CG8X`;
- app and helper are arm64; Doctor overall pass;
- empty OUTBOX is absent on launch and Agent Tasks receives the released space;
- provisional Token shows verification state, not cumulative or API-equivalent
  headline numbers;
- exact `git status` request exposes Allow and Deny; Allow returns the official
  Codex Hook decision after the delayed-submit window;
- medium `apply_patch` exposes Deny and return-to-original only; hand-back leaves
  Hook stdout empty;
- `git status ; rm temporary.txt` is unknown/compound and has no Allow; Deny
  returns the official Codex Hook denial;
- `rm -rf temporary-r0-fixture` is high risk and has no Allow;
- after completion reminders are acknowledged and the pending-decision status
  expires, empty OUTBOX collapses again.

The commands above were synthetic Hook requests against the real installed
Runtime/UI. No dangerous command was executed.

## R1 workspace and hierarchy implemented after build 82

- Join moved from the main header into Settings → General → Collaboration;
- main window minimum width reduced from 1160pt to 900pt;
- narrow attention layout uses Agent Tasks as the main column and stacks OUTBOX
  above Quota in a bounded sidebar;
- expanded running tasks show Plan/Recent Activity before secondary details;
- current action, current target, context, workspace, recovery, and control stay
  in the primary facts grid;
- Token components, IDs, pricing and developer facts move into a collapsed More
  Details section;
- History internal-validation matching now includes H2.4 anywhere in the title,
  explicit real-acceptance wording, and known fixture project identities;
- default design-system body, card, section, and micro type sizes increased.

## R2 Token truth and performance implemented after build 82

Independent current-session audit:

- source rollout size: about 188 MiB and 38,080 JSONL lines at audit time;
- latest Provider cumulative total: about 1.768B Token;
- sum of unique `last_token_usage` entries: about 1.776B Token;
- duplicate exact usage entries: 1 of 7,699;
- ActRealm canonical value at the same audit boundary: about 1.686B, therefore
  partial/behind rather than inflated for this session;
- another old partial session had about 1.241B latest raw cumulative but only
  30.39M canonical observed Token, proving partial rows cannot support ranking
  or peak claims.

Consequent changes:

- partial/rebuilding/suspect Token pages show Observed Token and Observed
  API-equivalent value;
- API-equivalent wording explicitly says it is not a subscription bill or
  actual spend;
- until the ledger is verified and caught up, peak day, common model,
  attribution, heatmap, breakdown ranking, and trend charts are withheld;
- live burn rate remains available because it uses bounded monotonic samples;
- the heavy Token window renders only while its AppKit window is genuinely
  visible; closing or occluding it removes the chart tree;
- usage backfill polls every two seconds and caught-up usage every five seconds,
  instead of scanning every second indefinitely.
- build 83 sampling confirmed that a closed Token window no longer retained
  `TokenUsageDashboardContent` or heatmap/chart work. The remaining main-window
  cost came from the global one-second workspace clock and repeated expanded
  detail derivation; the build 83 follow-up publishes the clock every five
  seconds during ordinary observation, retains one-second precision only for a
  pending decision/focus/HUD deadline, and computes expanded detail items once
  per render pass.
- the final clock policy uses 30 seconds when there is no running/waiting work,
  five seconds for active observation, and one second only for a pending
  decision or focus/HUD deadline.
- build 84 follow-up sampling then isolated the remaining transport churn:
  Runtime still rebuilt a full snapshot every 100ms, first-scan progress flipped
  on every worker iteration, and pure status timestamps changed payload identity.
  The final working tree uses a 250ms snapshot cadence, stable first-scan state,
  and 30-second UI timestamp buckets while preserving exact export/diagnostic
  timestamps. The existing event-to-render p95-under-300ms test still passes.
- quota `capturedAt` is also projected at the same 30-second UI granularity;
  percent/reset/source changes still publish immediately, while repeated
  identical “updated now” timestamps no longer rebuild every Quota card.

## R3 Review and Recent Activity implemented after build 82

- Recent Activity defaults to at most five important/latest rows;
- failures, validation, file edits, version-control work, interaction, live and
  long operations are prioritized;
- the user can expand all current-Turn rows and then load earlier pages;
- running tasks hide empty Review chrome and show Review only when current
  changes, validations, or failure evidence exist;
- zero diff now means “no uncommitted workspace changes,” not “the task made no
  changes”; committed results direct the user to commits/history.

## R4 diagnostics and recovery hierarchy implemented after build 82

- diagnostic layer conclusions and recovery actions remain visible by default;
- versions, instance, Provider capability cards, Cowork, and paused H6 details
  move into a collapsed Technical and Capability Details section;
- History empty Review/Checkpoint/security cards remain consolidated;
- app-only and terminal jump wording remains capability-specific;
- metadata-only Checkpoint is no longer visible on running tasks without Review
  evidence; full Checkpoint product semantics remain a follow-up decision;
- approval copy now states that only the unsent decision can be withdrawn; it
  never implies that a command can be undone after Provider submission.

## Automated gates for final build 88 code

- Rust fmt, Clippy `-D warnings`, full workspace tests: pass;
- server unit suite includes the adaptive usage-refresh cadence test and passes
  57/57;
- macOS: 35 XCTest plus 201 Swift Testing cases pass;
- language/runtime contracts and `git diff --check`: pass;
- 28 Chinese compiled snapshots rendered; wide, narrow, expanded, Settings and
  diagnostics layouts visually inspected;
- narrow screenshot exposed vertical centering of the task column; the layout
  was corrected to top alignment before the full gate.

## Still open

- a parser or sandbox cannot prove arbitrary shell safety; unknown remains
  hand-back/deny;
- Token first-scan completion and caught-up long-duration resource behavior
  still need observation on build 88;
- 20 new post-reframe real Codex/Claude Review tasks and 7-day soak remain;
- final VoiceOver, contrast, keyboard order, Checkpoint positioning, Team Today
  boundary, and Agent Focus interruption acceptance remain open;
- H6, mobile, Watch, Cowork, and H8 Display remain parked.

## Final build 88 resource and lifecycle evidence

- strict codesign, exact build/commit metadata, arm64, and Doctor overall pass;
- closing the Token dashboard removes Token dashboard, heatmap, and trend code
  from the Native sample stack;
- partial Token Trends shows only the coverage explanation, with no hidden
  ranking/chart content;
- wide, narrow, expanded task, Settings collaboration, History filter and
  collapsed diagnostics were checked in compiled snapshots or Computer Use;
- pre-reframe Native sampling was commonly 39–55% during chart churn and later
  13–20% during ordinary observation;
- after the window lifecycle, snapshot, quota, global clock, and per-task
  Timeline cadence changes, warmed idle Native samples repeatedly reached
  0–4%, with brief event/30-second refresh spikes;
- Runtime stayed around 3–8% while the first bounded scan processed about
  2.8GiB of Codex history. This is a backfill measurement, not a caught-up idle
  claim; the 2-second backfill / 5-second live cadence and full 7-day result
  remain visible acceptance boundaries.

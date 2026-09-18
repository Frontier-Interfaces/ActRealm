# ActRealm product reframe — P0 optimization pass

Date: 2026-08-25

Branch: `actrealm最新版`

Committed candidate: `03b8d21bac534589da3775513341c1274f39eba4`

State: committed and pushed; installed as Apple Development signed build 82.
Automated gates, compiled snapshot review, Doctor, exact commit check, real
approval matrix, and OUTBOX collapse pass. Overall user acceptance remains
open.

The complete issue inventory and follow-up plan are maintained in
`../ACTREALM_PRODUCT_REFRAME_EXECUTION_PLAN_2026-08-25.md`.

## Why this pass exists

The product audit found that ActRealm's Runtime and Provider plumbing is real,
but the daily control surface had accumulated too many secondary features. It
also found a P0 mismatch between the declared approval risk model and the
actions exposed by the main OUTBOX, HUD, menu bar, and remote projection.

This pass does not add another feature area. It narrows the product around the
human-control loop: notice work that needs attention, understand the bounded
evidence, return to the Provider when evidence is insufficient, and avoid
presenting incomplete analytics as a primary decision fact.

## Approval safety corrections

1. Compound shell syntax is evaluated before any low- or medium-risk return.
   `git status ; rm file`, `git log && terraform destroy`, pipelines, redirects,
   and composed package commands now fail closed as unknown unless an earlier
   high-impact rule already applies.
2. Known high-impact matches still keep their stronger classification. A
   privileged package install remains PackageInstall/high instead of losing its
   category.
3. Risk labels are not sufficient approval evidence. A redacted surface may
   expose Allow only for an exact low-risk Git read operation: `git status`,
   `git diff`, or `git log`.
4. Medium, high, unknown, hidden-target reads, generic shell commands, and
   unknown command shapes expose Deny plus return-to-original-window locally;
   remote projections are deny-only.
5. Main OUTBOX, HUD, menu bar, shared-task UI, personal remote envelope, and the
   Firebase validation policy now apply the same rule.
6. The local primary approval card now displays the declared risk and reason.
   The previous simultaneous Allow and “Allow after confirmation” choices were
   removed for requests that require original-window review.
7. Remote command shapes retain only the three exact safe Git read labels. All
   other executable families remain argument-free `<redacted>` shapes or
   `Unknown shell operation`.

This remains a conservative policy layer, not a shell sandbox or formal proof
that a command is harmless. Full scope and targets stay in the Provider window.

## Workspace and information hierarchy

- OUTBOX collapses completely when there is no open local/remote attention and
  no pending undo decision. Agent Tasks receives the released space.
- During Token scanning, partial collection, unavailable collection, or suspect
  quality, the workspace shows a compact verification state instead of daily,
  monthly, cumulative, or API-equivalent headline numbers.
- A task with provisional usage shows “Token data under review” in its collapsed
  strip. Expanded details may identify the observed amount as incomplete, but
  API-equivalent price remains hidden until verification completes.
- The panel previously titled Workflow is now Recent Activity. The underlying
  bounded tool-event contract and pagination are unchanged.
- History hides only explicit internal H2.4/smoke fixtures by default and offers
  a switch to reveal them.
- History no longer renders three separate empty Review, Checkpoint, and
  security cards. It shows one bounded no-evidence explanation, and the return
  button says whether it opens an exact conversation, terminal session, or only
  the Agent application.

## Update and rendering efficiency

`AppModel` no longer publishes equal Token totals, equal Token decision data,
equal threshold notices, or an equal DerivedState for every Runtime snapshot.
This reduces unnecessary SwiftUI invalidation without delaying real attention,
task, quota, or activity changes.

## Automated evidence

- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: pass.
- `cargo test --workspace --offline`: pass.
- `cargo build --workspace --release --offline`: pass.
- Firebase Functions TypeScript build and 42 tests: pass.
- macOS full test script: 35 XCTest cases plus 197 Swift Testing cases pass.
- Language/runtime contracts: pass.
- `plutil -lint` and `git diff --check`: pass.
- Compiled SnapshotTool rendered 28 Chinese artifacts. Main workspace,
  expanded task, and menu-bar popover were visually checked. The high-risk demo
  request exposes Deny and return-to-original only; provisional Token totals are
  absent from the workspace headline.

## Deliberate limits and next gate

- No unsafe command was executed to test the classifier.
- The source executable cannot be launched with `swift run` because macOS
  UserNotifications requires an application bundle; packaged behavior is
  covered by the normal app packaging path and tests.
- Build 82 embeds the exact P0 commit and replaced build 81 only after the old
  app was moved to the Trash as a recoverable backup.
- H6 mobile, Watch, Cowork, and H8 Display remain paused.
- Real local Hook/UI checks covered exact safe Git allow, medium hand-back,
  compound-command deny, high-risk hand-back, official Hook output, completion
  acknowledgement, provisional Token state, History filtering, and delayed
  empty-OUTBOX collapse.

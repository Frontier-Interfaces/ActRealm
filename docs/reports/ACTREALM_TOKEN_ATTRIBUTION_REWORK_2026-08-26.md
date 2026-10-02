# ActRealm Token attribution rework

Date: 2026-08-26

Branch: `actrealm最新版`

## Problem

The verified schema-3 ledger recovered Provider sessions that the earlier
parser had incorrectly merged into parent identities. The total ledger was
correct, but the dashboard still used one task-table join for both task and
project attribution. This made `4,953,910,637` Token look wholly unassigned.

The pre-convergence database used the same join rule. Its smaller partial
ledger reported `587,788,879` unattributed Token; fixing collection recovered
the missing child/history sessions but exposed the attribution-model gap.

## Reference behavior

Token Monitor decorates each session independently from Provider session
metadata: it reads `cwd`, `project_path` or `workingDirectory`, hashes the
normalized path and keeps a bounded final component as the project label. It
does not require the session to exist in a separate task dashboard first.

ActRealm now follows that useful boundary while preserving its stricter local
privacy model:

- raw working directories and Git remotes are never persisted or exported;
- project IDs are SHA-256 values over normalized local or sanitized remote
  identities;
- only the bounded repository/project name is retained;
- `.git/config` reads are direct, bounded, non-symlink reads;
- multiple repository candidates resolve only when every origin matches or a
  unique repository name matches the technical workspace slug.

## New attribution layers

Project and task attribution are independent facts:

- **Project attribution** uses Provider metadata or a verified repository
  identity, even when no ActRealm task exists.
- **Task attribution** uses an exact ActRealm session or follows
  `parent_thread_id` / `forked_from_id` to the nearest verified parent task.
- Parent traversal is cycle-safe and capped at eight edges.
- Every ledger session contributes to at most one project and one task.
- Legacy `attributedTokens` fields remain task-scoped for old clients; schema 2
  exposes explicit project/task totals and coverage.

## Storage and checkpoint changes

- Usage checkpoint schema 4 adds only `projectId`, `projectLabel` and opaque
  `parentProviderSessionId`.
- Runtime schema 35 adds the same fields to the rebuildable local Token cursor.
- The canonical Token total, day/model tables, cost, cache and pricing rules do
  not change.
- Project metadata is rebuilt from Provider sources; old schema-3 checkpoints
  are intentionally rejected.

## Real-ledger validation

An isolated production-collector replay of the real 5.7 GiB ledger completed
with 214 records, zero partial records and identical session/day totals of
`12,181,765,583` Token at the validation sample.

| Layer | Coverage | Unresolved |
| --- | ---: | ---: |
| Project | 99.4% | 72,334,192 Token |
| Task | 98.4% | 194,020,638 Token |

The previous 4.95 billion combined bucket is therefore removed. The largest
technical ActRealm workspaces resolve through their shared Git origin and merge
into one `ActRealm-Cloud` project (`6,822,892,969` Token in this sample).

Remaining project-unknown usage belongs to historical workspaces whose source
metadata contains only an opaque workspace ID and no uniquely verifiable
repository. Remaining task-unavailable usage has a project but no recoverable
ActRealm task or verified parent. Neither case is guessed.

## Automated evidence

- Usage: 35/35.
- Runtime: 57 unit plus 78 integration tests.
- Server: 59 unit, 8 API and 3 performance tests.
- macOS: 201 Swift Testing cases plus all XCTest suites.
- full Rust workspace tests, doc tests, release build and Clippy with warnings
  denied pass.
- language contracts and JavaScript syntax checks pass.

## Build 97 installed acceptance

- embedded code commit: `b5fc9d998dc9fec6bf4c612d841d6289c9e870f0`;
- Apple Development signed, arm64 and strict deep codesign pass;
- Runtime schema 35 and checkpoint schema 4 are active;
- 210/210 Token cursor records contain project identity; 63 contain parent
  session identity;
- Doctor passes 14/14 and stable Hook / packaged Helper hashes match;
- SQLite quick check and both day-projection invariant checks pass;
- Computer Use shows separate project and task coverage without clipping;
- `ActRealm-Cloud` is one project row rather than separate UUID/URL workspace
  rows, and child usage appears in the verified parent task totals;
- short two-Agent live sample: Native 2.55% CPU / 177.0 MiB RSS; Runtime 4.90%
  / 30.6 MiB.

Backup before migration:

`~/.actrealm/token-backups/token-attribution-v4-build97-20260826/data-before-schema35.sqlite`

SHA-256:

`47369048cde6d9fc6c44cefb11cc0d50670807d76f31da0687a3d548c6c6b8cb`

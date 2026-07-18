# M0 verification record

Status: **PASS** on 2026-07-15 (Asia/Shanghai).

Branch: `agent/v1-full`

Baseline: `ffca3c3f213341fd4ac2dc90a48b99a219ce7940`

Environment: macOS 26.5.2 (25F84), rustc 1.97.0, cargo 1.97.0.

## Automated gate

All commands were run from the repository root with the working tree that will
form the M0 milestone commit.

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | PASS, zero warnings |
| `cargo test --workspace --offline` | PASS, 29 tests, zero failures |
| `cargo build --workspace --release --offline` | PASS |
| `./scripts/m0-e2e.sh` | PASS: Claude allow, Codex deny, explicit pass-through, and missing-runtime fail-open |

The automated suite includes exact minimal provider response JSON, 3-second
undo semantics, request-ID mismatch rejection, provider-aligned deadlines,
missing runtime under 200ms, socket EOF, stdin-never-closes protection,
unknown events, versioned fixtures, and the Codex multiple-hook false
confirmation regression.

## Live provider probes

Sanitized live input is checked in under `fixtures/claude/2.1.210/` and
`fixtures/codex/0.144.4/`. Each `fixture-set.json` records
`live_probe_confirmed` and the fields sanitized before commit.

| Probe | Result |
| --- | --- |
| Claude Code 2.1.210 allow | PASS: native UI reported approval by PermissionRequest hook; the safe probe proceeded |
| Claude Code 2.1.210 deny | PASS: native UI reported denial; the command did not execute |
| Claude Code 2.1.210 pass-through | PASS: empty ActRealm stdout restored a usable native approval dialog |
| Codex CLI 0.144.4 allow | PASS: PermissionRequest was followed by PostToolUse and Stop |
| Codex CLI 0.144.4 deny | PASS: hook blocked execution; no PostToolUse followed |
| Codex CLI 0.144.4 pass-through | PASS: empty ActRealm stdout restored native approval; native No remained usable |
| Codex trust boundary | PASS: untrusted project emitted no ActRealm events; trusting the exact `/hooks` command enabled the full lifecycle |

Live probes used an isolated local repository and a non-mutating dry-run push
or a command that failed before any push. No remote repository state changed.

## Reference gate

`REFERENCE_REVIEW.md` records exact reviewed revisions of Open Vibe Island and
CodeIsland, their license boundaries, accepted reliability lessons, rejected
patterns, and owning milestone. M0 code implements the accepted timing, stdin,
output, and fail-open decisions. Later decisions are binding M1-M5 gates.

## Defects found before acceptance

1. Buffered terminal input could hide `u` from the old file-descriptor poll and
   let an undone allow reach the provider. Fixed with one prompt input channel;
   regression test passes.
2. A provider that never closed Hook stdin could hold the process forever.
   Fixed with a poll-based 5-second input deadline; regression test passes.
3. A normal Codex Stop after another hook vetoed our allow could be shown as a
   false confirmation. Stop no longer confirms allow; only explicit tool
   completion evidence does. Regression test passes.

## Gate decision

M0 meets its repository acceptance criteria and may be committed and pushed.
This decision covers only the provider control-path foundation; it does not
claim M1 runtime persistence, M2 UI, M3 installation, M4 quota/settings, or M5
release readiness.

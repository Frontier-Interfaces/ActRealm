# M0 reference review and hook reliability decisions

Status: accepted as an M0 gate on 2026-07-15. These decisions amend the
`WIDGET_V1_PLAN.md` v1.1 baseline where real probes or production reference
experience disproved an assumption. They do not expand Flow Agent into managed
session control.

## Reviewed inputs

| Project | Reviewed revision | License | Use in Flow Agent |
| --- | --- | --- | --- |
| [Open Vibe Island](https://github.com/Octane0411/open-vibe-island) | `6e5e7a6a5b5097ee627a7d4dea6226c128747a71` | GPL-3.0 | Architecture and failure-mode study only; no source copied |
| [CodeIsland](https://github.com/wxtsky/CodeIsland) | `3e2aec7fa87c56b0f5129d7ba11d0dc3699dd500` | MIT | Architecture and failure-mode study; Flow Agent remains an independent Rust implementation |

The review covered hook CLIs, socket servers, installers, health checks,
permission queues, session reducers, process discovery, diagnostics, quota
bridges, tests, and project documentation. Current official provider contracts
and Flow Agent's live Claude Code 2.1.210 / Codex CLI 0.144.4 probes remain the
authority when a third-party example conflicts with them.

## Accepted decisions

| Area | Production lesson | Flow Agent decision | Gate |
| --- | --- | --- | --- |
| Human approval | A 60-second global deadline is too short for an attention surface | Claude waits up to 24 hours; Codex waits up to 1 hour. Runtime absence, connect failure, socket EOF, or explicit pass-through still returns native control immediately. Tests inject millisecond budgets. | M0 |
| Hook stdin | Provider launchers can leave stdin open | Hook input has an independent 5-second hard deadline and 256 KB cap; failure is silent pass-through. | M0 |
| Hook output | Old examples can lag provider contracts | Emit only the current official minimal `PermissionRequest` directive. Never add unsupported Codex `continue`, `stopReason`, or `suppressOutput`. Empty stdout is pass-through. | M0 |
| Default Codex events | Full tool lifecycle hooks can add terminal noise | Default to `SessionStart`, `UserPromptSubmit`, `PermissionRequest`, and `Stop`; make `PreToolUse` / `PostToolUse` an explicit enhanced-observation option. | M3 |
| Request identity | A session ID cannot identify concurrent or repeated approvals | Key waiters by Flow Agent `requestId` plus provider correlation fields; maintain a concurrent request queue across sessions. | M1 |
| Duplicate requests | A provider can repeat the same logical approval | Deduplicate by provider correlation key. Replacing a waiter must resolve the older waiter safely and never reuse its decision. | M1 |
| Socket half-close | Read-side EOF can be a normal client `SHUT_WR`, not disconnection | Never auto-deny or drain waiters solely because request-side EOF is observed. Add a half-close regression test and distinguish real peer failure. | M1 |
| Runtime restart | Permission continuations cannot survive a process restart safely | Keep waiters memory-only; expire old approvals after restart; never replay a permission request from spool. | M1 |
| Session completion | `Stop` is turn completion, and some exits emit no session-end event | Reconcile hook projections with provider process liveness and sanitized transcript/rollout metadata; do not equate `Stop` with process death. | M1/M3 |
| Installation intent | Auto-repair must not undo an intentional uninstall or partial manual removal | Persist tri-state intent: untouched, installed, or uninstalled. Repair only previously managed entries the user still intends to keep. | M3 |
| Installer safety | Config rewrites and moved binaries are common failure sources | Install a stable helper path, use backup + lock + semantic merge + atomic rename, honor `CODEX_HOME`, refuse malformed config, and remove only manifest-owned entries. | M3 |
| Codex compatibility | Feature keys and trust rules changed across releases | Write canonical `[features].hooks`, recognize legacy `codex_hooks`, report exact-command trust separately, and version-gate compatibility behavior. | M3 |
| Diagnostics | “Connected/not connected” is not actionable | `doctor` returns structured issues: missing/not executable binary, malformed config, stale path, missing manifest, conflicting hooks, untrusted Codex hook, runtime/probe failure, and whether repair is safe. | M3 |
| Terminal return | Jump-back needs more than `TERM_PROGRAM` | Capture only non-secret terminal identifiers needed for iTerm, Terminal, VS Code, tmux, kitty, WezTerm/Kaku, cmux, and zellij return paths. | M3/M4 |
| Quota bridge | Replacing an existing status line breaks user setup | Never overwrite a custom status line; show quota as unavailable when a safe bridge cannot be installed. | M4 |
| Diagnostics privacy | Raw hook logging can expose prompts, commands, paths, and tokens | No raw payload logging by default. Use redacted bounded diagnostics and an explicit short-lived diagnostic mode only. | M5 |

## Explicitly rejected patterns

- Copying GPL-3.0 implementation code into Flow Agent's MIT codebase.
- Treating a successful Flow Agent write as proof that the provider executed an
  action; later provider evidence is required.
- Auto-denying a permission because the client half-closed its write side.
- Indexing pending permission state by session ID alone.
- Silently recreating hook entries after the user removed or uninstalled them.
- Creating provider configuration for a provider that is not installed.
- Logging unsanitized hook stdin to `/tmp` or persistent application logs.
- Assuming third-party hook response examples are current when official docs or
  live probes say otherwise.

## M0 completion condition

M0 cannot be committed or pushed until the accepted M0 decisions above have
automated tests, the live provider probe report is current, and the entire M0
gate passes from a clean checkout. Later-milestone decisions are binding
acceptance criteria for their named milestone.

# M0 provider capability and boundary report

Verified on macOS on 2026-07-15 with Claude Code 2.1.210 and Codex CLI
0.144.4. The checked-in JSON fixtures are sanitized copies of real hook input;
the fixture metadata records which fields were replaced.

## Capability matrix

| Capability | Claude Code 2.1.210 | Codex CLI 0.144.4 | v1 behavior |
| --- | --- | --- | --- |
| Session lifecycle | SessionStart, UserPromptSubmit, Stop | SessionStart, UserPromptSubmit, Stop | Observe |
| Tool lifecycle | PreToolUse; success/failure events are provider-dependent | PreToolUse, PostToolUse | Observe |
| Permission control | PermissionRequest allow/deny | PermissionRequest allow/deny | Control one request |
| Pass-through | Empty hook stdout restores native dialog | Empty hook stdout restores native dialog | Supported |
| Request correlation | `prompt_id`; PermissionRequest has no `tool_use_id` | `turn_id`; PermissionRequest has no `tool_use_id` | Internal request ID plus provider IDs |
| Hook trust | Settings file is loaded directly | Exact command hash must be trusted in `/hooks` | Doctor must report separately |
| Multiple hooks | Provider executes configured hooks | Hooks may run concurrently; any deny wins | Never infer allow from our write alone |
| Runtime absent/dead | Hook must return empty stdout | Hook must return empty stdout | Fail open |
| Human approval deadline | 24 hours | 1 hour | Hook process owns the provider-aligned deadline |

## Integration boundary

Flow Agent v1 is an External Hook Control integration. It observes provider
events and may answer one `PermissionRequest` with allow or deny. It does not
own the provider process, conversation, tool execution, native permission
policy, or other hooks. It therefore does not claim reply, cancel, interrupt,
steer, queue, or managed-session semantics.

A directive written to the provider is `decision_sent`, not confirmed. Only a
later provider event can confirm progress. Codex can run several hooks for the
same event; a denial from any hook wins, so a Flow Agent allow cannot be shown
as confirmed until subsequent provider evidence arrives.

Permission requests are never persisted for replay. Runtime absence, protocol
error, socket EOF, explicit pass-through, or the provider-aligned deadline
produces empty stdout so the native provider remains responsible for the
decision. The original global 60-second plan value was rejected after the
production reference review recorded in `REFERENCE_REVIEW.md`.

## Real-probe outcomes

- Claude allow: the terminal reported “Allowed by PermissionRequest hook”; the
  command ran and the session stopped normally.
- Claude deny: the terminal reported “Denied by PermissionRequest hook”; the
  command did not execute.
- Claude pass-through: Flow Agent returned no directive and the native Claude
  approval dialog appeared and remained usable.
- Codex allow: PermissionRequest was followed by PostToolUse and Stop.
- Codex deny: the terminal reported the hook as blocked; no PostToolUse was
  emitted and Stop reported that the command did not execute.
- Codex pass-through: Flow Agent returned no directive; the native Codex dialog
  appeared, and native “No” produced no PostToolUse.
- Codex trust: continuing without trust emitted no Flow Agent events. After the
  exact project hook command was inspected and trusted in `/hooks`, the same
  probe emitted the complete lifecycle.

The probe command was always a local, non-mutating dry-run push in an isolated
test repository (or a command that failed before any push); no remote state was
changed.

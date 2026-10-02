# Native task status and activity parity — Build 120

Date: 2026-09-16. Scope: native task cards, expanded facts, workflow events and
the Runtime's bounded tool categorization. The retired Web source is unchanged.

## Changes

- A dedicated current-action line uses semantic categories such as reading a
  file, searching a project, editing, testing, building, checking code,
  managing dependencies and operating an interface. Bash is no longer the main
  action label; the original tool identity remains secondary metadata.
- Provider-supplied target basenames appear beside live actions and in event
  titles. No filename is inferred from a shell command, and raw commands,
  source code or tool output are not added to the persisted event payload.
- Pending permission, native-only permission, question, decision commit,
  decision-sent, completion awaiting acknowledgement, confirmed completion,
  errors and unconfirmed sources have distinct native presentation. The
  underlying Provider execution/control state is not rewritten by the UI.
- Event rows show action, explicit target and outcome over up to two lines,
  followed by the source tool. The event viewport remains bounded and scrollable.
- Pairing a terminal event with its start retains the richer start category and
  target when the terminal message only contains Bash/Shell metadata.
- Distinct file targets, tests/checks and failures remain individual events;
  only adjacent generic short shell completions are collapsed.
- A missing tool completion is not shown as still running just because its old
  validation state was running. Only the current matching tool gets that label.
  Tests/builds/checks without explicit result evidence remain unverifiable.
- Runtime now recognizes CUA interactions, code runners, image reads, process
  interaction, Node tests and code checks. Interpreter bodies mentioning tests
  or URLs are not mistaken for actually running those commands.

## Validation

- 404 Rust tests passed; 3 explicitly ignored tests remain ignored.
- 207 Swift tests passed, including semantic action, waiting/decision/completion
  state, stale tool suppression, retained start metadata, distinct targets and
  validation-outcome regressions.
- Clippy with warnings denied, formatting, language contracts and diff whitespace
  checks passed.
- Synthetic native previews exercise running tests and pending completion;
  workflow fixtures contain read/edit/check/test events without executing them.
- Existing Build 119 and shared Runtime are preserved in rollback/.

Older events without captured category or target metadata can only show a
bounded generic action such as running a command. This change does not invent
historical command details or mark missing outcomes successful. No commit or
push was made. Installed-app observations are appended after delivery.

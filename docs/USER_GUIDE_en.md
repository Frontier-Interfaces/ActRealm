# ActRealm v1 English user guide

This guide covers the current source-built test candidate. It supports Apple
Silicon Macs running macOS 26. Intel Macs are not supported. Claude Code and
Codex are the two supported Providers, through either their local CLI or local
desktop application.

> The current hardening candidate is based on
> `1dff02d879443876a1ab59aca1654ca9d084e7ed` plus uncommitted local changes.
> Tasks 1–9 have passed their scoped gates. The full Task 10 gate, exact
> candidate installation, real Provider acceptance, commit, and push are still
> pending. This candidate does not claim Intel support, automatic updates, a
> 48-hour soak, or accessibility qualification.

## 1. Requirements and support boundary

You need:

- an Apple Silicon Mac running macOS 26;
- Git;
- Rust 1.97;
- at least one local Provider installation: Claude Code CLI/Desktop or Codex
  CLI/Desktop.

A global `claude` or `codex` command is not required when a supported desktop
app provides its official executable. Remote/cloud Provider sessions cannot
connect to the local Runtime.

ActRealm does not own a Provider session. It observes official local events and
offers an action only when a live reply channel proves that the action is
available.

| Provider surface | Candidate support | Boundary |
| --- | --- | --- |
| Claude Code CLI | Supported | Local installed Hook |
| Claude Desktop local Code session | Supported | Shares the supported local Hook/settings path |
| Claude remote/cloud session | Not locally controllable | No path to the local Unix socket |
| Codex CLI | Supported | Local installed and user-trusted Hook |
| ChatGPT/Codex desktop local task | Supported | Uses the bundled official Codex executable; trust remains user controlled |
| Codex cloud/Web task | Not locally controllable | No path to the local Runtime |

## 2. Build from source

```bash
git clone --branch agent/v1-full https://github.com/Frontier-Interfaces/ActRealm.git
cd ActRealm
cargo build --workspace --release
./target/release/actrealm --version
```

The embedded Web UI requires no Node.js server or separate frontend process.

To build the native app:

```bash
apps/macos/Scripts/test.sh
apps/macos/Scripts/package-app.sh
open apps/macos/dist/ActRealm.app
```

Local source packages are QA artifacts. A public signed/notarized installer is
not available until the release workflow and clean-Mac gate pass.

## 3. Install Provider integrations

Install only Providers that are present:

```bash
./target/release/actrealm install-hooks claude
./target/release/actrealm install-hooks codex --enhanced-codex-activity
```

Installation preserves unknown/user configuration and creates private
source-aware backups before a change.

Codex has an additional mandatory trust step:

1. open a new local Codex session;
2. enter `/hooks`;
3. inspect the exact ActRealm commands;
4. trust them yourself;
5. start another new session and verify that a real event reaches ActRealm.

ActRealm never bypasses this review.

## 4. Start and verify the Runtime

For the command-line/Web path, keep this process running:

```bash
~/.actrealm/bin/actrealm serve --open
```

Use the page opened by that process. Do not open `web/index.html` directly and
do not reuse a bookmarked localhost URL after the Runtime exits.

Verify the complete installation:

```bash
~/.actrealm/bin/actrealm doctor
```

Installation is complete only when:

- the stable helper exists and runs;
- the Runtime control loop is reachable;
- the selected Hooks are installed;
- Codex trust is complete when applicable;
- a new real local Provider session reaches the UI.

“Runtime disconnected” means the current page cannot reach its Runtime. Restart
the Runtime and use the newly opened page instead of repeatedly editing Hooks.

## 5. First run and language

The first-run workspace shows factual setup state for installed Providers.
Connect, repair, refresh, and remove actions call the authenticated local setup
API; they are not placeholders.

The native app supports System Default, Simplified Chinese, and English. The
embedded Web page also supports System, Simplified Chinese, and English. The
choice is local to that client and never changes Runtime facts. Provider and
user text is never machine-translated.

## 6. OUTBOX control semantics

OUTBOX can contain:

- request-keyed permission decisions;
- Claude `AskUserQuestion`;
- Claude `Elicitation`;
- managed Codex `requestUserInput`;
- completion/error confirmations;
- observation-only Provider-native waiting states.

Allow, deny, and pass-through are the only v1 approval outcomes. Allow/deny has
a three-second undo window before the directive is written.

Provider-native `request_permissions` or `waitingOnApproval` is different from
an ActRealm reply channel. For those cards, ActRealm can show the waiting state
and open the Provider, but it must not present fake allow/deny buttons or infer
whether the user approved, denied, or executed the action.

Question answers exist only in memory while the official waiter is alive.
Secret fields use protected input and answers are not written to SQLite,
diagnostics, logs, or export.

Runtime absence, socket EOF, protocol mismatch, or deadline expiry fails open
to the Provider with empty Hook stdout. Permission requests are never spooled
or replayed.

## 7. Tasks, usage, quota, and recovery

Task cards show only facts available from the Provider/runtime safe-field
catalog. Missing plan, tool, token, model, sub-Agent, jump, or control data is
omitted or shown as unavailable; it is not invented.

Usage and quota behavior:

- cumulative and current-turn Token values remain distinct;
- context occupancy uses current-turn structured usage, not cumulative Token;
- estimated API cost uses Provider cost when supplied or a dated local pricing
  snapshot and is not a subscription bill;
- quota cards show every valid window returned by the Provider instead of
  forcing fixed weekly labels;
- a failed refresh preserves the last validated value with its real timestamp.

Manual Claude quota refresh is available in Settings. If the official
credential is expired and an official Claude CLI is available, ActRealm may
run bounded `claude auth status --json` to let Claude maintain its own
credential. ActRealm never stores a refresh token. Without a usable official
CLI, open Claude/Claude Code and complete its login, then retry.

Recovery labels are capability statements:

- **Reconnected, controllable:** a managed Codex Thread was resumed and a live
  supported reply channel exists.
- **Still running, observation only:** the Provider process is alive but
  ActRealm does not own the session.
- **History restored, waiting for a new event:** durable state exists but no
  live process/event has confirmed control.
- **Control lost:** the previously recorded Provider process is gone.
- **Ended:** the turn/session has ended.

Restart can restore durable display state, but never an old Hook stdout/RPC
waiter. A new Provider event or managed Thread reconnection is required before
control returns.

## 8. Retention, export, and backups

ActRealm stores data under:

```text
~/.actrealm/
```

Client retention choices are 30, 90, 180 days, or forever. With a finite
period, closed expired session graphs are deleted transactionally; actionable
Attention and live work are preserved. The bounded UI snapshot is separate
from full export.

Export sanitized local data:

```bash
./target/release/actrealm export > actrealm-backup.json
```

Export aggregate metrics only:

```bash
./target/release/actrealm export-metrics > actrealm-metrics.json
```

Provider configuration backups:

- include a source identity;
- use a `0700` directory and `0600` files;
- are never deleted automatically;
- expose count and total size in Settings;
- require exact `DELETE BACKUPS` confirmation for deletion.

Before deletion, ActRealm checks the complete backup directory. A symlink,
public permission, non-regular file, or unknown file causes the whole operation
to fail without partial deletion.

“Clear all local data” requires exact `DELETE`. It removes Runtime data and
settings but preserves Provider Hooks and configuration backups. Backup
deletion is intentionally a separate operation.

## 9. Local Web security

The control page is served on a random loopback port. Session and CSRF secrets
are generated from 32 bytes of OS randomness and compared in constant time.
WebSocket access uses a short-lived, single-use ticket carried through
`Sec-WebSocket-Protocol`; no bearer/CSRF token appears in the URL.

Runtime validates Origin, Cookie, CSRF, and ticket state and applies CSP and
other security response headers. Web/native clients use authenticated Runtime
APIs and never read SQLite directly.

ActRealm has no account, telemetry, cloud backend, CDN, automatic metrics
upload, automatic update, or outbound update check.

## 10. Diagnostics and troubleshooting

Enable bounded diagnostics only while reproducing a problem:

```bash
./target/release/actrealm diagnostics enable --minutes 10
./target/release/actrealm diagnostics status
./target/release/actrealm diagnostics clear
```

Diagnostics contain fixed categories and sizes, not raw prompts, full commands,
transcripts, file contents, tokens, or full local paths.

If state looks stale:

1. check Settings → Runtime monitor;
2. verify the current Runtime and WebSocket rather than an old browser page;
3. use the controlled Runtime restart when appropriate;
4. run `actrealm doctor`;
5. start a new local Provider session and look for a fresh event.

## 11. Uninstall

Remove only ActRealm-managed Hook entries:

```bash
~/.actrealm/bin/actrealm uninstall-hooks claude
~/.actrealm/bin/actrealm uninstall-hooks codex
```

Unknown/user Hook configuration is preserved. Hook uninstall does not
automatically delete Runtime data or backups.

## 12. Manual acceptance checklist

Before calling a candidate accepted:

1. first-run/setup shows real Provider state;
2. a new Claude and Codex task appears and updates while ActRealm is not key;
3. request-keyed approval works with undo;
4. Provider-native approval shows observation/open-app controls only;
5. Claude question/Elicitation and managed Codex question are answerable;
6. completion appears reliably and clears correctly;
7. elapsed time, Token/context/cost, and quota timestamps remain truthful;
8. Runtime restart restores display state without reviving old waiters;
9. Chinese/English native and Web copy switch without changing source text;
10. backup count/size and both destructive confirmations behave as documented;
11. `actrealm doctor` passes;
12. the exact packaged Helper and source revision match the tested candidate.

Commit, push, merge, tag, signing/notarization, and public release are separate
authorization decisions.

# Kimi Code and Grok Build

ActRealm observes official CLI Hooks and receives live interactions from Grok's
shared local leader and Kimi's existing desktop/web server. Explicit ACP clients
remain available for isolated sessions. Native UI enrollment and Provider integration are separate trust
boundaries: the official Provider CLI does not need ActRealm's signing identity.

## Setup

Install and sign in to the official Kimi Code or Grok Build CLI first. In
ActRealm, open Agent Setup and choose the provider's connection action. Existing
configuration is backed up before changes. Removal preserves unrelated hooks;
edited ActRealm-owned definitions are treated as conflicts.

- Kimi configuration: `~/.kimi-code/config.toml`.
- Grok configuration: `~/.grok/hooks/actrealm.json`.
- ActRealm backups: `~/.actrealm/backups/providers`.

Grok setup also enables the official `[cli] use_leader = true` preference, with
a configuration backup and ownership-aware restoration on uninstall. Thereafter
ordinary `grok` sessions keep their original terminal interface and automatically
share questions and approvals with ActRealm. Exit and resume any terminal session
that was already running before this preference changed. Explicit `--no-leader`
sessions cannot acquire this shared reply channel.

ActRealm attaches only to sessions reported resident by that same local leader.
It never resumes a dormant on-disk session into a second executing agent. Pending
questions replayed by the live leader remain answerable; historical tool replay
does not become fresh activity. Answering in either interface closes the other
interface's request. Subscriber loss returns control to the original interface
without answering, denying, cancelling or completing the provider session.

Kimi desktop/web servers are discovered from their owned local instance registry.
ActRealm uses their existing local bearer token only against loopback, with no
HTTP proxy or redirects. It reads pending requests only for already-busy sessions
and posts only explicit ActRealm answers or approval decisions. No model prompt,
account login, server start or cold-session load is performed by discovery.
Standalone Kimi TUI sessions currently require the explicit ACP command below
for direct answers; their question Hooks still raise observed attention. They
must not be presented as functionally identical to shared desktop sessions.

The normal CLI continues to report lifecycle, tool and subagent events. Kimi's
`PermissionRequest` Hook is observation-only. A Hook observation never receives
an approval or answer control unless a separate live reply channel exists.
Grok's imported Claude hook files cannot relabel Grok events as Claude events.

## Connected sessions

For an isolated ACP session, copy the connected-session command from Agent Setup
and run it from the desired project directory:

```sh
~/.actrealm/bin/actrealm agent kimi --cwd "$PWD"
~/.actrealm/bin/actrealm agent grok --cwd "$PWD"
```

Enter prompts in that terminal. Approvals and structured questions appear in
ActRealm. `/cancel` requests cancellation of the current turn; `/exit` closes
the client. Optional `--prompt`, `--model`, and `--resume` arguments support
single prompts, explicit model selection, and session continuation. Keep
`--cwd` set to the session's actual workspace when resuming.

The adapter negotiates ACP version 1, maps tool/plan notifications, and responds
only to recognized live requests for its session. One-time permission options
stay one-time: persistent allow options are not substituted for them. Grok's
question extension and Kimi form elicitation retain their own response formats.
Unknown methods and unsupported shapes are rejected rather than approved.

The existing three-second approval commit window remains undoable. Full file
targets for pending new-provider approvals are available only as transient
native-owner context; they are not persisted or exposed in the Companion field
allowlist. Answers remain memory-only. A disconnected requester loses its
pending actions; disconnection is not Provider completion. Historical replay
is not ingested as fresh execution or reused as permission to act.

Grok's plan review and MCP form elicitation use their distinct native response
formats. Full plan text is transient native-owner review context, disappears when
the live request closes, and is excluded from exports and Companion snapshots.
Missing plan context and URL-only elicitation stay in the original provider UI.

## Usage and coverage

Grok's `usage.json` session summaries are preferred over process-local ACP usage.
They already contain persisted session totals and separate turn deltas; ActRealm
does not add the same spend twice. Cache-read and cache-creation tokens are
subtracted from full prompt input before displaying disjoint buckets. Reasoning
is a subset of output, not another addition to total tokens. Provider USD ticks
are converted to integer microdollars; missing or partial cost stays unknown.

Kimi's `usage.record` wire events supply input, output and cache counters. The
bounded scanner retries partial appends and uses stable source identities across
restarts. Child usage is separately attributed to its parent. Context occupancy
from ACP is not treated as billed token usage. Model/day history remains separate
when a session changes models.

Available statistics remain visible with incomplete-history coverage. Logs can
omit interrupted runs or disappear outside ActRealm, so these adapters do not
claim complete account-wide billing or website-chat coverage.

Kimi quota refresh reads only its official managed account endpoints and stores
numeric results, never OAuth credentials. Unknown quota responses are explicitly
unavailable. Grok billing information is captured from the connected CLI when
provided; unavailable or stale values do not become a zero balance. Neither
adapter purchases credits or changes subscription settings.

## Compatibility and validation

Initial protocol probes used Kimi Code 2.0.0 and Grok Build 1.0.34, both reporting
ACP version 1. Interface references:

- [Kimi Code](https://github.com/MoonshotAI/kimi-code)
- [Kimi ACP](https://moonshotai.github.io/kimi-code/en/reference/kimi-acp.html)
- [Kimi server API](https://moonshotai.github.io/kimi-code/en/reference/server-api.html)
- [Grok Build](https://github.com/xai-org/grok-build)
- [Grok Hooks](https://docs.x.ai/build/features/hooks)
- [Grok ACP](https://docs.x.ai/build/cli/headless-scripting)

The isolated `agent_integration` example provides the same authenticated Runtime
API and request registry for acceptance work without loading unrelated accounts.
Its connection file is private test material and must not be published.

The local acceptance record covers a real Grok question, answer, file approval
with delayed commit, file result, and restart-stable numeric usage. Follow-up
shared-leader tests verify both directions: an ActRealm answer reaches the same
original client and model, and an original-client answer clears ActRealm's live
request. The earlier build 126 ACP-only acceptance did not cover ordinary CLI
questions; build 127 addresses that specific gap.
Kimi login/protocol checks succeeded, but its account reported an exhausted
monthly allowance; the user explicitly deferred real Kimi model acceptance.
Protocol, parsing, ownership, cancellation and data tests remain required.

This phase updates ActRealm. Companion presentation changes are deferred until
the ActRealm integration is accepted; no Companion upgrade is implied.

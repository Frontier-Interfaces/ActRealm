# flow-agent

Local-first attention surface for coding agents.

中文用户请从 [Flow Agent v1 中文使用教程](docs/USER_GUIDE_zh-CN.md) 开始。

Third-party asset attribution is recorded in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

The repository is an M5 release candidate under strict v1 verification. It
provides the fail-open Hook
bridge, persistent single-instance Runtime, authenticated localhost control
panel, fixed three-module web UI, safe Claude/Codex installer, first-run
onboarding, structured diagnostics, honest quota adapters, notification
settings, retention, aggregate metrics, export, and destructive local-data
clearing:

```bash
cargo run -p flow-agent -- serve --open
cargo run -p flow-agent -- install-hooks all
cargo run -p flow-agent -- doctor
cargo run -p flow-agent -- export
cargo run -p flow-agent -- export-metrics
cargo run -p flow-agent -- diagnostics status
cargo run -p flow-agent -- hook --provider claude < fixture.json
```

`serve` defaults to `--approval widget`. The explicit `prompt`, `allow`,
`deny`, and `pass-through` modes remain available for diagnostics and contract
testing.

Runtime data defaults to `~/.flow-agent`. Override it in tests or development
with `FLOW_AGENT_HOME=/path/to/data`.

`install-hooks` backs up existing configuration, preserves user and unknown
fields, writes through a lock and atomic rename, and installs a stable helper
at `~/.flow-agent/bin/flow-agent`. Codex requires a separate user-controlled
trust step: open Codex, run `/hooks`, review the exact Flow Agent commands, and
trust them. Flow Agent never edits Codex trust state or bypasses that review.

```bash
flow-agent install-hooks claude
flow-agent install-hooks codex
flow-agent install-hooks codex --enhanced-codex-activity
flow-agent uninstall-hooks all
flow-agent doctor --json
```

The onboarding UI uses the same installer implementation. It does not show
"connected" until a real provider event arrives after installation. Doctor and
automated tests do not modify the user's real Claude or Codex configuration.

The settings panel can opt into the Claude status-line quota bridge. A custom
`statusLine` is never silently replaced: the explicit **keep existing and
enable** action stores the complete original object, delegates visible output
to it, captures only the bounded quota fields, and restores it verbatim on
uninstall. A newly created Claude cache invalidates an earlier unavailable
snapshot immediately. Codex quota parsing is read-only and currently accepts
only the fixture-verified 0.144.2 desktop, 0.144.4, and 0.144.5 rollout
families. The UI always renders
the three factual slots Claude 5h, Claude 7d, and Codex week; unknown, missing,
or data older than 30 minutes remains unavailable without an invented
percentage.

Agent sessions are titled from a privacy-bounded current-task summary, show a
live activity timer/tool state, and stay in the main list only while active,
waiting for attention, or seen in the last 30 minutes. Selecting an attention
item pins and highlights its corresponding session. Local export contains the
sanitized SQLite tables; destructive clear requires the exact confirmation
`DELETE` and preserves Hook integration and backups.

Aggregate metrics never leave the machine automatically. `export-metrics`
creates a separate metrics-only JSON file containing daily counters and their
definitions, without sessions, events, attention items, commands, projects, or
paths. The settings panel shows the same factual counters.

Temporary diagnostic capture is disabled by default. It records only fixed
event categories, provider, capture time, whether a reply was required, and
payload size. It never records raw Hook bodies, session identifiers, paths,
prompts, commands, arguments, URLs, or tokens:

```bash
flow-agent diagnostics enable --minutes 10
flow-agent diagnostics status
flow-agent diagnostics clear
```

## v1 plan

- [Full v1.1 implementation plan](docs/WIDGET_V1_PLAN.md)
- [Executable milestone acceptance](docs/V1_ACCEPTANCE.md)
- [Open Vibe Island / CodeIsland reference decisions](docs/REFERENCE_REVIEW.md)
- [M0 verification record](docs/M0_VERIFICATION.md)
- [M1 verification record](docs/M1_VERIFICATION.md)
- [M2 verification record](docs/M2_VERIFICATION.md)
- [M3 verification record](docs/M3_VERIFICATION.md)
- [M4 verification record](docs/M4_VERIFICATION.md)
- [M5 verification record](docs/M5_VERIFICATION.md)
- [v1.1 functional-correction record](docs/V1_1_FUNCTIONAL_CORRECTIONS.md)

## Local quality gate

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test --workspace --offline
cargo build --workspace --release --offline
./scripts/m0-e2e.sh
./scripts/m5-resource-check.sh target/release/flow-agent
```

The common suite covers the provider path, SQLite Runtime, waiter, spool,
single-instance, restart, duplicate-request, authenticated API, UI contract,
safe install/uninstall, tri-state repair, onboarding, trust inspection, factual
task progress, quota degradation, settings/data management, and half-close
behavior. The E2E suites verify provider
directives, widget control, pass-through, silent fail-open behavior when the
Runtime is absent, and the post-install real-event verification boundary.

M5 is not complete until the browser render/memory measurement and a continuous
48-hour soak pass on the exact frozen release candidate. Until then this branch
must not be represented as a finished v1 release.

## Privacy

flow-agent is local-first and does not include telemetry or a cloud backend.
Raw prompts, transcripts, source files, survey responses, and contact details
must not be committed to this repository.

## License

MIT

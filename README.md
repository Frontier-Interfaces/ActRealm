# flow-agent

Local-first attention surface for coding agents.

中文用户请从 [Flow Agent v1 中文使用教程](docs/USER_GUIDE_zh-CN.md) 开始。

> **安装硬性前提：** 只克隆仓库或写入 Claude/Codex Hook 不算安装完成。
> `flow-agent serve --open` 必须持续运行，Hook 才能连接本机 Runtime；浏览器必须由
> 本次 `serve --open` 打开，不能复用上一次随机端口的旧页面。请按中文教程中的
> [安装完成判定与 Agent 交接指令](docs/USER_GUIDE_zh-CN.md#21-安装完成判定硬性要求)
> 执行并验收。

Third-party asset attribution is recorded in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## Native clients

ActRealm keeps one local Rust Runtime and separate native clients for each
desktop platform:

- `apps/macos/` contains the SwiftUI/AppKit client, including local foreground
  scheduling, menu-bar UI, HUD, Runtime supervision, and packaging.
- `apps/windows/` records the Windows client boundary and staged WinUI plan.
- `shared/contracts/` contains platform-neutral settings and API contracts; UI
  code is intentionally not shared across macOS and Windows.

The native clients do not own Hooks, SQLite, or Provider reply state. Those
remain in the Rust Runtime and are accessed through its authenticated loopback
API and WebSocket. See [Native client architecture](docs/NATIVE_CLIENT_ARCHITECTURE.md).

```bash
apps/macos/Scripts/test.sh
apps/macos/Scripts/package-app.sh
```

The `agent/v1-full` branch contains functional implementation through **M13**;
see the [current status](docs/STATUS.md) for the exact milestone, acceptance,
branch, and release state. It remains a v1 test candidate because the separate
48-hour Runtime stability gate has not passed. The branch provides the
fail-open Hook bridge, persistent single-instance Runtime, authenticated localhost control
panel, fixed three-module web UI, configurable safe task fields and detail
drawer, direct Claude question forms, an explicit version-gated Codex
app-server Connector with Thread recovery, safe Claude/Codex installer, first-run
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
at `~/.flow-agent/bin/flow-agent`. A global provider CLI is not required when
Claude.app or ChatGPT/Codex.app is installed: the installer discovers the
desktop app and, for Codex, exposes its bundled official executable for the
manual trust review. Codex still requires a separate user-controlled trust
step: start the discovered Codex executable, run `/hooks`, review the exact
Flow Agent commands, and trust them. Flow Agent never edits Codex trust state
or bypasses that review.

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
snapshot immediately. Codex quota parsing is read-only and structurally
validates the bounded `rate_limits` record instead of hard-coding an account
period or patch version. The UI renders every valid window returned by the
source (for example 5 hours, 7 days, 30 days, or a named extra allowance).
Values older than 30 minutes remain visible as the last valid sample with their
capture time; Flow Agent never turns age into a fabricated percentage.

Agent sessions use the Provider's own local conversation title as the visible
main title: Claude's official `session_title` plus bounded `custom-title` /
`ai-title` compatibility records, or Codex's latest `thread_name` from its
bounded local session index. The privacy-bounded current question is the plain
second line with no synthetic prefix, followed only by the current model when
the Provider supplies one. Project, Provider name, title provenance, and Token
figures are not mixed into these three visible title lines. Only the resolved
title and its source are persisted; transcript content and paths never enter
the browser snapshot. Metadata refreshes while recent sessions are visible and
falls back honestly to the current-question summary when no Provider title is
available. Sessions also show total turn time and current phase. Running state
survives a Runtime restart; completed/idle sessions stay in the main list for
30 minutes.
Attention handled in the original Agent is reconciled automatically. Session
rows expose one of four honest jump levels: exact Codex conversation, matching
Terminal/iTerm session, application only, or unsupported. Local export contains
the sanitized SQLite tables; destructive clear requires the exact confirmation
`DELETE` and preserves Hook integration and backups.

Approval UI is capability-specific. A request with a live Flow Agent reply
channel can be allowed, denied, or passed through. A Provider-native approval
observed through `request_permissions` or managed Thread status is shown only
as an original-interface request: Flow Agent synchronizes waiting/resolved
state and never invents allow/deny controls or an approval outcome.

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

- [Current development and release status](docs/STATUS.md)
- [Development changelog](CHANGELOG.md)
- [Full v1.1 implementation plan](docs/WIDGET_V1_PLAN.md)
- [Executable milestone acceptance](docs/V1_ACCEPTANCE.md)
- [Open Vibe Island / CodeIsland reference decisions](docs/REFERENCE_REVIEW.md)
- [M0 verification record](docs/M0_VERIFICATION.md)
- [M1 verification record](docs/M1_VERIFICATION.md)
- [M2 verification record](docs/M2_VERIFICATION.md)
- [M3 verification record](docs/M3_VERIFICATION.md)
- [M4 verification record](docs/M4_VERIFICATION.md)
- [M5 verification record](docs/M5_VERIFICATION.md)
- [M6 verification record](docs/M6_VERIFICATION.md)
- [M7 verification record](docs/M7_VERIFICATION.md)
- [M8 verification record](docs/M8_VERIFICATION.md)
- [M9 verification record](docs/M9_VERIFICATION.md)
- [M10-M12 verification record](docs/M10_M12_VERIFICATION.md)
- [M13 Provider-state verification record](docs/M13_PROVIDER_STATE_COORDINATION.md)
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

The common suite currently covers 153 passing tests plus two explicit/manual
tests ignored by default. It covers the provider path, SQLite Runtime, waiter, spool,
single-instance, restart, duplicate-request, authenticated API, UI contract,
safe install/uninstall, tri-state repair, onboarding, trust inspection, factual
task progress, quota degradation, settings/data management, and half-close
behavior. The E2E suites verify provider
directives, widget control, pass-through, silent fail-open behavior when the
Runtime is absent, and the post-install real-event verification boundary.

The short browser/resource measurements pass, but M5 release qualification is
not complete until a continuous 48-hour Runtime RSS soak passes on the exact
frozen release candidate. M13's real-Provider manual acceptance is also still
pending. Until both are recorded, this branch must not be represented as a
finished v1 release.

## Privacy

flow-agent is local-first and does not include telemetry or a cloud backend.
Raw prompts, transcripts, source files, survey responses, and contact details
must not be committed to this repository.

## License

MIT

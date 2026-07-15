# flow-agent

Local-first attention surface for coding agents.

The repository is under active v1 development. M2 provides the fail-open Hook
bridge, persistent single-instance Runtime, authenticated localhost control
panel, and fixed three-module web UI:

```bash
cargo run -p flow-agent -- serve --open
cargo run -p flow-agent -- hook --provider claude < fixture.json
```

`serve` defaults to `--approval widget`. The explicit `prompt`, `allow`,
`deny`, and `pass-through` modes remain available for diagnostics and contract
testing.

Runtime data defaults to `~/.flow-agent`. Override it in tests or development
with `FLOW_AGENT_HOME=/path/to/data`.

## v1 plan

- [Full v1.1 implementation plan](docs/WIDGET_V1_PLAN.md)
- [Executable milestone acceptance](docs/V1_ACCEPTANCE.md)
- [Open Vibe Island / CodeIsland reference decisions](docs/REFERENCE_REVIEW.md)
- [M0 verification record](docs/M0_VERIFICATION.md)
- [M1 verification record](docs/M1_VERIFICATION.md)
- [M2 verification record](docs/M2_VERIFICATION.md)

## Local quality gate

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test --workspace --offline
cargo build --workspace --release --offline
./scripts/m0-e2e.sh
```

The common suite covers the provider path, SQLite Runtime, waiter, spool,
single-instance, restart, duplicate-request, authenticated API, UI contract,
and half-close behavior. The E2E suites verify provider directives, widget
control, pass-through, and silent fail-open behavior when the Runtime is
absent.

## Privacy

flow-agent is local-first and does not include telemetry or a cloud backend.
Raw prompts, transcripts, source files, survey responses, and contact details
must not be committed to this repository.

## License

MIT

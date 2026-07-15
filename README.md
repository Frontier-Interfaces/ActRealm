# flow-agent

Local-first attention surface for coding agents.

The repository is under active v1 development. The first executable slice is
the fail-open hook bridge:

```bash
cargo run -p flow-agent -- serve --approval prompt
cargo run -p flow-agent -- hook --provider claude < fixture.json
```

Runtime data defaults to `~/.flow-agent`. Override it in tests or development
with `FLOW_AGENT_HOME=/path/to/data`.

## v1 plan

- [Full v1.1 implementation plan](docs/WIDGET_V1_PLAN.md)
- [Executable milestone acceptance](docs/V1_ACCEPTANCE.md)
- [Open Vibe Island / CodeIsland reference decisions](docs/REFERENCE_REVIEW.md)
- [M0 verification record](docs/M0_VERIFICATION.md)

## Local quality gate

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test --workspace --offline
cargo build --workspace --release --offline
./scripts/m0-e2e.sh
```

The E2E script verifies Claude approval, Codex denial, and silent fail-open
behavior when the runtime is absent.

## Privacy

flow-agent is local-first and does not include telemetry or a cloud backend.
Raw prompts, transcripts, source files, survey responses, and contact details
must not be committed to this repository.

## License

MIT

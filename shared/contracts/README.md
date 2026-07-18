# Shared contracts

This directory contains stable, platform-neutral contracts consumed by native
clients. It is not a shared UI or shared application-state implementation.

- `foreground-scheduling.schema.json` describes the persisted policy shared by
  the macOS and future Windows clients.
- Runtime HTTP/WebSocket payloads remain defined by Rust types and API tests.
  When a native client needs a frozen contract, add a sanitized fixture and a
  schema here in the same change as the Runtime API update.

Contract changes require compatibility tests in every implemented client.

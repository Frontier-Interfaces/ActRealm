# Shared contracts

This directory contains stable, platform-neutral contracts consumed by native
clients. It is not a shared UI or shared application-state implementation.

- `foreground-scheduling.schema.json` describes the persisted policy shared by
  the macOS and future Windows clients.
- `runtime-messages.json` is the machine-readable registry for Runtime message
  codes, parameters, and reference English/Simplified Chinese translations.
- `api-errors.json` is the machine-readable registry for stable Runtime and
  local-client error codes plus reference English/Simplified Chinese wording.
- `fact-envelope.schema.json` freezes the additive session fact metadata used
  by Runtime, macOS, Web, and Companion. It explains source, freshness,
  verification, absence, and control capability without copying Provider raw
  payloads or changing the existing fact values.
- `runtime-diagnostics.schema.json` freezes the authenticated local H7 control-
  plane status. It permits only bounded Runtime identity, snapshot freshness,
  SQLite schema/integrity, collector state, Companion scopes, and neutral
  conditional-feature state; paths, prompts, commands, credentials, tokens,
  transcripts, file contents, and Provider reply channels are excluded.

All current APIs use authenticated local Runtime connections. Provider capability
and reply-channel checks also apply to local Companion requests.

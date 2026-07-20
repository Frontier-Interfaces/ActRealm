# Security

ActRealm processes local coding-agent events and permission decisions. Do
not include secrets, source files, raw transcripts, or personal survey data in
bug reports.

Until a private reporting channel is published, please contact the maintainer
directly rather than opening a public issue for suspected vulnerabilities.

The v1 security baseline is:

- local-only transports;
- fail-open hooks when the runtime is unavailable;
- user-private runtime directories and sockets;
- Hook bodies capped at 256 KiB, bridge frames capped at 320 KiB, and local API
  request bodies capped at 64 KiB;
- category-only persisted command previews with arguments, URLs, tokens, paths,
  raw session identifiers, and raw Hook bodies excluded;
- opt-in diagnostic capture disabled by default, limited to 1-60 minutes,
  fixed-field redacted records, private files, symlink refusal, a 1 MiB cap,
  automatic expiry, and explicit clearing;
- aggregate-only metrics export separate from full local backup, with no
  automatic sharing;
- no telemetry or cloud service.

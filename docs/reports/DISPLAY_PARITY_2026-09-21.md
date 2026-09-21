# Display connection observation and official provider icons

The authenticated local Companion projection now includes the optional
`managedConnectionState` already computed by Runtime. Display can distinguish
automatic connection, retry, unavailable service and original-writer ownership.
This is observation only: it grants no response scope and does not replace a
live request channel. The existing approval regression now asserts projection
parity and the absence of response authority for a read-only client.

Grok and Kimi avatars use original PNG assets from their official websites in
native and web clients. The native package includes both files; web routes serve
them as PNGs. Source URLs and SHA-256 values are in
`../../web/assets/PROVIDER_ICON_SOURCES.md`.

Display's separate local candidate adds the matching source/connection labels,
correct Codex most-recent-call wording, four-provider identity and a Runtime
usage summary with explicit historical completeness. Display source remains in
its own repository; this change does not merge or release either product.

## Validation

450 Rust tests passed (3 intentionally ignored), 223 Swift tests and 14 web
tests passed. Formatting, all-target Clippy with warnings denied, release,
localization, CI pins, Info.plist and diff checks passed. A first test run hit a
fake-process initialization timeout during slow macOS executable launches;
the unchanged connector tests and full final workspace rerun passed without
relaxing production deadlines.

Installed ActRealm build 136 passed strict signatures, helper hash and native
UI verification of both official logos. A pinned-TLS Companion probe confirmed
`managedConnectionState: owned_elsewhere` after update, where the old projection
omitted the field. The active original task remained running and its token
count kept increasing. Historical aggregation remains explicitly partial.
Build 136 was packaged before this submission and records its base commit and
source-modified marker. Full-machine reboot and long soak were not performed.

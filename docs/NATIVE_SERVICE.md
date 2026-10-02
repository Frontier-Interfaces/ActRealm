# Independent native Runtime service

ActRealm and Display are peers of one per-user service. Neither app owns the
other app's credentials, and neither app needs to open the other's UI.

## Lifecycle

The signed Runtime helper implements `service ensure`, `service status`, and
`service restart`. It installs the shared executable under `~/.actrealm/bin`
and a user LaunchAgent named `com.frontierinterfaces.actrealm.runtime`.
The service runs in the logged-in user's session, survives closing either app,
and is restarted by launchd after an unexpected process exit. Its stdout/stderr
log is private, and service mode never prints a bootstrap token.

`ensure` serializes installation, reuses a healthy compatible Runtime, and does
not kill another app's process to obtain credentials. An older pre-v7 Runtime
must be stopped by its owning app before migration. Updating a packaged Runtime
is an explicit installation operation; application launch order never selects
an arbitrary replacement for a healthy service. User background-service
restrictions remain effective.

## Native connection

The apps use the private `~/.actrealm/run/native-clients.sock` socket. Both ends
verify the same user and the kernel-supplied audit token, then validate the
live peer's code signature and Hardened Runtime flag. Runtime accepts only these signing identifiers
from its own developer team:

- `com.frontierinterfaces.actrealm`
- `com.mmx.animation-display-demo`

Clients require the peer identifier `com.frontierinterfaces.actrealm.runtime`.
Application names or PIDs supplied in JSON confer no authority. Unsigned,
unknown, or differently signed processes cannot enroll; there is no environment
variable or public HTTP endpoint that bypasses this verification. Native mode
requires correctly signed app and helper packages; unsigned source/CI packages
must be signed before their native connection can be used.

A bounded schema-v1 request may carry an existing token for migration and an
explicit `enableAccess` action. The response contains the current instance,
HTTPS endpoint, ephemeral certificate SHA-256 pin, and client credentials.
The native API and WebSocket use pinned HTTPS/WSS. Certificates and private
keys are created in memory for each Runtime instance; no CA is installed in
system trust. Clients reject certificate mismatches and redirects. Discovery
files contain metadata only and cannot redirect a saved token to another server.

ActRealm receives an independent cookie/CSRF pair. Display receives its data
access token plus a separate session limited to Agent setup. Setup writes also
require the Display grant's control scope. Display cannot use that session for
unrelated administrative APIs. Existing request IDs, reply channels, expiry,
read-only behavior and approval undo rules remain enforced by Runtime.

## Migration and revocation

Existing valid Display tokens and scopes are retained. The existing hashed
`companion-auth.json` format remains readable; native policy is kept separately
in `native-client-policy.json`. A revoked or missing native registration cannot
be revived by restarting the service or removing the Keychain entry. Restoring
access requires the app's explicit Enable action.

Native apps no longer present pairing codes or cross-app authorization pages.
Legacy AR1 HTTP endpoints remain only for compatibility and tests; native apps
do not use them. Existing task history, usage accounting and Hook configuration
are not erased or reinstalled by this migration.

## Validation

The native regression suite covers independent sessions, CSRF binding,
least-privilege setup access, revocation, and read-only migration. Signed local
integration probes additionally cover process identity, forged peers, TLS pin
acceptance/rejection, and Runtime restart. Final installed-app lifecycle checks
must cover both launch orders, simultaneous clients, client exit, and service
crash recovery. The deployment record and PR validation identify the exact
verified package and commits.

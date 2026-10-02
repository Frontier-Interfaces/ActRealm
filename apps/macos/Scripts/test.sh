#!/bin/zsh
set -euo pipefail

SCRIPT_DIR="${0:a:h}"
PACKAGE_DIR="${SCRIPT_DIR:h}"
export SDKROOT="$("$SCRIPT_DIR/resolve-sdk.sh")"
# GitHub-hosted macOS runners use UTC. Default local tests to the same zone so
# date-sensitive regressions fail before a push; individual tests that need a
# user zone must inject one explicitly.
export TZ="${TZ:-UTC}"

swift test --disable-sandbox --package-path "$PACKAGE_DIR" "$@"

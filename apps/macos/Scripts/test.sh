#!/bin/zsh
set -euo pipefail

SCRIPT_DIR="${0:a:h}"
PACKAGE_DIR="${SCRIPT_DIR:h}"
export SDKROOT="$("$SCRIPT_DIR/resolve-sdk.sh")"

swift test --disable-sandbox --package-path "$PACKAGE_DIR" "$@"

#!/bin/zsh
# Packages ActRealm.app: SwiftUI frontend + the Rust flow-agent backend
# bundled as Contents/Helpers/flow-agent.
#
# Usage: Scripts/package-app.sh [output-dir]
#   FLOW_AGENT_REPO   path to the Rust checkout (default: monorepo root)
#   FLOW_AGENT_SKIP_RUST_BUILD=1 reuses the existing release backend binary
#   FLOW_AGENT_SWIFT_CONFIGURATION=debug packages a debug Swift build
#   SDKROOT           macOS SDK (default: installed 26.0 SDK, then xcrun)
set -euo pipefail

SCRIPT_DIR="${0:a:h}"
PACKAGE_DIR="${SCRIPT_DIR:h}" # apps/macos/
REPO_ROOT="${PACKAGE_DIR:h:h}"
RUST_REPO="${FLOW_AGENT_REPO:-$REPO_ROOT}"
SWIFT_CONFIGURATION="${FLOW_AGENT_SWIFT_CONFIGURATION:-release}"
if [[ "$SWIFT_CONFIGURATION" == "release" ]]; then
  SWIFT_DEBUG_INFO="${FLOW_AGENT_SWIFT_DEBUG_INFO:-none}"
else
  SWIFT_DEBUG_INFO="${FLOW_AGENT_SWIFT_DEBUG_INFO:-dwarf}"
fi
OUT_DIR="${1:-$PACKAGE_DIR/dist}"
APP="$OUT_DIR/ActRealm.app"
INFO_PLIST="$PACKAGE_DIR/Resources/Info.plist"
if [[ -z "${SDKROOT:-}" ]]; then
  export SDKROOT="$("$SCRIPT_DIR/resolve-sdk.sh")"
fi

if [[ ! -f "$RUST_REPO/Cargo.toml" ]]; then
  echo "error: flow-agent checkout not found at $RUST_REPO" >&2
  echo "       set FLOW_AGENT_REPO=/path/to/flow-agent" >&2
  exit 1
fi
[[ -f "$INFO_PLIST" ]] || { echo "error: missing $INFO_PLIST" >&2; exit 1; }
if [[ "${FLOW_AGENT_SKIP_RUST_BUILD:-0}" == "1" ]]; then
  echo "==> Reusing existing Rust backend (release)"
  [[ -x "$RUST_REPO/target/release/flow-agent" ]] || {
    echo "error: reusable Rust backend not found at $RUST_REPO/target/release/flow-agent" >&2
    exit 1
  }
else
  echo "==> Building Rust backend (release)"
  (cd "$RUST_REPO" && cargo build --release -p flow-agent)
fi

echo "==> Building SwiftUI app ($SWIFT_CONFIGURATION)"
(cd "$PACKAGE_DIR" && swift build --disable-sandbox -c "$SWIFT_CONFIGURATION" -debug-info-format "$SWIFT_DEBUG_INFO" --product ActRealmApp)
SWIFT_BIN="$(cd "$PACKAGE_DIR" && swift build --disable-sandbox -c "$SWIFT_CONFIGURATION" -debug-info-format "$SWIFT_DEBUG_INFO" --product ActRealmApp --show-bin-path)/ActRealmApp"
if [[ ! -x "$SWIFT_BIN" ]]; then
  # The swiftbuild backend lays products out differently; fall back to a search.
  SWIFT_BIN="$(find "$PACKAGE_DIR/.build" -type f -name ActRealmApp -perm +111 2>/dev/null | grep -i "$SWIFT_CONFIGURATION" | head -1)"
fi
[[ -x "$SWIFT_BIN" ]] || { echo "error: built ActRealmApp binary not found" >&2; exit 1; }

echo "==> Assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Helpers" "$APP/Contents/Resources"

cp "$SWIFT_BIN" "$APP/Contents/MacOS/ActRealm"
cp "$RUST_REPO/target/release/flow-agent" "$APP/Contents/Helpers/flow-agent"
cp "$INFO_PLIST" "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"

echo "==> Codesigning (ad-hoc)"
codesign --force -s - "$APP/Contents/Helpers/flow-agent"
codesign --force -s - "$APP/Contents/MacOS/ActRealm"
codesign --force -s - "$APP"

echo "==> Done: $APP"
echo "    open \"$APP\""

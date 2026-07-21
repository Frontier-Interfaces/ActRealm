#!/bin/zsh
set -euo pipefail

if [[ -n "${SDKROOT:-}" ]]; then
  print -r -- "$SDKROOT"
  exit 0
fi

ACTIVE_DEVELOPER_DIR="${DEVELOPER_DIR:-}"
if [[ -z "$ACTIVE_DEVELOPER_DIR" ]]; then
  ACTIVE_DEVELOPER_DIR="$(xcode-select -p 2>/dev/null || true)"
fi

if [[ "$ACTIVE_DEVELOPER_DIR" == *.app/Contents/Developer ]]; then
  DEVELOPER_DIR="$ACTIVE_DEVELOPER_DIR" xcrun --sdk macosx --show-sdk-path
  exit 0
fi

PINNED_SDK="/Library/Developer/CommandLineTools/SDKs/MacOSX26.0.sdk"
if [[ -d "$PINNED_SDK" ]]; then
  print -r -- "$PINNED_SDK"
else
  xcrun --sdk macosx --show-sdk-path
fi

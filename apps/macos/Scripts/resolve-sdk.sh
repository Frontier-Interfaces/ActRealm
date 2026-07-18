#!/bin/zsh
set -euo pipefail

if [[ -n "${SDKROOT:-}" ]]; then
  print -r -- "$SDKROOT"
  exit 0
fi

PINNED_SDK="/Library/Developer/CommandLineTools/SDKs/MacOSX26.0.sdk"
if [[ -d "$PINNED_SDK" ]]; then
  print -r -- "$PINNED_SDK"
else
  xcrun --sdk macosx --show-sdk-path
fi

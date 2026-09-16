#!/bin/zsh

set -euo pipefail
export COPYFILE_DISABLE=1

REPO_DIR="${0:A:h:h}"
OUTPUT_PATH="${1:-$REPO_DIR/outputs/runtime-macos/actrealm}"

if [[ "$OUTPUT_PATH" != /* ]]; then
  print -u2 "error: output path must be absolute: $OUTPUT_PATH"
  exit 1
fi

cd "$REPO_DIR"
cargo build --release -p actrealm

SOURCE_BINARY="$REPO_DIR/target/release/actrealm"
if [[ ! -f "$SOURCE_BINARY" || -L "$SOURCE_BINARY" || ! -x "$SOURCE_BINARY" ]]; then
  print -u2 "error: release Runtime is missing or not executable: $SOURCE_BINARY"
  exit 1
fi

mkdir -p "${OUTPUT_PATH:h}"
cp "$SOURCE_BINARY" "$OUTPUT_PATH"
chmod 0755 "$OUTPUT_PATH"

print "ActRealm Runtime: $OUTPUT_PATH"
shasum -a 256 "$OUTPUT_PATH"

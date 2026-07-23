#!/bin/sh
set -eu

repo="$(cd "$(dirname "$0")/../../../.." && pwd)"
fixture="$repo/fixtures/codex/0.144.4/user-prompt-submit.json"
binary="/tmp/actrealm-global-audit-50b891a/target/release/actrealm"
socket="/tmp/actrealm-stress-20260722-1617/bridge.sock"

send_one() {
  index="$1"
  jq --arg index "$index" \
    '.session_id = ("stress-" + $index) |
     .turn_id = ("turn-" + $index) |
     .prompt = ("Stress event " + $index)' \
    "$fixture" |
    "$binary" hook --provider codex --socket "$socket" >/dev/null
}

export fixture binary socket
export -f send_one 2>/dev/null || true

# POSIX sh does not export functions portably, so invoke this file recursively.
if [ "${1:-}" = "--one" ]; then
  send_one "$2"
  exit 0
fi

seq 1 500 | xargs -P 16 -I '{}' "$0" --one '{}'

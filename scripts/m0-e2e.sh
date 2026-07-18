#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
BIN="$ROOT/target/release/actrealm"
TMP_ROOT="${TMPDIR:-/private/tmp}/actrealm-m0-e2e-$$"
SOCKET="$TMP_ROOT/bridge.sock"
SERVER_LOG="$TMP_ROOT/server.log"
DATABASE="$TMP_ROOT/bridge.sqlite"
LOCK_FILE="$TMP_ROOT/bridge.lock"
SERVER_PID=""

cleanup() {
    if [ -n "$SERVER_PID" ]; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    rm -f "$SOCKET" "$SERVER_LOG" "$DATABASE" "$DATABASE-shm" "$DATABASE-wal" "$LOCK_FILE"
    rmdir "$TMP_ROOT" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

wait_for_socket() {
    attempts=0
    while [ ! -S "$SOCKET" ]; do
        attempts=$((attempts + 1))
        if [ "$attempts" -ge 100 ]; then
            echo "runtime socket did not become ready" >&2
            exit 1
        fi
        sleep 0.05
    done
}

start_server() {
    mode=$1
    mkdir -p "$TMP_ROOT"
    ACTREALM_COMMIT_DELAY_MS=0 \
        "$BIN" serve --approval "$mode" --socket "$SOCKET" >"$SERVER_LOG" 2>&1 &
    SERVER_PID=$!
    wait_for_socket
}

stop_server() {
    kill "$SERVER_PID"
    wait "$SERVER_PID" 2>/dev/null || true
    SERVER_PID=""
    rm -f "$SOCKET"
}

start_server allow
allow_output=$("$BIN" hook --provider claude --socket "$SOCKET" \
    <"$ROOT/fixtures/claude/2.1.210/permission-request.json")
case "$allow_output" in
    *'"behavior":"allow"'*) ;;
    *) echo "Claude allow directive mismatch: $allow_output" >&2; exit 1 ;;
esac
case "$allow_output" in
    *'"continue"'*) echo "Claude directive was not minimal: $allow_output" >&2; exit 1 ;;
    *) ;;
esac
stop_server

start_server deny
deny_output=$("$BIN" hook --provider codex --socket "$SOCKET" \
    <"$ROOT/fixtures/codex/0.144.4/permission-request.json")
case "$deny_output" in
    *'"behavior":"deny"'*) ;;
    *) echo "Codex deny directive mismatch: $deny_output" >&2; exit 1 ;;
esac
case "$deny_output" in
    *'"continue"'*) echo "Codex directive contains unsupported continue: $deny_output" >&2; exit 1 ;;
    *) ;;
esac
stop_server

start_server pass-through
pass_output=$("$BIN" hook --provider codex --socket "$SOCKET" \
    <"$ROOT/fixtures/codex/0.144.4/permission-request.json")
if [ -n "$pass_output" ]; then
    echo "pass-through hook unexpectedly wrote to stdout: $pass_output" >&2
    exit 1
fi
stop_server

fail_open_output=$("$BIN" hook --provider codex \
    --socket "$TMP_ROOT/missing.sock" \
    <"$ROOT/fixtures/codex/0.144.4/permission-request.json")
if [ -n "$fail_open_output" ]; then
    echo "fail-open hook unexpectedly wrote to stdout: $fail_open_output" >&2
    exit 1
fi

echo "M0 E2E passed: Claude allow, Codex deny, pass-through, missing-runtime fail-open"

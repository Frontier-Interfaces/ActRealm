#!/bin/sh
set -eu

binary=${1:-target/release/flow-agent}
duration=${FLOW_AGENT_RESOURCE_DURATION_SECONDS:-15}
interval=${FLOW_AGENT_RESOURCE_INTERVAL_SECONDS:-1}
report=${FLOW_AGENT_RESOURCE_REPORT:-/tmp/flow-agent-m5-resource-report.json}
root=${TMPDIR:-/tmp}/flow-agent-m5-resource-$$
socket=$root/bridge.sock
samples=$root/samples.txt

mkdir -p "$root/home" "$root/codex"

cleanup() {
  if [ -n "${runtime_pid:-}" ]; then
    kill "$runtime_pid" 2>/dev/null || true
    wait "$runtime_pid" 2>/dev/null || true
  fi
  rm -rf "$root"
}
trap cleanup EXIT INT TERM

HOME=$root/home \
CODEX_HOME=$root/codex \
FLOW_AGENT_HOME=$root/flow \
"$binary" serve --approval widget --socket "$socket" >"$root/runtime.log" 2>&1 &
runtime_pid=$!

attempt=0
while [ ! -S "$socket" ]; do
  attempt=$((attempt + 1))
  if [ "$attempt" -gt 100 ] || ! kill -0 "$runtime_pid" 2>/dev/null; then
    echo "runtime failed to become ready" >&2
    exit 1
  fi
  sleep 0.05
done

sleep 2
started=$(date +%s)
deadline=$((started + duration))
while [ "$(date +%s)" -lt "$deadline" ]; do
  if ! kill -0 "$runtime_pid" 2>/dev/null; then
    echo "runtime exited during resource check" >&2
    exit 1
  fi
  ps -p "$runtime_pid" -o %cpu= -o rss= >>"$samples"
  sleep "$interval"
done

set -- $(awk '
  { cpu += $1; if ($2 > rss) rss = $2; count += 1 }
  END {
    if (count == 0) exit 1;
    printf "%.3f %.0f %d", cpu / count, rss, count
  }
' "$samples")
cpu_average=$1
rss_max_kib=$2
sample_count=$3

awk -v cpu="$cpu_average" 'BEGIN { exit !(cpu < 0.5) }' || {
  echo "idle CPU budget failed: $cpu_average%" >&2
  exit 1
}
awk -v rss="$rss_max_kib" 'BEGIN { exit !(rss < 81920) }' || {
  echo "runtime RSS budget failed: ${rss_max_kib} KiB" >&2
  exit 1
}

mkdir -p "$(dirname "$report")"
printf '{"schemaVersion":1,"durationSeconds":%s,"sampleCount":%s,"idleCpuAveragePct":%s,"runtimeRssMaxKiB":%s,"cpuBudgetPct":0.5,"rssBudgetKiB":81920}\n' \
  "$duration" "$sample_count" "$cpu_average" "$rss_max_kib" >"$report"
cat "$report"

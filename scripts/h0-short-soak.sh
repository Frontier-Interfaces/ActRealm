#!/bin/sh
set -eu

duration=${ACTREALM_H0_DURATION_SECONDS:-600}
interval=${ACTREALM_H0_INTERVAL_SECONDS:-5}
report_dir=${ACTREALM_H0_REPORT_DIR:-/tmp/actrealm-h0-short-soak-$$}
actrealm_data_home=${ACTREALM_HOME:-$HOME/.actrealm}
database=$actrealm_data_home/data.sqlite
samples=$report_dir/samples.csv
summary=$report_dir/summary.json

mkdir -p "$report_dir"

app_pid=$(pgrep -f '^/Applications/ActRealm.app/Contents/MacOS/ActRealm$' | head -n 1 || true)
if [ -z "$app_pid" ]; then
  echo "ActRealm macOS app is not running" >&2
  exit 1
fi

runtime_pid=$(pgrep -P "$app_pid" -f '/Applications/ActRealm.app/Contents/Helpers/actrealm serve' | head -n 1 || true)
if [ -z "$runtime_pid" ]; then
  echo "ActRealm Runtime helper is not running" >&2
  exit 1
fi

if [ ! -f "$database" ]; then
  echo "ActRealm database was not found: $database" >&2
  exit 1
fi

integrity_before=$(sqlite3 -readonly "$database" 'PRAGMA integrity_check;')
if [ "$integrity_before" != "ok" ]; then
  echo "SQLite integrity check failed before soak: $integrity_before" >&2
  exit 1
fi

printf '%s\n' 'timestamp,app_pid,app_cpu_pct,app_rss_kib,app_fds,runtime_pid,runtime_cpu_pct,runtime_rss_kib,runtime_fds,db_bytes,wal_bytes,canonical_tokens,daily_tokens,model_tokens' >"$samples"

started_at=$(date +%s)
deadline=$((started_at + duration))
while [ "$(date +%s)" -lt "$deadline" ]; do
  if ! kill -0 "$app_pid" 2>/dev/null; then
    echo "ActRealm macOS app exited during H0 soak" >&2
    exit 1
  fi
  if ! kill -0 "$runtime_pid" 2>/dev/null; then
    echo "ActRealm Runtime exited during H0 soak" >&2
    exit 1
  fi

  set -- $(ps -p "$app_pid" -o %cpu= -o rss=)
  app_cpu=$1
  app_rss=$2
  set -- $(ps -p "$runtime_pid" -o %cpu= -o rss=)
  runtime_cpu=$1
  runtime_rss=$2

  app_fds=$(lsof -p "$app_pid" 2>/dev/null | wc -l | tr -d ' ')
  runtime_fds=$(lsof -p "$runtime_pid" 2>/dev/null | wc -l | tr -d ' ')
  db_bytes=$(stat -f '%z' "$database")
  wal_bytes=0
  if [ -f "$database-wal" ]; then
    wal_bytes=$(stat -f '%z' "$database-wal")
  fi

  totals=$(sqlite3 -readonly -separator ',' "$database" '
    PRAGMA busy_timeout=5000;
    SELECT
      COALESCE((SELECT SUM(token_total) FROM token_usage_session_days), 0),
      COALESCE((SELECT SUM(token_total) FROM token_usage_daily), 0),
      COALESCE((SELECT SUM(token_total) FROM token_usage_daily_models), 0);
  ' | tail -n 1)

  printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
    "$(date +%s)" "$app_pid" "$app_cpu" "$app_rss" "$app_fds" \
    "$runtime_pid" "$runtime_cpu" "$runtime_rss" "$runtime_fds" \
    "$db_bytes" "$wal_bytes" "$totals" >>"$samples"

  sleep "$interval"
done

integrity_after=$(sqlite3 -readonly "$database" 'PRAGMA integrity_check;')
if [ "$integrity_after" != "ok" ]; then
  echo "SQLite integrity check failed after soak: $integrity_after" >&2
  exit 1
fi

set -- $(awk -F, '
  NR == 2 {
    app_start = $4; runtime_start = $8; db_start = $10; wal_start = $11;
    app_fds_start = $5; runtime_fds_start = $9;
  }
  NR > 1 {
    count += 1;
    app_cpu += $3; runtime_cpu += $7;
    if ($4 > app_max) app_max = $4;
    if ($8 > runtime_max) runtime_max = $8;
    if ($5 > app_fds_max) app_fds_max = $5;
    if ($9 > runtime_fds_max) runtime_fds_max = $9;
    if ($12 != $13 || $12 != $14) mismatch += 1;
    app_end = $4; runtime_end = $8; db_end = $10; wal_end = $11;
    app_fds_end = $5; runtime_fds_end = $9;
  }
  END {
    if (count == 0) exit 1;
    printf "%d %.3f %.0f %.0f %.0f %.3f %.0f %.0f %.0f %.0f %.0f %.0f %.0f %.0f %.0f %.0f %.0f %.0f %d", \
      count, app_cpu / count, app_start, app_max, app_end,
      runtime_cpu / count, runtime_start, runtime_max, runtime_end,
      app_fds_start, app_fds_max, app_fds_end,
      runtime_fds_start, runtime_fds_max, runtime_fds_end,
      db_start, db_end, wal_end, mismatch;
  }
' "$samples")

sample_count=$1
app_cpu_average=$2
app_rss_start=$3
app_rss_max=$4
app_rss_end=$5
runtime_cpu_average=$6
runtime_rss_start=$7
runtime_rss_max=$8
runtime_rss_end=$9
shift 9
app_fds_start=$1
app_fds_max=$2
app_fds_end=$3
runtime_fds_start=$4
runtime_fds_max=$5
runtime_fds_end=$6
db_bytes_start=$7
db_bytes_end=$8
wal_bytes_end=$9
shift 9
ledger_mismatch_count=$1

if [ "$ledger_mismatch_count" -ne 0 ]; then
  echo "Token ledger mismatch detected in $ledger_mismatch_count samples" >&2
  exit 1
fi

if [ "$runtime_rss_max" -ge 81920 ]; then
  echo "Runtime RSS budget failed: ${runtime_rss_max} KiB" >&2
  exit 1
fi

ended_at=$(date +%s)
printf '%s\n' \
  "{\"schemaVersion\":1,\"startedAt\":$started_at,\"endedAt\":$ended_at,\"requestedDurationSeconds\":$duration,\"sampleIntervalSeconds\":$interval,\"sampleCount\":$sample_count,\"app\":{\"pid\":$app_pid,\"cpuAveragePct\":$app_cpu_average,\"rssStartKiB\":$app_rss_start,\"rssMaxKiB\":$app_rss_max,\"rssEndKiB\":$app_rss_end,\"fdsStart\":$app_fds_start,\"fdsMax\":$app_fds_max,\"fdsEnd\":$app_fds_end},\"runtime\":{\"pid\":$runtime_pid,\"cpuAveragePct\":$runtime_cpu_average,\"rssStartKiB\":$runtime_rss_start,\"rssMaxKiB\":$runtime_rss_max,\"rssEndKiB\":$runtime_rss_end,\"fdsStart\":$runtime_fds_start,\"fdsMax\":$runtime_fds_max,\"fdsEnd\":$runtime_fds_end,\"rssBudgetKiB\":81920},\"database\":{\"bytesStart\":$db_bytes_start,\"bytesEnd\":$db_bytes_end,\"walBytesEnd\":$wal_bytes_end,\"integrityBefore\":\"$integrity_before\",\"integrityAfter\":\"$integrity_after\",\"ledgerMismatchSamples\":$ledger_mismatch_count},\"samplesPath\":\"$samples\"}" >"$summary"
sed -n '1p' "$summary"

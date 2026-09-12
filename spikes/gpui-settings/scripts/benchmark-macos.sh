#!/bin/sh

set -eu
export LC_ALL=C

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
SPIKE_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
BINARY="$SPIKE_DIR/target/release/bongocat-gpui-settings-spike"
OUTPUT_DIR=${BONGOCAT_BENCHMARK_OUTPUT_DIR:-"$SPIKE_DIR/target/benchmark"}
STARTUP_SAMPLES=${BONGOCAT_BENCHMARK_STARTUP_SAMPLES:-10}
IDLE_WARMUP_SECONDS=${BONGOCAT_BENCHMARK_IDLE_WARMUP_SECONDS:-5}
IDLE_SAMPLES=${BONGOCAT_BENCHMARK_IDLE_SAMPLES:-10}

validate_count() {
  name=$1
  value=$2
  case "$value" in
    ''|*[!0-9]*)
      printf '%s must be a positive integer, got: %s\n' "$name" "$value" >&2
      exit 2
      ;;
  esac
  if [ "$value" -lt 1 ] || [ "$value" -gt 120 ]; then
    printf '%s must be between 1 and 120, got: %s\n' "$name" "$value" >&2
    exit 2
  fi
}

validate_count BONGOCAT_BENCHMARK_STARTUP_SAMPLES "$STARTUP_SAMPLES"
validate_count BONGOCAT_BENCHMARK_IDLE_WARMUP_SECONDS "$IDLE_WARMUP_SECONDS"
validate_count BONGOCAT_BENCHMARK_IDLE_SAMPLES "$IDLE_SAMPLES"

if [ "$(uname -s)" != "Darwin" ]; then
  printf 'benchmark-macos.sh must run on macOS\n' >&2
  exit 2
fi

if [ ! -x "$BINARY" ]; then
  printf 'release binary not found; run scripts/package-macos.sh first\n' >&2
  exit 2
fi

mkdir -p "$OUTPUT_DIR"
TEMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/bongocat-gpui-benchmark.XXXXXX")
app_pid=
cleanup() {
  if [ -n "$app_pid" ] && kill -0 "$app_pid" 2>/dev/null; then
    kill -TERM "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
  fi
  rm -rf "$TEMP_DIR"
}
trap cleanup EXIT HUP INT TERM

RUN_ID=$(date -u '+%Y%m%dT%H%M%SZ')
STARTUP_CSV="$OUTPUT_DIR/$RUN_ID-startup.csv"
IDLE_CSV="$OUTPUT_DIR/$RUN_ID-idle.csv"
SUMMARY="$OUTPUT_DIR/$RUN_ID-summary.txt"

printf 'sample,first_frame_ms\n' >"$STARTUP_CSV"
sample=1
while [ "$sample" -le "$STARTUP_SAMPLES" ]; do
  log="$TEMP_DIR/startup-$sample.log"
  BONGOCAT_SPIKE_AUTO_QUIT_MS=300 "$BINARY" >"$log" 2>&1
  elapsed_ms=$(sed -n 's/^gpui-settings-spike: first frame elapsed_ms=//p' "$log")
  if [ -z "$elapsed_ms" ]; then
    printf 'startup sample %s did not report a first frame\n' "$sample" >&2
    cat "$log" >&2
    exit 1
  fi
  printf '%s,%s\n' "$sample" "$elapsed_ms" >>"$STARTUP_CSV"
  sample=$((sample + 1))
done

idle_duration_ms=$(((IDLE_WARMUP_SECONDS + IDLE_SAMPLES + 3) * 1000))
IDLE_LOG="$TEMP_DIR/idle.log"
BONGOCAT_SPIKE_AUTO_QUIT_MS=$idle_duration_ms "$BINARY" >"$IDLE_LOG" 2>&1 &
app_pid=$!
sleep "$IDLE_WARMUP_SECONDS"

printf 'sample,cpu_percent,rss_kib\n' >"$IDLE_CSV"
sample=1
while [ "$sample" -le "$IDLE_SAMPLES" ]; do
  if ! kill -0 "$app_pid" 2>/dev/null; then
    printf 'application exited before idle sample %s\n' "$sample" >&2
    cat "$IDLE_LOG" >&2
    exit 1
  fi
  metrics=$(ps -p "$app_pid" -o %cpu= -o rss= | awk '{$1=$1; print}')
  cpu_percent=$(printf '%s\n' "$metrics" | awk '{print $1}')
  rss_kib=$(printf '%s\n' "$metrics" | awk '{print $2}')
  printf '%s,%s,%s\n' "$sample" "$cpu_percent" "$rss_kib" >>"$IDLE_CSV"
  sample=$((sample + 1))
  sleep 1
done
wait "$app_pid"
app_pid=

printf 'fn main() {}\n' | rustc - \
  --crate-name bongocat_empty_baseline \
  -C opt-level=2 \
  -C lto=thin \
  -o "$TEMP_DIR/bongocat-empty-baseline"

tail -n +2 "$STARTUP_CSV" | cut -d, -f2 | sort -n >"$TEMP_DIR/startup-sorted.txt"
p50_rank=$(((STARTUP_SAMPLES + 1) / 2))
p95_rank=$(((95 * STARTUP_SAMPLES + 99) / 100))
startup_p50=$(sed -n "${p50_rank}p" "$TEMP_DIR/startup-sorted.txt")
startup_p95=$(sed -n "${p95_rank}p" "$TEMP_DIR/startup-sorted.txt")
startup_min=$(sed -n '1p' "$TEMP_DIR/startup-sorted.txt")
startup_max=$(sed -n '$p' "$TEMP_DIR/startup-sorted.txt")

idle_summary=$(awk -F, '
  NR > 1 {
    cpu_sum += $2;
    rss_sum += $3;
    if (NR == 2 || $2 > cpu_max) cpu_max = $2;
    if (NR == 2 || $3 > rss_max) rss_max = $3;
    count += 1;
  }
  END {
    printf "%.3f %.3f %.0f %.0f", cpu_sum / count, cpu_max, rss_sum / count, rss_max;
  }
' "$IDLE_CSV")
idle_cpu_mean=$(printf '%s\n' "$idle_summary" | awk '{print $1}')
idle_cpu_max=$(printf '%s\n' "$idle_summary" | awk '{print $2}')
idle_rss_mean_kib=$(printf '%s\n' "$idle_summary" | awk '{print $3}')
idle_rss_max_kib=$(printf '%s\n' "$idle_summary" | awk '{print $4}')

binary_bytes=$(stat -f '%z' "$BINARY")
baseline_bytes=$(stat -f '%z' "$TEMP_DIR/bongocat-empty-baseline")
binary_increment_bytes=$((binary_bytes - baseline_bytes))

{
  printf 'run_id=%s\n' "$RUN_ID"
  printf 'commit=%s\n' "$(git -C "$SPIKE_DIR" rev-parse HEAD)"
  printf 'target=%s\n' "$(rustc -vV | sed -n 's/^host: //p')"
  printf 'rustc=%s\n' "$(rustc --version)"
  printf 'macos=%s (%s)\n' "$(sw_vers -productVersion)" "$(sw_vers -buildVersion)"
  printf 'startup_samples=%s\n' "$STARTUP_SAMPLES"
  printf 'startup_first_frame_ms_min=%s\n' "$startup_min"
  printf 'startup_first_frame_ms_p50=%s\n' "$startup_p50"
  printf 'startup_first_frame_ms_p95=%s\n' "$startup_p95"
  printf 'startup_first_frame_ms_max=%s\n' "$startup_max"
  printf 'idle_warmup_seconds=%s\n' "$IDLE_WARMUP_SECONDS"
  printf 'idle_samples=%s\n' "$IDLE_SAMPLES"
  printf 'idle_cpu_percent_mean=%s\n' "$idle_cpu_mean"
  printf 'idle_cpu_percent_max=%s\n' "$idle_cpu_max"
  printf 'idle_rss_kib_mean=%s\n' "$idle_rss_mean_kib"
  printf 'idle_rss_kib_max=%s\n' "$idle_rss_max_kib"
  printf 'release_binary_bytes=%s\n' "$binary_bytes"
  printf 'empty_rust_binary_bytes=%s\n' "$baseline_bytes"
  printf 'binary_increment_bytes=%s\n' "$binary_increment_bytes"
  printf 'startup_csv=%s\n' "$STARTUP_CSV"
  printf 'idle_csv=%s\n' "$IDLE_CSV"
} >"$SUMMARY"

cat "$SUMMARY"

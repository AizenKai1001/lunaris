#!/usr/bin/env bash
# Two server binaries (or one binary with two sets of arguments) against the same load, taken in turns so that the noise of a shared machine falls on both alike (a Linux test machine: a shell with HL_DIR=scratch-lunaris-perf):
#   bash tools/srvperf_ab.sh target/release/lunaris.base target/release/lunaris.new 3 200 40 [arguments for both]
#   ARGS_B=--spin bash tools/srvperf_ab.sh target/release/lunaris target/release/lunaris 3 200 40 --governor 0     (ARGS_A and ARGS_B: arguments for the first and the second only)
# The live-run lock is taken once for all the runs. The full report of every run is kept in /tmp/srvperf_ab_<TAG>_<n>_<A or B>.txt (TAG in the environment); what is printed is the summary of each.
A="$1"; B="$2"; RUNS="${3:-3}"; BOTS="${4:-200}"; SECS="${5:-40}"; shift 5
HERE="$(cd "$(dirname "$0")" && pwd)"
exec 8>/tmp/vb_live.lock
flock 8
export LOCK_HELD=1
for i in $(seq 1 "$RUNS"); do
  for SIDE in A B; do
    if [ "$SIDE" = A ]; then X="$A"; EXTRA="${ARGS_A:-}"; else X="$B"; EXTRA="${ARGS_B:-}"; fi
    # (word splitting of $EXTRA is meant: it is a list of arguments)
    # shellcheck disable=SC2086
    bash "$HERE/srvperf_load.sh" "$X" "$BOTS" "$SECS" "$@" $EXTRA > "/tmp/srvperf_ab_${TAG:-x}_${i}_${SIDE}.txt" 2>&1
    echo "=== $SIDE ($(basename "$X") $* $EXTRA) run $i"
    grep -E "^SUMMARY|^  phases" "/tmp/srvperf_ab_${TAG:-x}_${i}_${SIDE}.txt"
  done
done

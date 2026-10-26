#!/usr/bin/env bash
# Runs `cargo test ARGS...` for a CI step and, when it fails or runs too long, writes what happened as a workflow annotation (a check run's annotations can be read through the API without a token;
# its log cannot, and a step that the workflow's own time limit kills leaves nothing at all).
#     bash tools/ci_step.sh -p lunaris-gpu --test high
# STEP_TIMEOUT=seconds (default 900) stops the test run first and reports the last lines it printed, so that a hang says where it hangs.
out=$(mktemp)
limit=${STEP_TIMEOUT:-900}
timeout "$limit" cargo test "$@" 2>&1 | tee "$out"
code=${PIPESTATUS[0]}
if [ "$code" = 124 ]; then
  msg=$(tail -n 14 "$out" | tr '\n' '|' | cut -c1-1800)
  echo "::error title=a test run took more than ${limit} s and was stopped (the last lines it printed follow)::${msg}"
elif [ "$code" != 0 ]; then
  msg=$(grep -E -A4 "panicked at|^error(\[|:)|FAILED|^failures:" "$out" | head -60 | tr '\n' '|' | cut -c1-1800)
  echo "::error title=a test failed::${msg}"
fi
exit "$code"

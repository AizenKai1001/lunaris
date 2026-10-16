#!/usr/bin/env bash
# Runs `cargo test ARGS...` for a CI step and, when it fails, writes what failed as a workflow annotation (a check run's annotations can be read through the API without a token; its log cannot).
#     bash tools/ci_step.sh -p lunaris-gpu --test high
out=$(mktemp)
cargo test "$@" 2>&1 | tee "$out"
code=${PIPESTATUS[0]}
if [ "$code" != 0 ]; then
  msg=$(grep -E -A4 "panicked at|^error(\[|:)|FAILED|^failures:" "$out" | head -60 | tr '\n' '|' | cut -c1-1800)
  echo "::error title=a graphics test failed::${msg}"
fi
exit "$code"

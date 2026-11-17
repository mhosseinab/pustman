#!/usr/bin/env bash
# pustman-v1-test — fast verify loop for one crate. Refuses zero-test passes.
# Usage: scripts/pustman-v1-test.sh <crate> [filter]
# Prints exactly "PASS <n>" on success (n > 0), or one "ERROR ..." line and exit 1.
set -euo pipefail

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "ERROR usage: scripts/pustman-v1-test.sh <crate> [filter]" >&2
  exit 1
fi

crate="$1"
filter="${2:-}"

log="$(mktemp)"
trap 'rm -f "$log"' EXIT

status=0
if [ -n "$filter" ]; then
  cargo test -p "$crate" -- "$filter" >"$log" 2>&1 || status=$?
else
  cargo test -p "$crate" -- >"$log" 2>&1 || status=$?
fi

if [ "$status" -ne 0 ]; then
  error_line="$(grep -m1 -E 'FAILED|panicked|^error' "$log" || true)"
  if [ -z "$error_line" ]; then
    error_line="$(grep -m1 -v '^[[:space:]]*$' "$log" || true)"
  fi
  echo "ERROR ${error_line:-cargo test -p $crate failed (exit $status)}" >&2
  exit 1
fi

passed="$(awk '/^test result: ok\./ { for (i = 1; i <= NF; i++) if ($i ~ /^passed/) total += $(i - 1) } END { print total + 0 }' "$log")"
if [ "$passed" -eq 0 ]; then
  echo "ERROR 0 tests ran" >&2
  exit 1
fi
echo "PASS $passed"

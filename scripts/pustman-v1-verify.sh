#!/usr/bin/env bash
# pustman-v1-verify — the merge gate: fmt, clippy -D warnings, workspace tests, tripwire.
# Usage: scripts/pustman-v1-verify.sh
# One line per stage ("fmt ok" ...); last line "VERIFY OK", or "VERIFY FAIL <stage>" and exit 1.
set -euo pipefail

log="$(mktemp)"
trap 'rm -f "$log"' EXIT

# On failure, name what failed (CI has no other window into a hidden stage), then the verdict.
stage_fail() {
  if [ -s "$log" ]; then
    grep -E -A2 '^test .* FAILED|panicked at|^error(\[|:)' "$log" | head -60 >&2 || true
  fi
  echo "VERIFY FAIL $1" >&2
  exit 1
}

if [ ! -f Cargo.toml ]; then
  scripts/pustman-v1-tripwire.sh || stage_fail tripwire
  echo "tripwire ok"
  echo "VERIFY OK (no workspace yet)"
  exit 0
fi

cargo fmt --all -- --check >/dev/null 2>&1 || stage_fail fmt
echo "fmt ok"

cargo clippy --workspace --all-targets -- -D warnings >"$log" 2>&1 || stage_fail clippy
echo "clippy ok"

cargo test --workspace --no-fail-fast >"$log" 2>&1 || stage_fail test
echo "test ok"

scripts/pustman-v1-tripwire.sh || stage_fail tripwire
echo "tripwire ok"

echo "VERIFY OK"

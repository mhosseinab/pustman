#!/usr/bin/env bash
# pustman-v1-tripwire — counts banned constructs in non-test crate source. Baseline 0; it never grows.
# Scope: crates/*/src/**/*.rs, excluding *_tests.rs, test_support.rs and anything under tests/.
# Prints "TRIPWIRE <n>"; exits 1 if n > 0. See AGENTS.md (Rules) and plan §9.
set -euo pipefail

PATTERNS=(
  '.unwrap()'
  '.expect('
  'std::process::Command'
  'serde_json::from_reader'
  'let _ ='
  'smol::Timer'
  'std::thread::sleep'
  'mod tests {'
)

files=()
if [ -d crates ]; then
  while IFS= read -r file; do
    case "$file" in
      */tests/* | *_tests.rs | */test_support.rs) continue ;;
    esac
    files+=("$file")
  done < <(find crates -type f -name '*.rs' -path '*/src/*' | sort)
fi

total=0
if [ "${#files[@]}" -gt 0 ]; then
  for pattern in "${PATTERNS[@]}"; do
    n="$( { grep -oF -- "$pattern" "${files[@]}" || true; } | wc -l)"
    total=$((total + n))
  done
fi

echo "TRIPWIRE $total"
if [ "$total" -gt 0 ]; then
  exit 1
fi

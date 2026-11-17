#!/usr/bin/env bash
# protect-steps-and-tests — PreToolUse hook (Edit|Write|NotebookEdit).
# Blocks edits to docs/pustman-v1-implementation-steps.md (read-only for workers) and to test
# files that already exist on pustman-v1/main (existing tests are read-only; a later step adds
# NEW test files, which stay writable). Reads the tool call JSON on stdin; exit 2 blocks the
# call with the reason on stderr, exit 0 allows it.
set -euo pipefail

payload="$(cat)"
tool_name="$(printf '%s' "$payload" | jq -r '.tool_name // empty')"
file_path="$(printf '%s' "$payload" | jq -r '.tool_input.file_path // .tool_input.notebook_path // empty')"

case "$tool_name" in
  Edit | Write | NotebookEdit) ;;
  *) exit 0 ;;
esac

[ -n "$file_path" ] || exit 0

case "$file_path" in
  *pustman-v1-implementation-steps.md)
    echo "protect-steps-and-tests: $file_path is read-only for workers (the steps doc and its Verify blocks)" >&2
    exit 2
    ;;
esac

case "$file_path" in
  */tests/*.rs | *_tests.rs | *test_support.rs) ;;
  *) exit 0 ;;
esac

repo_root="$(git rev-parse --show-toplevel 2>/dev/null)" || exit 0
case "$file_path" in
  "$repo_root"/*) rel="${file_path#"$repo_root"/}" ;;
  /*) exit 0 ;;
  *) rel="$file_path" ;;
esac

if git -C "$repo_root" cat-file -e "pustman-v1/main:$rel" 2>/dev/null; then
  echo "protect-steps-and-tests: $rel already exists on pustman-v1/main; existing test files are read-only (add a NEW test file instead)" >&2
  exit 2
fi
exit 0

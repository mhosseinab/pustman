#!/usr/bin/env bash
# pustman-v1-cleanup — reclaim disk after merges, then gate on free space.
# Usage: scripts/pustman-v1-cleanup.sh [--min-free-gb N]   (default 80; run from any checkout)
#
# Every step worktree builds its own target/ (several GB; wire steps ~10 GB), so worktrees of
# merged steps must not linger. A step counts as merged when pustman-v1/main has its squash
# commit "S<N>: …". Its worktree is removed only when it holds no uncommitted work (the
# untracked .cargo/ jobs cap does not count). The branch pustman-v1/S<N> is always kept.
#
# Output: "REMOVED S<N>" / "KEEP S<N> dirty" per worktree, "DISK <n>G free", then
# "CLEANUP OK", or "CLEANUP LOW-DISK" + per-worktree target/ sizes and exit 1.
set -euo pipefail

min_free_gb=80
while [ $# -gt 0 ]; do
  case "$1" in
    --min-free-gb) min_free_gb="$2"; shift 2 ;;
    *) echo "usage: $0 [--min-free-gb N]" >&2; exit 2 ;;
  esac
done

repo=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")
root="$repo/.claude/worktrees/pustman-v1"
merged_subjects=$(git -C "$repo" log --format=%s pustman-v1/main)

for worktree in "$root"/S*/; do
  [ -d "$worktree" ] || continue
  worktree=${worktree%/}
  step=$(basename "$worktree")
  grep -q "^${step}: " <<<"$merged_subjects" || continue
  if git -C "$worktree" status --porcelain --untracked-files=all | grep -qv '^?? \.cargo/'; then
    echo "KEEP $step dirty"
    continue
  fi
  git -C "$repo" worktree remove --force "$worktree"
  echo "REMOVED $step"
done
git -C "$repo" worktree prune

free_gb=$(df -Pk "$repo" | awk 'NR == 2 { print int($4 / 1048576) }')
echo "DISK ${free_gb}G free"
if [ "$free_gb" -lt "$min_free_gb" ]; then
  du -sh "$repo/target" "$root"/*/target 2>/dev/null || true
  echo "CLEANUP LOW-DISK (need ${min_free_gb}G)"
  exit 1
fi
echo "CLEANUP OK"

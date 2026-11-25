#!/usr/bin/env bash
# pustman-v1-cleanup — reclaim disk after merges, then gate on free space.
# Usage: scripts/pustman-v1-cleanup.sh [--min-free-gb N]   (run from any checkout)
# Default 80 on the build host; 15 in a cloud session (CLAUDE_CODE_REMOTE=true), whose VM has
# ~30 GB of disk in all — there, run one worker at a time.
#
# Every step worktree builds its own target/ (several GB; wire steps ~10 GB), so worktrees of
# merged steps must not linger. A step counts as merged when pustman-v1/main has its squash
# commit "S<N>: …". Its worktree is removed only when it holds no uncommitted work (the
# untracked .cargo/ jobs cap does not count). The branch pustman-v1/S<N> is always kept.
#
# Output: "REMOVED S<N>" / "KEEP S<N> dirty" per worktree, "DISK <n>G free, need <m>G", then
# "CLEANUP OK", or "CLEANUP LOW-DISK" + per-worktree target/ sizes and exit 1.
set -euo pipefail

min_free_gb=80
if [ "${CLAUDE_CODE_REMOTE:-}" = "true" ]; then
  min_free_gb=15
fi
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

# Each rebuild of a test target links a new `<name>-<hash>` binary in target/debug/deps and
# never deletes the old one; a long-lived checkout piles up hundreds (31 GB once). Keep, per
# name, every binary built within 10 minutes of that name's newest — a crate's lib and bin
# tests share a name and are linked together — and delete the rest. Cargo relinks anything it
# still needs.
for deps in "$repo/target/debug/deps" "$repo"/.claude/worktrees/*/target/debug/deps \
  "$root"/*/target/debug/deps; do
  [ -d "$deps" ] || continue
  pruned=$(find "$deps" -maxdepth 1 -type f -perm -u+x -regextype posix-extended \
      -regex '.*/[A-Za-z0-9_]+-[0-9a-f]{16}' -printf '%T@ %p\n' \
    | awk '{
        name = $2; sub(/.*\//, "", name); sub(/-[0-9a-f]{16}$/, "", name)
        time[NR] = $1; path[NR] = $2; key[NR] = name
        if ($1 > newest[name]) newest[name] = $1
      }
      END { for (i = 1; i <= NR; i++) if (time[i] < newest[key[i]] - 600) print path[i] }')
  [ -n "$pruned" ] || continue
  count=$(wc -l <<<"$pruned")
  xargs -d '\n' rm -f -- <<<"$pruned"
  echo "PRUNED $count stale test binaries in ${deps#"$repo"/}"
done

free_gb=$(df -Pk "$repo" | awk 'NR == 2 { print int($4 / 1048576) }')
echo "DISK ${free_gb}G free, need ${min_free_gb}G"
if [ "$free_gb" -lt "$min_free_gb" ]; then
  du -sh "$repo/target" "$root"/*/target 2>/dev/null || true
  echo "CLEANUP LOW-DISK (need ${min_free_gb}G)"
  exit 1
fi
echo "CLEANUP OK"

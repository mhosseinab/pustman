#!/usr/bin/env bash
# pustman-v1-init — bootstrap check for a Pustman v1 worktree. Idempotent; it changes nothing.
# Usage: scripts/pustman-v1-init.sh [--require-gpui]
#   --require-gpui  missing GPUI system libs become an error (exit 1) instead of a report line.
# Prints TOOLCHAIN=ok and one GPUI_DEPS= line; see docs/pustman-v1-plan.md §2 (gate G-DEPS).
set -euo pipefail

REQUIRED_RUST="1.98.1"
GPUI_LIBS=(xkbcommon xkbcommon-x11 wayland-client x11-xcb vulkan fontconfig freetype2)

require_gpui=0
for arg in "$@"; do
  case "$arg" in
    --require-gpui) require_gpui=1 ;;
    *)
      echo "ERROR unknown argument: $arg (usage: scripts/pustman-v1-init.sh [--require-gpui])" >&2
      exit 1
      ;;
  esac
done

version_line="$(rustc --version 2>/dev/null || true)"
actual="${version_line#rustc }"
actual="${actual%% *}"
if [ "$actual" = "$REQUIRED_RUST" ]; then
  echo "TOOLCHAIN=ok"
else
  if [ -z "$version_line" ]; then
    echo "ERROR toolchain rustc not found on PATH; need $REQUIRED_RUST" >&2
  else
    echo "ERROR toolchain $version_line; need rustc $REQUIRED_RUST" >&2
  fi
  exit 1
fi

# macOS links the system frameworks directly; there is nothing to probe with pkg-config.
if [ "$(uname -s)" = "Darwin" ]; then
  echo "GPUI_DEPS=ok"
  exit 0
fi

missing=()
for lib in "${GPUI_LIBS[@]}"; do
  if ! pkg-config --exists "$lib" 2>/dev/null; then
    missing+=("$lib")
  fi
done

if [ "${#missing[@]}" -eq 0 ]; then
  echo "GPUI_DEPS=ok"
  exit 0
fi

if [ "$require_gpui" -eq 1 ]; then
  echo "ERROR GPUI system libs missing: ${missing[*]}" >&2
  exit 1
fi
echo "GPUI_DEPS=missing ${missing[*]}"

#!/usr/bin/env bash
# Install Pustman's desktop entry and icons for the current user, so launchers, the
# Wayland dock and Alt-Tab show the app icon. The entry's basename must equal the
# window's app id (`pustman::app_icon::APP_ID`).
#
# Usage: scripts/install-desktop-entry.sh [path/to/pustman-binary]
#        (default: target/release/pustman)
set -euo pipefail

APP_ID=pustman
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pack="$repo_root/assets/pustman-icon-pack/linux"
binary="$(realpath "${1:-$repo_root/target/release/pustman}")"
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"

if [[ ! -x "$binary" ]]; then
    echo "install-desktop-entry: no executable at $binary (build it: cargo build --release -p pustman)" >&2
    exit 1
fi

for size_dir in "$pack"/hicolor/*/; do
    size="$(basename "$size_dir")"
    source_icon="$(find "$size_dir/apps" -maxdepth 1 -name 'app-icon.*' | head -n 1)"
    target_dir="$data_home/icons/hicolor/$size/apps"
    mkdir -p "$target_dir"
    cp "$source_icon" "$target_dir/$APP_ID.${source_icon##*.}"
done

mkdir -p "$data_home/applications"
sed -e "s|@APP_NAME@|Pustman|" \
    -e "s|@APP_EXECUTABLE@|$binary|" \
    -e "s|@ICON_BASENAME@|$APP_ID|" \
    -e "s|^Categories=.*|Categories=Development;|" \
    "$pack/app.desktop.in" > "$data_home/applications/$APP_ID.desktop"

if command -v gtk-update-icon-cache >/dev/null; then
    gtk-update-icon-cache --quiet --ignore-theme-index "$data_home/icons/hicolor" || true
fi
if command -v update-desktop-database >/dev/null; then
    update-desktop-database --quiet "$data_home/applications" || true
fi

echo "installed $data_home/applications/$APP_ID.desktop (Exec=$binary)"

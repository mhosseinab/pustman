#!/usr/bin/env bash
# Build a Debian/Ubuntu package (.deb) of Pustman: the binary in /usr/bin, a desktop entry
# and the hicolor icons, so it installs with `sudo apt install ./pustman_….deb` and shows in
# the launcher. Depends: is derived from the libraries the binary actually links (ldd +
# dpkg -S on the build host), so it cannot drift from the code.
#
# Usage: scripts/package-deb.sh <path/to/pustman> <version> <output.deb>
#        version: a tag like v0.1.0-beta.6 or 0.1.0 (a pre-release suffix becomes ~beta.6,
#        which Debian sorts before the final 0.1.0)
set -euo pipefail

binary="$1"
version="${2#v}"
version="${version/-/\~}"
out="$3"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pack="$repo_root/assets/pustman-icon-pack/linux"
app_id=pustman

if [[ ! -x "$binary" ]]; then
    echo "package-deb: no executable at $binary" >&2
    exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
root="$work/root"
mkdir -p "$root/DEBIAN" "$root/usr/bin" "$root/usr/share/applications"
install -m 0755 "$binary" "$root/usr/bin/$app_id"

for size_dir in "$pack"/hicolor/*/; do
    size="$(basename "$size_dir")"
    icon="$(find "$size_dir/apps" -maxdepth 1 -name 'app-icon.*' | head -n 1)"
    [[ -n "$icon" ]] || continue
    mkdir -p "$root/usr/share/icons/hicolor/$size/apps"
    install -m 0644 "$icon" "$root/usr/share/icons/hicolor/$size/apps/$app_id.${icon##*.}"
done

sed -e "s|@APP_NAME@|Pustman|" \
    -e "s|@APP_EXECUTABLE@|$app_id|" \
    -e "s|@ICON_BASENAME@|$app_id|" \
    -e "s|^Categories=.*|Categories=Development;|" \
    "$pack/app.desktop.in" > "$root/usr/share/applications/$app_id.desktop"

# Depends: the package owning each shared library the binary links.
missing="$(ldd "$binary" | awk '/not found/ {print $1}')"
if [[ -n "$missing" ]]; then
    echo "package-deb: unresolved libraries on the build host: $missing" >&2
    exit 1
fi
depends="$(ldd "$binary" | awk '/=> \// {print $3}' \
    | while read -r lib; do dpkg -S "$(readlink -f "$lib")" 2>/dev/null || dpkg -S "$lib" 2>/dev/null || true; done \
    | cut -d: -f1 | sort -u | paste -sd, - | sed 's/,/, /g')"

# Loaded at run time with dlopen, so ldd cannot see them: wgpu renders through Vulkan
# (or EGL as its fallback), and a Wayland session needs the Wayland client libraries.
depends="$depends, libvulkan1 | libegl1"
recommends="libwayland-client0, libwayland-egl1, mesa-vulkan-drivers"

installed_kb="$(du -sk "$root/usr" | cut -f1)"
cat > "$root/DEBIAN/control" <<EOF
Package: $app_id
Version: $version
Section: devel
Priority: optional
Architecture: $(dpkg --print-architecture)
Installed-Size: $installed_kb
Depends: $depends
Recommends: $recommends
Maintainer: Pustman <noreply@users.noreply.github.com>
Homepage: https://github.com/mhosseinab/pustman
Description: Native, GPU-accelerated API client
 Pustman sends and inspects HTTP requests, organizes them in collections and
 environments stored as plain files you can share with Git, and keeps secret
 values in the OS keychain.
EOF

mkdir -p "$(dirname "$out")"
dpkg-deb --root-owner-group --build "$root" "$out"
echo "built $out (Depends: $depends)"

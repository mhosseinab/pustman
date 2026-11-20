#!/usr/bin/env bash
# Wrap a built Pustman binary in a minimal macOS app bundle, so Finder, the Dock and
# Cmd-Tab show the app icon (macOS reads it from the bundle, not from the window).
#
# Usage: scripts/bundle-macos.sh [path/to/pustman-binary] [output-dir]
#        (defaults: target/release/pustman, target/release)
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${1:-$repo_root/target/release/pustman}"
out_dir="${2:-$repo_root/target/release}"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo_root/crates/pustman/Cargo.toml" | head -n 1)"
bundle="$out_dir/Pustman.app"

if [[ ! -x "$binary" ]]; then
    echo "bundle-macos: no executable at $binary (build it: cargo build --release -p pustman)" >&2
    exit 1
fi

rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$binary" "$bundle/Contents/MacOS/pustman"
cp "$repo_root/assets/pustman-icon-pack/macos/app-icon.icns" "$bundle/Contents/Resources/app-icon.icns"
cat > "$bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Pustman</string>
    <key>CFBundleDisplayName</key><string>Pustman</string>
    <key>CFBundleIdentifier</key><string>dev.pustman.Pustman</string>
    <key>CFBundleExecutable</key><string>pustman</string>
    <key>CFBundleIconFile</key><string>app-icon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

echo "bundled $bundle"

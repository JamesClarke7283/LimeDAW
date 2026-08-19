#!/usr/bin/env bash
# Bundle LimeDAW into a .app and wrap it in a .dmg. macOS only.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

version="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
target="${1:-aarch64-apple-darwin}"
arch_label="$([ "$target" = "aarch64-apple-darwin" ] && echo arm64 || echo x86_64)"
binary="target/$target/release/limedaw"

[ -x "$binary" ] || { echo "build first: cargo build --release --target $target" >&2; exit 1; }

app="target/LimeDAW.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

install -m755 "$binary" "$app/Contents/MacOS/limedaw"

# Build the .icns from the PNG the icon module generates.
iconset="target/limedaw.iconset"
rm -rf "$iconset"; mkdir -p "$iconset"
for size in 16 32 64 128 256 512; do
    sips -z $size $size assets/limedaw.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double assets/limedaw.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/limedaw.icns"

sed -e "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"

# An ad-hoc signature is enough for Gatekeeper to run it after the user allows
# it once; a real Developer ID signature would be applied here instead.
codesign --force --deep --sign - "$app" 2>/dev/null || \
    echo "note: could not sign the bundle; users will need to allow it in System Settings" >&2

mkdir -p dist
staging="target/dmg"
rm -rf "$staging"; mkdir -p "$staging"
cp -R "$app" "$staging/"
ln -s /Applications "$staging/Applications"

output="dist/LimeDAW-${version}-macos-${arch_label}.dmg"
rm -f "$output"
hdiutil create -volname "LimeDAW $version" -srcfolder "$staging" \
    -ov -format UDZO "$output" >/dev/null

echo "$output"

#!/usr/bin/env bash
# Bundle LimeDAW into a .app and wrap it in a .dmg. macOS only.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

version="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
target="${1:-$(rustc -vV | sed -n 's/^host: //p')}"
case "$target" in
    aarch64-apple-darwin) arch_label=arm64 ;;
    x86_64-apple-darwin) arch_label=x86_64 ;;
    *) echo "Unsupported macOS target: $target" >&2; exit 1 ;;
esac
binary="target/$target/release/limedaw"

[ -x "$binary" ] || { echo "build first: cargo build --release --target $target" >&2; exit 1; }

app="target/LimeDAW.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

install -m755 "$binary" "$app/Contents/MacOS/limedaw"
install -m644 README.md LICENSE "$app/Contents/Resources/"

# The standalone bundle must not depend on libraries from the build machine.
while IFS= read -r library; do
    case "$library" in
        /System/Library/*|/usr/lib/*) ;;
        *) echo "Unbundled macOS library: $library" >&2; exit 1 ;;
    esac
done < <(otool -L "$binary" | awk 'NR > 1 { print $1 }')

# Build the .icns from the PNG the icon module generates.
iconset="target/limedaw.iconset"
rm -rf "$iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
    sips -z $size $size assets/limedaw.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double assets/limedaw.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/limedaw.icns"

sed -e "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"

# Ad-hoc signing supports local execution (including Apple silicon). Public
# Developer ID signing and notarization require the maintainer's credentials.
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"
"$app/Contents/MacOS/limedaw" --version

mkdir -p dist
staging="target/dmg"
rm -rf "$staging"; mkdir -p "$staging"
cp -R "$app" "$staging/"
ln -s /Applications "$staging/Applications"

output="dist/LimeDAW-${version}-macos-${arch_label}.dmg"
rm -f "$output"
hdiutil create -volname "LimeDAW $version" -srcfolder "$staging" \
    -ov -format UDZO "$output" >/dev/null
hdiutil verify "$output" >/dev/null

echo "$output"

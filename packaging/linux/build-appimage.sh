#!/usr/bin/env bash
# Package an already-built native Linux binary, including its shared libraries.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

target="${1:-$(rustc -vV | sed -n 's/^host: //p')}"
case "$target" in
    x86_64-unknown-linux-gnu) arch=x86_64 ;;
    aarch64-unknown-linux-gnu) arch=aarch64 ;;
    *) echo "Unsupported Linux target: $target" >&2; exit 1 ;;
esac
version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)"
binary="target/$target/release/limedaw"
appdir="target/$target/AppDir"
output="$root/dist/LimeDAW-${version}-linux-${arch}.AppImage"

[ -x "$binary" ] || { echo "Build first: cargo build --release --locked --target $target" >&2; exit 1; }
command -v linuxdeploy >/dev/null || { echo "linuxdeploy must be on PATH (see packaging/README.md)" >&2; exit 1; }

rm -rf "$appdir"
mkdir -p "$appdir/usr/share/metainfo" "$appdir/usr/share/doc/limedaw" dist
# appimagetool discovers metadata using the desktop file's basename.
install -m644 packaging/linux/dev.limedaw.LimeDAW.metainfo.xml "$appdir/usr/share/metainfo/limedaw.appdata.xml"
install -m644 README.md LICENSE "$appdir/usr/share/doc/limedaw/"

# winit loads xkbcommon dynamically, so it is not discovered by ldd.
# linuxdeploy discovers the remaining linked, non-system libraries itself.
xkb_libdir="$(pkg-config --variable=libdir xkbcommon)"
export APPIMAGE_EXTRACT_AND_RUN=1
export ARCH="$arch"
export OUTPUT="$output"
linuxdeploy --appdir "$appdir" --executable "$binary" \
    --desktop-file assets/limedaw.desktop --icon-file assets/limedaw.png \
    --library "$xkb_libdir/libxkbcommon.so.0" \
    --library "$xkb_libdir/libxkbcommon-x11.so.0" --output appimage

[ -s "$output" ] || { echo "linuxdeploy did not produce $output" >&2; exit 1; }
chmod +x "$output"
echo "$output"

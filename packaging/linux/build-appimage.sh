#!/usr/bin/env bash
# Build a self-contained AppImage from an already-compiled release binary.
#
# Expects target/release/limedaw to exist and appimagetool to be on PATH (the
# CI workflow downloads it). Produces dist/LimeDAW-<version>-<arch>.AppImage.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

version="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
arch="$(uname -m)"
appdir="target/AppDir"
binary="target/release/limedaw"

[ -x "$binary" ] || { echo "build the release binary first: cargo build --release" >&2; exit 1; }

rm -rf "$appdir"
mkdir -p "$appdir/usr/bin" \
         "$appdir/usr/share/applications" \
         "$appdir/usr/share/icons/hicolor/scalable/apps" \
         "$appdir/usr/share/icons/hicolor/256x256/apps" \
         "$appdir/usr/share/metainfo"

install -m755 "$binary" "$appdir/usr/bin/limedaw"
install -m644 assets/limedaw.desktop "$appdir/usr/share/applications/limedaw.desktop"
install -m644 assets/limedaw.svg "$appdir/usr/share/icons/hicolor/scalable/apps/limedaw.svg"
install -m644 assets/limedaw.png "$appdir/usr/share/icons/hicolor/256x256/apps/limedaw.png"

# AppImage looks for these at the AppDir root.
cp assets/limedaw.desktop "$appdir/limedaw.desktop"
cp assets/limedaw.png "$appdir/limedaw.png"
cp assets/limedaw.svg "$appdir/.DirIcon.svg" 2>/dev/null || true
cp assets/limedaw.png "$appdir/.DirIcon"

cat > "$appdir/AppRun" <<'APPRUN'
#!/bin/sh
# Run LimeDAW from inside the AppImage, with the bundled libraries visible.
here="$(dirname "$(readlink -f "$0")")"
export PATH="$here/usr/bin:$PATH"
export LD_LIBRARY_PATH="$here/usr/lib:${LD_LIBRARY_PATH:-}"
export XDG_DATA_DIRS="$here/usr/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
exec "$here/usr/bin/limedaw" "$@"
APPRUN
chmod +x "$appdir/AppRun"

cat > "$appdir/usr/share/metainfo/dev.limedaw.LimeDAW.appdata.xml" <<XML
<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>dev.limedaw.LimeDAW</id>
  <name>LimeDAW</name>
  <summary>Write, arrange and mix music</summary>
  <metadata_license>CC0-1.0</metadata_license>
  <project_license>GPL-3.0-or-later</project_license>
  <description>
    <p>
      A pattern-based digital audio workstation with a channel rack, piano roll,
      playlist and mixer, a built-in synthesiser and sampler, SoundFont
      playback, and an MCP server for driving it programmatically.
    </p>
  </description>
  <launchable type="desktop-id">limedaw.desktop</launchable>
  <releases><release version="$version"/></releases>
</component>
XML

mkdir -p dist
output="dist/LimeDAW-${version}-${arch}.AppImage"

if command -v linuxdeploy >/dev/null 2>&1; then
    # linuxdeploy pulls in the shared libraries the binary needs.
    linuxdeploy --appdir "$appdir" --output appimage \
        --desktop-file assets/limedaw.desktop --icon-file assets/limedaw.png
    mv ./*.AppImage "$output"
else
    ARCH="$arch" appimagetool "$appdir" "$output"
fi

chmod +x "$output"
echo "$output"

#!/usr/bin/env bash
# Build AppImage, DEB, RPM and a portable tarball from the same release binary.
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
prefix="dist/LimeDAW-${version}-linux-${arch}"

[ -x "$binary" ] || { echo "Build first: cargo build --release --locked --target $target" >&2; exit 1; }
mkdir -p dist
bash packaging/linux/build-appimage.sh "$target"
cargo deb --locked --no-build --target "$target" --output "$prefix.deb"
cargo generate-rpm --target "$target" --auto-req builtin --output "$prefix.rpm"

staging="target/$target/tarball"
rm -rf "$staging"
mkdir -p "$staging/LimeDAW-$version"
install -m755 "$binary" "$staging/LimeDAW-$version/limedaw"
install -m644 README.md LICENSE assets/limedaw.desktop assets/limedaw.svg assets/limedaw.png "$staging/LimeDAW-$version/"
tar -czf "$prefix.tar.gz" -C "$staging" "LimeDAW-$version"

for extension in AppImage deb rpm tar.gz; do
    [ -s "$prefix.$extension" ] || { echo "Missing package: $prefix.$extension" >&2; exit 1; }
done

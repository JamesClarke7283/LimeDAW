# Building distributables

The release workflow builds on native runners for Linux x86_64/ARM64,
Windows x64/ARM64, and macOS Intel/Apple silicon. All packages land in `dist/`.
The checked-in icon files are ready for packaging; regenerate them with
`cargo run --example generate_icon` when changing the artwork.

## Linux

Use Ubuntu 22.04 to match CI's minimum glibc version. On another distribution,
packages inherit that build host's glibc requirement. Cross-compilation alone
is insufficient for automatic DEB/RPM dependency discovery; package natively
on the target architecture.

```bash
sudo apt-get update
sudo apt-get install -y libasound2-dev libudev-dev pkg-config \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev desktop-file-utils rpm file
cargo install cargo-deb --locked --version 3.8.0
cargo install cargo-generate-rpm --locked --version 0.21.1
```

Download [linuxdeploy](https://github.com/linuxdeploy/linuxdeploy/releases/tag/continuous)
for the host architecture. This extraction avoids a FUSE dependency in the
build environment:

```bash
mkdir -p target/packaging-tools/linuxdeploy
curl --fail --location --retry 3 \
  "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$(uname -m).AppImage" \
  -o target/packaging-tools/linuxdeploy.AppImage
chmod +x target/packaging-tools/linuxdeploy.AppImage
(
  cd target/packaging-tools/linuxdeploy
  ../linuxdeploy.AppImage --appimage-extract > /dev/null
  ln -sf "$PWD/squashfs-root/AppRun" linuxdeploy
)
export PATH="$PWD/target/packaging-tools/linuxdeploy:$PATH"
target="$(rustc -vV | sed -n 's/^host: //p')"
cargo build --release --locked --target "$target"
bash packaging/linux/build-packages.sh "$target"
```

This produces `.AppImage`, `.deb`, `.rpm`, and `.tar.gz` files. The AppImage
bundles non-system shared libraries with linuxdeploy and explicitly includes
xkbcommon, which the windowing library loads dynamically. DEB and RPM packages
declare their runtime library dependencies. A missing tool or failed package
aborts the build; there is no fallback that silently drops a requested format.

To build only the AppImage after compilation:

```bash
bash packaging/linux/build-appimage.sh "$target"
```

## macOS

Install Rust and Xcode command line tools. Build on the target architecture:

```bash
export MACOSX_DEPLOYMENT_TARGET=11.0
target="$(rustc -vV | sed -n 's/^host: //p')"
cargo build --release --locked --target "$target"
bash packaging/macos/build-dmg.sh "$target"
```

The script creates `LimeDAW.app`, generates its ICNS icon, includes the license
and README, checks for unbundled third-party libraries, applies and verifies
an ad-hoc signature, and produces a verified DMG. Developer ID signing and
notarization are not configured; those require Apple credentials.

## Windows

Use a native Windows runner with Rust's MSVC toolchain, Visual Studio C++ build
tools, and the .NET SDK. Run from PowerShell:

```powershell
dotnet tool install --global wix --version 5.0.2
wix extension add -g WixToolset.UI.wixext/5.0.2
cargo build --release --locked --target x86_64-pc-windows-msvc
packaging/windows/build-msi.ps1 -Target x86_64-pc-windows-msvc
```

On ARM64, substitute `aarch64-pc-windows-msvc`. This produces an MSI installer
and a portable ZIP, both including the license and README. The MSVC runtime
is linked statically. The installer uses a version-matched WiX UI extension.
Public code signing is not configured.

## GitHub Actions

[`.github/workflows/release.yml`](../.github/workflows/release.yml) runs for
every push to `main`, `v*` tags, pull requests targeting `main`, and manual
dispatches. It checks formatting, Clippy, and tests, then builds and tests each
native target and validates its packages. Pull requests upload build artifacts
without publishing releases.

After all six native builds succeed, a push to `main` (or a manual run on
`main`) moves the `nightly` tag to the exact built commit and refreshes the
GitHub prerelease. Older nightly assets are removed after new uploads succeed.
Pushing a `v*` tag publishes a separate versioned release. Bump the version in
`Cargo.toml` and update `Cargo.lock` before tagging a new version.

Each release contains 14 packages and `SHA256SUMS`. Only the publishing job
has `contents: write`; a failed package prevents publishing the entire release.
The workflow preserves the last successful nightly when an earlier build or
test fails. Package versions come from `Cargo.toml`; nightly commit identity
is recorded in the release notes rather than changing the installer version.
MSI installers replace earlier builds with the same version, allowing nightly
updates without a version bump (including installing an older nightly).

# Build the Windows installer and a portable zip.
#
#   packaging\windows\build-msi.ps1 -Target x86_64-pc-windows-msvc
#   packaging\windows\build-msi.ps1 -Target aarch64-pc-windows-msvc
#
# Expects the release binary for that target to be built already, and the WiX
# CLI on PATH (dotnet tool install --global wix).
param(
    [string]$Target = "x86_64-pc-windows-msvc"
)
$ErrorActionPreference = "Stop"

$root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
$arch = if ($Target -like "aarch64*") { "arm64" } else { "x64" }
$binary = "target\$Target\release\limedaw.exe"

if (-not (Test-Path $binary)) {
    throw "build first: cargo build --release --target $Target"
}

New-Item -ItemType Directory -Force -Path dist | Out-Null

# The portable build: no installer, just the executable and its docs.
$portable = "dist\LimeDAW-$version-windows-$arch-portable.zip"
$stage = "target\portable"
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item $binary "$stage\limedaw.exe"
Copy-Item README.md, LICENSE $stage
Compress-Archive -Path "$stage\*" -DestinationPath $portable -Force
Write-Host "wrote $portable"

# WiX wants an RTF licence; wrap the plain text in a minimal one.
$licenseRtf = "target\LICENSE.rtf"
$text = (Get-Content LICENSE -Raw) -replace '\\', '\\\\' -replace '{', '\{' -replace '}', '\}'
$text = $text -replace "`r`n", "\par`r`n" -replace "`n", "\par`n"
"{\rtf1\ansi\deff0{\fonttbl{\f0\fnil\fcharset0 Courier New;}}\fs16 $text }" |
    Set-Content -Path $licenseRtf -Encoding ASCII

$msi = "dist\LimeDAW-$version-windows-$arch.msi"
& wix build packaging\windows\limedaw.wxs `
    -arch $arch `
    -ext WixToolset.UI.wixext `
    -d "Version=$version" `
    -d "BinaryPath=$root\$binary" `
    -d "IconPath=$root\assets\limedaw.ico" `
    -d "LicensePath=$root\$licenseRtf" `
    -d "ReadmePath=$root\README.md" `
    -o $msi

if ($LASTEXITCODE -ne 0) { throw "wix build failed" }
Write-Host "wrote $msi"

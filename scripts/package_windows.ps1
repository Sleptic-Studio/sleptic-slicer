# Copyright (c) 2026 Sleptic
# SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
# Run from any directory with Windows PowerShell 5.1 or PowerShell 7.
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath("$PSScriptRoot/..")
Push-Location $root
try {
    & "$PSScriptRoot/render_sleptic_icon.ps1"
    $ErrorActionPreference = 'Continue'
    & "$root/scripts/cargo-msvc.bat" build -p sc-slicer-gui --release --offline
    $buildExit = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($buildExit -ne 0) { throw 'Offline release build failed' }
    $manifest = Get-Content -LiteralPath "$root/apps/slicer-gui/Cargo.toml" -Raw
    $version = [regex]::Match($manifest, '(?m)^version = "([^"]+)"').Groups[1].Value
    if (!$version) { throw 'Missing version' }
    $dist = Join-Path $root 'target/dist'
    New-Item -ItemType Directory -Force $dist | Out-Null
    # A fresh staging directory prevents stale files from previous packages.
    $stage = Join-Path $dist ('stage-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force "$stage/slicer" | Out-Null
    Copy-Item -LiteralPath "$root/target/release/sc-slicer-gui.exe" -Destination "$stage/Sleptic Slicer.exe"
    Copy-Item -LiteralPath "$root/assets/profiles" -Destination "$stage/slicer/profiles" -Recurse
    Copy-Item -LiteralPath "$root/apps/slicer-gui/assets/icons/sleptic.ico" -Destination "$stage/sleptic.ico"
    Copy-Item -LiteralPath "$root/README.md" -Destination "$stage/README.md"
    Copy-Item -LiteralPath "$root/apps/slicer-gui/LICENSE.md" -Destination "$stage/LICENSE.md"
    # App-local VC runtime makes the archive usable without a separate VC installer.
    $redistRoot = if ($env:VCToolsRedistDir) { $env:VCToolsRedistDir } else { 'C:/Program Files/Microsoft Visual Studio/2022/Community/VC/Redist/MSVC' }
    $crt = Get-ChildItem -Path "$redistRoot/*/x64/Microsoft.VC143.CRT/vcruntime140.dll", "$redistRoot/x64/Microsoft.VC143.CRT/vcruntime140.dll" -ErrorAction SilentlyContinue | Sort-Object FullName -Descending | Select-Object -First 1
    if (!$crt) { throw 'Offline VC runtime not found; set VCToolsRedistDir to the installed VC redistributables directory.' }
    Copy-Item -Path (Join-Path $crt.DirectoryName '*.dll') -Destination $stage
    $zip = Join-Path $dist "Sleptic-Slicer-$version-windows.zip"
    Compress-Archive -Path "$stage/*" -DestinationPath $zip -Force
    Get-Item -LiteralPath $zip | Select-Object FullName,Length
} finally { Pop-Location }

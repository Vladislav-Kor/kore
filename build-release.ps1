<#
  Builds the portable KORE: one release exe with the C runtime linked in, so no Visual C++ redistributable
  is needed. Puts it into dist\KORE with the guard scripts, the default guard.conf, README and licenses, and zips it.
  Run: powershell -ExecutionPolicy Bypass -File build-release.ps1 [-TargetDir <cargo target dir>]
#>
param([string]$TargetDir = 'target')
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

# --target keeps the static C runtime off build scripts and proc macros.
$env:RUSTFLAGS = '-C target-feature=+crt-static'
cargo build --release --target x86_64-pc-windows-msvc --target-dir $TargetDir
if ($LASTEXITCODE -ne 0) { throw "cargo build failed with code $LASTEXITCODE" }

$out = Join-Path $PSScriptRoot 'dist\KORE'
Remove-Item $out -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Join-Path $out 'guard') | Out-Null
Copy-Item (Join-Path $TargetDir 'x86_64-pc-windows-msvc\release\kore.exe') (Join-Path $out 'KORE.exe')
Copy-Item 'guard\kore-guard.ps1', 'guard\install-guard.ps1', 'guard\guard.conf' (Join-Path $out 'guard')
Copy-Item 'dist-README.txt' (Join-Path $out 'README.txt')
Copy-Item 'LICENSE' (Join-Path $out 'LICENSE.txt')
Copy-Item 'THIRD-PARTY-NOTICES.txt' $out

$zip = Join-Path $PSScriptRoot 'dist\KORE.zip'
Compress-Archive -Path $out -DestinationPath $zip -Force
Write-Host "folder: $out"
Write-Host "zip: $zip"

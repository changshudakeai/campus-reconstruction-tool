# Run the V2 dev app directly from source (debug incremental build),
# sharing the SAME database as the installed desktop dev version
# (%LOCALAPPDATA%\MCRebuildV2\dev\campus-rebuild.db).
# Usage: double-click the desktop shortcut, or run:  .\scripts\run-dev.ps1
# Note: does NOT overwrite the installed dev copy or the desktop shortcut.
# Note: keep this file ASCII-only so Windows PowerShell 5.1 parses it correctly.
# Note: do NOT force SLINT_BACKEND=software here - it renders buttons flat
# gray-white; the desktop shortcut uses the default renderer (colored buttons).

$ErrorActionPreference = 'Stop'
$Repo = Split-Path -Parent $PSScriptRoot   # = New-branch-v2
$Manifest = Join-Path $Repo 'Cargo.toml'

$env:CARGO_BUILD_JOBS = '2'

# The app opens "campus-rebuild.db" relative to its working directory, so run it
# from the dev install dir to share the same DB as the desktop "dev" shortcut.
$DevDir = Join-Path $env:LOCALAPPDATA 'MCRebuildV2\dev'
if (-not (Test-Path -LiteralPath $DevDir)) {
    New-Item -ItemType Directory -Force -Path $DevDir | Out-Null
}
Set-Location $DevDir

Write-Host '==> cargo run --manifest-path <repo>\Cargo.toml -p desktop-shell --bin campus-tool-dev'
Write-Host ("    working dir (DB): " + $DevDir)
cargo run --manifest-path $Manifest -p desktop-shell --bin campus-tool-dev

if ($LASTEXITCODE -ne 0) {
    Write-Host ("cargo run exit code: " + $LASTEXITCODE)
    Read-Host 'Press Enter to close'
}
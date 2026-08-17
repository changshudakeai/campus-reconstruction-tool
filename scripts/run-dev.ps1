# Run the V2 dev app directly from source (debug incremental build).
# Usage: double-click the desktop shortcut, or run:  .\scripts\run-dev.ps1
# Note: does NOT overwrite the installed dev copy or the desktop shortcut.
# Note: keep this file ASCII-only so Windows PowerShell 5.1 parses it correctly.

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot   # = New-branch-v2
Set-Location $Root

$env:CARGO_BUILD_JOBS = '2'
$env:SLINT_BACKEND = 'software'

Write-Host '==> cargo run -p desktop-shell --bin campus-tool-dev'
cargo run -p desktop-shell --bin campus-tool-dev

if ($LASTEXITCODE -ne 0) {
    Write-Host ("cargo run exit code: " + $LASTEXITCODE)
    Read-Host 'Press Enter to close'
}

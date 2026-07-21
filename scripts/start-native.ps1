param(
    [switch]$PrepareOnly
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$targetRoot = Join-Path $root "native\target"
$requiredExecutables = @(
    "campus-native.exe",
    "campus-map.exe",
    "campus-preview.exe"
)

function Test-CompleteRuntime([string]$directory) {
    foreach ($name in $requiredExecutables) {
        if (-not (Test-Path -LiteralPath (Join-Path $directory $name) -PathType Leaf)) {
            return $false
        }
    }
    return $true
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "Rust toolchain was not found. Install Rust to run the source workspace, or use scripts\run-installed.ps1 for an installed application."
}

& cargo +stable build --manifest-path (Join-Path $root "native\Cargo.toml") --workspace
if ($LASTEXITCODE -ne 0) {
    throw "Native workspace build exited with code $LASTEXITCODE."
}

$debugDirectory = Join-Path $targetRoot "debug"
if (-not (Test-CompleteRuntime $debugDirectory)) {
    throw "Native workspace build did not produce the main, map, and preview executables together."
}

if (-not $PrepareOnly) {
    Start-Process -FilePath (Join-Path $debugDirectory "campus-native.exe")
}

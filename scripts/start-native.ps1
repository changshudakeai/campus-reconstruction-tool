$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$release = Join-Path $root "native\target\release\campus-native.exe"

if (Test-Path -LiteralPath $release) {
    Start-Process -FilePath $release
    exit 0
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "No packaged application or Rust toolchain was found. Install the V1 package from artifacts\installer."
}

Push-Location $root
try {
    & cargo +stable run --manifest-path native\Cargo.toml -p campus-native
    if ($LASTEXITCODE -ne 0) { throw "Native application exited with code $LASTEXITCODE." }
}
finally {
    Pop-Location
}

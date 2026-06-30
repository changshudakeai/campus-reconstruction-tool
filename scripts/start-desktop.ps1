$ErrorActionPreference = "Stop"
& (Join-Path $PSScriptRoot "start-native.ps1")
if ($LASTEXITCODE -ne 0) {
    throw "Native desktop entry exited with code $LASTEXITCODE."
}

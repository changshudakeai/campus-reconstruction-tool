$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$target = Join-Path $root "native\target"
$installerPath = Join-Path $root "artifacts\installer\Campus-Reconstruction-Tool-V1-Setup.exe"

function Measure-Directory([string]$Path) {
    if (-not (Test-Path $Path)) { return 0 }
    return (Get-ChildItem $Path -Recurse -File | Measure-Object Length -Sum).Sum
}

$debugBytes = Measure-Directory (Join-Path $target "debug")
$releaseBytes = Measure-Directory (Join-Path $target "release")
$installer = Get-Item -LiteralPath $installerPath -ErrorAction SilentlyContinue

Write-Host ("Rust debug cache:   {0:N2} GB" -f ($debugBytes / 1GB))
Write-Host ("Rust release cache: {0:N2} GB" -f ($releaseBytes / 1GB))
if ($installer) {
    Write-Host ("Installer: {0} ({1:N2} MB)" -f $installer.Name, ($installer.Length / 1MB))
}

if ($installer -and $installer.Length -gt 50MB) {
    throw "Release installer exceeded the 50 MB V1 budget."
}

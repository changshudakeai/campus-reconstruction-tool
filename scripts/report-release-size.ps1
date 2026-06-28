$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$target = Join-Path $root "src-tauri\target"

function Measure-Directory([string]$Path) {
    if (-not (Test-Path $Path)) { return 0 }
    return (Get-ChildItem $Path -Recurse -File | Measure-Object Length -Sum).Sum
}

$debugBytes = Measure-Directory (Join-Path $target "debug")
$releaseBytes = Measure-Directory (Join-Path $target "release")
$bundles = Get-ChildItem (Join-Path $target "release\bundle") -Recurse -File -ErrorAction SilentlyContinue |
    Where-Object { $_.Extension -in @(".exe", ".msi") }

Write-Host ("Rust debug cache:   {0:N2} GB" -f ($debugBytes / 1GB))
Write-Host ("Rust release cache: {0:N2} GB" -f ($releaseBytes / 1GB))
foreach ($bundle in $bundles) {
    Write-Host ("Installer: {0} ({1:N2} MB)" -f $bundle.Name, ($bundle.Length / 1MB))
}

if ($bundles -and ($bundles | Measure-Object Length -Maximum).Maximum -gt 50MB) {
    throw "Release installer exceeded the 50 MB V1 budget."
}

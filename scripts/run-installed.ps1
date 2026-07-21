param(
    [switch]$PrepareOnly
)

$ErrorActionPreference = "Stop"
$installDirectory = Join-Path $env:LOCALAPPDATA "Programs\Campus Reconstruction Tool"
$application = Join-Path $installDirectory "campus-native.exe"

if (-not (Test-Path -LiteralPath $application -PathType Leaf)) {
    throw "Installed application was not found at '$application'. Install the package first, or use npm run dev for the source workspace."
}

if (-not $PrepareOnly) {
    Start-Process -FilePath $application
}

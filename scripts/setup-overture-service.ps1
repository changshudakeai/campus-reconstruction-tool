$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$venv = Join-Path $root ".venv-overture"

if (-not (Test-Path (Join-Path $venv "Scripts\python.exe"))) {
  Write-Host "Creating the Overture Python environment..."
  python -m venv $venv
}

Write-Host "Installing pinned Overture service dependencies..."
& (Join-Path $venv "Scripts\python.exe") -m pip install --disable-pip-version-check -r (Join-Path $root "services\overture-requirements.txt")
Write-Host "Overture service setup complete."

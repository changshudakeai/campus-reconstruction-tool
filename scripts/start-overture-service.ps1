$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$python = Join-Path $root ".venv-overture\Scripts\python.exe"
if (-not (Test-Path $python)) {
  & (Join-Path $PSScriptRoot "setup-overture-service.ps1")
}
& $python (Join-Path $root "services\overture_bridge.py")

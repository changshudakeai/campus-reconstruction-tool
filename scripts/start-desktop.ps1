$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$health = "http://127.0.0.1:8765/health"
$bridge = $null

try {
  try { Invoke-RestMethod -Uri $health -TimeoutSec 2 | Out-Null } catch {
    $serviceScript = Join-Path $PSScriptRoot "start-overture-service.ps1"
    $bridge = Start-Process powershell -ArgumentList "-NoProfile -ExecutionPolicy Bypass -File `"$serviceScript`"" -WindowStyle Hidden -PassThru
    for ($attempt = 0; $attempt -lt 30; $attempt++) {
      Start-Sleep -Milliseconds 500
      try { Invoke-RestMethod -Uri $health -TimeoutSec 2 | Out-Null; break } catch {}
    }
    Invoke-RestMethod -Uri $health -TimeoutSec 2 | Out-Null
  }
  $env:OVERTURE_BUILDING_ENDPOINT = "http://127.0.0.1:8765/overture/buildings"
  $env:OVERTURE_RELEASE_ID = "latest"
  Set-Location $root
  & npm.cmd run tauri -- dev
} finally {
  if ($bridge -and -not $bridge.HasExited) { Stop-Process -Id $bridge.Id }
}

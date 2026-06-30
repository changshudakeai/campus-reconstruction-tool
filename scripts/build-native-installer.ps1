$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$target = Join-Path $root "native\target\release"
$artifact = Join-Path $root "artifacts\installer"
$bundledMakensis = Join-Path $env:LOCALAPPDATA "tauri\NSIS\makensis.exe"
$commandMakensis = Get-Command makensis.exe -ErrorAction SilentlyContinue
$makensisCandidates = @(
    $(if ($commandMakensis) { $commandMakensis.Source }),
    $(if (${env:ProgramFiles(x86)}) { Join-Path ${env:ProgramFiles(x86)} "NSIS\makensis.exe" }),
    $(if ($env:ProgramFiles) { Join-Path $env:ProgramFiles "NSIS\makensis.exe" }),
    $bundledMakensis
) | Where-Object { $_ -and (Test-Path -LiteralPath $_) }
$makensis = $makensisCandidates | Select-Object -First 1

Push-Location $root
try {
    & cargo +stable build --manifest-path native\Cargo.toml --workspace --release --locked
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Online build failed; retrying from the local Cargo cache..."
        & cargo +stable build --manifest-path native\Cargo.toml --workspace --release --locked --offline
    }
    if ($LASTEXITCODE -ne 0) { throw "Rust release build failed." }

    New-Item -ItemType Directory -Force -Path $artifact | Out-Null
    if (-not $makensis) {
        throw "NSIS compiler was not found in PATH, Program Files, or the bundled Tauri cache."
    }
    & $makensis (Join-Path $root "installer\campus-reconstruction-tool.nsi")
    if ($LASTEXITCODE -ne 0) { throw "NSIS packaging failed." }

    $payload = @(
        "campus-native.exe",
        "campus-map.exe",
        "campus-preview.exe"
    ) | ForEach-Object { Get-Item -LiteralPath (Join-Path $target $_) }
    $installer = Get-Item -LiteralPath (Join-Path $artifact "Campus-Reconstruction-Tool-V1-Setup.exe")
    $payloadBytes = ($payload | Measure-Object -Property Length -Sum).Sum
    $limit = 50MB
    if ($installer.Length -gt $limit) {
        throw "Installer size $([math]::Round($installer.Length / 1MB, 2)) MB exceeds the 50 MB V1 budget."
    }
    [pscustomobject]@{
        PayloadMB = [math]::Round($payloadBytes / 1MB, 2)
        InstallerMB = [math]::Round($installer.Length / 1MB, 2)
        Installer = $installer.FullName
    } | Format-List
}
finally {
    Pop-Location
}

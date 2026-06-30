param(
    [int]$Cycles = 3
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$installer = Join-Path $root "artifacts\installer\Campus-Reconstruction-Tool-V1-Setup.exe"
$installDir = Join-Path $env:LOCALAPPDATA "Programs\Campus Reconstruction Tool"
$uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CampusReconstructionTool"
$report = Join-Path $env:TEMP "campus-reconstruction-tool-installed-self-test.json"

if (-not (Test-Path -LiteralPath $installer)) {
    throw "Installer does not exist: $installer"
}
if (Test-Path -LiteralPath $installDir -PathType Container -ErrorAction SilentlyContinue) {
    throw "Refusing to replace an existing installation at $installDir"
}
if (Test-Path -LiteralPath $uninstallKey) {
    throw "Refusing to replace an existing Campus Reconstruction Tool uninstall registration"
}

$setup = Start-Process -FilePath $installer -ArgumentList "/S" -Wait -PassThru -WindowStyle Hidden
if ($setup.ExitCode -ne 0) {
    throw "Silent installer exited with code $($setup.ExitCode)"
}

$expected = @(
    "campus-native.exe",
    "campus-map.exe",
    "campus-preview.exe",
    "THIRD_PARTY_NOTICES.md",
    "Uninstall.exe"
)
foreach ($name in $expected) {
    $path = Join-Path $installDir $name
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Installed payload is missing $name"
    }
}
$unexpected = Get-ChildItem -LiteralPath $installDir -File |
    Where-Object { $_.Name -notin $expected }
if ($unexpected) {
    throw "Unexpected installed payload: $($unexpected.Name -join ', ')"
}

$registration = Get-ItemProperty -LiteralPath $uninstallKey
if ($registration.InstallLocation -ne $installDir) {
    throw "Uninstall registration points to '$($registration.InstallLocation)' instead of '$installDir'"
}

Remove-Item -LiteralPath $report -ErrorAction SilentlyContinue
$application = Join-Path $installDir "campus-native.exe"
$selfTest = Start-Process -FilePath $application -ArgumentList @(
    "--self-test",
    "--cycles", [string]$Cycles,
    "--self-test-report", "`"$report`""
) -Wait -PassThru -WindowStyle Hidden
if ($selfTest.ExitCode -ne 0) {
    throw "Installed offline self-test exited with code $($selfTest.ExitCode)"
}
$result = Get-Content -Raw -Encoding UTF8 -LiteralPath $report | ConvertFrom-Json
if ($result.status -ne "pass" -or -not $result.offline -or -not $result.recovery) {
    throw "Installed self-test report did not prove offline recovery"
}
if ($result.arnisGenerations -ne (19 * $Cycles) -or $result.foundationPresets -ne 4) {
    throw "Installed self-test did not exercise every appearance preset"
}

$uninstaller = Join-Path $installDir "Uninstall.exe"
$uninstall = Start-Process -FilePath $uninstaller -ArgumentList "/S" -Wait -PassThru -WindowStyle Hidden
if ($uninstall.ExitCode -ne 0) {
    throw "Silent uninstaller exited with code $($uninstall.ExitCode)"
}
for ($attempt = 0; $attempt -lt 80 -and (Test-Path -LiteralPath $installDir); $attempt += 1) {
    Start-Sleep -Milliseconds 250
}
if (Test-Path -LiteralPath $installDir) {
    throw "Uninstaller left the installation directory behind"
}
if (Test-Path -LiteralPath $uninstallKey) {
    throw "Uninstaller left the uninstall registration behind"
}

[pscustomobject]@{
    Installer = $installer
    Install = "pass"
    InstalledPayload = $expected.Count
    OfflineSelfTestCycles = $Cycles
    ArnisGenerations = $result.arnisGenerations
    Recovery = $result.recovery
    Uninstall = "pass"
    Report = $report
} | Format-List

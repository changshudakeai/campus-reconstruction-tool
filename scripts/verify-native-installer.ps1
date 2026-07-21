param(
    [string]$CandidateDirectory,
    [ValidateSet("Silent", "Interactive")]
    [string]$InstallMode = "Silent",
    [ValidateSet("Fresh", "Upgrade")]
    [string]$Scenario = "Fresh",
    [string]$UpgradeFromInstaller
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$supportedPredecessorLabel = "V1.0.1 accepted Windows installer (legacy DisplayVersion 0.1.0)"
$supportedPredecessorSha256 = "e59c1d1e523501db373db51ae0f2167c4d4fd368125dd6d71889ab08ac77e202"
$supportedPredecessorDisplayVersion = "0.1.0"
$root = (Resolve-Path (Split-Path -Parent $PSScriptRoot)).Path
if (-not $CandidateDirectory) {
    $latest = Join-Path $root "artifacts\candidates\latest.txt"
    if (-not (Test-Path -LiteralPath $latest -PathType Leaf)) {
        throw "CandidateDirectory is required when no latest candidate exists"
    }
    $CandidateDirectory = (Get-Content -Raw -Encoding UTF8 -LiteralPath $latest).Trim()
}
$candidate = (Resolve-Path -LiteralPath $CandidateDirectory).Path
$distributionPath = Join-Path $candidate "distribution.json"
$manifestPath = Join-Path $candidate "release-candidate.json"
if (-not (Test-Path -LiteralPath $distributionPath -PathType Leaf)) {
    throw "Candidate distribution record does not exist: $distributionPath"
}
$distribution = Get-Content -Raw -Encoding UTF8 -LiteralPath $distributionPath | ConvertFrom-Json
$manifest = Get-Content -Raw -Encoding UTF8 -LiteralPath $manifestPath | ConvertFrom-Json
$installer = Join-Path $candidate $distribution.installer
$installDir = Join-Path $env:LOCALAPPDATA "Programs\Campus Reconstruction Tool"
$uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CampusReconstructionTool"
$smokeReport = Join-Path $env:TEMP "campus-reconstruction-tool-v1.1-candidate-smoke.json"

if (-not (Test-Path -LiteralPath $installer -PathType Leaf)) {
    throw "Installer does not exist: $installer"
}
$installerHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $installer).Hash.ToLowerInvariant()
if ($installerHash -ne $distribution.installerSha256) {
    throw "Installer SHA-256 does not match distribution.json"
}
if (Test-Path -LiteralPath $installDir -PathType Container -ErrorAction SilentlyContinue) {
    throw "Refusing to replace an existing installation at $installDir"
}
if (Test-Path -LiteralPath $uninstallKey) {
    throw "Refusing to replace an existing uninstall registration"
}
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "Installed acceptance must run from a non-elevated standard-user process"
}

$upgradeFromVersion = $null
$upgradeFromSha256 = $null
if ($Scenario -eq "Upgrade") {
    if (-not $UpgradeFromInstaller) {
        throw "UpgradeFromInstaller is required for the Upgrade scenario"
    }
    $predecessor = (Resolve-Path -LiteralPath $UpgradeFromInstaller).Path
    if ($predecessor -eq $installer) {
        throw "The predecessor installer must be different from the V1.1 candidate"
    }
    $upgradeFromSha256 = (
        Get-FileHash -Algorithm SHA256 -LiteralPath $predecessor
    ).Hash.ToLowerInvariant()
    if ($upgradeFromSha256 -ne $supportedPredecessorSha256) {
        throw "Predecessor SHA-256 is not the supported V1.0.1 installer baseline"
    }
    $predecessorSetup = Start-Process -FilePath $predecessor -ArgumentList "/S" -Wait -PassThru -WindowStyle Hidden
    if ($predecessorSetup.ExitCode -ne 0) {
        throw "Predecessor installer exited with code $($predecessorSetup.ExitCode)"
    }
    if (-not (Test-Path -LiteralPath $uninstallKey)) {
        throw "Predecessor installer did not create uninstall registration"
    }
    $upgradeFromVersion = (Get-ItemProperty -LiteralPath $uninstallKey).DisplayVersion
    if ($upgradeFromVersion -ne $supportedPredecessorDisplayVersion) {
        throw "Predecessor version '$upgradeFromVersion' does not prove a supported upgrade"
    }
}

$setup = if ($InstallMode -eq "Silent") {
    Start-Process -FilePath $installer -ArgumentList "/S" -Wait -PassThru -WindowStyle Hidden
} else {
    Start-Process -FilePath $installer -Wait -PassThru
}
if ($setup.ExitCode -ne 0) {
    throw "$InstallMode installer exited with code $($setup.ExitCode)"
}

$expected = @(
    "campus-native.exe",
    "campus-map.exe",
    "campus-preview.exe",
    "THIRD_PARTY_NOTICES.md",
    "release-candidate.json",
    "V1.1.0-RELEASE-NOTES.md",
    "Uninstall.exe"
)
foreach ($name in $expected) {
    if (-not (Test-Path -LiteralPath (Join-Path $installDir $name) -PathType Leaf)) {
        throw "Installed payload is missing $name"
    }
}
$installedFiles = @(Get-ChildItem -LiteralPath $installDir -Recurse -File)
$unexpected = @($installedFiles | Where-Object { $_.Name -notin $expected })
if ($unexpected.Count -ne 0) {
    throw "Unexpected installed payload: $($unexpected.FullName -join ', ')"
}
$forbidden = @($installedFiles | Where-Object {
    $_.Name -match '(credential|secret|fixture|test-project|cargo|rustc|python|node)' -or
    $_.FullName -match '(cache|target)'
})
if ($forbidden.Count -ne 0) {
    throw "Installed payload contains a forbidden credential, fixture, cache, or toolchain artifact"
}

$registration = Get-ItemProperty -LiteralPath $uninstallKey
if ($registration.DisplayVersion -ne "1.1.0") {
    throw "Uninstall registration reports '$($registration.DisplayVersion)' instead of 1.1.0"
}
if ($registration.InstallLocation -ne $installDir) {
    throw "Uninstall registration points to '$($registration.InstallLocation)' instead of '$installDir'"
}
$mainVersion = (Get-Item -LiteralPath (Join-Path $installDir "campus-native.exe")).VersionInfo.ProductVersion
if (-not $mainVersion.StartsWith("1.1.0")) {
    throw "Installed application ProductVersion is '$mainVersion'"
}
$installedManifest = Get-Content -Raw -Encoding UTF8 -LiteralPath (Join-Path $installDir "release-candidate.json") | ConvertFrom-Json
if ($installedManifest.candidateId -ne $manifest.candidateId -or $installedManifest.commit -ne $manifest.commit) {
    throw "Installed candidate manifest does not match the packaged candidate"
}
foreach ($name in @("campus-native.exe", "campus-map.exe", "campus-preview.exe")) {
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $installDir $name)).Hash.ToLowerInvariant()
    $expectedHash = $manifest.binaryDigests.PSObject.Properties[$name].Value
    if ($actual -ne $expectedHash) {
        throw "Installed SHA-256 mismatch for $name"
    }
}

Remove-Item -LiteralPath $smokeReport -ErrorAction SilentlyContinue
$application = Join-Path $installDir "campus-native.exe"
$smoke = Start-Process -FilePath $application -ArgumentList @(
    "--candidate-smoke-report", $smokeReport
) -Wait -PassThru -WindowStyle Hidden
if ($smoke.ExitCode -ne 0) {
    throw "Installed three-process candidate smoke exited with code $($smoke.ExitCode)"
}
$smokeResult = Get-Content -Raw -Encoding UTF8 -LiteralPath $smokeReport | ConvertFrom-Json
if (
    $smokeResult.status -ne "pass" -or
    $smokeResult.version -ne "1.1.0" -or
    $smokeResult.productionProjectModel -ne "schema-2-only" -or
    $smokeResult.helpers.campusMap -ne "started-and-shut-down" -or
    $smokeResult.helpers.campusPreview -ne "started-and-shut-down"
) {
    throw "Installed candidate smoke did not prove the V1.1 three-process contract"
}

$firstLaunch = Start-Process -FilePath $application -PassThru
Start-Sleep -Seconds 4
if ($firstLaunch.HasExited) {
    throw "Installed application exited during first launch with code $($firstLaunch.ExitCode)"
}
$null = $firstLaunch.CloseMainWindow()
if (-not $firstLaunch.WaitForExit(10000)) {
    $firstLaunch.Kill()
    $firstLaunch.WaitForExit()
    throw "Installed application did not complete a normal window shutdown"
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

$acceptance = [ordered]@{
    candidateId = $manifest.candidateId
    version = "1.1.0"
    upgradeFromBaseline = if ($Scenario -eq "Upgrade") { $supportedPredecessorLabel } else { $null }
    scenario = $Scenario
    installMode = $InstallMode
    standardUser = $true
    payloadFiles = $expected
    firstLaunch = "pass"
    threeProcessStartup = "pass"
    normalShutdown = "pass"
    supportedUpgrade = if ($Scenario -eq "Upgrade") { "pass" } else { "not-applicable" }
    upgradeFromVersion = $upgradeFromVersion
    upgradeFromInstallerSha256 = $upgradeFromSha256
    uninstall = "pass"
    installerSha256 = $installerHash
    completedUtc = (Get-Date).ToUniversalTime().ToString("o")
}
$utf8 = [System.Text.UTF8Encoding]::new($false)
$acceptanceName = "installed-acceptance-$($Scenario.ToLowerInvariant())-$($InstallMode.ToLowerInvariant()).json"
[IO.File]::WriteAllText(
    (Join-Path $candidate $acceptanceName),
    ($acceptance | ConvertTo-Json -Depth 8),
    $utf8
)
$acceptance | Format-List

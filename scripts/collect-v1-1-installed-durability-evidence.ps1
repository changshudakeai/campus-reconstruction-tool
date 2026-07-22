param(
    [Parameter(Mandatory = $true)][string]$CandidateDirectory,
    [string]$InstallDirectory = (Join-Path $env:LOCALAPPDATA "Programs\Campus Reconstruction Tool"),
    [string]$EvidenceDirectory,
    [ValidateRange(0, 10000)][int]$ReliabilityCycles = 120,
    [switch]$DevelopmentShortRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$candidate = (Resolve-Path -LiteralPath $CandidateDirectory).Path
$installed = (Resolve-Path -LiteralPath $InstallDirectory).Path
$manifestPath = Join-Path $candidate "release-candidate.json"
$distributionPath = Join-Path $candidate "distribution.json"
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf) -or
    -not (Test-Path -LiteralPath $distributionPath -PathType Leaf)) {
    throw "Candidate is missing release-candidate.json or distribution.json"
}
if ($ReliabilityCycles -lt 120 -and -not $DevelopmentShortRun) {
    throw "Formal installed reliability evidence requires at least 120 cycles"
}
$manifest = Get-Content -Raw -Encoding UTF8 -LiteralPath $manifestPath | ConvertFrom-Json
$installedManifestPath = Join-Path $installed "release-candidate.json"
if (-not (Test-Path -LiteralPath $installedManifestPath -PathType Leaf)) {
    throw "Installed candidate manifest was not found"
}
$installedManifest = Get-Content -Raw -Encoding UTF8 -LiteralPath $installedManifestPath | ConvertFrom-Json
if ($installedManifest.candidateId -ne $manifest.candidateId -or $installedManifest.commit -ne $manifest.commit) {
    throw "Installed candidate identity does not match the selected candidate"
}

$binaryDigests = [ordered]@{}
foreach ($name in @("campus-native.exe", "campus-map.exe", "campus-preview.exe")) {
    $path = Join-Path $installed $name
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Installed candidate is missing $name"
    }
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
    $expected = $manifest.binaryDigests.PSObject.Properties[$name].Value
    if ($actual -ne $expected) {
        throw "Installed binary digest does not match the candidate: $name"
    }
    $binaryDigests[$name] = $actual
}

if (-not $EvidenceDirectory) {
    $suffix = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
    $EvidenceDirectory = Join-Path $candidate "installed-durability-$suffix"
}
if (Test-Path -LiteralPath $EvidenceDirectory) {
    throw "Evidence directory already exists: $EvidenceDirectory"
}
$evidence = New-Item -ItemType Directory -Path $EvidenceDirectory
$reportPath = Join-Path $evidence.FullName "installed-durability-report.json"
$application = Join-Path $installed "campus-native.exe"
$stdoutPath = Join-Path $evidence.FullName "installed-durability.stdout.log"
$stderrPath = Join-Path $evidence.FullName "installed-durability.stderr.log"
$argumentLine = @(
    "--installed-durability-report",
    '"' + ($reportPath -replace '"', '\"') + '"',
    "--reliability-cycles",
    $ReliabilityCycles
) -join " "
$process = Start-Process -FilePath $application -ArgumentList $argumentLine `
    -Wait -PassThru -WindowStyle Hidden `
    -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
if (-not (Test-Path -LiteralPath $reportPath -PathType Leaf)) {
    $failureDetail = (Get-Content -Raw -LiteralPath $stderrPath -ErrorAction SilentlyContinue).Trim()
    throw "Installed candidate did not produce a durability report (exit $($process.ExitCode)): $failureDetail"
}
$report = Get-Content -Raw -Encoding UTF8 -LiteralPath $reportPath | ConvertFrom-Json
if ($report.version -ne "1.1.0" -or $report.architecture -ne "x86_64" -or $report.cases.Count -ne 8 -or $report.reliabilityCyclesRequired -ne $ReliabilityCycles) {
    throw "Installed durability report has the wrong version, architecture, or mandatory case count"
}
foreach ($case in $report.cases) {
    if (-not $case.mandatory -or
        $case.status -notin @("pass", "fail") -or
        $case.inputDigestSha256 -notmatch '^[0-9a-f]{64}$' -or
        $case.projectSummaryDigestSha256 -notmatch '^[0-9a-f]{64}$' -or
        -not $case.failurePoint -or
        -not $case.eventIds -or
        -not $case.resultEvidence) {
        throw "Installed durability case is incomplete: $($case.caseId)"
    }
}

$reportText = Get-Content -Raw -Encoding UTF8 -LiteralPath $reportPath
foreach ($secretName in @("GAODE_JS_API_KEY", "VITE_GAODE_JS_API_KEY", "GAODE_SECURITY_CODE", "VITE_GAODE_SECURITY_CODE", "CAMPUS_ACQUISITION_SERVICE_SECRET")) {
    $secretValue = [Environment]::GetEnvironmentVariable($secretName)
    if ($secretValue -and $reportText.Contains($secretValue)) {
        throw "Installed evidence leaked a configured secret"
    }
}

$formal = $ReliabilityCycles -ge 120 -and -not $DevelopmentShortRun
$status = if ($process.ExitCode -eq 0 -and $report.status -eq "pass" -and $formal) {
    "pass"
} elseif ($process.ExitCode -eq 0 -and $report.status -eq "pass") {
    "development-only"
} else {
    "fail"
}
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
$windows = Get-ItemProperty -LiteralPath "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion"
$envelope = [ordered]@{
    candidateId = $manifest.candidateId
    commit = $manifest.commit
    version = $manifest.version
    status = $status
    formalEvidence = $formal
    standardUser = -not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    windowsBuild = [int]$windows.CurrentBuildNumber
    architecture = "x64"
    installedDirectoryName = (Split-Path -Leaf $installed)
    binaryDigests = $binaryDigests
    reliabilityCyclesRequired = 120
    reliabilityCyclesExecuted = $ReliabilityCycles
    elapsedWallClockMs = [uint64]($report.finishedUtcUnixMs - $report.startedUtcUnixMs)
    applicationExitCode = $process.ExitCode
    report = "installed-durability-report.json"
    reportSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $reportPath).Hash.ToLowerInvariant()
    releaseBlockers = @($report.releaseBlockers)
    completedUtc = (Get-Date).ToUniversalTime().ToString("o")
}
$utf8 = [System.Text.UTF8Encoding]::new($false)
$envelopePath = Join-Path $evidence.FullName "installed-durability-evidence.json"
[IO.File]::WriteAllText($envelopePath, ($envelope | ConvertTo-Json -Depth 12), $utf8)
if ($status -eq "fail") {
    throw "One or more mandatory durability, migration, or reliability cases are Release Blockers"
}
Write-Host "Installed durability evidence: $envelopePath ($status)"

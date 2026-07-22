param(
    [Parameter(Mandatory = $true)][string]$CandidateDirectory,
    [Parameter(Mandatory = $true)][string]$OperatorRecord,
    [string]$InstallDirectory = (Join-Path $env:LOCALAPPDATA "Programs\Campus Reconstruction Tool"),
    [string]$EvidenceDirectory
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$candidate = (Resolve-Path -LiteralPath $CandidateDirectory).Path
$installed = (Resolve-Path -LiteralPath $InstallDirectory).Path
$operatorRecordPath = (Resolve-Path -LiteralPath $OperatorRecord).Path
$manifestPath = Join-Path $candidate "release-candidate.json"
$installedManifestPath = Join-Path $installed "release-candidate.json"
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf) -or
    -not (Test-Path -LiteralPath $installedManifestPath -PathType Leaf)) {
    throw "Candidate or installed release-candidate.json is missing"
}
$manifest = Get-Content -Raw -Encoding UTF8 -LiteralPath $manifestPath | ConvertFrom-Json
$installedManifest = Get-Content -Raw -Encoding UTF8 -LiteralPath $installedManifestPath | ConvertFrom-Json
$operator = Get-Content -Raw -Encoding UTF8 -LiteralPath $operatorRecordPath | ConvertFrom-Json
if ($installedManifest.candidateId -ne $manifest.candidateId -or
    $installedManifest.commit -ne $manifest.commit -or
    $operator.candidateId -ne $manifest.candidateId -or
    $operator.commit -ne $manifest.commit) {
    throw "Candidate, installed candidate, and operator-record identities must match exactly"
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
    $EvidenceDirectory = Join-Path $candidate "operator-experience-$suffix"
}
$operatorRoot = Split-Path -Parent $operatorRecordPath
$sourceRoot = [IO.Path]::GetFullPath($operatorRoot).TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
$destinationPath = [IO.Path]::GetFullPath($EvidenceDirectory)
if (($destinationPath + [IO.Path]::DirectorySeparatorChar).StartsWith($sourceRoot, [StringComparison]::OrdinalIgnoreCase)) {
    throw "EvidenceDirectory must not be inside the operator-record source directory"
}
if (Test-Path -LiteralPath $EvidenceDirectory) {
    throw "Evidence directory already exists: $EvidenceDirectory"
}
$evidence = New-Item -ItemType Directory -Path $EvidenceDirectory
$source = New-Item -ItemType Directory -Path (Join-Path $evidence.FullName "operator-record")
Get-ChildItem -Force -LiteralPath $operatorRoot | Copy-Item -Destination $source.FullName -Recurse -Force
$copiedRecord = Join-Path $source.FullName (Split-Path -Leaf $operatorRecordPath)
if (-not (Test-Path -LiteralPath $copiedRecord -PathType Leaf)) {
    throw "Operator record was not copied into the evidence bundle"
}

$reportPath = Join-Path $evidence.FullName "operator-experience-report.json"
$application = Join-Path $installed "campus-native.exe"
$argumentLine = @(
    "--operator-experience-record", '"' + ($copiedRecord -replace '"', '\"') + '"',
    "--operator-experience-report", '"' + ($reportPath -replace '"', '\"') + '"'
) -join " "
$process = Start-Process -FilePath $application -ArgumentList $argumentLine `
    -Wait -PassThru -WindowStyle Hidden
if (-not (Test-Path -LiteralPath $reportPath -PathType Leaf)) {
    throw "Installed candidate did not produce an operator-experience report (exit $($process.ExitCode))"
}
$report = Get-Content -Raw -Encoding UTF8 -LiteralPath $reportPath | ConvertFrom-Json

$credentialLeaks = @()
$textExtensions = @(".json", ".log", ".txt", ".md", ".csv", ".xml", ".yaml", ".yml")
$textFiles = Get-ChildItem -LiteralPath $evidence.FullName -File -Recurse | Where-Object {
    $textExtensions -contains $_.Extension.ToLowerInvariant()
}
foreach ($secretName in @("GAODE_JS_API_KEY", "VITE_GAODE_JS_API_KEY", "GAODE_SECURITY_CODE", "VITE_GAODE_SECURITY_CODE", "CAMPUS_ACQUISITION_SERVICE_SECRET")) {
    $secretValue = [Environment]::GetEnvironmentVariable($secretName)
    if (-not $secretValue) { continue }
    foreach ($file in $textFiles) {
        if ([IO.File]::ReadAllText($file.FullName).Contains($secretValue)) {
            $credentialLeaks += "$secretName in $($file.Name)"
        }
    }
}
$releaseBlockers = @($report.releaseBlockers)
foreach ($leak in $credentialLeaks) {
    $releaseBlockers += [pscustomobject]@{ caseId = "secret-safety"; reason = $leak }
}
$windows = Get-ItemProperty -LiteralPath "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion"
$architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
if ([int]$windows.CurrentBuildNumber -lt 22000 -or $architecture -ne "X64") {
    $releaseBlockers += [pscustomobject]@{
        caseId = "operator-environment"; reason = "Evidence host must be Windows 11 x64"
    }
}
$status = if ($process.ExitCode -eq 0 -and $report.status -eq "pass" -and $releaseBlockers.Count -eq 0) { "pass" } else { "fail" }
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
$windows = Get-ItemProperty -LiteralPath "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion"
$envelope = [ordered]@{
    candidateId = $manifest.candidateId
    commit = $manifest.commit
    version = $manifest.version
    status = $status
    standardUser = -not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    windowsBuild = [int]$windows.CurrentBuildNumber
    architecture = "x64"
    binaryDigests = $binaryDigests
    applicationExitCode = $process.ExitCode
    operatorRecord = "operator-record/$((Split-Path -Leaf $operatorRecordPath))"
    operatorRecordSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $copiedRecord).Hash.ToLowerInvariant()
    report = "operator-experience-report.json"
    reportSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $reportPath).Hash.ToLowerInvariant()
    releaseBlockers = $releaseBlockers
    completedUtc = (Get-Date).ToUniversalTime().ToString("o")
}
$envelope.architecture = $architecture
$utf8 = [System.Text.UTF8Encoding]::new($false)
$envelopePath = Join-Path $evidence.FullName "operator-experience-evidence.json"
[IO.File]::WriteAllText($envelopePath, ($envelope | ConvertTo-Json -Depth 12), $utf8)
if ($status -eq "fail") {
    throw "One or more mandatory non-developer, guidance, localisation, scale, accessibility, or secret-safety cases are Release Blockers"
}
Write-Host "Operator-experience evidence: $envelopePath ($status)"

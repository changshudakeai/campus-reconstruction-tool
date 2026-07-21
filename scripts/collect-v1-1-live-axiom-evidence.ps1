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
$record = (Resolve-Path -LiteralPath $OperatorRecord).Path
$sourceRoot = Split-Path -Parent $record
$manifestPath = Join-Path $candidate "release-candidate.json"
$installedManifestPath = Join-Path $installed "release-candidate.json"
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf) -or
    -not (Test-Path -LiteralPath $installedManifestPath -PathType Leaf)) {
    throw "Candidate or installed candidate manifest was not found"
}
$manifest = Get-Content -Raw -Encoding UTF8 -LiteralPath $manifestPath | ConvertFrom-Json
$installedManifest = Get-Content -Raw -Encoding UTF8 -LiteralPath $installedManifestPath | ConvertFrom-Json
if ($installedManifest.candidateId -ne $manifest.candidateId -or $installedManifest.commit -ne $manifest.commit) {
    throw "Installed candidate identity does not match the selected candidate"
}
$operatorIdentity = Get-Content -Raw -Encoding UTF8 -LiteralPath $record | ConvertFrom-Json
if ($operatorIdentity.candidateId -ne $manifest.candidateId -or $operatorIdentity.commit -ne $manifest.commit) {
    throw "Operator record identity does not match the selected candidate"
}

$binaryDigests = [ordered]@{}
foreach ($name in @("campus-native.exe", "campus-map.exe", "campus-preview.exe")) {
    $path = Join-Path $installed $name
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Installed candidate is missing $name" }
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
    $expected = $manifest.binaryDigests.PSObject.Properties[$name].Value
    if ($actual -ne $expected) { throw "Installed binary digest does not match the candidate: $name" }
    $binaryDigests[$name] = $actual
}

if (-not $EvidenceDirectory) {
    $suffix = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
    $EvidenceDirectory = Join-Path $candidate "live-axiom-$suffix"
}
$destinationPath = [IO.Path]::GetFullPath($EvidenceDirectory)
$sourcePrefix = $sourceRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
if ($destinationPath.StartsWith($sourcePrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "EvidenceDirectory must not be inside the operator-record source directory"
}
if (Test-Path -LiteralPath $destinationPath) { throw "Evidence directory already exists: $destinationPath" }
$evidence = New-Item -ItemType Directory -Path $destinationPath
Get-ChildItem -Force -LiteralPath $sourceRoot | Copy-Item -Destination $evidence.FullName -Recurse
$copiedRecord = Join-Path $evidence.FullName (Split-Path -Leaf $record)
$reportPath = Join-Path $evidence.FullName "live-axiom-report.json"
$application = Join-Path $installed "campus-native.exe"
$argumentLine = @(
    "--live-axiom-operator-record", '"' + ($copiedRecord -replace '"', '\"') + '"',
    "--live-axiom-report", '"' + ($reportPath -replace '"', '\"') + '"'
) -join " "
$process = Start-Process -FilePath $application -ArgumentList $argumentLine -Wait -PassThru -WindowStyle Hidden
if (-not (Test-Path -LiteralPath $reportPath -PathType Leaf)) {
    throw "Installed candidate did not produce a live/Axiom report (exit $($process.ExitCode))"
}
$report = Get-Content -Raw -Encoding UTF8 -LiteralPath $reportPath | ConvertFrom-Json
$status = if ($process.ExitCode -eq 0 -and $report.status -eq "pass" -and @($report.releaseBlockers).Count -eq 0) { "pass" } else { "fail" }
$envelope = [ordered]@{
    candidateId = $manifest.candidateId
    commit = $manifest.commit
    version = $manifest.version
    status = $status
    binaryDigests = $binaryDigests
    operatorRecord = (Split-Path -Leaf $copiedRecord)
    operatorRecordSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $copiedRecord).Hash.ToLowerInvariant()
    report = "live-axiom-report.json"
    reportSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $reportPath).Hash.ToLowerInvariant()
    releaseBlockers = @($report.releaseBlockers)
    completedUtc = (Get-Date).ToUniversalTime().ToString("o")
}
$utf8 = [Text.UTF8Encoding]::new($false)
$envelopePath = Join-Path $evidence.FullName "live-axiom-evidence.json"
[IO.File]::WriteAllText($envelopePath, ($envelope | ConvertTo-Json -Depth 12), $utf8)
if ($status -ne "pass") { throw "One or more mandatory live-service, six-campus, or Axiom cases are Release Blockers" }
Write-Host "Live/Axiom evidence: $envelopePath (pass)"

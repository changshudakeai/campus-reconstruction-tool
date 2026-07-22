param(
    [Parameter(Mandatory = $true)]
    [string]$CandidateDirectory
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Split-Path -Parent $PSScriptRoot)).Path
$candidate = (Resolve-Path -LiteralPath $CandidateDirectory).Path
$manifestPath = Join-Path $candidate "release-candidate.json"
$payload = Join-Path $candidate "tested-binaries"
$releaseNotes = Join-Path $root "docs\releases\v1.1.0-unsigned.md"

if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    throw "Candidate manifest does not exist: $manifestPath"
}
if (-not (Test-Path -LiteralPath $releaseNotes -PathType Leaf)) {
    throw "Unsigned release guidance does not exist: $releaseNotes"
}

$manifest = Get-Content -Raw -Encoding UTF8 -LiteralPath $manifestPath | ConvertFrom-Json
if ($manifest.version -ne "1.1.0") {
    throw "Candidate manifest version '$($manifest.version)' is not 1.1.0"
}
$headCommit = (& git -C $root rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $manifest.commit -ne $headCommit) {
    throw "Candidate commit does not match the checked-out commit"
}
$sourceStatus = @(& git -C $root status --porcelain=v1)
if ($LASTEXITCODE -ne 0) {
    throw "Could not inspect the candidate worktree"
}
if ($sourceStatus.Count -ne 0) {
    throw "Packaging requires the exact clean candidate commit"
}

$payloadNames = @("campus-native.exe", "campus-map.exe", "campus-preview.exe")
$hashesBefore = [ordered]@{}
foreach ($name in $payloadNames) {
    $path = Join-Path $payload $name
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Already-tested candidate payload is missing $name"
    }
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
    $expectedProperty = $manifest.binaryDigests.PSObject.Properties[$name]
    if (-not $expectedProperty -or $expectedProperty.Value -ne $actual) {
        throw "Pre-package SHA-256 mismatch for $name"
    }
    $hashesBefore[$name] = $actual
}

$bundledMakensis = Join-Path $env:LOCALAPPDATA "tauri\NSIS\makensis.exe"
$commandMakensis = Get-Command makensis.exe -ErrorAction SilentlyContinue
$programFilesX86 = [Environment]::GetFolderPath("ProgramFilesX86")
$makensisCandidates = @(
    $(if ($commandMakensis) { $commandMakensis.Source }),
    $(if ($programFilesX86) { Join-Path $programFilesX86 "NSIS\makensis.exe" }),
    $(if ($env:ProgramFiles) { Join-Path $env:ProgramFiles "NSIS\makensis.exe" }),
    $bundledMakensis
) | Where-Object { $_ -and (Test-Path -LiteralPath $_) }
$makensis = $makensisCandidates | Select-Object -First 1
if (-not $makensis) {
    throw "NSIS compiler was not found in PATH, Program Files, or the bundled cache"
}

$installerName = "Campus-Reconstruction-Tool-1.1.0-$($manifest.candidateId)-Windows-x64.exe"
$installer = Join-Path $candidate $installerName
$nsisScript = Join-Path $root "installer\campus-reconstruction-tool.nsi"
$arguments = @(
    "/DPRODUCT_VERSION=1.1.0",
    "/DPAYLOAD_DIR=$payload",
    "/DCANDIDATE_MANIFEST=$manifestPath",
    "/DRELEASE_NOTES=$releaseNotes",
    "/DOUTPUT_FILE=$installer",
    $nsisScript
)
& $makensis @arguments
if ($LASTEXITCODE -ne 0) {
    throw "NSIS packaging failed with exit code $LASTEXITCODE"
}

foreach ($name in $payloadNames) {
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $payload $name)).Hash.ToLowerInvariant()
    if ($hashesBefore[$name] -ne $actual) {
        throw "Packaging changed the already-tested binary $name"
    }
}

$candidateReleaseNotesName = "V1.1.0-RELEASE-NOTES.md"
$candidateReleaseNotes = Join-Path $candidate $candidateReleaseNotesName
Copy-Item -LiteralPath $releaseNotes -Destination $candidateReleaseNotes -Force
$candidateNoticesName = "THIRD_PARTY_NOTICES.md"
Copy-Item -LiteralPath (Join-Path $root $candidateNoticesName) -Destination (Join-Path $candidate $candidateNoticesName) -Force
$knownIssuesName = "KNOWN-ISSUES.json"
$knownIssuesPath = Join-Path $candidate $knownIssuesName
if (-not (Test-Path -LiteralPath $knownIssuesPath)) {
    [IO.File]::WriteAllText($knownIssuesPath, "{`n  `"issues`": []`n}", [Text.UTF8Encoding]::new($false))
}

$installerItem = Get-Item -LiteralPath $installer
$installerSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $installer).Hash.ToLowerInvariant()
$distribution = [ordered]@{
    candidateId = $manifest.candidateId
    version = "1.1.0"
    commit = $headCommit
    sourceClean = $true
    platform = "Windows 11 x64"
    onlineRequired = $true
    signed = $false
    installer = $installerItem.Name
    installerBytes = $installerItem.Length
    installerSha256 = $installerSha256
    releaseNotes = $candidateReleaseNotesName
    knownIssues = $knownIssuesName
    payloadSha256BeforePackaging = $hashesBefore
    payloadSha256AfterPackaging = $hashesBefore
    packagingRebuiltBinaries = $false
    installedAcceptanceStatus = $manifest.installedAcceptance.status
    smartScreenGuidance = "Verify this SHA-256, then choose More info and Run anyway only when it matches."
}
$utf8 = [System.Text.UTF8Encoding]::new($false)
$distributionPath = Join-Path $candidate "distribution.json"
[IO.File]::WriteAllText(
    $distributionPath,
    ($distribution | ConvertTo-Json -Depth 8),
    $utf8
)

$statement = @"
# V1.1.0 candidate distribution

- Candidate ID: $($manifest.candidateId)
- Commit: $headCommit
- Installer: $($installerItem.Name)
- Installer size: $([math]::Round($installerItem.Length / 1MB, 2)) MB (informational; no 50 MB gate)
- SHA-256: $installerSha256
- Platform: Windows 11 x64
- Network: required for campus search and new/refresh Foundation acquisition
- Signature: unsigned
- Installed acceptance: $($manifest.installedAcceptance.status)
- Release notes: $candidateReleaseNotesName
- Known Issues: $knownIssuesName

Source statement: built from the exact clean commit above. NSIS consumed the
already-tested binaries from tested-binaries and their SHA-256 values were
unchanged before and after packaging.

$(if ($manifest.installedAcceptance.status -eq "not-run-user-waived") {
    "This candidate is not installed-acceptance tested. Clean-Windows fresh install, interactive install, upgrade, first launch, shutdown, payload inspection, and uninstall were explicitly waived by the user and must not be treated as passed."
})

SmartScreen: compare the installer SHA-256 with the exact value above. When it
matches the trusted release record, choose More info and Run anyway.
Do not bypass SmartScreen when it differs.
"@
[IO.File]::WriteAllText((Join-Path $candidate "DISTRIBUTION.md"), $statement, $utf8)

[pscustomobject]@{
    CandidateId = $manifest.candidateId
    Installer = $installer
    InstallerMB = [math]::Round($installerItem.Length / 1MB, 2)
    SHA256 = $installerSha256
    RebuiltBinaries = $false
} | Format-List

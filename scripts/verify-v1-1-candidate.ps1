param(
    [string]$CandidateId,
    [string]$CleanWindowsImageId,
    [string]$CleanWindowsImageManifest,
    [string]$UpgradeFromInstaller
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Split-Path -Parent $PSScriptRoot)).Path
if (-not [Environment]::Is64BitOperatingSystem -or $env:OS -ne "Windows_NT") {
    throw "V1.1.0 candidates must be produced on 64-bit Windows"
}
if (-not $CleanWindowsImageId) {
    throw "CleanWindowsImageId is required to identify the clean Windows 11 x64 acceptance image"
}
if (-not $CleanWindowsImageManifest) {
    throw "CleanWindowsImageManifest is required to bind the candidate to an immutable image attestation"
}
$cleanImageManifestPath = (Resolve-Path -LiteralPath $CleanWindowsImageManifest).Path
$cleanImage = Get-Content -Raw -Encoding UTF8 -LiteralPath $cleanImageManifestPath | ConvertFrom-Json
if (
    $cleanImage.imageId -ne $CleanWindowsImageId -or
    $cleanImage.cleanBaseline -ne $true -or
    $cleanImage.architecture -ne "x64" -or
    $cleanImage.snapshotSha256 -notmatch '^[0-9a-fA-F]{64}$' -or
    -not $cleanImage.source
) {
    throw "Clean Windows image manifest must identify a clean x64 baseline, source, and immutable snapshot SHA-256"
}
$cleanImageManifestSha256 = (
    Get-FileHash -Algorithm SHA256 -LiteralPath $cleanImageManifestPath
).Hash.ToLowerInvariant()
$windowsRelease = Get-ItemProperty -LiteralPath "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion"
$windowsBuild = [int]$windowsRelease.CurrentBuildNumber
if ($windowsBuild -lt 22000) {
    throw "V1.1.0 installed acceptance requires Windows 11 (build 22000 or newer)"
}
if ([int]$cleanImage.windowsBuild -ne $windowsBuild) {
    throw "Clean image manifest build does not match the running Windows image"
}
$UpgradeFromInstaller = if ($UpgradeFromInstaller) { $UpgradeFromInstaller } else {
    Join-Path $root "artifacts\installer\Campus-Reconstruction-Tool-V1-Setup.exe"
}

$commit = (& git -C $root rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0) {
    throw "Could not resolve the candidate commit"
}
$sourceStateBefore = @(& git -C $root status --porcelain=v1)
if ($sourceStateBefore.Count -ne 0) {
    throw "Candidate verification requires an exact clean commit"
}
if (-not $CandidateId) {
    $shortCommit = $commit.Substring(0, 12)
    $timestamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
    $CandidateId = "v1.1.0-$shortCommit-$timestamp"
}
if ($CandidateId -notmatch '^[A-Za-z0-9._-]+$') {
    throw "CandidateId may contain only letters, numbers, dot, underscore, and hyphen"
}

$candidate = Join-Path $root "artifacts\candidates\$CandidateId"
if (Test-Path -LiteralPath $candidate) {
    throw "Candidate directory already exists: $candidate"
}
$logs = Join-Path $candidate "logs"
$payload = Join-Path $candidate "tested-binaries"
New-Item -ItemType Directory -Force -Path $logs, $payload | Out-Null
$utf8 = [System.Text.UTF8Encoding]::new($false)
$commandRecords = [System.Collections.Generic.List[object]]::new()

function Invoke-EvidenceCommand {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][string[]]$Arguments
    )
    $log = Join-Path $logs "$Name.log"
    $stdout = Join-Path $logs "$Name.stdout.log"
    $stderr = Join-Path $logs "$Name.stderr.log"
    $started = (Get-Date).ToUniversalTime()
    $display = "$Executable $($Arguments -join ' ')"
    $argumentLine = ($Arguments | ForEach-Object {
        if ($_ -match '[\s"]') {
            '"' + ($_ -replace '"', '\"') + '"'
        } else {
            $_
        }
    }) -join " "
    $process = Start-Process -FilePath $Executable -ArgumentList $argumentLine -WorkingDirectory $root `
        -Wait -PassThru -NoNewWindow -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $exitCode = $process.ExitCode
    $output = @(
        [IO.File]::ReadAllText($stdout),
        [IO.File]::ReadAllText($stderr)
    ) -join [Environment]::NewLine
    [IO.File]::WriteAllText($log, $output, $utf8)
    Write-Host $output
    $finished = (Get-Date).ToUniversalTime()
    $commandRecords.Add([ordered]@{
        name = $Name
        command = $display
        startedUtc = $started.ToString("o")
        finishedUtc = $finished.ToString("o")
        exitCode = $exitCode
        log = "logs/$Name.log"
        stdout = "logs/$Name.stdout.log"
        stderr = "logs/$Name.stderr.log"
    })
    if ($exitCode -ne 0) {
        throw "$Name failed with exit code $exitCode"
    }
}

Invoke-EvidenceCommand "format" "cargo" @(
    "+stable", "fmt", "--manifest-path", "native/Cargo.toml", "--all", "--", "--check"
)
Invoke-EvidenceCommand "clippy" "cargo" @(
    "+stable", "clippy", "--manifest-path", "native/Cargo.toml", "--workspace",
    "--all-targets", "--locked", "--", "-D", "warnings"
)
Invoke-EvidenceCommand "rust-tests" "cargo" @(
    "+stable", "test", "--manifest-path", "native/Cargo.toml", "--workspace", "--locked"
)
Invoke-EvidenceCommand "service-tests" "python" @(
    "-m", "unittest", "discover", "-s", "services", "-p", "test_*.py"
)
Invoke-EvidenceCommand "release-contract" "powershell" @(
    "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
    (Join-Path $PSScriptRoot "test-v1-1-release-contract.ps1")
)
Invoke-EvidenceCommand "release-build" "cargo" @(
    "+stable", "build", "--manifest-path", "native/Cargo.toml", "--workspace",
    "--release", "--locked"
)
Invoke-EvidenceCommand "cargo-metadata" "cargo" @(
    "+stable", "metadata", "--manifest-path", "native/Cargo.toml", "--locked",
    "--offline", "--format-version", "1"
)

$sourceStateAfter = @(& git -C $root status --porcelain=v1)
if ($sourceStateAfter.Count -ne 0) {
    throw "Verification commands changed tracked source state"
}

$releaseMain = Join-Path $root "native\target\release\campus-native.exe"
$releaseMap = Join-Path $root "native\target\release\campus-map.exe"
$releasePreview = Join-Path $root "native\target\release\campus-preview.exe"
foreach ($releaseBinary in @($releaseMain, $releaseMap, $releasePreview)) {
    if (-not (Test-Path -LiteralPath $releaseBinary -PathType Leaf)) {
        throw "Locked release build did not produce $releaseBinary"
    }
}
$prepackageSmoke = Join-Path $candidate "prepackage-binary-smoke.json"
Invoke-EvidenceCommand "release-binary-smoke" $releaseMain @(
    "--candidate-smoke-report", $prepackageSmoke
)
$prepackageResult = Get-Content -Raw -Encoding UTF8 -LiteralPath $prepackageSmoke | ConvertFrom-Json
if ($prepackageResult.status -ne "pass" -or $prepackageResult.version -ne "1.1.0") {
    throw "The exact release binaries did not pass the three-process candidate smoke"
}

$binaryDigests = [ordered]@{}
foreach ($name in @("campus-native.exe", "campus-map.exe", "campus-preview.exe")) {
    $built = Join-Path $root "native\target\release\$name"
    if (-not (Test-Path -LiteralPath $built -PathType Leaf)) {
        throw "Locked release build did not produce $name"
    }
    $tested = Join-Path $payload $name
    Copy-Item -LiteralPath $built -Destination $tested
    $binaryDigests[$name] = (Get-FileHash -Algorithm SHA256 -LiteralPath $tested).Hash.ToLowerInvariant()
}

$rustTestCount = 0
foreach ($match in (Select-String -Path (Join-Path $logs "rust-tests.log") -Pattern 'test result: ok\. ([0-9]+) passed' -AllMatches).Matches) {
    $rustTestCount += [int]$match.Groups[1].Value
}
$serviceTestCount = 0
$serviceMatch = Select-String -Path (Join-Path $logs "service-tests.log") -Pattern 'Ran ([0-9]+) tests' -AllMatches
foreach ($match in $serviceMatch.Matches) {
    $serviceTestCount += [int]$match.Groups[1].Value
}

$windowsImage = "$($windowsRelease.ProductName) $($windowsRelease.DisplayVersion) build $windowsBuild"
$toolchains = [ordered]@{
    rustc = (& rustc +stable --version).Trim()
    cargo = (& cargo +stable --version).Trim()
    python = (& python --version 2>&1).Trim()
    powershell = $PSVersionTable.PSVersion.ToString()
}
$manifest = [ordered]@{
    candidateId = $CandidateId
    version = "1.1.0"
    commit = $commit
    sourceClean = $true
    sourceStatusBefore = @()
    sourceStatusAfter = @()
    windowsImage = $windowsImage
    cleanWindowsImageId = $CleanWindowsImageId
    cleanWindowsImage = [ordered]@{
        imageId = $CleanWindowsImageId
        manifestSha256 = $cleanImageManifestSha256
        snapshotSha256 = $cleanImage.snapshotSha256.ToLowerInvariant()
        source = $cleanImage.source
        cleanBaseline = $true
    }
    windowsBuild = $windowsBuild
    architecture = "x64"
    onlineRequired = $true
    productionProjectModel = "schema-2-only"
    createdUtc = (Get-Date).ToUniversalTime().ToString("o")
    toolchains = $toolchains
    dependencyVersions = [ordered]@{
        cargoMetadata = "logs/cargo-metadata.log"
        cargoLockSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $root "native\Cargo.lock")).Hash.ToLowerInvariant()
        packageLockSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $root "package-lock.json")).Hash.ToLowerInvariant()
    }
    commands = $commandRecords
    testCounts = [ordered]@{
        rust = $rustTestCount
        service = $serviceTestCount
        total = $rustTestCount + $serviceTestCount
    }
    binaryDigests = $binaryDigests
    packagingContract = [ordered]@{
        consumesAlreadyTestedBinaries = $true
        rebuildsDuringPackaging = $false
        verifiesBeforeAndAfterPackaging = $true
    }
}
$manifestPath = Join-Path $candidate "release-candidate.json"
[IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json -Depth 12), $utf8)

$packageScript = Join-Path $PSScriptRoot "build-native-installer.ps1"
$verifyInstallerScript = Join-Path $PSScriptRoot "verify-native-installer.ps1"
Invoke-EvidenceCommand "package" "powershell" @(
    "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $packageScript,
    "-CandidateDirectory", $candidate
)
Invoke-EvidenceCommand "install-silent-fresh" "powershell" @(
    "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $verifyInstallerScript,
    "-CandidateDirectory", $candidate, "-Scenario", "Fresh", "-InstallMode", "Silent"
)
Invoke-EvidenceCommand "install-interactive-fresh" "powershell" @(
    "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $verifyInstallerScript,
    "-CandidateDirectory", $candidate, "-Scenario", "Fresh", "-InstallMode", "Interactive"
)
Invoke-EvidenceCommand "install-silent-upgrade" "powershell" @(
    "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $verifyInstallerScript,
    "-CandidateDirectory", $candidate, "-Scenario", "Upgrade", "-InstallMode", "Silent",
    "-UpgradeFromInstaller", $UpgradeFromInstaller
)

$sourceStateFinal = @(& git -C $root status --porcelain=v1)
if ($sourceStateFinal.Count -ne 0) {
    throw "Packaging or installed acceptance changed tracked source state"
}
$acceptanceFiles = @(
    Get-ChildItem -LiteralPath $candidate -Filter "installed-acceptance-*.json" -File |
        Sort-Object Name |
        ForEach-Object { $_.Name }
)
if ($acceptanceFiles.Count -ne 3) {
    throw "Expected three distinct installed-acceptance records"
}
$finalEvidence = [ordered]@{
    candidateId = $CandidateId
    version = "1.1.0"
    commit = $commit
    sourceClean = $true
    cleanWindowsImageId = $CleanWindowsImageId
    windowsImage = $windowsImage
    cleanWindowsImage = [ordered]@{
        imageId = $CleanWindowsImageId
        manifestSha256 = $cleanImageManifestSha256
        snapshotSha256 = $cleanImage.snapshotSha256.ToLowerInvariant()
        source = $cleanImage.source
    }
    commands = @($commandRecords)
    testCounts = $manifest.testCounts
    binaryDigests = $binaryDigests
    releaseCandidateManifest = "release-candidate.json"
    distributionRecord = "distribution.json"
    installedAcceptance = $acceptanceFiles
    sealedUtc = (Get-Date).ToUniversalTime().ToString("o")
}
[IO.File]::WriteAllText(
    (Join-Path $candidate "candidate-evidence.json"),
    ($finalEvidence | ConvertTo-Json -Depth 12),
    $utf8
)

$latest = Join-Path $root "artifacts\candidates\latest.txt"
[IO.File]::WriteAllText($latest, $candidate, $utf8)
Write-Host "Candidate complete: $candidate"

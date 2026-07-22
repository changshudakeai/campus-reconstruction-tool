Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$script:RequiredEvidenceIds = @(
    "automated", "installer", "durability-migration", "live-service", "six-campus", "axiom",
    "non-developer", "localisation", "reliability", "diagnostics", "redaction", "dependencies",
    "licences", "known-issues"
)
$script:RequiredArtifactRoles = @(
    "installer", "executable-main", "executable-map", "executable-preview", "notices",
    "foundation-schematic", "detailed-schematic", "foundation-manifest", "dataset-summary",
    "coverage-summary"
)

function Get-Sha256Hex([string]$Path) {
    $resolved = (Resolve-Path -LiteralPath $Path).Path
    $stream = [IO.File]::OpenRead($resolved)
    $sha256 = [Security.Cryptography.SHA256]::Create()
    try {
        return ([BitConverter]::ToString($sha256.ComputeHash($stream))).Replace("-", "").ToLowerInvariant()
    } finally {
        $sha256.Dispose()
        $stream.Dispose()
    }
}

function Read-JsonFile([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Required JSON file is missing: $Path" }
    try { return Get-Content -Raw -Encoding UTF8 -LiteralPath $Path | ConvertFrom-Json }
    catch { throw "Invalid JSON file '$Path': $($_.Exception.Message)" }
}

function Assert-Text($Value, [string]$Label) {
    if ($Value -isnot [string] -or [string]::IsNullOrWhiteSpace($Value)) { throw "$Label is required" }
}

function Assert-CandidateIdentity($Expected, $Actual, [string]$Label) {
    foreach ($field in @("candidateId", "version", "commit")) {
        Assert-Text $Actual.$field "$Label $field"
        if ($Actual.$field -ne $Expected.$field) { throw "$Label $field does not match the candidate" }
    }
}

function Resolve-CandidateFile([string]$CandidateDirectory, [string]$RelativePath, [string]$Label) {
    Assert-Text $RelativePath $Label
    if ([IO.Path]::IsPathRooted($RelativePath)) { throw "$Label must be relative to the candidate directory" }
    $candidate = [IO.Path]::GetFullPath($CandidateDirectory).TrimEnd([IO.Path]::DirectorySeparatorChar)
    $path = [IO.Path]::GetFullPath((Join-Path $candidate $RelativePath))
    $prefix = $candidate + [IO.Path]::DirectorySeparatorChar
    if (-not $path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { throw "$Label escapes the candidate directory" }
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "$Label is missing: $RelativePath" }
    return $path
}

function Get-FileRecord([string]$CandidateDirectory, [string]$RelativePath, [string]$Label) {
    $path = Resolve-CandidateFile $CandidateDirectory $RelativePath $Label
    return [ordered]@{
        path = $RelativePath.Replace('\', '/')
        bytes = (Get-Item -LiteralPath $path).Length
        sha256 = Get-Sha256Hex $path
    }
}

function Assert-FileRecordUnchanged([string]$CandidateDirectory, $FileRecord, [string]$Label) {
    $actual = Get-FileRecord $CandidateDirectory $FileRecord.path $Label
    if ($actual.sha256 -ne $FileRecord.sha256 -or $actual.bytes -ne $FileRecord.bytes) {
        throw "$Label changed: $($FileRecord.path)"
    }
}

function Assert-KnownIssues([string]$CandidateDirectory, [string]$RelativePath) {
    $path = Resolve-CandidateFile $CandidateDirectory $RelativePath "Known Issues record"
    $record = Read-JsonFile $path
    if ($null -eq $record.PSObject.Properties["issues"]) { throw "Known Issues record must contain an issues array" }
    foreach ($issue in @($record.issues)) {
        if ($issue.blocking -ne $false) { throw "Known Issue '$($issue.id)' is blocking; it must be a Release Blocker" }
        foreach ($field in @("id", "impact", "scope", "workaround", "trackingTicket", "targetVersion")) {
            Assert-Text $issue.$field "Known Issue $field"
        }
    }
}

function New-EvidenceIndex([string]$CandidateDirectory, [string]$GateRecord) {
    $candidate = (Resolve-Path -LiteralPath $CandidateDirectory).Path
    $manifest = Read-JsonFile (Join-Path $candidate "release-candidate.json")
    $candidateEvidence = Read-JsonFile (Join-Path $candidate "candidate-evidence.json")
    $distribution = Read-JsonFile (Join-Path $candidate "distribution.json")
    $gate = Read-JsonFile (Resolve-Path -LiteralPath $GateRecord).Path
    if ($manifest.version -ne "1.1.0" -or $manifest.commit -notmatch '^[0-9a-fA-F]{40}$') { throw "Candidate must identify V1.1.0 and a full commit SHA" }
    Assert-CandidateIdentity $manifest $candidateEvidence "Candidate evidence"
    Assert-CandidateIdentity $manifest $distribution "Distribution record"
    Assert-CandidateIdentity $manifest $gate "Release gate record"
    if ($manifest.installedAcceptance.status -ne "passed" -or $candidateEvidence.installedAcceptance.status -ne "passed" -or $distribution.installedAcceptanceStatus -ne "passed") {
        throw "Installed acceptance must pass; waiver, pending, skipped, and not-run states cannot be sealed"
    }
    if ($candidateEvidence.sourceClean -ne $true) { throw "Candidate evidence must record a clean source commit" }
    if ($distribution.onlineRequired -ne $true -or $distribution.platform -ne "Windows 11 x64") { throw "Distribution must record the online-required Windows 11 x64 boundary" }
    Assert-Text $distribution.releaseNotes "Distribution release notes"
    Assert-Text $distribution.knownIssues "Distribution Known Issues"
    if ($distribution.knownIssues -ne $gate.knownIssues) { throw "Distribution and gate must identify the same Known Issues record" }

    foreach ($field in @("windowsImage", "cleanWindowsImageId")) { Assert-Text $manifest.$field "Candidate environment $field" }
    if ($manifest.architecture -ne "x64") { throw "Candidate environment architecture must be x64" }
    if ($null -eq $manifest.toolchains -or @($manifest.toolchains.PSObject.Properties).Count -eq 0) { throw "Candidate environment toolchains are required" }

    $commandRecords = @($manifest.commands | ForEach-Object {
        Assert-Text $_.name "Candidate command name"
        Assert-Text $_.command "Candidate command line"
        if ([int]$_.exitCode -ne 0) { throw "Candidate command failed: $($_.name)" }
        $commandFiles = @()
        foreach ($field in @("log", "stdout", "stderr")) {
            $property = $_.PSObject.Properties[$field]
            if ($property -and -not [string]::IsNullOrWhiteSpace([string]$property.Value)) {
                $commandFiles += Get-FileRecord $candidate $property.Value "Command $($_.name) $field"
            }
        }
        if ($commandFiles.Count -eq 0) { throw "Candidate command has no log: $($_.name)" }
        [ordered]@{ name = $_.name; command = $_.command; exitCode = 0; files = $commandFiles }
    })
    if ($commandRecords.Count -eq 0) { throw "Candidate has no recorded verification commands" }

    $evidenceEntries = @($gate.evidence)
    $ids = @($evidenceEntries | ForEach-Object { $_.id })
    if (@($ids | Sort-Object -Unique).Count -ne $ids.Count) { throw "Evidence IDs must be unique" }
    foreach ($required in $script:RequiredEvidenceIds) {
        $entry = @($evidenceEntries | Where-Object { $_.id -eq $required })
        if ($entry.Count -ne 1) { throw "Mandatory evidence entry is missing: $required" }
        if ($entry[0].mandatory -ne $true -or $entry[0].status -ne "pass") { throw "Mandatory evidence '$required' must pass" }
    }
    foreach ($entry in $evidenceEntries) {
        if ($entry.status -notin @("pass", "fail", "not-applicable")) { throw "Evidence '$($entry.id)' uses forbidden status '$($entry.status)'" }
        if ($entry.status -eq "fail") { throw "Evidence '$($entry.id)' failed; zero Release Blockers are required" }
        if ($entry.mandatory -eq $true -and $entry.status -ne "pass") { throw "Mandatory evidence '$($entry.id)' must pass" }
        if ($entry.status -eq "not-applicable") { Assert-Text $entry.reason "Not-applicable reason for $($entry.id)" }
        if (@($entry.evidence).Count -eq 0) { throw "Evidence '$($entry.id)' has no result files" }
    }
    $evidenceRecords = @($evidenceEntries | ForEach-Object {
        [ordered]@{
            id = $_.id; mandatory = [bool]$_.mandatory; status = $_.status; reason = $_.reason
            files = @($_.evidence | ForEach-Object { Get-FileRecord $candidate $_ "Evidence '$($_)'" })
        }
    })

    $artifactEntries = @($gate.artifacts)
    foreach ($role in $script:RequiredArtifactRoles) {
        if (@($artifactEntries | Where-Object { $_.role -eq $role }).Count -lt 1) { throw "Required artifact role is missing: $role" }
    }
    $foundationCampuses = @("putuo", "shanghai-jiao-tong-minhang", "wuhan-university", "xiamen-university-siming", "xian-jiaotong-xingqing")
    foreach ($role in @("foundation-schematic", "foundation-manifest")) {
        $campuses = @($artifactEntries | Where-Object { $_.role -eq $role } | ForEach-Object { $_.campus })
        if ($campuses.Count -ne 5 -or (Compare-Object $foundationCampuses $campuses)) { throw "$role must cover each of the five full-flow campuses exactly once" }
    }
    $detailed = @($artifactEntries | Where-Object { $_.role -eq "detailed-schematic" })
    if ($detailed.Count -ne 1 -or $detailed[0].campus -ne "putuo") { throw "Detailed schematic must identify the representative Putuo output" }
    $artifactRecords = @($artifactEntries | ForEach-Object {
        $record = Get-FileRecord $candidate $_.path "Artifact '$($_.role)'"
        [ordered]@{ role = $_.role; campus = $(if ($_.PSObject.Properties["campus"]) { $_.campus } else { $null }); path = $record.path; bytes = $record.bytes; sha256 = $record.sha256 }
    })
    $installer = @($artifactRecords | Where-Object { $_.role -eq "installer" })
    if ($installer.Count -ne 1 -or $installer[0].path -ne $distribution.installer.Replace('\', '/') -or $installer[0].sha256 -ne $distribution.installerSha256) {
        throw "Installer artifact does not match distribution.json"
    }
    $binaryRoles = @{ "executable-main" = "campus-native.exe"; "executable-map" = "campus-map.exe"; "executable-preview" = "campus-preview.exe" }
    foreach ($role in $binaryRoles.Keys) {
        $artifact = @($artifactRecords | Where-Object { $_.role -eq $role })
        $expected = $manifest.binaryDigests.PSObject.Properties[$binaryRoles[$role]]
        if ($artifact.Count -ne 1 -or -not $expected -or $artifact[0].sha256 -ne $expected.Value) { throw "$role digest does not match release-candidate.json" }
    }
    Assert-KnownIssues $candidate $gate.knownIssues

    $service = $gate.controlledService
    foreach ($field in @("version", "deploymentId", "contractVersion", "rollbackVersion")) { Assert-Text $service.$field "Controlled service $field" }
    if ($service.contractVersion -ne "v1" -or $service.deployed -ne $true -or $service.pinned -ne $true -or $service.healthStatus -ne "pass" -or $service.rollbackReady -ne $true) {
        throw "The contract-compatible controlled service must be deployed, pinned, healthy, and rollback-ready"
    }
    $serviceHealth = Get-FileRecord $candidate $service.healthEvidence "Controlled-service health evidence"
    $operator = $gate.independentOperator
    Assert-Text $operator.name "Independent operator name"; Assert-Text $operator.attestedUtc "Independent operator attestation time"
    if ($operator.attested -ne $true) { throw "Independent operator attestation is required" }
    $operatorRecord = Get-FileRecord $candidate $operator.record "Independent operator record"
    $owner = $gate.releaseOwner
    Assert-Text $owner.name "Release owner name"; Assert-Text $owner.approvedUtc "Release owner approval time"
    $blockerCount = $owner.PSObject.Properties["releaseBlockerCount"]
    if ($owner.approved -ne $true -or -not $blockerCount -or $null -eq $blockerCount.Value -or
        $blockerCount.Value -is [bool] -or $blockerCount.Value -is [string] -or
        $blockerCount.Value -isnot [ValueType] -or [decimal]$blockerCount.Value -ne 0) {
        throw "Release owner must explicitly approve with a numeric zero Release Blocker count"
    }

    $candidateRecords = @(
        Get-FileRecord $candidate "release-candidate.json" "Release candidate manifest"
        Get-FileRecord $candidate "candidate-evidence.json" "Candidate evidence"
        Get-FileRecord $candidate "distribution.json" "Distribution record"
        Get-FileRecord $candidate $distribution.releaseNotes "Release notes"
    )

    return [ordered]@{
        schemaVersion = 1; candidateId = $manifest.candidateId; version = $manifest.version; commit = $manifest.commit
        environment = [ordered]@{ windowsImage = $manifest.windowsImage; cleanWindowsImageId = $manifest.cleanWindowsImageId; architecture = $manifest.architecture; toolchains = $manifest.toolchains }
        commands = $commandRecords; candidateRecords = $candidateRecords; evidence = $evidenceRecords; artifacts = $artifactRecords
        knownIssues = Get-FileRecord $candidate $gate.knownIssues "Known Issues record"
        controlledService = [ordered]@{ version = $service.version; deploymentId = $service.deploymentId; contractVersion = $service.contractVersion; pinned = $true; health = $serviceHealth; rollbackReady = $true; rollbackVersion = $service.rollbackVersion }
        independentOperator = [ordered]@{ name = $operator.name; attested = $true; attestedUtc = $operator.attestedUtc; record = $operatorRecord }
        releaseOwner = [ordered]@{ name = $owner.name; approved = $true; approvedUtc = $owner.approvedUtc; releaseBlockerCount = 0 }
        releaseBlockerCount = 0
    }
}

function Test-EvidenceBundle([string]$CandidateDirectory) {
    $candidate = (Resolve-Path -LiteralPath $CandidateDirectory).Path
    $bundle = Join-Path $candidate "sealed-evidence"
    $indexPath = Join-Path $bundle "evidence-index.json"
    $sealPath = Join-Path $bundle "seal.json"
    $index = Read-JsonFile $indexPath
    $seal = Read-JsonFile $sealPath
    Assert-CandidateIdentity $index $seal "Seal"
    $indexHash = Get-Sha256Hex $indexPath
    if ($seal.evidenceIndexSha256 -ne $indexHash -or $index.releaseBlockerCount -ne 0) { throw "Evidence index seal or Release Blocker count is invalid" }
    foreach ($command in @($index.commands)) {
        if ([int]$command.exitCode -ne 0) { throw "Sealed command failed: $($command.name)" }
        foreach ($file in @($command.files)) {
                        Assert-FileRecordUnchanged $candidate $file "Sealed command log"
        }
    }
    foreach ($file in @($index.candidateRecords)) {
                Assert-FileRecordUnchanged $candidate $file "Sealed candidate record"
    }
    foreach ($entry in @($index.evidence)) {
        if ($entry.status -notin @("pass", "not-applicable")) { throw "Sealed evidence '$($entry.id)' is not releasable" }
        foreach ($file in @($entry.files)) {
                        Assert-FileRecordUnchanged $candidate $file "Sealed evidence '$($entry.id)'"
        }
    }
    foreach ($artifact in @($index.artifacts)) {
                Assert-FileRecordUnchanged $candidate $artifact "Sealed artifact '$($artifact.role)'"
    }
    foreach ($file in @($index.knownIssues, $index.controlledService.health, $index.independentOperator.record)) {
                Assert-FileRecordUnchanged $candidate $file "Sealed release record"
    }
    return $index
}

Export-ModuleMember -Function Get-Sha256Hex, New-EvidenceIndex, Test-EvidenceBundle

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Split-Path -Parent $PSScriptRoot)).Path
Import-Module (Join-Path $PSScriptRoot "v1-1-evidence-bundle.psm1") -Force
$sandbox = Join-Path ([IO.Path]::GetTempPath()) ("campus-v11-seal-test-" + [guid]::NewGuid().ToString("N"))
$candidate = Join-Path $sandbox "candidate"
$utf8 = [Text.UTF8Encoding]::new($false)

function Write-Json([string]$Path, $Value) {
    $parent = Split-Path -Parent $Path
    if ($parent) { [IO.Directory]::CreateDirectory($parent) | Out-Null }
    [IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 20), $utf8)
}
function Write-Fixture([string]$RelativePath, [string]$Content = "fixture") {
    $path = Join-Path $candidate $RelativePath
    [IO.Directory]::CreateDirectory((Split-Path -Parent $path)) | Out-Null
    [IO.File]::WriteAllText($path, $Content, $utf8)
}
function Invoke-Seal([string]$GateRecord) {
    $priorPreference = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    $output = & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $root "scripts\seal-v1-1-evidence-bundle.ps1") `
        -CandidateDirectory $candidate -GateRecord $GateRecord 2>&1
    $exitCode = $LASTEXITCODE
    $ErrorActionPreference = $priorPreference
    if ($exitCode -eq 0) { $output | Out-Host }
    return $exitCode
}
function Invoke-Verify {
    $priorPreference = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    $output = & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $root "scripts\verify-v1-1-evidence-bundle.ps1") `
        -CandidateDirectory $candidate 2>&1
    $exitCode = $LASTEXITCODE
    $ErrorActionPreference = $priorPreference
    return $exitCode
}

try {
    [IO.Directory]::CreateDirectory($candidate) | Out-Null
    $identity = [ordered]@{ candidateId = "v1.1.0-abc1234-20260722"; version = "1.1.0"; commit = ("a" * 40) }
    Write-Json (Join-Path $candidate "release-candidate.json") ($identity + @{ installedAcceptance = @{ status = "passed" }; commands = @(@{ name = "tests"; command = "test"; exitCode = 0; log = "logs/tests.log" }) })
    Write-Json (Join-Path $candidate "candidate-evidence.json") ($identity + @{ installedAcceptance = @{ status = "passed" }; sourceClean = $true })

    $files = @(
        "installer.exe", "tested-binaries/campus-native.exe", "tested-binaries/campus-map.exe",
        "tested-binaries/campus-preview.exe", "THIRD_PARTY_NOTICES.md", "outputs/putuo-foundation.schem", "outputs/sjtu-foundation.schem", "outputs/wuhan-foundation.schem", "outputs/xiamen-foundation.schem", "outputs/xian-foundation.schem",
        "outputs/detailed.schem", "outputs/putuo-manifest.json", "outputs/sjtu-manifest.json", "outputs/wuhan-manifest.json", "outputs/xiamen-manifest.json", "outputs/xian-manifest.json", "outputs/dataset-summary.json",
        "outputs/coverage-summary.json", "logs/tests.log", "evidence/installer.json", "evidence/durability.json",
        "evidence/live.json", "evidence/campuses.json", "evidence/axiom.json", "evidence/operator.json",
        "evidence/localisation.json", "evidence/reliability.json", "evidence/diagnostics.json",
        "evidence/redaction.json", "evidence/dependencies.json", "evidence/licences.json", "evidence/known-issues.json",
        "evidence/service-health.json", "evidence/operator-record.json", "V1.1.0-RELEASE-NOTES.md"
    )
    foreach ($file in $files) { Write-Fixture $file $file }
    $binaryDigests = [ordered]@{}
    foreach ($name in @("campus-native.exe", "campus-map.exe", "campus-preview.exe")) {
        $binaryDigests[$name] = Get-Sha256Hex (Join-Path $candidate "tested-binaries/$name")
    }
    Write-Json (Join-Path $candidate "release-candidate.json") ($identity + @{
        installedAcceptance = @{ status = "passed" }; commands = @(@{ name = "tests"; command = "test"; exitCode = 0; log = "logs/tests.log" })
        binaryDigests = $binaryDigests; windowsImage = "Windows 11 test"; cleanWindowsImageId = "clean-test"
        architecture = "x64"; toolchains = @{ powershell = "test" }
    })
    $installerHash = Get-Sha256Hex (Join-Path $candidate "installer.exe")
    Write-Json (Join-Path $candidate "distribution.json") ($identity + @{
        installer = "installer.exe"; installerSha256 = $installerHash; installedAcceptanceStatus = "passed"
        onlineRequired = $true; platform = "Windows 11 x64"; signed = $false
        releaseNotes = "V1.1.0-RELEASE-NOTES.md"; knownIssues = "evidence/known-issues.json"
    })
    Write-Json (Join-Path $candidate "evidence/known-issues.json") @{ issues = @(@{
        id = "ISSUE-101"; blocking = $false; impact = "Cosmetic"; scope = "About page"
        workaround = "None required"; trackingTicket = "ISSUE-101"; targetVersion = "1.1.1"
    }) }

    $requiredEvidence = @(
        @{ id = "automated"; path = "logs/tests.log" }, @{ id = "installer"; path = "evidence/installer.json" },
        @{ id = "durability-migration"; path = "evidence/durability.json" }, @{ id = "live-service"; path = "evidence/live.json" },
        @{ id = "six-campus"; path = "evidence/campuses.json" }, @{ id = "axiom"; path = "evidence/axiom.json" },
        @{ id = "non-developer"; path = "evidence/operator.json" }, @{ id = "localisation"; path = "evidence/localisation.json" },
        @{ id = "reliability"; path = "evidence/reliability.json" }, @{ id = "diagnostics"; path = "evidence/diagnostics.json" },
        @{ id = "redaction"; path = "evidence/redaction.json" }, @{ id = "dependencies"; path = "evidence/dependencies.json" },
        @{ id = "licences"; path = "evidence/licences.json" }, @{ id = "known-issues"; path = "evidence/known-issues.json" }
    ) | ForEach-Object { [ordered]@{ id = $_.id; mandatory = $true; status = "pass"; reason = $null; evidence = @($_.path) } }
    $artifacts = @(
        @{ role = "installer"; path = "installer.exe" }, @{ role = "executable-main"; path = "tested-binaries/campus-native.exe" },
        @{ role = "executable-map"; path = "tested-binaries/campus-map.exe" }, @{ role = "executable-preview"; path = "tested-binaries/campus-preview.exe" },
        @{ role = "notices"; path = "THIRD_PARTY_NOTICES.md" },
        @{ role = "foundation-schematic"; campus = "putuo"; path = "outputs/putuo-foundation.schem" },
        @{ role = "foundation-schematic"; campus = "shanghai-jiao-tong-minhang"; path = "outputs/sjtu-foundation.schem" },
        @{ role = "foundation-schematic"; campus = "wuhan-university"; path = "outputs/wuhan-foundation.schem" },
        @{ role = "foundation-schematic"; campus = "xiamen-university-siming"; path = "outputs/xiamen-foundation.schem" },
        @{ role = "foundation-schematic"; campus = "xian-jiaotong-xingqing"; path = "outputs/xian-foundation.schem" },
        @{ role = "detailed-schematic"; campus = "putuo"; path = "outputs/detailed.schem" },
        @{ role = "foundation-manifest"; campus = "putuo"; path = "outputs/putuo-manifest.json" },
        @{ role = "foundation-manifest"; campus = "shanghai-jiao-tong-minhang"; path = "outputs/sjtu-manifest.json" },
        @{ role = "foundation-manifest"; campus = "wuhan-university"; path = "outputs/wuhan-manifest.json" },
        @{ role = "foundation-manifest"; campus = "xiamen-university-siming"; path = "outputs/xiamen-manifest.json" },
        @{ role = "foundation-manifest"; campus = "xian-jiaotong-xingqing"; path = "outputs/xian-manifest.json" },
        @{ role = "dataset-summary"; path = "outputs/dataset-summary.json" }, @{ role = "coverage-summary"; path = "outputs/coverage-summary.json" }
    )
    $gate = $identity + @{
        evidence = $requiredEvidence; artifacts = $artifacts; knownIssues = "evidence/known-issues.json"
        controlledService = @{ version = "service-1.1.4"; deploymentId = "prod-42"; contractVersion = "v1"; deployed = $true; pinned = $true; healthStatus = "pass"; healthEvidence = "evidence/service-health.json"; rollbackReady = $true; rollbackVersion = "service-1.1.3" }
        independentOperator = @{ name = "Independent Operator"; attested = $true; attestedUtc = "2026-07-22T01:00:00Z"; record = "evidence/operator-record.json" }
        releaseOwner = @{ name = "Release Owner"; approved = $true; approvedUtc = "2026-07-22T02:00:00Z"; releaseBlockerCount = 0 }
    }
    $gatePath = Join-Path $sandbox "gate.json"
    Write-Json $gatePath $gate
    if ((Invoke-Seal $gatePath) -ne 0) { throw "A complete zero-waiver record must seal" }
    $bundle = Join-Path $candidate "sealed-evidence"
    if (-not (Test-Path (Join-Path $bundle "evidence-index.json"))) { throw "Seal did not write evidence-index.json" }
    if (-not (Test-Path (Join-Path $bundle "seal.json"))) { throw "Seal did not write seal.json" }
    if ((Invoke-Verify) -ne 0) { throw "A newly sealed bundle must verify" }
    $approvalPath = Join-Path $sandbox "release-owner-approval.json"
    Write-Json $approvalPath ($identity + @{ owner = "Release Owner"; approved = $true; approvedUtc = "2026-07-22T03:00:00Z" })
    $handedOffArtifact = Join-Path $sandbox "downloaded-installer.exe"
    Copy-Item -LiteralPath (Join-Path $candidate "installer.exe") -Destination $handedOffArtifact
    $handoffRecord = Join-Path $sandbox "handoff-verification.json"
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $root "scripts\verify-v1-1-distributed-artifact.ps1") `
        -CandidateDirectory $candidate -Artifact $handedOffArtifact -ReleaseOwnerApproval $approvalPath -OutputRecord $handoffRecord 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $handoffRecord)) { throw "Approved handed-off artifact must be hash-verified" }

    [IO.File]::AppendAllText((Join-Path $candidate "installer.exe"), "tampered", $utf8)
    if ((Invoke-Verify) -eq 0) { throw "Artifact digest drift must invalidate the seal" }
    Write-Fixture "installer.exe" "installer.exe"

    $secondCandidate = Join-Path $sandbox "candidate-pending"
    Copy-Item -LiteralPath $candidate -Destination $secondCandidate -Recurse
    Remove-Item -LiteralPath (Join-Path $secondCandidate "sealed-evidence") -Recurse
    $candidate = $secondCandidate
    $pendingGate = $gate | ConvertTo-Json -Depth 20 | ConvertFrom-Json
    $pendingGate.evidence[0].status = "pending"
    $pendingPath = Join-Path $sandbox "pending.json"
    Write-Json $pendingPath $pendingGate
    if ((Invoke-Seal $pendingPath) -eq 0) { throw "Pending mandatory evidence must block sealing" }

    $extraMandatoryGate = $gate | ConvertTo-Json -Depth 20 | ConvertFrom-Json
    $extraMandatoryGate.evidence = [pscustomobject]@{ id = "extra-mandatory"; mandatory = $true; status = "not-applicable"; reason = "claimed N/A"; evidence = @("logs/tests.log") }
    $extraMandatoryPath = Join-Path $sandbox "extra-mandatory.json"
    Write-Json $extraMandatoryPath $extraMandatoryGate
    if ((Invoke-Seal $extraMandatoryPath) -eq 0) { throw "Every mandatory entry, including extensions, must pass" }

    $missingBlockerCountGate = $gate | ConvertTo-Json -Depth 20 | ConvertFrom-Json
    $missingBlockerCountGate.releaseOwner.PSObject.Properties.Remove("releaseBlockerCount")
    $missingBlockerCountPath = Join-Path $sandbox "missing-blocker-count.json"
    Write-Json $missingBlockerCountPath $missingBlockerCountGate
    if ((Invoke-Seal $missingBlockerCountPath) -eq 0) { throw "Release owner must explicitly record zero Release Blockers" }
    $nullBlockerCountGate = $gate | ConvertTo-Json -Depth 20 | ConvertFrom-Json
    $nullBlockerCountGate.releaseOwner.releaseBlockerCount = $null
    $nullBlockerCountPath = Join-Path $sandbox "null-blocker-count.json"
    Write-Json $nullBlockerCountPath $nullBlockerCountGate
    if ((Invoke-Seal $nullBlockerCountPath) -eq 0) { throw "A null Release Blocker count must not be interpreted as zero" }

    $missingFoundationGate = $gate | ConvertTo-Json -Depth 20 | ConvertFrom-Json
    $missingFoundationGate.artifacts = @($missingFoundationGate.artifacts | Where-Object { -not ($_.role -eq "foundation-schematic" -and $_.campus -eq "xian-jiaotong-xingqing") })
    $missingFoundationPath = Join-Path $sandbox "missing-foundation.json"
    Write-Json $missingFoundationPath $missingFoundationGate
    if ((Invoke-Seal $missingFoundationPath) -eq 0) { throw "All five Foundation outputs must be hashed" }

    $blankEnvironment = Get-Content -Raw -Encoding UTF8 -LiteralPath (Join-Path $candidate "release-candidate.json") | ConvertFrom-Json
    $blankEnvironment.windowsImage = ""
    Write-Json (Join-Path $candidate "release-candidate.json") $blankEnvironment
    if ((Invoke-Seal $gatePath) -eq 0) { throw "A blank candidate environment must block sealing" }

    Write-Host "V1.1 evidence seal tests passed"
} finally {
    if (Test-Path -LiteralPath $sandbox) { Remove-Item -LiteralPath $sandbox -Recurse -Force }
}

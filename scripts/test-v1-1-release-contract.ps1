Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Split-Path -Parent $PSScriptRoot)).Path

function Read-RepoFile([string]$RelativePath) {
    [IO.File]::ReadAllText((Join-Path $root $RelativePath))
}
function Assert-Contains([string]$Text, [string]$Needle, [string]$Message) {
    if (-not $Text.Contains($Needle)) { throw $Message }
}
function Assert-NotContains([string]$Text, [string]$Needle, [string]$Message) {
    if ($Text.Contains($Needle)) { throw $Message }
}

$package = Get-Content -Raw -Encoding UTF8 -LiteralPath (Join-Path $root "package.json") | ConvertFrom-Json
$packageLockText = Read-RepoFile "package-lock.json"
$lockedVersions = [regex]::Matches($packageLockText, '"version"\s*:\s*"1\.1\.0"').Count
if ($package.version -ne "1.1.0" -or $lockedVersions -lt 2) {
    throw "npm package metadata must report 1.1.0"
}

$cargo = Read-RepoFile "native\Cargo.toml"
$installer = Read-RepoFile "installer\campus-reconstruction-tool.nsi"
$about = Read-RepoFile "native\apps\campus-native\ui\app.slint"
$main = Read-RepoFile "native\apps\campus-native\src\main.rs"
$packager = Read-RepoFile "scripts\build-native-installer.ps1"
$sizeReport = Read-RepoFile "scripts\report-release-size.ps1"
$releaseNotes = Read-RepoFile "docs\releases\v1.1.0-unsigned.md"
$durabilityEvidence = Read-RepoFile "scripts\collect-v1-1-installed-durability-evidence.ps1"
$liveAxiomEvidence = Read-RepoFile "scripts\collect-v1-1-live-axiom-evidence.ps1"
$liveAxiomTemplate = Read-RepoFile "docs\releases\v1.1-live-axiom-operator-record.template.json"
$operatorExperienceEvidence = Read-RepoFile "scripts\collect-v1-1-operator-experience-evidence.ps1"
$operatorExperienceTemplate = Read-RepoFile "docs\releases\v1.1-operator-experience-record.template.json"
$operatorExperienceGuide = Read-RepoFile "docs\releases\v1.1-operator-experience-evidence.md"

Assert-Contains $cargo 'version = "1.1.0"' "Cargo workspace version must be 1.1.0"
$candidateVerifier = Read-RepoFile "scripts\verify-v1-1-candidate.ps1"
$installerVerifier = Read-RepoFile "scripts\verify-native-installer.ps1"
$releaseWorkflow = Read-RepoFile ".github\workflows\release.yml"
$releaseDecision = Read-RepoFile "docs\adr\0034-supersede-v1-desktop-release-assumptions-for-v1-1.md"
Assert-Contains $installer '!define PRODUCT_VERSION "1.1.0"' "Installer version must be 1.1.0"
$tracer = Read-RepoFile "native\apps\campus-native\src\v11_tracer_bullet.rs"
Assert-Contains $about "V1.1.0" "About UI must report V1.1.0"
Assert-Contains $main "V11ConstructionCapability::for_controlled_release()" "Production must always construct schema 2"
Assert-NotContains $main 'std::env::var("CAMPUS_V11_PROJECT_KERNEL")' "Production must not expose the V1.1 project gate"
Assert-NotContains $main 'argument == "--self-test"' "Release binary must not expose the legacy manual-feature self-test"
Assert-NotContains $packager "cargo +stable build" "Packaging must not rebuild release binaries"
Assert-NotContains $installer 'native\target\release' "NSIS must consume the candidate payload, not a mutable target directory"
Assert-Contains $packager "Pre-package SHA-256 mismatch" "Packaging must verify payload digests before NSIS"
Assert-Contains $packager "Packaging changed the already-tested binary" "Packaging must verify payload digests after NSIS"
Assert-NotContains $sizeReport "50MB" "V1.1 must not retain the 50 MB release gate"
Assert-Contains $releaseNotes "Windows 11 x64" "Release guidance must state the supported Windows image"
Assert-Contains $releaseNotes "requires network" "Release guidance must state the online requirement"
Assert-Contains $releaseNotes "SmartScreen" "Unsigned release guidance must explain SmartScreen"
Assert-Contains $about "continue-active-project" "Schema-2 Resume Point must be actionable in the production UI"
Assert-Contains $main "on_continue_active_project" "Production must route the current schema-2 task"
Assert-Contains $tracer "continue_active_project" "Production must expose the controlled schema-2 orchestration path"
Assert-Contains $tracer "FoundationResumePoint::Acquisition" "Production routing must include controlled acquisition"
Assert-Contains $tracer "FoundationResumePoint::Generation | FoundationResumePoint::Export" "Production routing must include generation and export"
Assert-Contains $releaseNotes "SHA-256" "Unsigned release guidance must require exact SHA-256 verification"

Assert-Contains $candidateVerifier "CleanWindowsImageId" "Candidate evidence must identify the clean Windows image"
Assert-Contains $candidateVerifier "release-binary-smoke" "Exact release binaries must be smoked before packaging"
Assert-Contains $candidateVerifier "CleanWindowsImageManifest" "Clean image identity must be bound to a manifest"
Assert-Contains $candidateVerifier '"release-contract"' "Candidate evidence must record the release contract test"
Assert-NotContains $candidateVerifier '"--offline"' "Candidate evidence commands must resolve locked dependencies when the local cache is incomplete"
Assert-Contains $installerVerifier '$supportedPredecessorSha256' "Supported upgrade must pin the predecessor digest"
Assert-Contains $installerVerifier "e59c1d1e523501db373db51ae0f2167c4d4fd368125dd6d71889ab08ac77e202" "Supported V1.0.1 predecessor digest changed"
if (
    $candidateVerifier.IndexOf('"release-binary-smoke"') -gt
    $candidateVerifier.IndexOf('$binaryDigests = [ordered]@{}')
) {
    throw "Release binary smoke must run before binaries are copied into the packaging payload"
}
Assert-Contains $candidateVerifier '"install-silent-fresh"' "Silent fresh install evidence is mandatory"
Assert-Contains $candidateVerifier '"install-interactive-fresh"' "Interactive fresh install evidence is mandatory"
Assert-Contains $candidateVerifier '"install-silent-upgrade"' "Predecessor upgrade evidence is mandatory"
Assert-Contains $candidateVerifier '[switch]$SkipInstalledAcceptance' "Installed acceptance may be skipped only through an explicit CLI waiver"
Assert-Contains $candidateVerifier 'status = "not-run-user-waived"' "A skipped installed acceptance must be recorded as user-waived, never passed"
Assert-Contains $candidateVerifier 'status = "passed"' "Successful default installed acceptance must transition from pending to passed"
if ($candidateVerifier -notmatch '(?s)if \(-not \$SkipInstalledAcceptance\) \{\s*Invoke-EvidenceCommand "install-silent-fresh"[^}]*Invoke-EvidenceCommand "install-interactive-fresh"[^}]*Invoke-EvidenceCommand "install-silent-upgrade"[^}]*\}') {
    throw "The default candidate path must guard all three installed-acceptance scenarios"
}
Assert-Contains $packager 'installedAcceptanceStatus' "Distribution evidence must publish the installed-acceptance status"
Assert-Contains $packager 'releaseNotes = $candidateReleaseNotesName' "Distribution evidence must identify exact release notes"
Assert-Contains $packager 'knownIssues = $knownIssuesName' "Distribution evidence must identify Known Issues"
$evidenceSealer = Read-RepoFile "scripts\seal-v1-1-evidence-bundle.ps1"
$evidenceVerifier = Read-RepoFile "scripts\verify-v1-1-evidence-bundle.ps1"
$releaseTagger = Read-RepoFile "scripts\create-v1-1-release-tag.ps1"
$distributionVerifier = Read-RepoFile "scripts\verify-v1-1-distributed-artifact.ps1"
Assert-Contains $evidenceSealer 'Evidence Bundle is already sealed' "Evidence Bundle sealing must be append-once"
Assert-Contains $evidenceVerifier 'Test-EvidenceBundle' "Sealed evidence must be independently re-verifiable"
Assert-Contains $releaseTagger 'Tag $Tag already exists and will not be moved' "The V1.1.0 tag must be immutable"
Assert-Contains $distributionVerifier 'Explicit release-owner approval' "Handoff verification must require release-owner approval"
if ($packager -notmatch '(?s)\$sourceStatus = @\(& git -C \$root status --porcelain=v1\)\s*if \(\$LASTEXITCODE -ne 0\) \{[^}]*\}\s*if \(\$sourceStatus\.Count -ne 0\)') {
    throw "Packaging must fail closed on git-status errors before it evaluates clean-worktree output"
}
Assert-Contains $releaseNotes 'not installed-acceptance tested' "Unsigned guidance must disclose an installed-acceptance waiver"
Assert-Contains $candidateVerifier '"candidate-evidence.json"' "Final evidence must be sealed after installed acceptance"
Assert-Contains $durabilityEvidence '--installed-durability-report' "Installed durability evidence must run the installed release binary"
Assert-Contains $durabilityEvidence 'SoakSeconds = 7200' "Installed reliability evidence must default to two hours"
Assert-Contains $durabilityEvidence 'Formal installed reliability evidence requires' "Short development runs must not become formal evidence"
Assert-Contains $durabilityEvidence 'releaseBlockers' "Mandatory failures must remain Release Blockers"
Assert-Contains $durabilityEvidence 'binaryDigests' "Installed evidence must bind all candidate binary digests"
Assert-Contains $liveAxiomEvidence '--live-axiom-operator-record' "Live/Axiom evidence must be validated by the installed release binary"
Assert-Contains $liveAxiomEvidence 'releaseBlockers' "Live/Axiom mandatory failures must remain Release Blockers"
Assert-Contains $liveAxiomEvidence 'binaryDigests' "Live/Axiom evidence must bind all installed candidate binary digests"
Assert-Contains $operatorExperienceEvidence '--operator-experience-record' "Operator evidence must be validated by the installed release binary"
Assert-Contains $operatorExperienceEvidence 'binaryDigests' "Operator evidence must bind all installed candidate binary digests"
Assert-Contains $operatorExperienceEvidence 'releaseBlockers' "Operator mandatory failures must remain Release Blockers"
Assert-Contains $operatorExperienceEvidence 'OSArchitecture' "Operator evidence must inspect the actual host architecture"
Assert-Contains $operatorExperienceEvidence '22000' "Operator evidence must require a Windows 11 build"
Assert-Contains $operatorExperienceEvidence 'architecture = $architecture' "Operator envelope must record detected architecture"
Assert-Contains $operatorExperienceEvidence 'GetEnvironmentVariable' "Operator evidence must scan configured secret values"
foreach ($scale in @(100, 125, 150)) {
    Assert-Contains $operatorExperienceTemplate ('"percent": ' + $scale) "Operator template is missing $scale% Windows scaling"
}
foreach ($locale in @('zh-CN', 'en')) {
    Assert-Contains $operatorExperienceTemplate ('"locale": "' + $locale + '"') "Operator template is missing the $locale sweep"
}
Assert-Contains $operatorExperienceTemplate 'receivedGoalOnly' "Operator template must attest goal-only instructions"
Assert-Contains $operatorExperienceTemplate 'blockingDeveloperExplanations' "Operator template must record blocking developer explanations"
Assert-Contains $operatorExperienceGuide 'F1' "Operator guide must cover F1 guidance reopening"
Assert-Contains $operatorExperienceGuide 'Release Blocker' "Operator guide must fail closed on blocking usability findings"
foreach ($campusId in @('putuo', 'sjtu-minhang', 'wuhan-university', 'xiamen-siming', 'xian-jiaotong-xingqing', 'nyu-shanghai-qiantan')) {
    Assert-Contains $liveAxiomTemplate $campusId "Live/Axiom operator template is missing $campusId"
}
Assert-Contains $liveAxiomTemplate 'honest-unavailability' "Six-campus template must retain the accepted negative risk"
Assert-Contains $liveAxiomTemplate 'determinismRepeat' "Every schematic must have an independently generated determinism repeat"
Assert-Contains $installerVerifier '$predecessorSetup' "Upgrade must install a real predecessor"
Assert-NotContains $installerVerifier "Supported same-line upgrade" "Reinstalling the candidate is not upgrade evidence"
Assert-Contains $releaseWorkflow "workflow_dispatch:" "The source gate must run before tagging"
Assert-NotContains $releaseWorkflow "tags:" "A tag must not build an unsealed candidate"
Assert-NotContains $releaseWorkflow "softprops/action-gh-release" "The pre-tag gate must not publish a release"
Assert-Contains $releaseDecision "both Foundation and Detailed" "The V1.1 release decision must retain both-mode parity"
Assert-Contains $tracer "ShowTaskError" "Map validation failures must remain visible in the active tool"
Assert-Contains $tracer "ProductionWorkflowOutcome::Cancelled" "Closing review must preserve the current task explicitly"

[pscustomobject]@{
    Version = "1.1.0"
    ProductionProjectModel = "schema-2-only"
    PackagingRebuildsBinaries = $false
    SizeGate = "informational-only"
    UnsignedGuidance = "present"
} | Format-List

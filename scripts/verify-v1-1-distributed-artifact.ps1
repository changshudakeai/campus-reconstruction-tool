param(
    [Parameter(Mandatory = $true)][string]$CandidateDirectory,
    [Parameter(Mandatory = $true)][string]$Artifact,
    [Parameter(Mandatory = $true)][string]$ReleaseOwnerApproval,
    [Parameter(Mandatory = $true)][string]$OutputRecord
)
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
Import-Module (Join-Path $PSScriptRoot "v1-1-evidence-bundle.psm1") -Force
$index = Test-EvidenceBundle -CandidateDirectory $CandidateDirectory
$artifactPath = (Resolve-Path -LiteralPath $Artifact).Path
$approvalPath = (Resolve-Path -LiteralPath $ReleaseOwnerApproval).Path
$approval = Get-Content -Raw -Encoding UTF8 -LiteralPath $approvalPath | ConvertFrom-Json
foreach ($field in @("candidateId", "commit", "owner", "approvedUtc")) {
    if ([string]::IsNullOrWhiteSpace([string]$approval.$field)) { throw "Release-owner approval $field is required" }
}
if ($approval.approved -ne $true -or $approval.candidateId -ne $index.candidateId -or $approval.commit -ne $index.commit) {
    throw "Explicit release-owner approval does not match the sealed candidate"
}
$installer = @($index.artifacts | Where-Object { $_.role -eq "installer" })
if ($installer.Count -ne 1) { throw "Sealed bundle must identify exactly one installer" }
$actualHash = Get-Sha256Hex $artifactPath
if ($actualHash -ne $installer[0].sha256) { throw "Downloaded or handed-off artifact SHA-256 does not match the sealed installer" }
$output = [IO.Path]::GetFullPath($OutputRecord)
if (Test-Path -LiteralPath $output) { throw "Distribution verification record already exists: $output" }
$record = [ordered]@{
    schemaVersion = 1; candidateId = $index.candidateId; version = $index.version; commit = $index.commit
    artifactName = (Split-Path -Leaf $artifactPath); bytes = (Get-Item -LiteralPath $artifactPath).Length
    sha256 = $actualHash; expectedSha256 = $installer[0].sha256; status = "pass"
    releaseOwner = $approval.owner; approvalUtc = $approval.approvedUtc
    approvalRecordSha256 = Get-Sha256Hex $approvalPath
    verifiedUtc = (Get-Date).ToUniversalTime().ToString("o")
}
$parent = Split-Path -Parent $output
if ($parent) { [IO.Directory]::CreateDirectory($parent) | Out-Null }
[IO.File]::WriteAllText($output, ($record | ConvertTo-Json -Depth 10), [Text.UTF8Encoding]::new($false))
Write-Host "Distributed artifact verified after release-owner approval: $output"

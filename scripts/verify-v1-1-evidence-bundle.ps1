param([Parameter(Mandatory = $true)][string]$CandidateDirectory)
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
Import-Module (Join-Path $PSScriptRoot "v1-1-evidence-bundle.psm1") -Force
$index = Test-EvidenceBundle -CandidateDirectory $CandidateDirectory
Write-Host "Evidence Bundle verified: $($index.candidateId), zero Release Blockers"

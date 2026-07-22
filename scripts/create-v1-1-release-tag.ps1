param(
    [Parameter(Mandatory = $true)][string]$CandidateDirectory,
    [ValidateSet("v1.1.0")][string]$Tag = "v1.1.0"
)
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Split-Path -Parent $PSScriptRoot)).Path
Import-Module (Join-Path $PSScriptRoot "v1-1-evidence-bundle.psm1") -Force
$index = Test-EvidenceBundle -CandidateDirectory $CandidateDirectory
$head = (& git -C $root rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $head -ne $index.commit) { throw "HEAD is not the sealed candidate commit" }
$status = @(& git -C $root status --porcelain=v1)
if ($LASTEXITCODE -ne 0 -or $status.Count -ne 0) { throw "Release tagging requires the exact clean candidate commit" }
& git -C $root rev-parse --verify --quiet "refs/tags/$Tag" 2>$null | Out-Null
if ($LASTEXITCODE -eq 0) { throw "Tag $Tag already exists and will not be moved" }
$indexPath = Join-Path (Resolve-Path -LiteralPath $CandidateDirectory).Path "sealed-evidence\evidence-index.json"
$indexHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $indexPath).Hash.ToLowerInvariant()
$message = "Campus Reconstruction Tool V1.1.0`n`nCandidate: $($index.candidateId)`nEvidence index SHA-256: $indexHash`nRelease Blockers: 0"
& git -C $root tag -a $Tag $index.commit -m $message
if ($LASTEXITCODE -ne 0) { throw "Could not create immutable release tag $Tag" }
Write-Host "Created $Tag on $($index.commit). Do not rebuild, replace, or retag this candidate."

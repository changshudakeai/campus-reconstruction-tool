param(
    [Parameter(Mandatory = $true)][string]$CandidateDirectory,
    [Parameter(Mandatory = $true)][string]$GateRecord
)
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
Import-Module (Join-Path $PSScriptRoot "v1-1-evidence-bundle.psm1") -Force
$candidate = (Resolve-Path -LiteralPath $CandidateDirectory).Path
$bundle = Join-Path $candidate "sealed-evidence"
if (Test-Path -LiteralPath $bundle) { throw "Evidence Bundle is already sealed and will not be replaced: $bundle" }
$index = New-EvidenceIndex -CandidateDirectory $candidate -GateRecord $GateRecord
$staging = Join-Path $candidate (".sealing-" + [guid]::NewGuid().ToString("N"))
$utf8 = [Text.UTF8Encoding]::new($false)
try {
    [IO.Directory]::CreateDirectory($staging) | Out-Null
    $indexPath = Join-Path $staging "evidence-index.json"
    [IO.File]::WriteAllText($indexPath, ($index | ConvertTo-Json -Depth 30), $utf8)
    $seal = [ordered]@{
        schemaVersion = 1; candidateId = $index.candidateId; version = $index.version; commit = $index.commit
        evidenceIndexSha256 = Get-Sha256Hex $indexPath
        releaseBlockerCount = 0; independentOperator = $index.independentOperator
        releaseOwner = $index.releaseOwner; sealedUtc = (Get-Date).ToUniversalTime().ToString("o")
        immutableAfterTag = $true
    }
    [IO.File]::WriteAllText((Join-Path $staging "seal.json"), ($seal | ConvertTo-Json -Depth 20), $utf8)
    Move-Item -LiteralPath $staging -Destination $bundle
} catch {
    if (Test-Path -LiteralPath $staging) { Remove-Item -LiteralPath $staging -Recurse -Force }
    throw
}
Test-EvidenceBundle -CandidateDirectory $candidate | Out-Null
Write-Host "Evidence Bundle sealed: $bundle"

param(
    [string]$CandidateDirectory
)

$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Split-Path -Parent $PSScriptRoot)).Path
$target = Join-Path $root "native\target"
if (-not $CandidateDirectory) {
    $latest = Join-Path $root "artifacts\candidates\latest.txt"
    if (Test-Path -LiteralPath $latest) {
        $CandidateDirectory = (Get-Content -Raw -Encoding UTF8 -LiteralPath $latest).Trim()
    }
}

function Measure-Directory([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return 0 }
    return (Get-ChildItem -LiteralPath $Path -Recurse -File | Measure-Object Length -Sum).Sum
}

$debugBytes = Measure-Directory (Join-Path $target "debug")
$releaseBytes = Measure-Directory (Join-Path $target "release")
Write-Host ("Rust debug cache:   {0:N2} GB" -f ($debugBytes / 1GB))
Write-Host ("Rust release cache: {0:N2} GB" -f ($releaseBytes / 1GB))

if ($CandidateDirectory) {
    $distributionPath = Join-Path $CandidateDirectory "distribution.json"
    if (Test-Path -LiteralPath $distributionPath) {
        $distribution = Get-Content -Raw -Encoding UTF8 -LiteralPath $distributionPath | ConvertFrom-Json
        $installer = Get-Item -LiteralPath (Join-Path $CandidateDirectory $distribution.installer)
        Write-Host ("Installer: {0} ({1:N2} MB, informational only)" -f $installer.Name, ($installer.Length / 1MB))
        Write-Host ("SHA-256: {0}" -f $distribution.installerSha256)
    }
}

# M5 发布构建脚本 —— ADR-0003“轻快”目标的可复现便携 zip 产物。
#
# 用法：
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-release.ps1
#
# 产物：dist\MCRebuild-V2.0.0-portable.zip（含 exe 与可选资源目录）。
# 运行：解压后双击 campus-rebuild-dev.exe；文案/主题资源随 exe 旁 resources/，
#       删除 resources/ 也由编译期内嵌兜底（ADR-0005/0023）。

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$Version = '2.0.0'
$BuiltBin = 'campus-tool-dev.exe'
$BinName = 'campus-rebuild.exe'
$DistDir = Join-Path $Root 'dist'
$StageDir = Join-Path $DistDir "MCRebuild-V$Version"
$ZipPath = Join-Path $DistDir "MCRebuild-V$Version-portable.zip"

$env:SLINT_BACKEND = 'software'
$env:CARGO_BUILD_JOBS = '2'

Write-Host "==> cargo build --release -p desktop-shell"
Push-Location $Root
try {
    cargo build --release -p desktop-shell
    if ($LASTEXITCODE -ne 0) { throw "release build failed: $LASTEXITCODE" }
} finally {
    Pop-Location
}

$Built = Join-Path $Root "target\release\$BuiltBin"
if (-not (Test-Path -LiteralPath $Built)) { throw "built exe not found: $Built" }

if (Test-Path -LiteralPath $StageDir) {
    Remove-Item -LiteralPath $StageDir -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null
Copy-Item -LiteralPath $Built -Destination (Join-Path $StageDir $BinName)

# 可选运行时资源（主题/文案热编辑用；缺失时走内嵌兜底）
$Resources = Join-Path $Root 'apps\desktop\resources'
if (Test-Path -LiteralPath $Resources) {
    Copy-Item -LiteralPath $Resources -Destination $StageDir -Recurse
}

if (Test-Path -LiteralPath $ZipPath) {
    Remove-Item -LiteralPath $ZipPath -Force
}
Compress-Archive -Path (Join-Path $StageDir '*') -DestinationPath $ZipPath

$SizeMB = [Math]::Round((Get-Item -LiteralPath $ZipPath).Length / 1MB, 2)
Write-Host "==> $ZipPath ($SizeMB MB)"
Write-Host "    staging: $StageDir"

# 快速运行开发版（debug 增量构建，直接查看最新代码效果）。
# 用法：双击桌面“校园复刻工具 - 开发运行”快捷方式，或在仓库目录执行：
#   .\scripts\run-dev.ps1
# 说明：不覆盖 %LOCALAPPDATA%\MCRebuildV2\dev\ 安装位，也不改桌面开发版快捷方式。

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot   # = New-branch-v2
Set-Location $Root

$env:CARGO_BUILD_JOBS = '2'
$env:SLINT_BACKEND = 'software'

Write-Host '==> cargo run -p desktop-shell --bin campus-tool-dev'
cargo run -p desktop-shell --bin campus-tool-dev

if ($LASTEXITCODE -ne 0) {
    Write-Host "cargo run 退出码：$LASTEXITCODE"
    Read-Host '按回车关闭'
}

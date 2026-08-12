<#
.SYNOPSIS
Runs Cargo with bounded, automatically recycled per-worktree build caches.

.EXAMPLE
.\scripts\cargo-managed.ps1 -- test --workspace

.EXAMPLE
.\scripts\cargo-managed.ps1 -TimeoutMinutes 30 -- test -p desktop-shell --test s1_13_leave_workspace_confirmation

.EXAMPLE
.\scripts\cargo-managed.ps1 -MaintenanceOnly
#>
[CmdletBinding()]
param(
    [ValidateRange(1, 29)]
    [int]$HighWaterGiB = 24,

    [ValidateRange(1, 28)]
    [int]$LowWaterGiB = 16,

    [ValidateRange(0, 1440)]
    [int]$TimeoutMinutes = 120,

    [switch]$MaintenanceOnly,

    [Parameter(Position = 0, ValueFromRemainingArguments = $true)]
    [string[]]$CargoArgs
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($LowWaterGiB -ge $HighWaterGiB) {
    throw 'LowWaterGiB must be lower than HighWaterGiB.'
}
if (-not $MaintenanceOnly -and $CargoArgs.Count -eq 0) {
    throw 'Pass Cargo arguments after --, or use -MaintenanceOnly.'
}

function Get-DirectoryStats {
    param([Parameter(Mandatory)][string]$Path)

    if (-not [System.IO.Directory]::Exists($Path)) {
        return [pscustomobject]@{
            Bytes = [int64]0
            Files = 0
            NewestUtc = [datetime]::MinValue
        }
    }

    [int64]$bytes = 0
    [int]$files = 0
    $newestUtc = [datetime]::MinValue
    foreach ($file in [System.IO.Directory]::EnumerateFiles(
        $Path,
        '*',
        [System.IO.SearchOption]::AllDirectories
    )) {
        try {
            $info = [System.IO.FileInfo]::new($file)
            $bytes += $info.Length
            $files += 1
            if ($info.LastWriteTimeUtc -gt $newestUtc) {
                $newestUtc = $info.LastWriteTimeUtc
            }
        }
        catch [System.IO.FileNotFoundException] {
            # Cargo may finish a rename between enumeration and metadata lookup.
        }
    }
    [pscustomobject]@{ Bytes = $bytes; Files = $files; NewestUtc = $newestUtc }
}

function Get-WorktreeTargets {
    param([Parameter(Mandatory)][string]$CurrentWorktree)

    $worktreeLines = @(& git worktree list --porcelain 2>$null)
    if ($LASTEXITCODE -ne 0) {
        throw 'Unable to enumerate Git worktrees.'
    }
    foreach ($line in $worktreeLines) {
        if (-not $line.StartsWith('worktree ', [System.StringComparison]::Ordinal)) {
            continue
        }
        $worktree = [System.IO.Path]::GetFullPath($line.Substring(9))
        $target = [System.IO.Path]::GetFullPath(
            [System.IO.Path]::Combine($worktree, 'target')
        )
        $stats = Get-DirectoryStats -Path $target
        [pscustomobject]@{
            Worktree = $worktree
            Target = $target
            IsCurrent = $worktree.Equals(
                $CurrentWorktree,
                [System.StringComparison]::OrdinalIgnoreCase
            )
            Bytes = $stats.Bytes
            Files = $stats.Files
            NewestUtc = $stats.NewestUtc
        }
    }
}

function Get-CargoProcesses {
    @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {
        $_.Name -in @('cargo.exe', 'rustc.exe', 'xtask.exe')
    })
}

function Remove-WorktreeTarget {
    param([Parameter(Mandatory)]$Candidate)

    $expected = [System.IO.Path]::GetFullPath(
        [System.IO.Path]::Combine($Candidate.Worktree, 'target')
    )
    if (-not $Candidate.Target.Equals(
        $expected,
        [System.StringComparison]::OrdinalIgnoreCase
    )) {
        Write-Warning "Skip unexpected cache path: $($Candidate.Target)"
        return $false
    }
    if (-not [System.IO.Directory]::Exists($expected)) {
        return $true
    }
    $item = [System.IO.DirectoryInfo]::new($expected)
    if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        Write-Warning "Skip reparse-point cache path: $expected"
        return $false
    }

    try {
        [System.IO.Directory]::Delete($expected, $true)
        $candidateGiB = $Candidate.Bytes / 1GB
        Write-Host ('Recycled Cargo cache: {0:N2} GiB, {1} files, {2}' -f `
            $candidateGiB, $Candidate.Files, $expected)
        return $true
    }
    catch {
        Write-Warning "Unable to recycle Cargo cache ${expected}: $($_.Exception.Message)"
        return $false
    }
}

function Invoke-CacheMaintenance {
    param(
        [Parameter(Mandatory)][string]$CurrentWorktree,
        [Parameter(Mandatory)][string]$Stage
    )

    $targets = @(Get-WorktreeTargets -CurrentWorktree $CurrentWorktree)
    [int64]$totalBytes = ($targets | Measure-Object -Property Bytes -Sum).Sum
    $totalGiB = $totalBytes / 1GB
    Write-Host ('Cargo cache {0}: {1:N2} GiB across {2} worktree target(s)' -f `
        $Stage, $totalGiB, $targets.Count)
    if ($totalBytes -lt ([int64]$HighWaterGiB * 1GB)) {
        return
    }

    $active = @(Get-CargoProcesses)
    if ($active.Count -gt 0) {
        Write-Warning 'Cache is above the automatic-recycle waterline, but another Cargo process is active; defer recycling to the next managed run.'
        return
    }

    $sortProperties = @(
        @{ Expression = 'IsCurrent'; Ascending = $true }
        @{ Expression = 'NewestUtc'; Ascending = $true }
    )
    $candidates = @(
        $targets |
            Where-Object { $_.Bytes -gt 0 } |
            Sort-Object -Property $sortProperties
    )
    $lowWaterBytes = [int64]$LowWaterGiB * 1GB
    foreach ($candidate in $candidates) {
        if ($totalBytes -le $lowWaterBytes) {
            break
        }
        if (Remove-WorktreeTarget -Candidate $candidate) {
            $totalBytes -= $candidate.Bytes
        }
    }
    Write-Host ('Cargo cache after recycle: {0:N2} GiB' -f ($totalBytes / 1GB))
}

function Invoke-SafeCacheMaintenance {
    param(
        [Parameter(Mandatory)][string]$CurrentWorktree,
        [Parameter(Mandatory)][string]$Stage
    )

    try {
        Invoke-CacheMaintenance -CurrentWorktree $CurrentWorktree -Stage $Stage
    }
    catch {
        Write-Warning "Cargo cache maintenance was deferred: $($_.Exception.Message)"
    }
}

function Stop-ProcessTree {
    param([Parameter(Mandatory)][int]$RootProcessId)

    $processes = @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue)
    $childrenByParent = @{}
    foreach ($process in $processes) {
        $parent = [int]$process.ParentProcessId
        if (-not $childrenByParent.ContainsKey($parent)) {
            $childrenByParent[$parent] = [System.Collections.Generic.List[int]]::new()
        }
        $childrenByParent[$parent].Add([int]$process.ProcessId)
    }
    $ordered = [System.Collections.Generic.List[int]]::new()
    function Add-Descendants([int]$ProcessId) {
        if ($childrenByParent.ContainsKey($ProcessId)) {
            foreach ($child in $childrenByParent[$ProcessId]) {
                Add-Descendants -ProcessId $child
            }
        }
        $ordered.Add($ProcessId)
    }
    Add-Descendants -ProcessId $RootProcessId
    foreach ($processId in $ordered) {
        Stop-Process -Id $processId -Force -ErrorAction SilentlyContinue
    }
}

$repoRoot = (& git rev-parse --show-toplevel).Trim()
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($repoRoot)) {
    throw 'Run this script from an MCRebuild Git worktree.'
}
$repoRoot = [System.IO.Path]::GetFullPath($repoRoot)
$commonGitDir = (& git rev-parse --git-common-dir).Trim()
if (-not [System.IO.Path]::IsPathRooted($commonGitDir)) {
    $commonGitDir = [System.IO.Path]::GetFullPath(
        [System.IO.Path]::Combine($repoRoot, $commonGitDir)
    )
}
$lockPath = [System.IO.Path]::Combine($commonGitDir, 'cargo-cache-manager.lock')
$lock = $null
$cargoProcess = $null
$cargoExitCode = 0

try {
    while ($null -eq $lock) {
        try {
            $lock = [System.IO.File]::Open(
                $lockPath,
                [System.IO.FileMode]::OpenOrCreate,
                [System.IO.FileAccess]::ReadWrite,
                [System.IO.FileShare]::None
            )
        }
        catch [System.IO.IOException] {
            Write-Host 'Waiting for another managed Cargo command to finish...'
            Start-Sleep -Seconds 2
        }
    }

    Invoke-SafeCacheMaintenance -CurrentWorktree $repoRoot -Stage 'before command'
    if ($MaintenanceOnly) {
        return
    }

    $env:CARGO_BUILD_JOBS = '2'
    $env:CARGO_TARGET_DIR = [System.IO.Path]::Combine($repoRoot, 'target')
    $env:SLINT_BACKEND = 'software'

    $cargoExecutable = (Get-Command cargo -ErrorAction Stop).Source
    Write-Host "Managed Cargo invocation: cargo $($CargoArgs -join ' ')"
    $cargoProcess = Start-Process -FilePath $cargoExecutable -ArgumentList $CargoArgs `
        -NoNewWindow -PassThru
    $watchdogPath = [System.IO.Path]::Combine($PSScriptRoot, 'cargo-watchdog.ps1')
    $cargoStartedUtcTicks = $cargoProcess.StartTime.ToUniversalTime().Ticks
    Start-Process -FilePath 'powershell.exe' -ArgumentList @(
        '-NoProfile',
        '-NonInteractive',
        '-ExecutionPolicy',
        'Bypass',
        '-File',
        $watchdogPath,
        '-WrapperProcessId',
        $PID,
        '-CargoProcessId',
        $cargoProcess.Id,
        '-CargoStartedUtcTicks',
        $cargoStartedUtcTicks
    ) -WindowStyle Hidden | Out-Null
    $started = [datetime]::UtcNow
    while (-not $cargoProcess.HasExited) {
        if ($TimeoutMinutes -gt 0 -and
            ([datetime]::UtcNow - $started).TotalMinutes -ge $TimeoutMinutes) {
            Write-Warning "Cargo exceeded ${TimeoutMinutes} minutes; stopping only its verified process tree."
            Stop-ProcessTree -RootProcessId $cargoProcess.Id
            $cargoExitCode = 124
            break
        }
        Start-Sleep -Seconds 2
        $cargoProcess.Refresh()
    }
    if ($cargoExitCode -ne 124) {
        $cargoProcess.WaitForExit()
        $cargoExitCode = $cargoProcess.ExitCode
    }
}
finally {
    if ($null -ne $cargoProcess -and -not $cargoProcess.HasExited) {
        Stop-ProcessTree -RootProcessId $cargoProcess.Id
    }
    if ($null -ne $lock) {
        if (-not $MaintenanceOnly) {
            Invoke-SafeCacheMaintenance -CurrentWorktree $repoRoot -Stage 'after command'
        }
        $lock.Dispose()
    }
}
exit $cargoExitCode

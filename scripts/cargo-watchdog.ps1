<#
Hidden helper for cargo-managed.ps1. If the wrapper is externally terminated,
stop only the Cargo process instance that it started and that process's descendants.
#>
param(
    [Parameter(Mandatory)]
    [int]$WrapperProcessId,

    [Parameter(Mandatory)]
    [int]$CargoProcessId,

    [Parameter(Mandatory)]
    [int64]$CargoStartedUtcTicks
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'SilentlyContinue'

Wait-Process -Id $WrapperProcessId
$cargo = Get-Process -Id $CargoProcessId
if ($null -eq $cargo) {
    exit 0
}
if ($cargo.StartTime.ToUniversalTime().Ticks -ne $CargoStartedUtcTicks) {
    # The original Cargo process exited and Windows reused its PID.
    exit 0
}

$processes = @(Get-CimInstance Win32_Process)
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
Add-Descendants -ProcessId $CargoProcessId
foreach ($processId in $ordered) {
    Stop-Process -Id $processId -Force
}

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$nativeRoot = Join-Path $root "native"
$manifest = Join-Path $nativeRoot "Cargo.toml"
$debugDirectory = Join-Path $nativeRoot "target\debug"
$application = Join-Path $debugDirectory "campus-native.exe"
$requiredExecutables = @("campus-native.exe", "campus-map.exe", "campus-preview.exe")
$relevantExtensions = @(".rs", ".slint", ".toml")

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "Rust toolchain was not found. Install Rust to run the source workspace."
}

function Assert-CompleteDebugRuntime {
    foreach ($name in $requiredExecutables) {
        if (-not (Test-Path -LiteralPath (Join-Path $debugDirectory $name) -PathType Leaf)) {
            throw "Debug build did not produce $name."
        }
    }
}

function Build-DebugRuntime {
    & cargo +stable build --manifest-path $manifest --workspace
    if ($LASTEXITCODE -ne 0) {
        throw "Native workspace build exited with code $LASTEXITCODE."
    }
    Assert-CompleteDebugRuntime
}

function Test-RelevantChange([string]$path) {
    return $relevantExtensions -contains ([System.IO.Path]::GetExtension($path).ToLowerInvariant())
}

function Stop-DevelopmentRuntime($process) {
    if ($null -ne $process -and -not $process.HasExited) {
        Stop-Process -Id $process.Id -ErrorAction SilentlyContinue
        $process.WaitForExit()
    }
}

$watcher = [System.IO.FileSystemWatcher]::new($nativeRoot, "*.*")
$watcher.IncludeSubdirectories = $true
$watcher.NotifyFilter = [System.IO.NotifyFilters]::FileName -bor [System.IO.NotifyFilters]::LastWrite
$watcher.EnableRaisingEvents = $true
$running = $null

try {
    while ($true) {
        Build-DebugRuntime
        $running = Start-Process -FilePath $application -PassThru
        Write-Host "Development runtime started. Watching native source changes..."

        do {
            $change = $watcher.WaitForChanged([System.IO.WatcherChangeTypes]::All, 500)
            if ($running.HasExited) {
                throw "Development runtime exited with code $($running.ExitCode)."
            }
        } while ($change.TimedOut -or -not (Test-RelevantChange $change.Name))

        Write-Host "Change detected in $($change.Name). Rebuilding development runtime..."
        Stop-DevelopmentRuntime $running
        $running = $null

        do {
            Start-Sleep -Milliseconds 300
            $change = $watcher.WaitForChanged([System.IO.WatcherChangeTypes]::All, 100)
        } while (-not $change.TimedOut)
    }
}
finally {
    Stop-DevelopmentRuntime $running
    $watcher.Dispose()
}

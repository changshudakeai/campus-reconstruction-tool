$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$manifest = Join-Path $root "native\Cargo.toml"

# Keep both supervised tool processes current. Cargo's incremental check makes
# this a fast no-op when their sources and dependencies have not changed.
& cargo +stable build --manifest-path $manifest -p campus-map -p campus-preview
if ($LASTEXITCODE -ne 0) {
    throw "Native tool build exited with code $LASTEXITCODE."
}

# Run the main process in this terminal so ordinary errors and panics remain
# visible during development. Persistent JSONL diagnostics are written as well.
& cargo +stable run --manifest-path $manifest -p campus-native
if ($LASTEXITCODE -ne 0) {
    throw "Native application exited with code $LASTEXITCODE."
}

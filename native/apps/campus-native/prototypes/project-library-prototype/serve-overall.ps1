$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$url = "http://127.0.0.1:4175/overall.html?variant=A"
Write-Host "V1.1 overall product prototype: $url"
python -m http.server 4175 --bind 127.0.0.1 --directory $root

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
Write-Host "Campus Project Library prototype: http://127.0.0.1:4174/?variant=A"
python -m http.server 4174 --bind 127.0.0.1 --directory $root

@echo off
setlocal
cd /d "%~dp0"

echo Starting Campus Reconstruction Tool development watcher...
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\watch-native.ps1"
if errorlevel 1 pause

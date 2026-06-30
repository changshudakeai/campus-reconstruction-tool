@echo off
setlocal
cd /d "%~dp0"

echo Starting native Campus Reconstruction Tool...
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\start-native.ps1"
if errorlevel 1 pause

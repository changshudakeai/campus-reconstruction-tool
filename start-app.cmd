@echo off
setlocal
cd /d "%~dp0"

where npm.cmd >nul 2>nul
if errorlevel 1 (
  echo Node.js and npm are required to start the application.
  pause
  exit /b 1
)

if not exist node_modules (
  echo Installing workspace dependencies...
  call npm install
  if errorlevel 1 (
    pause
    exit /b 1
  )
)

echo Starting Overture and Campus Reconstruction Tool...
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\start-desktop.ps1"
if errorlevel 1 pause

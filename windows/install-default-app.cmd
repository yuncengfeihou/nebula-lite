@echo off
rem Double-click friendly wrapper: registers nebula-lite as a default-app candidate.
rem Keep this file *pure ASCII* -- cmd.exe reads batch files in the console code page.
setlocal
where pwsh.exe >nul 2>nul
if errorlevel 1 (
  echo PowerShell 7 ^(pwsh^) was not found on PATH.
  echo Install it first: https://aka.ms/powershell
  pause
  exit /b 1
)
pwsh -NoProfile -ExecutionPolicy Bypass -File "%~dp0install-default-app.ps1" %*
echo.
pause

@echo off
rem Double-click friendly wrapper for uninstall-context-menu.ps1.
rem Keep this file *pure ASCII* -- see install-context-menu.cmd for why.
setlocal
where pwsh.exe >nul 2>nul
if errorlevel 1 (
  echo PowerShell 7 ^(pwsh^) was not found on PATH.
  echo Install it first: https://aka.ms/powershell
  pause
  exit /b 1
)
pwsh -NoProfile -ExecutionPolicy Bypass -File "%~dp0uninstall-context-menu.ps1" %*
echo.
pause

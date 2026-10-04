@echo off
rem Double-click friendly wrapper: runs the sibling install-context-menu.ps1 with
rem PowerShell 7. Keep this file *pure ASCII* -- cmd.exe reads batch files in the
rem console code page, so any non-ASCII text here would render as mojibake.
setlocal
where pwsh.exe >nul 2>nul
if errorlevel 1 (
  echo PowerShell 7 ^(pwsh^) was not found on PATH.
  echo Install it first: https://aka.ms/powershell
  pause
  exit /b 1
)
pwsh -NoProfile -ExecutionPolicy Bypass -File "%~dp0install-context-menu.ps1" %*
echo.
pause

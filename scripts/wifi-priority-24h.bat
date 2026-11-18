@echo off
chcp 65001 >nul

net session >nul 2>&1
if %errorlevel% neq 0 (
    powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)

powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0wifi-priority-24h.ps1"
if %errorlevel% neq 0 (
    echo [ERROR] Failed to apply interface metrics.
)
pause

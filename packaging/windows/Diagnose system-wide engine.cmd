@echo off
rem Shows what the TAW Equaliser's system-wide engine is doing (changes nothing).
rem Double-click it. Windows asks for administrator permission, then the
rem PowerShell window shows what it's doing; close it when it's finished.
set "EQBOX_SCRIPT=%~dp0diagnose-engine.ps1"
powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process powershell -Verb RunAs -ArgumentList ('-NoProfile -ExecutionPolicy Bypass -NoExit -File ' + [char]34 + $env:EQBOX_SCRIPT + [char]34)"
if errorlevel 1 (
    echo.
    echo Windows didn't give administrator permission, so nothing was changed.
    pause
)

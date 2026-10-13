@echo off
rem Removes the TAW Equaliser's system-wide engine (or an EQ Box one) and puts every device back as it was.
rem Double-click it. Windows asks for administrator permission, then the
rem PowerShell window shows what it's doing; close it when it's finished.
set "EQBOX_SCRIPT=%~dp0remove-engine.ps1"
powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process powershell -Verb RunAs -ArgumentList ('-NoProfile -ExecutionPolicy Bypass -NoExit -File ' + [char]34 + $env:EQBOX_SCRIPT + [char]34)"
if errorlevel 1 (
    echo.
    echo Windows didn't give administrator permission, so nothing was changed.
    pause
)

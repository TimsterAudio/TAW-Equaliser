<#
.SYNOPSIS
    Shows what the TAW Equaliser's system-wide engine is doing. Changes nothing.

.DESCRIPTION
    Run from an Administrator PowerShell and send the output to the
    developer:
        powershell -ExecutionPolicy Bypass -File .\diagnose-engine.ps1
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Continue'

# A 32-bit PowerShell sees redirected copies of Program Files and the
# registry, so rerun this script in the 64-bit one.
if ([Environment]::Is64BitOperatingSystem -and -not [Environment]::Is64BitProcess) {
    $ps64 = Join-Path $env:windir 'sysnative\WindowsPowerShell\v1.0\powershell.exe'
    & $ps64 -NoProfile -ExecutionPolicy Bypass -File $PSCommandPath
    exit $LASTEXITCODE
}
. (Join-Path $PSScriptRoot 'engine-common.ps1')

function Section([string]$title) { Write-Host ''; Write-Host "== $title" -ForegroundColor Cyan }

Section 'System'
Write-Host ("Windows: {0}" -f [Environment]::OSVersion.VersionString)
Write-Host ("64-bit PowerShell: {0}" -f [Environment]::Is64BitProcess)
Get-Service -Name audiosrv, AudioEndpointBuilder | ForEach-Object { Write-Host ("Service {0}: {1}" -f $_.Name, $_.Status) }
$dll = Join-Path $InstallDir $DllName
Write-Host ("Engine DLL: {0} ({1})" -f $dll, $(if (Test-Path -LiteralPath $dll) { (Get-Item -LiteralPath $dll).LastWriteTime } else { 'missing' }))
if (Test-LegacyInstall) {
    Write-Host ("An EQ Box engine (the old name) is also installed in {0}. Run remove-engine.ps1, then install again." -f $LegacyInstallDir) -ForegroundColor Yellow
}
$com = "HKLM:\SOFTWARE\Classes\CLSID\$ClsidText\InprocServer32"
Write-Host ("COM registration: {0}" -f $(if (Test-Path -LiteralPath $com) { (Get-Item -LiteralPath $com).GetValue('') } else { 'missing' }))
$apo = "HKLM:\SOFTWARE\Classes\AudioEngine\AudioProcessingObjects\$ClsidText"
Write-Host ("Audio engine registration: {0}" -f $(if (Test-Path -LiteralPath $apo) { 'present' } else { 'missing' }))
$audioKey = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Audio'
Write-Host ("DisableProtectedAudioDG: {0}" -f (Get-ItemProperty -LiteralPath $audioKey -ErrorAction SilentlyContinue).DisableProtectedAudioDG)

Section 'Is the engine loaded by Windows audio?'
# Only Windows' audio process: scanning every process can take minutes.
tasklist /fi "imagename eq audiodg.exe" /m $DllName
# Which copy of the DLL it loaded, to tell builds apart.
foreach ($p in Get-Process -Name audiodg -ErrorAction SilentlyContinue) {
    foreach ($m in $p.Modules | Where-Object ModuleName -eq $DllName) {
        $f = Get-Item -LiteralPath $m.FileName -ErrorAction SilentlyContinue
        Write-Host ("Loaded from {0}  ({1} bytes, {2})" -f $m.FileName, $f.Length, $f.LastWriteTime)
    }
    Write-Host ("audiodg.exe started {0}" -f $p.StartTime)
}
if (Test-Path -LiteralPath $dll) {
    $f = Get-Item -LiteralPath $dll
    Write-Host ("Installed DLL: {0} bytes, {1}" -f $f.Length, $f.LastWriteTime)
}

Section 'Playback devices'
foreach ($d in Get-RenderDevices) {
    $mark = if ($d.HasEqBox) { '  <-- Equaliser' } else { '' }
    Write-Host ("{0}  state {1}  enhancements off: {2}  {3}{4}" -f $d.Guid, $d.State, $d.EnhancementsOff, $d.Label, $mark)
}

foreach ($d in Get-RenderDevices | Where-Object HasEqBox) {
    Section "Effect settings of: $($d.Label)"
    $fx = Get-Item -LiteralPath "$($d.Key)\FxProperties"
    foreach ($name in $fx.GetValueNames()) {
        $v = $fx.GetValue($name)
        if ($v -is [array]) { $v = $v -join ' | ' }
        Write-Host ("{0} = {1}" -f $name, $v)
    }
}

Section 'Settings file (written by the Equaliser window)'
$settings = Join-Path $SettingsDir 'engine.json'
if (Test-Path -LiteralPath $settings) {
    Write-Host ("Last written: {0}" -f (Get-Item -LiteralPath $settings).LastWriteTime)
    $j = Get-Content -LiteralPath $settings -Raw | ConvertFrom-Json
    $i = 0
    foreach ($b in $j.bands) { $i++; if ($b.enabled -and $b.gain_db -ne 0) { Write-Host ("  band {0}: {1:+0.0;-0.0} dB at {2} Hz" -f $i, $b.gain_db, $b.freq) } }
    Write-Host ("  preamp {0} dB, bypass {1}" -f $j.preamp_db, $j.bypass)
} else {
    Write-Host 'missing'
}
icacls $SettingsDir

Section 'Engine status (written by the engine itself)'
# Next to the settings, or (if Windows refused that) in the audio engine's
# own temp folder.
$found = $false
foreach ($status in @(
        (Join-Path $SettingsDir 'engine-status.txt'),
        (Join-Path $env:windir 'ServiceProfiles\LocalService\AppData\Local\Temp\engine-status.txt'),
        (Join-Path $env:windir 'Temp\engine-status.txt'))) {
    if (Test-Path -LiteralPath $status) {
        $found = $true
        Write-Host ("{0}" -f $status)
        Write-Host ("Last written: {0} (now {1})" -f (Get-Item -LiteralPath $status).LastWriteTime, (Get-Date))
        Get-Content -LiteralPath $status
        Write-Host ''
    }
}
if (-not $found) {
    Write-Host 'No status file in either place: the engine has not been created since this build was installed.'
}

Section 'Engine messages (Application event log, newest last)'
$events = @(Get-EventLog -LogName Application -Source $EventSource -Newest 40 -ErrorAction SilentlyContinue)
if ($events.Count -eq 0) {
    Write-Host 'None: the engine has not reported anything.'
} else {
    [array]::Reverse($events)
    foreach ($e in $events) { Write-Host ("{0:HH:mm:ss}  {1,-11} {2}" -f $e.TimeGenerated, $e.EntryType, $e.Message) }
}

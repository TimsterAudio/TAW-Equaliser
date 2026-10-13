<#
.SYNOPSIS
    Installs the TAW Equaliser's system-wide engine on one Windows output
    (test build).

.DESCRIPTION
    Copies taw_equaliser_apo.dll to "C:\Program Files\Timster's Audio
    Works\Equaliser", registers it with Windows, and adds it to the output
    you choose as an endpoint effect. Windows' audio service then restarts,
    and everything that output plays goes through the Equaliser. Set the EQ
    in the TAW Equaliser window with System set to "Windows (system-wide)".

    If an EQ Box engine (the app's old name) is installed, it offers to
    remove it first, putting every device back as it was.

    The device's effect settings are backed up first. remove-engine.ps1
    puts every device back exactly as it was.

    Run from an Administrator PowerShell, in the folder with the DLL:
        powershell -ExecutionPolicy Bypass -File .\install-engine.ps1

.PARAMETER DeviceId
    The output's endpoint id, such as {0.0.0.00000000}.{guid}. Leave it out
    to pick from a list.

.PARAMETER NoChain
    If the output already has an endpoint effect from its driver, switch it
    off while the Equaliser is installed instead of running it after the
    Equaliser. Only
    for a driver effect that misbehaves; it comes back on removal.
#>
#Requires -RunAsAdministrator
[CmdletBinding()]
param(
    [string]$DeviceId,
    [switch]$NoChain
)

$ErrorActionPreference = 'Stop'

# A 32-bit PowerShell sees a redirected copy of the registry (no devices),
# so rerun this script in the 64-bit one.
if ([Environment]::Is64BitOperatingSystem -and -not [Environment]::Is64BitProcess) {
    $ps64 = Join-Path $env:windir 'sysnative\WindowsPowerShell\v1.0\powershell.exe'
    $relaunch = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $PSCommandPath)
    if ($DeviceId) { $relaunch += @('-DeviceId', $DeviceId) }
    if ($NoChain) { $relaunch += '-NoChain' }
    & $ps64 @relaunch
    exit $LASTEXITCODE
}
. (Join-Path $PSScriptRoot 'engine-common.ps1')

Write-Host ''
Write-Host 'TAW Equaliser: install the system-wide engine (test build)' -ForegroundColor Cyan
Write-Host ''

$dllSource = Join-Path $PSScriptRoot $DllName
if (-not (Test-Path $dllSource)) {
    throw "Can't find $DllName next to this script ($PSScriptRoot)."
}

# --- An EQ Box engine (the old name) comes off first ----------------------
if (Test-LegacyInstall) {
    Write-Host 'An EQ Box engine is installed. That is this app under its old name.' -ForegroundColor Yellow
    Write-Host 'It has to be removed first: every device goes back as it was, then the'
    Write-Host 'TAW Equaliser engine is installed in its place. Save your EQ as a preset'
    Write-Host 'in the app first if you want to keep it.'
    $answer = Read-Host 'Remove EQ Box now and carry on? (Y/n)'
    if ($answer -and $answer.Trim().ToLower() -notin @('y', 'yes')) {
        throw 'Stopped: EQ Box is still installed. Nothing was changed.'
    }
    & (Join-Path $PSScriptRoot 'remove-engine.ps1')
    if (Test-LegacyInstall) {
        throw "EQ Box couldn't be removed completely. Run remove-engine.ps1 and send its output to the developer."
    }
    Write-Host ''
}

# --- Pick the output ------------------------------------------------------
$devices = @(Get-RenderDevices | Where-Object Active)
if ($devices.Count -eq 0) {
    # Usually Windows audio isn't running: start it and look again.
    $svc = Get-Service -Name audiosrv, AudioEndpointBuilder
    if ($svc | Where-Object Status -ne 'Running') {
        Write-Host 'Windows audio is not running; starting it...'
        Start-Service -Name AudioEndpointBuilder, audiosrv
        Start-Sleep -Seconds 2
        $devices = @(Get-RenderDevices | Where-Object Active)
    }
}
if ($devices.Count -eq 0) {
    Write-Host ''
    Write-Host 'No active playback devices. What this script can see:' -ForegroundColor Yellow
    Write-Host ("  64-bit PowerShell: {0}" -f [Environment]::Is64BitProcess)
    Get-Service -Name audiosrv, AudioEndpointBuilder | ForEach-Object { Write-Host ("  Service {0}: {1}" -f $_.Name, $_.Status) }
    foreach ($d in Get-RenderDevices) { Write-Host ("  {0}  state {1}  {2}" -f $d.Guid, $d.State, $d.Label) }
    throw 'No active playback devices found. Please send the lines above to the developer.'
}

if (-not $DeviceId) {
    Write-Host 'Playback devices:'
    for ($i = 0; $i -lt $devices.Count; $i++) {
        $d = $devices[$i]
        $mark = if ($d.HasEqBox) { '  (Equaliser installed)' } else { '' }
        Write-Host ("  [{0}] {1}{2}" -f ($i + 1), $d.Label, $mark)
    }
    $choice = Read-Host "Which one gets the Equaliser? (1-$($devices.Count))"
    $n = 0
    if (-not [int]::TryParse($choice, [ref]$n) -or $n -lt 1 -or $n -gt $devices.Count) {
        throw 'No device chosen.'
    }
    $device = $devices[$n - 1]
} else {
    $device = $devices | Where-Object Id -eq $DeviceId
    if (-not $device) { throw "No active playback device with id $DeviceId." }
}
Write-Host ''
Write-Host "Device: $($device.Label)"

if ($device.EnhancementsOff) {
    Write-Warning ('"Audio enhancements" are turned off for this device, so Windows will skip the Equaliser. ' +
        'Turn them on in Settings > System > Sound > (device) > Audio enhancements.')
}


# --- Files ----------------------------------------------------------------
New-Item -ItemType Directory -Force -Path $InstallDir, $BackupDir | Out-Null
# Left by an earlier test build.
Remove-Item -LiteralPath (Join-Path $InstallDir 'setup') -Recurse -Force -ErrorAction SilentlyContinue
# Only administrators may change the install folder (backups live here).
icacls $InstallDir /inheritance:r /grant:r '*S-1-5-32-544:(OI)(CI)F' '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-545:(OI)(CI)RX' '*S-1-5-19:(OI)(CI)RX' | Out-Null

$dllTarget = Join-Path $InstallDir $DllName
Write-Host 'Stopping Windows audio to copy the engine...'
Stop-Service -Name audiosrv -Force
try {
    Copy-Item -Force $dllSource $dllTarget
} finally {
    Start-Service -Name audiosrv
}

# The settings file: the Equaliser window (any user) writes it, the audio engine reads
# it and writes engine-status.txt next to it. The audio engine runs as
# LOCAL SERVICE with a restricted token, so its restricted identities
# (RESTRICTED, WRITE RESTRICTED and the AudioSrv service) need access too.
New-Item -ItemType Directory -Force -Path $SettingsDir | Out-Null
icacls $SettingsDir /grant '*S-1-5-32-545:(OI)(CI)M' '*S-1-5-19:(OI)(CI)M' '*S-1-5-12:(OI)(CI)M' '*S-1-5-33:(OI)(CI)M' /T | Out-Null
# Separate call: if this name didn't resolve it would undo the grants above.
icacls $SettingsDir /grant 'NT SERVICE\AudioSrv:(OI)(CI)M' /T 2>&1 | Out-Null
# The family folder above it: just enough to pass through (read and list,
# not inherited), for the same identities.
$familyData = Split-Path -Parent $SettingsDir
icacls $familyData /grant '*S-1-5-32-545:(RX)' '*S-1-5-19:(RX)' '*S-1-5-12:(RX)' '*S-1-5-33:(RX)' | Out-Null
icacls $familyData /grant 'NT SERVICE\AudioSrv:(RX)' 2>&1 | Out-Null

# --- Backups (only the first time, so a reinstall never overwrites them) --
$backup = Join-Path $BackupDir "$($device.Guid).json"
if (-not (Test-Path -LiteralPath $backup)) {
    $fx = Join-Path $device.Key 'FxProperties'
    $saved = [ordered]@{
        DeviceId    = $device.Id
        Label       = $device.Label
        FxExisted   = [bool](Test-Path -LiteralPath $fx)
        Values      = @{}
    }
    if ($saved.FxExisted) {
        $props = Get-Item -LiteralPath $fx
        foreach ($name in @($PkeyEfx, $PkeyEfxModes)) {
            $v = $props.GetValue($name, $null, 'DoNotExpandEnvironmentNames')
            if ($null -ne $v) {
                $saved.Values[$name] = @{ Kind = $props.GetValueKind($name).ToString(); Data = $v }
            }
        }
    }
    $saved | ConvertTo-Json -Depth 5 | Set-Content -Path $backup -Encoding UTF8
}
$globalBackup = Join-Path $BackupDir 'global.json'
if (-not (Test-Path $globalBackup)) {
    $audioKey = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Audio'
    $prev = (Get-ItemProperty -LiteralPath $audioKey -ErrorAction SilentlyContinue).DisableProtectedAudioDG
    @{ DisableProtectedAudioDG = $prev } | ConvertTo-Json | Set-Content -Path $globalBackup -Encoding UTF8
}

# The original owner and permissions of the key the engine needs to write to:
# FxProperties if the device has one, otherwise the device key (to create
# FxProperties under it). Saved once, before anything changes.
$savedDevice = Get-Content -LiteralPath $backup -Raw | ConvertFrom-Json
$deviceSub = "$RenderSub\$($device.Guid)"
$aclTarget = if ($savedDevice.FxExisted) { "$deviceSub\FxProperties" } else { $deviceSub }
$aclBackup = Join-Path $BackupDir "$($device.Guid).acl.json"
if (-not (Test-Path -LiteralPath $aclBackup)) {
    @{ Key = $aclTarget; Sddl = (Get-KeySddl $aclTarget) } | ConvertTo-Json | Set-Content -Path $aclBackup -Encoding UTF8
}

# The driver's own endpoint effect, as it was before the Equaliser (from the backup,
# so a reinstall still finds it).
$NoEffect = '{00000000-0000-0000-0000-000000000000}'
$original = $null
$originalModes = @()
if ($savedDevice.Values -and $savedDevice.Values.$PkeyEfx) {
    $original = [string]$savedDevice.Values.$PkeyEfx.Data
    if ($original -eq $ClsidText -or $original -eq $NoEffect -or -not $original.Trim()) { $original = $null }
}
if ($savedDevice.Values -and $savedDevice.Values.$PkeyEfxModes) {
    $originalModes = @($savedDevice.Values.$PkeyEfxModes.Data)
}
if ($original) {
    if ($NoChain) {
        Write-Host "The device's own effect $original is switched off while the Equaliser is installed (-NoChain)."
    } else {
        Write-Host "Keeping the device's own effect $original`: it runs after the Equaliser."
    }
}

# --- Registry -------------------------------------------------------------
Write-Host 'Registering the engine...'

# The engine reports what it's doing to the Application event log.
if (-not [System.Diagnostics.EventLog]::SourceExists($EventSource)) {
    New-EventLog -LogName Application -Source $EventSource
}

# COM class
$c = "HKLM:\SOFTWARE\Classes\CLSID\$ClsidText"
New-Item -Force -Path $c | Out-Null
Set-Item -Path $c -Value 'TAW Equaliser'
New-Item -Force -Path "$c\InprocServer32" | Out-Null
Set-Item -Path "$c\InprocServer32" -Value $dllTarget
New-ItemProperty -Force -Path "$c\InprocServer32" -Name ThreadingModel -Value 'Both' -PropertyType String | Out-Null

# Audio engine registration
$a = "HKLM:\SOFTWARE\Classes\AudioEngine\AudioProcessingObjects\$ClsidText"
New-Item -Force -Path $a | Out-Null
$strings = @{ FriendlyName = 'TAW Equaliser'; Copyright = "Timster's Audio Works"; APOInterface0 = $IidApoText }
foreach ($k in $strings.Keys) { New-ItemProperty -Force -Path $a -Name $k -Value $strings[$k] -PropertyType String | Out-Null }
# MaxInstances -1 is stored as the DWORD 0xFFFFFFFF (no limit).
$dwords = @{ MajorVersion = 0; MinorVersion = 1; Flags = 15; MinInputConnections = 1; MaxInputConnections = 1;
             MinOutputConnections = 1; MaxOutputConnections = 1; MaxInstances = -1; NumAPOInterfaces = 1 }
foreach ($k in $dwords.Keys) { New-ItemProperty -Force -Path $a -Name $k -Value ([int]$dwords[$k]) -PropertyType DWord | Out-Null }

# The device: the Equaliser as its endpoint effect, in the default processing mode
Write-Host 'Taking ownership of the device''s effect settings (saved for removal)...'
Grant-KeyAccess $aclTarget
$fxKey = "$($device.Key)\FxProperties"
if (-not (Test-Path -LiteralPath $fxKey)) { New-Item -Path $fxKey | Out-Null }
New-ItemProperty -Force -LiteralPath $fxKey -Name $PkeyEfx -Value $ClsidText -PropertyType String | Out-Null
if ($original -and -not $NoChain) {
    # The engine runs the driver's effect itself, in every mode the driver's
    # effect offered.
    New-ItemProperty -Force -LiteralPath $fxKey -Name $PkeyChained -Value $original -PropertyType String | Out-Null
    $modes = @($originalModes | Where-Object { $_ })
    if ($modes -notcontains $ModeDefault) { $modes = @($ModeDefault) + $modes }
} else {
    Remove-ItemProperty -LiteralPath $fxKey -Name $PkeyChained -ErrorAction SilentlyContinue
    $modes = @($ModeDefault)
}
New-ItemProperty -Force -LiteralPath $fxKey -Name $PkeyEfxModes -Value ([string[]]$modes) -PropertyType MultiString | Out-Null

# Let the audio engine load effects that aren't Microsoft-signed
# (Not needed on every Windows version, so a failure here only warns.)
try {
    $audio = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Audio'
    if (-not (Test-Path -LiteralPath $audio)) { New-Item -Path $audio | Out-Null }
    New-ItemProperty -Force -Path $audio -Name DisableProtectedAudioDG -Value 1 -PropertyType DWord | Out-Null
} catch {
    Write-Warning "Couldn't set DisableProtectedAudioDG ($($_.Exception.Message)). Carrying on; if you hear no EQ, tell the developer."
}

Write-Host 'Restarting Windows audio...'
Restart-Service -Name audiosrv -Force

Write-Host ''
Write-Host "Done. The Equaliser is on: $($device.Label)" -ForegroundColor Green
Write-Host ''
Write-Host 'Next:'
Write-Host '  1. Open TAW Equaliser, set System to "Windows (system-wide)", and press Run / Stop.'
Write-Host '  2. Play music in any app on that device and move the EQ.'
Write-Host ''
Write-Host 'To undo: run remove-engine.ps1 from an Administrator PowerShell.'
Write-Host 'If a device goes silent: Settings > System > Sound > (device) > Audio enhancements: Off bypasses the Equaliser at once.'

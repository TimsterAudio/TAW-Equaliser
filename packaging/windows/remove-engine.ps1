<#
.SYNOPSIS
    Removes the TAW Equaliser's system-wide engine (or an EQ Box one, its old
    name) and puts every device back as it was.

.DESCRIPTION
    Restores each device's effect settings from the backups install-engine.ps1
    made, unregisters the engine, deletes the DLL and restarts Windows audio.
    It finds both this version's install and one made when the app was
    called EQ Box. The app itself is untouched.

    Run from an Administrator PowerShell:
        powershell -ExecutionPolicy Bypass -File .\remove-engine.ps1
#>
#Requires -RunAsAdministrator
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

# A 32-bit PowerShell sees a redirected copy of the registry, so rerun this
# script in the 64-bit one.
if ([Environment]::Is64BitOperatingSystem -and -not [Environment]::Is64BitProcess) {
    $ps64 = Join-Path $env:windir 'sysnative\WindowsPowerShell\v1.0\powershell.exe'
    & $ps64 -NoProfile -ExecutionPolicy Bypass -File $PSCommandPath
    exit $LASTEXITCODE
}
. (Join-Path $PSScriptRoot 'engine-common.ps1')

Write-Host ''
Write-Host 'TAW Equaliser: remove the system-wide engine' -ForegroundColor Cyan
Write-Host ''

# Each install kept its DLL and backups in its own folder: the old EQ Box
# one first (its backups are the oldest, so they hold the true originals),
# then this version's.
$roots = @(
    @{ Dir = $LegacyInstallDir; Dll = $LegacyDllName },
    @{ Dir = $InstallDir; Dll = $DllName }
)
$backupDirs = @($roots | ForEach-Object { Join-Path $_.Dir 'backup' } |
    Where-Object { Test-Path -LiteralPath $_ })

$backups = @()
foreach ($dir in $backupDirs) {
    $backups += @(Get-ChildItem -LiteralPath $dir -Filter '*.json' |
        Where-Object { $_.Name -ne 'global.json' -and $_.Name -notlike '*.acl.json' })
}
foreach ($b in $backups) {
    $label = (Get-Content -LiteralPath $b.FullName -Raw | ConvertFrom-Json).Label
    Write-Host "Restoring: $label"
}

function Test-HasBackup([string]$Guid) {
    foreach ($dir in $backupDirs) {
        if (Test-Path -LiteralPath (Join-Path $dir "$Guid.json")) { return $true }
    }
    return $false
}

# Devices with the engine but no backup (shouldn't happen) just lose it.
$orphans = @(Get-RenderDevices | Where-Object HasEqBox | Where-Object { -not (Test-HasBackup $_.Guid) })
foreach ($o in $orphans) { Write-Host "Removing the engine from: $($o.Label)" }

# Each backed-up device: the values first, then the key's original owner and
# permissions (which takes away the access setup gave Administrators).
foreach ($file in $backups) {
    $saved = Get-Content -LiteralPath $file.FullName -Raw | ConvertFrom-Json
    $fx = "$RenderRoot\$($file.BaseName)\FxProperties"
    # Only touch the values if the engine is actually on the device (an
    # install that stopped early never changed them, and a device already
    # restored from an older backup no longer has it).
    $ours = (Test-Path -LiteralPath $fx) -and ((Get-Item -LiteralPath $fx).GetValue($PkeyEfx, $null) -eq $ClsidText)
    if ($ours) {
        if (-not $saved.FxExisted) {
            Remove-Item -LiteralPath $fx -Recurse -Force
        } else {
            foreach ($name in @($PkeyEfx, $PkeyEfxModes, $PkeyChained)) {
                Remove-ItemProperty -LiteralPath $fx -Name $name -ErrorAction SilentlyContinue
            }
            foreach ($p in $saved.Values.PSObject.Properties) {
                $data = $p.Value.Data
                if ($p.Value.Kind -eq 'MultiString') { $data = [string[]]@($data) }
                New-ItemProperty -Force -LiteralPath $fx -Name $p.Name -Value $data -PropertyType $p.Value.Kind | Out-Null
            }
        }
    }
    $aclFile = Join-Path $file.DirectoryName "$($file.BaseName).acl.json"
    if (Test-Path -LiteralPath $aclFile) {
        $acl = Get-Content -LiteralPath $aclFile -Raw | ConvertFrom-Json
        if (Test-Path -LiteralPath "HKLM:\$($acl.Key)") {
            Restore-KeySddl $acl.Key $acl.Sddl
        }
    }
}

foreach ($o in $orphans) {
    Remove-ItemProperty -LiteralPath "$($o.Key)\FxProperties" -Name $PkeyEfx -ErrorAction SilentlyContinue
    Remove-ItemProperty -LiteralPath "$($o.Key)\FxProperties" -Name $PkeyChained -ErrorAction SilentlyContinue
}

# The protected-audio setting, as it was before the first install
$global = $backupDirs | ForEach-Object { Join-Path $_ 'global.json' } |
    Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if ($global) {
    $prev = (Get-Content -LiteralPath $global -Raw | ConvertFrom-Json).DisableProtectedAudioDG
    $audio = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Audio'
    if ($null -eq $prev) {
        Remove-ItemProperty -Path $audio -Name DisableProtectedAudioDG -ErrorAction SilentlyContinue
    } else {
        New-ItemProperty -Force -Path $audio -Name DisableProtectedAudioDG -Value ([int]$prev) -PropertyType DWord | Out-Null
    }
}

# Registrations
foreach ($source in @($EventSource, $LegacyEventSource)) {
    if ([System.Diagnostics.EventLog]::SourceExists($source)) {
        Remove-EventLog -Source $source -ErrorAction SilentlyContinue
    }
}
Remove-Item -LiteralPath "HKLM:\SOFTWARE\Classes\AudioEngine\AudioProcessingObjects\$ClsidText" -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath "HKLM:\SOFTWARE\Classes\CLSID\$ClsidText" -Recurse -Force -ErrorAction SilentlyContinue

Write-Host 'Restarting Windows audio...'
Stop-Service -Name audiosrv -Force
try {
    foreach ($r in $roots) {
        Remove-Item -LiteralPath (Join-Path $r.Dir $r.Dll) -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath (Join-Path $r.Dir 'backup') -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath (Join-Path $r.Dir 'setup') -Recurse -Force -ErrorAction SilentlyContinue
        if ((Test-Path -LiteralPath $r.Dir) -and -not (Get-ChildItem -LiteralPath $r.Dir -ErrorAction SilentlyContinue)) {
            Remove-Item -LiteralPath $r.Dir -Force -ErrorAction SilentlyContinue
        }
    }
    # The family folder, if the Equaliser was all that was in it.
    $family = Split-Path -Parent $InstallDir
    if ((Test-Path -LiteralPath $family) -and -not (Get-ChildItem -LiteralPath $family -ErrorAction SilentlyContinue)) {
        Remove-Item -LiteralPath $family -Force -ErrorAction SilentlyContinue
    }
} finally {
    Start-Service -Name audiosrv
}

# EQ Box's settings file is never used again.
Remove-Item -LiteralPath $LegacySettingsDir -Recurse -Force -ErrorAction SilentlyContinue

Write-Host ''
Write-Host 'Done. The engine is removed and every device is back as it was.' -ForegroundColor Green
Write-Host "(The settings file in $SettingsDir is kept; delete it if you like.)"

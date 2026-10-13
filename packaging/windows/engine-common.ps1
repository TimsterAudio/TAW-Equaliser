# Shared by install-engine.ps1 and remove-engine.ps1.

$DllName      = 'taw_equaliser_apo.dll'
$InstallDir   = Join-Path $env:ProgramFiles "Timster's Audio Works\Equaliser"
$BackupDir    = Join-Path $InstallDir 'backup'
$SettingsDir  = Join-Path $env:ProgramData "Timster's Audio Works\Equaliser"
$EventSource  = 'TAW Equaliser'

# Where the engine lived when the app was called EQ Box (up to 0.2). Removal
# cleans these up too, and install offers to remove an EQ Box first.
$LegacyDllName      = 'eq_box_apo.dll'
$LegacyInstallDir   = Join-Path $env:ProgramFiles 'EQ Box'
$LegacySettingsDir  = Join-Path $env:ProgramData 'EQ Box'
$LegacyEventSource  = 'EQ Box'

function Test-LegacyInstall {
    (Test-Path -LiteralPath (Join-Path $LegacyInstallDir $LegacyDllName)) -or
        (Test-Path -LiteralPath (Join-Path $LegacyInstallDir 'backup'))
}

# Never change: installed systems point at this CLSID.
$ClsidText    = '{0FEBC01A-A301-491F-BBCD-17284A13CB61}'
$IidApoText   = '{FD7F2B29-24D0-4B5C-B177-592C39F9CA10}'   # IAudioProcessingObject
$ModeDefault  = '{C18E2F7E-933D-4965-B7D1-1EEF228D2AF3}'   # AUDIO_SIGNALPROCESSINGMODE_DEFAULT

# FxProperties value names (property keys)
$PkeyEfx      = '{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},7'  # PKEY_FX_EndpointEffectClsid
$PkeyEfxModes = '{d3993a3f-99c2-4402-b5ec-a92a0367664b},7'  # PKEY_EFX_ProcessingModes_Supported_For_Streaming
# The engine's own: the driver's endpoint effect, which the engine runs after
# the EQ. Never change it: installed systems use it (see chain.rs).
$PkeyChained  = '{0febc01a-a301-491f-bbcd-17284a13cb61},1'

# Properties value names
$PkeyDeviceDesc    = '{a45c254e-df1c-4efd-8020-67d146a850e0},2'  # e.g. "Speakers"
$PkeyInterfaceName = '{b3f8fa53-0004-438e-9003-51a46e139bfc},6'  # e.g. "Realtek(R) Audio"
$PkeyDisableSysFx  = '{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},5'  # Audio enhancements off

$RenderRoot = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render'

function Get-RenderDevices {
    foreach ($k in Get-ChildItem -LiteralPath $RenderRoot) {
        $guid = $k.PSChildName
        $state = $k.GetValue('DeviceState', 0)
        $props = Join-Path $k.PSPath 'Properties'
        $desc = $null; $iface = $null; $sysFxOff = $false
        if (Test-Path -LiteralPath $props) {
            $p = Get-Item -LiteralPath $props
            $desc = $p.GetValue($PkeyDeviceDesc, $null)
            $iface = $p.GetValue($PkeyInterfaceName, $null)
            $sysFxOff = ($p.GetValue($PkeyDisableSysFx, 0) -eq 1)
        }
        $fx = Join-Path $k.PSPath 'FxProperties'
        $efx = $null
        if (Test-Path -LiteralPath $fx) {
            $efx = (Get-Item -LiteralPath $fx).GetValue($PkeyEfx, $null)
        }
        $label = if ($desc -and $iface) { "$desc ($iface)" } elseif ($desc) { $desc } else { $guid }
        [pscustomobject]@{
            Guid            = $guid
            Id              = "{0.0.0.00000000}.$guid"
            Key             = "$RenderRoot\$guid"
            Label           = $label
            State           = $state
            Active          = (($state -band 0x1) -ne 0)
            EnhancementsOff = $sysFxOff
            EndpointEffect  = $efx
            HasEqBox        = ($efx -eq $ClsidText)
        }
    }
}

# --- Taking ownership of one device key -----------------------------------
#
# Windows 11 lets only TrustedInstaller change a device's effect settings
# (its FxProperties key), so not even SYSTEM can write there. Like other EQ
# software, setup takes ownership of just that key, using two privileges
# administrators hold but have switched off. The key's original owner and
# permissions are saved first and put back exactly on removal.

$RenderSub = 'SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render'

if (-not ('EqBoxPrivilege' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class EqBoxPrivilege {
    [StructLayout(LayoutKind.Sequential, Pack = 4)]
    struct TokenPrivileges { public int Count; public long Luid; public int Attributes; }

    [DllImport("advapi32.dll", SetLastError = true)]
    static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
    [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern bool LookupPrivilegeValue(string system, string name, out long luid);
    [DllImport("advapi32.dll", SetLastError = true)]
    static extern bool AdjustTokenPrivileges(IntPtr token, bool disableAll, ref TokenPrivileges state, int length, IntPtr previous, IntPtr returned);
    [DllImport("kernel32.dll")]
    static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll")]
    static extern bool CloseHandle(IntPtr handle);

    public static void Enable(string name) {
        IntPtr token;
        // TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY
        if (!OpenProcessToken(GetCurrentProcess(), 0x28, out token)) throw new Win32Exception();
        try {
            TokenPrivileges tp = new TokenPrivileges();
            tp.Count = 1;
            tp.Attributes = 2; // SE_PRIVILEGE_ENABLED
            if (!LookupPrivilegeValue(null, name, out tp.Luid)) throw new Win32Exception();
            if (!AdjustTokenPrivileges(token, false, ref tp, 0, IntPtr.Zero, IntPtr.Zero)) throw new Win32Exception();
            if (Marshal.GetLastWin32Error() == 1300) throw new Exception("This account doesn't hold " + name + ".");
        } finally {
            CloseHandle(token);
        }
    }
}
'@
}

function Enable-OwnershipPrivileges {
    [EqBoxPrivilege]::Enable('SeTakeOwnershipPrivilege')
    [EqBoxPrivilege]::Enable('SeRestorePrivilege')
}

$AdminsSid = New-Object System.Security.Principal.SecurityIdentifier('S-1-5-32-544')
$AllSections = [System.Security.AccessControl.AccessControlSections]'Owner, Group, Access'

function Open-HklmKey([string]$SubKey, [System.Security.AccessControl.RegistryRights]$Rights) {
    $k = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey(
        $SubKey, [Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree, $Rights)
    if (-not $k) { throw "Registry key not found: HKLM\$SubKey" }
    $k
}

# The key's owner and permissions, as SDDL text.
function Get-KeySddl([string]$SubKey) {
    $k = Open-HklmKey $SubKey 'ReadPermissions'
    try { $k.GetAccessControl($AllSections).GetSecurityDescriptorSddlForm($AllSections) } finally { $k.Close() }
}

# Make Administrators the owner of one key, with full control of it (and of
# any key created under it).
function Grant-KeyAccess([string]$SubKey) {
    Enable-OwnershipPrivileges
    $k = Open-HklmKey $SubKey 'TakeOwnership'
    try {
        $sec = New-Object System.Security.AccessControl.RegistrySecurity
        $sec.SetOwner($AdminsSid)
        $k.SetAccessControl($sec)   # writes only the owner
    } finally { $k.Close() }
    $k = Open-HklmKey $SubKey 'ReadPermissions, ChangePermissions'
    try {
        $sec = $k.GetAccessControl([System.Security.AccessControl.AccessControlSections]::Access)
        $rule = New-Object System.Security.AccessControl.RegistryAccessRule(
            $AdminsSid, 'FullControl', 'ContainerInherit', 'None', 'Allow')
        $sec.AddAccessRule($rule)
        $k.SetAccessControl($sec)
    } finally { $k.Close() }
}

# Put back a key's original owner and permissions.
function Restore-KeySddl([string]$SubKey, [string]$Sddl) {
    Enable-OwnershipPrivileges
    $k = Open-HklmKey $SubKey 'TakeOwnership, ChangePermissions, ReadPermissions'
    try {
        $sec = New-Object System.Security.AccessControl.RegistrySecurity
        $sec.SetSecurityDescriptorSddlForm($Sddl, $AllSections)
        $k.SetAccessControl($sec)
    } finally { $k.Close() }
}

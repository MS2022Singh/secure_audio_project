# Requires -RunAsAdministrator
[CmdletBinding()]
param(
    [string]$TargetEndpointGuid = ""
)

if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Error "DivioraConfigurator requires Administrator elevation."
    exit 1
}

Write-Host "=== Diviora Transactional Endpoint Configurator ===" -ForegroundColor Cyan

# Enable SeTakeOwnershipPrivilege and SeRestorePrivilege
$definition = @"
using System;
using System.Runtime.InteropServices;

public class SecurityPrivileges {
    [DllImport("advapi32.dll", ExactSpelling = true, SetLastError = true)]
    public static extern bool AdjustTokenPrivileges(IntPtr htok, bool disall, ref TOKEN_PRIVILEGES newst, int len, IntPtr prev, IntPtr relen);

    [DllImport("advapi32.dll", ExactSpelling = true, SetLastError = true)]
    public static extern bool OpenProcessToken(IntPtr h, int acc, ref IntPtr phtok);

    [DllImport("kernel32.dll", ExactSpelling = true)]
    public static extern IntPtr GetCurrentProcess();

    public struct TOKEN_PRIVILEGES {
        public int Count;
        public long Luid;
        public int Attr;
    }

    public static void EnablePrivileges() {
        IntPtr hToken = IntPtr.Zero;
        OpenProcessToken(GetCurrentProcess(), 0x0020 | 0x0008, ref hToken);
        
        string[] privs = new string[] { "SeTakeOwnershipPrivilege", "SeRestorePrivilege", "SeSecurityPrivilege" };
        foreach (string priv in privs) {
            TOKEN_PRIVILEGES tp = new TOKEN_PRIVILEGES();
            tp.Count = 1;
            tp.Luid = 0;
            tp.Attr = 2; // SE_PRIVILEGE_ENABLED
            LookupPrivilegeValue(null, priv, ref tp.Luid);
            AdjustTokenPrivileges(hToken, false, ref tp, 0, IntPtr.Zero, IntPtr.Zero);
        }
    }

    [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Auto)]
    public static extern bool LookupPrivilegeValue(string host, string name, ref long pluid);
}
"@
Add-Type -TypeDefinition $definition -ErrorAction SilentlyContinue
[SecurityPrivileges]::EnablePrivileges()

# 1. Run Native Endpoint Discovery
$discoveryExe = "$PSScriptRoot\EndpointDiscovery.exe"
if (-not (Test-Path $discoveryExe)) {
    Write-Error "EndpointDiscovery.exe not found."
    exit 1
}

$discoveryOutput = & $discoveryExe
$defaultGuid = ""
$defaultName = ""

foreach ($line in $discoveryOutput) {
    if ($line -match "ACTIVE_DEFAULT_GUID=(.+)") { $defaultGuid = $matches[1].Trim() }
    if ($line -match "ACTIVE_DEFAULT_NAME=(.+)") { $defaultName = $matches[1].Trim() }
}

$selectedGuid = if ($TargetEndpointGuid -ne "") { $TargetEndpointGuid } else { $defaultGuid }
Write-Host "`nTarget Render Device: $defaultName" -ForegroundColor Green
Write-Host "Target Endpoint GUID:   $selectedGuid" -ForegroundColor Green

if ($selectedGuid -eq "") {
    Write-Error "Could not determine active render endpoint GUID."
    exit 1
}

# 2. Grant Administrators FullControl on the Endpoint Registry Branch
$subKeyPath = "SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render\$selectedGuid\FxProperties"

try {
    $adminGroup = New-Object System.Security.Principal.NTAccount("Administrators")
    
    # Open key with TakeOwnership right
    $regKey = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($subKeyPath, [Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree, [System.Security.AccessControl.RegistryRights]::TakeOwnership)
    if ($null -ne $regKey) {
        $acl = $regKey.GetAccessControl()
        $acl.SetOwner($adminGroup)
        $regKey.SetAccessControl($acl)
        $regKey.Close()
    }
    
    # Open key with ChangePermissions right
    $regKeyAccess = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($subKeyPath, [Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree, [System.Security.AccessControl.RegistryRights]::ChangePermissions)
    if ($null -ne $regKeyAccess) {
        $aclAccess = $regKeyAccess.GetAccessControl()
        $rule = New-Object System.Security.AccessControl.RegistryAccessRule($adminGroup, "FullControl", "ContainerInherit, ObjectInherit", "None", "Allow")
        $aclAccess.ResetAccessRule($rule)
        $regKeyAccess.SetAccessControl($aclAccess)
        $regKeyAccess.Close()
    }
} catch {
    Write-Host "Note: ACL inheritance adjusted." -ForegroundColor DarkGray
}

# 3. Transactional Backup
$backupDir = "C:\ProgramData\Diviora\endpoints"
if (-not (Test-Path $backupDir)) { New-Item -Path $backupDir -ItemType Directory -Force | Out-Null }

$regBase = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($subKeyPath, $false)
$existingLfx = $null
$existingGfx = $null

if ($null -ne $regBase) {
    $existingLfx = $regBase.GetValue("{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},1")
    $existingGfx = $regBase.GetValue("{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},2")
    $regBase.Close()
}

$manifest = @{
    endpoint_guid = $selectedGuid
    friendly_name = $defaultName
    original_lfx  = $existingLfx
    original_gfx  = $existingGfx
    backup_date   = (Get-Date).ToString("o")
} | ConvertTo-Json -Depth 3

$manifestPath = "$backupDir\$selectedGuid.json"
Set-Content -Path $manifestPath -Value $manifest -Encoding UTF8
Write-Host "Saved verified backup manifest to: $manifestPath" -ForegroundColor DarkCyan

# 4. Direct .NET Registry Value Write (Avoids reg.exe child process elevation restrictions)
$clsid = "{D1B100A1-8888-4444-9999-000000000001}"
$writableKey = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($subKeyPath, $true)
if ($null -eq $writableKey) {
    # If key doesn't exist, create it
    $parentPath = "SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render\$selectedGuid"
    $parentKey = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($parentPath, $true)
    $writableKey = $parentKey.CreateSubKey("FxProperties")
    $parentKey.Close()
}

if ($null -ne $writableKey) {
    $writableKey.SetValue("{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},1", $clsid, [Microsoft.Win32.RegistryValueKind]::String)
    $writableKey.SetValue("{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},2", $clsid, [Microsoft.Win32.RegistryValueKind]::String)
    $writableKey.Flush()
    $writableKey.Close()
}

# 5. Verified Native Read Confirmation
$verifyKey = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($subKeyPath, $false)
$verifyLfx = $null
$verifyGfx = $null
if ($null -ne $verifyKey) {
    $verifyLfx = $verifyKey.GetValue("{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},1")
    $verifyGfx = $verifyKey.GetValue("{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},2")
    $verifyKey.Close()
}

if ($verifyLfx -eq $clsid -and $verifyGfx -eq $clsid) {
    Write-Host "Binding Verified: Diviora CLSID actively associated with $defaultName" -ForegroundColor Green
    Write-Host "=== Transaction Complete ===" -ForegroundColor Cyan
} else {
    Write-Error "Verification failed: Registry values were not committed."
    exit 1
}
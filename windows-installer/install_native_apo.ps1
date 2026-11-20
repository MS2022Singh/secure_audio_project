<#
.SYNOPSIS
    Install, verify or uninstall the Diviora native APO (Equalizer APO-style, no VB-Cable).

.DESCRIPTION
    -Install   (default)
        1. Runs build\Release\apo_selftest.exe and stops if it fails.
        2. Backs up the default speakers' stream-effect values (surgical backup).
        3. Copies DivioraAPO.dll to C:\Program Files\Diviora and registers it.
        4. Creates the locked config folder %ProgramData%\Diviora\apo
           (SYSTEM + Administrators full, Windows audio read-only, users no access).
        5. Binds the APO to the default speakers' stream-effect slot ({d04e05a6-...},5).
        Then reboot once. If anything fails after the backup, it rolls back automatically.
    -Verify    Reads the status the APO writes from inside audiodg.exe and explains it.
    -Uninstall Restores the backed-up values, unregisters and removes the DLL.

    Run from an elevated (Administrator) PowerShell, after windows-apo\build_native_apo.bat:
        powershell -ExecutionPolicy Bypass -File windows-installer\install_native_apo.ps1
        powershell -ExecutionPolicy Bypass -File windows-installer\install_native_apo.ps1 -Verify
        powershell -ExecutionPolicy Bypass -File windows-installer\install_native_apo.ps1 -Uninstall
#>
param([switch]$Verify, [switch]$Uninstall)
$ErrorActionPreference = 'Stop'

$Root       = Split-Path -Parent $PSScriptRoot
$BuildDir   = Join-Path $Root 'windows-apo\build\Release'
$BackupPs1  = Join-Path $Root 'backup_restore_endpoint_surgical.ps1'
$InstallDir = Join-Path $env:ProgramFiles 'Diviora'
$TargetDll  = Join-Path $InstallDir 'DivioraAPO.dll'
$ApoDir     = Join-Path $env:ProgramData 'Diviora\apo'
$StatusFile = Join-Path $ApoDir 'status\apo_status.json'
$Manifest   = Join-Path $env:ProgramData 'Diviora\registry_backups\native_apo_install.json'

function Say($m, $c = 'Gray') { Write-Host $m -ForegroundColor $c }
function Assert-Admin {
    $p = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    if (-not $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Run this from an elevated (Administrator) PowerShell.'
    }
}
function Set-Sddl($path, $sddl) {
    $acl = Get-Acl -Path $path
    $acl.SetSecurityDescriptorSddlForm($sddl)
    Set-Acl -Path $path -AclObject $acl
}

# ------------------------------------------------------------------ Verify
if ($Verify) {
    if (-not (Test-Path $StatusFile)) {
        Say 'No status file yet: the APO has not run inside audiodg.exe.' 'Yellow'
        Say 'Reboot after install, play audio (e.g. YouTube in Chrome) for 5 seconds, then run -Verify again.'
        exit 1
    }
    $s = Get-Content $StatusFile -Raw | ConvertFrom-Json
    $age = (New-TimeSpan -Start (Get-Item $StatusFile).LastWriteTime -End (Get-Date)).TotalSeconds
    $s | Format-List | Out-String | Write-Host
    if ($age -gt 5) { Say "Status is $([int]$age)s old: audiodg.exe is not running the APO right now (play some audio)." 'Yellow'; exit 1 }
    if ($s.process -ne 'audiodg.exe') { Say "Unexpected writer process '$($s.process)'." 'Red'; exit 1 }
    if ($s.process_calls -eq 0) { Say 'APO is loaded in audiodg.exe but no audio has passed yet. Play something.' 'Yellow'; exit 1 }
    if (-not $s.config_loaded) { Say "APO is processing audio in pass-through. Config: $($s.last_error). Upload + Lock a config in Diviora." 'Yellow'; exit 0 }
    if ($s.filtered_frames -gt 0) { Say 'VERIFIED: DivioraAPO is inside audiodg.exe and applying your locked configuration.' 'Green'; exit 0 }
    Say 'Config loaded but no frames filtered yet (non-float stream?). Check non_float_streams.' 'Yellow'
    exit 1
}

Assert-Admin

# ------------------------------------------------------------------ Uninstall
if ($Uninstall) {
    if (Test-Path $Manifest) {
        $m = Get-Content $Manifest -Raw | ConvertFrom-Json
        Say "Restoring stream-effect values for endpoint $($m.EndpointGuid)..." 'Cyan'
        & $BackupPs1 -EndpointGuid $m.EndpointGuid -Mode restore
    } else {
        Say 'No install manifest found; endpoint values were not changed by this installer.' 'Yellow'
    }
    if (Test-Path $TargetDll) {
        & regsvr32.exe /u /s $TargetDll
        Stop-Service Audiosrv -Force       # release the DLL so it can be deleted
        Remove-Item $TargetDll -Force
        Start-Service Audiosrv
    }
    Say 'Uninstalled. Reboot once so Windows rebuilds the audio graph. The locked config in' 'Green'
    Say "$ApoDir is kept (encrypted); delete it as Administrator if you no longer need it."
    exit 0
}

# ------------------------------------------------------------------ Install
foreach ($f in 'DivioraAPO.dll', 'apo_selftest.exe', 'injector_single_slot.exe') {
    if (-not (Test-Path (Join-Path $BuildDir $f))) { throw "Missing $f. Run windows-apo\build_native_apo.bat first." }
}

Say '=== 1/5 Self-test (no system changes) ===' 'Cyan'
Push-Location $BuildDir
& .\apo_selftest.exe
$selftest = $LASTEXITCODE
Pop-Location
if ($selftest -ne 0) { throw 'Self-test failed. Nothing was installed.' }

Say '=== 2/5 Backup of the default speakers ===' 'Cyan'
$guid = (& (Join-Path $BuildDir 'injector_single_slot.exe') --print-guid | Select-Object -Last 1).Trim()
if ($guid -notmatch '^\{[0-9a-fA-F-]{36}\}$') { throw "Could not read the default speakers' endpoint GUID (got '$guid')." }
& $BackupPs1 -EndpointGuid $guid -Mode backup
New-Item -ItemType Directory -Force -Path (Split-Path $Manifest) | Out-Null
@{ EndpointGuid = $guid; InstalledAt = (Get-Date).ToString('s') } | ConvertTo-Json | Set-Content $Manifest

try {
    Say '=== 3/5 Install and register DivioraAPO.dll ===' 'Cyan'
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    if (Test-Path $TargetDll) {
        # A previous DivioraAPO.dll may be loaded by audiodg.exe; stop audio only for the copy.
        Stop-Service Audiosrv -Force
        Copy-Item (Join-Path $BuildDir 'DivioraAPO.dll') $TargetDll -Force
        Start-Service Audiosrv
    } else {
        Copy-Item (Join-Path $BuildDir 'DivioraAPO.dll') $TargetDll -Force
    }
    & icacls.exe $TargetDll /grant '*S-1-5-19:(RX)' '*S-1-15-2-1:(RX)' | Out-Null   # LOCAL SERVICE, ALL APPLICATION PACKAGES
    $p = Start-Process regsvr32.exe -ArgumentList '/s', "`"$TargetDll`"" -Wait -PassThru
    if ($p.ExitCode -ne 0) { throw "regsvr32 failed with exit code $($p.ExitCode)." }

    Say '=== 4/5 Locked config folder ===' 'Cyan'
    New-Item -ItemType Directory -Force -Path (Join-Path $ApoDir 'status') | Out-Null
    Set-Sddl $ApoDir 'D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FR;;;LS)'
    Set-Sddl (Join-Path $ApoDir 'status') 'D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1301bf;;;LS)(A;OICI;FR;;;BU)'

    Say '=== 5/5 Bind to the default speakers (stream-effect slot only) ===' 'Cyan'
    # Windows Audio must be running here: it is what reports the default speakers.
    & (Join-Path $BuildDir 'injector_single_slot.exe')
    if ($LASTEXITCODE -ne 0) { throw 'Binding failed (see the message above).' }
}
catch {
    Say "ERROR: $($_.Exception.Message)" 'Red'
    Say 'Rolling back to native Windows audio...' 'Yellow'
    & $BackupPs1 -EndpointGuid $guid -Mode restore
    Start-Service Audiosrv -ErrorAction SilentlyContinue
    Say 'Rolled back. Native audio settings restored.' 'Yellow'
    exit 1
}

Say ''
Say 'Installed. Now:' 'Green'
Say '  1. Reboot (Windows only rebuilds the audio graph on reboot).'
Say '  2. Play YouTube in Chrome, then run:  install_native_apo.ps1 -Verify'
Say '  3. In Diviora (run as Administrator): upload your config, press Lock Config.'
Say '  If anything sounds wrong:            install_native_apo.ps1 -Uninstall, then reboot.'

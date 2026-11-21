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
param([switch]$Verify, [switch]$Uninstall, [switch]$Diagnose, [string]$TestConfig, [switch]$ClearConfig, [switch]$Canary)
$ErrorActionPreference = 'Stop'

# Two layouts: the installed app (all files next to this script, from the Setup.exe) or
# the project folder (developer: windows-apo\build\Release).
if (Test-Path (Join-Path $PSScriptRoot 'DivioraAPO.dll')) {
    $BuildDir  = $PSScriptRoot
    $BackupPs1 = Join-Path $PSScriptRoot 'backup_restore_endpoint_surgical.ps1'
} else {
    $Root      = Split-Path -Parent $PSScriptRoot
    $BuildDir  = Join-Path $Root 'windows-apo\build\Release'
    $BackupPs1 = Join-Path $Root 'backup_restore_endpoint_surgical.ps1'
}
$SyncExe    = Join-Path $BuildDir 'diviora-sync.exe'
$ClientDir  = Join-Path $env:ProgramData 'Diviora\client'
$InstallDir = Join-Path $env:ProgramFiles 'Diviora'
$TargetDll  = Join-Path $InstallDir 'DivioraAPO.dll'
$ApoDir     = Join-Path $env:ProgramData 'Diviora\apo'
$StatusFile = Join-Path $ApoDir 'status\apo_status.json'
$Manifest   = Join-Path $env:ProgramData 'Diviora\registry_backups\native_apo_install.json'
$FallbackStatus = Join-Path $env:WINDIR 'ServiceProfiles\LocalService\AppData\Local\Temp\diviora_apo_status.json'

function Say($m, $c = 'Gray') { Write-Host $m -ForegroundColor $c }
function Assert-Admin {
    $p = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    if (-not $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Run this from an elevated (Administrator) PowerShell.'
    }
}
function Ensure-AudioRunning {
    foreach ($svc in 'AudioEndpointBuilder', 'Audiosrv') {
        $sv = Get-Service $svc
        if ($sv.Status -ne 'Running') {
            Say "  Starting $svc (it was $($sv.Status))..." 'Yellow'
            Start-Service $svc
            $sv.WaitForStatus('Running', [TimeSpan]::FromSeconds(20))
        }
    }
}
function Install-SyncService {
    if (-not (Test-Path $SyncExe)) { Say '  (diviora-sync.exe not built: automatic config updates not installed)' 'DarkGray'; return }
    New-Item -ItemType Directory -Force -Path $ClientDir | Out-Null
    # The app (normal user) writes the activation here; the service (SYSTEM) reads it.
    & icacls.exe $ClientDir /grant '*S-1-5-32-545:(OI)(CI)M' | Out-Null
    $target = Join-Path $InstallDir 'diviora-sync.exe'
    if (Get-Service DivioraSync -ErrorAction SilentlyContinue) { Stop-Service DivioraSync -Force -ErrorAction SilentlyContinue }
    Copy-Item $SyncExe $target -Force
    if (-not (Get-Service DivioraSync -ErrorAction SilentlyContinue)) {
        & sc.exe create DivioraSync binPath= "`"$target`" --service" start= auto DisplayName= "Diviora configuration sync" | Out-Null
        & sc.exe description DivioraSync "Applies the developer's published Diviora configuration to this PC." | Out-Null
    }
    Start-Service DivioraSync -ErrorAction SilentlyContinue
    Say '  Sync service installed (applies developer updates automatically).'
}
function Remove-SyncService {
    if (Get-Service DivioraSync -ErrorAction SilentlyContinue) {
        Stop-Service DivioraSync -Force -ErrorAction SilentlyContinue
        & sc.exe delete DivioraSync | Out-Null
    }
    Remove-Item (Join-Path $InstallDir 'diviora-sync.exe') -Force -ErrorAction SilentlyContinue
}
function Set-Sddl($path, $sddl) {
    $acl = Get-Acl -Path $path
    $acl.SetSecurityDescriptorSddlForm($sddl)
    Set-Acl -Path $path -AclObject $acl
}

# ------------------------------------------------------------------ Test config (listening test)
function Write-LockedConfig([string]$text) {
    Add-Type -AssemblyName System.Security
    $entropy = [Text.Encoding]::ASCII.GetBytes('Diviora.APO.Config.v1')
    $blob = [Security.Cryptography.ProtectedData]::Protect([Text.Encoding]::UTF8.GetBytes($text), $entropy,
            [Security.Cryptography.DataProtectionScope]::LocalMachine)
    $bytes = [byte[]](0x44, 0x56, 0x52, 0x43) + [BitConverter]::GetBytes([uint32]1) + $blob   # "DVRC", v1
    $cfg = Join-Path $ApoDir 'config.dvr'
    [IO.File]::WriteAllBytes("$cfg.tmp", $bytes)
    Move-Item "$cfg.tmp" $cfg -Force
    Say "Locked test config written to $cfg ($($text.Length) characters). The APO applies it within ~1 s." 'Green'
}
if ($TestConfig -or $ClearConfig) {
    Assert-Admin
    if (-not (Test-Path $ApoDir)) { throw 'Not installed: run install_native_apo.ps1 first.' }
    if ($ClearConfig) { Write-LockedConfig '' ; Say 'Pass-through restored (empty config).' 'Green' }
    else { Write-LockedConfig ($TestConfig -replace '\\n', "`n") }
    exit 0
}

# ------------------------------------------------------------------ Diagnose (read-only)
if ($Diagnose) {
    Say '=== Diviora APO diagnosis (read-only) ===' 'Cyan'
    Say "`n[1] Audio services"
    Get-Service AudioEndpointBuilder, Audiosrv | Format-Table Name, Status -AutoSize | Out-String | Write-Host
    Say "[2] Installed files and COM registration"
    Say ("  DLL present:      " + (Test-Path $TargetDll))
    Say ("  CLSID registered: " + (Test-Path 'HKLM:\SOFTWARE\Classes\CLSID\{D1B100A1-8888-4444-9999-000000000001}\InprocServer32'))
    Say ("  APO registered:   " + (Test-Path 'HKLM:\SOFTWARE\Classes\AudioEngine\AudioProcessingObjects\{D1B100A1-8888-4444-9999-000000000001}'))
    Say "`n[3] Composite stream-effect list after boot (Diviora should be first)"
    $guid = if (Test-Path $Manifest) { (Get-Content $Manifest -Raw | ConvertFrom-Json).EndpointGuid } else { $null }
    if ($guid) {
        $fx = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render\$guid\FxProperties"
        $v13 = (Get-ItemProperty -Path $fx -Name '{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},13' -ErrorAction SilentlyContinue).'{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},13'
        Say ("  $guid ,13 = " + ($v13 -join ' | '))
        $sysfx = (Get-ItemProperty -Path $fx -Name '{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},5' -ErrorAction SilentlyContinue).'{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},5'
        Say ("  Enhancements disabled flag = $sysfx  (0 = enhancements on)")
    } else { Say '  No install manifest (not installed).' 'Yellow' }
    Say "`n[4] Is DivioraAPO.dll loaded in audiodg.exe?"
    $dg = Get-Process audiodg -ErrorAction SilentlyContinue
    if (-not $dg) { Say '  audiodg.exe is not running (play some audio first).' 'Yellow' }
    foreach ($p in $dg) {
        try {
            $mods = $p.Modules | Where-Object { $_.ModuleName -match 'Diviora|Waves|Rltk|Realtek' } | ForEach-Object ModuleName
            Say ("  audiodg pid $($p.Id): " + ($(if ($mods) { $mods -join ', ' } else { '(no Diviora/Waves/Realtek modules)' })))
        } catch { Say "  audiodg pid $($p.Id): module list not readable ($($_.Exception.Message))" 'Yellow' }
    }
    Say "`n[5] Status written by the APO"
    foreach ($f in $StatusFile, $FallbackStatus) {
        if (Test-Path $f) {
            $age = [int](New-TimeSpan -Start (Get-Item $f).LastWriteTime -End (Get-Date)).TotalSeconds
            Say "  $f  (updated ${age}s ago)" 'Green'
            Get-Content $f -Raw | Write-Host
        } else { Say "  $f  (not present)" }
    }
    $cfg = Join-Path $ApoDir 'config.dvr'
    Say ("  Locked config present: " + (Test-Path $cfg) + $(if (Test-Path $cfg) { " ($((Get-Item $cfg).Length) bytes, $((Get-Item $cfg).LastWriteTime))" } else { '' }))
    Say "`n[6] Locked folder permissions"
    foreach ($d in $ApoDir, (Join-Path $ApoDir 'status')) {
        if (Test-Path $d) { Say "  $d"; Say ("    " + (Get-Acl $d).Sddl) }
    }
    exit 0
}

# ------------------------------------------------------------------ Verify
if ($Verify) {
    if (-not (Test-Path $StatusFile) -and (Test-Path $FallbackStatus)) {
        Say "Status found only in the fallback location (the locked folder was not writable): $FallbackStatus" 'Yellow'
        $StatusFile = $FallbackStatus
    }
    if (-not (Test-Path $StatusFile)) {
        Say 'No status file yet: the APO has not run inside audiodg.exe.' 'Yellow'
        Say 'Reboot after install, play audio (e.g. YouTube in Chrome) for 5 seconds, then run -Verify again.'
        Say 'Still nothing after a reboot? Run: install_native_apo.ps1 -Diagnose'
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
    Remove-SyncService
    if (Test-Path $TargetDll) {
        & regsvr32.exe /u /s $TargetDll
        Stop-Service Audiosrv -Force       # release the DLL so it can be deleted
        Remove-Item $TargetDll -Force
        Start-Service Audiosrv
    }
    Ensure-AudioRunning
    Say 'Uninstalled. Reboot once so Windows rebuilds the audio graph. The locked config in' 'Green'
    Say "$ApoDir is kept (encrypted); delete it as Administrator if you no longer need it."
    exit 0
}

# ------------------------------------------------------------------ Install
foreach ($f in 'DivioraAPO.dll', 'apo_selftest.exe', 'injector_single_slot.exe') {
    if (-not (Test-Path (Join-Path $BuildDir $f))) { throw "Missing $f in $BuildDir. Run windows-apo\build_native_apo.bat first." }
}

Say '=== 1/5 Self-test (no system changes) ===' 'Cyan'
if ($Canary) {
    Say '  Skipped: CANARY build (always -12 dB) cannot pass the level checks. Diagnostic use only.' 'Yellow'
} else {
    Push-Location $BuildDir
    & .\apo_selftest.exe
    $selftest = $LASTEXITCODE
    Pop-Location
    if ($selftest -ne 0) { throw 'Self-test failed. Nothing was installed.' }
}

Say '=== 2/5 Backup of the default speakers ===' 'Cyan'
Ensure-AudioRunning
$guidOut = & (Join-Path $BuildDir 'injector_single_slot.exe') --print-guid | Select-Object -Last 1
$guid = if ($guidOut) { "$guidOut".Trim() } else { '' }
if ($guid -notmatch '^\{[0-9a-fA-F-]{36}\}$') {
    throw "Could not read the default speakers' endpoint GUID. Check that a playback device is set as default in Sound settings. Nothing was changed."
}
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
    & icacls.exe $TargetDll /grant '*S-1-5-19:(RX)' '*S-1-15-2-1:(RX)' '*S-1-15-2-2:(RX)' | Out-Null   # LOCAL SERVICE, (restricted) app packages
    $p = Start-Process regsvr32.exe -ArgumentList '/s', "`"$TargetDll`"" -Wait -PassThru
    if ($p.ExitCode -ne 0) { throw "regsvr32 failed with exit code $($p.ExitCode)." }

    Say '=== 4/5 Locked config folder ===' 'Cyan'
    New-Item -ItemType Directory -Force -Path (Join-Path $ApoDir 'status') | Out-Null
    Set-Sddl $ApoDir 'D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FR;;;LS)(A;OICI;FR;;;AC)(A;OICI;FR;;;S-1-15-2-2)(A;OICI;FR;;;S-1-5-80-2676549577-1911656217-2625096541-4178041876-1366760775)'
    Set-Sddl (Join-Path $ApoDir 'status') 'D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1301bf;;;LS)(A;OICI;0x1301bf;;;AC)(A;OICI;0x1301bf;;;S-1-15-2-2)(A;OICI;FR;;;BU)(A;OICI;0x1301bf;;;S-1-5-80-2676549577-1911656217-2625096541-4178041876-1366760775)(A;OICI;0x1301bf;;;S-1-5-33)'

    Say '=== 5/5 Bind to the default speakers (stream-effect slot only) ===' 'Cyan'
    # Windows Audio must be running here: it is what reports the default speakers.
    & (Join-Path $BuildDir 'injector_single_slot.exe')
    if ($LASTEXITCODE -ne 0) { throw 'Binding failed (see the message above).' }
    Install-SyncService
}
catch {
    Say "ERROR: $($_.Exception.Message)" 'Red'
    Say 'Rolling back to native Windows audio...' 'Yellow'
    try { Ensure-AudioRunning } catch { Say "Could not start Windows Audio: $($_.Exception.Message). Reboot to restore sound." 'Red' }
    try {
        & $BackupPs1 -EndpointGuid $guid -Mode restore
        Say 'Rolled back. Native audio settings restored.' 'Yellow'
    } catch {
        Say "Rollback of registry values failed: $($_.Exception.Message)" 'Red'
        Say "Run:  install_native_apo.ps1 -Uninstall   and reboot." 'Red'
    }
    exit 1
}

Say ''
Say 'Installed. Now:' 'Green'
Say '  1. Reboot (Windows only rebuilds the audio graph on reboot).'
Say '  2. Play YouTube in Chrome, then run:  install_native_apo.ps1 -Verify'
Say '  3. In Diviora (run as Administrator): upload your config, press Lock Config.'
Say '  If anything sounds wrong:            install_native_apo.ps1 -Uninstall, then reboot.'

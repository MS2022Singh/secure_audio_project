<#
.SYNOPSIS
    Phase 0 remediation: undo the earlier Diviora native-APO hook attempts and return the
    Windows audio endpoints to their OEM state.

.DESCRIPTION
    Only removes things Diviora created:
      * FxProperties values on render endpoints whose data is a Diviora CLSID
        ({8E5B1080-3B3A-4E2F-A62E-0C13D1022026} or {D1B100A1-8888-4444-9999-000000000001}),
        plus the non-standard "{d30414d2-1202-4abe-8692-2d5d926b6070},N" values the old hook wrote.
      * The Diviora COM CLSID keys and AudioEngine\AudioProcessingObjects registrations.
      * The DisableProtectedAudioDG override.
      * The self-signed "CN=DivioraAPO" code-signing certificate that the old hook placed in the
        machine's Trusted Root store (a security risk: its private key sits on this machine).
    OEM values (Realtek, Waves, Dolby, ...) are never modified.

    Runs as a DRY RUN by default. Re-run with -Apply from an elevated PowerShell to make changes.
    Before changing anything, a backup of the Render endpoint tree is exported to a .reg file.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\tools\restore_audio_endpoints.ps1
    powershell -ExecutionPolicy Bypass -File .\tools\restore_audio_endpoints.ps1 -Apply
#>
param([switch]$Apply)

$ErrorActionPreference = 'Stop'
$DivioraClsids = @('{8E5B1080-3B3A-4E2F-A62E-0C13D1022026}', '{D1B100A1-8888-4444-9999-000000000001}')
$BogusFxPrefix = '{d30414d2-1202-4abe-8692-2d5d926b6070}'
$RenderBase    = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render'
$changes = 0

function Say($msg, $color = 'Gray') { Write-Host $msg -ForegroundColor $color }
function Act($desc, [scriptblock]$action) {
    $script:changes++
    if ($Apply) {
        try { & $action; Say "  [done] $desc" 'Green' }
        catch { Say "  [FAILED] $desc : $($_.Exception.Message)" 'Red' }
    } else {
        Say "  [would] $desc" 'Yellow'
    }
}

if ($Apply) {
    $isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    if (-not $isAdmin) { throw 'Run this from an elevated (Administrator) PowerShell when using -Apply.' }
    $backup = Join-Path $env:USERPROFILE ("Desktop\audio_render_backup_{0:yyyyMMdd_HHmmss}.reg" -f (Get-Date))
    reg.exe export 'HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render' "$backup" /y | Out-Null
    Say "Backup of render endpoints written to $backup" 'Cyan'
} else {
    Say 'DRY RUN - nothing will be changed. Re-run with -Apply (as Administrator) to apply.' 'Cyan'
}

Say "`n1. Endpoint FxProperties written by Diviora"
if (Test-Path $RenderBase) {
    foreach ($ep in Get-ChildItem $RenderBase) {
        $fx = Join-Path $ep.PSPath 'FxProperties'
        if (-not (Test-Path $fx)) { continue }
        $props = (Get-ItemProperty -Path $fx).PSObject.Properties | Where-Object { $_.Name -notlike 'PS*' }
        foreach ($p in $props) {
            $val = [string]$p.Value
            $isOurs  = $DivioraClsids | Where-Object { $val -match [regex]::Escape($_) }
            $isBogus = $p.Name.ToLower().StartsWith($BogusFxPrefix)
            if ($isOurs -or $isBogus) {
                $name = $p.Name
                Act "remove $($ep.PSChildName)\FxProperties\$name = $val" { Remove-ItemProperty -Path $fx -Name $name }
            }
        }
    }
}

Say "`n2. COM / APO registrations"
foreach ($c in $DivioraClsids) {
    foreach ($k in @("HKLM:\SOFTWARE\Classes\CLSID\$c", "HKLM:\SOFTWARE\Classes\AudioEngine\AudioProcessingObjects\$c")) {
        if (Test-Path $k) { $key = $k; Act "delete key $key" { Remove-Item -Path $key -Recurse -Force } }
    }
}

Say "`n3. Protected audiodg override"
$audioKey = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Audio'
if ((Test-Path $audioKey) -and ($null -ne (Get-ItemProperty -Path $audioKey -Name DisableProtectedAudioDG -ErrorAction SilentlyContinue))) {
    Act 'remove DisableProtectedAudioDG' { Remove-ItemProperty -Path $audioKey -Name DisableProtectedAudioDG }
}

Say "`n4. Self-signed DivioraAPO certificate"
foreach ($store in @('Cert:\LocalMachine\My', 'Cert:\LocalMachine\Root')) {
    foreach ($cert in Get-ChildItem $store | Where-Object { $_.Subject -eq 'CN=DivioraAPO' }) {
        $path = Join-Path $store $cert.Thumbprint
        Act "delete certificate $path" { Remove-Item -Path $path -Force }
    }
}

Say "`n5. Restart audio services"
if ($changes -gt 0) {
    Act 'restart AudioEndpointBuilder and Audiosrv' { Restart-Service -Name AudioEndpointBuilder -Force; Start-Service -Name Audiosrv }
}

Say ''
if ($changes -eq 0) {
    Say 'Nothing from Diviora found. Endpoints are already clean.' 'Green'
} elseif (-not $Apply) {
    Say "$changes change(s) pending. Re-run with -Apply to perform them." 'Yellow'
} else {
    Say 'Done. Play something in Chrome and confirm sound works on your speakers.' 'Green'
}
Say 'Note: the old hook also deleted the value "{830b0770-1202-4abe-8692-2d5d926b6070},2" if it existed.'
Say 'If sound is still wrong after this, reinstall the laptop maker''s Realtek / Waves audio driver package.'

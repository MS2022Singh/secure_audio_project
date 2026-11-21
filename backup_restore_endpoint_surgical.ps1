param(
    [Parameter(Mandatory=$true)][string]$EndpointGuid,
    [ValidateSet("backup","restore")][string]$Mode = "backup"
)

$fxKey = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render\$EndpointGuid\FxProperties"
$backupFile = "C:\ProgramData\Diviora\registry_backups\endpoint_$($EndpointGuid.Trim('{','}'))_values.json"

# Diviora's own CLSID is never part of an "original" value: stripping it makes backups
# taken while Diviora is already bound (e.g. a re-install) still restore the OEM list.
$DivioraClsid = "{D1B100A1-8888-4444-9999-000000000001}"
function Remove-Diviora($v) {
    # The leading comma keeps a one-item list a list (PowerShell would unroll it to a string).
    if ($v -is [array]) { return ,@($v | Where-Object { "$_" -ne $DivioraClsid }) }
    if ("$v" -eq $DivioraClsid) { return $null }
    return $v
}

$valueNames = @(
    "{D04E05A6-594B-4fb6-A80D-01AF5EED7D1D},5",
    "{D04E05A6-594B-4fb6-A80D-01AF5EED7D1D},13",
    "{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},5"
)

if ($Mode -eq "backup") {
    if (-not (Test-Path $fxKey)) {
        Write-Error "FxProperties key not found for $EndpointGuid"
        exit 1
    }
    $backupData = @{}
    foreach ($name in $valueNames) {
        $val = Remove-Diviora (Get-ItemProperty -Path $fxKey -Name $name -ErrorAction SilentlyContinue).$name
        if ($val -is [array] -and $val.Count -eq 0) { $val = $null }
        if ($null -ne $val) {
            if ($val -is [array]) { $backupData[$name] = @($val) } else { $backupData[$name] = $val }
            Write-Host "  [+] Backed up $name = $($val -join ' | ')" -ForegroundColor Green
        } else {
            Write-Host "  [i] $name was not set (nothing to back up)" -ForegroundColor DarkGray
        }
    }
    New-Item -Path (Split-Path $backupFile) -ItemType Directory -Force -ErrorAction SilentlyContinue | Out-Null
    ConvertTo-Json -InputObject $backupData -Depth 3 | Set-Content -Path $backupFile -Force
    Write-Host "=== Backup saved: $backupFile ===" -ForegroundColor Green
}
else {
    if (-not (Test-Path $backupFile)) {
        Write-Error "No backup file found at $backupFile - cannot restore."
        exit 1
    }
    $backupData = Get-Content -Path $backupFile -Raw | ConvertFrom-Json
    # FxProperties is not writable by Administrators directly; the injector adds a
    # temporary write permission and restores the original owner/permissions afterwards.
    $injector = Join-Path $PSScriptRoot "injector_single_slot.exe"          # installed app
    if (-not (Test-Path $injector)) { $injector = Join-Path $PSScriptRoot "windows-apo\build\Release\injector_single_slot.exe" }  # project folder
    if (-not (Test-Path $injector)) {
        Write-Error "Missing $injector. Run windows-apo\build_native_apo.bat first."
        exit 1
    }
    $failed = 0
    foreach ($name in $valueNames) {
        $val = Remove-Diviora $backupData.$name
        if ($val -is [array] -and $val.Count -eq 0) { $val = $null }
        if ($null -eq $val) {
            & $injector --write $EndpointGuid $name delete
        } elseif ($val -is [array]) {
            & $injector --write $EndpointGuid $name msz ($val -join '|')
        } elseif ($val -is [int] -or $val -is [long]) {
            & $injector --write $EndpointGuid $name dword $val
        } else {
            & $injector --write $EndpointGuid $name sz "$val"
        }
        if ($LASTEXITCODE -ne 0) { $failed++ }
    }
    if ($failed -gt 0) {
        Write-Error "$failed value(s) could not be restored."
        exit 1
    }
    Write-Host "=== Restore complete for $EndpointGuid ===" -ForegroundColor Green
}
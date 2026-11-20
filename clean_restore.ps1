Write-Host "=== Performing Factory Audio State Restoration ===" -ForegroundColor Cyan

# 1. Terminate audio graph and services
Stop-Service AudioEndpointBuilder -Force -ErrorAction SilentlyContinue
Stop-Service Audiosrv -Force -ErrorAction SilentlyContinue
Get-Process audiodg -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500

# 2. Strip every custom APO key from all render endpoints
$renderPath = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render"
$endpoints = Get-ChildItem -Path $renderPath

foreach ($ep in $endpoints) {
    $fxKey = "$($ep.PSPath)\FxProperties"
    if (Test-Path $fxKey) {
        $props = Get-ItemProperty -Path $fxKey
        foreach ($p in $props.PSObject.Properties) {
            if ($p.Name -like "*{D04E05A6*" -or $p.Name -like "*{d04e05a6*" -or $p.Name -like "*{d04e05a5*") {
                Remove-ItemProperty -Path $fxKey -Name $p.Name -ErrorAction SilentlyContinue
                Write-Host "  [-] Cleared slot: $($ep.PSChildName) -> $($p.Name)" -ForegroundColor Yellow
            }
        }
    }
}

# 3. Clean CLSID
Remove-Item -Path "HKLM:\SOFTWARE\Classes\CLSID\{D1B100A1-8888-4444-9999-000000000001}" -Recurse -Force -ErrorAction SilentlyContinue

# 4. Restart Core Audio Engine
Start-Service AudioEndpointBuilder
Start-Service Audiosrv
Start-Sleep -Seconds 1

Write-Host "=== System Audio Engine Restored to Factory OEM Driver State ===" -ForegroundColor Green
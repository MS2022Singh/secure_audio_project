Write-Host "=== Restoring Native Windows Audio Engine & OEM Driver State ===" -ForegroundColor Cyan

# 1. Stop Audio Engine
Stop-Service AudioEndpointBuilder -Force -ErrorAction SilentlyContinue
Stop-Service Audiosrv -Force -ErrorAction SilentlyContinue
Get-Process audiodg -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500

# 2. Clean all Diviora slots across every render endpoint
$renderPath = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render"
$endpoints = Get-ChildItem -Path $renderPath

$clsid = "{D1B100A1-8888-4444-9999-000000000001}"

foreach ($ep in $endpoints) {
    $fxKey = "$($ep.PSPath)\FxProperties"
    if (Test-Path $fxKey) {
        $props = Get-ItemProperty -Path $fxKey
        foreach ($propName in $props.PSObject.Properties.Name) {
            $val = $props.$propName
            if ($val -eq $clsid) {
                Remove-ItemProperty -Path $fxKey -Name $propName -ErrorAction SilentlyContinue
                Write-Host "  [-] Removed Diviora binding from: $($ep.PSChildName) -> $propName" -ForegroundColor Yellow
            }
        }
    }
}

# 3. Clean COM registration
Remove-Item -Path "HKLM:\SOFTWARE\Classes\CLSID\$clsid" -Recurse -Force -ErrorAction SilentlyContinue

# 4. Restart Services
Start-Service AudioEndpointBuilder
Start-Service Audiosrv
Start-Sleep -Seconds 1

Write-Host "=== Native Audio Fully Restored. Normal sound is now active. ===" -ForegroundColor Green
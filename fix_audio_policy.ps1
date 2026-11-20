Write-Host "=== Setting Windows Audio Development Policies & Chain Injection ===" -ForegroundColor Cyan

# 1. Enable Unsigned In-Process APO Loading in audiodg.exe
$audioRegPath = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Audio"
if (-not (Test-Path $audioRegPath)) { New-Item -Path $audioRegPath -Force | Out-Null }
Set-ItemProperty -Path $audioRegPath -Name "DisableProtectedAudioDG" -Value 1 -Type DWord -Force
Write-Host "[+] Set DisableProtectedAudioDG = 1" -ForegroundColor Green

# 2. Comprehensive Effect Slot Injection for All Active Render Endpoints
$clsid = "{D1B100A1-8888-4444-9999-000000000001}"
$renderPath = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render"
$endpoints = Get-ChildItem -Path $renderPath

foreach ($ep in $endpoints) {
    $fxKey = "$($ep.PSPath)\FxProperties"
    $propKey = "$($ep.PSPath)\Properties"

    if (Test-Path $propKey) {
        $devState = (Get-ItemProperty -Path $ep.PSPath -Name "DeviceState" -ErrorAction SilentlyContinue).DeviceState
        if ($devState -eq 1) {
            Write-Host "  -> Injecting Effect Slots into: $($ep.PSChildName)" -ForegroundColor Yellow
            if (-not (Test-Path $fxKey)) { New-Item -Path $fxKey -Force | Out-Null }

            # Windows 10/11 Stream/Mode/Endpoint Effect Properties
            Set-ItemProperty -Path $fxKey -Name "{d04e05a5-594b-4fb6-a80d-01af5eed7d1d},0" -Value $clsid -Type String -Force
            Set-ItemProperty -Path $fxKey -Name "{d04e05a5-594b-4fb6-a80d-01af5eed7d1d},5" -Value $clsid -Type String -Force
            Set-ItemProperty -Path $fxKey -Name "{d04e05a5-594b-4fb6-a80d-01af5eed7d1d},6" -Value $clsid -Type String -Force
            Set-ItemProperty -Path $fxKey -Name "{d04e05a5-594b-4fb6-a80d-01af5eed7d1d},7" -Value $clsid -Type String -Force

            # Legacy & Composite Properties
            Set-ItemProperty -Path $fxKey -Name "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},1" -Value $clsid -Type String -Force
            Set-ItemProperty -Path $fxKey -Name "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},2" -Value $clsid -Type String -Force
            Set-ItemProperty -Path $fxKey -Name "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},3" -Value $clsid -Type String -Force

            # Audio Enhancements Force Enabled
            Set-ItemProperty -Path $fxKey -Name "{b3f8fa53-0004-438e-9003-51a46e139bfc},2" -Value 0 -Type DWord -Force
        }
    }
}

# 3. Audio Engine Full Reset
Stop-Service AudioEndpointBuilder -Force -ErrorAction SilentlyContinue
Stop-Service Audiosrv -Force -ErrorAction SilentlyContinue
Get-Process audiodg -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500
Start-Service AudioEndpointBuilder
Start-Service Audiosrv

Write-Host "=== Environment & Policy Reset Complete ===" -ForegroundColor Green
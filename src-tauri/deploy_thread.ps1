$rootDir = "$env:USERPROFILE\secure_audio_project"
$deployDir = "C:\Program Files\Diviora"
if (-not (Test-Path $deployDir)) { New-Item -Path $deployDir -ItemType Directory -Force | Out-Null }
Copy-Item "$rootDir\windows-apo\build\Release\DivioraAPO.dll" "$deployDir\DivioraAPO.dll" -Force
icacls "$deployDir\DivioraAPO.dll" /grant "NT AUTHORITY\LOCAL SERVICE:(RX)" "ALL APPLICATION PACKAGES:(RX)" "Users:(RX)" /c | Out-Null

$audioReg = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Audio"
if (-not (Test-Path $audioReg)) { New-Item -Path $audioReg -Force | Out-Null }
Set-ItemProperty -Path $audioReg -Name "DisableProtectedAudioDG" -Value 1 -Type DWord -Force

$clsid = "{D1B100A1-8888-4444-9999-000000000001}"
$keyPath = "HKLM:\SOFTWARE\Classes\CLSID\$clsid"
$inprocPath = "$keyPath\InprocServer32"
$catPath = "$keyPath\Implemented Categories\{4391F069-42E0-449D-927A-92F6C7D1B78B}"

if (-not (Test-Path $keyPath)) { New-Item -Path $keyPath -Force | Out-Null }
if (-not (Test-Path $inprocPath)) { New-Item -Path $inprocPath -Force | Out-Null }
if (-not (Test-Path $catPath)) { New-Item -Path $catPath -Force | Out-Null }

Set-ItemProperty -Path $keyPath -Name "(default)" -Value "Diviora Endpoint APO" -Force
Set-ItemProperty -Path $inprocPath -Name "(default)" -Value "$deployDir\DivioraAPO.dll" -Force
Set-ItemProperty -Path $inprocPath -Name "ThreadingModel" -Value "Both" -Force

& "$rootDir\windows-installer\injector_safe.exe"

Stop-Service Audiosrv -Force -ErrorAction SilentlyContinue
Stop-Service AudioEndpointBuilder -Force -ErrorAction SilentlyContinue
Get-Process audiodg -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

Start-Service AudioEndpointBuilder
Start-Sleep -Seconds 1
Start-Service Audiosrv
Start-Sleep -Seconds 2

Write-Host "=== Diviora Direct APO Active in Graph ===" -ForegroundColor Green
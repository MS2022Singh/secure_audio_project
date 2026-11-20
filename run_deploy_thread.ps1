$rootDir = "$env:USERPROFILE\secure_audio_project"
$deployDir = "C:\Program Files\Diviora"
Copy-Item "$rootDir\windows-apo\build\Release\DivioraAPO.dll" "$deployDir\DivioraAPO.dll" -Force
icacls "$deployDir\DivioraAPO.dll" /grant "NT AUTHORITY\LOCAL SERVICE:(RX)" "ALL APPLICATION PACKAGES:(RX)" "Users:(RX)" /c | Out-Null

Stop-Service Audiosrv -Force -ErrorAction SilentlyContinue
Stop-Service AudioEndpointBuilder -Force -ErrorAction SilentlyContinue
Get-Process audiodg -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

Start-Service AudioEndpointBuilder
Start-Sleep -Seconds 1
Start-Service Audiosrv
Start-Sleep -Seconds 2

Write-Host "=== Diviora File-Watching APO Deployed & Active in Windows Audio Graph ===" -ForegroundColor Green
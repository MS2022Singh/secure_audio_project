$clsid = "{D1B100A1-8888-4444-9999-000000000001}"
$dllPath = "C:\Program Files\Diviora\DivioraAPO.dll"

Write-Host "=== Setting AudioSandbox Permissions & Catalog Keys ===" -ForegroundColor Cyan

# 1. Grant Full Read & Execute permissions to LOCAL SERVICE & ALL APPLICATION PACKAGES
& icacls "C:\Program Files\Diviora" /grant "*S-1-5-19:(OI)(CI)RX" /grant "*S-1-15-2-1:(OI)(CI)RX" /grant "*S-1-5-32-545:(OI)(CI)RX" /T /C /Q
& icacls $dllPath /grant "*S-1-5-19:RX" /grant "*S-1-15-2-1:RX" /grant "*S-1-5-32-545:RX" /C /Q
Write-Host "[+] Granted Sandbox ACLs on Diviora directory and DLL." -ForegroundColor Green

# 2. Register in AudioProcessingObjects Catalog (HKLM\SOFTWARE\Classes\AudioEngine\AudioProcessingObjects)
$catKeyPath = "HKLM:\SOFTWARE\Classes\AudioEngine\AudioProcessingObjects\$clsid"
if (-not (Test-Path $catKeyPath)) {
    New-Item -Path $catKeyPath -Force | Out-Null
}
Set-ItemProperty -Path $catKeyPath -Name "FriendlyName" -Value "Diviora Endpoint APO" -Type String -Force
Set-ItemProperty -Path $catKeyPath -Name "Flags" -Value 0x0000000f -Type DWord -Force
Set-ItemProperty -Path $catKeyPath -Name "MinInputConnections" -Value 1 -Type DWord -Force
Set-ItemProperty -Path $catKeyPath -Name "MaxInputConnections" -Value 1 -Type DWord -Force
Set-ItemProperty -Path $catKeyPath -Name "MinOutputConnections" -Value 1 -Type DWord -Force
Set-ItemProperty -Path $catKeyPath -Name "MaxOutputConnections" -Value 1 -Type DWord -Force
Set-ItemProperty -Path $catKeyPath -Name "MaxInstances" -Value 0xffffffff -Type DWord -Force
Set-ItemProperty -Path $catKeyPath -Name "NumAPOInterfaces" -Value 1 -Type DWord -Force
Set-ItemProperty -Path $catKeyPath -Name "APOInterface0" -Value "{65104E9C-338B-426C-B433-4D9088E09D3E}" -Type String -Force

Write-Host "[+] Registered in AudioProcessingObjects Catalog." -ForegroundColor Green

# 3. Clean Restart of Audio Stack
Stop-Service Audiosrv -Force -ErrorAction SilentlyContinue
Stop-Service AudioEndpointBuilder -Force -ErrorAction SilentlyContinue
Get-Process audiodg -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

Start-Service AudioEndpointBuilder
Start-Sleep -Milliseconds 500
Start-Service Audiosrv
Start-Sleep -Seconds 1

Write-Host "=== Audio Stack Reloaded ===" -ForegroundColor Green
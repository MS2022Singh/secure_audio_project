Write-Host "=== Elevated Diviora Build & Deployment Engine ===" -ForegroundColor Cyan

# 1. Stop background service to release locks
Get-Process DivioraControlService -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500

# 2. Compile DivioraAPO.dll
Set-Location "$PSScriptRoot\windows-apo"
cmd.exe /c "$pwd\build_dll.bat"
$dllSource = "$pwd\build\Release\DivioraAPO.dll"

if (-not (Test-Path $dllSource)) {
    Write-Error "CRITICAL: DivioraAPO.dll compilation failed."
    exit 1
}

# 3. Deploy to C:\Program Files\Diviora
$deployDir = "C:\Program Files\Diviora"
if (-not (Test-Path $deployDir)) { New-Item -Path $deployDir -ItemType Directory -Force | Out-Null }
Copy-Item $dllSource "$deployDir\DivioraAPO.dll" -Force

# Grant LOCAL SERVICE and ALL APPLICATION PACKAGES read/execute access
icacls "$deployDir\DivioraAPO.dll" /grant "NT AUTHORITY\LOCAL SERVICE:(RX)" "ALL APPLICATION PACKAGES:(RX)" /c | Out-Null

# 4. Explicit COM Registration
$clsid = "{D1B100A1-8888-4444-9999-000000000001}"
$keyPath = "HKLM:\SOFTWARE\Classes\CLSID\$clsid"
$inprocPath = "$keyPath\InprocServer32"

if (-not (Test-Path $keyPath)) { New-Item -Path $keyPath -Force | Out-Null }
if (-not (Test-Path $inprocPath)) { New-Item -Path $inprocPath -Force | Out-Null }

Set-ItemProperty -Path $keyPath -Name "(default)" -Value "Diviora System Audio Processing Object" -Force
Set-ItemProperty -Path $inprocPath -Name "(default)" -Value "$deployDir\DivioraAPO.dll" -Force
Set-ItemProperty -Path $inprocPath -Name "ThreadingModel" -Value "Both" -Force

# 5. Compile and Launch Control Service
Set-Location "$PSScriptRoot\windows-service"
$vcvars = Get-ChildItem -Path "${env:ProgramFiles}\Microsoft Visual Studio" -Recurse -Filter "vcvars64.bat" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $vcvars) {
    $vcvars = Get-ChildItem -Path "${env:ProgramFiles(x86)}\Microsoft Visual Studio" -Recurse -Filter "vcvars64.bat" -ErrorAction SilentlyContinue | Select-Object -First 1
}

$buildService = @"
call "$($vcvars.FullName)"
cl.exe /O2 /std:c++20 /EHsc /I..\windows-apo\include DivioraControlService.cpp /link /OUT:DivioraControlService.exe Advapi32.lib User32.lib
"@
[System.IO.File]::WriteAllText("$pwd\build_service.bat", $buildService, (New-Object System.Text.UTF8Encoding($false)))
cmd.exe /c "$pwd\build_service.bat"
Start-Process -FilePath "$pwd\DivioraControlService.exe" -WindowStyle Hidden

# 6. Bind Active Endpoint & Restart Audio Engine
Set-Location "$PSScriptRoot"
& "$PSScriptRoot\windows-installer\EndpointConfigurator.exe"
Restart-Service AudioEndpointBuilder -Force
Restart-Service Audiosrv -Force

Write-Host "=== Deployment & System Audio Reset Complete ===" -ForegroundColor Green
Write-Host "========================================================" -ForegroundColor Cyan
Write-Host "  Diviora System APO: Complete COM Pipeline Deployment   " -ForegroundColor Cyan
Write-Host "========================================================" -ForegroundColor Cyan

# 1. Stop background processes
Get-Process DivioraControlService -ErrorAction SilentlyContinue | Stop-Process -Force
Stop-Service AudioEndpointBuilder -Force -ErrorAction SilentlyContinue
Stop-Service Audiosrv -Force -ErrorAction SilentlyContinue
Get-Process audiodg -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500

# 2. Build DLL with Class Factory and Exports
Set-Location "$PSScriptRoot\windows-apo"
Write-Host "[1/6] Compiling DivioraAPO.dll with COM exports..." -ForegroundColor Yellow
cmd.exe /c "$pwd\build_dll.bat"
$dllSource = "$pwd\build\Release\DivioraAPO.dll"

if (-not (Test-Path $dllSource)) {
    Write-Error "CRITICAL: DivioraAPO.dll build failed."
    exit 1
}

# 3. Deploy to C:\Program Files\Diviora
Write-Host "[2/6] Deploying to C:\Program Files\Diviora..." -ForegroundColor Yellow
$deployDir = "C:\Program Files\Diviora"
if (-not (Test-Path $deployDir)) { New-Item -Path $deployDir -ItemType Directory -Force | Out-Null }
Copy-Item $dllSource "$deployDir\DivioraAPO.dll" -Force
icacls "$deployDir\DivioraAPO.dll" /grant "NT AUTHORITY\LOCAL SERVICE:(RX)" "ALL APPLICATION PACKAGES:(RX)" "Users:(RX)" /c | Out-Null

# 4. Explicit COM Registration in HKLM
Write-Host "[3/6] Registering COM Class in HKLM..." -ForegroundColor Yellow
$clsid = "{D1B100A1-8888-4444-9999-000000000001}"
$keyPath = "HKLM:\SOFTWARE\Classes\CLSID\$clsid"
$inprocPath = "$keyPath\InprocServer32"

if (-not (Test-Path $keyPath)) { New-Item -Path $keyPath -Force | Out-Null }
if (-not (Test-Path $inprocPath)) { New-Item -Path $inprocPath -Force | Out-Null }

Set-ItemProperty -Path $keyPath -Name "(default)" -Value "Diviora Endpoint APO" -Force
Set-ItemProperty -Path $inprocPath -Name "(default)" -Value "$deployDir\DivioraAPO.dll" -Force
Set-ItemProperty -Path $inprocPath -Name "ThreadingModel" -Value "Both" -Force

# 5. Build & Launch Control Service
Write-Host "[4/6] Building & Launching Control Service..." -ForegroundColor Yellow
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

# 6. Inject APO into all Active Render Endpoints
Write-Host "[5/6] Binding APO across all render endpoints..." -ForegroundColor Yellow
Set-Location "$PSScriptRoot\windows-installer"
cmd.exe /c "$pwd\build_configurator.bat"
& "$pwd\EndpointConfigurator.exe"

# 7. Restart Windows Audio Services
Write-Host "[6/6] Restarting Windows Audio Services..." -ForegroundColor Yellow
Start-Service AudioEndpointBuilder
Start-Service Audiosrv
Start-Sleep -Seconds 1

Write-Host "=== COM Pipeline Deployment & Audio Restart Complete ===" -ForegroundColor Green
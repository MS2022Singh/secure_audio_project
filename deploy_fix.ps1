Write-Host "========================================================" -ForegroundColor Cyan
Write-Host "  Diviora System APO: Build, Deploy & Multi-Endpoint    " -ForegroundColor Cyan
Write-Host "========================================================" -ForegroundColor Cyan

# 1. Stop background service & audio engine
Get-Process DivioraControlService -ErrorAction SilentlyContinue | Stop-Process -Force
Stop-Service AudioEndpointBuilder -Force -ErrorAction SilentlyContinue
Stop-Service Audiosrv -Force -ErrorAction SilentlyContinue
Get-Process audiodg -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500

# 2. Compile DivioraAPO.dll
Set-Location "$PSScriptRoot\windows-apo"
Write-Host "[1/6] Compiling DivioraAPO.dll..." -ForegroundColor Yellow
$vcvars = Get-ChildItem -Path "${env:ProgramFiles}\Microsoft Visual Studio" -Recurse -Filter "vcvars64.bat" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $vcvars) {
    $vcvars = Get-ChildItem -Path "${env:ProgramFiles(x86)}\Microsoft Visual Studio" -Recurse -Filter "vcvars64.bat" -ErrorAction SilentlyContinue | Select-Object -First 1
}

$cmd = @"
call "$($vcvars.FullName)"
if not exist "build\Release" mkdir "build\Release"
cl.exe /O2 /std:c++20 /EHsc /Iinclude src\DivioraAPO.cpp src\ClassFactory.cpp /link /DLL /DEF:DivioraAPO.def /OUT:build\Release\DivioraAPO.dll Ole32.lib Advapi32.lib User32.lib
"@
[System.IO.File]::WriteAllText("$pwd\build_dll.bat", $cmd, (New-Object System.Text.UTF8Encoding($false)))
cmd.exe /c "$pwd\build_dll.bat"

$dllSource = "$pwd\build\Release\DivioraAPO.dll"
if (-not (Test-Path $dllSource)) {
    Write-Error "CRITICAL: DivioraAPO.dll compilation failed. Review MSVC errors above."
    exit 1
}
Write-Host "  -> DivioraAPO.dll built successfully." -ForegroundColor Green

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

# 5. Compile and Launch Control Service
Write-Host "[4/6] Building & Starting DivioraControlService..." -ForegroundColor Yellow
Set-Location "$PSScriptRoot\windows-service"
$buildService = @"
call "$($vcvars.FullName)"
cl.exe /O2 /std:c++20 /EHsc /I..\windows-apo\include DivioraControlService.cpp /link /OUT:DivioraControlService.exe Advapi32.lib User32.lib
"@
[System.IO.File]::WriteAllText("$pwd\build_service.bat", $buildService, (New-Object System.Text.UTF8Encoding($false)))
cmd.exe /c "$pwd\build_service.bat"
Start-Process -FilePath "$pwd\DivioraControlService.exe" -WindowStyle Hidden

# 6. Bind All Active Render Endpoints
Write-Host "[5/6] Injecting APO across all render endpoints..." -ForegroundColor Yellow
Set-Location "$PSScriptRoot\windows-installer"
$buildConfig = @"
call "$($vcvars.FullName)"
cl.exe /O2 /std:c++20 /EHsc EndpointConfigurator.cpp /link /OUT:EndpointConfigurator.exe Ole32.lib Shell32.lib Advapi32.lib
"@
[System.IO.File]::WriteAllText("$pwd\build_configurator.bat", $buildConfig, (New-Object System.Text.UTF8Encoding($false)))
cmd.exe /c "$pwd\build_configurator.bat"
& "$pwd\EndpointConfigurator.exe"

# 7. Restart Audio Engine
Write-Host "[6/6] Restarting Windows Audio Engine..." -ForegroundColor Yellow
Start-Service AudioEndpointBuilder
Start-Service Audiosrv
Start-Sleep -Seconds 1

Write-Host "`n========================================================" -ForegroundColor Green
Write-Host "  DEPLOYMENT COMPLETE! Ready for live stream testing.   " -ForegroundColor Green
Write-Host "========================================================" -ForegroundColor Green
Write-Host "=== Deploying Safe Stream-Only Diviora APO ===" -ForegroundColor Cyan

# 1. Compile DLL & Injector
$vcvars = Get-ChildItem -Path "${env:ProgramFiles}\Microsoft Visual Studio" -Recurse -Filter "vcvars64.bat" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $vcvars) {
    $vcvars = Get-ChildItem -Path "${env:ProgramFiles(x86)}\Microsoft Visual Studio" -Recurse -Filter "vcvars64.bat" -ErrorAction SilentlyContinue | Select-Object -First 1
}

$buildBat = @"
call "$($vcvars.FullName)"
cd "$PSScriptRoot\windows-apo"
if not exist "build\Release" mkdir "build\Release"
cl.exe /O2 /std:c++20 /EHsc /DUNICODE /D_UNICODE /Iinclude src\DivioraAPO.cpp src\ClassFactory.cpp /link /DLL /DEF:DivioraAPO.def /OUT:build\Release\DivioraAPO.dll Ole32.lib Advapi32.lib User32.lib
cd "$PSScriptRoot\windows-installer"
cl.exe /EHsc /W3 /O2 /DUNICODE /D_UNICODE injector_single.cpp /link /OUT:injector_single.exe ole32.lib advapi32.lib
"@
[System.IO.File]::WriteAllText("$pwd\build_safe.bat", $buildBat, (New-Object System.Text.UTF8Encoding($false)))
cmd.exe /c "$pwd\build_safe.bat"

# 2. Deploy DLL
$deployDir = "C:\Program Files\Diviora"
if (-not (Test-Path $deployDir)) { New-Item -Path $deployDir -ItemType Directory -Force | Out-Null }
Copy-Item "$PSScriptRoot\windows-apo\build\Release\DivioraAPO.dll" "$deployDir\DivioraAPO.dll" -Force
icacls "$deployDir\DivioraAPO.dll" /grant "NT AUTHORITY\LOCAL SERVICE:(RX)" "ALL APPLICATION PACKAGES:(RX)" "Users:(RX)" /c | Out-Null

# 3. COM Registration
$clsid = "{D1B100A1-8888-4444-9999-000000000001}"
$keyPath = "HKLM:\SOFTWARE\Classes\CLSID\$clsid"
$inprocPath = "$keyPath\InprocServer32"
if (-not (Test-Path $keyPath)) { New-Item -Path $keyPath -Force | Out-Null }
if (-not (Test-Path $inprocPath)) { New-Item -Path $inprocPath -Force | Out-Null }
Set-ItemProperty -Path $keyPath -Name "(default)" -Value "Diviora Endpoint APO" -Force
Set-ItemProperty -Path $inprocPath -Name "(default)" -Value "$deployDir\DivioraAPO.dll" -Force
Set-ItemProperty -Path $inprocPath -Name "ThreadingModel" -Value "Both" -Force

# 4. Inject Stream Effect Only
& "$PSScriptRoot\windows-installer\injector_single.exe"

# 5. Restart Audio Subsystem
Restart-Service AudioEndpointBuilder -Force
Restart-Service Audiosrv -Force

Write-Host "=== Safe Stream Injection Complete. Audiodg Graph Active. ===" -ForegroundColor Green
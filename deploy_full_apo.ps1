Write-Host "=== Deploying Complete Windows APO Registration ===" -ForegroundColor Cyan

# 1. Stop background service to release locks
Get-Process DivioraControlService -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500

# 2. Compile DivioraAPO.dll
Set-Location "$PSScriptRoot\windows-apo"
$vcvars = Get-ChildItem -Path "${env:ProgramFiles}\Microsoft Visual Studio" -Recurse -Filter "vcvars64.bat" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $vcvars) {
    $vcvars = Get-ChildItem -Path "${env:ProgramFiles(x86)}\Microsoft Visual Studio" -Recurse -Filter "vcvars64.bat" -ErrorAction SilentlyContinue | Select-Object -First 1
}

$buildBat = @"
call "$($vcvars.FullName)"
if not exist "build\Release" mkdir "build\Release"
cl.exe /O2 /std:c++20 /EHsc /DUNICODE /D_UNICODE /Iinclude src\DivioraAPO.cpp src\ClassFactory.cpp /link /DLL /DEF:DivioraAPO.def /OUT:build\Release\DivioraAPO.dll Ole32.lib Advapi32.lib User32.lib
"@
[System.IO.File]::WriteAllText("$pwd\build_dll.bat", $buildBat, (New-Object System.Text.UTF8Encoding($false)))
cmd.exe /c "$pwd\build_dll.bat"

$dllSource = "$pwd\build\Release\DivioraAPO.dll"
if (-not (Test-Path $dllSource)) {
    Write-Error "DivioraAPO.dll compilation failed."
    exit 1
}

# 3. Deploy to Program Files & Set Access Rights
$deployDir = "C:\Program Files\Diviora"
if (-not (Test-Path $deployDir)) { New-Item -Path $deployDir -ItemType Directory -Force | Out-Null }
Copy-Item $dllSource "$deployDir\DivioraAPO.dll" -Force
icacls "$deployDir\DivioraAPO.dll" /grant "NT AUTHORITY\LOCAL SERVICE:(RX)" "ALL APPLICATION PACKAGES:(RX)" "Users:(RX)" /c | Out-Null

# 4. Complete COM + AudioEngine Subkey Registration
$clsid = "{D1B100A1-8888-4444-9999-000000000001}"
$keyPath = "HKLM:\SOFTWARE\Classes\CLSID\$clsid"
$inprocPath = "$keyPath\InprocServer32"
$apoSubKey = "$keyPath\AudioEngine\AudioProcessingObjects"

if (-not (Test-Path $keyPath)) { New-Item -Path $keyPath -Force | Out-Null }
if (-not (Test-Path $inprocPath)) { New-Item -Path $inprocPath -Force | Out-Null }
if (-not (Test-Path $apoSubKey)) { New-Item -Path $apoSubKey -Force | Out-Null }

Set-ItemProperty -Path $keyPath -Name "(default)" -Value "Diviora Endpoint APO" -Force
Set-ItemProperty -Path $inprocPath -Name "(default)" -Value "$deployDir\DivioraAPO.dll" -Force
Set-ItemProperty -Path $inprocPath -Name "ThreadingModel" -Value "Both" -Force

# Register APO Properties flag (In-Place + Match Flags = 0x0000000E)
Set-ItemProperty -Path $apoSubKey -Name "APOEndpointEffect" -Value 1 -Type DWord -Force
Set-ItemProperty -Path $apoSubKey -Name "APOSystemEffect" -Value 1 -Type DWord -Force

# 5. Bind Mode Effect Slot (,6) on Speakers
Set-Location "$PSScriptRoot"
& "$PSScriptRoot\windows-installer\injector_single.exe"

# 6. Restart Audio Subsystem
Restart-Service AudioEndpointBuilder -Force
Restart-Service Audiosrv -Force
Write-Host "=== Complete Registration & Binding Active ===" -ForegroundColor Green
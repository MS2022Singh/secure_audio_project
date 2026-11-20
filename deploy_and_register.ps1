# Requires Administrator
Set-Location "$PSScriptRoot\windows-apo"

# 1. Compile DivioraAPO.dll
cmd.exe /c "$pwd\build_dll.bat"
if (-not (Test-Path "$pwd\build\Release\DivioraAPO.dll")) {
    Write-Error "DLL Build failed."
    exit 1
}

# 2. Deploy to C:\Program Files\Diviora
$deployDir = "C:\Program Files\Diviora"
if (-not (Test-Path $deployDir)) { New-Item -Path $deployDir -ItemType Directory -Force | Out-Null }
Copy-Item "$pwd\build\Release\DivioraAPO.dll" "$deployDir\DivioraAPO.dll" -Force

# Grant LOCAL SERVICE and ALL APPLICATION PACKAGES read/execute access
icacls "$deployDir\DivioraAPO.dll" /grant "NT AUTHORITY\LOCAL SERVICE:(RX)" "ALL APPLICATION PACKAGES:(RX)" /c | Out-Null

# 3. Explicit COM InprocServer32 Registration
$clsid = "{D1B100A1-8888-4444-9999-000000000001}"
$keyPath = "HKLM:\SOFTWARE\Classes\CLSID\$clsid"
$inprocPath = "$keyPath\InprocServer32"

if (-not (Test-Path $keyPath)) { New-Item -Path $keyPath -Force | Out-Null }
if (-not (Test-Path $inprocPath)) { New-Item -Path $inprocPath -Force | Out-Null }

Set-ItemProperty -Path $keyPath -Name "(default)" -Value "Diviora System Audio Processing Object" -Force
Set-ItemProperty -Path $inprocPath -Name "(default)" -Value "$deployDir\DivioraAPO.dll" -Force
Set-ItemProperty -Path $inprocPath -Name "ThreadingModel" -Value "Both" -Force

Write-Host "DivioraAPO.dll successfully compiled, deployed, and registered in COM." -ForegroundColor Green
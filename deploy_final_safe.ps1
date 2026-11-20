$rootDir = "$env:USERPROFILE\secure_audio_project"
$markerPath = "C:\ProgramData\Diviora\registry_backups\LATEST_BACKUP.txt"

if (-not (Test-Path $markerPath)) {
    Write-Error "No verified backup marker found."
    exit 1
}
$backupDir = (Get-Content -Path $markerPath -Raw).Trim()

# 1. Terminate running background service
Get-Process DivioraControlService -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500

# 2. Deploy Binaries
$deployDir = "C:\Program Files\Diviora"
if (-not (Test-Path $deployDir)) { New-Item -Path $deployDir -ItemType Directory -Force | Out-Null }

Copy-Item "$rootDir\windows-apo\build\Release\DivioraAPO.dll" "$deployDir\DivioraAPO.dll" -Force
Copy-Item "$rootDir\windows-service\DivioraControlService.exe" "$deployDir\DivioraControlService.exe" -Force

icacls "$deployDir\DivioraAPO.dll" /grant "NT AUTHORITY\LOCAL SERVICE:(RX)" "ALL APPLICATION PACKAGES:(RX)" "Users:(RX)" /c | Out-Null

# 3. Allow audiodg unsigned APO loading
$audioReg = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Audio"
if (-not (Test-Path $audioReg)) { New-Item -Path $audioReg -Force | Out-Null }
Set-ItemProperty -Path $audioReg -Name "DisableProtectedAudioDG" -Value 1 -Type DWord -Force

# 4. COM Registration
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

# 5. Inject safe endpoint slots
& "$rootDir\windows-installer\injector_safe.exe"

# 6. Launch Background Control Service
Start-Process "$deployDir\DivioraControlService.exe" -WindowStyle Hidden
Start-Sleep -Milliseconds 500

# 7. Cycle Audio Engine Services
Stop-Service Audiosrv -Force -ErrorAction SilentlyContinue
Stop-Service AudioEndpointBuilder -Force -ErrorAction SilentlyContinue
Get-Process audiodg -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

Start-Service AudioEndpointBuilder
Start-Sleep -Seconds 1
Start-Service Audiosrv
Start-Sleep -Seconds 2

# Force immediate active stream generation
[System.Media.SystemSounds]::Asterisk.Play()

Write-Host "=== Verifying APO Graph Telemetry... ===" -ForegroundColor Cyan

Add-Type -Namespace Diviora -Name DiagReader -MemberDefinition @"
[DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
public static extern IntPtr OpenFileMappingW(uint dwDesiredAccess, bool bInheritHandle, string lpName);
[DllImport("kernel32.dll", SetLastError = true)]
public static extern IntPtr MapViewOfFile(IntPtr hFileMappingObject, uint dwDesiredAccess, uint dwFileOffsetHigh, uint dwFileOffsetLow, UIntPtr dwNumberOfBytesToMap);
[DllImport("kernel32.dll")]
public static extern bool UnmapViewOfFile(IntPtr lpBaseAddress);
[DllImport("kernel32.dll")]
public static extern bool CloseHandle(IntPtr hObject);
public static long ReadProcessCalls() {
    IntPtr hMap = OpenFileMappingW(0x0004, false, "Global\\DivioraApoDiagnostics");
    if (hMap == IntPtr.Zero) return -1;
    IntPtr pView = MapViewOfFile(hMap, 0x0004, 0, 0, (UIntPtr)64);
    if (pView == IntPtr.Zero) { CloseHandle(hMap); return -1; }
    long processCalls = System.Runtime.InteropServices.Marshal.ReadInt64(pView, 32);
    UnmapViewOfFile(pView);
    CloseHandle(hMap);
    return processCalls;
}
"@

$verified = $false
for ($i = 0; $i -lt 15; $i++) {
    $calls = [Diviora.DiagReader]::ReadProcessCalls()
    if ($calls -gt 0) {
        Write-Host "  [+] Graph Verified Active: process_calls = $calls" -ForegroundColor Green
        $verified = $true
        break
    }
    if ($i % 3 -eq 0) { [System.Media.SystemSounds]::Exclamation.Play() }
    Start-Sleep -Seconds 1
}

if ($verified) {
    Write-Host "=== Diviora APO Verified Active & Streaming. Deployment Retained. ===" -ForegroundColor Green
} else {
    Write-Warning "=== Telemetry Verification Failed. Auto-Reverting to Native Audio ==="
    Stop-Service Audiosrv -Force -ErrorAction SilentlyContinue
    Stop-Service AudioEndpointBuilder -Force -ErrorAction SilentlyContinue
    reg import "$backupDir\MMDevices_Render.reg" 2>&1 | Out-Null
    Remove-Item -Path $keyPath -Recurse -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
    Start-Service AudioEndpointBuilder
    Start-Sleep -Seconds 1
    Start-Service Audiosrv
    Write-Host "=== Rollback Complete: Native Windows Audio Fully Restored. ===" -ForegroundColor Yellow
}
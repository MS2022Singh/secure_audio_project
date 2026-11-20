$rootDir = "$env:USERPROFILE\secure_audio_project"
$markerPath = "C:\ProgramData\Diviora\registry_backups\LATEST_BACKUP.txt"

if (-not (Test-Path $markerPath)) {
    Write-Error "No registry backup marker found."
    exit 1
}
$backupDir = (Get-Content -Path $markerPath -Raw).Trim()

# 1. Deploy DLL
$deployDir = "C:\Program Files\Diviora"
if (-not (Test-Path $deployDir)) { New-Item -Path $deployDir -ItemType Directory -Force | Out-Null }
Copy-Item "$rootDir\windows-apo\build\Release\DivioraAPO.dll" "$deployDir\DivioraAPO.dll" -Force
icacls "$deployDir\DivioraAPO.dll" /grant "NT AUTHORITY\LOCAL SERVICE:(RX)" "ALL APPLICATION PACKAGES:(RX)" "Users:(RX)" /c | Out-Null

# 2. COM Registration
$clsid = "{D1B100A1-8888-4444-9999-000000000001}"
$keyPath = "HKLM:\SOFTWARE\Classes\CLSID\$clsid"
$inprocPath = "$keyPath\InprocServer32"
if (-not (Test-Path $keyPath)) { New-Item -Path $keyPath -Force | Out-Null }
if (-not (Test-Path $inprocPath)) { New-Item -Path $inprocPath -Force | Out-Null }
Set-ItemProperty -Path $keyPath -Name "(default)" -Value "Diviora Endpoint APO" -Force
Set-ItemProperty -Path $inprocPath -Name "(default)" -Value "$deployDir\DivioraAPO.dll" -Force
Set-ItemProperty -Path $inprocPath -Name "ThreadingModel" -Value "Both" -Force

# 3. Inject Mode Slot
& "$rootDir\windows-installer\injector_single.exe"

# 4. Sequential Service Restart (No race conditions)
Write-Host "Restarting Audio Subsystem..." -ForegroundColor Cyan
Stop-Service Audiosrv -Force -ErrorAction SilentlyContinue
Stop-Service AudioEndpointBuilder -Force -ErrorAction SilentlyContinue
Get-Process audiodg -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

Start-Service AudioEndpointBuilder
Start-Sleep -Seconds 1
Start-Service Audiosrv
Start-Sleep -Seconds 2

Write-Host "=== Deployment complete. Verifying graph telemetry... ===" -ForegroundColor Cyan

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
        Write-Host "  [+] Graph verified active: process_calls = $calls" -ForegroundColor Green
        $verified = $true
        break
    }
    Start-Sleep -Seconds 1
}

if ($verified) {
    Write-Host "=== Diviora APO verified active and streaming. Deployment stays in place. ===" -ForegroundColor Green
} else {
    Write-Warning "=== Verification FAILED (process_calls stayed 0 for 15s). Auto-rolling back. ==="
    reg import "$backupDir\MMDevices_Render.reg" 2>&1 | Out-Null
    if (Test-Path "$backupDir\Diviora_CLSID.reg") {
        reg import "$backupDir\Diviora_CLSID.reg" 2>&1 | Out-Null
    } else {
        Remove-Item -Path $keyPath -Recurse -Force -ErrorAction SilentlyContinue
    }
    Stop-Service Audiosrv -Force -ErrorAction SilentlyContinue
    Stop-Service AudioEndpointBuilder -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
    Start-Service AudioEndpointBuilder
    Start-Sleep -Seconds 1
    Start-Service Audiosrv
    Write-Host "=== Rollback complete. Native Windows audio is in control. ===" -ForegroundColor Yellow
}
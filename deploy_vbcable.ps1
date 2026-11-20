$rootDir = "$env:USERPROFILE\secure_audio_project"

Write-Host "=== Injecting into VB-Cable (CABLE Input) ===" -ForegroundColor Cyan
$injectOutput = & "$rootDir\windows-installer\injector_vbcable.exe" "CABLE Input" 2>&1
$injectOutput | ForEach-Object { Write-Host $_ }
$injectExitCode = $LASTEXITCODE

if ($injectExitCode -ne 0) {
    Write-Error "Injector failed to find/bind VB-Cable endpoint. No changes made. Native audio untouched."
    exit 1
}

$guidLine = $injectOutput | Where-Object { $_ -match "Endpoint GUID \(save this\): (\{.+\})" }
if ($guidLine -match "Endpoint GUID \(save this\): (\{.+\})") {
    $vbCableGuid = $Matches[1]
} else {
    Write-Error "Could not parse endpoint GUID from injector output. Aborting."
    exit 1
}

Write-Host "=== Target endpoint: $vbCableGuid ===" -ForegroundColor Cyan

# Surgical backup of target endpoint
& "$rootDir\backup_restore_endpoint_surgical.ps1" -EndpointGuid $vbCableGuid -Mode backup

$deployDir = "C:\Program Files\Diviora"
$clsid = "{D1B100A1-8888-4444-9999-000000000001}"
$keyPath = "HKLM:\SOFTWARE\Classes\CLSID\$clsid"
$inprocPath = "$keyPath\InprocServer32"
if (-not (Test-Path $keyPath)) { New-Item -Path $keyPath -Force | Out-Null }
if (-not (Test-Path $inprocPath)) { New-Item -Path $inprocPath -Force | Out-Null }
Set-ItemProperty -Path $keyPath -Name "(default)" -Value "Diviora Endpoint APO" -Force
Set-ItemProperty -Path $inprocPath -Name "(default)" -Value "$deployDir\DivioraAPO.dll" -Force
Set-ItemProperty -Path $inprocPath -Name "ThreadingModel" -Value "Both" -Force

# Restart audio engine
Stop-Service Audiosrv -Force -ErrorAction SilentlyContinue
Stop-Service AudioEndpointBuilder -Force -ErrorAction SilentlyContinue
Get-Process audiodg -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1
Start-Service AudioEndpointBuilder
Start-Sleep -Seconds 1
Start-Service Audiosrv
Start-Sleep -Seconds 2

if (-not (Get-Process DivioraControlService -ErrorAction SilentlyContinue)) {
    Start-Process "$deployDir\DivioraControlService.exe" -WindowStyle Hidden
    Start-Sleep -Seconds 1
}

Write-Host ""
Write-Host "=== Route audio to CABLE Input now (set as default output in Windows Sound Settings," -ForegroundColor Yellow
Write-Host "    or route a specific app to it), then start playback. ===" -ForegroundColor Yellow
Write-Host ""
Read-Host "Press Enter once audio is playing through CABLE Input..."

Write-Host "=== Verifying APO telemetry on VB-Cable (15s window)... ===" -ForegroundColor Cyan

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
    long v = System.Runtime.InteropServices.Marshal.ReadInt64(pView, 32);
    UnmapViewOfFile(pView);
    CloseHandle(hMap);
    return v;
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
    Start-Sleep -Seconds 1
}

if ($verified) {
    Write-Host "=== Diviora verified active on VB-Cable. ===" -ForegroundColor Green
} else {
    Write-Warning "=== Not verified. Rolling back (surgical, VB-Cable only - native Speakers untouched). ==="
    & "$rootDir\backup_restore_endpoint_surgical.ps1" -EndpointGuid $vbCableGuid -Mode restore
    Remove-Item -Path $keyPath -Recurse -Force -ErrorAction SilentlyContinue
    Stop-Service Audiosrv -Force -ErrorAction SilentlyContinue
    Stop-Service AudioEndpointBuilder -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
    Start-Service AudioEndpointBuilder
    Start-Sleep -Seconds 1
    Start-Service Audiosrv
    Write-Host "=== Rolled back. ===" -ForegroundColor Yellow
}
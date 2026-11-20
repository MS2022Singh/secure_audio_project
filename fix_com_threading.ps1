$clsid = "{D1B100A1-8888-4444-9999-000000000001}"
$dllPath = "C:\Program Files\Diviora\DivioraAPO.dll"

Write-Host "=== Registering Proper COM ThreadingModel for audiodg.exe ===" -ForegroundColor Cyan

# Ensure HKCR\CLSID\{GUID}\InprocServer32 has ThreadingModel = Both
$clsidPath = "HKLM:\SOFTWARE\Classes\CLSID\$clsid"
$inprocPath = "$clsidPath\InprocServer32"

if (Test-Path $inprocPath) {
    Set-ItemProperty -Path $inprocPath -Name "ThreadingModel" -Value "Both" -Type String -Force
    Set-ItemProperty -Path $inprocPath -Name "(default)" -Value $dllPath -Type String -Force
    Write-Host "[+] Set InprocServer32 ThreadingModel to Both." -ForegroundColor Green
} else {
    Write-Host "[-] CLSID InprocServer32 path not found." -ForegroundColor Red
}

# Ensure AudioProcessingObjects Catalog entry is correct
$catPath = "HKLM:\SOFTWARE\Classes\AudioEngine\AudioProcessingObjects\$clsid"
if (Test-Path $catPath) {
    Set-ItemProperty -Path $catPath -Name "FriendlyName" -Value "Diviora Endpoint APO" -Type String -Force
    Set-ItemProperty -Path $catPath -Name "Flags" -Value 0xf -Type DWord -Force
    Write-Host "[+] Verified AudioProcessingObjects catalog entry." -ForegroundColor Green
}

# Restart audio engine
Stop-Service Audiosrv -Force -ErrorAction SilentlyContinue
Stop-Service AudioEndpointBuilder -Force -ErrorAction SilentlyContinue
Get-Process audiodg -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

Start-Service AudioEndpointBuilder
Start-Sleep -Milliseconds 500
Start-Service Audiosrv
Start-Sleep -Seconds 1

Write-Host "=== Audio Engine Reloaded with Correct COM Threading Model ===" -ForegroundColor Green
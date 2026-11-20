$epGuid = "{74e3dfaf-ea45-49ef-a4c7-eaef0bbbdc51}"
$divioraClsid = "{D1B100A1-8888-4444-9999-000000000001}"
$fxKeyPath = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render\$epGuid\FxProperties"

Write-Host "=== Applying Equalizer APO-Style Multi-Mode Registration ===" -ForegroundColor Cyan

# 1. Open FxProperties with write access
$regKey = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($fxKeyPath.Replace("HKLM:\", ""), $true)
if (-not $regKey) {
    Write-Host "[-] Could not open FxProperties for $epGuid" -ForegroundColor Red
    return
}

# The 4 DCH Mode Processing Keys
$modeKeys = @(
    "{d3993a3f-99c2-4402-b5ec-a92a0367664b},5",
    "{d3993a3f-99c2-4402-b5ec-a92a0367664b},6",
    "{d3993a3f-99c2-4402-b5ec-a92a0367664b},11",
    "{d3993a3f-99c2-4402-b5ec-a92a0367664b},12"
)

# Replace all mode slots with Diviora CLSID as the single primary processor
foreach ($keyName in $modeKeys) {
    $arr = [string[]]@($divioraClsid)
    $regKey.SetValue($keyName, $arr, [Microsoft.Win32.RegistryValueKind]::MultiString)
    Write-Host "  [+] Set $keyName -> $divioraClsid" -ForegroundColor Green
}

# Legacy Fallback Keys
$regKey.SetValue("{D04E05A6-594B-4fb6-A80D-01AF5EED7D1D},5", $divioraClsid, [Microsoft.Win32.RegistryValueKind]::String)
$regKey.SetValue("{D04E05A6-594B-4fb6-A80D-01AF5EED7D1D},6", $divioraClsid, [Microsoft.Win32.RegistryValueKind]::String)

$regKey.Close()
Write-Host "=== Multi-Mode Registration Applied Successfully ===" -ForegroundColor Green

# 2. Cycle Windows Audio Service cleanly
Stop-Service Audiosrv -Force -ErrorAction SilentlyContinue
Stop-Service AudioEndpointBuilder -Force -ErrorAction SilentlyContinue
Get-Process audiodg -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

Start-Service AudioEndpointBuilder
Start-Sleep -Milliseconds 500
Start-Service Audiosrv
Start-Sleep -Seconds 1

Write-Host "=== Audio Engine Restarted ===" -ForegroundColor Green
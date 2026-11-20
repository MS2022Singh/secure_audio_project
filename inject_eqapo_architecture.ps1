$epGuid = "{74e3dfaf-ea45-49ef-a4c7-eaef0bbbdc51}"
$clsid = "{D1B100A1-8888-4444-9999-000000000001}"
$fxPath = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render\$epGuid\FxProperties"
$propPath = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render\$epGuid\Properties"

Write-Host "=== Applying Equalizer APO Full Pin Architecture Keys ===" -ForegroundColor Cyan

# 1. Open FxProperties
$fxKey = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($fxPath.Replace("HKLM:\", ""), $true)
if ($fxKey) {
    # Stream Effect (SFX)
    $fxKey.SetValue("{D04E05A6-594B-4fb6-A80D-01AF5EED7D1D},5", $clsid, [Microsoft.Win32.RegistryValueKind]::String)
    # Mode Effect (MFX)
    $fxKey.SetValue("{D04E05A6-594B-4fb6-A80D-01AF5EED7D1D},6", $clsid, [Microsoft.Win32.RegistryValueKind]::String)
    # Endpoint Effect (EFX)
    $fxKey.SetValue("{D04E05A6-594B-4fb6-A80D-01AF5EED7D1D},1", $clsid, [Microsoft.Win32.RegistryValueKind]::String)
    
    # Modern MultiString entries (Windows 10/11 DCH architecture)
    $arr = [string[]]@($clsid)
    $fxKey.SetValue("{d3993a3f-99c2-4402-b5ec-a92a0367664b},5", $arr, [Microsoft.Win32.RegistryValueKind]::MultiString)
    $fxKey.SetValue("{d3993a3f-99c2-4402-b5ec-a92a0367664b},6", $arr, [Microsoft.Win32.RegistryValueKind]::MultiString)
    $fxKey.SetValue("{d3993a3f-99c2-4402-b5ec-a92a0367664b},7", $arr, [Microsoft.Win32.RegistryValueKind]::MultiString)
    $fxKey.SetValue("{d3993a3f-99c2-4402-b5ec-a92a0367664b},11", $arr, [Microsoft.Win32.RegistryValueKind]::MultiString)
    $fxKey.SetValue("{d3993a3f-99c2-4402-b5ec-a92a0367664b},12", $arr, [Microsoft.Win32.RegistryValueKind]::MultiString)
    $fxKey.SetValue("{d3993a3f-99c2-4402-b5ec-a92a0367664b},13", $arr, [Microsoft.Win32.RegistryValueKind]::MultiString)
    
    $fxKey.Close()
    Write-Host "[+] All FxProperties keys assigned to Diviora CLSID." -ForegroundColor Green
}

# 2. Open Properties key and ensure System Effects are enabled
$propKey = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($propPath.Replace("HKLM:\", ""), $true)
if ($propKey) {
    # PKEY_AudioEndpoint_Ext_Disable_SysFx = 0 (Enhancements ON)
    $propKey.SetValue("{A45C254E-DF1C-4EFD-8020-67D146A850E0},5", 0, [Microsoft.Win32.RegistryValueKind]::DWord)
    $propKey.Close()
    Write-Host "[+] Ensured Endpoint System Effects are explicitly enabled." -ForegroundColor Green
}

# 3. Clean Restart of Audio Engine
Stop-Service Audiosrv -Force -ErrorAction SilentlyContinue
Stop-Service AudioEndpointBuilder -Force -ErrorAction SilentlyContinue
Get-Process audiodg -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

Start-Service AudioEndpointBuilder
Start-Sleep -Milliseconds 500
Start-Service Audiosrv
Start-Sleep -Seconds 1

Write-Host "=== Audio Engine Restarted ===" -ForegroundColor Green
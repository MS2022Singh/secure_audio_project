$clsid = "{D1B100A1-8888-4444-9999-000000000001}"
$epGuid = "{74e3dfaf-ea45-49ef-a4c7-eaef0bbbdc51}"

Write-Host "=== Configuring Windows Audio Engine for Unsigned Custom APOs ===" -ForegroundColor Cyan

# 1. Enable Unsigned / 3rd Party APO loading in audiodg.exe
$audioEngineKey = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Audio"
if (-not (Test-Path $audioEngineKey)) { New-Item -Path $audioEngineKey -Force | Out-Null }
Set-ItemProperty -Path $audioEngineKey -Name "DisableProtectedAudioDG" -Value 1 -Type DWord -Force
Write-Host "[+] Set DisableProtectedAudioDG = 1" -ForegroundColor Green

# 2. Register Diviora under MMDevices Render Endpoint Properties & FxProperties
$fxKeyPath = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render\$epGuid\FxProperties"
$regKey = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($fxKeyPath.Replace("HKLM:\", ""), $true)
if ($regKey) {
    # Set Composite Mode & Stream Effects
    $arr = [string[]]@($clsid)
    $regKey.SetValue("{d3993a3f-99c2-4402-b5ec-a92a0367664b},5", $arr, [Microsoft.Win32.RegistryValueKind]::MultiString)
    $regKey.SetValue("{d3993a3f-99c2-4402-b5ec-a92a0367664b},6", $arr, [Microsoft.Win32.RegistryValueKind]::MultiString)
    $regKey.SetValue("{d3993a3f-99c2-4402-b5ec-a92a0367664b},11", $arr, [Microsoft.Win32.RegistryValueKind]::MultiString)
    $regKey.SetValue("{d3993a3f-99c2-4402-b5ec-a92a0367664b},12", $arr, [Microsoft.Win32.RegistryValueKind]::MultiString)

    # Set Legacy LFX / GFX slots as fallback
    $regKey.SetValue("{D04E05A6-594B-4fb6-A80D-01AF5EED7D1D},5", $clsid, [Microsoft.Win32.RegistryValueKind]::String)
    $regKey.SetValue("{D04E05A6-594B-4fb6-A80D-01AF5EED7D1D},6", $clsid, [Microsoft.Win32.RegistryValueKind]::String)
    $regKey.Close()
    Write-Host "[+] Updated MMDevices FxProperties for $epGuid" -ForegroundColor Green
}

# 3. Check and apply to DeviceClasses KSCATEGORY_AUDIO
$devClassKey = "HKLM:\SYSTEM\CurrentControlSet\Control\DeviceClasses\{6994ad04-93ef-11d0-a3cc-00a0c9223196}"
if (Test-Path $devClassKey) {
    Get-ChildItem -Path $devClassKey -Recurse -ErrorAction SilentlyContinue | Where-Object { $_.PSChildName -eq "FxProperties" } | ForEach-Object {
        $sub = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($_.PSPath.Replace("HKLM:\", ""), $true)
        if ($sub) {
            $sub.SetValue("{d3993a3f-99c2-4402-b5ec-a92a0367664b},5", [string[]]@($clsid), [Microsoft.Win32.RegistryValueKind]::MultiString)
            $sub.SetValue("{d3993a3f-99c2-4402-b5ec-a92a0367664b},6", [string[]]@($clsid), [Microsoft.Win32.RegistryValueKind]::MultiString)
            $sub.Close()
            Write-Host "  [+] Hooked KSCATEGORY_AUDIO FxProperties: $($_.PSPath)" -ForegroundColor Green
        }
    }
}

# 4. Restart Windows Audio Graph cleanly
Stop-Service Audiosrv -Force -ErrorAction SilentlyContinue
Stop-Service AudioEndpointBuilder -Force -ErrorAction SilentlyContinue
Get-Process audiodg -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

Start-Service AudioEndpointBuilder
Start-Sleep -Milliseconds 500
Start-Service Audiosrv
Start-Sleep -Seconds 1

Write-Host "=== Audio Pipeline Reloaded with Protection Policy Disabled ===" -ForegroundColor Green
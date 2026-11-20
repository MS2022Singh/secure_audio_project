$divioraClsid = "{D1B100A1-8888-4444-9999-000000000001}"
$baseClass = "HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e96c-e325-11ce-bfc1-08002be10318}\0000\DeviceInterfaces"

# Target the active speaker pin
$pin = "eSpeaker\DeviceParameters\MSFX\0"
$fullPath = "$baseClass\$pin"

if (Test-Path $fullPath) {
    # 1. Read the existing DCH Multi-String Array
    $regKey = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($fullPath.Replace("HKLM:\", ""), $true)
    if ($regKey) {
        $existing = $regKey.GetValue("{D3993A3F-99C2-4402-B5EC-A92A0367664B},5")
        
        if ($existing -and $existing.Length -gt 0) {
            $originalApo = $existing[0]
            
            # Skip if Diviora is already primary
            if ($originalApo -eq $divioraClsid) {
                Write-Host "[i] Diviora is already the primary APO on this pin." -ForegroundColor Yellow
            } else {
                # 2. Store the Original APO as a "Child APO" for Diviora to load later
                $childKeyPath = "$fullPath\DivioraChildApo"
                [Microsoft.Win32.Registry]::LocalMachine.CreateSubKey($childKeyPath.Replace("HKLM:\", "")).SetValue("OriginalStreamApo", $originalApo)
                
                # 3. Replace the active array with Diviora ONLY
                $list = [System.Collections.Generic.List[string]]::new()
                $list.Add($divioraClsid)
                
                $regKey.SetValue("{D3993A3F-99C2-4402-B5EC-A92A0367664B},5", $list.ToArray(), [Microsoft.Win32.RegistryValueKind]::MultiString)
                $regKey.SetValue("{D04E05A6-594B-4fb6-A80D-01AF5EED7D1D},5", $divioraClsid, [Microsoft.Win32.RegistryValueKind]::String)
                
                Write-Host "[+] Equalizer APO Proxy Injection Successful." -ForegroundColor Green
                Write-Host "    Primary APO: $divioraClsid" -ForegroundColor Cyan
                Write-Host "    Child APO stored: $originalApo" -ForegroundColor Cyan
            }
        }
        $regKey.Close()
    }
} else {
    Write-Host "[-] Could not find DeviceInterface pin." -ForegroundColor Red
}

# 4. Restart audio services
Stop-Service Audiosrv -Force -ErrorAction SilentlyContinue
Stop-Service AudioEndpointBuilder -Force -ErrorAction SilentlyContinue
Get-Process audiodg -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

Start-Service AudioEndpointBuilder
Start-Sleep -Milliseconds 500
Start-Service Audiosrv
Start-Sleep -Seconds 1

Write-Host "=== Audio Engine Reloaded ===" -ForegroundColor Green
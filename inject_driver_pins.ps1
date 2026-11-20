$clsid = "{D1B100A1-8888-4444-9999-000000000001}"
$baseClass = "HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e96c-e325-11ce-bfc1-08002be10318}\0000\DeviceInterfaces"

# Target active speaker pins
$pins = @(
    "eSpeaker\DeviceParameters\MSFX\0",
    "eDuplicatedHpSpeaker\DeviceParameters\MSFX\0",
    "eDuplicatedSpeakerSpeaker\DeviceParameters\MSFX\0",
    "eDuplicatedLineOutSpeaker\DeviceParameters\MSFX\0"
)

# 1. Backup existing driver pin MSFX settings
$backupDir = "C:\ProgramData\Diviora\driver_pin_backups"
if (-not (Test-Path $backupDir)) { New-Item -Path $backupDir -ItemType Directory -Force | Out-Null }

foreach ($pin in $pins) {
    $fullPath = "$baseClass\$pin"
    if (Test-Path $fullPath) {
        $props = Get-ItemProperty -Path $fullPath
        $safeName = $pin -replace "[\\\/]", "_"
        $props | Export-Clixml -Path "$backupDir\$safeName.xml" -Force
        Write-Host "[+] Backed up: $pin" -ForegroundColor Green

        # 2. Prepend Diviora CLSID to the multi-string collection {D3993A3F-99C2-4402-B5EC-A92A0367664B},5
        $regKey = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($fullPath.Replace("HKLM:\", ""), $true)
        if ($regKey) {
            $existing = $regKey.GetValue("{D3993A3F-99C2-4402-B5EC-A92A0367664B},5")
            $list = [System.Collections.Generic.List[string]]::new()
            $list.Add($clsid)
            if ($existing) {
                foreach ($item in $existing) {
                    if ($item -ne $clsid) { $list.Add($item) }
                }
            }
            $regKey.SetValue("{D3993A3F-99C2-4402-B5EC-A92A0367664B},5", $list.ToArray(), [Microsoft.Win32.RegistryValueKind]::MultiString)
            
            # Also set the legacy stream effect slot for full fallback compatibility
            $regKey.SetValue("{D04E05A6-594B-4fb6-A80D-01AF5EED7D1D},5", $clsid, [Microsoft.Win32.RegistryValueKind]::String)
            
            $regKey.Close()
            Write-Host "[+] Injected Diviora into: $pin" -ForegroundColor Cyan
        }
    }
}
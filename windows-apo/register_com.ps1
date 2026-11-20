$clsid = '{D1B100A1-8888-4444-9999-000000000001}'
$keyPath = 'HKLM:\SOFTWARE\Classes\CLSID\' + $clsid
$inprocPath = $keyPath + '\InprocServer32'

if (-not (Test-Path $keyPath)) { New-Item -Path $keyPath -Force | Out-Null }
if (-not (Test-Path $inprocPath)) { New-Item -Path $inprocPath -Force | Out-Null }

Set-ItemProperty -Path $keyPath -Name '(default)' -Value 'Diviora System Audio Processing Object' -Force
Set-ItemProperty -Path $inprocPath -Name '(default)' -Value 'C:\Program Files\Diviora\DivioraAPO.dll' -Force
Set-ItemProperty -Path $inprocPath -Name 'ThreadingModel' -Value 'Both' -Force
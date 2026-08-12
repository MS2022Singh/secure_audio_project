@echo off
echo Registering Diviora Master System Processing Core Matrix COM CLSID...
reg add "HKLM\SOFTWARE\Classes\CLSID\{D1B100A1-8888-4444-9999-000000000001}" /ve /t REG_SZ /d "Diviora Master System Processing Object" /f
reg add "HKLM\SOFTWARE\Classes\CLSID\{D1B100A1-8888-4444-9999-000000000001}\InProcServer32" /ve /t REG_SZ /d "%~dp0DivioraAPO.dll" /f
reg add "HKLM\SOFTWARE\Classes\CLSID\{D1B100A1-8888-4444-9999-000000000001}\InProcServer32" /v "ThreadingModel" /t REG_SZ /d "Both" /f
echo Registration complete.

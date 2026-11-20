call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
cd "C:\Users\Diviora\secure_audio_project\windows-service"
if not exist "bin" mkdir "bin"
cl.exe /EHsc /W3 /O2 /std:c++20 /DUNICODE /D_UNICODE /I"..\windows-apo\include" src\*.cpp /link /OUT:bin\DivioraControlService.exe Advapi32.lib User32.lib Ole32.lib
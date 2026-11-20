call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
cd "C:\Users\Diviora\secure_audio_project\windows-service"
cl.exe /EHsc /W3 /O2 /std:c++20 /DUNICODE /D_UNICODE /I"..\windows-apo\include" DivioraControlService.cpp /link /OUT:DivioraControlService.exe Advapi32.lib User32.lib Ole32.lib
call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
cd "C:\Users\Diviora\secure_audio_project\windows-apo"
cl.exe /O2 /std:c++20 /LD /EHsc /DUNICODE /D_UNICODE /I"include" DivioraAPOProxy.cpp /link /OUT:DivioraAPO.dll Ole32.lib User32.lib Advapi32.lib
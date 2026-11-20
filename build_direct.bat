call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
cd "C:\Users\Diviora\secure_audio_project\windows-apo"
if not exist "build\Release" mkdir "build\Release"
cl.exe /O2 /std:c++20 /EHsc /DUNICODE /D_UNICODE /Iinclude src\DivioraAPO.cpp src\ClassFactory.cpp /link /DLL /DEF:DivioraAPO.def /OUT:build\Release\DivioraAPO.dll Ole32.lib Advapi32.lib User32.lib
cd "C:\Users\Diviora\secure_audio_project\windows-installer"
cl.exe /EHsc /W3 /O2 /std:c++20 /DUNICODE /D_UNICODE injector_safe.cpp /link /OUT:injector_safe.exe ole32.lib advapi32.lib
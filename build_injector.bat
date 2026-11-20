call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
cd "C:\Users\Diviora\secure_audio_project\windows-installer"
cl.exe /EHsc /W3 /O2 /DUNICODE /D_UNICODE injector_single.cpp /link /OUT:injector_single.exe ole32.lib advapi32.lib
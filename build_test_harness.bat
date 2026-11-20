call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
cd "C:\Users\Diviora\secure_audio_project\windows-installer"
cl.exe /EHsc /W3 /O2 /std:c++20 /DUNICODE /D_UNICODE /I"..\windows-apo\include" test_com_harness.cpp /link /OUT:test_com_harness.exe ole32.lib advapi32.lib
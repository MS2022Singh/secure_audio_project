call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
cl.exe /EHsc /W3 /O2 /DUNICODE /D_UNICODE injector.cpp /link /OUT:injector.exe ole32.lib advapi32.lib
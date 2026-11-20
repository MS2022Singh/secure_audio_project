@echo off
REM ============================================================================
REM  Diviora native APO - the single supported build.
REM  Builds (all into windows-apo\build\Release):
REM    DivioraAPO.dll      the APO (src\*.cpp + Rust DSP core, static runtime: no
REM                        VC++ redistributable needed inside audiodg.exe)
REM    apo_selftest.exe    safe self-test (no registry, no audiodg)
REM    injector_single_slot.exe  binds the APO to the default speakers' stream slot
REM  Run from any prompt:  windows-apo\build_native_apo.bat
REM ============================================================================
setlocal
cd /d "%~dp0"

echo === 1/4 Rust DSP core (diviora-dsp) ===
pushd ..\diviora-dsp
set "RUSTFLAGS=-C target-feature=+crt-static"
cargo build --release --target x86_64-pc-windows-msvc
if errorlevel 1 (popd & goto :fail)
popd
set "DSPLIB=..\diviora-dsp\target\x86_64-pc-windows-msvc\release\diviora_dsp.lib"

echo === 2/4 MSVC environment ===
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
for /f "usebackq tokens=*" %%i in (`"%VSWHERE%" -latest -products * -property installationPath`) do set "VSPATH=%%i"
if not defined VSPATH (echo Visual Studio not found & goto :fail)
call "%VSPATH%\VC\Auxiliary\Build\vcvars64.bat" >nul
if errorlevel 1 goto :fail
if not exist build\Release mkdir build\Release

set "CFLAGS=/nologo /std:c++20 /O2 /MT /EHsc /W4 /DUNICODE /D_UNICODE /D_WIN32_WINNT=0x0A00"

echo === 3/4 DivioraAPO.dll ===
cl %CFLAGS% /Iinclude src\DivioraAPO.cpp src\ConfigManager.cpp src\ClassFactory.cpp /Fobuild\Release\ ^
   /link /DLL /DEF:DivioraAPO.def /OUT:build\Release\DivioraAPO.dll ^
   "%DSPLIB%" ole32.lib advapi32.lib crypt32.lib ntdll.lib userenv.lib ws2_32.lib dbghelp.lib
if errorlevel 1 goto :fail
dumpbin /nologo /exports build\Release\DivioraAPO.dll | findstr /c:"Dll"
dumpbin /nologo /dependents build\Release\DivioraAPO.dll | findstr /i "vcruntime msvcp" && (echo ERROR: DLL depends on the VC++ runtime & goto :fail)

echo === 4/4 apo_selftest.exe and injector_single_slot.exe ===
cl %CFLAGS% tests\apo_selftest.cpp /Fobuild\Release\ /Fe:build\Release\apo_selftest.exe ole32.lib crypt32.lib
if errorlevel 1 goto :fail
cl %CFLAGS% ..\windows-installer\injector_single_slot.cpp /Fobuild\Release\ /Fe:build\Release\injector_single_slot.exe ole32.lib
if errorlevel 1 goto :fail

echo.
echo === BUILD OK ===
echo Next: build\Release\apo_selftest.exe   (must print ALL CHECKS PASSED)
exit /b 0

:fail
echo.
echo === BUILD FAILED (see the first error above) ===
exit /b 1

# Diviora native APO

System-wide processing works the way Equalizer APO 1.4.2 does: Windows loads `DivioraAPO.dll` into `audiodg.exe` on the real speakers. There is no VB-Cable, service, shared memory or extra install.

```
Diviora app (Administrator)            audiodg.exe (Windows audio)
  Upload config  -> held in RAM          DivioraAPO.dll
  Lock Config    -> encrypt (DPAPI) ---> watches %ProgramData%\Diviora\apo\config.dvr
                    + ACL-lock folder    decrypts, parses (Rust diviora-dsp), hot-swaps filters
                                         writes status\apo_status.json every second
  Developer panel status <---------------- (pid, process, frames, config state)
```

## Supported files (only these are built)

| File | Role |
|---|---|
| `src/DivioraAPO.cpp`, `include/DivioraAPO.h` | The APO itself (IAudioProcessingObject, IAudioProcessingObjectConfiguration, IAudioProcessingObjectRT, IAudioSystemEffects2) |
| `src/ConfigManager.cpp`, `include/ConfigManager.h` | Watches, decrypts and hot-reloads the config; writes the status file |
| `src/ClassFactory.cpp` | COM factory, `DllRegisterServer` (CLSID + AudioEngine registration) |
| `include/diviora_dsp.h` | C interface to the Rust parser and filter engine (`../diviora-dsp`) |
| `DivioraAPO.def` | DLL exports |
| `tests/apo_selftest.cpp` | Safe test: drives the DLL as audiodg does, with no system changes |
| `build_native_apo.bat` | The only build script |
| `../windows-installer/install_native_apo.ps1` | Install, verify and uninstall, with backup and automatic rollback |

## Legacy files (kept for history, do not build or run)

- `DivioraAPO.cpp`, `DivioraAPO.h`, `ClassFactory.*`, `DllMain.cpp`, `DivioraAPOProxy.cpp` at the folder root, plus `build_all.bat`, `build_dll.bat`, `build_verified_dll.bat`, `build_and_dump.bat` and `CMakeLists.txt`. These are the earlier competing implementations.
- `include/shared_protocol.h` belongs to the old shared-memory design.
- `../windows-service/DivioraControlService.cpp` is a **VB-Cable bridge**. It also wrote `instantiated` and `process_calls` into the "APO diagnostics" itself, which made the history look as if the APO was processing audio when it was not. **Do not run it.**
- `../windows-installer/injector_single.cpp` binds the mode-effect slot (`,6`). The supported injector is `injector_single_slot.cpp`, which binds the stream-effect slot (`,5`).

## Steps on Windows

```bat
windows-apo\build_native_apo.bat
windows-apo\build\Release\apo_selftest.exe
```
The self-test must print `ALL CHECKS PASSED`. Then, in an Administrator PowerShell:
```powershell
powershell -ExecutionPolicy Bypass -File windows-installer\install_native_apo.ps1
# reboot, play YouTube, then:
powershell -ExecutionPolicy Bypass -File windows-installer\install_native_apo.ps1 -Verify
```
Run Diviora as Administrator, upload the config and press **Lock Config**. Changes apply in about one second, with no reboot. To undo everything, run `install_native_apo.ps1 -Uninstall` and then reboot.

## What Windows requires (learned on the target hardware)

- **COM aggregation:** `audiodg.exe` creates the APO with `CreateInstance(outer, IID_IUnknown)`.
  `src/DivioraAPO.cpp` implements an inner non-delegating `IUnknown`; the self-test checks this path.
- **Composite effect lists:** on current DCH drivers (here Realtek + Waves), Windows reads
  `{d04e05a6-…},13` (stream), `,14` (mode) and `,15` (endpoint). The installer prepends Diviora to
  `,13` and keeps the existing entries. Legacy `,5/,6` are used only when no composite list exists.
- **Write-restricted token:** the status folder grants `NT SERVICE\Audiosrv` and `WRITE RESTRICTED`.
- **Reboot or `Restart-Service Audiosrv`** after changing the effect list or the DLL.

## Safety rules built in

- A missing, corrupt or undecryptable config means pass-through (normal sound), never silence.
- The audio thread never allocates, locks or touches files. New filters are swapped in lock-free.
- Rust panics are caught at the C interface, so a bug in the parser cannot crash `audiodg.exe`.
- The DLL uses the static runtime, so it has no VC++ redistributable dependency inside `audiodg.exe`.
- Only the backed-up values on the default speakers (the stream-effect list and the enhancements flag) are changed, and they are restored exactly on uninstall or on a failed install.

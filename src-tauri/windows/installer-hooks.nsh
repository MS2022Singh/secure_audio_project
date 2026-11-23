; Diviora NSIS installer hooks (Tauri v2 bundler).
; After the app files are installed, set up the system-wide audio engine with the same
; script the developer uses (self-test, backup, register, bind, sync service, rollback on error).

; The installer is a 32-bit program: "System32" would be redirected to the 32-bit PowerShell,
; which sees the 32-bit registry view and cannot reach the speakers' audio settings.
; "Sysnative" is the 64-bit System32 as seen from a 32-bit process.
!macro DIVIORA_FIND_PS
  StrCpy $R9 "$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe"
  IfFileExists "$R9" +2 0
    StrCpy $R9 "$WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "Installing the Diviora system-wide audio engine..."
  !insertmacro DIVIORA_FIND_PS
  nsExec::ExecToLog '"$R9" -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\apo\install_native_apo.ps1"'
  Pop $0
  StrCmp $0 "0" diviora_ok
    MessageBox MB_ICONEXCLAMATION|MB_OK /SD IDOK "The system-wide audio engine could not be installed (code $0).$\r$\nThe player still works. Details: C:\ProgramData\Diviora\install.log$\r$\nSee the User Manual, section Troubleshooting."
    Goto diviora_done
  diviora_ok:
    ; Windows rebuilds the audio graph only on reboot.
    SetRebootFlag true
  diviora_done:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DetailPrint "Removing the Diviora system-wide audio engine and restoring the original speaker settings..."
  !insertmacro DIVIORA_FIND_PS
  nsExec::ExecToLog '"$R9" -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\apo\install_native_apo.ps1" -Uninstall'
  Pop $0
  SetRebootFlag true
!macroend

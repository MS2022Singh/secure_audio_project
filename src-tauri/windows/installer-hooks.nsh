; Diviora NSIS installer hooks (Tauri v2 bundler).
; After the app files are installed, set up the system-wide audio engine with the same
; script the developer uses (self-test, backup, register, bind, sync service, rollback on error).

!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "Installing the Diviora system-wide audio engine..."
  nsExec::ExecToLog 'powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\apo\install_native_apo.ps1"'
  Pop $0
  StrCmp $0 "0" diviora_ok
    MessageBox MB_ICONEXCLAMATION|MB_OK "The system-wide audio engine could not be installed (code $0).$\r$\nThe player still works. See the User Manual, section Troubleshooting."
    Goto diviora_done
  diviora_ok:
    ; Windows rebuilds the audio graph only on reboot.
    SetRebootFlag true
  diviora_done:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DetailPrint "Removing the Diviora system-wide audio engine and restoring the original speaker settings..."
  nsExec::ExecToLog 'powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\apo\install_native_apo.ps1" -Uninstall'
  Pop $0
  SetRebootFlag true
!macroend

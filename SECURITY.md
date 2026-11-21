# Security

## Reporting a vulnerability

Please report security problems privately to the Diviora developer, not in public issues.
Include steps to reproduce and the affected version. You'll get an acknowledgement within
7 days.

## Design summary

- **Configurations** are encrypted with Windows DPAPI (machine scope) and stored in a folder
  that only SYSTEM, Administrators and Windows audio can read. They never appear in plain
  text on disk or in reports.
- **Developer password** and recovery key: salted PBKDF2-HMAC-SHA256 (600 000 rounds),
  lockout after 5 failures. There are no built-in or default passwords.
- **Licences:** one licence per device, enforced by the server. Sessions are HMAC-signed and
  expire after 7 days. Revocation takes effect at the next sync.
- **Updates** are signed. The app refuses unsigned or wrongly signed updates.
- **Audio engine:** the APO's audio thread never allocates, locks or does file I/O. Parser
  panics are contained. Any configuration error falls back to normal (unprocessed) sound,
  never silence. Install and uninstall back up and restore the exact original speaker
  settings and registry permissions.

## Limits

An administrator on a PC can always inspect that PC's memory and files. The protections stop
normal users, stop copying configurations to other machines, and make leaks traceable through
the watermark. They don't make a determined administrator unable to analyse their own machine.

## Secrets the developer must protect

- The update signing private key (`TAURI_SIGNING_PRIVATE_KEY`)
- `DIVIORA_ADMIN_TOKEN`, `DIVIORA_SECRET` and `DIVIORA_CONFIG_KEY` on the licence server
- The developer recovery key

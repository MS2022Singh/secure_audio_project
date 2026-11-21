# Diviora: Developer Guide

This guide is for the Diviora developer: building, the developer panel, publishing
configurations, releases and auto-update, and the licence server.

## 1. Architecture

```
Developer PC                         Licence server (backend/)            User PC
────────────                         ─────────────────────────            ───────
Diviora app (developer panel)        FastAPI + SQLite                     Diviora app (consumer)
  upload Equalizer APO config  ──►   POST /admin/config (AES-GCM at rest)   activate licence ──► /activate
  watermark settings                                                      diviora-sync service (SYSTEM)
  Lock Config ─► local config.dvr                                            polls /config every 5 min
              └► publish ───────────────────────────────────────────────►   writes config.dvr (DPAPI + ACL)
                                                                          DivioraAPO.dll in audiodg.exe
GitHub Actions (tag v*) ─► signed installer + latest.json ─► app auto-update   reads config.dvr, EQ + watermark
```

| Folder | What it is |
|---|---|
| `windows-apo/` | Native APO (`DivioraAPO.dll`), self-test, build script. See `windows-apo/README.md`. |
| `diviora-dsp/` | Rust DSP core: Equalizer APO parser, filters, watermark, analysis, CamillaDSP export. Linked into the APO and the app. |
| `diviora-client/` | Shared by app and service: locked-config writer, device ID, server client, sync. |
| `diviora-sync/` | Windows service that applies published configurations. |
| `src-tauri/`, `frontend/` | The desktop app (Tauri v2). |
| `windows-installer/` | APO install/verify/uninstall script and endpoint injector. |
| `backend/` | Licence and configuration server. |

## 2. Building on Windows

Requirements: Visual Studio 2022 (Desktop C++ workload), Rust (stable, MSVC), Node.js.

```bat
windows-apo\build_native_apo.bat
windows-apo\build\Release\apo_selftest.exe
npm install
npx tauri build
```

The self-test must print `ALL CHECKS PASSED`. The installer is written to
`src-tauri\target\release\bundle\nsis\`. To develop with hot reload, run `npx tauri dev`
after `build_native_apo.bat`; the bundle references the built APO files.

Every push also runs **`.github/workflows/windows-ci.yml`** on GitHub's Windows machines:
DSP tests, the MSVC APO build, the self-test (including audiodg-style COM aggregation),
the sync service build, the server tests and the app unit tests.

## 3. Developer panel

Open it with **Ctrl + Shift + D**, then sign in with the developer password.

- **First use:** click **Reset Password**, leave *Current password* empty, and choose a password
  (at least 10 characters, three character types). A **recovery key** is shown **once**. Store it
  offline; it's the only way to reset a forgotten password.
- **Upload config:** Equalizer APO 1.4.x `.txt` (Preamp, Filter, Channel, Copy, Delay, Device,
  Include). Unsupported lines (`If`, `GraphicEQ`, `VST`, …) are listed, never silently dropped.
- **Lock Config:** encrypts the config with DPAPI (machine-bound) and locks it so only Windows
  audio and Administrators can read it. The app must run **as Administrator** for this. With
  developer credentials (section 5), the config is also published to all users.
- **Watermark:** Signature ID, carrier frequency, amplitude, optional uploaded clip, time window,
  interval and auto mode (only under music). **Lock Watermark Settings** saves them; they
  travel inside the locked config.
- **Analysis panel:** measured peak gain, latency, init time and CPU for the uploaded config.
- **Install / Repair System-Wide APO:** re-runs the APO installer (administrator prompt).
- **Free Access Token Generator:** creates single-use invite tokens on the licence server.
- **Reports:** system report (configuration fingerprint only, never the configuration itself),
  watermark report (detects the signature in the loaded file).

## 4. System-wide APO on a development PC

```powershell
powershell -ExecutionPolicy Bypass -File windows-installer\install_native_apo.ps1             # install
powershell -ExecutionPolicy Bypass -File windows-installer\install_native_apo.ps1 -Diagnose   # read-only check
powershell -ExecutionPolicy Bypass -File windows-installer\install_native_apo.ps1 -TestConfig "Preamp: -24 dB"
powershell -ExecutionPolicy Bypass -File windows-installer\install_native_apo.ps1 -ClearConfig
powershell -ExecutionPolicy Bypass -File windows-installer\install_native_apo.ps1 -Uninstall
```

Live logs from inside `audiodg.exe`: Sysinternals **DebugView** as Administrator, with
*Capture Global Win32* on, filtered on `[Diviora]`.

## 5. Licence server

See `backend/README.md` to deploy it. On the developer PC, create
`%LOCALAPPDATA%\Diviora\developer.json`:

```json
{ "server_url": "https://licences.example.com", "admin_token": "<DIVIORA_ADMIN_TOKEN>" }
```

This file lets the developer panel create invites and publish configurations. **Never put it on
a user's PC.** User builds find the server through the `DIVIORA_SERVER_URL` value baked in at
build time (a GitHub secret in the release workflow).

Create licence keys (`curl`, or any HTTP client):

```bash
curl -X POST https://licences.example.com/api/v3/admin/licenses -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" -d '{"days":365,"note":"customer name"}'
```

Revoke a key or release its device binding:

```bash
curl -X POST https://licences.example.com/api/v3/admin/licenses/DVL-XXXX/revoke -H "Authorization: Bearer $TOKEN"
```
```bash
curl -X POST https://licences.example.com/api/v3/admin/licenses/DVL-XXXX/reset-device -H "Authorization: Bearer $TOKEN"
```

## 6. Releases and automatic updates

One-time setup:

1. Generate the update signing key on your PC (keep the private key secret and backed up):
   ```bash
   npx tauri signer generate -w %USERPROFILE%\.tauri\diviora.key
   ```
2. In GitHub, **Settings → Secrets and variables → Actions**, add these secrets:
   - `TAURI_SIGNING_PRIVATE_KEY`: contents of `diviora.key`
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: its password
   - `TAURI_UPDATER_PUBKEY`: contents of `diviora.key.pub`
   - `DIVIORA_SERVER_URL`: your licence server URL

Each release:

1. Raise `version` in `src-tauri/tauri.conf.json` (for example `0.2.0` → `0.3.0`) and commit.
2. Push a tag:
   ```bash
   git tag v0.3.0
   ```
   ```bash
   git push origin v0.3.0
   ```
3. **`.github/workflows/release.yml`** builds and signs the installer and publishes a GitHub
   Release with `latest.json`. Every installed copy updates itself at its next start.

Configuration updates don't need a release: **Lock Config** on the developer PC publishes them,
and users receive them within about 5 minutes.

## 7. Security notes

- No built-in passwords. Passwords and recovery keys are stored as salted PBKDF2-SHA256 hashes.
- `config.dvr` is DPAPI-encrypted per machine and ACL-locked. An administrator on a user's PC can
  still read Windows audio's memory; DPAPI and ACLs stop normal users and stop copying a config to
  another PC.
- The update signing key and `DIVIORA_ADMIN_TOKEN` are the two critical secrets. Store them offline.
- This repository is **public**. Make it private if the source code should not be visible.

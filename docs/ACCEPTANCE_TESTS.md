# Acceptance tests: is Diviora working as required?

Work through these in order. Each test names the requirement, the steps and the expected
result. Record **Pass / Fail** and keep the notes for anything that fails.

Before starting, download the latest test build (works in any browser or phone):

- Windows: https://github.com/MS2022Singh/secure_audio_project/releases/download/test-build/Diviora-Setup-windows-x64.exe
- Android: https://github.com/MS2022Singh/secure_audio_project/releases/download/test-build/Diviora-android-test.apk

Test builds aren't code-signed: on Windows click **More info → Run anyway** if SmartScreen warns.

---

## A. Windows: install and system-wide audio

| # | Requirement | Steps | Expected |
|---|---|---|---|
| A1 | One installer, no extra apps | Run `Diviora-Setup-windows-x64.exe` and accept the administrator prompt | It shows the licence page, installs without asking for VB-Cable or anything else, and asks to restart at the end. If it reports a problem, send `C:\ProgramData\Diviora\install.log` |
| A2 | Native audio unharmed | After the restart, play YouTube in Chrome | Sound is normal (no config yet, so pass-through) |
| A3 | Diviora is inside Windows audio | Administrator PowerShell: `powershell -ExecutionPolicy Bypass -File "C:\Program Files\Diviora Psychoacoustics\apo\install_native_apo.ps1" -Diagnose` while YouTube plays | `[4]` lists `DivioraAPO.dll`; `[5]` shows `"process":"AUDIODG.EXE"` and `process_calls` rising |
| A4 | Config changes **all** system audio | Same folder: `... install_native_apo.ps1 -TestConfig "Preamp: -24 dB"` while YouTube plays | YouTube gets much quieter within about 1 s; Spotify or other apps too |
| A5 | Back to normal | `... install_native_apo.ps1 -ClearConfig` | Volume returns to normal within about 1 s |
| A6 | Frequency-specific EQ | `-TestConfig "Filter: ON PK Fc 1000 Hz Gain -24 dB Q 2"` and play a 1 kHz test tone (search "1000 Hz tone") and then music | The tone almost disappears; music sounds thinner in the mids but is not silent. Then `-ClearConfig` |
| A7 | Clean uninstall | **Settings → Apps → Diviora Psychoacoustics → Uninstall**, then restart | Sound is normal; `-Diagnose` (if the file is still there) or Sound settings show no Diviora effects |

## B. Developer panel

| # | Requirement | Steps | Expected |
|---|---|---|---|
| B1 | Activation first, developer hidden | Open Diviora on a device that is not activated | Only **Activation** is shown; no player and no developer panel. After entering a key (test mode accepts any key while no licence server is configured) the **Consumer Deck** with the player and a **Licence** card opens |
| B2 | Hidden shortcut | Press **Ctrl + Shift + D** (phone: tap the DIVIORA logo 7 times) | **Master Security** appears and asks for a password |
| B3 | Only the developer's password | Try `admin123`, `diviora2026` and a licence key such as `DVL-TEST-1` | All rejected ("Invalid Access Token") |
| B4 | Users cannot create their own developer password | **Reset Password** with an empty current password and any new one | Refused: "built into the app" |
| B5 | Lockout survives restart | Enter a wrong password 5 times, close and reopen the app, try again | "Too many attempts" for 60 s |
| B6 | Developer password and recovery key work | Unlock with the developer password; lock again (shortcut), then unlock with the recovery key `DVR-…` | Both open the developer panel |
| B7 | Upload + lock config | Run Diviora **as Administrator**, upload an Equalizer APO `.txt`, press **Lock Config** | "Master configuration encrypted and locked …"; system audio changes (as in A4) |
| B8 | Config not visible to users | As a normal (non-admin) Windows user, open `C:\ProgramData\Diviora\apo\` | "Access denied" |
| B9 | Measured analysis | **Apply Matrix** after uploading | Peak gain, latency, init time and CPU show real values (e.g. peak gain equals your preamp + largest boost) |
| B10 | Reports don't leak the config | **Download Hardware Matrix Report** and open it on the Desktop | Only a fingerprint (SHA-256), stage counts and APO status; no filter lines |

## C. Watermark

| # | Requirement | Steps | Expected |
|---|---|---|---|
| C1 | Developer sets the watermark | Set Signature ID / Carrier 16000 / Amplitude 0.0008, enable **DSSS Signal Injection**, **Lock Watermark Settings**, then **Lock Config** | Confirmation message with ID, frequency and dBFS |
| C2 | Inaudible | Play music | No audible tone or hiss (16 kHz at −62 dBFS, and only under music in auto mode) |
| C3 | Embedded in saved files | Load a song, **Download Processed Audio** | `<name>_DIVIORA_PROCESSED.wav` on the Desktop |
| C4 | Detectable | Drag the processed WAV into the player, then **Export Watermark Audit Report** | Report says `Watermark detected: true` with a score above 0.8 |
| C5 | Not in the original | Load the original (unprocessed) song, export the report again | `Watermark detected: false` |
| C6 | Uploaded watermark audio | **Upload Watermark Audio** with a short WAV/MP3, lock, then download a processed file | The file contains the clip at the chosen amplitude (audible only if you raise the amplitude for testing) |

## D. Licences, invites and automatic updates

For testing on one PC you don't need any hosting: start the free local server in an
Administrator PowerShell with `powershell -ExecutionPolicy Bypass -File backend\run_test_server.ps1`
(needs Python from python.org) and keep that window open. For D1/D2 you need two devices,
which needs the server reachable from both; the single-PC tests (D3–D5) work with the local one.

| # | Requirement | Steps | Expected |
|---|---|---|---|
| D1 | One ID = one device | Create a licence key; activate it on PC 1, then on PC 2 | PC 1: "Activated … bound to this device"; PC 2: "already bound to another device" |
| D2 | Move a licence | Developer runs `reset-device` for the key; activate on PC 2 | PC 2 is activated |
| D3 | Invite tokens | Developer panel → **Generate Free Key Token**; enter it on a user PC | Activated for the chosen number of days; using the same token again fails |
| D4 | Developer update reaches users | Developer PC: change the config, **Lock Config** ("Published to all users as version N") | On an activated user PC the sound changes within about 5 minutes, with the user's app closed |
| D5 | Revocation | Developer revokes a key | That PC stops receiving updates at the next sync |
| D6 | App auto-update | Developer publishes a new version tag (DEVELOPER_GUIDE §6); start the app on a user PC | The app updates itself and restarts with the new version |

## E. Player (Windows and Android)

| # | Requirement | Steps | Expected |
|---|---|---|---|
| E1 | Add files | Click the drop zone and pick audio files (WAV/MP3/FLAC/OGG) | They appear in the Playlist Queue |
| E2 | Kept until deleted | Close and reopen the app | The playlist is still there |
| E3 | Delete | ✕ on a track → confirm | The track disappears and its stored copy is deleted |
| E4 | Controls | Play / Pause / Stop / Prev / Next | They behave as labelled |
| E5 | Processed playback | With a config active, play a track | It sounds processed (on Windows through the system-wide engine; on Android in the app) |

## F. Android

| # | Requirement | Steps | Expected |
|---|---|---|---|
| F1 | Install | Download `Diviora-android-test.apk` on the phone, open it, and allow "install unknown apps" | Diviora installs and opens |
| F2 | Activation | Enter a licence key or invite (internet needed once) | "Activated … bound to this device" |
| F3 | Config + watermark applied | After activation (wait up to 5 min for the first sync), import a song and play it | It sounds processed by the developer's configuration |
| F4 | Save processed | **Download Processed Audio** | A `…_DIVIORA_PROCESSED.wav` appears in the playlist; C4/C5-style detection works on it |
| F5 | Library kept | Close and reopen | Imported and processed files are still in the playlist until deleted |

Android limitation (by design of Android): other apps' audio (YouTube app, Spotify) is **not**
processed. Only audio played inside Diviora is.

---

## Automatic checks (no hardware needed)

Every push runs these on GitHub (Actions → windows-ci): DSP and watermark tests, the Windows
APO build, the APO self-test (including how audiodg.exe creates the APO), the sync-service
build, the licence-server tests, the app tests, the Windows installer build and the Android APK
build. A green tick means all of them passed.

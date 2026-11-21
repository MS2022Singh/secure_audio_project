# Diviora Psychoacoustics: User Manual

Diviora is a music player and system-wide audio engine for Windows. Once installed, the
developer's sound configuration applies to **everything you hear on this PC**: the
Diviora player, YouTube in your browser, Spotify, games and system sounds. Updates to the
app and to the sound configuration arrive automatically.

## 1. Installing

1. Run **`Diviora-Psychoacoustics_x.y.z_x64-setup.exe`** and accept the administrator prompt.
   Administrator rights are needed because the audio engine becomes part of Windows audio.
2. Accept the licence agreement and finish the setup. The installer:
   - installs the app,
   - backs up your current speaker settings,
   - adds the Diviora audio engine to your default speakers,
   - installs the background service that applies the developer's updates.
3. **Restart your PC** when asked. Windows only loads new audio components after a restart.

After the restart your sound is unchanged until a configuration is active, and that's normal.

If you change your default speakers or headphones in Windows later, run setup again
(**Settings → Apps → Diviora Psychoacoustics → Modify**, or reinstall) so the engine is added
to the new device.

## 2. Activating your licence

1. Open Diviora. On the **Consumer Deck**, find **Consumer Activation Gateway**.
2. Enter your **licence key** (`DVL-…`) or **invite token** (`DVI-…`) and click **Activate Device Key**.
3. You'll see "Activated … bound to this device until <date>".

One licence works on **one device**. To move it to another PC, click **Reset Client Token**
on the old PC and ask the developer to release the licence.

## 3. Using the player

- **Add music:** drag audio files or folders (WAV, MP3, FLAC, OGG) onto the green drop zone,
  or click it to choose files. They appear in the **Playlist Queue** on the left.
- **Play / Pause / Stop / Prev / Next:** buttons below the drop zone.
- **Download Processed Audio:** saves the current track to your Desktop as
  `<name>_DIVIORA_PROCESSED.wav`, processed with the active configuration and watermark.

Music from other apps (browser, Spotify, …) is processed automatically. You don't need to
play it through Diviora.

## 4. Updates

- **App updates** install automatically when you start Diviora, and the app restarts by itself.
- **Sound configuration updates** from the developer are applied by the background service
  within about 5 minutes, even if Diviora isn't open. You'll hear the change; no restart is needed.

## 5. Uninstalling

**Settings → Apps → Diviora Psychoacoustics → Uninstall.** Your original speaker settings are
restored. Restart your PC when asked.

## 6. Troubleshooting

| Problem | What to do |
|---|---|
| No sound at all after installing | Restart the PC once more. If it continues, uninstall Diviora (this restores the original speaker settings) and restart, then contact the developer. |
| Sound didn't change after a configuration update | Wait 5 minutes, and check that the licence is activated. Play audio through your **default** speakers. |
| "Licence is already bound to another device" | That key is in use on another PC. Ask the developer to reset it. |
| "Cannot reach the licence server" | Check your internet connection. Activation needs it once; afterwards Diviora keeps working offline until the session needs renewal (about every 7 days). |
| Setup said "system-wide audio engine could not be installed" | The player still works. Run setup again as administrator; if it fails again, send the developer the file `C:\ProgramData\Diviora\client\sync.log` and the setup log. |

Some audio isn't processed: apps that use exclusive-mode output, DRM-protected streams, or
audio sent to a device other than your default speakers.

## 7. Privacy and protection

- The sound configuration is stored encrypted on your PC and only Windows audio can read it.
- The device ID is derived from your Windows installation ID. It's used only to enforce
  "one licence = one device".
- Processed audio may carry an inaudible watermark that identifies the licence. Removing it
  breaches the licence agreement.


## Compare with Equalizer APO 1.4.2 (ground truth)

Checks Diviora against the real Equalizer APO with your own configuration, on your own PC.

1. Upload the configuration in the developer panel.
2. Press **Compare with Equalizer APO 1.4.2: 1. Create test files**. A folder
   `Diviora_EqualizerAPO_Compare` appears on the Desktop, holding `test_input.wav` (6 s of noise) and `run_with_equalizer_apo.bat`.
3. On a PC with Equalizer APO 1.4.2:
   1. Load the same configuration in its Configuration Editor and save it.
   2. Double-click the .bat. It runs Equalizer APO's own `Benchmark.exe -i test_input.wav -o eqapo_output.wav -v`, and also keeps Equalizer APO's log.
4. Press **2. Compare**. You get:
   - the level and the gain in 20 bands for each channel, Equalizer APO next to Diviora;
   - the waveform difference;
   - the L/R mixing;
   - Equalizer APO's own messages about invalid or ignored lines.

   All numbers are hidden in the messages as `#`.

The comparison only reads those files. Nothing is uploaded.

## Network use (security)

| Connection | When it happens |
|---|---|
| The update manifest (GitHub release) | Only the version check. Nothing is sent. |
| Your licence server | Activation, licence check and configuration download, using HTTPS only (plain HTTP is allowed only to this computer, for testing). |
| Your licence server | Publishing to users happens only when the developer presses **Publish current configuration**. **Lock Config never publishes.** |

The GitHub project sync was removed.

The web view runs under a strict Content-Security-Policy (`connect-src ipc:`), so the user interface cannot send anything anywhere itself. Reports (Explain, Compare, hardware report) are only saved on the Desktop.

## Publish to users (end-to-end encrypted, one licence = one device)

The developer panel has a card for this: **📡 Publish to Users**.

1. Enter your licence server address (`https://…`) and the developer token, then press **Save server**.
   - The token is stored encrypted for your Windows user (DPAPI).
2. Press **Publish current configuration**. This is how the configuration travels:

| Step | What happens |
|---|---|
| Device key | Every activated device has its own key pair, created on the device. The private key never leaves it (DPAPI, machine scope). Its public key is registered with the licence at activation, and a licence binds to one device. |
| Encryption | Diviora encrypts the configuration with a new random key (AES-256-GCM). |
| Per-device copy | That key is wrapped separately for each activated device's public key (X25519 + HKDF-SHA256). |
| Server | The server receives only ciphertext. It cannot read the configuration, and it refuses plain text. |
| Device | The device unwraps its copy, decrypts the configuration and locks it for the APO. Another device cannot open it. |

Devices activated after a publish get access automatically the next time the developer panel is open. You don't need to publish again.

**Withdraw from users** stops the distribution.

## Updates and rollback

Every push builds and publishes two pre-releases on GitHub:

| Release | Contents |
|---|---|
| `test-build` | The newest build. Apps update to it automatically. |
| `previous-build` | The build before it, for rollback. Only one is kept, so older builds don't pile up. Its tag points at that version's source code. |

**Licence → ↩ Roll back to previous version** (Windows):
1. Installs the previous build, after checking its signature.
2. Holds the version you left, so it is not installed again automatically.
3. Resumes automatic updates with the next newer build.

Android does not allow installing an older version over a newer one. To roll back on a phone, uninstall Diviora, then install the APK from `previous-build`.

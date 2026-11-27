
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
| Your licence server | Publishing to users happens only when the developer calls `publish_config_to_users`. **Lock Config never publishes.** |

The GitHub project sync was removed.

The web view runs under a strict Content-Security-Policy (`connect-src ipc:`), so the user interface cannot send anything anywhere itself. Reports (Explain, Compare, hardware report) are only saved on the Desktop.

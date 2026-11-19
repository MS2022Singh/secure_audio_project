# diviora-dsp

System-wide Equalizer APO–style processing without a signed driver, a modified `audiodg.exe`, or Secure Boot changes.

```
Chrome / Spotify / games
   └─► Virtual cable (signed, e.g. VB-CABLE) — set as Windows default output
          └─► Diviora relay: WASAPI capture → Engine (your EQ APO file) → WASAPI render
                 └─► Real speakers / headphones
```

## Modules

| Module | What it does |
|---|---|
| `eqapo` | Parses Equalizer APO 1.4.x `config.txt` / include files: `Preamp`, `Filter` (PK, LS/LSC, HS/HSC, LP/LPQ, HP/HPQ, BP, NO, AP; `Q`, `BW Oct`, shelf slope), `Channel`, `Copy`, `Delay`, `Device`, `Include`. Anything it cannot convert (`If`, `GraphicEQ`, `Convolution`, `VST`, …) is returned as a warning. Nothing is dropped silently. |
| `engine` | Compiles stages into RBJ biquads, gains, delays and a mixing matrix. The processing call never allocates or locks. |
| `camilla` | Writes CamillaDSP YAML in the v3 format: one file for all devices plus one per `Device:` block. `Copy:` becomes a `mixers` entry. |
| `relay` (feature `relay`) | Runs the capture → DSP → render relay with cpal/WASAPI. New settings are swapped in lock-free. It counts underruns, drops and drift corrections, and it detects when a device disappears. |

## Use

```bash
cargo test                                   # parser, filter math, export, acceptance (-24 dB) tests
cargo run --example eqapo2camilla -- config.txt "CABLE Output (VB-Audio Virtual Cable)" "Speakers (Realtek(R) Audio)"
```

In the app, `upload_apo_profile` parses the uploaded file and hot-swaps it into the relay. The UI starts the relay from **System-Wide Audio Route**, and **Export as CamillaDSP YAML** writes the same file in CamillaDSP format.

## Windows setup and acceptance test

1. Install a signed virtual cable (VB-CABLE for prototyping; check its license before bundling it in a commercial installer).
2. Under Sound settings → *CABLE Input* → Properties → Advanced, set **48000 Hz**. Set your speakers to 48000 Hz the same way.
3. Make *CABLE Input* the default playback device.
4. In Diviora, choose capture *CABLE Output* and output = your speakers, then **Start System Route**.
5. Play YouTube. You should hear it once, unchanged.
6. Load a file containing `Preamp: -24 dB`. The audio should get clearly quieter within a fraction of a second.
7. Load `Filter: ON PK Fc 1000 Hz Gain -24 dB Q 2` and check that the 1 kHz content is cut.
8. Unplug the headphones. The status should read *device disconnected*; press Start again.

## Known limits

* Both devices must run at the same sample rate; the relay reports an error if they don't. Drift between the two clocks is handled by dropping samples, not by resampling.
* Audio is processed only if it is routed to the virtual cable. Exclusive-mode apps, apps pinned to another device, and DRM-protected paths are not processed.
* `If:` blocks and `GraphicEQ:` are reported but not converted yet.

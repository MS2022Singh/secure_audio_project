# diviora-dsp

Shared DSP logic for Diviora. It is pure Rust with no OS dependencies, so the Tauri app and the native APO path can use the same code.

| Module | What it does |
|---|---|
| `eqapo` | Parses Equalizer APO 1.4.x `config.txt` and include files: `Preamp`, `Filter` (PK, LS/LSC, HS/HSC, LP/LPQ, HP/HPQ, BP, NO, AP; `Q`, `BW Oct`, shelf slope), `Channel`, `Copy`, `Delay`, `Device` and `Include`. Lines it cannot convert (`If`, `GraphicEQ`, `Convolution`, `VST`, …) are returned as warnings and never dropped silently. |
| `engine` | Compiles the parsed stages into RBJ biquads, gains, delays and a mixing matrix. `process_interleaved` does no allocation, locking or I/O, so it is safe to call from a real-time audio callback. |
| `watermark` | Frequency-signature watermark: a carrier (for example 16 kHz at amplitude 0.0008) keyed with a code derived from the Signature ID, or an uploaded clip. It has a time window, a burst interval and signal gating, and `detect()` traces the ID in a recording. |
| `camilla` | Exports CamillaDSP v3 YAML: one file for all devices, plus one per `Device:` block. |

```bash
cargo test
cargo run --example eqapo2camilla -- config.txt
```

This crate is not yet wired into the app. Wiring it to the native APO (`windows-apo`) and the existing `dsp-core` is the next step.

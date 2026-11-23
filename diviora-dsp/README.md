# diviora-dsp

Shared DSP logic for Diviora. It is pure Rust with no OS dependencies, so the Tauri app and the native APO path can use the same code.

| Module | What it does |
|---|---|
| `eqapo` | Parses Equalizer APO 1.4.x `config.txt` and include files: `Preamp`, `Filter` (PK, LS/LSC, HS/HSC incl. `6dB` first-order and `12dB`, LP/LPQ, HP/HPQ, BP, NO, AP, raw `IIR Order n Coefficients`; `Q`, `BW Oct`, shelf slope), `GraphicEQ`, `Convolution` (impulse response uploaded with the config), `Channel` (up to 32 channels), `Copy`, `Delay`, `Device`, `Include`, `Stage`, `If`/`ElseIf`/`Else`/`EndIf`, `Eval` and `` `expressions` `` — evaluated for the real sample rate, channel count and device, like Equalizer APO. Only `VST` and `LoudnessCorrection` remain unsupported; they are reported as warnings, never dropped silently. |
| `expr` | The expression language of `If`/`Eval`/backticks (variables `sampleRate`, `channelCount`, `deviceName`, …; operators, `regexSearch`, maths functions). |
| `conv` | GraphicEQ FIR design (linear phase, like Equalizer APO) and partitioned FFT convolution for GraphicEQ and Convolution. |
| `engine` | Compiles the parsed stages into RBJ biquads, gains, delays and a mixing matrix. `process_interleaved` does no allocation, locking or I/O, so it is safe to call from a real-time audio callback. |
| `watermark` | Frequency-signature watermark: a carrier (for example 16 kHz at amplitude 0.0008) keyed with a code derived from the Signature ID, or an uploaded clip. It has a time window, a burst interval and signal gating, and `detect()` traces the ID in a recording. |
| `camilla` | Exports CamillaDSP v3 YAML: one file for all devices, plus one per `Device:` block. |

```bash
cargo test
cargo run --example eqapo2camilla -- config.txt
```

Used by the native APO (`windows-apo`, through the C interface in `ffi.rs`), the desktop and Android app (in-app processing, export, analysis) and the watermark detection.

//! Measured "Estimated properties" for the developer panel (like Equalizer APO's
//! Configuration Editor), and offline processing of whole files.

use crate::engine::{Coefs, Engine};
use crate::eqapo::{self, Stage};
use std::time::Instant;

#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    /// Highest gain of the filter chain over 10 Hz – 20 kHz, any channel (dB).
    pub peak_gain_db: f64,
    /// Frequency where that peak occurs.
    pub peak_freq_hz: f64,
    /// Added latency. IIR biquads, gains and the mixer add none; Delay stages add their delay.
    pub latency_ms: f64,
    /// Measured time to compile the configuration into an engine.
    pub init_ms: f64,
    /// Measured CPU load of processing at `sample_rate`, as % of one core.
    pub cpu_percent: f64,
    pub stages: usize,
    pub warnings: Vec<String>,
}

/// Analyse configuration text for a given output format.
pub fn analyze(text: &str, sample_rate: u32, channels: usize, device: Option<&str>) -> Analysis {
    analyze_with(text, sample_rate, channels, device, None, 400)
}

/// Like [`analyze`], with the developer panel's analysis options: `only_channel` limits the
/// peak-gain search to one channel (0 = L, 1 = R, ...; None = all), and `points` is the number
/// of frequency points evaluated between 10 Hz and 20 kHz (the "Resolution", clamped to
/// 64..=65536).
pub fn analyze_with(text: &str, sample_rate: u32, channels: usize, device: Option<&str>,
                    only_channel: Option<usize>, points: usize) -> Analysis {
    let ctx = eqapo::Context { sample_rate, channels, device_name: device.unwrap_or("").to_string(), ..Default::default() };
    let cfg = eqapo::parse_with(text, None, &ctx);
    let stages = match device {
        Some(d) if !d.is_empty() => cfg.stages_for_device(d),
        _ => cfg.stages_for_all_devices(),
    };
    let sr = sample_rate.max(8000) as f64;
    let ch = channels.clamp(1, 8);

    // Peak gain: sum of per-channel stage responses on a log frequency grid.
    let (mut peak, mut peak_f) = (f64::NEG_INFINITY, 1000.0);
    let points = points.clamp(64, 65536);
    let chans: Vec<usize> = match only_channel {
        Some(c) if c < ch => vec![c],
        _ => (0..ch).collect(),
    };
    for i in 0..points {
        let f = 10.0 * (20000.0f64 / 10.0).powf(i as f64 / (points - 1) as f64);
        if f >= sr * 0.49 {
            break;
        }
        for &c in &chans {
            let mut db = 0.0;
            for s in &stages {
                match s {
                    Stage::Gain { gain_db, channels } if channels.contains(c) => db += gain_db,
                    Stage::Biquad { spec, channels } if channels.contains(c) => {
                        db += Coefs::from_spec(spec, sr).magnitude_db(f, sr)
                    }
                    Stage::Iir { b, a, channels } if channels.contains(c) => {
                        db += crate::engine::iir_magnitude_db(b, a, f, sr)
                    }
                    Stage::GraphicEq { points, channels } if channels.contains(c) => {
                        db += eqapo::graphic_eq_gain(points, f)
                    }
                    Stage::Convolution { ir, channels } if channels.contains(c) => {
                        let sel = channels.indices(ch);
                        let j = sel.iter().position(|s| *s == c).unwrap_or(0);
                        if let Some(h) = ir.channels.get(j % ir.channels.len().max(1)) {
                            db += ir_magnitude_db(h, f, ir.sample_rate as f64);
                        }
                    }
                    _ => {}
                }
            }
            if db > peak {
                peak = db;
                peak_f = f;
            }
        }
    }
    if !peak.is_finite() {
        peak = 0.0;
    }
    let t0 = Instant::now();
    let mut engine = Engine::compile(&stages, sample_rate, ch, 0);
    let init_ms = t0.elapsed().as_secs_f64() * 1000.0;
    // Delay commands plus the latency of GraphicEQ/Convolution stages.
    let latency_ms = stages
        .iter()
        .map(|s| if let Stage::Delay { ms, .. } = s { *ms } else { 0.0 })
        .fold(0.0f64, f64::max)
        + engine.latency_samples() as f64 * 1000.0 / sr;

    // CPU: process one second of audio in 10 ms blocks and time it.
    let block = (sample_rate as usize / 100).max(1);
    let mut buf: Vec<f32> = (0..block * ch).map(|i| ((i as f32) * 0.618).sin() * 0.25).collect();
    let t1 = Instant::now();
    for _ in 0..100 {
        engine.process_interleaved(&mut buf);
    }
    let cpu_percent = t1.elapsed().as_secs_f64() * 100.0; // 1 s of audio -> seconds * 100 = %

    Analysis {
        peak_gain_db: peak,
        peak_freq_hz: peak_f,
        latency_ms,
        init_ms,
        cpu_percent,
        stages: stages.len(),
        warnings: cfg.warnings.iter().map(|w| w.to_string()).collect(),
    }
}

/// Magnitude (dB) of an impulse response at `f` (direct DFT; long IRs are truncated).
fn ir_magnitude_db(h: &[f32], f: f64, sr: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * f / sr;
    let (mut re, mut im) = (0.0, 0.0);
    for (k, v) in h.iter().take(16384).enumerate() {
        re += *v as f64 * (w * k as f64).cos();
        im -= *v as f64 * (w * k as f64).sin();
    }
    10.0 * (re * re + im * im).max(1e-30).log10()
}

/// Process interleaved samples of a whole file with a configuration (EQ + watermark).
pub fn process_file_samples(text: &str, samples: &mut [f32], sample_rate: u32, channels: usize) -> usize {
    let ctx = eqapo::Context { sample_rate, channels, ..Default::default() };
    let cfg = eqapo::parse_with(text, None, &ctx);
    let stages = cfg.stages_for_all_devices();
    let mut engine = Engine::compile(&stages, sample_rate, channels.clamp(1, 8), 0);
    if channels <= 8 {
        engine.process_interleaved(samples);
    }
    stages.len()
}

/// 32-bit float WAV file bytes.
pub fn wav_f32(samples: &[f32], sample_rate: u32, channels: u16) -> Vec<u8> {
    let data_len = (samples.len() * 4) as u32;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
    v.extend_from_slice(&channels.to_le_bytes());
    v.extend_from_slice(&sample_rate.to_le_bytes());
    v.extend_from_slice(&(sample_rate * channels as u32 * 4).to_le_bytes());
    v.extend_from_slice(&(channels * 4).to_le_bytes());
    v.extend_from_slice(&32u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_reports_real_values() {
        let a = analyze("Preamp: -6 dB\nFilter: ON PK Fc 1000 Hz Gain 4 dB Q 1\n", 48000, 2, None);
        assert!((a.peak_gain_db - (-2.0)).abs() < 0.05, "{:?}", a);
        assert!((a.peak_freq_hz - 1000.0).abs() < 60.0);
        assert_eq!(a.latency_ms, 0.0);
        assert!(a.cpu_percent >= 0.0 && a.cpu_percent < 50.0);
        assert_eq!(a.stages, 2);
        let d = analyze("Delay: 12 ms", 48000, 2, None);
        assert_eq!(d.latency_ms, 12.0);
        assert_eq!(analyze("", 48000, 2, None).peak_gain_db, 0.0);
    }

    #[test]
    fn wav_header_and_file_processing() {
        let mut s = vec![0.5f32; 960];
        assert_eq!(process_file_samples("Preamp: -6.0206 dB", &mut s, 48000, 2), 1);
        assert!((s[0] - 0.25).abs() < 1e-4);
        let w = wav_f32(&s, 48000, 2);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes([w[20], w[21]]), 3);
        assert_eq!(w.len(), 44 + 960 * 4);
    }
}

#[cfg(test)]
mod analyze_with_tests {
    use super::*;

    #[test]
    fn channel_and_resolution_options() {
        let cfg = "Channel: L\nFilter: ON PK Fc 1000 Hz Gain 9 dB Q 1\nChannel: R\nFilter: ON PK Fc 1000 Hz Gain 3 dB Q 1\n";
        let all = analyze_with(cfg, 48000, 2, None, None, 4000);
        let right = analyze_with(cfg, 48000, 2, None, Some(1), 4000);
        assert!((all.peak_gain_db - 9.0).abs() < 0.1, "{}", all.peak_gain_db);
        assert!((right.peak_gain_db - 3.0).abs() < 0.1, "{}", right.peak_gain_db);
        assert_eq!(analyze(cfg, 48000, 2, None).stages, all.stages);
    }
}

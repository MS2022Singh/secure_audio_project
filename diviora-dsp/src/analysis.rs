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

/// What a configuration audibly does, measured by running test noise through the compiled
/// engine (privacy-safe: reports only these numbers, never the configuration).
#[derive(Debug, Clone)]
pub struct Effect {
    /// Output level relative to input, per output channel (dB).
    pub level_change_db: Vec<f64>,
    /// Largest difference in gain between frequency bands (50 Hz – 16 kHz), any channel (dB).
    pub tonal_range_db: f64,
    /// Correlation of the first two output channels for independent inputs (0 = none,
    /// 1 = identical): shows crossfeed / channel mixing.
    pub channel_mixing: f64,
    pub stages: usize,
}

pub fn measure_effect(text: &str, sample_rate: u32, channels: usize, device: Option<&str>) -> Effect {
    use realfft::RealFftPlanner;
    let ch = channels.clamp(1, eqapo::MAX_CHANNELS);
    let ctx = eqapo::Context { sample_rate, channels: ch, device_name: device.unwrap_or("").to_string(), ..Default::default() };
    let cfg = eqapo::parse_with(text, None, &ctx);
    let stages = match device {
        Some(d) if !d.is_empty() => cfg.stages_for_device(d),
        _ => cfg.stages_for_all_devices(),
    };
    let mut engine = Engine::compile(&stages, sample_rate, ch, 0);
    // 4 s of independent white noise per channel (deterministic), at -20 dBFS RMS.
    let frames = sample_rate as usize * 4;
    let mut seed: u64 = 0x9E3779B97F4A7C15;
    let mut rnd = || { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; ((seed >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0 };
    let input: Vec<f32> = (0..frames * ch).map(|_| (rnd() * 0.1 * 3f64.sqrt()) as f32).collect();
    let mut out = input.clone();
    engine.process_interleaved(&mut out);
    // Skip the first second (delays, filters and FIR latency settle).
    let skip = sample_rate as usize;
    let chan = |buf: &[f32], c: usize| -> Vec<f64> { buf.iter().skip(skip * ch + c).step_by(ch).map(|v| *v as f64).collect() };
    let rms = |v: &[f64]| (v.iter().map(|x| x * x).sum::<f64>() / v.len().max(1) as f64).sqrt();
    let n = 8192usize;
    let fft = RealFftPlanner::<f64>::new().plan_fft_forward(n);
    let bands: Vec<(f64, f64)> = (0..20).map(|i| {
        let lo = 50.0 * (16000.0f64 / 50.0).powf(i as f64 / 20.0);
        let hi = 50.0 * (16000.0f64 / 50.0).powf((i + 1) as f64 / 20.0);
        (lo, hi)
    }).collect();
    let band_energy = |x: &[f64]| -> Vec<f64> {
        let mut e = vec![1e-30; bands.len()];
        let mut buf = vec![0.0f64; n];
        let mut spec = fft.make_output_vec();
        for seg in x.chunks_exact(n) {
            for (k, (b, v)) in buf.iter_mut().zip(seg).enumerate() {
                let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * k as f64 / n as f64).cos();
                *b = v * w;
            }
            let _ = fft.process(&mut buf, &mut spec);
            for (bi, (lo, hi)) in bands.iter().enumerate() {
                let (a, b) = ((lo * n as f64 / sample_rate as f64) as usize, (hi * n as f64 / sample_rate as f64) as usize);
                for s in spec.iter().take(b.min(n / 2) + 1).skip(a.max(1)) { e[bi] += s.norm_sqr(); }
            }
        }
        e
    };
    let mut level = Vec::new();
    let mut tonal: f64 = 0.0;
    for c in 0..ch {
        let (i, o) = (chan(&input, c), chan(&out, c));
        level.push(20.0 * (rms(&o).max(1e-12) / rms(&i).max(1e-12)).log10());
        let (ei, eo) = (band_energy(&i), band_energy(&o));
        let gains: Vec<f64> = ei.iter().zip(&eo).map(|(a, b)| 10.0 * (b / a).log10()).collect();
        let (mn, mx) = gains.iter().fold((f64::MAX, f64::MIN), |(a, b), g| (a.min(*g), b.max(*g)));
        if mx > mn { tonal = tonal.max(mx - mn); }
    }
    let mixing = if ch >= 2 {
        let (a, b) = (chan(&out, 0), chan(&out, 1));
        let (ma, mb) = (a.iter().sum::<f64>() / a.len() as f64, b.iter().sum::<f64>() / b.len() as f64);
        let cov: f64 = a.iter().zip(&b).map(|(x, y)| (x - ma) * (y - mb)).sum();
        let va: f64 = a.iter().map(|x| (x - ma).powi(2)).sum();
        let vb: f64 = b.iter().map(|y| (y - mb).powi(2)).sum();
        if va > 0.0 && vb > 0.0 { cov / (va * vb).sqrt() } else { 0.0 }
    } else { 0.0 };
    Effect { level_change_db: level, tonal_range_db: tonal, channel_mixing: mixing, stages: stages.len() }
}

#[cfg(test)]
mod effect_tests {
    use super::*;

    #[test]
    fn measures_level_tone_and_mixing() {
        let none = measure_effect("", 48000, 2, None);
        assert!(none.level_change_db.iter().all(|d| d.abs() < 0.01) && none.tonal_range_db < 0.5 && none.channel_mixing.abs() < 0.05);
        let e = measure_effect("Preamp: -6 dB\nFilter: ON PK Fc 1000 Hz Gain 9 dB Q 1\nCopy: L=0.7*L+0.3*R R=0.7*R+0.3*L", 48000, 2, None);
        assert!(e.tonal_range_db > 6.0, "{:?}", e);
        assert!(e.channel_mixing > 0.5, "{:?}", e);
    }
}

/// Deterministic test sound for comparisons: independent white noise per channel, -20 dBFS RMS.
pub fn test_noise(sample_rate: u32, channels: usize, seconds: f64) -> Vec<f32> {
    let frames = (sample_rate as f64 * seconds) as usize;
    let mut seed: u64 = 0x9E3779B97F4A7C15;
    let mut rnd = || { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; ((seed >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0 };
    (0..frames * channels).map(|_| (rnd() * 0.1 * 3f64.sqrt()) as f32).collect()
}

/// Gain (dB) of `out` relative to `inp` in 20 log-spaced bands from 50 Hz to 16 kHz.
pub fn band_gains(inp: &[f64], out: &[f64], sample_rate: u32) -> Vec<(f64, f64)> {
    use realfft::RealFftPlanner;
    let n = 8192usize;
    let fft = RealFftPlanner::<f64>::new().plan_fft_forward(n);
    let bands: Vec<(f64, f64)> = (0..20).map(|i| {
        let lo = 50.0 * (16000.0f64 / 50.0).powf(i as f64 / 20.0);
        let hi = 50.0 * (16000.0f64 / 50.0).powf((i + 1) as f64 / 20.0);
        (lo, hi)
    }).collect();
    let energy = |x: &[f64]| -> Vec<f64> {
        let mut e = vec![1e-30; bands.len()];
        let mut buf = vec![0.0f64; n];
        let mut spec = fft.make_output_vec();
        for seg in x.chunks_exact(n) {
            for (k, (b, v)) in buf.iter_mut().zip(seg).enumerate() {
                *b = v * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * k as f64 / n as f64).cos());
            }
            let _ = fft.process(&mut buf, &mut spec);
            for (bi, (lo, hi)) in bands.iter().enumerate() {
                let (a, b) = ((lo * n as f64 / sample_rate as f64) as usize, (hi * n as f64 / sample_rate as f64) as usize);
                for s in spec.iter().take(b.min(n / 2) + 1).skip(a.max(1)) { e[bi] += s.norm_sqr(); }
            }
        }
        e
    };
    let (ei, eo) = (energy(inp), energy(out));
    bands.iter().zip(ei.iter().zip(eo.iter()))
        .map(|((lo, hi), (a, b))| ((lo * hi).sqrt(), 10.0 * (b / a).log10()))
        .collect()
}

/// One channel of an interleaved buffer, skipping the first `skip` frames.
pub fn channel(buf: &[f32], ch: usize, c: usize, skip: usize) -> Vec<f64> {
    buf.iter().skip(skip * ch + c).step_by(ch).map(|v| *v as f64).collect()
}

/// Comparison of two processed versions (A = reference, B = candidate) of the same input.
#[derive(Debug, Clone)]
pub struct Comparison {
    pub level_a_db: Vec<f64>,
    pub level_b_db: Vec<f64>,
    /// Per channel: (band centre Hz, gain A dB, gain B dB).
    pub bands: Vec<Vec<(f64, f64, f64)>>,
    /// Largest band-gain difference, any channel (dB).
    pub max_band_diff_db: f64,
    /// Difference between the waveforms relative to A (dB; below -40 = practically identical),
    /// after aligning B to A by `lag_frames`.
    pub residual_db: Vec<f64>,
    pub lag_frames: i64,
    pub mixing_a: f64,
    pub mixing_b: f64,
}

pub fn compare(input: &[f32], a: &[f32], b: &[f32], sample_rate: u32, ch: usize) -> Comparison {
    let ch = ch.max(1);
    let skip = (sample_rate as usize / 2).min(input.len() / ch / 4);
    let rms = |v: &[f64]| (v.iter().map(|x| x * x).sum::<f64>() / v.len().max(1) as f64).sqrt();
    let corr = |x: &[f64], y: &[f64]| {
        let n = x.len().min(y.len());
        let (mx, my) = (x[..n].iter().sum::<f64>() / n.max(1) as f64, y[..n].iter().sum::<f64>() / n.max(1) as f64);
        let cov: f64 = (0..n).map(|i| (x[i] - mx) * (y[i] - my)).sum();
        let vx: f64 = (0..n).map(|i| (x[i] - mx).powi(2)).sum();
        let vy: f64 = (0..n).map(|i| (y[i] - my).powi(2)).sum();
        if vx > 0.0 && vy > 0.0 { cov / (vx * vy).sqrt() } else { 0.0 }
    };
    // Align B to A (latency differences, e.g. linear-phase filters): best lag on channel 0.
    let a0 = channel(a, ch, 0, skip);
    let b0 = channel(b, ch, 0, skip);
    let probe = a0.len().min(b0.len()).min(sample_rate as usize).saturating_sub(4096);
    let mut best = (0i64, f64::MIN);
    for lag in -4096i64..=4096 {
        let mut s = 0.0;
        for i in (4096..probe).step_by(4) {
            let j = i as i64 + lag;
            if j >= 0 && (j as usize) < b0.len() { s += a0[i] * b0[j as usize]; }
        }
        if s > best.1 { best = (lag, s); }
    }
    let lag = best.0;
    let mut out = Comparison { level_a_db: vec![], level_b_db: vec![], bands: vec![], max_band_diff_db: 0.0,
        residual_db: vec![], lag_frames: lag, mixing_a: 0.0, mixing_b: 0.0 };
    for c in 0..ch {
        let i = channel(input, ch, c, skip);
        let (ac, bc) = (channel(a, ch, c, skip), channel(b, ch, c, skip));
        out.level_a_db.push(20.0 * (rms(&ac).max(1e-12) / rms(&i).max(1e-12)).log10());
        out.level_b_db.push(20.0 * (rms(&bc).max(1e-12) / rms(&i).max(1e-12)).log10());
        let ga = band_gains(&i, &ac, sample_rate);
        let gb = band_gains(&i, &bc, sample_rate);
        let rows: Vec<(f64, f64, f64)> = ga.iter().zip(gb.iter()).map(|((f, x), (_, y))| (*f, *x, *y)).collect();
        for (_, x, y) in &rows { out.max_band_diff_db = out.max_band_diff_db.max((x - y).abs()); }
        out.bands.push(rows);
        // Residual after alignment.
        let n = ac.len().min(bc.len());
        let (mut e, mut r) = (0.0, 0.0);
        for k in 0..n {
            let j = k as i64 + lag;
            if j < 0 || j as usize >= bc.len() { continue; }
            let d = ac[k] - bc[j as usize];
            r += d * d;
            e += ac[k] * ac[k];
        }
        out.residual_db.push(if e > 0.0 { 10.0 * (r.max(1e-30) / e).log10() } else { -120.0 });
    }
    if ch >= 2 {
        out.mixing_a = corr(&channel(a, ch, 0, skip), &channel(a, ch, 1, skip));
        out.mixing_b = corr(&channel(b, ch, 0, skip), &channel(b, ch, 1, skip));
    }
    out
}

#[cfg(test)]
mod compare_tests {
    use super::*;

    #[test]
    fn identical_and_different_processing() {
        let inp = test_noise(48000, 2, 3.0);
        let run = |cfg: &str| {
            let c = eqapo::parse(cfg, None);
            let mut e = Engine::compile(&c.stages_for_all_devices(), 48000, 2, 0);
            let mut o = inp.clone();
            e.process_interleaved(&mut o);
            o
        };
        let a = run("Preamp: -3 dB\nFilter: ON PK Fc 1000 Hz Gain 6 dB Q 1");
        let same = compare(&inp, &a, &a.clone(), 48000, 2);
        assert!(same.residual_db.iter().all(|r| *r < -100.0) && same.max_band_diff_db < 0.01, "{:?}", same.residual_db);
        let b = run("Preamp: -3 dB");
        let diff = compare(&inp, &a, &b, 48000, 2);
        assert!(diff.max_band_diff_db > 4.0, "{}", diff.max_band_diff_db);
        // A pure delay is recognised as a time offset, not as a difference.
        let d = run("Preamp: -3 dB\nFilter: ON PK Fc 1000 Hz Gain 6 dB Q 1\nDelay: 100 samples");
        let lagged = compare(&inp, &a, &d, 48000, 2);
        assert_eq!(lagged.lag_frames, 100);
        assert!(lagged.residual_db.iter().all(|r| *r < -60.0), "{:?}", lagged.residual_db);
    }
}

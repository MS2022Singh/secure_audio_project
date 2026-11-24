//! Real-time DSP engine compiled from parsed Equalizer APO stages.
//!
//! `Engine::compile` allocates everything up front; `Engine::process_interleaved`
//! does not allocate, lock or perform I/O, so it is safe to call from an audio callback.

use crate::conv::{graphic_eq_fir, FirStage};
use crate::eqapo::{BiquadSpec, ChannelSel, FilterKind, Shape, Stage, MAX_CHANNELS};
use crate::watermark::Watermarker;
use std::f64::consts::PI;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coefs {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Coefs {
    /// RBJ "Audio EQ Cookbook" coefficients, normalised so a0 == 1.
    pub fn from_spec(spec: &BiquadSpec, sample_rate: f64) -> Coefs {
        // Same formulas as Equalizer APO 1.4 (filters/BiQuad.cpp, filters/BiQuadFilter.cpp).
        let is_shelf = matches!(spec.kind, FilterKind::LowShelf | FilterKind::HighShelf);
        let a = if matches!(spec.kind, FilterKind::Peaking | FilterKind::LowShelf | FilterKind::HighShelf) {
            10f64.powf(spec.gain_db / 40.0)
        } else {
            10f64.powf(spec.gain_db / 20.0)
        };
        let mut f = spec.freq;
        if spec.corner && is_shelf {
            // Corner-frequency shelves (LS/HS): Equalizer APO's DCX2496 adjustment.
            let s = match spec.shape {
                Shape::SlopeDbPerOct(db) => db / 12.0,
                Shape::Q(q) => 1.0 / ((1.0 / (q * q) - 2.0) / (a + 1.0 / a) + 1.0),
                Shape::BwOct(_) => 0.9,
            };
            let factor = 10f64.powf(spec.gain_db.abs() / 80.0 / s);
            if spec.kind == FilterKind::LowShelf { f *= factor } else { f /= factor }
        }
        let f = f.min(sample_rate * 0.49);
        let w0 = 2.0 * PI * f / sample_rate;
        let (sn, cs) = w0.sin_cos();
        let alpha = match spec.shape {
            Shape::Q(q) => sn / (2.0 * q),
            Shape::SlopeDbPerOct(db) if is_shelf => {
                let s = (db / 12.0).max(1e-9);
                sn / 2.0 * ((a + 1.0 / a) * (1.0 / s - 1.0) + 2.0).max(0.0).sqrt()
            }
            Shape::SlopeDbPerOct(_) => sn / (2.0 * std::f64::consts::FRAC_1_SQRT_2),
            Shape::BwOct(bw) => sn * (std::f64::consts::LN_2 / 2.0 * bw * w0 / sn).sinh(),
        };
        let sa = 2.0 * a.sqrt() * alpha;
        // First-order low/high pass (bilinear transform).
        if matches!(spec.kind, FilterKind::LowPass1 | FilterKind::HighPass1) {
            let t = (PI * f / sample_rate).tan();
            let (b0, b1) = if spec.kind == FilterKind::LowPass1 { (t, t) } else { (1.0, -1.0) };
            let (a0, a1) = (1.0 + t, t - 1.0);
            return Coefs { b0: b0 / a0, b1: b1 / a0, b2: 0.0, a1: a1 / a0, a2: 0.0 };
        }
        // First-order shelves (bilinear transform, transition centred on Fc).
        if matches!(spec.kind, FilterKind::LowShelf1 | FilterKind::HighShelf1) {
            let t = (PI * f / sample_rate).tan();
            let g = 10f64.powf(spec.gain_db / 20.0).sqrt(); // sqrt of the linear shelf gain
            let (b0, b1, a0, a1) = if spec.kind == FilterKind::LowShelf1 {
                (1.0 + g * t, g * t - 1.0, 1.0 + t / g, t / g - 1.0)
            } else {
                let gg = g * g;
                (gg * (1.0 + t / g), gg * (t / g - 1.0), 1.0 + g * t, g * t - 1.0)
            };
            return Coefs { b0: b0 / a0, b1: b1 / a0, b2: 0.0, a1: a1 / a0, a2: 0.0 };
        }
        let (b0, b1, b2, a0, a1, a2) = match spec.kind {
            FilterKind::Peaking => (1.0 + alpha * a, -2.0 * cs, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cs, 1.0 - alpha / a),
            FilterKind::LowShelf => (
                a * ((a + 1.0) - (a - 1.0) * cs + sa),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cs),
                a * ((a + 1.0) - (a - 1.0) * cs - sa),
                (a + 1.0) + (a - 1.0) * cs + sa,
                -2.0 * ((a - 1.0) + (a + 1.0) * cs),
                (a + 1.0) + (a - 1.0) * cs - sa,
            ),
            FilterKind::HighShelf => (
                a * ((a + 1.0) + (a - 1.0) * cs + sa),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cs),
                a * ((a + 1.0) + (a - 1.0) * cs - sa),
                (a + 1.0) - (a - 1.0) * cs + sa,
                2.0 * ((a - 1.0) - (a + 1.0) * cs),
                (a + 1.0) - (a - 1.0) * cs - sa,
            ),
            FilterKind::LowPass => ((1.0 - cs) / 2.0, 1.0 - cs, (1.0 - cs) / 2.0, 1.0 + alpha, -2.0 * cs, 1.0 - alpha),
            FilterKind::HighPass => ((1.0 + cs) / 2.0, -(1.0 + cs), (1.0 + cs) / 2.0, 1.0 + alpha, -2.0 * cs, 1.0 - alpha),
            FilterKind::BandPass => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cs, 1.0 - alpha),
            FilterKind::Notch => (1.0, -2.0 * cs, 1.0, 1.0 + alpha, -2.0 * cs, 1.0 - alpha),
            FilterKind::AllPass => (1.0 - alpha, -2.0 * cs, 1.0 + alpha, 1.0 + alpha, -2.0 * cs, 1.0 - alpha),
            FilterKind::LowShelf1 | FilterKind::HighShelf1 | FilterKind::LowPass1 | FilterKind::HighPass1 => unreachable!(),
        };
        Coefs { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
    }

    /// Magnitude response in dB at `freq` (used by tests and UI previews).
    pub fn magnitude_db(&self, freq: f64, sample_rate: f64) -> f64 {
        let w = 2.0 * PI * freq / sample_rate;
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = -(self.b1 * s1 + self.b2 * s2);
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = -(self.a1 * s1 + self.a2 * s2);
        10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).log10()
    }
}

/// Magnitude in dB of a general IIR (b, a with a[0] == 1) at `freq`.
pub fn iir_magnitude_db(b: &[f64], a: &[f64], freq: f64, sample_rate: f64) -> f64 {
    let w = 2.0 * PI * freq / sample_rate;
    let eval = |c: &[f64]| {
        let (mut re, mut im) = (0.0, 0.0);
        for (k, v) in c.iter().enumerate() {
            re += v * (w * k as f64).cos();
            im -= v * (w * k as f64).sin();
        }
        re * re + im * im
    };
    10.0 * (eval(b) / eval(a).max(1e-300)).log10()
}

enum CStage {
    Gain { lin: [f32; MAX_CHANNELS] },
    Biquad { c: Coefs, on: [bool; MAX_CHANNELS], z: [[f64; 2]; MAX_CHANNELS] },
    Delay { lines: Vec<Option<(Vec<f32>, usize)>> },
    Mix { m: Box<[[f32; MAX_CHANNELS]; MAX_CHANNELS]> },
    Watermark(Box<Watermarker>),
    /// General IIR, transposed direct form II; `z[c]` has `order` state values.
    Iir { b: Vec<f64>, a: Vec<f64>, on: [bool; MAX_CHANNELS], z: Vec<Vec<f64>> },
    Fir(Box<FirStage>),
}

pub struct Engine {
    /// Channels of the stream (what the speakers get).
    channels: usize,
    /// Channels processed internally: the stream's plus virtual channels the config creates.
    width: usize,
    work: [f32; MAX_CHANNELS],
    sample_rate: u32,
    stages: Vec<CStage>,
    pub revision: u64,
}

/// Highest channel index a stage refers to, plus one.
fn referenced_width(stages: &[Stage]) -> usize {
    let mut w = 0;
    let mut see = |sel: &ChannelSel| if let ChannelSel::List(v) = sel { for c in v { w = w.max(c + 1); } };
    for s in stages {
        match s {
            Stage::Gain { channels, .. } | Stage::Biquad { channels, .. } | Stage::Delay { channels, .. }
            | Stage::Iir { channels, .. } | Stage::GraphicEq { channels, .. } | Stage::Convolution { channels, .. } => see(channels),
            Stage::Mix { routes } => {
                let mut v = Vec::new();
                for (d, srcs) in routes {
                    v.push(*d);
                    v.extend(srcs.iter().map(|(c, _)| *c));
                }
                see(&ChannelSel::List(v));
            }
            Stage::Watermark(_) => {}
        }
    }
    w
}

fn mask(sel: &ChannelSel, channels: usize) -> [bool; MAX_CHANNELS] {
    let mut m = [false; MAX_CHANNELS];
    for c in sel.indices(channels.min(MAX_CHANNELS)) {
        m[c] = true;
    }
    m
}

impl Engine {
    /// A pass-through engine.
    pub fn bypass(sample_rate: u32, channels: usize) -> Engine {
        let channels = channels.clamp(1, MAX_CHANNELS);
        Engine { channels, width: channels, work: [0.0; MAX_CHANNELS], sample_rate, stages: Vec::new(), revision: 0 }
    }

    pub fn compile(stages: &[Stage], sample_rate: u32, stream_channels: usize, revision: u64) -> Engine {
        let stream_channels = stream_channels.clamp(1, MAX_CHANNELS);
        // Everything below is built for the internal width (stream + virtual channels).
        let channels = stream_channels.max(referenced_width(stages)).min(MAX_CHANNELS);
        let sr = sample_rate as f64;
        let mut out = Vec::with_capacity(stages.len());
        for s in stages {
            match s {
                Stage::Gain { gain_db, channels: sel } => {
                    let g = 10f32.powf(*gain_db as f32 / 20.0);
                    let m = mask(sel, channels);
                    let mut lin = [1.0f32; MAX_CHANNELS];
                    for c in 0..MAX_CHANNELS {
                        if m[c] { lin[c] = g; }
                    }
                    out.push(CStage::Gain { lin });
                }
                Stage::Biquad { spec, channels: sel } => {
                    out.push(CStage::Biquad { c: Coefs::from_spec(spec, sr), on: mask(sel, channels), z: [[0.0; 2]; MAX_CHANNELS] });
                }
                Stage::Delay { ms, channels: sel } => {
                    let n = ((ms / 1000.0) * sr).round() as usize;
                    if n == 0 { continue; }
                    let m = mask(sel, channels);
                    let lines = (0..channels).map(|c| if m[c] { Some((vec![0.0f32; n], 0usize)) } else { None }).collect();
                    out.push(CStage::Delay { lines });
                }
                Stage::Mix { routes } => {
                    let mut m = Box::new([[0.0f32; MAX_CHANNELS]; MAX_CHANNELS]);
                    for (i, row) in m.iter_mut().enumerate() { row[i] = 1.0; }
                    for (dest, srcs) in routes {
                        if *dest >= MAX_CHANNELS { continue; }
                        m[*dest] = [0.0; MAX_CHANNELS];
                        for (src, f) in srcs {
                            if *src < MAX_CHANNELS { m[*dest][*src] += *f as f32; }
                        }
                    }
                    out.push(CStage::Mix { m });
                }
                Stage::Watermark(spec) => out.push(CStage::Watermark(Box::new(Watermarker::new(spec, sample_rate)))),
                Stage::Iir { b, a, channels: sel } => {
                    let order = b.len().max(a.len()) - 1;
                    let mut bb = b.clone();
                    let mut aa = a.clone();
                    bb.resize(order + 1, 0.0);
                    aa.resize(order + 1, 0.0);
                    out.push(CStage::Iir { b: bb, a: aa, on: mask(sel, channels), z: vec![vec![0.0; order.max(1)]; channels] });
                }
                Stage::GraphicEq { points, channels: sel } => {
                    let fir = Arc::new(graphic_eq_fir(points, sample_rate));
                    let group_delay = (fir.len() - 1) / 2;
                    let m = mask(sel, channels);
                    let firs = (0..channels).map(|c| if m[c] { Some(fir.clone()) } else { None }).collect();
                    out.push(CStage::Fir(Box::new(FirStage::new(firs, group_delay))));
                }
                Stage::Convolution { ir, channels: sel } => {
                    let irs: Vec<Arc<Vec<f32>>> = ir.channels.iter()
                        .map(|c| Arc::new(crate::wav::resample(c, ir.sample_rate, sample_rate)))
                        .collect();
                    if irs.is_empty() { continue; }
                    let selected = sel.indices(channels);
                    let firs = (0..channels)
                        .map(|c| selected.iter().position(|s| *s == c).map(|j| irs[j % irs.len()].clone()))
                        .collect();
                    out.push(CStage::Fir(Box::new(FirStage::new(firs, 0))));
                }
            }
        }
        Engine { channels: stream_channels, width: channels, work: [0.0; MAX_CHANNELS], sample_rate, stages: out, revision }
    }

    pub fn channels(&self) -> usize { self.channels }
    pub fn sample_rate(&self) -> u32 { self.sample_rate }
    pub fn is_bypass(&self) -> bool { self.stages.is_empty() }

    /// Added latency in samples (FIR stages: one block plus their group delay).
    pub fn latency_samples(&self) -> usize {
        self.stages.iter().map(|s| if let CStage::Fir(f) = s { f.latency } else { 0 }).sum()
    }

    /// Process interleaved samples in place. `buf.len()` must be a multiple of `channels()`.
    pub fn process_interleaved(&mut self, buf: &mut [f32]) {
        let n = self.channels;
        if self.stages.is_empty() {
            return;
        }
        for frame in buf.chunks_exact_mut(n) {
            self.process_frame(frame);
        }
    }

    #[inline]
    pub fn process_frame(&mut self, frame: &mut [f32]) {
        let n = frame.len().min(self.channels);
        if self.width == n {
            Self::run(&mut self.stages, &mut frame[..n], n);
        } else {
            // Virtual channels start silent each frame; Copy fills them.
            self.work[..n].copy_from_slice(&frame[..n]);
            for v in self.work[n..self.width].iter_mut() { *v = 0.0; }
            Self::run(&mut self.stages, &mut self.work[..self.width], n);
            frame[..n].copy_from_slice(&self.work[..n]);
        }
        // Hard safety clip so positive EQ gain can never wrap or blow speakers.
        for s in frame[..n].iter_mut() {
            *s = if s.is_finite() { s.clamp(-1.0, 1.0) } else { 0.0 };
        }
    }

    /// Run all stages over `frame` (stream + virtual channels); `out_n` = stream channels.
    #[inline]
    fn run(stages: &mut [CStage], frame: &mut [f32], out_n: usize) {
        let n = frame.len();
        for st in stages.iter_mut() {
            match st {
                CStage::Gain { lin } => {
                    for c in 0..n { frame[c] *= lin[c]; }
                }
                CStage::Biquad { c: k, on, z } => {
                    for c in 0..n {
                        if !on[c] { continue; }
                        // Transposed direct form II, f64 state.
                        let x = frame[c] as f64;
                        let y = k.b0 * x + z[c][0];
                        z[c][0] = k.b1 * x - k.a1 * y + z[c][1];
                        z[c][1] = k.b2 * x - k.a2 * y;
                        frame[c] = y as f32;
                    }
                }
                CStage::Delay { lines } => {
                    for c in 0..n {
                        if let Some(Some((line, pos))) = lines.get_mut(c) {
                            let out = line[*pos];
                            line[*pos] = frame[c];
                            *pos = (*pos + 1) % line.len();
                            frame[c] = out;
                        }
                    }
                }
                CStage::Watermark(w) => w.process_frame(&mut frame[..out_n]),
                CStage::Iir { b, a, on, z } => {
                    let order = b.len() - 1;
                    for c in 0..n {
                        if !on[c] { continue; }
                        let zc = &mut z[c];
                        let x = frame[c] as f64;
                        let y = b[0] * x + zc[0];
                        for k in 1..order {
                            zc[k - 1] = b[k] * x - a[k] * y + zc[k];
                        }
                        zc[order - 1] = b[order] * x - a[order] * y;
                        frame[c] = if y.is_finite() { y as f32 } else { zc.iter_mut().for_each(|v| *v = 0.0); 0.0 };
                    }
                }
                CStage::Fir(f) => f.process_frame(&mut frame[..n]),
                CStage::Mix { m } => {
                    let mut tmp = [0.0f32; MAX_CHANNELS];
                    tmp[..n].copy_from_slice(&frame[..n]);
                    for d in 0..n {
                        let mut acc = 0.0f32;
                        for s in 0..n { acc += m[d][s] * tmp[s]; }
                        frame[d] = acc;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eqapo::parse;

    fn spec(kind: FilterKind, freq: f64, gain: f64, q: f64) -> BiquadSpec {
        BiquadSpec { kind, freq, gain_db: gain, shape: Shape::Q(q), corner: false }
    }

    #[test]
    fn peaking_hits_target_gain() {
        let c = Coefs::from_spec(&spec(FilterKind::Peaking, 1000.0, -24.0, 1.41), 48000.0);
        assert!((c.magnitude_db(1000.0, 48000.0) + 24.0).abs() < 0.01);
        assert!(c.magnitude_db(100.0, 48000.0).abs() < 0.5);
        assert!(c.magnitude_db(10000.0, 48000.0).abs() < 0.5);
    }

    #[test]
    fn shelves_and_passes() {
        let ls = Coefs::from_spec(&spec(FilterKind::LowShelf, 100.0, 6.0, 0.707), 48000.0);
        assert!((ls.magnitude_db(20.0, 48000.0) - 6.0).abs() < 0.3);
        assert!(ls.magnitude_db(5000.0, 48000.0).abs() < 0.1);
        let hp = Coefs::from_spec(&spec(FilterKind::HighPass, 1000.0, 0.0, 0.707), 48000.0);
        assert!((hp.magnitude_db(1000.0, 48000.0) + 3.01).abs() < 0.05);
        assert!(hp.magnitude_db(100.0, 48000.0) < -35.0);
    }

    fn sine_rms_db(engine: &mut Engine, freq: f64) -> f64 {
        let sr = 48000.0;
        let mut buf: Vec<f32> = (0..48000).flat_map(|i| {
            let v = (0.5 * (2.0 * PI * freq * i as f64 / sr).sin()) as f32;
            [v, v]
        }).collect();
        engine.process_interleaved(&mut buf);
        let tail = &buf[48000..]; // skip transient
        let rms = (tail.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / tail.len() as f64).sqrt();
        20.0 * (rms / (0.5 / 2f64.sqrt())).log10()
    }

    #[test]
    fn acceptance_preamp_minus_24_and_1k_notch() {
        // The acceptance test from the design discussion: Preamp -24 dB, then a 1 kHz -24 dB peak.
        let cfg = parse("Preamp: -24 dB", None);
        let mut e = Engine::compile(&cfg.stages_for_all_devices(), 48000, 2, 1);
        assert!((sine_rms_db(&mut e, 440.0) + 24.0).abs() < 0.05);

        let cfg = parse("Filter: ON PK Fc 1000 Hz Gain -24 dB Q 2", None);
        let mut e = Engine::compile(&cfg.stages_for_all_devices(), 48000, 2, 2);
        assert!((sine_rms_db(&mut e, 1000.0) + 24.0).abs() < 0.2);
        let mut e = Engine::compile(&cfg.stages_for_all_devices(), 48000, 2, 2);
        assert!(sine_rms_db(&mut e, 100.0).abs() < 0.2);
    }

    #[test]
    fn mix_swaps_channels_and_delay_shifts() {
        let cfg = parse("Copy: L=R R=L", None);
        let mut e = Engine::compile(&cfg.stages_for_all_devices(), 48000, 2, 1);
        let mut f = [0.25f32, -0.5];
        e.process_frame(&mut f);
        assert_eq!(f, [-0.5, 0.25]);

        let cfg = parse("Channel: R\nDelay: 2 samples", None);
        let mut e = Engine::compile(&cfg.stages_for_all_devices(), 48000, 2, 1);
        let mut buf = [1.0f32, 1.0, 0.0, 0.0, 0.0, 0.0];
        e.process_interleaved(&mut buf);
        assert_eq!(buf, [1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn iir_first_order_shelf_graphic_eq_and_conditions() {
        // Raw IIR equal to a known biquad.
        let spec = BiquadSpec { kind: FilterKind::Peaking, freq: 1000.0, gain_db: -12.0, shape: Shape::Q(1.0), corner: false };
        let c = Coefs::from_spec(&spec, 48000.0);
        let text = format!("Filter: ON IIR Order 2 Coefficients {} {} {} 1 {} {}", c.b0, c.b1, c.b2, c.a1, c.a2);
        let cfg = parse(&text, None);
        assert!(cfg.warnings.is_empty(), "{:?}", cfg.warnings);
        let mut e = Engine::compile(&cfg.stages_for_all_devices(), 48000, 2, 1);
        assert!((sine_rms_db(&mut e, 1000.0) + 12.0).abs() < 0.2);

        // First-order low shelf: full gain far below Fc, half (in dB) at Fc.
        let s1 = BiquadSpec { kind: FilterKind::LowShelf1, freq: 200.0, gain_db: 6.0, shape: Shape::Q(0.7), corner: false };
        let k = Coefs::from_spec(&s1, 48000.0);
        assert!((k.magnitude_db(5.0, 48000.0) - 6.0).abs() < 0.1);
        assert!((k.magnitude_db(200.0, 48000.0) - 3.0).abs() < 0.1);
        assert!(k.magnitude_db(20000.0, 48000.0).abs() < 0.1);
        let cfg = parse("Filter: ON LS 6dB Fc 200 Hz Gain 6 dB\nFilter: ON HS 6dB Fc 5000 Hz Gain -3 dB", None);
        assert!(cfg.warnings.is_empty(), "{:?}", cfg.warnings);

        // GraphicEQ through the engine (latency-compensated measurement).
        let cfg = parse("GraphicEQ: 20 -10; 20000 -10", None);
        let mut e = Engine::compile(&cfg.stages_for_all_devices(), 48000, 2, 1);
        assert!(e.latency_samples() > 4096);
        assert!((sine_rms_db(&mut e, 1000.0) + 10.0).abs() < 0.3);

        // If/Else picks the branch for the real sample rate.
        let text = "If: sampleRate == 44100\nPreamp: -1 dB\nElseIf: sampleRate == 48000\nPreamp: -2 dB\nElse:\nPreamp: -3 dB\nEndIf:";
        let ctx = crate::eqapo::Context { sample_rate: 48000, ..Default::default() };
        let cfg = crate::eqapo::parse_with(text, None, &ctx);
        assert!(cfg.warnings.is_empty(), "{:?}", cfg.warnings);
        assert_eq!(cfg.stages.len(), 1);
        assert_eq!(cfg.stages[0].1, Stage::Gain { gain_db: -2.0, channels: ChannelSel::List(vec![0, 1]) });
    }

    #[test]
    fn convolution_from_embedded_wav() {
        let ir = crate::analysis::wav_f32(&[0.5, 0.0, 0.0, 0.0], 48000, 1);
        let text = format!("{}\nConvolution: C:\\Users\\dev\\room.wav\n", crate::eqapo::embed_file_line("room.wav", &ir));
        let cfg = parse(&text, None);
        assert!(cfg.warnings.is_empty(), "{:?}", cfg.warnings);
        let mut e = Engine::compile(&cfg.stages_for_all_devices(), 48000, 2, 1);
        assert!((sine_rms_db(&mut e, 1000.0) + 6.02).abs() < 0.1);
    }

    #[test]
    fn virtual_channels_per_channel_delay_and_filter_channels() {
        // Mid channel CX = (L+R)/2, delayed 1 sample, mixed back into L only.
        let text = "DivioraExtensions: on\nCopy: CX=0.5*L+0.5*R\nDelay: CX=1 samples L=0ms\nFilter: ON NOF Fc 50 Hz Q 30 Channels=R\nCopy: L=L+CX\n";
        let cfg = parse(text, None);
        assert!(cfg.warnings.is_empty(), "{:?}", cfg.warnings);
        let mut e = Engine::compile(&cfg.stages_for_all_devices(), 48000, 2, 1);
        let mut f1 = [0.4f32, 0.2];
        e.process_frame(&mut f1);
        assert!((f1[0] - 0.4).abs() < 1e-6, "{:?}", f1); // CX delayed: nothing yet
        let mut f2 = [0.0f32, 0.0];
        e.process_frame(&mut f2);
        assert!((f2[0] - 0.3).abs() < 1e-6, "{:?}", f2); // previous mid (0.4+0.2)/2 arrives in L
        // Virtual channels never reach the output; a stereo stream stays stereo.
        assert_eq!(e.channels(), 2);
        // Notch on R only (50 Hz removed from R, L untouched).
        let cfg = parse("DivioraExtensions: on\nFilter: ON NOF Fc 1000 Hz Q 2 Channels=R", None);
        let mut e = Engine::compile(&cfg.stages_for_all_devices(), 48000, 2, 1);
        let sr = 48000.0;
        let mut buf: Vec<f32> = (0..96000).flat_map(|i| { let v = (0.5 * (2.0 * PI * 1000.0 * i as f64 / sr).sin()) as f32; [v, v] }).collect();
        e.process_interleaved(&mut buf);
        let rms = |c: usize| (buf[96000..].iter().skip(c).step_by(2).map(|s| (*s as f64).powi(2)).sum::<f64>() / 48000.0).sqrt();
        assert!(rms(0) > 0.3 && rms(1) < 0.01, "{} {}", rms(0), rms(1));
    }

    #[test]
    fn bypass_is_bit_exact() {
        let mut e = Engine::bypass(48000, 2);
        let mut buf = [0.1f32, -0.2, 0.3, -0.4];
        e.process_interleaved(&mut buf);
        assert_eq!(buf, [0.1, -0.2, 0.3, -0.4]);
    }
}

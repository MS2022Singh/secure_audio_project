//! FIR processing for `GraphicEQ:` and `Convolution:`.
//!
//! * [`graphic_eq_fir`] designs a linear-phase FIR from GraphicEQ points (frequency
//!   sampling + Blackman window), like Equalizer APO's GraphicEQ.
//! * [`FirStage`] runs FIRs with uniformly partitioned FFT convolution (overlap-save).
//!   Everything is allocated when the stage is built; processing does not allocate.
//!   Latency: one block ([`BLOCK`] samples); channels without a FIR are delayed by the same
//!   amount (plus the FIR's own group delay where known) so all channels stay aligned.

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};
use std::sync::Arc;

pub const BLOCK: usize = 256;

/// FIR length used for GraphicEQ at this sample rate (8192 taps at 44.1/48 kHz).
pub fn graphic_eq_len(sample_rate: u32) -> usize {
    let mut n = 8192;
    while (n as f64) < 8192.0 * sample_rate as f64 / 48000.0 {
        n *= 2;
    }
    n
}

/// Linear-phase FIR (odd length, group delay `(len-1)/2`) for a GraphicEQ.
pub fn graphic_eq_fir(points: &[(f64, f64)], sample_rate: u32) -> Vec<f32> {
    let n = graphic_eq_len(sample_rate);
    let mut planner = RealFftPlanner::<f64>::new();
    let inv = planner.plan_fft_inverse(n);
    let sr = sample_rate as f64;
    let mut spec: Vec<Complex<f64>> = (0..=n / 2)
        .map(|k| {
            let f = (k as f64 * sr / n as f64).max(1.0);
            let mag = 10f64.powf(crate::eqapo::graphic_eq_gain(points, f) / 20.0);
            Complex::new(mag, 0.0) // zero phase; centred below
        })
        .collect();
    spec[n / 2].im = 0.0;
    let mut h = vec![0.0f64; n];
    let _ = inv.process(&mut spec, &mut h);
    // Zero-phase impulse is centred at 0 (wrapped); rotate it to the middle, window it.
    let len = n - 1;
    let half = (len / 2) as isize;
    (0..len)
        .map(|i| {
            let k = (i as isize - half).rem_euclid(n as isize) as usize;
            let x = i as f64 / (len - 1) as f64;
            let w = 0.42 - 0.5 * (2.0 * std::f64::consts::PI * x).cos() + 0.08 * (4.0 * std::f64::consts::PI * x).cos();
            (h[k] / n as f64 * w) as f32
        })
        .collect()
}

/// Frequency-domain partitions of one impulse response.
struct Partitions {
    h: Vec<Vec<Complex<f32>>>,
}

fn partition(ir: &[f32], fft: &Arc<dyn RealToComplex<f32>>) -> Partitions {
    let p = ir.len().div_ceil(BLOCK).max(1);
    let mut h = Vec::with_capacity(p);
    let mut time = vec![0.0f32; 2 * BLOCK];
    let mut scratch = fft.make_scratch_vec();
    for i in 0..p {
        time.iter_mut().for_each(|v| *v = 0.0);
        let src = &ir[(i * BLOCK).min(ir.len())..((i + 1) * BLOCK).min(ir.len())];
        time[..src.len()].copy_from_slice(src);
        let mut out = fft.make_output_vec();
        let _ = fft.process_with_scratch(&mut time, &mut out, &mut scratch);
        h.push(out);
    }
    Partitions { h }
}

struct ChannelConv {
    parts: Arc<Partitions>,
    fdl: Vec<Vec<Complex<f32>>>, // past input spectra, ring buffer
    fdl_pos: usize,
    prev: Vec<f32>,  // previous input block
    input: Vec<f32>, // current input block (being filled)
    output: Vec<f32>,
}

enum Lane {
    Fir(ChannelConv),
    /// Channel without a FIR: delayed so it stays aligned with the filtered channels.
    Delay { line: Vec<f32>, pos: usize },
}

pub struct FirStage {
    lanes: Vec<Lane>,
    pos: usize,
    fwd: Arc<dyn RealToComplex<f32>>,
    inv: Arc<dyn ComplexToReal<f32>>,
    time: Vec<f32>,
    spec: Vec<Complex<f32>>,
    acc: Vec<Complex<f32>>,
    scratch_f: Vec<Complex<f32>>,
    scratch_i: Vec<Complex<f32>>,
    pub latency: usize,
}

impl FirStage {
    /// `firs[c]`: the FIR for channel `c`, or None (pass-through, delay-compensated).
    /// `extra_delay`: group delay of the FIRs, also applied to unfiltered channels.
    pub fn new(firs: Vec<Option<Arc<Vec<f32>>>>, extra_delay: usize) -> FirStage {
        let mut planner = RealFftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(2 * BLOCK);
        let inv = planner.plan_fft_inverse(2 * BLOCK);
        // Identical IRs share their partitions.
        let mut cache: Vec<(Arc<Vec<f32>>, Arc<Partitions>)> = Vec::new();
        let lanes = firs
            .into_iter()
            .map(|f| match f {
                Some(ir) => {
                    let parts = match cache.iter().find(|(k, _)| Arc::ptr_eq(k, &ir)) {
                        Some((_, p)) => p.clone(),
                        None => {
                            let p = Arc::new(partition(&ir, &fwd));
                            cache.push((ir.clone(), p.clone()));
                            p
                        }
                    };
                    let n = parts.h.len();
                    Lane::Fir(ChannelConv {
                        fdl: vec![vec![Complex::new(0.0, 0.0); BLOCK + 1]; n],
                        parts,
                        fdl_pos: 0,
                        prev: vec![0.0; BLOCK],
                        input: vec![0.0; BLOCK],
                        output: vec![0.0; BLOCK],
                    })
                }
                None => Lane::Delay { line: vec![0.0; BLOCK + extra_delay], pos: 0 },
            })
            .collect();
        FirStage {
            lanes,
            pos: 0,
            time: vec![0.0; 2 * BLOCK],
            spec: fwd.make_output_vec(),
            acc: fwd.make_output_vec(),
            scratch_f: fwd.make_scratch_vec(),
            scratch_i: inv.make_scratch_vec(),
            fwd,
            inv,
            latency: BLOCK + extra_delay,
        }
    }

    #[inline]
    pub fn process_frame(&mut self, frame: &mut [f32]) {
        let pos = self.pos;
        for (c, s) in frame.iter_mut().enumerate() {
            match self.lanes.get_mut(c) {
                Some(Lane::Fir(cc)) => {
                    cc.input[pos] = *s;
                    *s = cc.output[pos];
                }
                Some(Lane::Delay { line, pos: p }) => {
                    let out = line[*p];
                    line[*p] = *s;
                    *p = (*p + 1) % line.len();
                    *s = out;
                }
                None => {}
            }
        }
        self.pos += 1;
        if self.pos == BLOCK {
            self.pos = 0;
            self.run_block();
        }
    }

    fn run_block(&mut self) {
        let scale = 1.0 / (2 * BLOCK) as f32;
        for lane in self.lanes.iter_mut() {
            let Lane::Fir(cc) = lane else { continue };
            // Spectrum of [previous block, current block].
            self.time[..BLOCK].copy_from_slice(&cc.prev);
            self.time[BLOCK..].copy_from_slice(&cc.input);
            cc.prev.copy_from_slice(&cc.input);
            let _ = self.fwd.process_with_scratch(&mut self.time, &mut self.spec, &mut self.scratch_f);
            let p = cc.parts.h.len();
            cc.fdl_pos = (cc.fdl_pos + p - 1) % p;
            cc.fdl[cc.fdl_pos].copy_from_slice(&self.spec);
            // Sum over partitions: X[k - i] * H[i].
            self.acc.iter_mut().for_each(|v| *v = Complex::new(0.0, 0.0));
            for i in 0..p {
                let x = &cc.fdl[(cc.fdl_pos + i) % p];
                let h = &cc.parts.h[i];
                for ((a, xv), hv) in self.acc.iter_mut().zip(x.iter()).zip(h.iter()) {
                    *a += xv * hv;
                }
            }
            let n = self.acc.len();
            self.acc[0].im = 0.0;
            self.acc[n - 1].im = 0.0;
            let _ = self.inv.process_with_scratch(&mut self.acc, &mut self.time, &mut self.scratch_i);
            for (o, t) in cc.output.iter_mut().zip(self.time[BLOCK..].iter()) {
                *o = t * scale;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convolution_matches_direct_and_delays_others() {
        // IR: 3 taps spread over partitions.
        let mut ir = vec![0.0f32; 700];
        ir[0] = 0.5;
        ir[300] = 0.25;
        ir[650] = -0.125;
        let ir = Arc::new(ir);
        let mut st = FirStage::new(vec![Some(ir.clone()), None], 0);
        let n = 4000;
        let x: Vec<f32> = (0..n).map(|i| ((i * 7919) % 97) as f32 / 97.0 - 0.5).collect();
        let mut out_l = vec![0.0; n];
        let mut out_r = vec![0.0; n];
        for i in 0..n {
            let mut f = [x[i], x[i]];
            st.process_frame(&mut f);
            out_l[i] = f[0];
            out_r[i] = f[1];
        }
        for i in BLOCK + 700..n {
            let j = i - BLOCK;
            let direct = 0.5 * x[j] + 0.25 * x[j - 300] - 0.125 * x[j - 650];
            assert!((out_l[i] - direct).abs() < 1e-4, "{} {} {}", i, out_l[i], direct);
            assert_eq!(out_r[i], x[j]);
        }
    }

    #[test]
    fn graphic_eq_fir_has_the_requested_response() {
        let pts = vec![(100.0, -6.0), (1000.0, 0.0), (10000.0, 6.0)];
        let h = graphic_eq_fir(&pts, 48000);
        let mag = |f: f64| {
            let w = 2.0 * std::f64::consts::PI * f / 48000.0;
            let (mut re, mut im) = (0.0, 0.0);
            for (k, v) in h.iter().enumerate() {
                re += *v as f64 * (w * k as f64).cos();
                im -= *v as f64 * (w * k as f64).sin();
            }
            20.0 * (re * re + im * im).sqrt().log10()
        };
        assert!((mag(50.0) + 6.0).abs() < 0.3, "{}", mag(50.0));
        assert!(mag(1000.0).abs() < 0.3, "{}", mag(1000.0));
        assert!((mag(15000.0) - 6.0).abs() < 0.3, "{}", mag(15000.0));
        assert!((mag(316.2) + 3.0).abs() < 0.3, "{}", mag(316.2)); // halfway in log frequency
    }
}

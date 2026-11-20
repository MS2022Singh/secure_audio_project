//! Frequency-signature watermark, mixed into every stream the APO processes.
//!
//! Tone mode: a sine carrier (e.g. 16 kHz at amplitude 0.0008, about -62 dBFS) is on/off
//! keyed with a 48-bit pattern: an 8-bit sync word followed by a 40-bit code derived from
//! the Signature ID. Each bit lasts 40 ms, with 5 ms raised-cosine edges (no clicks).
//! One burst is 1.92 s; bursts repeat every `interval_s` seconds (0 = continuously).
//!
//! Clip mode: a developer-uploaded clip (mono) is looped and mixed at the same amplitude,
//! with the same timing and gating as the tone.
//!
//! "Auto" gating: the watermark follows the programme level, so it is only present
//! (and masked) while audio is playing, never as a tone in silence.
//!
//! `detect` recovers the code from a recording, so a leaked file can be traced.

use std::f64::consts::PI;

pub const SYNC: [bool; 8] = [true, false, true, true, false, false, true, false];
pub const CODE_BITS: usize = 40;
pub const BIT_SECONDS: f64 = 0.040;
pub const EDGE_SECONDS: f64 = 0.005;
pub const MAX_CLIP_SECONDS: f64 = 10.0;

#[derive(Debug, Clone, PartialEq)]
pub struct WatermarkSpec {
    pub id: String,
    pub freq: f64,
    pub amp: f64,
    /// Active from `start_s` to `end_s` seconds into each stream (end <= start: always).
    pub start_s: f64,
    pub end_s: f64,
    /// Seconds between the starts of two bursts (0 = continuous).
    pub interval_s: f64,
    /// Only add the watermark while programme audio is present.
    pub gate_on_signal: bool,
    /// Optional uploaded clip, mono, at `clip_rate`.
    pub clip: Option<Vec<f32>>,
    pub clip_rate: u32,
}

impl Default for WatermarkSpec {
    fn default() -> Self {
        WatermarkSpec {
            id: "diviora".into(), freq: 16000.0, amp: 0.0008, start_s: 0.0, end_s: 0.0,
            interval_s: 0.0, gate_on_signal: true, clip: None, clip_rate: 48000,
        }
    }
}

/// 40-bit code from the Signature ID (FNV-1a 64, folded). Stable across builds and platforms.
pub fn id_code(id: &str) -> [bool; CODE_BITS] {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in id.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    let folded = h ^ (h >> 40);
    let mut out = [false; CODE_BITS];
    for (i, bit) in out.iter_mut().enumerate() {
        *bit = (folded >> i) & 1 == 1;
    }
    out
}

pub fn pattern(id: &str) -> Vec<bool> {
    let mut p = SYNC.to_vec();
    p.extend_from_slice(&id_code(id));
    p
}

/// Real-time state, created off the audio thread by `Watermarker::new`.
pub struct Watermarker {
    amp: f32,
    phase: f64,
    phase_inc: f64,
    tone_ok: bool,
    clip: Vec<f32>,
    clip_pos: usize,
    pattern: Vec<bool>,
    spb: u64,           // samples per bit
    edge: u64,          // samples per edge ramp
    burst_len: u64,
    period: u64,        // 0 = continuous
    start: u64,
    end: u64,           // 0 = no end
    frame: u64,
    gate: bool,
    env: f32,
    env_release: f32,
}

impl Watermarker {
    pub fn new(spec: &WatermarkSpec, sample_rate: u32) -> Watermarker {
        let sr = sample_rate.max(1) as f64;
        let spb = (BIT_SECONDS * sr).round().max(1.0) as u64;
        let pattern = pattern(&spec.id);
        let burst_len = spb * pattern.len() as u64;
        let period = if spec.interval_s > 0.0 { ((spec.interval_s * sr) as u64).max(burst_len) } else { 0 };
        let clip = match &spec.clip {
            Some(c) if !c.is_empty() => resample_linear(c, spec.clip_rate.max(1), sample_rate.max(1)),
            _ => Vec::new(),
        };
        Watermarker {
            amp: spec.amp.clamp(0.0, 0.05) as f32,
            phase: 0.0,
            phase_inc: 2.0 * PI * spec.freq / sr,
            tone_ok: spec.freq > 0.0 && spec.freq < 0.475 * sr,
            clip,
            clip_pos: 0,
            pattern,
            spb,
            edge: ((EDGE_SECONDS * sr) as u64).clamp(1, spb / 2),
            burst_len,
            period,
            start: (spec.start_s.max(0.0) * sr) as u64,
            end: if spec.end_s > spec.start_s { (spec.end_s * sr) as u64 } else { 0 },
            frame: 0,
            gate: spec.gate_on_signal,
            env: 0.0,
            env_release: (-1.0 / (0.3 * sr)).exp() as f32, // 300 ms release
        }
    }

    /// Keying envelope (0..1) at a position inside the burst.
    fn keying(&self, pos_in_burst: u64) -> f32 {
        if pos_in_burst >= self.burst_len {
            return 0.0;
        }
        let bit = (pos_in_burst / self.spb) as usize;
        if !self.pattern[bit] {
            return 0.0;
        }
        let k = pos_in_burst % self.spb;
        // Ramp up only if the previous bit was off, down only if the next bit is off.
        let prev_on = bit > 0 && self.pattern[bit - 1];
        let next_on = bit + 1 < self.pattern.len() && self.pattern[bit + 1];
        let e = self.edge as f32;
        let mut g = 1.0f32;
        if !prev_on && k < self.edge {
            g = 0.5 - 0.5 * (std::f32::consts::PI * k as f32 / e).cos();
        }
        let rem = self.spb - 1 - k;
        if !next_on && rem < self.edge {
            g = g.min(0.5 - 0.5 * (std::f32::consts::PI * rem as f32 / e).cos());
        }
        g
    }

    /// Add the watermark to one interleaved frame. Real-time safe.
    #[inline]
    pub fn process_frame(&mut self, frame: &mut [f32]) {
        let f = self.frame;
        self.frame = self.frame.wrapping_add(1);

        // Programme envelope (peak follower) for masking.
        let peak = frame.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        self.env = if peak > self.env { peak } else { self.env * self.env_release };

        let active_time = f >= self.start && (self.end == 0 || f < self.end);
        let mut w = 0.0f32;
        if active_time {
            let rel = f - self.start;
            let pos = if self.period > 0 { rel % self.period } else { rel % self.burst_len };
            let key = self.keying(pos);
            if key > 0.0 {
                let gate = if self.gate { (self.env / 0.02).min(1.0) } else { 1.0 };
                let carrier = if !self.clip.is_empty() {
                    self.clip[self.clip_pos]
                } else if self.tone_ok {
                    self.phase.sin() as f32
                } else {
                    0.0
                };
                w = self.amp * key * gate * carrier;
            }
        }
        if !self.clip.is_empty() {
            self.clip_pos = (self.clip_pos + 1) % self.clip.len();
        }
        self.phase += self.phase_inc;
        if self.phase > 2.0 * PI {
            self.phase -= 2.0 * PI;
        }
        if w != 0.0 {
            for s in frame.iter_mut() {
                *s += w;
            }
        }
    }
}

pub fn resample_linear(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.len() < 2 {
        return input.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let out_len = ((input.len() as f64) / ratio).floor() as usize;
    (0..out_len)
        .map(|i| {
            let x = i as f64 * ratio;
            let i0 = x.floor() as usize;
            let i1 = (i0 + 1).min(input.len() - 1);
            let t = (x - i0 as f64) as f32;
            input[i0] * (1.0 - t) + input[i1] * t
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    /// Normalised correlation with the expected pattern (1.0 = perfect).
    pub score: f64,
    /// True if score passes the threshold.
    pub found: bool,
    /// Burst start (seconds) where the best match was found.
    pub at_seconds: f64,
}

/// Look for the tone-mode watermark of `id` at `freq` in mono `samples`.
pub fn detect(samples: &[f32], sample_rate: u32, freq: f64, id: &str) -> Detection {
    let sr = sample_rate as f64;
    let spb = (BIT_SECONDS * sr).round() as usize;
    let pat = pattern(id);
    let none = Detection { score: 0.0, found: false, at_seconds: 0.0 };
    if spb == 0 || samples.len() < spb * pat.len() {
        return none;
    }
    // Carrier energy in quarter-bit hops (Goertzel over one bit length).
    let hop = (spb / 4).max(1);
    let coeff = 2.0 * (2.0 * PI * freq / sr).cos();
    let mut energy = Vec::new();
    let mut start = 0;
    while start + spb <= samples.len() {
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for &x in &samples[start..start + spb] {
            let s0 = x as f64 + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        energy.push((s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0).sqrt());
        start += hop;
    }
    let expected: Vec<f64> = pat.iter().map(|b| if *b { 1.0 } else { -1.0 }).collect();
    let mean_e: f64 = expected.iter().sum::<f64>() / expected.len() as f64;
    let mut best = none;
    let bits_in_hops = 4;
    for off in 0..energy.len().saturating_sub(pat.len() * bits_in_hops - 1) {
        let obs: Vec<f64> = (0..pat.len()).map(|b| energy[off + b * bits_in_hops]).collect();
        let mean_o = obs.iter().sum::<f64>() / obs.len() as f64;
        let (mut num, mut den_o, mut den_e) = (0.0, 0.0, 0.0);
        for (o, e) in obs.iter().zip(&expected) {
            num += (o - mean_o) * (e - mean_e);
            den_o += (o - mean_o).powi(2);
            den_e += (e - mean_e).powi(2);
        }
        let score = if den_o > 0.0 { num / (den_o * den_e).sqrt() } else { 0.0 };
        if score > best.score {
            best = Detection { score, found: score > 0.8, at_seconds: (off * hop) as f64 / sr };
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Music-like programme: a few partials below 5 kHz (like real music, little energy at 16 kHz).
    fn programme(n: usize, sr: f64) -> Vec<f32> {
        (0..n).map(|i| {
            let t = i as f64 / sr;
            (0.25 * (2.0 * PI * 220.0 * t).sin() + 0.15 * (2.0 * PI * 1250.0 * t).sin()
                + 0.08 * (2.0 * PI * 3700.0 * t).sin()) as f32
        }).collect()
    }

    fn run(spec: &WatermarkSpec, input: &[f32], sr: u32) -> Vec<f32> {
        let mut wm = Watermarker::new(spec, sr);
        input.iter().map(|&x| { let mut f = [x]; wm.process_frame(&mut f); f[0] }).collect()
    }

    #[test]
    fn embedded_id_is_detected_and_other_id_is_not() {
        let sr = 48000;
        let input = programme(sr as usize * 4, sr as f64);
        let spec = WatermarkSpec { id: "dev-key-v1".into(), ..Default::default() };
        let out = run(&spec, &input, sr);
        let added_peak = out.iter().zip(&input).map(|(o, i)| (o - i).abs()).fold(0.0f32, f32::max);
        assert!(added_peak <= 0.0008 + 1e-6, "watermark louder than amplitude: {}", added_peak);
        let d = detect(&out, sr, 16000.0, "dev-key-v1");
        assert!(d.found, "{:?}", d);
        assert!(!detect(&out, sr, 16000.0, "someone-else").found);
        assert!(!detect(&input, sr, 16000.0, "dev-key-v1").found);
    }

    #[test]
    fn silent_input_stays_silent_when_gated() {
        let spec = WatermarkSpec::default();
        let out = run(&spec, &vec![0.0f32; 48000], 48000);
        assert!(out.iter().all(|s| *s == 0.0));
        let ungated = WatermarkSpec { gate_on_signal: false, ..Default::default() };
        assert!(run(&ungated, &vec![0.0f32; 48000], 48000).iter().any(|s| *s != 0.0));
    }

    #[test]
    fn time_window_and_interval() {
        let sr = 48000;
        let input = programme(sr as usize * 6, sr as f64);
        let spec = WatermarkSpec { start_s: 2.0, end_s: 4.0, ..Default::default() };
        let out = run(&spec, &input, sr);
        let diff = |a: usize, b: usize| out[a..b].iter().zip(&input[a..b]).any(|(o, i)| o != i);
        assert!(!diff(0, 2 * 48000));
        assert!(diff(2 * 48000, 4 * 48000));
        assert!(!diff(4 * 48000, 6 * 48000));

        let spec = WatermarkSpec { interval_s: 5.0, ..Default::default() };
        let out = run(&spec, &input, sr);
        let d = |a: usize, b: usize| out[a..b].iter().zip(&input[a..b]).any(|(o, i)| o != i);
        assert!(d(0, 96000));               // first burst (1.92 s)
        assert!(!d(96000, 5 * 48000));      // gap until 5 s
        assert!(d(5 * 48000, 6 * 48000));   // next burst
    }

    #[test]
    fn clip_mode_mixes_uploaded_audio() {
        let clip: Vec<f32> = (0..4800).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 }).collect();
        let spec = WatermarkSpec { clip: Some(clip), clip_rate: 48000, gate_on_signal: false, ..Default::default() };
        let out = run(&spec, &vec![0.0f32; 4800], 48000);
        let max = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(max > 0.0007 && max <= 0.0008 + 1e-6);
    }
}

// ---------------------------------------------------------------------------
// Config-file representation (inside the encrypted config.dvr payload):
//   DivioraWatermark: Id dev-key-v1 Freq 16000 Amp 0.0008 Start 0 End 3600 Interval 30 Auto 1
//   DivioraWatermarkClip: Rate 48000 <base64 of mono 16-bit little-endian PCM>
// ---------------------------------------------------------------------------

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b' ' | b'\r' | b'\n' | b'\t' => continue,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

/// The two config lines for a spec (clip line only when a clip is present).
pub fn to_config_lines(spec: &WatermarkSpec) -> String {
    let mut s = format!(
        "DivioraWatermark: Id {} Freq {} Amp {} Start {} End {} Interval {} Auto {}\n",
        spec.id, spec.freq, spec.amp, spec.start_s, spec.end_s, spec.interval_s, spec.gate_on_signal as u8
    );
    if let Some(clip) = &spec.clip {
        let pcm: Vec<u8> = clip.iter().flat_map(|x| ((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
        s.push_str(&format!("DivioraWatermarkClip: Rate {} {}\n", spec.clip_rate, base64_encode(&pcm)));
    }
    s
}

pub fn parse_watermark_line(args: &str) -> Result<WatermarkSpec, String> {
    let t: Vec<&str> = args.split_whitespace().collect();
    let mut spec = WatermarkSpec::default();
    let mut i = 0;
    while i + 1 < t.len() {
        let v = t[i + 1];
        let num = || v.parse::<f64>().map_err(|_| format!("bad value '{}' for {}", v, t[i]));
        match t[i].to_ascii_lowercase().as_str() {
            "id" => { if !valid_id(v) { return Err("Signature ID must be 1-64 letters, digits, '-', '_' or '.'".into()); } spec.id = v.to_string(); }
            "freq" => spec.freq = num()?,
            "amp" => spec.amp = num()?,
            "start" => spec.start_s = num()?,
            "end" => spec.end_s = num()?,
            "interval" => spec.interval_s = num()?,
            "auto" => spec.gate_on_signal = v != "0",
            other => return Err(format!("unknown watermark field '{}'", other)),
        }
        i += 2;
    }
    if !(1000.0..=22000.0).contains(&spec.freq) { return Err("carrier frequency must be 1000-22000 Hz".into()); }
    if !(0.0..=0.05).contains(&spec.amp) { return Err("amplitude must be between 0 and 0.05".into()); }
    Ok(spec)
}

/// Returns (rate, mono samples).
pub fn parse_clip_line(args: &str) -> Result<(u32, Vec<f32>), String> {
    let mut it = args.split_whitespace();
    if !it.next().map(|k| k.eq_ignore_ascii_case("rate")).unwrap_or(false) { return Err("expected 'Rate <hz> <data>'".into()); }
    let rate: u32 = it.next().and_then(|r| r.parse().ok()).filter(|r| (8000..=192000).contains(r)).ok_or("bad clip rate")?;
    let bytes = base64_decode(it.next().ok_or("missing clip data")?).ok_or("clip data is not valid base64")?;
    let samples: Vec<f32> = bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
    if samples.len() as f64 > MAX_CLIP_SECONDS * rate as f64 + 1.0 { return Err("watermark clip longer than 10 s".into()); }
    Ok((rate, samples))
}

#[cfg(test)]
mod config_tests {
    use super::*;
    #[test]
    fn base64_roundtrip_and_config_lines() {
        for n in 0..10 {
            let d: Vec<u8> = (0..n).map(|i| (i * 37 + 5) as u8).collect();
            assert_eq!(base64_decode(&base64_encode(&d)).unwrap(), d);
        }
        let spec = WatermarkSpec { id: "dev-key-v1".into(), clip: Some(vec![0.5, -0.5, 0.25]), interval_s: 30.0, ..Default::default() };
        let lines = to_config_lines(&spec);
        let mut l = lines.lines();
        let w = parse_watermark_line(l.next().unwrap().splitn(2, ':').nth(1).unwrap()).unwrap();
        assert_eq!((w.id.as_str(), w.freq, w.amp, w.interval_s, w.gate_on_signal), ("dev-key-v1", 16000.0, 0.0008, 30.0, true));
        let (rate, clip) = parse_clip_line(l.next().unwrap().splitn(2, ':').nth(1).unwrap()).unwrap();
        assert_eq!(rate, 48000);
        assert!((clip[0] - 0.5).abs() < 1e-3 && (clip[1] + 0.5).abs() < 1e-3);
        assert!(parse_watermark_line("Id bad id! Freq 16000").is_err());
        assert!(parse_watermark_line("Freq 50").is_err());
    }
}

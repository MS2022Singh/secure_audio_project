//! Minimal WAV reader for impulse responses (`Convolution:`): PCM 16/24/32-bit and
//! IEEE float 32/64-bit, including WAVE_FORMAT_EXTENSIBLE.

/// Returns (sample rate, samples per channel).
pub fn decode(bytes: &[u8]) -> Result<(u32, Vec<Vec<f32>>), String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
    let u16le = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]);
    let u32le = |o: usize| u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
    let mut pos = 12;
    let mut fmt: Option<(u16, u16, u32, u16)> = None; // (format, channels, rate, bits)
    let mut data: Option<&[u8]> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let len = u32le(pos + 4) as usize;
        let body = pos + 8;
        let end = (body + len).min(bytes.len());
        if id == b"fmt " && len >= 16 {
            let mut format = u16le(body);
            if format == 0xFFFE && len >= 26 {
                format = u16le(body + 24); // sub-format GUID starts with the format code
            }
            fmt = Some((format, u16le(body + 2), u32le(body + 4), u16le(body + 14)));
        } else if id == b"data" {
            data = Some(&bytes[body..end]);
        }
        pos = body + len + (len & 1);
    }
    let (format, ch, rate, bits) = fmt.ok_or("WAV file has no format chunk")?;
    let data = data.ok_or("WAV file has no data")?;
    let ch = ch as usize;
    if ch == 0 || ch > 32 || rate == 0 {
        return Err("unsupported WAV channel count or sample rate".into());
    }
    let bps = (bits as usize + 7) / 8;
    if bps == 0 {
        return Err("unsupported WAV sample size".into());
    }
    let frames = data.len() / (bps * ch);
    if frames == 0 {
        return Err("WAV file is empty".into());
    }
    if frames > 10 * 192_000 {
        return Err("impulse response longer than 10 seconds".into());
    }
    let mut out = vec![Vec::with_capacity(frames); ch];
    for f in 0..frames {
        for (c, chan) in out.iter_mut().enumerate() {
            let o = (f * ch + c) * bps;
            let s = &data[o..o + bps];
            let v = match (format, bits) {
                (1, 16) => i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0,
                (1, 24) => ((i32::from_le_bytes([0, s[0], s[1], s[2]])) >> 8) as f32 / 8_388_608.0,
                (1, 32) => i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f32 / 2_147_483_648.0,
                (1, 8) => (s[0] as f32 - 128.0) / 128.0,
                (3, 32) => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                (3, 64) => f64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]) as f32,
                _ => return Err(format!("unsupported WAV format {} / {} bit", format, bits)),
            };
            chan.push(if v.is_finite() { v } else { 0.0 });
        }
    }
    Ok((rate, out))
}

/// Resample one channel (windowed-sinc, used once when the engine is built).
pub fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || x.is_empty() {
        return x.to_vec();
    }
    let ratio = to as f64 / from as f64;
    let n_out = ((x.len() as f64) * ratio).ceil() as usize;
    let cutoff = ratio.min(1.0); // low-pass at the lower Nyquist
    let half = 16.0 / cutoff;
    let mut y = Vec::with_capacity(n_out);
    for i in 0..n_out {
        let t = i as f64 / ratio; // position in input samples
        let lo = (t - half).floor().max(0.0) as usize;
        let hi = ((t + half).ceil() as usize).min(x.len() - 1);
        let mut acc = 0.0f64;
        for (k, xv) in x.iter().enumerate().take(hi + 1).skip(lo) {
            let d = t - k as f64;
            let w = 0.5 + 0.5 * (std::f64::consts::PI * d / half).cos(); // Hann
            let arg = std::f64::consts::PI * d * cutoff;
            let sinc = if d.abs() < 1e-12 { 1.0 } else { arg.sin() / arg };
            acc += *xv as f64 * cutoff * sinc * w;
        }
        y.push(acc as f32);
    }
    y
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav16(rate: u32, ch: u16, samples: &[i16]) -> Vec<u8> {
        let mut v = Vec::new();
        let data_len = (samples.len() * 2) as u32;
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + data_len).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&ch.to_le_bytes());
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&(rate * ch as u32 * 2).to_le_bytes());
        v.extend_from_slice(&(ch * 2).to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&data_len.to_le_bytes());
        for s in samples { v.extend_from_slice(&s.to_le_bytes()); }
        v
    }

    #[test]
    fn reads_pcm16_and_float() {
        let (r, c) = decode(&wav16(44100, 2, &[16384, -16384, 0, 32767])).unwrap();
        assert_eq!(r, 44100);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0], vec![0.5, 0.0]);
        assert!((c[1][1] - 1.0).abs() < 1e-4);
        let f = crate::analysis::wav_f32(&[0.25, -0.5], 48000, 1);
        let (r, c) = decode(&f).unwrap();
        assert_eq!((r, c[0].clone()), (48000, vec![0.25, -0.5]));
    }

    #[test]
    fn resample_keeps_dc_and_length() {
        let x = vec![1.0f32; 4410];
        let y = resample(&x, 44100, 48000);
        assert_eq!(y.len(), 4800);
        assert!((y[2400] - 1.0).abs() < 0.01, "{}", y[2400]);
    }
}

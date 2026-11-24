//! Emit a CamillaDSP (v3 config format) YAML file from parsed Equalizer APO stages.
//!
//! This is the export path the requirement asks for (an Equalizer APO file used in
//! CamillaDSP), and it covers more than the CamillaGUI importer does: Preamp, all
//! filter types, Channel selection, Copy (as a mixer), Delay and Device blocks
//! (one YAML file per device). The parser's warnings list whatever was not converted.

use crate::eqapo::{EqApoConfig, FilterKind, Shape, Stage, DeviceSel};
use std::fmt::Write;

#[derive(Debug, Clone)]
pub struct CamillaOptions {
    pub samplerate: u32,
    pub channels: usize,
    pub chunksize: u32,
    /// e.g. "CABLE Output (VB-Audio Virtual Cable)"
    pub capture_device: Option<String>,
    /// e.g. "Speakers (Realtek(R) Audio)"
    pub playback_device: Option<String>,
    /// Capture with WASAPI loopback from `capture_device` (a playback device) instead of a recording device.
    pub loopback: bool,
}

impl Default for CamillaOptions {
    fn default() -> Self {
        CamillaOptions { samplerate: 48000, channels: 2, chunksize: 1024, capture_device: None, playback_device: None, loopback: false }
    }
}

fn q(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn num(v: f64) -> String {
    let r = (v * 1e6).round() / 1e6;
    if r.fract() == 0.0 { format!("{:.1}", r) } else { format!("{}", r) }
}

/// Emit one YAML document for the given stages.
pub fn emit(stages: &[Stage], opt: &CamillaOptions, header_comment: &str) -> String {
    let n = opt.channels.max(1);
    let mut y = String::new();
    for l in header_comment.lines() {
        let _ = writeln!(y, "# {}", l);
    }
    let _ = writeln!(y, "devices:");
    let _ = writeln!(y, "  samplerate: {}", opt.samplerate);
    let _ = writeln!(y, "  chunksize: {}", opt.chunksize);
    let _ = writeln!(y, "  capture:");
    let _ = writeln!(y, "    type: Wasapi");
    let _ = writeln!(y, "    channels: {}", n);
    let _ = writeln!(y, "    device: {}", opt.capture_device.as_deref().map(q).unwrap_or_else(|| "null".into()));
    let _ = writeln!(y, "    format: FLOAT32LE");
    if opt.loopback {
        let _ = writeln!(y, "    loopback: true");
    }
    let _ = writeln!(y, "  playback:");
    let _ = writeln!(y, "    type: Wasapi");
    let _ = writeln!(y, "    channels: {}", n);
    let _ = writeln!(y, "    device: {}", opt.playback_device.as_deref().map(q).unwrap_or_else(|| "null".into()));
    let _ = writeln!(y, "    format: FLOAT32LE");

    let mut filters = String::new();
    let mut mixers = String::new();
    // Pipeline steps: (channels, filter names) or mixer name.
    enum Step { Filters(Vec<usize>, Vec<String>), Mixer(String) }
    let mut steps: Vec<Step> = Vec::new();

    let push_filter = |steps: &mut Vec<Step>, chans: Vec<usize>, name: String| {
        if let Some(Step::Filters(c, names)) = steps.last_mut() {
            if *c == chans { names.push(name); return; }
        }
        steps.push(Step::Filters(chans, vec![name]));
    };

    for (i, st) in stages.iter().enumerate() {
        let idx = i + 1;
        match st {
            Stage::Gain { gain_db, channels } => {
                let name = format!("preamp_{}", idx);
                let _ = writeln!(filters, "  {}:\n    type: Gain\n    parameters:\n      gain: {}", name, num(*gain_db));
                let chans = channels.indices(n);
                if !chans.is_empty() { push_filter(&mut steps, chans, name); }
            }
            Stage::Biquad { spec, channels } => {
                let name = format!("filter_{}", idx);
                let ty = match spec.kind {
                    FilterKind::Peaking => "Peaking",
                    FilterKind::LowShelf => "Lowshelf",
                    FilterKind::HighShelf => "Highshelf",
                    FilterKind::LowPass => "Lowpass",
                    FilterKind::HighPass => "Highpass",
                    FilterKind::BandPass => "Bandpass",
                    FilterKind::Notch => "Notch",
                    FilterKind::AllPass => "Allpass",
                    FilterKind::LowShelf1 => "LowshelfFO",
                    FilterKind::HighShelf1 => "HighshelfFO",
                    FilterKind::LowPass1 => "LowpassFO",
                    FilterKind::HighPass1 => "HighpassFO",
                };
                let _ = writeln!(filters, "  {}:\n    type: Biquad\n    parameters:\n      type: {}\n      freq: {}", name, ty, num(spec.freq));
                if matches!(spec.kind, FilterKind::Peaking | FilterKind::LowShelf | FilterKind::HighShelf) {
                    let _ = writeln!(filters, "      gain: {}", num(spec.gain_db));
                }
                match spec.shape {
                    Shape::Q(v) => { let _ = writeln!(filters, "      q: {}", num(v)); }
                    Shape::SlopeDbPerOct(s) => { let _ = writeln!(filters, "      slope: {}", num(s)); }
                    Shape::BwOct(b) => { let _ = writeln!(filters, "      bandwidth: {}", num(b)); }
                }
                let chans = channels.indices(n);
                if !chans.is_empty() { push_filter(&mut steps, chans, name); }
            }
            Stage::Delay { ms, channels } => {
                let name = format!("delay_{}", idx);
                let _ = writeln!(filters, "  {}:\n    type: Delay\n    parameters:\n      delay: {}\n      unit: ms", name, num(*ms));
                let chans = channels.indices(n);
                if !chans.is_empty() { push_filter(&mut steps, chans, name); }
            }
            Stage::Mix { routes } => {
                let name = format!("copy_{}", idx);
                let _ = writeln!(mixers, "  {}:\n    channels:\n      in: {}\n      out: {}\n    mapping:", name, n, n);
                for dest in 0..n {
                    let sources: Vec<(usize, f64)> = match routes.iter().find(|(d, _)| *d == dest) {
                        Some((_, s)) => s.iter().copied().filter(|(c, f)| *c < n && *f != 0.0).collect(),
                        None => vec![(dest, 1.0)],
                    };
                    let _ = writeln!(mixers, "      - dest: {}", dest);
                    if sources.is_empty() {
                        let _ = writeln!(mixers, "        sources: []");
                        continue;
                    }
                    let _ = writeln!(mixers, "        sources:");
                    for (c, f) in sources {
                        let db = 20.0 * f.abs().log10();
                        let _ = writeln!(mixers, "          - channel: {}\n            gain: {}\n            inverted: {}", c, num(db), f < 0.0);
                    }
                }
                steps.push(Step::Mixer(name));
            }
            Stage::Watermark(_) => {} // Diviora-only; not part of a CamillaDSP export
            Stage::Iir { b, a, channels } if b.len() <= 3 && a.len() <= 3 => {
                let name = format!("iir_{}", idx);
                let g = |v: &Vec<f64>, i: usize| v.get(i).copied().unwrap_or(0.0);
                let _ = writeln!(filters, "  {}:\n    type: Biquad\n    parameters:\n      type: Free\n      a1: {}\n      a2: {}\n      b0: {}\n      b1: {}\n      b2: {}",
                    name, num(g(a, 1)), num(g(a, 2)), num(g(b, 0)), num(g(b, 1)), num(g(b, 2)));
                let chans = channels.indices(n);
                if !chans.is_empty() { push_filter(&mut steps, chans, name); }
            }
            Stage::Iir { .. } | Stage::GraphicEq { .. } | Stage::Convolution { .. } => {
                let _ = writeln!(filters, "  # stage {}: higher-order IIR / GraphicEQ / Convolution runs in the Diviora engine only (not exported)", idx);
            }
        }
    }

    let _ = writeln!(y, "filters:{}", if filters.is_empty() { " {}" } else { "" });
    y.push_str(&filters);
    let _ = writeln!(y, "mixers:{}", if mixers.is_empty() { " {}" } else { "" });
    y.push_str(&mixers);
    let _ = writeln!(y, "pipeline:{}", if steps.is_empty() { " []" } else { "" });
    for s in steps {
        match s {
            Step::Filters(chans, names) => {
                let c: Vec<String> = chans.iter().map(|c| c.to_string()).collect();
                let _ = writeln!(y, "  - type: Filter\n    channels: [{}]\n    names:", c.join(", "));
                for nme in names { let _ = writeln!(y, "      - {}", nme); }
            }
            Step::Mixer(name) => { let _ = writeln!(y, "  - type: Mixer\n    name: {}", name); }
        }
    }
    y
}

/// Emit one YAML per output profile: `("default", yaml)` plus one per `Device:` selector.
pub fn emit_profiles(cfg: &EqApoConfig, opt: &CamillaOptions) -> Vec<(String, String)> {
    let mut warn_text = String::from("Generated by Diviora from an Equalizer APO configuration.");
    if !cfg.warnings.is_empty() {
        warn_text.push_str("\nNot converted:");
        for w in &cfg.warnings { warn_text.push_str(&format!("\n  {}", w)); }
    }
    let mut out = vec![("default".to_string(), emit(&cfg.stages_for_all_devices(), opt, &warn_text))];
    for sel in cfg.device_selectors() {
        if let DeviceSel::Matching(pats) = &sel {
            let label = pats.join(";");
            let stages: Vec<Stage> = cfg.stages.iter()
                .filter(|(d, _)| *d == DeviceSel::All || *d == sel)
                .map(|(_, s)| s.clone()).collect();
            let mut o = opt.clone();
            if o.playback_device.is_none() { o.playback_device = pats.first().cloned(); }
            out.push((label.clone(), emit(&stages, &o, &format!("{}\nProfile for device: {}", warn_text, label))));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eqapo::parse;

    #[test]
    fn emits_full_pipeline() {
        let cfg = parse(
            "Preamp: -6 dB\nFilter 1: ON PK Fc 1000 Hz Gain -3.0 dB Q 1.41\nChannel: R\nFilter: ON HSC 6 dB Fc 8000 Hz Gain 2 dB\nChannel: all\nCopy: L=L+0.5*R R=R+0.5*L\n",
            None,
        );
        let y = emit(&cfg.stages_for_all_devices(), &CamillaOptions {
            capture_device: Some("CABLE Output (VB-Audio Virtual Cable)".into()),
            playback_device: Some("Speakers (Realtek(R) Audio)".into()),
            ..Default::default()
        }, "test");
        let expected_bits = [
            "  preamp_1:\n    type: Gain\n    parameters:\n      gain: -6.0",
            "      type: Peaking\n      freq: 1000.0\n      gain: -3.0\n      q: 1.41",
            "      type: Highshelf\n      freq: 8000.0\n      gain: 2.0\n      slope: 6.0",
            "  - type: Filter\n    channels: [0, 1]\n    names:\n      - preamp_1\n      - filter_2",
            "  - type: Filter\n    channels: [1]\n    names:\n      - filter_3",
            "  - type: Mixer\n    name: copy_4",
            "          - channel: 1\n            gain: -6.0206\n            inverted: false",
            "    device: \"CABLE Output (VB-Audio Virtual Cable)\"",
        ];
        for b in expected_bits {
            assert!(y.contains(b), "missing:\n{}\n--- in ---\n{}", b, y);
        }
    }

    #[test]
    fn per_device_profiles() {
        let cfg = parse("Preamp: -1 dB\nDevice: Headphones\nPreamp: -5 dB\n", None);
        let p = emit_profiles(&cfg, &CamillaOptions::default());
        assert_eq!(p.len(), 2);
        assert!(!p[0].1.contains("preamp_2"));
        assert!(p[1].1.contains("preamp_2") && p[1].1.contains("preamp_1"));
    }
}

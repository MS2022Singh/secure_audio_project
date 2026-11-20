//! Parser for Equalizer APO 1.4.x configuration files (`config.txt` and include files).
//!
//! Every non-empty, non-comment line ends up in exactly one of two places:
//! a [`Stage`] in the parsed pipeline, or a [`Warning`] explaining why it was
//! not converted. Nothing is dropped silently.

use crate::watermark::{self, WatermarkSpec};
use std::fmt;

/// Equalizer APO channel order for up to 7.1: L R C SUB RL RR SL SR.
pub const CHANNEL_NAMES: [&str; 8] = ["L", "R", "C", "SUB", "RL", "RR", "SL", "SR"];
pub const MAX_CHANNELS: usize = 8;

#[derive(Debug, Clone, PartialEq)]
pub enum ChannelSel {
    All,
    /// 0-based channel indices.
    List(Vec<usize>),
}

impl ChannelSel {
    pub fn contains(&self, ch: usize) -> bool {
        match self {
            ChannelSel::All => true,
            ChannelSel::List(v) => v.contains(&ch),
        }
    }
    pub fn indices(&self, channels: usize) -> Vec<usize> {
        (0..channels).filter(|c| self.contains(*c)).collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterKind {
    Peaking,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
    BandPass,
    Notch,
    AllPass,
}

/// Shape parameter of a filter: either a Q value or (shelves only) a slope in dB/octave.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    Q(f64),
    SlopeDbPerOct(f64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiquadSpec {
    pub kind: FilterKind,
    pub freq: f64,
    pub gain_db: f64,
    pub shape: Shape,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stage {
    /// `Preamp:` – gain applied to the currently selected channels.
    Gain { gain_db: f64, channels: ChannelSel },
    Biquad { spec: BiquadSpec, channels: ChannelSel },
    Delay { ms: f64, channels: ChannelSel },
    /// `Copy:` – each entry is (destination channel, [(source channel, linear factor)]).
    /// Channels that are not a destination keep their own signal.
    Mix { routes: Vec<(usize, Vec<(usize, f64)>)> },
    /// Diviora frequency-signature watermark (DivioraWatermark: / DivioraWatermarkClip:).
    Watermark(WatermarkSpec),
}

/// Which output device a stage applies to (`Device:` command).
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceSel {
    All,
    /// Case-insensitive substrings; the stage applies if any matches the device name.
    Matching(Vec<String>),
}

impl DeviceSel {
    pub fn matches(&self, device_name: &str) -> bool {
        match self {
            DeviceSel::All => true,
            DeviceSel::Matching(pats) => {
                let d = device_name.to_lowercase();
                pats.iter().any(|p| d.contains(&p.to_lowercase()))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Warning {
    pub line: usize,
    pub file: String,
    pub text: String,
    pub reason: String,
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {} — {}", self.file, self.line, self.text, self.reason)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct EqApoConfig {
    pub stages: Vec<(DeviceSel, Stage)>,
    pub warnings: Vec<Warning>,
}

impl EqApoConfig {
    /// Stages that apply to a given output device name.
    pub fn stages_for_device(&self, device_name: &str) -> Vec<Stage> {
        self.stages
            .iter()
            .filter(|(d, _)| d.matches(device_name))
            .map(|(_, s)| s.clone())
            .collect()
    }

    /// Stages that apply regardless of device (used when no device name is known).
    pub fn stages_for_all_devices(&self) -> Vec<Stage> {
        self.stages
            .iter()
            .filter(|(d, _)| *d == DeviceSel::All)
            .map(|(_, s)| s.clone())
            .collect()
    }

    /// Distinct `Device:` selectors used in the file (for emitting per-device profiles).
    pub fn device_selectors(&self) -> Vec<DeviceSel> {
        let mut out: Vec<DeviceSel> = Vec::new();
        for (d, _) in &self.stages {
            if *d != DeviceSel::All && !out.contains(d) {
                out.push(d.clone());
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }
}

/// Resolves `Include:` file names to their text. Return `None` if unavailable.
pub type IncludeResolver<'a> = &'a dyn Fn(&str) -> Option<String>;

const MAX_INCLUDE_DEPTH: usize = 8;

struct ParseState {
    channels: ChannelSel,
    device: DeviceSel,
    /// >0 while inside an `If:` block we cannot evaluate.
    skip_depth: usize,
}

/// Parse an Equalizer APO configuration text.
pub fn parse(text: &str, resolver: Option<IncludeResolver>) -> EqApoConfig {
    let mut cfg = EqApoConfig::default();
    let mut st = ParseState { channels: ChannelSel::All, device: DeviceSel::All, skip_depth: 0 };
    parse_into(text, "config", resolver, 0, &mut st, &mut cfg);
    cfg
}

fn warn(cfg: &mut EqApoConfig, file: &str, line: usize, text: &str, reason: impl Into<String>) {
    cfg.warnings.push(Warning {
        line,
        file: file.to_string(),
        text: text.to_string(),
        reason: reason.into(),
    });
}

fn parse_into(
    text: &str,
    file: &str,
    resolver: Option<IncludeResolver>,
    depth: usize,
    st: &mut ParseState,
    cfg: &mut EqApoConfig,
) {
    for (idx, raw) in text.lines().enumerate() {
        let lineno = idx + 1;
        let line = raw.trim().trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(colon) = line.find(':') else {
            warn(cfg, file, lineno, line, "not an Equalizer APO command (missing ':')");
            continue;
        };
        let cmd_full = line[..colon].trim();
        let args = line[colon + 1..].trim();
        // "Filter 3" -> "filter"
        let cmd = cmd_full.split_whitespace().next().unwrap_or("").to_lowercase();

        // Conditional blocks: we cannot evaluate runtime expressions, so the whole
        // block is skipped and reported.
        match cmd.as_str() {
            "if" => {
                if st.skip_depth == 0 {
                    warn(cfg, file, lineno, line,
                        "conditional block not evaluated; its contents were skipped (split it into per-device profiles instead)");
                }
                st.skip_depth += 1;
                continue;
            }
            "elseif" | "else" => continue,
            "endif" => {
                st.skip_depth = st.skip_depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }
        if st.skip_depth > 0 {
            continue;
        }

        match cmd.as_str() {
            "preamp" => match parse_db(args) {
                Some(g) => cfg.stages.push((st.device.clone(), Stage::Gain { gain_db: g, channels: st.channels.clone() })),
                None => warn(cfg, file, lineno, line, "could not read preamp gain"),
            },
            "filter" => match parse_filter(args) {
                Ok(Some(spec)) => cfg.stages.push((st.device.clone(), Stage::Biquad { spec, channels: st.channels.clone() })),
                Ok(None) => {} // filter switched OFF
                Err(e) => warn(cfg, file, lineno, line, e),
            },
            "channel" => match parse_channel_sel(args) {
                Some(c) => st.channels = c,
                None => warn(cfg, file, lineno, line, "unknown channel name"),
            },
            "device" => {
                let a = args.trim();
                if a.eq_ignore_ascii_case("all") || a.is_empty() {
                    st.device = DeviceSel::All;
                } else {
                    let pats = a.split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                    st.device = DeviceSel::Matching(pats);
                }
            }
            "copy" => match parse_copy(args) {
                Ok(routes) => cfg.stages.push((st.device.clone(), Stage::Mix { routes })),
                Err(e) => warn(cfg, file, lineno, line, e),
            },
            "delay" => match parse_delay(args) {
                Some(d) => cfg.stages.push((st.device.clone(), d.with_channels(st.channels.clone()))),
                None => warn(cfg, file, lineno, line, "could not read delay (expected '<n> ms' or '<n> samples')"),
            },
            "include" => {
                if depth >= MAX_INCLUDE_DEPTH {
                    warn(cfg, file, lineno, line, "include nesting too deep");
                    continue;
                }
                let name = args.trim().trim_matches('"');
                match resolver.and_then(|r| r(name)) {
                    Some(sub) => parse_into(&sub, name, resolver, depth + 1, st, cfg),
                    None => warn(cfg, file, lineno, line, "included file not available; upload its contents too"),
                }
            }
            "diviorawatermark" => match watermark::parse_watermark_line(args) {
                Ok(w) => cfg.stages.push((st.device.clone(), Stage::Watermark(w))),
                Err(e) => warn(cfg, file, lineno, cmd_full, e),
            },
            "diviorawatermarkclip" => {
                let target = cfg.stages.iter_mut().rev().find_map(|(_, s)| match s { Stage::Watermark(w) => Some(w), _ => None });
                match (watermark::parse_clip_line(args), target) {
                    (Ok((rate, clip)), Some(w)) => { w.clip = Some(clip); w.clip_rate = rate; }
                    (Ok(_), None) => warn(cfg, file, lineno, cmd_full, "watermark clip without a DivioraWatermark line"),
                    (Err(e), _) => warn(cfg, file, lineno, cmd_full, e),
                }
            }
            "stage" => {} // pre-mix/post-mix placement has no meaning outside audiodg
            "graphiceq" => warn(cfg, file, lineno, line, "GraphicEQ is not converted yet; export it as parametric filters (e.g. from AutoEq/REW)"),
            "convolution" => warn(cfg, file, lineno, line, "convolution is not supported by the built-in engine"),
            "eval" | "loudnesscorrection" | "vst" => warn(cfg, file, lineno, line, "not supported outside Equalizer APO"),
            _ => warn(cfg, file, lineno, line, "unknown command"),
        }
    }
}

trait WithChannels {
    fn with_channels(self, c: ChannelSel) -> Stage;
}
impl WithChannels for Stage {
    fn with_channels(self, c: ChannelSel) -> Stage {
        match self {
            Stage::Delay { ms, .. } => Stage::Delay { ms, channels: c },
            other => other,
        }
    }
}

fn parse_num(s: &str) -> Option<f64> {
    s.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite())
}

/// "-6 dB", "-6dB", "-6"
fn parse_db(s: &str) -> Option<f64> {
    let t = s.trim();
    let t = t.strip_suffix("dB").or_else(|| t.strip_suffix("db")).or_else(|| t.strip_suffix("DB")).unwrap_or(t);
    parse_num(t)
}

fn parse_delay(s: &str) -> Option<Stage> {
    let toks: Vec<&str> = s.split_whitespace().collect();
    let v = parse_num(toks.first()?)?;
    let unit = toks.get(1).map(|u| u.to_lowercase()).unwrap_or_else(|| "ms".into());
    // Samples are converted at 48 kHz here; the engine recompiles in ms so this is the
    // only place a sample rate is assumed.
    let ms = match unit.as_str() {
        "ms" => v,
        "samples" | "sample" => v / 48.0,
        _ => return None,
    };
    if ms < 0.0 { return None; }
    Some(Stage::Delay { ms, channels: ChannelSel::All })
}

fn channel_index(name: &str) -> Option<usize> {
    let n = name.trim().to_uppercase();
    if let Ok(i) = n.parse::<usize>() {
        return if (1..=MAX_CHANNELS).contains(&i) { Some(i - 1) } else { None };
    }
    match n.as_str() {
        "LFE" => Some(3),
        "FL" => Some(0),
        "FR" => Some(1),
        "FC" => Some(2),
        "BL" => Some(4),
        "BR" => Some(5),
        _ => CHANNEL_NAMES.iter().position(|c| *c == n),
    }
}

fn parse_channel_sel(s: &str) -> Option<ChannelSel> {
    let t = s.trim();
    if t.eq_ignore_ascii_case("all") {
        return Some(ChannelSel::All);
    }
    let mut v = Vec::new();
    for tok in t.split_whitespace() {
        v.push(channel_index(tok)?);
    }
    if v.is_empty() { None } else { Some(ChannelSel::List(v)) }
}

/// Q from bandwidth in octaves.
pub fn bw_oct_to_q(n: f64) -> f64 {
    let p = 2f64.powf(n);
    p.sqrt() / (p - 1.0)
}

fn parse_filter(args: &str) -> Result<Option<BiquadSpec>, String> {
    let toks: Vec<&str> = args.split_whitespace().collect();
    let mut i = 0;
    match toks.first().map(|t| t.to_uppercase()) {
        Some(ref t) if t == "ON" => i += 1,
        Some(ref t) if t == "OFF" => return Ok(None),
        _ => {}
    }
    let ty = toks.get(i).ok_or("missing filter type")?.to_uppercase();
    i += 1;

    let kind = match ty.as_str() {
        "PK" | "PEQ" | "MODAL" => FilterKind::Peaking,
        "LS" | "LSC" | "LSQ" => FilterKind::LowShelf,
        "HS" | "HSC" | "HSQ" => FilterKind::HighShelf,
        "LP" | "LPQ" => FilterKind::LowPass,
        "HP" | "HPQ" => FilterKind::HighPass,
        "BP" => FilterKind::BandPass,
        "NO" => FilterKind::Notch,
        "AP" => FilterKind::AllPass,
        "NONE" => return Ok(None),
        "IIR" => return Err("raw IIR coefficient filters are not converted".into()),
        other => return Err(format!("filter type '{}' not supported", other)),
    };

    let mut freq = None;
    let mut gain = 0.0;
    let mut shape: Option<Shape> = None;

    // Shelves may carry a slope right after the type: "LSC 12 dB Fc ..." / "LS 6dB Fc ...".
    if matches!(kind, FilterKind::LowShelf | FilterKind::HighShelf) {
        if let Some(t) = toks.get(i) {
            if let Some(s) = t.strip_suffix("dB").and_then(parse_num) {
                shape = Some(Shape::SlopeDbPerOct(s));
                i += 1;
            } else if let (Some(v), Some(u)) = (parse_num(t), toks.get(i + 1)) {
                if u.eq_ignore_ascii_case("dB") {
                    shape = Some(Shape::SlopeDbPerOct(v));
                    i += 2;
                }
            }
        }
    }

    while i < toks.len() {
        let key = toks[i].to_uppercase();
        match key.as_str() {
            "FC" => {
                freq = toks.get(i + 1).and_then(|v| parse_num(v));
                i += 2;
                if toks.get(i).map(|u| u.eq_ignore_ascii_case("Hz")).unwrap_or(false) { i += 1; }
            }
            "GAIN" => {
                gain = toks.get(i + 1).and_then(|v| parse_db(v)).ok_or("bad Gain value")?;
                i += 2;
                if toks.get(i).map(|u| u.eq_ignore_ascii_case("dB")).unwrap_or(false) { i += 1; }
            }
            "Q" => {
                shape = Some(Shape::Q(toks.get(i + 1).and_then(|v| parse_num(v)).ok_or("bad Q value")?));
                i += 2;
            }
            "BW" => {
                // "BW Oct 1.0"
                let mut j = i + 1;
                if toks.get(j).map(|t| t.eq_ignore_ascii_case("Oct")).unwrap_or(false) { j += 1; }
                let n = toks.get(j).and_then(|v| parse_num(v)).ok_or("bad BW value")?;
                if n <= 0.0 { return Err("bandwidth must be positive".into()); }
                shape = Some(Shape::Q(bw_oct_to_q(n)));
                i = j + 1;
            }
            _ => i += 1,
        }
    }

    let freq = freq.ok_or("missing Fc")?;
    if !(1.0..=96_000.0).contains(&freq) {
        return Err(format!("frequency {} Hz out of range", freq));
    }
    if !(-60.0..=36.0).contains(&gain) {
        return Err(format!("gain {} dB out of range", gain));
    }
    let shape = shape.unwrap_or(Shape::Q(std::f64::consts::FRAC_1_SQRT_2));
    match shape {
        Shape::Q(q) if !(q > 0.0 && q <= 100.0) => return Err(format!("Q {} out of range", q)),
        Shape::SlopeDbPerOct(s) if !(s > 0.0 && s <= 24.0) => return Err(format!("slope {} dB/oct out of range", s)),
        _ => {}
    }
    Ok(Some(BiquadSpec { kind, freq, gain_db: gain, shape }))
}

/// `Copy: L=0.5*L+0.5*R R=L C=-6dB*L+-6dB*R SUB=0`
fn parse_copy(args: &str) -> Result<Vec<(usize, Vec<(usize, f64)>)>, String> {
    // Remove spaces around operators so assignments become single tokens.
    let mut s = args.to_string();
    for op in ["=", "+", "*"] {
        while s.contains(&format!(" {}", op)) { s = s.replace(&format!(" {}", op), op); }
        while s.contains(&format!("{} ", op)) { s = s.replace(&format!("{} ", op), op); }
    }
    let mut routes = Vec::new();
    for assign in s.split_whitespace() {
        let (dest, expr) = assign.split_once('=').ok_or_else(|| format!("'{}' is not an assignment", assign))?;
        let dest = channel_index(dest).ok_or_else(|| format!("unknown channel '{}'", dest))?;
        let mut sources = Vec::new();
        for term in expr.split('+').filter(|t| !t.is_empty()) {
            let (factor, ch) = match term.split_once('*') {
                Some((f, c)) => (parse_factor(f).ok_or_else(|| format!("bad factor '{}'", f))?, c),
                None => {
                    // "-R" or plain "R"; a bare number is a constant, which we do not support.
                    if let Some(c) = term.strip_prefix('-') { (-1.0, c) } else { (1.0, term) }
                }
            };
            if parse_num(ch).is_some() && channel_index(ch).is_none() {
                if parse_num(ch) == Some(0.0) { continue; } // "SUB=0" -> silence
                return Err("constant values in Copy are not supported".into());
            }
            let src = channel_index(ch).ok_or_else(|| format!("unknown channel '{}'", ch))?;
            sources.push((src, factor));
        }
        routes.push((dest, sources));
    }
    if routes.is_empty() {
        return Err("empty Copy command".into());
    }
    Ok(routes)
}

fn parse_factor(f: &str) -> Option<f64> {
    let t = f.trim();
    if let Some(db) = t.strip_suffix("dB").or_else(|| t.strip_suffix("db")) {
        return parse_num(db).map(|d| 10f64.powf(d / 20.0));
    }
    parse_num(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_preamp_and_filters() {
        let cfg = parse(
            "Preamp: -6.5 dB\nFilter 1: ON PK Fc 1000 Hz Gain -3.0 dB Q 1.41\nFilter 2: OFF PK Fc 50 Hz Gain 3 dB Q 1\nFilter: ON LSC 12 dB Fc 105 Hz Gain 6 dB\n",
            None,
        );
        assert!(cfg.warnings.is_empty(), "{:?}", cfg.warnings);
        assert_eq!(cfg.stages.len(), 3);
        assert_eq!(cfg.stages[0].1, Stage::Gain { gain_db: -6.5, channels: ChannelSel::All });
        match &cfg.stages[1].1 {
            Stage::Biquad { spec, .. } => {
                assert_eq!(spec.kind, FilterKind::Peaking);
                assert_eq!(spec.freq, 1000.0);
                assert_eq!(spec.gain_db, -3.0);
                assert_eq!(spec.shape, Shape::Q(1.41));
            }
            s => panic!("{:?}", s),
        }
        match &cfg.stages[2].1 {
            Stage::Biquad { spec, .. } => assert_eq!(spec.shape, Shape::SlopeDbPerOct(12.0)),
            s => panic!("{:?}", s),
        }
    }

    #[test]
    fn channel_and_copy() {
        let cfg = parse("Channel: L\nPreamp: -3 dB\nChannel: all\nCopy: L=0.5*L+0.5*R R = -6dB*L + R\n", None);
        assert!(cfg.warnings.is_empty(), "{:?}", cfg.warnings);
        assert_eq!(cfg.stages[0].1, Stage::Gain { gain_db: -3.0, channels: ChannelSel::List(vec![0]) });
        match &cfg.stages[1].1 {
            Stage::Mix { routes } => {
                assert_eq!(routes[0], (0, vec![(0, 0.5), (1, 0.5)]));
                assert_eq!(routes[1].0, 1);
                assert!((routes[1].1[0].1 - 0.501187).abs() < 1e-5);
                assert_eq!(routes[1].1[1], (1, 1.0));
            }
            s => panic!("{:?}", s),
        }
    }

    #[test]
    fn device_blocks_and_conditionals_and_includes() {
        let inc = |name: &str| if name == "bass.txt" { Some("Filter: ON LS Fc 80 Hz Gain 4 dB".to_string()) } else { None };
        let cfg = parse(
            "Device: Headphones\nPreamp: -2 dB\nDevice: all\nInclude: bass.txt\nInclude: missing.txt\nIf: sampleRate == 44100\nPreamp: -99 dB\nEndIf:\nGraphicEQ: 25 0; 40 1\nFoo: bar\n",
            Some(&inc),
        );
        assert_eq!(cfg.stages.len(), 2);
        assert_eq!(cfg.stages_for_device("Speakers (Realtek)").len(), 1);
        assert_eq!(cfg.stages_for_device("Headphones (USB)").len(), 2);
        let reasons: Vec<_> = cfg.warnings.iter().map(|w| w.text.clone()).collect();
        assert_eq!(reasons, vec!["Include: missing.txt", "If: sampleRate == 44100", "GraphicEQ: 25 0; 40 1", "Foo: bar"]);
    }

    #[test]
    fn bandwidth_to_q() {
        assert!((bw_oct_to_q(1.0) - 1.4142).abs() < 1e-3);
        let cfg = parse("Filter: ON PK Fc 500 Hz Gain 2 dB BW Oct 1.0", None);
        assert!(cfg.warnings.is_empty());
    }

    #[test]
    fn rejects_bad_values() {
        let cfg = parse("Filter: ON PK Fc 0 Hz Gain 2 dB Q 1\nFilter: ON PK Fc 100 Hz Gain 2 dB Q -1\nFilter: ON XYZ Fc 100 Hz\n", None);
        assert!(cfg.stages.is_empty());
        assert_eq!(cfg.warnings.len(), 3);
    }
}

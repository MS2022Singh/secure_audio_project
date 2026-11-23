//! Parser for Equalizer APO 1.4.x configuration files (`config.txt` and include files).
//!
//! Every non-empty, non-comment line ends up in exactly one of two places:
//! a [`Stage`] in the parsed pipeline, or a [`Warning`] explaining why it was
//! not converted. Nothing is dropped silently.

use crate::expr::{self, Value, Vars};
use crate::watermark::{self, WatermarkSpec};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

/// Equalizer APO channel order for up to 7.1: L R C SUB RL RR SL SR (more channels by number).
pub const CHANNEL_NAMES: [&str; 8] = ["L", "R", "C", "SUB", "RL", "RR", "SL", "SR"];
pub const MAX_CHANNELS: usize = 32;

/// What the configuration is being compiled for. `If:`, `Eval:` and backtick expressions
/// are evaluated against these values, like Equalizer APO does.
#[derive(Debug, Clone)]
pub struct Context {
    pub sample_rate: u32,
    pub channels: usize,
    pub device_name: String,
    pub connection_name: String,
    pub device_guid: String,
}

impl Default for Context {
    fn default() -> Self {
        Context { sample_rate: 48000, channels: 2, device_name: String::new(), connection_name: String::new(), device_guid: String::new() }
    }
}

impl Context {
    fn vars(&self) -> Vars {
        let mut v = Vars::new();
        let n = |x: f64| Value::Num(x);
        let t = |x: &str| Value::Str(x.to_string());
        v.insert("sampleRate".into(), n(self.sample_rate as f64));
        v.insert("channelCount".into(), n(self.channels as f64));
        v.insert("inputChannelCount".into(), n(self.channels as f64));
        v.insert("outputChannelCount".into(), n(self.channels as f64));
        v.insert("deviceName".into(), t(&self.device_name));
        v.insert("connectionName".into(), t(&self.connection_name));
        v.insert("deviceGuid".into(), t(&self.device_guid));
        v.insert("deviceString".into(), t(&format!("{} {} {}", self.device_name, self.connection_name, self.device_guid)));
        v.insert("stage".into(), t("pre-mix"));
        v.insert("unofficial".into(), n(0.0));
        v
    }
}

/// An impulse response (per channel samples) and its sample rate.
#[derive(Debug, Clone, PartialEq)]
pub struct ImpulseResponse {
    pub name: String,
    pub sample_rate: u32,
    pub channels: Vec<Vec<f32>>,
}

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
    /// First-order shelves (`LS 6dB` / `HS 6dB`).
    LowShelf1,
    HighShelf1,
    /// First-order low/high pass (`LP1` / `HP1`).
    LowPass1,
    HighPass1,
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
    /// `Filter: ON IIR Order n Coefficients b0..bn a0..an` (normalised so a0 == 1).
    Iir { b: Vec<f64>, a: Vec<f64>, channels: ChannelSel },
    /// `GraphicEQ: f1 g1; f2 g2; ...` – linear-phase FIR, gains interpolated in dB over log frequency.
    GraphicEq { points: Vec<(f64, f64)>, channels: ChannelSel },
    /// `Convolution: file.wav` – impulse response uploaded with the configuration.
    Convolution { ir: Arc<ImpulseResponse>, channels: ChannelSel },
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

/// One open `If:` block.
struct Cond {
    parent_active: bool,
    taken: bool,
    active: bool,
}

struct ParseState {
    channels: ChannelSel,
    device: DeviceSel,
    conds: Vec<Cond>,
    vars: Vars,
    /// Files uploaded with the configuration (`DivioraFile:` lines): lower-case file name -> bytes.
    files: HashMap<String, Arc<Vec<u8>>>,
    chans: ChanMap,
}

/// Channel names: the standard ones, numbers, and virtual channels that the configuration
/// creates by name (`Copy: CX=0.5*L+0.5*R`, then `Channel: CX`, `Copy: L=L+CX`), like
/// Equalizer APO. Virtual channels get indices after the device's channels (at least 8),
/// exist only inside the engine and are never sent to the speakers.
pub struct ChanMap {
    virt: HashMap<String, usize>,
    next: usize,
}

impl ChanMap {
    pub fn new(device_channels: usize) -> ChanMap {
        ChanMap { virt: HashMap::new(), next: device_channels.max(8).min(MAX_CHANNELS) }
    }
    /// Index of a channel name; unknown names become new virtual channels when `create`.
    pub fn resolve(&mut self, name: &str, create: bool) -> Option<usize> {
        let n = name.trim();
        if let Some(i) = channel_index(n) {
            return Some(i);
        }
        let key = n.to_uppercase();
        if let Some(i) = self.virt.get(&key) {
            return Some(*i);
        }
        let valid = !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && key.chars().next().map_or(false, |c| c.is_ascii_alphabetic());
        if create && valid && self.next < MAX_CHANNELS {
            let i = self.next;
            self.next += 1;
            self.virt.insert(key, i);
            return Some(i);
        }
        None
    }
}

impl ParseState {
    fn active(&self) -> bool {
        self.conds.last().map_or(true, |c| c.active)
    }
}

/// Parse an Equalizer APO configuration text for a typical output (48 kHz, stereo, no
/// device name). Use [`parse_with`] when the real output is known.
pub fn parse(text: &str, resolver: Option<IncludeResolver>) -> EqApoConfig {
    parse_with(text, resolver, &Context::default())
}

/// Parse for a concrete output: `If:`/`Eval:`/backticks use `ctx`.
pub fn parse_with(text: &str, resolver: Option<IncludeResolver>, ctx: &Context) -> EqApoConfig {
    let mut cfg = EqApoConfig::default();
    let mut st = ParseState {
        channels: ChannelSel::All,
        device: DeviceSel::All,
        conds: Vec::new(),
        vars: ctx.vars(),
        files: embedded_files(text),
        chans: ChanMap::new(ctx.channels),
    };
    parse_into(text, "config", resolver, 0, &mut st, &mut cfg);
    if !st.conds.is_empty() {
        warn(&mut cfg, "config", 0, "If:", "missing EndIf: (block closed at end of file)");
    }
    cfg
}

/// `DivioraFile: name | base64` lines: files (include texts, impulse responses) uploaded
/// together with the configuration, so the locked configuration is self-contained.
pub fn embedded_files(text: &str) -> HashMap<String, Arc<Vec<u8>>> {
    let mut m = HashMap::new();
    for l in text.lines() {
        let l = l.trim();
        if l.len() > 12 && l[..12].eq_ignore_ascii_case("DivioraFile:") {
            if let Some((name, data)) = l[12..].rsplit_once('|') {
                if let Some(bytes) = watermark::base64_decode(data.trim()) {
                    m.insert(file_key(name), Arc::new(bytes));
                }
            }
        }
    }
    m
}

/// The line that embeds a file in a configuration.
pub fn embed_file_line(name: &str, bytes: &[u8]) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).replace('|', "_");
    format!("DivioraFile: {} | {}", base, watermark::base64_encode(bytes))
}

/// Files are matched by name only (Equalizer APO paths point into the developer's PC).
fn file_key(name: &str) -> String {
    let n = name.trim().trim_matches('"');
    n.rsplit(['/', '\\']).next().unwrap_or(n).to_lowercase()
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
        // "Filter 3" -> "filter"
        // "Filter 3" / "Filter3" (some editors omit the space) -> "filter"
        let cmd = cmd_full.split_whitespace().next().unwrap_or("").to_lowercase();
        let cmd = cmd.trim_end_matches(|c: char| c.is_ascii_digit()).to_string();
        let raw_args = line[colon + 1..].trim();

        // Conditional blocks, evaluated for the real output (sample rate, channels, device).
        match cmd.as_str() {
            "if" => {
                let parent = st.active();
                let v = if parent { cond_value(raw_args, st, cfg, file, lineno, line) } else { false };
                st.conds.push(Cond { parent_active: parent, taken: v, active: v });
                continue;
            }
            "elseif" => {
                let Some(top) = st.conds.last() else { warn(cfg, file, lineno, line, "ElseIf: without If:"); continue };
                let (parent, taken) = (top.parent_active, top.taken);
                let v = if parent && !taken { cond_value(raw_args, st, cfg, file, lineno, line) } else { false };
                let top = st.conds.last_mut().unwrap();
                top.active = v;
                top.taken |= v;
                continue;
            }
            "else" => {
                match st.conds.last_mut() {
                    Some(top) => { top.active = top.parent_active && !top.taken; top.taken = true; }
                    None => warn(cfg, file, lineno, line, "Else: without If:"),
                }
                continue;
            }
            "endif" => {
                if st.conds.pop().is_none() { warn(cfg, file, lineno, line, "EndIf: without If:"); }
                continue;
            }
            _ => {}
        }
        if !st.active() {
            continue;
        }
        if cmd == "eval" {
            if let Err(e) = expr::eval_assignments(raw_args, &mut st.vars) {
                warn(cfg, file, lineno, line, format!("Eval: {}", e));
            }
            continue;
        }
        // `expression` substitutions inside any other command.
        let args_owned = match expr::substitute_backticks(raw_args, &st.vars) {
            Ok(a) => a,
            Err(e) => { warn(cfg, file, lineno, line, format!("expression: {}", e)); continue; }
        };
        let args = args_owned.as_str();

        match cmd.as_str() {
            "preamp" => match parse_db(args) {
                Some(g) => cfg.stages.push((st.device.clone(), Stage::Gain { gain_db: g, channels: st.channels.clone() })),
                None => warn(cfg, file, lineno, line, "could not read preamp gain"),
            },
            "filter" => {
                // Optional "Channels=L,R" inside the line applies the filter to those channels only.
                let mut chans = st.channels.clone();
                let mut rest = Vec::new();
                let mut bad = None;
                for tok in args.split_whitespace() {
                    if tok.len() > 9 && tok[..9].eq_ignore_ascii_case("channels=") {
                        match parse_channel_list(&tok[9..].replace(',', " "), &mut st.chans) {
                            Some(c) => chans = c,
                            None => bad = Some(tok.to_string()),
                        }
                    } else {
                        rest.push(tok);
                    }
                }
                if let Some(b) = bad {
                    warn(cfg, file, lineno, line, format!("unknown channel in '{}'", b));
                    continue;
                }
                match parse_filter(&rest.join(" ")) {
                    Ok(Some(FilterDef::Biquad(spec))) => cfg.stages.push((st.device.clone(), Stage::Biquad { spec, channels: chans })),
                    Ok(Some(FilterDef::Iir(b, a))) => cfg.stages.push((st.device.clone(), Stage::Iir { b, a, channels: chans })),
                    Ok(None) => {} // filter switched OFF
                    Err(e) => warn(cfg, file, lineno, line, e),
                }
            }
            "channel" => match parse_channel_sel(args, &mut st.chans) {
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
            "copy" => match parse_copy(args, &mut st.chans) {
                Ok(routes) => cfg.stages.push((st.device.clone(), Stage::Mix { routes })),
                Err(e) => warn(cfg, file, lineno, line, e),
            },
            "delay" => match parse_delay(args, &mut st.chans) {
                Ok(list) => {
                    for (ms, sel) in list {
                        let channels = sel.unwrap_or_else(|| st.channels.clone());
                        cfg.stages.push((st.device.clone(), Stage::Delay { ms, channels }));
                    }
                }
                Err(e) => warn(cfg, file, lineno, line, e),
            },
            "include" => {
                if depth >= MAX_INCLUDE_DEPTH {
                    warn(cfg, file, lineno, line, "include nesting too deep");
                    continue;
                }
                let name = args.trim().trim_matches('"').to_string();
                let sub = resolver.and_then(|r| r(&name)).or_else(|| {
                    st.files.get(&file_key(&name)).map(|b| String::from_utf8_lossy(b).into_owned())
                });
                match sub {
                    Some(sub) => parse_into(&sub, &name, resolver, depth + 1, st, cfg),
                    None => warn(cfg, file, lineno, line, "included file not available: upload it together with the configuration"),
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
            "stage" => {} // Diviora runs as one stream effect; the placement hint has no effect
            "diviorafile" => {} // embedded file, collected before parsing
            "graphiceq" => match parse_graphic_eq(args) {
                Ok(points) => cfg.stages.push((st.device.clone(), Stage::GraphicEq { points, channels: st.channels.clone() })),
                Err(e) => warn(cfg, file, lineno, line, e),
            },
            "convolution" => {
                let name = args.trim().trim_matches('"');
                match st.files.get(&file_key(name)) {
                    Some(bytes) => match crate::wav::decode(bytes) {
                        Ok((rate, chans)) => {
                            let ir = ImpulseResponse { name: file_key(name), sample_rate: rate, channels: chans };
                            cfg.stages.push((st.device.clone(), Stage::Convolution { ir: Arc::new(ir), channels: st.channels.clone() }));
                        }
                        Err(e) => warn(cfg, file, lineno, line, format!("impulse response: {}", e)),
                    },
                    None => warn(cfg, file, lineno, line, "impulse response file not available: upload the .wav together with the configuration"),
                }
            }
            "loudnesscorrection" => warn(cfg, file, lineno, line, "LoudnessCorrection follows the Windows volume slider and is not supported; use filters instead"),
            "vst" => warn(cfg, file, lineno, line, "VST plug-ins cannot be loaded into the Diviora engine"),
            _ => warn(cfg, file, lineno, line, "unknown command"),
        }
    }
}


fn cond_value(args: &str, st: &ParseState, cfg: &mut EqApoConfig, file: &str, lineno: usize, line: &str) -> bool {
    match expr::eval(args, &st.vars) {
        Ok(v) => v.truthy(),
        Err(e) => {
            warn(cfg, file, lineno, line, format!("condition could not be evaluated ({}); treated as false", e));
            false
        }
    }
}

/// `GraphicEQ: 25 -1.5; 40 0; 63 2.1` -> [(Hz, dB)], sorted by frequency.
fn parse_graphic_eq(args: &str) -> Result<Vec<(f64, f64)>, String> {
    let mut pts = Vec::new();
    for part in args.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let mut it = part.split_whitespace();
        let f = it.next().and_then(parse_num).ok_or_else(|| format!("bad GraphicEQ point '{}'", part))?;
        let g = it.next().and_then(parse_db).ok_or_else(|| format!("bad GraphicEQ point '{}'", part))?;
        if f <= 0.0 || !(-100.0..=40.0).contains(&g) {
            return Err(format!("GraphicEQ point '{}' out of range", part));
        }
        pts.push((f, g));
    }
    if pts.is_empty() {
        return Err("empty GraphicEQ".into());
    }
    pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    Ok(pts)
}

/// Gain of a GraphicEQ at `f`: linear in dB over log frequency, constant outside the points.
pub fn graphic_eq_gain(points: &[(f64, f64)], f: f64) -> f64 {
    if points.is_empty() { return 0.0; }
    if f <= points[0].0 { return points[0].1; }
    let last = points[points.len() - 1];
    if f >= last.0 { return last.1; }
    let i = points.partition_point(|p| p.0 <= f);
    let (a, b) = (points[i - 1], points[i]);
    if b.0 <= a.0 { return b.1; }
    let t = (f.ln() - a.0.ln()) / (b.0.ln() - a.0.ln());
    a.1 + t * (b.1 - a.1)
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

/// "10 ms" / "480 samples" (selected channels), or per channel: "L=5ms R=3 ms CX=2.5ms".
/// Returns (milliseconds, channels or None = the current Channel selection).
fn parse_delay(s: &str, chans: &mut ChanMap) -> Result<Vec<(f64, Option<ChannelSel>)>, String> {
    // Samples are converted at 48 kHz (the parser does not know the stream rate here).
    let to_ms = |v: f64, unit: &str| -> Result<f64, String> {
        let ms = match unit.to_lowercase().as_str() {
            "" | "ms" => v,
            "samples" | "sample" => v / 48.0,
            u => return Err(format!("unknown delay unit '{}'", u)),
        };
        if ms < 0.0 { Err("negative delay".into()) } else { Ok(ms) }
    };
    let split_unit = |t: &str| -> (String, String) {
        let i = t.find(|c: char| c.is_ascii_alphabetic() && c != 'e' && c != 'E').unwrap_or(t.len());
        (t[..i].to_string(), t[i..].to_string())
    };
    let toks: Vec<&str> = s.split_whitespace().collect();
    if !toks.iter().any(|t| t.contains('=')) {
        let first = toks.first().ok_or("missing delay value")?;
        let (num, glued) = split_unit(first);
        let v = parse_num(&num).ok_or("could not read delay (expected '<n> ms' or '<n> samples')")?;
        let unit = if !glued.is_empty() { glued } else { toks.get(1).map(|u| u.to_string()).unwrap_or_default() };
        return Ok(vec![(to_ms(v, &unit)?, None)]);
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let (name, val) = toks[i].split_once('=').ok_or_else(|| format!("'{}' is not CHANNEL=delay", toks[i]))?;
        let idx = chans.resolve(name, true).ok_or_else(|| format!("unknown channel '{}'", name))?;
        let (num, glued) = split_unit(val);
        let v = parse_num(&num).ok_or_else(|| format!("bad delay '{}'", val))?;
        let mut unit = glued;
        if unit.is_empty() {
            if let Some(u) = toks.get(i + 1) {
                if !u.contains('=') { unit = u.to_string(); i += 1; }
            }
        }
        out.push((to_ms(v, &unit)?, Some(ChannelSel::List(vec![idx]))));
        i += 1;
    }
    Ok(out)
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

fn parse_channel_sel(s: &str, chans: &mut ChanMap) -> Option<ChannelSel> {
    let t = s.trim();
    if t.eq_ignore_ascii_case("all") {
        return Some(ChannelSel::All);
    }
    parse_channel_list(t, chans)
}

fn parse_channel_list(t: &str, chans: &mut ChanMap) -> Option<ChannelSel> {
    let mut v = Vec::new();
    for tok in t.split_whitespace() {
        v.push(chans.resolve(tok, true)?);
    }
    if v.is_empty() { None } else { Some(ChannelSel::List(v)) }
}

/// Q from bandwidth in octaves.
pub fn bw_oct_to_q(n: f64) -> f64 {
    let p = 2f64.powf(n);
    p.sqrt() / (p - 1.0)
}

pub enum FilterDef {
    Biquad(BiquadSpec),
    Iir(Vec<f64>, Vec<f64>),
}

/// `IIR Order 2 Coefficients b0 b1 b2 a0 a1 a2`
fn parse_iir(toks: &[&str]) -> Result<FilterDef, String> {
    let mut order = None;
    let mut coefs = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        if toks[i].eq_ignore_ascii_case("Order") {
            order = toks.get(i + 1).and_then(|v| v.parse::<usize>().ok());
            i += 2;
        } else if toks[i].eq_ignore_ascii_case("Coefficients") {
            coefs = toks[i + 1..].iter().map(|t| parse_num(t)).collect::<Option<Vec<f64>>>()
                .ok_or("bad IIR coefficient")?;
            break;
        } else {
            i += 1;
        }
    }
    let order = order.ok_or("IIR filter without 'Order'")?;
    if order == 0 || order > 32 { return Err(format!("IIR order {} not supported (1..32)", order)); }
    if coefs.len() != 2 * (order + 1) {
        return Err(format!("IIR order {} needs {} coefficients, found {}", order, 2 * (order + 1), coefs.len()));
    }
    let (b, a) = coefs.split_at(order + 1);
    if a[0] == 0.0 { return Err("IIR coefficient a0 must not be 0".into()); }
    let a0 = a[0];
    Ok(FilterDef::Iir(b.iter().map(|v| v / a0).collect(), a.iter().map(|v| v / a0).collect()))
}

fn parse_filter(args: &str) -> Result<Option<FilterDef>, String> {
    let toks: Vec<&str> = args.split_whitespace().collect();
    if let Some(p) = toks.iter().position(|t| t.eq_ignore_ascii_case("IIR")) {
        if toks.first().map_or(false, |t| t.eq_ignore_ascii_case("OFF")) { return Ok(None); }
        return parse_iir(&toks[p + 1..]).map(Some);
    }
    parse_biquad(&toks).map(|o| o.map(FilterDef::Biquad))
}

fn parse_biquad(toks: &[&str]) -> Result<Option<BiquadSpec>, String> {
    let mut i = 0;
    match toks.first().map(|t| t.to_uppercase()) {
        Some(ref t) if t == "ON" => i += 1,
        Some(ref t) if t == "OFF" => return Ok(None),
        _ => {}
    }
    let ty = toks.get(i).ok_or("missing filter type")?.to_uppercase();
    i += 1;

    let kind = match ty.as_str() {
        "PK" | "PEQ" | "PEAK" | "PEAKING" | "MODAL" | "EQ" => FilterKind::Peaking,
        "LS" | "LSC" | "LSQ" | "LOWSHELF" => FilterKind::LowShelf,
        "HS" | "HSC" | "HSQ" | "HIGHSHELF" => FilterKind::HighShelf,
        "LP" | "LPQ" | "LPF" | "LOWPASS" => FilterKind::LowPass,
        "HP" | "HPQ" | "HPF" | "HIGHPASS" => FilterKind::HighPass,
        "LP1" | "LPF1" => FilterKind::LowPass1,
        "HP1" | "HPF1" => FilterKind::HighPass1,
        "BP" | "BPQ" | "BANDPASS" => FilterKind::BandPass,
        "NO" | "NOF" | "NOTCH" => FilterKind::Notch,
        "AP" | "APQ" | "ALLPASS" => FilterKind::AllPass,
        "NONE" => return Ok(None),
        other => return Err(format!("filter type '{}' not supported", other)),
    };
    // Plain "LS"/"HS" followed by "6dB" are first-order shelves; "12dB" is the usual
    // second-order shelf (Q 0.7071).
    let plain_shelf = ty == "LS" || ty == "HS";
    let mut kind = kind;

    let mut freq = None;
    let mut gain = 0.0;
    let mut shape: Option<Shape> = None;

    // Shelves may carry a slope right after the type: "LSC 12 dB Fc ..." / "LS 6dB Fc ...".
    if matches!(kind, FilterKind::LowShelf | FilterKind::HighShelf) {
        let mut slope = None;
        if let Some(t) = toks.get(i) {
            if let Some(s) = t.strip_suffix("dB").or_else(|| t.strip_suffix("db")).and_then(parse_num) {
                slope = Some(s);
                i += 1;
            } else if let (Some(v), Some(u)) = (parse_num(t), toks.get(i + 1)) {
                if u.eq_ignore_ascii_case("dB") {
                    slope = Some(v);
                    i += 2;
                }
            }
        }
        match slope {
            Some(s) if plain_shelf && (s - 6.0).abs() < 1e-9 => {
                kind = if kind == FilterKind::LowShelf { FilterKind::LowShelf1 } else { FilterKind::HighShelf1 };
            }
            Some(s) if plain_shelf && (s - 12.0).abs() < 1e-9 => shape = Some(Shape::Q(std::f64::consts::FRAC_1_SQRT_2)),
            Some(s) => shape = Some(Shape::SlopeDbPerOct(s)),
            None => {}
        }
    }

    while i < toks.len() {
        let key = toks[i].to_uppercase();
        match key.as_str() {
            "FC" => {
                // "Fc 1000 Hz", "Fc 1000Hz", "Fc 1.2 kHz", "Fc 1.2kHz"
                let raw = toks.get(i + 1).copied().unwrap_or("");
                let low = raw.to_lowercase();
                let (num, mut mult) = if let Some(n) = low.strip_suffix("khz") { (n, 1000.0) }
                    else if let Some(n) = low.strip_suffix("hz") { (n, 1.0) } else { (low.as_str(), 1.0) };
                let v = parse_num(num);
                i += 2;
                match toks.get(i).map(|u| u.to_lowercase()) {
                    Some(u) if u == "hz" => i += 1,
                    Some(u) if u == "khz" => { mult = 1000.0; i += 1; }
                    _ => {}
                }
                freq = v.map(|f| f * mult);
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
    // Equalizer APO accepts wide ranges; frequencies above Nyquist are clamped by the engine,
    // and the output safety clip protects the speakers from extreme gains.
    if !(freq > 0.0 && freq <= 1_000_000.0) {
        return Err(format!("frequency {} Hz out of range", freq));
    }
    if !(-200.0..=200.0).contains(&gain) {
        return Err(format!("gain {} dB out of range", gain));
    }
    let shape = shape.unwrap_or(Shape::Q(std::f64::consts::FRAC_1_SQRT_2));
    match shape {
        Shape::Q(q) if !(q > 0.0 && q <= 10_000.0) => return Err(format!("Q {} out of range", q)),
        Shape::SlopeDbPerOct(s) if !(s > 0.0 && s <= 96.0) => return Err(format!("slope {} dB/oct out of range", s)),
        _ => {}
    }
    Ok(Some(BiquadSpec { kind, freq, gain_db: gain, shape }))
}

/// `Copy: L=0.5*L+0.5*R R=L C=-6dB*L+-6dB*R SUB=0`
fn parse_copy(args: &str, chans: &mut ChanMap) -> Result<Vec<(usize, Vec<(usize, f64)>)>, String> {
    // Remove spaces around operators so assignments become single tokens.
    let mut s = args.to_string();
    for op in ["=", "+", "*"] {
        while s.contains(&format!(" {}", op)) { s = s.replace(&format!(" {}", op), op); }
        while s.contains(&format!("{} ", op)) { s = s.replace(&format!("{} ", op), op); }
    }
    let mut routes = Vec::new();
    for assign in s.split_whitespace() {
        let (dest, expr) = assign.split_once('=').ok_or_else(|| format!("'{}' is not an assignment", assign))?;
        let dest = chans.resolve(dest, true).ok_or_else(|| format!("unknown channel '{}'", dest))?;
        let mut sources = Vec::new();
        // "L-R" / "0.5*L-0.5*R": subtraction is addition of a negated term (but not the
        // minus of a negative factor or exponent such as "-6dB*L" or "1e-3*L").
        let mut norm = String::new();
        let chars: Vec<char> = expr.chars().collect();
        for (k, c) in chars.iter().enumerate() {
            if *c == '-' && k > 0 && !matches!(chars[k - 1], '+' | '*' | 'e' | 'E' | '=') {
                norm.push('+');
            }
            norm.push(*c);
        }
        for term in norm.split('+').filter(|t| !t.is_empty()) {
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
            let src = chans.resolve(ch, true).ok_or_else(|| format!("unknown channel '{}'", ch))?;
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
        // If: is evaluated (48 kHz here, so its block is skipped); GraphicEQ is converted.
        assert_eq!(cfg.stages.len(), 3);
        assert_eq!(cfg.stages_for_device("Speakers (Realtek)").len(), 2);
        assert_eq!(cfg.stages_for_device("Headphones (USB)").len(), 3);
        let reasons: Vec<_> = cfg.warnings.iter().map(|w| w.text.clone()).collect();
        assert_eq!(reasons, vec!["Include: missing.txt", "Foo: bar"]);
    }

    #[test]
    fn copy_with_subtraction_and_wide_ranges() {
        let cfg = parse("Copy: L=L-R R=0.5*R-0.5*L C=-6dB*L+-6dB*R\nFilter: ON PK Fc 30000 Hz Gain 48 dB Q 150\n", None);
        assert!(cfg.warnings.is_empty(), "{:?}", cfg.warnings);
        match &cfg.stages[0].1 {
            Stage::Mix { routes } => {
                assert_eq!(routes[0], (0, vec![(0, 1.0), (1, -1.0)]));
                assert_eq!(routes[1], (1, vec![(1, 0.5), (0, -0.5)]));
                assert!((routes[2].1[0].1 - 0.501187).abs() < 1e-5);
            }
            s => panic!("{:?}", s),
        }
    }

    #[test]
    fn editor_variants() {
        let cfg = parse("Filter1: ON PK Fc 1000Hz Gain 3dB Q 1.41\nFilter 2: ON PK Fc 1.2 kHz Gain -2 dB Q 2\nFilter3: ON LP1 Fc 18kHz\nFilter: ON NOTCH Fc 50 Hz Q 30\n", None);
        assert!(cfg.warnings.is_empty(), "{:?}", cfg.warnings);
        assert_eq!(cfg.stages.len(), 4);
        match &cfg.stages[1].1 { Stage::Biquad { spec, .. } => assert_eq!(spec.freq, 1200.0), s => panic!("{:?}", s) }
        match &cfg.stages[2].1 { Stage::Biquad { spec, .. } => assert_eq!((spec.kind, spec.freq), (FilterKind::LowPass1, 18000.0)), s => panic!("{:?}", s) }
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

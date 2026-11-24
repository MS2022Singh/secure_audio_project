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
    /// Shelf slope in dB per octave (S = slope / 12).
    SlopeDbPerOct(f64),
    /// Bandwidth in octaves (`BW Oct n`), Equalizer APO's digital formula.
    BwOct(f64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiquadSpec {
    pub kind: FilterKind,
    pub freq: f64,
    pub gain_db: f64,
    pub shape: Shape,
    /// `LS`/`HS` (not `LSC`/`HSC`): `freq` is the corner frequency, as in Equalizer APO.
    pub corner: bool,
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
    /// `device` is the output's device string: its name ("Speakers (Realtek(R) Audio)") and,
    /// where known, its endpoint GUID. Like Equalizer APO, a `Device:` pattern (for example
    /// "Realtek(R) Audio Speakers {4134526c-...}", as its editor writes them) matches when
    /// every word of it occurs in the device string (any order, case-insensitive, `*` = any).
    pub fn matches(&self, device: &str) -> bool {
        match self {
            DeviceSel::All => true,
            DeviceSel::Matching(pats) => pats.iter().any(|p| device_pattern_matches(p, device)),
        }
    }
}

fn device_words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',' || c == ';')
        .map(|w| w.trim_matches(|c: char| c == '*' || c == '"').to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

fn device_pattern_matches(pattern: &str, device: &str) -> bool {
    let d = device.to_lowercase();
    let p = pattern.trim().to_lowercase();
    if p.is_empty() || p == "all" || p == "*" {
        return true;
    }
    if d.contains(&p) {
        return true;
    }
    let dev_words = device_words(device);
    let pat_words = device_words(pattern);
    !pat_words.is_empty()
        && pat_words.iter().all(|w| {
            // "2- Realtek(R)" style prefixes Windows adds to duplicate names are ignored.
            dev_words.iter().any(|dw| dw == w || dw.contains(w.as_str()))
                || (w.ends_with('-') && w[..w.len() - 1].chars().all(|c| c.is_ascii_digit()))
        })
}

#[cfg(test)]
mod device_tests {
    use super::*;

    #[test]
    fn equalizer_apo_device_strings_match() {
        let dev = "Speakers (2- Realtek(R) Audio) {4134526c-7b35-49cd-b0a0-4c58c090de43}";
        let m = |p: &str| DeviceSel::Matching(vec![p.to_string()]).matches(dev);
        assert!(m("Realtek(R) Audio Speakers {4134526c-7b35-49cd-b0a0-4c58c090de43}"));
        assert!(m("Speakers Realtek"));
        assert!(m("{4134526C-7B35-49CD-B0A0-4C58C090DE43}"));
        assert!(m("Speakers*"));
        assert!(!m("Headphones"));
        assert!(!m("Realtek(R) Audio Speakers {00000000-0000-0000-0000-000000000000}"));
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
    /// The device's channel names, as Equalizer APO names them for this channel count.
    names: Vec<String>,
    /// Virtual channels created by `Copy` (upper-case name -> index after the device channels).
    virt: HashMap<String, usize>,
    next: usize,
    /// Diviora extensions (`DivioraExtensions: on`).
    pub extended: bool,
}

/// Equalizer APO's names for the default speaker layout of `n` channels
/// (helpers/ChannelHelper.cpp): stereo L R; quad L R RL RR; 5.1 L R C LFE SL SR;
/// 7.1 L R C LFE RL RR SL SR; otherwise numbers.
pub fn standard_channel_names(n: usize) -> Vec<String> {
    let v: &[&str] = match n {
        1 => &["C"],
        2 => &["L", "R"],
        4 => &["L", "R", "RL", "RR"],
        6 => &["L", "R", "C", "LFE", "SL", "SR"],
        8 => &["L", "R", "C", "LFE", "RL", "RR", "SL", "SR"],
        _ => &[],
    };
    if v.is_empty() {
        (1..=n).map(|i| i.to_string()).collect()
    } else {
        v.iter().map(|s| s.to_string()).collect()
    }
}

impl ChanMap {
    pub fn new(device_channels: usize) -> ChanMap {
        let n = device_channels.clamp(1, MAX_CHANNELS);
        ChanMap { names: standard_channel_names(n), virt: HashMap::new(), next: n, extended: false }
    }
    pub fn device_channels(&self) -> usize { self.names.len() }
    /// All channels that exist now (device + virtual), for "Channel: all".
    pub fn all_now(&self) -> ChannelSel { ChannelSel::List((0..self.next).collect()) }
    /// The device channels (selection before any Channel: line).
    pub fn device_sel(&self) -> ChannelSel { ChannelSel::List((0..self.names.len()).collect()) }

    /// Equalizer APO's lookup (ChannelHelper::getChannelIndex): number (from 1), name, the
    /// SL/RL, SR/RR and SUB/LFE fallbacks, then virtual channels.
    pub fn lookup(&self, name: &str) -> Option<usize> {
        let w = name.trim().to_uppercase();
        if w.is_empty() { return None; }
        if w.as_bytes()[0].is_ascii_digit() {
            let k: usize = w.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()?;
            return if k >= 1 && k <= self.next { Some(k - 1) } else { None };
        }
        let find = |n: &str| self.names.iter().position(|x| x == n);
        find(&w)
            .or_else(|| match w.as_str() {
                "SL" => find("RL"), "SR" => find("RR"), "RL" => find("SL"), "RR" => find("SR"),
                "SUB" => find("LFE"), _ => None,
            })
            .or_else(|| self.virt.get(&w).copied())
    }

    /// Lookup; when `create`, an unknown name becomes a new virtual channel (Equalizer APO does
    /// this for Copy targets).
    pub fn resolve(&mut self, name: &str, create: bool) -> Option<usize> {
        if let Some(i) = self.lookup(name) {
            return Some(i);
        }
        let key = name.trim().to_uppercase();
        if create && !key.is_empty() && !key.as_bytes()[0].is_ascii_digit() && self.next < MAX_CHANNELS {
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
        channels: ChannelSel::All, // replaced below by the device channels
        device: DeviceSel::All,
        conds: Vec::new(),
        vars: ctx.vars(),
        files: embedded_files(text),
        chans: ChanMap::new(ctx.channels),
    };
    st.channels = st.chans.device_sel();
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
                let has_channels_param = args.split_whitespace().any(|t| t.len() > 9 && t[..9].eq_ignore_ascii_case("channels="));
                let mut channels_note = false;
                for tok in args.split_whitespace() {
                    if tok.len() > 9 && tok[..9].eq_ignore_ascii_case("channels=") && !st.chans.extended {
                        // Equalizer APO does not read it: the filter uses the current Channel: selection.
                        channels_note = true;
                        continue;
                    }
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
                let _ = has_channels_param;
                match parse_filter(&rest.join(" "), st.chans.extended) {
                    Ok(Some(def)) => {
                        match def {
                            FilterDef::Biquad(spec) => cfg.stages.push((st.device.clone(), Stage::Biquad { spec, channels: chans })),
                            FilterDef::Iir(b, a) => cfg.stages.push((st.device.clone(), Stage::Iir { b, a, channels: chans })),
                        }
                        if channels_note {
                            warn(cfg, file, lineno, line, "partly applied, as in Equalizer APO 1.4.2: 'Channels=' is not read; the filter uses the current Channel: selection".to_string());
                        }
                    }
                    Ok(None) => {} // filter switched OFF
                    Err(e) => warn(cfg, file, lineno, line, e),
                }
            }
            "channel" => {
                // Equalizer APO selects the names that exist (device or virtual channels) and
                // silently skips the others; with none left, nothing is selected.
                let (sel, unknown) = parse_channel_sel(args, &mut st.chans);
                st.channels = sel;
                if !unknown.is_empty() {
                    warn(cfg, file, lineno, line, ignored(&format!("channel(s) {} do not exist at this point and are skipped", unknown.join(", "))));
                }
            }
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
                Ok((routes, notes)) => {
                    cfg.stages.push((st.device.clone(), Stage::Mix { routes }));
                    if !notes.is_empty() {
                        warn(cfg, file, lineno, line, format!("partly applied, as in Equalizer APO 1.4.2: {}", notes.join("; ")));
                    }
                }
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
            "divioraextensions" => {
                let a = args.trim().to_lowercase();
                st.chans.extended = matches!(a.as_str(), "on" | "true" | "yes" | "1");
            }
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
            _ => warn(cfg, file, lineno, line, ignored("unknown command")),
        }
    }
}


/// Reason text for lines Equalizer APO 1.4.2 itself silently ignores.
fn ignored(why: &str) -> String {
    format!("ignored, as in Equalizer APO 1.4.2: {}", why)
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
    if !chans.extended {
        return Err(ignored("Delay takes one value for the selected channels ('Delay: 10 ms'), not CHANNEL=value pairs"));
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


/// `Channel:` words -> (selection, unknown names). "all" = every channel existing now.
fn parse_channel_sel(s: &str, chans: &mut ChanMap) -> (ChannelSel, Vec<String>) {
    let mut v: Vec<usize> = Vec::new();
    let mut unknown = Vec::new();
    for tok in s.split_whitespace() {
        if tok.eq_ignore_ascii_case("all") {
            if let ChannelSel::List(all) = chans.all_now() { v = all; }
            continue;
        }
        // Extension: Channel: may create virtual channels; Equalizer APO only selects.
        let create = chans.extended;
        match chans.resolve(tok, create) {
            Some(i) => if !v.contains(&i) { v.push(i) },
            None => unknown.push(tok.to_string()),
        }
    }
    v.sort_unstable();
    (ChannelSel::List(v), unknown)
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

fn parse_filter(args: &str, extended: bool) -> Result<Option<FilterDef>, String> {
    let toks: Vec<&str> = args.split_whitespace().collect();
    if let Some(p) = toks.iter().position(|t| t.eq_ignore_ascii_case("IIR")) {
        if toks.first().map_or(false, |t| t.eq_ignore_ascii_case("OFF")) { return Ok(None); }
        return parse_iir(&toks[p + 1..]).map(Some);
    }
    parse_biquad(&toks, extended).map(|o| o.map(FilterDef::Biquad))
}

/// Leading number of a string, like C's wcstod (0.0 if none).
fn leading_num(s: &str) -> f64 {
    let t = s.trim_start();
    let mut end = 0;
    let b = t.as_bytes();
    while end < b.len() && (b[end].is_ascii_digit() || matches!(b[end], b'.' | b'-' | b'+' | b'e' | b'E')) {
        end += 1;
    }
    while end > 0 {
        if let Ok(v) = t[..end].parse::<f64>() {
            return v;
        }
        end -= 1;
    }
    0.0
}

/// Equalizer APO's frequency reader: "1.000" with 3 decimals is a thousands separator (Room EQ Wizard).
fn eqapo_freq(s: &str) -> f64 {
    let t: String = s.chars().filter(|c| *c != '\u{00A0}').collect();
    let v = leading_num(&t);
    if t.len() >= 5 && !t.contains(['e', 'E']) && t.as_bytes()[t.len() - 4] == b'.' {
        return v * 1000.0;
    }
    v
}

/// A `Filter:` line, exactly as Equalizer APO 1.4 reads it (filters/BiQuadFilterFactory.cpp).
fn parse_biquad(toks: &[&str], extended: bool) -> Result<Option<BiquadSpec>, String> {
    use std::sync::OnceLock;
    static RE: OnceLock<[regex_lite::Regex; 6]> = OnceLock::new();
    let re = RE.get_or_init(|| [
        regex_lite::Regex::new(r"^\s*ON\s+([A-Za-z]+)").unwrap(),
        regex_lite::Regex::new(r"\s+Fc\s*([-+0-9.eE\x{00A0}]+)\s*H\s*z").unwrap(),
        regex_lite::Regex::new(r"\s+Gain\s*([-+0-9.eE]+)\s*dB").unwrap(),
        regex_lite::Regex::new(r"\s+Q\s*([-+0-9.eE]+)").unwrap(),
        regex_lite::Regex::new(r"\s+BW\s+Oct\s*([-+0-9.eE]+)").unwrap(),
        regex_lite::Regex::new(r"^\s*([-+0-9.eE]+)\s*dB").unwrap(),
    ]);
    // Equalizer APO converts ',' to '.' first.
    let mut params = toks.join(" ").replace(',', ".");
    if toks.first().map_or(false, |t| t.eq_ignore_ascii_case("OFF")) {
        return Ok(None);
    }
    if extended && !toks.first().map_or(false, |t| t.eq_ignore_ascii_case("ON")) {
        params = format!("ON {}", params); // extension: "ON" may be omitted
    }
    let Some(m) = re[0].captures(&params) else {
        return Err(ignored("a Filter line must start with 'ON <type>'"));
    };
    let mut ty_raw = m.get(1).unwrap().as_str().to_string();
    let mut rest = params[m.get(0).unwrap().end()..].to_string();
    // Extension: first-order "LP1"/"HP1" (Equalizer APO reads the letters only: "LP").
    if extended && rest.starts_with('1') && matches!(ty_raw.to_uppercase().as_str(), "LP" | "HP" | "LPF" | "HPF") {
        ty_raw.push('1');
        rest = rest[1..].to_string();
    }
    let ty = ty_raw.to_uppercase();

    let kind = match ty.as_str() {
        "PK" | "PEQ" | "MODAL" => FilterKind::Peaking,
        "LP" | "LPQ" => FilterKind::LowPass,
        "HP" | "HPQ" => FilterKind::HighPass,
        "BP" => FilterKind::BandPass,
        "LS" | "LSC" => FilterKind::LowShelf,
        "HS" | "HSC" => FilterKind::HighShelf,
        "NO" => FilterKind::Notch,
        "AP" => FilterKind::AllPass,
        "NONE" => return Ok(None),
        // Diviora extensions (only with "DivioraExtensions: on").
        "NOF" | "NOTCH" if extended => FilterKind::Notch,
        "PEAK" | "PEAKING" | "EQ" if extended => FilterKind::Peaking,
        "LSQ" | "LOWSHELF" if extended => FilterKind::LowShelf,
        "HSQ" | "HIGHSHELF" if extended => FilterKind::HighShelf,
        "LPF" | "LOWPASS" if extended => FilterKind::LowPass,
        "HPF" | "HIGHPASS" if extended => FilterKind::HighPass,
        "LP1" | "LPF1" if extended => FilterKind::LowPass1,
        "HP1" | "HPF1" if extended => FilterKind::HighPass1,
        "BPQ" | "BANDPASS" if extended => FilterKind::BandPass,
        "APQ" | "ALLPASS" if extended => FilterKind::AllPass,
        _ => return Err(ignored(&format!("'{}' is not an Equalizer APO filter type", ty_raw))),
    };
    let is_shelf = matches!(kind, FilterKind::LowShelf | FilterKind::HighShelf);

    let freq = match re[1].captures(&rest) {
        Some(c) => eqapo_freq(c.get(1).unwrap().as_str()),
        None if extended => {
            // Extension: "Fc 1.2 kHz" / "Fc 18kHz".
            let k = regex_lite::Regex::new(r"(?i)\s+Fc\s*([-+0-9.eE]+)\s*kHz").unwrap();
            match k.captures(&rest) {
                Some(c) => leading_num(c.get(1).unwrap().as_str()) * 1000.0,
                None => return Err(ignored("no frequency given ('Fc <n> Hz')")),
            }
        }
        None => return Err(ignored("no frequency given ('Fc <n> Hz')")),
    };
    let mut gain = 0.0;
    match re[2].captures(&rest) {
        Some(c) => {
            if !matches!(kind, FilterKind::LowPass | FilterKind::HighPass | FilterKind::Notch | FilterKind::AllPass
                         | FilterKind::LowPass1 | FilterKind::HighPass1) {
                gain = leading_num(c.get(1).unwrap().as_str());
            }
        }
        None if matches!(kind, FilterKind::Peaking) || is_shelf => return Err(ignored("no gain given ('Gain <n> dB')")),
        None => {}
    }
    let mut value = 0.0;
    let mut is_bw_or_s = false;
    if let Some(c) = re[3].captures(&rest) {
        value = leading_num(c.get(1).unwrap().as_str());
    }
    if let Some(c) = re[4].captures(&rest) {
        if !is_shelf {
            value = leading_num(c.get(1).unwrap().as_str());
            is_bw_or_s = true;
        }
    }
    if let Some(c) = re[5].captures(&rest) {
        if is_shelf {
            value = leading_num(c.get(1).unwrap().as_str());
            is_bw_or_s = true;
        }
    }
    let shape = if value == 0.0 {
        match kind {
            FilterKind::Peaking | FilterKind::AllPass => return Err(ignored("no Q or bandwidth given")),
            FilterKind::LowShelf | FilterKind::HighShelf => Shape::SlopeDbPerOct(0.9 * 12.0), // S = 0.9
            FilterKind::Notch => Shape::Q(30.0),
            _ => Shape::Q(std::f64::consts::FRAC_1_SQRT_2),
        }
    } else if is_shelf {
        if is_bw_or_s { Shape::SlopeDbPerOct(value) } else { Shape::Q(value) }
    } else if is_bw_or_s {
        Shape::BwOct(value)
    } else {
        Shape::Q(value)
    };
    if !(freq > 0.0 && freq.is_finite()) {
        return Err(ignored("frequency must be above 0 Hz"));
    }
    // A zero or negative Q/bandwidth/slope would make an unstable filter: never applied.
    match shape {
        Shape::Q(v) | Shape::BwOct(v) | Shape::SlopeDbPerOct(v) if !(v > 0.0 && v.is_finite()) => {
            return Err(format!("Q, bandwidth or slope {} is not usable (must be above 0)", v));
        }
        _ => {}
    }
    let corner = is_shelf && !ty.ends_with('C');
    Ok(Some(BiquadSpec { kind, freq, gain_db: gain, shape, corner }))
}

/// `Copy: L=0.5*L+0.5*R R=L C=-6dB*L+-6dB*R LFE=0`, exactly as Equalizer APO 1.4 reads it
/// (filters/CopyFilterFactory.cpp, CopyFilter.cpp): assignments separated by spaces, summands
/// by '+', factor '*' channel; a factor ending in "dB" is in decibels; a lone "0" or a number
/// with '.' is a constant. An unknown target becomes a virtual channel. Returns the routes and
/// notes about parts that have no effect here.
fn parse_copy(args: &str, chans: &mut ChanMap) -> Result<(Vec<(usize, Vec<(usize, f64)>)>, Vec<String>), String> {
    let mut s = args.to_string();
    if chans.extended {
        // Extension: spaces around operators are allowed.
        for op in ["=", "+", "*"] {
            while s.contains(&format!(" {}", op)) { s = s.replace(&format!(" {}", op), op); }
            while s.contains(&format!("{} ", op)) { s = s.replace(&format!("{} ", op), op); }
        }
    }
    let mut routes = Vec::new();
    let mut notes = Vec::new();
    for assign in s.split_whitespace() {
        let parts: Vec<&str> = assign.split('=').collect();
        if parts.len() != 2 {
            notes.push(format!("'{}' is not an assignment", assign));
            continue;
        }
        let (target, mut source) = (parts[0], parts[1].to_string());
        if target.is_empty() {
            continue;
        }
        if chans.extended {
            // Extension: "L-R" subtraction.
            let chars: Vec<char> = source.chars().collect();
            let mut norm = String::new();
            for (k, c) in chars.iter().enumerate() {
                if *c == '-' && k > 0 && !matches!(chars[k - 1], '+' | '*' | 'e' | 'E' | '=') { norm.push('+'); }
                norm.push(*c);
            }
            source = norm;
        }
        let mut sources = Vec::new();
        for summand in source.split('+') {
            let f: Vec<&str> = summand.split('*').collect();
            let (factor_s, channel) = match f.len() {
                2 => (f[0], f[1]),
                1 if f[0] == "0" || f[0].contains('.') => (f[0], ""),
                1 => ("", f[0]),
                _ => { notes.push(format!("'{}' has more than one '*'", summand)); continue; }
            };
            let mut factor = if factor_s.is_empty() { 1.0 } else { leading_num(factor_s) };
            if factor_s.len() > 2 && factor_s[factor_s.len() - 2..].eq_ignore_ascii_case("db") {
                factor = 10f64.powf(factor / 20.0);
            }
            if channel.is_empty() {
                if factor != 0.0 {
                    notes.push(format!("constant value {} (a DC offset) is not applied", factor_s));
                }
                continue;
            }
            let (channel, factor) = match channel.strip_prefix('-') {
                Some(c) if chans.extended => (c, -factor), // extension: "-R"
                _ => (channel, factor),
            };
            match chans.lookup(channel) {
                Some(c) => sources.push((c, factor)),
                None => notes.push(format!("source channel '{}' does not exist at this point", channel)),
            }
        }
        // Equalizer APO creates a virtual channel for an unknown target.
        let Some(dest) = chans.resolve(target, true) else {
            notes.push(format!("too many channels for '{}'", target));
            continue;
        };
        routes.push((dest, sources));
    }
    if routes.is_empty() {
        return Err(ignored("no valid assignment"));
    }
    Ok((routes, notes))
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
        assert_eq!(cfg.stages[0].1, Stage::Gain { gain_db: -6.5, channels: ChannelSel::List(vec![0, 1]) });
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
        let cfg = parse("Channel: L\nPreamp: -3 dB\nChannel: all\nCopy: L=0.5*L+0.5*R R=-6dB*L+R\n", None);
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
        let cfg = parse("DivioraExtensions: on\nCopy: L=L-R R=0.5*R-0.5*L C=-6dB*L+-6dB*R\nFilter: ON PK Fc 30000 Hz Gain 48 dB Q 150\n", None);
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
        let cfg = parse("DivioraExtensions: on\nFilter1: ON PK Fc 1000Hz Gain 3dB Q 1.41\nFilter 2: ON PK Fc 1.2 kHz Gain -2 dB Q 2\nFilter3: ON LP1 Fc 18kHz\nFilter: ON NOTCH Fc 50 Hz Q 30\n", None);
        assert!(cfg.warnings.is_empty(), "{:?}", cfg.warnings);
        assert_eq!(cfg.stages.len(), 4);
        match &cfg.stages[1].1 { Stage::Biquad { spec, .. } => assert_eq!(spec.freq, 1200.0), s => panic!("{:?}", s) }
        match &cfg.stages[2].1 { Stage::Biquad { spec, .. } => assert_eq!((spec.kind, spec.freq), (FilterKind::LowPass1, 18000.0)), s => panic!("{:?}", s) }
    }

    #[test]
    fn equalizer_apo_defaults_from_its_source() {
        let spec = |line: &str| match &parse(line, None).stages[0].1 { Stage::Biquad { spec, .. } => *spec, s => panic!("{:?}", s) };
        // BiQuadFilterFactory.cpp: notch Q 30, shelf S 0.9, LP/HP Q 0.7071, slope = dB / 12.
        assert_eq!(spec("Filter: ON NO Fc 1000 Hz").shape, Shape::Q(30.0));
        let ls = spec("Filter: ON LS Fc 100 Hz Gain 6 dB");
        assert_eq!((ls.shape, ls.corner), (Shape::SlopeDbPerOct(0.9 * 12.0), true));
        let lsc = spec("Filter: ON LSC 12 dB Fc 100 Hz Gain 6 dB");
        assert_eq!((lsc.shape, lsc.corner), (Shape::SlopeDbPerOct(12.0), false));
        let ls6 = spec("Filter: ON LS 6dB Fc 100 Hz Gain 6 dB");
        assert_eq!((ls6.kind, ls6.shape, ls6.corner), (FilterKind::LowShelf, Shape::SlopeDbPerOct(6.0), true));
        assert_eq!(spec("Filter: ON LP Fc 100 Hz").shape, Shape::Q(std::f64::consts::FRAC_1_SQRT_2));
        assert_eq!(spec("Filter: ON PK Fc 1000 Hz Gain 2 dB BW Oct 1").shape, Shape::BwOct(1.0));
        // "1.000 Hz" = 1000 Hz (Room EQ Wizard thousands separator); comma decimals.
        assert_eq!(spec("Filter: ON PK Fc 1.000 Hz Gain 1 dB Q 1").freq, 1000.0);
        assert_eq!(spec("Filter: ON PK Fc 31,5 Hz Gain 1 dB Q 1").freq, 31.5);
        // Gain is ignored for LP/HP/NO/AP; OFF lines and NONE add nothing.
        assert_eq!(spec("Filter: ON HP Fc 80 Hz Gain 6 dB").gain_db, 0.0);
        assert!(parse("Filter: OFF PK Fc 100 Hz Gain 3 dB Q 1\nFilter: ON None", None).stages.is_empty());
        // Channel names depend on the layout: on stereo "C" only exists once Copy creates it.
        let cfg = parse("Channel: C\nPreamp: -3 dB\nCopy: C=0.5*L+0.5*R\nChannel: C\nPreamp: -6 dB", None);
        assert_eq!(cfg.stages[0].1, Stage::Gain { gain_db: -3.0, channels: ChannelSel::List(vec![]) });
        assert_eq!(cfg.stages[2].1, Stage::Gain { gain_db: -6.0, channels: ChannelSel::List(vec![2]) });
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

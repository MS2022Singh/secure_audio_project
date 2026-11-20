//! Convert an Equalizer APO config to CamillaDSP YAML from the command line.
//!
//!   cargo run --example eqapo2camilla -- "C:\Program Files\EqualizerAPO\config\config.txt" [capture device] [playback device]
//!
//! `Include:` files are resolved relative to the input file. One YAML is written per
//! profile next to the input (`<name>.camilla.<profile>.yml`); unconverted lines are listed.
use diviora_dsp::{camilla, eqapo};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(input) = args.get(1) else {
        eprintln!("usage: eqapo2camilla <config.txt> [capture device] [playback device]");
        std::process::exit(2);
    };
    let path = PathBuf::from(input);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| { eprintln!("cannot read {}: {}", input, e); std::process::exit(1) });
    let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let resolver = |name: &str| std::fs::read_to_string(dir.join(name)).ok();
    let cfg = eqapo::parse(&text, Some(&resolver));
    let opt = camilla::CamillaOptions { capture_device: args.get(2).cloned(), playback_device: args.get(3).cloned(), ..Default::default() };
    let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    for (name, yaml) in camilla::emit_profiles(&cfg, &opt) {
        let safe: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
        let out = path.with_file_name(format!("{}.camilla.{}.yml", stem, safe));
        std::fs::write(&out, yaml).expect("write failed");
        println!("wrote {}", out.display());
    }
    println!("{} stage(s) converted, {} line(s) not converted", cfg.stages.len(), cfg.warnings.len());
    for w in &cfg.warnings { println!("  {}", w); }
}

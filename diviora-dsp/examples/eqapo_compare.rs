//! Compares Equalizer APO's real output with Diviora's for one configuration.
//! eqapo_compare <config.txt> <input.wav> <eqapo_output.wav>
//! (Equalizer APO's output comes from its Benchmark.exe: -i input.wav -o eqapo_output.wav)
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&a[1]).unwrap();
    let (sr, inp) = diviora_dsp::wav::decode(&std::fs::read(&a[2]).unwrap()).unwrap();
    let (_, eq) = diviora_dsp::wav::decode(&std::fs::read(&a[3]).unwrap()).unwrap();
    let ch = inp.len();
    let frames = inp[0].len().min(eq[0].len());
    let il = |v: &[Vec<f32>]| -> Vec<f32> { (0..frames).flat_map(|i| v.iter().take(ch).map(move |c| c[i])).collect() };
    let (input, eqo) = (il(&inp), il(&eq));
    let ctx = diviora_dsp::Context { sample_rate: sr, channels: ch, device_name: "Benchmark".into(), ..Default::default() };
    let cfg = diviora_dsp::parse_with(&text, None, &ctx);
    let mut e = diviora_dsp::Engine::compile(&cfg.stages_for_device("Benchmark"), sr, ch, 0);
    let mut ours = input.clone();
    e.process_interleaved(&mut ours);
    let c = diviora_dsp::analysis::compare(&input, &eqo, &ours, sr, ch);
    let verdict = if c.max_band_diff_db < 0.1 { "SAME" } else if c.max_band_diff_db < 0.5 { "CLOSE" } else { "DIFFERENT" };
    println!("{:<34} {:<9} max band difference {:.2} dB, offset {} samples",
        std::path::Path::new(&a[1]).file_name().unwrap().to_string_lossy(), verdict, c.max_band_diff_db, c.lag_frames);
    for (i, b) in c.bands.iter().enumerate() {
        println!("    ch{} EqAPO  : {}", i, b.iter().map(|x| format!("{:6.1}", x.1)).collect::<Vec<_>>().join(""));
        println!("    ch{} Diviora: {}", i, b.iter().map(|x| format!("{:6.1}", x.2)).collect::<Vec<_>>().join(""));
    }
    for w in &cfg.warnings { println!("    diviora: line {}: {}", w.line, w.reason); }
}

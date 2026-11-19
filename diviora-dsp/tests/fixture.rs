use diviora_dsp::{camilla, eqapo, Engine};

#[test]
fn sample_config_converts_end_to_end() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
    let text = std::fs::read_to_string(format!("{}sample_config.txt", dir)).unwrap();
    let resolver = |n: &str| std::fs::read_to_string(format!("{}{}", dir, n)).ok();
    let cfg = eqapo::parse(&text, Some(&resolver));

    // preamp, 4 active filters, included shelf, delay, copy, device preamp
    assert_eq!(cfg.stages.len(), 9, "{:#?}", cfg.stages);
    assert_eq!(cfg.warnings.len(), 1, "only GraphicEQ should be unconverted: {:?}", cfg.warnings);

    let profiles = camilla::emit_profiles(&cfg, &camilla::CamillaOptions::default());
    assert_eq!(profiles.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(), vec!["default", "Headphones"]);
    assert!(profiles[0].1.contains("type: Lowshelf") && profiles[0].1.contains("slope: 12.0"));
    assert!(profiles[0].1.contains("# Not converted:"));

    // The engine runs the same stages without NaNs or blow-ups.
    let mut e = Engine::compile(&cfg.stages_for_device("Speakers (Realtek(R) Audio)"), 48000, 2, 1);
    let mut buf: Vec<f32> = (0..9600).map(|i| ((i as f32) * 0.37).sin() * 0.5).collect();
    e.process_interleaved(&mut buf);
    assert!(buf.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
}

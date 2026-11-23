use diviora_dsp::{camilla, eqapo, Engine};

#[test]
fn sample_config_converts_end_to_end() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
    let text = std::fs::read_to_string(format!("{}sample_config.txt", dir)).unwrap();
    let resolver = |n: &str| std::fs::read_to_string(format!("{}{}", dir, n)).ok();
    let cfg = eqapo::parse(&text, Some(&resolver));

    // preamp, 4 active filters, included shelf, delay, copy, device preamp, GraphicEQ
    assert_eq!(cfg.stages.len(), 10, "{:#?}", cfg.stages);
    assert!(cfg.warnings.is_empty(), "everything should convert: {:?}", cfg.warnings);

    let profiles = camilla::emit_profiles(&cfg, &camilla::CamillaOptions::default());
    assert_eq!(profiles.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(), vec!["default", "Headphones"]);
    assert!(profiles[0].1.contains("type: Lowshelf"));
    assert!(profiles[0].1.contains("GraphicEQ / Convolution runs in the Diviora engine only"));

    // The engine runs the same stages without NaNs or blow-ups.
    let mut e = Engine::compile(&cfg.stages_for_device("Speakers (Realtek(R) Audio)"), 48000, 2, 1);
    let mut buf: Vec<f32> = (0..9600).map(|i| ((i as f32) * 0.37).sin() * 0.5).collect();
    e.process_interleaved(&mut buf);
    assert!(buf.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
}

#[test]
fn virtual_channel_multichannel_config_converts_and_runs() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
    let text = std::fs::read_to_string(format!("{}virtual_channels_config.txt", dir)).unwrap();
    for ch in [2usize, 6, 8] {
        let ctx = eqapo::Context { sample_rate: 48000, channels: ch, ..Default::default() };
        let cfg = eqapo::parse_with(&text, None, &ctx);
        assert!(cfg.warnings.is_empty(), "{} ch: {:?}", ch, cfg.warnings);
        let mut e = Engine::compile(&cfg.stages_for_all_devices(), 48000, ch, 1);
        assert_eq!(e.channels(), ch);
        let mut buf: Vec<f32> = (0..48000 * ch).map(|i| (((i / ch) as f32) * 0.05).sin() * 0.5).collect();
        e.process_interleaved(&mut buf);
        assert!(buf.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
        let energy: f32 = buf.iter().map(|s| s * s).sum();
        assert!(energy > 1.0, "{} ch: output should not be silent", ch);
    }
}

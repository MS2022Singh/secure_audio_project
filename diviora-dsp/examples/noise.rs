//! Writes the deterministic comparison test sound: noise <file.wav> [channels]
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let ch: usize = a.get(2).map(|s| s.parse().unwrap()).unwrap_or(2);
    let x = diviora_dsp::analysis::test_noise(48000, ch, 3.0);
    std::fs::write(&a[1], diviora_dsp::wav::encode_f32(48000, ch as u16, &x)).unwrap();
}

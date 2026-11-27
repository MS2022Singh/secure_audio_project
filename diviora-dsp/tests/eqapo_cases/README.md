Made-up one-feature configurations (no real values) for checking Diviora against
Equalizer APO 1.4.2's own engine. For each file: put it in Equalizer APO's config.txt,
run `Benchmark.exe -i in.wav -o out.wav -v --nopause` (in.wav: `cargo run --example noise in.wav`),
then `cargo run --release --example eqapo_compare <case.txt> in.wav out.wav`.

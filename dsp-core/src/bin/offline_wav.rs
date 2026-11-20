use diviora_dsp_core::{compiler::*, DspConfig};
use std::env;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        println!("Usage: offline_wav <config.json> <output_log.txt>");
        return Ok(());
    }

    let config_path = &args[1];
    let log_path = &args[2];

    let config_raw = fs::read_to_string(config_path)?;
    let config: DspConfig = serde_json::from_str(&config_raw)?;

    let target_sample_rate = 48000.0;
    config.validate(target_sample_rate)?;

    let compiled = CompiledDspState::compile(&config, target_sample_rate)?;
    let mut processor = AudioProcessor::new(compiled, config.channels)?;

    // Generate 1-second stereo test signal (1 kHz sine)
    let num_samples = 48000 * config.channels;
    let mut pcm_buffer = vec![0.0f32; num_samples];
    for i in 0..48000 {
        let t = i as f32 / 48000.0;
        let sample = (2.0 * std::f32::consts::PI * 1000.0 * t).sin();
        pcm_buffer[i * 2] = sample;
        pcm_buffer[i * 2 + 1] = sample;
    }

    processor.process_interleaved(&mut pcm_buffer, config.channels)?;

    let log_content = format!(
        "OFFLINE DSP VERIFICATION REPORT\nRevision: {}\nChannels: {}\nProcessed Samples: {}\nStatus: SUCCESS\n",
        config.revision, config.channels, pcm_buffer.len()
    );

    fs::write(log_path, log_content)?;
    println!("Offline DSP test successfully processed and logged to {}", log_path);

    Ok(())
}